//! `coref` — Coreference scoring.
//!
//! Pure-Rust scorer. For each ambiguous span (pronoun or definite
//! description) in the input, pick the best-scoring antecedent from
//! `recent_focus` and emit a `CorefDecision`. `build_relations`'s existing
//! `apply_coref_decisions` (§4a) uses the decision to bind the span
//! to the antecedent's element rather than minting a fresh one.
//!
//! Identity is conservative: decisions only fire when the aggregate
//! score clears `policy.coref_threshold`. Below threshold, the span
//! falls through to `build_relations`'s normal mint path — a missed coref
//! produces a provisional element that replay can merge later;
//! a wrong coref binds two distinct entities together, which is
//! hard to unwind.
//!
//! See `new_foundation.md`.

use crate::embed::embed_span_in_context;
use crate::inference::deberta::predict::LabeledSpan;
use crate::math::dot;
use crate::types::{ElementId, Hypergraph, Policy, Tick};

/// A pronoun or definite-description span that referred to an
/// already-mentioned entity. `build_relations` uses these to bind the pronoun's
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
/// entity. Returned by `detect_ambiguous_spans` for `coref` scoring.
#[derive(Debug, Clone)]
pub struct AmbiguousSpan {
    pub text: String,
    pub char_start: usize,
    pub char_end: usize,
    pub kind: AmbiguousKind,
}

/// Closed-class pronouns `coref` considers for coref. Subject /
/// object / possessive forms all collapse to one antecedent.
const PRONOUNS: &[&str] = &[
    "he", "she", "it", "they", "this", "that", "him", "her", "them", "his", "hers", "its", "their",
];

/// Resolve coreferences within `input_text` given the current
/// hypergraph state.
///
/// For each ambiguous span (pronoun or definite description),
/// score every `recent_focus` candidate and emit a `CorefDecision`
/// if the max-scoring candidate clears `policy.coref_threshold`.
/// Below threshold: no decision (the span falls through to `build_relations`'s
/// normal mint path).
///
/// Returns an empty vector when `recent_focus` is empty (first tick
/// of a session) or when no ambiguous spans are present.
pub fn resolve_coref(
    input_text: &str,
    hypergraph: &Hypergraph,
    ner_spans: &[LabeledSpan],
    policy: &Policy,
) -> Vec<CorefDecision> {
    let spans = detect_ambiguous_spans(input_text, hypergraph, ner_spans);
    if spans.is_empty() || hypergraph.recent_focus.is_empty() {
        return Vec::new();
    }
    let candidates = collect_candidates(hypergraph);
    if candidates.is_empty() {
        return Vec::new();
    }

    // Max possible score: 1 (name) + 1 (embedding) + 0.5 (attr) +
    // 1 (recency) = 3.5. We divide by this for the published
    // confidence.
    const MAX_SCORE: f32 = 3.5;

    let mut decisions: Vec<CorefDecision> = Vec::new();
    for span in &spans {
        let mut best: Option<(&Candidate, f32)> = None;
        for cand in &candidates {
            let s = score_candidate(span, input_text, hypergraph, cand);
            match best {
                Some((_, b)) if s <= b => {}
                _ => best = Some((cand, s)),
            }
        }
        let Some((cand, score)) = best else { continue };
        if score < policy.coref_threshold {
            continue;
        }
        let antecedent_name = hypergraph.elements[cand.element.0 as usize]
            .names
            .first()
            .cloned()
            .unwrap_or_default();
        if antecedent_name.is_empty() {
            continue;
        }
        decisions.push(CorefDecision {
            pronoun_text: span.text.clone(),
            pronoun_char_start: span.char_start,
            pronoun_char_end: span.char_end,
            antecedent_text: antecedent_name,
            confidence: (score / MAX_SCORE).clamp(0.0, 1.0),
        });
    }
    decisions
}

/// Walk `input_text` and identify pronouns + definite descriptions
/// that aren't already covered by an NER span. Returns spans in
/// occurrence order. Whole-word match against lowercased text; the
/// surface form is preserved in `AmbiguousSpan.text`.
///
/// Pronouns: closed-class list above.
/// Definite descriptions: `the <head_noun>` where `head_noun`
/// (last token of the NP, English head-rightmost convention)
/// resolves via `hypergraph.by_name` to at least one Signal element.
pub fn detect_ambiguous_spans(
    input_text: &str,
    hypergraph: &Hypergraph,
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
        let lower = tok.to_ascii_lowercase();
        // Pronoun detection runs BEFORE the NER-overlap check.
        // Pronouns are closed-class so the surface-form match is
        // more reliable than NER's zero-shot label (GLiNER routinely
        // mistags pronouns as `org` / `person` with low confidence).
        // `build_relations`'s `apply_coref_decisions` runs first and rebinds the
        // pronoun's char range to the antecedent; downstream
        // `resolve_span` calls — including the ones driven by NER
        // proposals — short-circuit on the span cache, so the NER
        // proposal for "It" silently retargets at the antecedent.
        if PRONOUNS.contains(&lower.as_str()) {
            out.push(AmbiguousSpan {
                text: tok.to_string(),
                char_start: s,
                char_end: e,
                kind: AmbiguousKind::Pronoun,
            });
            continue;
        }
        if overlaps_ner(s, e, ner_spans) {
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
                if head_resolves_signal(hypergraph, head_tok) {
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

// ─── Phase 2: candidate set + scoring ────────────────────────────────

/// One coref candidate gathered from `Hypergraph.recent_focus`.
/// Multiple focus entries pointing at the same element collapse
/// into one Candidate that carries the union of their attribute
/// slots and the freshest tick.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub element: ElementId,
    /// Attribute slots this element was focused under (subject /
    /// target / actor / ...). Lets the attribute_overlap_hint
    /// score know whether the element has ever been focal.
    pub attributes: Vec<Option<ElementId>>,
    /// Freshest tick across this element's recent_focus entries.
    /// Drives the recency bonus.
    pub freshest_tick: Tick,
}

/// Dedupe `hypergraph.recent_focus` by element. Most recent entry wins for
/// `freshest_tick`; attribute slots accumulate. Returns candidates
/// in recency-first order (matching the deque order).
pub(crate) fn collect_candidates(hypergraph: &Hypergraph) -> Vec<Candidate> {
    let mut by_element: std::collections::HashMap<ElementId, Candidate> =
        std::collections::HashMap::new();
    let mut order: Vec<ElementId> = Vec::new();
    for entry in &hypergraph.recent_focus {
        let cand = by_element.entry(entry.element).or_insert_with(|| {
            order.push(entry.element);
            Candidate {
                element: entry.element,
                attributes: Vec::new(),
                freshest_tick: entry.tick,
            }
        });
        if !cand.attributes.contains(&entry.attribute) {
            cand.attributes.push(entry.attribute);
        }
        if entry.tick.0 > cand.freshest_tick.0 {
            cand.freshest_tick = entry.tick;
        }
    }
    order
        .into_iter()
        .map(|id| {
            by_element.remove(&id).expect(
                "by_element must contain every id from `order`; both are built from \
                 the same recent_focus walk in the same pass",
            )
        })
        .collect()
}

/// name_overlap: how well does the span's surface form match the
/// candidate's seeded / minted names? Pronouns return 0 (no surface
/// info). For definite descriptions, exact head-match → 1.0;
/// substring match → 0.5; else 0.
pub(crate) fn name_overlap(span: &AmbiguousSpan, hypergraph: &Hypergraph, candidate_id: ElementId) -> f32 {
    if matches!(span.kind, AmbiguousKind::Pronoun) {
        return 0.0;
    }
    // Definite-description head = last whitespace-delimited token.
    let head = span.text.split_whitespace().last().unwrap_or("");
    let head_lower = head.to_ascii_lowercase();
    let names = &hypergraph.elements[candidate_id.0 as usize].names;
    for name in names {
        let name_lower = name.to_ascii_lowercase();
        if name_lower == head_lower {
            return 1.0;
        }
    }
    for name in names {
        let name_lower = name.to_ascii_lowercase();
        if name_lower.contains(&head_lower) || head_lower.contains(&name_lower) {
            return 0.5;
        }
    }
    0.0
}

/// embedding_similarity: cosine between the span's contextualized
/// embedding and the candidate Element's embedding. Returns 0 if
/// the span doesn't tokenize cleanly (no contextualized vector).
pub(crate) fn embedding_similarity(
    span: &AmbiguousSpan,
    input_text: &str,
    hypergraph: &Hypergraph,
    candidate_id: ElementId,
) -> f32 {
    let Some(span_emb) = embed_span_in_context(input_text, span.char_start, span.char_end) else {
        return 0.0;
    };
    let cand_emb = &hypergraph.elements[candidate_id.0 as usize].embedding;
    if cand_emb.len() != span_emb.len() {
        return 0.0;
    }
    // Both are L2-normalized → cosine == dot. Clamp to [0, 1] so a
    // negative dot product (extremely rare for sentence embeddings)
    // doesn't drag the score below the threshold floor.
    dot(&span_emb, cand_emb).clamp(0.0, 1.0)
}

/// attribute_overlap_hint: in lieu of real syntactic-role detection,
/// boost candidates that have ANY recorded attribute slot. Rewards
/// elements that were focal in some role over elements that just
/// happen to be in recent_focus without an attribute binding.
pub(crate) fn attribute_overlap_hint(candidate: &Candidate) -> f32 {
    if candidate.attributes.iter().any(|a| a.is_some()) {
        0.5
    } else {
        0.0
    }
}

/// recency_bonus: linearly decays with tick distance. Same-tick
/// entries score 1.0; an entry from N ticks ago scores 1/(1+N).
/// Stays in [0, 1].
pub(crate) fn recency_bonus(candidate: &Candidate, now: Tick) -> f32 {
    let ticks_ago = now.0.saturating_sub(candidate.freshest_tick.0);
    1.0 / (1.0 + ticks_ago as f32)
}

/// Aggregate score per §11.8 (v0 terms only). Returns a non-negative
/// score; the caller compares against `policy.coref_threshold`.
pub(crate) fn score_candidate(
    span: &AmbiguousSpan,
    input_text: &str,
    hypergraph: &Hypergraph,
    candidate: &Candidate,
) -> f32 {
    name_overlap(span, hypergraph, candidate.element)
        + embedding_similarity(span, input_text, hypergraph, candidate.element)
        + attribute_overlap_hint(candidate)
        + recency_bonus(candidate, hypergraph.clock)
}

fn overlaps_ner(start: usize, end: usize, ner_spans: &[LabeledSpan]) -> bool {
    ner_spans
        .iter()
        .any(|s| !(end <= s.char_start || start >= s.char_end))
}

/// Resolves to at least one `Polarity::Signal` element via either
/// the exact-case name or its lowercased form? Used to gate
/// definite-description NP-head acceptance.
fn head_resolves_signal(hypergraph: &Hypergraph, head_tok: &str) -> bool {
    let any_signal = |ids: &[crate::types::ElementId]| {
        ids.iter().any(|id| {
            matches!(
                hypergraph.elements[id.0 as usize].polarity,
                crate::types::Polarity::Signal
            )
        })
    };
    if let Some(ids) = hypergraph.by_name.get(head_tok)
        && any_signal(ids)
    {
        return true;
    }
    let head_lower = head_tok.to_ascii_lowercase();
    if head_lower != head_tok
        && let Some(ids) = hypergraph.by_name.get(&head_lower)
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

    // ── Phase 5: end-to-end integration ─────────────────────────

    #[test]
    fn resolves_pronoun_to_subject_from_recent_focus() {
        // Set up: a candidate `Sarah` element in recent_focus with
        // a subject binding. A pronoun span "She" in the input
        // should resolve to Sarah and emit a CorefDecision.
        let mut hypergraph = load_seed_graph();
        let sarah_id = crate::tick_pipeline::build_relations::mint_element(
            &mut hypergraph,
            vec!["Sarah".to_string()],
            crate::embed::embed_text("Sarah"),
            crate::types::Polarity::Signal,
            1.0,
        );
        let subject = hypergraph.subject_attr;
        hypergraph.recent_focus.push_front(RecentFocusEntry {
            element: sarah_id,
            attribute: Some(subject),
            frame: None,
            tick: Tick(0),
        });
        let policy = Policy::default();

        let decisions = resolve_coref("She emailed me.", &hypergraph, &[], &policy);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].pronoun_text, "She");
        assert_eq!(decisions[0].antecedent_text, "Sarah");
        assert!(decisions[0].confidence > 0.0);
    }

    #[test]
    fn no_decision_when_no_compatible_candidates() {
        // recent_focus is empty → no candidates → no decision.
        let hypergraph = load_seed_graph();
        let policy = Policy::default();
        let decisions = resolve_coref("She emailed me.", &hypergraph, &[], &policy);
        assert!(decisions.is_empty());
    }

    #[test]
    fn threshold_gate_blocks_low_score_binds() {
        // With a recent_focus entry of an UNATTRIBUTED element and
        // a pronoun span, the score is just recency_bonus = 1.0 —
        // below default threshold (1.0 is not >= 1.0 since the
        // default is "must clear at least one substantive signal").
        // Bump threshold to 1.6 so even a 1.0 recency bonus + small
        // embedding bump won't clear, and verify nothing fires.
        let mut hypergraph = load_seed_graph();
        let user_id = hypergraph.by_name["user"][0];
        hypergraph.recent_focus.push_front(RecentFocusEntry {
            element: user_id,
            attribute: None, // no attribute → no hint bonus
            frame: None,
            tick: Tick(0),
        });
        let policy = Policy {
            coref_threshold: 2.5, // very high; pronouns can't clear
            ..Default::default()
        };
        let decisions = resolve_coref("She emailed me.", &hypergraph, &[], &policy);
        assert!(
            decisions.is_empty(),
            "high threshold should block low-score binds",
        );
    }

    #[test]
    fn definite_description_binds_when_head_matches() {
        // "the user" → user element via name_overlap = 1.0 (exact).
        let mut hypergraph = load_seed_graph();
        let user_id = hypergraph.by_name["user"][0];
        hypergraph.recent_focus.push_front(RecentFocusEntry {
            element: user_id,
            attribute: Some(hypergraph.subject_attr),
            frame: None,
            tick: Tick(0),
        });
        let policy = Policy::default();
        let decisions = resolve_coref("the user replied.", &hypergraph, &[], &policy);
        let dd: Vec<&CorefDecision> = decisions
            .iter()
            .filter(|d| d.pronoun_text.starts_with("the"))
            .collect();
        assert_eq!(dd.len(), 1);
        assert_eq!(dd[0].antecedent_text, "user");
        assert!(
            dd[0].confidence >= 0.7,
            "high-conf bind expected; got {}",
            dd[0].confidence
        );
    }

    #[test]
    fn returns_empty_with_no_recent_focus() {
        let hypergraph = Hypergraph::default();
        let decisions = resolve_coref("She left.", &hypergraph, &[], &Policy::default());
        assert!(decisions.is_empty());
    }

    #[test]
    fn detects_subject_pronoun() {
        let hypergraph = load_seed_graph();
        let spans = detect_ambiguous_spans("She left early.", &hypergraph, &[]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "She");
        assert_eq!(spans[0].kind, AmbiguousKind::Pronoun);
        assert_eq!(spans[0].char_start, 0);
        assert_eq!(spans[0].char_end, 3);
    }

    #[test]
    fn detects_multiple_pronouns() {
        let hypergraph = load_seed_graph();
        let spans = detect_ambiguous_spans("He gave her his book.", &hypergraph, &[]);
        let texts: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, vec!["He", "her", "his"]);
    }

    #[test]
    fn pronoun_match_is_case_insensitive() {
        let hypergraph = load_seed_graph();
        let spans = detect_ambiguous_spans("THEY arrived.", &hypergraph, &[]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "THEY");
        assert_eq!(spans[0].kind, AmbiguousKind::Pronoun);
    }

    #[test]
    fn detects_definite_description_for_seeded_head() {
        // `user` is seeded; "the user" should fire as a definite
        // description.
        let hypergraph = load_seed_graph();
        let spans = detect_ambiguous_spans("the user replied.", &hypergraph, &[]);
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
        let hypergraph = load_seed_graph();
        let spans = detect_ambiguous_spans("the qwlfkjsk arrived.", &hypergraph, &[]);
        let dd: Vec<&AmbiguousSpan> = spans
            .iter()
            .filter(|s| s.kind == AmbiguousKind::DefiniteDescription)
            .collect();
        assert!(dd.is_empty(), "DD shouldn't fire for unresolved head");
    }

    #[test]
    fn pronoun_detection_overrides_ner_label() {
        // NER zero-shot routinely mistags pronouns as `org` / `person`
        // with low confidence; the surface-form pronoun list is more
        // reliable. So even when an NER span covers the pronoun, the
        // pronoun should still be detected — `build_relations`'s
        // `apply_coref_decisions` will rebind it to the antecedent
        // if coref fires, and the NER proposal's `resolve_span` will
        // hit the rebound span cache. This is intentional: pronouns
        // are closed-class.
        let hypergraph = load_seed_graph();
        let ner = vec![LabeledSpan {
            char_start: 0,
            char_end: 3,
            label: "person".to_string(),
            text: "She".to_string(),
            score: 0.99,
        }];
        let spans = detect_ambiguous_spans("She left.", &hypergraph, &ner);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, "She");
        assert_eq!(spans[0].kind, AmbiguousKind::Pronoun);
    }

    #[test]
    fn detects_demonstrative_pronouns() {
        let hypergraph = load_seed_graph();
        let spans = detect_ambiguous_spans("That is mine.", &hypergraph, &[]);
        let texts: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        assert!(texts.contains(&"That"));
    }

    // ── Phase 2 tests: candidates + scoring ─────────────────────

    use crate::types::RecentFocusEntry;

    fn pronoun_span(text: &str, start: usize, end: usize) -> AmbiguousSpan {
        AmbiguousSpan {
            text: text.to_string(),
            char_start: start,
            char_end: end,
            kind: AmbiguousKind::Pronoun,
        }
    }

    fn def_desc_span(text: &str, start: usize, end: usize) -> AmbiguousSpan {
        AmbiguousSpan {
            text: text.to_string(),
            char_start: start,
            char_end: end,
            kind: AmbiguousKind::DefiniteDescription,
        }
    }

    #[test]
    fn collect_candidates_dedupes_by_element() {
        let mut hypergraph = load_seed_graph();
        let user_id = hypergraph.by_name["user"][0];
        // Push the same element twice — under subject AND target.
        hypergraph.recent_focus.push_front(RecentFocusEntry {
            element: user_id,
            attribute: Some(hypergraph.subject_attr),
            frame: None,
            tick: Tick(0),
        });
        hypergraph.recent_focus.push_front(RecentFocusEntry {
            element: user_id,
            attribute: Some(hypergraph.target_attr),
            frame: None,
            tick: Tick(1),
        });
        let cands = collect_candidates(&hypergraph);
        assert_eq!(cands.len(), 1, "duplicate element should collapse");
        let c = &cands[0];
        assert_eq!(c.element, user_id);
        assert_eq!(c.attributes.len(), 2);
        assert!(c.attributes.contains(&Some(hypergraph.subject_attr)));
        assert!(c.attributes.contains(&Some(hypergraph.target_attr)));
        assert_eq!(c.freshest_tick.0, 1, "freshest tick should win");
    }

    #[test]
    fn name_overlap_pronoun_returns_zero() {
        let hypergraph = load_seed_graph();
        let user_id = hypergraph.by_name["user"][0];
        let span = pronoun_span("She", 0, 3);
        assert_eq!(name_overlap(&span, &hypergraph, user_id), 0.0);
    }

    #[test]
    fn name_overlap_definite_description_exact_match() {
        let hypergraph = load_seed_graph();
        let user_id = hypergraph.by_name["user"][0];
        let span = def_desc_span("the user", 0, 8);
        assert_eq!(name_overlap(&span, &hypergraph, user_id), 1.0);
    }

    #[test]
    fn name_overlap_definite_description_substring() {
        // Pick an element whose name contains "user" but isn't exact.
        // (Synthetic case — `user` is a one-word seeded element so
        // we mint a custom element to test the substring branch.)
        let mut hypergraph = load_seed_graph();
        let elem = crate::types::Element {
            id: ElementId(hypergraph.elements.len() as u32),
            names: vec!["super_user".to_string()],
            stats: crate::types::MemoryStats::default(),
            created_at: Tick(0),
            embedding: vec![0.0; crate::embed::EMBEDDING_DIM],
            polarity: crate::types::Polarity::Signal,
        };
        let id = elem.id;
        hypergraph.elements.push(elem);
        hypergraph.by_name.insert("super_user".to_string(), vec![id]);

        let span = def_desc_span("the user", 0, 8);
        assert_eq!(name_overlap(&span, &hypergraph, id), 0.5);
    }

    #[test]
    fn recency_bonus_drops_with_tick_distance() {
        let mut hypergraph = load_seed_graph();
        let user_id = hypergraph.by_name["user"][0];
        hypergraph.clock = Tick(10);
        let cand = Candidate {
            element: user_id,
            attributes: vec![],
            freshest_tick: Tick(10),
        };
        assert_eq!(recency_bonus(&cand, hypergraph.clock), 1.0);
        let cand_old = Candidate {
            freshest_tick: Tick(0),
            ..cand.clone()
        };
        let bonus = recency_bonus(&cand_old, hypergraph.clock);
        assert!(bonus < 0.1, "10 ticks ago should score < 0.1; got {bonus}");
    }

    #[test]
    fn attribute_overlap_hint_rewards_attributed_focus() {
        let hypergraph = load_seed_graph();
        let user_id = hypergraph.by_name["user"][0];
        let attributed = Candidate {
            element: user_id,
            attributes: vec![Some(hypergraph.subject_attr)],
            freshest_tick: Tick(0),
        };
        let unattributed = Candidate {
            element: user_id,
            attributes: vec![None],
            freshest_tick: Tick(0),
        };
        assert_eq!(attribute_overlap_hint(&attributed), 0.5);
        assert_eq!(attribute_overlap_hint(&unattributed), 0.0);
    }

    #[test]
    fn score_candidate_combines_terms() {
        let mut hypergraph = load_seed_graph();
        let user_id = hypergraph.by_name["user"][0];
        hypergraph.clock = Tick(0);
        let span = def_desc_span("the user", 0, 8);
        let cand = Candidate {
            element: user_id,
            attributes: vec![Some(hypergraph.subject_attr)],
            freshest_tick: Tick(0),
        };
        let s = score_candidate(&span, "the user replied.", &hypergraph, &cand);
        // name_overlap = 1.0; embedding_sim ∈ [0, 1]; attr hint = 0.5;
        // recency = 1.0 (same-tick). Lower bound ≈ 2.5.
        assert!(s >= 2.5, "expected >= 2.5; got {s}");
        // Upper bound 3.5 (1 + 1 + 0.5 + 1).
        assert!(s <= 3.5, "expected <= 3.5; got {s}");
    }

    #[test]
    fn no_false_positive_for_bare_the() {
        let hypergraph = load_seed_graph();
        // "the." with no following word — skip.
        let spans = detect_ambiguous_spans("the.", &hypergraph, &[]);
        let dd: Vec<&AmbiguousSpan> = spans
            .iter()
            .filter(|s| s.kind == AmbiguousKind::DefiniteDescription)
            .collect();
        assert!(dd.is_empty());
    }
}
