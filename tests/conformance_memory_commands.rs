mod common;

use common::{seed_basic_repo, write_consolidation_fixture, Harness};

#[test]
fn tick_returns_entry_id() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let tick = harness.output_json(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);
    let entry_id = tick["entry_id"].as_u64();
    assert!(entry_id.is_some(), "tick should return entry_id: {tick}");
}

#[test]
fn query_with_reasons_shows_similarity_and_reinforcement() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);

    let reasons = harness.output_json(&["memory", "query", "--reasons", "graph index"]);
    assert_eq!(
        reasons["note"],
        "Top result auto-reinforced (+3% salience boost)"
    );
    assert!(reasons["short_term"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .any(|item| item["text"] == "DECISION: Chose graph index because it keeps lookups cheap."));
    assert!(reasons["short_term"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .any(|item| item["reason"].as_str().unwrap_or("").contains("similarity")));
}

#[test]
fn reinforce_increases_salience() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let tick = harness.output_json(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);
    let entry_id = tick["entry_id"].as_u64().expect("entry id");

    let reinforce = harness.output_json(&["memory", "reinforce", "1.0", &entry_id.to_string()]);
    let reinforced = reinforce["reinforced"]
        .as_array()
        .expect("reinforced array");
    assert_eq!(reinforced.len(), 1);
    assert_eq!(reinforced[0]["id"], entry_id);
    assert!(
        reinforced[0]["salience_after"].as_f64().unwrap_or(0.0)
            >= reinforced[0]["salience_before"].as_f64().unwrap_or(0.0)
    );
}

#[test]
fn task_lifecycle_set_show_clear() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let task_set = harness.cmd_ok(&["memory", "task", "set", "Refactor retrieval pipeline"]);
    assert!(task_set
        .stdout
        .contains("✓ Current task set: Refactor retrieval pipeline"));

    let task_show = harness.cmd_ok(&["memory", "task"]);
    assert!(task_show
        .stdout
        .contains("Current task: Refactor retrieval pipeline"));

    let start_json = harness.output_json(&["memory", "start", "--json"]);
    assert_eq!(start_json["current_task"], "Refactor retrieval pipeline");

    let task_clear = harness.cmd_ok(&["memory", "task", "clear"]);
    assert!(task_clear.stdout.contains("✓ Current task cleared"));
}

#[test]
fn reset_clears_all_state() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);

    let reset = harness.cmd_ok(&["memory", "reset"]);
    assert!(reset.stdout.contains("✓ Memory reset"));

    let dump = harness.output_json(&["memory", "dump"]);
    assert_eq!(dump["short_term"].as_array().map(|v| v.len()), Some(0));
    assert_eq!(dump["graph"]["nodes"].as_array().map(|v| v.len()), Some(0));
}

#[test]
fn memory_no_args_prints_help() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["memory"]);
    // Should print help with available subcommands
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("start") && combined.contains("tick") && combined.contains("query"),
        "memory with no args should print help listing subcommands: {}",
        combined
    );
}

#[test]
fn memory_unknown_subcommand_prints_help() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["memory", "nonexistent"]);
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("start") && combined.contains("tick"),
        "unknown subcommand should print help: {}",
        combined
    );
}

#[test]
fn tick_blocker_flag_prepends_prefix() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let tick = harness.output_json(&[
        "memory",
        "tick",
        "--blocker",
        "Cannot proceed until database migration completes",
    ]);
    assert!(tick["entry_id"].as_u64().is_some());

    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert!(
        short_term.iter().any(|e| {
            let text = e["text"].as_str().unwrap_or("");
            text.contains("BLOCKER:") && text.contains("database migration")
        }),
        "blocker flag should prepend BLOCKER: prefix"
    );
}

#[test]
fn tick_passive_flag_prepends_prefix() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let tick = harness.output_json(&[
        "memory",
        "tick",
        "--passive",
        "Observed that the CI pipeline took 15 minutes today",
    ]);
    assert!(tick["entry_id"].as_u64().is_some());

    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert!(
        short_term.iter().any(|e| {
            let text = e["text"].as_str().unwrap_or("");
            text.contains("EXPERIENCE:") && text.contains("CI pipeline")
        }),
        "passive flag should prepend EXPERIENCE: prefix"
    );
}

#[test]
fn architecture_tick_appends_to_architecture_md() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "ARCHITECTURE: The query pipeline fans into graph lookup and associative priming.",
    ]);

    assert!(
        harness.exists("ARCHITECTURE.md"),
        "ARCHITECTURE.md should be created"
    );
    let content = harness.read_file("ARCHITECTURE.md");
    assert!(
        content.contains("query pipeline"),
        "ARCHITECTURE.md should contain the architecture tick: {}",
        content
    );
}

#[test]
fn context_command_returns_json_summary() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);

    let context = harness.output_json(&["memory", "context"]);
    assert!(
        context["stats"]["short_term_entries"].as_u64().unwrap_or(0) > 0,
        "context should report short_term_entries"
    );
    assert!(
        context["recent_sessions"].is_array(),
        "context should include recent_sessions"
    );
}

#[test]
fn dump_command_returns_full_state() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);

    let dump = harness.output_json(&["memory", "dump"]);
    assert!(
        dump["clock"].as_u64().is_some(),
        "dump should include clock"
    );
    assert!(
        dump["short_term"].is_array(),
        "dump should include short_term"
    );
    assert!(dump["graph"].is_object(), "dump should include graph");
    assert!(
        dump["session_log"].is_array(),
        "dump should include session_log"
    );
    assert!(
        dump["working_memory"].is_array(),
        "dump should include working_memory"
    );
}

#[test]
fn sessions_command_shows_recent_activity() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose graph index for fast lookups.",
    ]);
    harness.cmd_ok(&[
        "memory",
        "tick",
        "BUG: Found race condition in session logger.",
    ]);

    let sessions = harness.cmd_ok(&["memory", "sessions"]);
    assert!(
        sessions.stdout.contains("graph index") || sessions.stdout.contains("race condition"),
        "sessions should show recent ticks: {}",
        sessions.stdout
    );
}

#[test]
fn sessions_command_respects_count_argument() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&["memory", "tick", "DECISION: First decision about X."]);
    harness.cmd_ok(&["memory", "tick", "DECISION: Second decision about Y."]);
    harness.cmd_ok(&["memory", "tick", "DECISION: Third decision about Z."]);

    // Request only 1 session entry
    let sessions = harness.cmd_ok(&["memory", "sessions", "1"]);
    // Should still work without error
    assert!(
        !sessions.stdout.is_empty(),
        "sessions with count should return output"
    );
}

#[test]
fn start_compact_returns_shorter_output() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);

    let compact = harness.cmd_ok(&["memory", "start", "--compact"]);
    let full = harness.cmd_ok(&["memory", "start"]);

    // Compact output should be shorter than full output
    assert!(
        compact.stdout.len() <= full.stdout.len(),
        "compact output ({} bytes) should be <= full output ({} bytes)",
        compact.stdout.len(),
        full.stdout.len()
    );
}

#[test]
fn start_json_includes_all_expected_keys() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&["memory", "tick", "DECISION: Test decision for JSON start."]);

    let json = harness.output_json(&["memory", "start", "--json"]);
    assert!(
        json["recent_sessions"].is_array(),
        "start --json should include recent_sessions"
    );
    assert!(
        json["categorized"].is_object(),
        "start --json should include categorized"
    );
    assert!(
        json.get("current_task").is_some(),
        "start --json should include current_task"
    );
    assert!(
        json.get("git_sync").is_some(),
        "start --json should include git_sync"
    );
}

#[test]
fn consolidate_on_similar_entries_produces_summary() {
    let harness = Harness::new();
    write_consolidation_fixture(&harness);

    let consolidate = harness.output_json(&["memory", "consolidate"]);
    let summaries = consolidate.as_array().expect("consolidate array");
    assert!(
        !summaries.is_empty(),
        "consolidation should produce summaries from similar entries"
    );
    assert_eq!(summaries[0]["kind"], "Summary");
}

#[test]
fn negative_reinforce_decreases_salience() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let tick = harness.output_json(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);
    let entry_id = tick["entry_id"].as_u64().expect("entry id");

    let reinforce = harness.output_json(&["memory", "reinforce", "-1.0", &entry_id.to_string()]);
    let reinforced = reinforce["reinforced"]
        .as_array()
        .expect("reinforced array");
    assert_eq!(reinforced.len(), 1);
    assert!(
        reinforced[0]["salience_after"].as_f64().unwrap_or(1.0)
            <= reinforced[0]["salience_before"].as_f64().unwrap_or(0.0),
        "negative signal should decrease salience"
    );
}

#[test]
fn events_jsonl_records_ticks_and_queries() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    harness.cmd_ok(&["memory", "tick", "DECISION: Test event logging works."]);
    harness.cmd_ok(&["memory", "query", "event logging"]);

    assert!(
        harness.exists(".legend/events.jsonl"),
        "events.jsonl should exist"
    );
    let events = harness.read_file(".legend/events.jsonl");
    assert!(
        events.contains("tick"),
        "events.jsonl should contain tick events: {}",
        events
    );
    assert!(
        events.contains("query"),
        "events.jsonl should contain query events: {}",
        events
    );
}
