//! Step 6 — Coreference scoring.
//!
//! Pure-Rust scorer. For each ambiguous span (pronoun or definite
//! description) in the input, pick the best-scoring antecedent from
//! `recent_focus` and emit a `CorefDecision`. Step 8's existing
//! `apply_coref_decisions` (§4a) uses the decision to bind the span
//! to the antecedent's element rather than minting a fresh one.
//!
//! Identity is conservative: decisions only fire when the aggregate
//! score clears `policy.coref_threshold`. Below threshold, the span
//! falls through to Step 8's normal mint path — a missed coref
//! produces a provisional element that replay can merge later;
//! a wrong coref binds two distinct entities together, which is
//! hard to unwind.
//!
//! See `step_6_design.md` for the spec; this is phase 1 (ambiguous-
//! span detection only). Phases 2-5 add scoring + threshold + tests.

use crate::inference::deberta::predict::LabeledSpan;
use crate::types::Hypergraph;

/// A pronoun or definite-description span that referred to an
/// already-mentioned entity. Step 8 uses these to bind the pronoun's
/// argument slot to the antecedent's element rather than minting a
/// fresh provisional instance.
#[derive(Debug, Clone)]
pub struct CorefDecision {
    pub pronoun_text: String,
    pub pronoun_char_start: usize,
    pub pronoun_char_end: usize,
    pub antecedent_text: String,
    pub confidence: f32,
}

/// Kind of ambiguous span. `Pronoun` is closed-class (he/she/it/…);
/// `DefiniteDescription` is `the <head_noun>` where `head_noun`
/// resolves via `by_name` to a Signal element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbiguousKind {
    Pronoun,
    DefiniteDescription,
}

/// A span in the input text that may refer to an already-mentioned
/// entity. Returned by `detect_ambiguous_spans` for Step 6 scoring.
#[derive(Debug, Clone)]
pub struct AmbiguousSpan {
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
    pub kind: AmbiguousKind,
}

/// Closed-class pronouns Step 6 considers for coref. Subject /
/// object / possessive forms all collapse to one antecedent.
const PRONOUNS: &[&str] = &[
    "he", "she", "it", "they", "this", "that", "him", "her", "them", "his", "hers", "its", "their",
];

/// Resolve coreferences within `input_text` given the current
/// hypergraph state. Returns an empty vector while
/// `Hypergraph.recent_focus` is unpopulated.
///
/// Phase 1 ships ambiguous-span detection only; phases 2-5 wire
/// scoring + threshold gate + decisions.
pub fn resolve_coref(
    _input_text: &str,
    _hg: &Hypergraph,
    _ner_spans: &[LabeledSpan],
) -> Vec<CorefDecision> {
    Vec::new()
}

/// Walk `input_text` and identify pronouns + definite descriptions
/// that aren't already covered by an NER span. Returns spans in
/// occurrence order. Whole-word match against lowercased text; the
/// surface form is preserved in `AmbiguousSpan.text`.
///
/// Pronouns: closed-class list above.
/// Definite descriptions: `the <head_noun>` where `head_noun`
/// (last token of the NP, English head-rightmost convention)
/// resolves via `hg.by_name` to at least one Signal element.
pub fn detect_ambiguous_spans(
    input_text: &str,
    hg: &Hypergraph,
    ner_spans: &[LabeledSpan],
) -> Vec<AmbiguousSpan> {
    let mut out: Vec<AmbiguousSpan> = Vec::new();

    // Tokenize on whitespace into (char_start, char_end, text) triples.
    // The chunker module has richer tokenization, but for coref's
    // closed-class match the simple walk is enough — we just need
    // word-boundary spans we can check against NER overlap.
    let mut tokens: Vec<(usize, usize, &str)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, ch) in input_text.char_indices() {
        let is_word = ch.is_alphanumeric() || ch == '\'' || ch == '_';
        match (start, is_word) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                tokens.push((s, i, &input_text[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        tokens.push((s, input_text.len(), &input_text[s..]));
    }

    for (idx, &(s, e, tok)) in tokens.iter().enumerate() {
        if overlaps_ner(s, e, ner_spans) {
            continue;
        }
        let lower = tok.to_ascii_lowercase();
        // Pronoun detection.
        if PRONOUNS.contains(&lower.as_str()) {
            out.push(AmbiguousSpan {
                text: tok.to_string(),
                char_start: s,
                char_end: e,
                kind: AmbiguousKind::Pronoun,
            });
            continue;
        }
        // Definite description: `the <head>` where `head` is the
        // rightmost noun of the NP. Without a POS tagger we use a
        // tighter heuristic: try each NP length (1..=3 tokens past
        // `the`) and pick the LONGEST one whose head token resolves
        // to a Signal element via `by_name`. This rejects extensions
        // like "the user replied" (head "replied" doesn't resolve)
        // while still catching compound NPs like "the dentist
        // appointment" (both heads resolve; pick the longer).
        if lower == "the" {
            let max_len = 3.min(tokens.len() - idx - 1);
            if max_len == 0 {
                continue; // "the" with no following word.
            }
            let mut best_end_idx: Option<usize> = None;
            for len in 1..=max_len {
                let candidate_end = idx + len;
                let head_tok = tokens[candidate_end].2;
                // Stop extending if we hit a known boundary word —
                // these would push the NP past its real head noun.
                if matches!(
                    head_tok.to_ascii_lowercase().as_str(),
                    "of" | "in" | "and" | "for" | "to" | "with" | "at" | "from"
                ) {
                    break;
                }
                if head_resolves_signal(hg, head_tok) {
                    best_end_idx = Some(candidate_end);
                }
            }
            let Some(np_end_idx) = best_end_idx else {
                continue; // No NP-prefix had a head that resolved.
            };
            let np_start = tokens[idx].0;
            let np_end = tokens[np_end_idx].1;
            // Skip if the NP overlaps an NER span (NER already gave
            // it an identity).
            if overlaps_ner(np_start, np_end, ner_spans) {
                continue;
            }
            out.push(AmbiguousSpan {
                text: input_text[np_start..np_end].to_string(),
                char_start: np_start,
                char_end: np_end,
                kind: AmbiguousKind::DefiniteDescription,
            });
        }
    }

    out
}

fn overlaps_ner(start: usize, end: usize, ner_spans: &[LabeledSpan]) -> bool {
    ner_spans
        .iter()
        .any(|s| !(end <= s.char_start || start >= s.char_end))
}

/// Resolves to at least one `Polarity::Signal` element via either
/// the exact-case name or its lowercased form? Used to gate
/// definite-description NP-head acceptance.
fn head_resolves_signal(hg: &Hypergraph, head_tok: &str) -> bool {
    let any_signal = |ids: &[crate::types::ElementId]| {
        ids.iter().any(|id| {
            matches!(
                hg.elements[id.0 as usize].polarity,
                crate::types::Polarity::Signal
            )
        })
    };
    if let Some(ids) = hg.by_name.get(head_tok)
        && any_signal(ids)
    {
        return true;
    }
    let head_lower = head_tok.to_ascii_lowercase();
    if head_lower != head_tok
        && let Some(ids) = hg.by_name.get(&head_lower)
        && any_signal(ids)
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;

    #[test]
    fn returns_empty_with_no_recent_focus() {
        let hg = Hypergraph::default();
        let decisions = resolve_coref("She left.", &hg, &[]);
        assert!(decisions.is_empty());
    }

    #[test]
    fn detects_subject_pronoun() {
        let hg = load_seed_graph();
        let spans = detect_ambiguous_spans("She left early.", &hg, &[]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "She");
        assert_eq!(spans[0].kind, AmbiguousKind::Pronoun);
        assert_eq!(spans[0].char_start, 0);
        assert_eq!(spans[0].char_end, 3);
    }

    #[test]
    fn detects_multiple_pronouns() {
        let hg = load_seed_graph();
        let spans = detect_ambiguous_spans("He gave her his book.", &hg, &[]);
        let texts: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["He", "her", "his"]);
    }

    #[test]
    fn pronoun_match_is_case_insensitive() {
        let hg = load_seed_graph();
        let spans = detect_ambiguous_spans("THEY arrived.", &hg, &[]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "THEY");
        assert_eq!(spans[0].kind, AmbiguousKind::Pronoun);
    }

    #[test]
    fn detects_definite_description_for_seeded_head() {
        // `user` is seeded; "the user" should fire as a definite
        // description.
        let hg = load_seed_graph();
        let spans = detect_ambiguous_spans("the user replied.", &hg, &[]);
        let dd: Vec<&AmbiguousSpan> = spans
            .iter()
            .filter(|s| s.kind == AmbiguousKind::DefiniteDescription)
            .collect();
        assert_eq!(
            dd.len(),
            1,
            "expected one definite description; got {spans:?}"
        );
        assert_eq!(dd[0].text, "the user");
    }

    #[test]
    fn skips_definite_description_for_unknown_head() {
        // `qwlfkjsk` doesn't resolve in by_name — no DD should fire.
        let hg = load_seed_graph();
        let spans = detect_ambiguous_spans("the qwlfkjsk arrived.", &hg, &[]);
        let dd: Vec<&AmbiguousSpan> = spans
            .iter()
            .filter(|s| s.kind == AmbiguousKind::DefiniteDescription)
            .collect();
        assert!(dd.is_empty(), "DD shouldn't fire for unresolved head");
    }

    #[test]
    fn skips_pronoun_that_overlaps_ner_span() {
        // Synthetic NER span covering the pronoun → coref should
        // skip it (NER already gave it an identity).
        let hg = load_seed_graph();
        let ner = vec![LabeledSpan {
            char_start: 0,
            char_end: 3,
            label: "person".to_string(),
            text: "She".to_string(),
            score: 0.99,
        }];
        let spans = detect_ambiguous_spans("She left.", &hg, &ner);
        assert!(spans.is_empty());
    }

    #[test]
    fn detects_demonstrative_pronouns() {
        let hg = load_seed_graph();
        let spans = detect_ambiguous_spans("That is mine.", &hg, &[]);
        let texts: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(texts.contains(&"That"));
    }

    #[test]
    fn no_false_positive_for_bare_the() {
        let hg = load_seed_graph();
        // "the." with no following word — skip.
        let spans = detect_ambiguous_spans("the.", &hg, &[]);
        let dd: Vec<&AmbiguousSpan> = spans
            .iter()
            .filter(|s| s.kind == AmbiguousKind::DefiniteDescription)
            .collect();
        assert!(dd.is_empty());
    }
}
