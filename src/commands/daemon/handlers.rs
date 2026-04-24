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
    ConsolidatedGroup, EventData, GraphHit, MatchedEntry, ReinforceEntry, ReinforceEventData,
    TickEventData,
};
use crate::memory::{MemoryState, ReinforceResult};

// ---------------------------------------------------------------------------
// Tick — mutating
// ---------------------------------------------------------------------------

/// Render a `legend memory tick` — applies keyword directives, calls the core
/// tick, logs the event, and returns the JSON stdout the CLI would print.
///
/// `blocker` is accepted for forward compatibility; today's tick path does not
/// yet apply blocker-specific salience, matching the existing CLI behavior.
pub fn render_tick(
    state: &mut MemoryState,
    text: &str,
    _blocker: bool,
) -> Result<String, String> {
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
        return Ok(format!(
            "{{\"action\":\"keyword_only\",\"keywords_registered\":{}}}\n",
            keyword_directives.len()
        ));
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

    let output = serde_json::json!({
        "action": tick_result.action,
        "entry_id": tick_result.entry_id,
    });
    let json = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
    Ok(format!("{}\n", json))
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

// `memory start` and `memory query` are deferred from Phase 2 Commit B because
// their CLI-side render helpers are private inner functions; extracting them
// cleanly warrants its own pass. Both commands still work via the in-process
// path (try_over_ipc returns NotImplemented, the caller's fallback executes).

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
