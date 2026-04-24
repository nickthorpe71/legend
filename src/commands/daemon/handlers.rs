//! Command render functions — the shared logic between the daemon's server
//! dispatch and the in-process CLI fallback path.
//!
//! Each `render_*` function takes an already-loaded `&mut MemoryState` (or
//! `&MemoryState` for read-only), performs the command's work, and returns
//! the exact `String` that should be written to stdout. Callers decide how
//! to get at the state (daemon holds it under a lock; the CLI fallback
//! `load_or_default()`s it first) and how to persist it after (daemon calls
//! `persist`; the CLI fallback calls `memory::save`).
//!
//! Keeping the render logic here and out of each command file's old
//! `handle_*` makes the split clean: each file's `handle_*` becomes a thin
//! orchestrator that tries IPC and falls back to load → render → save.

use crate::commands::memory::{
    extract_keyword_directives, log_event_rich, truncate_text, ConsolidateEventData,
    ConsolidatedGroup, EventData, GraphHit, MatchedEntry, QueryEventData, ReinforceEntry,
    ReinforceEventData, TickEventData,
};
use crate::memory::{MemoryContext, MemoryState, ReinforceResult, TickResult};

// ---------------------------------------------------------------------------
// Tick — mutating
// ---------------------------------------------------------------------------

/// Outcome of a `tick` mutation when the daemon returns structured data to a
/// non-CLI consumer (currently: `mcp-serve`).
///
/// `KeywordOnly` is distinct from `Applied` because a tick whose text collapses
/// to just `KEYWORD:cat:term` directives never enters L1/L2 — it only
/// registers graph keyword nodes. MCP formats these two cases differently.
#[derive(Debug, Clone)]
pub enum TickApplied {
    Applied(TickResult),
    KeywordOnly { keywords_registered: usize },
}

/// Core tick mutation used by both the CLI render path and the MCP structured
/// path. Parses keyword directives, applies them, then runs `memory::tick` on
/// the residual text. Logs the rich tick event exactly once.
///
/// Returns `TickApplied::KeywordOnly` when the residual text is empty after
/// stripping keyword directives — in that case no TickResult is available.
pub fn apply_tick(
    state: &mut MemoryState,
    text: &str,
    _blocker: bool,
) -> Result<TickApplied, String> {
    if text.trim().is_empty() {
        return Err("No input provided for tick".into());
    }

    let (clean_text, keyword_directives) = extract_keyword_directives(text);
    let text = if keyword_directives.is_empty() {
        text.trim().to_string()
    } else {
        clean_text.trim().to_string()
    };

    let mut keywords_added = false;
    for directive in &keyword_directives {
        let created = crate::memory::add_keyword_node(
            &mut state.brain,
            &directive.category,
            &directive.term,
            directive.metadata.clone(),
        );
        if created {
            keywords_added = true;
            eprintln!(
                "[keyword registered: kw:{}:{}]",
                directive.category, directive.term
            );
        }
    }
    if keywords_added {
        crate::memory::rebuild_keyword_cache(&mut state.brain);
    }

    // Keyword-only input — short-circuit before the expensive tick path.
    if text.is_empty() && !keyword_directives.is_empty() {
        for directive in &keyword_directives {
            log_event_rich(
                "keyword_register",
                &format!("kw:{}:{}", directive.category, directive.term),
                None,
            );
        }
        return Ok(TickApplied::KeywordOnly {
            keywords_registered: keyword_directives.len(),
        });
    }

    let tick_result = crate::memory::tick(state, &text);

    let event_data = EventData::Tick(TickEventData {
        entry_id: Some(tick_result.entry_id),
        matches: tick_result
            .context
            .short_term
            .iter()
            .take(5)
            .map(|m| MatchedEntry {
                id: m.id,
                similarity: m.similarity,
                text_preview: truncate_text(&m.text, 80),
            })
            .collect(),
        graph_nodes: tick_result
            .context
            .long_term
            .iter()
            .take(5)
            .map(|n| GraphHit {
                id: n.id,
                label: n.label.clone(),
                kind: n.kind.clone(),
                weight: n.weight,
            })
            .collect(),
    });
    log_event_rich("tick", text.trim(), Some(event_data));

    Ok(TickApplied::Applied(tick_result))
}

/// Render a `legend memory tick` — wraps `apply_tick` and formats the CLI JSON
/// stdout (`{"action":..,"entry_id":..}` or `{"action":"keyword_only",...}`).
pub fn render_tick(
    state: &mut MemoryState,
    text: &str,
    blocker: bool,
) -> Result<String, String> {
    match apply_tick(state, text, blocker)? {
        TickApplied::KeywordOnly {
            keywords_registered,
        } => Ok(format!(
            "{{\"action\":\"keyword_only\",\"keywords_registered\":{}}}\n",
            keywords_registered
        )),
        TickApplied::Applied(tick_result) => {
            let output = serde_json::json!({
                "action": tick_result.action,
                "entry_id": tick_result.entry_id,
            });
            let json = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
            Ok(format!("{}\n", json))
        }
    }
}

// ---------------------------------------------------------------------------
// Task — mutating (set/clear) + read-only (get)
// ---------------------------------------------------------------------------

pub fn render_task_get(state: &MemoryState) -> Result<String, String> {
    Ok(match crate::memory::get_task(state) {
        Some(task) => format!("Current task: {}\n", task),
        None => "No current task set\n".to_string(),
    })
}

pub fn render_task_set(state: &mut MemoryState, task: &str) -> Result<String, String> {
    crate::memory::set_task(state, task);
    log_event_rich("task_set", task, None);
    Ok(format!("✓ Current task set: {}\n", task))
}

pub fn render_task_clear(state: &mut MemoryState) -> Result<String, String> {
    crate::memory::clear_task(state);
    log_event_rich("task_clear", "task cleared", None);
    Ok("✓ Current task cleared\n".to_string())
}

// ---------------------------------------------------------------------------
// Reinforce — mutating
// ---------------------------------------------------------------------------

pub fn render_reinforce(
    state: &mut MemoryState,
    signal: f32,
    ids: &[u64],
) -> Result<String, String> {
    let result = crate::memory::basal_ganglia::reinforce(&mut state.brain, ids, signal);

    let event_data = EventData::Reinforce(ReinforceEventData {
        signal,
        entries: result
            .reinforced
            .iter()
            .map(|r| ReinforceEntry {
                id: r.id,
                before: r.salience_before,
                after: r.salience_after,
            })
            .collect(),
        graph_nodes_affected: result.graph_nodes_affected,
    });
    log_event_rich(
        "reinforce",
        &format!("signal={} ids={:?}", signal, ids),
        Some(event_data),
    );

    let json = serde_json::to_string::<ReinforceResult>(&result).unwrap_or_else(|_| "{}".to_string());
    Ok(format!("{}\n", json))
}

// ---------------------------------------------------------------------------
// Consolidate — mutating (wholesale graph rewrite)
// ---------------------------------------------------------------------------

pub fn render_consolidate(state: &mut MemoryState) -> Result<String, String> {
    let summaries = crate::memory::consolidate(&mut state.brain);

    let event_data = EventData::Consolidate(ConsolidateEventData {
        groups_merged: summaries.len(),
        summaries: summaries
            .iter()
            .map(|s| ConsolidatedGroup {
                node_id: s.id,
                label: truncate_text(&s.label, 60),
            })
            .collect(),
    });
    log_event_rich(
        "consolidate",
        &format!("{} groups merged", summaries.len()),
        Some(event_data),
    );

    let json = serde_json::to_string(&summaries).unwrap_or_else(|_| "[]".to_string());
    Ok(format!("{}\n", json))
}

// ---------------------------------------------------------------------------
// Reset — destructive: wipes state on disk AND in memory
// ---------------------------------------------------------------------------

pub fn render_reset(state: &mut MemoryState) -> Result<String, String> {
    crate::memory::reset_memory().map_err(|e| e.to_string())?;
    // Replace the daemon's in-RAM state with a fresh default so future
    // commands don't see zombie pre-reset data.
    *state = MemoryState::default();
    log_event_rich("reset", "memory store cleared", None);
    Ok("✓ Memory reset\n".to_string())
}

// ---------------------------------------------------------------------------
// Context / Dump / Stats / Sessions — read-only
// ---------------------------------------------------------------------------

pub fn render_context(state: &MemoryState) -> Result<String, String> {
    let summary = crate::memory::build_context_summary(state);
    let json = serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_string());
    Ok(format!("{}\n", json))
}

pub fn render_dump(state: &MemoryState) -> Result<String, String> {
    let dump = crate::memory::build_dump(state);
    let json = serde_json::to_string(&dump).unwrap_or_else(|_| "{}".to_string());
    Ok(format!("{}\n", json))
}

pub fn render_stats(state: &MemoryState) -> Result<String, String> {
    let mut out = String::new();
    out.push_str("Memory stats:\n");
    out.push_str(&format!(
        "  Working memory (L1): {}\n",
        state.brain.working_memory.len()
    ));
    out.push_str(&format!(
        "  Short-term entries: {}\n",
        state.brain.short_term.len()
    ));
    out.push_str(&format!(
        "  Long-term nodes: {}\n",
        state.brain.long_term.nodes.len()
    ));
    out.push_str(&format!(
        "  Long-term edges: {}\n",
        state.brain.long_term.edges.len()
    ));
    out.push_str(&format!(
        "  Ticks since consolidation: {}\n",
        state.brain.ticks_since_consolidation
    ));
    if let Some(task) = crate::memory::get_task(state) {
        out.push_str(&format!("  Current task: {}\n", task));
    }
    // The session-quality panel reads events.jsonl from disk and stays on the
    // CLI side (see `src/commands/memory/stats.rs`); daemon clients still get
    // the base stats with byte-identical ordering.
    Ok(out)
}

pub fn render_sessions(
    state: &MemoryState,
    count: usize,
    show_all: bool,
) -> Result<String, String> {
    let recent = crate::memory::recent_sessions(state, count);

    if recent.is_empty() {
        return Ok("No session log entries yet.\n".to_string());
    }
    let mut out = String::new();
    for entry in recent {
        if !show_all && entry.text.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("[t={}] {}\n", entry.timestamp, entry.text));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Start — wraps build_start_summary_with_options + flush_working_memory
// ---------------------------------------------------------------------------

/// Options mirrored from `src/commands/memory/start.rs::StartOptions`. Kept
/// local so callers don't have to import the CLI type.
pub struct StartArgs<'a> {
    pub compact: bool,
    pub json: bool,
    pub category: Option<&'a str>,
    pub query: Option<&'a str>,
}

/// Render a `legend memory start`. Mutates state (flushes L1→L2, injects a
/// session-log warning + update-available notice, logs the rich event) and
/// returns the exact stdout string the CLI prints — markdown or JSON
/// depending on `args.json`.
///
/// Version-cache read is local disk I/O keyed off `.legend/.latest_version`,
/// which is co-located with state so the daemon sees the same file the CLI
/// would. `--tokens` (the stderr overhead printout) stays CLI-side.
pub fn render_start(state: &mut MemoryState, args: StartArgs) -> Result<String, String> {
    const SESSION_LOG_WARNING_THRESHOLD: usize = 90;

    let mut summary = crate::memory::build_start_summary_with_options(
        state,
        args.compact,
        args.category,
        args.query,
    );

    if state.session_log.len() >= SESSION_LOG_WARNING_THRESHOLD {
        if let Some(obj) = summary.as_object_mut() {
            obj.insert(
                "warning".to_string(),
                serde_json::json!(format!(
                    "Session log at {}% capacity ({}/100). Oldest entries will be dropped.",
                    state.session_log.len(),
                    state.session_log.len()
                )),
            );
        }
    }

    if let Some(latest) = read_cached_update_version() {
        if let Some(obj) = summary.as_object_mut() {
            obj.insert(
                "update_available".to_string(),
                serde_json::json!(format!(
                    "v{} → v{}",
                    env!("CARGO_PKG_VERSION"),
                    latest
                )),
            );
        }
    }

    // L1 → L2 flush (the mutating side-effect of `memory start`).
    crate::memory::prefrontal::flush_working_memory(&mut state.brain);

    let event_data = EventData::Start(crate::commands::memory::StartEventData {
        clock: state.brain.clock,
        short_term_count: state.brain.short_term.len(),
        long_term_nodes: state.brain.long_term.nodes.len(),
        session_log_entries: state.session_log.len(),
    });
    log_event_rich("start", "session cold-start", Some(event_data));

    Ok(if args.json {
        let json = serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_string());
        format!("{}\n", json)
    } else {
        crate::commands::memory::format_start_summary_markdown(&summary)
    })
}

/// Read the update-version cache file (`.legend/.latest_version`). Returns
/// the latest version string if it's newer than the compiled version and the
/// cache is fresh (< 24 h old). Same logic as the CLI's
/// `check_version_cached` — duplicated here so the daemon doesn't depend on
/// the CLI module tree.
fn read_cached_update_version() -> Option<String> {
    const PATH: &str = ".legend/.latest_version";
    const TTL_SECS: u64 = 86_400;

    let current = env!("CARGO_PKG_VERSION");
    let content = std::fs::read_to_string(PATH).ok()?;
    let mut lines = content.lines();
    let cached_ts: u64 = lines.next()?.parse().ok()?;
    let latest = lines.next()?.trim().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    if now.saturating_sub(cached_ts) < TTL_SECS && version_greater(&latest, current) {
        Some(latest)
    } else {
        None
    }
}

fn version_greater(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> { v.split('.').filter_map(|s| s.parse().ok()).collect() };
    parse(a) > parse(b)
}

// ---------------------------------------------------------------------------
// Query — CLI stdout rendering (wraps apply_query)
// ---------------------------------------------------------------------------

/// Render a `legend memory query` — wraps `apply_query` and formats the
/// stdout JSON in the shape the CLI expects. The `with_reasons` flag picks
/// between the richer annotated shape (`--reasons`) and the compact default.
pub fn render_query(
    state: &mut MemoryState,
    query: &str,
    with_reasons: bool,
) -> Result<String, String> {
    let context = apply_query(state, query)?;
    let primed_count = context
        .long_term
        .iter()
        .filter(|n| n.edge_type.is_some())
        .count();
    Ok(if with_reasons {
        format_query_with_reasons(&context, primed_count)
    } else {
        format_query_context(&context)
    })
}

fn format_query_with_reasons(context: &MemoryContext, primed_count: usize) -> String {
    let working_memory_with_reasons: Vec<serde_json::Value> = context
        .working_memory
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "text": m.text,
                "similarity": crate::tool::round3(m.similarity),
                "reason": "matched in working memory (L1)",
            })
        })
        .collect();

    let short_term_with_reasons: Vec<serde_json::Value> = context
        .short_term
        .iter()
        .map(|m| {
            let reason = if m.similarity >= 0.8 {
                "high semantic similarity to query"
            } else if m.similarity >= 0.5 {
                "moderate semantic similarity to query"
            } else if m.similarity >= 0.2 {
                "weak semantic similarity, may share related terms"
            } else {
                "low similarity, included for coverage"
            };
            serde_json::json!({
                "id": m.id,
                "text": m.text,
                "similarity": crate::tool::round3(m.similarity),
                "reason": reason,
            })
        })
        .collect();

    let long_term_with_reasons: Vec<serde_json::Value> = context
        .long_term
        .iter()
        .map(|n| {
            let reason = if n.edge_type.is_some() {
                format!(
                    "reached via {} edge from related entity",
                    n.edge_type.as_ref().unwrap()
                )
            } else {
                "direct entity match from query".to_string()
            };
            serde_json::json!({
                "id": n.id,
                "label": n.label,
                "kind": n.kind,
                "weight": crate::tool::round3(n.weight),
                "reason": reason,
            })
        })
        .collect();

    let result = serde_json::json!({
        "working_memory": working_memory_with_reasons,
        "short_term": short_term_with_reasons,
        "long_term": long_term_with_reasons,
        "primed_via_edges": primed_count,
        "note": "Read-only retrieval: no recall-time reinforcement or clock advance"
    });

    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    format!("{}\n", json)
}

fn format_query_context(context: &MemoryContext) -> String {
    let working_memory: Vec<&str> = context
        .working_memory
        .iter()
        .map(|m| m.text.as_str())
        .collect();
    let related_topics: Vec<&str> = context.long_term.iter().map(|n| n.label.as_str()).collect();

    // Emit rich objects when any memory has temporal metadata.
    let has_any_temporal = context
        .short_term
        .iter()
        .any(|m| m.wall_clock > 0 || !m.extracted_dates.is_empty() || m.created_at_clock > 0);

    let memories: Vec<serde_json::Value> = if has_any_temporal {
        context
            .short_term
            .iter()
            .map(|m| {
                let mut obj = serde_json::json!({ "text": m.text });
                if m.wall_clock > 0 {
                    obj["wall_clock"] = serde_json::json!(m.wall_clock);
                }
                if !m.extracted_dates.is_empty() {
                    obj["dates"] = serde_json::json!(m.extracted_dates);
                }
                if m.created_at_clock > 0 {
                    obj["seq"] = serde_json::json!(m.created_at_clock);
                }
                obj
            })
            .collect()
    } else {
        context
            .short_term
            .iter()
            .map(|m| serde_json::json!(m.text))
            .collect()
    };

    let result = serde_json::json!({
        "working_memory": working_memory,
        "memories": memories,
        "related_topics": related_topics,
    });
    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    format!("{}\n", json)
}

// ---------------------------------------------------------------------------
// Query — read-only, structured return (used by mcp-serve Phase 4)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Plan set-status — targeted item mutation (item #14)
// ---------------------------------------------------------------------------

/// Apply a targeted plan-item status flip. Thin wrapper over
/// `anterior_pfc::set_item_status_by_number` with CLI-friendly error shaping
/// and stdout text for the daemon response.
pub fn render_plan_set_status(
    state: &mut MemoryState,
    plan_name: &str,
    item_number: u64,
    status_str: &str,
) -> Result<String, String> {
    let new_status = crate::memory::anterior_pfc::parse_status(status_str)
        .ok_or_else(|| format!("invalid status '{}': expected active/pending/deferred/done", status_str))?;
    let embedding_dim = state.brain.config.embedding_dim;
    let clock = state.brain.clock;
    let item_idx = crate::memory::anterior_pfc::set_item_status_by_number(
        &mut state.brain.plans,
        plan_name,
        item_number,
        new_status.clone(),
        clock,
        embedding_dim,
    )?;
    Ok(format!(
        "✓ set item {} status to {} in plan '{}' (position {})\n",
        item_number,
        new_status.label(),
        plan_name,
        item_idx
    ))
}

// ---------------------------------------------------------------------------
// Query (daemon side)
// ---------------------------------------------------------------------------

/// Run the read-only query path and return the full `MemoryContext`. Used by
/// MCP's `tool_memory_query` which renders into its own MCP-shaped markdown.
///
/// No state mutation: `RetrievalMode::ReadOnly` prevents recall-time
/// reinforcement / clock advance. Still logs the rich query event so the
/// observability log sees the request.
pub fn apply_query(state: &mut MemoryState, query: &str) -> Result<MemoryContext, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Empty query".into());
    }

    let context = crate::memory::retrieve_context_with_mode(
        &mut state.brain,
        trimmed,
        crate::memory::RetrievalMode::ReadOnly,
    );

    let primed_count = context
        .long_term
        .iter()
        .filter(|n| n.edge_type.is_some())
        .count();

    let event_data = EventData::Query(QueryEventData {
        matches: context
            .short_term
            .iter()
            .take(5)
            .map(|m| MatchedEntry {
                id: m.id,
                similarity: m.similarity,
                text_preview: truncate_text(&m.text, 80),
            })
            .collect(),
        graph_nodes: context
            .long_term
            .iter()
            .take(8)
            .map(|n| GraphHit {
                id: n.id,
                label: n.label.clone(),
                kind: n.kind.clone(),
                weight: n.weight,
            })
            .collect(),
        primed_count,
    });
    log_event_rich("query", trimmed, Some(event_data));

    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cheap sanity checks — full coverage lives in the per-command files'
    // conformance tests, which exercise the IPC path end-to-end.

    fn state_with_task(task: &str) -> MemoryState {
        let mut s = MemoryState::default();
        crate::memory::set_task(&mut s, task);
        s
    }

    #[test]
    fn task_get_reports_set_task() {
        let s = state_with_task("finish daemon");
        let out = render_task_get(&s).unwrap();
        assert!(out.contains("finish daemon"), "{}", out);
    }

    #[test]
    fn task_get_no_task_message() {
        let s = MemoryState::default();
        let out = render_task_get(&s).unwrap();
        assert!(out.contains("No current task"), "{}", out);
    }

    #[test]
    fn task_clear_removes_task() {
        let mut s = state_with_task("temporary");
        let out = render_task_clear(&mut s).unwrap();
        assert!(out.contains("✓"), "{}", out);
        assert!(crate::memory::get_task(&s).is_none());
    }
}
