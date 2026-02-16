use crate::commands::discover;
use crate::storage;
use crate::types::{Feature, LegendState};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

const LEGEND_MARKER_START: &str = "<!-- legend-start -->";
const LEGEND_MARKER_END: &str = "<!-- legend-end -->";

/// Initialize a new Legend project
///
/// Creates `.legend/` directory, auto-discovers features, sets up
/// Claude Code hooks, and generates CLAUDE.md instructions.
/// Safe to run multiple times - won't error if directory already exists.
pub fn handle_init() -> Result<(), Box<dyn std::error::Error>> {
    let legend_dir = Path::new(".legend");

    if storage::is_initialized() {
        println!("Legend already initialized in this directory");
        println!("  .legend/ directory exists");
        println!("  Use 'legend show' to view current state");

        setup_claude_hooks()?;
        setup_claude_md()?;
        setup_codex_hooks()?;
        setup_codex_md()?;
        setup_copilot_instructions()?;

        return Ok(());
    }

    fs::create_dir_all(legend_dir).map_err(|e| {
        format!("Failed to create .legend directory: {}", e)
    })?;

    let project_name = detect_project_name();
    let mut state = LegendState::new(project_name);

    match discover::run_discovery(Path::new(".")) {
        Ok(report) => {
            let count = report.potential_features.len();
            for suggested in report.potential_features {
                let mut feature = Feature::new(
                    suggested.suggested_id,
                    suggested.suggested_name,
                    suggested.suggested_domain,
                    format!("Auto-discovered from project structure ({} files)", suggested.files.len()),
                );
                feature.files_involved = suggested.files;
                state.add_feature(feature);
            }
            if count > 0 {
                println!("  Auto-discovered {} features from project structure", count);
            }
        }
        Err(_) => {}
    }

    storage::save_state(&state)?;

    println!("✓ Initialized Legend");
    println!("  Created .legend/ directory");
    println!("  Saved initial state to .legend/state.lz4");

    setup_claude_hooks()?;
    setup_claude_md()?;
    setup_codex_hooks()?;
    setup_codex_md()?;
    setup_copilot_instructions()?;

    Ok(())
}

/// Detect project name from the current directory name
fn detect_project_name() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "My Project".to_string())
}

/// Generate or update CLAUDE.md with Legend usage instructions
///
/// If CLAUDE.md doesn't exist, creates it with Legend instructions.
/// If it exists, appends Legend section (unless already present).
/// Uses marker comments for idempotent detection.
fn setup_claude_md() -> Result<(), Box<dyn std::error::Error>> {
    write_legend_markdown(Path::new("CLAUDE.md"), "CLAUDE.md", None)
}

/// Generate or update CODEX.md with Legend usage instructions
fn setup_codex_md() -> Result<(), Box<dyn std::error::Error>> {
    write_legend_markdown(Path::new("CODEX.md"), "CODEX.md", None)
}

/// Generate or update .github/copilot-instructions.md
///
/// This file is automatically injected into VS Code Copilot Chat sessions,
/// making Legend memory available without manual intervention.
fn setup_copilot_instructions() -> Result<(), Box<dyn std::error::Error>> {
    let github_dir = Path::new(".github");
    let instructions_path = github_dir.join("copilot-instructions.md");
    write_legend_markdown(&instructions_path, ".github/copilot-instructions.md", Some(github_dir))
}

// ---------------------------------------------------------------------------
// Shared instruction content & file I/O helpers
// ---------------------------------------------------------------------------

/// Returns the canonical Legend instructions that all agent instruction files share.
fn generate_legend_instructions() -> String {
    format!(
        r#"{LEGEND_MARKER_START}
# Legend — Your Long-Term Memory

You have access to a persistent hierarchical memory system called **Legend**. It stores context across sessions so you can pick up where you left off. **Use it actively and frequently.**

## CRITICAL: Memory Workflow

### On every session start
Run this FIRST before doing anything else:
```bash
cargo run --quiet -- memory query "session start context"
```
This returns your stored memories — read them to understand prior work, decisions, and open issues.

### During the session (frequently!)
After every significant action (writing code, making a decision, discovering something, completing a task), record it:
```bash
cargo run --quiet -- memory tick "description of what just happened"
```
Do this **every few minutes** or after every meaningful change. More ticks = better memory.

### On session end
Summarize what was accomplished:
```bash
cargo run --quiet -- memory tick "Session summary: what was done, what's next, any blockers"
```

## Memory Commands

| Command | When to Use |
|---------|-------------|
| `cargo run --quiet -- memory tick "<text>"` | Record something (decision, progress, discovery, blocker) |
| `cargo run --quiet -- memory query "<text>"` | Recall related context before starting work |
| `cargo run --quiet -- memory stats` | Check how much is stored |
| `cargo run --quiet -- memory consolidate` | Merge similar memories into long-term graph |

## Feature Tracking Commands

| Command | Purpose |
|---------|---------|
| `cargo run --quiet -- get_state` | Load full project state as JSON |
| `cargo run --quiet -- search <query>` | Search features by keyword |
| `cargo run --quiet -- show` | Human-readable feature summary |
| `cargo run --quiet -- update` | Update feature state (pipe JSON to stdin) |

## What to Tick

- **Decisions**: "Chose X over Y because Z"
- **Progress**: "Implemented feature X in file Y"
- **Blockers**: "Can't do X until Y is resolved"
- **Architecture**: "Module X talks to Y via Z"
- **User preferences**: "User prefers approach X"
- **Bugs found**: "Bug: X happens when Y"
- **TODO items**: "TODO: still need to implement X"

## About This Project

Legend is a brain-inspired hierarchical memory system for LLMs built in Rust. It has:
- **Immediate buffer**: recent text chunks
- **Short-term memory**: vector store with cosine similarity, salience scoring, exponential decay
- **Long-term memory**: knowledge graph with multi-hop traversal, Hebbian reinforcement

Storage: bincode + LZ4 at `.legend/memory.lz4`. Key source files:
- `src/memory/mod.rs` — core memory engine
- `src/commands/memory.rs` — CLI handler
- `src/main.rs` — command routing
- `src/commands/init.rs` — hook setup
{LEGEND_MARKER_END}"#,
        LEGEND_MARKER_START = LEGEND_MARKER_START,
        LEGEND_MARKER_END = LEGEND_MARKER_END,
    )
}

/// Write Legend instructions to a markdown file (create, append, or skip).
///
/// - `path`: the file to write
/// - `display_name`: human-readable name for log messages
/// - `parent_dir`: if Some, will be created with `create_dir_all` before writing
fn write_legend_markdown(
    path: &Path,
    display_name: &str,
    parent_dir: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let content = generate_legend_instructions();

    if path.exists() {
        let existing = fs::read_to_string(path).map_err(|e| {
            format!("Failed to read {}: {}", display_name, e)
        })?;

        if existing.contains(LEGEND_MARKER_START) {
            println!("  {} already has Legend instructions", display_name);
            return Ok(());
        }

        let updated = format!("{}\n\n{}\n", existing.trim_end(), content);
        fs::write(path, updated).map_err(|e| {
            format!("Failed to update {}: {}", display_name, e)
        })?;

        println!("✓ Appended Legend instructions to existing {}", display_name);
    } else {
        if let Some(dir) = parent_dir {
            fs::create_dir_all(dir).map_err(|e| {
                format!("Failed to create {} directory: {}", dir.display(), e)
            })?;
        }

        fs::write(path, format!("{}\n", content)).map_err(|e| {
            format!("Failed to create {}: {}", display_name, e)
        })?;

        println!("✓ Created {} with Legend instructions", display_name);
    }

    Ok(())
}

/// Set up Claude Code hooks in .claude/settings.json
///
/// Creates or merges Legend hooks into the project's Claude Code configuration.
/// Sets up three hooks:
/// - SessionStart: loads Legend state automatically
/// - UserPromptSubmit: reminds Claude to search Legend for context
/// - Stop: detects file changes and reminds Claude to update Legend
fn setup_claude_hooks() -> Result<(), Box<dyn std::error::Error>> {
    setup_agent_hooks(".claude", "Claude Code")
}

/// Set up Codex hooks in .codex/settings.json
fn setup_codex_hooks() -> Result<(), Box<dyn std::error::Error>> {
    setup_agent_hooks(".codex", "Codex")
}

/// Set up agent hooks in a settings.json for the given tool directory.
///
/// Creates or merges Legend hooks into the project's agent configuration.
/// Sets up three hooks:
/// - SessionStart: loads Legend state automatically
/// - UserPromptSubmit: reminds the agent to search Legend for context
/// - Stop: detects file changes and reminds the agent to update Legend
fn setup_agent_hooks(dir_name: &str, display_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let agent_dir = Path::new(dir_name);
    let settings_path = agent_dir.join("settings.json");

    let legend_session_hook = json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": "echo '== Legend Project Context =='; legend get_state 2>/dev/null || echo 'Legend state not found'"
        }]
    });

    let legend_prompt_hook = json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": "echo 'Reminder: If the user mentions a feature or topic, run legend search <keyword> first for context.'"
        }]
    });

    let legend_stop_hook = json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": "changed=$(git diff --name-only 2>/dev/null); if [ -n \"$changed\" ]; then echo \"Legend: Files changed since last commit:\n$changed\nUpdate Legend if you made progress: echo '{\\\"features\\\": [{\\\"id\\\": \\\"...\\\", \\\"status\\\": \\\"...\\\", \\\"files_involved\\\": [...]}]}' | legend update\"; fi"
        }]
    });

    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path).map_err(|e| {
            format!("Failed to read {}/settings.json: {}", dir_name, e)
        })?;

        let mut settings: Value = serde_json::from_str(&content).map_err(|e| {
            format!("Failed to parse {}/settings.json: {}", dir_name, e)
        })?;

        if has_legend_hooks(&settings) {
            println!("  {} hooks already configured", display_name);
            return Ok(());
        }

        merge_legend_hooks(
            &mut settings,
            &legend_session_hook,
            &legend_prompt_hook,
            &legend_stop_hook,
        );

        let output = serde_json::to_string_pretty(&settings)?;
        fs::write(&settings_path, output).map_err(|e| {
            format!("Failed to write {}/settings.json: {}", dir_name, e)
        })?;

        println!("✓ Added Legend hooks to existing {}/settings.json", dir_name);
    } else {
        fs::create_dir_all(agent_dir).map_err(|e| {
            format!("Failed to create {} directory: {}", dir_name, e)
        })?;

        let settings = json!({
            "hooks": {
                "SessionStart": [legend_session_hook],
                "UserPromptSubmit": [legend_prompt_hook],
                "Stop": [legend_stop_hook]
            }
        });

        let output = serde_json::to_string_pretty(&settings)?;
        fs::write(&settings_path, output).map_err(|e| {
            format!("Failed to write {}/settings.json: {}", dir_name, e)
        })?;

        println!("✓ Created {}/settings.json with Legend hooks", dir_name);
    }

    Ok(())
}

/// Check if Legend hooks are already configured
fn has_legend_hooks(settings: &Value) -> bool {
    if let Some(session_hooks) = settings
        .get("hooks")
        .and_then(|h| h.get("SessionStart"))
        .and_then(|s| s.as_array())
    {
        for hook_entry in session_hooks {
            if let Some(hooks) = hook_entry.get("hooks").and_then(|h| h.as_array()) {
                for hook in hooks {
                    if let Some(cmd) = hook.get("command").and_then(|c| c.as_str()) {
                        if cmd.contains("legend get_state") {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Merge Legend hooks into existing settings
fn merge_legend_hooks(
    settings: &mut Value,
    session_hook: &Value,
    prompt_hook: &Value,
    stop_hook: &Value,
) {
    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    }

    let hooks = settings.get_mut("hooks").unwrap();

    // Add SessionStart hook
    if hooks.get("SessionStart").is_none() {
        hooks["SessionStart"] = json!([]);
    }
    if let Some(arr) = hooks.get_mut("SessionStart").and_then(|s| s.as_array_mut()) {
        arr.push(session_hook.clone());
    }

    // Add UserPromptSubmit hook
    if hooks.get("UserPromptSubmit").is_none() {
        hooks["UserPromptSubmit"] = json!([]);
    }
    if let Some(arr) = hooks.get_mut("UserPromptSubmit").and_then(|s| s.as_array_mut()) {
        arr.push(prompt_hook.clone());
    }

    // Add Stop hook
    if hooks.get("Stop").is_none() {
        hooks["Stop"] = json!([]);
    }
    if let Some(arr) = hooks.get_mut("Stop").and_then(|s| s.as_array_mut()) {
        arr.push(stop_hook.clone());
    }
}
