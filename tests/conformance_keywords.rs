mod common;

use common::{seed_basic_repo, Harness};

#[test]
fn init_seeds_keyword_graph_nodes() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    let output = harness.cmd_ok(&["init"]);
    assert!(
        output.stdout.contains("Seeded") && output.stdout.contains("keyword nodes"),
        "Expected keyword seeding message in init output, got: {}",
        output.stdout
    );
}

#[test]
fn init_seeds_language_specific_keywords_for_rust_project() {
    let harness = Harness::new();
    seed_basic_repo(&harness); // seeds Cargo.toml → Rust project

    let output = harness.cmd_ok(&["init"]);
    // Should seed Rust-specific code keywords (more than just universal)
    assert!(
        output.stdout.contains("Seeded"),
        "Expected keyword seeding in init output"
    );

    // Verify stats show graph nodes were created (keywords are graph nodes)
    let stats = harness.cmd_ok(&["memory", "stats"]);
    let nodes_line = stats.stdout.lines().find(|l| l.contains("Long-term nodes"));
    assert!(
        nodes_line.is_some(),
        "Expected long-term nodes in stats output"
    );
    // Keyword nodes + any entity nodes from onboarding should be > 0
    let node_count: usize = nodes_line
        .unwrap()
        .split(':')
        .last()
        .unwrap()
        .trim()
        .parse()
        .unwrap_or(0);
    assert!(
        node_count > 50,
        "Expected keyword nodes seeded, got only {node_count}"
    );
}

#[test]
fn reinit_migrates_keywords_for_existing_project_without_keyword_nodes() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    // First init seeds keywords
    harness.cmd_ok(&["init"]);

    // Second init should not re-seed (keywords already exist)
    let second = harness.cmd_ok(&["init"]);
    assert!(
        !second.stdout.contains("Seeded"),
        "Re-init should not re-seed when keyword nodes already exist"
    );
}

#[test]
fn keyword_tick_creates_graph_node() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Tick with KEYWORD: prefix
    let output = harness.cmd_ok(&["memory", "tick", "KEYWORD:tool:prisma"]);
    // Should output keyword-only action
    assert!(
        output.stdout.contains("keyword_only") || output.stderr.contains("keyword registered"),
        "Expected keyword registration output, got stdout: {} stderr: {}",
        output.stdout,
        output.stderr
    );
}

#[test]
fn tick_with_text_and_keyword_creates_both_memory_and_keyword() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Tick with both text and KEYWORD: directive
    let tick_text = "Added Prisma ORM\nKEYWORD:tool:prisma";
    let output = harness.cmd_ok(&["memory", "tick", tick_text]);

    // Should have the tick result (memory entry created)
    assert!(
        output.stdout.contains("\"action\""),
        "Expected JSON tick result, got: {}",
        output.stdout
    );
    // Should have registered keyword on stderr
    assert!(
        output.stderr.contains("keyword registered: kw:tool:prisma"),
        "Expected keyword registration on stderr, got: {}",
        output.stderr
    );
}

#[test]
fn discover_shows_deprecation_notice() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    // discover should show deprecation notice
    let output = harness.cmd(&["discover"]);
    assert!(
        output.stderr.contains("deprecated") || output.stderr.contains("Deprecated"),
        "Expected deprecation notice from discover, got stderr: {}",
        output.stderr
    );
}
