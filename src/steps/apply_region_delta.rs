//! Step 7 — apply the `RegionDelta` collected by Step 4 routing.
//!
//! Step 4 (`route_regions`) is read-only — it walks the region DAG,
//! scores each child, and *collects* what would change without
//! actually changing it. Everything it'd write lives in `RegionDelta`.
//!
//! This module commits the `prototype_updates` side of that delta
//! via the spherical-k-means EMA update documented on
//! `RegionDelta::prototype_updates`:
//!
//! ```text
//! new = normalize(old + lr · (target − old))
//! lr  = proto.stats.plasticity * policy.hebbian_rate
//! ```
//!
//! With v0 defaults (`hebbian_rate = 0.0`, `plasticity = 0.0`) the
//! update is a no-op — every prototype is fully sticky. The seed
//! substrate stays bit-stable until something tunes the knobs.
//! That's intentional: the substrate must be drift-capable for
//! Steps 9-10 to do their job, but we don't want unconditional
//! drift erasing the seed pack on day-zero.
//!
//! What this module **doesn't** do (deferred):
//! - **`parent_attachments`** — would commit `(child, parent_region,
//!   parent)` relations / reinforce existing ones. v0 keeps the
//!   seed-pack-shaped region tree; new attachments are §10.3.5
//!   mid-path insertion which needs Step 8.
//! - **`new_regions`**, **`new_members`** — also Step 8.
//! - **`region_stats` updates** — when prototypes drift, the
//!   region's mean/variance technically drift too, but we skip
//!   the Welford update for v0. Mahalanobis routing in Step 4
//!   degrades gracefully (variance prior keeps the denominator
//!   alive).
//! - **`unrouted_count`** — read by Step 12's frame, not mutated
//!   here.

use crate::types::{Hypergraph, Policy, RegionDelta};

/// Apply the drift portion of a `RegionDelta`. Idempotent on a
/// no-op delta (no `prototype_updates`).
pub fn apply_region_delta(hg: &mut Hypergraph, delta: &RegionDelta, policy: &Policy) {
    for (proto_id, target) in &delta.prototype_updates {
        let idx = proto_id.0 as usize;
        let proto = &mut hg.elements[idx];
        debug_assert_eq!(
            proto.embedding.len(),
            target.len(),
            "prototype embedding dim must match target dim",
        );

        let lr = proto.stats.plasticity * policy.hebbian_rate;
        if lr > 0.0 {
            // EMA: new = old + lr * (target - old). Drifts old toward
            // target at rate lr. lr ∈ (0, 1] in practice.
            for (x, &t) in proto.embedding.iter_mut().zip(target.iter()) {
                *x += lr * (t - *x);
            }
            // Re-normalize so cosine-as-dot stays valid downstream.
            let norm: f32 = proto
                .embedding
                .iter()
                .map(|x| x * x)
                .sum::<f32>()
                .sqrt()
                .max(1e-12);
            for x in proto.embedding.iter_mut() {
                *x /= norm;
            }
        }
        // Access count bumps whether or not drift fired — the
        // prototype was hit by routing, which is what
        // `access_count` measures.
        proto.stats.access_count = proto.stats.access_count.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::EMBEDDING_DIM;
    use crate::types::{ElementId, MemoryStats};

    /// Build a synthetic Hypergraph with one prototype-shaped Element
    /// at index `proto_idx`, set its embedding/plasticity, and return
    /// the graph alongside the prototype's ElementId.
    fn synth_hg_with_proto(initial: Vec<f32>, plasticity: f32) -> (Hypergraph, ElementId) {
        let mut hg = Hypergraph::default();
        // Two placeholder elements: anchor at 0, prototype at 1.
        for id_u32 in 0..2u32 {
            hg.elements.push(crate::types::Element {
                id: ElementId(id_u32),
                names: vec![format!("e{id_u32}")],
                stats: MemoryStats::default(),
                created_at: crate::types::Tick(0),
                embedding: vec![0.0; EMBEDDING_DIM],
                polarity: crate::types::Polarity::Signal,
            });
        }
        hg.elements[1].embedding = initial;
        hg.elements[1].stats.plasticity = plasticity;
        (hg, ElementId(1))
    }

    fn unit_e(idx: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; EMBEDDING_DIM];
        v[idx] = 1.0;
        v
    }

    #[test]
    fn zero_hebbian_rate_does_not_drift() {
        // Default policy: hebbian_rate = 0.0. Prototype stays put.
        let initial = unit_e(0);
        let target = unit_e(1);
        let (mut hg, proto_id) = synth_hg_with_proto(initial.clone(), /*plasticity*/ 1.0);

        let delta = RegionDelta {
            prototype_updates: vec![(proto_id, target.clone())],
            ..Default::default()
        };
        apply_region_delta(&mut hg, &delta, &Policy::default());

        assert_eq!(
            hg.elements[1].embedding, initial,
            "expected no drift at lr = 0",
        );
        // access_count still bumps even when lr=0 (the routing
        // touched this prototype).
        assert_eq!(hg.elements[1].stats.access_count, 1);
    }

    #[test]
    fn full_learning_rate_replaces_prototype() {
        // lr = 1.0: new = old + 1 * (target - old) = target.
        let initial = unit_e(0);
        let target = unit_e(1);
        let (mut hg, proto_id) = synth_hg_with_proto(initial, 1.0);

        let policy = Policy {
            hebbian_rate: 1.0,
            ..Default::default()
        };
        let delta = RegionDelta {
            prototype_updates: vec![(proto_id, target.clone())],
            ..Default::default()
        };
        apply_region_delta(&mut hg, &delta, &policy);

        let new = &hg.elements[1].embedding;
        for i in 0..EMBEDDING_DIM {
            assert!(
                (new[i] - target[i]).abs() < 1e-5,
                "i={i}: expected target value at lr=1, got {} vs {}",
                new[i],
                target[i],
            );
        }
    }

    #[test]
    fn half_learning_rate_lands_midway_normalized() {
        // lr = 0.5: new (unnormalized) = 0.5*old + 0.5*target.
        // Orthogonal unit basis vectors → midpoint pre-normalize is
        // [0.5, 0.5, 0…], normalized is [1/√2, 1/√2, 0…].
        let initial = unit_e(0);
        let target = unit_e(1);
        let (mut hg, proto_id) = synth_hg_with_proto(initial, 1.0);

        let policy = Policy {
            hebbian_rate: 0.5,
            ..Default::default()
        };
        let delta = RegionDelta {
            prototype_updates: vec![(proto_id, target)],
            ..Default::default()
        };
        apply_region_delta(&mut hg, &delta, &policy);

        let expected = 1.0 / 2.0f32.sqrt();
        let new = &hg.elements[1].embedding;
        assert!((new[0] - expected).abs() < 1e-4);
        assert!((new[1] - expected).abs() < 1e-4);
        for c in new.iter().skip(2) {
            assert!(c.abs() < 1e-5);
        }
    }

    #[test]
    fn re_normalization_preserves_unit_length() {
        let initial = unit_e(0);
        let target = unit_e(1);
        let (mut hg, proto_id) = synth_hg_with_proto(initial, 1.0);

        let policy = Policy {
            hebbian_rate: 0.3,
            ..Default::default()
        };
        let delta = RegionDelta {
            prototype_updates: vec![(proto_id, target)],
            ..Default::default()
        };
        apply_region_delta(&mut hg, &delta, &policy);

        let norm: f32 = hg.elements[1]
            .embedding
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "expected unit norm after drift, got {norm}",
        );
    }

    #[test]
    fn empty_delta_is_noop() {
        let (mut hg, _) = synth_hg_with_proto(unit_e(0), 1.0);
        let before = hg.elements[1].embedding.clone();
        apply_region_delta(&mut hg, &RegionDelta::default(), &Policy::default());
        assert_eq!(hg.elements[1].embedding, before);
        assert_eq!(hg.elements[1].stats.access_count, 0);
    }

    #[test]
    fn multiple_prototype_updates_all_apply() {
        let mut hg = Hypergraph::default();
        for id_u32 in 0..3u32 {
            hg.elements.push(crate::types::Element {
                id: ElementId(id_u32),
                names: vec![format!("e{id_u32}")],
                stats: MemoryStats {
                    plasticity: 1.0,
                    ..MemoryStats::default()
                },
                created_at: crate::types::Tick(0),
                embedding: unit_e(0),
                polarity: crate::types::Polarity::Signal,
            });
        }
        let policy = Policy {
            hebbian_rate: 1.0,
            ..Default::default()
        };
        let delta = RegionDelta {
            prototype_updates: vec![
                (ElementId(0), unit_e(5)),
                (ElementId(1), unit_e(6)),
                (ElementId(2), unit_e(7)),
            ],
            ..Default::default()
        };
        apply_region_delta(&mut hg, &delta, &policy);
        assert!((hg.elements[0].embedding[5] - 1.0).abs() < 1e-5);
        assert!((hg.elements[1].embedding[6] - 1.0).abs() < 1e-5);
        assert!((hg.elements[2].embedding[7] - 1.0).abs() < 1e-5);
    }
}
