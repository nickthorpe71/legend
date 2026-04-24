use crate::cli::{parse_args, CommandDef};
use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

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
//
// Read side moved to src/commands/daemon/handlers.rs::read_cached_update_version
// so the daemon reads the cache when rendering `memory start`. Write side
// (background refresh) stays CLI-side — it spawns `curl`, not something
// a long-running daemon should do.

const VERSION_CACHE_PATH: &str = ".legend/.latest_version";

/// Tests-only companion to the daemon-side version check. Kept here (rather
/// than deleted) so the existing `test_version_greater_*` suite in this
/// module still covers the comparison logic; the daemon side has its own
/// copy inside `render_start` to avoid a circular dependency between the
/// CLI and daemon modules.
#[cfg(test)]
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

pub(super) fn handle_start(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_start_args(args, def);

    // Try the daemon first. Daemon handles: build_start_summary +
    // session-log warning + update-cache injection + L1→L2 flush + event
    // log. Returns the exact stdout string we'd print (markdown or JSON).
    if let Some(stdout) = try_over_ipc(Command::Start {
        category: opts.category.clone(),
        compact: opts.compact,
        json: opts.json,
        tokens: opts.tokens,
        query: opts.query.clone(),
    })? {
        // --tokens is a CLI-side concern (writes to stderr). Token count is
        // derived from the stdout length, which is the same whether the
        // stdout came from the daemon or the in-process path.
        if opts.tokens && !opts.json {
            // We don't know session log count from the daemon payload
            // without parsing it back out. Use a rough upper-bound of 100
            // (the capacity) for the display; the exact number is a
            // secondary diagnostic anyway.
            print_token_overhead(&stdout, 100);
        }
        print!("{}", stdout);
        refresh_version_cache_background();
        return Ok(());
    }

    // In-process fallback — daemon unavailable. Same state mutations, same
    // stdout shape.
    let mut memory = crate::memory::load_or_default()?;
    let stdout = handlers::render_start(
        &mut memory,
        handlers::StartArgs {
            compact: opts.compact,
            json: opts.json,
            category: opts.category.as_deref(),
            query: opts.query.as_deref(),
        },
    )
    .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&memory)?;

    if opts.tokens && !opts.json {
        print_token_overhead(&stdout, memory.session_log.len());
    }
    print!("{}", stdout);
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
    out.push_str("*Plans are first-class in Legend. `memory start` surfaces the executive plan queue; update it with `PLAN: Plan Name\\n[done] Completed item\\n[active] Next item\\n[pending] Remaining` via `memory tick`.*\n");
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
