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
    wernicke::{extract_entities, KeywordCache},
    BrainState, EDGE_REINFORCE_DELTA, GRAPH_EDGE_CAPACITY, GRAPH_NODE_CAPACITY, GRAPH_PRUNE_WEIGHT,
    GRAPH_WEIGHT_TARGET_MAX, HEBBIAN_EDGE_BOOST, HEBBIAN_EDGE_CEILING, HEBBIAN_NODE_BOOST,
    HEBBIAN_NODE_CEILING, NEOCORTICAL_DECAY_RATE, NODE_WEIGHT_BASE, PRUNE_AGE_WEIGHT,
    REPLAY_EDGE_BOOST, REPLAY_SALIENCE_BOOST, REPLAY_TEMPORAL_WINDOW, SPREADING_ACTIVATION_DECAY,
    SPREADING_ACTIVATION_MAX_HOPS,
};

// ---------------------------------------------------------------------------
// Neocortical types
// ---------------------------------------------------------------------------

/// Long-term knowledge graph: labeled nodes connected by typed edges.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct GraphMemory {
    pub nodes: HashMap<u64, GraphNode>,
    pub edges: Vec<GraphEdge>,
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
        if a <= b { (a, b) } else { (b, a) }
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

/// Soft priors over edge kinds for different retrieval modes.
pub fn edge_kind_multiplier(mode: QueryMode, edge_kind: &str) -> f32 {
    match mode {
        QueryMode::Structural => match edge_kind {
            "contains" | "represents" => 1.0,
            "related" => 0.85,
            "temporal" => 0.55,
            _ => 0.8,
        },
        QueryMode::Temporal => match edge_kind {
            "temporal" => 1.0,
            "related" => 0.8,
            "contains" | "represents" => 0.6,
            _ => 0.75,
        },
        QueryMode::Diagnostic => match edge_kind {
            "related" => 1.0,
            "temporal" => 0.95,
            "contains" | "represents" => 0.75,
            _ => 0.8,
        },
        QueryMode::Semantic => match edge_kind {
            "related" => 1.0,
            _ => 0.85,
        },
        QueryMode::Neutral => 1.0,
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

    let mut results: Vec<(u64, f32)> = activations.into_iter().collect();
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
        if let Some(&node_id) = long_term.index.get(&entity.label) {
            if let Some(node) = long_term.nodes.get(&node_id) {
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

/// Remove low-weight graph nodes and orphaned/excess edges.
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
            long_term.index.remove(&node.label);
        }
    }

    // 2. Hard cap: if still over capacity, evict lowest-weight nodes
    if long_term.nodes.len() > GRAPH_NODE_CAPACITY {
        let mut sorted: Vec<(u64, f32)> = long_term
            .nodes
            .iter()
            .map(|(&id, n)| (id, n.weight))
            .collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let to_remove = long_term.nodes.len() - GRAPH_NODE_CAPACITY;
        for &(id, _) in sorted.iter().take(to_remove) {
            if let Some(node) = long_term.nodes.remove(&id) {
                long_term.index.remove(&node.label);
            }
        }
    }

    // 3. Remove edges referencing deleted nodes
    let node_ids = &long_term.nodes;
    long_term
        .edges
        .retain(|e| node_ids.contains_key(&e.from) && node_ids.contains_key(&e.to));

    // 4. Hard cap on edges: keep highest-weight
    if long_term.edges.len() > GRAPH_EDGE_CAPACITY {
        long_term
            .edges
            .sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
        long_term.edges.truncate(GRAPH_EDGE_CAPACITY);
    }

    // Rebuild edge index after retain/truncate may have invalidated it
    long_term.rebuild_edge_index();
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
pub fn upsert_edge(long_term: &mut GraphMemory, from: u64, to: u64, kind: &str, clock: u64) {
    let key = GraphMemory::edge_key(from, to);
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
        edge.last_seen = clock;
        if edge.kind == "related" && kind != "related" {
            edge.kind = kind.to_string();
        }
    } else {
        let new_idx = long_term.edges.len();
        long_term.edges.push(GraphEdge {
            from,
            to,
            weight: EDGE_REINFORCE_DELTA,
            kind: kind.to_string(),
            last_seen: clock,
            activation_count: 0,
            stability: 1.0,
            recent_interval_avg: 0.0,
            historical_interval_avg: 0.0,
        });
        long_term.edge_index.insert(key, new_idx);
    }
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

/// CPEB-inspired synaptic tagging: recently active edges capture the global
/// valence signal and receive extra stability for long-term retention.
pub fn cpeb_tag_edges(
    long_term: &mut GraphMemory,
    clock: u64,
    valence_magnitude: f32,
    tag_window: u64,
    stability_boost: f32,
) -> u32 {
    let mut tagged_count = 0u32;

    for edge in &mut long_term.edges {
        if clock.saturating_sub(edge.last_seen) <= tag_window {
            edge.stability = (edge.stability + stability_boost * valence_magnitude).min(10.0);
            tagged_count += 1;
        }
    }

    tagged_count
}

/// Apply L3 decay to graph nodes and edges.
pub fn apply_l3_decay(long_term: &mut GraphMemory, clock: u64) {
    for node in long_term.nodes.values_mut() {
        let decay = (-(clock.saturating_sub(node.last_seen) as f32) * NEOCORTICAL_DECAY_RATE).exp();
        node.weight *= decay;
        node.salience *= decay;
    }
    // Edge decay: edges that haven't been reinforced recently lose weight
    for edge in &mut long_term.edges {
        let effective_decay_rate = NEOCORTICAL_DECAY_RATE / edge.stability.max(1.0);
        let decay = (-(clock.saturating_sub(edge.last_seen) as f32) * effective_decay_rate).exp();
        edge.weight *= decay;
    }
}

// ── Wide-param functions (operate on &mut BrainState) ──────────────────

/// Extract entities from text and insert/update nodes and edges in the knowledge graph.
pub fn update_graph(state: &mut BrainState, text: &str, salience: f32) {
    let entities = extract_entities(text, &state.keyword_cache);
    if entities.is_empty() {
        return;
    }

    let mut node_ids = Vec::new();
    let mut edge_contexts = Vec::new();

    for entity in &entities {
        let id = if let Some(&existing) = state.long_term.index.get(&entity.label) {
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
                    source_texts: Vec::new(),
                    embedding: Vec::new(),
                    full_text: None,
                },
            );
            state.long_term.index.insert(entity.label.clone(), id);
            id
        };

        if let Some(node) = state.long_term.nodes.get_mut(&id) {
            // Code-aware weighting: boost high-signal kinds, penalize generic Terms
            let weight_multiplier = match entity.kind.as_str() {
                "FilePath" => 2.0,
                "Function" | "Struct" | "Enum" | "Trait" | "Class" => 1.5,
                "Symbol" | "Type" => 1.2,
                "Term" => 0.5, // Generic terms get less weight
                _ => 1.0,
            };

            node.weight += (NODE_WEIGHT_BASE + salience * 0.3) * weight_multiplier;
            node.last_seen = state.clock;
            node.salience = (node.salience + salience * 0.5 * weight_multiplier).min(1.0);

            // Update kind if it was previously generic or less specific
            if state.keyword_cache.kind_priority(&entity.kind)
                > state.keyword_cache.kind_priority(&node.kind)
            {
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
            upsert_edge(
                &mut state.long_term,
                node_ids[i],
                node_ids[j],
                edge_kind,
                state.clock,
            );
        }
    }

    // Phase E: Hebbian reinforcement for keyword nodes.
    // Scan text for matches against keyword graph nodes and boost their weight.
    let text_lower = text.to_lowercase();
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
                    upsert_edge(
                        &mut state.long_term,
                        kw_id,
                        other_id,
                        "keyword-co-occurs",
                        state.clock,
                    );
                }
            }
        }
    }
}

/// Replay consolidation: reinforce L3 edges between entities that co-occur
/// in temporally proximate L2 entries (offline replay / sleep consolidation).
pub fn replay_consolidation(state: &mut BrainState) {
    let n = state.short_term.len();
    if n < 2 {
        return;
    }

    // Collect (index, entities) for each L2 entry
    let entry_entities: Vec<(usize, Vec<(String, u64)>)> = state
        .short_term
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let entities = extract_entities(&entry.text, &state.keyword_cache);
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

    // Ensure edge index is populated (may be empty if edges were pushed without registration)
    if state.long_term.edge_index.is_empty() && !state.long_term.edges.is_empty() {
        state.long_term.rebuild_edge_index();
    }

    for i in 0..entry_entities.len() {
        for j in (i + 1)..entry_entities.len() {
            let ei = &state.short_term[entry_entities[i].0];
            let ej = &state.short_term[entry_entities[j].0];

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
                    }
                }
            }

            replayed_indices.insert(entry_entities[i].0);
            replayed_indices.insert(entry_entities[j].0);
        }
    }

    // Boost salience of replayed entries
    for &idx in &replayed_indices {
        state.short_term[idx].salience =
            (state.short_term[idx].salience + REPLAY_SALIENCE_BOOST).min(1.0);
    }
}
