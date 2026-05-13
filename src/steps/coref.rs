//! Heuristic coreference resolution for Step 5. Per §11.7 of the v0
//! doc, this is "pure Rust, recency-based — no model. Pronouns
//! (`he`/`she`/`it`/`they`/`this`/`that`) and definite descriptions
//! (`the dentist`) resolve to the most-recently-focused
//! `RecentFocusEntry` whose attribute name matches the span's
//! grammatical slot."
//!
//! Status: stub. The recency comparison relies on
//! `Hypergraph.recent_focus`, which gets populated by §11.11
//! (hebbian + salience). With nothing else writing into
//! `recent_focus` yet, this pass has no candidates to consider and
//! produces no decisions. Wired into Step 5's return shape so the
//! downstream API is stable; the resolution logic lands once the
//! working-memory phases of v0 are in place.

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

/// Resolve coreferences within `input_text` given the current
/// hypergraph state. Returns an empty vector while
/// `Hypergraph.recent_focus` is unpopulated.
pub fn resolve_coref(_input_text: &str, _hg: &Hypergraph) -> Vec<CorefDecision> {
    // Pronoun and definite-description detection is ready, but with
    // no candidates in recent_focus there's nothing to score against.
    // Returning empty keeps the Step 5 contract stable and lets Step 8
    // treat coref decisions as optional.
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_empty_with_no_recent_focus() {
        let hg = Hypergraph::default();
        let decisions = resolve_coref("She left.", &hg);
        assert!(decisions.is_empty());
    }
}
