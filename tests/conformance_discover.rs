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
