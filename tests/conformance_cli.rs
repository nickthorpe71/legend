mod common;

use common::{seed_basic_repo, Harness};
use insta::assert_snapshot;

#[test]
fn help_output_is_stable() {
    let harness = Harness::new();
    let output = harness.cmd_ok(&["help"]);

    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        output.stderr
    );
    assert_snapshot!(
        &harness.normalize(&output.stdout),
        @r###"
        Legend - Lightweight context memory for AI-assisted development

        Usage: legend <command> [options]

        Commands:
          memory [start|tick|query|...] Hierarchical memory (context, decisions, history)
          discover [path]               Scan project to suggest features and context
          init                          Initialize Legend in a new project
          mcp-serve                     Run MCP stdio server for AI tool integration
          dashboard [--3d]              Launch TUI dashboard (--3d for Bevy 3D view)
          help                          Show this help message
        "###
    );
}

#[test]
fn init_first_run_creates_expected_files_and_repeat_is_stable() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    let first = harness.cmd_ok(&["init"]);
    let second = harness.cmd_ok(&["init"]);

    for path in [
        ".legend",
        ".gitattributes",
        ".claude/settings.json",
        ".codex/settings.json",
        ".gemini/settings.json",
        ".github/copilot-instructions.md",
        ".rules",
        ".cursorrules",
        "CLAUDE.md",
        "CODEX.md",
        "AGENTS.md",
        "GEMINI.md",
        ".mcp.json",
        ".cursor/mcp.json",
        ".codex/config.toml",
        ".vscode/mcp.json",
        ".zed/settings.json",
    ] {
        assert!(harness.exists(path), "expected {path} to exist");
    }

    assert!(first.stderr.contains("Ingesting README.md..."));
    assert!(first.stderr.contains("Ingesting Cargo.toml..."));
    assert!(
        second.stderr.is_empty(),
        "unexpected stderr: {}",
        second.stderr
    );

    assert_snapshot!(
        &harness.normalize(&first.stdout),
        @"
      First-time initialization: Ingesting high-signal context into memory...
    ✓ Project onboarding complete. High-signal context ingested into Legend.

    Next Steps for AI Agent:
    1. Run 'legend memory start' to see the ingested context.
    2. Use 'legend memory query' to explore specific modules or features.
    3. If significant architectural details are missing, manually 'tick' them.
    ✓ Seeded 296 keyword nodes into knowledge graph
      (288 innate + 8 workspace: 1 tools, 1 architecture, 0 domain, 6 code, 0 env)
    ✓ Initialized Legend
      Created .legend/ directory
    ✓ Configured local Git merge driver for Legend
    ✓ Updated .gitattributes with Legend merge driver rules
    ✓ Installed Git post-merge hook for Legend state recovery
    ✓ Created .claude/settings.json with Legend hooks
    ✓ Created .mcp.json with Legend MCP config
    ✓ Created CLAUDE.md with Legend instructions
    ✓ Created .codex/settings.json with Legend hooks
    ✓ Created .codex/config.toml with Legend MCP config
    ✓ Created CODEX.md with Legend instructions
    ✓ Created AGENTS.md with Legend instructions
    ✓ Created .github/copilot-instructions.md with Legend instructions
    ✓ Created .vscode/mcp.json with Legend MCP config
    ✓ Created .rules with Legend instructions
    ✓ Created .zed/settings.json with Legend MCP config
    ✓ Created .gemini/styleguide.md with Legend instructions
    ✓ Created GEMINI.md with Legend instructions
    ✓ Created .gemini/settings.json with Legend hooks
    ✓ Added Legend MCP to existing .gemini/settings.json (MCP)
    ✓ Created .cursorrules with Legend instructions
    ✓ Created .cursor/mcp.json with Legend MCP config
    "
    );

    let second_normalized = harness.normalize(&second.stdout);
    assert!(second_normalized.contains("Legend already initialized in this directory"));
    assert!(second_normalized.contains(".legend/ directory exists"));
    assert!(second_normalized.contains("Use 'legend memory start' to view current memory context"));
    assert!(
        second_normalized.contains("Memory store OK ("),
        "expected 'Memory store OK (' in output, got: {}",
        second_normalized
    );
    assert!(second_normalized.contains("✓ Updated Legend instructions in CODEX.md"));
    assert!(second_normalized.contains("✓ Updated Legend instructions in GEMINI.md"));
}
