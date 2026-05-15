//! Sentence embedding via the bundled all-MiniLM-L6-v2 model.
//!
//! Inference runs through our own pure-Rust BERT engine in
//! `crate::inference` — no `tract-onnx`, no `ort`, no C deps. The
//! tokenizer is HuggingFace's `tokenizers` crate (also pure Rust).
//! All model weights are baked into the binary at compile time.

use std::sync::LazyLock;

use tokenizers::Tokenizer;

use crate::inference::{WeightsInt8, bert_int8};

/// Output dimensionality of the bundled all-MiniLM-L6-v2 model.
/// Single source of truth — callers allocate buffers and arrays against
/// this constant.
pub const EMBEDDING_DIM: usize = 384;

/// True token count of `text` under the bundled MiniLM tokenizer.
/// Used by `lib::run` to gate inputs against `MAX_INPUT_TOKENS`.
/// Uses an *untruncated* tokenizer so the count is accurate for
/// inputs above the model's 512-token max-position limit.
pub fn token_count(text: &str) -> usize {
    UNTRUNCATED_TOKENIZER
        .encode(text, true)
        .expect("tokenization failed")
        .get_ids()
        .len()
}

/// Compute a semantic embedding via the bundled BERT forward pass.
/// Returns a 384-dim L2-normalized vector — cosine similarity reduces
/// to a dot product.
pub fn embed_text(text: &str) -> Vec<f32> {
    if text.trim().is_empty() {
        return vec![0.0f32; EMBEDDING_DIM];
    }
    let encoding = TOKENIZER.encode(text, true).expect("tokenization failed");
    let ids: Vec<u32> = encoding.get_ids().to_vec();
    let mask: Vec<u32> = encoding.get_attention_mask().to_vec();
    let weights: &WeightsInt8 = WeightsInt8::load_bundled();
    bert_int8::forward(weights, &ids, &mask)
}

/// Run the forward pass and return the **per-token contextualized
/// embeddings** together with each token's character offsets into the
/// original `text`. Used to build span-level contextualized vectors
/// (mean-pool over the tokens whose offsets fall inside a span).
///
/// Returns `(sequence_output, offsets)` where:
/// - `sequence_output` is `seq_len * EMBEDDING_DIM` row-major,
/// - `offsets[t]` is `(char_start, char_end)` for token `t` (special
///   tokens like `[CLS]`/`[SEP]` get `(0, 0)`).
///
/// No L2-normalization is applied. The caller pools over span tokens
/// and normalizes if it wants cosine-as-dot.
pub fn embed_sequence_with_offsets(text: &str) -> (Vec<f32>, Vec<(usize, usize)>) {
    if text.trim().is_empty() {
        return (Vec::new(), Vec::new());
    }
    let encoding = TOKENIZER.encode(text, true).expect("tokenization failed");
    let ids: Vec<u32> = encoding.get_ids().to_vec();
    let mask: Vec<u32> = encoding.get_attention_mask().to_vec();
    let offsets: Vec<(usize, usize)> = encoding.get_offsets().to_vec();
    let weights: &WeightsInt8 = WeightsInt8::load_bundled();
    let sequence = bert_int8::forward_sequence(weights, &ids, &mask);
    (sequence, offsets)
}

/// Update an existing element embedding *in place* by folding in a
/// fresh observation (typically a re-mention's contextualized span
/// vector). Implements the streaming centroid update:
///
/// ```text
/// current ← (n_prev * current + observation) / (n_prev + 1)
/// current ← current / ||current||
/// ```
///
/// As `n_prev` grows, each new observation moves the centroid less
/// — appropriate for accumulating stable evidence about an entity's
/// typical usage context. With `n_prev = 0` the fold reduces to "use
/// the observation as-is" (the first mention bootstraps the
/// embedding).
///
/// Both inputs are expected to be L2-normalized; the output is also
/// L2-normalized so cosine-as-dot continues to hold. The math is
/// approximate (direction-correct, magnitude-renormalized) — for
/// exact-mean tracking we'd carry the un-normalized sum, but
/// direction is what cosine routing reads, so this is sufficient.
///
/// Phase 4 of `contextualized_embeddings_plan.md`. The call-site
/// for re-mentions lands when Step 8 (`build_relations`) gets
/// wired; until then this is a tested-but-uncalled primitive.
///
/// # Panics
/// - if `current.len() != observation.len()`.
pub fn fold_streaming_centroid(current: &mut [f32], observation: &[f32], n_prev: u32) {
    assert_eq!(
        current.len(),
        observation.len(),
        "centroid dimensions must match"
    );
    let n = n_prev as f32;
    let denom = n + 1.0;
    for i in 0..current.len() {
        current[i] = (n * current[i] + observation[i]) / denom;
    }
    let norm: f32 = current.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    for x in current.iter_mut() {
        *x /= norm;
    }
}

/// Embed `text` once, then mean-pool the contextualized token vectors
/// whose character offsets overlap `[char_start, char_end)`. Returns
/// an L2-normalized `Vec<f32>` for cosine-as-dot, or `None` if the
/// span doesn't intersect any non-special token (e.g., span sits
/// inside a multi-byte character, or input is empty).
///
/// This is the substrate for description-rich element embeddings
/// (see `contextualized_embeddings_plan.md`). Routing accuracy on
/// the v3 18-case fixture: **83% top-1**, vs 44% for
/// `embed_text(name)` and 78% for `mean(embed_text(member))`.
pub fn embed_span_in_context(
    text: &str,
    char_start: usize,
    char_end: usize,
) -> Option<Vec<f32>> {
    if char_end <= char_start {
        return None;
    }
    let (sequence, offsets) = embed_sequence_with_offsets(text);
    if sequence.is_empty() {
        return None;
    }
    let mut acc = vec![0.0f32; EMBEDDING_DIM];
    let mut n = 0usize;
    for (t, &(start, end)) in offsets.iter().enumerate() {
        // Skip special tokens ([CLS] / [SEP] / pad — all reported as (0,0)).
        if start == 0 && end == 0 {
            continue;
        }
        // Token overlaps the span iff their char ranges intersect.
        let overlaps = end > char_start && start < char_end;
        if !overlaps {
            continue;
        }
        let base = t * EMBEDDING_DIM;
        for i in 0..EMBEDDING_DIM {
            acc[i] += sequence[base + i];
        }
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let inv = 1.0 / n as f32;
    for x in &mut acc {
        *x *= inv;
    }
    let norm: f32 = acc.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    for x in &mut acc {
        *x /= norm;
    }
    Some(acc)
}

// Same tokenizer.json as the embedder, but with truncation and
// padding both cleared so `token_count` reports the true length.
// The bundled tokenizer.json ships with `padding: Fixed(128)` baked
// in — without `with_padding(None)` every short input would report
// as 128 tokens.
static UNTRUNCATED_TOKENIZER: LazyLock<Tokenizer> = LazyLock::new(|| {
    let bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");
    let mut t = Tokenizer::from_bytes(bytes).expect("Failed to load embedded tokenizer");
    t.with_truncation(None).expect("clear truncation");
    t.with_padding(None);
    t
});

// Embedding-path tokenizer: cap at the model's 512-token max-position
// so a long input doesn't index past the position-embedding table.
// Padding is explicitly disabled — we process one sequence per call
// (no batching) so padding adds purely wasted matmul work. The
// bundled tokenizer.json ships with `padding: Fixed(128)` which would
// silently process a 12-token input as 128 tokens, wasting ~10× of
// the forward-pass cost. `with_padding(None)` strips that.
static TOKENIZER: LazyLock<Tokenizer> = LazyLock::new(|| {
    let bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");
    let mut t = Tokenizer::from_bytes(bytes).expect("Failed to load embedded tokenizer");
    let _ = t.with_truncation(Some(tokenizers::TruncationParams {
        max_length: 512,
        ..Default::default()
    }));
    t.with_padding(None);
    t
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embed_empty_returns_zeros() {
        let v = embed_text("");
        assert_eq!(v.len(), EMBEDDING_DIM);
        assert!(v.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn embed_yields_unit_vector() {
        let v = embed_text("hello world");
        assert_eq!(v.len(), EMBEDDING_DIM);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "expected unit vector, got norm = {norm}"
        );
    }

    #[test]
    fn embed_is_deterministic() {
        let v1 = embed_text("the meeting is at 3pm");
        let v2 = embed_text("the meeting is at 3pm");
        assert_eq!(v1, v2);
    }

    #[test]
    fn token_count_matches_expected() {
        // "hello world" → [CLS] hello world [SEP] = 4 tokens.
        assert_eq!(token_count("hello world"), 4);
    }

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>()
    }

    #[test]
    fn embed_span_in_context_yields_unit_vector() {
        let text = "Sarah moved to Berlin last year.";
        let span_start = text.find("Berlin").unwrap();
        let v = embed_span_in_context(text, span_start, span_start + "Berlin".len())
            .expect("Berlin span should resolve");
        assert_eq!(v.len(), EMBEDDING_DIM);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4, "expected unit, got {norm}");
    }

    #[test]
    fn embed_span_differs_from_bare_word_embedding() {
        // The whole point: contextualized 'Berlin' in a movement
        // sentence should not be identical to bare embed_text('Berlin').
        let text = "She moved to Berlin last year.";
        let span_start = text.find("Berlin").unwrap();
        let ctx = embed_span_in_context(text, span_start, span_start + "Berlin".len())
            .expect("span should resolve");
        let bare = embed_text("Berlin");
        // Same direction-ish (both about Berlin), but materially different.
        let c = cosine(&ctx, &bare);
        assert!(c > 0.4 && c < 0.99,
            "expected cosine in (0.4, 0.99); got {c}");
    }

    #[test]
    fn embed_span_empty_or_oob_returns_none() {
        assert!(embed_span_in_context("", 0, 5).is_none());
        assert!(embed_span_in_context("hello", 5, 5).is_none()); // zero-width
        assert!(embed_span_in_context("hello", 100, 200).is_none()); // out of bounds
    }

    #[test]
    fn embed_span_deterministic() {
        let text = "She moved to Berlin last year.";
        let s = text.find("Berlin").unwrap();
        let a = embed_span_in_context(text, s, s + "Berlin".len()).unwrap();
        let b = embed_span_in_context(text, s, s + "Berlin".len()).unwrap();
        assert_eq!(a, b);
    }

    fn unit(mut v: Vec<f32>) -> Vec<f32> {
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        for x in &mut v {
            *x /= norm;
        }
        v
    }

    #[test]
    fn fold_first_observation_is_identity() {
        // n_prev = 0 means current is meaningless / uninitialized;
        // the fold should yield the observation as-is (normalized).
        let mut current = vec![0.0; EMBEDDING_DIM];
        let obs = unit(vec![1.0; EMBEDDING_DIM]);
        fold_streaming_centroid(&mut current, &obs, 0);
        for i in 0..EMBEDDING_DIM {
            assert!(
                (current[i] - obs[i]).abs() < 1e-5,
                "i={i}: got {} expected {}",
                current[i],
                obs[i],
            );
        }
    }

    #[test]
    fn fold_same_vector_is_idempotent() {
        // Folding a vector into itself at any n leaves the vector
        // unchanged (direction).
        let v = unit({
            let mut x = vec![0.0; EMBEDDING_DIM];
            x[0] = 1.0;
            x[1] = 1.0;
            x[2] = 1.0;
            x
        });
        let mut current = v.clone();
        for n in [0u32, 1, 5, 100] {
            fold_streaming_centroid(&mut current, &v, n);
            for i in 0..EMBEDDING_DIM {
                assert!(
                    (current[i] - v[i]).abs() < 1e-5,
                    "n={n} i={i}: drifted from input",
                );
            }
        }
    }

    #[test]
    fn fold_yields_unit_vector() {
        let mut current = unit({
            let mut x = vec![0.1; EMBEDDING_DIM];
            x[0] = 0.9;
            x
        });
        let obs = unit({
            let mut x = vec![0.05; EMBEDDING_DIM];
            x[5] = 0.7;
            x
        });
        fold_streaming_centroid(&mut current, &obs, 3);
        let norm: f32 = current.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "expected unit norm after fold, got {norm}",
        );
    }

    #[test]
    fn fold_pulls_toward_observation_at_low_n() {
        // Two orthogonal unit vectors. Fold A into B with n=0;
        // result should be A. Fold A into B with n=1; result should
        // be midpoint of A and B (50/50).
        let mut a = vec![0.0; EMBEDDING_DIM];
        a[0] = 1.0;
        let mut b = vec![0.0; EMBEDDING_DIM];
        b[1] = 1.0;

        let mut current = a.clone();
        fold_streaming_centroid(&mut current, &b, 1);
        // 0.5*A + 0.5*B unnormalized = [0.5, 0.5, 0, ...]. Norm =
        // sqrt(0.5) ≈ 0.707. Normalized = [1/sqrt(2), 1/sqrt(2), 0, ...].
        let expected = 1.0 / 2.0f32.sqrt();
        assert!((current[0] - expected).abs() < 1e-4);
        assert!((current[1] - expected).abs() < 1e-4);
        for i in 2..EMBEDDING_DIM {
            assert!(current[i].abs() < 1e-5);
        }
    }

    #[test]
    fn fold_stabilizes_at_high_n() {
        // Same orthogonal A/B but fold with n=1000. The centroid
        // should barely move toward B.
        let a = {
            let mut x = vec![0.0; EMBEDDING_DIM];
            x[0] = 1.0;
            x
        };
        let b = {
            let mut x = vec![0.0; EMBEDDING_DIM];
            x[1] = 1.0;
            x
        };
        let mut current = a.clone();
        fold_streaming_centroid(&mut current, &b, 1000);
        assert!(current[0] > 0.999, "should remain near A, got {}", current[0]);
        assert!(current[1] < 0.005, "should barely move toward B, got {}", current[1]);
    }

    #[test]
    fn embed_span_separates_different_kinds() {
        // 'London' as a place should cluster with another place
        // contextualization more than with a random temporal context.
        let london = {
            let t = "She moved to London last year.";
            let s = t.find("London").unwrap();
            embed_span_in_context(t, s, s + "London".len()).unwrap()
        };
        let paris = {
            let t = "He flew to Paris on Tuesday.";
            let s = t.find("Paris").unwrap();
            embed_span_in_context(t, s, s + "Paris".len()).unwrap()
        };
        let tuesday = {
            let t = "The meeting is on Tuesday.";
            let s = t.find("Tuesday").unwrap();
            embed_span_in_context(t, s, s + "Tuesday".len()).unwrap()
        };
        let london_paris = cosine(&london, &paris);
        let london_tuesday = cosine(&london, &tuesday);
        assert!(
            london_paris > london_tuesday,
            "expected London-Paris (place/place) cosine to exceed London-Tuesday (place/time); got {london_paris:.3} vs {london_tuesday:.3}",
        );
    }
}
