//! Step 12 — Assemble Attention Frame.
//!
//! No model. The frame is a post-tick snapshot of the focused
//! subgraph; the calling LLM derives any natural-language response
//! off `focused_relations` + `supporting_claims` + `history`.
//!
//! Step 12 itself does two things:
//! 1. RRF-merges already-computed signals into `focused_relations`.
//! 2. Maps uncertainty signals to `next_actions`.
//!
//! Everything else gathers from per-tick buffers earlier steps
//! populated as a side effect of their own work.
//!
//! See `step_12_design.md` for the spec; this is phase 2 (RRF
//! helper) — phase 3 fills in the assembly body.

use crate::types::RelationId;

/// Reciprocal Rank Fusion constant per Cormack, Clarke & Buettcher,
/// SIGIR 2009. Their paper-recommended default. Larger `k` softens
/// the contribution of rank-1 items; we keep it at 60 because the
/// signals Step 12 fuses (dense activation + path-reinforced focus
/// count) live on different value distributions and RRF's whole
/// point is to be robust to that.
pub const RRF_K: u32 = 60;

/// Merge `ranked_lists` via Reciprocal Rank Fusion. Each inner Vec
/// must already be ordered best-first (rank 0 = best). For each
/// item appearing in any list, the fused score is
/// `Σ 1 / (k + 1 + rank_i)` (ranks are 0-indexed in the input but
/// the standard 1-indexed `1/(k + rank_1based)` is what RRF actually
/// computes, so we adjust by `+1`). Returns items in descending
/// fused-score order.
///
/// Items not in a given list contribute nothing from that list —
/// missing-rank-term = 0. RRF degrades gracefully: with one input
/// list it just sorts by `1/(k+1+rank)`, equivalent to the input
/// order.
///
/// Duplicates within a single list are ignored (the first
/// occurrence wins for rank).
pub fn rrf_merge(ranked_lists: &[Vec<RelationId>], k: u32) -> Vec<(RelationId, f32)> {
    use std::collections::HashMap;
    let mut scores: HashMap<RelationId, f32> = HashMap::new();
    for list in ranked_lists {
        let mut seen_in_list: std::collections::HashSet<RelationId> =
            std::collections::HashSet::new();
        for (rank0, &rid) in list.iter().enumerate() {
            if !seen_in_list.insert(rid) {
                continue;
            }
            let rank1 = (rank0 as u32).saturating_add(1);
            let contribution = 1.0 / (k as f32 + rank1 as f32);
            *scores.entry(rid).or_insert(0.0) += contribution;
        }
    }
    let mut merged: Vec<(RelationId, f32)> = scores.into_iter().collect();
    // Stable descending sort; ties broken by RelationId ascending so
    // output is deterministic.
    merged.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.0.cmp(&b.0.0))
    });
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rid(n: u32) -> RelationId {
        RelationId(n)
    }

    #[test]
    fn empty_input_returns_empty() {
        let out = rrf_merge(&[], RRF_K);
        assert!(out.is_empty());
    }

    #[test]
    fn single_list_preserves_order() {
        let list = vec![rid(0), rid(1), rid(2)];
        let out = rrf_merge(std::slice::from_ref(&list), RRF_K);
        assert_eq!(out.len(), 3);
        let ids: Vec<RelationId> = out.iter().map(|(r, _)| *r).collect();
        assert_eq!(ids, list);
        // Score should be strictly descending.
        assert!(out[0].1 > out[1].1);
        assert!(out[1].1 > out[2].1);
    }

    #[test]
    fn rank_one_in_one_list_known_value() {
        let list = vec![rid(42)];
        let out = rrf_merge(&[list], RRF_K);
        assert_eq!(out.len(), 1);
        let expected = 1.0 / (60.0 + 1.0);
        assert!((out[0].1 - expected).abs() < 1e-6);
    }

    #[test]
    fn item_in_three_lists_sums_contributions() {
        // Item rid(7) is rank 0 in list A, rank 0 in list B, rank 2
        // in list C. Score = 1/61 + 1/61 + 1/63.
        let list_a = vec![rid(7), rid(8)];
        let list_b = vec![rid(7), rid(9)];
        let list_c = vec![rid(8), rid(9), rid(7)];
        let out = rrf_merge(&[list_a, list_b, list_c], RRF_K);
        let r7 = out.iter().find(|(r, _)| *r == rid(7)).unwrap();
        let expected = 1.0 / 61.0 + 1.0 / 61.0 + 1.0 / 63.0;
        assert!(
            (r7.1 - expected).abs() < 1e-6,
            "expected {expected}; got {}",
            r7.1
        );
    }

    #[test]
    fn missing_from_list_contributes_zero() {
        // rid(5) only in list A; rid(6) only in list B. Both should
        // get the same score (rank 0 in their respective list, 0 from
        // the other).
        let a = vec![rid(5)];
        let b = vec![rid(6)];
        let out = rrf_merge(&[a, b], RRF_K);
        let r5 = out.iter().find(|(r, _)| *r == rid(5)).unwrap();
        let r6 = out.iter().find(|(r, _)| *r == rid(6)).unwrap();
        assert!((r5.1 - r6.1).abs() < 1e-6);
    }

    #[test]
    fn duplicates_within_list_ignored() {
        // rid(1) appears twice in the same list; should only count
        // once at its first-occurrence rank (0).
        let list = vec![rid(1), rid(2), rid(1)];
        let out = rrf_merge(&[list], RRF_K);
        assert_eq!(out.len(), 2);
        let r1 = out.iter().find(|(r, _)| *r == rid(1)).unwrap();
        let expected = 1.0 / 61.0;
        assert!((r1.1 - expected).abs() < 1e-6);
    }

    #[test]
    fn fused_order_picks_winner_across_signals() {
        // rid(10) is rank 0 in both lists; rid(11) is rank 1 in both.
        // Fused order should put rid(10) first.
        let a = vec![rid(10), rid(11)];
        let b = vec![rid(10), rid(11)];
        let out = rrf_merge(&[a, b], RRF_K);
        assert_eq!(out[0].0, rid(10));
        assert_eq!(out[1].0, rid(11));
        // Score of rid(10) should be 2/61; rid(11) should be 2/62.
        assert!((out[0].1 - 2.0 / 61.0).abs() < 1e-6);
        assert!((out[1].1 - 2.0 / 62.0).abs() < 1e-6);
    }

    #[test]
    fn tie_broken_by_relation_id_ascending() {
        // Build a scenario where two items have identical RRF score:
        // both rank 0 in different single-item lists.
        let a = vec![rid(20)];
        let b = vec![rid(15)];
        let out = rrf_merge(&[a, b], RRF_K);
        assert_eq!(out.len(), 2);
        // Identical score; tiebreaker = lower id first.
        assert_eq!(out[0].0, rid(15));
        assert_eq!(out[1].0, rid(20));
    }
}
