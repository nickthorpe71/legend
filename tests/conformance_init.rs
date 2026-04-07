mod common;

use common::{seed_basic_repo, Harness};

#[test]
fn init_help_flag_prints_usage() {
    let harness = Harness::new();

    let output = harness.cmd_ok(&["init", "--help"]);
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("init") || combined.contains("Initialize"),
        "init --help should print usage info: {}",
        combined
    );
}

#[test]
fn init_creates_gitattributes_with_merge_driver_rules() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let content = harness.read_file(".gitattributes");
    assert!(
        content.contains(".legend/memory.lz4 merge=legend"),
        ".gitattributes should contain memory.lz4 merge rule: {}",
        content
    );
    assert!(
        content.contains(".legend/events.jsonl merge=legend"),
        ".gitattributes should contain events.jsonl merge rule: {}",
        content
    );
}

#[test]
fn init_reinit_updates_markdown_with_markers() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Verify CLAUDE.md has legend markers
    let claude_md = harness.read_file("CLAUDE.md");
    assert!(
        claude_md.contains("<!-- legend-start -->"),
        "CLAUDE.md should contain legend-start marker"
    );
    assert!(
        claude_md.contains("<!-- legend-end -->"),
        "CLAUDE.md should contain legend-end marker"
    );

    // Re-init should update without duplicating
    harness.cmd_ok(&["init"]);
    let claude_md_after = harness.read_file("CLAUDE.md");

    let start_count = claude_md_after.matches("<!-- legend-start -->").count();
    assert_eq!(
        start_count, 1,
        "re-init should not duplicate legend markers, found {}",
        start_count
    );
}

#[test]
fn init_preserves_existing_content_in_markdown_files() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    // Write a CLAUDE.md with user content before init
    harness.write_file("CLAUDE.md", "# My Project\n\nCustom instructions here.\n");

    harness.cmd_ok(&["init"]);

    let content = harness.read_file("CLAUDE.md");
    assert!(
        content.contains("# My Project"),
        "init should preserve existing content before legend section"
    );
    assert!(
        content.contains("Custom instructions here."),
        "init should preserve existing custom instructions"
    );
    assert!(
        content.contains("<!-- legend-start -->"),
        "init should append legend section"
    );
}

#[test]
fn init_updates_existing_legend_section_in_markdown() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    // Write a CLAUDE.md with user content + old legend section
    harness.write_file(
        "CLAUDE.md",
        "# My Project\n\nCustom instructions.\n\n<!-- legend-start -->\nOld legend content\n<!-- legend-end -->\n",
    );

    harness.cmd_ok(&["init"]);

    let content = harness.read_file("CLAUDE.md");
    assert!(
        content.contains("# My Project"),
        "should preserve user content"
    );
    assert!(
        content.contains("Custom instructions."),
        "should preserve user content"
    );
    assert!(
        !content.contains("Old legend content"),
        "should replace old legend content"
    );
    assert!(
        content.contains("<!-- legend-start -->"),
        "should have legend markers"
    );
    assert!(
        content.contains("memory start"),
        "should have updated legend instructions"
    );
}

#[test]
fn init_creates_settings_json_with_hooks() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let claude_settings = harness.read_file(".claude/settings.json");
    let parsed: serde_json::Value =
        serde_json::from_str(&claude_settings).expect("parse claude settings.json");
    assert!(
        parsed["hooks"].is_object(),
        "settings.json should contain hooks section"
    );
}

#[test]
fn init_reinit_preserves_existing_hooks() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    // Create a settings.json with custom hooks before init
    harness.write_file(
        ".claude/settings.json",
        r#"{"hooks":{"SessionStart":[{"matcher":"","hooks":[{"type":"command","command":"echo custom-hook"}]}]}}"#,
    );

    harness.cmd_ok(&["init"]);

    let settings = harness.read_file(".claude/settings.json");
    let parsed: serde_json::Value = serde_json::from_str(&settings).expect("parse settings.json");
    // Should have hooks (Legend hooks added)
    assert!(
        parsed["hooks"].is_object(),
        "settings.json should have hooks"
    );
}

#[test]
fn init_creates_mcp_json_configs() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Check .mcp.json
    let mcp = harness.read_file(".mcp.json");
    let parsed: serde_json::Value = serde_json::from_str(&mcp).expect("parse .mcp.json");
    assert!(
        parsed["mcpServers"]["legend-memory"].is_object(),
        ".mcp.json should have legend-memory server config"
    );

    // Check .vscode/mcp.json
    let vscode_mcp = harness.read_file(".vscode/mcp.json");
    let parsed: serde_json::Value =
        serde_json::from_str(&vscode_mcp).expect("parse .vscode/mcp.json");
    assert!(
        parsed["servers"]["legend-memory"].is_object(),
        ".vscode/mcp.json should have legend-memory config"
    );

    // Check .cursor/mcp.json
    let cursor_mcp = harness.read_file(".cursor/mcp.json");
    let parsed: serde_json::Value =
        serde_json::from_str(&cursor_mcp).expect("parse .cursor/mcp.json");
    assert!(
        parsed["mcpServers"]["legend-memory"].is_object(),
        ".cursor/mcp.json should have legend-memory config"
    );

    // Check .codex/config.toml
    let codex_toml = harness.read_file(".codex/config.toml");
    assert!(
        codex_toml.contains("legend-memory"),
        ".codex/config.toml should contain legend-memory section"
    );

    // Check .zed/settings.json (Zed uses "context_servers" key)
    let zed_settings = harness.read_file(".zed/settings.json");
    let parsed: serde_json::Value =
        serde_json::from_str(&zed_settings).expect("parse .zed/settings.json");
    assert!(
        parsed["context_servers"]["legend-memory"].is_object(),
        ".zed/settings.json should have legend-memory config: {}",
        zed_settings
    );
}

#[test]
fn init_mcp_configs_are_idempotent() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Read MCP config after first init
    let _mcp_first = harness.read_file(".mcp.json");

    // Re-init
    let second = harness.cmd_ok(&["init"]);

    // MCP config should not be duplicated
    let mcp_second = harness.read_file(".mcp.json");
    let parsed: serde_json::Value = serde_json::from_str(&mcp_second).expect("parse .mcp.json");
    // Should still have exactly one legend-memory entry
    assert!(parsed["mcpServers"]["legend-memory"].is_object());

    // Check stdout mentions already configured
    assert!(
        second.stdout.contains("already") || second.stdout.contains("MCP"),
        "re-init should mention MCP is already configured"
    );
}

#[test]
fn init_discover_flag_rescans_project() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Re-init with --discover
    let output = harness.cmd_ok(&["init", "--discover"]);
    assert!(
        output.stdout.contains("Re-scanning") || output.stdout.contains("already"),
        "init --discover should mention re-scanning: {}",
        output.stdout
    );
}

#[test]
fn init_gitattributes_cleans_old_rules_on_reinit() {
    let harness = Harness::new();
    seed_basic_repo(&harness);

    // Write a .gitattributes with old-format legend rules
    harness.write_file(
        ".gitattributes",
        "*.txt text\n.legend/*.lz4 merge=legend\n.legend/state.lz4 merge=legend\n",
    );

    harness.cmd_ok(&["init"]);

    let content = harness.read_file(".gitattributes");
    assert!(
        content.contains("*.txt text"),
        "should preserve non-legend rules"
    );
    assert!(
        !content.contains(".legend/*.lz4 merge=legend"),
        "should remove old wildcard rule"
    );
    assert!(
        !content.contains("state.lz4"),
        "should remove deprecated state.lz4 rule"
    );
    assert!(
        content.contains(".legend/memory.lz4 merge=legend"),
        "should have current memory.lz4 rule"
    );
}

#[test]
fn init_creates_all_instruction_files() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Check each instruction file exists and contains Legend content
    for (path, keyword) in [
        ("CLAUDE.md", "memory start"),
        ("CODEX.md", "memory start"),
        ("AGENTS.md", "memory start"),
        ("GEMINI.md", "memory start"),
        (".rules", "memory start"),
        (".cursorrules", "memory start"),
        (".github/copilot-instructions.md", "memory start"),
    ] {
        assert!(harness.exists(path), "{} should exist", path);
        let content = harness.read_file(path);
        assert!(
            content.contains(keyword),
            "{} should contain '{}': {}",
            path,
            keyword,
            &content[..content.len().min(200)]
        );
    }
}

#[test]
fn init_memory_store_migration_on_reinit() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Tick some memories
    harness.cmd_ok(&["memory", "tick", "DECISION: Chose X over Y because Z."]);

    // Re-init should report memory store status
    let output = harness.cmd_ok(&["init"]);
    assert!(
        output.stdout.contains("Memory store OK"),
        "re-init should report memory store OK: {}",
        output.stdout
    );
}
