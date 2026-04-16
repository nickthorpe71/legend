//! Prefrontal Cortex — Working memory (L1), attention gating.
//!
//! The prefrontal cortex maintains a limited-capacity working memory buffer
//! and controls what information gets promoted to long-term storage. In Legend:
//!
//! - **Working memory (L1)**: A capacity-limited buffer (~7±2 items, matching
//!   Miller's Law) that holds recent tick content. Queried first during retrieval.
//! - **Attention gate**: Ticks with salience above `ATTENTION_GATE_THRESHOLD`
//!   are fast-tracked from L1 directly to L2 (hippocampus) during encoding.
//! - **Displacement**: When L1 is full, the oldest entry is displaced and
//!   always promoted to L2 (if not already promoted). No data is silently lost.
//! - **Flushing**: On context switch (detected by cosine drop between consecutive
//!   ticks), L1 is flushed — all unpromoted entries are promoted to L2.
//!
//! The `WorkingMemoryEntry` struct lives here as the canonical prefrontal type.

use serde::{Deserialize, Serialize};

use super::{
    hippocampus, hippocampus::extract_memory_refs_from_text, neocortex,
    neurochemistry::ChemicalStamp, BrainState, PRUNE_THRESHOLD,
};

/// A working memory entry — limited capacity, queried first, gates L2 encoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingMemoryEntry {
    pub id: u64,
    pub text: String,
    pub embedding: Vec<f32>,
    pub salience: f32,
    pub tick_created: u64,
    pub rehearsal_count: u32,
    /// Prevents double-insertion into L2 when attention gate already promoted.
    pub promoted: bool,
    /// Amygdala emotional valence carried from encoding.
    pub emotional_valence: f32,
}

/// Push an entry into working memory (L1).
/// When at capacity, the oldest entry is displaced and always promoted to L2.
pub fn push_working_memory(
    state: &mut BrainState,
    text: &str,
    embedding: &[f32],
    salience: f32,
    emotional_valence: f32,
) -> u64 {
    let id = state.next_id;
    state.next_id += 1;

    // Displace oldest if at capacity — always promote to L2 (no data loss)
    if state.working_memory.len() >= state.config.immediate_capacity {
        let displaced = state.working_memory.remove(0);
        if !displaced.promoted {
            // Floor salience so displaced entries survive decay + pruning.
            // PRUNE_THRESHOLD alone is borderline — add margin for decay headroom.
            let promotion_salience = displaced.salience.max(PRUNE_THRESHOLD * 2.0);
            let refs = extract_memory_refs_from_text(&displaced.text);
            hippocampus::insert_short_term(
                state,
                &displaced.text,
                displaced.embedding,
                promotion_salience,
                refs,
                displaced.emotional_valence,
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
            );
            let _ = neocortex::update_graph(state, &displaced.text, promotion_salience);
        }
    }

    state.working_memory.push(WorkingMemoryEntry {
        id,
        text: text.to_string(),
        embedding: embedding.to_vec(),
        salience,
        tick_created: state.clock,
        rehearsal_count: 0,
        promoted: false,
        emotional_valence,
    });
    id
}

/// Flush working memory on session boundary.
/// All unpromoted entries are promoted to L2 — no data is silently lost.
pub fn flush_working_memory(state: &mut BrainState) {
    let entries: Vec<WorkingMemoryEntry> = state.working_memory.drain(..).collect();
    for entry in entries {
        if !entry.promoted {
            let promotion_salience = entry.salience.max(PRUNE_THRESHOLD * 2.0);
            let refs = extract_memory_refs_from_text(&entry.text);
            hippocampus::insert_short_term(
                state,
                &entry.text,
                entry.embedding,
                promotion_salience,
                refs,
                entry.emotional_valence,
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
            );
            let _ = neocortex::update_graph(state, &entry.text, promotion_salience);
        }
    }
}
