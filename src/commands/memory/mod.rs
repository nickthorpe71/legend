mod consolidate;
mod event_log;
mod helpers;
mod personality;
mod plan;
mod query;
mod reinforce;
mod sessions;
mod simple;
mod start;
mod stats;
mod task;
mod tick;

// ---------------------------------------------------------------------------
// Public re-exports (keeps mcp.rs and other consumers working unchanged)
// ---------------------------------------------------------------------------

pub use event_log::log_event_rich;
pub use event_log::{
    ConsolidateEventData, ConsolidatedGroup, EventData, GraphHit, MatchedEntry, QueryEventData,
    ReinforceEntry, ReinforceEventData, StartEventData, TickEventData,
};
pub use helpers::truncate_text;
pub use start::format_start_summary_markdown;

// Used by the daemon's shared render functions.
pub(crate) use helpers::extract_keyword_directives;

// ---------------------------------------------------------------------------
// Subcommand dispatch
// ---------------------------------------------------------------------------

pub fn handle_memory(
    args: &[String],
    def: &crate::cli::CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        print_memory_help();
        return Ok(());
    }

    let sub = args[0].as_str();
    let child_def = def.children.iter().find(|c| c.name == sub);

    match sub {
        "tick" => tick::handle_tick(&args[1..], child_def.expect("tick def")),
        "query" => query::handle_query(&args[1..], child_def.expect("query def")),
        "start" => start::handle_start(&args[1..], child_def.expect("start def")),
        "sessions" => sessions::handle_sessions(&args[1..], child_def.expect("sessions def")),
        "stats" => stats::handle_stats(),
        "reset" => simple::handle_reset(),
        "consolidate" => consolidate::handle_consolidate(),
        "context" => simple::handle_context(),
        "reinforce" => reinforce::handle_reinforce(&args[1..]),
        "dump" => simple::handle_dump(),
        "task" => task::handle_task(&args[1..]),
        "plan" => plan::handle_plan(&args[1..]),
        "personality" => personality::handle_personality(&args[1..]),
        _ => {
            print_memory_help();
            Ok(())
        }
    }
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
    println!("  legend memory query [options] <text>  Query memory (read-only retrieval)");
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
    println!("  legend memory personality       Distill recurring preferences/decisions/concerns");
    println!("  legend memory reset             Reset memory store");
}

// ---------------------------------------------------------------------------
// Tests (module-level tests that span multiple submodules)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::helpers::*;

    // ts_to_iso_date tests
    #[test]
    fn test_ts_to_iso_date_epoch() {
        assert_eq!(ts_to_iso_date(0), "1970-01-01");
    }

    #[test]
    fn test_ts_to_iso_date_known_date() {
        assert_eq!(ts_to_iso_date(1704067200), "2024-01-01");
    }

    #[test]
    fn test_ts_to_iso_date_leap_year() {
        assert_eq!(ts_to_iso_date(1709164800), "2024-02-29");
    }

    #[test]
    fn test_ts_to_iso_date_non_leap_year() {
        assert_eq!(ts_to_iso_date(1677628800), "2023-03-01");
    }

    #[test]
    fn test_ts_to_iso_date_year_boundary() {
        assert_eq!(ts_to_iso_date(1704067199), "2023-12-31");
    }

    // extract_keyword_directives tests
    #[test]
    fn test_keyword_directive_simple() {
        let (clean, directives) = extract_keyword_directives("KEYWORD:tool:bevy");
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].category, "tool");
        assert_eq!(directives[0].term, "bevy");
        assert!(clean.trim().is_empty());
    }

    #[test]
    fn test_keyword_directive_with_text() {
        let (clean, directives) =
            extract_keyword_directives("Added ECS framework\nKEYWORD:tool:bevy");
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].term, "bevy");
        assert_eq!(clean.trim(), "Added ECS framework");
    }

    #[test]
    fn test_keyword_directive_multiple() {
        let (clean, directives) = extract_keyword_directives(
            "KEYWORD:tool:prisma\nKEYWORD:environment:vercel\nSome text here",
        );
        assert_eq!(directives.len(), 2);
        assert_eq!(directives[0].category, "tool");
        assert_eq!(directives[0].term, "prisma");
        assert_eq!(directives[1].category, "environment");
        assert_eq!(directives[1].term, "vercel");
        assert_eq!(clean.trim(), "Some text here");
    }

    #[test]
    fn test_keyword_directive_none() {
        let (clean, directives) = extract_keyword_directives("Normal tick text with no directives");
        assert!(directives.is_empty());
        assert_eq!(clean, "Normal tick text with no directives");
    }

    #[test]
    fn test_keyword_directive_preserves_text() {
        let (clean, directives) =
            extract_keyword_directives("Line one\nKEYWORD:decision:tradeoff\nLine three");
        assert_eq!(directives.len(), 1);
        assert!(clean.contains("Line one"));
        assert!(clean.contains("Line three"));
        assert!(!clean.contains("KEYWORD"));
    }

    #[test]
    fn test_keyword_directive_category_normalized() {
        let (_, directives) = extract_keyword_directives("KEYWORD:TOOL:Redis");
        assert_eq!(directives[0].category, "tool");
        assert_eq!(directives[0].term, "Redis");
    }

    #[test]
    fn test_keyword_directive_invalid_ignored() {
        let (clean, directives) = extract_keyword_directives("KEYWORD:badformat");
        assert!(directives.is_empty());
        assert_eq!(clean, "KEYWORD:badformat");
    }

    #[test]
    fn test_keyword_directive_with_description() {
        let (clean, directives) =
            extract_keyword_directives("KEYWORD:tool:bevy Added ECS framework support");
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].term, "bevy");
        assert_eq!(directives[0].metadata, vec!["Added ECS framework support"]);
        assert!(clean.trim().is_empty());
    }
}
