//! Pattern fast-path for zero-shot relation extraction (§15.1 / §24.1
//! of the v0 doc). Operates on `LabeledSpan` output from Step 5's NER
//! pass: for each pair of nearby spans, checks for canonical
//! surface-form templates ("X from Y to Z", "X at Y", etc.) and emits
//! a [`RelationCandidate`] per match.
//!
//! These templates are deliberately a small bootstrap set covering
//! the seed-pack frames (frame, valid_from / valid_to, location).
//! Real coverage grows through replay and warm-region labels; this
//! module exists so v0 ships with a working RE path while we defer
//! the GLiNER multi-task model.
//!
//! Some templates emit a synthetic `from` half pointing at the
//! `unknown_prior` label (encoded as [`ObjectRef::Label`]) — see
//! Template 5 and 6 for "moved to Y" / "is now Y" handling.

use crate::inference::deberta::predict::LabeledSpan;
use crate::tick_pipeline::relation_patterns::{ObjectRef, PatternSource, RelationCandidate};
use crate::types::RelationStatus;

/// Build a NER-anchored candidate. Helper to keep the eight emit
/// sites below readable.
fn make(
    subj: (usize, usize),
    attr: &'static str,
    object: ObjectRef,
    conf: f32,
    status: RelationStatus,
    event_anchor: Option<String>,
) -> RelationCandidate {
    RelationCandidate {
        source: PatternSource::NerAnchored,
        subject_char_start: subj.0,
        subject_char_end: subj.1,
        attribute_name: attr.to_string(),
        // Canonical seed-name predicate; resolved via exact-match in
        // Step 8, so no contextualization needed.
        attribute_char_start: None,
        attribute_char_end: None,
        object,
        confidence: conf,
        status,
        event_anchor,
    }
}

/// Try every supported template against the available spans.
/// `text` is the raw input (the templates inspect the
/// inter-span connective words). Spans must be ordered by
/// `char_start` ascending.
pub fn extract_relations(text: &str, spans: &[LabeledSpan]) -> Vec<RelationCandidate> {
    let mut spans = spans.to_vec();
    spans.sort_by_key(|s| s.char_start);

    let mut out: Vec<RelationCandidate> = Vec::new();

    // Template 1: "X from A to B" — three-way binding of subject /
    // valid_from / valid_to. Iterates (from-value, to-value) pairs
    // and looks BACKWARD for the subject + verb anchor. This is the
    // important shape: when the input is "I switched Polaris from
    // Rust to Go" the event target is Polaris, not "I" — picking
    // the latest NER span before " from " (rather than an earlier
    // agent token) is what makes that work. For the simpler form
    // "My appointment changed from Tuesday to Friday" the same
    // backward walk lands on the appointment.
    let mut handled_to_pairs: std::collections::HashSet<(usize, usize)> =
        std::collections::HashSet::new();
    for j in 0..spans.len() {
        for k in (j + 1)..spans.len() {
            let between_jk = &text[spans[j].char_end..spans[k].char_start];
            let lower_jk = between_jk.to_ascii_lowercase();
            if !lower_jk.contains(" to ") || lower_jk.len() >= 16 {
                continue;
            }
            // " from " must appear in the text PRECEDING span_j.
            // Use the last occurrence (closest to span_j) so chained
            // "from … from … to …" doesn't bind to a stale "from".
            let pre = text[..spans[j].char_start].to_ascii_lowercase();
            let Some(from_byte_pos) = pre.rfind(" from ") else {
                continue;
            };
            // Reject if " from " ends inside span_j (means span_j is
            // wholly inside the connective, not after it).
            if from_byte_pos + 6 > spans[j].char_start {
                continue;
            }
            // Subject = latest NER span ending at or before " from ".
            let subject_idx = spans
                .iter()
                .enumerate()
                .filter(|(idx, s)| *idx < j && s.char_end <= from_byte_pos)
                .max_by_key(|(_, s)| s.char_end)
                .map(|(idx, _)| idx);
            let Some(subj_idx) = subject_idx else {
                continue;
            };
            // Connective-length cap mirrors the prior T1 implementation:
            // anchor + " from " < 32 chars to keep the binding local.
            let pre_anchor_len = from_byte_pos - spans[subj_idx].char_end;
            if pre_anchor_len >= 26 {
                continue;
            }
            // Verb anchor: prefer text between subject and " from "
            // (the "subject verb from-to" shape). If empty, fall back
            // to text BEFORE subject (the "agent verb subject from-to"
            // shape, where the verb sits before its patient).
            let anchor_after = text[spans[subj_idx].char_end..from_byte_pos].trim();
            let anchor: Option<String> = if !anchor_after.is_empty() {
                Some(anchor_after.to_string())
            } else if subj_idx > 0 {
                let anchor_before_subj =
                    text[spans[subj_idx - 1].char_end..spans[subj_idx].char_start].trim();
                if !anchor_before_subj.is_empty() {
                    Some(anchor_before_subj.to_string())
                } else {
                    None
                }
            } else {
                None
            };
            // Dedup: a single (from-value, to-value) pair should only
            // emit one event, even if the input has structures that
            // would let multiple subjects bind.
            if !handled_to_pairs.insert((j, k)) {
                continue;
            }
            let conf = 0.85;
            let status = if conf >= 0.6 {
                RelationStatus::Entailed
            } else {
                RelationStatus::Defeasible
            };
            out.push(make(
                (spans[subj_idx].char_start, spans[subj_idx].char_end),
                "from",
                ObjectRef::Span {
                    char_start: spans[j].char_start,
                    char_end: spans[j].char_end,
                },
                conf,
                status,
                anchor.clone(),
            ));
            out.push(make(
                (spans[subj_idx].char_start, spans[subj_idx].char_end),
                "to",
                ObjectRef::Span {
                    char_start: spans[k].char_start,
                    char_end: spans[k].char_end,
                },
                conf,
                status,
                anchor,
            ));
        }
    }

    // Templates 2–4: binary connectives — "X with Y", "X at Y",
    // "X's Y". One emit per match, mapped to the corresponding
    // seed attribute name.
    let templates: &[(&[&str], &'static str)] = &[
        (&[" with ", " w/ "], "with"),
        (&[" at ", " in "], "at"),
        (&["'s ", "’s "], "property"),
    ];
    for i in 0..spans.len() {
        for j in (i + 1)..spans.len() {
            let between = &text[spans[i].char_end..spans[j].char_start];
            if between.is_empty() || between.len() > 24 {
                continue;
            }
            let lower = between.to_ascii_lowercase();
            for &(needles, attr) in templates {
                if needles.iter().any(|n| lower.contains(n)) {
                    let conf = 0.6_f32.min(spans[i].score.min(spans[j].score));
                    let status = if conf >= 0.55 {
                        RelationStatus::Entailed
                    } else {
                        RelationStatus::Defeasible
                    };
                    out.push(make(
                        (spans[i].char_start, spans[i].char_end),
                        attr,
                        ObjectRef::Span {
                            char_start: spans[j].char_start,
                            char_end: spans[j].char_end,
                        },
                        conf,
                        status,
                        None,
                    ));
                    break; // one template per pair max
                }
            }
        }
    }

    // Template 6: restatement adverbial — "X is now Y", "X are now Y",
    // or "X now <verb> Y / in Y / at Y" — supersedes by restating the
    // current value without an explicit change verb. The anchor is
    // always `"now"` (canonical) so multiple restatements about the
    // same subject merge into one event identity per tick.
    for i in 0..spans.len() {
        for j in (i + 1)..spans.len() {
            let between = &text[spans[i].char_end..spans[j].char_start];
            if between.is_empty() || between.len() > 40 {
                continue;
            }
            let lower = between.to_ascii_lowercase();
            // Skip if we already matched a change-verb-to template here
            // (those carry " to " and we don't want both firing).
            if lower.contains(" from ") || lower.contains(" to ") {
                continue;
            }
            // Must contain a "now" marker bounded by spaces (so we don't
            // false-positive on "snowing" etc.).
            let has_now = lower.contains(" is now ")
                || lower.contains(" are now ")
                || lower.contains(" now ");
            if !has_now {
                continue;
            }
            // Same rationale as T1: the surface pattern is the signal.
            let conf = 0.85;
            let status = if conf >= 0.55 {
                RelationStatus::Entailed
            } else {
                RelationStatus::Defeasible
            };
            out.push(make(
                (spans[i].char_start, spans[i].char_end),
                "from",
                ObjectRef::Label("unknown_prior".to_string()),
                conf,
                status,
                Some("now".to_string()),
            ));
            out.push(make(
                (spans[i].char_start, spans[i].char_end),
                "to",
                ObjectRef::Span {
                    char_start: spans[j].char_start,
                    char_end: spans[j].char_end,
                },
                conf,
                status,
                Some("now".to_string()),
            ));
        }
    }

    // Template 5: change-verb singletons — "X <verb> to Y" where
    // <verb> is a state-change verb. Only `to` is overt; the implicit
    // `from` becomes a synthetic `unknown_prior` proposal sharing the
    // same anchor so Step 8's n-ary merge picks them up as an event.
    // Without this, naturally-phrased property changes ("Bob moved
    // to Austin") never reach Step 9's supersession gate.
    for i in 0..spans.len() {
        for j in (i + 1)..spans.len() {
            let between = &text[spans[i].char_end..spans[j].char_start];
            if between.is_empty() || between.len() > 40 {
                continue;
            }
            let lower = between.to_ascii_lowercase();
            // `" to "` must be present and not be part of a `" from … to …"`
            // span (those are handled by Template 1).
            let Some(to_idx) = lower.find(" to ") else {
                continue;
            };
            if lower[..to_idx].contains(" from ") {
                continue;
            }
            let prefix = between[..to_idx].trim();
            let Some(anchor) = match_change_verb(prefix) else {
                continue;
            };
            // Same rationale as T1: the surface pattern is the signal.
            let conf = 0.85;
            let status = if conf >= 0.6 {
                RelationStatus::Entailed
            } else {
                RelationStatus::Defeasible
            };
            // Synthetic `from`: points at the canonical `unknown_prior`
            // label. Carries identical anchor + subject_span so the
            // merge groups it with the overt `to` below.
            out.push(make(
                (spans[i].char_start, spans[i].char_end),
                "from",
                ObjectRef::Label("unknown_prior".to_string()),
                conf,
                status,
                Some(anchor.clone()),
            ));
            out.push(make(
                (spans[i].char_start, spans[i].char_end),
                "to",
                ObjectRef::Span {
                    char_start: spans[j].char_start,
                    char_end: spans[j].char_end,
                },
                conf,
                status,
                Some(anchor),
            ));
        }
    }

    out
}

/// Recognize a known state-change verb phrase. Returns the canonical
/// anchor (lowercased, normalized auxiliaries trimmed) when matched.
/// Matches are conservative — pure surface forms, no morphology.
fn match_change_verb(prefix: &str) -> Option<String> {
    let p = prefix.to_ascii_lowercase();
    // Auxiliaries that combine with a participle: "was promoted",
    // "is now", "has been promoted". Strip them so the anchor reflects
    // the change verb itself.
    let stripped = strip_aux(&p);
    const CHANGE_VERBS: &[&str] = &[
        "moved",
        "moves",
        "move",
        "transferred",
        "transferring",
        "switched",
        "switch",
        "transitioned",
        "transition",
        "changed",
        "change",
        "became",
        "becomes",
        "become",
        "promoted",
        "demoted",
        "increased",
        "increase",
        "decreased",
        "decrease",
        "rose",
        "fell",
        "dropped",
        "is now",
        "are now",
        "now",
        "renamed",
        "rebranded",
        "shifted",
        "upgraded",
        "downgraded",
    ];
    // Try exact match first, then prefix-match (verb + space + tail).
    // Prefix-match catches naturally-phrased changes like "changed the
    // language we're using" where the verb leads but the prefix isn't
    // just the verb alone. Restricting to "verb " (with trailing space)
    // keeps it from false-positiving on compounds — e.g. "changedover"
    // would not match "changed".
    for &verb in CHANGE_VERBS {
        if stripped == verb
            || p == verb
            || stripped.starts_with(&format!("{verb} "))
            || p.starts_with(&format!("{verb} "))
        {
            return Some(verb.to_string());
        }
    }
    None
}

/// Strip a leading auxiliary cluster ("was", "is", "are", "has been",
/// "have been", "had been", "got"). Returns the rest, lowercased and
/// trimmed. Used so "was promoted" anchors to "promoted".
fn strip_aux(p: &str) -> String {
    const AUX: &[&str] = &[
        "has been ",
        "have been ",
        "had been ",
        "was ",
        "were ",
        "is ",
        "are ",
        "got ",
    ];
    for &aux in AUX {
        if let Some(rest) = p.strip_prefix(aux) {
            return rest.trim().to_string();
        }
    }
    p.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(s: usize, e: usize, label: &str, score: f32, text: &str) -> LabeledSpan {
        LabeledSpan {
            char_start: s,
            char_end: e,
            label: label.to_string(),
            text: text.to_string(),
            score,
        }
    }

    #[test]
    fn from_to_pattern_emits_two_relations() {
        // "appointment changed from Tuesday to Friday"
        //  0                       24      32 35
        // event span (0..21), weekday Tuesday (29..36), weekday Friday (40..46)
        let text = "My appointment changed from Tuesday to Friday.";
        let spans = vec![
            span(0, 14, "event", 0.8, "My appointment"),
            span(28, 35, "weekday", 0.95, "Tuesday"),
            span(39, 45, "weekday", 0.95, "Friday"),
        ];
        let rels = extract_relations(text, &spans);
        assert_eq!(rels.len(), 2);
        assert_eq!(rels[0].attribute_name, "from");
        assert_eq!(rels[1].attribute_name, "to");
        assert_eq!(rels[0].event_anchor.as_deref(), Some("changed"));
        assert_eq!(rels[1].event_anchor.as_deref(), Some("changed"));
    }

    #[test]
    fn from_to_with_no_verb_yields_no_anchor() {
        // "Flight from JFK to LAX" — no verb between subject and "from".
        let text = "Flight from JFK to LAX.";
        let spans = vec![
            span(0, 6, "event", 0.8, "Flight"),
            span(12, 15, "place", 0.95, "JFK"),
            span(19, 22, "place", 0.95, "LAX"),
        ];
        let rels = extract_relations(text, &spans);
        assert_eq!(rels.len(), 2);
        for r in &rels {
            assert!(
                r.event_anchor.is_none(),
                "no verb-anchor expected; got {:?}",
                r.event_anchor,
            );
        }
    }

    #[test]
    fn with_pattern_emits_one_relation() {
        let text = "Alice met with Bob.";
        let spans = vec![
            span(0, 5, "person", 0.9, "Alice"),
            span(15, 18, "person", 0.9, "Bob"),
        ];
        let rels = extract_relations(text, &spans);
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].attribute_name, "with");
        assert!(rels[0].event_anchor.is_none());
    }

    #[test]
    fn change_verb_singleton_emits_synthetic_from_plus_to() {
        // "Bob moved to Austin." — only `to` is overt; the implicit
        // `from` must come from the synthetic placeholder.
        let text = "Bob moved to Austin.";
        let spans = vec![
            span(0, 3, "person", 0.9, "Bob"),
            span(13, 19, "place", 0.9, "Austin"),
        ];
        let rels = extract_relations(text, &spans);
        let from = rels
            .iter()
            .find(|r| r.attribute_name == "from")
            .expect("synthetic from proposal missing");
        let to = rels
            .iter()
            .find(|r| r.attribute_name == "to")
            .expect("to proposal missing");
        assert_eq!(from.event_anchor.as_deref(), Some("moved"));
        assert_eq!(to.event_anchor.as_deref(), Some("moved"));
        assert!(
            matches!(&from.object, ObjectRef::Label(s) if s == "unknown_prior"),
            "synthetic from should carry the unknown_prior label, got {:?}",
            from.object
        );
        assert!(
            matches!(&to.object, ObjectRef::Span { .. }),
            "to should carry a span, got {:?}",
            to.object
        );
    }

    #[test]
    fn change_verb_with_aux_strips_to_verb() {
        // "Bob was promoted to senior engineer." — aux ("was") gets
        // stripped so the anchor is "promoted".
        let text = "Bob was promoted to senior engineer.";
        let spans = vec![
            span(0, 3, "person", 0.9, "Bob"),
            span(20, 35, "role", 0.85, "senior engineer"),
        ];
        let rels = extract_relations(text, &spans);
        let to = rels
            .iter()
            .find(|r| r.attribute_name == "to")
            .expect("to proposal missing");
        assert_eq!(to.event_anchor.as_deref(), Some("promoted"));
    }

    #[test]
    fn restatement_now_pattern_fires() {
        // "Bob is now a staff engineer." — no " to "; the "now"
        // adverbial should still surface an event-shaped proposal.
        let text = "Bob is now a staff engineer.";
        let spans = vec![
            span(0, 3, "person", 0.9, "Bob"),
            span(13, 27, "role", 0.85, "a staff engineer"),
        ];
        let rels = extract_relations(text, &spans);
        let from = rels
            .iter()
            .find(|r| r.attribute_name == "from")
            .expect("synthetic from proposal missing");
        let to = rels
            .iter()
            .find(|r| r.attribute_name == "to")
            .expect("to proposal missing");
        assert_eq!(from.event_anchor.as_deref(), Some("now"));
        assert_eq!(to.event_anchor.as_deref(), Some("now"));
        assert!(
            matches!(&from.object, ObjectRef::Label(s) if s == "unknown_prior"),
            "synthetic from should carry the unknown_prior label"
        );
    }

    #[test]
    fn from_to_picks_patient_when_agent_precedes_verb() {
        // "I switched Polaris from Rust to Go." — agent ("I") is the
        // grammatical subject, but the EVENT target is Polaris (the
        // patient — whose state changed). The from-to pattern should
        // land on Polaris with anchor "switched", not on "I" with
        // anchor "switched Polaris" (the previous bug).
        let text = "I switched Polaris from Rust to Go.";
        let spans = vec![
            span(0, 1, "person", 0.85, "I"),
            span(11, 18, "software", 0.9, "Polaris"),
            span(24, 28, "language", 0.85, "Rust"),
            span(32, 34, "language", 0.85, "Go"),
        ];
        let rels = extract_relations(text, &spans);
        let from = rels
            .iter()
            .find(|r| r.attribute_name == "from")
            .expect("from proposal missing");
        let to = rels
            .iter()
            .find(|r| r.attribute_name == "to")
            .expect("to proposal missing");
        assert_eq!(
            from.subject_char_start, 11,
            "subject must be Polaris, not I"
        );
        assert_eq!(from.subject_char_end, 18);
        assert_eq!(to.subject_char_start, 11);
        assert_eq!(to.subject_char_end, 18);
        assert_eq!(from.event_anchor.as_deref(), Some("switched"));
        assert_eq!(to.event_anchor.as_deref(), Some("switched"));
    }

    #[test]
    fn from_to_template_takes_precedence_over_change_verb() {
        // "X changed from A to B" must still fire the 3-span template
        // (overt from), not the singleton (which would add a bogus
        // synthetic from).
        let text = "My appointment changed from Tuesday to Friday.";
        let spans = vec![
            span(0, 14, "event", 0.8, "My appointment"),
            span(28, 35, "weekday", 0.95, "Tuesday"),
            span(39, 45, "weekday", 0.95, "Friday"),
        ];
        let rels = extract_relations(text, &spans);
        // Exactly two: from(Tuesday) + to(Friday). No synthetic from
        // (which would carry an ObjectRef::Label, not a Span).
        assert_eq!(rels.len(), 2);
        for r in &rels {
            assert!(
                matches!(r.object, ObjectRef::Span { .. }),
                "expected Span object for all triples; got {:?}",
                r.object
            );
        }
    }

    #[test]
    fn change_verb_prefix_match_fires_on_long_prefix() {
        // "we changed the language to be C" — the prefix before
        // " to " is "changed the language" which starts with
        // "changed ". Should fire Template 5 with anchor "changed".
        let text = "we changed the language to be C.";
        let spans = vec![
            span(0, 2, "person", 0.85, "we"),
            span(30, 31, "language", 0.85, "C"),
        ];
        let rels = extract_relations(text, &spans);
        let from = rels
            .iter()
            .find(|r| r.attribute_name == "from")
            .expect("change-verb prefix-match should synthesize a from");
        let to = rels
            .iter()
            .find(|r| r.attribute_name == "to")
            .expect("change-verb prefix-match should emit a to");
        assert_eq!(from.event_anchor.as_deref(), Some("changed"));
        assert_eq!(to.event_anchor.as_deref(), Some("changed"));
    }

    #[test]
    fn change_verb_prefix_match_rejects_non_change_prefix() {
        // "I went to the store" — `went` is not in the change-verb
        // list, so even though the prefix starts with a verb, the
        // template should not fire.
        let p = "went to the store";
        assert!(super::match_change_verb(p).is_none());
    }
}
