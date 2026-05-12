use crate::math::dot;
use crate::types::{
    ElementId, Hypergraph, Policy, RegionActivation, RegionDelta, RegionStats, UncertaintySignal,
};
use std::collections::HashSet;

/// What Step 4 returns to the rest of the read-mostly phase. The
/// `delta` is held through Steps 5–6 and committed by Step 7; the
/// active region list seeds Step 5's warm-bias label set; the
/// `uncertainty` signals get appended to the per-tick buffer Step 13
/// collects. `all_scores` is a debug-only side-channel — every
/// scored child along with both its cosine and Mahalanobis values.
/// Lets `lib::run` display the actual scoring distribution without
/// re-computing it.
pub struct RouteResult {
    pub active_regions: Vec<RegionActivation>,
    pub delta: RegionDelta,
    pub uncertainty: Vec<UncertaintySignal>,
    pub all_scores: Vec<RegionScore>,
}

/// One scored region in the descent. Carries both fusion inputs so
/// callers can inspect routing decisions:
/// `cosine` drives descent + leaf-vigilance (sharp signal),
/// `mahalanobis` drives the activation gate (distribution-aware focus).
#[derive(Clone, Copy, Debug)]
pub struct RegionScore {
    pub region: ElementId,
    pub cosine: f32,
    pub mahalanobis: f32,
}

/// Step 4 of the tick pipeline. Read-only DAG descent from GENESIS,
/// scoring each candidate region against the input embedding and
/// proposing structural updates to the routed-through path. Pure:
/// no model calls, no allocation beyond the result.
///
/// `embedding` must be the L2-normalized sentence embedding produced
/// by `embed::embed_text`.
///
/// **Two-score fusion** (option D from §4 design notes):
/// - **`cosine`** (max over the region's prototypes) drives the sharp
///   gates — descend and leaf-vigilance. Cosine has wide dynamic range
///   on the unit sphere, so it's decisive at these boundaries.
/// - **`mahalanobis`** (against the region's `region_stats`) drives
///   the activation gate. Mahalanobis is distribution-aware: it asks
///   "does this input look like a *typical member* of this region?"
///   not just "did it match the closest prototype?" — so the active
///   set focuses on regions where the input fits the spread, not just
///   the nearest point.
///
/// Algorithm at each node:
/// - Score every child by both metrics.
/// - If best `cosine` across children < `policy.leaf_vigilance`, the
///   current branch dies into VOID (`delta.void_count += 1`); descent
///   stops here.
/// - Otherwise descend into every child whose `cosine` ≥
///   `policy.descend_threshold` (no K cap).
/// - Among descended children, activate those whose `mahalanobis` ≥
///   `policy.region_activation_threshold`.
/// - If the final active set is empty, `DiffuseRouting` is raised.
///
/// `prototype_updates` carries `(best_cosine_prototype, target)`
/// pairs so Step 7's spherical-k-means drift has a per-prototype
/// target. Per-prototype drift is still meaningful even when
/// activation is decided by region-level stats.
///
/// Mid-path insertion (§10.3.5) is deliberately skipped here — that
/// pass uses span-level embeddings that don't exist until Step 6
/// mints elements. Step 8 owns it.
pub fn route_regions(embedding: &[f32], hg: &Hypergraph, policy: &Policy) -> RouteResult {
    let mut active_regions: Vec<RegionActivation> = Vec::new();
    let mut delta = RegionDelta::default();
    let mut uncertainty: Vec<UncertaintySignal> = Vec::new();
    let mut all_scores: Vec<RegionScore> = Vec::new();

    let mut visited: HashSet<ElementId> = HashSet::new();
    let mut frontier: Vec<ElementId> = vec![hg.genesis];

    while !frontier.is_empty() {
        let mut next_frontier: Vec<ElementId> = Vec::new();
        for current in frontier {
            if !visited.insert(current) {
                continue;
            }

            // Structural leaf — no children to score.
            let Some(children) = hg.region_children.get(&current) else {
                continue;
            };
            if children.is_empty() {
                continue;
            }

            // Score each child by both fusion inputs. `cosine` is the
            // mean of the top-K prototype cosines (requires agreement,
            // not just one outlier); `mahalanobis` is against
            // region_stats. Tracks (child_id, cosine, mahalanobis,
            // best_proto_id).
            let mut scored: Vec<(ElementId, f32, f32, ElementId)> =
                Vec::with_capacity(children.len());
            for &child in children {
                let Some(protos) = hg.region_prototypes.get(&child) else {
                    continue;
                };
                if protos.is_empty() {
                    continue;
                }
                // Score every prototype, keep (proto_id, cosine) pairs.
                let mut sims: Vec<(ElementId, f32)> = protos
                    .iter()
                    .map(|&pid| {
                        let proto = &hg.elements[pid.0 as usize];
                        (pid, dot(embedding, &proto.embedding))
                    })
                    .collect();
                sims.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                // best_proto = the single highest-cosine prototype
                // (still used for prototype_updates so Step 7's
                // per-prototype drift has a target).
                let best_proto = sims[0].0;
                // Mean-of-top-K cosine: averages out single-outlier
                // spikes. K is capped at the prototype count.
                let k = (policy.cosine_top_k as usize).min(sims.len()).max(1);
                let cosine_score = sims.iter().take(k).map(|(_, s)| *s).sum::<f32>() / k as f32;
                // Mahalanobis-similarity if stats are available;
                // otherwise reuse the cosine so the fusion gates still
                // make a decision (degrades gracefully for a hand-
                // built test graph that skipped stats population).
                let mahalanobis_score = match hg.region_stats.get(&child) {
                    Some(stats) => mahalanobis_similarity(embedding, stats, policy.variance_prior),
                    None => cosine_score,
                };
                scored.push((child, cosine_score, mahalanobis_score, best_proto));
                all_scores.push(RegionScore {
                    region: child,
                    cosine: cosine_score,
                    mahalanobis: mahalanobis_score,
                });
            }
            // No prototypes anywhere under this node — structural
            // anomaly (regions are seeded with prototypes; merges/
            // splits could create transient empties). Silent skip —
            // not a routing-quality failure, so don't bump void_count.
            if scored.is_empty() {
                continue;
            }

            // Leaf-vigilance gate — cosine. Best cosine across
            // children must clear the quality floor or this branch
            // routes to VOID.
            let best_cosine = scored
                .iter()
                .map(|(_, c, _, _)| *c)
                .fold(f32::NEG_INFINITY, f32::max);
            if best_cosine < policy.leaf_vigilance {
                delta.void_count += 1;
                continue;
            }

            // Descent gate — cosine ≥ descend_threshold (sharp).
            // Activation gate — mahalanobis ≥ region_activation_threshold
            // (distribution-aware, applied only to descended children).
            for (child, cosine, mahalanobis, best_proto) in scored {
                if cosine < policy.descend_threshold {
                    continue;
                }
                next_frontier.push(child);
                delta.parent_attachments.push((child, current, cosine));
                delta
                    .prototype_updates
                    .push((best_proto, embedding.to_vec()));
                if mahalanobis >= policy.region_activation_threshold {
                    active_regions.push(RegionActivation {
                        region: child,
                        activation: mahalanobis,
                    });
                }
            }
        }
        frontier = next_frontier;
    }

    if active_regions.is_empty() {
        uncertainty.push(UncertaintySignal::DiffuseRouting);
    }

    RouteResult {
        active_regions,
        delta,
        uncertainty,
        all_scores,
    }
}

/// Diagonal-Mahalanobis similarity transformed to a bounded score.
///
/// `D² = Σᵢ (xᵢ − μᵢ)² / (σ²ᵢ + ε)` is the per-dimension Mahalanobis
/// distance squared, with `ε = variance_prior` added to each variance
/// for stability. Returned `score = 1 / (1 + D² / EMBEDDING_DIM)`
/// — normalizing by dimension keeps the score on a sensible (0, 1]
/// scale that's roughly comparable to cosine: 1.0 = perfect match
/// (D = 0), 0.5 ≈ "average per-dim distance ≈ one prior-stddev,"
/// values near 0 indicate poor fit.
///
/// Pure: no allocation, single pass over the embedding.
pub(crate) fn mahalanobis_similarity(
    embedding: &[f32],
    stats: &RegionStats,
    variance_prior: f32,
) -> f32 {
    debug_assert_eq!(embedding.len(), stats.mean.len());
    debug_assert_eq!(embedding.len(), stats.var.len());
    let mut d_squared = 0.0f32;
    #[allow(clippy::needless_range_loop)]
    for i in 0..embedding.len() {
        let diff = embedding[i] - stats.mean[i];
        let var_smoothed = stats.var[i] + variance_prior;
        d_squared += (diff * diff) / var_smoothed;
    }
    let normalized = d_squared / embedding.len() as f32;
    1.0 / (1.0 + normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{EMBEDDING_DIM, embed_text};
    use crate::seed::load_seed_graph;
    use crate::types::{Hypergraph, Policy, UncertaintySignal};

    /// Mahalanobis-similarity is in (0, 1] so "no filtering" requires
    /// thresholds at or below 0. With both knobs at 0, every child of
    /// GENESIS descends. The seed substrate has 14 regions under
    /// GENESIS, so parent_attachments and prototype_updates each land
    /// 14 entries and void_count stays 0.
    #[test]
    fn descends_every_child_with_permissive_thresholds() {
        let hg = load_seed_graph();
        let mut policy = hg.policy.clone();
        policy.descend_threshold = 0.0;
        policy.leaf_vigilance = 0.0;

        let embedding = embed_text("my doctor rescheduled the appointment");
        let result = route_regions(&embedding, &hg, &policy);

        assert_eq!(result.delta.parent_attachments.len(), 14);
        assert_eq!(result.delta.prototype_updates.len(), 14);
        assert_eq!(result.delta.void_count, 0);
    }

    /// Crank `region_activation_threshold` above the
    /// Mahalanobis-similarity ceiling (max 1.0). No child activates →
    /// DiffuseRouting raised.
    #[test]
    fn raises_diffuse_routing_when_nothing_activates() {
        let hg = load_seed_graph();
        let mut policy = hg.policy.clone();
        policy.descend_threshold = 0.0;
        policy.leaf_vigilance = 0.0;
        policy.region_activation_threshold = 2.0;

        let embedding = embed_text("anything");
        let result = route_regions(&embedding, &hg, &policy);

        assert!(result.active_regions.is_empty());
        assert_eq!(result.uncertainty, vec![UncertaintySignal::DiffuseRouting]);
    }

    /// VOID routing: build a synthetic Hypergraph where the input is
    /// far from the region's mean in Mahalanobis units. With
    /// leaf_vigilance set above the resulting score, the parent voids.
    #[test]
    fn routes_to_void_when_below_leaf_vigilance() {
        use crate::types::{
            Attribute, Element, ElementId, MemoryStats, Relation, RelationId, RelationStatus, Term,
            Tick,
        };
        let mk_elem = |id: u32, embedding: Vec<f32>| Element {
            id: ElementId(id),
            names: vec![format!("e{id}")],
            stats: MemoryStats::default(),
            created_at: Tick(0),
            embedding,
        };

        let mut hg = Hypergraph {
            subject_attr: ElementId(0),
            prototype_attr: ElementId(1),
            parent_region_attr: ElementId(2),
            genesis: ElementId(3),
            ..Default::default()
        };

        let zero = vec![0.0f32; EMBEDDING_DIM];
        // Elements 0..3 — attr names + genesis sentinel.
        for i in 0..4 {
            hg.elements.push(mk_elem(i, zero.clone()));
        }
        // Element 4 = region; element 5 = prototype with embedding
        // far from the input we'll route.
        hg.elements.push(mk_elem(4, zero.clone()));
        let mut proto_emb = vec![0.0f32; EMBEDDING_DIM];
        proto_emb[0] = 1.0; // unit vector along axis 0
        hg.elements.push(mk_elem(5, proto_emb));

        // (region, parent_region, genesis)
        hg.relations.push(Relation {
            id: RelationId(0),
            attributes: vec![
                Attribute {
                    name: hg.subject_attr,
                    value: Term::Element(ElementId(4)),
                },
                Attribute {
                    name: hg.parent_region_attr,
                    value: Term::Element(hg.genesis),
                },
            ],
            stats: MemoryStats::default(),
            created_at: Tick(0),
            status: RelationStatus::Asserted,
            priority: 0,
        });
        // (region, prototype, proto)
        hg.relations.push(Relation {
            id: RelationId(1),
            attributes: vec![
                Attribute {
                    name: hg.subject_attr,
                    value: Term::Element(ElementId(4)),
                },
                Attribute {
                    name: hg.prototype_attr,
                    value: Term::Element(ElementId(5)),
                },
            ],
            stats: MemoryStats::default(),
            created_at: Tick(0),
            status: RelationStatus::Asserted,
            priority: 0,
        });
        crate::seed::rebuild_indices(&mut hg);

        // Route a vector pointing the opposite way from the prototype.
        let mut anti = vec![0.0f32; EMBEDDING_DIM];
        anti[0] = -1.0;
        // n=1 → variance is all zero → Mahalanobis dominated by the
        // prior. With prior=0.001 and diff=2.0 in one dim, D² = 4/0.001
        // = 4000; normalized = 4000/384 ≈ 10.4; similarity ≈ 0.087.
        // Set leaf_vigilance comfortably above to force a void.
        let policy = Policy {
            leaf_vigilance: 0.5,
            variance_prior: 0.001,
            ..Default::default()
        };

        let result = route_regions(&anti, &hg, &policy);
        assert_eq!(result.delta.void_count, 1);
        assert!(result.active_regions.is_empty());
        assert_eq!(result.uncertainty, vec![UncertaintySignal::DiffuseRouting]);
    }

    /// `Hypergraph::default()` has no `region_children` entries — even
    /// GENESIS (sentinel ID) maps to nothing. Descent exits
    /// immediately with empty results and DiffuseRouting raised.
    #[test]
    fn empty_hypergraph_returns_diffuse_routing() {
        let hg = Hypergraph::default();
        let policy = Policy::default();

        let embedding = vec![0.0f32; EMBEDDING_DIM];
        let result = route_regions(&embedding, &hg, &policy);

        assert!(result.active_regions.is_empty());
        assert!(result.delta.parent_attachments.is_empty());
        assert_eq!(result.uncertainty, vec![UncertaintySignal::DiffuseRouting]);
    }

    /// Mahalanobis-similarity arithmetic, hand-checked.
    /// Input equal to mean → D² = 0 → score = 1.0 (perfect fit).
    #[test]
    fn mahalanobis_similarity_perfect_match_is_one() {
        let stats = RegionStats {
            mean: vec![0.5; EMBEDDING_DIM],
            var: vec![0.1; EMBEDDING_DIM],
            n: 20,
        };
        let x = vec![0.5; EMBEDDING_DIM];
        let s = mahalanobis_similarity(&x, &stats, 0.001);
        assert!((s - 1.0).abs() < 1e-6, "expected 1.0, got {s}");
    }

    /// Known per-dimension distance + variance → known score.
    /// With diff=0.1 in every dim, var=0.1, prior=0:
    ///   per-dim contribution = 0.01 / 0.1 = 0.1
    ///   Σ over 384 dims = 38.4
    ///   normalized = 38.4 / 384 = 0.1
    ///   score = 1 / 1.1 ≈ 0.9091
    #[test]
    fn mahalanobis_similarity_uniform_offset_arithmetic() {
        let stats = RegionStats {
            mean: vec![0.0; EMBEDDING_DIM],
            var: vec![0.1; EMBEDDING_DIM],
            n: 20,
        };
        let x = vec![0.1; EMBEDDING_DIM];
        let s = mahalanobis_similarity(&x, &stats, 0.0);
        let expected = 1.0 / 1.1;
        assert!((s - expected).abs() < 1e-5, "expected {expected}, got {s}");
    }

    /// Variance prior dominates when empirical variance is 0 (n=1
    /// case). Score behavior should still be well-defined.
    #[test]
    fn mahalanobis_similarity_handles_zero_variance() {
        let stats = RegionStats {
            mean: vec![0.0; EMBEDDING_DIM],
            var: vec![0.0; EMBEDDING_DIM],
            n: 1,
        };
        let mut x = vec![0.0; EMBEDDING_DIM];
        x[0] = 0.1; // single-dim offset
        let s = mahalanobis_similarity(&x, &stats, 0.001);
        // D² = (0.1)² / 0.001 = 10. normalized = 10/384. score = 1 / (1 + 10/384) ≈ 0.974.
        let expected = 1.0 / (1.0 + 10.0 / EMBEDDING_DIM as f32);
        assert!((s - expected).abs() < 1e-5, "expected {expected}, got {s}");
        assert!(s.is_finite(), "score should be finite, got {s}");
    }
}
