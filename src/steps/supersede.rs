//! Step 9 — supersession and cache.
//!
//! Reads Step 8's `minted_relations`, filters to event-shaped
//! relations (have both `from` AND `to` attributes), and for each
//! event:
//!
//! 1. Infers a `property_kind` label from the `from`/`to` value
//!    types via `infer_property_kind` (§4 of `step_9_design.md`).
//! 2. Mints a binary cache relation
//!    `[subject: target, current_<property>: to_value]`.
//! 3. Looks up prior caches for the same `(target, property)` pair
//!    and flips them to `Superseded`.
//! 4. Writes `derived_from` and `supersedes` linking meta-relations.
//!
//! No model. Everything reads from the seven Step 8 indices.
//! See `step_9_design.md` for the full spec.

use crate::types::{ElementId, Hypergraph, Term};

/// Walk `value_id`'s `instance_of` relations and return the kind
/// label's surface form. Returns `None` if `value_id` has no
/// `instance_of` relation in the subject slot.
///
/// "Subject slot" means `[subject: value_id, instance_of: kind]`.
/// Relations where `value_id` appears in other slots (e.g. as
/// `from` or `to`) are skipped — we want the typing of `value_id`,
/// not relations that mention it.
pub(crate) fn kind_of(hg: &Hypergraph, value_id: ElementId) -> Option<String> {
    let instance_of_attr = hg.by_name.get("instance_of")?.first().copied()?;
    let candidates = hg.relations_by_element.get(&value_id)?;
    for &rid in candidates {
        let r = &hg.relations[rid.0 as usize];
        let mut is_subject = false;
        let mut kind_value: Option<ElementId> = None;
        for attr in &r.attributes {
            if attr.name == hg.subject_attr {
                if let Term::Element(e) = attr.value
                    && e == value_id
                {
                    is_subject = true;
                }
            } else if attr.name == instance_of_attr
                && let Term::Element(e) = attr.value
            {
                kind_value = Some(e);
            }
        }
        if is_subject
            && let Some(kid) = kind_value
        {
            return hg.elements[kid.0 as usize].names.first().cloned();
        }
    }
    None
}

/// Derive a coarse property-kind label from the `from` and `to`
/// value types of a state-change event. Used by Step 9 to construct
/// the cache attribute name `current_<property>`.
///
/// Match table:
///
/// - both weekday or month (any combination) → `"date"`
/// - both time → `"time"`
/// - both quantity → `"amount"`
/// - both place → `"location"`
/// - else → `"value"` (generic fallback)
pub fn infer_property_kind(hg: &Hypergraph, from_id: ElementId, to_id: ElementId) -> &'static str {
    let f = kind_of(hg, from_id);
    let t = kind_of(hg, to_id);
    let (f, t) = (f.as_deref(), t.as_deref());
    match (f, t) {
        // Dates: both temporal-class kinds line up under one bucket.
        (Some("weekday" | "month"), Some("weekday" | "month")) => "date",
        (Some("time"), Some("time")) => "time",
        (Some("quantity"), Some("quantity")) => "amount",
        (Some("place"), Some("place")) => "location",
        _ => "value",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::EMBEDDING_DIM;
    use crate::steps::build_relations::{mint_element, mint_relation};
    use crate::types::{Attribute, Element, Hypergraph, MemoryStats, Polarity, RelationStatus, Tick};

    /// Set up a synthetic Hypergraph with the seeded structural
    /// attribute-name elements at fixed IDs, then return it. We
    /// re-create a minimal slice rather than loading the full seed
    /// pack so unit tests run fast.
    fn synth_hg() -> Hypergraph {
        let mut hg = Hypergraph::default();
        // Slot 0 = subject_attr, slot 1 = instance_of_attr.
        let subject = Element {
            id: ElementId(0),
            names: vec!["subject".to_string()],
            stats: MemoryStats::default(),
            created_at: Tick(0),
            embedding: vec![0.0; EMBEDDING_DIM],
            polarity: Polarity::Signal,
        };
        let instance_of = Element {
            id: ElementId(1),
            names: vec!["instance_of".to_string()],
            stats: MemoryStats::default(),
            created_at: Tick(0),
            embedding: vec![0.0; EMBEDDING_DIM],
            polarity: Polarity::Signal,
        };
        hg.elements.push(subject);
        hg.elements.push(instance_of);
        hg.subject_attr = ElementId(0);
        hg.by_name.insert("subject".to_string(), vec![ElementId(0)]);
        hg.by_name
            .insert("instance_of".to_string(), vec![ElementId(1)]);
        hg
    }

    /// Mint a value Element `v` plus a `(v, instance_of, kind)`
    /// relation. Returns `v`'s ElementId. The kind Element is also
    /// minted/re-used by name.
    fn mint_with_kind(hg: &mut Hypergraph, name: &str, kind: &str) -> ElementId {
        let v_id = mint_element(
            hg,
            vec![name.to_string()],
            vec![0.0; EMBEDDING_DIM],
            Polarity::Signal,
            1.0,
        );
        let kind_id = match hg.by_name.get(kind).and_then(|ids| ids.first().copied()) {
            Some(id) => id,
            None => mint_element(
                hg,
                vec![kind.to_string()],
                vec![0.0; EMBEDDING_DIM],
                Polarity::Signal,
                1.0,
            ),
        };
        let instance_of_attr = hg.by_name["instance_of"][0];
        mint_relation(
            hg,
            vec![
                Attribute {
                    name: hg.subject_attr,
                    value: Term::Element(v_id),
                },
                Attribute {
                    name: instance_of_attr,
                    value: Term::Element(kind_id),
                },
            ],
            RelationStatus::Entailed,
            1.0,
        );
        v_id
    }

    #[test]
    fn kind_of_returns_instance_of_kind() {
        let mut hg = synth_hg();
        let tuesday = mint_with_kind(&mut hg, "Tuesday", "weekday");
        assert_eq!(kind_of(&hg, tuesday).as_deref(), Some("weekday"));
    }

    #[test]
    fn kind_of_returns_none_for_unkinded_value() {
        let mut hg = synth_hg();
        let mystery = mint_element(
            &mut hg,
            vec!["mystery".to_string()],
            vec![0.0; EMBEDDING_DIM],
            Polarity::Signal,
            1.0,
        );
        assert!(kind_of(&hg, mystery).is_none());
    }

    #[test]
    fn weekday_to_weekday_yields_date() {
        let mut hg = synth_hg();
        let tue = mint_with_kind(&mut hg, "Tuesday", "weekday");
        let fri = mint_with_kind(&mut hg, "Friday", "weekday");
        assert_eq!(infer_property_kind(&hg, tue, fri), "date");
    }

    #[test]
    fn month_to_weekday_still_yields_date() {
        let mut hg = synth_hg();
        let mar = mint_with_kind(&mut hg, "March", "month");
        let fri = mint_with_kind(&mut hg, "Friday", "weekday");
        assert_eq!(infer_property_kind(&hg, mar, fri), "date");
    }

    #[test]
    fn quantity_to_quantity_yields_amount() {
        let mut hg = synth_hg();
        let q1 = mint_with_kind(&mut hg, "$10", "quantity");
        let q2 = mint_with_kind(&mut hg, "$20", "quantity");
        assert_eq!(infer_property_kind(&hg, q1, q2), "amount");
    }

    #[test]
    fn place_to_place_yields_location() {
        let mut hg = synth_hg();
        let a = mint_with_kind(&mut hg, "Berlin", "place");
        let b = mint_with_kind(&mut hg, "Paris", "place");
        assert_eq!(infer_property_kind(&hg, a, b), "location");
    }

    #[test]
    fn mixed_kinds_fall_back_to_value() {
        let mut hg = synth_hg();
        let tue = mint_with_kind(&mut hg, "Tuesday", "weekday");
        let ten = mint_with_kind(&mut hg, "$10", "quantity");
        assert_eq!(infer_property_kind(&hg, tue, ten), "value");
    }

    #[test]
    fn no_kind_at_all_falls_back_to_value() {
        let mut hg = synth_hg();
        let a = mint_element(
            &mut hg,
            vec!["a".to_string()],
            vec![0.0; EMBEDDING_DIM],
            Polarity::Signal,
            1.0,
        );
        let b = mint_element(
            &mut hg,
            vec!["b".to_string()],
            vec![0.0; EMBEDDING_DIM],
            Polarity::Signal,
            1.0,
        );
        assert_eq!(infer_property_kind(&hg, a, b), "value");
    }
}
