mod common;

use common::{seed_basic_repo, Harness};

#[test]
fn tick_rejects_empty_input() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["memory", "tick", ""]);
    assert!(
        !output.status.success(),
        "tick with empty string should fail"
    );
    assert!(
        output.stderr.contains("No input provided for tick"),
        "stderr should explain the error: {}",
        output.stderr
    );
}

#[test]
fn tick_rejects_no_argument() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // No argument at all — stdin is closed so text is empty
    let output = harness.cmd(&["memory", "tick"]);
    assert!(
        !output.status.success(),
        "tick with no argument should fail"
    );
}

#[test]
fn query_rejects_empty_input() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["memory", "query"]);
    assert!(
        !output.status.success(),
        "query with no argument should fail"
    );
    assert!(
        output.stderr.contains("Provide a query string"),
        "stderr should explain the error: {}",
        output.stderr
    );
}

#[test]
fn reinforce_rejects_invalid_signal() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["memory", "reinforce", "abc", "1"]);
    assert!(
        !output.status.success(),
        "reinforce with invalid signal should fail"
    );
    assert!(
        output.stderr.contains("Invalid signal"),
        "stderr should contain 'Invalid signal': {}",
        output.stderr
    );
}

#[test]
fn reinforce_rejects_missing_args() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["memory", "reinforce"]);
    assert!(
        !output.status.success(),
        "reinforce with no args should fail"
    );
    assert!(
        output.stderr.contains("Usage"),
        "stderr should contain usage instructions: {}",
        output.stderr
    );
}

#[test]
fn consolidate_on_empty_store_returns_empty_array() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Reset to get a truly empty store
    harness.cmd_ok(&["memory", "reset"]);

    let output = harness.cmd_ok(&["memory", "consolidate"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("parse consolidate output");
    assert_eq!(parsed, serde_json::json!([]));
}

#[test]
fn reinforce_rejects_invalid_entry_id() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["memory", "reinforce", "1.0", "not_a_number"]);
    assert!(
        !output.status.success(),
        "reinforce with non-integer ID should fail"
    );
    assert!(
        output.stderr.contains("Invalid entry ID"),
        "should mention invalid entry ID: {}",
        output.stderr
    );
}

#[test]
fn reinforce_with_only_signal_no_ids_returns_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["memory", "reinforce", "1.0"]);
    assert!(
        !output.status.success(),
        "reinforce with only signal (no IDs) should fail"
    );
    assert!(
        output.stderr.contains("Usage"),
        "should show usage: {}",
        output.stderr
    );
}

#[test]
fn unknown_top_level_command_returns_error() {
    let harness = Harness::new();

    let output = harness.cmd(&["nonexistent_command"]);
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("help") || combined.contains("Usage") || !output.status.success(),
        "unknown command should error or print help: {}",
        combined
    );
}

#[test]
fn help_command_succeeds() {
    let harness = Harness::new();

    let output = harness.cmd_ok(&["help"]);
    assert!(
        output.stdout.contains("Legend"),
        "help should mention Legend"
    );
    assert!(
        output.stdout.contains("memory"),
        "help should mention memory command"
    );
    assert!(
        output.stdout.contains("discover"),
        "help should mention discover command"
    );
    assert!(
        output.stdout.contains("init"),
        "help should mention init command"
    );
}
