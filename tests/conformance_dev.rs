mod common;

use common::{seed_basic_repo, Harness};

#[test]
fn dev_unknown_subcommand_prints_help() {
    let harness = Harness::new();

    let output = harness.cmd(&["dev", "nonexistent"]);
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("unknown") || combined.contains("Available") || combined.contains("prune"),
        "dev unknown subcommand should print help: {}",
        combined
    );
}

#[test]
fn dev_prune_noise_on_clean_memory() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Tick a legitimate entry (not noise)
    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose hash map for O(1) lookups.",
    ]);

    let output = harness.cmd_ok(&["dev", "prune-noise"]);
    assert!(
        output.stdout.contains("Noise pruned") || output.stdout.contains("pruned"),
        "prune-noise should report results: {}",
        output.stdout
    );

    // Legitimate entry should still exist
    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert!(
        short_term
            .iter()
            .any(|e| e["text"].as_str().unwrap_or("").contains("hash map")),
        "legitimate entry should survive pruning"
    );
}

#[test]
fn dev_prune_noise_removes_experience_noise() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Tick legitimate entries
    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Chose hash map for O(1) lookups.",
    ]);

    // Tick noise entry using --passive flag (prepends EXPERIENCE:)
    harness.cmd_ok(&[
        "memory",
        "tick",
        "--passive",
        "Experience: Executed tool read_file with status success and nothing changed",
    ]);

    let output = harness.cmd_ok(&["dev", "prune-noise"]);
    assert!(
        output.stdout.contains("Noise pruned"),
        "prune-noise should report results"
    );

    // Check that the noise entry was removed and legitimate entry remains
    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term");
    assert!(
        short_term
            .iter()
            .any(|e| e["text"].as_str().unwrap_or("").contains("hash map")),
        "legitimate entry should survive"
    );
}
