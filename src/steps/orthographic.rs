//! Step 5a — orthographic chunker. Pure functions, no model, no
//! persistent state. Produces content-bearing chunk candidates from
//! punctuation, whitespace, casing, and slash separators. Always
//! produces output for non-empty inputs; never depends on a label
//! schema.
//!
//! ### Phrase chunks
//! Punctuation- and whitespace-delimited spans. The cognate of the
//! brain's prosodic / orthographic boundary cues (Cutler & Norris 1988;
//! Pierrehumbert & Hirschberg 1990) — pre-semantic segmentation.
//! Always emits at least one chunk for non-empty input.
//!
//! Phrases get dropped when:
//! - shorter than 3 characters after trimming, or
//! - they don't contain at least one letter or digit, or
//! - the exact same text already appeared as a Phrase this input.
//!
//! ### Token chunks
//! Emitted by `src/steps/void_filter.rs` — every whitespace-and-slash
//! atom whose lowercased form does *not* resolve to a `Polarity::Void`
//! element. Closed-class function words drop out; content tokens
//! survive.
//!
//! Within-input n-gram repetition was tried as a third scale and
//! removed: signal hit rate is low within a single input, and the
//! useful version of repetition is cross-tick — that lives in replay
//! once the stats store exists.
//!
//! Sub-millisecond on typical inputs.

use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct OrthographicChunk {
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
    pub scale: ChunkScale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkScale {
    /// Major-punctuation-delimited span.
    Phrase,
    /// Whitespace-and-slash atom after punctuation stripping. Emitted
    /// by `src/steps/void_filter.rs` only for tokens whose lowercase
    /// form does *not* resolve to a `Polarity::Void` element — i.e.,
    /// content tokens.
    Token,
}

/// Major punctuation that ends a phrase. Comma is included because comma-
/// delimited lists are exactly the cold-start pattern ("edi, esi, edx, …") —
/// each list item deserves to be its own phrase candidate.
const PHRASE_DELIMS: &[char] = &[
    '.', ',', ';', ':', '!', '?', '—', '(', ')', '[', ']', '{', '}',
];

/// Characters stripped from token edges but allowed inside tokens.
const EDGE_PUNCT: &[char] = &[
    '.', ',', ';', ':', '!', '?', '(', ')', '[', ']', '{', '}', '"', '\'', '`', '—', '–', '"', '"',
    '\u{2018}', '\u{2019}',
];

/// Internal token separator (splits e.g. `eax/rax` into two tokens).
/// Hyphen is deliberately NOT in this set — `x86-64`, `front-end`,
/// `well-defined` stay whole.
const TOKEN_INTERNAL_SPLIT: char = '/';

/// Internal token record — every whitespace-and-slash atom in the
/// input, with edges stripped. Consumed by `src/steps/void_filter.rs`
/// to emit content-Token chunks.
pub(crate) struct RawToken {
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
}

/// Extract Phrase chunks from raw input text. Always returns at least
/// one chunk for non-empty input (worst case: the whole input as a
/// single Phrase chunk).
pub fn extract_chunks(text: &str) -> Vec<OrthographicChunk> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let mut out: Vec<OrthographicChunk> = Vec::new();
    let mut emitted_phrases: HashSet<String> = HashSet::new();

    for (start, end) in split_on_chars(text, PHRASE_DELIMS) {
        let raw = &text[start..end];
        let (trimmed_text, trim_start, trim_end) = trim_span(raw, start);
        if trimmed_text.chars().count() < 3 {
            continue;
        }
        if !has_letter_or_digit(trimmed_text) {
            continue;
        }
        if !emitted_phrases.insert(trimmed_text.to_string()) {
            continue;
        }
        out.push(OrthographicChunk {
            text: trimmed_text.to_string(),
            char_start: trim_start,
            char_end: trim_end,
            scale: ChunkScale::Phrase,
        });
    }

    out.sort_by_key(|c| c.char_start);
    out
}

/// Walk the input once, edge-strip every whitespace-and-slash atom,
/// keep every surviving token (no dedup, no rejection of repeats).
pub(crate) fn collect_raw_tokens(text: &str) -> Vec<RawToken> {
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
        accept_raw(&stripped[seg_start..], stripped_start + seg_start, &mut out);
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
    fn x86_64_sentence_produces_useful_phrases() {
        let text = "On x86-64 Linux, integer args often come in registers like edi, esi, edx, etc., and return values usually come back in eax/rax.";
        let chunks = extract_chunks(text);

        let phrases = texts(&chunks, ChunkScale::Phrase);
        assert!(phrases.contains(&"On x86-64 Linux"));
        // Comma splits *after* `edi`, so `edi` is the tail of the
        // preceding phrase. The standalone list items (between two
        // commas) emit as their own phrases.
        assert!(phrases.contains(&"esi"));
        assert!(phrases.contains(&"edx"));
        assert!(phrases.contains(&"etc"));
        assert!(
            phrases
                .iter()
                .any(|p| p.contains("integer args") && p.contains("edi"))
        );
        assert!(
            phrases
                .iter()
                .any(|p| p.contains("return values") && p.contains("eax/rax"))
        );
    }

    #[test]
    fn single_word_input_still_emits_phrase() {
        // Pre-change, single-word inputs survived only via the Token
        // branch. Now Phrases pick them up — empty output is never a
        // valid result for non-empty input.
        let chunks = extract_chunks("Hello");
        assert!(!chunks.is_empty(), "single-word input should emit a Phrase");
        let phrases = texts(&chunks, ChunkScale::Phrase);
        assert!(phrases.contains(&"Hello"));
    }

    #[test]
    fn strips_punctuation_edges_from_phrases() {
        let chunks = extract_chunks("\"hello,\" she said.");
        let phrases = texts(&chunks, ChunkScale::Phrase);
        assert!(phrases.iter().any(|p| p.contains("hello")));
        assert!(phrases.iter().any(|p| p.contains("said")));
    }

    #[test]
    fn unicode_text_does_not_panic() {
        let chunks = extract_chunks("café résumé naïve. C'est très important — vraiment.");
        let phrases = texts(&chunks, ChunkScale::Phrase);
        assert!(phrases.iter().any(|p| p.contains("café")));
    }

    #[test]
    fn phrase_offsets_round_trip_into_original_text() {
        let text = "On x86-64 Linux, integer args.";
        let chunks = extract_chunks(text);
        for c in &chunks {
            let slice = &text[c.char_start..c.char_end];
            assert_eq!(slice, c.text, "offset mismatch for {c:?}: slice={slice:?}");
        }
    }
}
