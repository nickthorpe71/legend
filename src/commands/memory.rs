use crate::memory::{reset_memory, MemoryContext, MemoryState, ReinforceResult};
use std::io::{self, Read};

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
    memory.save()?;

    log_event("tick", text.trim());
    print_context(context);
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

    log_event("query", query.trim());
    print_context(context);
    Ok(())
}

fn handle_start() -> Result<(), Box<dyn std::error::Error>> {
    let mut memory = MemoryState::load_or_default()?;
    let summary = memory.build_start_summary();
    memory.save()?;
    log_event("start", "session cold-start");
    let json = serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_string());
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
    log_event("consolidate", &format!("{} groups merged", summaries.len()));
    let json = serde_json::to_string_pretty(&summaries).unwrap_or_else(|_| "[]".to_string());
    println!("{}", json);
    Ok(())
}

fn handle_context() -> Result<(), Box<dyn std::error::Error>> {
    let memory = MemoryState::load_or_default()?;
    let summary = memory.build_context_summary();
    let json = serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
    Ok(())
}

fn handle_sessions(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let n: usize = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

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

    let signal: f32 = args[0].parse()
        .map_err(|_| format!("Invalid signal '{}': expected a float like 1.0 or -0.5", args[0]))?;

    let ids: Result<Vec<u64>, _> = args[1..].iter().map(|s| s.parse()).collect();
    let ids = ids.map_err(|_| "Invalid entry ID: expected integer(s)")?;

    let mut memory = MemoryState::load_or_default()?;
    let result = memory.reinforce(&ids, signal);
    memory.save()?;

    log_event("reinforce", &format!("signal={} ids={:?}", signal, ids));
    print_reinforce_result(&result);
    Ok(())
}

fn read_stdin() -> Result<String, Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

fn print_context(context: MemoryContext) {
    let json = serde_json::to_string_pretty(&context).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

fn print_reinforce_result(result: &ReinforceResult) {
    let json = serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

fn handle_dump() -> Result<(), Box<dyn std::error::Error>> {
    let memory = MemoryState::load_or_default()?;
    let dump = memory.build_dump();
    let json = serde_json::to_string_pretty(&dump).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
    Ok(())
}

/// Append a structured event to `.legend/events.jsonl` for dashboard streaming.
fn log_event(cmd: &str, detail: &str) {
    use std::io::Write;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entry = serde_json::json!({"ts": ts, "cmd": cmd, "detail": detail});
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(".legend/events.jsonl") else { return };
    let _ = writeln!(f, "{}", entry);
}

fn print_memory_help() {
    println!("Legend Memory - hierarchical memory subsystem");
    println!();
    println!("Usage:");
    println!("  legend memory start            Session start: context + retrieval in one call");
    println!("  legend memory tick <text>       Record a memory (decision, progress, discovery)");
    println!("  legend memory tick              Record a memory (reads stdin)");
    println!("  legend memory query <text>      Query memory (auto-reinforces top result)");
    println!("  legend memory reinforce <sig> <id...>  Explicit feedback on retrieved entries");
    println!("  legend memory dump              Export full memory state as JSON");
    println!("  legend memory stats             Show memory stats");
    println!("  legend memory context           Structured context summary (JSON)");
    println!("  legend memory sessions [n]      Show last n session log entries (default 10)");
    println!("  legend memory consolidate       Merge similar memories into long-term graph");
    println!("  legend memory reset             Reset memory store");
}
