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

use crate::embed::embed_text;
use crate::steps::build_relations::{mint_element, mint_relation};
use crate::types::{
    Attribute, ElementId, Hypergraph, Polarity, Relation, RelationId, RelationStatus, Term,
};

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
        if is_subject && let Some(kid) = kind_value {
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

// ─── Step 9 surface ────────────────────────────────────────────────────

/// Per-tick summary of what `supersede` actually did. Surfaces in
/// the debug print and feeds the frame's `superseded` field
/// (§11.12 of `tick_pipeline_focus.md`).
#[derive(Debug, Default, Clone)]
pub struct Step9Output {
    /// Cache relations minted this tick (one per event that fired).
    pub cache_relations: Vec<RelationId>,
    /// Prior cache relations flipped from `Asserted`/`Entailed` to
    /// `Superseded` this tick.
    pub superseded: Vec<RelationId>,
    /// `supersedes` and `derived_from` meta-relations written.
    pub meta_relations: Vec<RelationId>,
    /// New attribute-name elements minted by Step 9 itself
    /// (typically `current_<property>` siblings). Lets the caller
    /// fold these into the tick-wide observability total.
    pub attr_names_minted: u32,
}

/// Run Step 9 over the relations Step 8 minted this tick.
///
/// Filters `new_relations` to event-shaped entries (have both
/// `from` AND `to` attribute names), and for each event:
///
/// 1. Extracts `target`, `from_value`, `to_value` from the event's
///    attribute list. Skips defensively if any are missing.
/// 2. Infers a coarse property kind from the value types.
/// 3. Resolves (or mints) `current_<property>` as a Signal
///    attribute-name element.
/// 4. Mints a binary cache relation
///    `[subject: target, current_<property>: to_value]`.
///
/// Phase 2 (this commit) stops here — prior-cache supersession and
/// linking meta-relations land in phase 3.
pub fn supersede(
    hg: &mut Hypergraph,
    new_relations: &[RelationId],
    _policy: &crate::types::Policy,
) -> Step9Output {
    let mut out = Step9Output::default();

    let Some((from_attr, to_attr)) = signal_from_to_attrs(hg) else {
        return out;
    };

    // Take a snapshot of the relation IDs we care about up front —
    // we'll mutate `hg.relations` (via mint_relation) inside the
    // loop, so we can't hold borrows across iterations.
    let event_ids: Vec<RelationId> = new_relations
        .iter()
        .copied()
        .filter(|&rid| is_event_shaped(hg, rid, from_attr, to_attr))
        .collect();

    for event_id in event_ids {
        let frame = match extract_event_frame(hg, event_id, from_attr, to_attr) {
            Some(f) => f,
            None => continue,
        };

        let property_kind = infer_property_kind(hg, frame.from_value, frame.to_value);
        let cache_attr_label = format!("current_{property_kind}");
        let cache_attr_id = resolve_or_mint_signal_attr(hg, &cache_attr_label, &mut out);

        // ── Prior-cache lookup ─────────────────────────────────────
        // Any relation that mentions `target` AND carries this
        // current_<property> attribute name is a candidate cache.
        // Live (Asserted/Entailed) candidates get flipped.
        let priors = collect_prior_caches(hg, frame.target, cache_attr_id);
        for &prior in &priors {
            hg.relations[prior.0 as usize].status = RelationStatus::Superseded;
            out.superseded.push(prior);
        }

        // ── New cache relation ─────────────────────────────────────
        let event_confidence = hg.relations[event_id.0 as usize].stats.confidence;
        let cache_id = mint_relation(
            hg,
            vec![
                Attribute {
                    name: hg.subject_attr,
                    value: Term::Element(frame.target),
                },
                Attribute {
                    name: cache_attr_id,
                    value: Term::Element(frame.to_value),
                },
            ],
            RelationStatus::Asserted,
            event_confidence,
        );
        out.cache_relations.push(cache_id);

        // ── Linking meta-relations ─────────────────────────────────
        // (target: cache, derived_from: event) — always. The cache
        // derived from this event regardless of whether it
        // superseded anything.
        if let Some(derived_attr) = signal_attr(hg, "derived_from") {
            let meta = mint_relation(
                hg,
                vec![
                    Attribute {
                        name: hg.target_attr,
                        value: Term::Relation(cache_id),
                    },
                    Attribute {
                        name: derived_attr,
                        value: Term::Relation(event_id),
                    },
                ],
                RelationStatus::Entailed,
                1.0,
            );
            out.meta_relations.push(meta);
        }
        // (target: cache, supersedes: prior) — one per flipped prior.
        if !priors.is_empty()
            && let Some(supersedes_attr) = signal_attr(hg, "supersedes")
        {
            for &prior in &priors {
                let meta = mint_relation(
                    hg,
                    vec![
                        Attribute {
                            name: hg.target_attr,
                            value: Term::Relation(cache_id),
                        },
                        Attribute {
                            name: supersedes_attr,
                            value: Term::Relation(prior),
                        },
                    ],
                    RelationStatus::Entailed,
                    1.0,
                );
                out.meta_relations.push(meta);
            }
        }
    }

    out
}

/// Find live prior cache relations for the same `(target, property)`
/// pair. Live = not Superseded, not Retracted. We dedup via the
/// candidate Vec because `relations_by_element` allows a single
/// relation to appear multiple times when the target is referenced
/// in two attribute slots — unusual for caches but the dedup is
/// cheap and prevents double-flips.
fn collect_prior_caches(
    hg: &Hypergraph,
    target: ElementId,
    cache_attr: ElementId,
) -> Vec<RelationId> {
    let Some(candidates) = hg.relations_by_element.get(&target) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for &rid in candidates {
        if !seen.insert(rid) {
            continue;
        }
        let r = &hg.relations[rid.0 as usize];
        if matches!(
            r.status,
            RelationStatus::Superseded | RelationStatus::Retracted
        ) {
            continue;
        }
        // Must have the cache_attr in its attribute list AND have
        // target in the subject slot (the cache shape). If target
        // sits in some other slot of this relation, it's not the
        // matching cache.
        let mut has_cache_attr = false;
        let mut subject_is_target = false;
        for attr in &r.attributes {
            if attr.name == cache_attr {
                has_cache_attr = true;
            }
            if attr.name == hg.subject_attr
                && let Term::Element(e) = attr.value
                && e == target
            {
                subject_is_target = true;
            }
        }
        if has_cache_attr && subject_is_target {
            out.push(rid);
        }
    }
    out
}

/// Slots extracted from an event-shaped Relation. Step 9 needs all
/// three to synthesize a cache; missing any means the relation
/// isn't really an event and gets skipped defensively.
#[derive(Debug, Clone, Copy)]
struct EventFrame {
    target: ElementId,
    from_value: ElementId,
    to_value: ElementId,
}

/// Pull the Signal-polarity `from` and `to` attribute-name IDs out
/// of `by_name`. Returns `None` if either isn't seeded (means the
/// pack is broken — defensive only).
fn signal_from_to_attrs(hg: &Hypergraph) -> Option<(ElementId, ElementId)> {
    let from = signal_attr(hg, "from")?;
    let to = signal_attr(hg, "to")?;
    Some((from, to))
}

fn signal_attr(hg: &Hypergraph, name: &str) -> Option<ElementId> {
    let ids = hg.by_name.get(name)?;
    ids.iter()
        .copied()
        .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
        .or_else(|| ids.first().copied())
}

fn is_event_shaped(
    hg: &Hypergraph,
    rid: RelationId,
    from_attr: ElementId,
    to_attr: ElementId,
) -> bool {
    let r = &hg.relations[rid.0 as usize];
    let mut has_from = false;
    let mut has_to = false;
    for attr in &r.attributes {
        if attr.name == from_attr {
            has_from = true;
        }
        if attr.name == to_attr {
            has_to = true;
        }
    }
    has_from && has_to
}

/// Pull the event's `target`, `from`-value, `to`-value Element IDs.
/// Returns `None` if any are missing — Step 8's n-ary events should
/// always have all three, but the filter is permissive on shape so
/// guard at extraction time.
fn extract_event_frame(
    hg: &Hypergraph,
    rid: RelationId,
    from_attr: ElementId,
    to_attr: ElementId,
) -> Option<EventFrame> {
    let r: &Relation = &hg.relations[rid.0 as usize];
    let target_attr = hg.target_attr;
    let mut target = None;
    let mut from_value = None;
    let mut to_value = None;
    for attr in &r.attributes {
        let Term::Element(e) = attr.value else {
            continue;
        };
        if attr.name == target_attr {
            target = Some(e);
        } else if attr.name == from_attr {
            from_value = Some(e);
        } else if attr.name == to_attr {
            to_value = Some(e);
        }
    }
    Some(EventFrame {
        target: target?,
        from_value: from_value?,
        to_value: to_value?,
    })
}

/// Resolve `label` to a Signal attribute-name Element ID. Mints
/// one if no Signal match exists, bumping
/// `Step9Output.attr_names_minted`. Step 9 mints these on the fly
/// rather than seeding them so the seed pack stays slim.
fn resolve_or_mint_signal_attr(
    hg: &mut Hypergraph,
    label: &str,
    out: &mut Step9Output,
) -> ElementId {
    if let Some(ids) = hg.by_name.get(label) {
        // Prefer Signal; only fall through to Void if no Signal exists.
        for &id in ids {
            if hg.elements[id.0 as usize].polarity == Polarity::Signal {
                return id;
            }
        }
        if let Some(&id) = ids.first() {
            return id;
        }
    }
    let id = mint_element(
        hg,
        vec![label.to_string()],
        embed_text(label),
        Polarity::Signal,
        hg.policy.default_conf,
    );
    out.attr_names_minted = out.attr_names_minted.saturating_add(1);
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::EMBEDDING_DIM;
    use crate::types::{Element, MemoryStats, Tick};

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

    /// End-to-end fixture: load the real seed pack, run Step 5 and
    /// Step 8, then return the Hypergraph + Step8Output so Step 9
    /// tests can hand its `minted_relations` to `supersede`. This
    /// integration-style fixture mirrors what `lib.rs::run` does
    /// between Step 8 and Step 9.
    fn run_through_step8(text: &str, labels: &[&str]) -> (Hypergraph, Vec<RelationId>) {
        use crate::seed::load_seed_graph;
        use crate::steps::build_relations::build_relations;
        use crate::steps::run_extractors::run_extractors;
        let mut hg = load_seed_graph();
        let policy = crate::types::Policy::default();
        let ext = run_extractors(text, labels, &policy, &hg, &[]);
        let step8 = build_relations(text, &mut hg, &ext, &policy, None);
        (hg, step8.minted_relations)
    }

    #[test]
    fn supersede_skips_non_event_relations() {
        // "Sarah called me yesterday." — produces instance_of
        // relations but no event-shaped from/to relations. Step 9
        // should mint zero caches.
        let (mut hg, minted) = run_through_step8("Sarah called me yesterday.", &[]);
        let policy = crate::types::Policy::default();
        let out = supersede(&mut hg, &minted, &policy);
        assert!(
            out.cache_relations.is_empty(),
            "expected no cache mints without a from/to event; got {:?}",
            out.cache_relations,
        );
    }

    #[test]
    fn supersede_mints_cache_for_dentist_event() {
        let (mut hg, minted) = run_through_step8(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
        );
        let policy = crate::types::Policy::default();
        let out = supersede(&mut hg, &minted, &policy);

        assert!(
            !out.cache_relations.is_empty(),
            "expected at least one cache mint from the changed-from/to event",
        );

        // Cache shape: [subject: target, current_date: to_value].
        let current_date_id = hg.by_name["current_date"]
            .iter()
            .copied()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
            .expect("current_date should have been minted by Step 9");
        for &rid in &out.cache_relations {
            let r = &hg.relations[rid.0 as usize];
            assert_eq!(r.attributes.len(), 2, "cache is binary");
            let has_subject = r.attributes.iter().any(|a| a.name == hg.subject_attr);
            let has_current = r.attributes.iter().any(|a| a.name == current_date_id);
            assert!(has_subject && has_current);
            assert_eq!(r.status, RelationStatus::Asserted);
        }
    }

    #[test]
    fn prior_cache_flips_to_superseded() {
        // Two-step setup: run a "Tuesday" event, then a "Friday"
        // event over the same target. The first event's cache
        // should flip when the second one mints.
        let text1 = "The meeting moved from Monday to Tuesday.";
        let (mut hg, minted1) = run_through_step8(text1, &["event", "weekday"]);
        let policy = crate::types::Policy::default();
        let out1 = supersede(&mut hg, &minted1, &policy);
        assert!(
            !out1.cache_relations.is_empty(),
            "tick 1 should mint a cache"
        );
        // Tick 1's cache should be Asserted.
        for &rid in &out1.cache_relations {
            assert_eq!(
                hg.relations[rid.0 as usize].status,
                RelationStatus::Asserted
            );
        }

        // Run a second event that should supersede tick 1's cache.
        // Use the same subject name so resolve_span hits the existing
        // element via by_name.
        let text2 = "The meeting moved from Tuesday to Friday.";
        let ext2 = crate::steps::run_extractors::run_extractors(
            text2,
            &["event", "weekday"],
            &policy,
            &hg,
            &[],
        );
        let step8 =
            crate::steps::build_relations::build_relations(text2, &mut hg, &ext2, &policy, None);
        let out2 = supersede(&mut hg, &step8.minted_relations, &policy);

        assert!(
            !out2.superseded.is_empty(),
            "tick 2 should flip at least one prior cache",
        );
        for &flipped in &out2.superseded {
            assert_eq!(
                hg.relations[flipped.0 as usize].status,
                RelationStatus::Superseded,
            );
        }
    }

    #[test]
    fn supersession_emits_derived_from_and_supersedes_metas() {
        let text1 = "The meeting moved from Monday to Tuesday.";
        let (mut hg, minted1) = run_through_step8(text1, &["event", "weekday"]);
        let policy = crate::types::Policy::default();
        let _ = supersede(&mut hg, &minted1, &policy);

        let text2 = "The meeting moved from Tuesday to Friday.";
        let ext2 = crate::steps::run_extractors::run_extractors(
            text2,
            &["event", "weekday"],
            &policy,
            &hg,
            &[],
        );
        let step8 =
            crate::steps::build_relations::build_relations(text2, &mut hg, &ext2, &policy, None);
        let out2 = supersede(&mut hg, &step8.minted_relations, &policy);

        let derived_from_id = hg.by_name["derived_from"][0];
        let supersedes_id = hg.by_name["supersedes"][0];

        let derived_metas: Vec<RelationId> = out2
            .meta_relations
            .iter()
            .copied()
            .filter(|rid| {
                hg.relations[rid.0 as usize]
                    .attributes
                    .iter()
                    .any(|a| a.name == derived_from_id)
            })
            .collect();
        let supersedes_metas: Vec<RelationId> = out2
            .meta_relations
            .iter()
            .copied()
            .filter(|rid| {
                hg.relations[rid.0 as usize]
                    .attributes
                    .iter()
                    .any(|a| a.name == supersedes_id)
            })
            .collect();

        assert!(
            !derived_metas.is_empty(),
            "at least one derived_from meta per minted cache",
        );
        assert!(
            !supersedes_metas.is_empty(),
            "at least one supersedes meta when a prior flipped",
        );

        // Every meta should point at a Step9-minted cache via target.
        for &mid in derived_metas.iter().chain(supersedes_metas.iter()) {
            let r = &hg.relations[mid.0 as usize];
            let target_slot = r
                .attributes
                .iter()
                .find(|a| a.name == hg.target_attr)
                .expect("meta must carry a target slot");
            match target_slot.value {
                Term::Relation(rid) => {
                    assert!(
                        out2.cache_relations.contains(&rid),
                        "meta target should point at a Step9 cache",
                    );
                }
                Term::Element(_) => panic!("target slot must be a Term::Relation"),
            }
        }
    }

    #[test]
    fn no_prior_no_supersedes() {
        // First event over a fresh target. Cache mints, derived_from
        // fires (audit trail), but no supersedes meta should appear.
        let text = "Sarah rescheduled the meeting from Tuesday to Friday.";
        let (mut hg, minted) = run_through_step8(text, &["person", "event", "weekday"]);
        let policy = crate::types::Policy::default();
        let out = supersede(&mut hg, &minted, &policy);

        assert!(!out.cache_relations.is_empty());
        assert!(
            out.superseded.is_empty(),
            "no prior caches existed; nothing should flip",
        );
        let supersedes_id = hg.by_name["supersedes"][0];
        let has_supersedes = out.meta_relations.iter().any(|rid| {
            hg.relations[rid.0 as usize]
                .attributes
                .iter()
                .any(|a| a.name == supersedes_id)
        });
        assert!(!has_supersedes, "no supersedes meta when no prior flipped",);
    }

    #[test]
    fn current_property_attribute_is_reused_across_events() {
        // Two events in one tick that both resolve to property=date
        // should both bind to the SAME current_date attribute-name
        // Element. Tests Step 9's mint-and-cache behavior.
        let (mut hg, minted) = run_through_step8(
            "Sarah rescheduled the meeting from Tuesday to Friday.",
            &["person", "event", "weekday"],
        );
        let policy = crate::types::Policy::default();
        let out = supersede(&mut hg, &minted, &policy);

        // attr_names_minted should be ≤ 1 — the first event mints
        // current_date, every subsequent event reuses it.
        assert!(
            out.attr_names_minted <= 1,
            "expected current_date to be minted at most once, got {}",
            out.attr_names_minted,
        );
        // The current_date attr should exist in by_name now.
        assert!(
            hg.by_name.contains_key("current_date"),
            "Step 9 should have minted current_date",
        );
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
