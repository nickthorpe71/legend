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
use crate::steps::relation_patterns::{ObjectRef, PatternSource, RelationCandidate};
use crate::steps::temporal::TemporalSpan;
use crate::types::{Policy, RelationStatus};

/// Build span-typing proposals from NER spans + temporal regex spans.
/// Temporal spans that overlap any NER span are dropped.
pub fn build_instance_of_proposals(
    ner_spans: &[LabeledSpan],
    temporal_spans: &[TemporalSpan],
    policy: &Policy,
) -> Vec<RelationCandidate> {
    let mut out: Vec<RelationCandidate> = Vec::new();

    for s in ner_spans {
        let status = if s.score >= policy.ner_assertion_threshold {
            RelationStatus::Entailed
        } else {
            RelationStatus::Defeasible
        };
        out.push(RelationCandidate {
            source: PatternSource::NerSpanTyping,
            subject_char_start: s.char_start,
            subject_char_end: s.char_end,
            attribute_name: "instance_of".to_string(),
            attribute_char_start: None,
            attribute_char_end: None,
            object: ObjectRef::Label(s.label.clone()),
            confidence: s.score,
            status,
            event_anchor: None,
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
        out.push(RelationCandidate {
            source: PatternSource::TemporalSpanTyping,
            subject_char_start: t.char_start,
            subject_char_end: t.char_end,
            attribute_name: "instance_of".to_string(),
            attribute_char_start: None,
            attribute_char_end: None,
            object: ObjectRef::Label(t.kind.to_string()),
            confidence: conf,
            status,
            event_anchor: None,
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
