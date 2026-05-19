//! Pattern fast-path for zero-shot relation extraction (§15.1 / §24.1
//! of the v0 doc). Operates on `LabeledSpan` output from Step 5's NER
//! pass: for each pair of nearby spans, checks for canonical
//! surface-form templates ("X from Y to Z", "X at Y", etc.) and emits
//! a `RelationProposal` per match.
//!
//! These templates are deliberately a small bootstrap set covering
//! the seed-pack frames (frame, valid_from / valid_to, location).
//! Real coverage grows through replay and warm-region labels; this
//! module exists so v0 ships with a working RE path while we defer
//! the GLiNER multi-task model.

use crate::inference::deberta::predict::LabeledSpan;
use crate::types::RelationStatus;

#[derive(Debug, Clone)]
pub struct RelationProposal {
    /// Subject span — references a NER-tagged entity.
    pub subject_char_start: usize,
    pub subject_char_end: usize,
    /// Canonical seed attribute name: `"from"`, `"to"`, `"with"`,
    /// `"at"`, `"property"`, etc.
    pub attribute_name: &'static str,
    /// Object span — references a different NER-tagged entity.
    pub object_char_start: usize,
    pub object_char_end: usize,
    pub confidence: f32,
    pub status: RelationStatus,
    /// Surface verb that anchored this proposal, when the template
    /// was verb-anchored (e.g. `"changed"` in "X changed from A to B").
    /// `None` for non-verb templates (`with`, `at`, `'s`). Step 8's
    /// n-ary merge pass groups by `(subject, event_anchor)` to
    /// synthesize one event relation from co-occurring `from`/`to`
    /// proposals.
    pub event_anchor: Option<String>,
    /// When `Some(label)`, the object isn't a span in `input_text` —
    /// Step 8 resolves it via `resolve_label_element(label)` instead
    /// of slicing `object_char_start..object_char_end`. Used for
    /// change-verb singletons like "X moved to Y", where the `from`
    /// slot is implicit and points at a synthetic `unknown_prior`
    /// element so the proposal still pairs into an event-shaped
    /// n-ary relation.
    pub synthetic_object_label: Option<&'static str>,
}

/// Try every supported template against the available spans.
/// `text` is the raw input (the templates inspect the
/// inter-span connective words). Spans must be ordered by
/// `char_start` ascending.
pub fn extract_relations(text: &str, spans: &[LabeledSpan]) -> Vec<RelationProposal> {
    let mut spans = spans.to_vec();
    spans.sort_by_key(|s| s.char_start);

    let mut out: Vec<RelationProposal> = Vec::new();

    // Template 1: "X from A to B" — three-way binding of subject /
    // valid_from / valid_to. Matches the rescheduling case from §13
    // Tick 1.
    for i in 0..spans.len() {
        for j in (i + 1)..spans.len() {
            for k in (j + 1)..spans.len() {
                let between_ij = &text[spans[i].char_end..spans[j].char_start];
                let between_jk = &text[spans[j].char_end..spans[k].char_start];
                let connective_ij = between_ij.to_ascii_lowercase();
                let connective_jk = between_jk.to_ascii_lowercase();
                if connective_ij.contains(" from ")
                    && connective_jk.contains(" to ")
                    && connective_ij.len() < 32
                    && connective_jk.len() < 16
                {
                    let conf = 0.7_f32.min(spans[i].score.min(spans[j].score).min(spans[k].score));
                    let status = if conf >= 0.6 {
                        RelationStatus::Entailed
                    } else {
                        RelationStatus::Defeasible
                    };
                    // Verb-anchor extraction: everything in `between_ij`
                    // up to (not including) " from ", trimmed. Empty
                    // → no anchor (the "from" sat directly after the
                    // subject, e.g. "appointment from Tuesday").
                    let anchor = anchor_before(between_ij, " from ");
                    out.push(RelationProposal {
                        subject_char_start: spans[i].char_start,
                        subject_char_end: spans[i].char_end,
                        attribute_name: "from",
                        object_char_start: spans[j].char_start,
                        object_char_end: spans[j].char_end,
                        confidence: conf,
                        status,
                        event_anchor: anchor.clone(),
                        synthetic_object_label: None,
                    });
                    out.push(RelationProposal {
                        subject_char_start: spans[i].char_start,
                        subject_char_end: spans[i].char_end,
                        attribute_name: "to",
                        object_char_start: spans[k].char_start,
                        object_char_end: spans[k].char_end,
                        confidence: conf,
                        status,
                        event_anchor: anchor,
                        synthetic_object_label: None,
                    });
                }
            }
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
                    out.push(RelationProposal {
                        subject_char_start: spans[i].char_start,
                        subject_char_end: spans[i].char_end,
                        attribute_name: attr,
                        object_char_start: spans[j].char_start,
                        object_char_end: spans[j].char_end,
                        confidence: conf,
                        status,
                        event_anchor: None,
                        synthetic_object_label: None,
                    });
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
            let conf = 0.6_f32.min(spans[i].score.min(spans[j].score));
            let status = if conf >= 0.55 {
                RelationStatus::Entailed
            } else {
                RelationStatus::Defeasible
            };
            out.push(RelationProposal {
                subject_char_start: spans[i].char_start,
                subject_char_end: spans[i].char_end,
                attribute_name: "from",
                object_char_start: spans[i].char_start,
                object_char_end: spans[i].char_end,
                confidence: conf,
                status,
                event_anchor: Some("now".to_string()),
                synthetic_object_label: Some("unknown_prior"),
            });
            out.push(RelationProposal {
                subject_char_start: spans[i].char_start,
                subject_char_end: spans[i].char_end,
                attribute_name: "to",
                object_char_start: spans[j].char_start,
                object_char_end: spans[j].char_end,
                confidence: conf,
                status,
                event_anchor: Some("now".to_string()),
                synthetic_object_label: None,
            });
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
            let conf = 0.65_f32.min(spans[i].score.min(spans[j].score));
            let status = if conf >= 0.6 {
                RelationStatus::Entailed
            } else {
                RelationStatus::Defeasible
            };
            // Synthetic `from`: points at the canonical `unknown_prior`
            // label. Carries identical anchor + subject_span so the
            // merge groups it with the overt `to` below.
            out.push(RelationProposal {
                subject_char_start: spans[i].char_start,
                subject_char_end: spans[i].char_end,
                attribute_name: "from",
                object_char_start: spans[i].char_start,
                object_char_end: spans[i].char_end,
                confidence: conf,
                status,
                event_anchor: Some(anchor.clone()),
                synthetic_object_label: Some("unknown_prior"),
            });
            out.push(RelationProposal {
                subject_char_start: spans[i].char_start,
                subject_char_end: spans[i].char_end,
                attribute_name: "to",
                object_char_start: spans[j].char_start,
                object_char_end: spans[j].char_end,
                confidence: conf,
                status,
                event_anchor: Some(anchor),
                synthetic_object_label: None,
            });
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
    // Try exact match against stripped prefix; if not, try the original
    // (handles "is now" where the stripped form is "now").
    for &verb in CHANGE_VERBS {
        if stripped == verb || p == verb {
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

/// Pull a verb-style anchor from the inter-span text. Given e.g.
/// `" changed from "` and the needle `" from "`, returns
/// `Some("changed")`. Returns `None` if the prefix is empty or
/// reduces to whitespace after trimming.
fn anchor_before(between: &str, needle: &str) -> Option<String> {
    let lower = between.to_ascii_lowercase();
    let cut = lower.find(needle)?;
    let prefix = between[..cut].trim();
    if prefix.is_empty() {
        return None;
    }
    Some(prefix.to_string())
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
        assert_eq!(from.synthetic_object_label, Some("unknown_prior"));
        assert_eq!(to.synthetic_object_label, None);
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
        assert_eq!(from.synthetic_object_label, Some("unknown_prior"));
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
        // Exactly two: from(Tuesday) + to(Friday). No synthetic from.
        assert_eq!(rels.len(), 2);
        for r in &rels {
            assert!(r.synthetic_object_label.is_none());
        }
    }
}
