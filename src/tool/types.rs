/// Tool-layer types — data structures for Legend's CLI/MCP bindings.
///
/// These types represent tool infrastructure (session logging, tick results,
/// task management, git sync, LLM integration) as opposed to cognitive
/// mechanisms. The brain (memory/) never creates or consumes these directly;
/// the tool layer (tool/mod.rs) translates between brain outputs and these types.

use serde::{Deserialize, Serialize};

use crate::memory::{GraphNodeSummary, MemorySnippet};

// ---------------------------------------------------------------------------
// Memory configuration (innate parameters)
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

// ---------------------------------------------------------------------------
// Statistical learning
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Session & task management
// ---------------------------------------------------------------------------

/// A timestamped session log entry preserving full tick text.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionEntry {
    pub timestamp: u64,
    pub text: String,
}

// ---------------------------------------------------------------------------
// Tick result
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Query results
// ---------------------------------------------------------------------------

/// Query result: ranked snippets from short-term + related nodes from long-term.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub short_term: Vec<MemorySnippet>,
    pub long_term: Vec<GraphNodeSummary>,
    /// Matches from working memory (L1), scanned before L2.
    #[serde(default)]
    pub working_memory: Vec<MemorySnippet>,
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Git integration
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct GitSyncInfo {
    pub last_sha: Option<String>,
    pub current_sha: Option<String>,
    pub new_commits: Vec<String>,
    pub uncommitted_summary: Option<String>,
}

// ---------------------------------------------------------------------------
// LLM entity integration
// ---------------------------------------------------------------------------

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

// ReinforceResult, ReinforcedEntry — moved to memory/basal_ganglia.rs (brain types).
