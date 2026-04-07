mod common;

use common::{seed_basic_repo, write_consolidation_fixture, Harness};
use serde_json::json;

#[test]
fn start_query_context_and_dump_preserve_core_workflow_behavior() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);
    harness.cmd_ok(&[
        "memory",
        "tick",
        "ARCHITECTURE: Query pipeline fans into graph lookup and associative priming.",
    ]);
    harness.cmd_ok(&[
        "memory",
        "tick",
        "BUG: UTF-8 truncation panicked on multi-byte text before floor_char_boundary fix.",
    ]);

    let start = harness.cmd_ok(&["memory", "start"]);
    let query = harness.output_json(&["memory", "query", "architecture"]);
    let context = harness.output_json(&["memory", "context"]);
    let dump = harness.output_json(&["memory", "dump"]);

    let normalized_start = harness.normalize(&start.stdout);
    assert!(normalized_start.contains("# Legend Memory Context"));
    assert!(normalized_start.contains("## Recent Activity"));
    assert!(normalized_start.contains("## Categorized Memories"));
    assert!(
        normalized_start.contains("DECISION: Chose graph index because it keeps lookups cheap.")
    );
    assert!(normalized_start
        .contains("ARCHITECTURE: Query pipeline fans into graph lookup and associative priming."));
    assert!(normalized_start.contains(
        "BUG: UTF-8 truncation panicked on multi-byte text before floor_char_boundary fix."
    ));
    assert!(normalized_start.contains("*Use heredoc for tick/query:"));

    let query_projection = json!({
        "memories": query["memories"],
        "related_topics_len": query["related_topics"].as_array().map(|v| v.len()).unwrap_or(0),
    });
    insta::assert_json_snapshot!(
        query_projection,
        @r#"
    {
      "memories": [
        "ARCHITECTURE: Query pipeline fans into graph lookup and associative priming.",
        "BUG: UTF-8 truncation panicked on multi-byte text before floor_char_boundary fix."
      ],
      "related_topics_len": 15
    }
    "#
    );

    let recent_sessions = context["recent_sessions"]
        .as_array()
        .expect("recent_sessions array");
    assert!(recent_sessions
        .iter()
        .any(|entry| entry == "DECISION: Chose graph index because it keeps lookups cheap."));
    assert!(recent_sessions.iter().any(|entry| {
        entry == "ARCHITECTURE: Query pipeline fans into graph lookup and associative priming."
    }));
    assert!(recent_sessions.iter().any(|entry| {
        entry == "BUG: UTF-8 truncation panicked on multi-byte text before floor_char_boundary fix."
    }));

    assert!(context["stats"]["short_term_entries"].as_u64().unwrap_or(0) >= 4);
    assert!(
        context["stats"]["session_log_entries"]
            .as_u64()
            .unwrap_or(0)
            >= 4
    );
    assert!(context["stats"]["long_term_nodes"].as_u64().unwrap_or(0) > 0);

    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert!(short_term.iter().any(
        |entry| entry["text"] == "DECISION: Chose graph index because it keeps lookups cheap."
    ));
    assert!(short_term.iter().any(|entry| entry["text"]
        == "ARCHITECTURE: Query pipeline fans into graph lookup and associative priming."));
    assert!(short_term.iter().any(|entry| entry["text"]
        == "BUG: UTF-8 truncation panicked on multi-byte text before floor_char_boundary fix."));
    assert!(dump["session_log"].as_array().map(|v| v.len()).unwrap_or(0) >= 4);
}

#[test]
fn stats_and_sessions_reflect_recent_activity() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);
    harness.cmd_ok(&[
        "memory",
        "tick",
        "BUG: UTF-8 truncation panicked on multi-byte text before floor_char_boundary fix.",
    ]);
    harness.cmd_ok(&["memory", "start"]);
    harness.cmd_ok(&["memory", "query", "graph index"]);

    let stats = harness.cmd_ok(&["memory", "stats"]);
    let sessions = harness.cmd_ok(&["memory", "sessions"]);
    let normalized_sessions = harness.normalize(&sessions.stdout);

    assert!(stats.stdout.contains("Memory stats:"));
    assert!(stats.stdout.contains("Working memory (L1):"));
    assert!(stats.stdout.contains("Short-term entries:"));
    assert!(stats.stdout.contains("Session quality (current session):"));
    assert!(stats.stdout.contains("Meaningful ticks: 2 (100%)"));
    assert!(stats.stdout.contains("Queries:          1"));

    assert!(normalized_sessions.contains("ONBOARDING:"));
    assert!(
        normalized_sessions.contains("DECISION: Chose graph index because it keeps lookups cheap.")
    );
    assert!(normalized_sessions.contains(
        "BUG: UTF-8 truncation panicked on multi-byte text before floor_char_boundary fix."
    ));
}

#[test]
fn consolidate_on_seeded_store_creates_summary_output() {
    let harness = Harness::new();
    write_consolidation_fixture(&harness);

    let consolidate = harness.output_json(&["memory", "consolidate"]);
    let dump = harness.output_json(&["memory", "dump"]);

    let summaries = consolidate.as_array().expect("consolidate output array");
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0]["kind"], "Summary");
    assert_eq!(
        summaries[0]["source_texts"].as_array().map(|v| v.len()),
        Some(2)
    );
    assert!(summaries[0]["label"]
        .as_str()
        .expect("summary label")
        .contains("graph lookup"));

    let graph_nodes = dump["graph"]["nodes"]
        .as_array()
        .expect("graph nodes array");
    assert!(graph_nodes.iter().any(|node| node["kind"] == "Summary"));
}
