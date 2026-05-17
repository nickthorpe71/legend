//! Step 11 — Focus-radius decay.
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
//! See `step_11_design.md` for the full spec; this is phase 1
//! (skeleton + utility helpers). Phases 2-4 add the BFS walk
//! + wiring + integration tests.

use crate::types::{Policy, Relation, RelationId};

/// Per-tick summary of what Step 11 actually did. Counts only —
/// the work is diffuse (a chatty tick can walk hundreds of
/// relations); per-relation introspection lives on the relations
/// themselves.
#[derive(Debug, Default, Clone, Copy)]
pub struct Step11Output {
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
/// into `r.stats.salience` by Step 10, so re-adding it here would
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

/// Run Step 11. Default policy (`decay_rate == 0` AND/OR
/// `focus_decay_radius == 0`) returns an empty summary without
/// touching the graph.
///
/// Phase 1 (this commit) ships only the early-return path;
/// phase 2 wires the BFS walk over Step 8/9/10 outputs.
pub fn focus_radius_decay(
    _hg: &mut crate::types::Hypergraph,
    _reinforced_relations: &[RelationId],
    policy: &Policy,
) -> Step11Output {
    if policy.focus_decay_radius == 0 || policy.decay_rate <= 0.0 {
        // No-op gate. Skip building the seed set, skip BFS.
        return Step11Output::default();
    }
    // Phase 2 lands the BFS body.
    Step11Output::default()
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
        let mut hg = crate::types::Hypergraph::default();
        let policy = Policy {
            focus_decay_radius: 0,
            decay_rate: 0.3,
            ..Default::default()
        };
        let out = focus_radius_decay(&mut hg, &[], &policy);
        assert_eq!(out.elements_walked, 0);
        assert_eq!(out.relations_decayed, 0);
    }

    #[test]
    fn no_op_when_rate_zero() {
        let mut hg = crate::types::Hypergraph::default();
        let policy = Policy {
            focus_decay_radius: 3,
            decay_rate: 0.0,
            ..Default::default()
        };
        let out = focus_radius_decay(&mut hg, &[], &policy);
        assert_eq!(out.elements_walked, 0);
        assert_eq!(out.relations_decayed, 0);
    }
}
