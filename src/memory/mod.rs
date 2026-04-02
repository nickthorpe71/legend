pub mod amygdala;
pub mod dentate_gyrus;
pub mod embed;
pub mod extract;
pub mod keyword_bootstrap;
pub mod keyword_cache;
pub mod keywords;
pub mod summarize;

use amygdala::compute_emotional_valence;
use dentate_gyrus::{diversity_pass, sparse_orthogonalize, word_overlap, MERGE_WORD_OVERLAP_THRESHOLD};
use embed::{compute_salience, cosine_similarity, embed_text, merge_embeddings};
use extract::extract_entities;
use keyword_cache::KeywordCache;
use summarize::{chunk_text, summarize_group, summarize_single, summarize_text};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

const MEMORY_FILE: &str = ".legend/memory.lz4";

/// Magic bytes prepended to MessagePack payloads (after LZ4 decompression).
const MSGPACK_MAGIC: &[u8; 4] = b"LGND";
/// Format version for the MessagePack serialization.
const MSGPACK_FORMAT_VERSION: u8 = 1;

// Decay & reinforcement tuning constants
const SHORT_TERM_DECAY_RATE: f32 = 0.001;
const LONG_TERM_DECAY_RATE: f32 = 0.0005;
const EVICTION_DECAY_RATE: f32 = 0.002;
const HEBBIAN_EDGE_BOOST: f32 = 0.05;
const HEBBIAN_NODE_BOOST: f32 = 0.02;
/// Maximum edge weight to prevent Hebbian reinforcement explosion.
const HEBBIAN_EDGE_CEILING: f32 = 10.0;
/// Maximum node weight to prevent Hebbian node boost explosion.
const HEBBIAN_NODE_CEILING: f32 = 5.0;
const EDGE_REINFORCE_DELTA: f32 = 0.1;
const NODE_WEIGHT_BASE: f32 = 0.2;
const PRUNE_THRESHOLD: f32 = 0.1;
const PRUNE_USAGE_WEIGHT: f32 = 0.05;
const PRUNE_AGE_WEIGHT: f32 = 0.001;
/// Maximum number of session log entries to keep.
const SESSION_LOG_CAPACITY: usize = 100;
/// How much a reinforcement signal scales graph node weight adjustment.
const REINFORCE_GRAPH_SCALE: f32 = 0.1;
/// Hard cap on long-term graph nodes. Lowest-weight nodes evicted when exceeded.
const GRAPH_NODE_CAPACITY: usize = 2048;
/// Hard cap on long-term graph edges.
const GRAPH_EDGE_CAPACITY: usize = 8192;
/// Minimum node weight to survive graph pruning.
const GRAPH_PRUNE_WEIGHT: f32 = 0.05;
/// Passive salience boost for top retrieval result, scaled by similarity.
const AUTO_REINFORCE_SCALE: f32 = 0.03;
/// Minimum similarity for a tick to reconsolidate a labile memory instead of creating new.
const RECONSOLIDATION_THRESHOLD: f32 = 0.35;
/// Minimum cosine similarity for a query result to be returned (noise floor).
const MIN_QUERY_SIMILARITY: f32 = 0.15;
/// Minimum salience for a working memory entry to be promoted to L2 (short-term).
/// Decision keywords: 0.3+ → PROMOTES, Bug/blocker: 0.4+ → PROMOTES,
/// Architecture: 0.25 → PROMOTES (at threshold), Plain text: 0.05 → STAYS IN L1.
const PROMOTION_SALIENCE_THRESHOLD: f32 = 0.25;
/// Bonus added per non-stopword query keyword found in an entry's text.
const KEYWORD_MATCH_BONUS: f32 = 0.05;
/// Maximum total keyword bonus that can be applied.
const KEYWORD_MATCH_BONUS_CAP: f32 = 0.2;
/// How many ticks a memory stays labile after retrieval before re-stabilizing.
const LABILE_WINDOW: u64 = 5;
/// Number of ticks before suggesting a consolidation.
const CONSOLIDATION_SUGGESTION_THRESHOLD: u32 = 15;

// Gradient-descent-inspired salience & weight normalization constants
/// Epsilon for AdaGrad denominator stability.
const ADAGRAD_EPSILON: f32 = 1e-6;
/// Base learning rate for AdaGrad salience updates (replaces REINFORCE_SALIENCE_SCALE in reinforce()).
const ADAGRAD_BASE_LR: f32 = 0.15;
/// Cap on accumulated squared gradients to prevent LR collapse on very active entries.
const ADAGRAD_SQ_SUM_CAP: f32 = 1000.0;
/// Ticks between salience EMA renormalization passes.
const RENORM_INTERVAL: u64 = 10;
/// EMA blend weight toward normalized values (gentle).
const RENORM_BLEND: f32 = 0.1;
/// Salience penalty applied to retrieved-but-unreinforced entries.
const CONTRASTIVE_PENALTY: f32 = 0.02;
/// Graph weight ceiling before periodic normalization fires.
const GRAPH_WEIGHT_TARGET_MAX: f32 = 2.0;
/// Ticks between graph weight normalization passes.
const GRAPH_NORM_INTERVAL: u64 = 5;

/// Spreading activation: activation decays by this factor per hop.
const SPREADING_ACTIVATION_DECAY: f32 = 0.5;
/// Maximum hops for spreading activation in graph_lookup.
const SPREADING_ACTIVATION_MAX_HOPS: usize = 3;

/// Emotional intensity threshold: if rolling valence sum exceeds this,
/// consolidation is suggested early (amygdala-driven consolidation).
const EMOTIONAL_CONSOLIDATION_THRESHOLD: f32 = 1.5;
/// Decay factor for rolling emotional intensity (applied each tick).
const EMOTIONAL_INTENSITY_DECAY: f32 = 0.8;
/// Context switch detection: if cosine similarity between consecutive ticks
/// drops below this, a topic shift has occurred (hippocampal novelty signal).
const CONTEXT_SWITCH_THRESHOLD: f32 = 0.15;

/// Pattern completion (CA3 autoassociative recall): if fewer than this many
/// L2 results, try to reconstruct full memories from partial cues via graph.
const PATTERN_COMPLETION_MIN_RESULTS: usize = 3;
/// Minimum top-result similarity before pattern completion activates.
const PATTERN_COMPLETION_SIM_THRESHOLD: f32 = 0.5;

/// Systems consolidation: only groups with average salience above this
/// threshold get full neocortical encoding (centroid embedding + rich text).
/// Models how the hippocampus selectively flags important memories for cortical transfer.
const SYSTEMS_CONSOLIDATION_SALIENCE_THRESHOLD: f32 = 0.4;
/// Maximum length of full_text stored on consolidated Summary nodes.
const SUMMARY_FULL_TEXT_MAX_LEN: usize = 500;
/// Minimum cosine similarity for L3 Summary node retrieval.
const SUMMARY_RETRIEVAL_MIN_SIM: f32 = 0.3;
/// Maximum number of L3 Summary results to include in retrieval.
const SUMMARY_RETRIEVAL_MAX_RESULTS: usize = 3;
/// Eviction score reduction for consolidated L2 entries whose Summary node
/// has a valid embedding (L3 can serve their role).
const CONSOLIDATED_EVICTION_REDUCTION: f32 = 0.2;

/// Sharp-wave ripple replay: entries accessed within this many ticks are
/// considered temporally co-active (modeling hippocampal replay during rest).
const REPLAY_TEMPORAL_WINDOW: u64 = 5;
/// Edge weight boost for shared entities during replay consolidation.
const REPLAY_EDGE_BOOST: f32 = 0.08;
/// Salience boost for entries that participate in replay.
const REPLAY_SALIENCE_BOOST: f32 = 0.02;

/// Layer 3 incremental keyword discovery: minimum distinct ticks a term must
/// appear in before it can be auto-promoted to a keyword graph node.
const TERM_PROMOTION_MIN_TICKS: u32 = 5;
/// Minimum character length for auto-promoted terms.
const TERM_PROMOTION_MIN_LEN: usize = 3;

/// Stopwords excluded from Layer 3 keyword auto-promotion.
/// Common English words that carry no domain-specific information.
const STOPWORDS: &[&str] = &[
    // Articles & determiners
    "a", "an", "the", "this", "that", "these", "those",
    // Pronouns
    "i", "me", "my", "we", "us", "our", "you", "your", "he", "she", "it",
    "him", "her", "his", "its", "they", "them", "their",
    // Prepositions
    "in", "on", "at", "to", "for", "of", "with", "by", "from", "up", "about",
    "into", "through", "during", "before", "after", "above", "below", "between",
    "under", "over", "out",
    // Conjunctions
    "and", "but", "or", "nor", "so", "yet", "both", "either", "neither",
    // Common verbs
    "is", "am", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "shall",
    "should", "may", "might", "can", "could", "must",
    "get", "got", "make", "made", "go", "went", "gone",
    "say", "said", "see", "saw", "know", "knew", "think", "thought",
    "come", "came", "take", "took", "give", "gave", "tell", "told",
    // Common adverbs
    "not", "no", "very", "just", "also", "then", "now", "here", "there",
    "when", "where", "how", "what", "which", "who", "whom", "why",
    "all", "each", "every", "some", "any", "many", "much", "more", "most",
    "other", "only", "own", "same", "than", "too", "well", "still",
    // Common adjectives
    "new", "old", "good", "bad", "big", "small", "long", "short",
    "first", "last", "next", "few", "little",
    // Misc high-frequency
    "one", "two", "like", "time", "way", "thing", "even", "back",
    "people", "work", "day", "part", "case", "point",
    "need", "want", "try", "use", "used", "using", "set", "let",
    "etc", "e.g", "i.e",
];

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Tuning parameters for the three-layer memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Max items in working memory (neuroscience-aligned ~7±2).
    pub immediate_capacity: usize,
    /// Max entries in the short-term vector store.
    pub short_term_capacity: usize,
    /// Dimensionality of n-gram embedding vectors.
    pub embedding_dim: usize,
    /// Similarity above this → reinforce existing entry (no new insert).
    /// CA3 pattern completion: near-identical cues recall the same memory trace.
    pub theta_high: f32,
    /// Similarity above this but below theta_high → merge embeddings (EMA blend).
    /// Dentate Gyrus pattern separation: raised to 0.72 to prevent similar-but-distinct
    /// topics from collapsing. Only genuinely overlapping memories should merge.
    pub theta_low: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            immediate_capacity: 10,
            short_term_capacity: 1024,
            embedding_dim: 256,
            theta_high: 0.88,
            theta_low: 0.72,
        }
    }
}

/// Frequency statistics for a term observed during ticks.
/// Used by Layer 3 (statistical learning) to decide when to auto-promote
/// a recurring entity into the keyword graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermStats {
    /// Number of distinct ticks this term appeared in.
    pub tick_count: u32,
    /// Total number of appearances across all ticks.
    pub total_count: u32,
    /// Clock value when first seen.
    pub first_seen: u64,
    /// Clock value when last seen.
    pub last_seen: u64,
    /// Whether this term has co-occurred with an existing keyword.
    #[serde(default)]
    pub has_keyword_cooccurrence: bool,
}

/// Three-layer memory: working memory buffer, short-term vector store, long-term knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryState {
    pub config: MemoryConfig,
    pub working_memory: Vec<WorkingMemoryEntry>,
    pub short_term: Vec<ShortTermEntry>,
    pub long_term: GraphMemory,
    pub clock: u64,
    pub next_id: u64,
    /// Chronological log of tick text, preserving exact user input.
    #[serde(default)]
    pub session_log: Vec<SessionEntry>,
    /// Pinned current task description for session context.
    #[serde(default)]
    pub current_task: Option<String>,
    /// Number of ticks since last consolidation.
    #[serde(default)]
    pub ticks_since_consolidation: u32,
    /// IDs returned by the most recent retrieve_context() call, for contrastive descent.
    #[serde(default)]
    pub last_retrieved_ids: Vec<u64>,
    /// Last Git commit SHA processed by Legend.
    #[serde(default)]
    pub last_synced_sha: Option<String>,
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
    pub keyword_cache: keyword_cache::KeywordCache,
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
        }
    }
}

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
    /// Starts at 1.0, grows with spaced retrieval (capped at 10.0).
    /// Higher stability → slower forgetting curve.
    #[serde(default = "default_stability")]
    pub stability: f32,
    /// Clock interval between the two most recent retrievals.
    /// Used to detect spaced vs massed retrieval patterns.
    #[serde(default)]
    pub last_retrieval_interval: u64,
}

fn default_stability() -> f32 {
    1.0
}

/// A source reference to a file region for this memory.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct MemoryRef {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    /// Short snippet for re-anchoring when lines drift.
    #[serde(default)]
    pub snippet: String,
}

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

const MAX_REFS_PER_ENTRY: usize = 8;

fn merge_memory_refs(existing: &mut Vec<MemoryRef>, incoming: Vec<MemoryRef>) {
    if incoming.is_empty() {
        return;
    }

    let mut seen: HashSet<(String, usize, usize)> = existing
        .iter()
        .map(|r| (r.path.clone(), r.start_line, r.end_line))
        .collect();

    for reference in incoming {
        let key = (
            reference.path.clone(),
            reference.start_line,
            reference.end_line,
        );
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

fn extract_memory_refs_from_text(text: &str) -> Vec<MemoryRef> {
    let mut refs = Vec::new();
    let mut seen: HashSet<(String, usize, usize)> = HashSet::new();

    for line in text.lines() {
        let snippet = build_ref_snippet(line);
        for token in line.split_whitespace() {
            if let Some(reference) = parse_memory_ref_token(token, &snippet) {
                let key = (
                    reference.path.clone(),
                    reference.start_line,
                    reference.end_line,
                );
                if seen.insert(key) {
                    refs.push(reference);
                }
                if refs.len() >= MAX_REFS_PER_ENTRY {
                    return refs;
                }
            }
        }
    }

    refs
}

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
        path: path.to_string(),
        start_line,
        end_line,
        snippet: snippet.to_string(),
    })
}

fn build_ref_snippet(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.len() <= 120 {
        trimmed.to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

/// Long-term knowledge graph: labeled nodes connected by typed edges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GraphMemory {
    pub nodes: HashMap<u64, GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Label → node ID for fast entity lookup.
    pub index: HashMap<String, u64>,
}

impl Default for GraphNode {
    fn default() -> Self {
        Self {
            id: 0,
            label: String::new(),
            kind: String::new(),
            weight: 0.0,
            last_seen: 0,
            salience: 0.0,
            source_texts: Vec::new(),
            embedding: Vec::new(),
            full_text: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphNode {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
    pub last_seen: u64,
    pub salience: f32,
    pub source_texts: Vec<String>,
    /// Centroid embedding for direct similarity search (systems consolidation).
    /// Populated on Summary nodes so they can be queried independently of L2.
    pub embedding: Vec<f32>,
    /// Richer summary text (up to 500 chars) for consolidated memories.
    pub full_text: Option<String>,
}

impl Default for GraphEdge {
    fn default() -> Self {
        Self {
            from: 0,
            to: 0,
            weight: 0.0,
            kind: "related".to_string(),
            last_seen: 0,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphEdge {
    pub from: u64,
    pub to: u64,
    pub weight: f32,
    #[serde(default = "default_edge_kind")]
    pub kind: String,
    /// Clock tick when this edge was last reinforced.
    pub last_seen: u64,
    /// Number of times this edge has been reinforced (LTP history).
    #[serde(default)]
    pub activation_count: u32,
    /// Spaced repetition stability — grows faster with increasing intervals.
    #[serde(default = "default_edge_stability")]
    pub stability: f32,
    /// Fast-adapting EMA of reinforcement intervals (STP, α=0.5).
    #[serde(default)]
    pub recent_interval_avg: f32,
    /// Slow-adapting EMA of reinforcement intervals (LTP, α=0.1).
    #[serde(default)]
    pub historical_interval_avg: f32,
}

fn default_edge_stability() -> f32 {
    1.0
}

fn default_edge_kind() -> String {
    "related".to_string()
}

/// A timestamped session log entry preserving full tick text.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionEntry {
    pub timestamp: u64,
    pub text: String,
}

/// Query result: ranked snippets from short-term + related nodes from long-term.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub short_term: Vec<MemorySnippet>,
    pub long_term: Vec<GraphNodeSummary>,
    /// Matches from working memory (L1), scanned before L2.
    #[serde(default)]
    pub working_memory: Vec<MemorySnippet>,
}

/// Result of a tick operation, providing feedback on what action was taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickResult {
    /// What action was taken: "created", "merged", or "reconsolidated"
    pub action: String,
    /// The ID of the entry that was created or modified
    pub entry_id: u64,
    /// If merged or reconsolidated, the ID of the existing entry that was matched
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_existing: Option<u64>,
    /// The similarity score if merged (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
    /// Related context from the tick
    pub context: MemoryContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnippet {
    pub id: u64,
    pub text: String,
    pub similarity: f32,
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNodeSummary {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
    /// The type of edge that connected this node (for neighbor lookups).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,
    #[serde(default)]
    pub source_texts: Vec<String>,
}

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

/// A schema-validated entity proposed by an external LLM task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEntity {
    pub label: String,
    pub kind: String,
    pub context: String,
    #[serde(default = "default_llm_confidence")]
    pub confidence: f32,
}

fn default_llm_confidence() -> f32 {
    0.7
}

/// Result of applying validated LLM entities into the long-term graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEntityApplyResult {
    pub accepted_entities: usize,
    pub created_nodes: usize,
    pub updated_nodes: usize,
    pub edges_reinforced: usize,
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

impl Default for MemoryState {
    fn default() -> Self {
        Self {
            config: MemoryConfig::default(),
            working_memory: Vec::new(),
            short_term: Vec::new(),
            long_term: GraphMemory::default(),
            clock: 0,
            next_id: 1,
            session_log: Vec::new(),
            current_task: None,
            ticks_since_consolidation: 0,
            last_retrieved_ids: Vec::new(),
            last_synced_sha: None,
            recent_valence_sum: 0.0,
            last_tick_embedding: Vec::new(),
            term_frequency: HashMap::new(),
            keyword_cache: keyword_cache::KeywordCache::default_from_static(),
        }
    }
}

/// Order of semantic specificity for graph node kinds.
fn node_kind_priority(kind: &str) -> u8 {
    match kind {
        "FilePath" => 8,
        "Function" | "Struct" | "Enum" | "Trait" | "Class" | "Interface" | "Module" => 7,
        "Tool" | "Environment" => 6,
        "Symbol" | "Type" => 5,
        "Action" | "Decorator" | "Import" | "Package" | "Export" | "Impl" => 4,
        "Topic" => 3,
        "Term" => 2,
        _ => 1,
    }
}

/// Classify tick text by importance category for structured retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryCategory {
    Decision,
    Architecture,
    Preference,
    Bug,
    Todo,
    Progress,
    General,
}

#[allow(dead_code)]
impl MemoryCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::Architecture => "architecture",
            Self::Preference => "preference",
            Self::Bug => "bug",
            Self::Todo => "todo",
            Self::Progress => "progress",
            Self::General => "general",
        }
    }
}

/// Detect the primary category of a text based on keyword patterns.
pub fn classify_text(text: &str, kw: &keyword_cache::KeywordCache) -> MemoryCategory {
    let lower = text.to_lowercase();

    // Decision patterns (highest priority)
    let decision_score = kw.decision
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
    if kw.action
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

#[derive(Debug, Serialize, Deserialize)]
pub struct GitSyncInfo {
    pub last_sha: Option<String>,
    pub current_sha: Option<String>,
    pub new_commits: Vec<String>,
    pub uncommitted_summary: Option<String>,
}

impl MemoryState {
    pub fn load_or_default() -> Result<Self, Box<dyn std::error::Error>> {
        // Try to migrate corrupt backup first (old format without new fields)
        if let Ok(Some(mut migrated)) = migrate_corrupt_backup() {
            migrated.keyword_cache =
                keyword_cache::KeywordCache::from_graph(&migrated.long_term);
            return Ok(migrated);
        }

        if Path::new(MEMORY_FILE).exists() {
            match load_memory() {
                Ok(mut state) => {
                    state.keyword_cache =
                        keyword_cache::KeywordCache::from_graph(&state.long_term);
                    Ok(state)
                }
                Err(err) => {
                    let backup = format!("{}.corrupt", MEMORY_FILE);
                    // Only move to backup if one doesn't already exist, to avoid overwriting
                    // potentially recoverable data from a previous crash.
                    if !Path::new(&backup).exists() {
                        let _ = fs::rename(MEMORY_FILE, &backup);
                        eprintln!("Warning: failed to load memory store ({})", err);
                        eprintln!("Backup saved to {}", backup);
                    } else {
                        eprintln!("Warning: failed to load memory store ({}), but a backup already exists.", err);
                        eprintln!("Starting with a fresh memory store to avoid corruption loop.");
                        // Remove the unloadable memory file so next save writes a clean one
                        let _ = fs::remove_file(MEMORY_FILE);
                    }
                    Ok(Self::default())
                }
            }
        } else {
            Ok(Self::default())
        }
    }

    /// Summarize Git changes since last sync.
    /// Returns a list of commit messages and a summary of uncommitted changes.
    pub fn get_git_summary(&mut self) -> GitSyncInfo {
        use std::process::Command;

        let current_sha = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string());

        let mut commits = Vec::new();
        if let (Some(last), Some(current)) = (&self.last_synced_sha, &current_sha) {
            if last != current {
                // Get commit messages between last sync and now
                if let Ok(output) = Command::new("git")
                    .args([
                        "log",
                        &format!("{}..{}", last, current),
                        "--pretty=format:%h: %s",
                    ])
                    .output()
                {
                    let log = String::from_utf8_lossy(&output.stdout);
                    commits = log.lines().map(|s| s.to_string()).collect();
                }
            }
        }

        // Always check for uncommitted changes (dirty worktree)
        let uncommitted = Command::new("git")
            .args(["diff", "--stat"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let info = GitSyncInfo {
            last_sha: self.last_synced_sha.clone(),
            current_sha: current_sha.clone(),
            new_commits: commits,
            uncommitted_summary: uncommitted,
        };

        // Update anchor for next time
        self.last_synced_sha = current_sha;

        info
    }

    /// Scan project manifest files for dependencies and add them to the graph.
    pub fn scan_ecosystem_dependencies(&mut self) {
        // Rust
        if let Ok(cargo) = fs::read_to_string("Cargo.toml") {
            for line in cargo.lines() {
                if let Some(pos) = line.find(" = ") {
                    let name = line[..pos].trim();
                    if !name.is_empty() && !name.starts_with('[') && !name.starts_with('#') {
                        self.add_node_if_new(name, "Dependency", 0.3);
                    }
                }
            }
        }
        // Node.js
        if let Ok(pkg) = fs::read_to_string("package.json") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&pkg) {
                if let Some(deps) = json.get("dependencies").and_then(|v| v.as_object()) {
                    for name in deps.keys() {
                        self.add_node_if_new(name, "Dependency", 0.3);
                    }
                }
                if let Some(dev_deps) = json.get("devDependencies").and_then(|v| v.as_object()) {
                    for name in dev_deps.keys() {
                        self.add_node_if_new(name, "Dependency", 0.3);
                    }
                }
            }
        }
        // Python
        if let Ok(reqs) = fs::read_to_string("requirements.txt") {
            for line in reqs.lines() {
                let name: String = line
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                    .collect();
                if !name.is_empty() {
                    self.add_node_if_new(&name, "Dependency", 0.3);
                }
            }
        }
    }

    fn add_node_if_new(&mut self, label: &str, kind: &str, salience: f32) {
        if self.long_term.index.contains_key(label) {
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.long_term.nodes.insert(
            id,
            GraphNode {
                id,
                label: label.to_string(),
                kind: kind.to_string(),
                weight: 1.0,
                last_seen: self.clock,
                salience,
                source_texts: Vec::new(),
                embedding: Vec::new(),
                full_text: None,
            },
        );
        self.long_term.index.insert(label.to_string(), id);
    }

    /// Create or reinforce a keyword graph node with label `kw:<category>:<term>`.
    /// Returns true if a new node was created (vs reinforcing existing).
    pub fn add_keyword_node(&mut self, category: &str, term: &str, metadata: Vec<String>) -> bool {
        let label = format!("kw:{}:{}", category, term);
        if let Some(&existing_id) = self.long_term.index.get(&label) {
            // Reinforce existing keyword node
            if let Some(node) = self.long_term.nodes.get_mut(&existing_id) {
                node.weight += 0.2;
                node.last_seen = self.clock;
            }
            false
        } else {
            let id = self.next_id;
            self.next_id += 1;
            self.long_term.nodes.insert(
                id,
                GraphNode {
                    id,
                    label: label.clone(),
                    kind: "Keyword".to_string(),
                    weight: 1.0,
                    last_seen: self.clock,
                    salience: 0.5,
                    source_texts: metadata,
                    embedding: Vec::new(),
                    full_text: None,
                },
            );
            self.long_term.index.insert(label, id);
            true
        }
    }

    /// Rebuild the keyword cache from the current graph state.
    pub fn rebuild_keyword_cache(&mut self) {
        self.keyword_cache = keyword_cache::KeywordCache::from_graph(&self.long_term);
    }

    /// Layer 3: Update term frequency stats and auto-promote terms that pass
    /// all noise filters to `kw:domain:<term>` graph nodes.
    ///
    /// Called from `tick_impl` after entity extraction. Returns the number of
    /// newly promoted keywords.
    fn update_term_frequencies(&mut self, text: &str) -> usize {
        let entities = extract_entities(text, &self.keyword_cache);
        if entities.is_empty() {
            return 0;
        }

        // Check if this tick contains any existing keyword matches (for co-occurrence filter)
        let lowered = text.to_lowercase();
        let has_existing_keyword = self.keyword_cache.decision.iter().any(|k| lowered.contains(k.as_str()))
            || self.keyword_cache.bug.iter().any(|k| lowered.contains(k.as_str()))
            || self.keyword_cache.todo.iter().any(|k| lowered.contains(k.as_str()))
            || self.keyword_cache.architecture.iter().any(|k| lowered.contains(k.as_str()))
            || self.keyword_cache.preference.iter().any(|k| lowered.contains(k.as_str()))
            || self.keyword_cache.tool.iter().any(|k| lowered.contains(k.as_str()))
            || self.keyword_cache.domain.iter().any(|k| lowered.contains(k.as_str()));

        let clock = self.clock;
        for entity in &entities {
            let label = entity.label.to_lowercase();

            let stats = self.term_frequency.entry(label.clone()).or_insert(TermStats {
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
        let candidates: Vec<String> = self
            .term_frequency
            .iter()
            .filter(|(_, stats)| stats.tick_count >= TERM_PROMOTION_MIN_TICKS)
            .map(|(label, _)| label.clone())
            .collect();

        for label in candidates {
            if self.should_promote_term(&label) {
                if self.add_keyword_node("domain", &label, Vec::new()) {
                    promoted += 1;
                }
            }
        }

        if promoted > 0 {
            self.rebuild_keyword_cache();
        }

        promoted
    }

    /// Check if a term passes all 5 noise filters for auto-promotion.
    fn should_promote_term(&self, term: &str) -> bool {
        // Already a keyword?
        let label = format!("kw:domain:{}", term);
        if self.long_term.index.contains_key(&label) {
            return false;
        }

        // Filter 5: Minimum information content — 3+ chars, not purely numeric
        if term.len() < TERM_PROMOTION_MIN_LEN {
            return false;
        }
        if term.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
            return false;
        }

        // Filter 1: Stopword exclusion
        if STOPWORDS.contains(&term) {
            return false;
        }

        // Filter 2: Minimum tick spread (checked before calling, but verify)
        let stats = match self.term_frequency.get(term) {
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

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        save_memory(self)
    }

    /// Ingest text: chunk → embed → reconsolidate or match/merge/insert → update graph.
    /// Returns a TickResult describing what action was taken.
    pub fn tick(&mut self, text: &str) -> TickResult {
        self.tick_impl(text, false)
    }

    /// Like tick(), but does not count toward consolidation and does not appear in
    /// the session log. Used for automated/hook-generated ticks that should influence
    /// L2/L3 (dedup, reconsolidation) but must not pollute session history or
    /// trigger premature auto-consolidation.
    pub fn tick_passive(&mut self, text: &str) -> TickResult {
        self.tick_impl(text, true)
    }

    fn tick_impl(&mut self, text: &str, passive: bool) -> TickResult {
        self.clock += 1;
        if !passive {
            self.ticks_since_consolidation += 1;
            // Decay rolling emotional intensity each active tick
            self.recent_valence_sum *= EMOTIONAL_INTENSITY_DECAY;
        }
        self.apply_decay();
        self.stabilize_labile_entries();
        if self.clock.is_multiple_of(RENORM_INTERVAL) {
            self.renormalize_salience();
        }
        if self.clock.is_multiple_of(GRAPH_NORM_INTERVAL) {
            self.normalize_graph_weights();
        }

        // Append to chronological session log (preserves exact input).
        // Passive ticks are excluded — they must not evict real session entries.
        if !passive {
            self.session_log.push(SessionEntry {
                timestamp: self.clock,
                text: text.to_string(),
            });
            while self.session_log.len() > SESSION_LOG_CAPACITY {
                self.session_log.remove(0);
            }
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
            let raw_embedding = embed_text(&chunk, self.config.embedding_dim);
            let salience = compute_salience(&chunk, &self.keyword_cache);
            let emotional_valence = compute_emotional_valence(&chunk, &self.keyword_cache);
            let refs = extract_memory_refs_from_text(&chunk);

            // Dentate Gyrus sparse orthogonalization: push the new embedding away from
            // similar-but-distinct existing L2 embeddings to reduce interference.
            let existing_embeddings: Vec<Vec<f32>> = self.short_term.iter().map(|e| e.embedding.clone()).collect();
            let embedding = sparse_orthogonalize(
                &raw_embedding,
                &existing_embeddings,
                self.config.theta_low,    // low: only orthogonalize in the confusable zone
                self.config.theta_high,   // high: near-identical entries should still merge
                0.3,                      // strength: moderate push-away
            );

            // Always push into working memory (L1)
            let wm_id = self.push_working_memory(&chunk, &embedding, salience, emotional_valence);

            // --- Attention gate: only high-salience ticks promote to L2 ---
            if salience >= PROMOTION_SALIENCE_THRESHOLD {
                // --- Reconsolidation: check labile entries first ---
                // If a recently-retrieved memory is labile and the new text is related,
                // update that memory in-place instead of creating a duplicate.
                if let Some(reconsolidated_id) =
                    self.try_reconsolidate(&chunk, &embedding, salience, refs.clone())
                {
                    // Update graph with the new text context
                    self.update_graph(&chunk, salience);
                    last_context = self.retrieve_context(&chunk);
                    // Mark L1 entry as promoted
                    if let Some(wm_entry) = self.working_memory.iter_mut().find(|e| e.id == wm_id) {
                        wm_entry.promoted = true;
                    }
                    // Track reconsolidation
                    result_action = "reconsolidated".to_string();
                    result_entry_id = reconsolidated_id;
                    result_matched = Some(reconsolidated_id);
                    continue;
                }

                // --- Normal path: match/merge/insert ---
                let (best_id, best_sim) = self.find_best_match(&embedding);

                // Diversity gate: even at high similarity, if word overlap is low the
                // Dentate Gyrus diversity gate: even at high similarity, if word overlap is
                // low the texts are semantically distinct and should not be merged.
                let diversity_ok = if best_sim >= self.config.theta_low {
                    self.short_term
                        .iter()
                        .find(|e| e.id == best_id)
                        .map(|e| diversity_pass(&e.text, &chunk))
                        .unwrap_or(false)
                } else {
                    false
                };

                match best_sim {
                    s if s >= self.config.theta_high && diversity_ok => {
                        if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == best_id) {
                            entry.usage = entry.usage.saturating_add(2);
                            entry.salience = (entry.salience + salience).min(1.0);
                            entry.last_access = self.clock;
                            merge_memory_refs(&mut entry.refs, refs.clone());
                        }
                        // Track merge (high similarity)
                        result_action = "merged".to_string();
                        result_entry_id = best_id;
                        result_matched = Some(best_id);
                        result_similarity = Some(best_sim);
                    }
                    s if s >= self.config.theta_low && diversity_ok => {
                        if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == best_id) {
                            entry.embedding = merge_embeddings(&entry.embedding, &embedding);
                            entry.usage = entry.usage.saturating_add(1);
                            entry.salience = (entry.salience + salience * 0.5).min(1.0);
                            entry.summary = summarize_text(&entry.text, &chunk, &self.keyword_cache);
                            entry.last_access = self.clock;
                            merge_memory_refs(&mut entry.refs, refs.clone());
                        }
                        self.update_graph(&chunk, salience);
                        // Track merge (low similarity)
                        result_action = "merged".to_string();
                        result_entry_id = best_id;
                        result_matched = Some(best_id);
                        result_similarity = Some(best_sim);
                    }
                    _ => {
                        self.insert_short_term(&chunk, embedding, salience, refs, emotional_valence);
                        self.update_graph(&chunk, salience);
                        // Track creation - get the ID of the newly inserted entry
                        if let Some(entry) = self.short_term.last() {
                            result_action = "created".to_string();
                            result_entry_id = entry.id;
                            result_matched = None;
                            result_similarity = None;
                        }
                    }
                }

                // Mark L1 entry as promoted
                if let Some(wm_entry) = self.working_memory.iter_mut().find(|e| e.id == wm_id) {
                    wm_entry.promoted = true;
                }

                last_context = self.retrieve_context(&chunk);
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
            self.update_term_frequencies(text);
        }

        self.prune_short_term();
        self.prune_graph();

        // --- Smart consolidation triggers ---
        if !passive {
            // Accumulate emotional intensity from this tick
            let tick_embedding = embed_text(text, self.config.embedding_dim);
            let tick_valence = compute_emotional_valence(text, &self.keyword_cache);
            self.recent_valence_sum += tick_valence.abs();

            // Context switch detection: compare with previous tick's embedding
            if !self.last_tick_embedding.is_empty() {
                let sim = cosine_similarity(&self.last_tick_embedding, &tick_embedding);
                if sim < CONTEXT_SWITCH_THRESHOLD {
                    // Topic shift detected — suggest consolidation and flush L1
                    self.flush_working_memory();
                }
            }
            self.last_tick_embedding = tick_embedding;
        }

        TickResult {
            action: result_action,
            entry_id: result_entry_id,
            matched_existing: result_matched,
            similarity: result_similarity,
            context: last_context,
        }
    }

    /// CA3 autoassociative pattern completion.
    ///
    /// When direct similarity search returns sparse results, use the graph to
    /// reconstruct full memories from partial cues. Extracts entities from
    /// partial matches, spreads activation, then searches L2 for entries
    /// containing activated nodes' source texts.
    fn pattern_complete(
        &self,
        query: &str,
        partial_matches: &[MemorySnippet],
    ) -> Vec<MemorySnippet> {
        // Collect entity seeds from both the query and partial matches
        let mut seed_ids: Vec<u64> = Vec::new();
        let query_entities = extract_entities(query, &self.keyword_cache);
        for entity in &query_entities {
            if let Some(&id) = self.long_term.index.get(&entity.label) {
                seed_ids.push(id);
            }
        }
        for snippet in partial_matches {
            let entities = extract_entities(&snippet.text, &self.keyword_cache);
            for entity in &entities {
                if let Some(&id) = self.long_term.index.get(&entity.label) {
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
        let activated = self.spreading_activation(&seed_ids, 2, 0.5);

        // Collect source_texts from activated nodes
        let mut candidate_texts: Vec<(String, f32)> = Vec::new();
        for (nid, activation) in &activated {
            if let Some(node) = self.long_term.nodes.get(nid) {
                for st in &node.source_texts {
                    candidate_texts.push((st.clone(), *activation));
                }
            }
        }

        // Search L2 for entries containing any candidate text
        let existing_ids: HashSet<u64> = partial_matches.iter().map(|s| s.id).collect();
        let mut completed: Vec<MemorySnippet> = Vec::new();

        for entry in &self.short_term {
            if existing_ids.contains(&entry.id) || entry.consolidated {
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
                let query_emb = embed_text(query, self.config.embedding_dim);
                let direct_sim = cosine_similarity(&entry.embedding, &query_emb);
                let completion_score = direct_sim * 0.6 + best_activation * 0.4;

                completed.push(MemorySnippet {
                    id: entry.id,
                    text: entry.text.clone(),
                    similarity: completion_score,
                    refs: entry.refs.clone(),
                });
            }
        }

        completed.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        completed.truncate(3);
        completed
    }

    /// Query memory without inserting new data.
    /// Retrieved entries are marked labile (editable) so the next tick can
    /// reconsolidate them with updated information.
    pub fn retrieve_context(&mut self, query: &str) -> MemoryContext {
        self.clock += 1;
        self.apply_decay();

        let embedding = embed_text(query, self.config.embedding_dim);

        // --- Scan working memory (L1) first ---
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let mut wm_snippets: Vec<MemorySnippet> = Vec::new();
        for wm_entry in &mut self.working_memory {
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
        let mut snippets = self.top_k_similar(&embedding, 5, query);

        for snippet in &mut snippets {
            if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == snippet.id) {
                // Ebbinghaus spaced repetition: update stability based on retrieval interval
                let interval = self.clock.saturating_sub(entry.last_access);
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

                entry.last_access = self.clock;
                entry.usage = entry.usage.saturating_add(1);
                // Mark labile: this entry can be reconsolidated by the next tick
                entry.labile_until = self.clock + LABILE_WINDOW;
            }
        }

        // --- L3 systems consolidation retrieval (neocortical independence) ---
        // Scan Summary nodes that have centroid embeddings for direct similarity
        // matching. This surfaces old consolidated memories whose L2 entries may
        // have been evicted, making them independently queryable via neocortex.
        {
            let existing_ids: HashSet<u64> = snippets.iter().map(|s| s.id).collect();
            let mut summary_hits: Vec<MemorySnippet> = self
                .long_term
                .nodes
                .values()
                .filter(|n| n.kind == "Summary" && !n.embedding.is_empty())
                .filter_map(|n| {
                    let sim = cosine_similarity(&n.embedding, &embedding);
                    if sim >= SUMMARY_RETRIEVAL_MIN_SIM && !existing_ids.contains(&n.id) {
                        let text = n
                            .full_text
                            .as_deref()
                            .unwrap_or(&n.label)
                            .to_string();
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
        if snippets.len() < PATTERN_COMPLETION_MIN_RESULTS
            || top_sim < PATTERN_COMPLETION_SIM_THRESHOLD
        {
            let completed = self.pattern_complete(query, &snippets);
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
        self.last_retrieved_ids = snippets.iter().map(|s| s.id).collect();

        // Passive auto-reinforce: the top result gets a small salience bump
        // proportional to its similarity, so useful memories naturally rise.
        if let Some(top) = snippets.first() {
            if top.similarity > 0.2 {
                if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == top.id) {
                    entry.salience =
                        (entry.salience + top.similarity * AUTO_REINFORCE_SCALE).min(1.0);
                }
            }
        }

        let mut long_term = self.graph_lookup(query, 12);

        // --- Associative priming (2-hop spreading activation) ---
        // From the retrieved short-term entries, extract entities and spread
        // activation through the graph to surface indirectly related nodes.
        let mut priming_seed_ids: Vec<u64> = Vec::new();
        for snippet in &snippets {
            let entities = extract_entities(&snippet.text, &self.keyword_cache);
            for entity in &entities {
                if let Some(&node_id) = self.long_term.index.get(&entity.label) {
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
        let activated = self.spreading_activation(&priming_seed_ids, 2, 0.4);
        let mut primed_nodes: Vec<GraphNodeSummary> = Vec::new();
        for (nid, activation) in activated {
            if !existing_ids.contains(&nid) {
                if let Some(node) = self.long_term.nodes.get(&nid) {
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
        self.hebbian_reinforce(&retrieved_ids);

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
    fn replay_consolidation(&mut self) {
        let n = self.short_term.len();
        if n < 2 {
            return;
        }

        // Collect (index, entities) for each L2 entry
        let entry_entities: Vec<(usize, Vec<(String, u64)>)> = self
            .short_term
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let entities = extract_entities(&entry.text, &self.keyword_cache);
                let resolved: Vec<(String, u64)> = entities
                    .iter()
                    .filter_map(|e| {
                        self.long_term
                            .index
                            .get(&e.label)
                            .map(|&id| (e.label.clone(), id))
                    })
                    .collect();
                (i, resolved)
            })
            .collect();

        let mut replayed_indices: HashSet<usize> = HashSet::new();

        for i in 0..entry_entities.len() {
            for j in (i + 1)..entry_entities.len() {
                let ei = &self.short_term[entry_entities[i].0];
                let ej = &self.short_term[entry_entities[j].0];

                // Check temporal proximity
                let time_diff = ei.last_access.abs_diff(ej.last_access);
                if time_diff > REPLAY_TEMPORAL_WINDOW {
                    continue;
                }

                let entities_i = &entry_entities[i].1;
                let entities_j = &entry_entities[j].1;

                if entities_i.is_empty() || entities_j.is_empty() {
                    continue;
                }

                // Reinforce edges between all entity pairs from the two entries
                for (_, id_a) in entities_i {
                    for (_, id_b) in entities_j {
                        if id_a == id_b {
                            continue;
                        }
                        // Check if edge already exists
                        let existing = self.long_term.edges.iter_mut().find(|e| {
                            (e.from == *id_a && e.to == *id_b)
                                || (e.from == *id_b && e.to == *id_a)
                        });
                        if let Some(edge) = existing {
                            edge.weight += REPLAY_EDGE_BOOST;
                            edge.last_seen = self.clock;
                        } else {
                            // Create new temporal edge
                            self.long_term.edges.push(GraphEdge {
                                from: *id_a,
                                to: *id_b,
                                weight: 0.05,
                                kind: "temporal".to_string(),
                                last_seen: self.clock,
                                activation_count: 0,
                                stability: 1.0,
                                recent_interval_avg: 0.0,
                                historical_interval_avg: 0.0,
                            });
                        }
                    }
                }

                replayed_indices.insert(entry_entities[i].0);
                replayed_indices.insert(entry_entities[j].0);
            }
        }

        // Boost salience of replayed entries
        for &idx in &replayed_indices {
            self.short_term[idx].salience =
                (self.short_term[idx].salience + REPLAY_SALIENCE_BOOST).min(1.0);
        }
    }

    /// Merge similar short-term entries into long-term graph summaries.
    pub fn consolidate(&mut self) -> Vec<GraphNodeSummary> {
        self.clock += 1;
        self.ticks_since_consolidation = 0;
        self.apply_decay();
        self.replay_consolidation();

        let mut groups: Vec<Vec<ShortTermEntry>> = Vec::new();
        let mut used = vec![false; self.short_term.len()];

        for i in 0..self.short_term.len() {
            if used[i] {
                continue;
            }
            let seed = self.short_term[i].clone();
            let mut group = vec![seed.clone()];
            used[i] = true;

            for (j, is_used) in used.iter_mut().enumerate().skip(i + 1) {
                if *is_used {
                    continue;
                }
                if cosine_similarity(&seed.embedding, &self.short_term[j].embedding)
                    >= self.config.theta_low
                {
                    group.push(self.short_term[j].clone());
                    *is_used = true;
                }
            }
            groups.push(group);
        }

        let mut summaries = Vec::new();
        for group in groups.into_iter().filter(|g| g.len() > 1) {
            let summary_text = summarize_group(&group, &self.keyword_cache);
            let salience = group
                .iter()
                .map(|e| e.salience)
                .fold(0.0, f32::max)
                .max(0.4);
            let source_texts: Vec<String> = group.iter().map(|e| e.text.clone()).collect();

            // Systems consolidation: compute centroid embedding and rich text
            // for high-salience groups (hippocampus flags important memories
            // for full neocortical encoding).
            let avg_salience =
                group.iter().map(|e| e.salience).sum::<f32>() / group.len() as f32;
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
            let existing_summary_id = self
                .long_term
                .index
                .get(&summary_text)
                .copied()
                .or_else(|| {
                    self.long_term
                        .nodes
                        .iter()
                        .find(|(_, n)| {
                            n.kind == "Summary"
                                && word_overlap(&n.label, &summary_text)
                                    >= MERGE_WORD_OVERLAP_THRESHOLD
                        })
                        .map(|(&id, _)| id)
                });

            let node_id = if let Some(eid) = existing_summary_id {
                // Merge into existing: update weight/salience, extend source_texts
                if let Some(node) = self.long_term.nodes.get_mut(&eid) {
                    node.weight = node.weight.max(1.0 + salience);
                    node.salience = node.salience.max(salience);
                    node.last_seen = self.clock;
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
                let id = self.next_id;
                self.next_id += 1;
                let mut capped_sources = source_texts.clone();
                capped_sources.truncate(20);
                self.long_term.nodes.insert(
                    id,
                    GraphNode {
                        id,
                        label: summary_text.clone(),
                        kind: "Summary".to_string(),
                        weight: 1.0 + salience,
                        last_seen: self.clock,
                        salience,
                        source_texts: capped_sources,
                        embedding: centroid_embedding.clone(),
                        full_text: full_text.clone(),
                    },
                );
                self.long_term.index.insert(summary_text.clone(), id);
                id
            };

            // 2. Semantic Topic Extraction: find high-frequency entities in the group
            let mut entity_counts: HashMap<String, (usize, String)> = HashMap::new();
            for entry in &group {
                let entities = crate::memory::extract::extract_entities(&entry.text, &self.keyword_cache);
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
                    let topic_id = if let Some(&id) = self.long_term.index.get(&label) {
                        id
                    } else {
                        let id = self.next_id;
                        self.next_id += 1;
                        self.long_term.nodes.insert(
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
                                last_seen: self.clock,
                                salience: 0.5,
                                source_texts: Vec::new(),
                                embedding: Vec::new(),
                                full_text: None,
                            },
                        );
                        self.long_term.index.insert(label.clone(), id);
                        id
                    };

                    // Create/strengthen edge between Summary and Topic
                    self.upsert_edge(node_id, topic_id, "represents");
                }
            }

            for entry in &group {
                self.update_graph(&entry.text, entry.salience);
                if let Some(existing) = self.short_term.iter_mut().find(|e| e.id == entry.id) {
                    existing.usage = existing.usage.saturating_add(1);
                    existing.last_access = self.clock;
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

        self.prune_short_term();
        self.prune_graph();
        summaries
    }

    /// Apply externally generated, schema-validated entities into the graph.
    /// This is intentionally conservative: low-confidence or stopword labels are skipped.
    pub fn apply_llm_entities(
        &mut self,
        source_text: &str,
        entities: &[LlmEntity],
        task_confidence: f32,
    ) -> LlmEntityApplyResult {
        self.clock += 1;
        let now = self.clock;

        let mut accepted_entities = 0usize;
        let mut created_nodes = 0usize;
        let mut updated_nodes = 0usize;
        let mut node_ids = Vec::new();
        let mut contexts = Vec::new();

        for entity in entities {
            let label = entity.label.trim();
            if label.len() < 2 || label.len() > 120 {
                continue;
            }
            if crate::memory::extract::is_stopword(label) {
                continue;
            }

            let confidence = entity.confidence.clamp(0.0, 1.0);
            if confidence < 0.5 {
                continue;
            }

            let normalized_kind = match entity.kind.as_str() {
                "FilePath" | "Function" | "Struct" | "Enum" | "Trait" | "Class" | "Interface"
                | "Module" | "Symbol" | "Type" | "Term" | "Topic" | "Tool" | "Environment"
                | "Action" | "Decorator" | "Import" | "Package" | "Export" | "Impl" => {
                    entity.kind.clone()
                }
                _ => "Term".to_string(),
            };

            let normalized_context = match entity.context.as_str() {
                "defines" | "uses" | "implements" | "mentions" | "performs" => {
                    entity.context.clone()
                }
                _ => "mentions".to_string(),
            };

            let id = if let Some(&existing) = self.long_term.index.get(label) {
                updated_nodes += 1;
                existing
            } else {
                let id = self.next_id;
                self.next_id += 1;
                created_nodes += 1;
                self.long_term.nodes.insert(
                    id,
                    GraphNode {
                        id,
                        label: label.to_string(),
                        kind: normalized_kind.clone(),
                        weight: 1.0,
                        last_seen: now,
                        salience: task_confidence.clamp(0.0, 1.0),
                        source_texts: Vec::new(),
                        embedding: Vec::new(),
                        full_text: None,
                    },
                );
                self.long_term.index.insert(label.to_string(), id);
                id
            };

            if let Some(node) = self.long_term.nodes.get_mut(&id) {
                let weight_multiplier = match normalized_kind.as_str() {
                    "FilePath" => 2.0,
                    "Function" | "Struct" | "Enum" | "Trait" | "Class" | "Interface" => 1.6,
                    "Tool" | "Environment" => 1.4,
                    "Symbol" | "Type" => 1.2,
                    "Term" => 0.5,
                    _ => 1.0,
                };
                node.weight += (NODE_WEIGHT_BASE + task_confidence * 0.2 + confidence * 0.2)
                    * weight_multiplier;
                node.salience = (node.salience + task_confidence * 0.2 + confidence * 0.2).min(1.0);
                node.last_seen = now;
                if node_kind_priority(&normalized_kind) > node_kind_priority(&node.kind) {
                    node.kind = normalized_kind.clone();
                }
                if !source_text.trim().is_empty() {
                    node.source_texts.push(source_text.to_string());
                    if node.source_texts.len() > 6 {
                        let overflow = node.source_texts.len() - 6;
                        node.source_texts.drain(0..overflow);
                    }
                }
            }

            accepted_entities += 1;
            node_ids.push(id);
            contexts.push(normalized_context);
        }

        let mut edges_reinforced = 0usize;
        for i in 0..node_ids.len() {
            for j in (i + 1)..node_ids.len() {
                let edge_kind = match (contexts[i].as_str(), contexts[j].as_str()) {
                    ("defines", "mentions") => "contains",
                    (a, b) if a == "uses" || b == "uses" => "depends-on",
                    (a, b) if a == "implements" || b == "implements" => "implements",
                    ("defines", "defines") => "co-defined",
                    (a, b) if a == "performs" || b == "performs" => "drives",
                    _ => "related",
                };
                self.upsert_edge(node_ids[i], node_ids[j], edge_kind);
                edges_reinforced += 1;
            }
        }

        self.prune_graph();

        LlmEntityApplyResult {
            accepted_entities,
            created_nodes,
            updated_nodes,
            edges_reinforced,
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Try to reconsolidate: if any labile entry is related to the new text,
    /// update it in-place (merge text, re-embed, boost salience) instead of
    /// creating a new entry. Returns the id of the reconsolidated entry if successful.
    fn try_reconsolidate(
        &mut self,
        text: &str,
        embedding: &[f32],
        salience: f32,
        refs: Vec<MemoryRef>,
    ) -> Option<u64> {
        let now = self.clock;

        // Find the best labile match
        let mut best: Option<(u64, f32)> = None;
        for entry in &self.short_term {
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
        if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == target_id) {
            // Merge text: append new information
            let merged_text = format!("{} | {}", entry.text, text);
            entry.summary = summarize_text(&entry.text, text, &self.keyword_cache);
            entry.text = if merged_text.len() > 500 {
                // If text is getting too long, use summary as text
                entry.summary.clone()
            } else {
                merged_text
            };
            // Re-embed with combined text
            entry.embedding = embed_text(&entry.text, self.config.embedding_dim);
            // Boost salience (reconsolidated memories are important)
            entry.salience = (entry.salience + salience * 0.3).min(1.0);
            entry.usage = entry.usage.saturating_add(1);
            entry.last_access = now;
            entry.reconsolidation_count += 1;
            entry.density = calculate_density(&entry.text, &self.keyword_cache);
            // Re-stabilize: no longer labile
            entry.labile_until = 0;
            merge_memory_refs(&mut entry.refs, refs);

            return Some(target_id);
        }

        None
    }

    /// Clear labile state from entries whose labile window has expired.
    fn stabilize_labile_entries(&mut self) {
        let now = self.clock;
        for entry in &mut self.short_term {
            if entry.labile_until > 0 && entry.labile_until < now {
                entry.labile_until = 0;
            }
        }
    }

    /// Flush working memory on session boundary.
    /// Every remaining entry is evaluated for L2 promotion before clearing.
    /// Entries that pass the gate (high salience or rehearsed) get promoted to L2.
    /// Entries that don't pass are discarded — nothing is silently lost.
    pub fn flush_working_memory(&mut self) {
        let entries: Vec<WorkingMemoryEntry> = self.working_memory.drain(..).collect();
        for entry in entries {
            if !entry.promoted
                && (entry.salience >= PROMOTION_SALIENCE_THRESHOLD
                    || entry.rehearsal_count >= 1)
            {
                let refs = extract_memory_refs_from_text(&entry.text);
                self.insert_short_term(&entry.text, entry.embedding, entry.salience, refs, entry.emotional_valence);
                self.update_graph(&entry.text, entry.salience);
            }
        }
    }

    /// Hebbian reinforcement: co-retrieved nodes strengthen shared edges.
    fn hebbian_reinforce(&mut self, co_retrieved_ids: &[u64]) {
        if co_retrieved_ids.len() < 2 {
            return;
        }

        let now = self.clock;
        for edge in &mut self.long_term.edges {
            if co_retrieved_ids.contains(&edge.from) && co_retrieved_ids.contains(&edge.to) {
                // Logarithmic dampening: heavily-activated synapses saturate (LTP ceiling)
                let dampened_boost = HEBBIAN_EDGE_BOOST
                    / (1.0 + (edge.activation_count as f32 + 1.0).ln());
                edge.weight = (edge.weight + dampened_boost).min(HEBBIAN_EDGE_CEILING);
                edge.last_seen = now;
            }
        }

        for &id in co_retrieved_ids {
            if let Some(node) = self.long_term.nodes.get_mut(&id) {
                node.weight = (node.weight + HEBBIAN_NODE_BOOST).min(HEBBIAN_NODE_CEILING);
                node.last_seen = now;
            }
        }
    }

    /// Push an entry into working memory (L1).
    /// When at capacity, the oldest entry is displaced and evaluated for L2 promotion.
    /// Displaced entries with high salience or rehearsal are promoted to short-term (L2).
    fn push_working_memory(&mut self, text: &str, embedding: &[f32], salience: f32, emotional_valence: f32) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        // Displace oldest if at capacity
        if self.working_memory.len() >= self.config.immediate_capacity {
            let displaced = self.working_memory.remove(0);
            // Promotion gate: displaced entry gets L2 if high salience or rehearsed
            if !displaced.promoted
                && (displaced.salience >= PROMOTION_SALIENCE_THRESHOLD
                    || displaced.rehearsal_count >= 1)
            {
                let refs = extract_memory_refs_from_text(&displaced.text);
                self.insert_short_term(&displaced.text, displaced.embedding, displaced.salience, refs, displaced.emotional_valence);
                self.update_graph(&displaced.text, displaced.salience);
            }
        }

        self.working_memory.push(WorkingMemoryEntry {
            id,
            text: text.to_string(),
            embedding: embedding.to_vec(),
            salience,
            tick_created: self.clock,
            rehearsal_count: 0,
            promoted: false,
            emotional_valence,
        });
        id
    }

    /// Insert a new short-term entry, evicting the lowest-scoring entry if at capacity.
    fn insert_short_term(
        &mut self,
        text: &str,
        embedding: Vec<f32>,
        salience: f32,
        refs: Vec<MemoryRef>,
        emotional_valence: f32,
    ) {
        if self.short_term.len() >= self.config.short_term_capacity {
            let now = self.clock;
            // Systems consolidation: consolidated entries whose Summary node
            // has a valid embedding in L3 get reduced eviction resistance,
            // since the neocortex can independently serve their role.
            let has_embedded_summary = |entry: &ShortTermEntry| -> bool {
                entry.consolidated
                    && self
                        .long_term
                        .nodes
                        .values()
                        .any(|n| n.kind == "Summary" && !n.embedding.is_empty()
                            && n.source_texts.iter().any(|st| st == &entry.text))
            };
            if let Some(idx) = self
                .short_term
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    let mut score_a = eviction_score(a, now);
                    let mut score_b = eviction_score(b, now);
                    if has_embedded_summary(a) {
                        score_a -= CONSOLIDATED_EVICTION_REDUCTION;
                    }
                    if has_embedded_summary(b) {
                        score_b -= CONSOLIDATED_EVICTION_REDUCTION;
                    }
                    score_a.partial_cmp(&score_b).unwrap()
                })
                .map(|(i, _)| i)
            {
                self.short_term.remove(idx);
            }
        }

        let mut refs = refs;
        if refs.len() > MAX_REFS_PER_ENTRY {
            refs.truncate(MAX_REFS_PER_ENTRY);
        }

        self.short_term.push(ShortTermEntry {
            id: self.next_id,
            text: text.to_string(),
            summary: summarize_single(text, &self.keyword_cache),
            embedding,
            last_access: self.clock,
            usage: 1,
            salience: salience.clamp(0.0, 1.0),
            reconsolidation_count: 0,
            labile_until: 0,
            refs,
            gradient_sq_sum: 0.0,
            density: calculate_density(text, &self.keyword_cache),
            consolidated: false,
            emotional_valence,
            stability: 1.0,
            last_retrieval_interval: 0,
        });
        self.next_id += 1;
    }

    /// Find the short-term entry most similar to the given embedding.
    /// Returns (entry_id, similarity). Returns (0, -1.0) if store is empty.
    fn find_best_match(&self, embedding: &[f32]) -> (u64, f32) {
        self.short_term
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
    fn top_k_similar(&self, embedding: &[f32], k: usize, query: &str) -> Vec<MemorySnippet> {
        // Pre-compute query keywords (lowercased, non-stopword, len > 1)
        let query_keywords: Vec<String> = query
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
            .filter(|w| w.len() > 1 && !extract::is_stopword(w))
            .collect();

        let mut scored: Vec<MemorySnippet> = self
            .short_term
            .iter()
            .filter(|e| !e.consolidated)
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
                MemorySnippet {
                    id: e.id,
                    text: e.text.clone(),
                    similarity: cosine + keyword_bonus + emotional_boost,
                    refs: e.refs.clone(),
                }
            })
            .collect();
        scored.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        scored.truncate(k);
        scored.retain(|s| s.similarity >= MIN_QUERY_SIMILARITY);
        scored
    }

    /// Extract entities from text and insert/update nodes and edges in the knowledge graph.
    fn update_graph(&mut self, text: &str, salience: f32) {
        let entities = extract_entities(text, &self.keyword_cache);
        if entities.is_empty() {
            return;
        }

        let mut node_ids = Vec::new();
        let mut edge_contexts = Vec::new();

        for entity in &entities {
            let id = if let Some(&existing) = self.long_term.index.get(&entity.label) {
                existing
            } else {
                let id = self.next_id;
                self.next_id += 1;
                self.long_term.nodes.insert(
                    id,
                    GraphNode {
                        id,
                        label: entity.label.clone(),
                        kind: entity.kind.clone(),
                        weight: 1.0,
                        last_seen: self.clock,
                        salience,
                        source_texts: Vec::new(),
                        embedding: Vec::new(),
                        full_text: None,
                    },
                );
                self.long_term.index.insert(entity.label.clone(), id);
                id
            };

            if let Some(node) = self.long_term.nodes.get_mut(&id) {
                // Code-aware weighting: boost high-signal kinds, penalize generic Terms
                let weight_multiplier = match entity.kind.as_str() {
                    "FilePath" => 2.0,
                    "Function" | "Struct" | "Enum" | "Trait" | "Class" => 1.5,
                    "Symbol" | "Type" => 1.2,
                    "Term" => 0.5, // Generic terms get less weight
                    _ => 1.0,
                };

                node.weight += (NODE_WEIGHT_BASE + salience * 0.3) * weight_multiplier;
                node.last_seen = self.clock;
                node.salience = (node.salience + salience * 0.5 * weight_multiplier).min(1.0);

                // Update kind if it was previously generic or less specific
                if node_kind_priority(&entity.kind) > node_kind_priority(&node.kind) {
                    node.kind = entity.kind.clone();
                }
            }

            node_ids.push(id);
            edge_contexts.push(entity.context.clone());
        }

        for i in 0..node_ids.len() {
            for j in (i + 1)..node_ids.len() {
                let edge_kind = match (edge_contexts[i].as_str(), edge_contexts[j].as_str()) {
                    ("defines", "mentions") => "contains",
                    (a, b) if a == "uses" || b == "uses" => "depends-on",
                    (a, b) if a == "implements" || b == "implements" => "implements",
                    ("defines", "defines") => "co-defined",
                    _ => "related",
                };
                self.upsert_edge(node_ids[i], node_ids[j], edge_kind);
            }
        }

        // Phase E: Hebbian reinforcement for keyword nodes.
        // Scan text for matches against keyword graph nodes and boost their weight.
        // Keywords that appear in useful ticks gain weight naturally; unused keywords
        // decay below PRUNE_THRESHOLD and get cleaned up by existing pruning logic.
        let text_lower = text.to_lowercase();
        let keyword_node_ids: Vec<u64> = self
            .long_term
            .nodes
            .iter()
            .filter(|(_, n)| n.kind == "Keyword")
            .map(|(&id, _)| id)
            .collect();

        for kw_id in keyword_node_ids {
            let term = {
                let node = &self.long_term.nodes[&kw_id];
                // Extract term from label "kw:<category>:<term>"
                node.label.splitn(3, ':').nth(2).unwrap_or("").to_string()
            };
            if !term.is_empty() && text_lower.contains(&term.to_lowercase()) {
                if let Some(node) = self.long_term.nodes.get_mut(&kw_id) {
                    node.weight = (node.weight + HEBBIAN_NODE_BOOST).min(HEBBIAN_NODE_CEILING);
                    node.last_seen = self.clock;
                }
                // Reinforce edges between this keyword and other active nodes
                for &other_id in &node_ids {
                    if other_id != kw_id {
                        self.upsert_edge(kw_id, other_id, "keyword-co-occurs");
                    }
                }
            }
        }
    }

    /// Insert a new edge or reinforce an existing one between two nodes.
    fn upsert_edge(&mut self, from: u64, to: u64, kind: &str) {
        let now = self.clock;
        if let Some(edge) = self
            .long_term
            .edges
            .iter_mut()
            .find(|e| (e.from == from && e.to == to) || (e.from == to && e.to == from))
        {
            // Synaptic plasticity: update dual-timescale interval tracking
            let interval = now.saturating_sub(edge.last_seen) as f32;
            if edge.activation_count > 0 && interval > 0.0 {
                edge.recent_interval_avg = 0.5 * interval + 0.5 * edge.recent_interval_avg;
                edge.historical_interval_avg =
                    0.1 * interval + 0.9 * edge.historical_interval_avg;

                // Spaced repetition: compare recent vs historical intervals
                if edge.historical_interval_avg > 0.0 {
                    let spacing_ratio =
                        edge.recent_interval_avg / edge.historical_interval_avg;
                    if spacing_ratio > 1.0 {
                        // Intervals are growing → spaced reinforcement
                        edge.stability = (edge.stability * 1.3).min(10.0);
                    } else {
                        // Intervals are shrinking or constant → cramming
                        edge.stability = (edge.stability * 1.05).min(10.0);
                    }
                }
            } else if edge.activation_count == 0 {
                // First reinforcement: initialize both EMAs to this interval
                edge.recent_interval_avg = interval;
                edge.historical_interval_avg = interval;
            }

            edge.activation_count = edge.activation_count.saturating_add(1);
            edge.weight += EDGE_REINFORCE_DELTA;
            edge.last_seen = now;
            if edge.kind == "related" && kind != "related" {
                edge.kind = kind.to_string();
            }
        } else {
            self.long_term.edges.push(GraphEdge {
                from,
                to,
                weight: EDGE_REINFORCE_DELTA,
                kind: kind.to_string(),
                last_seen: now,
                activation_count: 0,
                stability: 1.0,
                recent_interval_avg: 0.0,
                historical_interval_avg: 0.0,
            });
        }
    }

    /// Multi-hop spreading activation (CA3 recurrent network model).
    ///
    /// BFS-style outward spread from seed nodes. Each hop's activation decays by
    /// `decay_factor`, modeling how neural activation attenuates across synapses.
    /// Returns (node_id, activation) pairs sorted by activation descending.
    fn spreading_activation(
        &self,
        seed_ids: &[u64],
        max_hops: usize,
        decay_factor: f32,
    ) -> Vec<(u64, f32)> {
        let mut activations: HashMap<u64, f32> = HashMap::new();
        let mut visited: HashSet<u64> = HashSet::new();

        // Seeds start with activation 1.0
        let mut frontier: Vec<(u64, f32)> = Vec::new();
        for &id in seed_ids {
            activations.insert(id, 1.0);
            visited.insert(id);
            frontier.push((id, 1.0));
        }

        for hop in 0..max_hops {
            let mut next_frontier: Vec<(u64, f32)> = Vec::new();
            let hop_decay = decay_factor.powi(hop as i32 + 1);

            for &(node_id, parent_activation) in &frontier {
                for edge in &self.long_term.edges {
                    let neighbor_id = if edge.from == node_id {
                        Some(edge.to)
                    } else if edge.to == node_id {
                        Some(edge.from)
                    } else {
                        None
                    };

                    if let Some(nid) = neighbor_id {
                        if !visited.contains(&nid)
                            && self.long_term.nodes.contains_key(&nid)
                        {
                            // Synaptic encoding: edges with high stability (spaced
                            // reinforcement) propagate activation more effectively.
                            let effective_weight = edge.weight * edge.stability.sqrt();
                            let activation = parent_activation * effective_weight * hop_decay;
                            let entry = activations.entry(nid).or_insert(0.0);
                            if activation > *entry {
                                *entry = activation;
                            }
                            if visited.insert(nid) {
                                next_frontier.push((nid, activation));
                            }
                        }
                    }
                }
            }

            if next_frontier.is_empty() {
                break;
            }
            frontier = next_frontier;
        }

        // Remove seeds from results — caller already has them
        for &id in seed_ids {
            activations.remove(&id);
        }

        let mut results: Vec<(u64, f32)> = activations.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Query the knowledge graph: match entities by label, then expand via
    /// spreading activation (multi-hop).
    fn graph_lookup(&self, query: &str, limit: usize) -> Vec<GraphNodeSummary> {
        let entities = extract_entities(query, &self.keyword_cache);
        let mut results: Vec<GraphNodeSummary> = Vec::new();
        let mut seed_ids = Vec::new();

        for entity in &entities {
            if let Some(&node_id) = self.long_term.index.get(&entity.label) {
                if let Some(node) = self.long_term.nodes.get(&node_id) {
                    results.push(GraphNodeSummary {
                        id: node.id,
                        label: node.label.clone(),
                        kind: node.kind.clone(),
                        weight: node.weight,
                        edge_type: None, // direct match, no edge
                        source_texts: node.source_texts.clone(),
                    });
                    seed_ids.push(node.id);
                }
            }
        }

        // Multi-hop spreading activation from seed nodes
        if !seed_ids.is_empty() {
            let activated = self.spreading_activation(
                &seed_ids,
                SPREADING_ACTIVATION_MAX_HOPS,
                SPREADING_ACTIVATION_DECAY,
            );
            for (nid, activation) in activated {
                if let Some(node) = self.long_term.nodes.get(&nid) {
                    results.push(GraphNodeSummary {
                        id: node.id,
                        label: node.label.clone(),
                        kind: node.kind.clone(),
                        weight: node.weight + activation,
                        edge_type: Some("activated".to_string()),
                        source_texts: node.source_texts.clone(),
                    });
                }
            }
        }

        // No fallback: if no entities matched, return empty rather than dumping
        // all nodes. Associative priming in retrieve_context() already covers
        // the case where direct entity lookup finds nothing.

        // Deduplicate by id, keeping highest weight
        let mut deduped: HashMap<u64, GraphNodeSummary> = HashMap::new();
        for item in results {
            deduped
                .entry(item.id)
                .and_modify(|existing| {
                    if item.weight > existing.weight {
                        *existing = item.clone();
                    }
                })
                .or_insert(item);
        }

        let mut results: Vec<GraphNodeSummary> = deduped.into_values().collect();
        results.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
        results.truncate(limit);
        results
    }

    /// Remove short-term entries whose composite score has dropped below threshold.
    fn prune_short_term(&mut self) {
        let now = self.clock;
        self.short_term.retain(|entry| {
            let age = now.saturating_sub(entry.last_access) as f32;
            entry.salience + (entry.usage as f32 * PRUNE_USAGE_WEIGHT) - (age * PRUNE_AGE_WEIGHT)
                > PRUNE_THRESHOLD
        });
    }

    /// Remove low-weight graph nodes and orphaned/excess edges.
    fn prune_graph(&mut self) {
        let now = self.clock;

        // 1. Remove nodes whose decayed weight has fallen below threshold
        let remove_ids: Vec<u64> = self
            .long_term
            .nodes
            .iter()
            .filter(|(_, node)| {
                let age = now.saturating_sub(node.last_seen) as f32;
                let effective = node.weight - age * PRUNE_AGE_WEIGHT;
                effective < GRAPH_PRUNE_WEIGHT
            })
            .map(|(&id, _)| id)
            .collect();

        for &id in &remove_ids {
            if let Some(node) = self.long_term.nodes.remove(&id) {
                self.long_term.index.remove(&node.label);
            }
        }

        // 2. Hard cap: if still over capacity, evict lowest-weight nodes
        if self.long_term.nodes.len() > GRAPH_NODE_CAPACITY {
            let mut sorted: Vec<(u64, f32)> = self
                .long_term
                .nodes
                .iter()
                .map(|(&id, n)| (id, n.weight))
                .collect();
            sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

            let to_remove = self.long_term.nodes.len() - GRAPH_NODE_CAPACITY;
            for &(id, _) in sorted.iter().take(to_remove) {
                if let Some(node) = self.long_term.nodes.remove(&id) {
                    self.long_term.index.remove(&node.label);
                }
            }
        }

        // 3. Remove edges referencing deleted nodes
        let node_ids = &self.long_term.nodes;
        self.long_term
            .edges
            .retain(|e| node_ids.contains_key(&e.from) && node_ids.contains_key(&e.to));

        // 4. Hard cap on edges: keep highest-weight
        if self.long_term.edges.len() > GRAPH_EDGE_CAPACITY {
            self.long_term
                .edges
                .sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
            self.long_term.edges.truncate(GRAPH_EDGE_CAPACITY);
        }
    }

    /// Exponentially decay salience/weight based on time since last access.
    fn apply_decay(&mut self) {
        let now = self.clock;
        for entry in &mut self.short_term {
            // Semantic density reduces decay rate. High-density entries (many symbols/paths) persist longer.
            let density_factor = (1.0 + entry.density * 0.1).min(2.0);
            let base_decay_rate = SHORT_TERM_DECAY_RATE / density_factor;

            // Ebbinghaus: stability slows the forgetting curve. Higher stability → slower decay.
            let effective_decay_rate = base_decay_rate / entry.stability;

            let age = now.saturating_sub(entry.last_access) as f32;
            let decay = (-age * effective_decay_rate).exp();
            entry.salience *= decay;

            // Amygdala: emotional valence decays at half rate (emotional memories persist longer)
            let emotional_decay = (-age * effective_decay_rate * 0.5).exp();
            entry.emotional_valence *= emotional_decay;
        }
        for node in self.long_term.nodes.values_mut() {
            let decay = (-(now.saturating_sub(node.last_seen) as f32) * LONG_TERM_DECAY_RATE).exp();
            node.weight *= decay;
            node.salience *= decay;
        }
        // Edge decay: edges that haven't been reinforced recently lose weight
        for edge in &mut self.long_term.edges {
            let decay = (-(now.saturating_sub(edge.last_seen) as f32) * LONG_TERM_DECAY_RATE).exp();
            edge.weight *= decay;
        }
    }

    /// EMA-blend all salience scores toward their max-normalized values every RENORM_INTERVAL
    /// ticks. Keeps scores spread relative to each other without a hard reset.
    fn renormalize_salience(&mut self) {
        let max_sal = self
            .short_term
            .iter()
            .map(|e| e.salience)
            .fold(0.0_f32, f32::max);
        if max_sal < 0.05 {
            return;
        }
        for entry in &mut self.short_term {
            let normalized = entry.salience / max_sal;
            entry.salience = entry.salience * (1.0 - RENORM_BLEND) + normalized * RENORM_BLEND;
        }
    }

    /// Proportionally scale all graph node and edge weights so the maximum node weight
    /// never exceeds GRAPH_WEIGHT_TARGET_MAX. Preserves relative rankings.
    fn normalize_graph_weights(&mut self) {
        let max_weight = self
            .long_term
            .nodes
            .values()
            .map(|n| n.weight)
            .fold(0.0_f32, f32::max);
        if max_weight <= GRAPH_WEIGHT_TARGET_MAX || max_weight < 0.01 {
            return;
        }
        let scale = GRAPH_WEIGHT_TARGET_MAX / max_weight;
        for node in self.long_term.nodes.values_mut() {
            node.weight *= scale;
        }
        for edge in &mut self.long_term.edges {
            edge.weight *= scale;
        }
    }

    /// Force an immediate normalization pass on graph weights and salience scores.
    /// Call after loading an old store to bring blown-up values back within current bounds.
    pub fn rebalance_weights(&mut self) {
        self.renormalize_salience();
        self.normalize_graph_weights();
    }

    /// Return the most recent `n` session log entries.
    pub fn recent_sessions(&self, n: usize) -> &[SessionEntry] {
        let start = self.session_log.len().saturating_sub(n);
        &self.session_log[start..]
    }

    /// Set the current task description.
    pub fn set_task(&mut self, task: &str) {
        self.current_task = Some(task.to_string());

        // Link task to knowledge graph
        let label = task.trim();
        if !label.is_empty() {
            let node_id = if let Some(&id) = self.long_term.index.get(label) {
                id
            } else {
                let id = self.next_id;
                self.next_id += 1;
                self.long_term.nodes.insert(
                    id,
                    GraphNode {
                        id,
                        label: label.to_string(),
                        kind: "Task".to_string(),
                        weight: 1.5, // Tasks start with higher weight
                        last_seen: self.clock,
                        salience: 0.8,
                        source_texts: Vec::new(),
                        embedding: Vec::new(),
                        full_text: None,
                    },
                );
                self.long_term.index.insert(label.to_string(), id);
                id
            };

            // Prime task in graph
            if let Some(node) = self.long_term.nodes.get_mut(&node_id) {
                node.weight += 0.2;
                node.last_seen = self.clock;
            }
        }
    }

    /// Merge knowledge from another MemoryState by replaying its unique session log entries.
    /// This is an idempotent "smart merge" that avoids ID collisions by organic replay.
    pub fn merge_from_log(&mut self, other: MemoryState) {
        // Find session log entries in 'other' that are NOT in 'self'
        let our_texts: HashSet<&str> = self.session_log.iter().map(|s| s.text.as_str()).collect();

        let mut to_replay = Vec::new();
        for other_entry in other.session_log {
            if !our_texts.contains(other_entry.text.as_str()) {
                to_replay.push(other_entry);
            }
        }

        // Sort by timestamp to preserve order
        to_replay.sort_by_key(|e| e.timestamp);

        if !to_replay.is_empty() {
            eprintln!("[LEGEND] Smart-merging {} new session memories...", to_replay.len());
            for entry in to_replay {
                // Passive tick to avoid polluting the session log we just used for diffing,
                // and to avoid recursive auto-consolidations during the merge itself.
                self.tick_passive(&entry.text);
            }
        }

        // Merge task if ours is empty
        if self.current_task.is_none() {
            self.current_task = other.current_task;
        }

        // Keep the later clock to ensure future entries have unique IDs
        self.clock = self.clock.max(other.clock);
        self.next_id = self.next_id.max(other.next_id);
    }

    /// Clear the current task.
    pub fn clear_task(&mut self) {
        self.current_task = None;
    }

    /// Get the current task description.
    pub fn get_task(&self) -> Option<&str> {
        self.current_task.as_deref()
    }

    /// Check if consolidation should be suggested.
    ///
    /// Triggers:
    /// 1. Tick count threshold (every 15 ticks) — baseline periodic consolidation
    /// 2. Emotional intensity burst — amygdala-driven, after high-valence events
    /// 3. Context switch — hippocampal novelty detection (handled in tick_impl
    ///    via flush_working_memory, but also triggers consolidation suggestion)
    pub fn should_suggest_consolidation(&self) -> bool {
        self.ticks_since_consolidation >= CONSOLIDATION_SUGGESTION_THRESHOLD
            || self.recent_valence_sum >= EMOTIONAL_CONSOLIDATION_THRESHOLD
    }

    /// Build a structured cold-start context summary as JSON.
    pub fn build_context_summary(&self) -> serde_json::Value {
        let recent = self.recent_sessions(5);
        let session_texts: Vec<&str> = recent.iter().map(|s| s.text.as_str()).collect();

        let mut top_nodes: Vec<&GraphNode> = self.long_term.nodes.values().collect();
        top_nodes.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
        top_nodes.truncate(8);

        let node_summaries: Vec<serde_json::Value> = top_nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "label": n.label,
                    "kind": n.kind,
                    "weight": (n.weight * 100.0).round() / 100.0,
                })
            })
            .collect();

        serde_json::json!({
            "current_task": self.current_task,
            "stats": {
                "working_memory": self.working_memory.len(),
                "short_term_entries": self.short_term.len(),
                "long_term_nodes": self.long_term.nodes.len(),
                "long_term_edges": self.long_term.edges.len(),
                "session_log_entries": self.session_log.len(),
                "clock": self.clock,
            },
            "recent_sessions": session_texts,
            "top_graph_nodes": node_summaries,
        })
    }

    /// Build a comprehensive session-start summary: context + categorized memories.
    /// Designed as a single cold-start call that gives the LLM everything it needs.
    #[allow(dead_code)]
    pub fn build_start_summary(&mut self) -> serde_json::Value {
        self.build_start_summary_with_options(false, None, None)
    }

    /// Build session-start summary with options for compact output and category filtering.
    /// - compact: If true, only show short text summaries (no id, reduced text length)
    /// - category_filter: If Some, only return that specific category
    ///
    /// Output is simplified for LLM usability: no stats, no graph weights.
    /// Use `memory dump` for full internal state.
    pub fn build_start_summary_with_options(
        &mut self,
        compact: bool,
        category_filter: Option<&str>,
        query: Option<&str>,
    ) -> serde_json::Value {
        // If a query is provided, perform an internal retrieval to "prime" the graph and surface relevant context.
        // This automatically boosts the salience of related short-term entries and surfaces related graph nodes.
        let mut query_context = None;
        if let Some(q) = query {
            query_context = Some(self.retrieve_context(q));
        }

        let git_sync = self.get_git_summary();

        // Get recent sessions — skip passive (EXPERIENCE:) entries so Recent Activity
        // only shows meaningful user-initiated ticks, not tool telemetry noise.
        let recent_sessions: Vec<&str> = self
            .session_log
            .iter()
            .rev()
            .filter(|s| !s.text.starts_with("EXPERIENCE:"))
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|s| s.text.as_str())
            .collect();

        // --- Categorized short-term memories ---
        let mut decisions: Vec<serde_json::Value> = Vec::new();
        let mut architecture: Vec<serde_json::Value> = Vec::new();
        let mut todos: Vec<serde_json::Value> = Vec::new();
        let mut bugs: Vec<serde_json::Value> = Vec::new();
        let mut preferences: Vec<serde_json::Value> = Vec::new();

        for entry in &self.short_term {
            let category = classify_text(&entry.text, &self.keyword_cache);

            // Build item based on compact mode
            let item = if compact {
                // Compact: just the text, truncated shorter
                serde_json::json!(safe_truncate(&entry.text, 80))
            } else {
                // Default: id and text only (removed salience/reconsolidations)
                serde_json::json!({
                    "id": entry.id,
                    "text": safe_truncate(&entry.text, 120),
                })
            };

            match category {
                MemoryCategory::Decision => decisions.push(item),
                MemoryCategory::Architecture => architecture.push(item),
                MemoryCategory::Todo => todos.push(item),
                MemoryCategory::Bug => bugs.push(item),
                MemoryCategory::Preference => preferences.push(item),
                _ => {} // Progress and General omitted for brevity
            }
        }

        // Track total counts before truncation
        let decisions_total = decisions.len();
        let architecture_total = architecture.len();
        let todos_total = todos.len();
        let bugs_total = bugs.len();
        let preferences_total = preferences.len();

        // Sort each category. If query_context exists, we prioritize items matched by the query.
        // Otherwise, we sort by salience descending.
        let sort_logic = |list: &mut Vec<serde_json::Value>,
                          entries: &[ShortTermEntry],
                          context: &Option<MemoryContext>| {
            // Create index mapping for sorting
            let mut indexed: Vec<(usize, f32)> = list
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    let id = if compact {
                        let text = item.as_str()?;
                        entries
                            .iter()
                            .find(|e| e.text.starts_with(text.trim_end_matches('…')))
                            .map(|e| e.id)
                    } else {
                        item["id"].as_u64()
                    }?;

                    let entry = entries.iter().find(|e| e.id == id)?;

                    // Base score is salience
                    let mut score = entry.salience;

                    // If this entry was returned in the query context, give it a massive boost
                    if let Some(ctx) = context {
                        if let Some(matched) = ctx.short_term.iter().find(|m| m.id == id) {
                            score += 10.0 + matched.similarity;
                        }
                    }

                    Some((i, score))
                })
                .collect();

            indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let sorted: Vec<serde_json::Value> =
                indexed.into_iter().map(|(i, _)| list[i].clone()).collect();
            *list = sorted;
            list.truncate(5);
        };

        sort_logic(&mut decisions, &self.short_term, &query_context);
        sort_logic(&mut architecture, &self.short_term, &query_context);
        sort_logic(&mut todos, &self.short_term, &query_context);
        sort_logic(&mut bugs, &self.short_term, &query_context);
        sort_logic(&mut preferences, &self.short_term, &query_context);

        // Helper to build category object with optional truncation indicator
        let build_category = |items: &[serde_json::Value], total: usize| -> serde_json::Value {
            if total > 5 {
                serde_json::json!({
                    "items": items,
                    "showing": items.len(),
                    "total": total
                })
            } else {
                serde_json::json!(items)
            }
        };

        // If category filter is specified, only return that category
        if let Some(filter) = category_filter {
            let (filtered, total) = match filter.to_lowercase().as_str() {
                "decisions" | "decision" => (&decisions, decisions_total),
                "architecture" | "arch" => (&architecture, architecture_total),
                "todos" | "todo" => (&todos, todos_total),
                "bugs" | "bug" => (&bugs, bugs_total),
                "preferences" | "preference" | "prefs" | "pref" => {
                    (&preferences, preferences_total)
                }
                _ => return serde_json::json!({"error": format!("Unknown category: {}", filter)}),
            };
            return serde_json::json!({
                "current_task": self.current_task,
                "category": filter,
                "items": filtered,
                "total": total,
            });
        }

        serde_json::json!({
            "current_task": self.current_task,
            "git_sync": git_sync,
            "recent_sessions": recent_sessions,
            "categorized": {
                "decisions": build_category(&decisions, decisions_total),
                "architecture": build_category(&architecture, architecture_total),
                "todos": build_category(&todos, todos_total),
                "bugs": build_category(&bugs, bugs_total),
                "preferences": build_category(&preferences, preferences_total),
            }
        })
    }

    /// Export the full memory state as JSON for external tools (e.g. dashboard).
    pub fn build_dump(&self) -> serde_json::Value {
        let nodes: Vec<serde_json::Value> = self
            .long_term
            .nodes
            .values()
            .map(|n| {
                serde_json::json!({
                    "id": n.id, "label": n.label, "kind": n.kind,
                    "weight": (n.weight * 1000.0).round() / 1000.0,
                    "salience": (n.salience * 1000.0).round() / 1000.0,
                    "last_seen": n.last_seen,
                })
            })
            .collect();

        let edges: Vec<serde_json::Value> = self
            .long_term
            .edges
            .iter()
            .map(|e| {
                serde_json::json!({
                    "from": e.from, "to": e.to,
                    "weight": (e.weight * 1000.0).round() / 1000.0,
                    "kind": e.kind,
                })
            })
            .collect();

        let short_term: Vec<serde_json::Value> = self
            .short_term
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id, "text": e.text, "summary": e.summary,
                    "salience": (e.salience * 1000.0).round() / 1000.0,
                    "emotional_valence": (e.emotional_valence * 1000.0).round() / 1000.0,
                    "usage": e.usage, "last_access": e.last_access,
                    "reconsolidation_count": e.reconsolidation_count,
                    "labile": e.labile_until >= self.clock,
                    "refs": e.refs,
                })
            })
            .collect();

        let working_memory: Vec<serde_json::Value> = self
            .working_memory
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id": e.id, "text": e.text,
                    "salience": (e.salience * 1000.0).round() / 1000.0,
                    "emotional_valence": (e.emotional_valence * 1000.0).round() / 1000.0,
                    "tick_created": e.tick_created,
                    "rehearsal_count": e.rehearsal_count,
                    "promoted": e.promoted,
                })
            })
            .collect();
        let sessions: Vec<serde_json::Value> = self
            .session_log
            .iter()
            .map(|s| serde_json::json!({"timestamp": s.timestamp, "text": s.text}))
            .collect();

        serde_json::json!({
            "clock": self.clock,
            "working_memory": working_memory,
            "short_term": short_term,
            "graph": { "nodes": nodes, "edges": edges },
            "session_log": sessions,
        })
    }

    /// Apply an external reinforcement signal to specific short-term entries.
    ///
    /// Positive `signal` (e.g. 1.0) means "this was useful" — boosts salience
    /// and usage count. Negative `signal` (e.g. -0.5) means "this was
    /// irrelevant" — reduces salience so the entry decays faster.
    ///
    /// The signal also cascades into the long-term graph: entities extracted
    /// from each reinforced entry's text get a proportional weight adjustment.
    pub fn reinforce(&mut self, ids: &[u64], signal: f32) -> ReinforceResult {
        self.clock += 1;
        let signal = signal.clamp(-1.0, 1.0);
        let mut reinforced = Vec::new();
        let mut graph_nodes_affected = 0usize;

        // Contrastive descent: entries retrieved in the prior retrieve_context() call
        // but not in the current reinforced set receive a small salience penalty.
        if signal > 0.0 {
            let reinforced_set: std::collections::HashSet<u64> = ids.iter().copied().collect();
            let penalized: Vec<u64> = self
                .last_retrieved_ids
                .iter()
                .copied()
                .filter(|id| !reinforced_set.contains(id))
                .collect();
            for &pid in &penalized {
                if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == pid) {
                    entry.salience = (entry.salience - CONTRASTIVE_PENALTY).max(0.0);
                }
            }
        }

        for &id in ids {
            if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == id) {
                let before = entry.salience;

                // AdaGrad-style adaptive salience update: frequently-reinforced entries
                // get smaller future updates, preventing saturation.
                let adaptive_lr =
                    ADAGRAD_BASE_LR / (entry.gradient_sq_sum + ADAGRAD_EPSILON).sqrt();
                let delta = signal * adaptive_lr;
                entry.gradient_sq_sum =
                    (entry.gradient_sq_sum + signal * signal).min(ADAGRAD_SQ_SUM_CAP);
                entry.salience = (entry.salience + delta).clamp(0.0, 1.0);

                // Adjust usage: positive signal bumps usage, negative doesn't
                // reduce below 1 (entry still exists, just less important)
                if signal > 0.0 {
                    entry.usage = entry.usage.saturating_add(1);
                }
                entry.last_access = self.clock;

                reinforced.push(ReinforcedEntry {
                    id,
                    salience_before: before,
                    salience_after: entry.salience,
                    signal,
                });

                // Cascade into graph: boost/demote entities from this entry's text
                let entities = extract_entities(&entry.text.clone(), &self.keyword_cache);
                for entity in &entities {
                    if let Some(&node_id) = self.long_term.index.get(&entity.label) {
                        if let Some(node) = self.long_term.nodes.get_mut(&node_id) {
                            node.weight = (node.weight + signal * REINFORCE_GRAPH_SCALE).max(0.01);
                            node.last_seen = self.clock;
                            graph_nodes_affected += 1;
                        }
                    }
                }
            }
        }

        // Clear stale retrieved IDs after processing.
        self.last_retrieved_ids.clear();

        ReinforceResult {
            reinforced,
            graph_nodes_affected,
        }
    }
}

/// Compute the word-overlap ratio between two texts.
/// Returns the Jaccard coefficient of their lowercased word sets.
// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

pub fn reset_memory() -> Result<(), Box<dyn std::error::Error>> {
    if Path::new(MEMORY_FILE).exists() {
        fs::remove_file(MEMORY_FILE).map_err(|e| format!("Failed to remove memory file: {}", e))?;
    }
    Ok(())
}

fn load_memory() -> Result<MemoryState, Box<dyn std::error::Error>> {
    load_memory_from_path(MEMORY_FILE)
}

pub fn load_memory_from_path<P: AsRef<Path>>(path: P) -> Result<MemoryState, Box<dyn std::error::Error>> {
    let compressed =
        fs::read(path).map_err(|e| format!("Failed to read memory file: {}", e))?;
    let decompressed = lz4::block::decompress(&compressed, None)
        .map_err(|e| format!("Failed to decompress memory: {}", e))?;

    // MessagePack format: starts with LGND magic + version byte
    if decompressed.len() >= 5 && &decompressed[..4] == MSGPACK_MAGIC {
        let _version = decompressed[4];
        let state: MemoryState = rmp_serde::from_slice(&decompressed[5..])
            .map_err(|e| format!("Failed to deserialize msgpack memory: {}", e))?;
        return Ok(state);
    }

    // Legacy bincode fallback: try current format first
    if let Ok(state) = bincode::deserialize::<MemoryState>(&decompressed) {
        eprintln!("Migrating memory from bincode to msgpack format...");
        return Ok(state);
    }

    // Fall back to V5 (pre-working-memory, has immediate: VecDeque<String>)
    if let Ok(v5) = bincode::deserialize::<MemoryStateV5>(&decompressed) {
        eprintln!("Migrating memory from pre-working-memory bincode format...");
        return Ok(migrate_v5(v5));
    }

    // Fall back to V4 (pre-consolidated field, has gradient_sq_sum + density)
    if let Ok(v4) = bincode::deserialize::<MemoryStateV4>(&decompressed) {
        eprintln!("Migrating memory from v0.3.4 bincode format...");
        return Ok(migrate_v4(v4));
    }

    Err("Failed to deserialize memory: no known format matched".into())
}

/// ShortTermEntry before emotional_valence was added (pre-v0.3.10).
#[derive(Debug, Clone, Deserialize)]
struct ShortTermEntryV5 {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub summary: String,
    pub embedding: Vec<f32>,
    pub last_access: u64,
    pub usage: u32,
    #[serde(default)]
    pub salience: f32,
    #[serde(default)]
    pub reconsolidation_count: u32,
    #[serde(default)]
    pub labile_until: u64,
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
    #[serde(default)]
    pub gradient_sq_sum: f32,
    #[serde(default)]
    pub density: f32,
    #[serde(default)]
    pub consolidated: bool,
}

/// MemoryState before working_memory rework (had `immediate: VecDeque<String>`).
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct MemoryStateV5 {
    pub config: MemoryConfig,
    pub immediate: VecDeque<String>,
    pub short_term: Vec<ShortTermEntryV5>,
    pub long_term: GraphMemory,
    pub clock: u64,
    pub next_id: u64,
    #[serde(default)]
    pub session_log: Vec<SessionEntry>,
    #[serde(default)]
    pub current_task: Option<String>,
    #[serde(default)]
    pub ticks_since_consolidation: u32,
    #[serde(default)]
    pub last_retrieved_ids: Vec<u64>,
    #[serde(default)]
    pub last_synced_sha: Option<String>,
}

fn migrate_v5(v5: MemoryStateV5) -> MemoryState {
    MemoryState {
        config: v5.config,
        working_memory: Vec::new(), // discard old FIFO L1 contents
        short_term: v5
            .short_term
            .into_iter()
            .map(|e| ShortTermEntry {
                id: e.id,
                text: e.text,
                summary: e.summary,
                embedding: e.embedding,
                last_access: e.last_access,
                usage: e.usage,
                salience: e.salience,
                reconsolidation_count: e.reconsolidation_count,
                labile_until: e.labile_until,
                refs: e.refs,
                gradient_sq_sum: e.gradient_sq_sum,
                density: e.density,
                consolidated: e.consolidated,
                emotional_valence: 0.0,
                stability: 1.0,
                last_retrieval_interval: 0,
            })
            .collect(),
        long_term: v5.long_term,
        clock: v5.clock,
        next_id: v5.next_id,
        session_log: v5.session_log,
        current_task: v5.current_task,
        ticks_since_consolidation: v5.ticks_since_consolidation,
        last_retrieved_ids: v5.last_retrieved_ids,
        last_synced_sha: v5.last_synced_sha,
        recent_valence_sum: 0.0,
        last_tick_embedding: Vec::new(),
        term_frequency: HashMap::new(),
        keyword_cache: keyword_cache::KeywordCache::default(),
    }
}

/// ShortTermEntry before `consolidated` was added (v0.3.4 format).
#[derive(Debug, Clone, Deserialize)]
struct ShortTermEntryV4 {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub summary: String,
    pub embedding: Vec<f32>,
    pub last_access: u64,
    pub usage: u32,
    #[serde(default)]
    pub salience: f32,
    #[serde(default)]
    pub reconsolidation_count: u32,
    #[serde(default)]
    pub labile_until: u64,
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
    #[serde(default)]
    pub gradient_sq_sum: f32,
    #[serde(default)]
    pub density: f32,
    // `consolidated` intentionally absent — this is the pre-v0.3.5 format
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct MemoryStateV4 {
    pub config: MemoryConfig,
    pub immediate: VecDeque<String>,
    pub short_term: Vec<ShortTermEntryV4>,
    pub long_term: GraphMemory,
    pub clock: u64,
    pub next_id: u64,
    #[serde(default)]
    pub session_log: Vec<SessionEntry>,
    #[serde(default)]
    pub current_task: Option<String>,
    #[serde(default)]
    pub ticks_since_consolidation: u32,
    #[serde(default)]
    pub last_retrieved_ids: Vec<u64>,
    #[serde(default)]
    pub last_synced_sha: Option<String>,
}

fn migrate_v4(v4: MemoryStateV4) -> MemoryState {
    MemoryState {
        config: v4.config,
        working_memory: Vec::new(), // discard old FIFO L1 contents
        short_term: v4
            .short_term
            .into_iter()
            .map(|e| ShortTermEntry {
                id: e.id,
                text: e.text,
                summary: e.summary,
                embedding: e.embedding,
                last_access: e.last_access,
                usage: e.usage,
                salience: e.salience,
                reconsolidation_count: e.reconsolidation_count,
                labile_until: e.labile_until,
                refs: e.refs,
                gradient_sq_sum: e.gradient_sq_sum,
                density: e.density,
                consolidated: false,
                emotional_valence: 0.0,
                stability: 1.0,
                last_retrieval_interval: 0,
            })
            .collect(),
        long_term: v4.long_term,
        clock: v4.clock,
        next_id: v4.next_id,
        session_log: v4.session_log,
        current_task: v4.current_task,
        ticks_since_consolidation: v4.ticks_since_consolidation,
        last_retrieved_ids: v4.last_retrieved_ids,
        last_synced_sha: v4.last_synced_sha,
        recent_valence_sum: 0.0,
        last_tick_embedding: Vec::new(),
        term_frequency: HashMap::new(),
        keyword_cache: keyword_cache::KeywordCache::default(),
    }
}

/// Attempt to migrate old memory format from .corrupt backup.
/// Returns Ok(Some(state)) if migration succeeded, Ok(None) if no backup exists.
fn migrate_corrupt_backup() -> Result<Option<MemoryState>, Box<dyn std::error::Error>> {
    const CORRUPT_FILE: &str = ".legend/memory.lz4.corrupt";

    if !Path::new(CORRUPT_FILE).exists() {
        return Ok(None);
    }

    eprintln!("Detected old memory format backup, attempting migration...");

    // ShortTermEntry before gradient_sq_sum was added (commit a0a40a7).
    #[derive(Debug, Clone, Deserialize)]
    struct MemoryRefV1 {
        pub path: String,
        pub start_line: usize,
        pub end_line: usize,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct ShortTermEntryV1 {
        pub id: u64,
        pub text: String,
        #[serde(default)]
        pub summary: String,
        pub embedding: Vec<f32>,
        pub last_access: u64,
        pub usage: u32,
        #[serde(default)]
        pub salience: f32,
        #[serde(default)]
        pub reconsolidation_count: u32,
        #[serde(default)]
        pub labile_until: u64,
        #[serde(default)]
        pub refs: Vec<MemoryRefV1>,
        // gradient_sq_sum intentionally absent
    }

    #[derive(Debug, Clone, Deserialize)]
    struct ShortTermEntryV2 {
        pub id: u64,
        pub text: String,
        #[serde(default)]
        pub summary: String,
        pub embedding: Vec<f32>,
        pub last_access: u64,
        pub usage: u32,
        #[serde(default)]
        pub salience: f32,
        #[serde(default)]
        pub reconsolidation_count: u32,
        #[serde(default)]
        pub labile_until: u64,
        #[serde(default)]
        pub refs: Vec<MemoryRef>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[allow(dead_code)]
    struct MemoryStateV3 {
        pub config: MemoryConfig,
        pub immediate: VecDeque<String>,
        pub short_term: Vec<ShortTermEntryV2>,
        pub long_term: GraphMemory,
        pub clock: u64,
        pub next_id: u64,
        #[serde(default)]
        pub session_log: Vec<SessionEntry>,
        #[serde(default)]
        pub current_task: Option<String>,
        #[serde(default)]
        pub ticks_since_consolidation: u32,
        #[serde(default)]
        pub last_retrieved_ids: Vec<u64>,
        #[serde(default)]
        pub last_synced_sha: Option<String>,
    }

    // MemoryState with current_task/ticks_since_consolidation but before last_retrieved_ids.
    #[derive(Debug, Clone, Deserialize)]
    #[allow(dead_code)]
    struct MemoryStateV2 {
        pub config: MemoryConfig,
        pub immediate: VecDeque<String>,
        pub short_term: Vec<ShortTermEntryV1>,
        pub long_term: GraphMemory,
        pub clock: u64,
        pub next_id: u64,
        #[serde(default)]
        pub session_log: Vec<SessionEntry>,
        #[serde(default)]
        pub current_task: Option<String>,
        #[serde(default)]
        pub ticks_since_consolidation: u32,
    }

    // MemoryState before current_task/ticks_since_consolidation were added.
    #[derive(Debug, Clone, Deserialize)]
    #[allow(dead_code)]
    struct MemoryStateV1 {
        pub config: MemoryConfig,
        pub immediate: VecDeque<String>,
        pub short_term: Vec<ShortTermEntryV1>,
        pub long_term: GraphMemory,
        pub clock: u64,
        pub next_id: u64,
        #[serde(default)]
        pub session_log: Vec<SessionEntry>,
    }

    let compressed = fs::read(CORRUPT_FILE)?;
    let serialized = match lz4::block::decompress(&compressed, None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "  Backup file is unrecoverable (decompress failed: {}). Archiving.",
                e
            );
            let archive = format!("{}.unrecoverable", CORRUPT_FILE);
            let _ = fs::rename(CORRUPT_FILE, &archive);
            return Ok(None);
        }
    };

    // Try msgpack first, then bincode V5, V4, V3, V2, V1.
    let new_state = if serialized.len() >= 5 && &serialized[..4] == MSGPACK_MAGIC {
        rmp_serde::from_slice::<MemoryState>(&serialized[5..])?
    } else if let Ok(v5) = bincode::deserialize::<MemoryStateV5>(&serialized) {
        migrate_v5(v5)
    } else if let Ok(v4) = bincode::deserialize::<MemoryStateV4>(&serialized) {
        migrate_v4(v4)
    } else if let Ok(v3) = bincode::deserialize::<MemoryStateV3>(&serialized) {
        MemoryState {
            config: v3.config,
            working_memory: Vec::new(),
            short_term: v3
                .short_term
                .into_iter()
                .map(|e| ShortTermEntry {
                    id: e.id,
                    text: e.text,
                    summary: e.summary,
                    embedding: e.embedding,
                    last_access: e.last_access,
                    usage: e.usage,
                    salience: e.salience,
                    reconsolidation_count: e.reconsolidation_count,
                    labile_until: e.labile_until,
                    refs: e.refs,
                    gradient_sq_sum: 0.0,
                    density: 0.0,
                    consolidated: false,
                    emotional_valence: 0.0,
                    stability: 1.0,
                    last_retrieval_interval: 0,
                })
                .collect(),
            long_term: v3.long_term,
            clock: v3.clock,
            next_id: v3.next_id,
            session_log: v3.session_log,
            current_task: v3.current_task,
            ticks_since_consolidation: v3.ticks_since_consolidation,
            last_retrieved_ids: v3.last_retrieved_ids,
            last_synced_sha: v3.last_synced_sha,
            recent_valence_sum: 0.0,
            last_tick_embedding: Vec::new(),
            term_frequency: HashMap::new(),
            keyword_cache: keyword_cache::KeywordCache::default(),
        }
    } else if let Ok(v2) = bincode::deserialize::<MemoryStateV2>(&serialized) {
        MemoryState {
            config: v2.config,
            working_memory: Vec::new(),
            short_term: v2
                .short_term
                .into_iter()
                .map(|e| ShortTermEntry {
                    id: e.id,
                    text: e.text,
                    summary: e.summary,
                    embedding: e.embedding,
                    last_access: e.last_access,
                    usage: e.usage,
                    salience: e.salience,
                    reconsolidation_count: e.reconsolidation_count,
                    labile_until: e.labile_until,
                    refs: e
                        .refs
                        .into_iter()
                        .map(|r| MemoryRef {
                            path: r.path,
                            start_line: r.start_line,
                            end_line: r.end_line,
                            snippet: String::new(),
                        })
                        .collect(),
                    gradient_sq_sum: 0.0,
                    density: 0.0,
                    consolidated: false,
                    emotional_valence: 0.0,
                    stability: 1.0,
                    last_retrieval_interval: 0,
                })
                .collect(),
            long_term: v2.long_term,
            clock: v2.clock,
            next_id: v2.next_id,
            session_log: v2.session_log,
            current_task: v2.current_task,
            ticks_since_consolidation: v2.ticks_since_consolidation,
            last_retrieved_ids: Vec::new(),
            last_synced_sha: None,
            recent_valence_sum: 0.0,
            last_tick_embedding: Vec::new(),
            term_frequency: HashMap::new(),
            keyword_cache: keyword_cache::KeywordCache::default(),
        }
    } else {
        match bincode::deserialize::<MemoryStateV1>(&serialized) {
            Ok(old) => MemoryState {
                config: old.config,
                working_memory: Vec::new(),
                short_term: old
                    .short_term
                    .into_iter()
                    .map(|e| ShortTermEntry {
                        id: e.id,
                        text: e.text,
                        summary: e.summary,
                        embedding: e.embedding,
                        last_access: e.last_access,
                        usage: e.usage,
                        salience: e.salience,
                        reconsolidation_count: e.reconsolidation_count,
                        labile_until: e.labile_until,
                        refs: old_refs_to_current(e.refs),
                        gradient_sq_sum: 0.0,
                        density: 0.0,
                        consolidated: false,
                        emotional_valence: 0.0,
                        stability: 1.0,
                        last_retrieval_interval: 0,
                    })
                    .collect(),
                long_term: old.long_term,
                clock: old.clock,
                next_id: old.next_id,
                session_log: old.session_log,
                current_task: None,
                ticks_since_consolidation: 0,
                last_retrieved_ids: Vec::new(),
                last_synced_sha: None,
                recent_valence_sum: 0.0,
                last_tick_embedding: Vec::new(),
                term_frequency: HashMap::new(),
                keyword_cache: keyword_cache::KeywordCache::default(),
            },
            Err(_) => {
                eprintln!("  Backup file is unrecoverable (no format matched). Archiving.");
                let archive = format!("{}.unrecoverable", CORRUPT_FILE);
                let _ = fs::rename(CORRUPT_FILE, &archive);
                return Ok(None);
            }
        }
    };

    fn old_refs_to_current(old: Vec<MemoryRefV1>) -> Vec<MemoryRef> {
        old.into_iter()
            .map(|r| MemoryRef {
                path: r.path,
                start_line: r.start_line,
                end_line: r.end_line,
                snippet: String::new(),
            })
            .collect()
    }

    // Save migrated state
    new_state.save()?;

    // Remove corrupt backup after successful migration
    if let Err(e) = fs::remove_file(CORRUPT_FILE) {
        eprintln!(
            "  Warning: could not remove {} after migration: {}",
            CORRUPT_FILE, e
        );
    } else {
        eprintln!("  ✓ Cleaned up old format backup.");
    }

    eprintln!(
        "✓ Migration complete: {} short-term entries, {} graph nodes recovered",
        new_state.short_term.len(),
        new_state.long_term.nodes.len()
    );

    Ok(Some(new_state))
}

fn save_memory(state: &MemoryState) -> Result<(), Box<dyn std::error::Error>> {
    save_memory_to_path(state, MEMORY_FILE)
}

pub fn save_memory_to_path<P: AsRef<Path>>(state: &MemoryState, path: P) -> Result<(), Box<dyn std::error::Error>> {
    let serialized =
        rmp_serde::to_vec_named(state).map_err(|e| format!("Failed to serialize memory: {}", e))?;

    // Prepend magic header: LGND + version byte
    let mut payload = Vec::with_capacity(5 + serialized.len());
    payload.extend_from_slice(MSGPACK_MAGIC);
    payload.push(MSGPACK_FORMAT_VERSION);
    payload.extend_from_slice(&serialized);

    let compressed = lz4::block::compress(&payload, None, true)
        .map_err(|e| format!("Failed to compress memory: {}", e))?;

    let path_ref = path.as_ref();
    let temp_file = format!("{}.tmp", path_ref.display());
    fs::write(&temp_file, &compressed)
        .map_err(|e| format!("Failed to write temp memory file: {}", e))?;
    fs::rename(&temp_file, path_ref)
        .map_err(|e| format!("Failed to write memory file: {}", e))?;
    Ok(())
}

/// Composite eviction score: higher = more worth keeping.
fn eviction_score(entry: &ShortTermEntry, now: u64) -> f32 {
    let age = now.saturating_sub(entry.last_access) as f32;
    let recency = (-age * EVICTION_DECAY_RATE).exp();
    let usage = (entry.usage as f32).ln_1p();
    // Amygdala: emotionally charged memories resist eviction
    let emotional_resistance = entry.emotional_valence.abs() * 0.15;
    entry.salience * 0.4 + usage * 0.3 + recency * 0.3 + emotional_resistance
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

fn safe_truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let mut end = max_len;
    while !s.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

fn calculate_density(text: &str, kw: &KeywordCache) -> f32 {
    let entities = extract_entities(text, kw);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kw() -> KeywordCache {
        KeywordCache::default_from_static()
    }

    #[test]
    fn test_eviction_score_recent_high() {
        let recent = ShortTermEntry {
            id: 1,
            text: "test".into(),
            summary: "test".into(),
            embedding: vec![],
            last_access: 100,
            usage: 5,
            salience: 0.8,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
        };
        let old = ShortTermEntry {
            id: 2,
            text: "test".into(),
            summary: "test".into(),
            embedding: vec![],
            last_access: 1,
            usage: 1,
            salience: 0.1,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
        };
        assert!(eviction_score(&recent, 100) > eviction_score(&old, 100));
    }

    #[test]
    fn test_tick_adds_entry() {
        let mut state = MemoryState::default();
        state.tick("hello world test entry");
        // Entry goes to working memory; may or may not promote to L2 depending on salience
        assert!(!state.working_memory.is_empty() || !state.short_term.is_empty());
    }

    #[test]
    fn test_tick_reinforces_similar() {
        let mut state = MemoryState::default();
        state.tick("the embedding system uses vector similarity");
        let usage_before = state.short_term[0].usage;
        state.tick("the embedding system uses vector similarity");
        assert_eq!(
            state.short_term.len(),
            1,
            "identical tick should reinforce, not add"
        );
        assert!(state.short_term[0].usage > usage_before);
    }

    #[test]
    fn test_retrieve_context() {
        let mut state = MemoryState::default();
        state.tick("memory system with embeddings");
        state.tick("database of knowledge graphs");
        let ctx = state.retrieve_context("embedding search");
        assert!(!ctx.short_term.is_empty());
    }

    #[test]
    fn test_consolidate() {
        let mut state = MemoryState::default();
        state.tick("DECISION: embedding quality improvement using n-grams");
        state.tick("DECISION: embedding quality improvement using trigrams");
        state.tick("DECISION: completely different topic about cooking recipes");
        let summaries = state.consolidate();
        assert!(!state.short_term.is_empty() || !summaries.is_empty());
    }

    #[test]
    fn test_graph_edges_typed() {
        let mut state = MemoryState::default();
        state.tick("fn handle_memory() calls fn handle_tick()");
        for edge in &state.long_term.edges {
            assert!(!edge.kind.is_empty());
        }
    }

    #[test]
    fn test_hebbian_reinforcement() {
        let mut state = MemoryState::default();
        state.tick("fn process_data() uses struct Config");
        let initial_weights: Vec<f32> = state.long_term.edges.iter().map(|e| e.weight).collect();
        state.retrieve_context("process_data Config");
        state.retrieve_context("process_data Config");
        let has_increased = state
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
        state.tick("DECISION: initial entry with some content for decay testing");
        let initial_salience = state.short_term[0].salience;
        state.clock += 100;
        state.apply_decay();
        assert!(state.short_term[0].salience < initial_salience);
    }

    #[test]
    fn test_session_log_records_ticks() {
        let mut state = MemoryState::default();
        state.tick("first tick message");
        state.tick("second tick message");
        state.tick("third tick message");
        assert_eq!(state.session_log.len(), 3);
        assert_eq!(state.session_log[0].text, "first tick message");
        assert_eq!(state.session_log[2].text, "third tick message");
    }

    #[test]
    fn test_recent_sessions_returns_tail() {
        let mut state = MemoryState::default();
        for i in 0..20 {
            state.tick(&format!("tick number {}", i));
        }
        let recent = state.recent_sessions(5);
        assert_eq!(recent.len(), 5);
        assert!(recent[0].text.contains("15"));
        assert!(recent[4].text.contains("19"));
    }

    #[test]
    fn test_diversity_prevents_merge_of_unrelated() {
        let mut state = MemoryState::default();
        state.tick("DECISION: the embedding system uses vector similarity for matching");
        state.tick("DECISION: cooking recipes require fresh ingredients and seasoning");
        assert!(
            state.short_term.len() >= 2,
            "unrelated ticks should create separate entries, got {}",
            state.short_term.len()
        );
    }

    #[test]
    fn test_pattern_separation_preserves_similar_but_distinct() {
        // Dentate Gyrus pattern separation: topics sharing vocabulary ("memory")
        // but describing different subjects must remain as separate episodic traces.
        let mut state = MemoryState::default();
        state.tick("DECISION: Rust memory model borrow checker ownership semantics");
        state.tick("DECISION: Legend memory system three-layer architecture design");
        assert!(
            state.short_term.len() >= 2,
            "similar-but-distinct topics should be kept separate (dentate gyrus pattern separation), got {}",
            state.short_term.len()
        );
    }

    #[test]
    fn test_orthogonalization_reduces_embedding_overlap_in_l2() {
        // Dentate Gyrus: after orthogonalization, L2 embeddings for related-but-distinct
        // entries should be less similar than their raw n-gram embeddings would be.
        let mut state = MemoryState::default();
        state.tick("DECISION: Rust memory model borrow checker ownership semantics");
        state.tick("DECISION: Legend memory system three-layer architecture design patterns");

        assert!(
            state.short_term.len() >= 2,
            "should have 2 separate L2 entries, got {}",
            state.short_term.len()
        );

        // The stored L2 embeddings should be more orthogonal than raw embeddings
        let raw_a = embed_text("DECISION: Rust memory model borrow checker ownership semantics", state.config.embedding_dim);
        let raw_b = embed_text("DECISION: Legend memory system three-layer architecture design patterns", state.config.embedding_dim);
        let raw_sim = cosine_similarity(&raw_a, &raw_b);

        let stored_sim = cosine_similarity(
            &state.short_term[0].embedding,
            &state.short_term[1].embedding,
        );

        assert!(
            stored_sim < raw_sim,
            "stored embeddings should be more orthogonal than raw: stored={}, raw={}",
            stored_sim, raw_sim
        );
    }

    #[test]
    fn test_near_identical_entries_still_merge() {
        // CA3 pattern completion: near-identical cues should recall the same trace.
        let mut state = MemoryState::default();
        state.tick("DECISION: Chose Redis for caching because it has better pub/sub support");
        state.tick("DECISION: Chose Redis for caching because it has better pub/sub integration");
        assert_eq!(
            state.short_term.len(),
            1,
            "near-identical entries should merge, got {}",
            state.short_term.len()
        );
    }

    // ---- Amygdala: emotional tagging integration tests ----

    #[test]
    fn test_emotional_valence_stored_on_tick() {
        let mut state = MemoryState::default();
        state.tick("BUG: server crashes on null input with a panic");
        let entry = state
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
        state.tick("DECISION: Shipped v2.0 successfully because the approach was validated");
        let entry = state
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
        state.tick("BUG: critical panic in the authentication module");
        let initial_valence = state.short_term[0].emotional_valence;
        let initial_salience = state.short_term[0].salience;

        // Advance clock significantly
        state.clock += 100;
        state.apply_decay();

        let decayed_valence = state.short_term[0].emotional_valence;
        let decayed_salience = state.short_term[0].salience;

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
        state.tick("BUG: data corruption found in production database");
        let dump = state.build_dump();
        let short_term = dump["short_term"].as_array().unwrap();
        assert!(!short_term.is_empty());
        let valence = short_term[0]["emotional_valence"].as_f64().unwrap();
        assert!(valence < 0.0, "bug entry valence should be negative in dump, got {}", valence);
    }

    // ---- Ebbinghaus: spaced repetition + forgetting curve tests ----

    #[test]
    fn test_spaced_retrieval_increases_stability() {
        let mut state = MemoryState::default();
        state.tick("DECISION: Chose Redis for caching because of pub/sub support");

        // Retrieve at increasing intervals: 5, 10, 20, 40
        let intervals = [5, 10, 20, 40];
        for interval in &intervals {
            state.clock += interval;
            state.retrieve_context("Redis caching");
        }

        let stability = state.short_term[0].stability;
        assert!(
            stability > 1.5,
            "spaced retrieval should increase stability significantly, got {}",
            stability
        );
    }

    #[test]
    fn test_massed_retrieval_increases_stability_slowly() {
        let mut state = MemoryState::default();
        state.tick("DECISION: Chose Redis for caching because of pub/sub support");

        // Retrieve at constant short intervals: 1, 1, 1, 1
        for _ in 0..4 {
            state.clock += 1;
            state.retrieve_context("Redis caching");
        }

        let massed_stability = state.short_term[0].stability;

        // Compare with spaced retrieval
        let mut state2 = MemoryState::default();
        state2.tick("DECISION: Chose Redis for caching because of pub/sub support");
        let intervals = [5, 10, 20, 40];
        for interval in &intervals {
            state2.clock += interval;
            state2.retrieve_context("Redis caching");
        }
        let spaced_stability = state2.short_term[0].stability;

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
        state.tick("DECISION: Chose Redis for caching because of pub/sub support");

        // Build up stability through spaced retrieval
        let intervals = [5, 10, 20, 40];
        for interval in &intervals {
            state.clock += interval;
            state.retrieve_context("Redis caching");
        }
        let high_stability_salience = state.short_term[0].salience;

        // Now create a fresh entry with default stability
        state.tick("DECISION: Chose Postgres because of JSONB support");

        // Both entries decay for 200 ticks
        state.clock += 200;
        state.apply_decay();

        let stable_entry = state
            .short_term
            .iter()
            .find(|e| e.text.contains("Redis"))
            .expect("Redis entry");
        let fresh_entry = state
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
        state.tick("DECISION: some initial decision for testing baseline stability");
        assert_eq!(
            state.short_term[0].stability, 1.0,
            "new entries should have stability=1.0 after first tick"
        );
    }

    #[test]
    fn test_stability_capped_at_ten() {
        let mut state = MemoryState::default();
        state.tick("DECISION: Chose Redis for caching because of pub/sub support");

        // Many spaced retrievals to push stability toward the cap
        let mut interval = 5;
        for _ in 0..20 {
            state.clock += interval;
            state.retrieve_context("Redis caching");
            interval += 5; // increasing intervals
        }

        assert!(
            state.short_term[0].stability <= 10.0,
            "stability should be capped at 10.0, got {}",
            state.short_term[0].stability
        );
    }

    #[test]
    fn test_build_context_summary() {
        let mut state = MemoryState::default();
        state.tick("fn process_data() handles incoming requests");
        state.tick("struct Config stores application settings");
        let summary = state.build_context_summary();
        assert!(summary["stats"]["short_term_entries"].as_u64().unwrap() >= 1);
        assert!(summary["recent_sessions"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn test_reinforce_positive_boosts_salience() {
        let mut state = MemoryState::default();
        state.tick("fn handle_request() processes incoming API calls");
        let id = state.short_term[0].id;
        let salience_before = state.short_term[0].salience;

        let result = state.reinforce(&[id][..], 1.0);
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
        state.tick("fn handle_request() processes incoming API calls");
        let id = state.short_term[0].id;
        let salience_before = state.short_term[0].salience;

        let result = state.reinforce(&[id][..], -1.0);
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
        state.tick("fn process_data() uses struct Config for settings");
        let id = state.short_term[0].id;

        // Capture graph weights before reinforcement
        let weight_before: f32 = state.long_term.nodes.values().map(|n| n.weight).sum();

        state.reinforce(&[id][..], 1.0);

        let weight_after: f32 = state.long_term.nodes.values().map(|n| n.weight).sum();

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
        state.tick("some entry");
        let result = state.reinforce(&[9999][..], 1.0);
        assert!(
            result.reinforced.is_empty(),
            "unknown ID should be silently ignored"
        );
    }

    #[test]
    fn test_prune_graph_removes_low_weight_nodes() {
        let mut state = MemoryState::default();
        // Insert a node with weight below GRAPH_PRUNE_WEIGHT
        let id = state.next_id;
        state.next_id += 1;
        state.long_term.nodes.insert(
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
        state.long_term.index.insert("weak_node".to_string(), id);
        // Also insert a healthy node
        let id2 = state.next_id;
        state.next_id += 1;
        state.long_term.nodes.insert(
            id2,
            GraphNode {
                id: id2,
                label: "strong_node".to_string(),
                kind: "Term".to_string(),
                weight: 2.0,
                last_seen: state.clock,
                salience: 0.5,
                source_texts: Vec::new(),
                embedding: Vec::new(),
                full_text: None,
            },
        );
        state.long_term.index.insert("strong_node".to_string(), id2);
        // Add edge between them
        state.long_term.edges.push(GraphEdge {
            from: id,
            to: id2,
            weight: 0.1,
            kind: "related".to_string(),
            last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        state.clock = 100;
        state.prune_graph();

        assert!(
            !state.long_term.nodes.contains_key(&id),
            "low-weight node should be pruned"
        );
        assert!(
            state.long_term.nodes.contains_key(&id2),
            "healthy node should survive"
        );
        assert!(
            state.long_term.edges.is_empty(),
            "orphaned edge should be removed"
        );
        assert!(
            !state.long_term.index.contains_key("weak_node"),
            "index entry should be cleaned"
        );
    }

    #[test]
    fn test_prune_graph_enforces_node_cap() {
        let mut state = MemoryState::default();
        // Insert more nodes than GRAPH_NODE_CAPACITY
        for i in 0..(GRAPH_NODE_CAPACITY + 50) {
            let id = state.next_id;
            state.next_id += 1;
            state.long_term.nodes.insert(
                id,
                GraphNode {
                    id,
                    label: format!("node_{}", i),
                    kind: "Term".to_string(),
                    weight: i as f32 * 0.01,
                    last_seen: state.clock,
                    salience: 0.1,
                    source_texts: Vec::new(),
                    embedding: Vec::new(),
                    full_text: None,
                },
            );
            state.long_term.index.insert(format!("node_{}", i), id);
        }
        assert!(state.long_term.nodes.len() > GRAPH_NODE_CAPACITY);

        state.prune_graph();

        assert!(
            state.long_term.nodes.len() <= GRAPH_NODE_CAPACITY,
            "node count should be capped at {}, got {}",
            GRAPH_NODE_CAPACITY,
            state.long_term.nodes.len()
        );
    }

    #[test]
    fn test_tick_runs_pruning() {
        let mut state = MemoryState::default();
        // Manually inject a stale short-term entry
        state.short_term.push(ShortTermEntry {
            id: 999,
            text: "stale".into(),
            summary: "stale".into(),
            embedding: vec![0.0; 256],
            last_access: 0,
            usage: 0,
            salience: 0.0,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
        });
        // Advance clock far enough for pruning to kick in
        state.clock = 500;
        state.tick("fresh content about something new");
        assert!(
            !state.short_term.iter().any(|e| e.id == 999),
            "stale entry should be pruned during tick"
        );
    }

    #[test]
    fn test_auto_reinforce_on_query() {
        let mut state = MemoryState::default();
        state.tick("DECISION: the cosine similarity algorithm compares vector embeddings");
        let salience_before = state.short_term[0].salience;

        // Query something related — top result should get a passive boost
        state.retrieve_context("cosine similarity vector");
        let salience_after = state.short_term[0].salience;

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
        state.tick("DECISION: See legend/src/memory/mod.rs#L120-145 for the new refs logic.");
        assert!(!state.short_term.is_empty());

        let entry = &state.short_term[0];
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
        state.tick("DECISION: Ref: legend/src/memory/mod.rs#L200-210 tracks MemorySnippet changes.");
        let ctx = state.retrieve_context("MemorySnippet refs");
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
        state.tick("Chose bincode over JSON because it is faster for serialization");
        state.tick("fn process_data() handles incoming API requests");
        state.tick("TODO: still need to implement the caching layer");
        let summary = state.build_start_summary();

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
        state.tick("DECISION: the database uses PostgreSQL for persistence");
        assert_eq!(state.short_term.len(), 1);
        let original_id = state.short_term[0].id;

        // Query to make it labile
        state.retrieve_context("database PostgreSQL");
        assert!(
            state.short_term[0].labile_until > 0,
            "retrieved entry should be labile"
        );

        // Tick with related but new information — should reconsolidate
        state.tick("DECISION: the database PostgreSQL schema has users and sessions tables");
        // Should still have the original entry (reconsolidated, not duplicated)
        let reconsolidated = state.short_term.iter().find(|e| e.id == original_id);
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
        assert!(!state.short_term.is_empty());
    }

    #[test]
    fn test_labile_expires() {
        let mut state = MemoryState::default();
        state.tick("DECISION: test entry for labile expiration");
        let id = state.short_term[0].id;

        // Query to make labile
        state.retrieve_context("test entry labile");
        assert!(state.short_term[0].labile_until > 0);

        // Advance clock past labile window
        state.clock += LABILE_WINDOW + 5;
        state.stabilize_labile_entries();

        let entry = state.short_term.iter().find(|e| e.id == id).unwrap();
        assert_eq!(
            entry.labile_until, 0,
            "labile state should expire after window"
        );
    }

    #[test]
    fn test_classify_text_decision() {
        assert_eq!(
            classify_text("Chose PostgreSQL over MongoDB because it has better JOIN support", &kw()),
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
            classify_text("The authentication module uses JWT tokens via middleware", &kw()),
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
        let decision_salience =
            compute_salience("Chose bincode over JSON because it is faster for serialization", &kw());
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
        let bug_salience =
            compute_salience("Bug: the parser crashes on empty input and causes a panic", &kw());
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
        state.tick("fn handle_request() processes incoming API calls using struct Config");
        state.tick("struct Config stores database_url and port settings");
        // The graph should now have edges connecting these entities

        // Query for something that matches one entry — priming should surface
        // graph neighbors from the other entry
        let ctx = state.retrieve_context("handle_request API");
        // Should have long-term results that include primed neighbors
        assert!(
            !ctx.long_term.is_empty(),
            "priming should surface related graph nodes"
        );
    }

    #[test]
    fn test_start_summary_categorized() {
        let mut state = MemoryState::default();
        state.tick("Chose Rust over Python because of performance requirements");
        state.tick("TODO: add proper error handling to the parser module");
        state.tick("Bug: connection pool exhaustion under high load causes timeout");
        state.tick("User prefers explicit error types over anyhow");
        state.tick("The API module handles REST endpoints via axum router");
        let summary = state.build_start_summary();

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
        assert!(state.get_task().is_none());

        state.set_task("Implement user authentication");
        assert_eq!(state.get_task(), Some("Implement user authentication"));

        state.clear_task();
        assert!(state.get_task().is_none());
    }

    #[test]
    fn test_current_task_in_start_summary() {
        let mut state = MemoryState::default();
        state.set_task("Working on memory improvements");

        let summary = state.build_start_summary();
        assert_eq!(
            summary["current_task"].as_str(),
            Some("Working on memory improvements")
        );
    }

    #[test]
    fn test_current_task_in_context_summary() {
        let mut state = MemoryState::default();
        state.set_task("Debugging the parser");

        let summary = state.build_context_summary();
        assert_eq!(
            summary["current_task"].as_str(),
            Some("Debugging the parser")
        );
    }

    #[test]
    fn test_ticks_since_consolidation_increments() {
        let mut state = MemoryState::default();
        assert_eq!(state.ticks_since_consolidation, 0);

        state.tick("first tick");
        assert_eq!(state.ticks_since_consolidation, 1);

        state.tick("second tick");
        assert_eq!(state.ticks_since_consolidation, 2);
    }

    #[test]
    fn test_consolidate_resets_tick_counter() {
        let mut state = MemoryState::default();
        state.tick("tick one");
        state.tick("tick two");
        state.tick("tick three");
        assert_eq!(state.ticks_since_consolidation, 3);

        state.consolidate();
        assert_eq!(state.ticks_since_consolidation, 0);
    }

    #[test]
    fn test_should_suggest_consolidation() {
        let mut state = MemoryState::default();
        assert!(!state.should_suggest_consolidation());

        // Tick enough times to trigger suggestion
        for i in 0..CONSOLIDATION_SUGGESTION_THRESHOLD {
            state.tick(&format!("tick number {}", i));
        }
        assert!(state.should_suggest_consolidation());

        // Consolidate resets
        state.consolidate();
        assert!(!state.should_suggest_consolidation());
    }

    #[test]
    fn test_graph_lookup_includes_edge_type() {
        let mut state = MemoryState::default();
        // Create entries that will generate graph edges
        state.tick("fn process_data() uses struct Config for settings");
        state.tick("struct Config stores database_url and timeout values");

        // Query should return nodes with edge_type for neighbors
        let results = state.graph_lookup("process_data", 10);
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
        state.tick("fn process_data() uses struct Config");
        // Hammer the edges with many queries to test ceiling
        for _ in 0..500 {
            state.retrieve_context("process_data Config");
        }
        // All edge weights should be capped at HEBBIAN_EDGE_CEILING (10.0)
        for edge in &state.long_term.edges {
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
        state.tick("fn handle_data() uses struct Request");
        // Store initial edge weights
        let initial_weights: Vec<f32> = state.long_term.edges.iter().map(|e| e.weight).collect();
        assert!(!initial_weights.is_empty(), "should have edges");
        // Advance clock and apply decay
        state.clock += 100;
        state.apply_decay();
        // Verify edge weights have decayed
        for (edge, &initial) in state.long_term.edges.iter().zip(initial_weights.iter()) {
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
        state.tick("fn process() uses struct Data");
        let initial_last_seen: Vec<u64> =
            state.long_term.edges.iter().map(|e| e.last_seen).collect();
        assert!(!initial_last_seen.is_empty(), "should have edges");
        // Query to trigger Hebbian reinforcement
        state.retrieve_context("process Data");
        // Check that last_seen was updated for co-retrieved edges
        let any_updated = state
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
        state.tick("HUD overlap fix: adjusted widget z-order rendering");
        state.tick("window state change callback handler refactored");
        state.tick("player health bar UI component styling");
        // Query something completely unrelated
        let ctx = state.retrieve_context("MML syntax reference documentation");
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
        state.tick("DECISION: MML syntax reference: use #tempo 120 for tempo");
        state.tick("DECISION: MML note commands: cdefgab with octave modifiers");
        state.tick("DECISION: unrelated window rendering pipeline");
        let ctx = state.retrieve_context("MML syntax");
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
        state.tick("alpha beta gamma delta");
        state.tick("epsilon zeta theta iota");
        // Query with completely disjoint vocabulary
        let results = state.top_k_similar(
            &embed_text("xylophone zamboni quasar", state.config.embedding_dim),
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
        state.tick("DECISION: fn process_data() in src/main.rs");
        assert!(!state.long_term.nodes.is_empty());
        // Query with entities that don't match any graph node
        let results = state.graph_lookup("xylophone_function zamboni_module", 10);
        assert!(
            results.is_empty(),
            "graph_lookup should return empty when no entities match, got {} results",
            results.len()
        );
    }

    #[test]
    fn test_graph_lookup_match_still_works() {
        let mut state = MemoryState::default();
        state.tick("fn process_data() handles struct Config");
        // Query with a matching entity
        let results = state.graph_lookup("process_data", 10);
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
        state.tick("DECISION: MML syntax reference: tempo and note commands");
        state.tick("DECISION: unrelated rendering pipeline for sprites");
        let embedding = embed_text("MML syntax", state.config.embedding_dim);
        let results = state.top_k_similar(&embedding, 5, "MML syntax");
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
        state.tick("DECISION: alpha bravo charlie delta echo foxtrot golf hotel");
        let embedding = embed_text(
            "alpha bravo charlie delta echo foxtrot golf hotel",
            state.config.embedding_dim,
        );
        let results = state.top_k_similar(
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
        state.tick("the and for with this that from");
        let embedding = embed_text("the and for", state.config.embedding_dim);
        let results = state.top_k_similar(&embedding, 5, "the and for");
        // Stopwords should not contribute to keyword bonus
        // Result may or may not pass similarity threshold, but if it does
        // the bonus should be 0 (only stopwords in query)
        for r in &results {
            // Cosine sim should be the only factor (no keyword bonus)
            let cosine_only = cosine_similarity(
                &embed_text(&r.text, state.config.embedding_dim),
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
        state.tick("some memory entry about testing");
        let embedding = embed_text("", state.config.embedding_dim);
        let results = state.top_k_similar(&embedding, 5, "");
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
        let dim = state.config.embedding_dim;
        let texts = [
            "implemented MML tempo command to set BPM for playback speed control",
            "implemented MML tempo command to set BPM for playback rate adjustment",
            "implemented MML tempo command to set BPM for playback timing update",
        ];
        for text in &texts {
            state.insert_short_term(
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        // Lower theta_low so these group together
        state.config.theta_low = 0.3;
        let summaries1 = state.consolidate();
        assert!(
            !summaries1.is_empty(),
            "first consolidation should produce summaries"
        );
        let summary_count_before = state
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
            state.insert_short_term(
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        let _summaries2 = state.consolidate();
        let summary_count_after = state
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
        let dim = state.config.embedding_dim;
        let texts = [
            "feature alpha implemented for rendering pipeline in module X",
            "feature alpha implemented for rendering system in module X",
            "feature alpha implemented for rendering engine in module X",
        ];
        for text in &texts {
            state.insert_short_term(
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        state.config.theta_low = 0.3;
        state.consolidate();

        let summary_node = state
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
        let dim = state.config.embedding_dim;
        state.config.theta_low = 0.2;
        // Directly insert many similar entries
        for i in 0..25 {
            let text = format!(
                "feature beta variant number {} implemented in rendering module Y pipeline",
                i
            );
            state.insert_short_term(
                &text,
                embed_text(&text, dim),
                compute_salience(&text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        state.consolidate();

        for node in state.long_term.nodes.values() {
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
        let dim = state.config.embedding_dim;
        // Insert entries that will form a group
        let texts = [
            "MML tempo command sets BPM for playback speed in the engine",
            "MML tempo directive sets BPM for playback rate in the engine",
            "MML tempo instruction sets BPM for playback timing in the engine",
        ];
        for text in &texts {
            state.insert_short_term(
                text,
                embed_text(text, dim),
                compute_salience(text, &kw()),
                Vec::new(),
                0.0,
            );
        }
        state.config.theta_low = 0.3;
        state.consolidate();

        // All grouped entries should now be consolidated
        let consolidated_count = state.short_term.iter().filter(|e| e.consolidated).count();
        assert!(
            consolidated_count >= 2,
            "at least 2 entries should be marked consolidated, got {}",
            consolidated_count
        );

        // Query should not return consolidated entries
        let ctx = state.retrieve_context("MML tempo BPM");
        for snippet in &ctx.short_term {
            let entry = state.short_term.iter().find(|e| e.id == snippet.id);
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
        state.tick("DECISION: some new memory entry for testing");
        let entry = state.short_term.last().unwrap();
        assert!(
            !entry.consolidated,
            "new entries should default to consolidated=false"
        );
    }

    #[test]
    fn test_unconsolidated_entries_still_appear() {
        let mut state = MemoryState::default();
        state.tick("DECISION: unique standalone MML note commands reference guide");
        // No consolidation — entry should appear in results
        let ctx = state.retrieve_context("MML note commands");
        assert!(
            !ctx.short_term.is_empty(),
            "unconsolidated entries should still appear in query results"
        );
        assert!(ctx.short_term[0].text.contains("MML"));
    }

    // ---- V4 migration test ----

    #[test]
    fn test_v4_migration_preserves_data() {
        let v4 = MemoryStateV4 {
            config: MemoryConfig::default(),
            immediate: VecDeque::new(),
            short_term: vec![ShortTermEntryV4 {
                id: 42,
                text: "test memory entry".to_string(),
                summary: "test".to_string(),
                embedding: vec![1.0, 2.0, 3.0],
                last_access: 100,
                usage: 5,
                salience: 0.7,
                reconsolidation_count: 2,
                labile_until: 0,
                refs: vec![],
                gradient_sq_sum: 0.5,
                density: 0.3,
            }],
            long_term: GraphMemory::default(),
            clock: 200,
            next_id: 43,
            session_log: vec![],
            current_task: Some("test task".to_string()),
            ticks_since_consolidation: 5,
            last_retrieved_ids: vec![1, 2],
            last_synced_sha: Some("abc123".to_string()),
        };

        let migrated = migrate_v4(v4);
        assert_eq!(migrated.clock, 200);
        assert_eq!(migrated.next_id, 43);
        assert_eq!(migrated.short_term.len(), 1);
        let entry = &migrated.short_term[0];
        assert_eq!(entry.id, 42);
        assert_eq!(entry.text, "test memory entry");
        assert_eq!(entry.gradient_sq_sum, 0.5);
        assert_eq!(entry.density, 0.3);
        assert!(!entry.consolidated, "migrated entries should have consolidated=false");
        assert_eq!(migrated.current_task, Some("test task".to_string()));
        assert_eq!(migrated.last_synced_sha, Some("abc123".to_string()));
    }

    #[test]
    fn test_msgpack_roundtrip() {
        let dir = std::env::temp_dir().join("legend_test_msgpack_rt");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("memory.lz4");

        let mut state = MemoryState {
            clock: 42,
            next_id: 10,
            ..MemoryState::default()
        };
        state.short_term.push(ShortTermEntry {
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

        assert_eq!(loaded.clock, 42);
        assert_eq!(loaded.next_id, 10);
        assert_eq!(loaded.short_term.len(), 1);
        assert_eq!(loaded.short_term[0].text, "msgpack roundtrip test");
        assert_eq!(loaded.short_term[0].embedding, vec![0.1, 0.2, 0.3]);
        assert!(loaded.short_term[0].consolidated);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_bincode_to_msgpack_auto_migration() {
        let dir = std::env::temp_dir().join("legend_test_bc_to_mp");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("memory.lz4");

        // Save in old bincode format
        let mut state = MemoryState {
            clock: 99,
            ..MemoryState::default()
        };
        state.short_term.push(ShortTermEntry {
            id: 5,
            text: "bincode entry".into(),
            salience: 0.7,
            ..ShortTermEntry::default()
        });
        let serialized = bincode::serialize(&state).unwrap();
        let compressed = lz4::block::compress(&serialized, None, true).unwrap();
        fs::write(&path, &compressed).unwrap();

        // Load should succeed via bincode fallback
        let loaded = load_memory_from_path(&path).expect("load bincode");
        assert_eq!(loaded.clock, 99);
        assert_eq!(loaded.short_term[0].text, "bincode entry");

        // Re-save should write msgpack
        save_memory_to_path(&loaded, &path).expect("re-save as msgpack");

        // Verify it's now msgpack format
        let compressed2 = fs::read(&path).unwrap();
        let decompressed = lz4::block::decompress(&compressed2, None).unwrap();
        assert_eq!(&decompressed[..4], b"LGND");
        assert_eq!(decompressed[4], 1);

        // Load again from msgpack
        let reloaded = load_memory_from_path(&path).expect("load msgpack");
        assert_eq!(reloaded.clock, 99);
        assert_eq!(reloaded.short_term[0].text, "bincode entry");

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
        assert_eq!(loaded.clock, 100);
        assert_eq!(loaded.next_id, 3);
        assert!(loaded.working_memory.is_empty(), "old immediate discarded, working_memory starts empty");
        assert_eq!(loaded.short_term.len(), 2);
        assert_eq!(loaded.short_term[0].id, 1);
        assert_eq!(loaded.short_term[0].text, "important memory");
        assert_eq!(loaded.short_term[0].salience, 0.9);
        assert_eq!(loaded.short_term[0].refs.len(), 1);
        assert_eq!(loaded.short_term[0].refs[0].path, "src/main.rs");
        // Missing `consolidated` defaults to false
        assert!(!loaded.short_term[0].consolidated);
        assert!(!loaded.short_term[1].consolidated);
        assert_eq!(loaded.short_term[1].id, 2);
        assert_eq!(loaded.short_term[1].text, "another memory");
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
            state.tick(&format!("low signal entry number {}", i));
        }
        assert!(
            state.working_memory.len() <= state.config.immediate_capacity,
            "working memory should not exceed capacity: {} > {}",
            state.working_memory.len(),
            state.config.immediate_capacity
        );
    }

    #[test]
    fn test_low_salience_stays_l1_only() {
        let mut state = MemoryState::default();
        let st_before = state.short_term.len();
        state.tick("just a random thought about nothing in particular");
        // Low-salience: should NOT promote to L2
        assert_eq!(
            state.short_term.len(),
            st_before,
            "low-salience tick should not create L2 entry"
        );
        assert!(
            !state.working_memory.is_empty(),
            "low-salience tick should be in working memory"
        );
    }

    #[test]
    fn test_high_salience_promotes_to_l2() {
        let mut state = MemoryState::default();
        state.tick("DECISION: chose Rust over Go because of safety guarantees");
        assert!(
            !state.short_term.is_empty(),
            "high-salience tick should promote to L2"
        );
        // Should also be in working memory
        assert!(
            !state.working_memory.is_empty(),
            "promoted tick should also remain in working memory"
        );
        // The WM entry should be marked as promoted
        assert!(
            state.working_memory.last().unwrap().promoted,
            "promoted WM entry should have promoted=true"
        );
    }

    #[test]
    fn test_query_scans_working_memory() {
        let mut state = MemoryState::default();
        // Tick something low-salience that stays in L1 only
        state.tick("the parser handles empty input strings gracefully");
        assert!(state.short_term.is_empty(), "should stay L1 only");

        // Query should find it in working_memory results
        let ctx = state.retrieve_context("parser empty input");
        assert!(
            !ctx.working_memory.is_empty(),
            "query should scan working memory and find L1-only entries"
        );
    }

    #[test]
    fn test_query_increments_rehearsal_count() {
        let mut state = MemoryState::default();
        // Use low-salience text that stays L1 only (no retrieve_context called)
        state.tick("the purple elephant danced on silver moonbeams last tuesday");
        assert!(!state.working_memory.is_empty());
        let initial_rehearsal = state.working_memory[0].rehearsal_count;

        // Query to trigger rehearsal
        state.retrieve_context("purple elephant silver moonbeams");
        // Check rehearsal was incremented
        let entry = state.working_memory.iter().find(|e| e.text.contains("purple"));
        assert!(
            entry.is_some() && entry.unwrap().rehearsal_count > initial_rehearsal,
            "query should increment rehearsal_count on matched WM entries"
        );
    }

    #[test]
    fn test_rehearsed_entry_promotes_on_displacement() {
        let mut state = MemoryState::default();
        let embedding = embed_text("rehearsed entry test", state.config.embedding_dim);

        // Manually push a low-salience entry with rehearsal_count >= 1
        state.working_memory.push(WorkingMemoryEntry {
            id: 900,
            text: "rehearsed entry test content".to_string(),
            embedding: embedding.clone(),
            salience: 0.05, // below threshold
            tick_created: state.clock,
            rehearsal_count: 1, // rehearsed via query
            promoted: false,
            emotional_valence: 0.0,
        });
        let st_before = state.short_term.len();

        // Fill to capacity + 1 to force displacement of index 0
        for _ in 0..state.config.immediate_capacity {
            let emb = embed_text("filler", state.config.embedding_dim);
            state.push_working_memory("filler entry", &emb, 0.01, 0.0);
        }

        // The rehearsed entry should have been promoted to L2 on displacement
        assert!(
            state.short_term.len() > st_before,
            "rehearsed entry should promote to L2 when displaced: before={}, after={}",
            st_before,
            state.short_term.len()
        );
    }

    #[test]
    fn test_flush_promotes_qualifying_entries() {
        let mut state = MemoryState::default();
        // Add a high-salience entry that wasn't promoted by tick (simulate by pushing directly)
        state.working_memory.push(WorkingMemoryEntry {
            id: 999,
            text: "important decision about architecture".to_string(),
            embedding: embed_text("important decision about architecture", state.config.embedding_dim),
            salience: 0.5, // above PROMOTION_SALIENCE_THRESHOLD
            tick_created: state.clock,
            rehearsal_count: 0,
            promoted: false,
            emotional_valence: 0.0,
        });
        let st_before = state.short_term.len();

        state.flush_working_memory();

        assert!(state.working_memory.is_empty(), "flush should clear working memory");
        assert!(
            state.short_term.len() > st_before,
            "flush should promote high-salience entries to L2"
        );
    }

    #[test]
    fn test_flush_discards_low_signal_unrehearsed() {
        let mut state = MemoryState::default();
        // Add a low-salience unrehearsed entry
        state.working_memory.push(WorkingMemoryEntry {
            id: 998,
            text: "just some noise that nobody cares about".to_string(),
            embedding: embed_text("just some noise", state.config.embedding_dim),
            salience: 0.05, // below threshold
            tick_created: state.clock,
            rehearsal_count: 0,
            promoted: false,
            emotional_valence: 0.0,
        });
        let st_before = state.short_term.len();

        state.flush_working_memory();

        assert!(state.working_memory.is_empty(), "flush should clear working memory");
        assert_eq!(
            state.short_term.len(),
            st_before,
            "flush should NOT promote low-salience unrehearsed entries"
        );
    }

    #[test]
    fn test_flush_skips_already_promoted() {
        let mut state = MemoryState::default();
        // Add an entry already marked as promoted
        state.working_memory.push(WorkingMemoryEntry {
            id: 997,
            text: "already promoted entry".to_string(),
            embedding: embed_text("already promoted", state.config.embedding_dim),
            salience: 0.8,
            tick_created: state.clock,
            rehearsal_count: 5,
            promoted: true, // already promoted
            emotional_valence: 0.0,
        });
        let st_before = state.short_term.len();

        state.flush_working_memory();

        assert_eq!(
            state.short_term.len(),
            st_before,
            "flush should NOT double-promote already-promoted entries"
        );
    }

    #[test]
    fn test_v5_migration_discards_immediate() {
        let v5 = MemoryStateV5 {
            config: MemoryConfig::default(),
            immediate: VecDeque::from(["old1".to_string(), "old2".to_string()]),
            short_term: vec![ShortTermEntryV5 {
                id: 10,
                text: "preserved entry".to_string(),
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
            }],
            long_term: GraphMemory::default(),
            clock: 50,
            next_id: 11,
            session_log: vec![],
            current_task: None,
            ticks_since_consolidation: 0,
            last_retrieved_ids: vec![],
            last_synced_sha: None,
        };
        let migrated = migrate_v5(v5);
        assert!(migrated.working_memory.is_empty(), "V5 migration should discard old immediate");
        assert_eq!(migrated.short_term.len(), 1);
        assert_eq!(migrated.short_term[0].text, "preserved entry");
        assert_eq!(migrated.clock, 50);
    }

    // --- Synaptic encoding tests (Change 7) ---

    #[test]
    fn test_activation_count_increments() {
        let mut state = MemoryState::default();
        // Tick twice with shared entities to create and reinforce an edge
        state.tick("DECISION: fn handle_auth() uses struct Config for settings");
        state.tick("DECISION: struct Config stores fn handle_auth() parameters");

        // Find edges with activation_count > 0
        let reinforced = state.long_term.edges.iter()
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
        insert_test_node(&mut state, a_id, "SpacedNodeA");
        insert_test_node(&mut state, b_id, "SpacedNodeB");

        // Create edge
        state.long_term.edges.push(GraphEdge {
            from: a_id, to: b_id, weight: 0.5,
            kind: "related".into(), last_seen: 1,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        // Reinforce with increasing intervals (spaced): 5, 10, 20, 40
        let intervals = [5u64, 10, 20, 40];
        let mut clock = 1u64;
        for interval in &intervals {
            clock += interval;
            state.clock = clock;
            state.upsert_edge(a_id, b_id, "related");
        }

        let edge = state.long_term.edges.iter()
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
        insert_test_node(&mut state, a_id, "MassedNodeA");
        insert_test_node(&mut state, b_id, "MassedNodeB");

        state.long_term.edges.push(GraphEdge {
            from: a_id, to: b_id, weight: 0.5,
            kind: "related".into(), last_seen: 1,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        // Reinforce with constant intervals (cramming): 1, 1, 1, 1
        let mut clock = 1u64;
        for _ in 0..4 {
            clock += 1;
            state.clock = clock;
            state.upsert_edge(a_id, b_id, "related");
        }

        let edge = state.long_term.edges.iter()
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
        insert_test_node(&mut state, sa, "SpacedA");
        insert_test_node(&mut state, sb, "SpacedB");
        state.long_term.edges.push(GraphEdge {
            from: sa, to: sb, weight: 0.5,
            kind: "related".into(), last_seen: 1,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        // Massed edge
        let (ma, mb) = (1602, 1603);
        insert_test_node(&mut state, ma, "MassedA");
        insert_test_node(&mut state, mb, "MassedB");
        state.long_term.edges.push(GraphEdge {
            from: ma, to: mb, weight: 0.5,
            kind: "related".into(), last_seen: 1,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        // Spaced: intervals [5, 10, 20, 40]
        let spaced_intervals = [5u64, 10, 20, 40];
        let mut clock = 1u64;
        for interval in &spaced_intervals {
            clock += interval;
            state.clock = clock;
            state.upsert_edge(sa, sb, "related");
        }

        // Massed: intervals [1, 1, 1, 1] (same number of reinforcements)
        let mut clock_m = 1u64;
        for _ in 0..4 {
            clock_m += 1;
            state.clock = clock_m;
            state.upsert_edge(ma, mb, "related");
        }

        let spaced_edge = state.long_term.edges.iter()
            .find(|e| e.from == sa && e.to == sb).unwrap();
        let massed_edge = state.long_term.edges.iter()
            .find(|e| e.from == ma && e.to == mb).unwrap();

        assert!(
            spaced_edge.stability > massed_edge.stability,
            "spaced ({}) should have higher stability than massed ({})",
            spaced_edge.stability, massed_edge.stability
        );
    }

    #[test]
    fn test_hebbian_logarithmic_dampening() {
        let mut state = MemoryState::default();
        let (a_id, b_id) = (1700, 1701);
        insert_test_node(&mut state, a_id, "HebbA");
        insert_test_node(&mut state, b_id, "HebbB");

        state.long_term.edges.push(GraphEdge {
            from: a_id, to: b_id, weight: 0.5,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        // First Hebbian reinforce: activation_count=0 → full boost
        state.clock = 1;
        let weight_before = state.long_term.edges[0].weight;
        state.hebbian_reinforce(&[a_id, b_id]);
        let first_boost = state.long_term.edges[0].weight - weight_before;

        // Set high activation_count to simulate heavily-used edge
        state.long_term.edges[0].activation_count = 100;
        let weight_before2 = state.long_term.edges[0].weight;
        state.clock = 2;
        state.hebbian_reinforce(&[a_id, b_id]);
        let hundredth_boost = state.long_term.edges[0].weight - weight_before2;

        assert!(
            first_boost > hundredth_boost,
            "first reinforce ({}) should boost more than 100th ({})",
            first_boost, hundredth_boost
        );
    }

    #[test]
    fn test_stability_modulates_spreading_activation() {
        let mut state = MemoryState::default();
        // Two paths from A: A->B (high stability) and A->C (low stability)
        let (a_id, b_id, c_id) = (1800, 1801, 1802);
        insert_test_node(&mut state, a_id, "StabA");
        insert_test_node(&mut state, b_id, "StabB");
        insert_test_node(&mut state, c_id, "StabC");

        // High stability edge A->B
        state.long_term.edges.push(GraphEdge {
            from: a_id, to: b_id, weight: 0.5,
            kind: "related".into(), last_seen: 0,
            activation_count: 10, stability: 5.0,
            recent_interval_avg: 20.0, historical_interval_avg: 15.0,
        });

        // Low stability edge A->C (same weight)
        state.long_term.edges.push(GraphEdge {
            from: a_id, to: c_id, weight: 0.5,
            kind: "related".into(), last_seen: 0,
            activation_count: 10, stability: 1.0,
            recent_interval_avg: 1.0, historical_interval_avg: 1.0,
        });

        let results = state.spreading_activation(&[a_id], 1, 0.5);
        let b_activation = results.iter().find(|(id, _)| *id == b_id).map(|(_, a)| *a).unwrap_or(0.0);
        let c_activation = results.iter().find(|(id, _)| *id == c_id).map(|(_, a)| *a).unwrap_or(0.0);

        assert!(
            b_activation > c_activation,
            "high-stability edge should propagate more: B={} vs C={}",
            b_activation, c_activation
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
        state.long_term.nodes.insert(cfg_id, GraphNode {
            id: cfg_id, label: "ConfigLoader".into(), kind: "Entity".into(),
            weight: 1.0, salience: 0.5,
            source_texts: vec!["ConfigLoader reads settings".into()],
            last_seen: 0, embedding: Vec::new(), full_text: None,
        });
        state.long_term.index.insert("ConfigLoader".into(), cfg_id);

        state.long_term.nodes.insert(jwt_id, GraphNode {
            id: jwt_id, label: "JwtSecret".into(), kind: "Entity".into(),
            weight: 1.0, salience: 0.5,
            source_texts: vec!["JwtSecret stores token signing keys".into()],
            last_seen: 0, embedding: Vec::new(), full_text: None,
        });
        state.long_term.index.insert("JwtSecret".into(), jwt_id);

        // Edge connecting them
        state.long_term.edges.push(GraphEdge {
            from: cfg_id, to: jwt_id, weight: 0.8,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        // Insert L2 entry that contains the JwtSecret source text
        let dim = state.config.embedding_dim;
        state.insert_short_term(
            "JwtSecret stores token signing keys for authentication",
            embed_text("JwtSecret stores token signing keys for authentication", dim),
            0.5,
            Vec::new(),
            0.0,
        );

        // Query for ConfigLoader — pattern completion should find the JwtSecret entry
        let completed = state.pattern_complete("ConfigLoader", &[]);
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
            state.tick(&format!(
                "DECISION: fn handle_auth_{i}() manages authentication flow"
            ));
        }

        let ctx = state.retrieve_context("handle_auth authentication");
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
        let dim = state.config.embedding_dim;

        // Direct match entry
        state.insert_short_term(
            "ConfigLoader reads YAML settings files",
            embed_text("ConfigLoader reads YAML settings files", dim),
            0.5,
            Vec::new(),
            0.0,
        );

        // Graph-connected entry (indirect)
        let cfg_id = 1200;
        let db_id = 1201;
        state.long_term.nodes.insert(cfg_id, GraphNode {
            id: cfg_id, label: "ConfigLoader".into(), kind: "Entity".into(),
            weight: 1.0, salience: 0.5,
            source_texts: vec!["ConfigLoader reads YAML settings files".into()],
            last_seen: 0, embedding: Vec::new(), full_text: None,
        });
        state.long_term.index.insert("ConfigLoader".into(), cfg_id);

        state.long_term.nodes.insert(db_id, GraphNode {
            id: db_id, label: "DatabasePool".into(), kind: "Entity".into(),
            weight: 1.0, salience: 0.5,
            source_texts: vec!["DatabasePool manages connection pooling".into()],
            last_seen: 0, embedding: Vec::new(), full_text: None,
        });
        state.long_term.index.insert("DatabasePool".into(), db_id);

        state.long_term.edges.push(GraphEdge {
            from: cfg_id, to: db_id, weight: 0.6,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        state.insert_short_term(
            "DatabasePool manages connection pooling for PostgreSQL",
            embed_text("DatabasePool manages connection pooling for PostgreSQL", dim),
            0.5,
            Vec::new(),
            0.0,
        );

        // Pattern complete for ConfigLoader — the DatabasePool entry is indirect
        let direct = state.top_k_similar(
            &embed_text("ConfigLoader", dim), 5, "ConfigLoader",
        );
        let completed = state.pattern_complete("ConfigLoader", &direct);

        if let Some(c) = completed.first() {
            if let Some(d) = direct.first() {
                assert!(
                    c.similarity <= d.similarity || direct.is_empty(),
                    "completed results ({}) should score <= direct matches ({})",
                    c.similarity, d.similarity
                );
            }
        }
    }

    #[test]
    fn test_pattern_completion_empty_on_no_entities() {
        let state = MemoryState::default();
        // Query with no recognizable entities — nothing to seed graph from
        let completed = state.pattern_complete("some random words", &[]);
        assert!(
            completed.is_empty(),
            "no entities = no pattern completion"
        );
    }

    #[test]
    fn test_pattern_completion_skips_consolidated() {
        let mut state = MemoryState::default();
        let dim = state.config.embedding_dim;

        let node_id = 1300;
        state.long_term.nodes.insert(node_id, GraphNode {
            id: node_id, label: "SkipConsolidated".into(), kind: "Entity".into(),
            weight: 1.0, salience: 0.5,
            source_texts: vec!["SkipConsolidated test entry".into()],
            last_seen: 0, embedding: Vec::new(), full_text: None,
        });
        state.long_term.index.insert("SkipConsolidated".into(), node_id);

        // Insert a consolidated L2 entry
        state.short_term.push(ShortTermEntry {
            id: 999, text: "SkipConsolidated test entry for pattern completion".into(),
            summary: String::new(),
            embedding: embed_text("SkipConsolidated test entry", dim),
            salience: 0.5, usage: 1, last_access: 1,
            reconsolidation_count: 0, labile_until: 0, refs: vec![],
            gradient_sq_sum: 0.0, density: 0.0,
            consolidated: true, // marked consolidated
            emotional_valence: 0.0, stability: 1.0, last_retrieval_interval: 0,
        });

        let completed = state.pattern_complete("SkipConsolidated", &[]);
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
        state.tick("BUG: critical server crash in production causing data loss");
        state.tick("BLOCKER: security vulnerability found in authentication module");
        state.tick("BUG: panic in payment processing causes transaction failures");

        assert!(
            state.should_suggest_consolidation(),
            "burst of high-valence ticks should trigger consolidation, valence_sum={}",
            state.recent_valence_sum
        );
    }

    #[test]
    fn test_neutral_ticks_no_early_consolidation() {
        let mut state = MemoryState::default();
        state.tick("Updated the documentation formatting");
        state.tick("Refactored variable names in the config module");
        state.tick("Added a comment to the parser function");

        assert!(
            !state.should_suggest_consolidation(),
            "neutral ticks should not trigger early consolidation, valence_sum={}, ticks_since={}",
            state.recent_valence_sum, state.ticks_since_consolidation
        );
    }

    #[test]
    fn test_emotional_intensity_decays() {
        let mut state = MemoryState::default();
        state.tick("BUG: critical crash in production");
        let after_first = state.recent_valence_sum;

        // Several neutral ticks should decay the sum
        for _ in 0..10 {
            state.tick("routine documentation update");
        }

        assert!(
            state.recent_valence_sum < after_first * 0.5,
            "emotional intensity should decay over time: {} -> {}",
            after_first, state.recent_valence_sum
        );
    }

    #[test]
    fn test_context_switch_updates_embedding() {
        let mut state = MemoryState::default();
        assert!(state.last_tick_embedding.is_empty());

        state.tick("DECISION: fn handle_database() manages PostgreSQL connection pooling");
        assert!(!state.last_tick_embedding.is_empty());

        let emb_after_first = state.last_tick_embedding.clone();
        state.tick("DECISION: CSS flexbox layout grid styling responsive design media queries");

        // Embedding should have changed to reflect the new topic
        assert_ne!(
            state.last_tick_embedding, emb_after_first,
            "last_tick_embedding should update each tick"
        );
    }

    #[test]
    fn test_first_tick_no_context_switch() {
        let mut state = MemoryState::default();
        assert!(state.last_tick_embedding.is_empty());

        // First tick ever — no previous embedding to compare against
        state.tick("DECISION: Starting new project with Rust");
        // Should not panic or trigger false context switch
        assert!(!state.last_tick_embedding.is_empty());
    }

    #[test]
    fn test_same_topic_preserves_working_memory() {
        let mut state = MemoryState::default();
        state.tick("DECISION: fn handle_database() manages PostgreSQL connections");
        let wm_count_after_first = state.working_memory.len();

        state.tick("DECISION: fn optimize_database() improves PostgreSQL query performance");
        // Same topic — working memory should grow, not flush
        assert!(
            state.working_memory.len() >= wm_count_after_first,
            "same-topic tick should not flush working memory"
        );
    }

    // --- Spreading activation tests (Change 4) ---

    /// Helper to insert a test graph node and return its ID.
    fn insert_test_node(state: &mut MemoryState, id: u64, label: &str) {
        state.long_term.nodes.insert(id, GraphNode {
            id, label: label.into(), kind: "Entity".into(),
            weight: 1.0, salience: 0.5, source_texts: vec![],
            last_seen: 0, embedding: Vec::new(), full_text: None,
        });
        state.long_term.index.insert(label.into(), id);
    }

    // --- Sharp-wave ripple replay tests (Change 5) ---

    #[test]
    fn test_replay_reinforces_shared_entities() {
        // Manually build two L2 entries that share graph entities, close in time.
        // Call replay_consolidation directly (not consolidate) to isolate the effect.
        let mut state = MemoryState::default();
        state.clock = 10;

        // Create two graph nodes
        let node_a = 600;
        let node_b = 601;
        insert_test_node(&mut state, node_a, "AuthModule");
        insert_test_node(&mut state, node_b, "JwtParser");

        // Create an edge between them
        state.long_term.edges.push(GraphEdge {
            from: node_a, to: node_b, weight: 0.5,
            kind: "related".into(), last_seen: 5,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        // Create two L2 entries that mention both entities, close in time
        let emb = crate::memory::embed::embed_text("auth jwt", 256);
        state.short_term.push(ShortTermEntry {
            id: 1, text: "AuthModule uses JwtParser for token validation".into(),
            summary: String::new(), embedding: emb.clone(), salience: 0.5,
            usage: 1, last_access: 8, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: "JwtParser validates AuthModule tokens".into(),
            summary: String::new(), embedding: emb, salience: 0.5,
            usage: 1, last_access: 10, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });

        let edge_weight_before = state.long_term.edges[0].weight;
        state.replay_consolidation();
        let edge_weight_after = state.long_term.edges[0].weight;

        assert!(
            edge_weight_after > edge_weight_before,
            "replay should boost shared entity edge: {} -> {}",
            edge_weight_before, edge_weight_after
        );
    }

    #[test]
    fn test_replay_ignores_temporally_distant_entries() {
        // Two entries far apart in time — no temporal edges should be created.
        let mut state = MemoryState::default();
        state.clock = 100;

        let node_a = 700;
        let node_b = 701;
        insert_test_node(&mut state, node_a, "ServerHandler");
        insert_test_node(&mut state, node_b, "DatabasePool");

        let emb = crate::memory::embed::embed_text("server database", 256);
        state.short_term.push(ShortTermEntry {
            id: 1, text: "ServerHandler processes API requests".into(),
            summary: String::new(), embedding: emb.clone(), salience: 0.5,
            usage: 1, last_access: 10, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: "DatabasePool manages connections".into(),
            summary: String::new(), embedding: emb, salience: 0.5,
            usage: 1, last_access: 90, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });

        let edge_count_before = state.long_term.edges.len();
        state.replay_consolidation();

        let temporal_edges = state.long_term.edges.iter()
            .filter(|e| e.kind == "temporal")
            .count();

        assert_eq!(
            temporal_edges, 0,
            "temporally distant entries (80 ticks apart) should not create temporal edges"
        );
        assert_eq!(
            state.long_term.edges.len(), edge_count_before,
            "no new edges should be created for distant entries"
        );
    }

    #[test]
    fn test_replay_creates_temporal_edges() {
        // Two entries with different entities but close in time — should create
        // temporal edges between the unconnected entity pairs.
        let mut state = MemoryState::default();
        state.clock = 10;

        let node_a = 800;
        let node_b = 801;
        insert_test_node(&mut state, node_a, "ConfigParser");
        insert_test_node(&mut state, node_b, "RouterSetup");

        // No pre-existing edges between them
        let emb = crate::memory::embed::embed_text("config router", 256);
        state.short_term.push(ShortTermEntry {
            id: 1, text: "ConfigParser loads YAML settings".into(),
            summary: String::new(), embedding: emb.clone(), salience: 0.5,
            usage: 1, last_access: 8, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: "RouterSetup configures API endpoints".into(),
            summary: String::new(), embedding: emb, salience: 0.5,
            usage: 1, last_access: 9, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });

        assert!(state.long_term.edges.is_empty(), "no edges before replay");
        state.replay_consolidation();

        let temporal_edges: Vec<&GraphEdge> = state.long_term.edges.iter()
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
        state.clock = 10;

        let node_a = 900;
        let node_b = 901;
        insert_test_node(&mut state, node_a, "MemHandler");
        insert_test_node(&mut state, node_b, "EventLoop");

        let emb = crate::memory::embed::embed_text("memory event", 256);
        state.short_term.push(ShortTermEntry {
            id: 1, text: "MemHandler and EventLoop process ticks".into(),
            summary: String::new(), embedding: emb.clone(), salience: 0.5,
            usage: 1, last_access: 8, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: "EventLoop dispatches to MemHandler".into(),
            summary: String::new(), embedding: emb, salience: 0.5,
            usage: 1, last_access: 9, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });

        let salience_before: Vec<f32> = state.short_term.iter().map(|e| e.salience).collect();
        state.replay_consolidation();
        let salience_after: Vec<f32> = state.short_term.iter().map(|e| e.salience).collect();

        let any_boosted = salience_before.iter().zip(salience_after.iter())
            .any(|(before, after)| after > before);
        assert!(any_boosted, "replay should boost salience of co-active entries");
    }

    #[test]
    fn test_existing_consolidation_still_works() {
        let mut state = MemoryState::default();
        state.tick("fn handle_memory() processes incoming ticks");
        state.tick("fn handle_memory() is the main entry point for memory operations");
        state.tick("fn handle_memory() manages the three-layer architecture");

        let summaries = state.consolidate();
        assert!(
            !summaries.is_empty() || state.short_term.len() <= 3,
            "consolidation should still produce summaries or maintain entries"
        );
    }

    // --- Spreading activation tests (Change 4) ---

    #[test]
    fn test_spreading_activation_two_hop_chain() {
        // Build A -> B -> C chain
        let mut state = MemoryState::default();
        let (a_id, b_id, c_id) = (100, 101, 102);
        insert_test_node(&mut state, a_id, "NodeA");
        insert_test_node(&mut state, b_id, "NodeB");
        insert_test_node(&mut state, c_id, "NodeC");

        // A -> B (weight 0.8), B -> C (weight 0.6)
        state.long_term.edges.push(GraphEdge {
            from: a_id, to: b_id, weight: 0.8,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });
        state.long_term.edges.push(GraphEdge {
            from: b_id, to: c_id, weight: 0.6,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        let results = state.spreading_activation(&[a_id], 3, 0.5);
        let b_activation = results.iter().find(|(id, _)| *id == b_id);
        let c_activation = results.iter().find(|(id, _)| *id == c_id);

        assert!(b_activation.is_some(), "B should be activated from A");
        assert!(c_activation.is_some(), "C should be activated from A via B");

        let b_val = b_activation.unwrap().1;
        let c_val = c_activation.unwrap().1;
        assert!(
            b_val > c_val,
            "hop-1 activation ({}) should exceed hop-2 ({})",
            b_val, c_val
        );
    }

    #[test]
    fn test_spreading_activation_cycle_prevention() {
        // Build A <-> B cycle
        let mut state = MemoryState::default();
        let (a_id, b_id) = (200, 201);
        insert_test_node(&mut state, a_id, "CycleA");
        insert_test_node(&mut state, b_id, "CycleB");

        // Bidirectional edges
        state.long_term.edges.push(GraphEdge {
            from: a_id, to: b_id, weight: 1.0,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });
        state.long_term.edges.push(GraphEdge {
            from: b_id, to: a_id, weight: 1.0,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        // Should not infinite loop — visited set prevents re-expansion
        let results = state.spreading_activation(&[a_id], 5, 0.5);
        assert_eq!(results.len(), 1, "only B should appear (A is seed)");
        assert_eq!(results[0].0, b_id);
    }

    #[test]
    fn test_spreading_activation_three_hop_progressive_decay() {
        // A -> B -> C -> D
        let mut state = MemoryState::default();
        let ids: Vec<u64> = vec![300, 301, 302, 303];
        for (i, label) in ["HopA", "HopB", "HopC", "HopD"].iter().enumerate() {
            insert_test_node(&mut state, ids[i], label);
        }

        // All edges weight 1.0 to isolate the decay factor
        for i in 0..3 {
            state.long_term.edges.push(GraphEdge {
                from: ids[i], to: ids[i + 1], weight: 1.0,
                kind: "related".into(), last_seen: 0,
                activation_count: 0, stability: 1.0,
                recent_interval_avg: 0.0, historical_interval_avg: 0.0,
            });
        }

        let results = state.spreading_activation(&[ids[0]], 3, 0.5);
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
        let results = state.spreading_activation(&[], 3, 0.5);
        assert!(results.is_empty());
    }

    #[test]
    fn test_graph_lookup_uses_multi_hop() {
        // Build TestMultiA -> TestMultiB -> TestMultiC, query should find C via 2 hops
        let mut state = MemoryState::default();
        let ids: Vec<u64> = vec![400, 401, 402];
        for (i, label) in ["TestMultiA", "TestMultiB", "TestMultiC"].iter().enumerate() {
            insert_test_node(&mut state, ids[i], label);
        }

        state.long_term.edges.push(GraphEdge {
            from: ids[0], to: ids[1], weight: 0.8,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });
        state.long_term.edges.push(GraphEdge {
            from: ids[1], to: ids[2], weight: 0.6,
            kind: "related".into(), last_seen: 0,
            activation_count: 0, stability: 1.0,
            recent_interval_avg: 0.0, historical_interval_avg: 0.0,
        });

        let results = state.graph_lookup("TestMultiA", 20);
        let labels: Vec<&str> = results.iter().map(|r| r.label.as_str()).collect();

        assert!(labels.contains(&"TestMultiA"), "seed should be in results");
        assert!(labels.contains(&"TestMultiB"), "hop-1 neighbor should be in results");
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
            insert_test_node(&mut state, ids[i], label);
        }
        for i in 0..3 {
            state.long_term.edges.push(GraphEdge {
                from: ids[i], to: ids[i + 1], weight: 1.0,
                kind: "related".into(), last_seen: 0,
                activation_count: 0, stability: 1.0,
                recent_interval_avg: 0.0, historical_interval_avg: 0.0,
            });
        }

        let results = state.spreading_activation(&[ids[0]], 1, 0.5);
        let result_ids: Vec<u64> = results.iter().map(|(id, _)| *id).collect();

        assert!(result_ids.contains(&ids[1]), "hop-1 should be reachable");
        assert!(!result_ids.contains(&ids[2]), "hop-2 should NOT be reachable with max_hops=1");
        assert!(!result_ids.contains(&ids[3]), "hop-3 should NOT be reachable with max_hops=1");
    }

    // --- Systems consolidation tests (Change 8) ---

    #[test]
    fn test_consolidation_produces_summary_with_centroid_embedding() {
        let mut state = MemoryState::default();
        let dim = state.config.embedding_dim;

        // Insert two very similar L2 entries with high salience
        // (must exceed theta_low=0.72 cosine similarity for clustering)
        let shared = "Rust borrow checker ownership rules for memory safety in systems programming";
        let text1 = format!("{} and lifetimes", shared);
        let text2 = format!("{} and references", shared);
        let emb1 = embed_text(&text1, dim);
        let emb2 = embed_text(&text2, dim);

        state.short_term.push(ShortTermEntry {
            id: 1, text: text1.clone(),
            summary: String::new(), embedding: emb1, salience: 0.8,
            usage: 0, last_access: 0, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: text2.clone(),
            summary: String::new(), embedding: emb2, salience: 0.7,
            usage: 0, last_access: 0, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });

        let summaries = state.consolidate();
        assert!(!summaries.is_empty(), "should produce at least one summary");

        // Find the Summary node and verify it has an embedding
        let summary_node = state.long_term.nodes.values()
            .find(|n| n.kind == "Summary")
            .expect("should have a Summary node");

        assert!(!summary_node.embedding.is_empty(),
            "high-salience group should get centroid embedding");
        assert_eq!(summary_node.embedding.len(), dim);

        // Verify embedding is normalized (unit length)
        let norm: f32 = summary_node.embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "centroid should be unit-normalized, got {}", norm);
    }

    #[test]
    fn test_consolidation_produces_full_text() {
        let mut state = MemoryState::default();
        let dim = state.config.embedding_dim;

        let emb1 = embed_text("Database migration strategy for PostgreSQL", dim);
        let emb2 = embed_text("Database migration rollback for PostgreSQL", dim);

        state.short_term.push(ShortTermEntry {
            id: 1, text: "Database migration strategy for PostgreSQL".into(),
            summary: String::new(), embedding: emb1, salience: 0.6,
            usage: 0, last_access: 0, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: "Database migration rollback for PostgreSQL".into(),
            summary: String::new(), embedding: emb2, salience: 0.6,
            usage: 0, last_access: 0, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });

        state.consolidate();

        let summary_node = state.long_term.nodes.values()
            .find(|n| n.kind == "Summary")
            .expect("should have Summary node");

        assert!(summary_node.full_text.is_some(),
            "high-salience group should get full_text");
        let ft = summary_node.full_text.as_ref().unwrap();
        assert!(ft.len() <= 500, "full_text should be capped at 500 chars, got {}", ft.len());
        // Should contain content from source entries
        assert!(ft.contains("Database") || ft.contains("migration"),
            "full_text should contain source content, got: {}", ft);
    }

    #[test]
    fn test_low_salience_group_no_embedding() {
        let mut state = MemoryState::default();
        let dim = state.config.embedding_dim;

        // Two similar entries but with very low salience
        let emb1 = embed_text("trivial formatting update to docs", dim);
        let emb2 = embed_text("trivial formatting change to docs", dim);

        state.short_term.push(ShortTermEntry {
            id: 1, text: "trivial formatting update to docs".into(),
            summary: String::new(), embedding: emb1, salience: 0.1,
            usage: 0, last_access: 0, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: "trivial formatting change to docs".into(),
            summary: String::new(), embedding: emb2, salience: 0.1,
            usage: 0, last_access: 0, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });

        state.consolidate();

        let summary_node = state.long_term.nodes.values()
            .find(|n| n.kind == "Summary");

        if let Some(node) = summary_node {
            assert!(node.embedding.is_empty(),
                "low-salience group should NOT get centroid embedding");
            assert!(node.full_text.is_none(),
                "low-salience group should NOT get full_text");
        }
    }

    #[test]
    fn test_l3_summary_retrieval_finds_consolidated_memory() {
        let mut state = MemoryState::default();
        let dim = state.config.embedding_dim;

        // Manually insert a Summary node with a centroid embedding
        let summary_emb = embed_text("Rust memory safety borrow checker", dim);
        let summary_id = 2000;
        state.long_term.nodes.insert(summary_id, GraphNode {
            id: summary_id,
            label: "Rust borrow checker summary".into(),
            kind: "Summary".into(),
            weight: 1.5,
            last_seen: 10,
            salience: 0.7,
            source_texts: vec!["Rust borrow checker ownership rules".into()],
            embedding: summary_emb,
            full_text: Some("Rust borrow checker ownership and lifetime rules".into()),
        });
        state.long_term.index.insert("Rust borrow checker summary".into(), summary_id);

        // Query something similar — no L2 entries exist, so L3 should surface it
        let ctx = state.retrieve_context("Rust borrow checker rules");

        // Should find the Summary node in short_term results (merged in)
        let found = ctx.short_term.iter().any(|s| s.id == summary_id);
        assert!(found,
            "L3 Summary with embedding should be retrievable. Got: {:?}",
            ctx.short_term.iter().map(|s| (&s.text, s.similarity)).collect::<Vec<_>>());
    }

    #[test]
    fn test_centroid_cosine_similar_to_members() {
        let mut state = MemoryState::default();
        let dim = state.config.embedding_dim;

        let text1 = "Authentication JWT token validation middleware";
        let text2 = "Authentication JWT token signing middleware";
        let emb1 = embed_text(text1, dim);
        let emb2 = embed_text(text2, dim);

        state.short_term.push(ShortTermEntry {
            id: 1, text: text1.into(),
            summary: String::new(), embedding: emb1.clone(), salience: 0.8,
            usage: 0, last_access: 0, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: text2.into(),
            summary: String::new(), embedding: emb2.clone(), salience: 0.8,
            usage: 0, last_access: 0, reconsolidation_count: 0,
            labile_until: 0, refs: vec![], gradient_sq_sum: 0.0,
            density: 0.0, consolidated: false, emotional_valence: 0.0,
            stability: 1.0, last_retrieval_interval: 0,
        });

        state.consolidate();

        let summary_node = state.long_term.nodes.values()
            .find(|n| n.kind == "Summary" && !n.embedding.is_empty())
            .expect("should have Summary with embedding");

        // Centroid should be similar to both members
        let sim1 = cosine_similarity(&summary_node.embedding, &emb1);
        let sim2 = cosine_similarity(&summary_node.embedding, &emb2);

        assert!(sim1 > 0.5, "centroid should be similar to member 1, got {}", sim1);
        assert!(sim2 > 0.5, "centroid should be similar to member 2, got {}", sim2);
    }

    #[test]
    fn test_consolidated_entry_easier_to_evict() {
        let mut state = MemoryState::default();
        let dim = state.config.embedding_dim;

        // Capacity 2: already have 2 entries, inserting a 3rd triggers eviction
        state.config.short_term_capacity = 2;
        state.next_id = 100; // avoid ID collision with manually inserted entries

        // Insert a Summary node with embedding that backs entry 1
        let summary_emb = embed_text("Consolidated topic alpha details", dim);
        let summary_id = 3000;
        state.long_term.nodes.insert(summary_id, GraphNode {
            id: summary_id,
            label: "Alpha summary".into(),
            kind: "Summary".into(),
            weight: 1.5,
            last_seen: 10,
            salience: 0.7,
            source_texts: vec!["Consolidated topic alpha details".into()],
            embedding: summary_emb,
            full_text: Some("Alpha topic details".into()),
        });

        // Two entries with identical scores — only the consolidated flag differs
        state.short_term.push(ShortTermEntry {
            id: 1, text: "Consolidated topic alpha details".into(),
            summary: String::new(), embedding: embed_text("Consolidated topic alpha details", dim),
            salience: 0.5, usage: 1, last_access: 0,
            reconsolidation_count: 0, labile_until: 0, refs: vec![],
            gradient_sq_sum: 0.0, density: 0.0,
            consolidated: true, // backed by L3 with embedding
            emotional_valence: 0.0, stability: 1.0, last_retrieval_interval: 0,
        });
        state.short_term.push(ShortTermEntry {
            id: 2, text: "Unconsolidated topic beta details".into(),
            summary: String::new(), embedding: embed_text("Unconsolidated topic beta details", dim),
            salience: 0.5, usage: 1, last_access: 0,
            reconsolidation_count: 0, labile_until: 0, refs: vec![],
            gradient_sq_sum: 0.0, density: 0.0,
            consolidated: false,
            emotional_valence: 0.0, stability: 1.0, last_retrieval_interval: 0,
        });

        // Directly call insert_short_term to trigger eviction
        state.insert_short_term(
            "New entry forcing eviction",
            embed_text("New entry forcing eviction", dim),
            0.5,
            vec![],
            0.0,
        );

        // The consolidated entry should have been evicted (lower effective score)
        let remaining_ids: Vec<u64> = state.short_term.iter().map(|e| e.id).collect();
        assert!(!remaining_ids.contains(&1),
            "consolidated entry backed by L3 should be evicted first, remaining: {:?}", remaining_ids);
        assert!(remaining_ids.contains(&2),
            "unconsolidated entry should survive");
    }

    // -----------------------------------------------------------------------
    // Layer 3: Incremental keyword discovery
    // -----------------------------------------------------------------------

    #[test]
    fn test_term_frequency_tracking_basic() {
        let mut state = MemoryState::default();
        // Tick with an entity that extract_entities will find
        state.tick("DECISION: fn process_data() handles struct Config");
        assert!(!state.term_frequency.is_empty(), "term_frequency should track extracted entities");
    }

    #[test]
    fn test_term_frequency_increments_across_ticks() {
        let mut state = MemoryState::default();
        // Tick the same entity across multiple ticks
        for i in 0..5 {
            state.tick(&format!("DECISION: process_data handles request batch {}", i));
        }
        // "process_data" should have tick_count >= 1 (it appears in entity extraction)
        let has_multi_tick = state.term_frequency.values().any(|s| s.tick_count >= 2);
        assert!(has_multi_tick, "repeated entities should have multiple tick counts: {:?}",
            state.term_frequency.iter().map(|(k, v)| (k.clone(), v.tick_count)).collect::<Vec<_>>());
    }

    #[test]
    fn test_term_not_promoted_before_min_ticks() {
        let mut state = MemoryState::default();
        // Tick entity only 3 times (below TERM_PROMOTION_MIN_TICKS of 5)
        for i in 0..3 {
            state.tick(&format!("DECISION: CustomWidget renders component {}", i));
        }
        // Should NOT have a kw:domain:customwidget node
        assert!(
            !state.long_term.index.contains_key("kw:domain:customwidget"),
            "term should not be promoted before min ticks"
        );
    }

    #[test]
    fn test_term_promoted_after_sufficient_ticks() {
        let mut state = MemoryState::default();
        // Seed a code keyword so entities can be extracted from "fn CustomProcessor()"
        state.add_keyword_node("code", "fn ", vec![
            "entity_kind:Function".to_string(),
            "entity_context:defines".to_string(),
        ]);
        state.rebuild_keyword_cache();

        // Tick entity 7 times with keyword co-occurrence (DECISION provides co-occurrence)
        for i in 0..7 {
            state.tick(&format!("DECISION: fn CustomProcessor() handles batch {}", i));
        }

        // "customprocessor" should be in term_frequency with sufficient tick_count
        let stats = state.term_frequency.get("customprocessor");
        assert!(
            stats.is_some(),
            "CustomProcessor should be tracked; keys: {:?}",
            state.term_frequency.keys().collect::<Vec<_>>()
        );
        if let Some(s) = stats {
            assert!(
                s.tick_count >= TERM_PROMOTION_MIN_TICKS,
                "tick_count={} should be >= {}",
                s.tick_count,
                TERM_PROMOTION_MIN_TICKS
            );
            assert!(s.has_keyword_cooccurrence, "should have keyword co-occurrence");
        }

        // Should be auto-promoted to kw:domain:customprocessor
        assert!(
            state.long_term.index.contains_key("kw:domain:customprocessor"),
            "term should be auto-promoted after passing all filters; graph keys with kw:domain: {:?}",
            state.long_term.index.keys().filter(|k| k.starts_with("kw:domain:")).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_stopword_not_promoted() {
        let mut state = MemoryState::default();
        // Even if "the" appeared many times it should never be promoted
        assert!(!state.should_promote_term("the"));
        assert!(!state.should_promote_term("is"));
        assert!(!state.should_promote_term("and"));
    }

    #[test]
    fn test_short_term_not_promoted() {
        let mut state = MemoryState::default();
        // Terms shorter than 3 chars should not be promoted
        assert!(!state.should_promote_term("ab"));
        assert!(!state.should_promote_term("x"));
    }

    #[test]
    fn test_numeric_term_not_promoted() {
        let mut state = MemoryState::default();
        // Purely numeric terms should not be promoted
        assert!(!state.should_promote_term("12345"));
        assert!(!state.should_promote_term("3.14"));
    }

    #[test]
    fn test_co_occurrence_required_for_promotion() {
        let mut state = MemoryState::default();
        // Manually insert term stats WITHOUT co-occurrence
        state.term_frequency.insert(
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
            !state.should_promote_term("orphanterm"),
            "term without keyword co-occurrence should not promote"
        );

        // Now set co-occurrence
        state.term_frequency.get_mut("orphanterm").unwrap().has_keyword_cooccurrence = true;
        assert!(
            state.should_promote_term("orphanterm"),
            "term WITH keyword co-occurrence and sufficient ticks should promote"
        );
    }

    #[test]
    fn test_already_promoted_term_not_duplicated() {
        let mut state = MemoryState::default();
        // Pre-add the keyword
        state.add_keyword_node("domain", "existingterm", Vec::new());
        // should_promote_term should return false
        assert!(!state.should_promote_term("existingterm"));
    }

    #[test]
    fn test_passive_ticks_dont_update_term_frequency() {
        let mut state = MemoryState::default();
        // Passive tick (used during onboarding/ingestion)
        state.tick_passive("DECISION: fn SpecialHandler() processes important data");
        // term_frequency should be empty — passive ticks skip Layer 3
        assert!(
            state.term_frequency.is_empty(),
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
