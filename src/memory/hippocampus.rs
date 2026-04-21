use super::{
    dentate_gyrus::word_overlap,
    entorhinal::cosine_similarity,
    entorhinal::embed_text,
    entorhinal::{summarize_single, summarize_text},
    neocortex::{self, GraphNode},
    neurochemistry::{self, ChemicalStamp, Neurochemistry, STATE_DEPENDENT_BONUS},
    wernicke::{self, extract_entities},
    BrainState, CONSOLIDATED_EVICTION_REDUCTION, EVICTION_DECAY_RATE, HIPPOCAMPAL_DECAY_RATE,
    KEYWORD_MATCH_BONUS, KEYWORD_MATCH_BONUS_CAP, L3_BACKUP_SIMILARITY_THRESHOLD,
    MIN_QUERY_SIMILARITY, PRUNE_AGE_WEIGHT, PRUNE_THRESHOLD, PRUNE_USAGE_WEIGHT,
    RECONSOLIDATION_THRESHOLD, TRACE_INITIAL_SALIENCE, TRACE_NODE_CAP,
};
/// Hippocampus — Episodic memory (L2) functions.
///
/// The hippocampus is the brain's episodic memory system, responsible for encoding,
/// storing, and retrieving specific experiences. In Legend, this corresponds to the
/// `short_term` vector store (`Vec<ShortTermEntry>`):
///
/// - **Encoding**: New ticks that pass the prefrontal attention gate are stored as
///   L2 entries with embeddings, salience, and emotional valence.
/// - **Reconsolidation**: Retrieved memories enter a labile window during which
///   related new information can update them in-place (rather than creating duplicates).
/// - **Pattern completion (CA3)**: Partial cues activate graph structure to reconstruct
///   full memories, modeling the hippocampal CA3 autoassociative network.
/// - **Replay (sharp-wave ripples)**: During consolidation, temporally co-active entries
///   reinforce shared graph edges, modeling hippocampal replay during offline periods.
/// - **Forgetting curve**: Salience decays exponentially, modulated by Ebbinghaus-style
///   stability that grows with spaced retrieval.
///
/// Core types (`ShortTermEntry`, `MemoryRef`) remain in `mod.rs` for serialization.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Hippocampal types
// ---------------------------------------------------------------------------

/// A single entry in the short-term vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortTermEntry {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub summary: String,
    pub embedding: Vec<f32>,
    pub last_access: u64,
    pub usage: u32,
    #[serde(default)]
    pub salience: f32,
    /// How many times this entry has been reconsolidated (retrieved then updated).
    #[serde(default)]
    pub reconsolidation_count: u32,
    /// Clock tick until which this entry is labile (editable after retrieval).
    /// Zero means stable.
    #[serde(default)]
    pub labile_until: u64,
    /// Source references (file + line range) associated with this memory.
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
    /// Accumulated squared gradient for AdaGrad adaptive learning rate.
    #[serde(default)]
    pub gradient_sq_sum: f32,
    /// Semantic density: weighted count of high-signal entities (CodeSymbols, FilePaths).
    #[serde(default)]
    pub density: f32,
    /// Whether this entry has been consolidated into the long-term graph.
    /// Consolidated entries are filtered from query results to avoid redundancy.
    #[serde(default)]
    pub consolidated: bool,
    /// Amygdala emotional valence: negative = threat, positive = reward.
    /// Decays at half the hippocampal rate, modeling emotional persistence.
    #[serde(default)]
    pub emotional_valence: f32,
    /// Ebbinghaus stability: how resistant this memory is to decay.
    /// Starts at 1.0 and grows with spaced retrieval using a soft cap.
    /// Higher stability → slower forgetting curve.
    #[serde(default = "default_stability")]
    pub stability: f32,
    /// Clock interval between the two most recent retrievals.
    /// Used to detect spaced vs massed retrieval patterns.
    #[serde(default)]
    pub last_retrieval_interval: u64,
    /// Monotonic clock tick when this entry was created (never updated).
    #[serde(default)]
    pub created_at_clock: u64,
    /// Unix timestamp (seconds since epoch) when this entry was recorded.
    #[serde(default)]
    pub wall_clock: u64,
    /// Date references extracted from memory text (e.g., "March 15th", "yesterday").
    #[serde(default)]
    pub extracted_dates: Vec<String>,
    /// TCM temporal context snapshot at encoding time (64-dim).
    #[serde(default)]
    pub temporal_context: Vec<f32>,
    /// Neurochemical state at encoding time (Phase B).
    #[serde(default)]
    pub chemical_stamp: ChemicalStamp,
}

impl Default for ShortTermEntry {
    fn default() -> Self {
        Self {
            id: 0,
            text: String::new(),
            summary: String::new(),
            embedding: Vec::new(),
            last_access: 0,
            usage: 0,
            salience: 0.0,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: Vec::new(),
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
            created_at_clock: 0,
            wall_clock: 0,
            extracted_dates: Vec::new(),
            temporal_context: Vec::new(),
            chemical_stamp: ChemicalStamp::default(),
        }
    }
}

/// A typed evidence anchor associated with this memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryRef {
    /// Anchor kind, such as source, time, duration, percent, version, url, path, or quantity.
    #[serde(default = "default_memory_ref_kind")]
    pub kind: String,
    /// Exact extracted anchor value for non-source refs.
    #[serde(default)]
    pub value: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    /// Short snippet for re-anchoring when lines drift.
    #[serde(default)]
    pub snippet: String,
}

fn default_memory_ref_kind() -> String {
    "source".to_string()
}

impl Default for MemoryRef {
    fn default() -> Self {
        Self {
            kind: default_memory_ref_kind(),
            value: String::new(),
            path: String::new(),
            start_line: 0,
            end_line: 0,
            snippet: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnippet {
    pub id: u64,
    pub text: String,
    pub similarity: f32,
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
    /// Unix timestamp when recorded (0 = unknown).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wall_clock: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extracted_dates: Vec<String>,
    /// Monotonic creation order for chronological sorting.
    #[serde(default)]
    pub created_at_clock: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// Maximum evidence anchors per memory entry.
pub const MAX_REFS_PER_ENTRY: usize = 8;

/// Default stability for new entries (Ebbinghaus forgetting curve).
pub fn default_stability() -> f32 {
    1.0
}

/// Hippocampal stability reinforcement with diminishing returns.
///
/// Mirrors the L3 edge stability shaping so episodic durability can keep
/// separating above the old 10.0 ceiling without exploding linearly.
pub fn reinforce_stability(stability: f32, multiplier: f32) -> f32 {
    neocortex::soft_cap_stability(stability * multiplier).max(default_stability())
}

// ---------------------------------------------------------------------------
// Narrow-param functions (operate on specific fields, not full BrainState)
// ---------------------------------------------------------------------------

/// Find the short-term entry most similar to the given embedding.
/// Returns (entry_id, similarity). Returns (0, -1.0) if store is empty.
pub fn find_best_match(short_term: &[ShortTermEntry], embedding: &[f32]) -> (u64, f32) {
    short_term
        .iter()
        .fold((0, -1.0_f32), |(best_id, best_sim), entry| {
            let sim = cosine_similarity(&entry.embedding, embedding);
            if sim > best_sim {
                (entry.id, sim)
            } else {
                (best_id, best_sim)
            }
        })
}

/// Return the top-k most similar short-term entries to the given embedding.
/// When `query` is provided, non-stopword keywords that appear in entry text
/// receive a small similarity bonus to improve lexical precision.
pub fn top_k_similar(
    short_term: &[ShortTermEntry],
    embedding: &[f32],
    k: usize,
    query: &str,
    current_chemistry: &Neurochemistry,
) -> Vec<MemorySnippet> {
    // Pre-compute query keywords (lowercased, non-stopword, len > 1)
    let query_keywords: Vec<String> = query
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| w.len() > 1 && !wernicke::is_stopword(w))
        .collect();

    let mut scored: Vec<MemorySnippet> = short_term
        .iter()
        // No consolidated filter — episodic facts must remain retrievable
        // even after L3 summary creation. L3 summaries supplement, not replace.
        .map(|e| {
            let cosine = cosine_similarity(&e.embedding, embedding);
            let keyword_bonus = if !query_keywords.is_empty() {
                let entry_lower = e.text.to_lowercase();
                let matches = query_keywords
                    .iter()
                    .filter(|kw| entry_lower.contains(kw.as_str()))
                    .count();
                (matches as f32 * KEYWORD_MATCH_BONUS).min(KEYWORD_MATCH_BONUS_CAP)
            } else {
                0.0
            };
            // Amygdala boost: emotionally charged memories surface more readily
            let emotional_boost = e.emotional_valence.abs() * 0.05;
            // State-dependent retrieval: memories encoded under similar chemistry surface more readily
            let state_bonus =
                neurochemistry::chemical_state_match(&e.chemical_stamp, current_chemistry)
                    * STATE_DEPENDENT_BONUS;
            MemorySnippet {
                id: e.id,
                text: e.text.clone(),
                similarity: cosine + keyword_bonus + emotional_boost + state_bonus,
                refs: e.refs.clone(),
                wall_clock: e.wall_clock.clone(),
                extracted_dates: e.extracted_dates.clone(),
                created_at_clock: e.created_at_clock,
            }
        })
        .collect();
    scored.retain(|s| s.similarity >= MIN_QUERY_SIMILARITY);
    scored.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    // No hard cap — threshold-based filtering above is sufficient.
    // Safety cap only to prevent unbounded memory in degenerate cases.
    scored.truncate(k.max(50));
    scored
}

/// Clear labile state from entries whose labile window has expired.
/// Kept for potential L3 reuse — currently disconnected from tick pipeline.
#[allow(dead_code)]
pub fn stabilize_labile_entries(short_term: &mut [ShortTermEntry], clock: u64) {
    for entry in short_term.iter_mut() {
        if entry.labile_until > 0 && entry.labile_until < clock {
            entry.labile_until = 0;
        }
    }
}

/// Remove short-term entries whose composite score has fallen below threshold.
/// `pruning_pressure`: neurochemical multiplier (Phase C) — higher eCB lowers
/// the threshold, making pruning more aggressive.
pub fn prune_short_term(short_term: &mut Vec<ShortTermEntry>, clock: u64, pruning_pressure: f32) {
    // Lower threshold when pruning pressure is high → more aggressive pruning
    let dynamic_threshold = PRUNE_THRESHOLD * (1.0 - pruning_pressure * 0.5).max(0.3);
    short_term.retain(|entry| {
        let age = clock.saturating_sub(entry.last_access) as f32;
        entry.salience + (entry.usage as f32 * PRUNE_USAGE_WEIGHT) - (age * PRUNE_AGE_WEIGHT)
            > dynamic_threshold
    });
}

/// L2 exponential decay: salience decays modulated by density and Ebbinghaus stability.
/// Emotional valence decays at half rate (amygdala persistence).
/// `decay_rate_mod`: neurochemical multiplier (Phase C) — >1.0 accelerates decay (eCB),
/// <1.0 slows it (serotonin stability).
pub fn apply_l2_decay(short_term: &mut [ShortTermEntry], clock: u64, decay_rate_mod: f32) {
    for entry in short_term.iter_mut() {
        // Semantic density reduces decay rate. High-density entries (many symbols/paths) persist longer.
        let density_factor = (1.0 + entry.density * 0.1).min(2.0);
        let base_decay_rate = HIPPOCAMPAL_DECAY_RATE * decay_rate_mod / density_factor;

        // Ebbinghaus: stability slows the forgetting curve. Higher stability → slower decay.
        let effective_decay_rate = base_decay_rate / entry.stability;

        let age = clock.saturating_sub(entry.last_access) as f32;
        let decay = (-age * effective_decay_rate).exp();
        entry.salience *= decay;

        // Amygdala: emotional valence decays at half rate (emotional memories persist longer)
        // NE at encoding slows emotional decay (flashbulb memory effect)
        let ne_protection = 1.0 + entry.chemical_stamp.ne_at_encoding * 0.5; // 1.0–1.5x
        let emotional_decay = (-age * effective_decay_rate * 0.5 / ne_protection).exp();
        entry.emotional_valence *= emotional_decay;
    }
}

// ---------------------------------------------------------------------------
// Wide-param functions (operate on full BrainState)
// ---------------------------------------------------------------------------

/// Try to reconsolidate a new tick into an existing labile memory.
/// Returns the target entry ID if reconsolidation occurred.
/// Kept for potential L3 reuse — currently disconnected from tick pipeline.
#[allow(dead_code)]
pub fn try_reconsolidate(
    state: &mut BrainState,
    text: &str,
    embedding: &[f32],
    salience: f32,
    refs: Vec<MemoryRef>,
) -> Option<u64> {
    let now = state.clock;

    // Find the best labile match
    let mut best: Option<(u64, f32)> = None;
    for entry in &state.short_term {
        if entry.labile_until < now {
            continue; // not labile
        }
        let sim = cosine_similarity(&entry.embedding, embedding);
        let overlap = word_overlap(&entry.text, text);
        // Reconsolidation requires meaningful relation but lower bar than merge
        if sim >= RECONSOLIDATION_THRESHOLD && overlap >= 0.1 {
            let score = sim * 0.6 + overlap * 0.4;
            if best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some((entry.id, score));
            }
        }
    }

    let (target_id, _) = best?;

    // Perform reconsolidation: update the entry in-place
    if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == target_id) {
        // Merge text: append new information
        let merged_text = format!("{} | {}", entry.text, text);
        entry.summary = summarize_text(&entry.text, text, &state.keyword_cache);
        entry.text = if merged_text.len() > 500 {
            // If text is getting too long, use summary as text
            entry.summary.clone()
        } else {
            merged_text
        };
        // Re-embed with combined text
        entry.embedding = embed_text(&entry.text, state.config.embedding_dim);
        // Boost salience (reconsolidated memories are important)
        entry.salience =
            super::signal::reinforce_bounded_signal(entry.salience, salience, 0.3, 0.45, 1.4);
        entry.usage = entry.usage.saturating_add(1);
        entry.last_access = now;
        entry.reconsolidation_count += 1;
        entry.density = calculate_density(&entry.text, &state.keyword_cache);
        // Re-stabilize: no longer labile
        entry.labile_until = 0;
        merge_memory_refs(&mut entry.refs, refs);

        return Some(target_id);
    }

    None
}

/// Insert a new short-term entry, evicting the lowest-scoring entry if at capacity.
pub fn insert_short_term(
    state: &mut BrainState,
    text: &str,
    embedding: Vec<f32>,
    salience: f32,
    refs: Vec<MemoryRef>,
    emotional_valence: f32,
    wall_clock: u64,
    extracted_dates: Vec<String>,
    temporal_context: Vec<f32>,
    chemical_stamp: ChemicalStamp,
) {
    if state.short_term.len() >= state.config.short_term_capacity {
        // Sharp-wave ripple (SWR) analog: emergency consolidation before eviction.
        // The brain fires micro-consolidation bursts under hippocampal load.
        // This ensures evicted entries have L3 backup before they're removed.
        // Guard: skip if we just consolidated (ticks_since_consolidation == 0)
        // to prevent infinite loop.
        if state.ticks_since_consolidation > 0 {
            super::consolidate(state);
        }

        let now = state.clock;
        // CA3 pattern completion analog: use embedding similarity (not exact text
        // match) to determine if an L3 node backs this entry. The brain never does
        // exact recall — it pattern-completes from distributed representations.
        let has_l3_backup = |entry: &ShortTermEntry| -> bool {
            entry.consolidated
                && !entry.embedding.is_empty()
                && state.long_term.nodes.values().any(|n| {
                    matches!(n.kind.as_str(), "Summary" | "Trace")
                        && !n.embedding.is_empty()
                        && cosine_similarity(&n.embedding, &entry.embedding)
                            >= L3_BACKUP_SIMILARITY_THRESHOLD
                })
        };
        if let Some(idx) = state
            .short_term
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let mut score_a = eviction_score(a, now);
                let mut score_b = eviction_score(b, now);
                if has_l3_backup(a) {
                    score_a -= CONSOLIDATED_EVICTION_REDUCTION;
                }
                if has_l3_backup(b) {
                    score_b -= CONSOLIDATED_EVICTION_REDUCTION;
                }
                score_a.partial_cmp(&score_b).unwrap()
            })
            .map(|(i, _)| i)
        {
            // Fast mapping: before evicting an unconsolidated entry, create a
            // lightweight L3 "Trace" node preserving its semantic content.
            // Brain analog: the neocortex can rapidly absorb one-shot facts via
            // a direct pathway (bypassing normal slow consolidation) when the
            // hippocampus is under pressure.
            let victim = &state.short_term[idx];
            if !victim.consolidated && !victim.embedding.is_empty() {
                fast_map_trace(state, idx);
            }
            state.short_term.remove(idx);
        }
    }

    let mut refs = refs;
    if refs.len() > MAX_REFS_PER_ENTRY {
        refs.truncate(MAX_REFS_PER_ENTRY);
    }

    state.short_term.push(ShortTermEntry {
        id: state.next_id,
        text: text.to_string(),
        summary: summarize_single(text, &state.keyword_cache),
        embedding,
        last_access: state.clock,
        usage: 1,
        salience,
        reconsolidation_count: 0,
        labile_until: 0,
        refs,
        gradient_sq_sum: 0.0,
        density: calculate_density(text, &state.keyword_cache),
        consolidated: false,
        emotional_valence,
        stability: 1.0,
        last_retrieval_interval: 0,
        created_at_clock: state.clock,
        wall_clock,
        extracted_dates,
        temporal_context,
        chemical_stamp,
    });
    state.next_id += 1;
}

/// CA3 pattern completion: use graph structure to reconstruct related memories
/// from partial cues.
pub fn pattern_complete(
    state: &BrainState,
    query: &str,
    partial_matches: &[MemorySnippet],
    query_mode: super::neocortex::QueryMode,
) -> Vec<MemorySnippet> {
    // Collect entity seeds from both the query and partial matches
    let mut seed_ids: Vec<u64> = Vec::new();
    let query_entities = extract_entities(query, &state.keyword_cache);
    for entity in &query_entities {
        if let Some(&id) = state.long_term.index.get(&entity.label.to_lowercase()) {
            seed_ids.push(id);
        }
    }
    for snippet in partial_matches {
        let entities = extract_entities(&snippet.text, &state.keyword_cache);
        for entity in &entities {
            if let Some(&id) = state.long_term.index.get(&entity.label.to_lowercase()) {
                seed_ids.push(id);
            }
        }
    }
    seed_ids.sort();
    seed_ids.dedup();

    if seed_ids.is_empty() {
        return Vec::new();
    }

    // Spread activation to find related graph nodes
    let activated =
        super::neocortex::spreading_activation(&state.long_term, &seed_ids, 2, 0.5, query_mode);

    // Collect source_texts from activated nodes
    let mut candidate_texts: Vec<(String, f32)> = Vec::new();
    for (nid, activation) in &activated {
        if let Some(node) = state.long_term.nodes.get(nid) {
            for st in &node.source_texts {
                candidate_texts.push((st.clone(), *activation));
            }
        }
    }

    // Search L2 for entries containing any candidate text
    let existing_ids: HashSet<u64> = partial_matches.iter().map(|s| s.id).collect();
    let mut completed: Vec<MemorySnippet> = Vec::new();

    for entry in &state.short_term {
        if existing_ids.contains(&entry.id) {
            continue;
        }
        // Check if this entry's text matches any activated source_text
        let best_activation = candidate_texts
            .iter()
            .filter(|(text, _)| entry.text.contains(text.as_str()) || text.contains(&entry.text))
            .map(|(_, act)| *act)
            .fold(0.0_f32, f32::max);

        if best_activation > 0.0 {
            // Blend: use entry's own embedding similarity as a factor
            let query_emb = embed_text(query, state.config.embedding_dim);
            let direct_sim = cosine_similarity(&entry.embedding, &query_emb);
            let completion_score = direct_sim * 0.6 + best_activation * 0.4;

            completed.push(MemorySnippet {
                id: entry.id,
                text: entry.text.clone(),
                similarity: completion_score,
                refs: entry.refs.clone(),
                wall_clock: entry.wall_clock.clone(),
                extracted_dates: entry.extracted_dates.clone(),
                created_at_clock: entry.created_at_clock,
            });
        }
    }

    completed.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    // No truncation — return all pattern-completed entries above threshold.
    completed
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Composite eviction score: higher = more worth keeping.
///
/// Balances salience (40%), usage (30%), recency (30%), plus emotional
/// resistance from amygdala valence.
pub fn eviction_score(entry: &ShortTermEntry, now: u64) -> f32 {
    let age = now.saturating_sub(entry.last_access) as f32;
    let recency = (-age * EVICTION_DECAY_RATE).exp();
    let usage = (entry.usage as f32).ln_1p();
    // Amygdala: emotionally charged memories resist eviction
    let emotional_resistance = entry.emotional_valence.abs() * 0.15;
    entry.salience * 0.4 + usage * 0.3 + recency * 0.3 + emotional_resistance
}

/// Fast mapping: create a lightweight L3 "Trace" node from an L2 eviction victim.
///
/// Brain analog: neocortical fast mapping — a direct hippocampus→cortex pathway
/// that absorbs one-shot facts without requiring cluster-based consolidation.
/// The trace preserves the entry's embedding, summary, and entity links so the
/// information remains retrievable even after the episodic entry is evicted.
///
/// `victim_idx` is the index into `state.short_term` of the entry about to be evicted.
fn fast_map_trace(state: &mut BrainState, victim_idx: usize) {
    let victim = &state.short_term[victim_idx];
    let label = if victim.summary.is_empty() {
        summarize_single(&victim.text, &state.keyword_cache)
    } else {
        victim.summary.clone()
    };

    // Enforce Trace node cap: prune lowest-salience Trace before creating a new one.
    let trace_count = state
        .long_term
        .nodes
        .values()
        .filter(|n| n.kind == "Trace")
        .count();
    if trace_count >= TRACE_NODE_CAP {
        if let Some((&weakest_id, _)) = state
            .long_term
            .nodes
            .iter()
            .filter(|(_, n)| n.kind == "Trace")
            .min_by(|(_, a), (_, b)| a.salience.partial_cmp(&b.salience).unwrap())
        {
            state.long_term.nodes.remove(&weakest_id);
            // Clean index entry if it pointed to this node
            state.long_term.index.retain(|_, &mut v| v != weakest_id);
            // Remove edges involving this node
            state
                .long_term
                .edges
                .retain(|e| e.from != weakest_id && e.to != weakest_id);
            state.long_term.rebuild_edge_index();
        }
    }

    let trace_id = state.next_id;
    state.next_id += 1;
    state.long_term.nodes.insert(
        trace_id,
        GraphNode {
            id: trace_id,
            label: label.clone(),
            kind: "Trace".to_string(),
            weight: 1.0 + TRACE_INITIAL_SALIENCE,
            last_seen: state.clock,
            salience: TRACE_INITIAL_SALIENCE,
            gist: Some(label.clone()),
            source_texts: vec![victim.text.clone()],
            embedding: victim.embedding.clone(),
            full_text: Some(victim.text.clone()),
            coverage: None,
        },
    );
    state.long_term.index.insert(label.to_lowercase(), trace_id);

    // Link Trace to extracted entities (same as consolidation topic extraction)
    let entities = extract_entities(&victim.text, &state.keyword_cache);
    for entity in &entities {
        let index_key = entity.label.to_lowercase();
        if let Some(&entity_id) = state.long_term.index.get(&index_key) {
            neocortex::upsert_edge_with_chemical_stamp(
                &mut state.long_term,
                trace_id,
                entity_id,
                "traces",
                state.clock,
                &victim.chemical_stamp,
            );
        }
    }
}

/// Promote a Trace node to Summary when it proves useful (retrieved and reinforced).
///
/// Brain analog: initially weak cortical traces that receive hippocampal replay
/// strengthen into stable cortical representations. A Trace that gets retrieved
/// has proven its value — promote it to full Summary status with boosted salience.
pub fn promote_trace(state: &mut BrainState, node_id: u64) {
    if let Some(node) = state.long_term.nodes.get_mut(&node_id) {
        if node.kind == "Trace" {
            node.kind = "Summary".to_string();
            node.salience = (node.salience + 0.2).min(1.0);
            node.weight = (node.weight + 0.5).min(5.0);
        }
    }
}

/// Semantic density: weighted count of high-signal entities in text.
///
/// Used to prioritize code-rich entries during eviction — entries with
/// file paths and code symbols are harder to reconstruct and more valuable.
pub fn calculate_density(text: &str, kw: &super::wernicke::KeywordCache) -> f32 {
    let entities = super::wernicke::extract_entities(text, kw);
    let mut score = 0.0;
    for entity in entities {
        score += match entity.kind.as_str() {
            "FilePath" => 1.0,
            "Function" | "Struct" | "Enum" | "Trait" | "Class" => 0.8,
            "Symbol" | "Type" => 0.4,
            _ => 0.05,
        };
    }
    score
}

/// Merge incoming evidence anchors into existing refs, deduplicating by typed identity.
pub fn merge_memory_refs(existing: &mut Vec<MemoryRef>, incoming: Vec<MemoryRef>) {
    if incoming.is_empty() {
        return;
    }

    let mut seen: HashSet<(String, String, String, usize, usize)> =
        existing.iter().map(memory_ref_key).collect();

    for reference in incoming {
        let key = memory_ref_key(&reference);
        if seen.insert(key) {
            existing.push(reference);
        }
        if existing.len() >= MAX_REFS_PER_ENTRY {
            break;
        }
    }

    if existing.len() > MAX_REFS_PER_ENTRY {
        existing.truncate(MAX_REFS_PER_ENTRY);
    }
}

/// Extract source references and non-source evidence anchors from tick text.
///
/// Recognizes patterns like `path/to/file.rs#L42`, `file.rs#L10-20`,
/// `02:30 UTC`, `180 days`, `95%`, `v1.2.3`, URLs, and path-like anchors.
pub fn extract_memory_refs_from_text(text: &str) -> Vec<MemoryRef> {
    let mut refs = Vec::new();
    let mut seen: HashSet<(String, String, String, usize, usize)> = HashSet::new();

    for line in text.lines() {
        let snippet = build_ref_snippet(line);
        for token in line.split_whitespace() {
            if let Some(reference) = parse_memory_ref_token(token, &snippet) {
                push_memory_ref(&mut refs, &mut seen, reference);
                if refs.len() >= MAX_REFS_PER_ENTRY {
                    return refs;
                }
            }
        }

        for reference in extract_evidence_refs_from_line(line, &snippet) {
            push_memory_ref(&mut refs, &mut seen, reference);
            if refs.len() >= MAX_REFS_PER_ENTRY {
                return refs;
            }
        }
    }

    refs
}

fn push_memory_ref(
    refs: &mut Vec<MemoryRef>,
    seen: &mut HashSet<(String, String, String, usize, usize)>,
    reference: MemoryRef,
) {
    if seen.insert(memory_ref_key(&reference)) {
        refs.push(reference);
    }
}

fn memory_ref_key(reference: &MemoryRef) -> (String, String, String, usize, usize) {
    let kind = normalized_ref_kind(reference);
    let value = if kind == "source" {
        String::new()
    } else {
        reference.value.clone()
    };
    (
        kind,
        value,
        reference.path.clone(),
        reference.start_line,
        reference.end_line,
    )
}

fn normalized_ref_kind(reference: &MemoryRef) -> String {
    if reference.kind.is_empty() {
        "source".to_string()
    } else {
        reference.kind.clone()
    }
}

/// Parse a single token for a file:line reference pattern.
fn parse_memory_ref_token(token: &str, snippet: &str) -> Option<MemoryRef> {
    let trimmed =
        token.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | ',' | '.' | ')' | ']' | ';'));
    let (path_part, line_part) = trimmed.split_once("#L")?;
    let path = path_part.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '(' | '['));
    if path.is_empty() {
        return None;
    }

    let cleaned = line_part.trim_matches(|c: char| matches!(c, ',' | '.' | ')' | ']' | ';'));
    if cleaned.is_empty() {
        return None;
    }

    let (start_line, end_line) = if let Some((start_s, end_s)) = cleaned.split_once('-') {
        let start_line: usize = start_s.parse().ok()?;
        let end_line: usize = end_s.parse().ok()?;
        if end_line >= start_line {
            (start_line, end_line)
        } else {
            (end_line, start_line)
        }
    } else {
        let line: usize = cleaned.parse().ok()?;
        (line, line)
    };

    Some(MemoryRef {
        kind: "source".to_string(),
        value: format!("{}#L{}-{}", path, start_line, end_line),
        path: path.to_string(),
        start_line,
        end_line,
        snippet: snippet.to_string(),
    })
}

fn extract_evidence_refs_from_line(line: &str, snippet: &str) -> Vec<MemoryRef> {
    let tokens: Vec<String> = line
        .split_whitespace()
        .map(clean_ref_token)
        .filter(|token| !token.is_empty())
        .collect();
    let mut refs = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();

    for (idx, token) in tokens.iter().enumerate() {
        if token.contains("#L") {
            continue;
        }

        let lower = token.to_ascii_lowercase();
        if is_url_token(&lower) {
            push_evidence_ref(&mut refs, &mut seen, "url", token, snippet);
            continue;
        }

        if is_time_token(token) {
            let value = if let Some(next) = tokens.get(idx + 1) {
                if is_time_suffix(next) {
                    format!("{} {}", token, next)
                } else {
                    token.clone()
                }
            } else {
                token.clone()
            };
            push_evidence_ref(&mut refs, &mut seen, "time", &value, snippet);
            continue;
        }

        if is_percent_token(token) {
            push_evidence_ref(&mut refs, &mut seen, "percent", token, snippet);
            continue;
        }

        if is_version_token(token) {
            push_evidence_ref(&mut refs, &mut seen, "version", token, snippet);
            continue;
        }

        if is_money_token(token) {
            push_evidence_ref(&mut refs, &mut seen, "money", token, snippet);
            continue;
        }

        if let Some(next) = tokens.get(idx + 1) {
            let next_lower = next.to_ascii_lowercase();
            if (is_numeric_token(token) || is_spelled_number(&lower)) && is_unit_token(&next_lower)
            {
                let kind = evidence_kind_for_unit(&next_lower);
                let value = format!("{} {}", token, next);
                push_evidence_ref(&mut refs, &mut seen, kind, &value, snippet);
            } else if is_numeric_token(token) && is_currency_unit(&next_lower) {
                let value = format!("{} {}", token, next);
                push_evidence_ref(&mut refs, &mut seen, "money", &value, snippet);
            } else if is_numeric_token(token) && is_percent_word(&next_lower) {
                let value = format!("{} {}", token, next);
                push_evidence_ref(&mut refs, &mut seen, "percent", &value, snippet);
            }
        }

        if is_path_anchor(token) {
            push_evidence_ref(&mut refs, &mut seen, "path", token, snippet);
        }
    }

    refs
}

fn push_evidence_ref(
    refs: &mut Vec<MemoryRef>,
    seen: &mut HashSet<(String, String)>,
    kind: &str,
    value: &str,
    snippet: &str,
) {
    let key = (kind.to_string(), value.to_string());
    if !seen.insert(key) {
        return;
    }

    let path = if matches!(kind, "path" | "url") {
        value.to_string()
    } else {
        String::new()
    };
    refs.push(MemoryRef {
        kind: kind.to_string(),
        value: value.to_string(),
        path,
        start_line: 0,
        end_line: 0,
        snippet: snippet.to_string(),
    });
}

fn clean_ref_token(token: &str) -> String {
    token
        .trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
            )
        })
        .trim_end_matches(|c: char| matches!(c, '.' | ':' | '!'))
        .to_string()
}

fn is_url_token(lower: &str) -> bool {
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn is_time_token(token: &str) -> bool {
    let Some((hour, minute)) = token.split_once(':') else {
        return false;
    };
    !hour.is_empty()
        && !minute.is_empty()
        && hour.len() <= 2
        && minute.len() == 2
        && hour.chars().all(|c| c.is_ascii_digit())
        && minute.chars().all(|c| c.is_ascii_digit())
}

fn is_time_suffix(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "AM" | "PM" | "UTC" | "GMT" | "EST" | "EDT" | "CST" | "CDT" | "MST" | "MDT" | "PST" | "PDT"
    )
}

fn is_percent_token(token: &str) -> bool {
    let Some(number) = token.strip_suffix('%') else {
        return false;
    };
    is_numeric_token(number)
}

fn is_version_token(token: &str) -> bool {
    let core = token.strip_prefix('v').unwrap_or(token);
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() >= 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn is_money_token(token: &str) -> bool {
    token
        .strip_prefix('$')
        .is_some_and(|number| is_numeric_token(number))
}

fn is_numeric_token(token: &str) -> bool {
    let mut seen_digit = false;
    let mut seen_dot = false;
    for c in token.chars() {
        if c.is_ascii_digit() {
            seen_digit = true;
        } else if c == '.' && !seen_dot {
            seen_dot = true;
        } else {
            return false;
        }
    }
    seen_digit
}

fn is_spelled_number(lower: &str) -> bool {
    matches!(
        lower,
        "zero"
            | "one"
            | "two"
            | "three"
            | "four"
            | "five"
            | "six"
            | "seven"
            | "eight"
            | "nine"
            | "ten"
            | "eleven"
            | "twelve"
            | "thirteen"
            | "fourteen"
            | "fifteen"
            | "sixteen"
            | "seventeen"
            | "eighteen"
            | "nineteen"
            | "twenty"
            | "thirty"
            | "forty"
            | "fifty"
            | "sixty"
            | "seventy"
            | "eighty"
            | "ninety"
            | "hundred"
    )
}

fn is_unit_token(lower: &str) -> bool {
    matches!(
        lower,
        "second"
            | "seconds"
            | "minute"
            | "minutes"
            | "hour"
            | "hours"
            | "day"
            | "days"
            | "week"
            | "weeks"
            | "month"
            | "months"
            | "year"
            | "years"
            | "tick"
            | "ticks"
            | "row"
            | "rows"
            | "file"
            | "files"
            | "test"
            | "tests"
            | "retry"
            | "retries"
            | "request"
            | "requests"
            | "error"
            | "errors"
            | "item"
            | "items"
            | "entry"
            | "entries"
            | "node"
            | "nodes"
            | "edge"
            | "edges"
            | "memory"
            | "memories"
            | "token"
            | "tokens"
            | "byte"
            | "bytes"
            | "kb"
            | "mb"
            | "gb"
    )
}

fn evidence_kind_for_unit(lower: &str) -> &'static str {
    match lower {
        "second" | "seconds" | "minute" | "minutes" | "hour" | "hours" | "day" | "days"
        | "week" | "weeks" | "month" | "months" | "year" | "years" => "duration",
        _ => "quantity",
    }
}

fn is_currency_unit(lower: &str) -> bool {
    matches!(lower, "usd" | "dollar" | "dollars")
}

fn is_percent_word(lower: &str) -> bool {
    matches!(lower, "percent" | "percentage")
}

fn is_path_anchor(token: &str) -> bool {
    if !token.contains('/')
        || token.contains("#L")
        || is_url_token(&token.to_ascii_lowercase())
        || !token.chars().any(|c| c.is_ascii_alphabetic())
    {
        return false;
    }

    token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_ref(refs: &[MemoryRef], kind: &str, value: &str) -> bool {
        refs.iter()
            .any(|reference| reference.kind == kind && reference.value == value)
    }

    #[test]
    fn extract_memory_refs_preserves_source_line_refs() {
        let refs = extract_memory_refs_from_text("See `src/main.rs#L10-20` for the handler.");

        let source = refs
            .iter()
            .find(|reference| reference.kind == "source")
            .expect("source ref");
        assert_eq!(source.path, "src/main.rs");
        assert_eq!(source.start_line, 10);
        assert_eq!(source.end_line, 20);
        assert_eq!(source.value, "src/main.rs#L10-20");
    }

    #[test]
    fn extract_memory_refs_captures_quantitative_anchors() {
        let refs = extract_memory_refs_from_text(
            "Project Alpha verifies SQLite backups at 02:30 UTC, archives rows older than 180 days, and alerts at 95%.",
        );

        assert!(has_ref(&refs, "time", "02:30 UTC"));
        assert!(has_ref(&refs, "duration", "180 days"));
        assert!(has_ref(&refs, "percent", "95%"));
    }

    #[test]
    fn extract_memory_refs_captures_spelled_durations_versions_urls_and_paths() {
        let refs = extract_memory_refs_from_text(
            "Release v1.2.3 stores migration files in db/migrations, checks https://example.test/status, and pages after thirty minutes.",
        );

        assert!(has_ref(&refs, "version", "v1.2.3"));
        assert!(has_ref(&refs, "path", "db/migrations"));
        assert!(has_ref(&refs, "url", "https://example.test/status"));
        assert!(has_ref(&refs, "duration", "thirty minutes"));
    }

    #[test]
    fn extract_memory_refs_dedupes_repeated_non_source_anchors() {
        let refs = extract_memory_refs_from_text(
            "Retry window is 30 minutes; later, 30 minutes remains the retry window.",
        );

        let count = refs
            .iter()
            .filter(|reference| reference.kind == "duration" && reference.value == "30 minutes")
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn merge_memory_refs_dedupes_by_kind_and_value_for_evidence_refs() {
        let mut refs = vec![MemoryRef {
            kind: "duration".into(),
            value: "30 minutes".into(),
            snippet: "Retry window is 30 minutes.".into(),
            ..MemoryRef::default()
        }];

        merge_memory_refs(
            &mut refs,
            vec![
                MemoryRef {
                    kind: "duration".into(),
                    value: "30 minutes".into(),
                    snippet: "Later 30 minutes remains the retry window.".into(),
                    ..MemoryRef::default()
                },
                MemoryRef {
                    kind: "duration".into(),
                    value: "45 minutes".into(),
                    snippet: "Retry window became 45 minutes.".into(),
                    ..MemoryRef::default()
                },
            ],
        );

        assert_eq!(refs.len(), 2);
        assert!(has_ref(&refs, "duration", "30 minutes"));
        assert!(has_ref(&refs, "duration", "45 minutes"));
    }
}

/// Build a short snippet from a line for re-anchoring when line numbers drift.
fn build_ref_snippet(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= 120 {
        trimmed.to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}
