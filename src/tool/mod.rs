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

// Re-export all tool types at the tool:: level for convenience.
#[allow(unused_imports)]
pub use persistence::{
    load_memory_from_path, load_or_default, reset_memory, save, save_memory_to_path,
};
#[allow(unused_imports)]
pub use types::*;

use crate::memory::{
    add_node_if_new, classify_text, retrieve_context, safe_truncate, tick_impl,
    GraphNode, MemoryState, ShortTermEntry,
};
use std::collections::HashSet;
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
    let result = tick_impl(&mut state.brain, text, false);
    // Session log lives in tool layer — append after brain processing
    state.session_log.push(SessionEntry {
        timestamp: state.brain.clock,
        text: text.to_string(),
    });
    while state.session_log.len() > SESSION_LOG_CAPACITY {
        state.session_log.remove(0);
    }
    result
}

/// Like tick(), but does not count toward consolidation and does not appear in
/// the session log. Used for automated/hook-generated ticks that should influence
/// L2/L3 (dedup, reconsolidation) but must not pollute session history or
/// trigger premature auto-consolidation.
pub fn tick_passive(state: &mut MemoryState, text: &str) -> TickResult {
    tick_impl(&mut state.brain, text, true)
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
        query_context = Some(retrieve_context(&mut state.brain, q));
    }

    let git_sync = get_git_summary(state);

    // Get recent sessions — skip passive (EXPERIENCE:) entries so Recent Activity
    // only shows meaningful user-initiated ticks, not tool telemetry noise.
    let recent_sessions: Vec<&str> = state
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

    for entry in &state.brain.short_term {
        let category = classify_text(&entry.text, &state.brain.keyword_cache);

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
            "preferences" | "preference" | "prefs" | "pref" => {
                (&preferences, preferences_total)
            }
            _ => return serde_json::json!({"error": format!("Unknown category: {}", filter)}),
        };
        return serde_json::json!({
            "current_task": state.current_task,
            "category": filter,
            "items": filtered,
            "total": total,
        });
    }

    serde_json::json!({
        "current_task": state.current_task,
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

/// Merge knowledge from another MemoryState by replaying its unique session log entries.
pub fn merge_from_log(state: &mut MemoryState, other: MemoryState) {
    let our_texts: HashSet<&str> = state.session_log.iter().map(|s| s.text.as_str()).collect();

    let mut to_replay = Vec::new();
    for other_entry in other.session_log {
        if !our_texts.contains(other_entry.text.as_str()) {
            to_replay.push(other_entry);
        }
    }

    to_replay.sort_by_key(|e| e.timestamp);

    if !to_replay.is_empty() {
        eprintln!("[LEGEND] Smart-merging {} new session memories...", to_replay.len());
        for entry in to_replay {
            tick_passive(state, &entry.text);
        }
    }

    if state.current_task.is_none() {
        state.current_task = other.current_task;
    }

    state.brain.clock = state.brain.clock.max(other.brain.clock);
    state.brain.next_id = state.brain.next_id.max(other.brain.next_id);
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
pub fn should_suggest_consolidation(state: &MemoryState) -> bool {
    state.brain.ticks_since_consolidation >= crate::memory::CONSOLIDATION_SUGGESTION_THRESHOLD
        || state.brain.recent_valence_sum >= crate::memory::EMOTIONAL_CONSOLIDATION_THRESHOLD
}

// ---------------------------------------------------------------------------
// Context & dump summaries
// ---------------------------------------------------------------------------

/// Build a structured cold-start context summary as JSON.
pub fn build_context_summary(state: &MemoryState) -> serde_json::Value {
    let recent = recent_sessions(state, 5);
    let session_texts: Vec<&str> = recent.iter().map(|s| s.text.as_str()).collect();

    let mut top_nodes: Vec<&GraphNode> = state.brain.long_term.nodes.values().collect();
    top_nodes.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal));
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
        .brain.long_term
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
        .brain.long_term
        .edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "from": e.from, "to": e.to,
                "weight": round3(e.weight),
                "kind": e.kind,
            })
        })
        .collect();

    let short_term: Vec<serde_json::Value> = state
        .brain.short_term
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
        .brain.working_memory
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

    serde_json::json!({
        "clock": state.brain.clock,
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
