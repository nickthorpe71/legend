/// Neocortex — Semantic memory (L3), knowledge graph, consolidation.
///
/// The neocortex stores abstracted, generalized knowledge extracted from
/// repeated hippocampal experiences. In Legend, this corresponds to the
/// `long_term` knowledge graph (`GraphMemory`):
///
/// - **Graph structure**: Labeled nodes (entities, keywords, summaries) connected
///   by typed, weighted edges. Models the neocortical concept network.
/// - **Hebbian learning**: "Neurons that fire together wire together" — co-retrieved
///   entities get their shared edges strengthened.
/// - **Spreading activation**: Multi-hop BFS-style activation propagation through
///   the graph, modeling associative priming between related concepts.
/// - **Systems consolidation**: During consolidation, high-salience L2 groups get
///   centroid embeddings and rich text stored on Summary nodes, enabling the neocortex
///   to independently serve queries even after L2 entries decay.
/// - **Enriched synaptic encoding**: Edges track activation_count, stability, and
///   dual-timescale interval averages (STP/LTP), modeling structural plasticity.
///
/// Core types (`GraphMemory`, `GraphNode`, `GraphEdge`) remain in `mod.rs` for
/// serialization. This module contains free functions that operate on `GraphMemory`
/// and `BrainState`.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::{
    entorhinal::clean_semantic_noise,
    neurochemistry::{self, ChemicalStamp},
    signal::reinforce_bounded_signal,
    wernicke::{extract_entities, extract_relations, is_graph_entity_candidate, KeywordCache},
    BrainState, EDGE_REINFORCE_DELTA, GRAPH_PRUNE_WEIGHT, GRAPH_WEIGHT_TARGET_MAX,
    HEBBIAN_EDGE_BOOST, HEBBIAN_EDGE_CEILING, HEBBIAN_NODE_BOOST, HEBBIAN_NODE_CEILING,
    NEOCORTICAL_DECAY_RATE, NODE_WEIGHT_BASE, PRUNE_AGE_WEIGHT, REPLAY_EDGE_BOOST,
    REPLAY_SALIENCE_BOOST, REPLAY_TEMPORAL_WINDOW, SPREADING_ACTIVATION_DECAY,
    SPREADING_ACTIVATION_MAX_HOPS,
};

/// CPEB boost decay rate per tick. 0.95 = ~5% decay per tick.
/// After 90 ticks, boost is essentially zero (0.95^90 ≈ 0.01).
const CPEB_DECAY_RATE: f32 = 0.95;

/// Soft stability cap knee point — below this, linear growth.
const STABILITY_KNEE: f32 = 10.0;

/// Maximum reachable stability (asymptote of tanh).
const STABILITY_MAX: f32 = 20.0;

/// Scale parameter for soft cap transition.
const STABILITY_SCALE: f32 = 10.0;

// ---------------------------------------------------------------------------
// Neocortical types
// ---------------------------------------------------------------------------

/// Long-term knowledge graph: labeled nodes connected by typed edges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GraphMemory {
    pub nodes: HashMap<u64, GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// Per-edge neurochemical stamp keyed by canonical "min_id:max_id".
    #[serde(default)]
    pub edge_chemical_stamps: HashMap<String, ChemicalStamp>,
    /// Rich semantic metadata keyed by canonical "min_id:max_id".
    #[serde(default)]
    pub edge_semantics: HashMap<String, GraphEdgeSemantics>,
    /// Label → node ID for fast entity lookup.
    pub index: HashMap<String, u64>,
    /// (min_id, max_id) → edge index for O(1) edge lookup.
    /// Rebuilt on load, not serialized.
    #[serde(skip)]
    pub edge_index: HashMap<(u64, u64), usize>,
}

impl GraphMemory {
    /// Rebuild the edge_index from the edges Vec. Call after deserialization.
    pub fn rebuild_edge_index(&mut self) {
        self.edge_index = HashMap::with_capacity(self.edges.len());
        for (idx, edge) in self.edges.iter().enumerate() {
            let key = if edge.from <= edge.to {
                (edge.from, edge.to)
            } else {
                (edge.to, edge.from)
            };
            self.edge_index.insert(key, idx);
        }
    }

    /// Canonical edge key for index lookups.
    fn edge_key(a: u64, b: u64) -> (u64, u64) {
        if a <= b {
            (a, b)
        } else {
            (b, a)
        }
    }

    pub fn edge_stamp_key(a: u64, b: u64) -> String {
        let (from, to) = Self::edge_key(a, b);
        format!("{from}:{to}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GraphEdgeSemantics {
    pub kind: String,
    pub predicates: Vec<String>,
    pub evidence: Vec<String>,
    pub contradictory_evidence: Vec<String>,
    pub confidence: f32,
    pub polarity: String,
    pub conflict_state: String,
    pub reference_frames: Vec<GraphReferenceFrame>,
    pub support_count: u32,
    pub contradiction_count: u32,
    pub correction_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GraphReferenceFrame {
    pub kind: String,
    pub label: String,
    pub relation: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryCoverage {
    pub source_count: usize,
    pub evidence_count: usize,
    pub omitted_source_count: usize,
    pub full_evidence_preserved: bool,
}

impl Default for SummaryCoverage {
    fn default() -> Self {
        Self {
            source_count: 0,
            evidence_count: 0,
            omitted_source_count: 0,
            full_evidence_preserved: true,
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
    /// Semantic gist for Summary nodes. `label` stays the compact index/display
    /// handle; `gist` carries the extractive meaning of the consolidated group.
    pub gist: Option<String>,
    /// Evidence backing the node. For Summary nodes this is the preserved
    /// cleaned source evidence for the consolidated group.
    pub source_texts: Vec<String>,
    /// Centroid embedding for direct similarity search (systems consolidation).
    /// Populated on Summary nodes so they can be queried independently of L2.
    pub embedding: Vec<f32>,
    /// Richer summary text (up to 500 chars) for consolidated memories.
    pub full_text: Option<String>,
    /// How completely the Summary node represents the source group.
    pub coverage: Option<SummaryCoverage>,
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
            gist: None,
            source_texts: Vec::new(),
            embedding: Vec::new(),
            full_text: None,
            coverage: None,
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
    /// CPEB-specific stability boost that decays separately.
    /// Emotional events add to this; it decays each tick.
    #[serde(default)]
    pub cpeb_boost: f32,
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
            cpeb_boost: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNodeSummary {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gist: Option<String>,
    /// The type of edge that connected this node (for neighbor lookups).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_type: Option<String>,
    #[serde(default)]
    pub source_texts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<SummaryCoverage>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplayStats {
    pub entries_replayed: usize,
    pub edges_reinforced: usize,
}

const EDGE_SURVIVAL_PRUNE_THRESHOLD: f32 = 0.12;

/// Query-mode gated retrieval: bias spreading activation based on the current
/// retrieval goal without requiring graph schema changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryMode {
    Structural,
    Temporal,
    Diagnostic,
    Semantic,
    Neutral,
}

// ── Existing helpers ────────────────────────────────────────────────────

/// Default edge stability for new synaptic connections.
pub fn default_edge_stability() -> f32 {
    1.0
}

/// Default edge kind for new connections.
pub fn default_edge_kind() -> String {
    "related".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeKindClass {
    WeakAssociation,
    Association,
    FrameBinding,
    Temporal,
    KeywordAssociation,
    Structural,
    SummaryRepresentation,
    TypedFact,
    Unknown,
}

fn edge_kind_class(kind: &str) -> EdgeKindClass {
    match kind {
        "co_mentioned" => EdgeKindClass::WeakAssociation,
        "related" => EdgeKindClass::Association,
        "frame_bound" => EdgeKindClass::FrameBinding,
        "temporal" => EdgeKindClass::Temporal,
        "keyword_co_occurs" => EdgeKindClass::KeywordAssociation,
        "contains" | "implements" | "depends_on" | "co_defined" => EdgeKindClass::Structural,
        "represents" => EdgeKindClass::SummaryRepresentation,
        "uses_datastore"
        | "uses"
        | "backs"
        | "restricts_access"
        | "validates"
        | "validates_restore_with"
        | "dashboard_shows"
        | "located_near"
        | "keeps_in"
        | "verifies"
        | "records"
        | "stores"
        | "exceeds" => EdgeKindClass::TypedFact,
        _ => EdgeKindClass::Unknown,
    }
}

fn edge_kind_survival_prior(kind: &str) -> f32 {
    match edge_kind_class(kind) {
        EdgeKindClass::TypedFact => 0.22,
        EdgeKindClass::Structural | EdgeKindClass::SummaryRepresentation => 0.18,
        EdgeKindClass::FrameBinding => 0.15,
        EdgeKindClass::Temporal | EdgeKindClass::KeywordAssociation => 0.12,
        EdgeKindClass::Association | EdgeKindClass::Unknown => 0.04,
        EdgeKindClass::WeakAssociation => 0.0,
    }
}

/// Soft priors over edge kinds for different retrieval modes.
pub fn edge_kind_multiplier(mode: QueryMode, edge_kind: &str) -> f32 {
    let class = edge_kind_class(edge_kind);
    match (mode, class) {
        (QueryMode::Neutral, _) => 1.0,

        (QueryMode::Structural, EdgeKindClass::Structural)
        | (QueryMode::Structural, EdgeKindClass::SummaryRepresentation)
        | (QueryMode::Structural, EdgeKindClass::FrameBinding) => 1.0,
        (QueryMode::Structural, EdgeKindClass::TypedFact) => 0.95,
        (QueryMode::Structural, EdgeKindClass::Association) => 0.85,
        (QueryMode::Structural, EdgeKindClass::WeakAssociation) => 0.75,
        (QueryMode::Structural, EdgeKindClass::KeywordAssociation) => 0.7,
        (QueryMode::Structural, EdgeKindClass::Temporal) => 0.55,
        (QueryMode::Structural, EdgeKindClass::Unknown) => 0.8,

        (QueryMode::Temporal, EdgeKindClass::Temporal) => 1.0,
        (QueryMode::Temporal, EdgeKindClass::Association)
        | (QueryMode::Temporal, EdgeKindClass::WeakAssociation) => 0.8,
        (QueryMode::Temporal, EdgeKindClass::TypedFact) => 0.7,
        (QueryMode::Temporal, EdgeKindClass::KeywordAssociation) => 0.65,
        (QueryMode::Temporal, EdgeKindClass::Structural)
        | (QueryMode::Temporal, EdgeKindClass::SummaryRepresentation)
        | (QueryMode::Temporal, EdgeKindClass::FrameBinding) => 0.6,
        (QueryMode::Temporal, EdgeKindClass::Unknown) => 0.75,

        (QueryMode::Diagnostic, EdgeKindClass::TypedFact)
        | (QueryMode::Diagnostic, EdgeKindClass::FrameBinding)
        | (QueryMode::Diagnostic, EdgeKindClass::Association) => 1.0,
        (QueryMode::Diagnostic, EdgeKindClass::Temporal) => 0.95,
        (QueryMode::Diagnostic, EdgeKindClass::Structural)
        | (QueryMode::Diagnostic, EdgeKindClass::SummaryRepresentation)
        | (QueryMode::Diagnostic, EdgeKindClass::KeywordAssociation) => 0.75,
        (QueryMode::Diagnostic, EdgeKindClass::WeakAssociation) => 0.7,
        (QueryMode::Diagnostic, EdgeKindClass::Unknown) => 0.8,

        (QueryMode::Semantic, EdgeKindClass::TypedFact)
        | (QueryMode::Semantic, EdgeKindClass::FrameBinding)
        | (QueryMode::Semantic, EdgeKindClass::Association)
        | (QueryMode::Semantic, EdgeKindClass::SummaryRepresentation) => 1.0,
        (QueryMode::Semantic, EdgeKindClass::Structural)
        | (QueryMode::Semantic, EdgeKindClass::Temporal)
        | (QueryMode::Semantic, EdgeKindClass::KeywordAssociation)
        | (QueryMode::Semantic, EdgeKindClass::Unknown) => 0.85,
        (QueryMode::Semantic, EdgeKindClass::WeakAssociation) => 0.75,
    }
}

// ── Narrow-param functions (operate on GraphMemory only) ────────────────

/// Multi-hop spreading activation (CA3 recurrent network model).
///
/// BFS-style outward spread from seed nodes. Each hop's activation decays by
/// `decay_factor`, modeling how neural activation attenuates across synapses.
/// Returns (node_id, activation) pairs sorted by activation descending.
pub fn spreading_activation(
    long_term: &GraphMemory,
    seed_ids: &[u64],
    max_hops: usize,
    decay_factor: f32,
    query_mode: QueryMode,
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
            for edge in &long_term.edges {
                let neighbor_id = if edge.from == node_id {
                    Some(edge.to)
                } else if edge.to == node_id {
                    Some(edge.from)
                } else {
                    None
                };

                if let Some(nid) = neighbor_id {
                    if !visited.contains(&nid) && long_term.nodes.contains_key(&nid) {
                        // Synaptic encoding: edges with high stability (spaced
                        // reinforcement) propagate activation more effectively,
                        // while retrieval mode softly biases edge kinds.
                        let kind_multiplier = edge_kind_multiplier(query_mode, &edge.kind);
                        let effective_weight =
                            edge.weight * edge.stability.sqrt() * kind_multiplier;
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

    // Filter by minimum activation — nodes with negligible activation are noise.
    const MIN_ACTIVATION: f32 = 0.01;
    let mut results: Vec<(u64, f32)> = activations
        .into_iter()
        .filter(|&(_, a)| a >= MIN_ACTIVATION)
        .collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results
}

/// Query the knowledge graph: match entities by label, then expand via
/// spreading activation (multi-hop).
pub fn graph_lookup(
    long_term: &GraphMemory,
    query: &str,
    limit: usize,
    keyword_cache: &KeywordCache,
    query_mode: QueryMode,
) -> Vec<GraphNodeSummary> {
    let entities = extract_entities(query, keyword_cache);
    let mut results: Vec<GraphNodeSummary> = Vec::new();
    let mut seed_ids = Vec::new();

    for entity in &entities {
        // Normalize to lowercase to match how update_graph indexes nodes.
        let index_key = entity.label.to_lowercase();
        if let Some(&node_id) = long_term.index.get(&index_key) {
            if let Some(node) = long_term.nodes.get(&node_id) {
                results.push(GraphNodeSummary {
                    id: node.id,
                    label: node.label.clone(),
                    kind: node.kind.clone(),
                    weight: node.weight,
                    gist: node.gist.clone(),
                    edge_type: None, // direct match, no edge
                    source_texts: node.source_texts.clone(),
                    coverage: node.coverage.clone(),
                });
                seed_ids.push(node.id);
            }
        }
    }

    // Multi-hop spreading activation from seed nodes
    if !seed_ids.is_empty() {
        let activated = spreading_activation(
            long_term,
            &seed_ids,
            SPREADING_ACTIVATION_MAX_HOPS,
            SPREADING_ACTIVATION_DECAY,
            query_mode,
        );
        for (nid, activation) in activated {
            if let Some(node) = long_term.nodes.get(&nid) {
                results.push(GraphNodeSummary {
                    id: node.id,
                    label: node.label.clone(),
                    kind: node.kind.clone(),
                    weight: node.weight + activation,
                    gist: node.gist.clone(),
                    edge_type: Some("activated".to_string()),
                    source_texts: node.source_texts.clone(),
                    coverage: node.coverage.clone(),
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
    // Use caller's limit as a safety cap, but primary gating is the activation
    // threshold in spreading_activation (MIN_ACTIVATION = 0.15).
    results.truncate(limit);
    results
}

/// Boost L2 candidates using graph-connected entities not in the query.
///
/// Starting from query entities, spread activation through the graph to find
/// related entities. Each activated entity that appears in an L2 candidate's
/// text contributes a bonus weighted by **specificity** (inverse node weight).
/// Common entities like "Hawaii" (high weight) give small bonuses while specific
/// entities like "February" (low weight) give large bonuses — an IDF-like signal
/// derived purely from graph structure.
///
/// Spreading continues until activation falls below `min_activation` (no fixed
/// hop limit). Paths through high-stability, high-weight edges propagate further.
pub fn graph_boost_candidates(
    long_term: &GraphMemory,
    query_entities: &[super::wernicke::extract::ExtractedEntity],
    candidates: &mut [super::hippocampus::MemorySnippet],
    query_mode: QueryMode,
) {
    /// Per-entity bonus base before specificity weighting.
    const BONUS_BASE: f32 = 0.05;
    /// Maximum total graph boost per candidate.
    const BONUS_CAP: f32 = 0.3;
    /// Minimum activation to continue spreading. Stops expansion naturally
    /// when paths lead to weakly-connected regions.
    const MIN_ACTIVATION: f32 = 0.01;
    /// Per-hop decay factor for activation.
    const HOP_DECAY: f32 = 0.5;
    /// Maximum hops to prevent runaway in dense graphs.
    const MAX_HOPS: usize = 6;

    // Resolve query entity labels to graph seed nodes
    let mut seed_ids: Vec<u64> = Vec::new();
    let mut query_labels: HashSet<String> = HashSet::new();
    for entity in query_entities {
        let key = entity.label.to_lowercase();
        query_labels.insert(key.clone());
        if let Some(&node_id) = long_term.index.get(&key) {
            seed_ids.push(node_id);
        }
    }

    if seed_ids.is_empty() || candidates.is_empty() {
        return;
    }

    // Spreading activation with min_activation cutoff (no fixed hop limit)
    let mut activations: HashMap<u64, f32> = HashMap::new();
    let mut visited: HashSet<u64> = seed_ids.iter().copied().collect();
    let mut frontier: Vec<(u64, f32)> = seed_ids.iter().map(|&id| (id, 1.0_f32)).collect();

    for hop in 0..MAX_HOPS {
        let hop_decay = HOP_DECAY.powi(hop as i32 + 1);
        let mut next_frontier: Vec<(u64, f32)> = Vec::new();

        for &(node_id, parent_activation) in &frontier {
            for edge in &long_term.edges {
                let neighbor_id = if edge.from == node_id {
                    Some(edge.to)
                } else if edge.to == node_id {
                    Some(edge.from)
                } else {
                    None
                };

                if let Some(nid) = neighbor_id {
                    if !visited.contains(&nid) {
                        if let Some(neighbor) = long_term.nodes.get(&nid) {
                            let kind_mult = edge_kind_multiplier(query_mode, &edge.kind);
                            let effective_weight = edge.weight * edge.stability.sqrt() * kind_mult;
                            let activation = parent_activation * effective_weight * hop_decay;

                            if activation >= MIN_ACTIVATION {
                                let entry = activations.entry(nid).or_insert(0.0);
                                if activation > *entry {
                                    *entry = activation;
                                }
                                if visited.insert(nid) {
                                    // Only continue spreading from non-query entities
                                    let label_lower = neighbor.label.to_lowercase();
                                    if !query_labels.contains(&label_lower) {
                                        next_frontier.push((nid, activation));
                                    }
                                }
                            }
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

    // Collect (label, specificity-weighted bonus) for activated non-query entities.
    // Specificity = 1 / max(0.1, node_weight) — rare entities get higher bonus.
    let mut graph_labels: Vec<(String, f32)> = Vec::new();
    for (&nid, &activation) in &activations {
        if let Some(node) = long_term.nodes.get(&nid) {
            let label_lower = node.label.to_lowercase();
            if label_lower.len() > 2 && !query_labels.contains(&label_lower) {
                // Specificity: inverse of node weight (IDF-like)
                let specificity = 1.0 / node.weight.max(0.1);
                let weighted_bonus = BONUS_BASE * activation * specificity;
                graph_labels.push((label_lower, weighted_bonus));
            }
        }
    }

    // Apply boost to L2 candidates
    for candidate in candidates.iter_mut() {
        let entry_lower = candidate.text.to_lowercase();
        let bonus: f32 = graph_labels
            .iter()
            .filter(|(label, _)| entry_lower.contains(label.as_str()))
            .map(|(_, b)| b)
            .sum::<f32>()
            .min(BONUS_CAP);
        if bonus > 0.0 {
            candidate.similarity = (candidate.similarity + bonus).min(1.0);
        }
    }
}

fn edge_semantic_survival_score(semantics: Option<&GraphEdgeSemantics>) -> f32 {
    let Some(semantics) = semantics else {
        return 0.0;
    };

    let support = ((semantics.support_count as f32).ln_1p() * 0.08).min(0.24);
    let confidence = semantics.confidence.clamp(0.0, 1.0) * 0.12;
    let frames = (semantics.reference_frames.len() as f32 * 0.04).min(0.16);
    let conflict_protection = if semantics.correction_count > 0 {
        0.35
    } else if semantics.contradiction_count > 0 {
        0.28
    } else {
        0.0
    };

    support + confidence + frames + conflict_protection
}

fn edge_local_chemical_survival_score(stamp: Option<&ChemicalStamp>, edge: &GraphEdge) -> f32 {
    let chemical = stamp
        .map(|stamp| (edge_chemical_decay_protection(stamp) - 1.0).clamp(0.0, 0.35))
        .unwrap_or(0.0);
    chemical + edge.cpeb_boost.min(0.35)
}

fn edge_history_survival_score(edge: &GraphEdge, clock: u64) -> f32 {
    let activation = ((edge.activation_count as f32).ln_1p() * 0.04).min(0.2);
    let age = clock.saturating_sub(edge.last_seen) as f32;
    let recency = (1.0 / (1.0 + age / 500.0)) * 0.08;
    activation + recency
}

fn edge_survival_score(
    edge: &GraphEdge,
    semantics: Option<&GraphEdgeSemantics>,
    chemical_stamp: Option<&ChemicalStamp>,
    clock: u64,
) -> f32 {
    let weight = edge.weight.max(0.0).min(1.0) * 0.45;
    let stability = ((edge.stability.max(1.0).sqrt() - 1.0) * 0.08).min(0.22);
    weight
        + stability
        + edge_kind_survival_prior(&edge.kind)
        + edge_semantic_survival_score(semantics)
        + edge_local_chemical_survival_score(chemical_stamp, edge)
        + edge_history_survival_score(edge, clock)
}

/// Remove low-weight graph nodes and edges with weak survival evidence.
pub fn prune_graph(long_term: &mut GraphMemory, clock: u64) {
    // 1. Remove nodes whose decayed weight has fallen below threshold
    let remove_ids: Vec<u64> = long_term
        .nodes
        .iter()
        .filter(|(_, node)| {
            let age = clock.saturating_sub(node.last_seen) as f32;
            let effective = node.weight - age * PRUNE_AGE_WEIGHT;
            effective < GRAPH_PRUNE_WEIGHT
        })
        .map(|(&id, _)| id)
        .collect();

    for &id in &remove_ids {
        if let Some(node) = long_term.nodes.remove(&id) {
            long_term.index.remove(&node.label.to_lowercase());
        }
    }

    // 2. Remove edges referencing deleted nodes
    let node_ids = &long_term.nodes;
    long_term
        .edges
        .retain(|e| node_ids.contains_key(&e.from) && node_ids.contains_key(&e.to));

    // 3. Remove weak, stale edges only when they lack semantic, chemical,
    // replay/history, or conflict/correction survival evidence.
    let edge_semantics = long_term.edge_semantics.clone();
    let edge_stamps = long_term.edge_chemical_stamps.clone();
    long_term.edges.retain(|edge| {
        let key = GraphMemory::edge_stamp_key(edge.from, edge.to);
        let survival =
            edge_survival_score(edge, edge_semantics.get(&key), edge_stamps.get(&key), clock);
        survival >= EDGE_SURVIVAL_PRUNE_THRESHOLD
    });

    // Rebuild edge index after retain may have invalidated it
    long_term.rebuild_edge_index();
    let live_edge_keys: HashSet<String> = long_term
        .edges
        .iter()
        .map(|edge| GraphMemory::edge_stamp_key(edge.from, edge.to))
        .collect();
    long_term
        .edge_chemical_stamps
        .retain(|key, _| live_edge_keys.contains(key));
    long_term
        .edge_semantics
        .retain(|key, _| live_edge_keys.contains(key));
}

/// Proportionally scale all graph node and edge weights so the maximum node weight
/// never exceeds GRAPH_WEIGHT_TARGET_MAX. Preserves relative rankings.
pub fn normalize_graph_weights(long_term: &mut GraphMemory) {
    let max_weight = long_term
        .nodes
        .values()
        .map(|n| n.weight)
        .fold(0.0_f32, f32::max);
    if max_weight <= GRAPH_WEIGHT_TARGET_MAX || max_weight < 0.01 {
        return;
    }
    let scale = GRAPH_WEIGHT_TARGET_MAX / max_weight;
    for node in long_term.nodes.values_mut() {
        node.weight *= scale;
    }
    for edge in &mut long_term.edges {
        edge.weight *= scale;
    }
}

/// Insert a new edge or reinforce an existing one between two nodes.
#[allow(dead_code)]
pub fn upsert_edge(long_term: &mut GraphMemory, from: u64, to: u64, kind: &str, clock: u64) {
    upsert_edge_with_chemical_stamp(long_term, from, to, kind, clock, &ChemicalStamp::default());
}

/// Insert or reinforce an edge while preserving its local modulatory state.
pub fn upsert_edge_with_chemical_stamp(
    long_term: &mut GraphMemory,
    from: u64,
    to: u64,
    kind: &str,
    clock: u64,
    chemical_stamp: &ChemicalStamp,
) {
    upsert_edge_with_weight_and_chemical_stamp(
        long_term,
        from,
        to,
        kind,
        EDGE_REINFORCE_DELTA,
        clock,
        chemical_stamp,
    );
}

/// Insert or reinforce an edge with an evidence-specific plasticity delta.
///
/// This keeps the default Hebbian path intact while letting encoding distinguish
/// local reference-frame evidence from weak same-chunk co-mentions.
pub fn upsert_edge_with_weight_and_chemical_stamp(
    long_term: &mut GraphMemory,
    from: u64,
    to: u64,
    kind: &str,
    weight_delta: f32,
    clock: u64,
    chemical_stamp: &ChemicalStamp,
) {
    let key = GraphMemory::edge_key(from, to);
    let stamp_key = GraphMemory::edge_stamp_key(from, to);
    let weight_delta = weight_delta.max(0.0);
    if let Some(&idx) = long_term.edge_index.get(&key) {
        let edge = &mut long_term.edges[idx];
        // Synaptic plasticity: update dual-timescale interval tracking
        let interval = clock.saturating_sub(edge.last_seen) as f32;
        if edge.activation_count > 0 && interval > 0.0 {
            edge.recent_interval_avg = 0.5 * interval + 0.5 * edge.recent_interval_avg;
            edge.historical_interval_avg = 0.1 * interval + 0.9 * edge.historical_interval_avg;

            // Spaced repetition: compare recent vs historical intervals
            if edge.historical_interval_avg > 0.0 {
                let spacing_ratio = edge.recent_interval_avg / edge.historical_interval_avg;
                if spacing_ratio > 1.0 {
                    // Intervals are growing → spaced reinforcement
                    edge.stability = soft_cap_stability(edge.stability * 1.3);
                } else {
                    // Intervals are shrinking or constant → cramming
                    edge.stability = soft_cap_stability(edge.stability * 1.05);
                }
            }
        } else if edge.activation_count == 0 {
            // First reinforcement: initialize both EMAs to this interval
            edge.recent_interval_avg = interval;
            edge.historical_interval_avg = interval;
        }

        edge.activation_count = edge.activation_count.saturating_add(1);
        edge.weight += weight_delta;
        edge.last_seen = clock;
        if edge_kind_priority(kind) > edge_kind_priority(&edge.kind) {
            edge.kind = kind.to_string();
        }
        let stamp = long_term.edge_chemical_stamps.entry(stamp_key).or_default();
        blend_chemical_stamp(stamp, chemical_stamp, 0.25);
    } else {
        let new_idx = long_term.edges.len();
        long_term.edges.push(GraphEdge {
            from,
            to,
            weight: weight_delta,
            kind: kind.to_string(),
            last_seen: clock,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
            cpeb_boost: 0.0,
        });
        long_term.edge_index.insert(key, new_idx);
        long_term
            .edge_chemical_stamps
            .insert(stamp_key, chemical_stamp.clone());
    }
}

pub struct EdgeSemanticInput {
    pub predicate: String,
    pub evidence: String,
    pub confidence: f32,
    pub polarity: String,
    pub reference_frames: Vec<GraphReferenceFrame>,
}

pub fn upsert_edge_with_semantics(
    long_term: &mut GraphMemory,
    from: u64,
    to: u64,
    kind: &str,
    weight_delta: f32,
    clock: u64,
    chemical_stamp: &ChemicalStamp,
    semantics: EdgeSemanticInput,
) {
    upsert_edge_with_weight_and_chemical_stamp(
        long_term,
        from,
        to,
        kind,
        weight_delta,
        clock,
        chemical_stamp,
    );
    if let Some(support_count) = merge_edge_semantics(long_term, from, to, kind, semantics) {
        apply_semantic_support_reinforcement(long_term, from, to, support_count);
    }
}

fn merge_edge_semantics(
    long_term: &mut GraphMemory,
    from: u64,
    to: u64,
    kind: &str,
    incoming: EdgeSemanticInput,
) -> Option<u32> {
    let key = GraphMemory::edge_stamp_key(from, to);
    let semantics = long_term.edge_semantics.entry(key).or_default();
    if edge_kind_priority(kind) >= edge_kind_priority(&semantics.kind) {
        semantics.kind = kind.to_string();
    }
    push_unique_capped(&mut semantics.predicates, incoming.predicate, 8);
    let incoming_polarity = incoming.polarity;
    let incoming_evidence = incoming.evidence;
    let mut support_count_if_incremented = None;
    match incoming_polarity.as_str() {
        "Negated" => {
            push_unique_capped(&mut semantics.contradictory_evidence, incoming_evidence, 8);
            semantics.contradiction_count = semantics.contradiction_count.saturating_add(1);
        }
        "Corrective" => {
            push_unique_capped(&mut semantics.contradictory_evidence, incoming_evidence, 8);
            semantics.correction_count = semantics.correction_count.saturating_add(1);
        }
        _ => {
            push_unique_capped(&mut semantics.evidence, incoming_evidence, 8);
            semantics.support_count = semantics.support_count.saturating_add(1);
            support_count_if_incremented = Some(semantics.support_count);
        }
    }
    semantics.confidence = semantics
        .confidence
        .max(incoming.confidence.clamp(0.0, 1.0));
    semantics.polarity = merge_polarity(&semantics.polarity, &incoming_polarity);
    semantics.conflict_state = conflict_state_for(semantics);
    for frame in incoming.reference_frames {
        if !semantics.reference_frames.iter().any(|existing| {
            existing.kind == frame.kind
                && existing.label == frame.label
                && existing.relation == frame.relation
        }) {
            semantics.reference_frames.push(frame);
        }
    }
    semantics.reference_frames.truncate(8);
    support_count_if_incremented
}

fn merge_polarity(existing: &str, incoming: &str) -> String {
    if existing.is_empty() || existing == "Unknown" {
        return incoming.to_string();
    }
    if incoming == "Unknown" || existing == incoming {
        return existing.to_string();
    }
    if incoming == "Corrective" || existing == "Corrective" {
        return "Corrective".to_string();
    }
    "Mixed".to_string()
}

fn conflict_state_for(semantics: &GraphEdgeSemantics) -> String {
    if semantics.correction_count > 0 {
        "Corrected".to_string()
    } else if semantics.support_count > 0 && semantics.contradiction_count > 0 {
        "Conflicted".to_string()
    } else if semantics.contradiction_count > 0 {
        "Contradicted".to_string()
    } else {
        "Supported".to_string()
    }
}

fn apply_semantic_support_reinforcement(
    long_term: &mut GraphMemory,
    from: u64,
    to: u64,
    support_count: u32,
) {
    let support_gain = (support_count as f32).ln_1p();
    let edge_boost = (0.04 * support_gain).min(0.2);
    if let Some(&idx) = long_term.edge_index.get(&GraphMemory::edge_key(from, to)) {
        let edge = &mut long_term.edges[idx];
        edge.stability = soft_cap_stability(edge.stability + edge_boost);
    }

    let node_boost = (NODE_WEIGHT_BASE * 0.2 * support_gain).min(0.12);
    for node_id in [from, to] {
        if let Some(node) = long_term.nodes.get_mut(&node_id) {
            node.weight += node_boost;
            node.salience = reinforce_bounded_signal(node.salience, 0.55, 0.18, 0.45, 1.4);
        }
    }
}

fn push_unique_capped(values: &mut Vec<String>, value: String, cap: usize) {
    if value.trim().is_empty() {
        return;
    }
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
    if values.len() > cap {
        let excess = values.len() - cap;
        values.drain(0..excess);
    }
}

fn edge_kind_priority(kind: &str) -> u8 {
    match edge_kind_class(kind) {
        EdgeKindClass::TypedFact
        | EdgeKindClass::Structural
        | EdgeKindClass::SummaryRepresentation => 5,
        EdgeKindClass::FrameBinding => 4,
        EdgeKindClass::Temporal | EdgeKindClass::KeywordAssociation => 3,
        EdgeKindClass::Association | EdgeKindClass::Unknown => 2,
        EdgeKindClass::WeakAssociation => 1,
    }
}

fn blend_chemical_stamp(current: &mut ChemicalStamp, incoming: &ChemicalStamp, alpha: f32) {
    if current.ne_at_encoding == 0.0
        && current.cortisol_at_encoding == 0.0
        && current.da_at_encoding == 0.0
        && current.ach_at_encoding == 0.0
    {
        *current = incoming.clone();
        return;
    }

    let alpha = alpha.clamp(0.0, 1.0);
    current.ne_at_encoding =
        current.ne_at_encoding * (1.0 - alpha) + incoming.ne_at_encoding * alpha;
    current.cortisol_at_encoding =
        current.cortisol_at_encoding * (1.0 - alpha) + incoming.cortisol_at_encoding * alpha;
    current.da_at_encoding =
        current.da_at_encoding * (1.0 - alpha) + incoming.da_at_encoding * alpha;
    current.ach_at_encoding =
        current.ach_at_encoding * (1.0 - alpha) + incoming.ach_at_encoding * alpha;
}

fn averaged_chemical_stamp(a: &ChemicalStamp, b: &ChemicalStamp) -> ChemicalStamp {
    ChemicalStamp {
        ne_at_encoding: (a.ne_at_encoding + b.ne_at_encoding) * 0.5,
        cortisol_at_encoding: (a.cortisol_at_encoding + b.cortisol_at_encoding) * 0.5,
        da_at_encoding: (a.da_at_encoding + b.da_at_encoding) * 0.5,
        ach_at_encoding: (a.ach_at_encoding + b.ach_at_encoding) * 0.5,
    }
}

fn edge_chemical_decay_protection(stamp: &ChemicalStamp) -> f32 {
    1.0 + stamp.ne_at_encoding * 0.5 + stamp.da_at_encoding * 0.25
}

/// "Neurons that fire together wire together" — co-retrieved entities get
/// their shared edges strengthened, with logarithmic dampening for heavily-
/// activated synapses (LTP ceiling).
pub fn hebbian_reinforce(long_term: &mut GraphMemory, co_retrieved_ids: &[u64], clock: u64) {
    if co_retrieved_ids.len() < 2 {
        return;
    }

    for edge in &mut long_term.edges {
        if co_retrieved_ids.contains(&edge.from) && co_retrieved_ids.contains(&edge.to) {
            // Logarithmic dampening: heavily-activated synapses saturate (LTP ceiling)
            let dampened_boost =
                HEBBIAN_EDGE_BOOST / (1.0 + (edge.activation_count as f32 + 1.0).ln());
            edge.weight = (edge.weight + dampened_boost).min(HEBBIAN_EDGE_CEILING);
            edge.last_seen = clock;
        }
    }

    for &id in co_retrieved_ids {
        if let Some(node) = long_term.nodes.get_mut(&id) {
            node.weight = (node.weight + HEBBIAN_NODE_BOOST).min(HEBBIAN_NODE_CEILING);
            node.last_seen = clock;
        }
    }
}

/// Soft cap on stability — diminishing returns above the knee point.
/// f(x) = knee + (max - knee) * tanh((x - knee) / scale)
/// At knee (10.0), growth slows but never stops entirely.
/// At 2× knee, stability ≈ 14.5 (vs hard cap of 10.0).
pub fn soft_cap_stability(raw: f32) -> f32 {
    if raw <= STABILITY_KNEE {
        raw
    } else {
        STABILITY_KNEE
            + (STABILITY_MAX - STABILITY_KNEE) * ((raw - STABILITY_KNEE) / STABILITY_SCALE).tanh()
    }
}

/// CPEB-inspired synaptic tagging scoped to edges connected to nodes
/// that were actually touched during this tick's processing.
///
/// Unlike the old `cpeb_tag_edges` which tagged ALL recently-active edges
/// within a time window, this only tags edges connected to `touched_node_ids`,
/// preventing unrelated edges from receiving emotional boosts.
pub fn cpeb_tag_edges_scoped(
    long_term: &mut GraphMemory,
    _clock: u64,
    valence_magnitude: f32,
    stability_boost: f32,
    touched_node_ids: &[u64],
) -> u32 {
    let mut tagged_count = 0u32;
    for edge in &mut long_term.edges {
        if touched_node_ids.contains(&edge.from) || touched_node_ids.contains(&edge.to) {
            let boost = stability_boost * valence_magnitude;
            edge.cpeb_boost += boost;
            edge.stability = soft_cap_stability(edge.stability + boost);
            tagged_count += 1;
        }
    }
    tagged_count
}

/// Apply L3 decay to graph nodes and edges.
/// `decay_rate_mod`: neurochemical multiplier (Phase C) — >1.0 accelerates decay.
pub fn apply_l3_decay(long_term: &mut GraphMemory, clock: u64, decay_rate_mod: f32) {
    for node in long_term.nodes.values_mut() {
        let decay = (-(clock.saturating_sub(node.last_seen) as f32)
            * NEOCORTICAL_DECAY_RATE
            * decay_rate_mod)
            .exp();
        node.weight *= decay;
        node.salience *= decay;
    }
    // Edge decay: edges that haven't been reinforced recently lose weight.
    // Local NE/DA stamps protect salient/rewarded associations from decaying
    // as quickly as neutral edges under the same global chemistry.
    let edge_stamps = &long_term.edge_chemical_stamps;
    for edge in &mut long_term.edges {
        let chemical_protection = edge_stamps
            .get(&GraphMemory::edge_stamp_key(edge.from, edge.to))
            .map(edge_chemical_decay_protection)
            .unwrap_or(1.0);
        let effective_decay_rate = NEOCORTICAL_DECAY_RATE * decay_rate_mod
            / (edge.stability.max(1.0) * chemical_protection);
        let decay = (-(clock.saturating_sub(edge.last_seen) as f32) * effective_decay_rate).exp();
        edge.weight *= decay;

        // CPEB boost decays faster than base stability — emotional urgency fades
        if edge.cpeb_boost > 0.01 {
            let decay_amount = edge.cpeb_boost * (1.0 - CPEB_DECAY_RATE);
            edge.cpeb_boost -= decay_amount;
            edge.stability = (edge.stability - decay_amount).max(1.0);
        } else {
            edge.cpeb_boost = 0.0;
        }
    }
}

// ── Wide-param functions (operate on &mut BrainState) ──────────────────

/// Extract entities from text and insert/update nodes and edges in the knowledge graph.
/// Returns the node IDs that were created or updated.
pub fn update_graph(state: &mut BrainState, text: &str, salience: f32) -> Vec<u64> {
    let semantic_text = clean_semantic_noise(text);
    let graph_text = if semantic_text.trim().is_empty() {
        text
    } else {
        semantic_text.as_str()
    };
    let entities: Vec<_> = extract_entities(graph_text, &state.keyword_cache)
        .into_iter()
        .filter(is_graph_entity_candidate)
        .collect();
    if entities.is_empty() {
        return Vec::new();
    }

    let chemical_stamp = neurochemistry::stamp_from(&state.chemistry);
    let mut node_ids = Vec::new();
    let mut edge_mentions = Vec::new();

    for entity in &entities {
        // Normalize index key to lowercase so "Samsung" and "samsung" share one node.
        // Display label keeps original casing of first insertion.
        let index_key = entity.label.to_lowercase();
        let id = if let Some(&existing) = state.long_term.index.get(&index_key) {
            existing
        } else {
            let id = state.next_id;
            state.next_id += 1;
            state.long_term.nodes.insert(
                id,
                GraphNode {
                    id,
                    label: entity.label.clone(),
                    kind: entity.kind.clone(),
                    weight: 1.0,
                    last_seen: state.clock,
                    salience,
                    gist: None,
                    source_texts: Vec::new(),
                    embedding: Vec::new(),
                    full_text: None,
                    coverage: None,
                },
            );
            state.long_term.index.insert(index_key, id);
            id
        };

        if let Some(node) = state.long_term.nodes.get_mut(&id) {
            let weight_multiplier = entity_kind_weight_multiplier(&entity.kind);

            node.weight += (NODE_WEIGHT_BASE + salience * 0.3) * weight_multiplier;
            node.last_seen = state.clock;
            node.salience = reinforce_bounded_signal(
                node.salience,
                salience,
                0.5 * weight_multiplier,
                0.45,
                1.4,
            );

            // Update kind if it was previously generic or less specific
            if state.keyword_cache.kind_priority(&entity.kind)
                > state.keyword_cache.kind_priority(&node.kind)
            {
                node.kind = entity.kind.clone();
            }
        }

        node_ids.push(id);
        edge_mentions.push(GraphEntityMention {
            node_id: id,
            context: entity.context.clone(),
            position: find_entity_position(graph_text, &entity.label),
        });
    }

    for i in 0..edge_mentions.len() {
        for j in (i + 1)..edge_mentions.len() {
            let evidence = edge_evidence(&edge_mentions[i], &edge_mentions[j], graph_text);
            upsert_edge_with_weight_and_chemical_stamp(
                &mut state.long_term,
                edge_mentions[i].node_id,
                edge_mentions[j].node_id,
                evidence.kind,
                evidence.weight_delta,
                state.clock,
                &chemical_stamp,
            );
        }
    }

    for relation in extract_relations(graph_text, &state.keyword_cache) {
        let Some(&subject_id) = state
            .long_term
            .index
            .get(&relation.subject.label.to_ascii_lowercase())
        else {
            continue;
        };
        let Some(&object_id) = state
            .long_term
            .index
            .get(&relation.object.label.to_ascii_lowercase())
        else {
            continue;
        };
        if subject_id == object_id {
            continue;
        }
        let reference_frames = relation
            .reference_frames
            .iter()
            .map(|frame| GraphReferenceFrame {
                kind: frame.kind.clone(),
                label: frame.label.clone(),
                relation: frame.relation.clone(),
                confidence: frame.confidence,
            })
            .collect();
        upsert_edge_with_semantics(
            &mut state.long_term,
            subject_id,
            object_id,
            &relation.kind,
            EDGE_REINFORCE_DELTA * relation.confidence.max(0.5),
            state.clock,
            &chemical_stamp,
            EdgeSemanticInput {
                predicate: relation.predicate.clone(),
                evidence: relation.evidence.clone(),
                confidence: relation.confidence,
                polarity: format!("{:?}", relation.polarity),
                reference_frames,
            },
        );
    }

    // Phase E: Hebbian reinforcement for keyword nodes.
    // Scan text for matches against keyword graph nodes and boost their weight.
    let text_lower = graph_text.to_lowercase();
    let keyword_node_ids: Vec<u64> = state
        .long_term
        .nodes
        .iter()
        .filter(|(_, n)| n.kind == "Keyword")
        .map(|(&id, _)| id)
        .collect();

    for kw_id in keyword_node_ids {
        let term = {
            let node = &state.long_term.nodes[&kw_id];
            // Extract term from label "kw:<category>:<term>"
            node.label.splitn(3, ':').nth(2).unwrap_or("").to_string()
        };
        if !term.is_empty() && text_lower.contains(&term.to_lowercase()) {
            if let Some(node) = state.long_term.nodes.get_mut(&kw_id) {
                node.weight = (node.weight + HEBBIAN_NODE_BOOST).min(HEBBIAN_NODE_CEILING);
                node.last_seen = state.clock;
            }
            // Reinforce edges between this keyword and other active nodes
            for &other_id in &node_ids {
                if other_id != kw_id {
                    upsert_edge_with_chemical_stamp(
                        &mut state.long_term,
                        kw_id,
                        other_id,
                        "keyword_co_occurs",
                        state.clock,
                        &chemical_stamp,
                    );
                }
            }
        }
    }

    node_ids
}

struct GraphEntityMention {
    node_id: u64,
    context: String,
    position: Option<usize>,
}

struct EdgeEvidence {
    kind: &'static str,
    weight_delta: f32,
}

fn entity_kind_weight_multiplier(kind: &str) -> f32 {
    match kind {
        // Exact evidence anchors are valuable across domains: times, quantities,
        // paths, and URLs are hard to reconstruct from gist alone.
        "Date" | "Value" | "FilePath" | "Url" => 1.35,
        // Named objects and concepts carry most semantic graph structure.
        "Project" | "Person" | "Organization" | "Location" | "Tool" | "Environment" | "Type"
        | "Concept" => 1.25,
        // Code artifacts are structural entities, but not privileged above
        // non-code objects in the long-term graph.
        "Function" | "Struct" | "Enum" | "Trait" | "Class" | "Symbol" | "Module" | "Package"
        | "Decorator" => 1.2,
        // Generic terms should survive through repetition or typed relation
        // support, not because a single low-specificity mention appeared.
        "Term" => 0.7,
        _ => 1.0,
    }
}

fn edge_evidence(a: &GraphEntityMention, b: &GraphEntityMention, text: &str) -> EdgeEvidence {
    let context_kind = match (a.context.as_str(), b.context.as_str()) {
        ("defines", "mentions") | ("mentions", "defines") => Some("contains"),
        (left, right) if left == "uses" || right == "uses" => Some("depends_on"),
        (left, right) if left == "implements" || right == "implements" => Some("implements"),
        ("defines", "defines") => Some("co_defined"),
        _ => None,
    };

    if let Some(kind) = context_kind {
        return EdgeEvidence {
            kind,
            weight_delta: EDGE_REINFORCE_DELTA,
        };
    }

    let Some(pos_a) = a.position else {
        return weak_co_mention();
    };
    let Some(pos_b) = b.position else {
        return weak_co_mention();
    };

    if same_sentence(text, pos_a, pos_b) {
        let distance = pos_a.abs_diff(pos_b);
        if distance <= 140 {
            EdgeEvidence {
                kind: "frame_bound",
                weight_delta: EDGE_REINFORCE_DELTA * 0.85,
            }
        } else {
            EdgeEvidence {
                kind: "related",
                weight_delta: EDGE_REINFORCE_DELTA * 0.55,
            }
        }
    } else {
        weak_co_mention()
    }
}

fn weak_co_mention() -> EdgeEvidence {
    EdgeEvidence {
        kind: "co_mentioned",
        weight_delta: EDGE_REINFORCE_DELTA * 0.25,
    }
}

fn find_entity_position(text: &str, label: &str) -> Option<usize> {
    text.to_ascii_lowercase().find(&label.to_ascii_lowercase())
}

fn same_sentence(text: &str, a: usize, b: usize) -> bool {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    !text[start..end]
        .chars()
        .any(|c| matches!(c, '.' | '!' | '?' | '\n'))
}

/// Replay consolidation: reinforce L3 edges between entities that co-occur
/// in temporally proximate L2 entries (offline replay / sleep consolidation).
pub fn replay_consolidation(state: &mut BrainState) {
    let _ = replay_consolidation_for_entries(state, &[]);
}

/// Budgeted replay consolidation for selected L2 entry IDs.
///
/// When `selected_entry_ids` is empty this behaves like full replay. Otherwise
/// only temporal pairs where at least one member is selected can participate.
pub fn replay_consolidation_for_entries(
    state: &mut BrainState,
    selected_entry_ids: &[u64],
) -> ReplayStats {
    let n = state.short_term.len();
    if n < 2 {
        return ReplayStats::default();
    }
    let selected: HashSet<u64> = selected_entry_ids.iter().copied().collect();
    let full_replay = selected.is_empty();

    // Collect (index, entities) for each L2 entry
    let entry_entities: Vec<(usize, Vec<(String, u64)>)> = state
        .short_term
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let entities: Vec<_> = extract_entities(&entry.text, &state.keyword_cache)
                .into_iter()
                .filter(is_graph_entity_candidate)
                .collect();
            let resolved: Vec<(String, u64)> = entities
                .iter()
                .filter_map(|e| {
                    state
                        .long_term
                        .index
                        .get(&e.label)
                        .map(|&id| (e.label.clone(), id))
                })
                .collect();
            (i, resolved)
        })
        .collect();

    let mut replayed_indices: HashSet<usize> = HashSet::new();
    let mut edges_reinforced = 0usize;

    // Ensure edge index is populated (may be empty if edges were pushed without registration)
    if state.long_term.edge_index.is_empty() && !state.long_term.edges.is_empty() {
        state.long_term.rebuild_edge_index();
    }

    for i in 0..entry_entities.len() {
        for j in (i + 1)..entry_entities.len() {
            let ei = &state.short_term[entry_entities[i].0];
            let ej = &state.short_term[entry_entities[j].0];
            if !full_replay && !selected.contains(&ei.id) && !selected.contains(&ej.id) {
                continue;
            }

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

            // Reinforce existing edges between entity pairs from the two entries.
            // Only boost edges that already exist — replay strengthens known
            // associations, it doesn't create new ones (prevents edge explosion).
            for (_, id_a) in entities_i {
                for (_, id_b) in entities_j {
                    if id_a == id_b {
                        continue;
                    }
                    let key = if id_a <= id_b {
                        (*id_a, *id_b)
                    } else {
                        (*id_b, *id_a)
                    };
                    if let Some(&edge_idx) = state.long_term.edge_index.get(&key) {
                        state.long_term.edges[edge_idx].weight += REPLAY_EDGE_BOOST;
                        state.long_term.edges[edge_idx].last_seen = state.clock;
                        edges_reinforced += 1;
                        let replay_stamp =
                            averaged_chemical_stamp(&ei.chemical_stamp, &ej.chemical_stamp);
                        let stamp_key = GraphMemory::edge_stamp_key(*id_a, *id_b);
                        let stamp = state
                            .long_term
                            .edge_chemical_stamps
                            .entry(stamp_key)
                            .or_default();
                        blend_chemical_stamp(stamp, &replay_stamp, 0.1);
                    }
                }
            }

            replayed_indices.insert(entry_entities[i].0);
            replayed_indices.insert(entry_entities[j].0);
        }
    }

    // Boost salience of replayed entries
    for &idx in &replayed_indices {
        state.short_term[idx].salience = reinforce_bounded_signal(
            state.short_term[idx].salience,
            REPLAY_SALIENCE_BOOST,
            0.5,
            0.02,
            1.0,
        );
    }

    ReplayStats {
        entries_replayed: replayed_indices.len(),
        edges_reinforced,
    }
}
