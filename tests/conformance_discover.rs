mod common;

use common::{seed_basic_repo, Harness};
use serde_json::Value;

#[test]
fn discover_reports_repo_shape_without_modifying_memory() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    let output = harness.cmd_ok(&["discover"]);
    let report: Value = serde_json::from_str(&output.stdout).expect("discover json");

    assert_eq!(report["metadata"]["name"], "fixture-app");
    assert!(report["total_files"].as_u64().unwrap_or(0) >= 3);
    assert_eq!(report["languages"]["rs"], 1);
    assert!(
        report["metadata"]["tech_stack"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .any(|item| item == "Rust")
    );
    assert!(
        report["high_signal_files"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .any(|f| f["path"] == "README.md")
    );
    assert!(
        output
            .stderr
            .contains("Tip: Run 'legend init' or 'legend discover --apply' to start tracking this project.")
    );
    assert!(!harness.exists(".legend/memory.lz4"));
}

#[test]
fn discover_apply_ingests_context_into_memory() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    let output = harness.cmd_ok(&["discover", "--apply"]);
    assert!(output.stdout.contains("✓ Project onboarding complete."));
    assert!(output.stdout.contains("Run 'legend memory start'"));
    assert!(output.stderr.contains("Ingesting README.md..."));
    assert!(output.stderr.contains("Ingesting Cargo.toml..."));

    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert!(
        short_term
            .iter()
            .any(|entry| entry["text"].as_str().unwrap_or("").contains("ONBOARDING: Discovery report for fixture-app"))
    );
    assert!(
        short_term
            .iter()
            .any(|entry| entry["text"].as_str().unwrap_or("").contains("CONTEXT: High-signal file 'README.md'"))
    );
    assert!(
        short_term
            .iter()
            .any(|entry| entry["text"].as_str().unwrap_or("").contains("ONBOARDING TASKS:"))
    );
}

#[test]
fn discover_detects_javascript_project() {
    let harness = Harness::new();

    // Create a JS project instead of Rust
    harness.write_file(
        "package.json",
        r#"{"name": "my-js-app", "version": "1.0.0", "description": "A JS test app"}"#,
    );
    harness.write_file("src/index.js", "console.log('hello');\n");
    harness.write_file("README.md", "# My JS App\n");

    let output = harness.cmd_ok(&["discover"]);
    let report: Value = serde_json::from_str(&output.stdout).expect("discover json");

    assert_eq!(report["metadata"]["name"], "my-js-app");
    assert!(
        report["metadata"]["tech_stack"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .any(|item| item.as_str().unwrap_or("").contains("JavaScript")),
        "should detect JavaScript/TypeScript: {:?}",
        report["metadata"]["tech_stack"]
    );
    assert_eq!(report["languages"]["js"], 1);
}

#[test]
fn discover_with_custom_path_scans_subdirectory() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    // Create a subdirectory with its own project
    harness.write_file("subdir/README.md", "# Sub Project\n");
    harness.write_file("subdir/main.py", "print('hello')\n");

    let output = harness.cmd_ok(&["discover", "subdir"]);
    let report: Value = serde_json::from_str(&output.stdout).expect("discover json");

    assert!(
        report["total_files"].as_u64().unwrap_or(0) >= 2,
        "should find files in subdirectory"
    );
}

#[test]
fn discover_includes_entry_points_in_high_signal() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    let output = harness.cmd_ok(&["discover"]);
    let report: Value = serde_json::from_str(&output.stdout).expect("discover json");

    let high_signal = report["high_signal_files"]
        .as_array()
        .expect("high_signal_files");
    // Should include main.rs as entry point and Cargo.toml as manifest
    assert!(
        high_signal
            .iter()
            .any(|f| f["path"].as_str().unwrap_or("").contains("main.rs")),
        "should detect main.rs as high-signal entry point: {:?}",
        high_signal
    );
    assert!(
        high_signal
            .iter()
            .any(|f| f["path"] == "Cargo.toml"),
        "should detect Cargo.toml as high-signal manifest"
    );
}

#[test]
fn discover_generates_tasks() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    let output = harness.cmd_ok(&["discover"]);
    let report: Value = serde_json::from_str(&output.stdout).expect("discover json");

    let tasks = report["tasks"].as_array().expect("tasks array");
    assert!(
        !tasks.is_empty(),
        "discover should generate investigation tasks"
    );
    // Should have a read_readme task since README.md exists
    assert!(
        tasks.iter().any(|t| t["id"] == "read_readme"),
        "should generate read_readme task: {:?}",
        tasks
    );
}

#[test]
fn discover_stderr_shows_summary_info() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    let output = harness.cmd_ok(&["discover"]);
    assert!(
        output.stderr.contains("Discovered"),
        "stderr should contain 'Discovered': {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("Project:"),
        "stderr should contain project name: {}",
        output.stderr
    );
}
