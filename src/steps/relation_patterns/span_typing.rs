//! Span-typing pattern: `(span, instance_of, kind)`.
//!
//! Two sources, same shape:
//!
//! 1. **GLiNER NER** — `LabeledSpan { text, char_start, char_end, label,
//!    score }` from the multi-label entity predictor. Label is one of
//!    the seed kinds ∪ active-region names. Status = `Entailed` if
//!    score ≥ `policy.ner_assertion_threshold`, else `Defeasible`.
//!
//! 2. **Temporal regex** — `TemporalSpan { text, char_start, char_end,
//!    kind }` from the pure-Rust date/weekday/month/time pass. Kind
//!    is `"weekday"`, `"month"`, or `"time"`. Confidence is fixed at
//!    0.95 (regex hits are high-precision); status uses the same
//!    threshold for uniformity.
//!
//! Temporal spans that overlap any NER span are dropped — NER goes
//! first.

use crate::inference::deberta::predict::LabeledSpan;
use crate::steps::temporal::TemporalSpan;
use crate::types::{Policy, RelationStatus};

/// One span-typing proposal. Maps to a single `(span, instance_of, K)`
/// hypergraph relation.
#[derive(Debug, Clone)]
pub struct ExtractionProposal {
    pub subject_text: String,
    pub subject_char_start: usize,
    pub subject_char_end: usize,
    pub attribute_name: &'static str,
    pub object_label: String,
    pub confidence: f32,
    pub status: RelationStatus,
    pub provenance: ProposalSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalSource {
    Ner,
    Temporal,
    Pattern,
}

/// Build span-typing proposals from NER spans + temporal regex spans.
/// Temporal spans that overlap any NER span are dropped.
pub fn build_instance_of_proposals(
    ner_spans: &[LabeledSpan],
    temporal_spans: &[TemporalSpan],
    policy: &Policy,
) -> Vec<ExtractionProposal> {
    let mut out: Vec<ExtractionProposal> = Vec::new();

    for s in ner_spans {
        let status = if s.score >= policy.ner_assertion_threshold {
            RelationStatus::Entailed
        } else {
            RelationStatus::Defeasible
        };
        out.push(ExtractionProposal {
            subject_text: s.text.clone(),
            subject_char_start: s.char_start,
            subject_char_end: s.char_end,
            attribute_name: "instance_of",
            object_label: s.label.clone(),
            confidence: s.score,
            status,
            provenance: ProposalSource::Ner,
        });
    }

    for t in temporal_spans {
        if overlaps_any(t.char_start, t.char_end, ner_spans) {
            continue;
        }
        let conf: f32 = 0.95;
        let status = if conf >= policy.ner_assertion_threshold {
            RelationStatus::Entailed
        } else {
            RelationStatus::Defeasible
        };
        out.push(ExtractionProposal {
            subject_text: t.text.clone(),
            subject_char_start: t.char_start,
            subject_char_end: t.char_end,
            attribute_name: "instance_of",
            object_label: t.kind.to_string(),
            confidence: conf,
            status,
            provenance: ProposalSource::Temporal,
        });
    }

    out.sort_by_key(|p| (p.subject_char_start, p.subject_char_end));
    out
}

fn overlaps_any(start: usize, end: usize, spans: &[LabeledSpan]) -> bool {
    spans
        .iter()
        .any(|s| !(end <= s.char_start || start >= s.char_end))
}
