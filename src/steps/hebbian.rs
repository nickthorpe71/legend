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
use crate::steps::build_relations::{Step8Output, kind_of};
use crate::steps::supersede::Step9Output;
use crate::types::{Hypergraph, Policy, RelationId, RelationStatus, Term};

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
    let supersession_derived: HashSet<RelationId> = step9
        .cache_relations
        .iter()
        .chain(step9.meta_relations.iter())
        .copied()
        .collect();

    for &rid in &reinforcement_set {
        // Activation bump first — only touches stats.activation.
        let r = &mut hg.relations[rid.0 as usize];
        r.stats.activation = bounded_hebbian_bump(r.stats.activation, policy.hebbian_rate);

        // Salience score per §11.11. Each component contributes
        // additively; the sum is multiplied by salience_multiplier
        // (intent-modulated by Step 2) and folded into salience via
        // the same bounded Oja bump.
        let score = compute_salience_score(hg, rid, &supersession_derived, policy);
        let bump = score * policy.salience_multiplier;
        let r = &mut hg.relations[rid.0 as usize];
        r.stats.salience = bounded_hebbian_bump(
            r.stats.salience,
            (bump * policy.hebbian_rate).clamp(0.0, 1.0),
        );

        // support_count bumps every tick the relation is reinforced.
        // Per §11.11, support_diversity tracks topologically
        // independent sources — replay's job; stays at 0 for v0.
        r.stats.support_count = r.stats.support_count.saturating_add(1);
    }
    out.reinforced.extend(reinforcement_set.iter().copied());

    // Defeasible → Asserted promotion. Walk only the reinforcement
    // set; full-graph sweeps belong to replay (§14.8).
    for &rid in &reinforcement_set {
        if hg.relations[rid.0 as usize].status != RelationStatus::Defeasible {
            continue;
        }
        if check_promotion(hg, rid, policy) {
            let r = &mut hg.relations[rid.0 as usize];
            r.status = RelationStatus::Asserted;
            // Lift confidence so downstream queries see the higher
            // belief strength. Don't drop below the existing value.
            r.stats.confidence = r.stats.confidence.max(policy.default_conf).clamp(0.0, 1.0);
            out.promoted.push(rid);
        }
    }

    out
}

/// Returns `true` iff Defeasible relation `R` clears all three
/// promotion gates per §11.11:
///
/// 1. `support_count >= policy.promotion_min_count`
/// 2. `support_diversity >= policy.promotion_min_diversity`
///    (v0 leaves this at 0 — replay's job to maintain)
/// 3. No live `supersedes` meta-relation points at R
///    (`meta_relations_by_object[R]` filtered to entries whose
///    attribute list contains `supersedes`)
fn check_promotion(hg: &Hypergraph, rid: RelationId, policy: &Policy) -> bool {
    let r = &hg.relations[rid.0 as usize];
    if r.stats.support_count < policy.promotion_min_count {
        return false;
    }
    if r.stats.support_diversity < policy.promotion_min_diversity {
        return false;
    }
    if has_live_supersedes_meta(hg, rid) {
        return false;
    }
    true
}

fn has_live_supersedes_meta(hg: &Hypergraph, rid: RelationId) -> bool {
    let Some(supersedes_attr) = hg
        .by_name
        .get("supersedes")
        .and_then(|v| v.first().copied())
    else {
        return false;
    };
    let Some(metas) = hg.meta_relations_by_object.get(&rid) else {
        return false;
    };
    for &mid in metas {
        let m = &hg.relations[mid.0 as usize];
        // Skip metas that have themselves been retracted.
        if matches!(m.status, RelationStatus::Retracted) {
            continue;
        }
        if m.attributes.iter().any(|a| a.name == supersedes_attr) {
            return true;
        }
    }
    false
}

/// Salience score per §11.11. Components stack additively; v0 uses
/// the §11.11 weights plus a `salience_floor` baseline from policy.
///
/// Returns a non-negative score; the caller multiplies by
/// `salience_multiplier` (intent-modulated) and uses
/// `bounded_hebbian_bump` to apply.
fn compute_salience_score(
    hg: &Hypergraph,
    rid: RelationId,
    supersession_derived: &HashSet<RelationId>,
    policy: &Policy,
) -> f32 {
    let mut score = policy.salience_floor;

    // +1.0 if any Element-valued attribute references a typed leaf
    // value (date / number / named-entity kind via instance_of). The
    // detection reuses kind_of from build_relations.
    if relation_has_exact_value_attribute(hg, rid) {
        score += 1.0;
    }

    // +1.0 if R was just produced by supersession.
    if supersession_derived.contains(&rid) {
        score += 1.0;
    }

    // +0.5 if R is in the reinforcement set (focus-bearing this tick).
    // True for every R reaching this function, so unconditional.
    score += 0.5;

    score
}

/// True iff `R` carries at least one Element-valued attribute whose
/// value Element has an `instance_of` relation pointing at a kind
/// label like `weekday`, `month`, `time`, `quantity`, `person`,
/// `place`, or `org`. Reuses Step 8's `kind_of` walker.
fn relation_has_exact_value_attribute(hg: &Hypergraph, rid: RelationId) -> bool {
    const EXACT_KINDS: &[&str] = &[
        "weekday", "month", "time", "quantity", "person", "place", "org",
    ];
    let r = &hg.relations[rid.0 as usize];
    for attr in &r.attributes {
        if let Term::Element(e) = attr.value
            && let Some(kind) = kind_of(hg, e)
            && EXACT_KINDS.contains(&kind.as_str())
        {
            return true;
        }
    }
    false
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
    fn salience_bumps_on_typed_value_relation() {
        // NER tags `Tuesday` with kind=weekday, so the
        // [subject: Tuesday, instance_of: weekday] relation should
        // count as an exact-value attribute hit and earn the +1.0
        // bonus on top of the focus-bearing +0.5 baseline.
        let policy = Policy {
            hebbian_rate: 0.5,
            ..Default::default()
        };
        let (mut hg, step8, step9) = run_through_step9(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );

        // Find the instance_of relation for Tuesday — it should have
        // an exact-value attribute (its object is the `weekday` kind
        // element). Snapshot salience before/after.
        let weekday_id = hg.by_name["weekday"][0];
        let instance_of_attr = hg.by_name["instance_of"][0];
        let inst_rel = step8
            .minted_relations
            .iter()
            .copied()
            .find(|rid| {
                let r = &hg.relations[rid.0 as usize];
                r.attributes.iter().any(|a| {
                    a.name == instance_of_attr
                        && matches!(a.value, Term::Element(e) if e == weekday_id)
                })
            })
            .expect("expected at least one instance_of weekday relation");

        let before = hg.relations[inst_rel.0 as usize].stats.salience;
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        let after = hg.relations[inst_rel.0 as usize].stats.salience;
        assert!(
            after > before,
            "salience should bump for an exact-value relation; before={before} after={after}",
        );
    }

    #[test]
    fn salience_higher_for_supersession_derived() {
        // Cache + linking metas from Step 9 should get the +1.0
        // supersession-derived bonus on top of focus-bearing — total
        // ≥ 1.5 (before multiplier + floor). With salience_multiplier
        // = 1.0 and hebbian_rate = 0.5, salience after one bump =
        // bump(0, 0.5 * (0.0 + 1.5)) = bump(0, 0.75) which clamps to 0.75.
        let policy = Policy {
            hebbian_rate: 0.5,
            ..Default::default()
        };
        let (mut hg, step8, step9) = run_through_step9(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        assert!(!step9.cache_relations.is_empty(), "test prerequisite");
        let cache = step9.cache_relations[0];
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        let cache_sal = hg.relations[cache.0 as usize].stats.salience;
        assert!(
            cache_sal > 0.5,
            "cache (supersession-derived) salience should clear 0.5; got {cache_sal}",
        );
    }

    #[test]
    fn support_count_increments_per_reinforcement() {
        let policy = Policy::default();
        let (mut hg, step8, step9) = run_through_step9("Sarah called me yesterday.", &[], &policy);

        let pick = step8.minted_relations[0];
        // support_count starts at 0; bumps by 1 per Step 10 call.
        assert_eq!(hg.relations[pick.0 as usize].stats.support_count, 0);
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        assert_eq!(hg.relations[pick.0 as usize].stats.support_count, 1);
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        assert_eq!(hg.relations[pick.0 as usize].stats.support_count, 2);
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        assert_eq!(hg.relations[pick.0 as usize].stats.support_count, 3);
    }

    #[test]
    fn defeasible_promotes_when_gate_clears() {
        // Custom policy: min_count=3, min_diversity=0 (so v0's
        // 0-by-design diversity counter doesn't block).
        let policy = Policy {
            promotion_min_count: 3,
            promotion_min_diversity: 0,
            ..Default::default()
        };

        // Use a sentence whose NER score on `meeting` is below the
        // assertion threshold → instance_of relation lands Defeasible.
        let (mut hg, step8, step9) = run_through_step9(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );

        // Find a Defeasible instance_of relation to track.
        let instance_of_attr = hg.by_name["instance_of"][0];
        let target_rid = step8
            .minted_relations
            .iter()
            .copied()
            .find(|rid| {
                let r = &hg.relations[rid.0 as usize];
                r.status == RelationStatus::Defeasible
                    && r.attributes.iter().any(|a| a.name == instance_of_attr)
            })
            .expect("expected at least one Defeasible instance_of");

        // First two reinforcements: support_count climbs to 2; gate
        // not yet cleared.
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        assert_eq!(
            hg.relations[target_rid.0 as usize].status,
            RelationStatus::Defeasible,
            "not promoted yet at support_count=2",
        );

        // Third reinforcement clears support_count >= 3.
        let out = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        assert_eq!(
            hg.relations[target_rid.0 as usize].status,
            RelationStatus::Asserted,
            "expected promotion at support_count=3",
        );
        assert!(
            out.promoted.contains(&target_rid),
            "Step10Output.promoted must list the promoted relation",
        );
    }

    #[test]
    fn promotion_blocked_by_low_support_count() {
        let policy = Policy {
            promotion_min_count: 5,
            promotion_min_diversity: 0,
            ..Default::default()
        };
        let (mut hg, step8, step9) = run_through_step9(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        let out = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        // After one tick, support_count == 1; min is 5; nothing
        // should have promoted.
        assert!(
            out.promoted.is_empty(),
            "no promotion expected with support_count=1 and min=5",
        );
    }

    #[test]
    fn promotion_blocked_by_default_min_diversity() {
        // Default policy has min_diversity=2 and v0 leaves diversity
        // at 0 forever, so nothing ever promotes under defaults.
        let policy = Policy {
            promotion_min_count: 1, // even trivially low won't help
            ..Default::default()
        };
        let (mut hg, step8, step9) = run_through_step9(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        let out = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        assert!(
            out.promoted.is_empty(),
            "min_diversity=2 with diversity=0 should block promotion",
        );
    }

    #[test]
    fn promotion_blocked_by_supersedes_meta() {
        // Two-tick fixture: tick 1 mints a Defeasible cache-attribute
        // relation; tick 2 supersedes it. Step 10 on tick 2 must NOT
        // promote the flipped-prior cache even if support_count clears.
        // (The flipped prior is excluded from reinforcement, so this
        // is really a test of the supersedes-meta gate via secondary
        // visibility — the cache that *was* superseded has a
        // meta_relations_by_object entry containing a supersedes meta.)
        let policy = Policy {
            promotion_min_count: 1,
            promotion_min_diversity: 0,
            ..Default::default()
        };
        let labels: &[&str] = &["event", "weekday"];

        let mut hg = load_seed_graph();
        let ext1 = run_extractors(
            "The meeting moved from Monday to Tuesday.",
            labels,
            &policy,
            &hg,
            &[],
        );
        let step8_1 = build_relations(
            "The meeting moved from Monday to Tuesday.",
            &mut hg,
            &ext1,
            &policy,
            None,
        );
        let step9_1 = supersede(&mut hg, &step8_1.minted_relations, &policy);
        let _ = hebbian_and_salience(&mut hg, &step8_1, &step9_1, &policy);

        // Tick 2 supersedes tick 1's cache.
        let ext2 = run_extractors(
            "The meeting moved from Tuesday to Friday.",
            labels,
            &policy,
            &hg,
            &[],
        );
        let step8_2 = build_relations(
            "The meeting moved from Tuesday to Friday.",
            &mut hg,
            &ext2,
            &policy,
            None,
        );
        let step9_2 = supersede(&mut hg, &step8_2.minted_relations, &policy);
        assert!(!step9_2.superseded.is_empty());
        let prior = step9_2.superseded[0];

        // Force the prior into Defeasible just for this test — we
        // want to verify the supersedes-meta gate, not the status
        // bookkeeping the prior already underwent.
        hg.relations[prior.0 as usize].status = RelationStatus::Defeasible;

        // Manually craft a reinforcement set containing the prior so
        // promotion would be considered. Step10's normal path
        // excludes superseded; this synthetic call exercises the
        // gate logic only.
        let mut synthetic_step9 = step9_2.clone();
        synthetic_step9.superseded.clear();
        synthetic_step9.cache_relations.push(prior); // include in reinforcement set
        let out = hebbian_and_salience(&mut hg, &step8_2, &synthetic_step9, &policy);

        assert!(
            !out.promoted.contains(&prior),
            "promotion must be blocked by an existing supersedes meta",
        );
    }

    #[test]
    fn promotion_bumps_confidence_to_default_conf() {
        let policy = Policy {
            promotion_min_count: 1,
            promotion_min_diversity: 0,
            default_conf: 0.9,
            ..Default::default()
        };
        let (mut hg, step8, step9) = run_through_step9(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        let instance_of_attr = hg.by_name["instance_of"][0];
        let target_rid = step8
            .minted_relations
            .iter()
            .copied()
            .find(|rid| {
                let r = &hg.relations[rid.0 as usize];
                r.status == RelationStatus::Defeasible
                    && r.attributes.iter().any(|a| a.name == instance_of_attr)
            })
            .expect("expected at least one Defeasible instance_of");
        let conf_before = hg.relations[target_rid.0 as usize].stats.confidence;
        assert!(conf_before < 0.9);

        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        assert_eq!(
            hg.relations[target_rid.0 as usize].status,
            RelationStatus::Asserted,
        );
        assert!(
            (hg.relations[target_rid.0 as usize].stats.confidence - 0.9).abs() < 1e-5,
            "promotion should lift confidence to default_conf",
        );
    }

    #[test]
    fn salience_stays_at_zero_with_default_policy() {
        let policy = Policy::default();
        let (mut hg, step8, step9) = run_through_step9("Sarah called me yesterday.", &[], &policy);
        let _ = hebbian_and_salience(&mut hg, &step8, &step9, &policy);
        for &rid in &step8.minted_relations {
            assert_eq!(
                hg.relations[rid.0 as usize].stats.salience, 0.0,
                "default rate=0 → salience untouched",
            );
        }
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
