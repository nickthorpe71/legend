/// Tool layer — Legend's CLI/MCP bindings around the brain.
///
/// This module is the thin wrapper between the pure cognitive memory system
/// (src/memory/) and the outside world: persistence, session logging, git
/// sync, filesystem scanning, CLI output formatting.
///
/// The brain processes information; the tool feeds it in and reads it out.
pub mod bootstrap;
pub mod persistence;
pub mod types;
pub mod wal;

// Re-export all tool types at the tool:: level for convenience.
#[allow(unused_imports)]
pub use persistence::{
    load_memory_from_path, load_or_default, reset_memory, save, save_memory_to_path,
};
#[allow(unused_imports)]
pub use types::*;

use crate::memory::{
    add_node_if_new, anterior_pfc, classify_text, retrieve_context_with_mode, GraphNode,
    MemoryState, RetrievalMode, ShortTermEntry,
};
use std::collections::{HashMap, HashSet};
use std::fs;

/// Maximum number of session log entries to keep.
const SESSION_LOG_CAPACITY: usize = 100;

/// Round a float to 3 decimal places for display.
pub(crate) fn round3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

// ---------------------------------------------------------------------------
// Git integration
// ---------------------------------------------------------------------------

/// Summarize Git changes since last sync.
/// Returns a list of commit messages and a summary of uncommitted changes.
pub fn get_git_summary(state: &mut MemoryState) -> GitSyncInfo {
    use std::process::Command;

    let current_sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let mut commits = Vec::new();
    if let (Some(last), Some(current)) = (&state.last_synced_sha, &current_sha) {
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
        last_sha: state.last_synced_sha.clone(),
        current_sha: current_sha.clone(),
        new_commits: commits,
        uncommitted_summary: uncommitted,
    };

    // Update anchor for next time
    state.last_synced_sha = current_sha;

    info
}

// ---------------------------------------------------------------------------
// Tick — session-level wrappers around brain tick_impl
// ---------------------------------------------------------------------------

/// Ingest text: chunk -> embed -> reconsolidate or match/merge/insert -> update graph.
/// Returns a TickResult describing what action was taken.
pub fn tick(state: &mut MemoryState, text: &str) -> TickResult {
    tick_with_options(state, text, crate::memory::TickOptions::default())
}

/// Same as [`tick`] with explicit [`TickOptions`]. Daemon + MCP paths that
/// do not consume `TickResult.context` pass `compute_context: false` to skip
/// the costly activation pass. See `docs/latency-budgets.md`.
pub fn tick_with_options(
    state: &mut MemoryState,
    text: &str,
    options: crate::memory::TickOptions,
) -> TickResult {
    let result = crate::memory::tick_impl_with_options(&mut state.brain, text, options);
    // Session log lives in tool layer — append after brain processing.
    // PLAN ticks bypass L1/L2/L3 encoding; log only a compact summary so plan
    // bodies don't rotate real decisions out of recent session activity.
    let log_text =
        crate::memory::anterior_pfc::summarize_plan_tick(text).unwrap_or_else(|| text.to_string());
    state.session_log.push(SessionEntry {
        timestamp: state.brain.clock,
        text: log_text,
    });
    while state.session_log.len() > SESSION_LOG_CAPACITY {
        state.session_log.remove(0);
    }
    result
}

// ---------------------------------------------------------------------------
// Session start summaries
// ---------------------------------------------------------------------------

/// Build a comprehensive session-start summary: context + categorized memories.
/// Designed as a single cold-start call that gives the LLM everything it needs.
#[allow(dead_code)] // Public API — called by MCP consumers, not the binary itself
pub fn build_start_summary(state: &mut MemoryState) -> serde_json::Value {
    build_start_summary_with_options(state, false, None, None)
}

/// Build session-start summary with options for compact output and category filtering.
/// - compact: If true, only show short text summaries (no id, reduced text length)
/// - category_filter: If Some, only return that specific category
///
/// Output is simplified for LLM usability: no stats, no graph weights.
/// Use `memory dump` for full internal state.
pub fn build_start_summary_with_options(
    state: &mut MemoryState,
    compact: bool,
    category_filter: Option<&str>,
    query: Option<&str>,
) -> serde_json::Value {
    // If a query is provided, perform an internal retrieval to "prime" the graph and surface relevant context.
    // This automatically boosts the salience of related short-term entries and surfaces related graph nodes.
    let mut query_context = None;
    if let Some(q) = query {
        query_context = Some(retrieve_context_with_mode(
            &mut state.brain,
            q,
            RetrievalMode::ReadOnly,
        ));
    }

    let git_sync = get_git_summary(state);

    let recent_sessions: Vec<&str> = state
        .session_log
        .iter()
        .rev()
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

    for entry in &state.brain.short_term {
        let category = classify_text(&entry.text, &state.brain.keyword_cache);

        // Build item based on compact mode
        let item = if compact {
            serde_json::json!(&entry.text)
        } else {
            serde_json::json!({
                "id": entry.id,
                "text": &entry.text,
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
                        .find(|e| e.text.starts_with(text.trim_end_matches('\u{2026}')))
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

    sort_logic(&mut decisions, &state.brain.short_term, &query_context);
    sort_logic(&mut architecture, &state.brain.short_term, &query_context);
    sort_logic(&mut todos, &state.brain.short_term, &query_context);
    sort_logic(&mut bugs, &state.brain.short_term, &query_context);
    sort_logic(&mut preferences, &state.brain.short_term, &query_context);

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
            "current_task": state.current_task,
            "category": filter,
            "items": filtered,
            "total": total,
        });
    }

    // Include neurochemistry if any chemical is significantly elevated
    let chem = &state.brain.chemistry;
    let mut elevated: Vec<(&str, f32)> = Vec::new();
    if chem.norepinephrine > 0.3 {
        elevated.push(("norepinephrine", chem.norepinephrine));
    }
    if chem.cortisol > 0.3 {
        elevated.push(("cortisol", chem.cortisol));
    }
    if chem.dopamine > 0.3 {
        elevated.push(("dopamine", chem.dopamine));
    }
    if (chem.serotonin - 0.5).abs() > 0.2 {
        elevated.push(("serotonin", chem.serotonin));
    }
    if chem.acetylcholine > 0.3 {
        elevated.push(("acetylcholine", chem.acetylcholine));
    }
    if chem.endocannabinoid > 0.3 {
        elevated.push(("endocannabinoid", chem.endocannabinoid));
    }

    let chemistry = if elevated.is_empty() {
        None
    } else {
        let map: serde_json::Map<String, serde_json::Value> = elevated
            .into_iter()
            .map(|(k, v)| (k.to_string(), serde_json::json!(round3(v))))
            .collect();
        Some(serde_json::Value::Object(map))
    };

    // Plans from anterior PFC
    let plans_json = crate::memory::anterior_pfc::format_plans_for_summary(&state.brain.plans);

    // Auto-sync current_task from plan if not manually set
    if state.current_task.is_none() {
        if let Some((plan_name, item_text)) =
            crate::memory::anterior_pfc::find_next_plan_action(&state.brain.plans)
        {
            state.current_task = Some(format!("[{}] {}", plan_name, item_text));
        }
    }

    serde_json::json!({
        "current_task": state.current_task,
        "plans": plans_json,
        "git_sync": git_sync,
        "recent_sessions": recent_sessions,
        "chemistry": chemistry,
        "categorized": {
            "decisions": build_category(&decisions, decisions_total),
            "architecture": build_category(&architecture, architecture_total),
            "todos": build_category(&todos, todos_total),
            "bugs": build_category(&bugs, bugs_total),
            "preferences": build_category(&preferences, preferences_total),
        }
    })
}

// ---------------------------------------------------------------------------
// Session & task management
// ---------------------------------------------------------------------------

/// Return the most recent `n` session log entries.
pub fn recent_sessions(state: &MemoryState, n: usize) -> &[SessionEntry] {
    let start = state.session_log.len().saturating_sub(n);
    &state.session_log[start..]
}

/// Set the current task description.
pub fn set_task(state: &mut MemoryState, task: &str) {
    state.current_task = Some(task.to_string());

    // Link task to knowledge graph via brain layer
    let label = task.trim();
    if !label.is_empty() {
        crate::memory::upsert_task_node(&mut state.brain, label);
    }
}

/// Deep structural merge: union two MemoryStates field-by-field.
///
/// Preserves all existing entries, embeddings, and metadata intact.
/// Designed for cross-machine git sync where both sides have valid state.
pub fn merge_states(ours: &mut MemoryState, theirs: &MemoryState) -> MergeStats {
    let mut stats = MergeStats::default();

    // --- L2 short_term: union by ID, on collision keep higher salience ---
    let mut our_id_map: HashMap<u64, usize> = ours
        .brain
        .short_term
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id, i))
        .collect();
    for their_entry in &theirs.brain.short_term {
        if let Some(&idx) = our_id_map.get(&their_entry.id) {
            // Collision: keep higher salience
            if their_entry.salience > ours.brain.short_term[idx].salience {
                ours.brain.short_term[idx] = their_entry.clone();
                stats.l2_updated += 1;
            }
        } else {
            our_id_map.insert(their_entry.id, ours.brain.short_term.len());
            ours.brain.short_term.push(their_entry.clone());
            stats.l2_added += 1;
        }
    }

    // --- L1 working_memory: union by ID ---
    let our_wm_ids: HashSet<u64> = ours.brain.working_memory.iter().map(|e| e.id).collect();
    for their_entry in &theirs.brain.working_memory {
        if !our_wm_ids.contains(&their_entry.id) {
            ours.brain.working_memory.push(their_entry.clone());
            stats.l1_added += 1;
        }
    }

    // --- L3 nodes: union by ID, on collision take higher weight + merge source_texts ---
    for (id, their_node) in &theirs.brain.long_term.nodes {
        if let Some(our_node) = ours.brain.long_term.nodes.get_mut(id) {
            // Collision: take higher weight, merge source_texts
            let mut changed = false;
            if their_node.weight > our_node.weight {
                our_node.weight = their_node.weight;
                our_node.salience = their_node.salience;
                our_node.last_seen = our_node.last_seen.max(their_node.last_seen);
                changed = true;
            }
            // Merge source_texts
            let existing: HashSet<String> = our_node.source_texts.iter().cloned().collect();
            for text in &their_node.source_texts {
                if !existing.contains(text) {
                    our_node.source_texts.push(text.clone());
                    changed = true;
                }
            }
            // Prefer non-empty embedding and full_text
            if our_node.embedding.is_empty() && !their_node.embedding.is_empty() {
                our_node.embedding = their_node.embedding.clone();
                changed = true;
            }
            if our_node.full_text.is_none() && their_node.full_text.is_some() {
                our_node.full_text = their_node.full_text.clone();
                changed = true;
            }
            if changed {
                stats.l3_nodes_updated += 1;
            }
        } else {
            ours.brain.long_term.nodes.insert(*id, their_node.clone());
            stats.l3_nodes_added += 1;
        }
    }

    // Rebuild label→id index after node merge
    ours.brain.long_term.index.clear();
    for (id, node) in &ours.brain.long_term.nodes {
        ours.brain.long_term.index.insert(node.label.clone(), *id);
    }

    // --- L3 edges: union by (from, to, kind), on collision take higher weight ---
    {
        let mut edge_map: HashMap<(u64, u64, String), usize> = HashMap::new();
        for (i, edge) in ours.brain.long_term.edges.iter().enumerate() {
            let key = (
                edge.from.min(edge.to),
                edge.from.max(edge.to),
                edge.kind.clone(),
            );
            edge_map.insert(key, i);
        }
        for their_edge in &theirs.brain.long_term.edges {
            let key = (
                their_edge.from.min(their_edge.to),
                their_edge.from.max(their_edge.to),
                their_edge.kind.clone(),
            );
            if let Some(&idx) = edge_map.get(&key) {
                if their_edge.weight > ours.brain.long_term.edges[idx].weight {
                    ours.brain.long_term.edges[idx] = their_edge.clone();
                }
            } else {
                edge_map.insert(key, ours.brain.long_term.edges.len());
                ours.brain.long_term.edges.push(their_edge.clone());
                stats.l3_edges_added += 1;
            }
        }
    }
    ours.brain.long_term.rebuild_edge_index();
    for (key, their_stamp) in &theirs.brain.long_term.edge_chemical_stamps {
        ours.brain
            .long_term
            .edge_chemical_stamps
            .entry(key.clone())
            .or_insert_with(|| their_stamp.clone());
    }

    // --- Plans: union by name (case-insensitive), items unioned by text ---
    // Status priority: Done > Active > Deferred > Pending
    {
        let our_plan_indices: HashMap<String, usize> = ours
            .brain
            .plans
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name.to_lowercase(), i))
            .collect();

        for their_plan in &theirs.brain.plans {
            let key = their_plan.name.to_lowercase();
            if let Some(&idx) = our_plan_indices.get(&key) {
                let our_plan = &mut ours.brain.plans[idx];
                // Union items by text (case-insensitive)
                for their_item in &their_plan.items {
                    let item_key = their_item.text.to_lowercase();
                    if let Some(our_item) = our_plan
                        .items
                        .iter_mut()
                        .find(|i| i.text.to_lowercase() == item_key)
                    {
                        // Status: take most advanced
                        if item_status_rank(&their_item.status) > item_status_rank(&our_item.status)
                        {
                            our_item.status = their_item.status.clone();
                            stats.plan_items_updated += 1;
                        }
                    } else {
                        our_plan.items.push(their_item.clone());
                        stats.plan_items_added += 1;
                    }
                }
                // Take later updated_at
                our_plan.updated_at = our_plan.updated_at.max(their_plan.updated_at);
                our_plan.update_completion_status(ours.brain.clock.max(theirs.brain.clock));
            } else {
                ours.brain.plans.push(their_plan.clone());
                stats.plans_added += 1;
            }
        }
    }

    // --- session_log: union by text, sort by timestamp ---
    {
        let our_texts: HashSet<String> = ours.session_log.iter().map(|s| s.text.clone()).collect();
        for their_entry in &theirs.session_log {
            if !our_texts.contains(&their_entry.text) {
                ours.session_log.push(their_entry.clone());
                stats.session_entries_added += 1;
            }
        }
        ours.session_log.sort_by_key(|e| e.timestamp);
    }

    // --- term_frequency: union keys, on collision take higher count ---
    for (term, their_stats) in &theirs.brain.term_frequency {
        if let Some(our_stats) = ours.brain.term_frequency.get_mut(term) {
            if their_stats.total_count > our_stats.total_count {
                *our_stats = their_stats.clone();
            }
        } else {
            ours.brain
                .term_frequency
                .insert(term.clone(), their_stats.clone());
        }
    }

    // --- chemistry: take from state with higher clock ---
    if theirs.brain.clock > ours.brain.clock {
        ours.brain.chemistry = theirs.brain.chemistry.clone();
    }

    // --- temporal_context: take from state with higher clock ---
    if theirs.brain.clock > ours.brain.clock && !theirs.brain.temporal_context.is_empty() {
        ours.brain.temporal_context = theirs.brain.temporal_context.clone();
    }

    // --- current_task: prefer non-None, then higher-clock state ---
    if ours.current_task.is_none() {
        ours.current_task = theirs.current_task.clone();
    } else if theirs.current_task.is_some() && theirs.brain.clock > ours.brain.clock {
        ours.current_task = theirs.current_task.clone();
    }

    // --- clock / next_id: max(a, b) ---
    // Updated after clock-gated comparisons above so they compare against the pre-merge clock.
    ours.brain.clock = ours.brain.clock.max(theirs.brain.clock);
    ours.brain.next_id = ours.brain.next_id.max(theirs.brain.next_id);

    // --- ticks_since_consolidation: max ---
    ours.brain.ticks_since_consolidation = ours
        .brain
        .ticks_since_consolidation
        .max(theirs.brain.ticks_since_consolidation);

    stats
}

/// Rank an ItemStatus for merge conflict resolution (higher = more advanced).
fn item_status_rank(status: &anterior_pfc::ItemStatus) -> u8 {
    match status {
        anterior_pfc::ItemStatus::Pending => 0,
        anterior_pfc::ItemStatus::Deferred => 1,
        anterior_pfc::ItemStatus::Active => 2,
        anterior_pfc::ItemStatus::Done => 3,
    }
}

/// Statistics from a merge_states() call.
#[derive(Debug, Default)]
pub struct MergeStats {
    pub l1_added: usize,
    pub l2_added: usize,
    pub l2_updated: usize,
    pub l3_nodes_added: usize,
    pub l3_nodes_updated: usize,
    pub l3_edges_added: usize,
    pub plans_added: usize,
    pub plan_items_added: usize,
    pub plan_items_updated: usize,
    pub session_entries_added: usize,
}

impl MergeStats {
    pub fn total(&self) -> usize {
        self.l1_added
            + self.l2_added
            + self.l2_updated
            + self.l3_nodes_added
            + self.l3_nodes_updated
            + self.l3_edges_added
            + self.plans_added
            + self.plan_items_added
            + self.plan_items_updated
            + self.session_entries_added
    }
}

/// Clear the current task.
pub fn clear_task(state: &mut MemoryState) {
    state.current_task = None;
}

/// Get the current task description.
pub fn get_task(state: &MemoryState) -> Option<&str> {
    state.current_task.as_deref()
}

/// Check if consolidation should be suggested.
#[allow(dead_code)]
pub fn should_suggest_consolidation(state: &MemoryState) -> bool {
    let effective = crate::memory::neurochemistry::compute_effective(&state.brain.chemistry);
    state.brain.ticks_since_consolidation >= crate::memory::CONSOLIDATION_SUGGESTION_THRESHOLD
        || effective.consolidation_pressure >= crate::memory::CONSOLIDATION_PRESSURE_THRESHOLD
}

// ---------------------------------------------------------------------------
// Context & dump summaries
// ---------------------------------------------------------------------------

/// Build a structured cold-start context summary as JSON.
pub fn build_context_summary(state: &MemoryState) -> serde_json::Value {
    let recent = recent_sessions(state, 5);
    let session_texts: Vec<&str> = recent.iter().map(|s| s.text.as_str()).collect();

    let mut top_nodes: Vec<&GraphNode> = state.brain.long_term.nodes.values().collect();
    top_nodes.sort_by(|a, b| {
        b.weight
            .partial_cmp(&a.weight)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
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
        "current_task": state.current_task,
        "stats": {
            "working_memory": state.brain.working_memory.len(),
            "short_term_entries": state.brain.short_term.len(),
            "long_term_nodes": state.brain.long_term.nodes.len(),
            "long_term_edges": state.brain.long_term.edges.len(),
            "session_log_entries": state.session_log.len(),
            "clock": state.brain.clock,
        },
        "recent_sessions": session_texts,
        "top_graph_nodes": node_summaries,
    })
}

/// Export the full memory state as JSON for external tools (e.g. dashboard).
pub fn build_dump(state: &MemoryState) -> serde_json::Value {
    let nodes: Vec<serde_json::Value> = state
        .brain
        .long_term
        .nodes
        .values()
        .map(|n| {
            serde_json::json!({
                "id": n.id, "label": n.label, "kind": n.kind,
                "weight": round3(n.weight),
                "salience": round3(n.salience),
                "last_seen": n.last_seen,
            })
        })
        .collect();

    let edges: Vec<serde_json::Value> = state
        .brain
        .long_term
        .edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "from": e.from, "to": e.to,
                "weight": round3(e.weight),
                "kind": e.kind,
                "stability": round3(e.stability),
                "cpeb_boost": round3(e.cpeb_boost),
            })
        })
        .collect();

    let short_term: Vec<serde_json::Value> = state
        .brain
        .short_term
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id, "text": e.text, "summary": e.summary,
                "salience": round3(e.salience),
                "emotional_valence": round3(e.emotional_valence),
                "usage": e.usage, "last_access": e.last_access,
                "reconsolidation_count": e.reconsolidation_count,
                "labile": e.labile_until >= state.brain.clock,
                "refs": e.refs,
            })
        })
        .collect();

    let working_memory: Vec<serde_json::Value> = state
        .brain
        .working_memory
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id, "text": e.text,
                "salience": round3(e.salience),
                "emotional_valence": round3(e.emotional_valence),
                "tick_created": e.tick_created,
                "rehearsal_count": e.rehearsal_count,
                "promoted": e.promoted,
            })
        })
        .collect();
    let sessions: Vec<serde_json::Value> = state
        .session_log
        .iter()
        .map(|s| serde_json::json!({"timestamp": s.timestamp, "text": s.text}))
        .collect();

    let chemistry = serde_json::json!({
        "norepinephrine": round3(state.brain.chemistry.norepinephrine),
        "cortisol": round3(state.brain.chemistry.cortisol),
        "dopamine": round3(state.brain.chemistry.dopamine),
        "serotonin": round3(state.brain.chemistry.serotonin),
        "acetylcholine": round3(state.brain.chemistry.acetylcholine),
        "endocannabinoid": round3(state.brain.chemistry.endocannabinoid),
    });

    serde_json::json!({
        "clock": state.brain.clock,
        "chemistry": chemistry,
        "working_memory": working_memory,
        "short_term": short_term,
        "graph": { "nodes": nodes, "edges": edges },
        "session_log": sessions,
    })
}

// ---------------------------------------------------------------------------
// Ecosystem scanning
// ---------------------------------------------------------------------------

/// Scan project manifest files for dependencies and add them to the graph.
pub fn scan_ecosystem_dependencies(state: &mut MemoryState) {
    // Rust
    if let Ok(cargo) = fs::read_to_string("Cargo.toml") {
        for line in cargo.lines() {
            if let Some(pos) = line.find(" = ") {
                let name = line[..pos].trim();
                if !name.is_empty() && !name.starts_with('[') && !name.starts_with('#') {
                    add_node_if_new(&mut state.brain, name, "Dependency", 0.3);
                }
            }
        }
    }
    // Node.js
    if let Ok(pkg) = fs::read_to_string("package.json") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&pkg) {
            if let Some(deps) = json.get("dependencies").and_then(|v| v.as_object()) {
                for name in deps.keys() {
                    add_node_if_new(&mut state.brain, name, "Dependency", 0.3);
                }
            }
            if let Some(dev_deps) = json.get("devDependencies").and_then(|v| v.as_object()) {
                for name in dev_deps.keys() {
                    add_node_if_new(&mut state.brain, name, "Dependency", 0.3);
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
                add_node_if_new(&mut state.brain, &name, "Dependency", 0.3);
            }
        }
    }
}
