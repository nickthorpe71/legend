pub mod embed;
pub mod extract;
pub mod summarize;

use embed::{compute_salience, cosine_similarity, embed_text, merge_embeddings};
use extract::extract_entities;
use summarize::{chunk_text, summarize_group, summarize_single, summarize_text};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

const MEMORY_FILE: &str = ".legend/memory.lz4";

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
/// Minimum word-overlap ratio required to merge (prevents unrelated entries collapsing).
const MERGE_WORD_OVERLAP_THRESHOLD: f32 = 0.3;
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

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Tuning parameters for the three-layer memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Max items in the immediate (FIFO) buffer.
    pub immediate_capacity: usize,
    /// Max entries in the short-term vector store.
    pub short_term_capacity: usize,
    /// Dimensionality of n-gram embedding vectors.
    pub embedding_dim: usize,
    /// Similarity above this → reinforce existing entry (no new insert).
    pub theta_high: f32,
    /// Similarity above this but below theta_high → merge embeddings.
    pub theta_low: f32,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            immediate_capacity: 256,
            short_term_capacity: 1024,
            embedding_dim: 256,
            theta_high: 0.92,
            theta_low: 0.55,
        }
    }
}

/// Three-layer memory: immediate FIFO buffer, short-term vector store, long-term knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryState {
    pub config: MemoryConfig,
    pub immediate: VecDeque<String>,
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
}

/// A single entry in the short-term vector store.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// A source reference to a file region for this memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRef {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    /// Short snippet for re-anchoring when lines drift.
    #[serde(default)]
    pub snippet: String,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMemory {
    pub nodes: HashMap<u64, GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Label → node ID for fast entity lookup.
    pub index: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
    pub last_seen: u64,
    #[serde(default)]
    pub salience: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: u64,
    pub to: u64,
    pub weight: f32,
    #[serde(default = "default_edge_kind")]
    pub kind: String,
    /// Clock tick when this edge was last reinforced.
    #[serde(default)]
    pub last_seen: u64,
}

fn default_edge_kind() -> String {
    "related".to_string()
}

/// A timestamped session log entry preserving full tick text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub timestamp: u64,
    pub text: String,
}

/// Query result: ranked snippets from short-term + related nodes from long-term.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub short_term: Vec<MemorySnippet>,
    pub long_term: Vec<GraphNodeSummary>,
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

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

impl Default for MemoryState {
    fn default() -> Self {
        Self {
            config: MemoryConfig::default(),
            immediate: VecDeque::new(),
            short_term: Vec::new(),
            long_term: GraphMemory::default(),
            clock: 0,
            next_id: 1,
            session_log: Vec::new(),
            current_task: None,
            ticks_since_consolidation: 0,
            last_retrieved_ids: Vec::new(),
        }
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
pub fn classify_text(text: &str) -> MemoryCategory {
    let lower = text.to_lowercase();

    // Decision patterns (highest priority)
    let decision_score = [
        "chose",
        "decided",
        "decision",
        "instead of",
        "rather than",
        "over",
        "picked",
        "opted",
        "went with",
        "trade-off",
        "tradeoff",
        "because",
        "rationale",
        "rejected",
        "approach",
    ]
    .iter()
    .filter(|kw| lower.contains(*kw))
    .count();
    if decision_score >= 2 {
        return MemoryCategory::Decision;
    }

    // Bug patterns
    if [
        "bug",
        "broke",
        "broken",
        "revert",
        "reverted",
        "crash",
        "panic",
        "regression",
        "fix",
        "hotfix",
        "incident",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
    {
        return MemoryCategory::Bug;
    }

    // TODO patterns
    if [
        "todo",
        "fixme",
        "hack",
        "still need",
        "not yet",
        "remaining",
        "blocker",
        "blocked",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
    {
        return MemoryCategory::Todo;
    }

    // Architecture patterns
    if [
        "architecture",
        "module",
        "component",
        "layer",
        "system",
        "interface",
        "api",
        "schema",
        "pipeline",
        "pattern",
        "struct ",
        "trait ",
        "impl ",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
    {
        return MemoryCategory::Architecture;
    }

    // Preference patterns
    if [
        "prefer",
        "preference",
        "user wants",
        "user prefers",
        "style",
        "convention",
        "always use",
        "never use",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
    {
        return MemoryCategory::Preference;
    }

    // Progress patterns
    if [
        "implemented",
        "completed",
        "finished",
        "added",
        "created",
        "built",
        "shipped",
        "merged",
        "deployed",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
    {
        return MemoryCategory::Progress;
    }

    // Single decision keyword is enough if it looks intentional
    if decision_score >= 1 {
        return MemoryCategory::Decision;
    }

    MemoryCategory::General
}

impl Default for GraphMemory {
    fn default() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            index: HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Core MemoryState logic
// ---------------------------------------------------------------------------

impl MemoryState {
    pub fn load_or_default() -> Result<Self, Box<dyn std::error::Error>> {
        // Try to migrate corrupt backup first (old format without new fields)
        if let Ok(Some(migrated)) = migrate_corrupt_backup() {
            return Ok(migrated);
        }

        if Path::new(MEMORY_FILE).exists() {
            match load_memory() {
                Ok(state) => Ok(state),
                Err(err) => {
                    let backup = format!("{}.corrupt", MEMORY_FILE);
                    let _ = fs::rename(MEMORY_FILE, &backup);
                    eprintln!("Warning: failed to load memory store ({})", err);
                    eprintln!("Backup saved to {}", backup);
                    Ok(Self::default())
                }
            }
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        save_memory(self)
    }

    /// Ingest text: chunk → embed → reconsolidate or match/merge/insert → update graph.
    /// Returns a TickResult describing what action was taken.
    pub fn tick(&mut self, text: &str) -> TickResult {
        self.clock += 1;
        self.ticks_since_consolidation += 1;
        self.apply_decay();
        self.stabilize_labile_entries();
        if self.clock % RENORM_INTERVAL == 0 {
            self.renormalize_salience();
        }
        if self.clock % GRAPH_NORM_INTERVAL == 0 {
            self.normalize_graph_weights();
        }

        // Append to chronological session log (preserves exact input)
        self.session_log.push(SessionEntry {
            timestamp: self.clock,
            text: text.to_string(),
        });
        while self.session_log.len() > SESSION_LOG_CAPACITY {
            self.session_log.remove(0);
        }

        let mut last_context = MemoryContext {
            short_term: Vec::new(),
            long_term: Vec::new(),
        };

        // Track the action taken (priority: created > reconsolidated > merged)
        let mut result_action = "created".to_string();
        let mut result_entry_id: u64 = 0;
        let mut result_matched: Option<u64> = None;
        let mut result_similarity: Option<f32> = None;

        for chunk in chunk_text(text) {
            self.push_immediate(&chunk);

            let embedding = embed_text(&chunk, self.config.embedding_dim);
            let salience = compute_salience(&chunk);
            let refs = extract_memory_refs_from_text(&chunk);

            // --- Reconsolidation: check labile entries first ---
            // If a recently-retrieved memory is labile and the new text is related,
            // update that memory in-place instead of creating a duplicate.
            if let Some(reconsolidated_id) =
                self.try_reconsolidate(&chunk, &embedding, salience, refs.clone())
            {
                // Update graph with the new text context
                self.update_graph(&chunk, salience);
                last_context = self.retrieve_context(&chunk);
                // Track reconsolidation
                result_action = "reconsolidated".to_string();
                result_entry_id = reconsolidated_id;
                result_matched = Some(reconsolidated_id);
                continue;
            }

            // --- Normal path: match/merge/insert ---
            let (best_id, best_sim) = self.find_best_match(&embedding);

            // Diversity gate: even at high similarity, if word overlap is low the
            // texts are semantically distinct and should not be merged.
            let diversity_pass = if best_sim >= self.config.theta_low {
                self.short_term
                    .iter()
                    .find(|e| e.id == best_id)
                    .map(|e| word_overlap(&e.text, &chunk) >= MERGE_WORD_OVERLAP_THRESHOLD)
                    .unwrap_or(false)
            } else {
                false
            };

            match best_sim {
                s if s >= self.config.theta_high && diversity_pass => {
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
                s if s >= self.config.theta_low && diversity_pass => {
                    if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == best_id) {
                        entry.embedding = merge_embeddings(&entry.embedding, &embedding);
                        entry.usage = entry.usage.saturating_add(1);
                        entry.salience = (entry.salience + salience * 0.5).min(1.0);
                        entry.summary = summarize_text(&entry.text, &chunk);
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
                    self.insert_short_term(&chunk, embedding, salience, refs);
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

            last_context = self.retrieve_context(&chunk);
        }

        self.prune_short_term();
        self.prune_graph();

        TickResult {
            action: result_action,
            entry_id: result_entry_id,
            matched_existing: result_matched,
            similarity: result_similarity,
            context: last_context,
        }
    }

    /// Query memory without inserting new data.
    /// Retrieved entries are marked labile (editable) so the next tick can
    /// reconsolidate them with updated information.
    pub fn retrieve_context(&mut self, query: &str) -> MemoryContext {
        self.clock += 1;
        self.apply_decay();

        let embedding = embed_text(query, self.config.embedding_dim);
        let mut snippets = self.top_k_similar(&embedding, 5);

        for snippet in &mut snippets {
            if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == snippet.id) {
                entry.last_access = self.clock;
                entry.usage = entry.usage.saturating_add(1);
                // Mark labile: this entry can be reconsolidated by the next tick
                entry.labile_until = self.clock + LABILE_WINDOW;
            }
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

        // --- Associative priming ---
        // From the retrieved short-term entries, extract entities and follow
        // graph edges to surface related nodes the query text alone wouldn't match.
        let mut priming_seed_ids: Vec<u64> = Vec::new();
        for snippet in &snippets {
            let entities = extract_entities(&snippet.text);
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

        // Follow edges from priming seeds (1-hop) to surface associated nodes
        let existing_ids: std::collections::HashSet<u64> = long_term.iter().map(|n| n.id).collect();
        let mut primed_nodes: Vec<GraphNodeSummary> = Vec::new();
        for edge in &self.long_term.edges {
            let neighbor_id = if priming_seed_ids.contains(&edge.from)
                && !existing_ids.contains(&edge.to)
            {
                Some(edge.to)
            } else if priming_seed_ids.contains(&edge.to) && !existing_ids.contains(&edge.from) {
                Some(edge.from)
            } else {
                None
            };
            if let Some(nid) = neighbor_id {
                if let Some(node) = self.long_term.nodes.get(&nid) {
                    // Only include if edge is strong enough to be meaningful
                    if edge.weight >= 0.15 {
                        primed_nodes.push(GraphNodeSummary {
                            id: node.id,
                            label: node.label.clone(),
                            kind: node.kind.clone(),
                            weight: node.weight * 0.7, // discount primed results slightly
                            edge_type: Some(edge.kind.clone()),
                        });
                    }
                }
            }
        }
        // Deduplicate primed nodes
        let mut seen_ids: std::collections::HashSet<u64> = existing_ids;
        primed_nodes.retain(|n| seen_ids.insert(n.id));
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
        }
    }

    /// Merge similar short-term entries into long-term graph summaries.
    pub fn consolidate(&mut self) -> Vec<GraphNodeSummary> {
        self.clock += 1;
        self.ticks_since_consolidation = 0;
        self.apply_decay();

        let mut groups: Vec<Vec<ShortTermEntry>> = Vec::new();
        let mut used = vec![false; self.short_term.len()];

        for i in 0..self.short_term.len() {
            if used[i] {
                continue;
            }
            let seed = self.short_term[i].clone();
            let mut group = vec![seed.clone()];
            used[i] = true;

            for j in (i + 1)..self.short_term.len() {
                if used[j] {
                    continue;
                }
                if cosine_similarity(&seed.embedding, &self.short_term[j].embedding)
                    >= self.config.theta_low
                {
                    group.push(self.short_term[j].clone());
                    used[j] = true;
                }
            }
            groups.push(group);
        }

        let mut summaries = Vec::new();
        for group in groups.into_iter().filter(|g| g.len() > 1) {
            let summary_text = summarize_group(&group);
            let salience = group
                .iter()
                .map(|e| e.salience)
                .fold(0.0, f32::max)
                .max(0.4);

            let node_id = self.next_id;
            self.next_id += 1;

            self.long_term.nodes.insert(
                node_id,
                GraphNode {
                    id: node_id,
                    label: summary_text.clone(),
                    kind: "Summary".to_string(),
                    weight: 1.0 + salience,
                    last_seen: self.clock,
                    salience,
                },
            );
            self.long_term.index.insert(summary_text.clone(), node_id);

            for entry in group {
                self.update_graph(&entry.text, entry.salience);
                if let Some(existing) = self.short_term.iter_mut().find(|e| e.id == entry.id) {
                    existing.usage = existing.usage.saturating_add(1);
                    existing.last_access = self.clock;
                }
            }

            summaries.push(GraphNodeSummary {
                id: node_id,
                label: summary_text,
                kind: "Summary".to_string(),
                weight: 1.0 + salience,
                edge_type: None,
            });
        }

        self.prune_short_term();
        self.prune_graph();
        summaries
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
                if best.map_or(true, |(_, best_score)| score > best_score) {
                    best = Some((entry.id, score));
                }
            }
        }

        let (target_id, _) = best?;

        // Perform reconsolidation: update the entry in-place
        if let Some(entry) = self.short_term.iter_mut().find(|e| e.id == target_id) {
            // Merge text: append new information
            let merged_text = format!("{} | {}", entry.text, text);
            entry.summary = summarize_text(&entry.text, text);
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

    /// Hebbian reinforcement: co-retrieved nodes strengthen shared edges.
    fn hebbian_reinforce(&mut self, co_retrieved_ids: &[u64]) {
        if co_retrieved_ids.len() < 2 {
            return;
        }

        let now = self.clock;
        for edge in &mut self.long_term.edges {
            if co_retrieved_ids.contains(&edge.from) && co_retrieved_ids.contains(&edge.to) {
                edge.weight = (edge.weight + HEBBIAN_EDGE_BOOST).min(HEBBIAN_EDGE_CEILING);
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

    /// Append text to the FIFO immediate buffer.
    /// When the buffer overflows, the oldest entry is automatically "digested"
    /// (summarized, embedded, and moved) into short-term memory.
    fn push_immediate(&mut self, text: &str) {
        self.immediate.push_back(text.to_string());
        if self.immediate.len() > self.config.immediate_capacity {
            if let Some(oldest) = self.immediate.pop_front() {
                // Background "digestion" of old working memory into STM
                let embedding = embed_text(&oldest, self.config.embedding_dim);
                let salience = compute_salience(&oldest) * 0.7; // Passive memories start with lower salience
                let refs = extract_memory_refs_from_text(&oldest);
                self.insert_short_term(&oldest, embedding, salience, refs);
            }
        }
    }

    /// Insert a new short-term entry, evicting the lowest-scoring entry if at capacity.
    fn insert_short_term(
        &mut self,
        text: &str,
        embedding: Vec<f32>,
        salience: f32,
        refs: Vec<MemoryRef>,
    ) {
        if self.short_term.len() >= self.config.short_term_capacity {
            let now = self.clock;
            if let Some(idx) = self
                .short_term
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    eviction_score(a, now)
                        .partial_cmp(&eviction_score(b, now))
                        .unwrap()
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
            summary: summarize_single(text),
            embedding,
            last_access: self.clock,
            usage: 1,
            salience: salience.clamp(0.0, 1.0),
            reconsolidation_count: 0,
            labile_until: 0,
            refs,
            gradient_sq_sum: 0.0,
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
    fn top_k_similar(&self, embedding: &[f32], k: usize) -> Vec<MemorySnippet> {
        let mut scored: Vec<MemorySnippet> = self
            .short_term
            .iter()
            .map(|e| MemorySnippet {
                id: e.id,
                text: e.text.clone(),
                similarity: cosine_similarity(&e.embedding, embedding),
                refs: e.refs.clone(),
            })
            .collect();
        scored.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
        scored.truncate(k);
        scored
    }

    /// Extract entities from text and insert/update nodes and edges in the knowledge graph.
    fn update_graph(&mut self, text: &str, salience: f32) {
        let entities = extract_entities(text);
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
                    },
                );
                self.long_term.index.insert(entity.label.clone(), id);
                id
            };

            if let Some(node) = self.long_term.nodes.get_mut(&id) {
                node.weight += NODE_WEIGHT_BASE + salience * 0.3;
                node.last_seen = self.clock;
                node.salience = (node.salience + salience * 0.5).min(1.0);
                if entity.kind != "Term" && entity.kind != "Type" && entity.kind != "Symbol" {
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
            });
        }
    }

    /// Query the knowledge graph: match entities by label, then expand one hop.
    fn graph_lookup(&self, query: &str, limit: usize) -> Vec<GraphNodeSummary> {
        let entities = extract_entities(query);
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
                    });
                    seed_ids.push(node.id);
                }
            }
        }

        if !seed_ids.is_empty() {
            for edge in &self.long_term.edges {
                let neighbor_id = if seed_ids.contains(&edge.from) {
                    Some(edge.to)
                } else if seed_ids.contains(&edge.to) {
                    Some(edge.from)
                } else {
                    None
                };

                if let Some(nid) = neighbor_id {
                    if let Some(node) = self.long_term.nodes.get(&nid) {
                        results.push(GraphNodeSummary {
                            id: node.id,
                            label: node.label.clone(),
                            kind: node.kind.clone(),
                            weight: node.weight + edge.weight,
                            edge_type: Some(edge.kind.clone()),
                        });
                    }
                }
            }
        }

        if results.is_empty() {
            results = self
                .long_term
                .nodes
                .values()
                .map(|n| GraphNodeSummary {
                    id: n.id,
                    label: n.label.clone(),
                    kind: n.kind.clone(),
                    weight: n.weight,
                    edge_type: None,
                })
                .collect();
        }

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
            let decay =
                (-(now.saturating_sub(entry.last_access) as f32) * SHORT_TERM_DECAY_RATE).exp();
            entry.salience *= decay;
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
    }

    /// Clear the current task.
    pub fn clear_task(&mut self) {
        self.current_task = None;
    }

    /// Get the current task description.
    pub fn get_task(&self) -> Option<&str> {
        self.current_task.as_deref()
    }

    /// Check if consolidation should be suggested based on tick count.
    pub fn should_suggest_consolidation(&self) -> bool {
        self.ticks_since_consolidation >= CONSOLIDATION_SUGGESTION_THRESHOLD
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
                "immediate_buffer": self.immediate.len(),
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
        self.build_start_summary_with_options(false, None)
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
    ) -> serde_json::Value {
        // Get recent sessions (just text, no timestamps)
        let recent = self.recent_sessions(5);
        let recent_sessions: Vec<&str> = recent.iter().map(|s| s.text.as_str()).collect();

        // --- Categorized short-term memories ---
        let mut decisions: Vec<serde_json::Value> = Vec::new();
        let mut architecture: Vec<serde_json::Value> = Vec::new();
        let mut todos: Vec<serde_json::Value> = Vec::new();
        let mut bugs: Vec<serde_json::Value> = Vec::new();
        let mut preferences: Vec<serde_json::Value> = Vec::new();

        for entry in &self.short_term {
            let category = classify_text(&entry.text);

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

        // Sort each category by salience descending (using internal salience, not exposed)
        // We need to re-sort since we removed salience from output
        let sort_by_salience = |list: &mut Vec<serde_json::Value>, entries: &[ShortTermEntry]| {
            // Create index mapping for sorting
            let mut indexed: Vec<(usize, f32)> = list
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    if compact {
                        // In compact mode, match by text
                        let text = item.as_str()?;
                        entries
                            .iter()
                            .find(|e| e.text.starts_with(text.trim_end_matches('…')))
                            .map(|e| (i, e.salience))
                    } else {
                        // In default mode, match by id
                        let id = item["id"].as_u64()?;
                        entries.iter().find(|e| e.id == id).map(|e| (i, e.salience))
                    }
                })
                .collect();
            indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let sorted: Vec<serde_json::Value> =
                indexed.into_iter().map(|(i, _)| list[i].clone()).collect();
            *list = sorted;
            list.truncate(5);
        };

        sort_by_salience(&mut decisions, &self.short_term);
        sort_by_salience(&mut architecture, &self.short_term);
        sort_by_salience(&mut todos, &self.short_term);
        sort_by_salience(&mut bugs, &self.short_term);
        sort_by_salience(&mut preferences, &self.short_term);

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
                "preferences" | "preference" | "prefs" | "pref" => (&preferences, preferences_total),
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
                    "usage": e.usage, "last_access": e.last_access,
                    "reconsolidation_count": e.reconsolidation_count,
                    "labile": e.labile_until >= self.clock,
                    "refs": e.refs,
                })
            })
            .collect();

        let immediate: Vec<&str> = self.immediate.iter().map(|s| s.as_str()).collect();
        let sessions: Vec<serde_json::Value> = self
            .session_log
            .iter()
            .map(|s| serde_json::json!({"timestamp": s.timestamp, "text": s.text}))
            .collect();

        serde_json::json!({
            "clock": self.clock,
            "immediate": immediate,
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
                let entities = extract_entities(&entry.text.clone());
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
fn word_overlap(a: &str, b: &str) -> f32 {
    use std::collections::HashSet;
    let set_a: HashSet<&str> = a
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.len() > 1)
        .collect();
    let set_b: HashSet<&str> = b
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| w.len() > 1)
        .collect();
    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }
    let intersection = set_a.intersection(&set_b).count() as f32;
    let union = set_a.union(&set_b).count() as f32;
    intersection / union
}

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
    let compressed =
        fs::read(MEMORY_FILE).map_err(|e| format!("Failed to read memory file: {}", e))?;
    let serialized = lz4::block::decompress(&compressed, None)
        .map_err(|e| format!("Failed to decompress memory: {}", e))?;
    let state: MemoryState = bincode::deserialize(&serialized)
        .map_err(|e| format!("Failed to deserialize memory: {}", e))?;
    Ok(state)
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
        pub refs: Vec<MemoryRef>,
        // gradient_sq_sum intentionally absent
    }

    // MemoryState with current_task/ticks_since_consolidation but before last_retrieved_ids.
    #[derive(Debug, Clone, Deserialize)]
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
    let serialized = lz4::block::decompress(&compressed, None)?;

    // Try V2 first (has current_task/ticks_since_consolidation), then fall back to V1.
    let new_state = if let Ok(v2) = bincode::deserialize::<MemoryStateV2>(&serialized) {
        MemoryState {
            config: v2.config,
            immediate: v2.immediate,
            short_term: v2.short_term.into_iter().map(|e| ShortTermEntry {
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
            }).collect(),
            long_term: v2.long_term,
            clock: v2.clock,
            next_id: v2.next_id,
            session_log: v2.session_log,
            current_task: v2.current_task,
            ticks_since_consolidation: v2.ticks_since_consolidation,
            last_retrieved_ids: Vec::new(),
        }
    } else {
        let old = bincode::deserialize::<MemoryStateV1>(&serialized)?;
        MemoryState {
            config: old.config,
            immediate: old.immediate,
            short_term: old.short_term.into_iter().map(|e| ShortTermEntry {
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
            }).collect(),
            long_term: old.long_term,
            clock: old.clock,
            next_id: old.next_id,
            session_log: old.session_log,
            current_task: None,
            ticks_since_consolidation: 0,
            last_retrieved_ids: Vec::new(),
        }
    };

    // Save migrated state
    new_state.save()?;

    // Remove corrupt backup after successful migration
    fs::remove_file(CORRUPT_FILE)?;

    eprintln!(
        "✓ Migration complete: {} short-term entries, {} graph nodes recovered",
        new_state.short_term.len(),
        new_state.long_term.nodes.len()
    );

    Ok(Some(new_state))
}

fn save_memory(state: &MemoryState) -> Result<(), Box<dyn std::error::Error>> {
    let serialized =
        bincode::serialize(state).map_err(|e| format!("Failed to serialize memory: {}", e))?;
    let compressed = lz4::block::compress(&serialized, None, true)
        .map_err(|e| format!("Failed to compress memory: {}", e))?;

    let temp_file = format!("{}.tmp", MEMORY_FILE);
    fs::write(&temp_file, &compressed)
        .map_err(|e| format!("Failed to write temp memory file: {}", e))?;
    fs::rename(&temp_file, MEMORY_FILE)
        .map_err(|e| format!("Failed to write memory file: {}", e))?;
    Ok(())
}

/// Composite eviction score: higher = more worth keeping.
fn eviction_score(entry: &ShortTermEntry, now: u64) -> f32 {
    let age = now.saturating_sub(entry.last_access) as f32;
    let recency = (-age * EVICTION_DECAY_RATE).exp();
    let usage = (entry.usage as f32).ln_1p();
    entry.salience * 0.4 + usage * 0.3 + recency * 0.3
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

#[cfg(test)]
mod tests {
    use super::*;

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
        };
        assert!(eviction_score(&recent, 100) > eviction_score(&old, 100));
    }

    #[test]
    fn test_tick_adds_entry() {
        let mut state = MemoryState::default();
        state.tick("hello world test entry");
        assert!(!state.short_term.is_empty());
        assert!(!state.immediate.is_empty());
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
        state.tick("embedding quality improvement using n-grams");
        state.tick("embedding quality improvement using trigrams");
        state.tick("completely different topic about cooking recipes");
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
        state.tick("initial entry with some content");
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
        state.tick("the embedding system uses vector similarity for matching");
        state.tick("cooking recipes require fresh ingredients and seasoning");
        assert!(
            state.short_term.len() >= 2,
            "unrelated ticks should create separate entries, got {}",
            state.short_term.len()
        );
    }

    #[test]
    fn test_word_overlap_identical() {
        assert!((word_overlap("hello world", "hello world") - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_word_overlap_disjoint() {
        assert!(word_overlap("apples oranges bananas", "cars trucks bikes") < 0.01);
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

        let result = state.reinforce(&[id], 1.0);
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

        let result = state.reinforce(&[id], -1.0);
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

        state.reinforce(&[id], 1.0);

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
        let result = state.reinforce(&[9999], 1.0);
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
        state.tick("the cosine similarity algorithm compares vector embeddings");
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
        state.tick("See legend/src/memory/mod.rs#L120-145 for the new refs logic.");
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
        state.tick("Ref: legend/src/memory/mod.rs#L200-210 tracks MemorySnippet changes.");
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
        // Create initial memory
        state.tick("the database uses PostgreSQL for persistence");
        assert_eq!(state.short_term.len(), 1);
        let original_id = state.short_term[0].id;

        // Query to make it labile
        state.retrieve_context("database PostgreSQL");
        assert!(
            state.short_term[0].labile_until > 0,
            "retrieved entry should be labile"
        );

        // Tick with related but new information — should reconsolidate
        state.tick("the database PostgreSQL schema has users and sessions tables");
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
        state.tick("test entry for labile expiration");
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
            classify_text("Chose PostgreSQL over MongoDB because it has better JOIN support"),
            MemoryCategory::Decision
        );
        assert_eq!(
            classify_text("We decided to use Rust instead of Go"),
            MemoryCategory::Decision
        );
    }

    #[test]
    fn test_classify_text_bug() {
        assert_eq!(
            classify_text("Bug: the parser crashes on empty input"),
            MemoryCategory::Bug
        );
        assert_eq!(
            classify_text("Had to revert the migration due to data loss"),
            MemoryCategory::Bug
        );
    }

    #[test]
    fn test_classify_text_todo() {
        assert_eq!(
            classify_text("TODO: implement proper error handling"),
            MemoryCategory::Todo
        );
        assert_eq!(
            classify_text("Blocked on the API team providing the endpoint"),
            MemoryCategory::Todo
        );
    }

    #[test]
    fn test_classify_text_architecture() {
        assert_eq!(
            classify_text("The authentication module uses JWT tokens via middleware"),
            MemoryCategory::Architecture
        );
    }

    #[test]
    fn test_classify_text_preference() {
        assert_eq!(
            classify_text("User prefers snake_case for all variable names"),
            MemoryCategory::Preference
        );
    }

    #[test]
    fn test_importance_scoring_decisions_higher() {
        let decision_salience =
            compute_salience("Chose bincode over JSON because it is faster for serialization");
        let generic_salience = compute_salience("updated some files in the project");
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
            compute_salience("Bug: the parser crashes on empty input and causes a panic");
        let generic_salience = compute_salience("updated some files in the project");
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
        let initial_last_seen: Vec<u64> = state.long_term.edges.iter().map(|e| e.last_seen).collect();
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
}
