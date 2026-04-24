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
//! Keeping the render logic out of `server.rs` and out of each command file's
//! old `handle_*` makes the split clean: each file's `handle_*` becomes a
//! thin orchestrator that tries IPC and falls back to load → render → save.

use crate::commands::memory::{
    extract_keyword_directives, log_event_rich, truncate_text, EventData, GraphHit, MatchedEntry,
    TickEventData,
};
use crate::memory::MemoryState;

// ---------------------------------------------------------------------------
// Tick
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
