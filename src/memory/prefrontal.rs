//! Prefrontal Cortex — Working memory (L1), attention gating.
//!
//! The prefrontal cortex maintains a limited-capacity working memory buffer
//! and controls what information gets promoted to long-term storage. In Legend:
//!
//! - **Working memory (L1)**: A capacity-limited buffer (~7±2 items, matching
//!   Miller's Law) that holds recent tick content. Queried first during retrieval.
//! - **Attention gate**: Only ticks with salience above `ATTENTION_GATE_THRESHOLD`
//!   are promoted from L1 to L2 (hippocampus). This prevents noise from polluting
//!   episodic memory — like how the prefrontal cortex filters irrelevant stimuli.
//! - **Displacement**: When L1 is full, the oldest entry is displaced.
//!   If it was never promoted, it may be pushed to L2 as a last chance.
//! - **Flushing**: On context switch (detected by cosine drop between consecutive
//!   ticks), L1 is flushed — unpromoted entries get a final promotion opportunity.
//!   This models how a change in task context triggers working memory clearance.
//!
//! The `WorkingMemoryEntry` struct lives here as the canonical prefrontal type.

use serde::{Serialize, Deserialize};

use super::{
    BrainState,
    hippocampus, neocortex,
    hippocampus::extract_memory_refs_from_text,
    ATTENTION_GATE_THRESHOLD,
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
/// When at capacity, the oldest entry is displaced and evaluated for L2 promotion.
/// Displaced entries with high salience or rehearsal are promoted to short-term (L2).
pub fn push_working_memory(
    state: &mut BrainState,
    text: &str,
    embedding: &[f32],
    salience: f32,
    emotional_valence: f32,
) -> u64 {
    let id = state.next_id;
    state.next_id += 1;

    // Displace oldest if at capacity
    if state.working_memory.len() >= state.config.immediate_capacity {
        let displaced = state.working_memory.remove(0);
        // Promotion gate: displaced entry gets L2 if high salience or rehearsed
        if !displaced.promoted
            && (displaced.salience >= ATTENTION_GATE_THRESHOLD
                || displaced.rehearsal_count >= 1)
        {
            let refs = extract_memory_refs_from_text(&displaced.text);
            hippocampus::insert_short_term(
                state,
                &displaced.text,
                displaced.embedding,
                displaced.salience,
                refs,
                displaced.emotional_valence,
            );
            neocortex::update_graph(state, &displaced.text, displaced.salience);
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
/// Every remaining entry is evaluated for L2 promotion before clearing.
/// Entries that pass the gate (high salience or rehearsed) get promoted to L2.
/// Entries that don't pass are discarded — nothing is silently lost.
pub fn flush_working_memory(state: &mut BrainState) {
    let entries: Vec<WorkingMemoryEntry> = state.working_memory.drain(..).collect();
    for entry in entries {
        if !entry.promoted
            && (entry.salience >= ATTENTION_GATE_THRESHOLD || entry.rehearsal_count >= 1)
        {
            let refs = extract_memory_refs_from_text(&entry.text);
            hippocampus::insert_short_term(
                state,
                &entry.text,
                entry.embedding,
                entry.salience,
                refs,
                entry.emotional_valence,
            );
            neocortex::update_graph(state, &entry.text, entry.salience);
        }
    }
}
