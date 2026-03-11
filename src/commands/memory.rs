use super::llm::{auto_trigger_for_text, LlmAutoTriggerSummary};
use crate::memory::{reset_memory, MemoryContext, MemoryState, ReinforceResult, TickResult};
use serde::Serialize;
use std::io::{self, Read};
use std::time::{SystemTime, UNIX_EPOCH};

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
        // Find a valid char boundary at or before `max`
        let end = s.floor_char_boundary(max);
        format!("{}...", &s[..end])
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
    is_passive: bool,
}

fn parse_tick_args(args: &[String]) -> Result<TickOptions, Box<dyn std::error::Error>> {
    let mut is_blocker = false;
    let mut is_passive = false;
    let mut text_parts: Vec<&str> = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--blocker" | "-b" => is_blocker = true,
            "--passive" | "-p" => is_passive = true,
            _ => text_parts.push(arg),
        }
    }

    let text = if text_parts.is_empty() {
        read_stdin()?
    } else {
        text_parts.join(" ")
    };

    Ok(TickOptions {
        text,
        is_blocker,
        is_passive,
    })
}

/// Detect known noise patterns that should be rejected before storage.
fn is_noise_tick(text: &str) -> bool {
    let t = text.trim();
    // Empty or near-empty
    if t.len() < 10 {
        return true;
    }
    // Auto-generated tool telemetry
    if t.contains("Executed tool") && t.contains("with status") {
        return true;
    }
    // Agent turn noise
    if t.starts_with("EXPERIENCE:") && t.contains("Completed an agent turn") {
        return true;
    }
    // Generic experience with empty tool/status
    if t.starts_with("EXPERIENCE:") && t.contains("''") {
        return true;
    }
    false
}

fn handle_tick(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_tick_args(args)?;

    if opts.text.trim().is_empty() {
        return Err("No input provided for tick".into());
    }

    // Reject known noise patterns
    if is_noise_tick(opts.text.trim()) {
        eprintln!("[tick rejected: low-quality content detected]");
        return Ok(());
    }

    // Prepend prefixes if flags are set
    let text = if opts.is_blocker {
        format!("BLOCKER: {}", opts.text.trim())
    } else if opts.is_passive {
        format!("EXPERIENCE: {}", opts.text.trim())
    } else {
        opts.text.trim().to_string()
    };

    let mut memory = MemoryState::load_or_default()?;
    let tick_result = if opts.is_passive {
        memory.tick_passive(&text)
    } else {
        memory.tick(&text)
    };
    let should_consolidate = memory.should_suggest_consolidation();

    // Adjust salience based on flags
    if let Some(entry) = memory
        .short_term
        .iter_mut()
        .find(|e| e.id == tick_result.entry_id)
    {
        if opts.is_blocker {
            entry.salience = (entry.salience + 0.4).min(1.0);
        } else if opts.is_passive {
            entry.salience *= 0.5; // Passive experiences decay faster
        }
    }

    memory.save()?;

    // Reset pending-tick counter on every real tick
    let _ = std::fs::write(".legend/.pending_ticks", "0");

    // Log rich event data
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
    let llm_trigger = match auto_trigger_for_text(text.trim(), "memory_tick", Some(memory.clock)) {
        Ok(summary) => Some(summary),
        Err(err) => {
            eprintln!("[llm auto-trigger skipped: {}]", err);
            None
        }
    };

    print_tick_result(&tick_result, llm_trigger);

    // Auto-append to ARCHITECTURE.md when tick is tagged with ARCHITECTURE:
    if text.trim().to_uppercase().starts_with("ARCHITECTURE:") {
        append_to_architecture_md(text.trim());
    }

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
    json: bool,
    category: Option<String>,
    tokens: bool,
    query: Option<String>,
}

fn parse_start_args(args: &[String]) -> StartOptions {
    let mut opts = StartOptions::default();
    let mut i = 0;
    let mut query_parts: Vec<&str> = Vec::new();

    while i < args.len() {
        match args[i].as_str() {
            "--compact" | "-c" => opts.compact = true,
            "--json" | "-j" => opts.json = true,
            "--tokens" | "-t" => opts.tokens = true,
            "--query" | "-q" => {
                if i + 1 < args.len() {
                    opts.query = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            "--category" => {
                if i + 1 < args.len() {
                    opts.category = Some(args[i + 1].clone());
                    i += 1;
                }
            }
            arg if arg.starts_with("--category=") => {
                opts.category = Some(arg.trim_start_matches("--category=").to_string());
            }
            arg if arg.starts_with("--query=") => {
                opts.query = Some(arg.trim_start_matches("--query=").to_string());
            }
            arg if !arg.starts_with('-') => {
                query_parts.push(arg);
            }
            _ => {}
        }
        i += 1;
    }

    if opts.query.is_none() && !query_parts.is_empty() {
        opts.query = Some(query_parts.join(" "));
    }

    opts
}

// ---------------------------------------------------------------------------
// Version check (cached GitHub release)
// ---------------------------------------------------------------------------

const VERSION_CACHE_PATH: &str = ".legend/.latest_version";
const VERSION_CHECK_INTERVAL: u64 = 86400; // 24 hours

/// Read cached latest version; return Some(latest) if an update is available.
fn check_version_cached() -> Option<String> {
    let current = env!("CARGO_PKG_VERSION");
    let content = std::fs::read_to_string(VERSION_CACHE_PATH).ok()?;
    let mut lines = content.lines();
    let cached_ts: u64 = lines.next()?.parse().ok()?;
    let latest = lines.next()?.trim().to_string();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();

    // Only warn if cache is fresh and latest is strictly greater
    if now - cached_ts < VERSION_CHECK_INTERVAL && version_greater(&latest, current) {
        return Some(latest);
    }
    None
}

/// Compare semver strings numerically (major.minor.patch).
fn version_greater(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> {
        v.split('.').filter_map(|s| s.parse().ok()).collect()
    };
    parse(a) > parse(b)
}

/// Spawn a background process to refresh the version cache from GitHub releases.
fn refresh_version_cache_background() {
    let _ = std::process::Command::new("sh")
        .args([
            "-c",
            &format!(
                r#"latest=$(curl -sf --max-time 5 https://api.github.com/repos/nickthorpe71/legend/releases/latest | grep -o '"tag_name":"[^"]*"' | head -1 | sed 's/.*"v\?\([^"]*\)".*/\1/'); if [ -n "$latest" ]; then printf '%s\n%s\n' "$(date +%s)" "$latest" > {path}; fi"#,
                path = VERSION_CACHE_PATH
            ),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Session log capacity warning threshold (90% of SESSION_LOG_CAPACITY=100)
const SESSION_LOG_WARNING_THRESHOLD: usize = 90;

fn handle_start(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_start_args(args);
    let mut memory = MemoryState::load_or_default()?;
    let mut summary = memory.build_start_summary_with_options(
        opts.compact,
        opts.category.as_deref(),
        opts.query.as_deref(),
    );

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

    // Check for version updates (from cached GitHub release)
    let update_available = check_version_cached();
    if let Some(ref latest) = update_available {
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

    // Log rich event data with memory stats
    let event_data = EventData::Start(StartEventData {
        clock: memory.clock,
        short_term_count: memory.short_term.len(),
        long_term_nodes: memory.long_term.nodes.len(),
        session_log_entries: memory.session_log.len(),
    });

    memory.save()?;
    log_event_rich("start", "session cold-start", Some(event_data));

    if opts.json {
        let json = serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_string());
        println!("{}", json);
    } else {
        let output = format_start_summary_markdown(&summary);
        if opts.tokens {
            print_token_overhead(&output, memory.session_log.len());
        }
        print!("{}", output);
    }

    // Refresh version cache in the background (non-blocking)
    refresh_version_cache_background();

    Ok(())
}

fn format_start_summary_markdown(summary: &serde_json::Value) -> String {
    let mut out = String::new();
    out.push_str("# Legend Session Start Context\n\n");

    out.push_str("## LEGEND PROTOCOL (MANDATORY)\n");
    out.push_str("- **TICK DECISIONS**: After every file change or major decision, run `legend memory tick 'rationale...'`.\n");
    out.push_str(
        "- **RESEARCH FIRST**: Run `legend memory start` or `query` before starting new tasks.\n",
    );
    out.push_str("- **ARCHITECT**: Update ARCHITECTURE.md or TICK architecture insights when structure changes.\n");
    out.push_str("- **BRIDGE SESSIONS**: Record final decisions and next steps via `legend memory tick` before exiting.\n");
    out.push('\n');

    if let Some(task) = summary.get("current_task").and_then(|t| t.as_str()) {
        out.push_str("## Current Task\n");
        out.push_str(&format!("> {}\n\n", task));
    }

    if let Some(git_sync) = summary.get("git_sync") {
        let commits = git_sync.get("new_commits").and_then(|c| c.as_array());
        let uncommitted = git_sync.get("uncommitted_summary").and_then(|u| u.as_str());

        if (commits.is_some() && !commits.unwrap().is_empty()) || uncommitted.is_some() {
            out.push_str("## Git Synchronization (Background Changes)\n");
            out.push_str("> Legend has detected manual user changes since the last session. You should intelligently tick these into memory.\n\n");

            if let Some(commits) = commits {
                if !commits.is_empty() {
                    out.push_str("### New Commits\n");
                    for commit in commits {
                        if let Some(text) = commit.as_str() {
                            out.push_str(&format!("- {}\n", text));
                        }
                    }
                    out.push('\n');
                }
            }

            if let Some(uncommitted) = uncommitted {
                out.push_str("### Uncommitted Changes\n");
                out.push_str("```\n");
                out.push_str(uncommitted);
                out.push_str("\n```\n\n");
            }
        }
    }

    if let Some(sessions) = summary.get("recent_sessions").and_then(|s| s.as_array()) {
        if !sessions.is_empty() {
            out.push_str("## Recent Activity\n");
            for session in sessions {
                if let Some(text) = session.as_str() {
                    out.push_str(&format!("- {}\n", text));
                }
            }
            out.push('\n');
        }
    }

    if let Some(categorized) = summary.get("categorized").and_then(|c| c.as_object()) {
        out.push_str("## Categorized Memories\n");
        for (name, data) in categorized {
            let (items, total) = if let Some(obj) = data.as_object() {
                let items = obj
                    .get("items")
                    .and_then(|i| i.as_array())
                    .map(|a| a.as_slice())
                    .unwrap_or(&[]);
                let total = obj
                    .get("total")
                    .and_then(|t| t.as_u64())
                    .unwrap_or(items.len() as u64);
                (items, total)
            } else if let Some(arr) = data.as_array() {
                (arr.as_slice(), arr.len() as u64)
            } else {
                (&[][..], 0)
            };

            if total > 0 {
                out.push_str(&format!("\n### {}\n", name.to_uppercase()));
                for item in items {
                    if let Some(text) = item.as_str() {
                        out.push_str(&format!("- {}\n", text));
                    } else if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        out.push_str(&format!("- {}\n", text));
                    }
                }
                if total > items.len() as u64 {
                    out.push_str(&format!(
                        "- *...and {} more. Use `legend memory start --category {}` to see all.*\n",
                        total - items.len() as u64,
                        name
                    ));
                }
            }
        }
    }

    if let Some(warning) = summary.get("warning").and_then(|w| w.as_str()) {
        out.push_str("\n> [!WARNING]\n");
        out.push_str(&format!("> {}\n", warning));
    }

    if let Some(update) = summary.get("update_available").and_then(|u| u.as_str()) {
        out.push_str(&format!(
            "\n> **Update available:** Legend {} — run `curl -fsSL https://raw.githubusercontent.com/nickthorpe71/legend/master/install.sh | bash` to update.\n",
            update
        ));
    }

    out.push_str("\n---\n");
    out.push_str("*Use heredoc for tick/query: `legend memory tick <<'EOF'` ... `EOF`*\n");
    out
}

/// Estimate and print token overhead for the session start output.
fn print_token_overhead(output: &str, session_count: usize) {
    // Rough token estimate: ~4 chars per token (standard heuristic)
    let start_tokens = output.len() / 4;
    // Per-prompt hook reminder: ~100 tokens each, estimate 3 prompts/session
    let prompt_hook_tokens_per_session = 100 * 3;
    // Per-file-edit hook reminder: ~150 tokens, estimate 5 edits/session
    let edit_hook_tokens_per_session = 150 * 5;
    let total_per_session =
        start_tokens + prompt_hook_tokens_per_session + edit_hook_tokens_per_session;

    eprintln!("\n## Token Overhead Estimate");
    eprintln!("  Session start injection:  ~{} tokens", start_tokens);
    eprintln!(
        "  UserPromptSubmit hooks:   ~{} tokens/session (est. 3 prompts × 100)",
        prompt_hook_tokens_per_session
    );
    eprintln!(
        "  PostToolUse hooks:        ~{} tokens/session (est. 5 edits × 150)",
        edit_hook_tokens_per_session
    );
    eprintln!("  Total per session:        ~{} tokens", total_per_session);
    if session_count > 0 {
        eprintln!(
            "  Lifetime ({} sessions):   ~{} tokens",
            session_count,
            total_per_session * session_count
        );
    }
    eprintln!("  (Note: estimates carry ±30% uncertainty)");
    eprintln!();
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

    // Session quality metric from events.jsonl
    if let Some(quality) = compute_session_quality() {
        println!();
        println!("Session quality (current session):");
        println!("  Ticks total:      {}", quality.total_ticks);
        println!(
            "  Meaningful ticks: {} ({:.0}%)",
            quality.meaningful_ticks,
            quality.signal_rate * 100.0
        );
        println!("  Queries:          {}", quality.queries);
        println!("  Consolidations:   {}", quality.consolidations);
        println!("  Quality score:    {}/100", quality.score);
        if quality.score < 40 {
            println!("  [LOW] Aim for more meaningful ticks and queries.");
        } else if quality.score < 70 {
            println!("  [OK] Consider querying before new tasks.");
        } else {
            println!("  [GOOD] Strong session signal.");
        }
    }
    Ok(())
}

/// Quality metrics for the current session.
struct SessionQuality {
    total_ticks: usize,
    meaningful_ticks: usize,
    signal_rate: f32,
    queries: usize,
    consolidations: usize,
    score: u32,
}

/// Compute session quality by scanning events.jsonl since the last "start" event.
fn compute_session_quality() -> Option<SessionQuality> {
    use std::io::BufRead;

    let file = std::fs::File::open(EVENT_LOG_PATH).ok()?;
    let lines: Vec<String> = std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .collect();

    // Find the timestamp of the last "start" event
    let last_start_ts: u64 = lines.iter().rev().find_map(|line| {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        if v.get("cmd")?.as_str()? == "start" {
            v.get("ts")?.as_u64()
        } else {
            None
        }
    })?;

    let mut total_ticks = 0usize;
    let mut meaningful_ticks = 0usize;
    let mut queries = 0usize;
    let mut consolidations = 0usize;

    for line in &lines {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let ts = v.get("ts").and_then(|t| t.as_u64()).unwrap_or(0);
        if ts < last_start_ts {
            continue;
        }
        let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
        match cmd {
            "tick" => {
                total_ticks += 1;
                let detail = v.get("detail").and_then(|d| d.as_str()).unwrap_or("");
                if !detail.starts_with("EXPERIENCE:") {
                    meaningful_ticks += 1;
                }
            }
            "query" => queries += 1,
            "auto_consolidate" | "consolidate" => consolidations += 1,
            _ => {}
        }
    }

    let signal_rate = if total_ticks > 0 {
        meaningful_ticks as f32 / total_ticks as f32
    } else {
        0.0
    };

    // Quality score: 50% signal rate, 30% query presence, 20% consolidation bonus
    let signal_score = (signal_rate * 50.0) as u32;
    let query_score = ((queries as f32 / 3.0).min(1.0) * 30.0) as u32;
    let consolidation_score = if consolidations > 0 { 20u32 } else { 0u32 };
    let score = (signal_score + query_score + consolidation_score).min(100);

    Some(SessionQuality {
        total_ticks,
        meaningful_ticks,
        signal_rate,
        queries,
        consolidations,
        score,
    })
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
    let related_topics: Vec<&str> = context.long_term.iter().map(|n| n.label.as_str()).collect();

    let result = serde_json::json!({
        "memories": memories,
        "related_topics": related_topics,
    });
    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
}

fn print_tick_result(result: &TickResult, llm_trigger: Option<LlmAutoTriggerSummary>) {
    // Simplified output: action and entry_id; include llm trigger metadata when available.
    let output = serde_json::json!({
        "action": result.action,
        "entry_id": result.entry_id,
        "llm_trigger": llm_trigger,
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

/// Convert a unix timestamp (seconds) to an ISO date string (YYYY-MM-DD).
fn ts_to_iso_date(ts: u64) -> String {
    let mut remaining = ts / 86400; // total days since epoch
    let mut year = 1970u64;
    loop {
        let days_in_year = if (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400) {
            366
        } else {
            365
        };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let is_leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let month_days: &[u64] = if is_leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &days in month_days {
        if remaining < days {
            break;
        }
        remaining -= days;
        month += 1;
    }
    let day = remaining + 1;
    format!("{:04}-{:02}-{:02}", year, month, day)
}

/// Append a summary line to ARCHITECTURE.md after an ARCHITECTURE-tagged tick.
/// Creates ARCHITECTURE.md if it doesn't exist.
fn append_to_architecture_md(tick_text: &str) {
    use std::io::Write;

    let date = {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        ts_to_iso_date(ts)
    };

    // Truncate tick text to 200 chars for the entry
    let summary = if tick_text.len() > 200 {
        let end = tick_text.floor_char_boundary(200);
        format!("{}…", &tick_text[..end])
    } else {
        tick_text.to_string()
    };

    let entry = format!("- **{}** — {}\n", date, summary);

    let path = std::path::Path::new("ARCHITECTURE.md");
    if path.exists() {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(path) {
            let _ = f.write_all(entry.as_bytes());
        }
    } else {
        let header = "# Architecture Notes\n\nAuto-generated from ARCHITECTURE: memory ticks.\n\n";
        let content = format!("{}{}", header, entry);
        let _ = std::fs::write(path, content);
    }
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
    println!("    --tokens, -t                    Show token overhead estimate for this session");
    println!("    --category <name>               Filter to one category (bugs, todos, decisions, architecture, preferences)");
    println!(
        "  legend memory tick [options] <text>  Record a memory (decision, progress, discovery)"
    );
    println!("    --blocker, -b                   Mark as blocker (boosts salience, prefixes with BLOCKER:)");
    println!("  legend memory tick              Record a memory (reads stdin)");
    println!("  legend memory query [options] <text>  Query memory (auto-reinforces top result)");
    println!(
        "    --reasons, -r                   Include similarity scores and retrieval reasoning"
    );
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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // is_noise_tick
    // -----------------------------------------------------------------------

    #[test]
    fn test_noise_tick_rejects_short() {
        assert!(is_noise_tick("hi"));
        assert!(is_noise_tick("         ")); // whitespace-only
        assert!(is_noise_tick("123456789")); // exactly 9 chars
    }

    #[test]
    fn test_noise_tick_accepts_exactly_10_chars() {
        assert!(!is_noise_tick("1234567890")); // exactly 10 chars
    }

    #[test]
    fn test_noise_tick_rejects_tool_telemetry() {
        assert!(is_noise_tick(
            "Executed tool \"read_file\" with status \"success\""
        ));
        assert!(is_noise_tick(
            "EXPERIENCE: Executed tool 'ls' with status 'ok'"
        ));
    }

    #[test]
    fn test_noise_tick_rejects_agent_turn_noise() {
        assert!(is_noise_tick(
            "EXPERIENCE: Completed an agent turn with no changes"
        ));
    }

    #[test]
    fn test_noise_tick_rejects_empty_tool_status() {
        assert!(is_noise_tick(
            "EXPERIENCE: Experience: Executed tool '' with status ''"
        ));
    }

    #[test]
    fn test_noise_tick_accepts_legitimate_experience() {
        assert!(!is_noise_tick(
            "EXPERIENCE: User navigated to the settings page and changed theme"
        ));
    }

    #[test]
    fn test_noise_tick_accepts_normal_tick() {
        assert!(!is_noise_tick("Fixed bug in parser module"));
        assert!(!is_noise_tick(
            "DECISION: Chose Tokio over async-std because broader ecosystem support"
        ));
    }

    // -----------------------------------------------------------------------
    // version_greater
    // -----------------------------------------------------------------------

    #[test]
    fn test_version_greater_patch() {
        assert!(version_greater("1.0.1", "1.0.0"));
        assert!(!version_greater("1.0.0", "1.0.1"));
    }

    #[test]
    fn test_version_greater_minor() {
        assert!(version_greater("1.1.0", "1.0.9"));
        assert!(!version_greater("1.0.9", "1.1.0"));
    }

    #[test]
    fn test_version_greater_major() {
        assert!(version_greater("2.0.0", "1.9.9"));
        assert!(!version_greater("1.9.9", "2.0.0"));
    }

    #[test]
    fn test_version_greater_equal() {
        assert!(!version_greater("1.0.0", "1.0.0"));
    }

    #[test]
    fn test_version_greater_missing_components() {
        // "1" parses to [1], "1.0.0" to [1,0,0] — Vec comparison handles this
        assert!(!version_greater("1", "1.0.0"));
        assert!(version_greater("2", "1.0.0"));
    }

    #[test]
    fn test_version_greater_malformed() {
        // Non-numeric parts are skipped by filter_map
        assert!(!version_greater("1.a.0", "1.0.0")); // parses as [1,0] vs [1,0,0]
    }

    // -----------------------------------------------------------------------
    // parse_tick_args
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_tick_args_simple_text() {
        let args = vec!["hello".to_string(), "world".to_string()];
        let opts = parse_tick_args(&args).unwrap();
        assert_eq!(opts.text, "hello world");
        assert!(!opts.is_blocker);
        assert!(!opts.is_passive);
    }

    #[test]
    fn test_parse_tick_args_blocker_flag() {
        let args = vec!["--blocker".to_string(), "can't".to_string(), "proceed".to_string()];
        let opts = parse_tick_args(&args).unwrap();
        assert!(opts.is_blocker);
        assert_eq!(opts.text, "can't proceed");
    }

    #[test]
    fn test_parse_tick_args_passive_flag() {
        let args = vec!["--passive".to_string(), "observed".to_string(), "event".to_string()];
        let opts = parse_tick_args(&args).unwrap();
        assert!(opts.is_passive);
        assert_eq!(opts.text, "observed event");
    }

    #[test]
    fn test_parse_tick_args_short_flags() {
        let args = vec!["-b".to_string(), "stuck".to_string()];
        let opts = parse_tick_args(&args).unwrap();
        assert!(opts.is_blocker);
    }

    // -----------------------------------------------------------------------
    // parse_query_args
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_query_args_simple() {
        let args = vec!["memory".to_string(), "system".to_string()];
        let opts = parse_query_args(&args).unwrap();
        assert_eq!(opts.query, "memory system");
        assert!(!opts.show_reasons);
    }

    #[test]
    fn test_parse_query_args_reasons_flag() {
        let args = vec!["--reasons".to_string(), "test".to_string()];
        let opts = parse_query_args(&args).unwrap();
        assert!(opts.show_reasons);
        assert_eq!(opts.query, "test");
    }

    #[test]
    fn test_parse_query_args_short_reasons() {
        let args = vec!["-r".to_string(), "query".to_string()];
        let opts = parse_query_args(&args).unwrap();
        assert!(opts.show_reasons);
    }

    #[test]
    fn test_parse_query_args_empty_rejects() {
        let args: Vec<String> = vec![];
        assert!(parse_query_args(&args).is_err());
    }

    // -----------------------------------------------------------------------
    // parse_start_args
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_start_args_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_start_args(&args);
        assert!(!opts.compact);
        assert!(!opts.json);
        assert!(!opts.tokens);
        assert!(opts.category.is_none());
        assert!(opts.query.is_none());
    }

    #[test]
    fn test_parse_start_args_compact() {
        let args = vec!["--compact".to_string()];
        assert!(parse_start_args(&args).compact);
        let args = vec!["-c".to_string()];
        assert!(parse_start_args(&args).compact);
    }

    #[test]
    fn test_parse_start_args_category_space() {
        let args = vec!["--category".to_string(), "bugs".to_string()];
        let opts = parse_start_args(&args);
        assert_eq!(opts.category, Some("bugs".to_string()));
    }

    #[test]
    fn test_parse_start_args_category_equals() {
        let args = vec!["--category=architecture".to_string()];
        let opts = parse_start_args(&args);
        assert_eq!(opts.category, Some("architecture".to_string()));
    }

    #[test]
    fn test_parse_start_args_query_equals() {
        let args = vec!["--query=memory system".to_string()];
        let opts = parse_start_args(&args);
        assert_eq!(opts.query, Some("memory system".to_string()));
    }

    #[test]
    fn test_parse_start_args_json_and_tokens() {
        let args = vec!["--json".to_string(), "--tokens".to_string()];
        let opts = parse_start_args(&args);
        assert!(opts.json);
        assert!(opts.tokens);
    }

    #[test]
    fn test_parse_start_args_positional_query() {
        let args = vec!["some".to_string(), "topic".to_string()];
        let opts = parse_start_args(&args);
        assert_eq!(opts.query, Some("some topic".to_string()));
    }

    // -----------------------------------------------------------------------
    // ts_to_iso_date
    // -----------------------------------------------------------------------

    #[test]
    fn test_ts_to_iso_date_epoch() {
        assert_eq!(ts_to_iso_date(0), "1970-01-01");
    }

    #[test]
    fn test_ts_to_iso_date_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        assert_eq!(ts_to_iso_date(1704067200), "2024-01-01");
    }

    #[test]
    fn test_ts_to_iso_date_leap_year() {
        // 2024-02-29 00:00:00 UTC = 1709164800
        assert_eq!(ts_to_iso_date(1709164800), "2024-02-29");
    }

    #[test]
    fn test_ts_to_iso_date_non_leap_year() {
        // 2023-03-01 00:00:00 UTC = 1677628800
        assert_eq!(ts_to_iso_date(1677628800), "2023-03-01");
    }

    #[test]
    fn test_ts_to_iso_date_year_boundary() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        assert_eq!(ts_to_iso_date(1704067199), "2023-12-31");
    }

    // -----------------------------------------------------------------------
    // format_start_summary_markdown
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_start_summary_includes_protocol() {
        let summary = serde_json::json!({});
        let out = format_start_summary_markdown(&summary);
        assert!(out.contains("LEGEND PROTOCOL (MANDATORY)"));
        assert!(out.contains("TICK DECISIONS"));
    }

    #[test]
    fn test_format_start_summary_current_task() {
        let summary = serde_json::json!({
            "current_task": "Fix the parser"
        });
        let out = format_start_summary_markdown(&summary);
        assert!(out.contains("## Current Task"));
        assert!(out.contains("Fix the parser"));
    }

    #[test]
    fn test_format_start_summary_warning() {
        let summary = serde_json::json!({
            "warning": "Session log at 95% capacity (95/100). Oldest entries will be dropped."
        });
        let out = format_start_summary_markdown(&summary);
        assert!(out.contains("[!WARNING]"));
        assert!(out.contains("95% capacity"));
    }

    #[test]
    fn test_format_start_summary_update_available() {
        let summary = serde_json::json!({
            "update_available": "v0.3.3 → v0.4.0"
        });
        let out = format_start_summary_markdown(&summary);
        assert!(out.contains("Update available"));
        assert!(out.contains("v0.3.3 → v0.4.0"));
        assert!(out.contains("install.sh"));
    }

    #[test]
    fn test_format_start_summary_git_sync() {
        let summary = serde_json::json!({
            "git_sync": {
                "new_commits": ["abc123 Fix typo", "def456 Add feature"],
                "uncommitted_summary": "M src/main.rs"
            }
        });
        let out = format_start_summary_markdown(&summary);
        assert!(out.contains("Git Synchronization"));
        assert!(out.contains("abc123 Fix typo"));
        assert!(out.contains("M src/main.rs"));
    }

    #[test]
    fn test_format_start_summary_categorized_with_overflow() {
        let summary = serde_json::json!({
            "categorized": {
                "bugs": {
                    "items": ["Bug one", "Bug two"],
                    "total": 5
                }
            }
        });
        let out = format_start_summary_markdown(&summary);
        assert!(out.contains("### BUGS"));
        assert!(out.contains("Bug one"));
        assert!(out.contains("...and 3 more"));
    }

    #[test]
    fn test_format_start_summary_recent_sessions() {
        let summary = serde_json::json!({
            "recent_sessions": ["Fixed parser", "Added tests"]
        });
        let out = format_start_summary_markdown(&summary);
        assert!(out.contains("## Recent Activity"));
        assert!(out.contains("Fixed parser"));
    }
}
