use crate::memory::{reset_memory, MemoryContext, MemoryState, ReinforceResult};
use serde::Serialize;
use std::io::{self, Read};

// ---------------------------------------------------------------------------
// Rich Event Data Types for Dashboard Observability
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum EventData {
    Tick(TickEventData),
    Query(QueryEventData),
    Reinforce(ReinforceEventData),
    Consolidate(ConsolidateEventData),
    Start(StartEventData),
}

#[derive(Debug, Clone, Serialize)]
pub struct TickEventData {
    pub entry_id: Option<u64>,
    pub matches: Vec<MatchedEntry>,
    pub graph_nodes: Vec<GraphHit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryEventData {
    pub matches: Vec<MatchedEntry>,
    pub graph_nodes: Vec<GraphHit>,
    pub primed_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchedEntry {
    pub id: u64,
    pub similarity: f32,
    pub text_preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphHit {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReinforceEventData {
    pub signal: f32,
    pub entries: Vec<ReinforceEntry>,
    pub graph_nodes_affected: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReinforceEntry {
    pub id: u64,
    pub before: f32,
    pub after: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsolidateEventData {
    pub groups_merged: usize,
    pub summaries: Vec<ConsolidatedGroup>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsolidatedGroup {
    pub node_id: u64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartEventData {
    pub clock: u64,
    pub short_term_count: usize,
    pub long_term_nodes: usize,
    pub session_log_entries: usize,
}

fn truncate_text(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

pub fn handle_memory(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        print_memory_help();
        return Ok(());
    }

    match args[0].as_str() {
        "tick" => handle_tick(&args[1..]),
        "query" => handle_query(&args[1..]),
        "start" => handle_start(),
        "stats" => handle_stats(),
        "reset" => handle_reset(),
        "consolidate" => handle_consolidate(),
        "context" => handle_context(),
        "sessions" => handle_sessions(&args[1..]),
        "reinforce" => handle_reinforce(&args[1..]),
        "dump" => handle_dump(),
        "task" => handle_task(&args[1..]),
        _ => {
            print_memory_help();
            Ok(())
        }
    }
}

fn handle_tick(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let text = if args.is_empty() {
        read_stdin()?
    } else {
        args.join(" ")
    };

    if text.trim().is_empty() {
        return Err("No input provided for tick".into());
    }

    let mut memory = MemoryState::load_or_default()?;
    let context = memory.tick(text.trim());
    let should_consolidate = memory.should_suggest_consolidation();

    // Get the entry ID of the newly created/updated entry
    let entry_id = memory.short_term.last().map(|e| e.id);

    memory.save()?;

    // Log rich event data
    let event_data = EventData::Tick(TickEventData {
        entry_id,
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
    print_context(context);

    if should_consolidate {
        let summaries = memory.consolidate();
        memory.save()?; // save again after consolidation
        if !summaries.is_empty() {
            let consolidate_data = EventData::Consolidate(ConsolidateEventData {
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
                "auto_consolidate",
                &format!("{} groups merged", summaries.len()),
                Some(consolidate_data),
            );
            eprintln!(
                "[auto-consolidated {} group(s) into long-term memory]",
                summaries.len()
            );
        }
    }
    Ok(())
}

fn handle_query(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err("Provide a query string".into());
    }

    let query = args.join(" ");
    let mut memory = MemoryState::load_or_default()?;
    let context = memory.retrieve_context(query.trim());
    memory.save()?;

    // Count primed nodes (those reached via edge traversal)
    let primed_count = context
        .long_term
        .iter()
        .filter(|n| n.edge_type.is_some())
        .count();

    // Log rich event data
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
    log_event_rich("query", query.trim(), Some(event_data));
    print_context(context);
    Ok(())
}

fn handle_start() -> Result<(), Box<dyn std::error::Error>> {
    let mut memory = MemoryState::load_or_default()?;
    let summary = memory.build_start_summary();

    // Log rich event data with memory stats
    let event_data = EventData::Start(StartEventData {
        clock: memory.clock,
        short_term_count: memory.short_term.len(),
        long_term_nodes: memory.long_term.nodes.len(),
        session_log_entries: memory.session_log.len(),
    });

    memory.save()?;
    log_event_rich("start", "session cold-start", Some(event_data));
    let json = serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
    Ok(())
}

fn handle_stats() -> Result<(), Box<dyn std::error::Error>> {
    let memory = MemoryState::load_or_default()?;
    println!("Memory stats:");
    println!("  Immediate buffer: {}", memory.immediate.len());
    println!("  Short-term entries: {}", memory.short_term.len());
    println!("  Long-term nodes: {}", memory.long_term.nodes.len());
    println!("  Long-term edges: {}", memory.long_term.edges.len());
    println!(
        "  Ticks since consolidation: {}",
        memory.ticks_since_consolidation
    );
    if let Some(task) = memory.get_task() {
        println!("  Current task: {}", task);
    }
    Ok(())
}

fn handle_reset() -> Result<(), Box<dyn std::error::Error>> {
    reset_memory()?;
    log_event("reset", "memory store cleared");
    println!("✓ Memory reset");
    Ok(())
}

fn handle_consolidate() -> Result<(), Box<dyn std::error::Error>> {
    let mut memory = MemoryState::load_or_default()?;
    let summaries = memory.consolidate();
    memory.save()?;

    // Log rich event data
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
    println!("{}", json);
    Ok(())
}

fn handle_context() -> Result<(), Box<dyn std::error::Error>> {
    let memory = MemoryState::load_or_default()?;
    let summary = memory.build_context_summary();
    let json = serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
    Ok(())
}

fn handle_sessions(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let n: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(10);

    let memory = MemoryState::load_or_default()?;
    let recent = memory.recent_sessions(n);

    if recent.is_empty() {
        println!("No session log entries yet.");
    } else {
        for entry in recent {
            println!("[t={}] {}", entry.timestamp, entry.text);
        }
    }
    Ok(())
}

fn handle_reinforce(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 {
        return Err("Usage: legend memory reinforce <signal> <id1> [id2 ...]\n  signal: float from -1.0 (irrelevant) to 1.0 (very useful)".into());
    }

    let signal: f32 = args[0].parse().map_err(|_| {
        format!(
            "Invalid signal '{}': expected a float like 1.0 or -0.5",
            args[0]
        )
    })?;

    let ids: Result<Vec<u64>, _> = args[1..].iter().map(|s| s.parse()).collect();
    let ids = ids.map_err(|_| "Invalid entry ID: expected integer(s)")?;

    let mut memory = MemoryState::load_or_default()?;
    let result = memory.reinforce(&ids, signal);
    memory.save()?;

    // Log rich event data with before/after salience
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
    print_reinforce_result(&result);
    Ok(())
}

fn handle_task(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let subcommand = args.first().map(|s| s.as_str()).unwrap_or("show");

    match subcommand {
        "show" | "" => {
            let memory = MemoryState::load_or_default()?;
            match memory.get_task() {
                Some(task) => println!("Current task: {}", task),
                None => println!("No current task set"),
            }
        }
        "set" => {
            if args.len() < 2 {
                return Err("Usage: legend memory task set <task description>".into());
            }
            let task = args[1..].join(" ");
            let mut memory = MemoryState::load_or_default()?;
            memory.set_task(&task);
            memory.save()?;
            log_event("task_set", &task);
            println!("✓ Current task set: {}", task);
        }
        "clear" => {
            let mut memory = MemoryState::load_or_default()?;
            memory.clear_task();
            memory.save()?;
            log_event("task_clear", "task cleared");
            println!("✓ Current task cleared");
        }
        _ => {
            // Treat unknown subcommand as "set" with the full args
            let task = args.join(" ");
            let mut memory = MemoryState::load_or_default()?;
            memory.set_task(&task);
            memory.save()?;
            log_event("task_set", &task);
            println!("✓ Current task set: {}", task);
        }
    }

    Ok(())
}

fn read_stdin() -> Result<String, Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

fn print_context(context: MemoryContext) {
    let json = serde_json::to_string(&context).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

fn print_reinforce_result(result: &ReinforceResult) {
    let json = serde_json::to_string(result).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

fn handle_dump() -> Result<(), Box<dyn std::error::Error>> {
    let memory = MemoryState::load_or_default()?;
    let dump = memory.build_dump();
    let json = serde_json::to_string(&dump).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
    Ok(())
}

/// Append a structured event to `.legend/events.jsonl` for dashboard streaming.
/// Includes optional rich data payload for detailed observability.
fn log_event_rich(cmd: &str, detail: &str, data: Option<EventData>) {
    use std::io::Write;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entry = if let Some(data) = data {
        serde_json::json!({"ts": ts, "cmd": cmd, "detail": detail, "data": data})
    } else {
        serde_json::json!({"ts": ts, "cmd": cmd, "detail": detail})
    };
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(".legend/events.jsonl")
    else {
        return;
    };
    let _ = writeln!(f, "{}", entry);
}

/// Simple event logging without rich data (backwards compatible).
fn log_event(cmd: &str, detail: &str) {
    log_event_rich(cmd, detail, None);
}

fn print_memory_help() {
    println!("Legend Memory - hierarchical memory subsystem");
    println!();
    println!("Usage:");
    println!("  legend memory start            Session start: context + categorized in one call");
    println!("  legend memory tick <text>       Record a memory (decision, progress, discovery)");
    println!("  legend memory tick              Record a memory (reads stdin)");
    println!("  legend memory query <text>      Query memory (auto-reinforces top result)");
    println!("  legend memory task              Show current task");
    println!("  legend memory task set <text>   Set current task");
    println!("  legend memory task clear        Clear current task");
    println!("  legend memory reinforce <sig> <id...>  Explicit feedback on retrieved entries");
    println!("  legend memory dump              Export full memory state as JSON");
    println!("  legend memory stats             Show memory stats");
    println!("  legend memory context           Structured context summary (JSON)");
    println!("  legend memory sessions [n]      Show last n session log entries (default 10)");
    println!("  legend memory consolidate       Merge similar memories into long-term graph");
    println!("  legend memory reset             Reset memory store");
}
