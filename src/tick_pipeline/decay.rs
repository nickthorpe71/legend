//! `focus_radius_decay` — Focus-radius decay.
//!
//! Pure arithmetic over `MemoryStats` — no model. Bounded BFS
//! outward from this tick's focus set, decaying `stats.activation`
//! on each relation reached. Decay is utility-modulated: high-
//! utility relations decay slowly; sub-radius low-utility ones
//! decay fast. The bounded radius keeps the tick's latency budget
//! predictable; everything outside the radius is the background
//! sweep's job (§14.7, post-v0 replay thread).
//!
//! Two invariants:
//! - **Activation only.** `support_count`, `confidence`,
//!   `salience`, `support_diversity` — none of these decay here.
//!   Belief-strength signals only reset via supersession or
//!   replay.
//! - **Status untouched.** Decayed relations stay live; they're
//!   just harder to retrieve via activation-weighted ranking.
//!
//! See `new_foundation.md`.

use std::collections::{HashSet, VecDeque};

use crate::hebbian::bounded_hebbian_decay;
use crate::types::{ElementId, Hypergraph, Policy, Relation, RelationId, Term};

/// Per-tick summary of what `focus_radius_decay` actually did. Counts only —
/// the work is diffuse (a chatty tick can walk hundreds of
/// relations); per-relation introspection lives on the relations
/// themselves.
#[derive(Debug, Default, Clone, Copy)]
pub struct Decay {
    /// Elements reached during the BFS walk (deduped).
    pub elements_walked: u32,
    /// Relations whose `stats.activation` was decayed this tick.
    pub relations_decayed: u32,
    /// Maximum BFS depth actually traversed
    /// (≤ `policy.focus_decay_radius`).
    pub max_depth_reached: u32,
}

/// Compute the raw utility score for a Relation per §14.7 (v0
/// subset). v0 reads only the substrate currently maintained:
/// focus_success_count, support_count, and salience. The
/// `exact_value_bonus` from the full formula is already folded
/// into `r.stats.salience` by `hebbian_and_salience`, so re-adding it here would
/// double-count typed-value relations.
///
/// Replay-maintained terms (`source_quality`, `noise_score`,
/// `redundancy`, `age_without_access`, correction bonus) are
/// deferred to the §14.8 background sweep.
pub fn compute_utility(r: &Relation, _policy: &Policy) -> f32 {
    r.stats.focus_success_count as f32 + r.stats.support_count as f32 + r.stats.salience
}

/// Soft-cap `raw` utility into `[0, 1]` via the sigmoid-style
/// `u / (u + k)` mapping. Smoother than linear clamping; never
/// saturates exactly at 1.0 so the decay rate retains a small
/// positive floor even for high-utility relations.
///
/// With `k = 5.0`:
///   utility=0  →  0.0
///   utility=5  →  0.5
///   utility=10 →  0.67
///   utility=20 →  0.8
pub fn normalize_utility(raw: f32) -> f32 {
    const K: f32 = 5.0;
    if raw <= 0.0 {
        return 0.0;
    }
    raw / (raw + K)
}

/// Compute the actual decay rate applied to `R` this tick:
/// `policy.decay_rate * (1 - normalize(utility))`. High-utility
/// relations get a smaller rate, low-utility ones get the full
/// `policy.decay_rate`.
///
/// Returns 0.0 when `policy.decay_rate == 0` so the entire
/// pipeline collapses to a no-op under default policy.
pub fn effective_decay_rate(r: &Relation, policy: &Policy) -> f32 {
    if policy.decay_rate <= 0.0 {
        return 0.0;
    }
    let u = compute_utility(r, policy);
    let n = normalize_utility(u);
    (policy.decay_rate * (1.0 - n)).clamp(0.0, 1.0)
}

/// Run `focus_radius_decay`. Default policy (`decay_rate == 0` AND/OR
/// `focus_decay_radius == 0`) returns an empty summary without
/// touching the graph.
///
/// BFS expands outward from the seed Element set (every Element-
/// valued attribute of every relation in `reinforced_relations`),
/// up to `policy.focus_decay_radius` hops via `relations_by_element`.
/// Each relation reached at depth ≥ 1 is decayed via
/// `bounded_hebbian_decay(activation, effective_rate)`. Seed
/// elements themselves (depth 0) are NOT decayed — they're the
/// focus-bearing set; `hebbian_and_salience` just bumped their relations.
///
/// Per-relation dedup: once decayed, a relation is not decayed
/// again this tick, even if reached via a second path.
pub fn focus_radius_decay(
    hypergraph: &mut Hypergraph,
    reinforced_relations: &[RelationId],
    policy: &Policy,
) -> Decay {
    if policy.focus_decay_radius == 0 || policy.decay_rate <= 0.0 {
        // No-op gate. Skip building the seed set, skip BFS.
        return Decay::default();
    }

    // 1. Seed Element set: every Element-valued attribute of every
    //    reinforced relation. Term::Relation slots (meta-relation
    //    targets) are skipped — those are relation references, not
    //    elements to walk from.
    let seed: Vec<ElementId> = collect_seed_elements(hypergraph, reinforced_relations);
    if seed.is_empty() {
        return Decay::default();
    }

    // 2. BFS. Visited tracks elements we've enqueued so we don't
    //    revisit. Decayed tracks relations we've already applied
    //    decay to so a relation reachable via two paths only takes
    //    one decay hit per tick.
    let mut visited: HashSet<ElementId> = HashSet::new();
    let mut decayed: HashSet<RelationId> = HashSet::new();
    let mut reinforced_set: HashSet<RelationId> = HashSet::new();
    reinforced_set.extend(reinforced_relations.iter().copied());

    let mut queue: VecDeque<(ElementId, u32)> = VecDeque::new();
    for &e in &seed {
        if visited.insert(e) {
            queue.push_back((e, 0));
        }
    }

    let radius = policy.focus_decay_radius;
    let mut max_depth: u32 = 0;
    let mut relations_decayed: u32 = 0;

    while let Some((elem, depth)) = queue.pop_front() {
        if depth >= radius {
            // Reached the radius; don't expand further. But the
            // current element's relations at depth+1 would still
            // count as in-radius if depth+1 <= radius — which it
            // isn't here, so stop.
            continue;
        }
        let Some(relation_ids) = hypergraph.relations_by_element.get(&elem).cloned() else {
            continue;
        };
        for rid in relation_ids {
            // Skip reinforced relations — `hebbian_and_salience` just bumped them;
            // immediately decaying would partially undo the bump.
            if reinforced_set.contains(&rid) {
                continue;
            }
            // Decay each relation at most once per tick.
            if decayed.insert(rid) {
                let r = &hypergraph.relations[rid.0 as usize];
                let rate = effective_decay_rate(r, policy);
                let r_mut = &mut hypergraph.relations[rid.0 as usize];
                r_mut.stats.activation = bounded_hebbian_decay(r_mut.stats.activation, rate);
                relations_decayed += 1;
                max_depth = max_depth.max(depth + 1);
            }
            // Expand: enqueue each Element-valued attribute target
            // (other than `elem` itself) at depth + 1, capped by
            // radius. Term::Relation values are skipped.
            let next_depth = depth + 1;
            if next_depth <= radius {
                let r = &hypergraph.relations[rid.0 as usize];
                for attr in &r.attributes {
                    if let Term::Element(target) = attr.value
                        && target != elem
                        && visited.insert(target)
                    {
                        queue.push_back((target, next_depth));
                    }
                }
            }
        }
    }

    Decay {
        elements_walked: visited.len() as u32,
        relations_decayed,
        max_depth_reached: max_depth,
    }
}


/// Build the BFS seed Element set: every Element-valued attribute
/// of every relation in `reinforced_relations`. Deduped. Term::
/// Relation values (meta-relation targets) are skipped because
/// those are relation references, not elements.
fn collect_seed_elements(hypergraph: &Hypergraph, reinforced: &[RelationId]) -> Vec<ElementId> {
    let mut seen: HashSet<ElementId> = HashSet::new();
    let mut out: Vec<ElementId> = Vec::new();
    for &rid in reinforced {
        let r = &hypergraph.relations[rid.0 as usize];
        for attr in &r.attributes {
            if let Term::Element(e) = attr.value
                && seen.insert(e)
            {
                out.push(e);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MemoryStats, Relation, RelationStatus, Tick};

    fn synth_relation(focus_success: u32, support_count: u32, salience: f32) -> Relation {
        Relation {
            id: crate::types::RelationId(0),
            attributes: vec![],
            status: RelationStatus::Asserted,
            stats: MemoryStats {
                focus_success_count: focus_success,
                support_count,
                salience,
                ..MemoryStats::default()
            },
            priority: 0,
            created_at: Tick(0),
        }
    }

    #[test]
    fn compute_utility_sums_three_terms() {
        let r = synth_relation(2, 3, 0.5);
        let policy = Policy::default();
        let u = compute_utility(&r, &policy);
        assert!((u - 5.5).abs() < 1e-5, "expected 5.5; got {u}");
    }

    #[test]
    fn compute_utility_handles_zero_stats() {
        let r = synth_relation(0, 0, 0.0);
        let policy = Policy::default();
        assert_eq!(compute_utility(&r, &policy), 0.0);
    }

    #[test]
    fn normalize_utility_zero_at_zero() {
        assert_eq!(normalize_utility(0.0), 0.0);
        assert_eq!(normalize_utility(-1.0), 0.0);
    }

    #[test]
    fn normalize_utility_half_at_k() {
        // K = 5 internally; utility=5 should map to 0.5.
        assert!((normalize_utility(5.0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn normalize_utility_monotonic_and_bounded() {
        let mut last = -1.0;
        for step in 0..=100 {
            let n = normalize_utility(step as f32);
            assert!((0.0..1.0).contains(&n), "out of (0,1): {n}");
            assert!(n >= last, "non-monotonic: {n} < {last}");
            last = n;
        }
    }

    #[test]
    fn effective_decay_rate_zero_when_policy_zero() {
        let r = synth_relation(2, 3, 0.5);
        let policy = Policy {
            decay_rate: 0.0,
            ..Default::default()
        };
        assert_eq!(effective_decay_rate(&r, &policy), 0.0);
    }

    #[test]
    fn effective_decay_rate_high_for_low_utility() {
        let r = synth_relation(0, 0, 0.0);
        let policy = Policy {
            decay_rate: 0.4,
            ..Default::default()
        };
        let r_eff = effective_decay_rate(&r, &policy);
        // utility=0 → normalized=0 → rate = 0.4 * (1 - 0) = 0.4.
        assert!((r_eff - 0.4).abs() < 1e-5);
    }

    #[test]
    fn effective_decay_rate_low_for_high_utility() {
        // High utility → normalized near 1 → rate near 0.
        let r = synth_relation(100, 100, 1.0);
        let policy = Policy {
            decay_rate: 0.4,
            ..Default::default()
        };
        let r_eff = effective_decay_rate(&r, &policy);
        let r_low = effective_decay_rate(&synth_relation(0, 0, 0.0), &policy);
        assert!(
            r_eff < r_low,
            "high-utility rate ({r_eff}) should be smaller than low-utility ({r_low})",
        );
        assert!(
            r_eff < 0.1,
            "high-utility rate should be < 0.1; got {r_eff}"
        );
    }

    #[test]
    fn no_op_when_radius_zero() {
        let mut hypergraph = crate::types::Hypergraph::default();
        let policy = Policy {
            focus_decay_radius: 0,
            decay_rate: 0.3,
            ..Default::default()
        };
        let out = focus_radius_decay(&mut hypergraph, &[], &policy);
        assert_eq!(out.elements_walked, 0);
        assert_eq!(out.relations_decayed, 0);
    }

    // ── Phase 2 tests: BFS + decay behavior ─────────────────────

    use crate::tick_pipeline::build_relations::{mint_element, mint_relation};
    use crate::types::{Attribute, ElementId as ElId, Polarity};

    /// Build a synthetic Hypergraph with N "ring" elements where
    /// each element has one relation pointing to the next element,
    /// plus a "periphery" element reachable from the last ring
    /// element through one more relation. Lets us drive BFS depth
    /// explicitly.
    fn synth_chain(
        n: usize,
    ) -> (
        crate::types::Hypergraph,
        Vec<ElId>,
        Vec<crate::types::RelationId>,
    ) {
        let mut hypergraph = crate::types::Hypergraph::default();
        // Seed structural attribute names at low IDs so chain
        // relations can build [subject, attr] shape.
        let subject_id = mint_element(
            &mut hypergraph,
            vec!["subject".to_string()],
            vec![0.0; crate::embed::EMBEDDING_DIM],
            Polarity::Signal,
            1.0,
        );
        let attr_id = mint_element(
            &mut hypergraph,
            vec!["points_to".to_string()],
            vec![0.0; crate::embed::EMBEDDING_DIM],
            Polarity::Signal,
            1.0,
        );
        hypergraph.subject_attr = subject_id;
        hypergraph.target_attr = attr_id;
        let mut elements: Vec<ElId> = Vec::with_capacity(n);
        for i in 0..n {
            elements.push(mint_element(
                &mut hypergraph,
                vec![format!("e{i}")],
                vec![0.0; crate::embed::EMBEDDING_DIM],
                Polarity::Signal,
                1.0,
            ));
        }
        // Chain: e0 -> e1 -> e2 -> ... -> e_{n-1}, one relation
        // per hop, [subject: e_i, points_to: e_{i+1}].
        let mut relations = Vec::with_capacity(n.saturating_sub(1));
        for i in 0..(n.saturating_sub(1)) {
            let rid = mint_relation(
                &mut hypergraph,
                vec![
                    Attribute {
                        name: subject_id,
                        value: Term::Element(elements[i]),
                    },
                    Attribute {
                        name: attr_id,
                        value: Term::Element(elements[i + 1]),
                    },
                ],
                RelationStatus::Asserted,
                1.0,
            );
            relations.push(rid);
        }
        (hypergraph, elements, relations)
    }

    #[test]
    fn bfs_respects_radius_cap() {
        // 5-element chain: e0 -> e1 -> e2 -> e3 -> e4 via 4 relations.
        // Seed via the FIRST relation (which only references e0 and
        // e1). With radius=1, the seed set is {e0, e1} (visited at
        // depth 0); we expand each at depth 0 → 1, decaying their
        // relations. Crucially, e1 → e2 lives at depth 1 (still in
        // radius); e2 → e3 lives at depth 2 (out of radius); etc.
        let (mut hypergraph, _elements, relations) = synth_chain(5);
        // Pre-set activation on every chain relation so decay shows.
        for &rid in &relations {
            hypergraph.relations[rid.0 as usize].stats.activation = 1.0;
        }
        // Seed via first relation; `hebbian_and_salience` normally provides this.
        let seed_relation = &[relations[0]];
        let policy = Policy {
            focus_decay_radius: 1,
            decay_rate: 0.5,
            ..Default::default()
        };
        let out = focus_radius_decay(&mut hypergraph, seed_relation, &policy);

        // Relation 0 itself is in the reinforced set (seed) and
        // skipped by the "don't decay reinforced" guard. Relation 1
        // (e1 → e2) is reachable at depth 1 from e1 and should decay.
        // Relation 2+ are out of radius from the seed.
        assert!(
            out.relations_decayed >= 1,
            "at least one relation should decay"
        );
        assert_eq!(
            hypergraph.relations[relations[0].0 as usize].stats.activation, 1.0,
            "reinforced relation should NOT be decayed",
        );
        assert!(
            hypergraph.relations[relations[1].0 as usize].stats.activation < 1.0,
            "depth-1 relation should be decayed",
        );
        // Relation 3 is depth-2+; shouldn't decay.
        assert_eq!(
            hypergraph.relations[relations[3].0 as usize].stats.activation, 1.0,
            "out-of-radius relation should stay untouched; got {}",
            hypergraph.relations[relations[3].0 as usize].stats.activation,
        );
    }

    #[test]
    fn radius_zero_skips_walk_entirely() {
        let (mut hypergraph, _elements, relations) = synth_chain(3);
        for &rid in &relations {
            hypergraph.relations[rid.0 as usize].stats.activation = 1.0;
        }
        let policy = Policy {
            focus_decay_radius: 0,
            decay_rate: 0.5,
            ..Default::default()
        };
        let out = focus_radius_decay(&mut hypergraph, &[relations[0]], &policy);
        assert_eq!(out.relations_decayed, 0);
        for &rid in &relations {
            assert_eq!(hypergraph.relations[rid.0 as usize].stats.activation, 1.0);
        }
    }

    #[test]
    fn higher_radius_decays_more_relations() {
        let (mut hypergraph, _elements, relations) = synth_chain(5);
        for &rid in &relations {
            hypergraph.relations[rid.0 as usize].stats.activation = 1.0;
        }
        let policy = Policy {
            focus_decay_radius: 3,
            decay_rate: 0.5,
            ..Default::default()
        };
        let out = focus_radius_decay(&mut hypergraph, &[relations[0]], &policy);
        // With radius 3 we should reach all 4 chain relations
        // (relation 0 is reinforced/skipped).
        assert!(
            out.relations_decayed >= 3,
            "radius=3 over chain should decay ≥ 3 relations; got {}",
            out.relations_decayed,
        );
    }

    #[test]
    fn relation_decayed_at_most_once() {
        // Build a diamond: e0 -> e1, e0 -> e2, e1 -> e3, e2 -> e3.
        // The relation e1->e3 is reachable from e0 only via e1. But
        // we'll seed with TWO different relations that both touch
        // e3 to verify the visited/decayed set prevents double-decay.
        let mut hypergraph = crate::types::Hypergraph::default();
        let subject_id = mint_element(
            &mut hypergraph,
            vec!["subject".to_string()],
            vec![0.0; crate::embed::EMBEDDING_DIM],
            Polarity::Signal,
            1.0,
        );
        let attr_id = mint_element(
            &mut hypergraph,
            vec!["a".to_string()],
            vec![0.0; crate::embed::EMBEDDING_DIM],
            Polarity::Signal,
            1.0,
        );
        hypergraph.subject_attr = subject_id;
        hypergraph.target_attr = attr_id;
        let mk_node = |hypergraph: &mut crate::types::Hypergraph, name: &str| {
            mint_element(
                hypergraph,
                vec![name.to_string()],
                vec![0.0; crate::embed::EMBEDDING_DIM],
                Polarity::Signal,
                1.0,
            )
        };
        let e0 = mk_node(&mut hypergraph, "e0");
        let e1 = mk_node(&mut hypergraph, "e1");
        let e2 = mk_node(&mut hypergraph, "e2");
        let e3 = mk_node(&mut hypergraph, "e3");

        let mk_rel = |hypergraph: &mut crate::types::Hypergraph, s: ElId, t: ElId| {
            mint_relation(
                hypergraph,
                vec![
                    Attribute {
                        name: subject_id,
                        value: Term::Element(s),
                    },
                    Attribute {
                        name: attr_id,
                        value: Term::Element(t),
                    },
                ],
                RelationStatus::Asserted,
                1.0,
            )
        };
        let r01 = mk_rel(&mut hypergraph, e0, e1);
        let r02 = mk_rel(&mut hypergraph, e0, e2);
        let r13 = mk_rel(&mut hypergraph, e1, e3);
        let r23 = mk_rel(&mut hypergraph, e2, e3);

        // Set activation high so decay shows.
        for &rid in &[r01, r02, r13, r23] {
            hypergraph.relations[rid.0 as usize].stats.activation = 1.0;
        }

        // Seed via r01 + r02 (both reinforced — neither decays itself).
        let seed = &[r01, r02];
        let policy = Policy {
            focus_decay_radius: 3,
            decay_rate: 0.5,
            ..Default::default()
        };
        let out = focus_radius_decay(&mut hypergraph, seed, &policy);

        // r13 and r23 each reachable. Both should decay exactly once.
        // (If we re-decayed r13 via e3's incoming edges, activation
        // would drop further.)
        let act_13_after = hypergraph.relations[r13.0 as usize].stats.activation;
        let act_23_after = hypergraph.relations[r23.0 as usize].stats.activation;
        // One bounded_hebbian_decay(1.0, 0.5) = 0.5. Two would = 0.25.
        // Both should land at ~0.5 if dedup works.
        assert!(
            (act_13_after - 0.5).abs() < 1e-4,
            "r13 should decay exactly once; activation={act_13_after} (expected ~0.5)",
        );
        assert!(
            (act_23_after - 0.5).abs() < 1e-4,
            "r23 should decay exactly once; activation={act_23_after} (expected ~0.5)",
        );
        assert_eq!(
            out.relations_decayed, 2,
            "exactly 2 relations should decay (r13 + r23); got {}",
            out.relations_decayed,
        );
    }

    #[test]
    fn high_utility_relation_barely_decays() {
        let (mut hypergraph, _elements, relations) = synth_chain(3);
        // Set the peripheral relation's activation HIGH and stats
        // HIGH so utility is very large → effective rate near 0.
        let periphery = relations[1];
        hypergraph.relations[periphery.0 as usize].stats.activation = 1.0;
        hypergraph.relations[periphery.0 as usize].stats.focus_success_count = 100;
        hypergraph.relations[periphery.0 as usize].stats.support_count = 100;
        hypergraph.relations[periphery.0 as usize].stats.salience = 1.0;
        // The seed relation (relations[0]) keeps default zero stats
        // but won't be decayed (it's reinforced).
        let policy = Policy {
            focus_decay_radius: 2,
            decay_rate: 0.5,
            ..Default::default()
        };
        let _ = focus_radius_decay(&mut hypergraph, &[relations[0]], &policy);
        let act = hypergraph.relations[periphery.0 as usize].stats.activation;
        assert!(
            act > 0.95,
            "high-utility periphery should barely decay; got {act}",
        );
    }

    #[test]
    fn status_and_support_count_untouched_by_decay() {
        let (mut hypergraph, _elements, relations) = synth_chain(3);
        for &rid in &relations {
            hypergraph.relations[rid.0 as usize].stats.activation = 1.0;
            hypergraph.relations[rid.0 as usize].stats.support_count = 5;
        }
        let policy = Policy {
            focus_decay_radius: 2,
            decay_rate: 0.5,
            ..Default::default()
        };
        let _ = focus_radius_decay(&mut hypergraph, &[relations[0]], &policy);
        // Spec invariant: only activation decays. Support count
        // and status must stay put.
        for &rid in &relations {
            let r = &hypergraph.relations[rid.0 as usize];
            assert_eq!(r.stats.support_count, 5, "support_count must not decay");
            assert_eq!(r.status, RelationStatus::Asserted, "status must not change");
        }
    }

    // ── Phase 4: integration over the real pipeline ─────────────

    /// Drive the tick pipeline over a real sentence with a
    /// non-default policy that actually exercises decay. Tests that:
    /// - reinforced relations from `hebbian_and_salience` are NOT decayed
    /// - non-reinforced periphery relations DO decay
    /// - status + support_count stay untouched (spec invariant)
    /// - max_depth_reached <= focus_decay_radius
    #[test]
    fn integration_real_sentence_decays_periphery_not_focus() {
        use crate::seed::load_seed_graph;
        use crate::tick_pipeline::build_relations::build_relations;
        use crate::tick_pipeline::hebbian::hebbian_and_salience;
        use crate::tick_pipeline::run_extractors::run_extractors;
        use crate::tick_pipeline::supersede::supersede;

        let policy = Policy {
            focus_decay_radius: 2,
            decay_rate: 0.4,
            hebbian_rate: 0.6, // non-zero so `hebbian_and_salience` actually bumps
            ..Default::default()
        };
        let labels: &[&str] = &["event", "weekday"];
        let mut hypergraph = load_seed_graph();

        // Tick 1: mint baseline + bump activation on reinforced relations.
        let text1 = "The meeting moved from Monday to Tuesday.";
        let ext1 = run_extractors(text1, labels, &policy, &hypergraph, &[]);
        let built_relations_1 = build_relations(text1, &mut hypergraph, &ext1, &policy, None);
        let superseded_1 = supersede(&mut hypergraph, &built_relations_1.minted_relations, &policy);
        let reinforced_1 = hebbian_and_salience(&mut hypergraph, &built_relations_1, &superseded_1, None, &policy, &[]);

        // Snapshot every reinforced relation's activation before decay.
        let reinforced_before: Vec<(RelationId, f32)> = reinforced_1
            .reinforced
            .iter()
            .map(|&rid| (rid, hypergraph.relations[rid.0 as usize].stats.activation))
            .collect();
        // Snapshot every reinforced relation's support_count and status.
        let support_status_before: Vec<(RelationId, u32, RelationStatus)> = reinforced_1
            .reinforced
            .iter()
            .map(|&rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                (rid, r.stats.support_count, r.status)
            })
            .collect();

        // `focus_radius_decay`.
        let out = focus_radius_decay(&mut hypergraph, &reinforced_1.reinforced, &policy);

        assert!(
            out.elements_walked > 0,
            "BFS should walk at least the seed elements",
        );
        assert!(
            out.max_depth_reached <= policy.focus_decay_radius,
            "max depth must respect radius cap; got {} > {}",
            out.max_depth_reached,
            policy.focus_decay_radius,
        );

        // Invariant: reinforced relations' activation unchanged.
        for (rid, before) in &reinforced_before {
            let after = hypergraph.relations[rid.0 as usize].stats.activation;
            assert!(
                (after - before).abs() < 1e-5,
                "reinforced relation {rid:?} activation drifted: {before} → {after}",
            );
        }
        // Invariant: support_count and status untouched everywhere.
        for (rid, sup_before, status_before) in &support_status_before {
            let r = &hypergraph.relations[rid.0 as usize];
            assert_eq!(
                r.stats.support_count, *sup_before,
                "support_count for {rid:?} drifted: {sup_before} → {}",
                r.stats.support_count,
            );
            assert_eq!(
                r.status, *status_before,
                "status for {rid:?} drifted: {status_before:?} → {:?}",
                r.status,
            );
        }
    }

    #[test]
    fn integration_no_op_under_default_policy() {
        // The full pipeline under default policy should produce a
        // zero-count Decay and leave activation untouched
        // across the substrate.
        use crate::seed::load_seed_graph;
        use crate::tick_pipeline::build_relations::build_relations;
        use crate::tick_pipeline::hebbian::hebbian_and_salience;
        use crate::tick_pipeline::run_extractors::run_extractors;
        use crate::tick_pipeline::supersede::supersede;

        let policy = Policy::default();
        let mut hypergraph = load_seed_graph();
        let text = "Sarah called me yesterday.";
        let ext = run_extractors(text, &[], &policy, &hypergraph, &[]);
        let built_relations = build_relations(text, &mut hypergraph, &ext, &policy, None);
        let superseded = supersede(&mut hypergraph, &built_relations.minted_relations, &policy);
        let reinforced = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        let out = focus_radius_decay(&mut hypergraph, &reinforced.reinforced, &policy);
        assert_eq!(out.elements_walked, 0);
        assert_eq!(out.relations_decayed, 0);
    }

    #[test]
    fn no_op_when_rate_zero() {
        let mut hypergraph = crate::types::Hypergraph::default();
        let policy = Policy {
            focus_decay_radius: 3,
            decay_rate: 0.0,
            ..Default::default()
        };
        let out = focus_radius_decay(&mut hypergraph, &[], &policy);
        assert_eq!(out.elements_walked, 0);
        assert_eq!(out.relations_decayed, 0);
    }
}
