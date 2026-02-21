use crate::memory::{reset_memory, MemoryContext, MemoryState, ReinforceResult, TickResult};
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
        "start" => handle_start(&args[1..]),
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

/// Options for tick command
struct TickOptions {
    text: String,
    is_blocker: bool,
}

fn parse_tick_args(args: &[String]) -> Result<TickOptions, Box<dyn std::error::Error>> {
    let mut is_blocker = false;
    let mut text_parts: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--blocker" | "-b" => is_blocker = true,
            _ => text_parts.push(arg),
        }
    }

    let text = if text_parts.is_empty() {
        read_stdin()?
    } else {
        text_parts.join(" ")
    };

    Ok(TickOptions { text, is_blocker })
}

fn handle_tick(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_tick_args(args)?;

    if opts.text.trim().is_empty() {
        return Err("No input provided for tick".into());
    }

    // Prepend BLOCKER prefix if flag is set
    let text = if opts.is_blocker {
        format!("BLOCKER: {}", opts.text.trim())
    } else {
        opts.text.trim().to_string()
    };

    let mut memory = MemoryState::load_or_default()?;
    let tick_result = memory.tick(&text);
    let should_consolidate = memory.should_suggest_consolidation();

    // Boost salience for blocker entries
    if opts.is_blocker {
        if let Some(entry) = memory.short_term.iter_mut().find(|e| e.id == tick_result.entry_id) {
            entry.salience = (entry.salience + 0.4).min(1.0);
        }
    }

    memory.save()?;

    // Log rich event data
    let event_data = EventData::Tick(TickEventData {
        entry_id: Some(tick_result.entry_id),
        matches: tick_result.context
            .short_term
            .iter()
            .take(5)
            .map(|m| MatchedEntry {
                id: m.id,
                similarity: m.similarity,
                text_preview: truncate_text(&m.text, 80),
            })
            .collect(),
        graph_nodes: tick_result.context
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
    print_tick_result(&tick_result);

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

/// Options for query command
#[derive(Default)]
struct QueryOptions {
    query: String,
    show_reasons: bool,
}

fn parse_query_args(args: &[String]) -> Result<QueryOptions, Box<dyn std::error::Error>> {
    let mut show_reasons = false;
    let mut query_parts: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--reasons" | "-r" => show_reasons = true,
            _ => query_parts.push(arg),
        }
    }

    if query_parts.is_empty() {
        return Err("Provide a query string".into());
    }

    Ok(QueryOptions {
        query: query_parts.join(" "),
        show_reasons,
    })
}

fn handle_query(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_query_args(args)?;

    let mut memory = MemoryState::load_or_default()?;
    let context = memory.retrieve_context(opts.query.trim());
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
    log_event_rich("query", opts.query.trim(), Some(event_data));

    if opts.show_reasons {
        print_query_with_reasons(&context, primed_count);
    } else {
        print_context(context);
    }
    Ok(())
}

/// Print query results with reasoning for why each result was returned
fn print_query_with_reasons(context: &MemoryContext, primed_count: usize) {
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
                "similarity": (m.similarity * 1000.0).round() / 1000.0,
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
                "weight": (n.weight * 1000.0).round() / 1000.0,
                "reason": reason,
            })
        })
        .collect();

    let result = serde_json::json!({
        "short_term": short_term_with_reasons,
        "long_term": long_term_with_reasons,
        "primed_via_edges": primed_count,
        "note": "Top result auto-reinforced (+3% salience boost)"
    });

    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

/// Options for memory start command
#[derive(Default)]
struct StartOptions {
    compact: bool,
    category: Option<String>,
}

fn parse_start_args(args: &[String]) -> StartOptions {
    let mut opts = StartOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--compact" | "-c" => opts.compact = true,
            "--category" => {
                if i + 1 < args.len() {
                    opts.category = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            arg if arg.starts_with("--category=") => {
                opts.category = Some(arg.trim_start_matches("--category=").to_string());
            }
            _ => {}
        }
        i += 1;
    }
    opts
}

/// Session log capacity warning threshold (90% of SESSION_LOG_CAPACITY=100)
const SESSION_LOG_WARNING_THRESHOLD: usize = 90;

fn handle_start(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_start_args(args);
    let mut memory = MemoryState::load_or_default()?;
    let mut summary =
        memory.build_start_summary_with_options(opts.compact, opts.category.as_deref());

    // Add warning if session log is approaching capacity
    if memory.session_log.len() >= SESSION_LOG_WARNING_THRESHOLD {
        if let Some(obj) = summary.as_object_mut() {
            obj.insert(
                "warning".to_string(),
                serde_json::json!(format!(
                    "Session log at {}% capacity ({}/100). Oldest entries will be dropped.",
                    memory.session_log.len(),
                    memory.session_log.len()
                )),
            );
        }
    }

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

/// Options for sessions command
struct SessionsOptions {
    count: usize,
    show_all: bool,
}

fn parse_sessions_args(args: &[String]) -> SessionsOptions {
    let mut opts = SessionsOptions {
        count: 10,
        show_all: false,
    };
    for arg in args {
        match arg.as_str() {
            "--all" | "-a" => opts.show_all = true,
            s if s.parse::<usize>().is_ok() => {
                opts.count = s.parse().unwrap();
            }
            _ => {}
        }
    }
    opts
}

fn handle_sessions(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_sessions_args(args);

    let memory = MemoryState::load_or_default()?;
    let recent = memory.recent_sessions(opts.count);

    if recent.is_empty() {
        println!("No session log entries yet.");
    } else {
        for entry in recent {
            // Filter out empty/whitespace-only entries unless --all is specified
            if !opts.show_all && entry.text.trim().is_empty() {
                continue;
            }
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
    // Simplified output: just text and topic labels (no IDs, weights, refs)
    let memories: Vec<&str> = context.short_term.iter().map(|m| m.text.as_str()).collect();
    let related_topics: Vec<&str> = context
        .long_term
        .iter()
        .map(|n| n.label.as_str())
        .collect();

    let result = serde_json::json!({
        "memories": memories,
        "related_topics": related_topics,
    });
    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

fn print_tick_result(result: &TickResult) {
    // Simplified output: just action and entry_id (no context dump)
    let output = serde_json::json!({
        "action": result.action,
        "entry_id": result.entry_id,
    });
    let json = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
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

/// Maximum lines in events.jsonl before rotation.
const EVENT_LOG_MAX_LINES: usize = 10_000;
const EVENT_LOG_PATH: &str = ".legend/events.jsonl";
const EVENT_LOG_ARCHIVE: &str = ".legend/events.jsonl.1";

/// Rotate event log if it exceeds the max line count.
fn maybe_rotate_event_log() {
    use std::io::BufRead;
    let Ok(file) = std::fs::File::open(EVENT_LOG_PATH) else {
        return;
    };
    let line_count = std::io::BufReader::new(file).lines().count();
    if line_count >= EVENT_LOG_MAX_LINES {
        // Rotate: rename current to archive (overwrites old archive)
        let _ = std::fs::rename(EVENT_LOG_PATH, EVENT_LOG_ARCHIVE);
    }
}

/// Append a structured event to `.legend/events.jsonl` for dashboard streaming.
/// Includes optional rich data payload for detailed observability.
/// Automatically rotates the log when it exceeds EVENT_LOG_MAX_LINES.
fn log_event_rich(cmd: &str, detail: &str, data: Option<EventData>) {
    use std::io::Write;

    // Check for rotation before appending
    maybe_rotate_event_log();

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
        .open(EVENT_LOG_PATH)
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
    println!("  legend memory start [options]   Session start: context + categorized in one call");
    println!("    --compact, -c                   Compact output (text only, no ids)");
    println!("    --category <name>               Filter to one category (bugs, todos, decisions, architecture, preferences)");
    println!("  legend memory tick [options] <text>  Record a memory (decision, progress, discovery)");
    println!("    --blocker, -b                   Mark as blocker (boosts salience, prefixes with BLOCKER:)");
    println!("  legend memory tick              Record a memory (reads stdin)");
    println!("  legend memory query [options] <text>  Query memory (auto-reinforces top result)");
    println!("    --reasons, -r                   Include similarity scores and retrieval reasoning");
    println!("  legend memory task              Show current task");
    println!("  legend memory task set <text>   Set current task");
    println!("  legend memory task clear        Clear current task");
    println!("  legend memory reinforce <sig> <id...>  Explicit feedback on retrieved entries");
    println!("  legend memory dump              Export full memory state as JSON");
    println!("  legend memory stats             Show memory stats");
    println!("  legend memory context           Structured context summary (JSON)");
    println!("  legend memory sessions [n] [--all]  Show last n session log entries (default 10)");
    println!("    --all, -a                       Include empty entries");
    println!("  legend memory consolidate       Merge similar memories into long-term graph");
    println!("  legend memory reset             Reset memory store");
}
