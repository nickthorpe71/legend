//! Substrate-aware union of two `Hypergraph` snapshots.
//!
//! Used by the `git-merge-driver` subcommand to resolve `.legend/memory.lz4`
//! conflicts. Two branches running Legend will each mutate their local
//! snapshot independently — when those branches merge, this module
//! combines both sides into a single coherent substrate.
//!
//! ## The remap problem
//!
//! `ElementId` is a local handle into `Hypergraph.elements` — it is NOT
//! globally unique. Two machines that both mint "Polaris" on their own
//! branches will end up with different `ElementId`s for the same logical
//! entity. So this merge cannot just union by ID; it has to identify
//! same-entity-across-branches first.
//!
//! Strategy:
//! 1. **Seeded elements** (index < `seed_count`) — identical on both
//!    sides by construction. Trivial remap.
//! 2. **Post-seed elements** — match by `(first_name, polarity)`. Same
//!    name + same polarity post-seed = same logical entity. False
//!    matches are rare in practice and bounded (the substrate already
//!    treats canonical names as fuzzy anchors).
//! 3. **No match** — fresh-mint in `ours` with a new `ElementId`; build
//!    a `theirs_id → ours_id` remap for the next phase.
//!
//! ## The relation rewriting problem
//!
//! Once elements are remapped, every relation from `theirs` has to be
//! rewritten so its `Attribute.name` and `Attribute.value` fields point
//! at the corresponding `ours` ids. Meta-relations also reference other
//! relations via `Term::Relation`, so we process in two passes: base
//! relations first, then metas (whose dependencies are now resolved).
//!
//! ## Status resolution
//!
//! When two relations collapse into one, the merged status takes the
//! most "decisive" of the two:
//!
//!   Retracted > Superseded > Asserted > Entailed > Defeasible
//!
//! "Decisive" means closer to a hard yes/no. If either side flipped a
//! relation to Superseded or Retracted, the merge respects that.
//! Otherwise the more-confident-of-the-two wins.

use std::collections::HashMap;

use crate::seed::{load_seed_graph, rebuild_indices};
use crate::types::{
    Attribute, ElementId, Hypergraph, Polarity, RecentFocusEntry, RelationId, RelationStatus, Term,
    Tick,
};

/// Counts of what the merge did. Surfaces in the CLI output of
/// `legend git-merge-driver` so a teammate can see what reconciled.
#[derive(Debug, Default, Clone, Copy)]
pub struct MergeStats {
    pub elements_added: usize,
    pub elements_unified: usize,
    pub relations_added: usize,
    pub relations_merged: usize,
    pub recent_focus_added: usize,
}

impl MergeStats {
    /// Total items touched (added + collisions resolved). Used in
    /// the merge driver's stderr summary line.
    pub fn total(&self) -> usize {
        self.elements_added + self.elements_unified + self.relations_added + self.relations_merged
    }
}

#[derive(Debug)]
pub enum MergeError {
    /// Two sides have different anchor IDs — they were built from
    /// different seed binaries. Refuse to merge; the user has to
    /// regenerate the seed on one side before retrying.
    AnchorMismatch {
        which: &'static str,
        ours: ElementId,
        theirs: ElementId,
    },
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeError::AnchorMismatch {
                which,
                ours,
                theirs,
            } => write!(
                f,
                "anchor mismatch on `{which}`: ours={ours:?} theirs={theirs:?}. \
                 The two snapshots were built from different seed binaries. \
                 Regenerate the seed binary on one side and retry.",
            ),
        }
    }
}

impl std::error::Error for MergeError {}

/// Merge `theirs` into `ours` in place. After this returns, `ours`
/// carries the union of both substrates with collisions resolved per
/// the status / stats rules above.
///
/// `ours` is returned fully re-indexed (`rebuild_indices` is called
/// at the end), so the result is immediately usable for ticks.
pub fn merge_hypergraphs(
    ours: &mut Hypergraph,
    theirs: &Hypergraph,
) -> Result<MergeStats, MergeError> {
    check_anchors_match(ours, theirs)?;

    let mut stats = MergeStats::default();
    let seed_count = load_seed_graph().elements.len();

    // ── Phase 1: Element ID remap ─────────────────────────────────
    let mut elem_remap: HashMap<ElementId, ElementId> = HashMap::new();
    populate_seed_prefix_remap(ours, theirs, seed_count, &mut elem_remap);
    unify_or_append_post_seed_elements(ours, theirs, seed_count, &mut elem_remap, &mut stats);

    // ── Phase 2: Relation rewriting + merging ─────────────────────
    let mut rel_remap: HashMap<RelationId, RelationId> = HashMap::new();
    merge_relations(ours, theirs, &elem_remap, &mut rel_remap, &mut stats);

    // ── Phase 3: Other state ─────────────────────────────────────
    ours.clock = Tick(ours.clock.0.max(theirs.clock.0));
    merge_recent_focus(ours, theirs, &elem_remap, &mut stats);
    if theirs.clock > ours.clock {
        // Policy is intent-modulated per tick; the rest-state policy
        // shouldn't drift much between branches, but if it did, the
        // side with the later clock has the more recent rest state.
        ours.policy = theirs.policy.clone();
    }

    // ── Phase 4: Rebuild every derived index from the new ground truth.
    rebuild_indices(ours);

    Ok(stats)
}

// ─── Phase 1 helpers ────────────────────────────────────────────────

fn check_anchors_match(ours: &Hypergraph, theirs: &Hypergraph) -> Result<(), MergeError> {
    macro_rules! check {
        ($field:ident, $name:expr) => {
            if ours.$field != theirs.$field {
                return Err(MergeError::AnchorMismatch {
                    which: $name,
                    ours: ours.$field,
                    theirs: theirs.$field,
                });
            }
        };
    }
    check!(void, "void");
    check!(genesis, "genesis");
    check!(region_class, "region_class");
    check!(reference_frame_class, "reference_frame_class");
    check!(subject_attr, "subject_attr");
    check!(parent_region_attr, "parent_region_attr");
    check!(prototype_attr, "prototype_attr");
    check!(target_attr, "target_attr");
    Ok(())
}

/// Seeded elements (index < `seed_count`) are identical on both
/// sides — same IDs, same names, same embeddings — so the remap is
/// the identity function over that prefix. Max-merge `access_count`
/// and `last_seen` so the popularity signal doesn't reset.
fn populate_seed_prefix_remap(
    ours: &mut Hypergraph,
    theirs: &Hypergraph,
    seed_count: usize,
    elem_remap: &mut HashMap<ElementId, ElementId>,
) {
    let common_seed = seed_count
        .min(ours.elements.len())
        .min(theirs.elements.len());
    for i in 0..common_seed {
        let id = ElementId(i as u32);
        elem_remap.insert(id, id);
        let their_stats = theirs.elements[i].stats;
        let our_el = &mut ours.elements[i];
        our_el.stats.access_count = our_el.stats.access_count.max(their_stats.access_count);
        our_el.stats.last_seen = Tick(our_el.stats.last_seen.0.max(their_stats.last_seen.0));
    }
}

/// For each post-seed element in `theirs`, find a matching post-seed
/// element in `ours` by `(first_name, polarity)`. If found, unify;
/// otherwise append to `ours` and remap.
fn unify_or_append_post_seed_elements(
    ours: &mut Hypergraph,
    theirs: &Hypergraph,
    seed_count: usize,
    elem_remap: &mut HashMap<ElementId, ElementId>,
    stats: &mut MergeStats,
) {
    // Build a (name, polarity) → ElementId index over ours's post-seed
    // elements. Cheaper than scanning ours linearly per their_element.
    let mut by_name_post_seed: HashMap<(String, Polarity), ElementId> = HashMap::new();
    for (idx, el) in ours.elements.iter().enumerate() {
        if idx < seed_count {
            continue;
        }
        if let Some(name) = el.names.first() {
            by_name_post_seed.insert((name.clone(), el.polarity), ElementId(idx as u32));
        }
    }

    for (idx, their_el) in theirs.elements.iter().enumerate() {
        if idx < seed_count {
            continue;
        }
        let their_id = ElementId(idx as u32);
        let key = their_el
            .names
            .first()
            .map(|n| (n.clone(), their_el.polarity));
        let matched = key.as_ref().and_then(|k| by_name_post_seed.get(k).copied());

        if let Some(our_id) = matched {
            elem_remap.insert(their_id, our_id);
            unify_element(&mut ours.elements[our_id.0 as usize], their_el);
            stats.elements_unified += 1;
        } else {
            let new_id = ElementId(ours.elements.len() as u32);
            let mut clone = their_el.clone();
            clone.id = new_id;
            // `created_at` keeps theirs's earlier mint timestamp; no
            // semantic ambiguity here.
            ours.elements.push(clone);
            elem_remap.insert(their_id, new_id);
            if let Some(name) = ours.elements[new_id.0 as usize].names.first() {
                by_name_post_seed.insert(
                    (name.clone(), ours.elements[new_id.0 as usize].polarity),
                    new_id,
                );
            }
            stats.elements_added += 1;
        }
    }
}

/// Fold a `theirs` element into the matching `ours` element. Union
/// names (dedup-preserving), max-merge stats, keep the earlier
/// `created_at`. Embedding stays ours's — the streaming-centroid
/// math in `embed.rs` would need access-count weights from BOTH sides
/// to blend correctly, and a wrong blend is worse than picking ours.
fn unify_element(our_el: &mut crate::types::Element, their_el: &crate::types::Element) {
    for name in &their_el.names {
        if !our_el.names.contains(name) {
            our_el.names.push(name.clone());
        }
    }
    our_el.stats.access_count = our_el.stats.access_count.max(their_el.stats.access_count);
    our_el.stats.last_seen = Tick(our_el.stats.last_seen.0.max(their_el.stats.last_seen.0));
    our_el.stats.confidence = our_el.stats.confidence.max(their_el.stats.confidence);
    our_el.stats.salience = our_el.stats.salience.max(their_el.stats.salience);
    our_el.stats.activation = our_el.stats.activation.max(their_el.stats.activation);
    our_el.stats.support_count = our_el.stats.support_count.max(their_el.stats.support_count);
    our_el.stats.support_diversity = our_el
        .stats
        .support_diversity
        .max(their_el.stats.support_diversity);
    our_el.stats.focus_success_count = our_el
        .stats
        .focus_success_count
        .max(their_el.stats.focus_success_count);
    if their_el.created_at.0 < our_el.created_at.0 {
        our_el.created_at = their_el.created_at;
    }
}

// ─── Phase 2 helpers ────────────────────────────────────────────────

/// Two-pass relation merge: base relations first (no `Term::Relation`
/// references), then meta-relations whose dependencies are now in the
/// remap. Iterate until no progress — handles arbitrary meta-of-meta
/// nesting without depth assumptions.
fn merge_relations(
    ours: &mut Hypergraph,
    theirs: &Hypergraph,
    elem_remap: &HashMap<ElementId, ElementId>,
    rel_remap: &mut HashMap<RelationId, RelationId>,
    stats: &mut MergeStats,
) {
    let mut processed = vec![false; theirs.relations.len()];

    loop {
        let mut progress = false;
        for (idx, their_rel) in theirs.relations.iter().enumerate() {
            if processed[idx] {
                continue;
            }
            // Defer until all Term::Relation deps are remapped.
            let deps_ready = their_rel.attributes.iter().all(|a| match a.value {
                Term::Relation(parent) => rel_remap.contains_key(&parent),
                _ => true,
            });
            if !deps_ready {
                continue;
            }
            let rewritten = rewrite_attributes(&their_rel.attributes, elem_remap, rel_remap);
            let matched = ours
                .relations
                .iter()
                .position(|r| attributes_equal(&r.attributes, &rewritten));
            if let Some(our_pos) = matched {
                let our_id = ours.relations[our_pos].id;
                rel_remap.insert(their_rel.id, our_id);
                merge_relation_stats(&mut ours.relations[our_pos], their_rel);
                stats.relations_merged += 1;
            } else {
                let new_id = RelationId(ours.relations.len() as u32);
                let mut clone = their_rel.clone();
                clone.id = new_id;
                clone.attributes = rewritten;
                ours.relations.push(clone);
                rel_remap.insert(their_rel.id, new_id);
                stats.relations_added += 1;
            }
            processed[idx] = true;
            progress = true;
        }
        if !progress {
            break;
        }
    }
    // Any leftover unprocessed relations have cyclic / unresolvable
    // `Term::Relation` references. Pre-existing v0 substrate never
    // produces those (meta-relations form a DAG rooted at base
    // relations), so this would indicate corruption.
    debug_assert!(
        processed.iter().all(|p| *p),
        "merge: cyclic relation references; check substrate integrity"
    );
}

fn rewrite_attributes(
    attrs: &[Attribute],
    elem_remap: &HashMap<ElementId, ElementId>,
    rel_remap: &HashMap<RelationId, RelationId>,
) -> Vec<Attribute> {
    attrs
        .iter()
        .map(|a| {
            let new_value = match a.value {
                Term::Element(e) => Term::Element(*elem_remap.get(&e).unwrap_or(&e)),
                Term::Relation(r) => Term::Relation(*rel_remap.get(&r).unwrap_or(&r)),
            };
            Attribute {
                name: *elem_remap.get(&a.name).unwrap_or(&a.name),
                value: new_value,
            }
        })
        .collect()
}

fn attributes_equal(a: &[Attribute], b: &[Attribute]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(x, y)| x.name == y.name && x.value == y.value)
}

fn merge_relation_stats(our: &mut crate::types::Relation, their: &crate::types::Relation) {
    our.status = resolve_status(our.status, their.status);
    our.priority = our.priority.max(their.priority);
    if their.created_at.0 < our.created_at.0 {
        our.created_at = their.created_at;
    }
    our.stats.confidence = our.stats.confidence.max(their.stats.confidence);
    our.stats.support_count = our.stats.support_count.max(their.stats.support_count);
    our.stats.support_diversity = our
        .stats
        .support_diversity
        .max(their.stats.support_diversity);
    our.stats.salience = our.stats.salience.max(their.stats.salience);
    our.stats.activation = our.stats.activation.max(their.stats.activation);
    our.stats.focus_success_count = our
        .stats
        .focus_success_count
        .max(their.stats.focus_success_count);
    our.stats.last_seen = Tick(our.stats.last_seen.0.max(their.stats.last_seen.0));
    our.stats.access_count = our.stats.access_count.max(their.stats.access_count);
}

fn status_rank(s: RelationStatus) -> u8 {
    match s {
        RelationStatus::Retracted => 5,
        RelationStatus::Superseded => 4,
        RelationStatus::Asserted => 3,
        RelationStatus::Entailed => 2,
        RelationStatus::Defeasible => 1,
    }
}

fn resolve_status(a: RelationStatus, b: RelationStatus) -> RelationStatus {
    if status_rank(a) >= status_rank(b) {
        a
    } else {
        b
    }
}

// ─── Phase 3 helpers ────────────────────────────────────────────────

/// Union both sides' `recent_focus` entries, dedup by `(element,
/// attribute)`, sort by tick descending, cap at policy capacity.
fn merge_recent_focus(
    ours: &mut Hypergraph,
    theirs: &Hypergraph,
    elem_remap: &HashMap<ElementId, ElementId>,
    stats: &mut MergeStats,
) {
    let cap = ours.policy.recent_focus_capacity as usize;
    let mut union: Vec<RecentFocusEntry> = ours.recent_focus.iter().copied().collect();
    for their_entry in &theirs.recent_focus {
        let remapped = RecentFocusEntry {
            element: *elem_remap
                .get(&their_entry.element)
                .unwrap_or(&their_entry.element),
            attribute: their_entry
                .attribute
                .map(|a| *elem_remap.get(&a).unwrap_or(&a)),
            frame: their_entry.frame.map(|f| *elem_remap.get(&f).unwrap_or(&f)),
            tick: their_entry.tick,
        };
        let already = union
            .iter()
            .any(|e| e.element == remapped.element && e.attribute == remapped.attribute);
        if !already {
            union.push(remapped);
            stats.recent_focus_added += 1;
        }
    }
    union.sort_by_key(|e| std::cmp::Reverse(e.tick.0));
    if cap > 0 {
        union.truncate(cap);
    }
    ours.recent_focus = union.into_iter().collect();
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;

    /// Add a synthetic Signal element with `name` and return its ID.
    fn add_element(hg: &mut Hypergraph, name: &str) -> ElementId {
        let id = ElementId(hg.elements.len() as u32);
        hg.elements.push(crate::types::Element {
            id,
            names: vec![name.to_string()],
            stats: Default::default(),
            created_at: hg.clock,
            embedding: vec![0.0; crate::embed::EMBEDDING_DIM],
            polarity: Polarity::Signal,
        });
        rebuild_indices(hg);
        id
    }

    /// Add an `(subject: sub, attr: obj)` Asserted relation.
    fn add_binary_relation(
        hg: &mut Hypergraph,
        sub: ElementId,
        attr_name: ElementId,
        obj: ElementId,
    ) -> RelationId {
        let id = RelationId(hg.relations.len() as u32);
        hg.relations.push(crate::types::Relation {
            id,
            attributes: vec![
                Attribute {
                    name: hg.subject_attr,
                    value: Term::Element(sub),
                },
                Attribute {
                    name: attr_name,
                    value: Term::Element(obj),
                },
            ],
            status: RelationStatus::Asserted,
            stats: Default::default(),
            priority: 0,
            created_at: hg.clock,
        });
        rebuild_indices(hg);
        id
    }

    #[test]
    fn merge_into_empty_self_takes_other_state() {
        let ours_seed_count = load_seed_graph().elements.len();
        let mut ours = load_seed_graph();
        let mut theirs = load_seed_graph();
        let polaris = add_element(&mut theirs, "Polaris");
        assert!(polaris.0 >= ours_seed_count as u32);

        let stats = merge_hypergraphs(&mut ours, &theirs).expect("merge");
        assert!(stats.elements_added >= 1);
        // Polaris is now in ours.
        assert!(ours.by_name.contains_key("Polaris"));
    }

    #[test]
    fn name_match_unifies_post_seed_elements_across_branches() {
        // Both sides independently mint "Polaris". After merge, ours
        // should have exactly one Polaris.
        let mut ours = load_seed_graph();
        add_element(&mut ours, "Polaris");
        let mut theirs = load_seed_graph();
        add_element(&mut theirs, "Polaris");

        let stats = merge_hypergraphs(&mut ours, &theirs).expect("merge");
        let polaris_count = ours
            .elements
            .iter()
            .filter(|e| e.names.first().map(|n| n == "Polaris").unwrap_or(false))
            .count();
        assert_eq!(polaris_count, 1, "duplicate Polaris elements after merge");
        assert_eq!(
            stats.elements_unified, 1,
            "their Polaris should have unified with ours, not appended"
        );
    }

    #[test]
    fn anchor_mismatch_refuses_merge() {
        let mut ours = load_seed_graph();
        let mut theirs = load_seed_graph();
        theirs.genesis = ElementId(theirs.elements.len() as u32 - 1);
        let result = merge_hypergraphs(&mut ours, &theirs);
        match result {
            Err(MergeError::AnchorMismatch {
                which: "genesis", ..
            }) => {}
            other => panic!("expected AnchorMismatch on genesis, got {other:?}"),
        }
    }

    #[test]
    fn relation_remap_rewrites_attributes_to_ours_ids() {
        // Both sides mint "Polaris" and "Rust" independently, plus
        // a "Polaris at Rust" relation. After merge, the relation
        // should use ours's Polaris ID and ours's Rust ID, with the
        // relation deduped (count = 1, not 2).
        let mut ours = load_seed_graph();
        let our_polaris = add_element(&mut ours, "Polaris");
        let our_rust = add_element(&mut ours, "Rust");
        let our_at = ours.by_name["at"][0];
        add_binary_relation(&mut ours, our_polaris, our_at, our_rust);
        let pre_rel_count = ours.relations.len();

        let mut theirs = load_seed_graph();
        let their_polaris = add_element(&mut theirs, "Polaris");
        let their_rust = add_element(&mut theirs, "Rust");
        let their_at = theirs.by_name["at"][0];
        add_binary_relation(&mut theirs, their_polaris, their_at, their_rust);

        let stats = merge_hypergraphs(&mut ours, &theirs).expect("merge");
        assert_eq!(
            ours.relations.len(),
            pre_rel_count,
            "the same (Polaris, at, Rust) relation must dedup, not duplicate"
        );
        assert!(
            stats.relations_merged >= 1,
            "expected at least one relation merged via remap"
        );
    }

    #[test]
    fn superseded_status_wins_over_asserted() {
        // ours says Asserted; theirs says Superseded for the same
        // logical relation. After merge, status should be Superseded
        // (the more-decisive signal).
        let mut ours = load_seed_graph();
        let our_polaris = add_element(&mut ours, "Polaris");
        let our_rust = add_element(&mut ours, "Rust");
        let our_at = ours.by_name["at"][0];
        let ours_rel_id = add_binary_relation(&mut ours, our_polaris, our_at, our_rust);
        ours.relations[ours_rel_id.0 as usize].status = RelationStatus::Asserted;

        let mut theirs = load_seed_graph();
        let their_polaris = add_element(&mut theirs, "Polaris");
        let their_rust = add_element(&mut theirs, "Rust");
        let their_at = theirs.by_name["at"][0];
        let theirs_rel_id = add_binary_relation(&mut theirs, their_polaris, their_at, their_rust);
        theirs.relations[theirs_rel_id.0 as usize].status = RelationStatus::Superseded;

        let _ = merge_hypergraphs(&mut ours, &theirs).expect("merge");
        assert_eq!(
            ours.relations[ours_rel_id.0 as usize].status,
            RelationStatus::Superseded,
            "Superseded must win over Asserted in merge resolution"
        );
    }

    #[test]
    fn merge_stats_max_not_sum() {
        // If both sides saw support_count=3, the merged value should be
        // 3 (not 6) — they saw the same evidence, not independent
        // evidence. Max is the safe default.
        let mut ours = load_seed_graph();
        let our_p = add_element(&mut ours, "Polaris");
        let our_r = add_element(&mut ours, "Rust");
        let our_at = ours.by_name["at"][0];
        let rid = add_binary_relation(&mut ours, our_p, our_at, our_r);
        ours.relations[rid.0 as usize].stats.support_count = 3;

        let mut theirs = load_seed_graph();
        let their_p = add_element(&mut theirs, "Polaris");
        let their_r = add_element(&mut theirs, "Rust");
        let their_at = theirs.by_name["at"][0];
        let trid = add_binary_relation(&mut theirs, their_p, their_at, their_r);
        theirs.relations[trid.0 as usize].stats.support_count = 5;

        let _ = merge_hypergraphs(&mut ours, &theirs).expect("merge");
        assert_eq!(
            ours.relations[rid.0 as usize].stats.support_count, 5,
            "support_count should be max(ours, theirs) = 5"
        );
    }

    #[test]
    fn clock_takes_max() {
        let mut ours = load_seed_graph();
        ours.clock = Tick(10);
        let mut theirs = load_seed_graph();
        theirs.clock = Tick(20);
        let _ = merge_hypergraphs(&mut ours, &theirs).expect("merge");
        assert_eq!(ours.clock.0, 20);
    }

    #[test]
    fn recent_focus_unions_and_dedups() {
        let mut ours = load_seed_graph();
        let e1 = add_element(&mut ours, "Polaris");
        let attr = ours.subject_attr;
        ours.recent_focus.push_front(RecentFocusEntry {
            element: e1,
            attribute: Some(attr),
            frame: None,
            tick: Tick(5),
        });

        let mut theirs = load_seed_graph();
        let te1 = add_element(&mut theirs, "Polaris");
        let tattr = theirs.subject_attr;
        theirs.recent_focus.push_front(RecentFocusEntry {
            element: te1,
            attribute: Some(tattr),
            frame: None,
            tick: Tick(7),
        });

        let _ = merge_hypergraphs(&mut ours, &theirs).expect("merge");
        // Both sides referenced "Polaris" as subject; after remap +
        // dedup the union should have exactly one entry. Whichever
        // tick is higher is kept (we sort tick desc but don't merge
        // duplicates by max-tick — first seen wins after sort).
        let polaris_id = ours.by_name["Polaris"][0];
        let entries: Vec<_> = ours
            .recent_focus
            .iter()
            .filter(|e| e.element == polaris_id)
            .collect();
        assert_eq!(entries.len(), 1, "recent_focus should dedup");
    }
}
