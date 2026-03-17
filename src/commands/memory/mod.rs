mod consolidate;
mod event_log;
mod helpers;
mod query;
mod reinforce;
mod sessions;
mod simple;
mod start;
mod stats;
mod task;
mod tick;

use crate::cli::CommandDef;

// ---------------------------------------------------------------------------
// Public re-exports (keeps mcp.rs and other consumers working unchanged)
// ---------------------------------------------------------------------------

pub use event_log::{
    EventData, GraphHit, MatchedEntry, QueryEventData, StartEventData, TickEventData,
};
pub use event_log::{log_event, log_event_rich};
pub use helpers::{append_to_architecture_md, is_noise_tick, truncate_text};
pub use start::format_start_summary_markdown;

// ---------------------------------------------------------------------------
// Command definition
// ---------------------------------------------------------------------------

pub static COMMAND: CommandDef = CommandDef {
    name: "memory",
    about: "Hierarchical memory (context, decisions, history)",
    usage: "legend memory <subcommand> [options]",
    flags: &[],
    positionals: &[],
};

// ---------------------------------------------------------------------------
// Subcommand dispatch
// ---------------------------------------------------------------------------

pub fn handle_memory(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        print_memory_help();
        return Ok(());
    }

    match args[0].as_str() {
        "tick" => tick::handle_tick(&args[1..]),
        "query" => query::handle_query(&args[1..]),
        "start" => start::handle_start(&args[1..]),
        "stats" => stats::handle_stats(),
        "reset" => simple::handle_reset(),
        "consolidate" => consolidate::handle_consolidate(),
        "context" => simple::handle_context(),
        "sessions" => sessions::handle_sessions(&args[1..]),
        "reinforce" => reinforce::handle_reinforce(&args[1..]),
        "dump" => simple::handle_dump(),
        "task" => task::handle_task(&args[1..]),
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

// ---------------------------------------------------------------------------
// Tests (module-level tests that span multiple submodules)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::helpers::*;

    // is_noise_tick tests
    #[test]
    fn test_noise_tick_rejects_short() {
        assert!(is_noise_tick("hi"));
        assert!(is_noise_tick("         "));
        assert!(is_noise_tick("123456789"));
    }

    #[test]
    fn test_noise_tick_accepts_exactly_10_chars() {
        assert!(!is_noise_tick("1234567890"));
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
