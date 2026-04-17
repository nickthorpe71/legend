use super::event_log::*;
use crate::cli::{parse_args, CommandDef};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
struct StartOptions {
    compact: bool,
    json: bool,
    category: Option<String>,
    tokens: bool,
    query: Option<String>,
}

fn parse_start_args(args: &[String], def: &CommandDef) -> StartOptions {
    let parsed = parse_args(args, def);

    let query = parsed.get("query").map(|s| s.to_string()).or_else(|| {
        if parsed.positional.is_empty() {
            None
        } else {
            Some(parsed.positional.join(" "))
        }
    });

    StartOptions {
        compact: parsed.has("compact"),
        json: parsed.has("json"),
        tokens: parsed.has("tokens"),
        category: parsed.get("category").map(|s| s.to_string()),
        query,
    }
}

// ---------------------------------------------------------------------------
// Version check (cached GitHub release)
// ---------------------------------------------------------------------------

const VERSION_CACHE_PATH: &str = ".legend/.latest_version";
const VERSION_CHECK_INTERVAL: u64 = 86400;

fn check_version_cached() -> Option<String> {
    let current = env!("CARGO_PKG_VERSION");
    let content = std::fs::read_to_string(VERSION_CACHE_PATH).ok()?;
    let mut lines = content.lines();
    let cached_ts: u64 = lines.next()?.parse().ok()?;
    let latest = lines.next()?.trim().to_string();

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();

    if now - cached_ts < VERSION_CHECK_INTERVAL && version_greater(&latest, current) {
        return Some(latest);
    }
    None
}

fn version_greater(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> { v.split('.').filter_map(|s| s.parse().ok()).collect() };
    parse(a) > parse(b)
}

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

pub(super) fn handle_start(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_start_args(args, def);
    let mut memory = crate::memory::load_or_default()?;
    let mut summary = crate::memory::build_start_summary_with_options(
        &mut memory,
        opts.compact,
        opts.category.as_deref(),
        opts.query.as_deref(),
    );

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

    let update_available = check_version_cached();
    if let Some(ref latest) = update_available {
        if let Some(obj) = summary.as_object_mut() {
            obj.insert(
                "update_available".to_string(),
                serde_json::json!(format!("v{} → v{}", env!("CARGO_PKG_VERSION"), latest)),
            );
        }
    }

    // Flush working memory: promote qualifying entries to L2, then clear L1
    crate::memory::prefrontal::flush_working_memory(&mut memory.brain);

    let event_data = EventData::Start(StartEventData {
        clock: memory.brain.clock,
        short_term_count: memory.brain.short_term.len(),
        long_term_nodes: memory.brain.long_term.nodes.len(),
        session_log_entries: memory.session_log.len(),
    });

    crate::memory::save(&memory)?;
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

    refresh_version_cache_background();

    Ok(())
}

pub fn format_start_summary_markdown(summary: &serde_json::Value) -> String {
    let mut out = String::new();
    out.push_str("# Legend Memory Context\n\n");

    if let Some(task) = summary.get("current_task").and_then(|t| t.as_str()) {
        out.push_str("## Current Task\n");
        out.push_str(&format!("> {}\n\n", task));
    }

    // Plans from anterior PFC
    if let Some(plans) = summary.get("plans") {
        if !plans.is_null() {
            let active = plans.get("active").and_then(|a| a.as_array());
            let completed = plans.get("completed").and_then(|c| c.as_array());
            let has_plans = active.map_or(false, |a| !a.is_empty())
                || completed.map_or(false, |c| !c.is_empty());
            if has_plans {
                out.push_str("## Current Plans\n\n");
                if let Some(active_plans) = active {
                    for plan in active_plans {
                        if let Some(name) = plan.get("name").and_then(|n| n.as_str()) {
                            out.push_str(&format!("### {}\n", name));
                        }
                        if let Some(items) = plan.get("items").and_then(|i| i.as_array()) {
                            // Group by status
                            let mut by_status: std::collections::BTreeMap<&str, Vec<&str>> =
                                std::collections::BTreeMap::new();
                            for item in items {
                                let status = item
                                    .get("status")
                                    .and_then(|s| s.as_str())
                                    .unwrap_or("pending");
                                let text = item.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                by_status.entry(status).or_default().push(text);
                            }
                            let order = ["active", "pending", "deferred", "done"];
                            for status in &order {
                                if let Some(texts) = by_status.get(status) {
                                    let label = match *status {
                                        "active" => "Active",
                                        "pending" => "Pending",
                                        "deferred" => "Deferred",
                                        "done" => "Done",
                                        _ => status,
                                    };
                                    out.push_str(&format!("**{}:**\n", label));
                                    for text in texts {
                                        if *status == "done" {
                                            out.push_str(&format!("- ~~{}~~\n", text));
                                        } else {
                                            out.push_str(&format!("- {}\n", text));
                                        }
                                    }
                                }
                            }
                        }
                        out.push('\n');
                    }
                }
                if let Some(completed_plans) = completed {
                    if !completed_plans.is_empty() {
                        out.push_str("### Completed Plans\n");
                        for plan in completed_plans {
                            if let Some(name) = plan.get("name").and_then(|n| n.as_str()) {
                                out.push_str(&format!("- ~~{}~~\n", name));
                            }
                        }
                        out.push('\n');
                    }
                }
            }
        }
    }

    // Next Action callout — extracted from plans JSON
    if let Some(plans) = summary.get("plans") {
        if !plans.is_null() {
            if let Some(active_plans) = plans.get("active").and_then(|a| a.as_array()) {
                // Find first active item, fallback to first pending
                let mut next_action: Option<(&str, &str)> = None;
                'active_search: for plan in active_plans {
                    if let Some(items) = plan.get("items").and_then(|i| i.as_array()) {
                        for item in items {
                            if item.get("status").and_then(|s| s.as_str()) == Some("active") {
                                let name = plan.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                let text = item.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                next_action = Some((name, text));
                                break 'active_search;
                            }
                        }
                    }
                }
                if next_action.is_none() {
                    'pending_search: for plan in active_plans {
                        if let Some(items) = plan.get("items").and_then(|i| i.as_array()) {
                            for item in items {
                                if item.get("status").and_then(|s| s.as_str()) == Some("pending") {
                                    let name =
                                        plan.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                    let text =
                                        item.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                    next_action = Some((name, text));
                                    break 'pending_search;
                                }
                            }
                        }
                    }
                }
                if let Some((name, text)) = next_action {
                    out.push_str("## Next Action\n");
                    out.push_str(&format!("> **{}**: {}\n\n", name, text));
                }
            }
        }
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
    out.push_str("*Plans are first-class in Legend. When the user mentions \"plan\", \"next\", \"where we left off\", or \"what to work on\" — ALWAYS call `legend_memory_plan`. To advance: `PLAN: Plan Name\\n[done] Completed item\\n[active] Next item\\n[pending] Remaining`*\n");
    out
}

fn print_token_overhead(output: &str, session_count: usize) {
    let start_tokens = output.len() / 4;
    let prompt_hook_tokens_per_session = 15 * 3;
    let stop_hook_tokens_per_session = 15;
    let total_per_session =
        start_tokens + prompt_hook_tokens_per_session + stop_hook_tokens_per_session;

    eprintln!("\n## Token Overhead Estimate");
    eprintln!("  Session start injection:  ~{} tokens", start_tokens);
    eprintln!(
        "  UserPromptSubmit hooks:   ~{} tokens/session (est. 3 prompts × 15)",
        prompt_hook_tokens_per_session
    );
    eprintln!(
        "  Stop hook:                ~{} tokens/session",
        stop_hook_tokens_per_session
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::FlagDef;

    static TEST_START_DEF: CommandDef = CommandDef {
        name: "start",
        about: "Session start",
        usage: "legend memory start [options] [query]",
        flags: &[
            FlagDef {
                long: "--compact",
                short: Some('c'),
                about: "Compact output",
                takes_value: false,
            },
            FlagDef {
                long: "--json",
                short: Some('j'),
                about: "JSON output",
                takes_value: false,
            },
            FlagDef {
                long: "--tokens",
                short: Some('t'),
                about: "Show token overhead",
                takes_value: false,
            },
            FlagDef {
                long: "--category",
                short: None,
                about: "Filter by category",
                takes_value: true,
            },
            FlagDef {
                long: "--query",
                short: Some('q'),
                about: "Query string",
                takes_value: true,
            },
        ],
        positionals: &[],
        children: &[],
    };

    #[test]
    fn test_parse_start_args_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_start_args(&args, &TEST_START_DEF);
        assert!(!opts.compact);
        assert!(!opts.json);
        assert!(!opts.tokens);
        assert!(opts.category.is_none());
        assert!(opts.query.is_none());
    }

    #[test]
    fn test_parse_start_args_compact() {
        let args = vec!["--compact".to_string()];
        assert!(parse_start_args(&args, &TEST_START_DEF).compact);
        let args = vec!["-c".to_string()];
        assert!(parse_start_args(&args, &TEST_START_DEF).compact);
    }

    #[test]
    fn test_parse_start_args_category_space() {
        let args = vec!["--category".to_string(), "bugs".to_string()];
        let opts = parse_start_args(&args, &TEST_START_DEF);
        assert_eq!(opts.category, Some("bugs".to_string()));
    }

    #[test]
    fn test_parse_start_args_category_equals() {
        let args = vec!["--category=architecture".to_string()];
        let opts = parse_start_args(&args, &TEST_START_DEF);
        assert_eq!(opts.category, Some("architecture".to_string()));
    }

    #[test]
    fn test_parse_start_args_query_equals() {
        let args = vec!["--query=memory system".to_string()];
        let opts = parse_start_args(&args, &TEST_START_DEF);
        assert_eq!(opts.query, Some("memory system".to_string()));
    }

    #[test]
    fn test_parse_start_args_json_and_tokens() {
        let args = vec!["--json".to_string(), "--tokens".to_string()];
        let opts = parse_start_args(&args, &TEST_START_DEF);
        assert!(opts.json);
        assert!(opts.tokens);
    }

    #[test]
    fn test_parse_start_args_positional_query() {
        let args = vec!["some".to_string(), "topic".to_string()];
        let opts = parse_start_args(&args, &TEST_START_DEF);
        assert_eq!(opts.query, Some("some topic".to_string()));
    }

    // version_greater tests
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
        assert!(!version_greater("1", "1.0.0"));
        assert!(version_greater("2", "1.0.0"));
    }

    #[test]
    fn test_version_greater_malformed() {
        assert!(!version_greater("1.a.0", "1.0.0"));
    }

    // format_start_summary_markdown tests
    #[test]
    fn test_format_start_summary_includes_header() {
        let summary = serde_json::json!({});
        let out = format_start_summary_markdown(&summary);
        assert!(out.contains("# Legend Memory Context"));
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
