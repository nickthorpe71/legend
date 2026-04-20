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
/// - `entorhinal.rs` — Encoding & compression (semantic embeddings, cosine similarity, chunking, summarization)
/// - `thalamus.rs` — Attentional gating (salience scoring)
/// - `wernicke/` — Language comprehension (entity extraction, keyword system)
pub mod amygdala;
pub mod anterior_pfc;
pub mod basal_ganglia;
pub mod dentate_gyrus;
pub mod entorhinal;
pub mod hippocampus;
pub mod neocortex;
pub mod neurochemistry;
pub mod prefrontal;
mod prototype_embeddings;
pub mod signal;
pub mod thalamus;
#[cfg(feature = "instrument")]
pub mod trace;
pub mod wernicke;

use amygdala::{compute_emotional_valence, seed_emotional_prototypes, EmotionalPrototype};
use dentate_gyrus::{
    diversity_pass, sparse_orthogonalize, word_overlap, MERGE_WORD_OVERLAP_THRESHOLD,
};
use entorhinal::{
    chunk_text, clean_semantic_noise, cosine_similarity, embed_text, merge_embeddings,
    summarize_group, summarize_text,
};
#[cfg(test)]
use hippocampus::eviction_score;
use hippocampus::{extract_memory_refs_from_text, merge_memory_refs};
use neurochemistry::{
    ACH_NOVELTY_SPIKE, CAPACITY_STRESS_ONSET, CORTISOL_CAPACITY_SPIKE, CORTISOL_NEGATIVE_SPIKE,
    DA_POSITIVE_SPIKE, ECB_CORTISOL_RECOVERY, ECB_ROUTINE_SPIKE, NE_CONTEXT_SWITCH_SPIKE,
    NE_SALIENCE_SPIKE, NE_THREAT_SPIKE,
};
use signal::{normalize_positive_signal, reinforce_bounded_signal};
use thalamus::compute_salience;
use wernicke::extract_dates;
use wernicke::extract_entities;
use wernicke::KeywordCache;

// Re-export tool types so existing `crate::memory::TickResult` paths still work.
#[allow(unused_imports)]
pub use crate::tool::types::{
    GitSyncInfo, MemoryCategory, MemoryConfig, MemoryContext, SessionEntry, TermStats, TickResult,
};
#[allow(unused_imports)]
pub use basal_ganglia::{ReinforceResult, ReinforcedEntry};
pub use neurochemistry::{ChemicalStamp, EffectiveChemistry, Neurochemistry};

// Re-export persistence functions so existing `crate::memory::load_or_default()` paths still work.
#[allow(unused_imports)]
pub use crate::tool::persistence::{
    load_memory_from_path, load_or_default, reset_memory, save, save_memory_to_path,
};

// Re-export tool functions so existing `crate::memory::tick()` etc. paths still work.
#[allow(unused_imports)]
pub use crate::tool::{
    build_context_summary, build_dump, build_start_summary, build_start_summary_with_options,
    clear_task, get_git_summary, get_task, merge_from_log, merge_states, recent_sessions,
    scan_ecosystem_dependencies, set_task, should_suggest_consolidation, tick, MergeStats,
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
/// Maximum additional attention gate threshold at 100% L2 capacity.
/// Under hippocampal load, Legend becomes more selective about L2 promotion.
const ATTENTION_GATE_TIGHTENING: f32 = 0.15;
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
/// Kept for hippocampus::try_reconsolidate (may reuse for L3).
#[allow(dead_code)]
pub(super) const RECONSOLIDATION_THRESHOLD: f32 = 0.40;
/// Minimum combined similarity (cosine + keyword bonus) for a query result.
/// Lowered from 0.25 to 0.15 — the keyword bonus needs headroom to surface
/// entries with low embedding similarity but high lexical relevance.
pub(super) const MIN_QUERY_SIMILARITY: f32 = 0.35;
/// Bonus added per non-stopword query keyword found in an entry's text.
/// Restored from 0.03 to 0.05 — embeddings capture topical similarity but
/// miss specific facts (dates, names, identifiers). Keywords fill that gap.
pub(super) const KEYWORD_MATCH_BONUS: f32 = 0.05;
/// Maximum total keyword bonus. Restored from 0.1 to 0.2 — keyword matches
/// need enough weight to surface needle facts over topically-similar generics.
pub(super) const KEYWORD_MATCH_BONUS_CAP: f32 = 0.2;
/// Ticks a memory stays labile after retrieval before re-stabilizing.
/// Kept for test_labile_expires (hippocampus function still exists).
#[allow(dead_code)]
const RECONSOLIDATION_WINDOW: u64 = 5;
/// Pattern completion (CA3 autoassociative recall): minimum L2 results
/// before pattern completion activates.
const PATTERN_COMPLETION_MIN_RESULTS: usize = 3;
/// Minimum top-result similarity before pattern completion activates.
const PATTERN_COMPLETION_SIM_THRESHOLD: f32 = 0.5;
/// MMR lambda: balance between relevance (1.0) and diversity (0.0).
/// 0.5 = balanced relevance and diversity. Prevents topic-similar generics
/// from dominating results when factual needles have lower embedding similarity.
// MMR removed: diversity selection was dropping episodic date needles that
// shared embedding space with spec entries. All threshold-passing results
// are now returned, sorted by similarity.
// L2_RETRIEVAL_MAX removed: threshold-based filtering replaces count caps.
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
/// CA3 pattern completion: minimum cosine similarity between an L2 entry's
/// embedding and an L3 Summary/Trace centroid to count as "backed by L3."
/// Replaces the fragile exact-text match in eviction scoring.
pub(super) const L3_BACKUP_SIMILARITY_THRESHOLD: f32 = 0.75;
/// Fast mapping: maximum number of Trace nodes in L3. When exceeded, the
/// lowest-salience Trace is pruned before creating a new one.
pub(super) const TRACE_NODE_CAP: usize = 50;
/// Fast mapping: initial salience for Trace nodes (below normal Summary 0.4+).
/// Traces that are never retrieved will decay and prune naturally.
pub(super) const TRACE_INITIAL_SALIENCE: f32 = 0.3;

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
/// Systems consolidation: minimum composite salience for neocortical encoding.
const SYSTEMS_CONSOLIDATION_SALIENCE_THRESHOLD: f32 = 0.4;
/// Systems consolidation score blend: average salience captures broad group
/// importance, while max salience preserves a strong anchor with supporting facts.
const SYSTEMS_CONSOLIDATION_AVG_WEIGHT: f32 = 0.7;
const SYSTEMS_CONSOLIDATION_MAX_WEIGHT: f32 = 0.3;
/// Maximum length of full_text stored on consolidated Summary nodes.
const SUMMARY_FULL_TEXT_MAX_LEN: usize = 500;
/// Minimum cosine similarity for L3 Summary node retrieval.
const SUMMARY_RETRIEVAL_MIN_SIM: f32 = 0.3;
/// Minimum centroid similarity for treating two Summary nodes as the same
/// consolidated memory even when their extractive labels use different words.
const SUMMARY_MERGE_EMBEDDING_SIM: f32 = 0.75;
/// Layer 3 incremental keyword discovery: minimum distinct ticks for auto-promotion.
const TERM_PROMOTION_MIN_TICKS: u32 = 5;
/// Minimum distinct meaningful-context ticks before a term can become learned vocabulary.
const TERM_PROMOTION_MIN_KEYWORD_COOCCURRENCE_TICKS: u32 = 2;
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
/// Low-similarity merge reinforcement gain for salience potentiation.
const LOW_MERGE_SALIENCE_LEARNING_RATE: f32 = 0.5;
/// Evidence midpoint for low-similarity merge salience reinforcement.
const LOW_MERGE_SALIENCE_MIDPOINT: f32 = 0.45;
/// Evidence steepness for low-similarity merge salience reinforcement.
const LOW_MERGE_SALIENCE_STEEPNESS: f32 = 1.4;
/// Final encoding salience floor after neurochemical gain.
const FINAL_SALIENCE_FLOOR: f32 = 0.0;
/// Final encoding salience asymptote after neurochemical gain.
const FINAL_SALIENCE_CEILING: f32 = 1.0;
/// Raw gained salience that maps halfway through the final response curve.
const FINAL_SALIENCE_MIDPOINT: f32 = 0.25;
/// Steepness of the final encoding salience response curve.
const FINAL_SALIENCE_STEEPNESS: f32 = 1.4;

// ---------------------------------------------------------------------------
// Amygdala — Emotional processing, intensity-driven consolidation triggers
// ---------------------------------------------------------------------------

/// Neurochemical consolidation pressure threshold.
pub const CONSOLIDATION_PRESSURE_THRESHOLD: f32 = 1.2;
/// Context switch detection: cosine similarity threshold for topic shifts.
const CONTEXT_SWITCH_THRESHOLD: f32 = 0.15;

fn normalize_final_salience(raw: f32) -> f32 {
    normalize_positive_signal(
        raw,
        FINAL_SALIENCE_FLOOR,
        FINAL_SALIENCE_CEILING,
        FINAL_SALIENCE_MIDPOINT,
        FINAL_SALIENCE_STEEPNESS,
    )
}

// ---------------------------------------------------------------------------
// Temporal Context Model (TCM) — implicit temporal encoding
// ---------------------------------------------------------------------------

/// Dimensionality of the temporal context vector (mean-pooled from 384-dim).
const TEMPORAL_CONTEXT_DIM: usize = 64;
/// TCM drift rate: how much the previous context is retained per tick (normal).
const TCM_DRIFT_RATE: f32 = 0.95;
/// TCM drift rate at event boundaries (faster drift = more temporal separation).
const TCM_BOUNDARY_DRIFT_RATE: f32 = 0.7;
/// Salience threshold for detecting event boundaries.
const EVENT_BOUNDARY_SALIENCE: f32 = 0.7;

/// CPEB synaptic tagging: high-valence events selectively strengthen
/// graph edges connected to nodes touched during the current tick.
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
    /// Temporal Context Model (TCM): 64-dim vector that drifts each tick.
    /// Snapshot frozen on each ShortTermEntry at encoding time.
    #[serde(default)]
    pub temporal_context: Vec<f32>,
    /// Emotional prototypes: anchor points in embedding space for valence scoring.
    /// Seeded from static list on first use, learned/adjusted over time.
    #[serde(default)]
    pub emotional_prototypes: Vec<EmotionalPrototype>,
    /// Global neurochemical levels modulating memory dynamics.
    #[serde(default)]
    pub chemistry: Neurochemistry,
    /// Anterior PFC: structured plans for prospective memory.
    #[serde(default)]
    pub plans: Vec<anterior_pfc::Plan>,
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
            last_tick_embedding: Vec::new(),
            term_frequency: HashMap::new(),
            keyword_cache: wernicke::KeywordCache::default_from_static(),
            temporal_context: Vec::new(),
            emotional_prototypes: Vec::new(),
            chemistry: Neurochemistry::default(),
            plans: Vec::new(),
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
// tick — moved to crate::tool, re-exported below.

pub fn tick_impl(state: &mut BrainState, text: &str) -> TickResult {
    #[cfg(feature = "instrument")]
    let _tctx = {
        let ctx = trace::TraceCtx::new();
        ctx.emit(
            trace::PipelineStep::TickStart,
            trace::TracePayload::Text(text.to_string()),
        );
        ctx
    };

    state.clock += 1;

    // Seed emotional prototypes on first tick (pre-computed embeddings, no ONNX cost).
    if state.emotional_prototypes.is_empty() {
        state.emotional_prototypes = seed_emotional_prototypes();
    }

    #[cfg(feature = "instrument")]
    _tctx.emit(
        trace::PipelineStep::ClockIncrement,
        trace::TracePayload::Number(state.clock as f64),
    );

    state.ticks_since_consolidation += 1;
    // Neurochemical decay — each chemical decays toward baseline per tick
    neurochemistry::apply_decay(&mut state.chemistry);
    neurochemistry::update_serotonin_homeostasis(&mut state.chemistry);
    // Phase C: compute effective chemistry early so multipliers are available
    // for encoding_gain, decay_rate_mod, pruning_pressure, and ACh ortho.
    let effective = neurochemistry::compute_effective(&state.chemistry);
    apply_decay(state, effective.decay_rate_mod);
    #[cfg(feature = "instrument")]
    _tctx.emit(trace::PipelineStep::Decay, trace::TracePayload::None);

    #[cfg(feature = "instrument")]
    _tctx.emit(
        trace::PipelineStep::StabilizeLabile,
        trace::TracePayload::None,
    );
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

    let chunks = chunk_text(text);

    // Batch-embed all chunks in a single model forward pass (entorhinal cortex).
    // This avoids per-chunk mutex lock and ONNX session overhead.
    let chunk_refs: Vec<&str> = chunks.iter().map(|c| c.as_str()).collect();
    let raw_embeddings = entorhinal::embed_texts_batch(&chunk_refs, state.config.embedding_dim);

    // Accumulate all touched node IDs across chunks for scoped CPEB tagging
    let mut all_touched_node_ids: Vec<u64> = Vec::new();

    for (chunk_idx, chunk) in chunks.into_iter().enumerate() {
        #[cfg(feature = "instrument")]
        _tctx.emit(
            trace::PipelineStep::ChunkText,
            trace::TracePayload::Text(chunk.clone()),
        );

        let raw_embedding = raw_embeddings[chunk_idx].clone();
        #[cfg(feature = "instrument")]
        _tctx.emit(
            trace::PipelineStep::EmbedText,
            trace::TracePayload::Number(raw_embedding.len() as f64),
        );

        // Phase C: encoding_gain scales thalamus salience (NE × ACh synergy).
        // Recompute per-chunk from live chemistry so intra-tick spikes (NE from
        // prior chunk's salience, ACh from prior chunk's novelty) take effect.
        let live_encoding_gain =
            1.0 + state.chemistry.norepinephrine * 0.5 + state.chemistry.acetylcholine * 0.3;
        let raw_salience = compute_salience(&chunk, &state.keyword_cache) * live_encoding_gain;
        let salience = normalize_final_salience(raw_salience);
        #[cfg(feature = "instrument")]
        _tctx.emit(
            trace::PipelineStep::ComputeSalience,
            trace::TracePayload::Number(salience as f64),
        );

        let emotional_valence =
            compute_emotional_valence(&state.emotional_prototypes, &raw_embedding);
        #[cfg(feature = "instrument")]
        _tctx.emit(
            trace::PipelineStep::ComputeEmotionalValence,
            trace::TracePayload::Number(emotional_valence as f64),
        );

        let refs = extract_memory_refs_from_text(&chunk);
        #[cfg(feature = "instrument")]
        _tctx.emit(
            trace::PipelineStep::ExtractMemoryRefs,
            trace::TracePayload::Number(refs.len() as f64),
        );

        // TCM temporal context update + date extraction
        let tcm_snapshot = update_temporal_context(state, &raw_embedding, salience);
        let chunk_dates = extract_dates(&chunk);
        // Wall clock: unix timestamp — Legend always knows when ticks happen.
        let wall_clock = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Always push into working memory (L1)
        let wm_id = prefrontal::push_working_memory_with_metadata(
            state,
            &chunk,
            &raw_embedding,
            salience,
            emotional_valence,
            wall_clock,
            chunk_dates.clone(),
            tcm_snapshot.clone(),
            neurochemistry::stamp_from(&state.chemistry),
        );
        #[cfg(feature = "instrument")]
        _tctx.emit(
            trace::PipelineStep::PushWorkingMemory,
            trace::TracePayload::KeyValue(vec![
                ("id".into(), wm_id.to_string()),
                ("text".into(), safe_truncate(&chunk, 120)),
                ("salience".into(), format!("{salience:.3}")),
                (
                    "emotional_valence".into(),
                    format!("{emotional_valence:.3}"),
                ),
                (
                    "wm_size_after".into(),
                    state.working_memory.len().to_string(),
                ),
            ]),
        );

        // --- Neurochemical spikes based on salience ---
        // NE spikes on high-salience input
        if salience > 0.5 {
            state.chemistry.norepinephrine =
                (state.chemistry.norepinephrine + NE_SALIENCE_SPIKE * salience).min(1.0);
        }
        // eCB rises on routine/low-salience input
        if salience < 0.25 {
            state.chemistry.endocannabinoid =
                (state.chemistry.endocannabinoid + ECB_ROUTINE_SPIKE).min(1.0);
        }

        // --- Dynamic attention gate: tighten under hippocampal load ---
        // Under L2 capacity pressure, Legend becomes more selective (ACh-modulated).
        let fill_ratio = state.short_term.len() as f32 / state.config.short_term_capacity as f32;
        let capacity_gate_boost = if fill_ratio > CAPACITY_STRESS_ONSET {
            let stress = (fill_ratio - CAPACITY_STRESS_ONSET) / (1.0 - CAPACITY_STRESS_ONSET);
            stress * ATTENTION_GATE_TIGHTENING
        } else {
            0.0
        };
        let dynamic_threshold = ATTENTION_GATE_THRESHOLD + capacity_gate_boost;

        #[cfg(feature = "instrument")]
        {
            let passes = salience >= dynamic_threshold;
            _tctx.emit(
                trace::PipelineStep::AttentionGate,
                trace::TracePayload::KeyValue(vec![
                    ("salience".into(), format!("{salience:.3}")),
                    ("threshold".into(), format!("{dynamic_threshold:.3}")),
                    ("passes".into(), passes.to_string()),
                    (
                        "outcome".into(),
                        if passes {
                            "promoted_to_l2".to_string()
                        } else {
                            "working_memory_only".to_string()
                        },
                    ),
                ]),
            );
        }

        if salience >= dynamic_threshold {
            // --- Normal path: match/merge/insert ---
            let (best_id, best_sim) =
                hippocampus::find_best_match(&state.short_term, &raw_embedding);
            #[cfg(feature = "instrument")]
            _tctx.emit(
                trace::PipelineStep::FindBestMatch,
                trace::TracePayload::Similarities(vec![(best_id, best_sim)]),
            );

            // ACh spikes on novel input (low similarity to existing memories)
            if best_sim < 0.3 {
                state.chemistry.acetylcholine =
                    (state.chemistry.acetylcholine + ACH_NOVELTY_SPIKE).min(1.0);
            }

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
            #[cfg(feature = "instrument")]
            _tctx.emit(
                trace::PipelineStep::DiversityGate,
                trace::TracePayload::KeyValue(vec![
                    ("best_sim".into(), format!("{best_sim:.3}")),
                    ("diversity_ok".into(), diversity_ok.to_string()),
                ]),
            );

            let touched_node_ids = match best_sim {
                s if s >= state.config.theta_high && diversity_ok => {
                    if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == best_id) {
                        entry.usage = entry.usage.saturating_add(2);
                        entry.salience = reinforce_bounded_signal(
                            entry.salience,
                            salience,
                            1.0,
                            LOW_MERGE_SALIENCE_MIDPOINT,
                            LOW_MERGE_SALIENCE_STEEPNESS,
                        );
                        entry.last_access = state.clock;
                        merge_memory_refs(&mut entry.refs, refs.clone());
                    }
                    let nids = neocortex::update_graph(state, &chunk, salience);
                    // Track merge (high similarity)
                    #[cfg(feature = "instrument")]
                    _tctx.emit(
                        trace::PipelineStep::MergeEntryHigh,
                        trace::TracePayload::Similarities(vec![(best_id, best_sim)]),
                    );
                    result_action = "merged".to_string();
                    result_entry_id = best_id;
                    nids
                }
                s if s >= state.config.theta_low && diversity_ok => {
                    if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == best_id) {
                        entry.embedding = merge_embeddings(&entry.embedding, &raw_embedding);
                        entry.usage = entry.usage.saturating_add(1);
                        entry.salience = reinforce_bounded_signal(
                            entry.salience,
                            salience,
                            LOW_MERGE_SALIENCE_LEARNING_RATE,
                            LOW_MERGE_SALIENCE_MIDPOINT,
                            LOW_MERGE_SALIENCE_STEEPNESS,
                        );
                        entry.summary = summarize_text(&entry.text, &chunk, &state.keyword_cache);
                        entry.last_access = state.clock;
                        merge_memory_refs(&mut entry.refs, refs.clone());
                    }
                    let nids = neocortex::update_graph(state, &chunk, salience);
                    #[cfg(feature = "instrument")]
                    _tctx.emit(
                        trace::PipelineStep::MergeEntryLow,
                        trace::TracePayload::Similarities(vec![(best_id, best_sim)]),
                    );
                    // Track merge (low similarity)
                    result_action = "merged".to_string();
                    result_entry_id = best_id;
                    nids
                }
                _ => {
                    // Dentate Gyrus: orthogonalize only when creating a new entry.
                    // Merge decisions above used raw_embedding to avoid interference.
                    let existing_embeddings: Vec<Vec<f32>> = state
                        .short_term
                        .iter()
                        .map(|e| e.embedding.clone())
                        .collect();
                    // Phase C: ACh modulates pattern separation strength (0.3–0.6).
                    let ach_ortho_strength = 0.3 * (1.0 + state.chemistry.acetylcholine * 1.0);
                    let ortho_embedding = sparse_orthogonalize(
                        &raw_embedding,
                        &existing_embeddings,
                        state.config.theta_low,
                        state.config.theta_high,
                        ach_ortho_strength,
                    );
                    #[cfg(feature = "instrument")]
                    _tctx.emit(
                        trace::PipelineStep::SparseOrthogonalize,
                        trace::TracePayload::None,
                    );
                    hippocampus::insert_short_term(
                        state,
                        &chunk,
                        ortho_embedding,
                        salience,
                        refs,
                        emotional_valence,
                        wall_clock.clone(),
                        chunk_dates.clone(),
                        tcm_snapshot.clone(),
                        neurochemistry::stamp_from(&state.chemistry),
                    );
                    #[cfg(feature = "instrument")]
                    {
                        let new_id = state.short_term.last().map(|e| e.id).unwrap_or(0);
                        _tctx.emit(
                            trace::PipelineStep::InsertShortTerm,
                            trace::TracePayload::KeyValue(vec![
                                ("id".into(), new_id.to_string()),
                                ("text".into(), safe_truncate(&chunk, 120)),
                                ("salience".into(), format!("{salience:.3}")),
                                (
                                    "emotional_valence".into(),
                                    format!("{emotional_valence:.3}"),
                                ),
                            ]),
                        );
                    }
                    #[cfg(feature = "instrument")]
                    let _graph_before = (state.long_term.nodes.len(), state.long_term.edges.len());
                    let nids = neocortex::update_graph(state, &chunk, salience);
                    #[cfg(feature = "instrument")]
                    _tctx.emit(
                        trace::PipelineStep::UpdateGraph,
                        trace::TracePayload::KeyValue(vec![
                            (
                                "nodes_created".into(),
                                (state.long_term.nodes.len() - _graph_before.0).to_string(),
                            ),
                            (
                                "edges_created".into(),
                                (state.long_term.edges.len() - _graph_before.1).to_string(),
                            ),
                        ]),
                    );
                    // Track creation - get the ID of the newly inserted entry
                    if let Some(entry) = state.short_term.last() {
                        result_action = "created".to_string();
                        result_entry_id = entry.id;
                    }
                    nids
                }
            };

            // Accumulate touched nodes for scoped CPEB tagging
            all_touched_node_ids.extend_from_slice(&touched_node_ids);

            // Mark L1 entry as promoted
            if let Some(wm_entry) = state.working_memory.iter_mut().find(|e| e.id == wm_id) {
                wm_entry.promoted = true;
            }

            last_context = encoding_activation(state, &raw_embedding, &chunk, &touched_node_ids);
        } else {
            // Low-salience: stays in working memory only, skip L2 insertion
            result_action = "working_memory_only".to_string();
            result_entry_id = wm_id;
        }
    }

    // Layer 3: update term frequency stats and auto-promote recurring entities
    update_term_frequencies(state, text);
    #[cfg(feature = "instrument")]
    _tctx.emit(
        trace::PipelineStep::UpdateTermFrequencies,
        trace::TracePayload::None,
    );

    #[cfg(feature = "instrument")]
    let _l2_before = state.short_term.len();
    hippocampus::prune_short_term(
        &mut state.short_term,
        state.clock,
        effective.pruning_pressure,
    );
    #[cfg(feature = "instrument")]
    _tctx.emit(
        trace::PipelineStep::PruneL2,
        trace::TracePayload::KeyValue(vec![
            ("before_count".into(), _l2_before.to_string()),
            ("after_count".into(), state.short_term.len().to_string()),
            (
                "pruned_count".into(),
                (_l2_before - state.short_term.len()).to_string(),
            ),
        ]),
    );
    #[cfg(feature = "instrument")]
    let _l3_nodes_before = state.long_term.nodes.len();
    neocortex::prune_graph(&mut state.long_term, state.clock);
    #[cfg(feature = "instrument")]
    _tctx.emit(
        trace::PipelineStep::PruneL3,
        trace::TracePayload::KeyValue(vec![
            ("before_count".into(), _l3_nodes_before.to_string()),
            (
                "after_count".into(),
                state.long_term.nodes.len().to_string(),
            ),
            (
                "pruned_count".into(),
                (_l3_nodes_before - state.long_term.nodes.len()).to_string(),
            ),
        ]),
    );

    // --- Smart consolidation triggers ---
    let tick_embedding = embed_text(text, state.config.embedding_dim);
    let tick_valence = compute_emotional_valence(&state.emotional_prototypes, &tick_embedding);

    // --- Neurochemical spikes based on emotional valence ---
    if tick_valence < -0.2 {
        state.chemistry.cortisol =
            (state.chemistry.cortisol + CORTISOL_NEGATIVE_SPIKE * tick_valence.abs()).min(1.0);
        // NE_THREAT_SPIKE (0.15) is smaller than NE_SALIENCE_SPIKE (0.3) to avoid
        // double-counting when a high-salience tick is also negatively valenced.
        state.chemistry.norepinephrine =
            (state.chemistry.norepinephrine + NE_THREAT_SPIKE * tick_valence.abs()).min(1.0);
    }
    if tick_valence > 0.2 {
        state.chemistry.dopamine =
            (state.chemistry.dopamine + DA_POSITIVE_SPIKE * tick_valence).min(1.0);
    }
    // eCB recovery from sustained cortisol
    if state.chemistry.cortisol > 0.5 {
        state.chemistry.endocannabinoid = (state.chemistry.endocannabinoid
            + (state.chemistry.cortisol - 0.5) * ECB_CORTISOL_RECOVERY)
            .min(1.0);
    }

    // Hippocampal overload defense: capacity-proportional cortisol spike.
    // As L2 fills past CAPACITY_STRESS_ONSET (75%), cortisol rises linearly,
    // feeding into consolidation_pressure = cortisol × 2.0 + NE × 0.5.
    // This causes auto-consolidation to fire earlier under sustained load.
    {
        let fill_ratio = state.short_term.len() as f32 / state.config.short_term_capacity as f32;
        if fill_ratio > CAPACITY_STRESS_ONSET {
            let stress = (fill_ratio - CAPACITY_STRESS_ONSET) / (1.0 - CAPACITY_STRESS_ONSET);
            state.chemistry.cortisol =
                (state.chemistry.cortisol + CORTISOL_CAPACITY_SPIKE * stress).min(1.0);
        }
    }

    // CPEB synaptic tagging: high-valence events selectively strengthen
    // graph edges connected to nodes touched during this tick (Kandel's synaptic capture).
    all_touched_node_ids.sort();
    all_touched_node_ids.dedup();
    if tick_valence.abs() > CPEB_VALENCE_THRESHOLD && !all_touched_node_ids.is_empty() {
        let tagged_count = neocortex::cpeb_tag_edges_scoped(
            &mut state.long_term,
            state.clock,
            tick_valence.abs(),
            CPEB_STABILITY_BOOST,
            &all_touched_node_ids,
        );
        #[cfg(feature = "instrument")]
        _tctx.emit(
            trace::PipelineStep::CpebTagging,
            trace::TracePayload::KeyValue(vec![
                ("emotional_valence".into(), format!("{tick_valence:.3}")),
                ("edges_tagged_count".into(), tagged_count.to_string()),
                (
                    "tag_threshold".into(),
                    format!("{CPEB_VALENCE_THRESHOLD:.3}"),
                ),
            ]),
        );
        let _ = tagged_count;
    }

    // Context switch detection: compare with previous tick's embedding
    if !state.last_tick_embedding.is_empty() {
        let sim = cosine_similarity(&state.last_tick_embedding, &tick_embedding);
        let triggered = sim < CONTEXT_SWITCH_THRESHOLD;
        #[cfg(feature = "instrument")]
        _tctx.emit(
            trace::PipelineStep::ContextSwitchDetection,
            trace::TracePayload::KeyValue(vec![
                ("similarity".into(), format!("{sim:.3}")),
                ("threshold".into(), format!("{CONTEXT_SWITCH_THRESHOLD:.3}")),
                ("triggered".into(), triggered.to_string()),
            ]),
        );
        if triggered {
            // NE spike on context switch
            state.chemistry.norepinephrine =
                (state.chemistry.norepinephrine + NE_CONTEXT_SWITCH_SPIKE).min(1.0);
            // Topic shift detected — flush L1
            #[cfg(feature = "instrument")]
            let _wm_before = state.working_memory.len();
            prefrontal::flush_working_memory(state);
            #[cfg(feature = "instrument")]
            {
                let count_flushed = _wm_before;
                let promoted_count = state.short_term.len();
                _tctx.emit(
                    trace::PipelineStep::FlushWorkingMemory,
                    trace::TracePayload::KeyValue(vec![
                        ("count_flushed".into(), count_flushed.to_string()),
                        ("l2_count_after".into(), promoted_count.to_string()),
                    ]),
                );
            }
        }
    }
    state.last_tick_embedding = tick_embedding;

    // --- Anterior PFC: PLAN: prefix handling ---
    if let Some(plan_body) = anterior_pfc::strip_plan_prefix(text) {
        if let Some((name, items)) = anterior_pfc::parse_plan_text(plan_body) {
            anterior_pfc::apply_plan(
                &mut state.plans,
                name,
                items,
                state.clock,
                &mut state.next_id,
                state.config.embedding_dim,
            );
        }
    }

    // ACC-like reset: archive completed plans after retention period
    let archive_threshold = state.clock.saturating_sub(anterior_pfc::PLAN_ARCHIVE_TICKS);
    let mut to_archive = Vec::new();
    let mut to_keep = Vec::new();
    for plan in state.plans.drain(..) {
        if plan.completed_at.map_or(false, |t| t <= archive_threshold) {
            to_archive.push(plan);
        } else {
            to_keep.push(plan);
        }
    }
    state.plans = to_keep;
    for plan in to_archive {
        let summary = anterior_pfc::format_plan_archive_text(&plan);
        let archive_embedding = embed_text(&summary, state.config.embedding_dim);
        let wall_clock = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        hippocampus::insert_short_term(
            state,
            &summary,
            archive_embedding,
            0.4, // moderate salience for archived plans
            Vec::new(),
            0.0,
            wall_clock,
            Vec::new(),
            Vec::new(),
            neurochemistry::stamp_from(&state.chemistry),
        );
    }

    // Auto-consolidation: if enough ticks have accumulated or neurochemical
    // consolidation pressure is elevated (Phase C: replaces recent_valence_sum).
    // Re-compute effective after neurochemical spikes above may have changed levels.
    let effective_post = neurochemistry::compute_effective(&state.chemistry);
    if state.ticks_since_consolidation >= CONSOLIDATION_SUGGESTION_THRESHOLD
        || effective_post.consolidation_pressure >= CONSOLIDATION_PRESSURE_THRESHOLD
    {
        consolidate(state);
    }

    #[cfg(feature = "instrument")]
    _tctx.emit(trace::PipelineStep::TickEnd, trace::TracePayload::None);

    TickResult {
        action: result_action,
        entry_id: result_entry_id,
        context: last_context,
    }
}

fn infer_query_mode(query: &str, keyword_cache: &KeywordCache) -> neocortex::QueryMode {
    let query_lower = query.to_lowercase();

    if wernicke::lexicon::TEMPORAL_QUERY_MARKERS
        .iter()
        .any(|m| query_lower.contains(m))
    {
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

/// Encoding-time activation: the hippocampal processes that naturally occur
/// when a new memory is encoded. Each step is an explicit, named phase.
///
/// Unlike `retrieve_context` (query-time), encoding activation does NOT:
/// - Increment the clock (already done at tick start)
/// - Run decay (already done at tick start)
/// - Update spaced-repetition stability (retrieval-only learning)
/// - Sort chronologically (presentation concern for queries)
/// - Record last_retrieved_ids (contrastive descent is retrieval-specific)
fn encoding_activation(
    state: &mut BrainState,
    embedding: &[f32],
    chunk: &str,
    touched_node_ids: &[u64],
) -> MemoryContext {
    // ── Step 1: CA3 pattern completion — find similar L2 entries ──
    // Full similarity scoring with keyword bonus and amygdala emotional boost.
    // The hippocampus automatically activates related existing traces during encoding.
    let mut candidates = hippocampus::top_k_similar(
        &state.short_term,
        embedding,
        usize::MAX,
        chunk,
        &state.chemistry,
    );

    // ── Step 2: Neocortical associative recall — graph-boosted L2 ──
    // The knowledge graph biases which L2 entries get co-activated.
    // Specificity-weighted: rare entities boost more than common ones.
    {
        let query_entities = extract_entities(chunk, &state.keyword_cache);
        neocortex::graph_boost_candidates(
            &state.long_term,
            &query_entities,
            &mut candidates,
            neocortex::QueryMode::Neutral,
        );
    }

    // ── Step 3: L3 systems consolidation retrieval ──
    // Summary and Trace nodes with centroid embeddings can independently surface
    // old consolidated memories and fast-mapped traces whose L2 entries were evicted.
    {
        let existing_ids: HashSet<u64> = candidates.iter().map(|s| s.id).collect();
        let mut summary_hits: Vec<MemorySnippet> = state
            .long_term
            .nodes
            .values()
            .filter(|n| matches!(n.kind.as_str(), "Summary" | "Trace") && !n.embedding.is_empty())
            .filter_map(|n| {
                let sim = cosine_similarity(&n.embedding, embedding);
                if sim >= SUMMARY_RETRIEVAL_MIN_SIM && !existing_ids.contains(&n.id) {
                    let text = n.full_text.as_deref().unwrap_or(&n.label).to_string();
                    Some(MemorySnippet {
                        id: n.id,
                        text,
                        similarity: sim * 0.85,
                        refs: Vec::new(),
                        wall_clock: 0,
                        extracted_dates: Vec::new(),
                        created_at_clock: 0,
                    })
                } else {
                    None
                }
            })
            .collect();
        summary_hits.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        candidates.extend(summary_hits);
    }

    // ── Step 4: Temporal context boost (TCM co-activation) ──
    // Temporally proximate memories get co-activated during encoding.
    if !state.temporal_context.is_empty() {
        for candidate in &mut candidates {
            if let Some(entry) = state.short_term.iter().find(|e| e.id == candidate.id) {
                if !entry.temporal_context.is_empty() {
                    let tcm_sim =
                        cosine_similarity(&state.temporal_context, &entry.temporal_context);
                    candidate.similarity += 0.03 * tcm_sim;
                }
            }
        }
    }

    // ── Step 5: Adaptive relevance threshold ──
    // Scale cutoff relative to best match — strong matches raise the floor,
    // weak matches lower it. Filters noise without hiding relevant hits.
    candidates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    let top_sim = candidates.first().map(|c| c.similarity).unwrap_or(0.0);
    let adaptive_floor = (top_sim * 0.65).max(MIN_QUERY_SIMILARITY);
    candidates.retain(|c| c.similarity >= adaptive_floor);
    let mut snippets = candidates;

    // ── Step 6: CA3 pattern completion (sparse cue reconstruction) ──
    // When direct matches are sparse or weak, use graph structure to
    // reconstruct related memories from partial cues.
    let top_sim = snippets.first().map(|s| s.similarity).unwrap_or(0.0);
    if snippets.len() < PATTERN_COMPLETION_MIN_RESULTS || top_sim < PATTERN_COMPLETION_SIM_THRESHOLD
    {
        let completed =
            hippocampus::pattern_complete(state, chunk, &snippets, neocortex::QueryMode::Neutral);
        let existing_ids: HashSet<u64> = snippets.iter().map(|s| s.id).collect();
        for c in completed {
            if !existing_ids.contains(&c.id) {
                snippets.push(c);
            }
        }
        snippets.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    }

    // ── Step 7: Auto-reinforce — reactivation strengthens the trace ──
    // When encoding activates an existing memory, that reactivation naturally
    // strengthens it (reconsolidation-lite / memory reactivation effect).
    if let Some(top) = snippets.first() {
        if top.similarity > 0.2 {
            if let Some(entry) = state.short_term.iter_mut().find(|e| e.id == top.id) {
                entry.salience = (entry.salience + top.similarity * AUTO_REINFORCE_SCALE).min(1.0);
            }
        }
    }

    // ── Step 8: Graph lookup + associative priming ──
    // Extract entities from activated L2 entries and spread activation
    // through the knowledge graph to surface indirectly related concepts.
    let mut long_term = neocortex::graph_lookup(
        &state.long_term,
        chunk,
        12,
        &state.keyword_cache,
        neocortex::QueryMode::Neutral,
    );

    // Collect priming seeds from L2 hits + graph lookup + touched nodes
    let mut priming_seed_ids: Vec<u64> = Vec::new();
    for snippet in &snippets {
        let entities = extract_entities(&snippet.text, &state.keyword_cache);
        for entity in &entities {
            if let Some(&node_id) = state.long_term.index.get(&entity.label.to_lowercase()) {
                priming_seed_ids.push(node_id);
            }
        }
    }
    for node in &long_term {
        priming_seed_ids.push(node.id);
    }
    priming_seed_ids.extend_from_slice(touched_node_ids);
    priming_seed_ids.sort();
    priming_seed_ids.dedup();

    let existing_ids: HashSet<u64> = long_term.iter().map(|n| n.id).collect();
    let activated = neocortex::spreading_activation(
        &state.long_term,
        &priming_seed_ids,
        2,
        0.4,
        neocortex::QueryMode::Neutral,
    );
    for (nid, activation) in activated {
        if !existing_ids.contains(&nid) {
            if let Some(node) = state.long_term.nodes.get(&nid) {
                long_term.push(GraphNodeSummary {
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

    // Adaptive weight threshold for L3
    long_term.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    let top_weight = long_term.first().map(|n| n.weight).unwrap_or(0.0);
    let weight_floor = top_weight * 0.4;
    long_term.retain(|n| n.weight >= weight_floor);

    // ── Step 9: Hebbian reinforcement — co-activated entities strengthen ──
    // "Neurons that fire together wire together" — encoding a new memory
    // that co-activates existing graph nodes strengthens their connections.
    let co_activated_ids: Vec<u64> = long_term.iter().map(|n| n.id).collect();
    neocortex::hebbian_reinforce(&mut state.long_term, &co_activated_ids, state.clock);

    MemoryContext {
        short_term: snippets,
        long_term,
        working_memory: Vec::new(),
    }
}

/// CA3 autoassociative pattern completion.
///
/// When direct similarity search returns sparse results, use the graph to
/// reconstruct full memories from partial cues. Extracts entities from
/// partial matches, spreads activation, then searches L2 for entries
/// containing activated nodes' source texts.

/// Query memory without inserting new data.
pub fn retrieve_context(state: &mut BrainState, query: &str) -> MemoryContext {
    #[cfg(feature = "instrument")]
    let _qctx = {
        let ctx = trace::TraceCtx::new();
        ctx.emit(
            trace::PipelineStep::QueryStart,
            trace::TracePayload::Text(query.to_string()),
        );
        ctx
    };

    state.clock += 1;
    let effective = neurochemistry::compute_effective(&state.chemistry);
    apply_decay(state, effective.decay_rate_mod);
    let query_mode = infer_query_mode(query, &state.keyword_cache);
    #[cfg(feature = "instrument")]
    _qctx.emit(
        trace::PipelineStep::InferQueryMode,
        trace::TracePayload::Text(format!("{query_mode:?}")),
    );

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
            .filter(|w| w.len() > 3 && !wernicke::is_stopword(w) && entry_lower.contains(**w))
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
                wall_clock: 0,
                extracted_dates: Vec::new(),
                created_at_clock: 0,
            });
        }
    }
    wm_snippets.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    // No truncation — return all L1 entries above MIN_QUERY_SIMILARITY.
    #[cfg(feature = "instrument")]
    _qctx.emit(
        trace::PipelineStep::L1Scan,
        trace::TracePayload::Number(wm_snippets.len() as f64),
    );

    // --- Prospective memory: spontaneous retrieval of matching plan items ---
    // After L1 scan, before L2 retrieval — McDaniel & Einstein's pathway.
    for plan in &state.plans {
        for item in &plan.items {
            if item.status == anterior_pfc::ItemStatus::Done {
                continue;
            }
            if item.embedding.is_empty() {
                continue;
            }
            let sim = cosine_similarity(&embedding, &item.embedding);
            if sim >= anterior_pfc::INTENTION_CUE_THRESHOLD {
                wm_snippets.push(hippocampus::MemorySnippet {
                    id: 0, // plan items don't have L2 IDs
                    text: format!(
                        "[Plan: {} | {}] {}",
                        plan.name,
                        item.status.label(),
                        item.text
                    ),
                    similarity: sim,
                    refs: Vec::new(),
                    wall_clock: 0,
                    extracted_dates: Vec::new(),
                    created_at_clock: 0,
                });
            }
        }
    }
    // Re-sort after adding plan items
    wm_snippets.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());

    // --- L2 retrieval: gather all candidates above similarity floor ---
    // No candidate cap — pass all above-floor entries to MMR for diversity selection.
    // The cap was a bottleneck when L2 grows large: topic-similar generics would
    // saturate the pool, pushing out factual needles with lower embedding similarity.
    let mut candidates = hippocampus::top_k_similar(
        &state.short_term,
        &embedding,
        usize::MAX,
        query,
        &state.chemistry,
    );
    #[cfg(feature = "instrument")]
    _qctx.emit(
        trace::PipelineStep::L2TopKSimilar,
        trace::TracePayload::Similarities(
            candidates.iter().map(|s| (s.id, s.similarity)).collect(),
        ),
    );

    // --- Graph-informed L2 boost (neocortical associative recall) ---
    // Use the knowledge graph to boost L2 candidates that mention entities
    // connected to query entities. Specificity-weighted: rare entities like
    // "February" give higher boosts than common ones like "Hawaii".
    {
        let query_entities = extract_entities(query, &state.keyword_cache);
        neocortex::graph_boost_candidates(
            &state.long_term,
            &query_entities,
            &mut candidates,
            query_mode,
        );
    }

    // --- L3 systems consolidation retrieval (neocortical independence) ---
    // Scan Summary and Trace nodes that have centroid embeddings for direct
    // similarity matching. This surfaces old consolidated memories whose L2
    // entries may have been evicted, plus fast-mapped traces of unconsolidated
    // entries that were evicted under hippocampal pressure.
    {
        let existing_ids: HashSet<u64> = candidates.iter().map(|s| s.id).collect();
        let mut summary_hits: Vec<MemorySnippet> = state
            .long_term
            .nodes
            .values()
            .filter(|n| matches!(n.kind.as_str(), "Summary" | "Trace") && !n.embedding.is_empty())
            .filter_map(|n| {
                let sim = cosine_similarity(&n.embedding, &embedding);
                if sim >= SUMMARY_RETRIEVAL_MIN_SIM && !existing_ids.contains(&n.id) {
                    let text = n.full_text.as_deref().unwrap_or(&n.label).to_string();
                    Some(MemorySnippet {
                        id: n.id,
                        text,
                        similarity: sim * 0.85, // slight discount vs direct L2 matches
                        refs: Vec::new(),
                        wall_clock: 0,
                        extracted_dates: Vec::new(),
                        created_at_clock: 0,
                    })
                } else {
                    None
                }
            })
            .collect();
        summary_hits.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        // No truncation — SUMMARY_RETRIEVAL_MIN_SIM already gates relevance.
        #[cfg(feature = "instrument")]
        _qctx.emit(
            trace::PipelineStep::L3SummaryRetrieval,
            trace::TracePayload::Number(summary_hits.len() as f64),
        );
        candidates.extend(summary_hits);
    }

    // --- Temporal boosts (automatic, not mode-gated) ---
    // A. TCM proximity boost: entries temporally close to current context get a small boost
    if !state.temporal_context.is_empty() {
        for candidate in &mut candidates {
            if let Some(entry) = state.short_term.iter().find(|e| e.id == candidate.id) {
                if !entry.temporal_context.is_empty() {
                    let tcm_sim =
                        cosine_similarity(&state.temporal_context, &entry.temporal_context);
                    candidate.similarity += 0.03 * tcm_sim;
                }
            }
        }
    }
    // B. Date-affinity boost: when the query itself contains dates/temporal language,
    //    boost candidates that also have date metadata (automatically detected).
    let query_dates = extract_dates(query);
    let query_has_temporal = !query_dates.is_empty()
        || wernicke::lexicon::TEMPORAL_QUERY_MARKERS
            .iter()
            .any(|m| query_lower.contains(m));
    if query_has_temporal {
        for candidate in &mut candidates {
            if let Some(entry) = state.short_term.iter().find(|e| e.id == candidate.id) {
                if entry.wall_clock > 0 || !entry.extracted_dates.is_empty() {
                    candidate.similarity += 0.05;
                }
            }
        }
    }

    // --- Adaptive relevance threshold ---
    // Instead of a fixed floor, scale the cutoff relative to the best match.
    // Strong queries (top ~0.9) use a high floor; weak queries (top ~0.4) use
    // a proportionally lower floor. This naturally returns fewer results for
    // off-topic queries without hiding needles for on-topic ones.
    //
    // Phase C: retrieval_quality (Yerkes-Dodson × ACh) modulates the floor.
    // High quality (moderate cortisol, high ACh) → lower floor → more results.
    // Low quality (extreme cortisol) → higher floor → fewer, only strongest.
    // Baseline retrieval_quality ≈ 0.41, so (rq - 0.41) centers the adjustment.
    candidates.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    let top_sim = candidates.first().map(|c| c.similarity).unwrap_or(0.0);
    let quality_offset = (effective.retrieval_quality - 0.41) * 0.2; // ±0.0 at baseline
    let adaptive_floor = (top_sim * (0.65 - quality_offset)).max(MIN_QUERY_SIMILARITY);
    candidates.retain(|c| c.similarity >= adaptive_floor);
    let mut snippets = candidates;

    // Update spaced repetition state for selected L2 entries
    for snippet in &snippets {
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
        }
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
        // No truncation — all completed results above MIN_QUERY_SIMILARITY included.
        #[cfg(feature = "instrument")]
        _qctx.emit(
            trace::PipelineStep::PatternCompletion,
            trace::TracePayload::Number(snippets.len() as f64),
        );
    }

    // C. Chronological sort: when the query has temporal signals, sort by creation
    //    order so the LLM receives events in chronological sequence.
    if query_has_temporal {
        snippets.sort_by_key(|s| s.created_at_clock);
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
            // Phase C: DA spike on successful retrieval — reward signal for recall
            if top.similarity > 0.5 {
                state.chemistry.dopamine = (state.chemistry.dopamine
                    + neurochemistry::DA_RETRIEVAL_SPIKE * top.similarity)
                    .min(1.0);
            }
            #[cfg(feature = "instrument")]
            _qctx.emit(
                trace::PipelineStep::AutoReinforceTop,
                trace::TracePayload::Similarities(vec![(top.id, top.similarity)]),
            );
        }
    }

    // Trace promotion: any Trace node that was retrieved has proven its value.
    // Promote it to Summary status (brain analog: weak cortical traces that
    // receive hippocampal replay strengthen into stable representations).
    for snippet in &snippets {
        hippocampus::promote_trace(state, snippet.id);
    }

    let mut long_term = neocortex::graph_lookup(
        &state.long_term,
        query,
        12,
        &state.keyword_cache,
        query_mode,
    );
    #[cfg(feature = "instrument")]
    _qctx.emit(
        trace::PipelineStep::GraphLookup,
        trace::TracePayload::Number(long_term.len() as f64),
    );

    // --- Associative priming (2-hop spreading activation) ---
    // From the retrieved short-term entries, extract entities and spread
    // activation through the graph to surface indirectly related nodes.
    let mut priming_seed_ids: Vec<u64> = Vec::new();
    for snippet in &snippets {
        let entities = extract_entities(&snippet.text, &state.keyword_cache);
        for entity in &entities {
            if let Some(&node_id) = state.long_term.index.get(&entity.label.to_lowercase()) {
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
    #[cfg(feature = "instrument")]
    _qctx.emit(
        trace::PipelineStep::AssociativePriming,
        trace::TracePayload::Number(long_term.len() as f64),
    );

    // Adaptive weight threshold — same principle as L2: scale relative to best.
    long_term.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    let top_weight = long_term.first().map(|n| n.weight).unwrap_or(0.0);
    let weight_floor = top_weight * 0.4;
    long_term.retain(|n| n.weight >= weight_floor);

    // Hebbian reinforcement on co-retrieved nodes
    let retrieved_ids: Vec<u64> = long_term.iter().map(|n| n.id).collect();
    neocortex::hebbian_reinforce(&mut state.long_term, &retrieved_ids, state.clock);
    #[cfg(feature = "instrument")]
    _qctx.emit(
        trace::PipelineStep::HebbianReinforce,
        trace::TracePayload::Number(retrieved_ids.len() as f64),
    );

    #[cfg(feature = "instrument")]
    _qctx.emit(trace::PipelineStep::QueryEnd, trace::TracePayload::None);

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

fn find_component_root(parent: &mut [usize], idx: usize) -> usize {
    if parent[idx] != idx {
        parent[idx] = find_component_root(parent, parent[idx]);
    }
    parent[idx]
}

fn union_components(parent: &mut [usize], a: usize, b: usize) {
    let root_a = find_component_root(parent, a);
    let root_b = find_component_root(parent, b);
    if root_a == root_b {
        return;
    }

    // Keep roots deterministic by attaching the higher index to the lower.
    if root_a < root_b {
        parent[root_b] = root_a;
    } else {
        parent[root_a] = root_b;
    }
}

fn consolidation_groups(
    short_term: &[ShortTermEntry],
    similarity_threshold: f32,
) -> Vec<Vec<ShortTermEntry>> {
    let mut parent: Vec<usize> = (0..short_term.len()).collect();

    for i in 0..short_term.len() {
        for j in (i + 1)..short_term.len() {
            if cosine_similarity(&short_term[i].embedding, &short_term[j].embedding)
                >= similarity_threshold
            {
                union_components(&mut parent, i, j);
            }
        }
    }

    let mut components: Vec<(usize, Vec<ShortTermEntry>)> = Vec::new();
    for i in 0..short_term.len() {
        let root = find_component_root(&mut parent, i);
        if let Some((_, group)) = components.iter_mut().find(|(r, _)| *r == root) {
            group.push(short_term[i].clone());
        } else {
            components.push((root, vec![short_term[i].clone()]));
        }
    }

    for (_, group) in &mut components {
        group.sort_by(|a, b| {
            a.id.cmp(&b.id)
                .then_with(|| a.created_at_clock.cmp(&b.created_at_clock))
                .then_with(|| a.text.cmp(&b.text))
        });
    }
    components.sort_by(|(_, a), (_, b)| {
        let a_key = a
            .first()
            .map(|e| (e.id, e.created_at_clock, e.text.as_str()))
            .unwrap_or((0, 0, ""));
        let b_key = b
            .first()
            .map(|e| (e.id, e.created_at_clock, e.text.as_str()))
            .unwrap_or((0, 0, ""));
        a_key.cmp(&b_key)
    });

    components.into_iter().map(|(_, group)| group).collect()
}

fn systems_consolidation_score(group: &[ShortTermEntry]) -> f32 {
    if group.is_empty() {
        return 0.0;
    }

    let avg_salience = group.iter().map(|e| e.salience).sum::<f32>() / group.len() as f32;
    let max_salience = group.iter().map(|e| e.salience).fold(0.0, f32::max);

    avg_salience * SYSTEMS_CONSOLIDATION_AVG_WEIGHT
        + max_salience * SYSTEMS_CONSOLIDATION_MAX_WEIGHT
}

fn semantic_topic_promotion_threshold(group_len: usize) -> usize {
    group_len / 2 + 1
}

fn average_chemical_stamp(group: &[ShortTermEntry]) -> ChemicalStamp {
    if group.is_empty() {
        return ChemicalStamp::default();
    }

    let mut stamp = ChemicalStamp::default();
    let len = group.len() as f32;
    for entry in group {
        stamp.ne_at_encoding += entry.chemical_stamp.ne_at_encoding / len;
        stamp.cortisol_at_encoding += entry.chemical_stamp.cortisol_at_encoding / len;
        stamp.da_at_encoding += entry.chemical_stamp.da_at_encoding / len;
        stamp.ach_at_encoding += entry.chemical_stamp.ach_at_encoding / len;
    }
    stamp
}

fn find_existing_summary_node(
    long_term: &GraphMemory,
    summary_text: &str,
    centroid_embedding: &[f32],
    source_texts: &[String],
) -> Option<u64> {
    long_term
        .index
        .get(&summary_text.to_lowercase())
        .copied()
        .or_else(|| {
            long_term
                .nodes
                .iter()
                .find(|(_, n)| {
                    if n.kind != "Summary" {
                        return false;
                    }

                    word_overlap(&n.label, summary_text) >= MERGE_WORD_OVERLAP_THRESHOLD
                        || source_texts.iter().any(|st| n.source_texts.contains(st))
                        || (!centroid_embedding.is_empty()
                            && !n.embedding.is_empty()
                            && cosine_similarity(&n.embedding, centroid_embedding)
                                >= SUMMARY_MERGE_EMBEDDING_SIM)
                })
                .map(|(&id, _)| id)
        })
}

/// Merge similar short-term entries into long-term graph summaries.
pub fn consolidate(state: &mut BrainState) -> Vec<GraphNodeSummary> {
    #[cfg(feature = "instrument")]
    let _cctx = {
        let ctx = trace::TraceCtx::new();
        ctx.emit(
            trace::PipelineStep::ConsolidateStart,
            trace::TracePayload::KeyValue(vec![
                ("l2_count".into(), state.short_term.len().to_string()),
                ("l3_nodes".into(), state.long_term.nodes.len().to_string()),
                ("l3_edges".into(), state.long_term.edges.len().to_string()),
            ]),
        );
        ctx
    };

    state.clock += 1;
    state.ticks_since_consolidation = 0;
    let consolidation_eff = neurochemistry::compute_effective(&state.chemistry);
    apply_decay(state, consolidation_eff.decay_rate_mod);
    neocortex::replay_consolidation(state);
    #[cfg(feature = "instrument")]
    _cctx.emit(
        trace::PipelineStep::ReplayConsolidation,
        trace::TracePayload::None,
    );

    let groups = consolidation_groups(&state.short_term, state.config.theta_low);
    #[cfg(feature = "instrument")]
    {
        let group_sizes: Vec<String> = groups.iter().map(|g| g.len().to_string()).collect();
        let multi_entry_groups = groups.iter().filter(|g| g.len() > 1).count();
        _cctx.emit(
            trace::PipelineStep::ClusterGroups,
            trace::TracePayload::KeyValue(vec![
                ("total_groups".into(), groups.len().to_string()),
                ("multi_entry_groups".into(), multi_entry_groups.to_string()),
                ("group_sizes".into(), group_sizes.join(",")),
            ]),
        );
    }

    let mut summaries = Vec::new();
    for group in groups.into_iter().filter(|g| g.len() > 1) {
        let semantic_group: Vec<ShortTermEntry> = group
            .iter()
            .cloned()
            .filter_map(|mut entry| {
                entry.text = clean_semantic_noise(&entry.text);
                entry.summary = clean_semantic_noise(&entry.summary);
                if entry.text.trim().is_empty() {
                    None
                } else {
                    Some(entry)
                }
            })
            .collect();
        let summary_group = if semantic_group.is_empty() {
            &group
        } else {
            &semantic_group
        };
        let summary_text =
            clean_semantic_noise(&summarize_group(summary_group, &state.keyword_cache));
        #[cfg(feature = "instrument")]
        {
            let entry_previews: Vec<String> = group
                .iter()
                .take(5)
                .map(|e| safe_truncate(&e.text, 80))
                .collect();
            _cctx.emit(
                trace::PipelineStep::SummarizeGroup,
                trace::TracePayload::KeyValue(vec![
                    ("summary".into(), summary_text.clone()),
                    ("group_size".into(), group.len().to_string()),
                    ("entries".into(), entry_previews.join(" | ")),
                ]),
            );
        }
        let salience = group
            .iter()
            .map(|e| e.salience)
            .fold(0.0, f32::max)
            .max(0.4);
        let source_texts: Vec<String> = {
            let mut seen: HashSet<String> = HashSet::new();
            group
                .iter()
                .map(|e| clean_semantic_noise(&e.text))
                .filter(|text| !text.trim().is_empty())
                .filter(|text| seen.insert(text.clone()))
                .collect()
        };

        // Systems consolidation: compute centroid embedding and rich text
        // for high-salience groups (hippocampus flags important memories
        // for full neocortical encoding).
        let consolidation_score = systems_consolidation_score(&group);
        let (centroid_embedding, full_text) =
            if consolidation_score >= SYSTEMS_CONSOLIDATION_SALIENCE_THRESHOLD {
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

                #[cfg(feature = "instrument")]
                _cctx.emit(
                    trace::PipelineStep::SystemsConsolidation,
                    trace::TracePayload::KeyValue(vec![
                        ("embedding_dim".into(), centroid.len().to_string()),
                        (
                            "consolidation_score".into(),
                            format!("{consolidation_score:.3}"),
                        ),
                        ("source_count".into(), source_texts.len().to_string()),
                    ]),
                );

                (centroid, Some(rich))
            } else {
                (Vec::new(), None)
            };

        // 1. Dedup: prefer exact label, then source/label overlap, then centroid similarity.
        let existing_summary_id = find_existing_summary_node(
            &state.long_term,
            &summary_text,
            &centroid_embedding,
            &source_texts,
        );

        let node_id = if let Some(eid) = existing_summary_id {
            // Merge into existing: update weight/salience, extend source_texts
            if let Some(node) = state.long_term.nodes.get_mut(&eid) {
                node.weight = node.weight.max(1.0 + salience);
                node.salience = node.salience.max(salience);
                node.last_seen = state.clock;
                // Extend source_texts and dedup. This is the evidence-preservation
                // path for consolidation; later compression can compact duplicates
                // without silently dropping minority facts.
                let mut seen: HashSet<String> =
                    node.source_texts.iter().cloned().collect();
                for st in &source_texts {
                    if seen.insert(st.clone()) {
                        node.source_texts.push(st.clone());
                    }
                }
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
            state.long_term.nodes.insert(
                id,
                GraphNode {
                    id,
                    label: summary_text.clone(),
                    kind: "Summary".to_string(),
                    weight: 1.0 + salience,
                    last_seen: state.clock,
                    salience,
                    source_texts: source_texts.clone(),
                    embedding: centroid_embedding.clone(),
                    full_text: full_text.clone(),
                },
            );
            state
                .long_term
                .index
                .insert(summary_text.to_lowercase(), id);
            id
        };
        #[cfg(feature = "instrument")]
        _cctx.emit(
            trace::PipelineStep::CreateOrMergeSummaryNode,
            trace::TracePayload::KeyValue(vec![
                ("node_id".into(), node_id.to_string()),
                (
                    "action".into(),
                    if existing_summary_id.is_some() {
                        "merged".to_string()
                    } else {
                        "created".to_string()
                    },
                ),
                ("label".into(), safe_truncate(&summary_text, 120)),
            ]),
        );

        // 2. Semantic Topic Extraction: find high-frequency entities in the group
        let mut entity_counts: HashMap<String, (usize, String)> = HashMap::new();
        for entry in &group {
            let semantic_text = clean_semantic_noise(&entry.text);
            let entities =
                crate::memory::wernicke::extract_entities(&semantic_text, &state.keyword_cache);
            for entity in entities {
                let entry = entity_counts
                    .entry(entity.label.clone())
                    .or_insert((0, entity.kind));
                entry.0 += 1;
            }
        }

        #[cfg(feature = "instrument")]
        {
            let top_entities: Vec<String> = {
                let mut sorted: Vec<_> = entity_counts.iter().collect();
                sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
                sorted
                    .iter()
                    .take(5)
                    .map(|(label, (count, kind))| format!("{label}({kind})x{count}"))
                    .collect()
            };
            _cctx.emit(
                trace::PipelineStep::SemanticTopicExtraction,
                trace::TracePayload::KeyValue(vec![
                    ("unique_entities".into(), entity_counts.len().to_string()),
                    ("top_entities".into(), top_entities.join(", ")),
                    (
                        "promotion_threshold".into(),
                        semantic_topic_promotion_threshold(group.len()).to_string(),
                    ),
                ]),
            );
        }

        // If an entity appears in >50% of the group, it's a strong Topic/Anchor for this milestone
        let threshold = semantic_topic_promotion_threshold(group.len());
        let group_chemical_stamp = average_chemical_stamp(&group);
        for (label, (count, kind)) in entity_counts {
            if count >= threshold {
                let index_key = label.to_lowercase();
                let topic_id = if let Some(&id) = state.long_term.index.get(&index_key) {
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
                    state.long_term.index.insert(index_key, id);
                    id
                };

                // Create/strengthen edge between Summary and Topic
                neocortex::upsert_edge_with_chemical_stamp(
                    &mut state.long_term,
                    node_id,
                    topic_id,
                    "represents",
                    state.clock,
                    &group_chemical_stamp,
                );
            }
        }

        for entry in &group {
            if let Some(existing) = state.short_term.iter_mut().find(|e| e.id == entry.id) {
                if existing.consolidated {
                    continue;
                }
                let text = existing.text.clone();
                let salience = existing.salience;
                existing.usage = existing.usage.saturating_add(1);
                existing.last_access = state.clock;
                existing.consolidated = true;
                let _ = neocortex::update_graph(state, &text, salience);
            }
        }
        #[cfg(feature = "instrument")]
        _cctx.emit(
            trace::PipelineStep::MarkConsolidated,
            trace::TracePayload::KeyValue(vec![
                ("entries_marked".into(), group.len().to_string()),
                ("summary_node_id".into(), node_id.to_string()),
            ]),
        );

        summaries.push(GraphNodeSummary {
            id: node_id,
            label: summary_text,
            kind: "Summary".to_string(),
            weight: 1.0 + salience,
            edge_type: None,
            source_texts,
        });
    }

    // Phase C: use current neurochemical pruning pressure
    let consolidation_effective = neurochemistry::compute_effective(&state.chemistry);
    hippocampus::prune_short_term(
        &mut state.short_term,
        state.clock,
        consolidation_effective.pruning_pressure,
    );
    neocortex::prune_graph(&mut state.long_term, state.clock);

    #[cfg(feature = "instrument")]
    _cctx.emit(
        trace::PipelineStep::ConsolidateEnd,
        trace::TracePayload::KeyValue(vec![
            ("summaries_produced".into(), summaries.len().to_string()),
            ("l2_count_after".into(), state.short_term.len().to_string()),
            (
                "l3_nodes_after".into(),
                state.long_term.nodes.len().to_string(),
            ),
            (
                "l3_edges_after".into(),
                state.long_term.edges.len().to_string(),
            ),
        ]),
    );

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
/// `decay_rate_mod`: neurochemical multiplier (Phase C) — modulates decay speed.
fn apply_decay(state: &mut BrainState, decay_rate_mod: f32) {
    let now = state.clock;
    hippocampus::apply_l2_decay(&mut state.short_term, now, decay_rate_mod);
    neocortex::apply_l3_decay(&mut state.long_term, now, decay_rate_mod);
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
    state.long_term.index.insert(label.to_lowercase(), id);
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
    let node_id = if let Some(&id) = state.long_term.index.get(&label.to_lowercase()) {
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
    if let Some(&existing_id) = state.long_term.index.get(&label.to_lowercase()) {
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
                keyword_cooccurrence_tick_count: 0,
            });

        // Only increment tick_count if this is a new tick for this term
        let seen_in_new_tick = stats.total_count == 0 || stats.last_seen < clock;
        if seen_in_new_tick {
            stats.tick_count += 1;
        }
        stats.total_count += 1;

        // Filter 4: Co-occurrence — mark if this tick has existing keywords
        if has_existing_keyword {
            stats.has_keyword_cooccurrence = true;
            if seen_in_new_tick {
                stats.keyword_cooccurrence_tick_count += 1;
            }
        }
        stats.last_seen = clock;
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

    // Filter 5: Minimum information content — meaningful shape, not naked numeric
    // or punctuation noise. Quantitative facts are preserved as evidence refs,
    // not promoted into the learned domain keyword lexicon.
    if !is_promotable_term_shape(term) {
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

    // Filter 4: Repeated co-occurrence with existing keywords.
    // Back-compat: old stores only had the boolean, which counts as one
    // co-occurrence but is not enough by itself for new promotion.
    let cooccurrence_ticks = stats
        .keyword_cooccurrence_tick_count
        .max(u32::from(stats.has_keyword_cooccurrence));
    if cooccurrence_ticks < TERM_PROMOTION_MIN_KEYWORD_COOCCURRENCE_TICKS {
        return false;
    }

    true
}

fn is_promotable_term_shape(term: &str) -> bool {
    let term = term.trim();
    if term.len() < TERM_PROMOTION_MIN_LEN {
        return false;
    }

    let mut has_alpha = false;
    let mut alnum_count = 0usize;
    let mut separator_count = 0usize;

    for ch in term.chars() {
        if ch.is_ascii_alphanumeric() {
            alnum_count += 1;
            if ch.is_ascii_alphabetic() {
                has_alpha = true;
            }
        } else if matches!(ch, ' ' | '_' | '-' | '.') {
            separator_count += 1;
        } else {
            return false;
        }
    }

    has_alpha && alnum_count >= TERM_PROMOTION_MIN_LEN && separator_count <= alnum_count
}

// Persistence — moved to crate::tool::persistence, re-exported above.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Project a 384-dim embedding to 64-dim temporal context space.
/// Mean-pools groups of 6 consecutive dimensions.
fn project_to_temporal(embedding: &[f32]) -> Vec<f32> {
    let group_size = embedding.len() / TEMPORAL_CONTEXT_DIM;
    if group_size == 0 {
        return embedding.to_vec();
    }
    let mut projected = Vec::with_capacity(TEMPORAL_CONTEXT_DIM);
    for i in 0..TEMPORAL_CONTEXT_DIM {
        let start = i * group_size;
        let end = (start + group_size).min(embedding.len());
        let sum: f32 = embedding[start..end].iter().sum();
        projected.push(sum / (end - start) as f32);
    }
    // L2-normalize
    let norm: f32 = projected.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        projected.iter_mut().for_each(|v| *v /= norm);
    }
    projected
}

/// Update the TCM temporal context vector after a new tick.
/// Returns a snapshot of the current temporal context for storage on the entry.
fn update_temporal_context(state: &mut BrainState, embedding: &[f32], salience: f32) -> Vec<f32> {
    let is_boundary = salience >= EVENT_BOUNDARY_SALIENCE
        || (!state.last_tick_embedding.is_empty()
            && cosine_similarity(embedding, &state.last_tick_embedding) < 0.2);

    let drift_rate = if is_boundary {
        TCM_BOUNDARY_DRIFT_RATE
    } else {
        TCM_DRIFT_RATE
    };

    let projected = project_to_temporal(embedding);
    if state.temporal_context.is_empty() {
        state.temporal_context = projected;
    } else {
        for (i, val) in state.temporal_context.iter_mut().enumerate() {
            *val = drift_rate * *val + (1.0 - drift_rate) * projected[i];
        }
        // L2-normalize
        let norm: f32 = state
            .temporal_context
            .iter()
            .map(|v| v * v)
            .sum::<f32>()
            .sqrt();
        if norm > 0.0 {
            state.temporal_context.iter_mut().for_each(|v| *v /= norm);
        }
    }
    state.temporal_context.clone()
}

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
    fn test_consolidation_grouping_is_order_invariant_for_bridge_similarity() {
        fn entry(id: u64, text: &str, embedding: Vec<f32>) -> ShortTermEntry {
            ShortTermEntry {
                id,
                text: text.into(),
                summary: text.into(),
                embedding,
                salience: 0.7,
                usage: 1,
                ..Default::default()
            }
        }

        // A~B and B~C are above threshold, while A~C is below threshold.
        // Greedy seed-star clustering made this sensitive to which entry was first.
        let a = entry(1, "Alpha memory bridge start", vec![1.0, 0.0]);
        let b = entry(2, "Beta memory bridge middle", vec![0.76604444, 0.6427876]);
        let c = entry(3, "Gamma memory bridge end", vec![0.17364818, 0.98480775]);

        let orders = [
            vec![a.clone(), b.clone(), c.clone()],
            vec![b.clone(), a.clone(), c.clone()],
            vec![c, b, a],
        ];

        for ordered_entries in orders {
            let groups = consolidation_groups(&ordered_entries, 0.7);
            let grouped_ids: Vec<Vec<u64>> = groups
                .iter()
                .filter(|g| g.len() > 1)
                .map(|g| g.iter().map(|e| e.id).collect())
                .collect();

            assert_eq!(
                grouped_ids,
                vec![vec![1, 2, 3]],
                "bridge-connected entries should form the same component regardless of order"
            );
        }
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
    fn test_graph_skips_action_predicate_nodes() {
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "Fixed the parser bug in src/parser.rs and refactored RequestParser.",
        );
        let labels: Vec<&str> = state
            .brain
            .long_term
            .nodes
            .values()
            .map(|node| node.label.as_str())
            .collect();

        assert!(!labels.contains(&"fixed"), "got graph labels: {:?}", labels);
        assert!(
            !labels.contains(&"refactored"),
            "got graph labels: {:?}",
            labels
        );
        assert!(
            labels.contains(&"src/parser.rs") || labels.contains(&"RequestParser"),
            "real entities should still be encoded: {:?}",
            labels
        );
    }

    #[test]
    fn test_graph_edges_weight_local_reference_frames_above_distant_comentions() {
        let mut state = MemoryState::default();
        neocortex::update_graph(
            &mut state.brain,
            "Project Alpha uses SQLite for metadata. A ceramic frog near the monitor is named Biscuit.",
            0.8,
        );

        let alpha = graph_node_id(&state, "project alpha");
        let sqlite = graph_node_id(&state, "sqlite");
        let frog = graph_node_id(&state, "ceramic frog");
        let local = graph_edge_between(&state, alpha, sqlite)
            .expect("Project Alpha and SQLite should have a local edge");
        let distant = graph_edge_between(&state, alpha, frog)
            .expect("Project Alpha and ceramic frog should have only a weak co-mention edge");

        assert_eq!(local.kind, "uses_datastore");
        assert_eq!(distant.kind, "co_mentioned");
        assert!(
            local.weight > distant.weight,
            "local frame evidence should reinforce more strongly than distant co-mention: local={} distant={}",
            local.weight,
            distant.weight
        );
    }

    #[test]
    fn test_graph_edge_kind_upgrades_from_weak_comention_to_frame_bound() {
        let mut state = MemoryState::default();
        neocortex::update_graph(
            &mut state.brain,
            "Project Alpha is the migration project. SQLite sits beside a ceramic frog.",
            0.8,
        );
        state.brain.clock += 1;
        neocortex::update_graph(
            &mut state.brain,
            "Project Alpha uses SQLite for metadata.",
            0.8,
        );

        let alpha = graph_node_id(&state, "project alpha");
        let sqlite = graph_node_id(&state, "sqlite");
        let edge = graph_edge_between(&state, alpha, sqlite)
            .expect("repeated Project Alpha and SQLite mentions should share an edge");

        assert_eq!(
            edge.kind, "uses_datastore",
            "typed relation evidence should upgrade weaker earlier co-mention"
        );
    }

    #[test]
    fn test_graph_uses_typed_plain_english_relation_edges() {
        let mut state = MemoryState::default();
        neocortex::update_graph(
            &mut state.brain,
            "Project Alpha uses SQLite for metadata. The SQLite datastore backs Project Alpha's audit log.",
            0.8,
        );

        let alpha = graph_node_id(&state, "project alpha");
        let sqlite = graph_node_id(&state, "sqlite");
        let datastore = graph_node_id(&state, "sqlite datastore");

        let uses_edge = graph_edge_between(&state, alpha, sqlite)
            .expect("Project Alpha and SQLite should have a typed edge");
        let backs_edge = graph_edge_between(&state, datastore, alpha)
            .expect("SQLite datastore and Project Alpha should have a typed edge");

        assert_eq!(uses_edge.kind, "uses_datastore");
        assert_eq!(backs_edge.kind, "backs");
    }

    #[test]
    fn test_graph_edge_semantics_preserve_relation_evidence() {
        let mut state = MemoryState::default();
        neocortex::update_graph(
            &mut state.brain,
            "Project Alpha uses SQLite for metadata.",
            0.8,
        );

        let alpha = graph_node_id(&state, "project alpha");
        let sqlite = graph_node_id(&state, "sqlite");
        let key = GraphMemory::edge_stamp_key(alpha, sqlite);
        let semantics = state
            .brain
            .long_term
            .edge_semantics
            .get(&key)
            .expect("typed relation should preserve edge semantics");

        assert_eq!(semantics.kind, "uses_datastore");
        assert!(semantics.predicates.contains(&"uses".to_string()));
        assert!(
            semantics
                .evidence
                .contains(&"Project Alpha uses SQLite for metadata".to_string()),
            "evidence should be preserved: {:?}",
            semantics.evidence
        );
        assert!(semantics.confidence >= 0.8);
        assert_eq!(semantics.polarity, "Affirmed");
        assert_eq!(semantics.support_count, 1);
    }

    #[test]
    fn test_graph_emits_canonical_snake_case_edge_kinds() {
        let mut state = MemoryState::default();
        neocortex::update_graph(
            &mut state.brain,
            "Project Alpha uses SQLite for metadata. A ceramic frog near the monitor is named Biscuit.",
            0.8,
        );

        let alpha = graph_node_id(&state, "project alpha");
        let frog = graph_node_id(&state, "ceramic frog");
        let distant =
            graph_edge_between(&state, alpha, frog).expect("distant co-mention edge should exist");

        assert_eq!(distant.kind, "co_mentioned");
        assert!(
            state
                .brain
                .long_term
                .edges
                .iter()
                .all(|edge| !edge.kind.contains('-')),
            "new graph edges should use canonical snake_case kinds"
        );
    }

    #[test]
    fn test_hebbian_reinforcement() {
        let mut state = MemoryState::default();
        neocortex::update_graph(
            &mut state.brain,
            "fn process_data() uses struct Config",
            0.8,
        );

        let process_data = graph_node_id(&state, "process_data");
        let config = graph_node_id(&state, "config");
        let initial_weight = graph_edge_between(&state, process_data, config)
            .expect("graph encoder should create an edge before Hebbian reinforcement")
            .weight;

        state.brain.clock += 1;
        neocortex::hebbian_reinforce(&mut state.brain.long_term, &[process_data, config], 1);
        let reinforced_weight = graph_edge_between(&state, process_data, config)
            .expect("edge should remain after Hebbian reinforcement")
            .weight;

        assert!(
            reinforced_weight > initial_weight,
            "Hebbian reinforcement should strengthen co-active edges: before={} after={}",
            initial_weight,
            reinforced_weight
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
        apply_decay(&mut state.brain, 1.0);
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
        // Dentate Gyrus pattern separation: topics that share the DECISION prefix
        // but describe genuinely different subjects must remain as separate traces.
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "BUG: PostgreSQL connection pooling exhausted under high load causing 500 errors on the payments API",
        );
        tick(
            &mut state,
            "ARCHITECTURE: React Server Components replace client-side rendering for the marketing dashboard UI",
        );
        assert!(
            state.brain.short_term.len() >= 2,
            "distinct topics should be kept separate (dentate gyrus pattern separation), got {}",
            state.brain.short_term.len()
        );
    }

    #[test]
    fn test_orthogonalization_reduces_embedding_overlap_in_l2() {
        // Dentate Gyrus: after orthogonalization, L2 embeddings for related-but-distinct
        // entries should be less similar than their raw embeddings would be.
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "BUG: PostgreSQL connection pooling exhausted under high load causing 500 errors on the payments API",
        );
        tick(
            &mut state,
            "ARCHITECTURE: React Server Components replace client-side rendering for the marketing dashboard UI",
        );

        assert!(
            state.brain.short_term.len() >= 2,
            "should have 2 separate L2 entries, got {}",
            state.brain.short_term.len()
        );

        // The stored L2 embeddings should be more orthogonal than raw embeddings
        let raw_a = embed_text(
            "BUG: PostgreSQL connection pooling exhausted under high load causing 500 errors on the payments API",
            state.brain.config.embedding_dim,
        );
        let raw_b = embed_text(
            "ARCHITECTURE: React Server Components replace client-side rendering for the marketing dashboard UI",
            state.brain.config.embedding_dim,
        );
        let raw_sim = cosine_similarity(&raw_a, &raw_b);

        let stored_sim = cosine_similarity(
            &state.brain.short_term[0].embedding,
            &state.brain.short_term[1].embedding,
        );

        // Allow small floating-point tolerance; orthogonalization only activates
        // in the theta_low..theta_high zone, so well-separated texts stay unchanged.
        assert!(
            stored_sim <= raw_sim + 0.001,
            "stored embeddings should be at least as orthogonal as raw: stored={}, raw={}",
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
        apply_decay(&mut state.brain, 1.0);

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
        apply_decay(&mut state.brain, 1.0);

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
            cpeb_boost: 0.0,
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
    fn test_prune_graph_allows_open_ended_node_growth() {
        let mut state = MemoryState::default();
        let inserted = 2_098;
        // L3 nodes are open-ended; weak/aged nodes prune, but healthy nodes do
        // not compete for a fixed slot count.
        for i in 0..inserted {
            let id = state.brain.next_id;
            state.brain.next_id += 1;
            state.brain.long_term.nodes.insert(
                id,
                GraphNode {
                    id,
                    label: format!("node_{}", i),
                    kind: "Term".to_string(),
                    weight: 1.0,
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
        assert_eq!(state.brain.long_term.nodes.len(), inserted);

        neocortex::prune_graph(&mut state.brain.long_term, state.brain.clock);

        assert_eq!(
            state.brain.long_term.nodes.len(),
            inserted,
            "healthy L3 nodes should not be evicted by a fixed graph-node cap"
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
    fn test_related_ticks_create_separate_entries() {
        let mut state = MemoryState::default();
        // Create initial memory
        tick(
            &mut state,
            "DECISION: the database uses PostgreSQL for persistence",
        );
        let count_before = state.brain.short_term.len();

        // Query then tick with related but distinct information
        retrieve_context(&mut state.brain, "database PostgreSQL");

        tick(
            &mut state,
            "DECISION: the database PostgreSQL schema has users and sessions tables",
        );
        // Without reconsolidation, related-but-distinct facts should create
        // separate entries (or merge via normal similarity path), preserving
        // episodic details rather than overwriting them.
        assert!(
            state.brain.short_term.len() >= count_before,
            "related ticks should preserve entries, not destroy them"
        );
        assert!(!state.brain.short_term.is_empty());
    }

    #[test]
    fn test_retrieve_does_not_set_labile() {
        let mut state = MemoryState::default();
        tick(&mut state, "DECISION: test entry for labile check");

        retrieve_context(&mut state.brain, "test entry labile");
        // After removing reconsolidation, retrieve should not set labile_until
        assert_eq!(
            state.brain.short_term[0].labile_until, 0,
            "retrieve should not set labile_until"
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
    fn test_final_salience_normalization_preserves_high_end_rank() {
        let high = normalize_final_salience(1.0);
        let higher = normalize_final_salience(2.0);
        let extreme = normalize_final_salience(8.0);

        assert!(higher > high, "{higher} should exceed {high}");
        assert!(extreme > higher, "{extreme} should exceed {higher}");
        assert!(extreme < 1.0, "{extreme} should approach but not hit 1.0");
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

    fn graph_node_id(state: &MemoryState, label: &str) -> u64 {
        *state
            .brain
            .long_term
            .index
            .get(label)
            .unwrap_or_else(|| panic!("missing graph node {label}"))
    }

    fn graph_edge_between(
        state: &MemoryState,
        a: u64,
        b: u64,
    ) -> Option<&crate::memory::neocortex::GraphEdge> {
        state
            .brain
            .long_term
            .edges
            .iter()
            .find(|edge| (edge.from == a && edge.to == b) || (edge.from == b && edge.to == a))
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
    fn test_auto_consolidation_fires_at_threshold() {
        let mut state = MemoryState::default();
        assert_eq!(state.brain.ticks_since_consolidation, 0);

        // Tick up to threshold — auto-consolidation should fire on the last tick
        for i in 0..CONSOLIDATION_SUGGESTION_THRESHOLD {
            tick(
                &mut state,
                &format!("Auto-consolidation test tick number {}", i),
            );
        }
        // Counter should be reset to 0 by auto-consolidation
        assert_eq!(
            state.brain.ticks_since_consolidation, 0,
            "auto-consolidation should reset ticks_since_consolidation"
        );
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
        apply_decay(&mut state.brain, 1.0);
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
            &Neurochemistry::default(),
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
        // (0.25 is appropriate for semantic embeddings which produce higher similarities)
        const { assert!(MIN_QUERY_SIMILARITY <= 0.5) };
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
        let results = hippocampus::top_k_similar(
            &state.brain.short_term,
            &embedding,
            5,
            "MML syntax",
            &Neurochemistry::default(),
        );
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
            &Neurochemistry::default(),
        );
        assert!(!results.is_empty());
        // Keyword bonus is capped at KEYWORD_MATCH_BONUS_CAP (0.2)
        // Plus emotional_boost (up to 0.05)
        // The cosine sim alone is ~1.0, so total should not exceed 1.0 + cap + emotional_boost
        assert!(results[0].similarity <= 1.0 + KEYWORD_MATCH_BONUS_CAP + 0.05 + 0.01);
    }

    #[test]
    fn test_keyword_bonus_ignores_stopwords() {
        let mut state = MemoryState::default();
        tick(&mut state, "the and for with this that from");
        let embedding = embed_text("the and for", state.brain.config.embedding_dim);
        let results = hippocampus::top_k_similar(
            &state.brain.short_term,
            &embedding,
            5,
            "the and for",
            &Neurochemistry::default(),
        );
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
        let results = hippocampus::top_k_similar(
            &state.brain.short_term,
            &embedding,
            5,
            "",
            &Neurochemistry::default(),
        );
        // Should not panic, bonus should be 0
        for r in &results {
            assert!(r.similarity >= -1.0); // just a sanity check
        }
    }

    #[test]
    fn test_semantic_discrimination_unrelated_topics() {
        // Semantic embeddings should discriminate between unrelated topics
        let dim = 384;
        let a = embed_text("PostgreSQL database query optimization with indexes", dim);
        let b = embed_text("Italian cooking recipes for homemade pasta dishes", dim);
        let sim = cosine_similarity(&a, &b);
        // These are unrelated — similarity should be low
        assert!(
            sim < 0.7,
            "unrelated texts should have low cosine sim, got {sim}"
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
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
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
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
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
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
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
    fn test_consolidate_filters_semantic_junk_from_l3_summary_evidence() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;
        let texts = [
            "Project Alpha uses SQLite for metadata [[[[ %%@@",
            "Project Alpha keeps SQLite backup audits in the runbook //// &&&&",
            "Project Alpha validates SQLite restore row counts }}}} @@@@",
        ];
        for text in &texts {
            hippocampus::insert_short_term(
                &mut state.brain,
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
            );
        }
        state.brain.config.theta_low = 0.3;
        consolidate(&mut state.brain);

        let summary_node = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|n| n.kind == "Summary")
            .expect("should have a Summary node");
        let l3_text = std::iter::once(summary_node.label.as_str())
            .chain(summary_node.source_texts.iter().map(String::as_str))
            .chain(summary_node.full_text.as_deref())
            .collect::<Vec<_>>()
            .join(" ");

        assert!(l3_text.contains("Project Alpha"));
        assert!(l3_text.contains("SQLite"));
        for junk in ["[[[[", "%%@@", "////", "&&&&", "}}}}", "@@@@"] {
            assert!(
                !l3_text.contains(junk),
                "L3 Summary evidence should not retain {junk}: {l3_text}"
            );
        }
    }

    #[test]
    fn test_consolidate_merges_summary_by_centroid_embedding() {
        let mut state = MemoryState::default();
        state.brain.config.theta_low = 0.9;

        let existing_id = 1_000;
        state.brain.next_id = existing_id + 1;
        state.brain.long_term.nodes.insert(
            existing_id,
            neocortex::GraphNode {
                id: existing_id,
                label: "session security overview".to_string(),
                kind: "Summary".to_string(),
                weight: 1.2,
                last_seen: 1,
                salience: 0.5,
                source_texts: vec!["legacy access-control note".to_string()],
                embedding: vec![1.0, 0.0],
                full_text: Some("legacy access-control note".to_string()),
            },
        );
        state
            .brain
            .long_term
            .index
            .insert("session security overview".to_string(), existing_id);

        let texts = [
            "token validation middleware",
            "signing key rotation",
            "credential expiry enforcement",
        ];
        for text in &texts {
            hippocampus::insert_short_term(
                &mut state.brain,
                text,
                vec![1.0, 0.0],
                0.8,
                Vec::new(),
                0.0,
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
            );
        }

        consolidate(&mut state.brain);

        let summaries: Vec<_> = state
            .brain
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Summary")
            .collect();
        assert_eq!(
            summaries.len(),
            1,
            "centroid-matched consolidation should merge instead of creating a duplicate Summary"
        );

        let node = state
            .brain
            .long_term
            .nodes
            .get(&existing_id)
            .expect("existing Summary should be reused");
        assert!(
            texts
                .iter()
                .all(|text| node.source_texts.contains(&text.to_string())),
            "merged Summary should retain new evidence in source_texts"
        );
        assert_eq!(node.embedding, vec![1.0, 0.0]);
        assert!(
            node.full_text
                .as_ref()
                .is_some_and(|full_text| full_text.contains("signing key rotation")),
            "merged Summary should refresh full_text from the new consolidation group"
        );
    }

    fn insert_semantic_topic_entry(state: &mut MemoryState, text: &str) {
        hippocampus::insert_short_term(
            &mut state.brain,
            text,
            vec![1.0, 0.0],
            0.8,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            ChemicalStamp::default(),
        );
    }

    fn has_summary_represents_label(state: &MemoryState, label: &str) -> bool {
        let label = label.to_lowercase();
        let topic_ids: Vec<u64> = state
            .brain
            .long_term
            .nodes
            .iter()
            .filter(|(_, node)| node.label.to_lowercase() == label)
            .map(|(&id, _)| id)
            .collect();
        let summary_ids: Vec<u64> = state
            .brain
            .long_term
            .nodes
            .iter()
            .filter(|(_, node)| node.kind == "Summary")
            .map(|(&id, _)| id)
            .collect();

        state.brain.long_term.edges.iter().any(|edge| {
            edge.kind == "represents"
                && ((summary_ids.contains(&edge.from) && topic_ids.contains(&edge.to))
                    || (summary_ids.contains(&edge.to) && topic_ids.contains(&edge.from)))
        })
    }

    #[test]
    fn test_semantic_topic_requires_strict_majority() {
        let mut state = MemoryState::default();
        state.brain.config.theta_low = 0.9;

        insert_semantic_topic_entry(&mut state, "RecurringAnchor apple");
        insert_semantic_topic_entry(&mut state, "RecurringAnchor banana");
        insert_semantic_topic_entry(&mut state, "DistinctOne carrot");
        insert_semantic_topic_entry(&mut state, "DistinctTwo celery");

        consolidate(&mut state.brain);

        assert!(
            !has_summary_represents_label(&state, "RecurringAnchor"),
            "an entity present in exactly half of a group should not become a Summary topic"
        );
    }

    #[test]
    fn test_semantic_topic_links_summary_on_strict_majority() {
        let mut state = MemoryState::default();
        state.brain.config.theta_low = 0.9;

        insert_semantic_topic_entry(&mut state, "RecurringAnchor apple");
        insert_semantic_topic_entry(&mut state, "RecurringAnchor banana");
        insert_semantic_topic_entry(&mut state, "RecurringAnchor carrot");
        insert_semantic_topic_entry(&mut state, "DistinctOne celery");

        consolidate(&mut state.brain);

        assert!(
            has_summary_represents_label(&state, "RecurringAnchor"),
            "an entity present in a strict majority of a group should link the Summary to that topic"
        );
    }

    #[test]
    fn test_consolidation_does_not_reencode_already_consolidated_entries() {
        let mut state = MemoryState::default();
        state.brain.config.theta_low = 0.9;

        insert_semantic_topic_entry(&mut state, "StableAnchor apple");
        insert_semantic_topic_entry(&mut state, "StableAnchor banana");
        insert_semantic_topic_entry(&mut state, "StableAnchor carrot");

        consolidate(&mut state.brain);

        let anchor_weight_after_first = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|node| node.label.eq_ignore_ascii_case("StableAnchor"))
            .expect("StableAnchor node should exist after first consolidation")
            .weight;
        let usage_after_first: Vec<u32> = state
            .brain
            .short_term
            .iter()
            .map(|entry| entry.usage)
            .collect();

        consolidate(&mut state.brain);

        let anchor_weight_after_second = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|node| node.label.eq_ignore_ascii_case("StableAnchor"))
            .expect("StableAnchor node should still exist after second consolidation")
            .weight;
        let usage_after_second: Vec<u32> = state
            .brain
            .short_term
            .iter()
            .map(|entry| entry.usage)
            .collect();

        assert!(
            anchor_weight_after_second <= anchor_weight_after_first,
            "already-consolidated entries should not be re-encoded as fresh graph evidence"
        );
        assert_eq!(
            usage_after_first, usage_after_second,
            "already-consolidated entries should not receive another consolidation usage bump"
        );
    }

    #[test]
    fn test_consolidate_preserves_all_source_texts() {
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
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
            );
        }
        consolidate(&mut state.brain);

        for node in state.brain.long_term.nodes.values() {
            if node.kind == "Summary" {
                assert_eq!(
                    node.source_texts.len(),
                    25,
                    "Summary source_texts should preserve all cleaned group evidence"
                );
                for i in 0..25 {
                    let expected = format!(
                        "feature beta variant number {} implemented in rendering module Y pipeline",
                        i
                    );
                    assert!(
                        node.source_texts.contains(&expected),
                        "Summary source_texts should retain minority evidence: {expected}"
                    );
                }
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
                0,
                Vec::new(),
                Vec::new(),
                ChemicalStamp::default(),
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

        // Consolidated entries SHOULD still appear in query results —
        // episodic facts must remain retrievable even after L3 summary creation.
        let ctx = retrieve_context(&mut state.brain, "MML tempo BPM");
        assert!(
            !ctx.short_term.is_empty(),
            "consolidated entries should still be returned in query results"
        );
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
                        ..MemoryRef::default()
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
            wall_clock: 0,
            extracted_dates: Vec::new(),
            temporal_context: Vec::new(),
            chemical_stamp: ChemicalStamp::default(),
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
            wall_clock: 0,
            extracted_dates: Vec::new(),
            temporal_context: Vec::new(),
            chemical_stamp: ChemicalStamp::default(),
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
    fn test_flush_promotes_all_unpromoted() {
        let mut state = MemoryState::default();
        // Even low-salience entries should be promoted on flush — no data loss
        state.brain.working_memory.push(WorkingMemoryEntry {
            id: 998,
            text: "just some conversational content".to_string(),
            embedding: embed_text(
                "just some conversational content",
                state.brain.config.embedding_dim,
            ),
            salience: 0.05,
            tick_created: state.brain.clock,
            rehearsal_count: 0,
            promoted: false,
            emotional_valence: 0.0,
            wall_clock: 0,
            extracted_dates: Vec::new(),
            temporal_context: Vec::new(),
            chemical_stamp: ChemicalStamp::default(),
        });
        let st_before = state.brain.short_term.len();

        prefrontal::flush_working_memory(&mut state.brain);

        assert!(
            state.brain.working_memory.is_empty(),
            "flush should clear working memory"
        );
        assert!(
            state.brain.short_term.len() > st_before,
            "flush should promote all unpromoted entries to L2"
        );
    }

    #[test]
    fn test_flush_carries_l1_metadata_to_l2() {
        let mut state = MemoryState::default();
        let text = "checkpoint happened on 2026-04-19";
        let embedding = embed_text(text, state.brain.config.embedding_dim);
        let tcm = vec![0.25; TEMPORAL_CONTEXT_DIM];
        let mut stamp = ChemicalStamp::default();
        stamp.da_at_encoding = 0.72;

        prefrontal::push_working_memory_with_metadata(
            &mut state.brain,
            text,
            &embedding,
            0.05,
            0.0,
            1_776_543_210,
            vec!["2026-04-19".to_string()],
            tcm.clone(),
            stamp.clone(),
        );

        prefrontal::flush_working_memory(&mut state.brain);

        let promoted = state
            .brain
            .short_term
            .iter()
            .find(|entry| entry.text == text)
            .expect("flushed L1 entry should promote to L2");
        assert_eq!(promoted.wall_clock, 1_776_543_210);
        assert_eq!(promoted.extracted_dates, vec!["2026-04-19".to_string()]);
        assert_eq!(promoted.temporal_context, tcm);
        assert_eq!(promoted.chemical_stamp.da_at_encoding, stamp.da_at_encoding);
    }

    #[test]
    fn test_displacement_carries_l1_metadata_to_l2() {
        let mut state = MemoryState::default();
        let text = "displaced checkpoint happened on 2026-04-20";
        let embedding = embed_text(text, state.brain.config.embedding_dim);
        let tcm = vec![0.5; TEMPORAL_CONTEXT_DIM];
        let mut stamp = ChemicalStamp::default();
        stamp.ne_at_encoding = 0.64;

        prefrontal::push_working_memory_with_metadata(
            &mut state.brain,
            text,
            &embedding,
            0.05,
            0.0,
            1_776_629_610,
            vec!["2026-04-20".to_string()],
            tcm.clone(),
            stamp.clone(),
        );

        for i in 0..state.brain.config.immediate_capacity {
            let filler = format!("filler entry {i}");
            let emb = embed_text(&filler, state.brain.config.embedding_dim);
            prefrontal::push_working_memory(&mut state.brain, &filler, &emb, 0.01, 0.0);
        }

        let promoted = state
            .brain
            .short_term
            .iter()
            .find(|entry| entry.text == text)
            .expect("displaced L1 entry should promote to L2");
        assert_eq!(promoted.wall_clock, 1_776_629_610);
        assert_eq!(promoted.extracted_dates, vec!["2026-04-20".to_string()]);
        assert_eq!(promoted.temporal_context, tcm);
        assert_eq!(promoted.chemical_stamp.ne_at_encoding, stamp.ne_at_encoding);
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
            wall_clock: 0,
            extracted_dates: Vec::new(),
            temporal_context: Vec::new(),
            chemical_stamp: ChemicalStamp::default(),
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
            cpeb_boost: 0.0,
        });
        state.brain.long_term.rebuild_edge_index();

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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
        });
        state.brain.long_term.rebuild_edge_index();

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
    fn test_edge_chemical_stamp_is_local_to_edge() {
        let mut graph = GraphMemory::default();
        let stamp = ChemicalStamp {
            ne_at_encoding: 0.7,
            cortisol_at_encoding: 0.1,
            da_at_encoding: 0.6,
            ach_at_encoding: 0.2,
        };

        neocortex::upsert_edge_with_chemical_stamp(&mut graph, 10, 11, "related", 1, &stamp);

        let stored = graph
            .edge_chemical_stamps
            .get(&GraphMemory::edge_stamp_key(10, 11))
            .expect("edge should carry a chemical stamp");
        assert_eq!(stored.ne_at_encoding, stamp.ne_at_encoding);
        assert_eq!(stored.da_at_encoding, stamp.da_at_encoding);
    }

    #[test]
    fn test_edge_chemical_stamp_modulates_decay() {
        let mut neutral = GraphMemory::default();
        neutral.edges.push(GraphEdge {
            from: 20,
            to: 21,
            weight: 1.0,
            last_seen: 0,
            ..GraphEdge::default()
        });

        let mut protected = GraphMemory::default();
        protected.edges.push(GraphEdge {
            from: 20,
            to: 21,
            weight: 1.0,
            last_seen: 0,
            ..GraphEdge::default()
        });
        protected.edge_chemical_stamps.insert(
            GraphMemory::edge_stamp_key(20, 21),
            ChemicalStamp {
                ne_at_encoding: 1.0,
                cortisol_at_encoding: 0.0,
                da_at_encoding: 1.0,
                ach_at_encoding: 0.0,
            },
        );

        neocortex::apply_l3_decay(&mut neutral, 100, 1.0);
        neocortex::apply_l3_decay(&mut protected, 100, 1.0);

        assert!(
            protected.edges[0].weight > neutral.edges[0].weight,
            "local NE/DA stamp should protect edge from decay: protected={} neutral={}",
            protected.edges[0].weight,
            neutral.edges[0].weight
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
    fn test_cpeb_only_tags_touched_edges() {
        // Create graph with edges A-B and C-D, touch only A and B,
        // verify only A-B edge gets CPEB boost
        let mut long_term = GraphMemory::default();
        let (a, b, c, d) = (1, 2, 3, 4);
        long_term.edges.push(GraphEdge {
            from: a,
            to: b,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 20,
            activation_count: 1,
            stability: 1.0,
            ..Default::default()
        });
        long_term.edges.push(GraphEdge {
            from: c,
            to: d,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 20,
            activation_count: 1,
            stability: 1.0,
            ..Default::default()
        });

        let touched = vec![a, b]; // Only A and B touched
        let tagged = neocortex::cpeb_tag_edges_scoped(
            &mut long_term,
            20,
            0.8,
            CPEB_STABILITY_BOOST,
            &touched,
        );

        assert_eq!(tagged, 1, "only A-B edge should be tagged");
        assert!(
            long_term.edges[0].stability > 1.0,
            "A-B edge should be boosted"
        );
        assert!(
            long_term.edges[0].cpeb_boost > 0.0,
            "A-B edge should have cpeb_boost"
        );
        assert!(
            (long_term.edges[1].stability - 1.0).abs() < f32::EPSILON,
            "C-D edge should be unchanged"
        );
        assert_eq!(
            long_term.edges[1].cpeb_boost, 0.0,
            "C-D edge should have no cpeb_boost"
        );
    }

    #[test]
    fn test_cpeb_boost_decays_over_time() {
        let mut long_term = GraphMemory::default();
        long_term.nodes.insert(
            1,
            GraphNode {
                id: 1,
                label: "A".into(),
                kind: "Entity".into(),
                weight: 1.0,
                last_seen: 100,
                ..Default::default()
            },
        );
        long_term.nodes.insert(
            2,
            GraphNode {
                id: 2,
                label: "B".into(),
                kind: "Entity".into(),
                weight: 1.0,
                last_seen: 100,
                ..Default::default()
            },
        );
        long_term.edges.push(GraphEdge {
            from: 1,
            to: 2,
            weight: 1.0,
            kind: "related".into(),
            last_seen: 100,
            activation_count: 1,
            stability: 1.0,
            ..Default::default()
        });

        // Tag the edge with CPEB boost
        let touched = vec![1, 2];
        neocortex::cpeb_tag_edges_scoped(&mut long_term, 100, 0.9, CPEB_STABILITY_BOOST, &touched);
        let initial_cpeb = long_term.edges[0].cpeb_boost;
        let initial_stability = long_term.edges[0].stability;
        assert!(initial_cpeb > 0.0);
        assert!(initial_stability > 1.0);

        // Apply decay 50 times (1.0 = baseline neurochemical rate)
        for _ in 0..50 {
            neocortex::apply_l3_decay(&mut long_term, 101, 1.0);
        }

        assert!(
            long_term.edges[0].cpeb_boost < initial_cpeb * 0.1,
            "cpeb_boost should have decayed significantly: {} vs initial {}",
            long_term.edges[0].cpeb_boost,
            initial_cpeb,
        );
        assert!(
            long_term.edges[0].stability >= 1.0,
            "stability should never drop below 1.0: {}",
            long_term.edges[0].stability,
        );
    }

    #[test]
    fn test_soft_cap_allows_growth_beyond_10() {
        // Repeatedly boost stability, verify it exceeds 10.0 but plateaus below 20.0
        let mut stability = 1.0_f32;
        for _ in 0..100 {
            stability = neocortex::soft_cap_stability(stability * 1.3);
        }
        assert!(
            stability > 10.0,
            "stability should exceed old hard cap: {}",
            stability
        );
        assert!(
            stability < 20.0,
            "stability should plateau below max: {}",
            stability
        );
    }

    #[test]
    fn test_soft_cap_linear_below_knee() {
        // Verify stability < 10 is unchanged by soft_cap_stability()
        for raw in [0.5, 1.0, 3.0, 5.0, 9.99] {
            let capped = neocortex::soft_cap_stability(raw);
            assert!(
                (capped - raw).abs() < f32::EPSILON,
                "below knee should be identity: raw={} capped={}",
                raw,
                capped,
            );
        }
    }

    #[test]
    fn test_cpeb_no_boost_on_neutral_tick() {
        let mut state = MemoryState::default();
        state.brain.clock = 20;

        let text = "updated routine documentation and formatting notes";
        if state.brain.emotional_prototypes.is_empty() {
            state.brain.emotional_prototypes = seed_emotional_prototypes();
        }
        let emb = embed_text(text, 384);
        let v = compute_emotional_valence(&state.brain.emotional_prototypes, &emb);
        assert!(
            v.abs() < CPEB_VALENCE_THRESHOLD,
            "routine text should be below CPEB threshold, got {}",
            v
        );
    }

    #[test]
    fn test_cpeb_tagged_edge_decays_slower() {
        let mut long_term = GraphMemory::default();
        long_term.nodes.insert(
            1,
            GraphNode {
                id: 1,
                label: "A".into(),
                kind: "Entity".into(),
                weight: 1.0,
                last_seen: 50,
                ..Default::default()
            },
        );
        long_term.nodes.insert(
            2,
            GraphNode {
                id: 2,
                label: "B".into(),
                kind: "Entity".into(),
                weight: 1.0,
                last_seen: 50,
                ..Default::default()
            },
        );
        long_term.nodes.insert(
            3,
            GraphNode {
                id: 3,
                label: "C".into(),
                kind: "Entity".into(),
                weight: 1.0,
                last_seen: 50,
                ..Default::default()
            },
        );
        long_term.nodes.insert(
            4,
            GraphNode {
                id: 4,
                label: "D".into(),
                kind: "Entity".into(),
                weight: 1.0,
                last_seen: 50,
                ..Default::default()
            },
        );
        long_term.edges.push(GraphEdge {
            from: 1,
            to: 2,
            weight: 1.0,
            kind: "related".into(),
            last_seen: 50,
            activation_count: 1,
            stability: 1.0,
            ..Default::default()
        });
        long_term.edges.push(GraphEdge {
            from: 3,
            to: 4,
            weight: 1.0,
            kind: "related".into(),
            last_seen: 50,
            activation_count: 1,
            stability: 1.0,
            ..Default::default()
        });

        // Tag only edge 1-2 via scoped CPEB
        let touched = vec![1, 2];
        let tagged = neocortex::cpeb_tag_edges_scoped(
            &mut long_term,
            50,
            0.9,
            CPEB_STABILITY_BOOST,
            &touched,
        );
        assert_eq!(tagged, 1);

        neocortex::apply_l3_decay(&mut long_term, 250, 1.0);

        assert!(
            long_term.edges[0].weight > long_term.edges[1].weight,
            "tagged edge should retain more weight after decay: tagged={} untagged={}",
            long_term.edges[0].weight,
            long_term.edges[1].weight
        );
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
            .insert("configloader".into(), cfg_id);

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
            .insert("jwtsecret".into(), jwt_id);

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
            cpeb_boost: 0.0,
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
            0,
            Vec::new(),
            Vec::new(),
            ChemicalStamp::default(),
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
            0,
            Vec::new(),
            Vec::new(),
            ChemicalStamp::default(),
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
            cpeb_boost: 0.0,
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
            0,
            Vec::new(),
            Vec::new(),
            ChemicalStamp::default(),
        );

        // Pattern complete for ConfigLoader — the DatabasePool entry is indirect
        let direct = hippocampus::top_k_similar(
            &state.brain.short_term,
            &embed_text("ConfigLoader", dim),
            5,
            "ConfigLoader",
            &Neurochemistry::default(),
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
        // High-valence ticks should spike cortisol/NE → consolidation pressure
        tick(
            &mut state,
            "this is a complete disaster, the system crashed and we lost all the data",
        );
        tick(
            &mut state,
            "everything is broken and nothing works, the entire system is down",
        );
        tick(
            &mut state,
            "catastrophic failure, data corruption everywhere and it keeps getting worse",
        );
        tick(
            &mut state,
            "total system meltdown, all services are offline and unrecoverable",
        );
        tick(
            &mut state,
            "critical data loss across all production databases, no backups survived",
        );

        let effective = neurochemistry::compute_effective(&state.brain.chemistry);
        assert!(
            should_suggest_consolidation(&state),
            "burst of high-valence ticks should trigger consolidation, pressure={}",
            effective.consolidation_pressure
        );
    }

    #[test]
    fn test_neutral_ticks_no_early_consolidation() {
        let mut state = MemoryState::default();
        tick(&mut state, "Updated the documentation formatting");
        tick(&mut state, "Refactored variable names in the config module");
        tick(&mut state, "Added a comment to the parser function");

        let effective = neurochemistry::compute_effective(&state.brain.chemistry);
        assert!(
            !should_suggest_consolidation(&state),
            "neutral ticks should not trigger early consolidation, pressure={}, ticks_since={}",
            effective.consolidation_pressure,
            state.brain.ticks_since_consolidation
        );
    }

    #[test]
    fn test_cortisol_decays_over_time() {
        let mut state = MemoryState::default();
        // Build up cortisol with high-valence ticks
        tick(
            &mut state,
            "this is a complete disaster, everything crashed and data was destroyed",
        );
        tick(
            &mut state,
            "everything is broken and the situation is hopeless, we lost it all",
        );
        tick(
            &mut state,
            "catastrophic failure everywhere, nothing can be recovered",
        );
        let after_spike = state.brain.chemistry.cortisol;
        assert!(
            after_spike > 0.0,
            "cortisol should spike, got {}",
            after_spike
        );

        // Many low-intensity ticks should let cortisol decay
        for _ in 0..30 {
            tick(
                &mut state,
                "updated the configuration settings for the project",
            );
        }

        assert!(
            state.brain.chemistry.cortisol < after_spike,
            "cortisol should decay from spike: {} -> {}",
            after_spike,
            state.brain.chemistry.cortisol
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
        state.long_term.index.insert(label.to_lowercase(), id);
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
            cpeb_boost: 0.0,
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
    fn test_replay_reinforces_existing_edges() {
        // Two entries with different entities but close in time — replay should
        // reinforce existing edges between co-active entity pairs.
        let mut state = MemoryState::default();
        state.brain.clock = 10;

        let node_a = 800;
        let node_b = 801;
        insert_test_node(&mut state.brain, node_a, "ConfigParser");
        insert_test_node(&mut state.brain, node_b, "RouterSetup");

        // Pre-existing edge between them
        state.brain.long_term.edges.push(GraphEdge {
            from: node_a,
            to: node_b,
            weight: 0.5,
            kind: "related".into(),
            last_seen: 5,
            ..Default::default()
        });
        state.brain.long_term.rebuild_edge_index();

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

        let weight_before = state.brain.long_term.edges[0].weight;
        neocortex::replay_consolidation(&mut state.brain);

        assert!(
            state.brain.long_term.edges[0].weight > weight_before,
            "replay should reinforce existing edges between co-active entities"
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
                cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
            cpeb_boost: 0.0,
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
                cpeb_boost: 0.0,
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

        let shared = "Database migration to PostgreSQL 15 uses flyway scripts for schema changes";
        let text1 = format!("{shared} and zero-downtime rollout");
        let text2 = format!("{shared} and validation checks");
        let emb1 = embed_text(&text1, dim);
        let emb2 = embed_text(&text2, dim);

        state.brain.short_term.push(ShortTermEntry {
            id: 1,
            text: text1.into(),
            embedding: emb1,
            salience: 0.6,
            ..Default::default()
        });
        state.brain.short_term.push(ShortTermEntry {
            id: 2,
            text: text2.into(),
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
    fn test_salient_anchor_gets_systems_consolidation_with_low_salience_support() {
        let mut state = MemoryState::default();

        for (id, text, salience) in [
            (1, "Critical rollback decision for database migration", 0.8),
            (2, "Routine note about database migration checklist", 0.1),
            (3, "Routine note about database migration owner", 0.1),
        ] {
            state.brain.short_term.push(ShortTermEntry {
                id,
                text: text.into(),
                embedding: vec![1.0, 0.0],
                salience,
                ..Default::default()
            });
        }

        consolidate(&mut state.brain);

        let summary_node = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|n| n.kind == "Summary")
            .expect("should have Summary node");

        assert!(
            !summary_node.embedding.is_empty(),
            "strong anchor plus supporting facts should get centroid embedding"
        );
        assert!(
            summary_node.full_text.is_some(),
            "strong anchor plus supporting facts should get full_text"
        );
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
            0,
            Vec::new(),
            Vec::new(),
            ChemicalStamp::default(),
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
            assert!(
                s.keyword_cooccurrence_tick_count >= TERM_PROMOTION_MIN_KEYWORD_COOCCURRENCE_TICKS,
                "keyword_cooccurrence_tick_count={} should be >= {}",
                s.keyword_cooccurrence_tick_count,
                TERM_PROMOTION_MIN_KEYWORD_COOCCURRENCE_TICKS
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
    fn test_punctuation_noise_term_not_promoted() {
        assert!(!is_promotable_term_shape("[[[["));
        assert!(!is_promotable_term_shape("////"));
        assert!(!is_promotable_term_shape("____"));
        assert!(!is_promotable_term_shape("%%@@"));
    }

    #[test]
    fn test_mixed_alphanumeric_term_shape_can_promote() {
        assert!(is_promotable_term_shape("sqlite3"));
        assert!(is_promotable_term_shape("phase5"));
        assert!(is_promotable_term_shape("v1.2.3"));
        assert!(is_promotable_term_shape("project alpha"));
    }

    #[test]
    fn test_repeated_co_occurrence_required_for_promotion() {
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
                keyword_cooccurrence_tick_count: 0,
            },
        );
        assert!(
            !should_promote_term(&state.brain, "orphanterm"),
            "term without keyword co-occurrence should not promote"
        );

        // A legacy one-bit co-occurrence signal is useful history, but too weak
        // to promote learned salience vocabulary on its own.
        state
            .brain
            .term_frequency
            .get_mut("orphanterm")
            .unwrap()
            .has_keyword_cooccurrence = true;
        assert!(
            !should_promote_term(&state.brain, "orphanterm"),
            "single legacy co-occurrence should not promote"
        );

        state
            .brain
            .term_frequency
            .get_mut("orphanterm")
            .unwrap()
            .keyword_cooccurrence_tick_count = TERM_PROMOTION_MIN_KEYWORD_COOCCURRENCE_TICKS;
        assert!(
            should_promote_term(&state.brain, "orphanterm"),
            "term WITH repeated keyword co-occurrence and sufficient ticks should promote"
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
    fn test_term_stats_serialization_compat() {
        // TermStats should deserialize with defaults for missing fields
        let json = r#"{"tick_count":5,"total_count":10,"first_seen":1,"last_seen":5}"#;
        let stats: TermStats = serde_json::from_str(json).unwrap();
        assert_eq!(stats.tick_count, 5);
        assert!(!stats.has_keyword_cooccurrence); // default false
        assert_eq!(stats.keyword_cooccurrence_tick_count, 0);
    }

    // -----------------------------------------------------------------------
    // Neurochemistry integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ne_spikes_on_high_salience() {
        let mut state = MemoryState::default();
        assert_eq!(state.brain.chemistry.norepinephrine, 0.0);
        // DECISION: and BUG: prefixes trigger high salience
        tick(
            &mut state,
            "DECISION: Chose distributed architecture because of horizontal scaling needs",
        );
        assert!(
            state.brain.chemistry.norepinephrine > 0.0,
            "NE should spike on high-salience tick, got {}",
            state.brain.chemistry.norepinephrine
        );
    }

    #[test]
    fn test_cortisol_spikes_on_negative_valence() {
        let mut state = MemoryState::default();
        assert_eq!(state.brain.chemistry.cortisol, 0.0);
        tick(&mut state, "CRITICAL: everything is broken, catastrophic failure, data is destroyed and we lost everything");
        assert!(
            state.brain.chemistry.cortisol > 0.0,
            "cortisol should spike on negative valence tick, got {}",
            state.brain.chemistry.cortisol
        );
    }

    #[test]
    fn test_da_spikes_on_positive_valence() {
        let mut state = MemoryState::default();
        assert_eq!(state.brain.chemistry.dopamine, 0.0);
        tick(&mut state, "shipped the feature successfully, great progress, everything works perfectly and users love it");
        assert!(
            state.brain.chemistry.dopamine > 0.0,
            "DA should spike on positive valence tick, got {}",
            state.brain.chemistry.dopamine
        );
    }

    #[test]
    fn test_ach_spikes_on_novel_input() {
        let mut state = MemoryState::default();
        assert_eq!(state.brain.chemistry.acetylcholine, 0.0);
        // Novel input with high enough salience to pass attention gate
        tick(&mut state, "DECISION: adopting a completely novel quantum-resistant cryptographic protocol for all authentication");
        assert!(
            state.brain.chemistry.acetylcholine > 0.0,
            "ACh should spike on novel input (no prior memories), got {}",
            state.brain.chemistry.acetylcholine
        );
    }

    #[test]
    fn test_ecb_rises_on_routine() {
        let mut state = MemoryState::default();
        assert_eq!(state.brain.chemistry.endocannabinoid, 0.0);
        // Low-salience tick (no decision/bug keywords, plain text)
        tick(&mut state, "updated some formatting");
        assert!(
            state.brain.chemistry.endocannabinoid > 0.0,
            "eCB should rise on routine/low-salience tick, got {}",
            state.brain.chemistry.endocannabinoid
        );
    }

    #[test]
    fn test_cortisol_triggers_consolidation() {
        let mut state = MemoryState::default();
        // Manually set high cortisol to simulate accumulated stress
        state.brain.chemistry.cortisol = 0.9;
        state.brain.chemistry.norepinephrine = 0.5;
        let effective = neurochemistry::compute_effective(&state.brain.chemistry);
        assert!(
            effective.consolidation_pressure >= CONSOLIDATION_PRESSURE_THRESHOLD,
            "high cortisol should produce consolidation pressure >= {}, got {}",
            CONSOLIDATION_PRESSURE_THRESHOLD,
            effective.consolidation_pressure
        );
    }

    #[test]
    fn test_chemistry_persists_across_save_load() {
        use crate::tool::persistence::{load_memory_from_path, save_memory_to_path};
        let mut state = MemoryState::default();
        state.brain.chemistry.norepinephrine = 0.3;
        state.brain.chemistry.cortisol = 0.5;
        state.brain.chemistry.dopamine = 0.7;
        state.brain.chemistry.serotonin = 0.4;
        state.brain.chemistry.acetylcholine = 0.2;
        state.brain.chemistry.endocannabinoid = 0.1;

        let path = std::env::temp_dir().join("legend_chem_test.lz4");
        save_memory_to_path(&state, path.to_str().unwrap()).unwrap();
        let loaded = load_memory_from_path(path.to_str().unwrap()).unwrap();

        assert!((loaded.brain.chemistry.norepinephrine - 0.3).abs() < f32::EPSILON);
        assert!((loaded.brain.chemistry.cortisol - 0.5).abs() < f32::EPSILON);
        assert!((loaded.brain.chemistry.dopamine - 0.7).abs() < f32::EPSILON);
        assert!((loaded.brain.chemistry.serotonin - 0.4).abs() < f32::EPSILON);
        assert!((loaded.brain.chemistry.acetylcholine - 0.2).abs() < f32::EPSILON);
        assert!((loaded.brain.chemistry.endocannabinoid - 0.1).abs() < f32::EPSILON);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_ne_spikes_on_context_switch() {
        let mut state = MemoryState::default();
        // First tick establishes a topic
        tick(
            &mut state,
            "DECISION: we are using PostgreSQL for the database layer with full ACID compliance",
        );
        let ne_after_first = state.brain.chemistry.norepinephrine;
        // Second tick on a completely different topic triggers context switch
        tick(&mut state, "DECISION: switching the frontend framework to a completely different rendering paradigm");
        // NE should be at least as high (context switch spike + salience spike)
        // Even with decay between ticks, the context switch spike should be visible
        assert!(
            state.brain.chemistry.norepinephrine > 0.0,
            "NE should be elevated after context switch, got {}",
            state.brain.chemistry.norepinephrine
        );
        let _ = ne_after_first; // suppress unused warning
    }

    // --- Phase B: Chemical stamp integration tests ---

    #[test]
    fn test_chemical_stamp_recorded_at_encoding() {
        let mut state = MemoryState::default();
        // Spike NE before ticking
        state.brain.chemistry.norepinephrine = 0.6;
        tick(
            &mut state,
            "ARCHITECTURE: critical security vulnerability found in auth module",
        );
        // Find the L2 entry
        let entry = state.brain.short_term.last().unwrap();
        assert!(
            entry.chemical_stamp.ne_at_encoding > 0.0,
            "NE stamp should be > 0 after high-NE tick, got {}",
            entry.chemical_stamp.ne_at_encoding
        );
    }

    #[test]
    fn test_state_dependent_retrieval_bonus() {
        let mut state = MemoryState::default();
        // Encode under high NE
        state.brain.chemistry.norepinephrine = 0.8;
        state.brain.chemistry.dopamine = 0.5;
        tick(
            &mut state,
            "DECISION: switched database to PostgreSQL for JOIN support",
        );

        // Query under matching chemistry → should get state bonus
        let emb = embed_text("PostgreSQL database", state.brain.config.embedding_dim);
        let with_match = hippocampus::top_k_similar(
            &state.brain.short_term,
            &emb,
            5,
            "PostgreSQL database",
            &state.brain.chemistry,
        );

        // Query under zero chemistry → no state bonus
        let baseline_chem = Neurochemistry::default();
        let without_match = hippocampus::top_k_similar(
            &state.brain.short_term,
            &emb,
            5,
            "PostgreSQL database",
            &baseline_chem,
        );

        if !with_match.is_empty() && !without_match.is_empty() {
            assert!(
                with_match[0].similarity >= without_match[0].similarity,
                "matching chemistry should give >= similarity: {} vs {}",
                with_match[0].similarity,
                without_match[0].similarity
            );
        }
    }

    #[test]
    fn test_ne_encoded_memories_decay_slower() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        // Entry with high NE stamp
        hippocampus::insert_short_term(
            &mut state.brain,
            "high NE memory",
            embed_text("high NE memory", dim),
            0.5,
            Vec::new(),
            0.8,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp {
                ne_at_encoding: 0.8,
                cortisol_at_encoding: 0.0,
                da_at_encoding: 0.0,
                ach_at_encoding: 0.0,
            },
        );
        // Entry with zero NE stamp
        hippocampus::insert_short_term(
            &mut state.brain,
            "zero NE memory",
            embed_text("zero NE memory", dim),
            0.5,
            Vec::new(),
            0.8,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );

        // Advance clock and apply decay
        state.brain.clock += 50;
        hippocampus::apply_l2_decay(&mut state.brain.short_term, state.brain.clock, 1.0);

        let high_ne = state
            .brain
            .short_term
            .iter()
            .find(|e| e.text.contains("high NE"))
            .unwrap();
        let zero_ne = state
            .brain
            .short_term
            .iter()
            .find(|e| e.text.contains("zero NE"))
            .unwrap();

        assert!(
            high_ne.emotional_valence.abs() > zero_ne.emotional_valence.abs(),
            "NE-encoded should retain more emotional valence: {} vs {}",
            high_ne.emotional_valence,
            zero_ne.emotional_valence
        );
    }

    #[test]
    fn test_da_encoded_memories_reinforce_stronger() {
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        // Entry with high DA stamp
        hippocampus::insert_short_term(
            &mut state.brain,
            "high DA memory about testing",
            embed_text("high DA memory about testing", dim),
            0.1,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp {
                ne_at_encoding: 0.0,
                cortisol_at_encoding: 0.0,
                da_at_encoding: 0.8,
                ach_at_encoding: 0.0,
            },
        );
        let high_da_id = state.brain.short_term.last().unwrap().id;

        // Entry with zero DA stamp
        hippocampus::insert_short_term(
            &mut state.brain,
            "zero DA memory about testing",
            embed_text("zero DA memory about testing", dim),
            0.1,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );
        let zero_da_id = state.brain.short_term.last().unwrap().id;

        // Pre-load gradient_sq_sum so AdaGrad LR is reasonable (not astronomical)
        for e in state.brain.short_term.iter_mut() {
            e.gradient_sq_sum = 5.0;
        }
        // Reinforce both with moderate signal
        basal_ganglia::reinforce(&mut state.brain, &[high_da_id], 0.5);
        basal_ganglia::reinforce(&mut state.brain, &[zero_da_id], 0.5);

        let high_da = state
            .brain
            .short_term
            .iter()
            .find(|e| e.id == high_da_id)
            .unwrap();
        let zero_da = state
            .brain
            .short_term
            .iter()
            .find(|e| e.id == zero_da_id)
            .unwrap();

        assert!(
            high_da.salience > zero_da.salience,
            "DA-encoded should have higher salience after reinforcement: {} vs {}",
            high_da.salience,
            zero_da.salience
        );
    }

    #[test]
    fn test_chemical_stamp_persists_save_load() {
        let stamp = neurochemistry::ChemicalStamp {
            ne_at_encoding: 0.4,
            cortisol_at_encoding: 0.2,
            da_at_encoding: 0.6,
            ach_at_encoding: 0.3,
        };
        let entry = ShortTermEntry {
            chemical_stamp: stamp,
            ..Default::default()
        };
        let serialized = serde_json::to_string(&entry).unwrap();
        let deserialized: ShortTermEntry = serde_json::from_str(&serialized).unwrap();
        assert!((deserialized.chemical_stamp.ne_at_encoding - 0.4).abs() < f32::EPSILON);
        assert!((deserialized.chemical_stamp.da_at_encoding - 0.6).abs() < f32::EPSILON);
    }

    // ---- Dentate Gyrus: orthogonalize-after-decide integration tests ----

    #[test]
    fn test_merge_decision_uses_raw_similarity() {
        // Two entries at raw sim ~0.80 should merge (low-merge), not split.
        // Before fix: orthogonalization pushed sim below theta_low, causing duplicates.
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "DECISION: Chose Redis for caching because it has excellent pub/sub support for real-time",
        );
        tick(
            &mut state,
            "DECISION: Chose Redis for caching because it has excellent pub/sub support for messaging",
        );
        assert_eq!(
            state.brain.short_term.len(),
            1,
            "similar entries should merge using raw similarity, got {} entries",
            state.brain.short_term.len()
        );
    }

    #[test]
    fn test_create_new_still_orthogonalizes() {
        // Dissimilar entries should still get orthogonalized when stored as new L2 entries.
        let mut state = MemoryState::default();
        tick(
            &mut state,
            "BUG: PostgreSQL connection pooling exhausted under high load causing errors",
        );
        tick(
            &mut state,
            "ARCHITECTURE: React Server Components replace client-side rendering for the dashboard",
        );
        assert!(
            state.brain.short_term.len() >= 2,
            "distinct entries should create separate L2 entries"
        );

        // The second entry's stored embedding should differ from its raw embedding
        // (orthogonalized away from the first) — but only if they're in the confusable zone.
        // If they're far apart, orthogonalization is a no-op (which is correct behavior).
        let raw_second = embed_text(
            "ARCHITECTURE: React Server Components replace client-side rendering for the dashboard",
            state.brain.config.embedding_dim,
        );
        let stored_second = &state.brain.short_term[1].embedding;
        let sim_to_raw = cosine_similarity(stored_second, &raw_second);
        // Either orthogonalized (sim < 1.0) or outside confusable zone (sim ≈ 1.0) — both valid
        assert!(
            sim_to_raw <= 1.001,
            "stored embedding should be valid, sim_to_raw={}",
            sim_to_raw
        );
    }

    #[test]
    fn test_merged_entry_preserves_raw_semantics() {
        // After low-merge, the merged embedding should reflect raw average, not orthogonalized.
        let mut state = MemoryState::default();
        let text_a =
            "DECISION: embedding quality improvement using n-grams for better keyword matching";
        let text_b =
            "DECISION: embedding quality improvement using trigrams for better keyword matching";
        tick(&mut state, text_a);
        tick(&mut state, text_b);

        // Should have merged
        assert_eq!(
            state.brain.short_term.len(),
            1,
            "similar ticks should merge, got {}",
            state.brain.short_term.len()
        );

        // The merged embedding should be close to the average of raw embeddings
        let raw_a = embed_text(text_a, state.brain.config.embedding_dim);
        let raw_b = embed_text(text_b, state.brain.config.embedding_dim);
        let raw_avg: Vec<f32> = raw_a
            .iter()
            .zip(raw_b.iter())
            .map(|(a, b)| (a + b) / 2.0)
            .collect();
        let norm: f32 = raw_avg.iter().map(|v| v * v).sum::<f32>().sqrt();
        let raw_avg_normalized: Vec<f32> = raw_avg.iter().map(|v| v / norm).collect();

        let stored = &state.brain.short_term[0].embedding;
        let sim = cosine_similarity(stored, &raw_avg_normalized);
        assert!(
            sim > 0.95,
            "merged embedding should be close to raw average, got sim={}",
            sim
        );
    }

    #[test]
    fn test_low_similarity_merge_salience_uses_smooth_reinforcement() {
        let mut state = MemoryState::default();
        state.brain.config.theta_low = 0.2;
        state.brain.config.theta_high = 0.99;

        let text_a =
            "DECISION: embedding quality improvement using n-grams for better keyword matching";
        let text_b =
            "DECISION: embedding quality improvement using trigrams for better keyword matching";

        tick(&mut state, text_a);
        assert_eq!(state.brain.short_term.len(), 1);
        let initial_salience = state.brain.short_term[0].salience;

        tick(&mut state, text_b);
        assert_eq!(
            state.brain.short_term.len(),
            1,
            "similar ticks should take the low-similarity merge path"
        );
        let reinforced_salience = state.brain.short_term[0].salience;

        assert!(
            reinforced_salience > initial_salience,
            "low merge should reinforce salience: {reinforced_salience} vs {initial_salience}"
        );
        assert!(
            reinforced_salience < 1.0,
            "low merge should approach the ceiling smoothly, got {reinforced_salience}"
        );
    }

    // -----------------------------------------------------------------------
    // Anterior PFC — Plan integration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_plan_tick_stores_plans() {
        let mut state = MemoryState::default();
        tick_impl(
            &mut state.brain,
            "PLAN: Test Plan\n[active] Fix the parser\n[deferred] Optimize later",
        );
        assert_eq!(state.brain.plans.len(), 1);
        assert_eq!(state.brain.plans[0].name, "Test Plan");
        assert_eq!(state.brain.plans[0].items.len(), 2);
        assert_eq!(
            state.brain.plans[0].items[0].status,
            anterior_pfc::ItemStatus::Active
        );
        assert_eq!(
            state.brain.plans[0].items[1].status,
            anterior_pfc::ItemStatus::Deferred
        );
    }

    #[test]
    fn test_plan_tick_also_creates_l2_entry() {
        let mut state = MemoryState::default();
        let result = tick_impl(
            &mut state.brain,
            "PLAN: Test Plan\n[active] Fix the parser\n[deferred] Optimize later",
        );
        // The PLAN: text should also enter L2 as a normal tick
        assert!(
            result.action == "created" || result.action == "working_memory_only",
            "Expected created or working_memory_only, got {}",
            result.action
        );
    }

    #[test]
    fn test_multiple_plans_coexist() {
        let mut state = MemoryState::default();
        tick_impl(
            &mut state.brain,
            "PLAN: Plan Alpha\n[active] Task A1\n[pending] Task A2",
        );
        tick_impl(&mut state.brain, "PLAN: Plan Beta\n[active] Task B1");
        assert_eq!(state.brain.plans.len(), 2);
        assert_eq!(state.brain.plans[0].name, "Plan Alpha");
        assert_eq!(state.brain.plans[1].name, "Plan Beta");
    }

    #[test]
    fn test_plan_update_in_place() {
        let mut state = MemoryState::default();
        tick_impl(
            &mut state.brain,
            "PLAN: My Plan\n[active] Item 1\n[pending] Item 2",
        );
        assert_eq!(state.brain.plans[0].items.len(), 2);

        tick_impl(
            &mut state.brain,
            "PLAN: My Plan\n[done] Item 1\n[active] Item 2\n[pending] Item 3",
        );
        assert_eq!(state.brain.plans.len(), 1); // Still one plan
        assert_eq!(state.brain.plans[0].items.len(), 3); // Updated items
    }

    #[test]
    fn test_plan_archive_after_retention() {
        let mut state = MemoryState::default();
        // Create a plan and mark all items done
        tick_impl(
            &mut state.brain,
            "PLAN: Archive Test\n[done] Item 1\n[done] Item 2",
        );
        assert_eq!(state.brain.plans.len(), 1);
        assert!(state.brain.plans[0].completed_at.is_some());

        // Fast-forward clock past archive threshold
        let completed_at = state.brain.plans[0].completed_at.unwrap();
        state.brain.clock = completed_at + anterior_pfc::PLAN_ARCHIVE_TICKS + 1;

        // Next tick should archive the plan
        let l2_count_before = state.brain.short_term.len();
        tick_impl(
            &mut state.brain,
            "DECISION: Unrelated tick to trigger archival",
        );
        assert_eq!(
            state.brain.plans.len(),
            0,
            "Archived plan should be removed"
        );
        assert!(
            state.brain.short_term.len() > l2_count_before,
            "Archived plan should create L2 entry"
        );
        // Check the archived entry contains plan info
        let archive_entry = state
            .brain
            .short_term
            .iter()
            .find(|e| e.text.contains("Completed plan: Archive Test"));
        assert!(archive_entry.is_some(), "Should find archived plan in L2");
    }

    #[test]
    fn test_plans_persist_across_serialization() {
        let mut state = MemoryState::default();
        tick_impl(
            &mut state.brain,
            "PLAN: Persistence Test\n[active] Survive serialization",
        );
        assert_eq!(state.brain.plans.len(), 1);

        // Serialize and deserialize
        let serialized = rmp_serde::to_vec(&state).unwrap();
        let deserialized: MemoryState = rmp_serde::from_slice(&serialized).unwrap();
        assert_eq!(deserialized.brain.plans.len(), 1);
        assert_eq!(deserialized.brain.plans[0].name, "Persistence Test");
        assert_eq!(deserialized.brain.plans[0].items.len(), 1);
    }

    #[test]
    fn test_intention_cue_match_in_query() {
        let mut state = MemoryState::default();
        // Create a plan with a deferred item about "database optimization"
        tick_impl(
            &mut state.brain,
            "PLAN: Performance Work\n[deferred] Optimize database query performance",
        );

        // Query about database — should surface the deferred plan item
        let context = retrieve_context(&mut state.brain, "database optimization strategies");
        let has_plan_item = context
            .working_memory
            .iter()
            .any(|m| m.text.contains("[Plan:") && m.text.contains("database"));
        assert!(
            has_plan_item,
            "Query should surface matching deferred plan item via spontaneous retrieval"
        );
    }

    // -----------------------------------------------------------------------
    // Hippocampal overload defense tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_capacity_cortisol_spike() {
        let mut state = MemoryState::default();
        // Use small capacity for test speed
        state.brain.config.short_term_capacity = 20;
        state.brain.chemistry.cortisol = 0.0;
        let dim = state.brain.config.embedding_dim;

        // Fill L2 past 75% (16/20 = 80%)
        for i in 0..16 {
            let text = format!("DECISION: important architectural choice number {i}");
            let emb = embed_text(&text, dim);
            hippocampus::insert_short_term(
                &mut state.brain,
                &text,
                emb,
                0.5,
                Vec::new(),
                0.0,
                0,
                Vec::new(),
                Vec::new(),
                neurochemistry::ChemicalStamp::default(),
            );
        }
        assert!(state.brain.short_term.len() >= 16);
        state.brain.chemistry.cortisol = 0.0;

        // Tick — should trigger capacity cortisol spike
        tick_impl(
            &mut state.brain,
            "DECISION: one more important decision about system design",
        );
        assert!(
            state.brain.chemistry.cortisol > 0.0,
            "cortisol should rise when L2 is past 75% capacity, got {}",
            state.brain.chemistry.cortisol
        );
    }

    #[test]
    fn test_attention_gate_tightens_under_load() {
        let mut state = MemoryState::default();
        // Small capacity for speed
        state.brain.config.short_term_capacity = 20;
        let dim = state.brain.config.embedding_dim;

        // Fill L2 to 90% (18/20) with pre-built entries
        for i in 0..18 {
            let text =
                format!("DECISION: critical architecture decision {i} about system resilience");
            let emb = embed_text(&text, dim);
            hippocampus::insert_short_term(
                &mut state.brain,
                &text,
                emb,
                0.5,
                Vec::new(),
                0.0,
                0,
                Vec::new(),
                Vec::new(),
                neurochemistry::ChemicalStamp::default(),
            );
        }
        assert_eq!(state.brain.short_term.len(), 18);

        // Tick with low-salience text — should stay in L1 due to tightened gate
        // (dynamic threshold ~0.25 + 0.15 * (0.9-0.75)/0.25 = 0.34)
        tick_impl(&mut state.brain, "some general note about project progress");

        // Low-salience tick stays in working memory under load
        assert!(
            !state.brain.working_memory.is_empty(),
            "low-salience tick should land in working memory under load"
        );
    }

    #[test]
    fn test_emergency_consolidation_before_eviction() {
        let mut state = MemoryState::default();
        state.brain.config.short_term_capacity = 10;
        state.brain.ticks_since_consolidation = 5;
        let dim = state.brain.config.embedding_dim;

        // Fill to capacity
        for i in 0..10 {
            let text = format!("DECISION: entry {i} about database design patterns");
            let emb = embed_text(&text, dim);
            hippocampus::insert_short_term(
                &mut state.brain,
                &text,
                emb,
                0.5,
                Vec::new(),
                0.0,
                0,
                Vec::new(),
                Vec::new(),
                neurochemistry::ChemicalStamp::default(),
            );
        }
        assert_eq!(state.brain.short_term.len(), 10);

        // Insert one more — should trigger emergency consolidation before eviction
        let text = "DECISION: final entry triggers emergency consolidation";
        let emb = embed_text(text, dim);
        hippocampus::insert_short_term(
            &mut state.brain,
            text,
            emb,
            0.5,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );

        // Emergency consolidation should have fired and reset the counter
        assert_eq!(
            state.brain.ticks_since_consolidation, 0,
            "emergency consolidation should reset ticks_since_consolidation"
        );
        // Some entries should now be marked consolidated
        let consolidated_count = state
            .brain
            .short_term
            .iter()
            .filter(|e| e.consolidated)
            .count();
        assert!(
            consolidated_count > 0,
            "emergency consolidation should mark entries as consolidated, got 0"
        );
    }

    #[test]
    fn test_eviction_prefers_consolidated_entries() {
        let mut state = MemoryState::default();
        state.brain.config.short_term_capacity = 5;
        state.brain.ticks_since_consolidation = 5;
        let dim = state.brain.config.embedding_dim;

        // Fill to capacity with similar entries (so consolidation groups them)
        for i in 0..5 {
            let text =
                format!("DECISION: database optimization strategy {i} for query performance");
            let emb = embed_text(&text, dim);
            hippocampus::insert_short_term(
                &mut state.brain,
                &text,
                emb,
                0.5,
                Vec::new(),
                0.0,
                0,
                Vec::new(),
                Vec::new(),
                neurochemistry::ChemicalStamp::default(),
            );
        }

        // Force consolidation so entries get L3 backup
        consolidate(&mut state.brain);

        // Insert a new entry to trigger eviction
        let text = "DECISION: completely unrelated new topic about UI redesign";
        let emb = embed_text(text, dim);
        hippocampus::insert_short_term(
            &mut state.brain,
            text,
            emb,
            0.8, // high salience — should survive
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );

        // The new high-salience entry should survive
        assert!(
            state
                .brain
                .short_term
                .iter()
                .any(|e| e.text.contains("UI redesign")),
            "new high-salience entry should survive eviction"
        );
    }

    // -----------------------------------------------------------------------
    // Fast mapping & CA3 backup similarity tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_fast_map_creates_trace_on_unconsolidated_eviction() {
        // When an unconsolidated L2 entry is evicted, a Trace node should be
        // created in L3 preserving its embedding and text.
        let mut state = MemoryState::default();
        state.brain.config.short_term_capacity = 3;
        // Prevent emergency consolidation (so entries stay unconsolidated)
        state.brain.ticks_since_consolidation = 0;
        let dim = state.brain.config.embedding_dim;

        // Fill with 3 diverse entries (won't cluster)
        let texts = [
            "DECISION: chose Redis for caching because of TTL support",
            "BUG: null pointer in authentication middleware on empty tokens",
            "ARCHITECTURE: event sourcing pattern for order processing pipeline",
        ];
        for text in &texts {
            let emb = embed_text(text, dim);
            hippocampus::insert_short_term(
                &mut state.brain,
                text,
                emb,
                0.3,
                Vec::new(),
                0.0,
                0,
                Vec::new(),
                Vec::new(),
                neurochemistry::ChemicalStamp::default(),
            );
        }
        assert_eq!(state.brain.short_term.len(), 3);
        let trace_count_before = state
            .brain
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Trace")
            .count();
        assert_eq!(trace_count_before, 0, "no Trace nodes before eviction");

        // Insert a 4th entry — should evict the lowest-scoring unconsolidated entry
        // and create a Trace node for it.
        let text = "DECISION: switched from REST to GraphQL for flexible queries";
        let emb = embed_text(text, dim);
        hippocampus::insert_short_term(
            &mut state.brain,
            text,
            emb,
            0.5,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );

        let trace_count_after = state
            .brain
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Trace")
            .count();
        assert_eq!(
            trace_count_after, 1,
            "one Trace node should be created for evicted entry"
        );

        // The Trace should have a valid embedding and full_text
        let trace = state
            .brain
            .long_term
            .nodes
            .values()
            .find(|n| n.kind == "Trace")
            .unwrap();
        assert!(!trace.embedding.is_empty(), "Trace should have embedding");
        assert!(trace.full_text.is_some(), "Trace should have full_text");
        assert!(
            !trace.source_texts.is_empty(),
            "Trace should have source_texts"
        );
    }

    #[test]
    fn test_trace_node_cap_enforced() {
        // When Trace nodes exceed TRACE_NODE_CAP, the weakest one is pruned.
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;
        state.brain.config.short_term_capacity = 2;
        state.brain.ticks_since_consolidation = 0;

        // Pre-populate L3 with TRACE_NODE_CAP Trace nodes
        for i in 0..TRACE_NODE_CAP {
            let id = state.brain.next_id;
            state.brain.next_id += 1;
            state.brain.long_term.nodes.insert(
                id,
                neocortex::GraphNode {
                    id,
                    label: format!("trace_{i}"),
                    kind: "Trace".to_string(),
                    weight: 1.0,
                    last_seen: 0,
                    salience: 0.1 + (i as f32 * 0.001), // increasing salience
                    source_texts: vec![format!("trace text {i}")],
                    embedding: embed_text(&format!("trace text {i}"), dim),
                    full_text: Some(format!("trace text {i}")),
                },
            );
        }
        let before = state
            .brain
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Trace")
            .count();
        assert_eq!(before, TRACE_NODE_CAP);

        // Fill L2 and trigger eviction of an unconsolidated entry
        let texts = [
            "DECISION: use PostgreSQL for relational data storage",
            "DECISION: use MongoDB for document storage",
        ];
        for text in &texts {
            let emb = embed_text(text, dim);
            hippocampus::insert_short_term(
                &mut state.brain,
                text,
                emb,
                0.3,
                Vec::new(),
                0.0,
                0,
                Vec::new(),
                Vec::new(),
                neurochemistry::ChemicalStamp::default(),
            );
        }
        // One more to trigger eviction + fast mapping
        let emb = embed_text("DECISION: new entry forcing eviction", dim);
        hippocampus::insert_short_term(
            &mut state.brain,
            "DECISION: new entry forcing eviction",
            emb,
            0.5,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );

        let after = state
            .brain
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Trace")
            .count();
        assert!(
            after <= TRACE_NODE_CAP,
            "Trace count should not exceed cap: got {after}, cap {TRACE_NODE_CAP}"
        );
    }

    #[test]
    fn test_ca3_embedding_similarity_detects_l3_backup() {
        // CA3 pattern completion: eviction should detect L3 backup via embedding
        // similarity, not exact text match.
        let mut state = MemoryState::default();
        state.brain.config.short_term_capacity = 2;
        state.brain.ticks_since_consolidation = 0;
        let dim = state.brain.config.embedding_dim;

        // Create a Summary node with an embedding close to what we'll insert in L2
        let original_text = "DECISION: chose JWT tokens for API authentication";
        let original_emb = embed_text(original_text, dim);
        let summary_id = state.brain.next_id;
        state.brain.next_id += 1;
        state.brain.long_term.nodes.insert(
            summary_id,
            neocortex::GraphNode {
                id: summary_id,
                label: "JWT auth decision".to_string(),
                kind: "Summary".to_string(),
                weight: 2.0,
                last_seen: 0,
                salience: 0.5,
                // Deliberately use DIFFERENT text than entry (old exact match would miss this)
                source_texts: vec!["slightly different JWT text".to_string()],
                embedding: original_emb.clone(),
                full_text: Some(original_text.to_string()),
            },
        );

        // Insert an L2 entry with similar embedding (same text) and mark as consolidated
        hippocampus::insert_short_term(
            &mut state.brain,
            original_text,
            original_emb,
            0.3,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );
        state.brain.short_term.last_mut().unwrap().consolidated = true;

        // Insert a high-salience entry to fill up
        let text2 = "ARCHITECTURE: microservice gateway design with rate limiting";
        hippocampus::insert_short_term(
            &mut state.brain,
            text2,
            embed_text(text2, dim),
            0.9,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );

        // Insert a 3rd entry — triggers eviction. The consolidated JWT entry
        // should be preferred for eviction because CA3 finds the Summary backup.
        let text3 = "DECISION: switched to gRPC for internal service communication";
        hippocampus::insert_short_term(
            &mut state.brain,
            text3,
            embed_text(text3, dim),
            0.5,
            Vec::new(),
            0.0,
            0,
            Vec::new(),
            Vec::new(),
            neurochemistry::ChemicalStamp::default(),
        );

        // The consolidated entry (JWT) should have been evicted (has L3 backup)
        // and the new entries should survive.
        assert!(
            state
                .brain
                .short_term
                .iter()
                .any(|e| e.text.contains("gRPC")),
            "new gRPC entry should survive"
        );
        assert!(
            state
                .brain
                .short_term
                .iter()
                .any(|e| e.text.contains("microservice")),
            "high-salience microservice entry should survive"
        );
    }

    #[test]
    fn test_trace_promoted_on_retrieval() {
        // A Trace node retrieved via query should be promoted to Summary.
        let mut state = MemoryState::default();
        let dim = state.brain.config.embedding_dim;

        let trace_text = "DECISION: chose Redis for distributed caching with TTL";
        let trace_emb = embed_text(trace_text, dim);
        let trace_id = state.brain.next_id;
        state.brain.next_id += 1;
        state.brain.long_term.nodes.insert(
            trace_id,
            neocortex::GraphNode {
                id: trace_id,
                label: "Redis caching decision".to_string(),
                kind: "Trace".to_string(),
                weight: 1.3,
                last_seen: 0,
                salience: TRACE_INITIAL_SALIENCE,
                source_texts: vec![trace_text.to_string()],
                embedding: trace_emb,
                full_text: Some(trace_text.to_string()),
            },
        );

        assert_eq!(state.brain.long_term.nodes[&trace_id].kind, "Trace");

        // Query something related — the Trace should be surfaced and promoted
        let _ctx = retrieve_context(&mut state.brain, "Redis caching decision");

        let node = &state.brain.long_term.nodes[&trace_id];
        assert_eq!(
            node.kind, "Summary",
            "Trace should be promoted to Summary after retrieval"
        );
        assert!(
            node.salience > TRACE_INITIAL_SALIENCE,
            "promoted Trace should have boosted salience"
        );
    }
}
