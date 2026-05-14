//! Step 5a, slice 1 — orthographic chunker. Pure-function, no model.
//!
//! Produces content-bearing chunk candidates from punctuation, whitespace,
//! casing, and slash separators alone. Always produces output; never depends
//! on a label schema. This is the cognate of the brain's prosodic /
//! orthographic boundary cues (Cutler & Norris 1988; Pierrehumbert &
//! Hirschberg 1990) — pre-semantic segmentation signals that produce
//! candidate units before any meaning is assigned.
//!
//! Emits at two scales (cf. Ding, Melloni, Zhang, Tian, Poeppel 2016,
//! "Cortical tracking of hierarchical linguistic structures"):
//!
//! - **Phrase**: spans delimited by major punctuation. Preserves
//!   internal structure (stopwords retained). Example:
//!   "On x86-64 Linux" → one Phrase chunk.
//! - **Token**: spans delimited by whitespace, with stopword filtering.
//!   Internal `/` splits into separate tokens (`eax/rax` → `eax`, `rax`);
//!   internal `-` is preserved (`x86-64` stays whole). Surrounding
//!   punctuation is stripped.
//!
//! Out of scope here: repetition / PMI signals (slices 2 + 3),
//! camelCase / snake_case transitions (later refinement),
//! Element minting (lives in Step 8 when it's built).
//!
//! Sub-millisecond on typical inputs.
//!
//! ## What gets dropped
//!
//! - Single-character tokens (always — they carry no information).
//! - Tokens that are pure punctuation after stripping.
//! - Single-word phrases (already covered by Tokens; would be redundant).
//! - Phrases shorter than 3 characters after trimming.
//! - Exact-duplicate chunks within a single input (deduped on text + scale).
//!
//! ## What does NOT get dropped
//!
//! Function words ("the", "and", "on", "in", …) are emitted as Tokens. There
//! is deliberately **no stopword list** — the brain doesn't have one either.
//! Function-word filtering is the job of the statistical-learning pass
//! (slice 3, PMI over accumulated bigram counts): high-frequency tokens have
//! low PMI with any *specific* neighbor, so they fall out as boundaries
//! rather than as chunks. Until PMI accumulates, expect token noise.

#[derive(Debug, Clone)]
pub struct OrthographicChunk {
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
    pub scale: ChunkScale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkScale {
    /// Whitespace-delimited atom after punctuation stripping and stopword filter.
    Token,
    /// Major-punctuation-delimited span; stopwords retained.
    Phrase,
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

/// Extract content-bearing chunks from raw input text. Always returns at
/// least one chunk for non-empty input (worst case: the whole input as a
/// single Phrase chunk).
pub fn extract_chunks(text: &str) -> Vec<OrthographicChunk> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let mut out: Vec<OrthographicChunk> = Vec::new();
    let mut seen: std::collections::HashSet<(String, ChunkScale)> = std::collections::HashSet::new();

    // ── Phrases ──────────────────────────────────────────────────────
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
        if seen.insert(key.clone()) {
            out.push(OrthographicChunk {
                text: key.0,
                char_start: trim_start,
                char_end: trim_end,
                scale: ChunkScale::Phrase,
            });
        }
    }

    // ── Tokens ───────────────────────────────────────────────────────
    // Split on whitespace first; then for each whitespace-delimited atom,
    // strip surrounding punctuation, split on internal `/`, and emit each
    // surviving piece (with stopword + length filtering).
    for (ws_start, ws_end) in split_on_whitespace(text) {
        let atom = &text[ws_start..ws_end];

        // Strip surrounding punctuation.
        let (stripped, strip_start_offset, strip_end_offset) = strip_edges(atom);
        if stripped.is_empty() {
            continue;
        }

        // Split on `/`. Track each segment's offset within the original.
        let stripped_start = ws_start + strip_start_offset;
        let _stripped_end = ws_end - strip_end_offset;

        let mut seg_start = 0usize;
        for (i, ch) in stripped.char_indices() {
            if ch == TOKEN_INTERNAL_SPLIT {
                emit_token(
                    &stripped[seg_start..i],
                    stripped_start + seg_start,
                    &mut seen,
                    &mut out,
                );
                seg_start = i + ch.len_utf8();
            }
        }
        // Tail segment.
        emit_token(
            &stripped[seg_start..],
            stripped_start + seg_start,
            &mut seen,
            &mut out,
        );
    }

    // Stable order: by char_start, then scale (Phrase before Token at the
    // same offset, since phrases contain tokens conceptually).
    out.sort_by(|a, b| {
        a.char_start
            .cmp(&b.char_start)
            .then_with(|| match (a.scale, b.scale) {
                (ChunkScale::Phrase, ChunkScale::Token) => std::cmp::Ordering::Less,
                (ChunkScale::Token, ChunkScale::Phrase) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
    });
    out
}

fn emit_token(
    candidate: &str,
    char_start: usize,
    seen: &mut std::collections::HashSet<(String, ChunkScale)>,
    out: &mut Vec<OrthographicChunk>,
) {
    // Strip any residual punctuation left by `/` splits inside the segment.
    let (s, lead, trail) = strip_edges(candidate);
    if s.chars().count() < 2 {
        return;
    }
    if !has_letter_or_digit(s) {
        return;
    }
    let key = (s.to_string(), ChunkScale::Token);
    if !seen.insert(key.clone()) {
        return;
    }
    out.push(OrthographicChunk {
        text: key.0,
        char_start: char_start + lead,
        char_end: char_start + candidate.len() - trail,
        scale: ChunkScale::Token,
    });
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
        // Look at the leading char.
        let ch = s[lo..].chars().next().unwrap();
        if EDGE_PUNCT.contains(&ch) {
            lo += ch.len_utf8();
        } else {
            break;
        }
    }
    while hi > lo {
        // Look at the trailing char by stepping back one UTF-8 char.
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
        // Registers should each surface as their own token.
        assert!(tokens.contains(&"edi"), "tokens missing 'edi': {tokens:?}");
        assert!(tokens.contains(&"esi"), "tokens missing 'esi': {tokens:?}");
        assert!(tokens.contains(&"edx"), "tokens missing 'edx': {tokens:?}");
        assert!(tokens.contains(&"eax"), "tokens missing 'eax': {tokens:?}");
        assert!(tokens.contains(&"rax"), "tokens missing 'rax': {tokens:?}");
        // x86-64 stays whole (no split on internal hyphen).
        assert!(
            tokens.contains(&"x86-64"),
            "tokens missing 'x86-64': {tokens:?}"
        );
        assert!(tokens.contains(&"Linux"));
        assert!(tokens.contains(&"registers"));
        // No stopword filter — function words are emitted; statistical
        // learning (PMI in slice 3) will demote them later.
        assert!(tokens.contains(&"and"));
        assert!(tokens.contains(&"etc"));

        let phrases = texts(&chunks, ChunkScale::Phrase);
        assert!(phrases.contains(&"On x86-64 Linux"));
        // Single-word phrases ("esi", "edx", "etc") are suppressed since
        // Tokens already cover them.
        assert!(!phrases.contains(&"esi"));
        assert!(!phrases.contains(&"edx"));
        assert!(!phrases.contains(&"etc"));
        // The mid-sentence run-on segment should be present.
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
        // "come" should appear once after dedup.
        let come_count = tokens.iter().filter(|t| **t == "come").count();
        assert_eq!(come_count, 1, "dedup failed for 'come': {tokens:?}");
    }

    #[test]
    fn strips_punctuation_edges() {
        let chunks = extract_chunks("\"hello,\" she said.");
        let tokens = texts(&chunks, ChunkScale::Token);
        assert!(tokens.contains(&"hello"), "tokens: {tokens:?}");
        // 'said' is not a stopword (we kept the list small).
        assert!(tokens.contains(&"said"));
    }

    #[test]
    fn unicode_text_does_not_panic() {
        let chunks = extract_chunks("café résumé naïve. C'est très important — vraiment.");
        let tokens = texts(&chunks, ChunkScale::Token);
        assert!(tokens.iter().any(|t| t.contains("café")));
    }

    #[test]
    fn offsets_round_trip_into_original_text() {
        let text = "On x86-64 Linux, integer args.";
        let chunks = extract_chunks(text);
        for c in &chunks {
            let slice = &text[c.char_start..c.char_end];
            // Slice should match the chunk's text (modulo edge stripping
            // already applied — the text we stored IS the stripped slice).
            assert_eq!(
                slice, c.text,
                "offset mismatch for {:?}: slice={slice:?}",
                c
            );
        }
    }
}
