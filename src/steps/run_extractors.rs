//! Step 5 of the tick pipeline — `run_extractors`. Currently NER-only:
//! GLiNER2 zero-shot span tagging via the bundled INT8 forward pass.
//! Each tagged span becomes one `ExtractionProposal` for Step 7+
//! (`build_relations`) to turn into hypergraph relations.
//!
//! Out of scope for now (deferred to follow-up phases):
//! - Zero-shot relation extraction (would need GLiNER multi-task model
//!   or a pattern fast-path; the doc allows either).
//! - Temporal parser (chrono + chrono-english).
//! - Heuristic coreference.
//! - Warm-bias label set from active regions (Step 4 output) — for now
//!   we use a hardcoded seed-kind list.

use crate::inference::deberta::predict::{predict_entities, LabeledSpan};
use crate::types::{Policy, RelationStatus};

/// One proposal from an extractor. Step 8 (`build_relations`) maps
/// this into the hypergraph as a `(span, instance_of, K)` relation
/// whose `status` is set by the assertion-threshold gate below.
#[derive(Debug, Clone)]
pub struct ExtractionProposal {
    /// Surface text of the tagged span.
    pub subject_text: String,
    /// Inclusive char offset.
    pub subject_char_start: usize,
    /// Exclusive char offset.
    pub subject_char_end: usize,
    /// Always `"instance_of"` while we're NER-only.
    pub attribute_name: &'static str,
    /// The kind tag — `"person"`, `"event"`, etc. — drawn from
    /// `labels` (or the seed defaults if no labels provided).
    pub object_label: String,
    /// Raw sigmoid score from GLiNER2.
    pub confidence: f32,
    /// `Entailed` once confidence ≥ `policy.ner_assertion_threshold`;
    /// `Defeasible` below that. Mirrors §11.7 of the v0 doc.
    pub status: RelationStatus,
}

/// Seed entity kinds used as the NER label set when the caller doesn't
/// supply a custom set. Mirrors §11.7's `(person, org, place, weekday,
/// quantity, event, ...)` list. Once Step 4 surfaces warm labels we
/// extend this with active-region attribute names per tick.
pub const SEED_KINDS: &[&str] = &[
    "person",
    "org",
    "place",
    "weekday",
    "quantity",
    "event",
    "role",
    "state",
    "time",
];

/// Run Step 5 on a single input. Returns the proposals to feed into
/// Step 8 (`build_relations`).
///
/// `labels` overrides the seed-kind label set if non-empty (used by
/// tests + future warm-bias wiring). `policy` supplies the
/// assertion-threshold gate.
pub fn run_extractors(input_text: &str, labels: &[&str], policy: &Policy) -> Vec<ExtractionProposal> {
    if input_text.trim().is_empty() {
        return Vec::new();
    }

    let label_set: Vec<&str> = if labels.is_empty() {
        SEED_KINDS.to_vec()
    } else {
        labels.to_vec()
    };

    // GLiNER returns sigmoid-style scores; use a permissive base
    // threshold and let the assertion-threshold gate below decide
    // Asserted-vs-Defeasible promotion. Anything below the base
    // threshold is dropped entirely.
    const RAW_THRESHOLD: f32 = 0.3;

    let spans: Vec<LabeledSpan> = predict_entities(input_text, &label_set, RAW_THRESHOLD);

    spans
        .into_iter()
        .map(|s| {
            let status = if s.score >= policy.ner_assertion_threshold {
                RelationStatus::Entailed
            } else {
                RelationStatus::Defeasible
            };
            ExtractionProposal {
                subject_text: s.text,
                subject_char_start: s.char_start,
                subject_char_end: s.char_end,
                attribute_name: "instance_of",
                object_label: s.label,
                confidence: s.score,
                status,
            }
        })
        .collect()
}

#[cfg(all(test, feature = "gliner2_fp32"))]
mod tests {
    use super::*;
    use crate::types::Policy;

    fn default_policy() -> Policy {
        Policy {
            ner_assertion_threshold: 0.7,
            ..Default::default()
        }
    }

    #[test]
    fn run_extractors_finds_dentist_entities() {
        let policy = default_policy();
        let proposals = run_extractors(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
            &policy,
        );
        assert_eq!(proposals.len(), 4);
        assert_eq!(proposals[0].subject_text, "My dentist appointment");
        assert_eq!(proposals[0].object_label, "event");
        assert_eq!(proposals[1].subject_text, "Dr. Rao");
        assert_eq!(proposals[1].object_label, "person");
        assert_eq!(proposals[2].subject_text, "Tuesday");
        assert_eq!(proposals[3].subject_text, "Friday");

        // Threshold 0.7: low-conf entity becomes Defeasible.
        // (`event` at ~0.33 falls below; `person`/`weekday` at ~0.9+
        // come back Entailed.)
        assert_eq!(proposals[0].status, RelationStatus::Defeasible);
        assert_eq!(proposals[1].status, RelationStatus::Entailed);
        assert_eq!(proposals[2].status, RelationStatus::Entailed);
        assert_eq!(proposals[3].status, RelationStatus::Entailed);
    }

    #[test]
    fn empty_input_returns_no_proposals() {
        let policy = default_policy();
        let proposals = run_extractors("", &["person"], &policy);
        assert!(proposals.is_empty());
    }
}
