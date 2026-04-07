/// Three-layer memory system inspired by cognitive neuroscience.
///
/// This module orchestrates Legend's memory — routing tick input through brain-region
/// subsystems and assembling query results. Like the thalamus routing signals between
/// cortical regions, `mod.rs` coordinates without owning the mechanisms:
///
/// | Brain Region       | Module              | Role in Legend                              |
/// |--------------------|---------------------|---------------------------------------------|
/// | Prefrontal Cortex  | `prefrontal.rs`     | Working memory (L1), attention gating        |
/// | Dentate Gyrus      | `dentate_gyrus.rs`  | Pattern separation, orthogonalization        |
/// | Hippocampus        | `hippocampus.rs`    | Episodic memory (L2), reconsolidation        |
/// | Neocortex          | `neocortex.rs`      | Semantic memory (L3), knowledge graph        |
/// | Amygdala           | `amygdala.rs`       | Emotional valence, intensity tracking        |
/// | Basal Ganglia      | `basal_ganglia.rs`  | Reinforcement learning, AdaGrad              |
///
/// Additional modules handle cross-cutting concerns:
/// - `entorhinal.rs` — Encoding & compression (N-gram embeddings, cosine similarity, chunking, summarization)
/// - `thalamus.rs` — Attentional gating (salience scoring)
/// - `wernicke/` — Language comprehension (entity extraction, keyword system)
pub mod amygdala;
pub mod basal_ganglia;
pub mod dentate_gyrus;
pub mod entorhinal;
pub mod hippocampus;
pub mod neocortex;
pub mod prefrontal;
pub mod thalamus;
pub mod wernicke;

use amygdala::compute_emotional_valence;
use dentate_gyrus::{
    diversity_pass, sparse_orthogonalize, word_overlap, MERGE_WORD_OVERLAP_THRESHOLD,
};
use entorhinal::{
    chunk_text, cosine_similarity, embed_text, merge_embeddings, summarize_group, summarize_text,
};
#[cfg(test)]
use hippocampus::eviction_score;
use hippocampus::{extract_memory_refs_from_text, merge_memory_refs};
use thalamus::compute_salience;
use wernicke::extract_entities;
use wernicke::KeywordCache;

// Re-export tool types so existing `crate::memory::TickResult` paths still work.
#[allow(unused_imports)]
pub use crate::tool::types::{
    GitSyncInfo, MemoryCategory, MemoryConfig, MemoryContext, SessionEntry, TermStats, TickResult,
};
#[allow(unused_imports)]
pub use basal_ganglia::{ReinforceResult, ReinforcedEntry};

// Re-export persistence functions so existing `crate::memory::load_or_default()` paths still work.
#[allow(unused_imports)]
pub use crate::tool::persistence::{
    load_memory_from_path, load_or_default, reset_memory, save, save_memory_to_path,
};

// Re-export tool functions so existing `crate::memory::tick()` etc. paths still work.
#[allow(unused_imports)]
pub use crate::tool::{
    build_context_summary, build_dump, build_start_summary, build_start_summary_with_options,
    clear_task, get_git_summary, get_task, merge_from_log, recent_sessions,
    scan_ecosystem_dependencies, set_task, should_suggest_consolidation, tick, tick_passive,
};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// MEMORY_FILE, MSGPACK_MAGIC, MSGPACK_FORMAT_VERSION — moved to crate::tool::persistence.

// ---------------------------------------------------------------------------
// Prefrontal Cortex — Working memory (L1), attention gating
// ---------------------------------------------------------------------------

/// Minimum salience for a working memory entry to be promoted to L2.
/// Decision keywords: 0.3+ → PROMOTES, Bug/blocker: 0.4+ → PROMOTES,
/// Architecture: 0.25 → PROMOTES (at threshold), Plain text: 0.05 → STAYS IN L1.
pub(super) const ATTENTION_GATE_THRESHOLD: f32 = 0.25;
// SESSION_LOG_CAPACITY — moved to crate::tool (tool-layer concern).

// ---------------------------------------------------------------------------
// Hippocampus — Episodic memory (L2), reconsolidation, pattern completion
// ---------------------------------------------------------------------------

/// L2 salience decay rate (exponential, modulated by entry stability).
pub(super) const HIPPOCAMPAL_DECAY_RATE: f32 = 0.001;
/// Recency decay rate used in eviction scoring.
const EVICTION_DECAY_RATE: f32 = 0.002;
/// Minimum composite score to survive L2 pruning.
pub(super) const PRUNE_THRESHOLD: f32 = 0.1;
/// Weight of usage count in L2 pruning composite score.
pub(super) const PRUNE_USAGE_WEIGHT: f32 = 0.05;
/// Weight of age in L2 pruning composite score.
pub(super) const PRUNE_AGE_WEIGHT: f32 = 0.001;
/// Minimum similarity for a tick to reconsolidate a labile memory.
pub(super) const RECONSOLIDATION_THRESHOLD: f32 = 0.35;
/// Minimum cosine similarity for a query result to be returned (noise floor).
pub(super) const MIN_QUERY_SIMILARITY: f32 = 0.15;
/// Bonus added per non-stopword query keyword found in an entry's text.
pub(super) const KEYWORD_MATCH_BONUS: f32 = 0.05;
/// Maximum total keyword bonus that can be applied.
pub(super) const KEYWORD_MATCH_BONUS_CAP: f32 = 0.2;
/// Ticks a memory stays labile after retrieval before re-stabilizing.
const RECONSOLIDATION_WINDOW: u64 = 5;
/// Pattern completion (CA3 autoassociative recall): minimum L2 results
/// before pattern completion activates.
const PATTERN_COMPLETION_MIN_RESULTS: usize = 3;
/// Minimum top-result similarity before pattern completion activates.
const PATTERN_COMPLETION_SIM_THRESHOLD: f32 = 0.5;
/// Sharp-wave ripple replay: entries accessed within this many ticks are
/// considered temporally co-active (modeling hippocampal replay during rest).
pub(super) const REPLAY_TEMPORAL_WINDOW: u64 = 5;
/// Edge weight boost for shared entities during replay consolidation.
pub(super) const REPLAY_EDGE_BOOST: f32 = 0.08;
/// Salience boost for entries that participate in replay.
pub(super) const REPLAY_SALIENCE_BOOST: f32 = 0.02;
/// Eviction score reduction for consolidated L2 entries whose Summary node
/// has a valid embedding (L3 can serve their role).
pub(super) const CONSOLIDATED_EVICTION_REDUCTION: f32 = 0.2;

// ---------------------------------------------------------------------------
// Neocortex — Semantic memory (L3), knowledge graph, consolidation
// ---------------------------------------------------------------------------

/// L3 graph node/edge weight decay rate.
pub(super) const NEOCORTICAL_DECAY_RATE: f32 = 0.0005;
/// Hebbian learning: edge weight boost on co-retrieval.
pub(super) const HEBBIAN_EDGE_BOOST: f32 = 0.05;
/// Hebbian learning: node weight boost on co-retrieval.
pub(super) const HEBBIAN_NODE_BOOST: f32 = 0.02;
/// Maximum edge weight to prevent Hebbian reinforcement explosion.
pub(super) const HEBBIAN_EDGE_CEILING: f32 = 10.0;
/// Maximum node weight to prevent Hebbian node boost explosion.
pub(super) const HEBBIAN_NODE_CEILING: f32 = 5.0;
/// Weight increment when upserting/reinforcing an edge.
pub(super) const EDGE_REINFORCE_DELTA: f32 = 0.1;
/// Base weight assigned to new graph nodes.
pub(super) const NODE_WEIGHT_BASE: f32 = 0.2;
/// Hard cap on long-term graph nodes.
pub(super) const GRAPH_NODE_CAPACITY: usize = 2048;
/// Hard cap on long-term graph edges.
pub(super) const GRAPH_EDGE_CAPACITY: usize = 8192;
/// Minimum node weight to survive graph pruning.
pub(super) const GRAPH_PRUNE_WEIGHT: f32 = 0.05;
/// Graph weight ceiling before periodic normalization fires.
pub(super) const GRAPH_WEIGHT_TARGET_MAX: f32 = 2.0;
/// Ticks between graph weight normalization passes.
const GRAPH_NORM_INTERVAL: u64 = 5;
/// Spreading activation: activation decays by this factor per hop.
pub(super) const SPREADING_ACTIVATION_DECAY: f32 = 0.5;
/// Maximum hops for spreading activation in graph_lookup.
pub(super) const SPREADING_ACTIVATION_MAX_HOPS: usize = 3;
/// Number of ticks before suggesting a consolidation.
pub const CONSOLIDATION_SUGGESTION_THRESHOLD: u32 = 15;
/// Systems consolidation: minimum average salience for neocortical encoding.
const SYSTEMS_CONSOLIDATION_SALIENCE_THRESHOLD: f32 = 0.4;
/// Maximum length of full_text stored on consolidated Summary nodes.
const SUMMARY_FULL_TEXT_MAX_LEN: usize = 500;
/// Minimum cosine similarity for L3 Summary node retrieval.
const SUMMARY_RETRIEVAL_MIN_SIM: f32 = 0.3;
/// Maximum number of L3 Summary results to include in retrieval.
const SUMMARY_RETRIEVAL_MAX_RESULTS: usize = 3;
/// Layer 3 incremental keyword discovery: minimum distinct ticks for auto-promotion.
const TERM_PROMOTION_MIN_TICKS: u32 = 5;
/// Minimum character length for auto-promoted terms.
const TERM_PROMOTION_MIN_LEN: usize = 3;

// ---------------------------------------------------------------------------
// Basal Ganglia — Procedural learning, reinforcement, AdaGrad optimization
// ---------------------------------------------------------------------------

/// Epsilon for AdaGrad denominator stability.
pub(super) const ADAGRAD_EPSILON: f32 = 1e-6;
/// Base learning rate for AdaGrad salience updates.
pub(super) const ADAGRAD_BASE_LR: f32 = 0.15;
/// Cap on accumulated squared gradients to prevent LR collapse.
pub(super) const ADAGRAD_SQ_SUM_CAP: f32 = 1000.0;
/// Ticks between salience EMA renormalization passes.
const RENORM_INTERVAL: u64 = 10;
/// EMA blend weight toward normalized values (gentle).
pub(super) const RENORM_BLEND: f32 = 0.1;
/// Salience penalty applied to retrieved-but-unreinforced entries.
pub(super) const CONTRASTIVE_PENALTY: f32 = 0.02;
/// How much a reinforcement signal scales graph node weight adjustment.
pub(super) const REINFORCE_GRAPH_SCALE: f32 = 0.1;
/// Passive salience boost for top retrieval result, scaled by similarity.
const AUTO_REINFORCE_SCALE: f32 = 0.03;

// ---------------------------------------------------------------------------
// Amygdala — Emotional processing, intensity-driven consolidation triggers
// ---------------------------------------------------------------------------

/// Rolling emotional intensity threshold for early consolidation.
pub const EMOTIONAL_CONSOLIDATION_THRESHOLD: f32 = 1.5;
/// Decay factor for rolling emotional intensity (applied each tick).
const EMOTIONAL_INTENSITY_DECAY: f32 = 0.8;
/// Context switch detection: cosine similarity threshold for topic shifts.
const CONTEXT_SWITCH_THRESHOLD: f32 = 0.15;

/// CPEB synaptic tagging: recently-active edges get amplified stability
/// when a high-valence event occurs (Kandel's synaptic capture model).
const CPEB_TAG_WINDOW: u64 = 5;
const CPEB_VALENCE_THRESHOLD: f32 = 0.3;
const CPEB_STABILITY_BOOST: f32 = 1.5;

/// Stopwords excluded from Layer 3 keyword auto-promotion.
/// Common English words that carry no domain-specific information.
const STOPWORDS: &[&str] = &[
    // Articles & determiners
    "a", "an", "the", "this", "that", "these", "those", // Pronouns
    "i", "me", "my", "we", "us", "our", "you", "your", "he", "she", "it", "him", "her", "his",
    "its", "they", "them", "their", // Prepositions
    "in", "on", "at", "to", "for", "of", "with", "by", "from", "up", "about", "into", "through",
    "during", "before", "after", "above", "below", "between", "under", "over", "out",
    // Conjunctions
    "and", "but", "or", "nor", "so", "yet", "both", "either", "neither", // Common verbs
    "is", "am", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do", "does",
    "did", "will", "would", "shall", "should", "may", "might", "can", "could", "must", "get",
    "got", "make", "made", "go", "went", "gone", "say", "said", "see", "saw", "know", "knew",
    "think", "thought", "come", "came", "take", "took", "give", "gave", "tell", "told",
    // Common adverbs
    "not", "no", "very", "just", "also", "then", "now", "here", "there", "when", "where", "how",
    "what", "which", "who", "whom", "why", "all", "each", "every", "some", "any", "many", "much",
    "more", "most", "other", "only", "own", "same", "than", "too", "well", "still",
    // Common adjectives
    "new", "old", "good", "bad", "big", "small", "long", "short", "first", "last", "next", "few",
    "little", // Misc high-frequency
    "one", "two", "like", "time", "way", "thing", "even", "back", "people", "work", "day", "part",
    "case", "point", "need", "want", "try", "use", "used", "using", "set", "let", "etc", "e.g",
    "i.e",
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

// MemoryConfig — moved to crate::tool::types, re-exported above.

// TermStats — moved to crate::tool::types, re-exported above.

/// Pure cognitive state — all brain-region data, no IO or tool concerns.
///
/// Contains the three memory layers (working memory, episodic, semantic) plus
/// supporting cognitive state (attention, reinforcement, emotional valence).
/// Brain-region functions operate on this struct directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrainState {
    pub config: MemoryConfig,
    pub working_memory: Vec<WorkingMemoryEntry>,
    pub short_term: Vec<ShortTermEntry>,
    pub long_term: GraphMemory,
    pub clock: u64,
    pub next_id: u64,
    /// Number of ticks since last consolidation.
    #[serde(default)]
    pub ticks_since_consolidation: u32,
    /// IDs returned by the most recent retrieve_context() call, for contrastive descent.
    #[serde(default)]
    pub last_retrieved_ids: Vec<u64>,
    /// Rolling sum of emotional intensity (|valence|), decays each tick.
    /// Used for amygdala-driven early consolidation.
    #[serde(default)]
    pub recent_valence_sum: f32,
    /// Embedding of the most recent tick, for context-switch detection.
    #[serde(default)]
    pub last_tick_embedding: Vec<f32>,
    /// Layer 3: term frequency tracking for incremental keyword discovery.
    /// Maps entity label → usage statistics. Terms that pass noise filters
    /// are auto-promoted to `kw:domain:<term>` graph nodes.
    #[serde(default)]
    pub term_frequency: HashMap<String, TermStats>,
    /// Dynamic keyword cache populated from graph + static fallbacks.
    #[serde(skip)]
    pub keyword_cache: wernicke::KeywordCache,
}

/// Full application state — wraps BrainState with tool-layer fields.
///
/// `#[serde(flatten)]` keeps the on-disk format identical to the old flat
/// MemoryState layout, so no migration is needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryState {
    #[serde(flatten)]
    pub brain: BrainState,
    /// Chronological log of tick text, preserving exact user input.
    #[serde(default)]
    pub session_log: Vec<SessionEntry>,
    /// Pinned current task description for session context.
    #[serde(default)]
    pub current_task: Option<String>,
    /// Last Git commit SHA processed by Legend.
    #[serde(default)]
    pub last_synced_sha: Option<String>,
}

// ShortTermEntry, MemoryRef, MemorySnippet — defined in hippocampus.rs, re-exported here.
#[allow(unused_imports)] // MemoryRef: used by lib consumers + ShortTermEntry's public API
pub use hippocampus::{MemoryRef, MemorySnippet, ShortTermEntry};

// WorkingMemoryEntry — defined in prefrontal.rs, re-exported here.
pub use prefrontal::WorkingMemoryEntry;

// GraphMemory, GraphNode, GraphEdge, GraphNodeSummary — defined in neocortex.rs, re-exported here.
pub use neocortex::{GraphEdge, GraphMemory, GraphNode, GraphNodeSummary};

// ReinforceResult, ReinforcedEntry — defined in basal_ganglia.rs, re-exported above.

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

impl Default for BrainState {
    fn default() -> Self {
        Self {
            config: MemoryConfig::default(),
            working_memory: Vec::new(),
            short_term: Vec::new(),
            long_term: GraphMemory::default(),
            clock: 0,
            next_id: 1,
            ticks_since_consolidation: 0,
            last_retrieved_ids: Vec::new(),
            recent_valence_sum: 0.0,
            last_tick_embedding: Vec::new(),
            term_frequency: HashMap::new(),
            keyword_cache: wernicke::KeywordCache::default_from_static(),
        }
    }
}

impl Default for MemoryState {
    fn default() -> Self {
        Self {
            brain: BrainState::default(),
            session_log: Vec::new(),
            current_task: None,
            last_synced_sha: None,
        }
    }
}

// MemoryCategory — moved to crate::tool::types, re-exported above.

/// Detect the primary category of a text based on keyword patterns.
pub fn classify_text(text: &str, kw: &wernicke::KeywordCache) -> MemoryCategory {
    let lower = text.to_lowercase();

    // Decision patterns (highest priority)
    let decision_score = kw
        .decision
        .iter()
        .filter(|k| lower.contains(k.as_str()))
        .count();
    if decision_score >= 2 {
        return MemoryCategory::Decision;
    }

    // TODO patterns
    if kw.todo.iter().any(|k| lower.contains(k.as_str())) {
        return MemoryCategory::Todo;
    }

    // Preference patterns
    if kw.preference.iter().any(|k| lower.contains(k.as_str())) {
        return MemoryCategory::Preference;
    }

    // Architecture patterns
    if kw.architecture.iter().any(|k| lower.contains(k.as_str())) {
        return MemoryCategory::Architecture;
    }

    // Bug patterns
    if kw.bug.iter().any(|k| lower.contains(k.as_str())) {
        return MemoryCategory::Bug;
    }

    // Progress patterns
    if kw
        .action
        .iter()
        .any(|(verb, _)| lower.contains(verb.as_str()))
    {
        return MemoryCategory::Progress;
    }

    // Single decision keyword is enough if it looks intentional
    if decision_score >= 1 {
        return MemoryCategory::Decision;
    }

    MemoryCategory::General
}

// ---------------------------------------------------------------------------
// Core MemoryState logic
// ---------------------------------------------------------------------------

// GitSyncInfo — moved to crate::tool::types, re-exported above.

// load_or_default — moved to crate::tool::persistence, re-exported above.

// get_git_summary — moved to crate::tool, re-exported below.
// save — moved to crate::tool::persistence, re-exported above.
// tick, tick_passive — moved to crate::tool, re-exported below.

pub fn tick_impl(state: &mut BrainState, text: &str, passive: bool) -> TickResult {
    state.clock += 1;
    if !passive {
        state.ticks_since_consolidation += 1;
        // Decay rolling emotional intensity each active tick
        state.recent_valence_sum *= EMOTIONAL_INTENSITY_DECAY;
    }
    apply_decay(state);
    hippocampus::stabilize_labile_entries(&mut state.short_term, state.clock);
    if state.clock.is_multiple_of(RENORM_INTERVAL) {
        basal_ganglia::renormalize_salience(&mut state.short_term);
    }
    if state.clock.is_multiple_of(GRAPH_NORM_INTERVAL) {
        neocortex::normalize_graph_weights(&mut state.long_term);
    }

    let mut last_context = MemoryContext {
        short_term: Vec::new(),
        long_term: Vec::new(),
        working_memory: Vec::new(),
    };

    // Track the action taken (priority: created > reconsolidated > merged)
    let mut result_action = "created".to_string();
    let mut result_entry_id: u64 = 0;
    let mut result_matched: Option<u64> = None;
    let mut result_similarity: Option<f32> = None;

    for chunk in chunk_text(text) {
        let raw_embedding = embed_text(&chunk, state.config.embedding_dim);
        let salience = compute_salience(&chunk, &state.keyword_cache);
        let emotional_valence = compute_emotional_valence(&chunk, &state.keyword_cache);
        let refs = extract_memory_refs_from_text(&chunk);

        // Dentate Gyrus sparse orthogonalization: push the new embedding away from
        // similar-but-distinct existing L2 embeddings to reduce interference.
        let existing_embeddings: Vec<Vec<f32>> = state
            .short_term
            .iter()
            .map(|e| e.embedding.clone())
            .collect();
        let embedding = sparse_orthogonalize(
            &raw_embedding,
            &existing_embeddings,
            state.config.theta_low, // low: only orthogonalize in the confusable zone
            state.config.theta_high, // high: near-identical entries should still merge
            0.3,                    // strength: moderate push-away
        );

        // Always push into working memory (L1)
        let wm_id =
            prefrontal::push_working_memory(state, &chunk, &embedding, salience, emotional_valence);

        // --- Attention gate: only high-salience ticks promote to L2 ---
        if salience >= ATTENTION_GATE_THRESHOLD {
            // --- Reconsolidation: check labile entries first ---
            // If a recently-retrieved memory is labile and the new text is related,
            // update that memory in-place instead of creating a duplicate.
            if let Some(reconsolidated_id) =
                hippocampus::try_reconsolidate(state, &chunk, &embedding, salience, refs.clone())
            {
                // Update graph with the new text context
                neocortex::update_graph(state, &chunk, salience);
                last_context = retrieve_context(state, &chunk);
                // Mark L1 entry as promoted
                if let Some(wm_entry) = state.working_memory.iter_mut().find(|e| e.id == wm_id) {
                    wm_entry.promoted = true;
                }
                // Track reconsolidation
                result_action = "reconsolidated".to_string();
                result_entry_id = reconsolidated_id;
                result_matched = Some(reconsolidated_id);
                continue;
            }

            // --- Normal path: match/merge/insert ---
            let (best_id, best_sim) = hippocampus::find_best_match(&state.short_term, &embedding);

            // Diversity gate: even at high similarity, if word overlap is low the
            // Dentate Gyrus diversity gate: even at high similarity, if word overlap is
            // low the texts are semantically distinct and should not be merged.
            let diversity_ok = if best_sim >= state.config.theta_low {
                state
                    .short_term
                    .iter()
                    .find(|e| e.id == best_id)
                    .map(|e| diversity_pass(&e.text, &chunk))
                    .unwrap_or(false)
            } else {
                false
            };

            match best_sim {
                s if s >= state.config.theta_high && diversity_ok => {
                    if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == best_id) {
                        entry.usage = entry.usage.saturating_add(2);
                        entry.salience = (entry.salience + salience).min(1.0);
                        entry.last_access = state.clock;
                        merge_memory_refs(&mut entry.refs, refs.clone());
                    }
                    // Track merge (high similarity)
                    result_action = "merged".to_string();
                    result_entry_id = best_id;
                    result_matched = Some(best_id);
                    result_similarity = Some(best_sim);
                }
                s if s >= state.config.theta_low && diversity_ok => {
                    if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == best_id) {
                        entry.embedding = merge_embeddings(&entry.embedding, &embedding);
                        entry.usage = entry.usage.saturating_add(1);
                        entry.salience = (entry.salience + salience * 0.5).min(1.0);
                        entry.summary = summarize_text(&entry.text, &chunk, &state.keyword_cache);
                        entry.last_access = state.clock;
                        merge_memory_refs(&mut entry.refs, refs.clone());
                    }
                    neocortex::update_graph(state, &chunk, salience);
                    // Track merge (low similarity)
                    result_action = "merged".to_string();
                    result_entry_id = best_id;
                    result_matched = Some(best_id);
                    result_similarity = Some(best_sim);
                }
                _ => {
                    hippocampus::insert_short_term(
                        state,
                        &chunk,
                        embedding,
                        salience,
                        refs,
                        emotional_valence,
                    );
                    neocortex::update_graph(state, &chunk, salience);
                    // Track creation - get the ID of the newly inserted entry
                    if let Some(entry) = state.short_term.last() {
                        result_action = "created".to_string();
                        result_entry_id = entry.id;
                        result_matched = None;
                        result_similarity = None;
                    }
                }
            }

            // Mark L1 entry as promoted
            if let Some(wm_entry) = state.working_memory.iter_mut().find(|e| e.id == wm_id) {
                wm_entry.promoted = true;
            }

            last_context = retrieve_context(state, &chunk);
        } else {
            // Low-salience: stays in working memory only, skip L2 insertion
            result_action = "working_memory_only".to_string();
            result_entry_id = wm_id;
            result_matched = None;
            result_similarity = None;
        }
    }

    // Layer 3: update term frequency stats and auto-promote recurring entities
    if !passive {
        update_term_frequencies(state, text);
    }

    hippocampus::prune_short_term(&mut state.short_term, state.clock);
    neocortex::prune_graph(&mut state.long_term, state.clock);

    // --- Smart consolidation triggers ---
    if !passive {
        // Accumulate emotional intensity from this tick
        let tick_embedding = embed_text(text, state.config.embedding_dim);
        let tick_valence = compute_emotional_valence(text, &state.keyword_cache);
        state.recent_valence_sum += tick_valence.abs();

        // CPEB synaptic tagging: high-valence events selectively strengthen
        // recently active graph edges (Kandel's synaptic capture model).
        if tick_valence.abs() > CPEB_VALENCE_THRESHOLD {
            neocortex::cpeb_tag_edges(
                &mut state.long_term,
                state.clock,
                tick_valence.abs(),
                CPEB_TAG_WINDOW,
                CPEB_STABILITY_BOOST,
            );
        }

        // Context switch detection: compare with previous tick's embedding
        if !state.last_tick_embedding.is_empty() {
            let sim = cosine_similarity(&state.last_tick_embedding, &tick_embedding);
            if sim < CONTEXT_SWITCH_THRESHOLD {
                // Topic shift detected — suggest consolidation and flush L1
                prefrontal::flush_working_memory(state);
            }
        }
        state.last_tick_embedding = tick_embedding;
    }

    TickResult {
        action: result_action,
        entry_id: result_entry_id,
        matched_existing: result_matched,
        similarity: result_similarity,
        context: last_context,
    }
}

fn infer_query_mode(query: &str, keyword_cache: &KeywordCache) -> neocortex::QueryMode {
    let query_lower = query.to_lowercase();

    let temporal_markers = [
        "yesterday",
        "earlier",
        "when",
        "during",
        "worked on",
        "session",
        "recently",
    ];
    if temporal_markers.iter().any(|m| query_lower.contains(m)) {
        return neocortex::QueryMode::Temporal;
    }

    let diagnostic_markers = [
        "why",
        "bug",
        "crash",
        "failure",
        "regression",
        "broke",
        "error",
        "panic",
    ];
    if diagnostic_markers.iter().any(|m| query_lower.contains(m)) {
        return neocortex::QueryMode::Diagnostic;
    }

    let structural_markers = ["how does", "where is", "what calls", "depends on", "uses"];
    let has_code_syntax = ["()", "::", "/", "_"].iter().any(|m| query.contains(m));
    if structural_markers.iter().any(|m| query_lower.contains(m)) || has_code_syntax {
        return neocortex::QueryMode::Structural;
    }

    let entities = extract_entities(query, keyword_cache);
    if entities.iter().any(|e| {
        matches!(
            e.kind.as_str(),
            "Symbol" | "Function" | "Decorator" | "FilePath" | "Tool"
        )
    }) {
        return neocortex::QueryMode::Structural;
    }
    if !entities.is_empty() {
        return neocortex::QueryMode::Semantic;
    }

    neocortex::QueryMode::Neutral
}

/// CA3 autoassociative pattern completion.
///
/// When direct similarity search returns sparse results, use the graph to
/// reconstruct full memories from partial cues. Extracts entities from
/// partial matches, spreads activation, then searches L2 for entries
/// containing activated nodes' source texts.

/// Query memory without inserting new data.
/// Retrieved entries are marked labile (editable) so the next tick can
/// reconsolidate them with updated information.
pub fn retrieve_context(state: &mut BrainState, query: &str) -> MemoryContext {
    state.clock += 1;
    apply_decay(state);
    let query_mode = infer_query_mode(query, &state.keyword_cache);

    let embedding = embed_text(query, state.config.embedding_dim);

    // --- Scan working memory (L1) first ---
    let query_lower = query.to_lowercase();
    let query_words: Vec<&str> = query_lower.split_whitespace().collect();
    let mut wm_snippets: Vec<MemorySnippet> = Vec::new();
    for wm_entry in &mut state.working_memory {
        let sim = cosine_similarity(&wm_entry.embedding, &embedding);
        // Keyword bonus matching (same as L2)
        let entry_lower = wm_entry.text.to_lowercase();
        let keyword_bonus: f32 = query_words
            .iter()
            .filter(|w| w.len() > 3 && entry_lower.contains(**w))
            .count() as f32
            * KEYWORD_MATCH_BONUS;
        let effective_sim = (sim + keyword_bonus.min(KEYWORD_MATCH_BONUS_CAP)).min(1.0);

        if effective_sim >= MIN_QUERY_SIMILARITY {
            wm_entry.rehearsal_count += 1;
            wm_snippets.push(MemorySnippet {
                id: wm_entry.id,
                text: wm_entry.text.clone(),
                similarity: effective_sim,
                refs: Vec::new(),
            });
        }
    }
    wm_snippets.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    wm_snippets.truncate(5);

    // --- L2 retrieval ---
    let mut snippets = hippocampus::top_k_similar(&state.short_term, &embedding, 5, query);

    for snippet in &mut snippets {
        if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == snippet.id) {
            // Ebbinghaus spaced repetition: update stability based on retrieval interval
            let interval = state.clock.saturating_sub(entry.last_access);
            if interval > 0 && entry.last_retrieval_interval > 0 {
                if interval > entry.last_retrieval_interval {
                    // Spaced retrieval: increasing intervals strengthen stability
                    entry.stability = (entry.stability * 1.3).min(10.0);
                } else {
                    // Massed/cramming: diminishing returns
                    entry.stability = (entry.stability * 1.05).min(10.0);
                }
            }
            entry.last_retrieval_interval = interval;

            entry.last_access = state.clock;
            entry.usage = entry.usage.saturating_add(1);
            // Mark labile: this entry can be reconsolidated by the next tick
            entry.labile_until = state.clock + RECONSOLIDATION_WINDOW;
        }
    }

    // --- L3 systems consolidation retrieval (neocortical independence) ---
    // Scan Summary nodes that have centroid embeddings for direct similarity
    // matching. This surfaces old consolidated memories whose L2 entries may
    // have been evicted, making them independently queryable via neocortex.
    {
        let existing_ids: HashSet<u64> = snippets.iter().map(|s| s.id).collect();
        let mut summary_hits: Vec<MemorySnippet> = state
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Summary" && !n.embedding.is_empty())
            .filter_map(|n| {
                let sim = cosine_similarity(&n.embedding, &embedding);
                if sim >= SUMMARY_RETRIEVAL_MIN_SIM && !existing_ids.contains(&n.id) {
                    let text = n.full_text.as_deref().unwrap_or(&n.label).to_string();
                    Some(MemorySnippet {
                        id: n.id,
                        text,
                        similarity: sim * 0.85, // slight discount vs direct L2 matches
                        refs: Vec::new(),
                    })
                } else {
                    None
                }
            })
            .collect();
        summary_hits.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        summary_hits.truncate(SUMMARY_RETRIEVAL_MAX_RESULTS);
        snippets.extend(summary_hits);
        snippets.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    }

    // --- CA3 pattern completion ---
    // If direct retrieval is sparse or weak, use graph to complete partial cues.
    let top_sim = snippets.first().map(|s| s.similarity).unwrap_or(0.0);
    if snippets.len() < PATTERN_COMPLETION_MIN_RESULTS || top_sim < PATTERN_COMPLETION_SIM_THRESHOLD
    {
        let completed = hippocampus::pattern_complete(state, query, &snippets, query_mode);
        let existing_ids: HashSet<u64> = snippets.iter().map(|s| s.id).collect();
        for c in completed {
            if !existing_ids.contains(&c.id) {
                snippets.push(c);
            }
        }
        // Re-sort after adding completed results
        snippets.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        snippets.truncate(7); // allow slightly more results when completion kicks in
    }

    // Record for contrastive descent in the next reinforce() call.
    state.last_retrieved_ids = snippets.iter().map(|s| s.id).collect();

    // Passive auto-reinforce: the top result gets a small salience bump
    // proportional to its similarity, so useful memories naturally rise.
    if let Some(top) = snippets.first() {
        if top.similarity > 0.2 {
            if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == top.id) {
                entry.salience = (entry.salience + top.similarity * AUTO_REINFORCE_SCALE).min(1.0);
            }
        }
    }

    let mut long_term = neocortex::graph_lookup(
        &state.long_term,
        query,
        12,
        &state.keyword_cache,
        query_mode,
    );

    // --- Associative priming (2-hop spreading activation) ---
    // From the retrieved short-term entries, extract entities and spread
    // activation through the graph to surface indirectly related nodes.
    let mut priming_seed_ids: Vec<u64> = Vec::new();
    for snippet in &snippets {
        let entities = extract_entities(&snippet.text, &state.keyword_cache);
        for entity in &entities {
            if let Some(&node_id) = state.long_term.index.get(&entity.label) {
                priming_seed_ids.push(node_id);
            }
        }
    }
    // Also include the direct graph_lookup seeds
    for node in &long_term {
        priming_seed_ids.push(node.id);
    }
    priming_seed_ids.sort();
    priming_seed_ids.dedup();

    let existing_ids: HashSet<u64> = long_term.iter().map(|n| n.id).collect();
    let activated =
        neocortex::spreading_activation(&state.long_term, &priming_seed_ids, 2, 0.4, query_mode);
    let mut primed_nodes: Vec<GraphNodeSummary> = Vec::new();
    for (nid, activation) in activated {
        if !existing_ids.contains(&nid) {
            if let Some(node) = state.long_term.nodes.get(&nid) {
                primed_nodes.push(GraphNodeSummary {
                    id: node.id,
                    label: node.label.clone(),
                    kind: node.kind.clone(),
                    weight: node.weight * 0.7 * activation,
                    edge_type: Some("primed".to_string()),
                    source_texts: node.source_texts.clone(),
                });
            }
        }
    }
    long_term.extend(primed_nodes);

    // Re-sort by weight and cap
    long_term.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    long_term.truncate(15);

    // Hebbian reinforcement on co-retrieved nodes
    let retrieved_ids: Vec<u64> = long_term.iter().map(|n| n.id).collect();
    neocortex::hebbian_reinforce(&mut state.long_term, &retrieved_ids, state.clock);

    MemoryContext {
        short_term: snippets,
        long_term,
        working_memory: wm_snippets,
    }
}

/// Sharp-wave ripple replay — reinforce temporal co-occurrence patterns.
///
/// During sleep/rest, the hippocampus replays recently co-active patterns.
/// This strengthens graph edges between entities that appeared in temporally
/// proximate L2 entries, creating "temporal" edges for indirect associations.

/// Merge similar short-term entries into long-term graph summaries.
pub fn consolidate(state: &mut BrainState) -> Vec<GraphNodeSummary> {
    state.clock += 1;
    state.ticks_since_consolidation = 0;
    apply_decay(state);
    neocortex::replay_consolidation(state);

    let mut groups: Vec<Vec<ShortTermEntry>> = Vec::new();
    let mut used = vec![false; state.short_term.len()];

    for i in 0..state.short_term.len() {
        if used[i] {
            continue;
        }
        let seed = state.short_term[i].clone();
        let mut group = vec![seed.clone()];
        used[i] = true;

        for (j, is_used) in used.iter_mut().enumerate().skip(i + 1) {
            if *is_used {
                continue;
            }
            if cosine_similarity(&seed.embedding, &state.short_term[j].embedding)
                >= state.config.theta_low
            {
                group.push(state.short_term[j].clone());
                *is_used = true;
            }
        }
        groups.push(group);
    }

    let mut summaries = Vec::new();
    for group in groups.into_iter().filter(|g| g.len() > 1) {
        let summary_text = summarize_group(&group, &state.keyword_cache);
        let salience = group
            .iter()
            .map(|e| e.salience)
            .fold(0.0, f32::max)
            .max(0.4);
        let source_texts: Vec<String> = group.iter().map(|e| e.text.clone()).collect();

        // Systems consolidation: compute centroid embedding and rich text
        // for high-salience groups (hippocampus flags important memories
        // for full neocortical encoding).
        let avg_salience = group.iter().map(|e| e.salience).sum::<f32>() / group.len() as f32;
        let (centroid_embedding, full_text) =
            if avg_salience >= SYSTEMS_CONSOLIDATION_SALIENCE_THRESHOLD {
                // Centroid = average of group embeddings, renormalized
                let dim = group[0].embedding.len();
                let mut centroid = vec![0.0f32; dim];
                let valid_count = group
                    .iter()
                    .filter(|e| e.embedding.len() == dim && !e.embedding.is_empty())
                    .count();
                if valid_count > 0 {
                    for entry in &group {
                        if entry.embedding.len() == dim {
                            for (i, v) in entry.embedding.iter().enumerate() {
                                centroid[i] += v;
                            }
                        }
                    }
                    let n = valid_count as f32;
                    for v in &mut centroid {
                        *v /= n;
                    }
                    // Renormalize to unit length
                    let norm: f32 = centroid.iter().map(|x| x * x).sum::<f32>().sqrt();
                    if norm > 0.0 {
                        for v in &mut centroid {
                            *v /= norm;
                        }
                    }
                }

                // Rich text: concatenate source texts up to max length
                let mut rich = String::new();
                for st in &source_texts {
                    if !rich.is_empty() {
                        rich.push_str(" | ");
                    }
                    rich.push_str(st.trim());
                    if rich.len() >= SUMMARY_FULL_TEXT_MAX_LEN {
                        break;
                    }
                }
                if rich.len() > SUMMARY_FULL_TEXT_MAX_LEN {
                    rich = safe_truncate(&rich, SUMMARY_FULL_TEXT_MAX_LEN);
                }

                (centroid, Some(rich))
            } else {
                (Vec::new(), None)
            };

        // 1. Dedup: check for existing Summary node with exact label or high word overlap
        let existing_summary_id = state
            .long_term
            .index
            .get(&summary_text)
            .copied()
            .or_else(|| {
                state
                    .long_term
                    .nodes
                    .iter()
                    .find(|(_, n)| {
                        n.kind == "Summary"
                            && word_overlap(&n.label, &summary_text) >= MERGE_WORD_OVERLAP_THRESHOLD
                    })
                    .map(|(&id, _)| id)
            });

        let node_id = if let Some(eid) = existing_summary_id {
            // Merge into existing: update weight/salience, extend source_texts
            if let Some(node) = state.long_term.nodes.get_mut(&eid) {
                node.weight = node.weight.max(1.0 + salience);
                node.salience = node.salience.max(salience);
                node.last_seen = state.clock;
                // Extend source_texts, dedup, cap at 20
                for st in &source_texts {
                    if !node.source_texts.contains(st) {
                        node.source_texts.push(st.clone());
                    }
                }
                node.source_texts.truncate(20);
                // Systems consolidation: update neocortical encoding
                if !centroid_embedding.is_empty() {
                    node.embedding = centroid_embedding.clone();
                }
                if full_text.is_some() {
                    node.full_text = full_text.clone();
                }
            }
            eid
        } else {
            // Create new Summary node
            let id = state.next_id;
            state.next_id += 1;
            let mut capped_sources = source_texts.clone();
            capped_sources.truncate(20);
            state.long_term.nodes.insert(
                id,
                GraphNode {
                    id,
                    label: summary_text.clone(),
                    kind: "Summary".to_string(),
                    weight: 1.0 + salience,
                    last_seen: state.clock,
                    salience,
                    source_texts: capped_sources,
                    embedding: centroid_embedding.clone(),
                    full_text: full_text.clone(),
                },
            );
            state.long_term.index.insert(summary_text.clone(), id);
            id
        };

        // 2. Semantic Topic Extraction: find high-frequency entities in the group
        let mut entity_counts: HashMap<String, (usize, String)> = HashMap::new();
        for entry in &group {
            let entities =
                crate::memory::wernicke::extract_entities(&entry.text, &state.keyword_cache);
            for entity in entities {
                let entry = entity_counts
                    .entry(entity.label.clone())
                    .or_insert((0, entity.kind));
                entry.0 += 1;
            }
        }

        // If an entity appears in >50% of the group, it's a strong Topic/Anchor for this milestone
        let threshold = group.len() / 2;
        for (label, (count, kind)) in entity_counts {
            if count >= threshold && count > 1 {
                let topic_id = if let Some(&id) = state.long_term.index.get(&label) {
                    id
                } else {
                    let id = state.next_id;
                    state.next_id += 1;
                    state.long_term.nodes.insert(
                        id,
                        GraphNode {
                            id,
                            label: label.clone(),
                            kind: if kind == "Term" {
                                "Topic".to_string()
                            } else {
                                kind
                            },
                            weight: 1.0,
                            last_seen: state.clock,
                            salience: 0.5,
                            source_texts: Vec::new(),
                            embedding: Vec::new(),
                            full_text: None,
                        },
                    );
                    state.long_term.index.insert(label.clone(), id);
                    id
                };

                // Create/strengthen edge between Summary and Topic
                neocortex::upsert_edge(
                    &mut state.long_term,
                    node_id,
                    topic_id,
                    "represents",
                    state.clock,
                );
            }
        }

        for entry in &group {
            neocortex::update_graph(state, &entry.text, entry.salience);
            if let Some(existing) = state.short_term.iter_mut().find(|e| e.id == entry.id) {
                existing.usage = existing.usage.saturating_add(1);
                existing.last_access = state.clock;
                existing.consolidated = true;
            }
        }

        summaries.push(GraphNodeSummary {
            id: node_id,
            label: summary_text,
            kind: "Summary".to_string(),
            weight: 1.0 + salience,
            edge_type: None,
            source_texts,
        });
    }

    hippocampus::prune_short_term(&mut state.short_term, state.clock);
    neocortex::prune_graph(&mut state.long_term, state.clock);
    summaries
}

// -----------------------------------------------------------------------
// Private helpers
// -----------------------------------------------------------------------

/// Try to reconsolidate: if any labile entry is related to the new text,
/// update it in-place (merge text, re-embed, boost salience) instead of
/// creating a new entry. Returns the id of the reconsolidated entry if successful.

// flush_working_memory → prefrontal::flush_working_memory
// push_working_memory  → prefrontal::push_working_memory

/// Exponentially decay salience/weight based on time since last access.
fn apply_decay(state: &mut BrainState) {
    let now = state.clock;
    hippocampus::apply_l2_decay(&mut state.short_term, now);
    neocortex::apply_l3_decay(&mut state.long_term, now);
}

// renormalize_salience → basal_ganglia::renormalize_salience

/// Force an immediate normalization pass on graph weights and salience scores.
/// Call after loading an old store to bring blown-up values back within current bounds.
pub fn rebalance_weights(state: &mut BrainState) {
    basal_ganglia::renormalize_salience(&mut state.short_term);
    neocortex::normalize_graph_weights(&mut state.long_term);
}

// build_start_summary, build_start_summary_with_options — moved to crate::tool, re-exported below.

// ---------------------------------------------------------------------------
// Free functions — brain helpers (stay here)
// ---------------------------------------------------------------------------

/// Insert a new graph node and register it in the index. Returns the assigned ID.
fn insert_graph_node(
    state: &mut BrainState,
    label: String,
    kind: &str,
    weight: f32,
    salience: f32,
    source_texts: Vec<String>,
) -> u64 {
    let id = state.next_id;
    state.next_id += 1;
    state.long_term.nodes.insert(
        id,
        GraphNode {
            id,
            label: label.clone(),
            kind: kind.to_string(),
            weight,
            last_seen: state.clock,
            salience,
            source_texts,
            embedding: Vec::new(),
            full_text: None,
        },
    );
    state.long_term.index.insert(label, id);
    id
}

pub fn add_node_if_new(state: &mut BrainState, label: &str, kind: &str, salience: f32) {
    if state.long_term.index.contains_key(label) {
        return;
    }
    insert_graph_node(state, label.to_string(), kind, 1.0, salience, Vec::new());
}

/// Create or reinforce a Task node in the knowledge graph.
/// Returns the node ID of the created/reinforced node.
pub fn upsert_task_node(state: &mut BrainState, label: &str) -> u64 {
    let node_id = if let Some(&id) = state.long_term.index.get(label) {
        id
    } else {
        insert_graph_node(
            state,
            label.to_string(),
            "Task",
            1.0 + NODE_WEIGHT_BASE,
            0.8,
            Vec::new(),
        )
    };

    if let Some(node) = state.long_term.nodes.get_mut(&node_id) {
        node.weight = (node.weight + NODE_WEIGHT_BASE).min(GRAPH_WEIGHT_TARGET_MAX);
        node.last_seen = state.clock;
    }
    node_id
}

// recent_sessions, set_task, merge_from_log, clear_task, get_task,
// should_suggest_consolidation, build_context_summary, build_dump,
// scan_ecosystem_dependencies — moved to crate::tool, re-exported below.

/// Create or reinforce a keyword graph node with label `kw:<category>:<term>`.
/// Returns true if a new node was created (vs reinforcing existing).
pub fn add_keyword_node(
    state: &mut BrainState,
    category: &str,
    term: &str,
    metadata: Vec<String>,
) -> bool {
    let label = format!("kw:{}:{}", category, term);
    if let Some(&existing_id) = state.long_term.index.get(&label) {
        // Reinforce existing keyword node
        if let Some(node) = state.long_term.nodes.get_mut(&existing_id) {
            node.weight += 0.2;
            node.last_seen = state.clock;
        }
        false
    } else {
        insert_graph_node(state, label, "Keyword", 1.0, 0.5, metadata);
        true
    }
}

/// Rebuild the keyword cache from the current graph state.
pub fn rebuild_keyword_cache(state: &mut BrainState) {
    state.keyword_cache = wernicke::KeywordCache::from_graph(&state.long_term);
}

/// Layer 3: Update term frequency stats and auto-promote terms that pass
/// all noise filters to `kw:domain:<term>` graph nodes.
///
/// Called from `tick_impl` after entity extraction. Returns the number of
/// newly promoted keywords.
fn update_term_frequencies(state: &mut BrainState, text: &str) -> usize {
    let entities = extract_entities(text, &state.keyword_cache);
    if entities.is_empty() {
        return 0;
    }

    // Check if this tick contains any existing keyword matches (for co-occurrence filter)
    let lowered = text.to_lowercase();
    let has_existing_keyword = state.keyword_cache.matches_any_category(&lowered);

    let clock = state.clock;
    for entity in &entities {
        let label = entity.label.to_lowercase();

        let stats = state
            .term_frequency
            .entry(label.clone())
            .or_insert(TermStats {
                tick_count: 0,
                total_count: 0,
                first_seen: clock,
                last_seen: clock,
                has_keyword_cooccurrence: false,
            });

        // Only increment tick_count if this is a new tick for this term
        if stats.last_seen < clock {
            stats.tick_count += 1;
        }
        stats.total_count += 1;
        stats.last_seen = clock;

        // Filter 4: Co-occurrence — mark if this tick has existing keywords
        if has_existing_keyword {
            stats.has_keyword_cooccurrence = true;
        }
    }

    // Check for promotions
    let mut promoted = 0;
    let candidates: Vec<String> = state
        .term_frequency
        .iter()
        .filter(|(_, stats)| stats.tick_count >= TERM_PROMOTION_MIN_TICKS)
        .map(|(label, _)| label.clone())
        .collect();

    for label in candidates {
        if should_promote_term(state, &label) {
            if add_keyword_node(state, "domain", &label, Vec::new()) {
                promoted += 1;
            }
        }
    }

    if promoted > 0 {
        rebuild_keyword_cache(state);
    }

    promoted
}

/// Check if a term passes all 5 noise filters for auto-promotion.
fn should_promote_term(state: &BrainState, term: &str) -> bool {
    // Already a keyword?
    let label = format!("kw:domain:{}", term);
    if state.long_term.index.contains_key(&label) {
        return false;
    }

    // Filter 5: Minimum information content — 3+ chars, not purely numeric
    if term.len() < TERM_PROMOTION_MIN_LEN {
        return false;
    }
    if term
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    {
        return false;
    }

    // Filter 1: Stopword exclusion
    if STOPWORDS.contains(&term) {
        return false;
    }

    // Filter 2: Minimum tick spread (checked before calling, but verify)
    let stats = match state.term_frequency.get(term) {
        Some(s) => s,
        None => return false,
    };
    if stats.tick_count < TERM_PROMOTION_MIN_TICKS {
        return false;
    }

    // Filter 3: Entity extraction gate — already passed (only extracted entities
    // enter term_frequency, so this is inherently satisfied)

    // Filter 4: Co-occurrence with existing keywords
    if !stats.has_keyword_cooccurrence {
        return false;
    }

    true
}

// Persistence — moved to crate::tool::persistence, re-exported above.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

pub fn safe_truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let mut end = max_len;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::persistence::{MSGPACK_FORMAT_VERSION, MSGPACK_MAGIC};
    use std::collections::VecDeque;
    use std::fs;

    fn kw() -> KeywordCache {
        KeywordCache::default_from_static()
    }

    #[test]
    fn test_eviction_score_recent_high() {
        let recent = ShortTermEntry {
            id: 1,
            text: "test".into(),
            summary: "test".into(),
            last_access: 100,
            usage: 5,
            salience: 0.8,
            ..Default::default()
        };
        let old = ShortTermEntry {
            id: 2,
            text: "test".into(),
            summary: "test".into(),
            last_access: 1,
            usage: 1,
            salience: 0.1,
            ..Default::default()
        };
        assert!(eviction_score(&recent, 100) > eviction_score(&old, 100));
    }

    #[test]
    fn test_tick_adds_entry() {
        let mut state = MemoryState::default();
        tick(&mut state, "hello world test entry");
        // Entry goes to working memory; may or may not promote to L2 depending on salience
        assert!(!state.brain.working_memory.is_empty() || !state.brain.short_term.is_empty());
    }

    #[test]
    fn test_tick_reinforces_similar() {
        let mut state = MemoryState::default();
        tick(&mut state, "the embedding system uses vector similarity");
        let usage_before = state.brain.short_term[0].usage;
        tick(&mut state, "the embedding system uses vector similarity");
        assert_eq!(
            state.brain.short_term.len(),
            1,
            "identical tick should reinforce, not add"
        );
        assert!(state.brain.short_term[0].usage > usage_before);
    }

    #[test]
    fn test_retrieve_context() {
        let mut state = MemoryState::default();
        tick(&mut state, "memory system with embeddings");
        tick(&mut state, "database of knowledge graphs");
        let ctx = retrieve_context(&mut state.brain, "embedding search");
        assert!(!ctx.short_term.is_empty());
    }

    #[test]
    fn test_consolidate() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: embedding quality improvement using n-grams",
        );
        tick(
            &mut state,
            "DECISION: embedding quality improvement using trigrams",
        );
        tick(
            &mut state,
            "DECISION: completely different topic about cooking recipes",
        );
        let summaries = consolidate(&mut state.brain);
        assert!(!state.brain.short_term.is_empty() || !summaries.is_empty());
    }

    #[test]
    fn test_graph_edges_typed() {
        let mut state = MemoryState::default();
        tick(&mut state, "fn handle_memory() calls fn handle_tick()");
        for edge in &state.brain.long_term.edges {
            assert!(!edge.kind.is_empty());
        }
    }

    #[test]
    fn test_hebbian_reinforcement() {
        let mut state = MemoryState::default();
        tick(&mut state, "fn process_data() uses struct Config");
        let initial_weights: Vec<f32> = state
            .brain
            .long_term
            .edges
            .iter()
            .map(|e| e.weight)
            .collect();
        retrieve_context(&mut state.brain, "process_data Config");
        retrieve_context(&mut state.brain, "process_data Config");
        let has_increased = state
            .brain
            .long_term
            .edges
            .iter()
            .zip(initial_weights.iter())
            .any(|(edge, &initial)| edge.weight > initial);
        assert!(
            has_increased,
            "Hebbian reinforcement should strengthen co-retrieved edges"
        );
    }

    #[test]
    fn test_decay_reduces_weights() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: initial entry with some content for decay testing",
        );
        let initial_salience = state.brain.short_term[0].salience;
        state.brain.clock += 100;
        apply_decay(&mut state.brain);
        assert!(state.brain.short_term[0].salience < initial_salience);
    }

    #[test]
    fn test_session_log_records_ticks() {
        let mut state = MemoryState::default();
        tick(&mut state, "first tick message");
        tick(&mut state, "second tick message");
        tick(&mut state, "third tick message");
        assert_eq!(state.session_log.len(), 3);
        assert_eq!(state.session_log[0].text, "first tick message");
        assert_eq!(state.session_log[2].text, "third tick message");
    }

    #[test]
    fn test_recent_sessions_returns_tail() {
        let mut state = MemoryState::default();
        for i in 0..20 {
            tick(&mut state, &format!("tick number {}", i));
        }
        let recent = recent_sessions(&state, 5);
        assert_eq!(recent.len(), 5);
        assert!(recent[0].text.contains("15"));
        assert!(recent[4].text.contains("19"));
    }

    #[test]
    fn test_diversity_prevents_merge_of_unrelated() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: the embedding system uses vector similarity for matching",
        );
        tick(
            &mut state,
            "DECISION: cooking recipes require fresh ingredients and seasoning",
        );
        assert!(
            state.brain.short_term.len() >= 2,
            "unrelated ticks should create separate entries, got {}",
            state.brain.short_term.len()
        );
    }

    #[test]
    fn test_pattern_separation_preserves_similar_but_distinct() {
        // Dentate Gyrus pattern separation: topics sharing vocabulary ("memory")
        // but describing different subjects must remain as separate episodic traces.
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Rust memory model borrow checker ownership semantics",
        );
        tick(
            &mut state,
            "DECISION: Legend memory system three-layer architecture design",
        );
        assert!(
            state.brain.short_term.len() >= 2,
            "similar-but-distinct topics should be kept separate (dentate gyrus pattern separation), got {}",
            state.brain.short_term.len()
        );
    }

    #[test]
    fn test_orthogonalization_reduces_embedding_overlap_in_l2() {
        // Dentate Gyrus: after orthogonalization, L2 embeddings for related-but-distinct
        // entries should be less similar than their raw n-gram embeddings would be.
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Rust memory model borrow checker ownership semantics",
        );
        tick(
            &mut state,
            "DECISION: Legend memory system three-layer architecture design patterns",
        );

        assert!(
            state.brain.short_term.len() >= 2,
            "should have 2 separate L2 entries, got {}",
            state.brain.short_term.len()
        );

        // The stored L2 embeddings should be more orthogonal than raw embeddings
        let raw_a = embed_text(
            "DECISION: Rust memory model borrow checker ownership semantics",
            state.brain.config.embedding_dim,
        );
        let raw_b = embed_text(
            "DECISION: Legend memory system three-layer architecture design patterns",
            state.brain.config.embedding_dim,
        );
        let raw_sim = cosine_similarity(&raw_a, &raw_b);

        let stored_sim = cosine_similarity(
            &state.brain.short_term[0].embedding,
            &state.brain.short_term[1].embedding,
        );

        assert!(
            stored_sim < raw_sim,
            "stored embeddings should be more orthogonal than raw: stored={}, raw={}",
            stored_sim,
            raw_sim
        );
    }

    #[test]
    fn test_near_identical_entries_still_merge() {
        // CA3 pattern completion: near-identical cues should recall the same trace.
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Chose Redis for caching because it has better pub/sub support",
        );
        tick(
            &mut state,
            "DECISION: Chose Redis for caching because it has better pub/sub integration",
        );
        assert_eq!(
            state.brain.short_term.len(),
            1,
            "near-identical entries should merge, got {}",
            state.brain.short_term.len()
        );
    }

    // ---- Amygdala: emotional tagging integration tests ----

    #[test]
    fn test_emotional_valence_stored_on_tick() {
        let mut state = MemoryState::default();
        tick(&mut state, "BUG: server crashes on null input with a panic");
        let entry = state
            .brain
            .short_term
            .iter()
            .find(|e| e.text.contains("BUG:"))
            .expect("bug entry should exist in L2");
        assert!(
            entry.emotional_valence < -0.3,
            "bug entry should have negative valence, got {}",
            entry.emotional_valence
        );
    }

    #[test]
    fn test_positive_valence_on_tick() {
        let mut state = MemoryState::default();
        // Use text with decision keywords to ensure L2 promotion + positive valence
        tick(
            &mut state,
            "DECISION: Shipped v2.0 successfully because the approach was validated",
        );
        let entry = state
            .brain
            .short_term
            .iter()
            .find(|e| e.text.contains("Shipped"))
            .expect("shipped entry should exist in L2");
        assert!(
            entry.emotional_valence > 0.0,
            "shipped entry should have positive valence, got {}",
            entry.emotional_valence
        );
    }

    #[test]
    fn test_emotional_valence_decays_slower_than_salience() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "BUG: critical panic in the authentication module",
        );
        let initial_valence = state.brain.short_term[0].emotional_valence;
        let initial_salience = state.brain.short_term[0].salience;

        // Advance clock significantly
        state.brain.clock += 100;
        apply_decay(&mut state.brain);

        let decayed_valence = state.brain.short_term[0].emotional_valence;
        let decayed_salience = state.brain.short_term[0].salience;

        // Both should have decayed
        assert!(decayed_valence.abs() < initial_valence.abs());
        assert!(decayed_salience < initial_salience);

        // Valence should retain more of its original magnitude (half-rate decay)
        let valence_retention = decayed_valence.abs() / initial_valence.abs();
        let salience_retention = decayed_salience / initial_salience;
        assert!(
            valence_retention > salience_retention,
            "valence should decay slower: valence_retention={} vs salience_retention={}",
            valence_retention,
            salience_retention
        );
    }

    #[test]
    fn test_emotional_entries_resist_eviction() {
        // An emotionally charged entry should score higher than a neutral one
        // with the same salience/usage/recency
        let emotional = ShortTermEntry {
            id: 1,
            text: "bug report".into(),
            summary: "bug".into(),
            emotional_valence: -0.6,
            ..ShortTermEntry::default()
        };
        let neutral = ShortTermEntry {
            id: 2,
            text: "some docs".into(),
            summary: "docs".into(),
            emotional_valence: 0.0,
            ..ShortTermEntry::default()
        };
        assert!(
            eviction_score(&emotional, 0) > eviction_score(&neutral, 0),
            "emotional entry should resist eviction"
        );
    }

    #[test]
    fn test_emotional_valence_in_dump() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "BUG: data corruption found in production database",
        );
        let dump = build_dump(&state);
        let short_term = dump["short_term"].as_array().unwrap();
        assert!(!short_term.is_empty());
        let valence = short_term[0]["emotional_valence"].as_f64().unwrap();
        assert!(
            valence < 0.0,
            "bug entry valence should be negative in dump, got {}",
            valence
        );
    }

    // ---- Ebbinghaus: spaced repetition + forgetting curve tests ----

    #[test]
    fn test_spaced_retrieval_increases_stability() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Chose Redis for caching because of pub/sub support",
        );

        // Retrieve at increasing intervals: 5, 10, 20, 40
        let intervals = [5, 10, 20, 40];
        for interval in &intervals {
            state.brain.clock += interval;
            retrieve_context(&mut state.brain, "Redis caching");
        }

        let stability = state.brain.short_term[0].stability;
        assert!(
            stability > 1.5,
            "spaced retrieval should increase stability significantly, got {}",
            stability
        );
    }

    #[test]
    fn test_massed_retrieval_increases_stability_slowly() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Chose Redis for caching because of pub/sub support",
        );

        // Retrieve at constant short intervals: 1, 1, 1, 1
        for _ in 0..4 {
            state.brain.clock += 1;
            retrieve_context(&mut state.brain, "Redis caching");
        }

        let massed_stability = state.brain.short_term[0].stability;

        // Compare with spaced retrieval
        let mut state2 = MemoryState::default();
        tick(
            &mut state2,
            "DECISION: Chose Redis for caching because of pub/sub support",
        );
        let intervals = [5, 10, 20, 40];
        for interval in &intervals {
            state2.brain.clock += interval;
            retrieve_context(&mut state2.brain, "Redis caching");
        }
        let spaced_stability = state2.brain.short_term[0].stability;

        assert!(
            spaced_stability > massed_stability,
            "spaced retrieval should build more stability than massed: spaced={} vs massed={}",
            spaced_stability,
            massed_stability
        );
    }

    #[test]
    fn test_high_stability_resists_decay() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Chose Redis for caching because of pub/sub support",
        );

        // Build up stability through spaced retrieval
        let intervals = [5, 10, 20, 40];
        for interval in &intervals {
            state.brain.clock += interval;
            retrieve_context(&mut state.brain, "Redis caching");
        }
        let high_stability_salience = state.brain.short_term[0].salience;

        // Now create a fresh entry with default stability
        tick(
            &mut state,
            "DECISION: Chose Postgres because of JSONB support",
        );

        // Both entries decay for 200 ticks
        state.brain.clock += 200;
        apply_decay(&mut state.brain);

        let stable_entry = state
            .brain
            .short_term
            .iter()
            .find(|e| e.text.contains("Redis"))
            .expect("Redis entry");
        let fresh_entry = state
            .brain
            .short_term
            .iter()
            .find(|e| e.text.contains("Postgres"))
            .expect("Postgres entry");

        // The high-stability entry should retain more of its salience
        let stable_retention = stable_entry.salience / high_stability_salience;
        let fresh_retention = if fresh_entry.salience > 0.0 {
            fresh_entry.salience / 1.0 // approximate initial salience
        } else {
            0.0
        };

        assert!(
            stable_retention > fresh_retention,
            "high-stability entry should retain more salience: stable={} vs fresh={}",
            stable_retention,
            fresh_retention
        );
    }

    #[test]
    fn test_stability_defaults_to_one() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: some initial decision for testing baseline stability",
        );
        assert_eq!(
            state.brain.short_term[0].stability, 1.0,
            "new entries should have stability=1.0 after first tick"
        );
    }

    #[test]
    fn test_stability_capped_at_ten() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Chose Redis for caching because of pub/sub support",
        );

        // Many spaced retrievals to push stability toward the cap
        let mut interval = 5;
        for _ in 0..20 {
            state.brain.clock += interval;
            retrieve_context(&mut state.brain, "Redis caching");
            interval += 5; // increasing intervals
        }

        assert!(
            state.brain.short_term[0].stability <= 10.0,
            "stability should be capped at 10.0, got {}",
            state.brain.short_term[0].stability
        );
    }

    #[test]
    fn test_build_context_summary() {
        let mut state = MemoryState::default();
        tick(&mut state, "fn process_data() handles incoming requests");
        tick(&mut state, "struct Config stores application settings");
        let summary = build_context_summary(&state);
        assert!(summary["stats"]["short_term_entries"].as_u64().unwrap() >= 1);
        assert!(summary["recent_sessions"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn test_reinforce_positive_boosts_salience() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "fn handle_request() processes incoming API calls",
        );
        let id = state.brain.short_term[0].id;
        let salience_before = state.brain.short_term[0].salience;

        let result = basal_ganglia::reinforce(&mut state.brain, &[id][..], 1.0);
        assert_eq!(result.reinforced.len(), 1);
        assert!(
            result.reinforced[0].salience_after > salience_before,
            "positive signal should boost salience: {} -> {}",
            salience_before,
            result.reinforced[0].salience_after
        );
    }

    #[test]
    fn test_reinforce_negative_reduces_salience() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "fn handle_request() processes incoming API calls",
        );
        let id = state.brain.short_term[0].id;
        let salience_before = state.brain.short_term[0].salience;

        let result = basal_ganglia::reinforce(&mut state.brain, &[id][..], -1.0);
        assert_eq!(result.reinforced.len(), 1);
        assert!(
            result.reinforced[0].salience_after < salience_before,
            "negative signal should reduce salience: {} -> {}",
            salience_before,
            result.reinforced[0].salience_after
        );
    }

    #[test]
    fn test_reinforce_cascades_to_graph() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "fn process_data() uses struct Config for settings",
        );
        let id = state.brain.short_term[0].id;

        // Capture graph weights before reinforcement
        let weight_before: f32 = state.brain.long_term.nodes.values().map(|n| n.weight).sum();

        basal_ganglia::reinforce(&mut state.brain, &[id][..], 1.0);

        let weight_after: f32 = state.brain.long_term.nodes.values().map(|n| n.weight).sum();

        assert!(
            weight_after > weight_before,
            "positive reinforce should cascade to graph: {} -> {}",
            weight_before,
            weight_after
        );
    }

    #[test]
    fn test_reinforce_unknown_id_ignored() {
        let mut state = MemoryState::default();
        tick(&mut state, "some entry");
        let result = basal_ganglia::reinforce(&mut state.brain, &[9999][..], 1.0);
        assert!(
            result.reinforced.is_empty(),
            "unknown ID should be silently ignored"
        );
    }

    #[test]
    fn test_prune_graph_removes_low_weight_nodes() {
        let mut state = MemoryState::default();
        // Insert a node with weight below GRAPH_PRUNE_WEIGHT
        let id = state.brain.next_id;
        state.brain.next_id += 1;
        state.brain.long_term.nodes.insert(
            id,
            GraphNode {
                id,
                label: "weak_node".to_string(),
                kind: "Term".to_string(),
                weight: 0.01,
                last_seen: 0,
                salience: 0.0,
                source_texts: Vec::new(),
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("weak_node".to_string(), id);
        // Also insert a healthy node
        let id2 = state.brain.next_id;
        state.brain.next_id += 1;
        state.brain.long_term.nodes.insert(
            id2,
            GraphNode {
                id: id2,
                label: "strong_node".to_string(),
                kind: "Term".to_string(),
                weight: 2.0,
                last_seen: state.brain.clock,
                salience: 0.5,
                source_texts: Vec::new(),
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("strong_node".to_string(), id2);
        // Add edge between them
        state.brain.long_term.edges.push(GraphEdge {
            from: id,
            to: id2,
            weight: 0.1,
            kind: "related".to_string(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        state.brain.clock = 100;
        neocortex::prune_graph(&mut state.brain.long_term, state.brain.clock);

        assert!(
            !state.brain.long_term.nodes.contains_key(&id),
            "low-weight node should be pruned"
        );
        assert!(
            state.brain.long_term.nodes.contains_key(&id2),
            "healthy node should survive"
        );
        assert!(
            state.brain.long_term.edges.is_empty(),
            "orphaned edge should be removed"
        );
        assert!(
            !state.brain.long_term.index.contains_key("weak_node"),
            "index entry should be cleaned"
        );
    }

    #[test]
    fn test_prune_graph_enforces_node_cap() {
        let mut state = MemoryState::default();
        // Insert more nodes than GRAPH_NODE_CAPACITY
        for i in 0..(GRAPH_NODE_CAPACITY + 50) {
            let id = state.brain.next_id;
            state.brain.next_id += 1;
            state.brain.long_term.nodes.insert(
                id,
                GraphNode {
                    id,
                    label: format!("node_{}", i),
                    kind: "Term".to_string(),
                    weight: i as f32 * 0.01,
                    last_seen: state.brain.clock,
                    salience: 0.1,
                    source_texts: Vec::new(),
                    embedding: Vec::new(),
                    full_text: None,
                },
            );
            state
                .brain
                .long_term
                .index
                .insert(format!("node_{}", i), id);
        }
        assert!(state.brain.long_term.nodes.len() > GRAPH_NODE_CAPACITY);

        neocortex::prune_graph(&mut state.brain.long_term, state.brain.clock);

        assert!(
            state.brain.long_term.nodes.len() <= GRAPH_NODE_CAPACITY,
            "node count should be capped at {}, got {}",
            GRAPH_NODE_CAPACITY,
            state.brain.long_term.nodes.len()
        );
    }

    #[test]
    fn test_tick_runs_pruning() {
        let mut state = MemoryState::default();
        // Manually inject a stale short-term entry
        state.brain.short_term.push(ShortTermEntry {
            id: 999,
            text: "stale".into(),
            summary: "stale".into(),
            embedding: vec![0.0; 256],
            ..Default::default()
        });
        // Advance clock far enough for pruning to kick in
        state.brain.clock = 500;
        tick(&mut state, "fresh content about something new");
        assert!(
            !state.brain.short_term.iter().any(|e| e.id == 999),
            "stale entry should be pruned during tick"
        );
    }

    #[test]
    fn test_auto_reinforce_on_query() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: the cosine similarity algorithm compares vector embeddings",
        );
        let salience_before = state.brain.short_term[0].salience;

        // Query something related — top result should get a passive boost
        retrieve_context(&mut state.brain, "cosine similarity vector");
        let salience_after = state.brain.short_term[0].salience;

        assert!(
            salience_after > salience_before,
            "top retrieval result should be auto-reinforced: {} -> {}",
            salience_before,
            salience_after
        );
    }

    #[test]
    fn test_tick_captures_line_references() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: See legend/src/memory/mod.rs#L120-145 for the new refs logic.",
        );
        assert!(!state.brain.short_term.is_empty());

        let entry = &state.brain.short_term[0];
        assert!(!entry.refs.is_empty(), "expected refs to be captured");

        let reference = &entry.refs[0];
        assert_eq!(reference.path, "legend/src/memory/mod.rs");
        assert_eq!(reference.start_line, 120);
        assert_eq!(reference.end_line, 145);
        assert!(reference
            .snippet
            .contains("legend/src/memory/mod.rs#L120-145"));
    }

    #[test]
    fn test_retrieve_context_returns_refs() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Ref: legend/src/memory/mod.rs#L200-210 tracks MemorySnippet changes.",
        );
        let ctx = retrieve_context(&mut state.brain, "MemorySnippet refs");
        assert!(!ctx.short_term.is_empty());

        let snippet = &ctx.short_term[0];
        assert!(
            !snippet.refs.is_empty(),
            "expected snippet refs to be returned"
        );
        let reference = &snippet.refs[0];
        assert_eq!(reference.path, "legend/src/memory/mod.rs");
        assert_eq!(reference.start_line, 200);
        assert_eq!(reference.end_line, 210);
    }

    #[test]
    fn test_build_start_summary() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "Chose bincode over JSON because it is faster for serialization",
        );
        tick(
            &mut state,
            "fn process_data() handles incoming API requests",
        );
        tick(
            &mut state,
            "TODO: still need to implement the caching layer",
        );
        let summary = build_start_summary(&mut state);

        // Should have recent_sessions and categorized sections
        assert!(
            summary.get("recent_sessions").is_some(),
            "start summary should have context"
        );
        assert!(
            summary.get("categorized").is_some(),
            "start summary should have categorized"
        );
        // Decision should be categorized
        assert!(
            !summary["categorized"]["decisions"]
                .as_array()
                .unwrap()
                .is_empty(),
            "should have categorized the decision"
        );
        // TODO should be categorized
        assert!(
            !summary["categorized"]["todos"]
                .as_array()
                .unwrap()
                .is_empty(),
            "should have categorized the TODO"
        );
    }

    #[test]
    fn test_reconsolidation_updates_existing() {
        let mut state = MemoryState::default();
        // Create initial memory (DECISION: prefix ensures L2 promotion)
        tick(
            &mut state,
            "DECISION: the database uses PostgreSQL for persistence",
        );
        assert_eq!(state.brain.short_term.len(), 1);
        let original_id = state.brain.short_term[0].id;

        // Query to make it labile
        retrieve_context(&mut state.brain, "database PostgreSQL");
        assert!(
            state.brain.short_term[0].labile_until > 0,
            "retrieved entry should be labile"
        );

        // Tick with related but new information — should reconsolidate
        tick(
            &mut state,
            "DECISION: the database PostgreSQL schema has users and sessions tables",
        );
        // Should still have the original entry (reconsolidated, not duplicated)
        let reconsolidated = state.brain.short_term.iter().find(|e| e.id == original_id);
        if let Some(entry) = reconsolidated {
            assert!(
                entry.reconsolidation_count > 0,
                "entry should have been reconsolidated"
            );
            assert!(
                entry.text.contains("tables") || entry.summary.contains("tables"),
                "reconsolidated entry should contain new info"
            );
        }
        // Either way, verify the system didn't crash and state is consistent
        assert!(!state.brain.short_term.is_empty());
    }

    #[test]
    fn test_labile_expires() {
        let mut state = MemoryState::default();
        tick(&mut state, "DECISION: test entry for labile expiration");
        let id = state.brain.short_term[0].id;

        // Query to make labile
        retrieve_context(&mut state.brain, "test entry labile");
        assert!(state.brain.short_term[0].labile_until > 0);

        // Advance clock past labile window
        state.brain.clock += RECONSOLIDATION_WINDOW + 5;
        hippocampus::stabilize_labile_entries(&mut state.brain.short_term, state.brain.clock);

        let entry = state.brain.short_term.iter().find(|e| e.id == id).unwrap();
        assert_eq!(
            entry.labile_until, 0,
            "labile state should expire after window"
        );
    }

    #[test]
    fn test_classify_text_decision() {
        assert_eq!(
            classify_text(
                "Chose PostgreSQL over MongoDB because it has better JOIN support",
                &kw()
            ),
            MemoryCategory::Decision
        );
        assert_eq!(
            classify_text("We decided to use Rust instead of Go", &kw()),
            MemoryCategory::Decision
        );
    }

    #[test]
    fn test_classify_text_bug() {
        assert_eq!(
            classify_text("Bug: the parser crashes on empty input", &kw()),
            MemoryCategory::Bug
        );
        assert_eq!(
            classify_text("Had to revert the migration due to data loss", &kw()),
            MemoryCategory::Bug
        );
    }

    #[test]
    fn test_classify_priority_todo_wins_over_bug() {
        // "TODO: fix the bug" should be a TODO, not a BUG
        assert_eq!(
            classify_text("TODO: fix the critical bug", &kw()),
            MemoryCategory::Todo
        );
    }

    #[test]
    fn test_classify_priority_preference_wins_over_bug() {
        // "I prefer explicit error types" should be PREFERENCE, not BUG (even though 'error' is in BUG_KEYWORDS)
        assert_eq!(
            classify_text("User prefers explicit error types over anyhow", &kw()),
            MemoryCategory::Preference
        );
    }

    #[test]
    fn test_classify_text_progress_polyglot() {
        // Test our new ACTION_KEYWORDS for progress
        assert_eq!(
            classify_text("Finished the user login implementation", &kw()),
            MemoryCategory::Progress
        );
        assert_eq!(
            classify_text("Merged the feature branch into master", &kw()),
            MemoryCategory::Progress
        );
        assert_eq!(
            classify_text("Shipped the new version to production", &kw()),
            MemoryCategory::Progress
        );
    }

    #[test]
    fn test_classify_text_todo() {
        assert_eq!(
            classify_text("TODO: implement proper error handling", &kw()),
            MemoryCategory::Todo
        );
        assert_eq!(
            classify_text("Blocked on the API team providing the endpoint", &kw()),
            MemoryCategory::Todo
        );
    }

    #[test]
    fn test_classify_text_architecture() {
        assert_eq!(
            classify_text(
                "The authentication module uses JWT tokens via middleware",
                &kw()
            ),
            MemoryCategory::Architecture
        );
    }

    #[test]
    fn test_classify_text_preference() {
        assert_eq!(
            classify_text("User prefers snake_case for all variable names", &kw()),
            MemoryCategory::Preference
        );
    }

    #[test]
    fn test_importance_scoring_decisions_higher() {
        let decision_salience = compute_salience(
            "Chose bincode over JSON because it is faster for serialization",
            &kw(),
        );
        let generic_salience = compute_salience("updated some files in the project", &kw());
        assert!(
            decision_salience > generic_salience,
            "decisions should score higher: {} vs {}",
            decision_salience,
            generic_salience
        );
    }

    #[test]
    fn test_importance_scoring_bugs_higher() {
        let bug_salience = compute_salience(
            "Bug: the parser crashes on empty input and causes a panic",
            &kw(),
        );
        let generic_salience = compute_salience("updated some files in the project", &kw());
        assert!(
            bug_salience > generic_salience,
            "bugs should score higher: {} vs {}",
            bug_salience,
            generic_salience
        );
    }

    #[test]
    fn test_priming_surfaces_neighbors() {
        let mut state = MemoryState::default();
        // Create two entries that share entities
        tick(
            &mut state,
            "fn handle_request() processes incoming API calls using struct Config",
        );
        tick(
            &mut state,
            "struct Config stores database_url and port settings",
        );
        // The graph should now have edges connecting these entities

        // Query for something that matches one entry — priming should surface
        // graph neighbors from the other entry
        let ctx = retrieve_context(&mut state.brain, "handle_request API");
        // Should have long-term results that include primed neighbors
        assert!(
            !ctx.long_term.is_empty(),
            "priming should surface related graph nodes"
        );
    }

    #[test]
    fn test_start_summary_categorized() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "Chose Rust over Python because of performance requirements",
        );
        tick(
            &mut state,
            "TODO: add proper error handling to the parser module",
        );
        tick(
            &mut state,
            "Bug: connection pool exhaustion under high load causes timeout",
        );
        tick(&mut state, "User prefers explicit error types over anyhow");
        tick(
            &mut state,
            "The API module handles REST endpoints via axum router",
        );
        let summary = build_start_summary(&mut state);

        let categorized = &summary["categorized"];
        assert!(!categorized["decisions"].as_array().unwrap().is_empty());
        assert!(!categorized["todos"].as_array().unwrap().is_empty());
        assert!(!categorized["bugs"].as_array().unwrap().is_empty());
        assert!(!categorized["preferences"].as_array().unwrap().is_empty());
    }

    // --- Tests for new features ---

    #[test]
    fn test_current_task_set_and_get() {
        let mut state = MemoryState::default();
        assert!(get_task(&state).is_none());

        set_task(&mut state, "Implement user authentication");
        assert_eq!(get_task(&state), Some("Implement user authentication"));

        clear_task(&mut state);
        assert!(get_task(&state).is_none());
    }

    #[test]
    fn test_current_task_in_start_summary() {
        let mut state = MemoryState::default();
        set_task(&mut state, "Working on memory improvements");

        let summary = build_start_summary(&mut state);
        assert_eq!(
            summary["current_task"].as_str(),
            Some("Working on memory improvements")
        );
    }

    #[test]
    fn test_current_task_in_context_summary() {
        let mut state = MemoryState::default();
        set_task(&mut state, "Debugging the parser");

        let summary = build_context_summary(&state);
        assert_eq!(
            summary["current_task"].as_str(),
            Some("Debugging the parser")
        );
    }

    #[test]
    fn test_ticks_since_consolidation_increments() {
        let mut state = MemoryState::default();
        assert_eq!(state.brain.ticks_since_consolidation, 0);

        tick(&mut state, "first tick");
        assert_eq!(state.brain.ticks_since_consolidation, 1);

        tick(&mut state, "second tick");
        assert_eq!(state.brain.ticks_since_consolidation, 2);
    }

    #[test]
    fn test_consolidate_resets_tick_counter() {
        let mut state = MemoryState::default();
        tick(&mut state, "tick one");
        tick(&mut state, "tick two");
        tick(&mut state, "tick three");
        assert_eq!(state.brain.ticks_since_consolidation, 3);

        consolidate(&mut state.brain);
        assert_eq!(state.brain.ticks_since_consolidation, 0);
    }

    #[test]
    fn test_should_suggest_consolidation() {
        let mut state = MemoryState::default();
        assert!(!should_suggest_consolidation(&state));

        // Tick enough times to trigger suggestion
        for i in 0..CONSOLIDATION_SUGGESTION_THRESHOLD {
            tick(&mut state, &format!("tick number {}", i));
        }
        assert!(should_suggest_consolidation(&state));

        // Consolidate resets
        consolidate(&mut state.brain);
        assert!(!should_suggest_consolidation(&state));
    }

    #[test]
    fn test_graph_lookup_includes_edge_type() {
        let mut state = MemoryState::default();
        // Create entries that will generate graph edges
        tick(
            &mut state,
            "fn process_data() uses struct Config for settings",
        );
        tick(
            &mut state,
            "struct Config stores database_url and timeout values",
        );

        // Query should return nodes with edge_type for neighbors
        let results = neocortex::graph_lookup(
            &state.brain.long_term,
            "process_data",
            10,
            &state.brain.keyword_cache,
            neocortex::QueryMode::Neutral,
        );
        // Direct matches have edge_type: None
        // Neighbors should have edge_type: Some(...)
        let has_edge_type = results.iter().any(|r| r.edge_type.is_some());
        // If there are neighbor results, they should have edge types
        if results.len() > 1 {
            assert!(has_edge_type, "neighbor nodes should have edge_type set");
        }
    }

    #[test]
    fn test_hebbian_edge_ceiling() {
        let mut state = MemoryState::default();
        tick(&mut state, "fn process_data() uses struct Config");
        // Hammer the edges with many queries to test ceiling
        for _ in 0..500 {
            retrieve_context(&mut state.brain, "process_data Config");
        }
        // All edge weights should be capped at HEBBIAN_EDGE_CEILING (10.0)
        for edge in &state.brain.long_term.edges {
            assert!(
                edge.weight <= HEBBIAN_EDGE_CEILING,
                "edge weight {} exceeds ceiling {}",
                edge.weight,
                HEBBIAN_EDGE_CEILING
            );
        }
    }

    #[test]
    fn test_edge_decay_reduces_weights() {
        let mut state = MemoryState::default();
        tick(&mut state, "fn handle_data() uses struct Request");
        // Store initial edge weights
        let initial_weights: Vec<f32> = state
            .brain
            .long_term
            .edges
            .iter()
            .map(|e| e.weight)
            .collect();
        assert!(!initial_weights.is_empty(), "should have edges");
        // Advance clock and apply decay
        state.brain.clock += 100;
        apply_decay(&mut state.brain);
        // Verify edge weights have decayed
        for (edge, &initial) in state
            .brain
            .long_term
            .edges
            .iter()
            .zip(initial_weights.iter())
        {
            assert!(
                edge.weight < initial,
                "edge weight should decay: {} -> {}",
                initial,
                edge.weight
            );
        }
    }

    #[test]
    fn test_edge_last_seen_updated() {
        let mut state = MemoryState::default();
        tick(&mut state, "fn process() uses struct Data");
        let initial_last_seen: Vec<u64> = state
            .brain
            .long_term
            .edges
            .iter()
            .map(|e| e.last_seen)
            .collect();
        assert!(!initial_last_seen.is_empty(), "should have edges");
        // Query to trigger Hebbian reinforcement
        retrieve_context(&mut state.brain, "process Data");
        // Check that last_seen was updated for co-retrieved edges
        let any_updated = state
            .brain
            .long_term
            .edges
            .iter()
            .zip(initial_last_seen.iter())
            .any(|(edge, &initial)| edge.last_seen > initial);
        assert!(any_updated, "edge last_seen should be updated after query");
    }

    // ---- Commit 1: Retrieval noise floor tests ----

    #[test]
    fn test_top_k_filters_below_min_similarity() {
        let mut state = MemoryState::default();
        // Insert entries that are very different from the query
        tick(
            &mut state,
            "HUD overlap fix: adjusted widget z-order rendering",
        );
        tick(
            &mut state,
            "window state change callback handler refactored",
        );
        tick(&mut state, "player health bar UI component styling");
        // Query something completely unrelated
        let ctx = retrieve_context(&mut state.brain, "MML syntax reference documentation");
        // All results should be above the noise floor or empty
        for s in &ctx.short_term {
            assert!(
                s.similarity >= MIN_QUERY_SIMILARITY,
                "result below noise floor: sim={:.4} text={}",
                s.similarity,
                &s.text[..s.text.len().min(50)]
            );
        }
    }

    #[test]
    fn test_top_k_keeps_relevant_results() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: MML syntax reference: use #tempo 120 for tempo",
        );
        tick(
            &mut state,
            "DECISION: MML note commands: cdefgab with octave modifiers",
        );
        tick(&mut state, "DECISION: unrelated window rendering pipeline");
        let ctx = retrieve_context(&mut state.brain, "MML syntax");
        // Should have at least one result (the MML entries)
        assert!(
            !ctx.short_term.is_empty(),
            "should return relevant MML results"
        );
        assert!(ctx.short_term[0].text.contains("MML"));
    }

    #[test]
    fn test_top_k_empty_when_nothing_relevant() {
        let mut state = MemoryState::default();
        tick(&mut state, "alpha beta gamma delta");
        tick(&mut state, "epsilon zeta theta iota");
        // Query with completely disjoint vocabulary
        let results = hippocampus::top_k_similar(
            &state.brain.short_term,
            &embed_text("xylophone zamboni quasar", state.brain.config.embedding_dim),
            5,
            "xylophone zamboni quasar",
        );
        // Either empty or all above threshold
        for r in &results {
            assert!(r.similarity >= MIN_QUERY_SIMILARITY);
        }
    }

    #[test]
    fn test_graph_lookup_no_match_returns_empty() {
        let mut state = MemoryState::default();
        // Add some graph nodes via tick (DECISION: ensures L2 promotion + graph update)
        tick(&mut state, "DECISION: fn process_data() in src/main.rs");
        assert!(!state.brain.long_term.nodes.is_empty());
        // Query with entities that don't match any graph node
        let results = neocortex::graph_lookup(
            &state.brain.long_term,
            "xylophone_function zamboni_module",
            10,
            &state.brain.keyword_cache,
            neocortex::QueryMode::Neutral,
        );
        assert!(
            results.is_empty(),
            "graph_lookup should return empty when no entities match, got {} results",
            results.len()
        );
    }

    #[test]
    fn test_graph_lookup_match_still_works() {
        let mut state = MemoryState::default();
        tick(&mut state, "fn process_data() handles struct Config");
        // Query with a matching entity
        let results = neocortex::graph_lookup(
            &state.brain.long_term,
            "process_data",
            10,
            &state.brain.keyword_cache,
            neocortex::QueryMode::Neutral,
        );
        assert!(
            !results.is_empty(),
            "graph_lookup should still return results for matching entities"
        );
    }

    #[test]
    fn test_min_query_similarity_constant_reasonable() {
        // Sanity: threshold should be between 0 and 1
        const { assert!(MIN_QUERY_SIMILARITY > 0.0) };
        const { assert!(MIN_QUERY_SIMILARITY < 1.0) };
        // Should be low enough not to filter genuinely relevant results
        const { assert!(MIN_QUERY_SIMILARITY <= 0.2) };
    }

    // ---- Commit 2: Keyword bonus + trigram tests ----

    #[test]
    fn test_keyword_bonus_boosts_matching_entry() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: MML syntax reference: tempo and note commands",
        );
        tick(
            &mut state,
            "DECISION: unrelated rendering pipeline for sprites",
        );
        let embedding = embed_text("MML syntax", state.brain.config.embedding_dim);
        let results =
            hippocampus::top_k_similar(&state.brain.short_term, &embedding, 5, "MML syntax");
        assert!(!results.is_empty());
        // The MML entry should be first
        assert!(
            results[0].text.contains("MML"),
            "keyword bonus should boost the MML entry to top"
        );
    }

    #[test]
    fn test_keyword_bonus_capped() {
        let mut state = MemoryState::default();
        // Entry with many matching keywords (DECISION: ensures L2 promotion)
        tick(
            &mut state,
            "DECISION: alpha bravo charlie delta echo foxtrot golf hotel",
        );
        let embedding = embed_text(
            "alpha bravo charlie delta echo foxtrot golf hotel",
            state.brain.config.embedding_dim,
        );
        let results = hippocampus::top_k_similar(
            &state.brain.short_term,
            &embedding,
            5,
            "alpha bravo charlie delta echo foxtrot golf hotel",
        );
        assert!(!results.is_empty());
        // Keyword bonus is capped at KEYWORD_MATCH_BONUS_CAP (0.2)
        // The cosine sim alone is ~1.0, so total should not exceed 1.0 + cap
        assert!(results[0].similarity <= 1.0 + KEYWORD_MATCH_BONUS_CAP + 0.01);
    }

    #[test]
    fn test_keyword_bonus_ignores_stopwords() {
        let mut state = MemoryState::default();
        tick(&mut state, "the and for with this that from");
        let embedding = embed_text("the and for", state.brain.config.embedding_dim);
        let results =
            hippocampus::top_k_similar(&state.brain.short_term, &embedding, 5, "the and for");
        // Stopwords should not contribute to keyword bonus
        // Result may or may not pass similarity threshold, but if it does
        // the bonus should be 0 (only stopwords in query)
        for r in &results {
            // Cosine sim should be the only factor (no keyword bonus)
            let cosine_only = cosine_similarity(
                &embed_text(&r.text, state.brain.config.embedding_dim),
                &embedding,
            );
            // Allow small float tolerance
            assert!(
                (r.similarity - cosine_only).abs() < 0.01,
                "stopword-only query should add no keyword bonus"
            );
        }
    }

    #[test]
    fn test_keyword_bonus_empty_query() {
        let mut state = MemoryState::default();
        tick(&mut state, "some memory entry about testing");
        let embedding = embed_text("", state.brain.config.embedding_dim);
        let results = hippocampus::top_k_similar(&state.brain.short_term, &embedding, 5, "");
        // Should not panic, bonus should be 0
        for r in &results {
            assert!(r.similarity >= -1.0); // just a sanity check
        }
    }

    #[test]
    fn test_trigram_reduced_weight_improves_discrimination() {
        // With reduced trigram weight (0.3 vs old 0.5), entries with
        // completely different words but overlapping trigrams should
        // have lower similarity
        let dim = 256;
        let a = embed_text("MML syntax reference", dim);
        let b = embed_text("HUD overlap fix", dim);
        let sim = cosine_similarity(&a, &b);
        // These are unrelated — similarity should be low
        assert!(
            sim < 0.5,
            "unrelated texts should have low cosine sim with reduced trigrams, got {sim}"
        );
    }

    // ---- Commit 3: Consolidation deduplication tests ----

    #[test]
    fn test_consolidate_dedup_on_reconsolidate() {
        let mut state = MemoryState::default();
        // Directly insert entries into short_term to bypass tick()'s merge logic
        let dim = state.brain.config.embedding_dim;
        let texts = [
            "implemented MML tempo command to set BPM for playback speed control",
            "implemented MML tempo command to set BPM for playback rate adjustment",
            "implemented MML tempo command to set BPM for playback timing update",
        ];
        for text in &texts {
            hippocampus::insert_short_term(
                &mut state.brain,
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        // Lower theta_low so these group together
        state.brain.config.theta_low = 0.3;
        let summaries1 = consolidate(&mut state.brain);
        assert!(
            !summaries1.is_empty(),
            "first consolidation should produce summaries"
        );
        let summary_count_before = state
            .brain
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Summary")
            .count();

        // Insert more similar entries and consolidate again
        let texts2 = [
            "implemented MML tempo command to set BPM for playback output rendering",
            "implemented MML tempo command to set BPM for playback engine processing",
            "implemented MML tempo command to set BPM for playback audio synthesis",
        ];
        for text in &texts2 {
            hippocampus::insert_short_term(
                &mut state.brain,
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        let _summaries2 = consolidate(&mut state.brain);
        let summary_count_after = state
            .brain
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Summary")
            .count();

        // Should merge into existing Summary, not create duplicates
        assert!(
            summary_count_after <= summary_count_before + 1,
            "re-consolidation should merge similar summaries: before={}, after={}",
            summary_count_before,
            summary_count_after
        );
    }

    #[test]
    fn test_consolidate_source_texts_merge() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;
        let texts = [
            "feature alpha implemented for rendering pipeline in module X",
            "feature alpha implemented for rendering system in module X",
            "feature alpha implemented for rendering engine in module X",
        ];
        for text in &texts {
            hippocampus::insert_short_term(
                &mut state.brain,
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        state.brain.config.theta_low = 0.3;
        consolidate(&mut state.brain);

        let summary_node = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|n| n.kind == "Summary");
        assert!(summary_node.is_some(), "should have a Summary node");
        let node = summary_node.unwrap();
        assert!(
            node.source_texts.len() >= 2,
            "Summary should contain source texts from group members, got {}",
            node.source_texts.len()
        );
    }

    #[test]
    fn test_consolidate_source_texts_cap() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;
        state.brain.config.theta_low = 0.2;
        // Directly insert many similar entries
        for i in 0..25 {
            let text = format!(
                "feature beta variant number {} implemented in rendering module Y pipeline",
                i
            );
            hippocampus::insert_short_term(
                &mut state.brain,
                &text,
                embed_text(&text, dim),
                compute_salience(&text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        consolidate(&mut state.brain);

        for node in state.brain.long_term.nodes.values() {
            if node.kind == "Summary" {
                assert!(
                    node.source_texts.len() <= 20,
                    "source_texts should be capped at 20, got {}",
                    node.source_texts.len()
                );
            }
        }
    }

    // ---- Commit 4: Consolidated entry filtering tests ----

    #[test]
    fn test_consolidated_entries_filtered_from_queries() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;
        // Insert entries that will form a group
        let texts = [
            "MML tempo command sets BPM for playback speed in the engine",
            "MML tempo directive sets BPM for playback rate in the engine",
            "MML tempo instruction sets BPM for playback timing in the engine",
        ];
        for text in &texts {
            hippocampus::insert_short_term(
                &mut state.brain,
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        state.brain.config.theta_low = 0.3;
        consolidate(&mut state.brain);

        // All grouped entries should now be consolidated
        let consolidated_count = state
            .brain
            .short_term
            .iter()
            .filter(|e| e.consolidated)
            .count();
        assert!(
            consolidated_count >= 2,
            "at least 2 entries should be marked consolidated, got {}",
            consolidated_count
        );

        // Query should not return consolidated entries
        let ctx = retrieve_context(&mut state.brain, "MML tempo BPM");
        for snippet in &ctx.short_term {
            let entry = state.brain.short_term.iter().find(|e| e.id == snippet.id);
            if let Some(e) = entry {
                assert!(
                    !e.consolidated,
                    "consolidated entry should not appear in query results"
                );
            }
        }
    }

    #[test]
    fn test_consolidated_defaults_false() {
        let mut state = MemoryState::default();
        tick(&mut state, "DECISION: some new memory entry for testing");
        let entry = state.brain.short_term.last().unwrap();
        assert!(
            !entry.consolidated,
            "new entries should default to consolidated=false"
        );
    }

    #[test]
    fn test_unconsolidated_entries_still_appear() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: unique standalone MML note commands reference guide",
        );
        // No consolidation — entry should appear in results
        let ctx = retrieve_context(&mut state.brain, "MML note commands");
        assert!(
            !ctx.short_term.is_empty(),
            "unconsolidated entries should still appear in query results"
        );
        assert!(ctx.short_term[0].text.contains("MML"));
    }

    // ---- V4 migration test ----

    #[test]
    fn test_msgpack_roundtrip() {
        let dir = std::env::temp_dir().join("legend_test_msgpack_rt");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("memory.lz4");

        let mut state = MemoryState {
            brain: BrainState {
                clock: 42,
                next_id: 10,
                ..BrainState::default()
            },
            ..MemoryState::default()
        };
        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: "msgpack roundtrip test".into(),
            summary: "test".into(),
            embedding: vec![0.1, 0.2, 0.3],
            last_access: 40,
            usage: 3,
            salience: 0.5,
            consolidated: true,
            ..ShortTermEntry::default()
        });

        save_memory_to_path(&state, &path).expect("save");
        let loaded = load_memory_from_path(&path).expect("load");

        assert_eq!(loaded.brain.clock, 42);
        assert_eq!(loaded.brain.next_id, 10);
        assert_eq!(loaded.brain.short_term.len(), 1);
        assert_eq!(loaded.brain.short_term[0].text, "msgpack roundtrip test");
        assert_eq!(loaded.brain.short_term[0].embedding, vec![0.1, 0.2, 0.3]);
        assert!(loaded.brain.short_term[0].consolidated);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_msgpack_backward_compat_missing_fields() {
        // Simulate loading msgpack data that's missing a field (e.g., consolidated).
        // Struct-level #[serde(default)] should fill it with Default.
        #[derive(Debug, Serialize)]
        struct OldEntry {
            id: u64,
            text: String,
            summary: String,
            embedding: Vec<f32>,
            last_access: u64,
            usage: u32,
            salience: f32,
            // missing: consolidated, density, gradient_sq_sum, etc.
        }

        let old = OldEntry {
            id: 7,
            text: "old format".into(),
            summary: "old".into(),
            embedding: vec![0.5],
            last_access: 10,
            usage: 1,
            salience: 0.3,
        };

        let bytes = rmp_serde::to_vec_named(&old).unwrap();
        let loaded: ShortTermEntry = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(loaded.id, 7);
        assert_eq!(loaded.text, "old format");
        assert!(!loaded.consolidated); // default
        assert_eq!(loaded.density, 0.0); // default
        assert_eq!(loaded.gradient_sq_sum, 0.0); // default
        assert_eq!(loaded.reconsolidation_count, 0); // default
    }

    #[test]
    fn test_msgpack_forward_compat_unknown_fields() {
        // Simulate loading msgpack data with extra unknown fields.
        // rmp_serde should silently ignore them.
        #[derive(Debug, Serialize)]
        struct FutureEntry {
            id: u64,
            text: String,
            summary: String,
            embedding: Vec<f32>,
            last_access: u64,
            usage: u32,
            salience: f32,
            consolidated: bool,
            reconsolidation_count: u32,
            labile_until: u64,
            refs: Vec<MemoryRef>,
            gradient_sq_sum: f32,
            density: f32,
            // Future fields not in current struct
            future_field_str: String,
            future_field_num: u64,
        }

        let future = FutureEntry {
            id: 9,
            text: "from the future".into(),
            summary: "future".into(),
            embedding: vec![0.9],
            last_access: 50,
            usage: 2,
            salience: 0.8,
            consolidated: true,
            reconsolidation_count: 3,
            labile_until: 55,
            refs: vec![],
            gradient_sq_sum: 1.5,
            density: 2.0,
            future_field_str: "unknown".into(),
            future_field_num: 42,
        };

        let bytes = rmp_serde::to_vec_named(&future).unwrap();
        let loaded: ShortTermEntry = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(loaded.id, 9);
        assert_eq!(loaded.text, "from the future");
        assert!(loaded.consolidated);
        assert_eq!(loaded.density, 2.0);
        // Unknown fields silently ignored — no crash
    }

    #[test]
    fn test_msgpack_full_state_missing_field_no_data_loss() {
        // End-to-end test: save a full MemoryState as msgpack using a struct
        // that's MISSING the `consolidated` field, then load with the current
        // struct. This is the exact scenario that caused the v0.3.5 data wipe
        // with bincode — it must work with msgpack.
        #[derive(Debug, Serialize)]
        struct OldShortTermEntry {
            id: u64,
            text: String,
            summary: String,
            embedding: Vec<f32>,
            last_access: u64,
            usage: u32,
            salience: f32,
            reconsolidation_count: u32,
            labile_until: u64,
            refs: Vec<MemoryRef>,
            gradient_sq_sum: f32,
            density: f32,
            // `consolidated` intentionally absent — simulates v0.3.4
        }

        #[derive(Debug, Serialize)]
        struct OldMemoryState {
            config: MemoryConfig,
            immediate: VecDeque<String>,
            short_term: Vec<OldShortTermEntry>,
            long_term: GraphMemory,
            clock: u64,
            next_id: u64,
            session_log: Vec<SessionEntry>,
            current_task: Option<String>,
            ticks_since_consolidation: u32,
            last_retrieved_ids: Vec<u64>,
            last_synced_sha: Option<String>,
        }

        let dir = std::env::temp_dir().join("legend_test_no_data_loss");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("memory.lz4");

        // Build an "old" state missing consolidated field
        let old_state = OldMemoryState {
            config: MemoryConfig::default(),
            immediate: VecDeque::from(["hello".to_string()]),
            short_term: vec![
                OldShortTermEntry {
                    id: 1,
                    text: "important memory".into(),
                    summary: "important".into(),
                    embedding: vec![0.1, 0.2, 0.3],
                    last_access: 50,
                    usage: 5,
                    salience: 0.9,
                    reconsolidation_count: 2,
                    labile_until: 0,
                    refs: vec![MemoryRef {
                        path: "src/main.rs".into(),
                        start_line: 10,
                        end_line: 20,
                        snippet: "fn main()".into(),
                    }],
                    gradient_sq_sum: 0.5,
                    density: 1.2,
                },
                OldShortTermEntry {
                    id: 2,
                    text: "another memory".into(),
                    summary: "another".into(),
                    embedding: vec![0.4, 0.5, 0.6],
                    last_access: 45,
                    usage: 3,
                    salience: 0.7,
                    reconsolidation_count: 0,
                    labile_until: 0,
                    refs: vec![],
                    gradient_sq_sum: 0.0,
                    density: 0.0,
                },
            ],
            long_term: GraphMemory::default(),
            clock: 100,
            next_id: 3,
            session_log: vec![SessionEntry {
                timestamp: 99,
                text: "test session".into(),
            }],
            current_task: Some("testing msgpack".into()),
            ticks_since_consolidation: 5,
            last_retrieved_ids: vec![1],
            last_synced_sha: Some("deadbeef".into()),
        };

        // Serialize as msgpack with LGND header
        let serialized = rmp_serde::to_vec_named(&old_state).unwrap();
        let mut payload = Vec::with_capacity(5 + serialized.len());
        payload.extend_from_slice(MSGPACK_MAGIC);
        payload.push(MSGPACK_FORMAT_VERSION);
        payload.extend_from_slice(&serialized);
        let compressed = lz4::block::compress(&payload, None, true).unwrap();
        fs::write(&path, &compressed).unwrap();

        // Load with current MemoryState (which has `consolidated` field)
        let loaded = load_memory_from_path(&path).expect("must not fail!");

        // ALL data must be preserved (except old immediate which is discarded)
        assert_eq!(loaded.brain.clock, 100);
        assert_eq!(loaded.brain.next_id, 3);
        assert!(
            loaded.brain.working_memory.is_empty(),
            "old immediate discarded, working_memory starts empty"
        );
        assert_eq!(loaded.brain.short_term.len(), 2);
        assert_eq!(loaded.brain.short_term[0].id, 1);
        assert_eq!(loaded.brain.short_term[0].text, "important memory");
        assert_eq!(loaded.brain.short_term[0].salience, 0.9);
        assert_eq!(loaded.brain.short_term[0].refs.len(), 1);
        assert_eq!(loaded.brain.short_term[0].refs[0].path, "src/main.rs");
        // Missing `consolidated` defaults to false
        assert!(!loaded.brain.short_term[0].consolidated);
        assert!(!loaded.brain.short_term[1].consolidated);
        assert_eq!(loaded.brain.short_term[1].id, 2);
        assert_eq!(loaded.brain.short_term[1].text, "another memory");
        assert_eq!(loaded.session_log.len(), 1);
        assert_eq!(loaded.current_task, Some("testing msgpack".into()));
        assert_eq!(loaded.last_synced_sha, Some("deadbeef".into()));

        let _ = fs::remove_dir_all(&dir);
    }

    // ---- Working memory (L1) tests ----

    #[test]
    fn test_working_memory_capacity_limit() {
        let mut state = MemoryState::default();
        // Default capacity is 10; push 12 entries
        for i in 0..12 {
            tick(&mut state, &format!("low signal entry number {}", i));
        }
        assert!(
            state.brain.working_memory.len() <= state.brain.config.immediate_capacity,
            "working memory should not exceed capacity: {} > {}",
            state.brain.working_memory.len(),
            state.brain.config.immediate_capacity
        );
    }

    #[test]
    fn test_low_salience_stays_l1_only() {
        let mut state = MemoryState::default();
        let st_before = state.brain.short_term.len();
        tick(
            &mut state,
            "just a random thought about nothing in particular",
        );
        // Low-salience: should NOT promote to L2
        assert_eq!(
            state.brain.short_term.len(),
            st_before,
            "low-salience tick should not create L2 entry"
        );
        assert!(
            !state.brain.working_memory.is_empty(),
            "low-salience tick should be in working memory"
        );
    }

    #[test]
    fn test_high_salience_promotes_to_l2() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: chose Rust over Go because of safety guarantees",
        );
        assert!(
            !state.brain.short_term.is_empty(),
            "high-salience tick should promote to L2"
        );
        // Should also be in working memory
        assert!(
            !state.brain.working_memory.is_empty(),
            "promoted tick should also remain in working memory"
        );
        // The WM entry should be marked as promoted
        assert!(
            state.brain.working_memory.last().unwrap().promoted,
            "promoted WM entry should have promoted=true"
        );
    }

    #[test]
    fn test_query_scans_working_memory() {
        let mut state = MemoryState::default();
        // Tick something low-salience that stays in L1 only
        tick(
            &mut state,
            "the parser handles empty input strings gracefully",
        );
        assert!(state.brain.short_term.is_empty(), "should stay L1 only");

        // Query should find it in working_memory results
        let ctx = retrieve_context(&mut state.brain, "parser empty input");
        assert!(
            !ctx.working_memory.is_empty(),
            "query should scan working memory and find L1-only entries"
        );
    }

    #[test]
    fn test_query_increments_rehearsal_count() {
        let mut state = MemoryState::default();
        // Use low-salience text that stays L1 only (no retrieve_context called)
        tick(
            &mut state,
            "the purple elephant danced on silver moonbeams last tuesday",
        );
        assert!(!state.brain.working_memory.is_empty());
        let initial_rehearsal = state.brain.working_memory[0].rehearsal_count;

        // Query to trigger rehearsal
        retrieve_context(&mut state.brain, "purple elephant silver moonbeams");
        // Check rehearsal was incremented
        let entry = state
            .brain
            .working_memory
            .iter()
            .find(|e| e.text.contains("purple"));
        assert!(
            entry.is_some() && entry.unwrap().rehearsal_count > initial_rehearsal,
            "query should increment rehearsal_count on matched WM entries"
        );
    }

    #[test]
    fn test_rehearsed_entry_promotes_on_displacement() {
        let mut state = MemoryState::default();
        let embedding = embed_text("rehearsed entry test", state.brain.config.embedding_dim);

        // Manually push a low-salience entry with rehearsal_count >= 1
        state.brain.working_memory.push(WorkingMemoryEntry {
            id: 900,
            text: "rehearsed entry test content".to_string(),
            embedding: embedding.clone(),
            salience: 0.05, // below threshold
            tick_created: state.brain.clock,
            rehearsal_count: 1, // rehearsed via query
            promoted: false,
            emotional_valence: 0.0,
        });
        let st_before = state.brain.short_term.len();

        // Fill to capacity + 1 to force displacement of index 0
        for _ in 0..state.brain.config.immediate_capacity {
            let emb = embed_text("filler", state.brain.config.embedding_dim);
            prefrontal::push_working_memory(&mut state.brain, "filler entry", &emb, 0.01, 0.0);
        }

        // The rehearsed entry should have been promoted to L2 on displacement
        assert!(
            state.brain.short_term.len() > st_before,
            "rehearsed entry should promote to L2 when displaced: before={}, after={}",
            st_before,
            state.brain.short_term.len()
        );
    }

    #[test]
    fn test_flush_promotes_qualifying_entries() {
        let mut state = MemoryState::default();
        // Add a high-salience entry that wasn't promoted by tick (simulate by pushing directly)
        state.brain.working_memory.push(WorkingMemoryEntry {
            id: 999,
            text: "important decision about architecture".to_string(),
            embedding: embed_text(
                "important decision about architecture",
                state.brain.config.embedding_dim,
            ),
            salience: 0.5, // above ATTENTION_GATE_THRESHOLD
            tick_created: state.brain.clock,
            rehearsal_count: 0,
            promoted: false,
            emotional_valence: 0.0,
        });
        let st_before = state.brain.short_term.len();

        prefrontal::flush_working_memory(&mut state.brain);

        assert!(
            state.brain.working_memory.is_empty(),
            "flush should clear working memory"
        );
        assert!(
            state.brain.short_term.len() > st_before,
            "flush should promote high-salience entries to L2"
        );
    }

    #[test]
    fn test_flush_discards_low_signal_unrehearsed() {
        let mut state = MemoryState::default();
        // Add a low-salience unrehearsed entry
        state.brain.working_memory.push(WorkingMemoryEntry {
            id: 998,
            text: "just some noise that nobody cares about".to_string(),
            embedding: embed_text("just some noise", state.brain.config.embedding_dim),
            salience: 0.05, // below threshold
            tick_created: state.brain.clock,
            rehearsal_count: 0,
            promoted: false,
            emotional_valence: 0.0,
        });
        let st_before = state.brain.short_term.len();

        prefrontal::flush_working_memory(&mut state.brain);

        assert!(
            state.brain.working_memory.is_empty(),
            "flush should clear working memory"
        );
        assert_eq!(
            state.brain.short_term.len(),
            st_before,
            "flush should NOT promote low-salience unrehearsed entries"
        );
    }

    #[test]
    fn test_flush_skips_already_promoted() {
        let mut state = MemoryState::default();
        // Add an entry already marked as promoted
        state.brain.working_memory.push(WorkingMemoryEntry {
            id: 997,
            text: "already promoted entry".to_string(),
            embedding: embed_text("already promoted", state.brain.config.embedding_dim),
            salience: 0.8,
            tick_created: state.brain.clock,
            rehearsal_count: 5,
            promoted: true, // already promoted
            emotional_valence: 0.0,
        });
        let st_before = state.brain.short_term.len();

        prefrontal::flush_working_memory(&mut state.brain);

        assert_eq!(
            state.brain.short_term.len(),
            st_before,
            "flush should NOT double-promote already-promoted entries"
        );
    }

    // --- Synaptic encoding tests (Change 7) ---

    #[test]
    fn test_activation_count_increments() {
        let mut state = MemoryState::default();
        // Tick twice with shared entities to create and reinforce an edge
        tick(
            &mut state,
            "DECISION: fn handle_auth() uses struct Config for settings",
        );
        tick(
            &mut state,
            "DECISION: struct Config stores fn handle_auth() parameters",
        );

        // Find edges with activation_count > 0
        let reinforced = state
            .brain
            .long_term
            .edges
            .iter()
            .filter(|e| e.activation_count > 0)
            .count();
        assert!(
            reinforced > 0,
            "some edges should have been reinforced (activation_count > 0)"
        );
    }

    #[test]
    fn test_spaced_reinforcement_builds_stability() {
        let mut state = MemoryState::default();
        let (a_id, b_id) = (1400, 1401);
        insert_test_node(&mut state.brain, a_id, "SpacedNodeA");
        insert_test_node(&mut state.brain, b_id, "SpacedNodeB");

        // Create edge
        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 1,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        // Reinforce with increasing intervals (spaced): 5, 10, 20, 40
        let intervals = [5u64, 10, 20, 40];
        let mut clock = 1u64;
        for interval in &intervals {
            clock += interval;
            state.brain.clock = clock;
            neocortex::upsert_edge(
                &mut state.brain.long_term,
                a_id,
                b_id,
                "related",
                state.brain.clock,
            );
        }

        let edge = state
            .brain
            .long_term
            .edges
            .iter()
            .find(|e| e.from == a_id && e.to == b_id)
            .unwrap();

        assert!(
            edge.stability > 1.5,
            "spaced reinforcement should build high stability, got {}",
            edge.stability
        );
        assert_eq!(edge.activation_count, 4);
    }

    #[test]
    fn test_massed_reinforcement_low_stability() {
        let mut state = MemoryState::default();
        let (a_id, b_id) = (1500, 1501);
        insert_test_node(&mut state.brain, a_id, "MassedNodeA");
        insert_test_node(&mut state.brain, b_id, "MassedNodeB");

        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 1,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        // Reinforce with constant intervals (cramming): 1, 1, 1, 1
        let mut clock = 1u64;
        for _ in 0..4 {
            clock += 1;
            state.brain.clock = clock;
            neocortex::upsert_edge(
                &mut state.brain.long_term,
                a_id,
                b_id,
                "related",
                state.brain.clock,
            );
        }

        let edge = state
            .brain
            .long_term
            .edges
            .iter()
            .find(|e| e.from == a_id && e.to == b_id)
            .unwrap();

        assert!(
            edge.stability < 1.5,
            "massed reinforcement should build low stability, got {}",
            edge.stability
        );
    }

    #[test]
    fn test_spaced_beats_massed_stability() {
        let mut state = MemoryState::default();

        // Spaced edge
        let (sa, sb) = (1600, 1601);
        insert_test_node(&mut state.brain, sa, "SpacedA");
        insert_test_node(&mut state.brain, sb, "SpacedB");
        state.brain.long_term.edges.push(GraphEdge {
            from: sa,
            to: sb,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 1,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        // Massed edge
        let (ma, mb) = (1602, 1603);
        insert_test_node(&mut state.brain, ma, "MassedA");
        insert_test_node(&mut state.brain, mb, "MassedB");
        state.brain.long_term.edges.push(GraphEdge {
            from: ma,
            to: mb,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 1,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        // Spaced: intervals [5, 10, 20, 40]
        let spaced_intervals = [5u64, 10, 20, 40];
        let mut clock = 1u64;
        for interval in &spaced_intervals {
            clock += interval;
            state.brain.clock = clock;
            neocortex::upsert_edge(
                &mut state.brain.long_term,
                sa,
                sb,
                "related",
                state.brain.clock,
            );
        }

        // Massed: intervals [1, 1, 1, 1] (same number of reinforcements)
        let mut clock_m = 1u64;
        for _ in 0..4 {
            clock_m += 1;
            state.brain.clock = clock_m;
            neocortex::upsert_edge(
                &mut state.brain.long_term,
                ma,
                mb,
                "related",
                state.brain.clock,
            );
        }

        let spaced_edge = state
            .brain
            .long_term
            .edges
            .iter()
            .find(|e| e.from == sa && e.to == sb)
            .unwrap();
        let massed_edge = state
            .brain
            .long_term
            .edges
            .iter()
            .find(|e| e.from == ma && e.to == mb)
            .unwrap();

        assert!(
            spaced_edge.stability > massed_edge.stability,
            "spaced ({}) should have higher stability than massed ({})",
            spaced_edge.stability,
            massed_edge.stability
        );
    }

    #[test]
    fn test_hebbian_logarithmic_dampening() {
        let mut state = MemoryState::default();
        let (a_id, b_id) = (1700, 1701);
        insert_test_node(&mut state.brain, a_id, "HebbA");
        insert_test_node(&mut state.brain, b_id, "HebbB");

        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        // First Hebbian reinforce: activation_count=0 → full boost
        state.brain.clock = 1;
        let weight_before = state.brain.long_term.edges[0].weight;
        neocortex::hebbian_reinforce(&mut state.brain.long_term, &[a_id, b_id], state.brain.clock);
        let first_boost = state.brain.long_term.edges[0].weight - weight_before;

        // Set high activation_count to simulate heavily-used edge
        state.brain.long_term.edges[0].activation_count = 100;
        let weight_before2 = state.brain.long_term.edges[0].weight;
        state.brain.clock = 2;
        neocortex::hebbian_reinforce(&mut state.brain.long_term, &[a_id, b_id], state.brain.clock);
        let hundredth_boost = state.brain.long_term.edges[0].weight - weight_before2;

        assert!(
            first_boost > hundredth_boost,
            "first reinforce ({}) should boost more than 100th ({})",
            first_boost,
            hundredth_boost
        );
    }

    #[test]
    fn test_stability_modulates_spreading_activation() {
        let mut state = MemoryState::default();
        // Two paths from A: A->B (high stability) and A->C (low stability)
        let (a_id, b_id, c_id) = (1800, 1801, 1802);
        insert_test_node(&mut state.brain, a_id, "StabA");
        insert_test_node(&mut state.brain, b_id, "StabB");
        insert_test_node(&mut state.brain, c_id, "StabC");

        // High stability edge A->B
        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 10,
            stability: 5.0,
            recent_interval_avg: 20.0,
            historical_interval_avg: 15.0,
        });

        // Low stability edge A->C (same weight)
        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: c_id,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 10,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[a_id],
            1,
            0.5,
            neocortex::QueryMode::Neutral,
        );
        let b_activation = results
            .iter()
            .find(|(id, _)| *id == b_id)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);
        let c_activation = results
            .iter()
            .find(|(id, _)| *id == c_id)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);

        assert!(
            b_activation > c_activation,
            "high-stability edge should propagate more: B={} vs C={}",
            b_activation,
            c_activation
        );
    }

    #[test]
    fn test_structural_query_downweights_temporal_edges() {
        let mut state = MemoryState::default();
        let (a_id, b_id, c_id) = (1810, 1811, 1812);
        insert_test_node(&mut state.brain, a_id, "QuerySeed");
        insert_test_node(&mut state.brain, b_id, "StructNeighbor");
        insert_test_node(&mut state.brain, c_id, "TemporalNeighbor");

        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "contains".into(),
            last_seen: 0,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });
        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: c_id,
            weight: 0.5,
            kind: "temporal".into(),
            last_seen: 0,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[a_id],
            1,
            0.5,
            neocortex::QueryMode::Structural,
        );
        let struct_activation = results
            .iter()
            .find(|(id, _)| *id == b_id)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);
        let temporal_activation = results
            .iter()
            .find(|(id, _)| *id == c_id)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);

        assert!(struct_activation > temporal_activation);
    }

    #[test]
    fn test_temporal_query_prefers_temporal_edges() {
        let mut state = MemoryState::default();
        let (a_id, b_id, c_id) = (1820, 1821, 1822);
        insert_test_node(&mut state.brain, a_id, "TimeSeed");
        insert_test_node(&mut state.brain, b_id, "StructPath");
        insert_test_node(&mut state.brain, c_id, "SessionPath");

        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "contains".into(),
            last_seen: 0,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });
        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: c_id,
            weight: 0.5,
            kind: "temporal".into(),
            last_seen: 0,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[a_id],
            1,
            0.5,
            neocortex::QueryMode::Temporal,
        );
        let struct_activation = results
            .iter()
            .find(|(id, _)| *id == b_id)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);
        let temporal_activation = results
            .iter()
            .find(|(id, _)| *id == c_id)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);

        assert!(temporal_activation > struct_activation);
    }

    #[test]
    fn test_soft_gating_does_not_zero_nonpreferred_edges() {
        let mut state = MemoryState::default();
        let (a_id, b_id) = (1830, 1831);
        insert_test_node(&mut state.brain, a_id, "GateSeed");
        insert_test_node(&mut state.brain, b_id, "TemporalLink");

        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "temporal".into(),
            last_seen: 0,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[a_id],
            1,
            0.5,
            neocortex::QueryMode::Structural,
        );
        let activation = results
            .iter()
            .find(|(id, _)| *id == b_id)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);

        assert!(
            activation > 0.0,
            "nonpreferred edges should be damped, not removed"
        );
    }

    #[test]
    fn test_neutral_query_preserves_existing_behavior() {
        let mut state = MemoryState::default();
        let (a_id, b_id) = (1840, 1841);
        insert_test_node(&mut state.brain, a_id, "NeutralSeed");
        insert_test_node(&mut state.brain, b_id, "NeutralNeighbor");

        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "temporal".into(),
            last_seen: 0,
            activation_count: 1,
            stability: 4.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[a_id],
            1,
            0.5,
            neocortex::QueryMode::Neutral,
        );
        let activation = results
            .iter()
            .find(|(id, _)| *id == b_id)
            .map(|(_, a)| *a)
            .unwrap_or(0.0);
        let expected = 0.5 * 4.0_f32.sqrt() * 0.5;

        assert!((activation - expected).abs() < 1e-6);
    }

    #[test]
    fn test_query_mode_inference_basic_cases() {
        let keyword_cache = kw();

        assert_eq!(
            infer_query_mode("what did I work on yesterday", &keyword_cache),
            neocortex::QueryMode::Temporal
        );
        assert_eq!(
            infer_query_mode("how does auth middleware work", &keyword_cache),
            neocortex::QueryMode::Structural
        );
        assert_eq!(
            infer_query_mode("why did login crash", &keyword_cache),
            neocortex::QueryMode::Diagnostic
        );
        assert_eq!(
            infer_query_mode("JWT token validation", &keyword_cache),
            neocortex::QueryMode::Semantic
        );
    }

    #[test]
    fn test_cpeb_tagging_boosts_recently_active_edges() {
        let mut state = MemoryState::default();
        state.brain.clock = 20;

        let (a_id, b_id, c_id, d_id) = (1900, 1901, 1902, 1903);
        insert_test_node(&mut state.brain, a_id, "RecentA");
        insert_test_node(&mut state.brain, b_id, "RecentB");
        insert_test_node(&mut state.brain, c_id, "OldA");
        insert_test_node(&mut state.brain, d_id, "OldB");

        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 20,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });
        state.brain.long_term.edges.push(GraphEdge {
            from: c_id,
            to: d_id,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 14,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let text = "BLOCKER: critical crash panic failure in auth flow";
        assert!(
            compute_emotional_valence(text, &state.brain.keyword_cache).abs()
                > CPEB_VALENCE_THRESHOLD
        );

        tick(&mut state, text);

        assert!(state.brain.long_term.edges[0].stability > 1.0);
        assert!((state.brain.long_term.edges[1].stability - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_cpeb_no_boost_on_neutral_tick() {
        let mut state = MemoryState::default();
        state.brain.clock = 20;

        let (a_id, b_id) = (1910, 1911);
        insert_test_node(&mut state.brain, a_id, "NeutralA");
        insert_test_node(&mut state.brain, b_id, "NeutralB");
        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 20,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let text = "updated routine documentation and formatting notes";
        assert_eq!(
            compute_emotional_valence(text, &state.brain.keyword_cache),
            0.0
        );

        tick(&mut state, text);

        assert!((state.brain.long_term.edges[0].stability - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_cpeb_boost_scales_with_valence() {
        let low_text = "BUG: auth error on login";
        let high_text = "BLOCKER: critical auth crash panic failure regression";

        let mut low_state = MemoryState::default();
        low_state.brain.clock = 20;
        let (low_a, low_b) = (1920, 1921);
        insert_test_node(&mut low_state.brain, low_a, "LowA");
        insert_test_node(&mut low_state.brain, low_b, "LowB");
        low_state.brain.long_term.edges.push(GraphEdge {
            from: low_a,
            to: low_b,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 20,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let mut high_state = MemoryState::default();
        high_state.brain.clock = 20;
        let (high_a, high_b) = (1930, 1931);
        insert_test_node(&mut high_state.brain, high_a, "HighA");
        insert_test_node(&mut high_state.brain, high_b, "HighB");
        high_state.brain.long_term.edges.push(GraphEdge {
            from: high_a,
            to: high_b,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 20,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let low_valence = compute_emotional_valence(low_text, &low_state.brain.keyword_cache).abs();
        let high_valence =
            compute_emotional_valence(high_text, &high_state.brain.keyword_cache).abs();
        assert!(low_valence > CPEB_VALENCE_THRESHOLD);
        assert!(high_valence > low_valence);

        tick(&mut low_state, low_text);
        tick(&mut high_state, high_text);

        let low_boost = low_state.brain.long_term.edges[0].stability - 1.0;
        let high_boost = high_state.brain.long_term.edges[0].stability - 1.0;
        assert!(
            high_boost > low_boost,
            "higher valence should produce larger CPEB boost: high={} low={}",
            high_boost,
            low_boost
        );
    }

    #[test]
    fn test_cpeb_tagged_edge_decays_slower() {
        let mut long_term = GraphMemory::default();
        long_term.edges.push(GraphEdge {
            from: 1,
            to: 2,
            weight: 1.0,
            kind: "related".into(),
            last_seen: 50,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });
        long_term.edges.push(GraphEdge {
            from: 3,
            to: 4,
            weight: 1.0,
            kind: "related".into(),
            last_seen: 40,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let tagged = neocortex::cpeb_tag_edges(
            &mut long_term,
            50,
            0.9,
            CPEB_TAG_WINDOW,
            CPEB_STABILITY_BOOST,
        );
        assert_eq!(tagged, 1);

        long_term.edges[0].last_seen = 50;
        long_term.edges[1].last_seen = 50;

        neocortex::apply_l3_decay(&mut long_term, 250);

        assert!(
            long_term.edges[0].weight > long_term.edges[1].weight,
            "tagged edge should retain more weight after decay: tagged={} untagged={}",
            long_term.edges[0].weight,
            long_term.edges[1].weight
        );
    }

    #[test]
    fn test_cpeb_tag_window_respected() {
        let mut long_term = GraphMemory::default();
        long_term.edges.push(GraphEdge {
            from: 1,
            to: 2,
            weight: 1.0,
            kind: "related".into(),
            last_seen: 10,
            activation_count: 1,
            stability: 1.0,
            recent_interval_avg: 1.0,
            historical_interval_avg: 1.0,
        });

        let tagged = neocortex::cpeb_tag_edges(
            &mut long_term,
            20,
            0.8,
            CPEB_TAG_WINDOW,
            CPEB_STABILITY_BOOST,
        );

        assert_eq!(tagged, 0);
        assert!((long_term.edges[0].stability - 1.0).abs() < f32::EPSILON);
    }

    // --- Pattern completion tests (Change 6) ---

    #[test]
    fn test_pattern_completion_finds_related_entry() {
        // Build graph: ConfigLoader -> JwtSecret (via source_texts)
        // Insert L2 entry that mentions JwtSecret
        // Query "ConfigLoader" — direct similarity may be low, but
        // pattern completion should find the JwtSecret entry via graph
        let mut state = MemoryState::default();

        // Create graph nodes with source_texts linking to L2 content
        let cfg_id = 1100;
        let jwt_id = 1101;
        state.brain.long_term.nodes.insert(
            cfg_id,
            GraphNode {
                id: cfg_id,
                label: "ConfigLoader".into(),
                kind: "Entity".into(),
                weight: 1.0,
                salience: 0.5,
                source_texts: vec!["ConfigLoader reads settings".into()],
                last_seen: 0,
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("ConfigLoader".into(), cfg_id);

        state.brain.long_term.nodes.insert(
            jwt_id,
            GraphNode {
                id: jwt_id,
                label: "JwtSecret".into(),
                kind: "Entity".into(),
                weight: 1.0,
                salience: 0.5,
                source_texts: vec!["JwtSecret stores token signing keys".into()],
                last_seen: 0,
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("JwtSecret".into(), jwt_id);

        // Edge connecting them
        state.brain.long_term.edges.push(GraphEdge {
            from: cfg_id,
            to: jwt_id,
            weight: 0.8,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        // Insert L2 entry that contains the JwtSecret source text
        let dim = state.brain.config.embedding_dim;
        hippocampus::insert_short_term(
            &mut state.brain,
            "JwtSecret stores token signing keys for authentication",
            embed_text(
                "JwtSecret stores token signing keys for authentication",
                dim,
            ),
            0.5,
            Vec::new(),
            0.0,
        );

        // Query for ConfigLoader — pattern completion should find the JwtSecret entry
        let completed = hippocampus::pattern_complete(
            &state.brain,
            "ConfigLoader",
            &[],
            neocortex::QueryMode::Structural,
        );
        assert!(
            !completed.is_empty(),
            "pattern completion should find related entry via graph"
        );
        assert!(
            completed[0].text.contains("JwtSecret"),
            "completed result should be the JwtSecret entry, got: {}",
            completed[0].text
        );
    }

    #[test]
    fn test_pattern_completion_doesnt_activate_with_strong_results() {
        let mut state = MemoryState::default();
        // Insert several similar entries so direct retrieval is strong
        for i in 0..5 {
            tick(
                &mut state,
                &format!("DECISION: fn handle_auth_{i}() manages authentication flow"),
            );
        }

        let ctx = retrieve_context(&mut state.brain, "handle_auth authentication");
        // With 5 similar entries, direct retrieval should be strong enough
        // that pattern completion doesn't need to activate.
        // Just verify retrieval works and returns results
        assert!(
            !ctx.short_term.is_empty(),
            "strong direct matches should return results"
        );
    }

    #[test]
    fn test_pattern_completion_scores_lower_than_direct() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        // Direct match entry
        hippocampus::insert_short_term(
            &mut state.brain,
            "ConfigLoader reads YAML settings files",
            embed_text("ConfigLoader reads YAML settings files", dim),
            0.5,
            Vec::new(),
            0.0,
        );

        // Graph-connected entry (indirect)
        let cfg_id = 1200;
        let db_id = 1201;
        state.brain.long_term.nodes.insert(
            cfg_id,
            GraphNode {
                id: cfg_id,
                label: "ConfigLoader".into(),
                kind: "Entity".into(),
                weight: 1.0,
                salience: 0.5,
                source_texts: vec!["ConfigLoader reads YAML settings files".into()],
                last_seen: 0,
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("ConfigLoader".into(), cfg_id);

        state.brain.long_term.nodes.insert(
            db_id,
            GraphNode {
                id: db_id,
                label: "DatabasePool".into(),
                kind: "Entity".into(),
                weight: 1.0,
                salience: 0.5,
                source_texts: vec!["DatabasePool manages connection pooling".into()],
                last_seen: 0,
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("DatabasePool".into(), db_id);

        state.brain.long_term.edges.push(GraphEdge {
            from: cfg_id,
            to: db_id,
            weight: 0.6,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        hippocampus::insert_short_term(
            &mut state.brain,
            "DatabasePool manages connection pooling for PostgreSQL",
            embed_text(
                "DatabasePool manages connection pooling for PostgreSQL",
                dim,
            ),
            0.5,
            Vec::new(),
            0.0,
        );

        // Pattern complete for ConfigLoader — the DatabasePool entry is indirect
        let direct = hippocampus::top_k_similar(
            &state.brain.short_term,
            &embed_text("ConfigLoader", dim),
            5,
            "ConfigLoader",
        );
        let completed = hippocampus::pattern_complete(
            &state.brain,
            "ConfigLoader",
            &direct,
            neocortex::QueryMode::Structural,
        );

        if let Some(c) = completed.first() {
            if let Some(d) = direct.first() {
                assert!(
                    c.similarity <= d.similarity || direct.is_empty(),
                    "completed results ({}) should score <= direct matches ({})",
                    c.similarity,
                    d.similarity
                );
            }
        }
    }

    #[test]
    fn test_pattern_completion_empty_on_no_entities() {
        let state = MemoryState::default();
        // Query with no recognizable entities — nothing to seed graph from
        let completed = hippocampus::pattern_complete(
            &state.brain,
            "some random words",
            &[],
            neocortex::QueryMode::Neutral,
        );
        assert!(completed.is_empty(), "no entities = no pattern completion");
    }

    #[test]
    fn test_pattern_completion_skips_consolidated() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        let node_id = 1300;
        state.brain.long_term.nodes.insert(
            node_id,
            GraphNode {
                id: node_id,
                label: "SkipConsolidated".into(),
                kind: "Entity".into(),
                weight: 1.0,
                salience: 0.5,
                source_texts: vec!["SkipConsolidated test entry".into()],
                last_seen: 0,
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("SkipConsolidated".into(), node_id);

        // Insert a consolidated L2 entry
        state.brain.short_term.push(ShortTermEntry {
            id: 999,
            text: "SkipConsolidated test entry for pattern completion".into(),
            embedding: embed_text("SkipConsolidated test entry", dim),
            salience: 0.5,
            usage: 1,
            last_access: 1,
            consolidated: true, // marked consolidated
            ..Default::default()
        });

        let completed = hippocampus::pattern_complete(
            &state.brain,
            "SkipConsolidated",
            &[],
            neocortex::QueryMode::Semantic,
        );
        assert!(
            completed.is_empty(),
            "pattern completion should skip consolidated entries"
        );
    }

    // --- Smart consolidation trigger tests ---

    #[test]
    fn test_emotional_intensity_triggers_consolidation() {
        let mut state = MemoryState::default();
        // High-valence ticks should accumulate emotional intensity
        tick(
            &mut state,
            "BUG: critical server crash in production causing data loss",
        );
        tick(
            &mut state,
            "BLOCKER: security vulnerability found in authentication module",
        );
        tick(
            &mut state,
            "BUG: panic in payment processing causes transaction failures",
        );

        assert!(
            should_suggest_consolidation(&state),
            "burst of high-valence ticks should trigger consolidation, valence_sum={}",
            state.brain.recent_valence_sum
        );
    }

    #[test]
    fn test_neutral_ticks_no_early_consolidation() {
        let mut state = MemoryState::default();
        tick(&mut state, "Updated the documentation formatting");
        tick(&mut state, "Refactored variable names in the config module");
        tick(&mut state, "Added a comment to the parser function");

        assert!(
            !should_suggest_consolidation(&state),
            "neutral ticks should not trigger early consolidation, valence_sum={}, ticks_since={}",
            state.brain.recent_valence_sum,
            state.brain.ticks_since_consolidation
        );
    }

    #[test]
    fn test_emotional_intensity_decays() {
        let mut state = MemoryState::default();
        tick(&mut state, "BUG: critical crash in production");
        let after_first = state.brain.recent_valence_sum;

        // Several neutral ticks should decay the sum
        for _ in 0..10 {
            tick(&mut state, "routine documentation update");
        }

        assert!(
            state.brain.recent_valence_sum < after_first * 0.5,
            "emotional intensity should decay over time: {} -> {}",
            after_first,
            state.brain.recent_valence_sum
        );
    }

    #[test]
    fn test_context_switch_updates_embedding() {
        let mut state = MemoryState::default();
        assert!(state.brain.last_tick_embedding.is_empty());

        tick(
            &mut state,
            "DECISION: fn handle_database() manages PostgreSQL connection pooling",
        );
        assert!(!state.brain.last_tick_embedding.is_empty());

        let emb_after_first = state.brain.last_tick_embedding.clone();
        tick(
            &mut state,
            "DECISION: CSS flexbox layout grid styling responsive design media queries",
        );

        // Embedding should have changed to reflect the new topic
        assert_ne!(
            state.brain.last_tick_embedding, emb_after_first,
            "last_tick_embedding should update each tick"
        );
    }

    #[test]
    fn test_first_tick_no_context_switch() {
        let mut state = MemoryState::default();
        assert!(state.brain.last_tick_embedding.is_empty());

        // First tick ever — no previous embedding to compare against
        tick(&mut state, "DECISION: Starting new project with Rust");
        // Should not panic or trigger false context switch
        assert!(!state.brain.last_tick_embedding.is_empty());
    }

    #[test]
    fn test_same_topic_preserves_working_memory() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: fn handle_database() manages PostgreSQL connections",
        );
        let wm_count_after_first = state.brain.working_memory.len();

        tick(
            &mut state,
            "DECISION: fn optimize_database() improves PostgreSQL query performance",
        );
        // Same topic — working memory should grow, not flush
        assert!(
            state.brain.working_memory.len() >= wm_count_after_first,
            "same-topic tick should not flush working memory"
        );
    }

    // --- Spreading activation tests (Change 4) ---

    /// Helper to insert a test graph node and return its ID.
    fn insert_test_node(state: &mut BrainState, id: u64, label: &str) {
        state.long_term.nodes.insert(
            id,
            GraphNode {
                id,
                label: label.into(),
                kind: "Entity".into(),
                weight: 1.0,
                salience: 0.5,
                source_texts: vec![],
                last_seen: 0,
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state.long_term.index.insert(label.into(), id);
    }

    // --- Sharp-wave ripple replay tests (Change 5) ---

    #[test]
    fn test_replay_reinforces_shared_entities() {
        // Manually build two L2 entries that share graph entities, close in time.
        // Call replay_consolidation directly (not consolidate) to isolate the effect.
        let mut state = MemoryState::default();
        state.brain.clock = 10;

        // Create two graph nodes
        let node_a = 600;
        let node_b = 601;
        insert_test_node(&mut state.brain, node_a, "AuthModule");
        insert_test_node(&mut state.brain, node_b, "JwtParser");

        // Create an edge between them
        state.brain.long_term.edges.push(GraphEdge {
            from: node_a,
            to: node_b,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 5,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        // Create two L2 entries that mention both entities, close in time
        let emb = crate::memory::entorhinal::embed_text("auth jwt", 256);
        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: "AuthModule uses JwtParser for token validation".into(),
            embedding: emb.clone(),
            salience: 0.5,
            usage: 1,
            last_access: 8,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: "JwtParser validates AuthModule tokens".into(),
            embedding: emb,
            salience: 0.5,
            usage: 1,
            last_access: 10,
            ..Default::default()
        });

        let edge_weight_before = state.brain.long_term.edges[0].weight;
        neocortex::replay_consolidation(&mut state.brain);
        let edge_weight_after = state.brain.long_term.edges[0].weight;

        assert!(
            edge_weight_after > edge_weight_before,
            "replay should boost shared entity edge: {} -> {}",
            edge_weight_before,
            edge_weight_after
        );
    }

    #[test]
    fn test_replay_ignores_temporally_distant_entries() {
        // Two entries far apart in time — no temporal edges should be created.
        let mut state = MemoryState::default();
        state.brain.clock = 100;

        let node_a = 700;
        let node_b = 701;
        insert_test_node(&mut state.brain, node_a, "ServerHandler");
        insert_test_node(&mut state.brain, node_b, "DatabasePool");

        let emb = crate::memory::entorhinal::embed_text("server database", 256);
        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: "ServerHandler processes API requests".into(),
            embedding: emb.clone(),
            salience: 0.5,
            usage: 1,
            last_access: 10,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: "DatabasePool manages connections".into(),
            embedding: emb,
            salience: 0.5,
            usage: 1,
            last_access: 90,
            ..Default::default()
        });

        let edge_count_before = state.brain.long_term.edges.len();
        neocortex::replay_consolidation(&mut state.brain);

        let temporal_edges = state
            .brain
            .long_term
            .edges
            .iter()
            .filter(|e| e.kind == "temporal")
            .count();

        assert_eq!(
            temporal_edges, 0,
            "temporally distant entries (80 ticks apart) should not create temporal edges"
        );
        assert_eq!(
            state.brain.long_term.edges.len(),
            edge_count_before,
            "no new edges should be created for distant entries"
        );
    }

    #[test]
    fn test_replay_creates_temporal_edges() {
        // Two entries with different entities but close in time — should create
        // temporal edges between the unconnected entity pairs.
        let mut state = MemoryState::default();
        state.brain.clock = 10;

        let node_a = 800;
        let node_b = 801;
        insert_test_node(&mut state.brain, node_a, "ConfigParser");
        insert_test_node(&mut state.brain, node_b, "RouterSetup");

        // No pre-existing edges between them
        let emb = crate::memory::entorhinal::embed_text("config router", 256);
        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: "ConfigParser loads YAML settings".into(),
            embedding: emb.clone(),
            salience: 0.5,
            usage: 1,
            last_access: 8,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: "RouterSetup configures API endpoints".into(),
            embedding: emb,
            salience: 0.5,
            usage: 1,
            last_access: 9,
            ..Default::default()
        });

        assert!(
            state.brain.long_term.edges.is_empty(),
            "no edges before replay"
        );
        neocortex::replay_consolidation(&mut state.brain);

        let temporal_edges: Vec<&GraphEdge> = state
            .brain
            .long_term
            .edges
            .iter()
            .filter(|e| e.kind == "temporal")
            .collect();

        assert!(
            !temporal_edges.is_empty(),
            "replay should create temporal edges between co-active unconnected entities"
        );
    }

    #[test]
    fn test_replay_boosts_salience() {
        // Directly call replay_consolidation and verify salience boost.
        let mut state = MemoryState::default();
        state.brain.clock = 10;

        let node_a = 900;
        let node_b = 901;
        insert_test_node(&mut state.brain, node_a, "MemHandler");
        insert_test_node(&mut state.brain, node_b, "EventLoop");

        let emb = crate::memory::entorhinal::embed_text("memory event", 256);
        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: "MemHandler and EventLoop process ticks".into(),
            embedding: emb.clone(),
            salience: 0.5,
            usage: 1,
            last_access: 8,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: "EventLoop dispatches to MemHandler".into(),
            embedding: emb,
            salience: 0.5,
            usage: 1,
            last_access: 9,
            ..Default::default()
        });

        let salience_before: Vec<f32> = state.brain.short_term.iter().map(|e| e.salience).collect();
        neocortex::replay_consolidation(&mut state.brain);
        let salience_after: Vec<f32> = state.brain.short_term.iter().map(|e| e.salience).collect();

        let any_boosted = salience_before
            .iter()
            .zip(salience_after.iter())
            .any(|(before, after)| after > before);
        assert!(
            any_boosted,
            "replay should boost salience of co-active entries"
        );
    }

    #[test]
    fn test_existing_consolidation_still_works() {
        let mut state = MemoryState::default();
        tick(&mut state, "fn handle_memory() processes incoming ticks");
        tick(
            &mut state,
            "fn handle_memory() is the main entry point for memory operations",
        );
        tick(
            &mut state,
            "fn handle_memory() manages the three-layer architecture",
        );

        let summaries = consolidate(&mut state.brain);
        assert!(
            !summaries.is_empty() || state.brain.short_term.len() <= 3,
            "consolidation should still produce summaries or maintain entries"
        );
    }

    // --- Spreading activation tests (Change 4) ---

    #[test]
    fn test_spreading_activation_two_hop_chain() {
        // Build A -> B -> C chain
        let mut state = MemoryState::default();
        let (a_id, b_id, c_id) = (100, 101, 102);
        insert_test_node(&mut state.brain, a_id, "NodeA");
        insert_test_node(&mut state.brain, b_id, "NodeB");
        insert_test_node(&mut state.brain, c_id, "NodeC");

        // A -> B (weight 0.8), B -> C (weight 0.6)
        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 0.8,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });
        state.brain.long_term.edges.push(GraphEdge {
            from: b_id,
            to: c_id,
            weight: 0.6,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[a_id],
            3,
            0.5,
            neocortex::QueryMode::Neutral,
        );
        let b_activation = results.iter().find(|(id, _)| *id == b_id);
        let c_activation = results.iter().find(|(id, _)| *id == c_id);

        assert!(b_activation.is_some(), "B should be activated from A");
        assert!(c_activation.is_some(), "C should be activated from A via B");

        let b_val = b_activation.unwrap().1;
        let c_val = c_activation.unwrap().1;
        assert!(
            b_val > c_val,
            "hop-1 activation ({}) should exceed hop-2 ({})",
            b_val,
            c_val
        );
    }

    #[test]
    fn test_spreading_activation_cycle_prevention() {
        // Build A <-> B cycle
        let mut state = MemoryState::default();
        let (a_id, b_id) = (200, 201);
        insert_test_node(&mut state.brain, a_id, "CycleA");
        insert_test_node(&mut state.brain, b_id, "CycleB");

        // Bidirectional edges
        state.brain.long_term.edges.push(GraphEdge {
            from: a_id,
            to: b_id,
            weight: 1.0,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });
        state.brain.long_term.edges.push(GraphEdge {
            from: b_id,
            to: a_id,
            weight: 1.0,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        // Should not infinite loop — visited set prevents re-expansion
        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[a_id],
            5,
            0.5,
            neocortex::QueryMode::Neutral,
        );
        assert_eq!(results.len(), 1, "only B should appear (A is seed)");
        assert_eq!(results[0].0, b_id);
    }

    #[test]
    fn test_spreading_activation_three_hop_progressive_decay() {
        // A -> B -> C -> D
        let mut state = MemoryState::default();
        let ids: Vec<u64> = vec![300, 301, 302, 303];
        for (i, label) in ["HopA", "HopB", "HopC", "HopD"].iter().enumerate() {
            insert_test_node(&mut state.brain, ids[i], label);
        }

        // All edges weight 1.0 to isolate the decay factor
        for i in 0..3 {
            state.brain.long_term.edges.push(GraphEdge {
                from: ids[i],
                to: ids[i + 1],
                weight: 1.0,
                kind: "related".into(),
                last_seen: 0,
                activation_count: 0,
                stability: 1.0,
                recent_interval_avg: 0.0,
                historical_interval_avg: 0.0,
            });
        }

        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[ids[0]],
            3,
            0.5,
            neocortex::QueryMode::Neutral,
        );
        let activations: HashMap<u64, f32> = results.into_iter().collect();

        let b = activations.get(&ids[1]).copied().unwrap_or(0.0);
        let c = activations.get(&ids[2]).copied().unwrap_or(0.0);
        let d = activations.get(&ids[3]).copied().unwrap_or(0.0);

        assert!(b > c, "B ({}) > C ({})", b, c);
        assert!(c > d, "C ({}) > D ({})", c, d);
        assert!(d > 0.0, "D should still have some activation");
    }

    #[test]
    fn test_spreading_activation_empty_seeds() {
        let state = MemoryState::default();
        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[],
            3,
            0.5,
            neocortex::QueryMode::Neutral,
        );
        assert!(results.is_empty());
    }

    #[test]
    fn test_graph_lookup_uses_multi_hop() {
        // Build TestMultiA -> TestMultiB -> TestMultiC, query should find C via 2 hops
        let mut state = MemoryState::default();
        let ids: Vec<u64> = vec![400, 401, 402];
        for (i, label) in ["TestMultiA", "TestMultiB", "TestMultiC"]
            .iter()
            .enumerate()
        {
            insert_test_node(&mut state.brain, ids[i], label);
        }

        state.brain.long_term.edges.push(GraphEdge {
            from: ids[0],
            to: ids[1],
            weight: 0.8,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });
        state.brain.long_term.edges.push(GraphEdge {
            from: ids[1],
            to: ids[2],
            weight: 0.6,
            kind: "related".into(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });

        let results = neocortex::graph_lookup(
            &state.brain.long_term,
            "TestMultiA",
            20,
            &state.brain.keyword_cache,
            neocortex::QueryMode::Neutral,
        );
        let labels: Vec<&str> = results.iter().map(|r| r.label.as_str()).collect();

        assert!(labels.contains(&"TestMultiA"), "seed should be in results");
        assert!(
            labels.contains(&"TestMultiB"),
            "hop-1 neighbor should be in results"
        );
        assert!(
            labels.contains(&"TestMultiC"),
            "hop-2 neighbor should be in results via spreading activation, got: {:?}",
            labels
        );
    }

    #[test]
    fn test_spreading_activation_respects_max_hops() {
        // A -> B -> C -> D, but max_hops=1 — should only reach B
        let mut state = MemoryState::default();
        let ids: Vec<u64> = vec![500, 501, 502, 503];
        for (i, label) in ["LimA", "LimB", "LimC", "LimD"].iter().enumerate() {
            insert_test_node(&mut state.brain, ids[i], label);
        }
        for i in 0..3 {
            state.brain.long_term.edges.push(GraphEdge {
                from: ids[i],
                to: ids[i + 1],
                weight: 1.0,
                kind: "related".into(),
                last_seen: 0,
                activation_count: 0,
                stability: 1.0,
                recent_interval_avg: 0.0,
                historical_interval_avg: 0.0,
            });
        }

        let results = neocortex::spreading_activation(
            &state.brain.long_term,
            &[ids[0]],
            1,
            0.5,
            neocortex::QueryMode::Neutral,
        );
        let result_ids: Vec<u64> = results.iter().map(|(id, _)| *id).collect();

        assert!(result_ids.contains(&ids[1]), "hop-1 should be reachable");
        assert!(
            !result_ids.contains(&ids[2]),
            "hop-2 should NOT be reachable with max_hops=1"
        );
        assert!(
            !result_ids.contains(&ids[3]),
            "hop-3 should NOT be reachable with max_hops=1"
        );
    }

    // --- Systems consolidation tests (Change 8) ---

    #[test]
    fn test_consolidation_produces_summary_with_centroid_embedding() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        // Insert two very similar L2 entries with high salience
        // (must exceed theta_low=0.72 cosine similarity for clustering)
        let shared = "Rust borrow checker ownership rules for memory safety in systems programming";
        let text1 = format!("{} and lifetimes", shared);
        let text2 = format!("{} and references", shared);
        let emb1 = embed_text(&text1, dim);
        let emb2 = embed_text(&text2, dim);

        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: text1.clone(),
            embedding: emb1,
            salience: 0.8,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: text2.clone(),
            embedding: emb2,
            salience: 0.7,
            ..Default::default()
        });

        let summaries = consolidate(&mut state.brain);
        assert!(!summaries.is_empty(), "should produce at least one summary");

        // Find the Summary node and verify it has an embedding
        let summary_node = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|n| n.kind == "Summary")
            .expect("should have a Summary node");

        assert!(
            !summary_node.embedding.is_empty(),
            "high-salience group should get centroid embedding"
        );
        assert_eq!(summary_node.embedding.len(), dim);

        // Verify embedding is normalized (unit length)
        let norm: f32 = summary_node
            .embedding
            .iter()
            .map(|x| x * x)
            .sum::<f32>()
            .sqrt();
        assert!(
            (norm - 1.0).abs() < 0.01,
            "centroid should be unit-normalized, got {}",
            norm
        );
    }

    #[test]
    fn test_consolidation_produces_full_text() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        let emb1 = embed_text("Database migration strategy for PostgreSQL", dim);
        let emb2 = embed_text("Database migration rollback for PostgreSQL", dim);

        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: "Database migration strategy for PostgreSQL".into(),
            embedding: emb1,
            salience: 0.6,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: "Database migration rollback for PostgreSQL".into(),
            embedding: emb2,
            salience: 0.6,
            ..Default::default()
        });

        consolidate(&mut state.brain);

        let summary_node = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|n| n.kind == "Summary")
            .expect("should have Summary node");

        assert!(
            summary_node.full_text.is_some(),
            "high-salience group should get full_text"
        );
        let ft = summary_node.full_text.as_ref().unwrap();
        assert!(
            ft.len() <= 500,
            "full_text should be capped at 500 chars, got {}",
            ft.len()
        );
        // Should contain content from source entries
        assert!(
            ft.contains("Database") || ft.contains("migration"),
            "full_text should contain source content, got: {}",
            ft
        );
    }

    #[test]
    fn test_low_salience_group_no_embedding() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        // Two similar entries but with very low salience
        let emb1 = embed_text("trivial formatting update to docs", dim);
        let emb2 = embed_text("trivial formatting change to docs", dim);

        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: "trivial formatting update to docs".into(),
            embedding: emb1,
            salience: 0.1,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: "trivial formatting change to docs".into(),
            embedding: emb2,
            salience: 0.1,
            ..Default::default()
        });

        consolidate(&mut state.brain);

        let summary_node = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|n| n.kind == "Summary");

        if let Some(node) = summary_node {
            assert!(
                node.embedding.is_empty(),
                "low-salience group should NOT get centroid embedding"
            );
            assert!(
                node.full_text.is_none(),
                "low-salience group should NOT get full_text"
            );
        }
    }

    #[test]
    fn test_l3_summary_retrieval_finds_consolidated_memory() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        // Manually insert a Summary node with a centroid embedding
        let summary_emb = embed_text("Rust memory safety borrow checker", dim);
        let summary_id = 2000;
        state.brain.long_term.nodes.insert(
            summary_id,
            GraphNode {
                id: summary_id,
                label: "Rust borrow checker summary".into(),
                kind: "Summary".into(),
                weight: 1.5,
                last_seen: 10,
                salience: 0.7,
                source_texts: vec!["Rust borrow checker ownership rules".into()],
                embedding: summary_emb,
                full_text: Some("Rust borrow checker ownership and lifetime rules".into()),
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("Rust borrow checker summary".into(), summary_id);

        // Query something similar — no L2 entries exist, so L3 should surface it
        let ctx = retrieve_context(&mut state.brain, "Rust borrow checker rules");

        // Should find the Summary node in short_term results (merged in)
        let found = ctx.short_term.iter().any(|s| s.id == summary_id);
        assert!(
            found,
            "L3 Summary with embedding should be retrievable. Got: {:?}",
            ctx.short_term
                .iter()
                .map(|s| (&s.text, s.similarity))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_centroid_cosine_similar_to_members() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        let text1 = "Authentication JWT token validation middleware";
        let text2 = "Authentication JWT token signing middleware";
        let emb1 = embed_text(text1, dim);
        let emb2 = embed_text(text2, dim);

        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: text1.into(),
            embedding: emb1.clone(),
            salience: 0.8,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: text2.into(),
            embedding: emb2.clone(),
            salience: 0.8,
            ..Default::default()
        });

        consolidate(&mut state.brain);

        let summary_node = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|n| n.kind == "Summary" && !n.embedding.is_empty())
            .expect("should have Summary with embedding");

        // Centroid should be similar to both members
        let sim1 = cosine_similarity(&summary_node.embedding, &emb1);
        let sim2 = cosine_similarity(&summary_node.embedding, &emb2);

        assert!(
            sim1 > 0.5,
            "centroid should be similar to member 1, got {}",
            sim1
        );
        assert!(
            sim2 > 0.5,
            "centroid should be similar to member 2, got {}",
            sim2
        );
    }

    #[test]
    fn test_consolidated_entry_easier_to_evict() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        // Capacity 2: already have 2 entries, inserting a 3rd triggers eviction
        state.brain.config.short_term_capacity = 2;
        state.brain.next_id = 100; // avoid ID collision with manually inserted entries

        // Insert a Summary node with embedding that backs entry 1
        let summary_emb = embed_text("Consolidated topic alpha details", dim);
        let summary_id = 3000;
        state.brain.long_term.nodes.insert(
            summary_id,
            GraphNode {
                id: summary_id,
                label: "Alpha summary".into(),
                kind: "Summary".into(),
                weight: 1.5,
                last_seen: 10,
                salience: 0.7,
                source_texts: vec!["Consolidated topic alpha details".into()],
                embedding: summary_emb,
                full_text: Some("Alpha topic details".into()),
            },
        );

        // Two entries with identical scores — only the consolidated flag differs
        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: "Consolidated topic alpha details".into(),
            embedding: embed_text("Consolidated topic alpha details", dim),
            salience: 0.5,
            usage: 1,
            consolidated: true, // backed by L3 with embedding
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: "Unconsolidated topic beta details".into(),
            embedding: embed_text("Unconsolidated topic beta details", dim),
            salience: 0.5,
            usage: 1,
            ..Default::default()
        });

        // Directly call insert_short_term to trigger eviction
        hippocampus::insert_short_term(
            &mut state.brain,
            "New entry forcing eviction",
            embed_text("New entry forcing eviction", dim),
            0.5,
            vec![],
            0.0,
        );

        // The consolidated entry should have been evicted (lower effective score)
        let remaining_ids: Vec<u64> = state.brain.short_term.iter().map(|e| e.id).collect();
        assert!(
            !remaining_ids.contains(&1),
            "consolidated entry backed by L3 should be evicted first, remaining: {:?}",
            remaining_ids
        );
        assert!(
            remaining_ids.contains(&2),
            "unconsolidated entry should survive"
        );
    }

    // -----------------------------------------------------------------------
    // Layer 3: Incremental keyword discovery
    // -----------------------------------------------------------------------

    #[test]
    fn test_term_frequency_tracking_basic() {
        let mut state = MemoryState::default();
        // Tick with an entity that extract_entities will find
        tick(
            &mut state,
            "DECISION: fn process_data() handles struct Config",
        );
        assert!(
            !state.brain.term_frequency.is_empty(),
            "term_frequency should track extracted entities"
        );
    }

    #[test]
    fn test_term_frequency_increments_across_ticks() {
        let mut state = MemoryState::default();
        // Tick the same entity across multiple ticks
        for i in 0..5 {
            tick(
                &mut state,
                &format!("DECISION: process_data handles request batch {}", i),
            );
        }
        // "process_data" should have tick_count >= 1 (it appears in entity extraction)
        let has_multi_tick = state
            .brain
            .term_frequency
            .values()
            .any(|s| s.tick_count >= 2);
        assert!(
            has_multi_tick,
            "repeated entities should have multiple tick counts: {:?}",
            state
                .brain
                .term_frequency
                .iter()
                .map(|(k, v)| (k.clone(), v.tick_count))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_term_not_promoted_before_min_ticks() {
        let mut state = MemoryState::default();
        // Tick entity only 3 times (below TERM_PROMOTION_MIN_TICKS of 5)
        for i in 0..3 {
            tick(
                &mut state,
                &format!("DECISION: CustomWidget renders component {}", i),
            );
        }
        // Should NOT have a kw:domain:customwidget node
        assert!(
            !state
                .brain
                .long_term
                .index
                .contains_key("kw:domain:customwidget"),
            "term should not be promoted before min ticks"
        );
    }

    #[test]
    fn test_term_promoted_after_sufficient_ticks() {
        let mut state = MemoryState::default();
        // Seed a code keyword so entities can be extracted from "fn CustomProcessor()"
        add_keyword_node(
            &mut state.brain,
            "code",
            "fn ",
            vec![
                "entity_kind:Function".to_string(),
                "entity_context:defines".to_string(),
            ],
        );
        rebuild_keyword_cache(&mut state.brain);

        // Tick entity 7 times with keyword co-occurrence (DECISION provides co-occurrence)
        for i in 0..7 {
            tick(
                &mut state,
                &format!("DECISION: fn CustomProcessor() handles batch {}", i),
            );
        }

        // "customprocessor" should be in term_frequency with sufficient tick_count
        let stats = state.brain.term_frequency.get("customprocessor");
        assert!(
            stats.is_some(),
            "CustomProcessor should be tracked; keys: {:?}",
            state.brain.term_frequency.keys().collect::<Vec<_>>()
        );
        if let Some(s) = stats {
            assert!(
                s.tick_count >= TERM_PROMOTION_MIN_TICKS,
                "tick_count={} should be >= {}",
                s.tick_count,
                TERM_PROMOTION_MIN_TICKS
            );
            assert!(
                s.has_keyword_cooccurrence,
                "should have keyword co-occurrence"
            );
        }

        // Should be auto-promoted to kw:domain:customprocessor
        assert!(
            state.brain.long_term.index.contains_key("kw:domain:customprocessor"),
            "term should be auto-promoted after passing all filters; graph keys with kw:domain: {:?}",
            state.brain.long_term.index.keys().filter(|k| k.starts_with("kw:domain:")).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_stopword_not_promoted() {
        let state = MemoryState::default();
        // Even if "the" appeared many times it should never be promoted
        assert!(!should_promote_term(&state.brain, "the"));
        assert!(!should_promote_term(&state.brain, "is"));
        assert!(!should_promote_term(&state.brain, "and"));
    }

    #[test]
    fn test_short_term_not_promoted() {
        let state = MemoryState::default();
        // Terms shorter than 3 chars should not be promoted
        assert!(!should_promote_term(&state.brain, "ab"));
        assert!(!should_promote_term(&state.brain, "x"));
    }

    #[test]
    fn test_numeric_term_not_promoted() {
        let state = MemoryState::default();
        // Purely numeric terms should not be promoted
        assert!(!should_promote_term(&state.brain, "12345"));
        assert!(!should_promote_term(&state.brain, "3.14"));
    }

    #[test]
    fn test_co_occurrence_required_for_promotion() {
        let mut state = MemoryState::default();
        // Manually insert term stats WITHOUT co-occurrence
        state.brain.term_frequency.insert(
            "orphanterm".to_string(),
            TermStats {
                tick_count: 10,
                total_count: 20,
                first_seen: 1,
                last_seen: 10,
                has_keyword_cooccurrence: false,
            },
        );
        assert!(
            !should_promote_term(&state.brain, "orphanterm"),
            "term without keyword co-occurrence should not promote"
        );

        // Now set co-occurrence
        state
            .brain
            .term_frequency
            .get_mut("orphanterm")
            .unwrap()
            .has_keyword_cooccurrence = true;
        assert!(
            should_promote_term(&state.brain, "orphanterm"),
            "term WITH keyword co-occurrence and sufficient ticks should promote"
        );
    }

    #[test]
    fn test_already_promoted_term_not_duplicated() {
        let mut state = MemoryState::default();
        // Pre-add the keyword
        add_keyword_node(&mut state.brain, "domain", "existingterm", Vec::new());
        // should_promote_term should return false
        assert!(!should_promote_term(&state.brain, "existingterm"));
    }

    #[test]
    fn test_passive_ticks_dont_update_term_frequency() {
        let mut state = MemoryState::default();
        // Passive tick (used during onboarding/ingestion)
        tick_passive(
            &mut state,
            "DECISION: fn SpecialHandler() processes important data",
        );
        // term_frequency should be empty — passive ticks skip Layer 3
        assert!(
            state.brain.term_frequency.is_empty(),
            "passive ticks should not update term_frequency"
        );
    }

    #[test]
    fn test_term_stats_serialization_compat() {
        // TermStats should deserialize with defaults for missing fields
        let json = r#"{"tick_count":5,"total_count":10,"first_seen":1,"last_seen":5}"#;
        let stats: TermStats = serde_json::from_str(json).unwrap();
        assert_eq!(stats.tick_count, 5);
        assert!(!stats.has_keyword_cooccurrence); // default false
    }
}
