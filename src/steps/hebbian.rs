//! Step 10 — Hebbian + Salience.
//!
//! Pure arithmetic over `MemoryStats` — no model. Three pieces of
//! substrate maintenance plus one side-effectful write to
//! `Hypergraph.recent_focus` that unblocks Step 6 (coref) and Step
//! 4's frame inheritance:
//!
//! 1. Hebbian activation bump (bounded Oja rule) on every Relation
//!    produced or reinforced this tick.
//! 2. Salience computation + bump (intent-modulated).
//! 3. `support_count` increment + Defeasible → Asserted promotion
//!    when the gate clears.
//! 4. Push focal `RecentFocusEntry` records onto `recent_focus`.
//!
//! See `step_10_design.md` for the full spec; this is phase 2
//! (skeleton + activation bumps). Salience, promotion, and
//! `recent_focus` push land in phases 3-5.
//!
//! With v0 default `policy.hebbian_rate = 0.0`, every bump is a
//! mathematical no-op — `support_count` still increments and
//! `recent_focus` still populates, but `stats.activation` /
//! `stats.salience` stay put. Same gating story as Step 7's drift.

use std::collections::HashSet;

use crate::hebbian::bounded_hebbian_bump;
use crate::steps::build_relations::Step8Output;
use crate::steps::supersede::Step9Output;
use crate::types::{Hypergraph, Policy, RelationId};

/// Per-tick summary of what Step 10 actually did. Surfaces in the
/// debug print and (for `promoted`) in the frame's downstream
/// observability hooks.
#[derive(Debug, Default, Clone)]
pub struct Step10Output {
    /// Relations whose `stats.activation` was bumped this tick via
    /// the bounded Oja rule. With default policy this is a no-op
    /// list — the IDs are recorded but the math is a noop.
    pub reinforced: Vec<RelationId>,
    /// Relations that promoted from `Defeasible` to `Asserted`
    /// this tick. Populated in phase 4.
    pub promoted: Vec<RelationId>,
    /// `RecentFocusEntry` records pushed to `hg.recent_focus`.
    /// Populated in phase 5.
    pub focus_pushed: u32,
}

/// Run Step 10 over the relations Steps 8 and 9 minted this tick.
///
/// Reinforcement set =
/// `step8.minted_relations` ∪ `step9.cache_relations` ∪
/// `step9.meta_relations` minus `step9.superseded`. The superseded
/// set is excluded because reinforcing a relation we just flipped
/// to `Superseded` is paradoxical — supersession says "this is no
/// longer current state."
///
/// For each relation in the reinforcement set: bump
/// `stats.activation` via `bounded_hebbian_bump(activation,
/// policy.hebbian_rate)`. With default policy this is a no-op.
///
/// Phase 3-5 will add salience, promotion, and recent_focus
/// population at the same entry point.
pub fn hebbian_and_salience(
    hg: &mut Hypergraph,
    step8: &Step8Output,
    step9: &Step9Output,
    policy: &Policy,
) -> Step10Output {
    let mut out = Step10Output::default();

    let reinforcement_set = build_reinforcement_set(step8, step9);

    for &rid in &reinforcement_set {
        let r = &mut hg.relations[rid.0 as usize];
        r.stats.activation = bounded_hebbian_bump(r.stats.activation, policy.hebbian_rate);
    }
    out.reinforced.extend(reinforcement_set.iter().copied());

    out
}

/// Construct the per-tick reinforcement set. Deduplicates via a
/// `HashSet` so meta-relations that appear in both step8 and step9
/// (unlikely but defensive) don't get double-counted.
fn build_reinforcement_set(step8: &Step8Output, step9: &Step9Output) -> Vec<RelationId> {
    let superseded: HashSet<RelationId> = step9.superseded.iter().copied().collect();
    let mut seen: HashSet<RelationId> = HashSet::new();
    let mut out = Vec::new();
    for &rid in step8
        .minted_relations
        .iter()
        .chain(step9.cache_relations.iter())
        .chain(step9.meta_relations.iter())
    {
        if superseded.contains(&rid) {
            continue;
        }
        if seen.insert(rid) {
            out.push(rid);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;
    use crate::steps::build_relations::build_relations;
    use crate::steps::run_extractors::run_extractors;
    use crate::steps::supersede::supersede;

    /// Run Steps 5/8/9 over `text` to produce realistic Step8 + Step9
    /// outputs; return the Hypergraph + both outputs so Step 10 tests
    /// can drive `hebbian_and_salience` over a real reinforcement set.
    fn run_through_step9(
        text: &str,
        labels: &[&str],
        policy: &Policy,
    ) -> (Hypergraph, Step8Output, Step9Output) {
        let mut hg = load_seed_graph();
        let ext = run_extractors(text, labels, policy, &hg, &[]);
        let step8 = build_relations(text, &mut hg, &ext, policy, None);
        let step9 = supersede(&mut hg, &step8.minted_relations, policy);
        (hg, step8, step9)
    }

    #[test]
    fn reinforcement_set_includes_step8_minted() {
        let policy = Policy::default();
        let (mut hg, step8, step9) = run_through_step9("Sarah called me yesterday.", &[], &policy);
        let out = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        assert!(!out.reinforced.is_empty());
        for &rid in &step8.minted_relations {
            assert!(out.reinforced.contains(&rid));
        }
    }

    #[test]
    fn reinforcement_set_includes_step9_caches_and_metas() {
        let policy = Policy::default();
        let (mut hg, step8, step9) = run_through_step9(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        assert!(!step9.cache_relations.is_empty(), "test prerequisite");
        let out = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        for &rid in &step9.cache_relations {
            assert!(out.reinforced.contains(&rid));
        }
        for &rid in &step9.meta_relations {
            assert!(out.reinforced.contains(&rid));
        }
    }

    #[test]
    fn reinforcement_excludes_superseded() {
        // Two ticks: the second's events superseded the first's caches.
        // Step 10 on tick 2 must NOT include the superseded priors.
        let policy = Policy::default();
        let labels: &[&str] = &["event", "weekday"];
        let (mut hg, step8_1, step9_1) =
            run_through_step9_with("The meeting moved from Monday to Tuesday.", labels, &policy);
        let _ = hebbian_and_salience(&mut hg, &step8_1, &step9_1, &policy);

        let text2 = "The meeting moved from Tuesday to Friday.";
        let ext2 = run_extractors(text2, labels, &policy, &hg, &[]);
        let step8_2 = build_relations(text2, &mut hg, &ext2, &policy, None);
        let step9_2 = supersede(&mut hg, &step8_2.minted_relations, &policy);
        assert!(
            !step9_2.superseded.is_empty(),
            "test prerequisite: tick 2 should flip the prior",
        );
        let out2 = hebbian_and_salience(&mut hg, &step8_2, &step9_2, &policy);
        for &flipped in &step9_2.superseded {
            assert!(
                !out2.reinforced.contains(&flipped),
                "superseded relation {flipped:?} should not be reinforced",
            );
        }
    }

    fn run_through_step9_with(
        text: &str,
        labels: &[&str],
        policy: &Policy,
    ) -> (Hypergraph, Step8Output, Step9Output) {
        let mut hg = load_seed_graph();
        let ext = run_extractors(text, labels, policy, &hg, &[]);
        let step8 = build_relations(text, &mut hg, &ext, policy, None);
        let step9 = supersede(&mut hg, &step8.minted_relations, policy);
        (hg, step8, step9)
    }

    #[test]
    fn default_policy_keeps_activation_at_zero() {
        // hebbian_rate = 0 → activation should stay at 0 (the
        // MemoryStats default) even though the relation IDs land in
        // the reinforced list.
        let policy = Policy::default();
        let (mut hg, step8, step9) = run_through_step9("Sarah called me yesterday.", &[], &policy);
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        for &rid in &step8.minted_relations {
            assert_eq!(
                hg.relations[rid.0 as usize].stats.activation, 0.0,
                "default policy should leave activation untouched",
            );
        }
    }

    #[test]
    fn nonzero_rate_bumps_activation_toward_one() {
        let policy = Policy {
            hebbian_rate: 0.5,
            ..Default::default()
        };
        let (mut hg, step8, step9) = run_through_step9("Sarah called me yesterday.", &[], &policy);

        // Take a baseline.
        let pick = step8.minted_relations[0];
        let before = hg.relations[pick.0 as usize].stats.activation;
        assert_eq!(before, 0.0, "freshly minted relations start at 0");

        // First bump.
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        let after_one = hg.relations[pick.0 as usize].stats.activation;
        assert!(
            (after_one - 0.5).abs() < 1e-5,
            "bump(0, 0.5) = 0.5; got {after_one}"
        );

        // Second bump: bump(0.5, 0.5) = 0.75.
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        let after_two = hg.relations[pick.0 as usize].stats.activation;
        assert!(
            (after_two - 0.75).abs() < 1e-5,
            "bump(0.5, 0.5) = 0.75; got {after_two}"
        );
    }

    #[test]
    fn activation_caps_at_one() {
        let policy = Policy {
            hebbian_rate: 0.9,
            ..Default::default()
        };
        let (mut hg, step8, step9) = run_through_step9("Sarah called me yesterday.", &[], &policy);
        // 50 bumps at rate 0.9 from 0.0: x_{n+1} = x + 0.9 * (1 - x)
        // converges very fast. After ~5 iterations we're > 0.999.
        for _ in 0..50 {
            let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        }
        for &rid in &step8.minted_relations {
            let a = hg.relations[rid.0 as usize].stats.activation;
            assert!((0.0..=1.0).contains(&a), "activation out of range: {a}");
            assert!(a > 0.99, "expected near 1.0 after 50 bumps; got {a}");
        }
    }
}
