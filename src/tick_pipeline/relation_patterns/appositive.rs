//! Comma-appositive pattern extractor.
//!
//! Pattern: `<common-noun-head>, <Proper Name>` (or the reverse) emits
//! `Proper Name → instance_of → common-noun-head`. Operates on the
//! sentence-bounded global token stream — comma is a phrase boundary
//! in the orthographic chunker, so this can't run inside the per-phrase
//! SVO loop.
//!
//! ## Rules
//!
//! - One side's *head* (last content token) starts with a lowercase
//!   letter → common-noun side.
//! - The other side's tokens are *all* uppercase-initial or digit-
//!   containing → proper-name side.
//! - A coordinator ("and"/"or"/"but") in the raw-text gap between the
//!   two runs rejects the pair as list continuation, not appositive.
//!
//! ## Examples
//!
//! - "my new laptop, Dell XPS 13" → Dell XPS 13 → instance_of → laptop
//! - "Dell XPS 13, the new laptop" → Dell XPS 13 → instance_of → laptop
//! - "Sarah, John, Mike" → no relations (proper-name list)
//! - "shoes, socks, a shirt" → no relations (common-noun list)
//! - "Dell XPS 13, and my new smartphone" → no relation (coordinator)

use crate::tick_pipeline::orthographic::{ChunkScale, OrthographicChunk};
use crate::tick_pipeline::relation_patterns::{
    DEFAULT_SURFACE_CONFIDENCE, ObjectRef, PatternSource, RelationCandidate,
};
use crate::types::RelationStatus;

/// Extract appositive triples from a phrase + token chunk slice.
pub fn extract_appositives(text: &str, chunks: &[OrthographicChunk]) -> Vec<RelationCandidate> {
    let mut out = Vec::new();

    let all_tokens: Vec<&OrthographicChunk> = chunks
        .iter()
        .filter(|c| c.scale == ChunkScale::Token)
        .collect();
    let sentences = split_sentences(text, &all_tokens);
    for sentence in &sentences {
        let runs = split_runs_by_comma(text, &all_tokens, sentence);
        for pair in runs.windows(2) {
            let (left, right) = (&pair[0], &pair[1]);
            let left_end = all_tokens[left.1].char_end;
            let right_start = all_tokens[right.0].char_start;
            if gap_has_coordinator(&text[left_end..right_start]) {
                continue;
            }
            if let Some((proper_run, head_tok)) = classify_appositive(left, right, &all_tokens) {
                let head = all_tokens[head_tok];
                let proper_first = all_tokens[proper_run.0];
                let proper_last = all_tokens[proper_run.1];
                out.push(RelationCandidate {
                    source: PatternSource::Appositive,
                    subject_char_start: proper_first.char_start,
                    subject_char_end: proper_last.char_end,
                    attribute_name: "instance_of".to_string(),
                    attribute_char_start: None,
                    attribute_char_end: None,
                    object: ObjectRef::Span {
                        char_start: head.char_start,
                        char_end: head.char_end,
                    },
                    confidence: DEFAULT_SURFACE_CONFIDENCE,
                    status: RelationStatus::Defeasible,
                    event_anchor: None,
                });
            }
        }
    }

    out
}

/// True iff the raw text gap between two adjacent runs contains a
/// whole-word coordinator ("and", "or", "but"). Coordinators in this
/// position mark list continuation, not appositive — "Dell XPS 13,
/// and my new smartphone" pairs Dell with the next *list item*, not
/// with a renaming of itself.
fn gap_has_coordinator(gap: &str) -> bool {
    let lower = gap.to_ascii_lowercase();
    for coord in ["and", "or", "but"] {
        let mut search = lower.as_str();
        while let Some(pos) = search.find(coord) {
            let before_ok = pos == 0 || !search.as_bytes()[pos - 1].is_ascii_alphanumeric();
            let end = pos + coord.len();
            let after_ok = end >= search.len() || !search.as_bytes()[end].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
            search = &search[end..];
        }
    }
    false
}

/// Split a Token-chunk slice into sentence-bounded subranges. A
/// sentence-ending punctuation (`. ! ?`) in the raw gap between two
/// adjacent tokens ends a sentence.
fn split_sentences(text: &str, tokens: &[&OrthographicChunk]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if tokens.is_empty() {
        return out;
    }
    let mut start = 0usize;
    for i in 1..tokens.len() {
        let gap = &text[tokens[i - 1].char_end..tokens[i].char_start];
        if gap.contains('.') || gap.contains('!') || gap.contains('?') {
            out.push((start, i - 1));
            start = i;
        }
    }
    out.push((start, tokens.len() - 1));
    out
}

/// Returns the inclusive token-index ranges of comma-separated runs
/// inside one sentence-bounded subrange of `tokens`.
fn split_runs_by_comma(
    text: &str,
    tokens: &[&OrthographicChunk],
    sentence: &(usize, usize),
) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let (lo, hi) = (sentence.0, sentence.1);
    if lo > hi || tokens.is_empty() {
        return runs;
    }
    let mut run_start = lo;
    for i in (lo + 1)..=hi {
        let gap = &text[tokens[i - 1].char_end..tokens[i].char_start];
        if gap.contains(',') {
            runs.push((run_start, i - 1));
            run_start = i;
        }
    }
    runs.push((run_start, hi));
    runs
}

/// Decide whether two adjacent token-runs form a (common-noun, proper
/// name) pair in either order. Returns `(proper_run_indices,
/// head_token_index)` if yes.
///
/// The "all tokens are proper-shape" rule on the proper side rejects
/// predicate-headed clauses ("..., and I'm loving it"). No verb-shape
/// filter on the common-noun side because verbs legitimately appear
/// there ("I bought a phone, Samsung Galaxy S22").
fn classify_appositive(
    left: &(usize, usize),
    right: &(usize, usize),
    tokens: &[&OrthographicChunk],
) -> Option<((usize, usize), usize)> {
    if let Some(head) = head_is_common(left, tokens)
        && run_is_proper(right, tokens)
    {
        return Some((*right, head));
    }
    if let Some(head) = head_is_common(right, tokens)
        && run_is_proper(left, tokens)
    {
        return Some((*left, head));
    }
    None
}

fn head_is_common(run: &(usize, usize), tokens: &[&OrthographicChunk]) -> Option<usize> {
    let tail = run.1;
    let first_char = tokens[tail].text.chars().next()?;
    if first_char.is_ascii_lowercase() {
        Some(tail)
    } else {
        None
    }
}

fn run_is_proper(run: &(usize, usize), tokens: &[&OrthographicChunk]) -> bool {
    (run.0..=run.1).all(|i| {
        let t = &tokens[i].text;
        let Some(first) = t.chars().next() else {
            return false;
        };
        let has_digit = t.chars().any(|c| c.is_ascii_digit());
        first.is_ascii_uppercase() || has_digit
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;
    use crate::tick_pipeline::orthographic::extract_chunks;
    use crate::tick_pipeline::void_filter::extract_content_tokens;

    fn chunks_for(text: &str) -> Vec<OrthographicChunk> {
        let hypergraph = load_seed_graph();
        let mut chunks = extract_chunks(text);
        chunks.extend(extract_content_tokens(text, &hypergraph));
        chunks.sort_by_key(|c| (c.char_start, c.char_end));
        chunks
    }

    fn obj_text<'a>(r: &RelationCandidate, text: &'a str) -> &'a str {
        match r.object {
            ObjectRef::Span {
                char_start,
                char_end,
            } => &text[char_start..char_end],
            ObjectRef::Label(_) => panic!("expected Span object, got Label"),
        }
    }

    #[test]
    fn appositive_links_common_noun_to_proper_name() {
        let text = "I need a new laptop, Dell XPS 13";
        let chunks = chunks_for(text);
        let rels = extract_appositives(text, &chunks);
        let r = rels
            .iter()
            .find(|r| r.attribute_name == "instance_of")
            .expect("expected an instance_of relation from the appositive");
        assert_eq!(obj_text(r, text), "laptop");
        let subj = &text[r.subject_char_start..r.subject_char_end];
        assert!(
            subj == "Dell"
                || subj == "Dell XPS 13"
                || subj == "Dell XPS"
                || subj.starts_with("Dell"),
            "subject should start with the proper-name run; got {subj:?}"
        );
    }

    #[test]
    fn appositive_works_in_reversed_order() {
        let text = "Dell XPS 13, the new laptop";
        let chunks = chunks_for(text);
        let rels = extract_appositives(text, &chunks);
        let r = rels
            .iter()
            .find(|r| r.attribute_name == "instance_of")
            .expect("expected instance_of from reversed appositive");
        assert_eq!(obj_text(r, text), "laptop");
    }

    #[test]
    fn appositive_skips_lists_of_common_nouns() {
        let text = "I packed shoes, socks, and a shirt";
        let chunks = chunks_for(text);
        let rels = extract_appositives(text, &chunks);
        for r in &rels {
            assert_ne!(
                r.attribute_name, "instance_of",
                "list of common nouns should not produce instance_of: {r:?}"
            );
        }
    }

    #[test]
    fn appositive_rejects_coordinator_continuation() {
        let text = "Dell XPS 13, and my new smartphone, Samsung Galaxy S22";
        let chunks = chunks_for(text);
        let rels = extract_appositives(text, &chunks);
        let bad = rels.iter().find(|r| {
            r.attribute_name == "instance_of"
                && &text[r.subject_char_start..r.subject_char_end] == "Dell XPS 13"
                && obj_text(r, text) == "smartphone"
        });
        assert!(bad.is_none(), "should not pair across `and`; got {:?}", bad);
        let good = rels
            .iter()
            .find(|r| r.attribute_name == "instance_of" && obj_text(r, text) == "smartphone");
        assert!(
            good.is_some(),
            "legitimate (smartphone, Samsung Galaxy S22) pair should fire"
        );
    }

    #[test]
    fn appositive_skips_lists_of_proper_names() {
        let text = "Sarah, John, Mike";
        let chunks = chunks_for(text);
        let rels = extract_appositives(text, &chunks);
        for r in &rels {
            assert_ne!(
                r.attribute_name, "instance_of",
                "list of proper names should not produce instance_of: {r:?}"
            );
        }
    }
}
