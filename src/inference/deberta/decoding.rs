//! Span-decoding utilities shared by the fp32 and INT8 forward paths.
//! These are pure logic — they don't reference any weight type — so
//! they live outside the feature gate. Both paths import the same
//! `PredictedEntity`, `decode`, and `generate_span_indices`.

#[inline]
pub(crate) fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// One decoded entity. Indices are *word* indices (inclusive); the
/// caller maps them to character positions via the start/end-char
/// arrays produced during tokenization.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictedEntity {
    pub word_start: usize,
    pub word_end: usize,
    pub label_idx: usize,
    pub score: f32,
}

/// All valid `(start_word, end_word)` pairs for a sequence of
/// `num_words` words. Word indices are inclusive. Slots wider than
/// `max_width` or extending past the last word are emitted as
/// `(0, 0)` placeholders — matches GLiNER's `span_idx * span_mask`
/// behaviour — and accompanied by a `false` in the parallel `valid`
/// vector so callers can skip them during decode.
pub fn generate_span_indices(num_words: usize, max_width: usize) -> (Vec<(usize, usize)>, Vec<bool>) {
    let mut spans = Vec::with_capacity(num_words * max_width);
    let mut valid = Vec::with_capacity(num_words * max_width);
    for s in 0..num_words {
        for w in 0..max_width {
            let e = s + w;
            if e < num_words {
                spans.push((s, e));
                valid.push(true);
            } else {
                spans.push((0, 0));
                valid.push(false);
            }
        }
    }
    (spans, valid)
}

/// Greedy flat-NER decoding. For each span, take the argmax over
/// labels; keep it if `sigmoid(score) > threshold`. Then sort by score
/// descending and accept spans that don't overlap any already-accepted
/// span. Mirrors `gliner.decoding.decoder` flat-mode.
pub fn decode(
    scores: &[f32],
    spans: &[(usize, usize)],
    span_valid: &[bool],
    num_prompts: usize,
    threshold: f32,
) -> Vec<PredictedEntity> {
    let mut candidates: Vec<PredictedEntity> = Vec::new();
    for (s, &(ws, we)) in spans.iter().enumerate() {
        if !span_valid[s] {
            continue;
        }
        let row = &scores[s * num_prompts..(s + 1) * num_prompts];
        let (best_c, &best_logit) = row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .expect("non-empty row");
        let prob = sigmoid(best_logit);
        if prob > threshold {
            candidates.push(PredictedEntity {
                word_start: ws,
                word_end: we,
                label_idx: best_c,
                score: prob,
            });
        }
    }

    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    let mut accepted: Vec<PredictedEntity> = Vec::new();
    for c in candidates {
        let overlaps = accepted
            .iter()
            .any(|a| !(a.word_end < c.word_start || c.word_end < a.word_start));
        if !overlaps {
            accepted.push(c);
        }
    }

    accepted.sort_by_key(|e| (e.word_start, e.word_end));
    accepted
}
