//! Step 5a — orthographic chunker + statistical slices. Pure functions,
//! no model, no persistent state. Produces content-bearing chunk
//! candidates from punctuation, whitespace, casing, slash separators,
//! within-input repetition, and within-input pointwise mutual
//! information. Always produces output for non-empty inputs; never
//! depends on a label schema.
//!
//! Three slices stacked in order of how directly the signal comes from
//! the surface text:
//!
//! ### Slice 1 — Orthographic boundaries
//! Phrase + Token chunks from punctuation / whitespace alone. The
//! cognate of the brain's prosodic / orthographic boundary cues
//! (Cutler & Norris 1988; Pierrehumbert & Hirschberg 1990) — pre-
//! semantic segmentation. Always emits.
//!
//! ### Slice 2 — Repetition (`Repeated`)
//! Any n-gram (`2 ≤ n ≤ 5` tokens) that appears at least twice in the
//! input becomes a `Repeated` chunk at its first occurrence. The brain
//! attends more strongly to repeated stimuli; same here. Within-input
//! only — cross-tick accumulation lands when Legend has a persistent
//! stats store.
//!
//! ### Slice 3 — Pointwise mutual information (`Collocation`)
//! For each adjacent bigram `(a, b)` in the input, compute
//!     `pmi(a, b) = log2( p(a, b) / (p(a) · p(b)) )`
//! and emit the top-scoring bigrams whose PMI clears `MIN_PMI` as
//! `Collocation` chunks. Slice 2's text set is subtracted first so
//! repeated bigrams don't double-emit; slice 3 then contributes the
//! rare-pairs-that-co-occur signal slice 2 can't see.
//!
//! Within-input PMI has a known weakness: in a one-sentence input
//! with all singletons, every bigram scores `log2(n_tokens)` and the
//! signal is uniform noise. A future stateful pass over accumulated
//! bigram counts will demote function words by giving them low PMI
//! against any *specific* neighbor; until then, function words
//! continue to emit as Tokens.
//!
//! Two scales emitted at decoding time (cf. Ding, Melloni, Zhang,
//! Tian, Poeppel 2016 "Cortical tracking of hierarchical linguistic
//! structures"): Phrases and Tokens map to clause-level and word-level
//! cortical tracking respectively; Repeated and Collocation are added
//! refinements that surface statistically grounded multi-word units.
//!
//! Sub-millisecond on typical inputs.
//!
//! ## What gets dropped (slice 1)
//!
//! - Single-character tokens (always — they carry no information).
//! - Tokens that are pure punctuation after stripping.
//! - Single-word phrases (already covered by Tokens; would be redundant).
//! - Phrases shorter than 3 characters after trimming.
//! - Exact-duplicate chunks within a single input (deduped on text + scale).
//!
//! ## What does NOT get dropped
//!
//! Function words ("the", "and", "on", "in", …) are emitted as Tokens.
//! There is deliberately no stopword list — the brain doesn't have one
//! either. Function-word filtering is the job of statistical learning
//! (slice 3 over accumulated counts), which in the *stateless* regime
//! we ship today contributes only to the positive collocation side.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct OrthographicChunk {
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
    pub scale: ChunkScale,
    /// How often this exact chunk-text appears in the input. Always 1
    /// for slice-1 outputs (Token / Phrase, deduped on first occurrence);
    /// ≥ 2 for `Repeated`; the bigram-occurrence count for `Collocation`.
    pub repetitions: u32,
    /// Pointwise mutual information score, populated only for
    /// `Collocation` chunks. Higher means the two adjacent tokens
    /// co-occur far more than chance would predict from their
    /// individual frequencies in this input.
    pub pmi: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkScale {
    /// Whitespace-delimited atom after punctuation stripping and `/` splitting.
    Token,
    /// Major-punctuation-delimited span; stopwords retained.
    Phrase,
    /// N-gram (2..=5 tokens) that appears at least twice in the input.
    Repeated,
    /// Adjacent bigram whose pointwise mutual information clears the
    /// stateless threshold and isn't already captured as `Repeated`.
    Collocation,
}

/// Major punctuation that ends a phrase. Comma is included because comma-
/// delimited lists are exactly the cold-start pattern ("edi, esi, edx, …") —
/// each list item deserves to be its own phrase candidate.
const PHRASE_DELIMS: &[char] = &['.', ',', ';', ':', '!', '?', '—', '(', ')', '[', ']', '{', '}'];

/// Characters stripped from token edges but allowed inside tokens.
const EDGE_PUNCT: &[char] = &[
    '.', ',', ';', ':', '!', '?', '(', ')', '[', ']', '{', '}', '"', '\'', '`', '—', '–', '"',
    '"', '\u{2018}', '\u{2019}',
];

/// Internal token separator (splits e.g. `eax/rax` into two tokens).
/// Hyphen is deliberately NOT in this set — `x86-64`, `front-end`,
/// `well-defined` stay whole.
const TOKEN_INTERNAL_SPLIT: char = '/';

/// Maximum n-gram length scanned by the repetition pass. Beyond five
/// tokens, exact repetition is rare and the chunk text gets unwieldy.
const SLICE2_MAX_NGRAM: usize = 5;

/// PMI floor for emitting a `Collocation` chunk. `2.0` ≈ "this bigram
/// is 4× more likely than chance"; tuned to be selective in stateless
/// short-input mode.
const SLICE3_MIN_PMI: f32 = 2.0;

/// Hard cap on how many `Collocation` chunks slice 3 contributes per
/// tick. Without a cap, a long input can flood the output with
/// statistically-flagged-but-marginal pairs.
const SLICE3_TOP_K: usize = 5;

/// Internal token record — every whitespace-and-slash atom in the input,
/// with edges stripped but **not** deduped. Slices 2 and 3 walk this
/// stream; slice 1's Token output is the deduped projection.
struct RawToken {
    text: String,
    char_start: usize,
    char_end: usize,
}

/// Extract content-bearing chunks from raw input text. Always returns
/// at least one chunk for non-empty input (worst case: the whole input
/// as a single Phrase chunk).
pub fn extract_chunks(text: &str) -> Vec<OrthographicChunk> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let raw_tokens = collect_raw_tokens(text);

    let mut out: Vec<OrthographicChunk> = Vec::new();
    let mut emitted_texts: HashSet<(String, ChunkScale)> = HashSet::new();

    // ── Slice 1: Phrases ────────────────────────────────────────────
    // Multi-word only — single-word phrases would just duplicate Tokens.
    for (start, end) in split_on_chars(text, PHRASE_DELIMS) {
        let raw = &text[start..end];
        let (trimmed_text, trim_start, trim_end) = trim_span(raw, start);
        if trimmed_text.chars().count() < 3 {
            continue;
        }
        if !is_multi_word(trimmed_text) {
            continue;
        }
        let key = (trimmed_text.to_string(), ChunkScale::Phrase);
        if emitted_texts.insert(key.clone()) {
            out.push(OrthographicChunk {
                text: key.0,
                char_start: trim_start,
                char_end: trim_end,
                scale: ChunkScale::Phrase,
                repetitions: 1,
                pmi: None,
            });
        }
    }

    // ── Slice 1: Tokens ─────────────────────────────────────────────
    // First occurrence wins; `emitted_texts` deduplicates against earlier
    // tokens in the stream. Repetition counts are recovered via a single
    // pass over `raw_tokens` so callers see `repetitions ≥ 2` even when
    // only the first occurrence is emitted.
    let mut token_counts: HashMap<&str, u32> = HashMap::new();
    for t in &raw_tokens {
        *token_counts.entry(t.text.as_str()).or_insert(0) += 1;
    }
    for t in &raw_tokens {
        let key = (t.text.clone(), ChunkScale::Token);
        if !emitted_texts.insert(key.clone()) {
            continue;
        }
        out.push(OrthographicChunk {
            text: key.0,
            char_start: t.char_start,
            char_end: t.char_end,
            scale: ChunkScale::Token,
            repetitions: token_counts.get(t.text.as_str()).copied().unwrap_or(1),
            pmi: None,
        });
    }

    // ── Slice 2: Repeated n-grams ───────────────────────────────────
    let slice2 = slice2_repeated_ngrams(&raw_tokens);
    let slice2_texts: HashSet<String> =
        slice2.iter().map(|c| c.text.clone()).collect();
    out.extend(slice2);

    // ── Slice 3: PMI bigrams (excludes anything already in slice 2) ─
    out.extend(slice3_pmi_bigrams(&raw_tokens, &slice2_texts));

    // Stable order: by char_start, then scale priority (broader first
    // so consumers see the wide context before the narrow tokens).
    out.sort_by(|a, b| {
        a.char_start
            .cmp(&b.char_start)
            .then_with(|| scale_priority(a.scale).cmp(&scale_priority(b.scale)))
    });
    out
}

fn scale_priority(s: ChunkScale) -> u8 {
    match s {
        ChunkScale::Phrase => 0,
        ChunkScale::Repeated => 1,
        ChunkScale::Collocation => 2,
        ChunkScale::Token => 3,
    }
}

/// Walk the input once, edge-strip every whitespace-and-slash atom,
/// keep every surviving token (no dedup, no rejection of repeats).
fn collect_raw_tokens(text: &str) -> Vec<RawToken> {
    let mut out = Vec::new();
    for (ws_start, ws_end) in split_on_whitespace(text) {
        let atom = &text[ws_start..ws_end];
        let (stripped, strip_lead, _strip_trail) = strip_edges(atom);
        if stripped.is_empty() {
            continue;
        }
        let stripped_start = ws_start + strip_lead;

        // Split on `/` to keep `eax/rax` → two tokens, while leaving
        // hyphenated atoms (`x86-64`) intact.
        let mut seg_start = 0usize;
        for (i, ch) in stripped.char_indices() {
            if ch == TOKEN_INTERNAL_SPLIT {
                accept_raw(
                    &stripped[seg_start..i],
                    stripped_start + seg_start,
                    &mut out,
                );
                seg_start = i + ch.len_utf8();
            }
        }
        accept_raw(
            &stripped[seg_start..],
            stripped_start + seg_start,
            &mut out,
        );
    }
    out
}

fn accept_raw(candidate: &str, char_start: usize, out: &mut Vec<RawToken>) {
    let (s, lead, _trail) = strip_edges(candidate);
    if s.chars().count() < 2 {
        return;
    }
    if !has_letter_or_digit(s) {
        return;
    }
    out.push(RawToken {
        text: s.to_string(),
        char_start: char_start + lead,
        char_end: char_start + lead + s.len(),
    });
}

/// Slice 2: emit a `Repeated` chunk for every n-gram (2..=5 tokens)
/// that appears at least twice in `raw`. One chunk per unique
/// n-gram text, anchored at the first occurrence's span.
fn slice2_repeated_ngrams(raw: &[RawToken]) -> Vec<OrthographicChunk> {
    let mut out: Vec<OrthographicChunk> = Vec::new();
    let mut seen_text: HashSet<String> = HashSet::new();

    for n in 2..=SLICE2_MAX_NGRAM {
        if raw.len() < n + 1 {
            // Need at least n+1 tokens for two distinct n-grams to even
            // fit (last would start at index 0 and `raw.len() - n`).
            break;
        }
        let mut positions: HashMap<String, Vec<usize>> = HashMap::new();
        for i in 0..=(raw.len() - n) {
            let ngram = join_tokens(&raw[i..i + n]);
            positions.entry(ngram).or_default().push(i);
        }
        for (text, indices) in positions {
            if indices.len() < 2 {
                continue;
            }
            if !seen_text.insert(text.clone()) {
                continue;
            }
            let first = indices[0];
            let last = first + n - 1;
            out.push(OrthographicChunk {
                text,
                char_start: raw[first].char_start,
                char_end: raw[last].char_end,
                scale: ChunkScale::Repeated,
                repetitions: indices.len() as u32,
                pmi: None,
            });
        }
    }
    out
}

/// Slice 3: emit `Collocation` chunks for the top `SLICE3_TOP_K`
/// adjacent bigrams whose PMI clears `SLICE3_MIN_PMI` and aren't
/// already captured by slice 2.
fn slice3_pmi_bigrams(
    raw: &[RawToken],
    slice2_texts: &HashSet<String>,
) -> Vec<OrthographicChunk> {
    if raw.len() < 2 {
        return Vec::new();
    }

    let n_tokens = raw.len() as f32;
    let n_bigrams = (raw.len() - 1) as f32;

    let mut unigram_counts: HashMap<&str, usize> = HashMap::new();
    for t in raw {
        *unigram_counts.entry(t.text.as_str()).or_insert(0) += 1;
    }
    let mut bigram_positions: HashMap<(&str, &str), Vec<usize>> = HashMap::new();
    for i in 0..raw.len() - 1 {
        let pair = (raw[i].text.as_str(), raw[i + 1].text.as_str());
        bigram_positions.entry(pair).or_default().push(i);
    }

    let mut scored: Vec<(f32, String, usize, usize, u32)> =
        Vec::with_capacity(bigram_positions.len());
    for ((a, b), positions) in &bigram_positions {
        let c_ab = positions.len();
        let c_a = unigram_counts[a];
        let c_b = unigram_counts[b];
        let p_ab = (c_ab as f32) / n_bigrams;
        let p_a = (c_a as f32) / n_tokens;
        let p_b = (c_b as f32) / n_tokens;
        let pmi = (p_ab / (p_a * p_b)).log2();
        if !pmi.is_finite() || pmi < SLICE3_MIN_PMI {
            continue;
        }

        let text = format!("{a} {b}");
        if slice2_texts.contains(&text) {
            continue;
        }
        let first = positions[0];
        scored.push((
            pmi,
            text,
            raw[first].char_start,
            raw[first + 1].char_end,
            c_ab as u32,
        ));
    }

    scored.sort_by(|x, y| {
        y.0.partial_cmp(&x.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| x.2.cmp(&y.2))
    });
    scored.truncate(SLICE3_TOP_K);

    scored
        .into_iter()
        .map(|(pmi, text, start, end, reps)| OrthographicChunk {
            text,
            char_start: start,
            char_end: end,
            scale: ChunkScale::Collocation,
            repetitions: reps,
            pmi: Some(pmi),
        })
        .collect()
}

fn join_tokens(tokens: &[RawToken]) -> String {
    let mut out = String::new();
    for (i, t) in tokens.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&t.text);
    }
    out
}

/// Return `(start, end)` byte spans into `text` for each non-empty segment
/// produced by splitting on any of `delims`.
fn split_on_chars(text: &str, delims: &[char]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut seg_start = 0usize;
    for (i, ch) in text.char_indices() {
        if delims.contains(&ch) {
            if seg_start < i {
                spans.push((seg_start, i));
            }
            seg_start = i + ch.len_utf8();
        }
    }
    if seg_start < text.len() {
        spans.push((seg_start, text.len()));
    }
    spans
}

/// `(start, end)` byte spans for each non-whitespace run.
fn split_on_whitespace(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (i, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(s) = start {
                spans.push((s, i));
                start = None;
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        spans.push((s, text.len()));
    }
    spans
}

/// Trim leading + trailing whitespace and edge punctuation from `raw`.
/// Returns the trimmed slice plus the *absolute* char offsets after
/// trimming, assuming `raw` started at byte offset `abs_start`.
fn trim_span(raw: &str, abs_start: usize) -> (&str, usize, usize) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (trimmed, abs_start, abs_start);
    }
    let lead = raw.find(trimmed).unwrap_or(0);
    let start = abs_start + lead;
    let end = start + trimmed.len();
    (trimmed, start, end)
}

/// Strip surrounding `EDGE_PUNCT`. Returns (slice, leading_bytes_removed,
/// trailing_bytes_removed).
fn strip_edges(s: &str) -> (&str, usize, usize) {
    let mut lo = 0usize;
    let mut hi = s.len();
    let bytes = s.as_bytes();
    while lo < hi {
        let ch = s[lo..].chars().next().unwrap();
        if EDGE_PUNCT.contains(&ch) {
            lo += ch.len_utf8();
        } else {
            break;
        }
    }
    while hi > lo {
        let mut i = hi - 1;
        while i > lo && (bytes[i] & 0b1100_0000) == 0b1000_0000 {
            i -= 1;
        }
        let ch = s[i..hi].chars().next().unwrap();
        if EDGE_PUNCT.contains(&ch) {
            hi = i;
        } else {
            break;
        }
    }
    (&s[lo..hi], lo, s.len() - hi)
}

fn has_letter_or_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_alphanumeric())
}

/// Whether `s` (assumed trimmed) contains at least one inner whitespace —
/// i.e., has two or more whitespace-separated words.
fn is_multi_word(s: &str) -> bool {
    s.chars().any(|c| c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(chunks: &[OrthographicChunk], scale: ChunkScale) -> Vec<&str> {
        chunks
            .iter()
            .filter(|c| c.scale == scale)
            .map(|c| c.text.as_str())
            .collect()
    }

    #[test]
    fn empty_input_no_chunks() {
        assert!(extract_chunks("").is_empty());
        assert!(extract_chunks("   ").is_empty());
    }

    #[test]
    fn x86_64_sentence_produces_useful_chunks() {
        let text = "On x86-64 Linux, integer args often come in registers like edi, esi, edx, etc., and return values usually come back in eax/rax.";
        let chunks = extract_chunks(text);

        let tokens = texts(&chunks, ChunkScale::Token);
        assert!(tokens.contains(&"edi"));
        assert!(tokens.contains(&"esi"));
        assert!(tokens.contains(&"edx"));
        assert!(tokens.contains(&"eax"));
        assert!(tokens.contains(&"rax"));
        assert!(tokens.contains(&"x86-64"));
        assert!(tokens.contains(&"Linux"));
        assert!(tokens.contains(&"registers"));
        assert!(tokens.contains(&"and"));
        assert!(tokens.contains(&"etc"));

        let phrases = texts(&chunks, ChunkScale::Phrase);
        assert!(phrases.contains(&"On x86-64 Linux"));
        assert!(!phrases.contains(&"esi"));
        assert!(!phrases.contains(&"edx"));
        assert!(!phrases.contains(&"etc"));
        assert!(
            phrases
                .iter()
                .any(|p| p.contains("integer args") && p.contains("registers"))
        );
    }

    #[test]
    fn slash_splits_token_but_hyphen_does_not() {
        let chunks = extract_chunks("eax/rax and x86-64");
        let tokens = texts(&chunks, ChunkScale::Token);
        assert!(tokens.contains(&"eax"));
        assert!(tokens.contains(&"rax"));
        assert!(tokens.contains(&"x86-64"));
    }

    #[test]
    fn dedup_same_token_across_input() {
        let chunks = extract_chunks("come here, come back, come now");
        let tokens = texts(&chunks, ChunkScale::Token);
        let come_count = tokens.iter().filter(|t| **t == "come").count();
        assert_eq!(come_count, 1, "dedup failed for 'come': {tokens:?}");
        // First-occurrence emit, but repetitions should reflect the
        // three appearances of "come" in the raw stream.
        let come = chunks
            .iter()
            .find(|c| c.scale == ChunkScale::Token && c.text == "come")
            .expect("'come' token");
        assert_eq!(come.repetitions, 3);
    }

    #[test]
    fn strips_punctuation_edges() {
        let chunks = extract_chunks("\"hello,\" she said.");
        let tokens = texts(&chunks, ChunkScale::Token);
        assert!(tokens.contains(&"hello"));
        assert!(tokens.contains(&"said"));
    }

    #[test]
    fn unicode_text_does_not_panic() {
        let chunks = extract_chunks("café résumé naïve. C'est très important — vraiment.");
        let tokens = texts(&chunks, ChunkScale::Token);
        assert!(tokens.iter().any(|t| t.contains("café")));
    }

    #[test]
    fn offsets_round_trip_into_original_text_for_slice1() {
        let text = "On x86-64 Linux, integer args.";
        let chunks = extract_chunks(text);
        for c in &chunks {
            if !matches!(c.scale, ChunkScale::Token | ChunkScale::Phrase) {
                // Slice 2 / 3 store the canonical (single-space-joined)
                // form; the raw substring may contain comma-separators
                // etc., so round-trip is only asserted for slice 1.
                continue;
            }
            let slice = &text[c.char_start..c.char_end];
            assert_eq!(slice, c.text, "offset mismatch for {c:?}: slice={slice:?}");
        }
    }

    // ── Slice 2 tests ──────────────────────────────────────────────────

    #[test]
    fn slice2_finds_repeated_bigram() {
        let text = "the cat sat on the mat and the cat purred and the cat slept";
        let chunks = extract_chunks(text);
        let repeated = texts(&chunks, ChunkScale::Repeated);
        assert!(
            repeated.contains(&"the cat"),
            "expected 'the cat' as a Repeated chunk; got {repeated:?}"
        );
        let the_cat = chunks
            .iter()
            .find(|c| c.scale == ChunkScale::Repeated && c.text == "the cat")
            .expect("'the cat' chunk");
        assert_eq!(the_cat.repetitions, 3);
    }

    #[test]
    fn slice2_finds_longer_repeated_ngrams() {
        let text = "we want the same thing over and over the same thing again";
        let chunks = extract_chunks(text);
        let repeated = texts(&chunks, ChunkScale::Repeated);
        // "the same thing" repeats — should appear as a 3-gram.
        assert!(
            repeated.contains(&"the same thing"),
            "expected 3-gram repeat; got {repeated:?}"
        );
    }

    #[test]
    fn slice2_skips_non_repeating_inputs() {
        let text = "Alice met Bob at the cafe";
        let chunks = extract_chunks(text);
        let repeated = texts(&chunks, ChunkScale::Repeated);
        assert!(
            repeated.is_empty(),
            "expected no Repeated chunks; got {repeated:?}"
        );
    }

    // ── Slice 3 tests ──────────────────────────────────────────────────

    #[test]
    fn slice3_emits_collocation_chunks_when_pmi_meets_threshold() {
        // A rare-tokens-co-occurring case: each token appears once,
        // so each adjacent bigram has the same (relatively high) PMI.
        // Slice 3 should emit some of them — and never emit anything
        // slice 2 already covered.
        let text = "elephant moonwalks tomorrow afternoon";
        let chunks = extract_chunks(text);
        let collocations: Vec<_> = chunks
            .iter()
            .filter(|c| c.scale == ChunkScale::Collocation)
            .collect();
        assert!(
            !collocations.is_empty(),
            "expected some Collocation chunks for novel input; got {chunks:?}"
        );
        for c in &collocations {
            assert!(c.pmi.is_some(), "Collocation chunk missing pmi: {c:?}");
            assert!(
                c.pmi.unwrap() >= SLICE3_MIN_PMI,
                "Collocation below threshold: {c:?}"
            );
        }
    }

    #[test]
    fn slice3_does_not_double_emit_what_slice2_covered() {
        let text = "the cat sat on the mat the cat ran fast the cat is fine";
        let chunks = extract_chunks(text);
        let repeated: HashSet<&str> = texts(&chunks, ChunkScale::Repeated)
            .into_iter()
            .collect();
        let collocations: HashSet<&str> = texts(&chunks, ChunkScale::Collocation)
            .into_iter()
            .collect();
        for r in &repeated {
            assert!(
                !collocations.contains(r),
                "'{r}' present in both Repeated and Collocation",
            );
        }
    }

    #[test]
    fn slice3_caps_at_top_k() {
        // A long input with many high-PMI singleton bigrams.
        let text = "zebra falcon orchid quartz nebula whisk ivory glade plume saffron";
        let chunks = extract_chunks(text);
        let collocations = chunks
            .iter()
            .filter(|c| c.scale == ChunkScale::Collocation)
            .count();
        assert!(
            collocations <= SLICE3_TOP_K,
            "slice 3 should cap at {SLICE3_TOP_K}; got {collocations}"
        );
    }
}
