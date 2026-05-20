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
//! See `step_12_design.md` for the spec.

use std::collections::HashSet;

use crate::steps::build_relations::Step8Output;
use crate::steps::hebbian::Step10Output;
use crate::steps::print_util::truncate;
use crate::steps::route_regions::RouteResult;
use crate::steps::supersede::Step9Output;
use crate::types::{
    AttentionAction, ConsciousAttentionFrame, Hypergraph, Intent, Policy, RelationActivation,
    RelationId, RelationStatus, ReplayKind, Term, UncertaintySignal,
};

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

/// Assemble the tick's `ConsciousAttentionFrame`. Read-only over
/// the Hypergraph — all structural mutation belongs to Steps 7-11.
///
/// - `focused_relations`: RRF over (dense activation rank +
///   path-reinforced focus_success_count rank) of the
///   reinforcement set, status-filtered (Asserted/Entailed/
///   Defeasible pass; Superseded/Retracted excluded).
/// - `supporting_claims`: for each focused R, the meta-relations
///   pointing at R via `target` whose attribute list carries
///   `derived_from` or `source`.
/// - `history`: meta-relations pointing at R with `supersedes`,
///   plus the Superseded relations they reference.
/// - `next_actions`: maps the tick's UncertaintySignals to
///   advisory replay/follow-up actions.
#[allow(clippy::too_many_arguments)]
pub fn assemble_frame(
    input_text: &str,
    hg: &Hypergraph,
    intent: &Intent,
    active_frame: Option<crate::types::ElementId>,
    route: &RouteResult,
    step8: &Step8Output,
    step9: &Step9Output,
    step10: &Step10Output,
    _policy: &Policy,
) -> ConsciousAttentionFrame {
    // ── focused_relations ──────────────────────────────────────
    // Two ranked lists feeding RRF: dense (by stats.activation
    // descending) and path-reinforced (by stats.focus_success_count
    // descending). Status filter applied before ranking so
    // Superseded/Retracted never enter the RRF.
    let live: Vec<RelationId> = step10
        .reinforced
        .iter()
        .copied()
        .filter(|rid| {
            matches!(
                hg.relations[rid.0 as usize].status,
                RelationStatus::Asserted | RelationStatus::Entailed | RelationStatus::Defeasible,
            )
        })
        .collect();

    let mut dense: Vec<RelationId> = live.clone();
    dense.sort_by(|a, b| {
        let ax = hg.relations[a.0 as usize].stats.activation;
        let bx = hg.relations[b.0 as usize].stats.activation;
        bx.partial_cmp(&ax)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    let mut path: Vec<RelationId> = live.clone();
    path.sort_by(|a, b| {
        let ax = hg.relations[a.0 as usize].stats.focus_success_count;
        let bx = hg.relations[b.0 as usize].stats.focus_success_count;
        bx.cmp(&ax).then(a.0.cmp(&b.0))
    });

    let merged = rrf_merge(&[dense, path], RRF_K);
    let mut focused_relations: Vec<RelationActivation> = merged
        .into_iter()
        .map(|(rid, score)| RelationActivation {
            relation: rid,
            activation: score,
            is_defeasible: hg.relations[rid.0 as usize].status == RelationStatus::Defeasible,
        })
        .collect();

    // Promote cache relations (current_<property> attribute names)
    // to the top of the focus list. They represent the substrate's
    // current belief for a `(target, property)` bucket and should
    // outrank stale binary assertions in the same domain — without
    // this, a query like "what language is Polaris in?" surfaces
    // the original Rust assertion above the live "current_tech=Go"
    // cache because hebbian_rate defaults to 0 in v0, so
    // `stats.activation` never accumulates and the dense ranker
    // falls back to relation-id order (older wins). Stable sort
    // preserves RRF order within each group.
    focused_relations
        .sort_by_key(|ra| !crate::steps::build_relations::is_cache_relation(hg, ra.relation) as u8);

    // ── supporting_claims + history ─────────────────────────────
    // Walk meta_relations_by_subject[R] for each focused R; filter
    // by attribute name on each meta. Dedup the resulting flat lists.
    let derived_from_attr = hg
        .by_name
        .get("derived_from")
        .and_then(|v| v.first().copied());
    let source_attr = hg.by_name.get("source").and_then(|v| v.first().copied());
    let supersedes_attr = hg
        .by_name
        .get("supersedes")
        .and_then(|v| v.first().copied());

    let mut supporting_seen: HashSet<RelationId> = HashSet::new();
    let mut supporting_claims: Vec<RelationId> = Vec::new();
    let mut history_seen: HashSet<RelationId> = HashSet::new();
    let mut history: Vec<RelationId> = Vec::new();

    for ra in &focused_relations {
        let Some(metas) = hg.meta_relations_by_subject.get(&ra.relation) else {
            continue;
        };
        for &mid in metas {
            let m = &hg.relations[mid.0 as usize];
            let mut is_supporting = false;
            let mut is_history = false;
            let mut history_target: Option<RelationId> = None;
            for attr in &m.attributes {
                if derived_from_attr == Some(attr.name) || source_attr == Some(attr.name) {
                    is_supporting = true;
                }
                if supersedes_attr == Some(attr.name) {
                    is_history = true;
                    if let Term::Relation(r_old) = attr.value {
                        history_target = Some(r_old);
                    }
                }
            }
            if is_supporting && supporting_seen.insert(mid) {
                supporting_claims.push(mid);
            }
            if is_history {
                if history_seen.insert(mid) {
                    history.push(mid);
                }
                if let Some(r_old) = history_target
                    && history_seen.insert(r_old)
                {
                    history.push(r_old);
                }
            }
        }
    }

    // ── next_actions ────────────────────────────────────────────
    // Per §5 of step_12_design.md: uncertainty signals → advisory
    // actions. Dedup so repeated DiffuseRouting / LowConfidence
    // don't enqueue multiple identical replay jobs in one tick.
    let mut next_actions: Vec<AttentionAction> = Vec::new();
    let mut replay_emitted = false;
    let mut coref_emitted = false;
    let mut contradiction_emitted = false;
    // Merge uncertainty signals from every step that raises them.
    // Step 4 (route) raises DiffuseRouting; Step 8 raises
    // LowConfidence on Defeasible mints; Step 9 raises Contradiction
    // on gate-failed events with conflicting priors. Dedup by kind
    // so the consumer doesn't see N copies of the same flag.
    let mut uncertainty: Vec<UncertaintySignal> = Vec::new();
    for sig in route
        .uncertainty
        .iter()
        .chain(step8.uncertainty.iter())
        .chain(step9.uncertainty.iter())
    {
        if !uncertainty.contains(sig) {
            uncertainty.push(*sig);
        }
    }
    for sig in &uncertainty {
        match sig {
            UncertaintySignal::DiffuseRouting | UncertaintySignal::LowConfidence => {
                if !replay_emitted {
                    next_actions.push(AttentionAction::EnqueueReplay {
                        kind: ReplayKind::BackgroundSweep,
                    });
                    replay_emitted = true;
                }
            }
            UncertaintySignal::AmbiguousCoref => {
                if !coref_emitted {
                    next_actions.push(AttentionAction::FollowUpQuery(
                        "Could you clarify what the referenced entity is?".to_string(),
                    ));
                    coref_emitted = true;
                }
            }
            UncertaintySignal::Contradiction => {
                if !contradiction_emitted {
                    next_actions.push(AttentionAction::FollowUpQuery(
                        "The new claim conflicts with a prior one; which is correct?".to_string(),
                    ));
                    contradiction_emitted = true;
                }
            }
            UncertaintySignal::UngroundedTime => {
                // v0: no action. chrono parsing isn't wired; the
                // caller may still surface this from the uncertainty
                // field directly.
            }
        }
    }

    // ── current_state ──────────────────────────────────────────
    // For each element this tick referenced, gather every live
    // `current_<property>` cache that mentions it. Dedup across
    // elements (a cache that mentions multiple referenced entities
    // shouldn't appear twice). Status filter mirrors
    // `focused_relations`. Order follows `referenced_elements`
    // (first-mention wins) so the consumer can scan in input order.
    let mut current_state: Vec<RelationId> = Vec::new();
    let mut cs_seen: HashSet<RelationId> = HashSet::new();
    for &eid in &step8.referenced_elements {
        let Some(rels) = hg.relations_by_element.get(&eid) else {
            continue;
        };
        for &rid in rels {
            if !cs_seen.contains(&rid)
                && matches!(
                    hg.relations[rid.0 as usize].status,
                    RelationStatus::Asserted
                        | RelationStatus::Entailed
                        | RelationStatus::Defeasible,
                )
                && crate::steps::build_relations::is_cache_relation(hg, rid)
            {
                cs_seen.insert(rid);
                current_state.push(rid);
            }
        }
    }

    ConsciousAttentionFrame {
        tick: hg.clock,
        input_echo: input_text.to_string(),
        intent: *intent,
        active_frame,
        active_regions: route.active_regions.clone(),
        focused_relations,
        supporting_claims,
        history,
        uncertainty,
        durable_writes: step8.minted_elements.clone(),
        superseded: step9.superseded.clone(),
        current_state,
        next_actions,
    }
}

/// Hand-rolled debug print so the dev-time tick output shows the
/// frame the caller would see. Mirrors `print_step11`. Top-K is
/// hardcoded to 5 — small enough to read at a glance.
pub fn print_step12(frame: &ConsciousAttentionFrame, hg: &Hypergraph) {
    println!();
    println!("attention frame (Step 12)");
    println!("  tick                  {}", frame.tick.0);
    println!(
        "  active_frame          {}",
        frame
            .active_frame
            .map(|e| hg.elements[e.0 as usize]
                .names
                .first()
                .cloned()
                .unwrap_or_else(|| format!("?{}?", e.0)))
            .unwrap_or_else(|| "None".to_string()),
    );
    println!("  active_regions        {}", frame.active_regions.len());
    println!("  focused_relations     {}", frame.focused_relations.len());
    println!("  supporting_claims     {}", frame.supporting_claims.len());
    println!("  history               {}", frame.history.len());
    println!("  uncertainty           {:?}", frame.uncertainty);
    println!("  durable_writes        {}", frame.durable_writes.len());
    println!("  superseded            {}", frame.superseded.len());
    println!("  current_state         {}", frame.current_state.len());
    if !frame.next_actions.is_empty() {
        println!("  next_actions          {}", frame.next_actions.len());
        for action in &frame.next_actions {
            match action {
                AttentionAction::EnqueueReplay { kind } => {
                    println!("    EnqueueReplay {{ kind: {kind:?} }}");
                }
                AttentionAction::FollowUpQuery(text) => {
                    println!("    FollowUpQuery({text:?})");
                }
            }
        }
    }

    if frame.focused_relations.is_empty() {
        return;
    }
    println!();
    println!(
        "  {:<6} {:<10} {:<12} {:<22} {:<14} {:<12}",
        "id", "rrf_score", "status", "subject", "attr", "value",
    );
    println!(
        "  {:-<6} {:-<10} {:-<12} {:-<22} {:-<14} {:-<12}",
        "", "", "", "", "", ""
    );
    for ra in frame.focused_relations.iter().take(5) {
        let r = &hg.relations[ra.relation.0 as usize];
        let subj = r
            .attributes
            .iter()
            .find(|a| a.name == hg.subject_attr)
            .and_then(|a| match a.value {
                Term::Element(e) => hg.elements[e.0 as usize].names.first().cloned(),
                _ => None,
            })
            .unwrap_or_default();
        let (attr_name, val) = r
            .attributes
            .iter()
            .find(|a| a.name != hg.subject_attr)
            .map(|a| {
                let an = hg.elements[a.name.0 as usize]
                    .names
                    .first()
                    .cloned()
                    .unwrap_or_default();
                let v = match a.value {
                    Term::Element(e) => hg.elements[e.0 as usize]
                        .names
                        .first()
                        .cloned()
                        .unwrap_or_default(),
                    Term::Relation(rid) => format!("R{}", rid.0),
                };
                (an, v)
            })
            .unwrap_or_default();
        println!(
            "  R{:<5} {:>9.5}  {:<12} {:<22} {:<14} {:<12}",
            ra.relation.0,
            ra.activation,
            format!("{:?}", r.status),
            truncate(&subj, 22),
            truncate(&attr_name, 14),
            truncate(&val, 12),
        );
    }
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

    // ── Phase 3 tests: assemble_frame body ──────────────────────

    use crate::seed::load_seed_graph;
    use crate::steps::build_relations::build_relations;
    use crate::steps::hebbian::hebbian_and_salience;
    use crate::steps::route_regions::RouteResult;
    use crate::steps::run_extractors::run_extractors;
    use crate::steps::supersede::supersede;
    use crate::types::RegionDelta;

    /// Build a minimal RouteResult for tests that don't actually
    /// run Step 4. Empty active regions / no uncertainty.
    fn empty_route() -> RouteResult {
        RouteResult {
            all_scores: Vec::new(),
            active_regions: Vec::new(),
            delta: RegionDelta::default(),
            uncertainty: Vec::new(),
        }
    }

    /// Drive Step 5 → 8 → 9 → 10 over a sentence, return everything
    /// Step 12 needs. Steps 7 (apply_region_delta) and 11 (decay)
    /// are skipped — they don't materially affect frame assembly.
    fn run_through_step10(
        text: &str,
        labels: &[&str],
        policy: &Policy,
    ) -> (Hypergraph, Step8Output, Step9Output, Step10Output) {
        let mut hg = load_seed_graph();
        let ext = run_extractors(text, labels, policy, &hg, &[]);
        let s8 = build_relations(text, &mut hg, &ext, policy, None);
        let s9 = supersede(&mut hg, &s8.minted_relations, policy);
        let s10 = hebbian_and_salience(&mut hg, &s8, &s9, None, policy);
        (hg, s8, s9, s10)
    }

    #[test]
    fn frame_gathers_durable_writes_from_step8() {
        let policy = Policy::default();
        let (hg, s8, s9, s10) = run_through_step10("Sarah called me yesterday.", &[], &policy);
        let route = empty_route();
        let frame = assemble_frame(
            "Sarah called me yesterday.",
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8,
            &s9,
            &s10,
            &policy,
        );
        assert_eq!(frame.durable_writes, s8.minted_elements);
        assert_eq!(frame.input_echo, "Sarah called me yesterday.");
        assert_eq!(frame.tick, hg.clock);
    }

    #[test]
    fn frame_focused_relations_status_filter() {
        // Default policy yields some Defeasible relations (NER
        // sub-threshold). All focused should be Asserted/Entailed
        // /Defeasible — never Superseded/Retracted.
        let policy = Policy::default();
        let (hg, s8, s9, s10) = run_through_step10(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        let route = empty_route();
        let frame = assemble_frame(
            "The meeting moved from Tuesday to Friday.",
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8,
            &s9,
            &s10,
            &policy,
        );
        assert!(!frame.focused_relations.is_empty());
        for ra in &frame.focused_relations {
            let r = &hg.relations[ra.relation.0 as usize];
            assert!(
                matches!(
                    r.status,
                    RelationStatus::Asserted
                        | RelationStatus::Entailed
                        | RelationStatus::Defeasible,
                ),
                "frame should never include status={:?}",
                r.status,
            );
            // is_defeasible flag mirrors status.
            assert_eq!(
                ra.is_defeasible,
                r.status == RelationStatus::Defeasible,
                "is_defeasible flag drift",
            );
        }
    }

    #[test]
    fn frame_focused_relations_score_descending_within_groups() {
        // Caches are promoted to the top of `focused_relations` so the
        // whole vec is not strictly score-descending — but the cache
        // group and the non-cache group are each individually
        // score-descending. Check both partitions.
        let policy = Policy::default();
        let (hg, s8, s9, s10) = run_through_step10(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        let route = empty_route();
        let frame = assemble_frame(
            "The meeting moved from Tuesday to Friday.",
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8,
            &s9,
            &s10,
            &policy,
        );
        let is_cache = |rid| crate::steps::build_relations::is_cache_relation(&hg, rid);
        let (caches, non_caches): (Vec<&RelationActivation>, Vec<&RelationActivation>) = frame
            .focused_relations
            .iter()
            .partition(|ra| is_cache(ra.relation));
        // Caches come first in the focused_relations vec.
        let cache_count = caches.len();
        for ra in frame.focused_relations.iter().take(cache_count) {
            assert!(is_cache(ra.relation));
        }
        for w in caches.windows(2) {
            assert!(w[0].activation >= w[1].activation);
        }
        for w in non_caches.windows(2) {
            assert!(w[0].activation >= w[1].activation);
        }
    }

    #[test]
    fn frame_supporting_claims_walks_derived_from_meta() {
        // Step 9 emits derived_from metas for every cache it mints.
        // The frame's supporting_claims should pick them up.
        let policy = Policy::default();
        let (hg, s8, s9, s10) = run_through_step10(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        assert!(
            !s9.meta_relations.is_empty(),
            "test prerequisite: Step 9 should have minted metas",
        );
        let route = empty_route();
        let frame = assemble_frame(
            "The meeting moved from Tuesday to Friday.",
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8,
            &s9,
            &s10,
            &policy,
        );
        // At least one supporting_claim should exist (the
        // derived_from meta on a cache relation Step 9 minted).
        let derived_from_attr = hg.by_name["derived_from"][0];
        let has_derived = frame.supporting_claims.iter().any(|&rid| {
            hg.relations[rid.0 as usize]
                .attributes
                .iter()
                .any(|a| a.name == derived_from_attr)
        });
        assert!(
            has_derived,
            "expected at least one derived_from supporting_claim; got {:?}",
            frame.supporting_claims,
        );
    }

    #[test]
    fn frame_superseded_mirrors_step9_output() {
        // Two ticks: first establishes a cache; second supersedes
        // it. Frame from tick 2 should list the flipped prior.
        let policy = Policy::default();
        let mut hg = load_seed_graph();
        let labels: &[&str] = &["event", "weekday"];

        let text1 = "The meeting moved from Monday to Tuesday.";
        let ext1 = run_extractors(text1, labels, &policy, &hg, &[]);
        let s8_1 = build_relations(text1, &mut hg, &ext1, &policy, None);
        let s9_1 = supersede(&mut hg, &s8_1.minted_relations, &policy);
        let _ = hebbian_and_salience(&mut hg, &s8_1, &s9_1, None, &policy);

        let text2 = "The meeting moved from Tuesday to Friday.";
        let ext2 = run_extractors(text2, labels, &policy, &hg, &[]);
        let s8_2 = build_relations(text2, &mut hg, &ext2, &policy, None);
        let s9_2 = supersede(&mut hg, &s8_2.minted_relations, &policy);
        let s10_2 = hebbian_and_salience(&mut hg, &s8_2, &s9_2, None, &policy);

        let route = empty_route();
        let frame = assemble_frame(
            text2,
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8_2,
            &s9_2,
            &s10_2,
            &policy,
        );
        assert_eq!(frame.superseded, s9_2.superseded);
        assert!(
            !frame.superseded.is_empty(),
            "test prerequisite: tick 2 should supersede a prior",
        );
    }

    #[test]
    fn frame_next_actions_diffuse_routing_enqueues_replay() {
        let policy = Policy::default();
        let (hg, s8, s9, s10) = run_through_step10("Sarah called me yesterday.", &[], &policy);
        let mut route = empty_route();
        route.uncertainty.push(UncertaintySignal::DiffuseRouting);
        let frame = assemble_frame(
            "Sarah called me yesterday.",
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8,
            &s9,
            &s10,
            &policy,
        );
        let has_replay = frame.next_actions.iter().any(|a| {
            matches!(
                a,
                AttentionAction::EnqueueReplay {
                    kind: ReplayKind::BackgroundSweep,
                },
            )
        });
        assert!(has_replay, "DiffuseRouting should emit EnqueueReplay");
    }

    #[test]
    fn frame_next_actions_dedupe_repeat_signals() {
        let policy = Policy::default();
        let (hg, s8, s9, s10) = run_through_step10("Sarah called me yesterday.", &[], &policy);
        let mut route = empty_route();
        route.uncertainty.push(UncertaintySignal::DiffuseRouting);
        route.uncertainty.push(UncertaintySignal::LowConfidence);
        route.uncertainty.push(UncertaintySignal::DiffuseRouting);
        let frame = assemble_frame(
            "Sarah called me yesterday.",
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8,
            &s9,
            &s10,
            &policy,
        );
        let replay_count = frame
            .next_actions
            .iter()
            .filter(|a| matches!(a, AttentionAction::EnqueueReplay { .. }))
            .count();
        assert_eq!(replay_count, 1, "replay action should dedup");
    }

    #[test]
    fn frame_next_actions_ambiguous_coref_emits_follow_up() {
        let policy = Policy::default();
        let (hg, s8, s9, s10) = run_through_step10("Sarah called me yesterday.", &[], &policy);
        let mut route = empty_route();
        route.uncertainty.push(UncertaintySignal::AmbiguousCoref);
        let frame = assemble_frame(
            "Sarah called me yesterday.",
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8,
            &s9,
            &s10,
            &policy,
        );
        let has_follow_up = frame
            .next_actions
            .iter()
            .any(|a| matches!(a, AttentionAction::FollowUpQuery(_)));
        assert!(has_follow_up);
    }

    #[test]
    fn frame_ungrounded_time_no_action_emitted() {
        let policy = Policy::default();
        let (hg, mut s8, mut s9, s10) =
            run_through_step10("Sarah called me yesterday.", &[], &policy);
        // Real Step 8/9 may raise their own LowConfidence /
        // Contradiction signals on this input. Clear them so the
        // test isolates the UngroundedTime-alone behavior.
        s8.uncertainty.clear();
        s9.uncertainty.clear();
        let mut route = empty_route();
        route.uncertainty.push(UncertaintySignal::UngroundedTime);
        let frame = assemble_frame(
            "Sarah called me yesterday.",
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8,
            &s9,
            &s10,
            &policy,
        );
        // UngroundedTime is v0-no-action (chrono not wired); still
        // appears in uncertainty so caller can surface it.
        assert!(
            frame
                .uncertainty
                .contains(&UncertaintySignal::UngroundedTime),
        );
        assert!(frame.next_actions.is_empty());
    }

    /// End-to-end two-tick integration: tick 1 mints a cache;
    /// tick 2 supersedes it. Frame from tick 2 should surface:
    ///   - new cache (Asserted) in focused_relations
    ///   - prior cache id in superseded (mirrors Step 9)
    ///   - prior cache id appears in history
    ///   - derived_from meta in supporting_claims
    ///
    /// Exercises the full handoff from Steps 8/9/10 through Step 12.
    #[test]
    fn two_tick_integration_supersession_threads_through_frame() {
        let policy = Policy::default();
        let labels: &[&str] = &["event", "weekday"];
        let mut hg = load_seed_graph();

        // ── Tick 1 ─────────────────────────────────────────────────
        let text1 = "The meeting moved from Monday to Tuesday.";
        let ext1 = run_extractors(text1, labels, &policy, &hg, &[]);
        let s8_1 = build_relations(text1, &mut hg, &ext1, &policy, None);
        let s9_1 = supersede(&mut hg, &s8_1.minted_relations, &policy);
        let _ = hebbian_and_salience(&mut hg, &s8_1, &s9_1, None, &policy);
        // Tick 1 has no supersession (no priors exist yet).
        assert!(s9_1.superseded.is_empty(), "tick 1 has no priors");
        assert!(
            !s9_1.cache_relations.is_empty(),
            "tick 1 should mint a cache"
        );
        let prior_cache = *s9_1.cache_relations.first().unwrap();

        // ── Tick 2 ─────────────────────────────────────────────────
        let text2 = "The meeting moved from Tuesday to Friday.";
        let ext2 = run_extractors(text2, labels, &policy, &hg, &[]);
        let s8_2 = build_relations(text2, &mut hg, &ext2, &policy, None);
        let s9_2 = supersede(&mut hg, &s8_2.minted_relations, &policy);
        let s10_2 = hebbian_and_salience(&mut hg, &s8_2, &s9_2, None, &policy);
        assert!(
            s9_2.superseded.contains(&prior_cache),
            "tick 2 should flip tick 1's cache",
        );

        let route = empty_route();
        let frame = assemble_frame(
            text2,
            &hg,
            &Intent::default(),
            None,
            &route,
            &s8_2,
            &s9_2,
            &s10_2,
            &policy,
        );

        // ── (a) frame.superseded mirrors Step 9. ───────────────────
        assert_eq!(frame.superseded, s9_2.superseded);

        // ── (b) new cache lands in focused_relations as Asserted. ─
        let new_cache = *s9_2.cache_relations.first().unwrap();
        let focused_ids: Vec<RelationId> = frame
            .focused_relations
            .iter()
            .map(|ra| ra.relation)
            .collect();
        assert!(
            focused_ids.contains(&new_cache),
            "new cache should appear in focused_relations",
        );
        let new_cache_ra = frame
            .focused_relations
            .iter()
            .find(|ra| ra.relation == new_cache)
            .unwrap();
        assert!(
            !new_cache_ra.is_defeasible,
            "new cache should land Asserted, not Defeasible",
        );

        // ── (c) prior cache id appears in history (via the
        //        supersedes meta walked off the new cache). ────────
        assert!(
            frame.history.contains(&prior_cache),
            "prior cache should be reachable through history; got {:?}",
            frame.history,
        );

        // ── (d) at least one supporting_claim is a derived_from
        //        meta pointing at the n-ary event Step 8 minted. ───
        let derived_from_attr = hg.by_name["derived_from"][0];
        let any_derived = frame.supporting_claims.iter().any(|&rid| {
            hg.relations[rid.0 as usize]
                .attributes
                .iter()
                .any(|a| a.name == derived_from_attr)
        });
        assert!(
            any_derived,
            "expected derived_from meta in supporting_claims; got {:?}",
            frame.supporting_claims,
        );

        // ── (e) prior cache (Superseded) NOT in focused_relations. ─
        assert!(
            !focused_ids.contains(&prior_cache),
            "Superseded prior should be excluded from focused_relations",
        );
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
