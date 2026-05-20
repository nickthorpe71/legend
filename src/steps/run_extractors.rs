//! Step 5 of the tick pipeline — `run_extractors`. Orchestrates the
//! `relation_patterns/` extractors plus coref over the input text and
//! returns an [`ExtractionOutput`] for Step 8 (`build_relations`) to
//! convert into hypergraph relations.
//!
//! Every relation-shaped output is a
//! [`crate::steps::relation_patterns::RelationCandidate`]; the buckets
//! on `ExtractionOutput` exist to preserve Step 8's batch processing
//! order (instance_of first, then NER-anchored RE with n-ary merge,
//! then surface OpenIE).
//!
//! Pattern families called:
//!
//! 1. **GLiNER NER + temporal regex → span-typing** —
//!    [`crate::steps::relation_patterns::build_instance_of_proposals`].
//! 2. **NER-anchored RE templates** —
//!    [`crate::steps::relation_patterns::extract_relations`].
//! 3. **Surface OpenIE (SVO + appositive)** —
//!    [`crate::steps::relation_patterns::extract_surface_relations`].
//!
//! Coref runs alongside as a separate, non-relation output.

use crate::inference::deberta::predict::predict_entities;
use crate::steps::coref::{CorefDecision, resolve_coref};
use crate::steps::orthographic::{OrthographicChunk, extract_chunks, extract_proper_noun_runs};
use crate::steps::relation_patterns::{
    RelationCandidate, build_instance_of_proposals, extract_relations, extract_surface_relations,
};
use crate::steps::temporal::extract_temporal;
use crate::steps::void_filter::extract_content_tokens;
use crate::types::{Hypergraph, Policy, RegionActivation};

/// Everything Step 5 contributes to a tick. Every relation-shaped
/// proposal is a [`RelationCandidate`] regardless of source; the
/// `known` vs `novelty` split is preserved only because Step 8 batch-
/// preprocesses the NER-anchored proposals (event-merge) before
/// minting, and processes the buckets in a known-first-novelty-second
/// order.
#[derive(Debug, Default)]
pub struct ExtractionOutput {
    pub known: KnownExtractions,
    pub novelty: NoveltyExtractions,
}

/// Known-branch candidates: span-typing + NER-anchored RE templates.
/// All entries carry `ObjectRef::Label` (instance_of) or `ObjectRef::
/// Span` (the n-ary `from`/`to` pairs) and may carry an
/// `event_anchor` for n-ary merge.
#[derive(Debug, Default)]
pub struct KnownExtractions {
    /// `(span, instance_of, kind)` candidates from NER + temporal.
    /// Always carry `ObjectRef::Label`.
    pub instance_of: Vec<RelationCandidate>,
    /// `(subject, attr, object)` candidates from the NER-anchored RE
    /// templates. May carry `event_anchor` for n-ary merge; may carry
    /// `ObjectRef::Label("unknown_prior")` for the synthetic-from
    /// half of "moved to Y" / "is now Y" patterns.
    pub relations: Vec<RelationCandidate>,
    /// Pronoun / definite-description antecedent decisions.
    pub coref: Vec<CorefDecision>,
}

/// Surface chunks that survive without any label match, plus
/// candidate relations extracted purely from surface patterns. Always
/// populated for non-empty input (at minimum, one Phrase chunk).
#[derive(Debug, Default)]
pub struct NoveltyExtractions {
    /// Phrase + Token chunks from Step 5a.
    pub chunks: Vec<OrthographicChunk>,
    /// Surface-pattern (SVO + appositive) candidates. Always
    /// `Defeasible`; replay confirms or prunes.
    pub relations: Vec<RelationCandidate>,
}

/// Seed entity kinds used as the NER label set when the caller doesn't
/// supply a custom set. Mirrors §11.7's `(person, org, place, weekday,
/// quantity, event, ...)` list, extended with categories that
/// naturally-phrased prose mentions (months, years, languages, named
/// software) so the pattern matcher in `relation_patterns.rs` has
/// enough span coverage to fire on common change-event forms.
pub const SEED_KINDS: &[&str] = &[
    "person", "org", "place", "weekday", "month", "year", "quantity", "event", "role", "state",
    "time", "language", "software",
];

/// Run Step 5 on a single input.
///
/// - `labels` overrides the automatic label set when non-empty. The
///   automatic set is `SEED_KINDS ∪ names(active_regions)`.
/// - `hg` is read-only — used to resolve active-region names.
/// - `policy.ner_assertion_threshold` decides Entailed-vs-Defeasible
///   for each NER proposal.
pub fn run_extractors(
    input_text: &str,
    labels: &[&str],
    policy: &Policy,
    hg: &Hypergraph,
    active_regions: &[RegionActivation],
) -> ExtractionOutput {
    if input_text.trim().is_empty() {
        return ExtractionOutput::default();
    }

    let timing = std::env::var("LEGEND_TIME").is_ok();
    let mut mark = std::time::SystemTime::now();
    let mut stage = |label: &str| {
        if timing {
            let dt = mark.elapsed().unwrap_or_default();
            eprintln!(
                "[time]   step5/{label:<24} {:>6.1} ms",
                dt.as_secs_f64() * 1000.0
            );
            mark = std::time::SystemTime::now();
        }
    };

    // 0. Step 5a — orthographic chunker (Phrase) plus void-filtered
    //    content tokens (Token). The chunker is pure-Rust and model-
    //    free; the content-token pass consults the hypergraph's
    //    `by_name` index and drops any token whose lowercase form
    //    resolves to a `Polarity::Void` element (closed-class
    //    grammatical region — determiners, adpositions, etc.).
    let mut unconditional_chunks = extract_chunks(input_text);
    // Multi-word proper-noun runs ("Samsung Galaxy S22", "Best Buy",
    // "Dell XPS 13"). Without these, adjacent Title-Case tokens
    // would split into per-word elements that lose entity identity
    // — `Samsung`, `Galaxy`, `S22` as three unrelated singletons.
    unconditional_chunks.extend(extract_proper_noun_runs(input_text));
    unconditional_chunks.extend(extract_content_tokens(input_text, hg));
    unconditional_chunks.sort_by_key(|c| (c.char_start, c.char_end));
    stage("orthographic chunks");

    // 1. Build the NER label set. Caller-supplied labels override
    //    everything; otherwise SEED_KINDS plus a warm-bias set drawn
    //    from the names of the active regions.
    let owned_labels: Vec<String>;
    let label_set: Vec<&str> = if labels.is_empty() {
        owned_labels = build_label_set(hg, active_regions);
        owned_labels.iter().map(String::as_str).collect()
    } else {
        labels.to_vec()
    };
    stage("build_label_set");

    // 2. GLiNER2 NER pass.
    const RAW_THRESHOLD: f32 = 0.3;
    let ner_spans = predict_entities(input_text, &label_set, RAW_THRESHOLD);
    stage("ner predict_entities");

    // 3. Temporal pass — pure regex, complements NER.
    let temporal_spans = extract_temporal(input_text);
    stage("temporal");

    // 4. Span-typing: `(span, instance_of, kind)` from both NER and
    //    temporal. Overlap dedup handled inside the helper.
    let instance_of = build_instance_of_proposals(&ner_spans, &temporal_spans, policy);
    stage("span_typing");

    // 5. NER-anchored RE templates ("X from A to B", "X at Y", etc.).
    let relations: Vec<RelationCandidate> = extract_relations(input_text, &ner_spans);
    stage("relations");

    // 6. Coref. Reads `hg.recent_focus` (Step 10's output from
    //    prior ticks) plus this tick's NER spans to skip
    //    already-typed entities. Emits empty on the first tick
    //    of a session (no recent_focus yet).
    let coref: Vec<CorefDecision> = resolve_coref(input_text, hg, &ner_spans, policy);
    stage("coref");

    // 7. Surface-OpenIE: SVO + comma-appositive over the orthographic
    //    chunks. Always Defeasible. Step 8 merges with known-branch
    //    proposals.
    let novelty_relations: Vec<RelationCandidate> =
        extract_surface_relations(input_text, &unconditional_chunks);
    stage("novelty_relations");

    ExtractionOutput {
        known: KnownExtractions {
            instance_of,
            relations,
            coref,
        },
        novelty: NoveltyExtractions {
            chunks: unconditional_chunks,
            relations: novelty_relations,
        },
    }
}

/// Convenience wrapper for callers that only have text + policy and
/// don't yet have a routed hypergraph. Uses an empty active-region
/// list — equivalent to "no warm bias".
pub fn run_extractors_simple(input_text: &str, policy: &Policy) -> ExtractionOutput {
    let hg = Hypergraph::default();
    run_extractors(input_text, &[], policy, &hg, &[])
}

/// SEED_KINDS plus the (deduped) names of every element referenced by
/// an active region. Used as the GLiNER label set on the production
/// path. Most active-region IDs are themselves regions whose names
/// (`"time"`, `"events"`, `"locations"`, ...) double as useful NER
/// coarse-types — so we just pull `elements[region].names[0]` for each.
fn build_label_set(hg: &Hypergraph, active_regions: &[RegionActivation]) -> Vec<String> {
    let mut seen: std::collections::BTreeSet<String> =
        SEED_KINDS.iter().map(|s| s.to_string()).collect();
    for ra in active_regions {
        let id = ra.region.0 as usize;
        if let Some(el) = hg.elements.get(id)
            && let Some(name) = el.names.first()
        {
            seen.insert(name.clone());
        }
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::relation_patterns::{ObjectRef, PatternSource, RelationCandidate};

    fn default_policy() -> Policy {
        Policy {
            ner_assertion_threshold: 0.7,
            ..Default::default()
        }
    }

    fn empty_hg() -> Hypergraph {
        Hypergraph::default()
    }

    fn subject_slice<'a>(c: &RelationCandidate, text: &'a str) -> &'a str {
        &text[c.subject_char_start..c.subject_char_end]
    }

    fn object_label(c: &RelationCandidate) -> &str {
        match &c.object {
            ObjectRef::Label(l) => l.as_str(),
            ObjectRef::Span { .. } => panic!("expected ObjectRef::Label, got Span"),
        }
    }

    #[test]
    fn run_extractors_finds_dentist_entities() {
        let policy = default_policy();
        let hg = empty_hg();
        let text = "My dentist appointment with Dr. Rao changed from Tuesday to Friday.";
        let out = run_extractors(
            text,
            &["person", "event", "weekday", "role"],
            &policy,
            &hg,
            &[],
        );
        let ner: Vec<&RelationCandidate> = out
            .known
            .instance_of
            .iter()
            .filter(|p| p.source == PatternSource::NerSpanTyping)
            .collect();
        assert_eq!(ner.len(), 4);
        assert_eq!(subject_slice(ner[0], text), "My dentist appointment");
        assert_eq!(object_label(ner[0]), "event");
        assert_eq!(subject_slice(ner[1], text), "Dr. Rao");
        assert_eq!(object_label(ner[1]), "person");
        assert_eq!(object_label(ner[2]), "weekday");
        assert_eq!(object_label(ner[3]), "weekday");

        // Tuesday + Friday already covered by NER (label="weekday") so
        // the temporal pass shouldn't emit duplicates.
        let temp: Vec<&RelationCandidate> = out
            .known
            .instance_of
            .iter()
            .filter(|p| p.source == PatternSource::TemporalSpanTyping)
            .collect();
        assert!(
            temp.is_empty(),
            "expected no temporal duplicates; got {:?}",
            temp.iter()
                .map(|p| subject_slice(p, text))
                .collect::<Vec<_>>()
        );

        // Pattern RE: "appointment ... with Dr. Rao" + "changed from
        // Tuesday to Friday" → at least the from/to pair plus the
        // companion 'with' link should fire.
        let attrs: Vec<&str> = out
            .known
            .relations
            .iter()
            .map(|r| r.attribute_name.as_str())
            .collect();
        assert!(attrs.contains(&"from"), "missing 'from' rel: {attrs:?}");
        assert!(attrs.contains(&"to"), "missing 'to' rel: {attrs:?}");
        assert!(attrs.contains(&"with"), "missing 'with' rel: {attrs:?}");
    }

    #[test]
    fn temporal_pass_emits_weekday_when_ner_misses_it() {
        // Don't include "weekday" in the NER labels — temporal pass
        // should still pick up Tuesday + Friday on its own.
        let policy = default_policy();
        let hg = empty_hg();
        let text = "We met on Tuesday and again on Friday.";
        let out = run_extractors(text, &["person", "event"], &policy, &hg, &[]);
        let temp_texts: Vec<&str> = out
            .known
            .instance_of
            .iter()
            .filter(|p| p.source == PatternSource::TemporalSpanTyping)
            .map(|p| subject_slice(p, text))
            .collect();
        assert_eq!(temp_texts, vec!["Tuesday", "Friday"]);
    }

    #[test]
    fn empty_input_returns_no_proposals() {
        let out = run_extractors_simple("", &default_policy());
        assert!(out.known.instance_of.is_empty());
        assert!(out.known.relations.is_empty());
        assert!(out.known.coref.is_empty());
        assert!(out.novelty.chunks.is_empty());
        assert!(out.novelty.relations.is_empty());
    }
}
