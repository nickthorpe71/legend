//! Basal Ganglia — Procedural learning, reinforcement, optimization.
//!
//! The basal ganglia learn through reinforcement — strengthening pathways that
//! lead to reward and weakening those that don't. In Legend:
//!
//! - **Reinforcement (`reinforce()`)**: When the LLM explicitly reinforces a memory
//!   (marking it as useful), AdaGrad-adaptive gradient descent updates its salience.
//!   Frequently-reinforced memories develop smaller learning rates (diminishing returns),
//!   while rarely-reinforced ones stay responsive.
//! - **Contrastive descent**: Memories that were retrieved but NOT reinforced get a
//!   small salience penalty (`CONTRASTIVE_PENALTY`). This models extinction — unrewarded
//!   retrievals gradually weaken associations.
//! - **Graph cascade**: Reinforcement signals propagate from L2 entries to their
//!   associated L3 graph nodes, scaling by `REINFORCE_GRAPH_SCALE`.
//! - **Renormalization**: Periodic EMA-based salience normalization prevents score
//!   drift and maintains a healthy distribution across all entries.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::{
    neurochemistry, signal::apply_bounded_delta, wernicke::extract_entities, BrainState,
    ShortTermEntry, ADAGRAD_BASE_LR, ADAGRAD_EPSILON, ADAGRAD_SQ_SUM_CAP, CONTRASTIVE_PENALTY,
    REINFORCE_GRAPH_SCALE, RENORM_BLEND,
};

// ---------------------------------------------------------------------------
// Basal ganglia types
// ---------------------------------------------------------------------------

/// Feedback result from reinforcing entries after retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReinforceResult {
    pub reinforced: Vec<ReinforcedEntry>,
    pub graph_nodes_affected: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReinforcedEntry {
    pub id: u64,
    pub salience_before: f32,
    pub salience_after: f32,
    pub signal: f32,
}

/// Apply an external reinforcement signal to specific short-term entries.
///
/// Positive `signal` (e.g. 1.0) means "this was useful" — boosts salience
/// and usage count. Negative `signal` (e.g. -0.5) means "this was
/// irrelevant" — reduces salience so the entry decays faster.
///
/// The signal also cascades into the long-term graph: entities extracted
/// from each reinforced entry's text get a proportional weight adjustment.
pub fn reinforce(state: &mut BrainState, ids: &[u64], signal: f32) -> ReinforceResult {
    state.clock += 1;
    let signal = signal.clamp(-1.0, 1.0);
    let mut reinforced = Vec::new();
    let mut graph_nodes_affected = 0usize;

    // Phase C: global neurochemical reinforcement gain
    let effective = neurochemistry::compute_effective(&state.chemistry);

    // Contrastive descent: entries retrieved in the prior retrieve_context() call
    // but not in the current reinforced set receive a small salience penalty.
    if signal > 0.0 {
        let reinforced_set: HashSet<u64> = ids.iter().copied().collect();
        let penalized: Vec<u64> = state
            .last_retrieved_ids
            .iter()
            .copied()
            .filter(|id| !reinforced_set.contains(id))
            .collect();
        for &pid in &penalized {
            if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == pid) {
                entry.salience = apply_bounded_delta(entry.salience, -CONTRASTIVE_PENALTY);
            }
        }
    }

    for &id in ids {
        if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == id) {
            let before = entry.salience;

            // AdaGrad-style adaptive salience update: frequently-reinforced entries
            // get smaller future updates, preventing saturation.
            let adaptive_lr = ADAGRAD_BASE_LR / (entry.gradient_sq_sum + ADAGRAD_EPSILON).sqrt();
            // DA at encoding enhances reinforcement sensitivity
            let da_boost = 1.0 + entry.chemical_stamp.da_at_encoding * 0.3; // 1.0–1.3x
            let delta = signal * adaptive_lr * da_boost * effective.reinforcement_gain;
            entry.gradient_sq_sum =
                (entry.gradient_sq_sum + signal * signal).min(ADAGRAD_SQ_SUM_CAP);
            entry.salience = apply_bounded_delta(entry.salience, delta);

            // Adjust usage: positive signal bumps usage, negative doesn't
            // reduce below 1 (entry still exists, just less important)
            if signal > 0.0 {
                entry.usage = entry.usage.saturating_add(1);
            }
            entry.last_access = state.clock;

            reinforced.push(ReinforcedEntry {
                id,
                salience_before: before,
                salience_after: entry.salience,
                signal,
            });

            // Cascade into graph: boost/demote entities from this entry's text
            let entities = extract_entities(&entry.text.clone(), &state.keyword_cache);
            for entity in &entities {
                if let Some(&node_id) = state.long_term.index.get(&entity.label.to_lowercase()) {
                    if let Some(node) = state.long_term.nodes.get_mut(&node_id) {
                        node.weight = (node.weight + signal * REINFORCE_GRAPH_SCALE).max(0.01);
                        node.last_seen = state.clock;
                        graph_nodes_affected += 1;
                    }
                }
            }
        }
    }

    // Clear stale retrieved IDs after processing.
    state.last_retrieved_ids.clear();

    ReinforceResult {
        reinforced,
        graph_nodes_affected,
    }
}

/// EMA-blend all salience scores toward their max-normalized values.
/// Keeps scores spread relative to each other without a hard reset.
pub fn renormalize_salience(short_term: &mut [ShortTermEntry]) {
    let max_sal = short_term
        .iter()
        .map(|e| e.salience)
        .fold(0.0_f32, f32::max);
    if max_sal < 0.05 {
        return;
    }
    for entry in short_term.iter_mut() {
        let normalized = entry.salience / max_sal;
        entry.salience = entry.salience * (1.0 - RENORM_BLEND) + normalized * RENORM_BLEND;
    }
}
