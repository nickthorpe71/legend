// Init command - creates .legend directory and initializes state
//
// R* principle: Working code first
// Layer 3: Create directory structure
// Layer 4: Add serialization (bincode + LZ4) ✓
// Layer 11: Claude Code hooks setup ✓
// Layer 12: Self-bootstrapping (CLAUDE.md, auto-discover, Stop hook) ✓

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

    // Check if already initialized
    if storage::is_initialized() {
        println!("Legend already initialized in this directory");
        println!("  .legend/ directory exists");
        println!("  Use 'legend show' to view current state");
        return Ok(());
    }

    // Create .legend directory
    fs::create_dir_all(legend_dir).map_err(|e| {
        format!("Failed to create .legend directory: {}", e)
    })?;

    // Create initial state
    let project_name = detect_project_name();
    let mut state = LegendState::new(project_name);

    // Auto-discover features from the project directory
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
        Err(_) => {
            // Discovery is best-effort; don't fail init if it errors
        }
    }

    // Save the state to disk (bincode + LZ4)
    storage::save_state(&state)?;

    println!("✓ Initialized Legend");
    println!("  Created .legend/ directory");
    println!("  Saved initial state to .legend/state.lz4");

    // Set up Claude Code hooks in this project
    setup_claude_hooks()?;

    // Generate CLAUDE.md with Legend instructions
    setup_claude_md()?;

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
    let claude_md_path = Path::new("CLAUDE.md");

    let legend_section = format!(
        r#"{LEGEND_MARKER_START}
# Legend - Project Context Memory

This project uses **Legend** to track features, status, and file relationships across sessions. Legend is your memory — use it actively.

## Commands

| Command | Purpose |
|---------|---------|
| `legend get_state` | Load full project state as JSON |
| `legend search <query>` | Search features by keyword |
| `legend search --domain <d>` | Filter by domain |
| `legend search --status <s>` | Filter by status (Pending, InProgress, Blocked, Complete) |
| `legend search --tag <t>` | Filter by tag |
| `legend show` | Human-readable summary table |
| `legend update` | Update state (pipe JSON to stdin) |
| `legend discover` | Re-scan project for new features |

## When to Use Legend

- **Session start**: State is loaded automatically via hook. Read it to understand what exists.
- **User mentions a feature/topic**: Run `legend search <keyword>` before starting work to get context.
- **After making progress**: Update Legend immediately — don't wait for the user to ask. This includes status changes, new files, or new features.
- **New feature emerges**: Add it to Legend so future sessions have context.

## Update Format

Pipe JSON to `legend update`. For existing features, only `id` plus changed fields are needed:

```bash
echo '{{"features": [{{"id": "feature-id", "status": "InProgress", "files_involved": ["src/new_file.rs"]}}]}}' | legend update
```

For new features, include all required fields:

```bash
echo '{{"features": [{{"id": "new-feature", "name": "Feature Name", "domain": "api", "description": "What it does", "status": "InProgress", "tags": ["relevant"], "files_involved": ["src/file.rs"]}}]}}' | legend update
```

To remove features:

```bash
echo '{{"remove_features": ["old-feature-id"]}}' | legend update
```

## Status Values

- `Pending` — Not started
- `InProgress` — Currently being worked on
- `Blocked` — Waiting on something
- `Complete` — Done
{LEGEND_MARKER_END}"#,
        LEGEND_MARKER_START = LEGEND_MARKER_START,
        LEGEND_MARKER_END = LEGEND_MARKER_END,
    );

    if claude_md_path.exists() {
        let content = fs::read_to_string(claude_md_path).map_err(|e| {
            format!("Failed to read CLAUDE.md: {}", e)
        })?;

        // Check if Legend section already exists
        if content.contains(LEGEND_MARKER_START) {
            println!("  CLAUDE.md already has Legend instructions");
            return Ok(());
        }

        // Append Legend section to existing CLAUDE.md
        let updated = format!("{}\n\n{}\n", content.trim_end(), legend_section);
        fs::write(claude_md_path, updated).map_err(|e| {
            format!("Failed to update CLAUDE.md: {}", e)
        })?;

        println!("✓ Appended Legend instructions to existing CLAUDE.md");
    } else {
        fs::write(claude_md_path, format!("{}\n", legend_section)).map_err(|e| {
            format!("Failed to create CLAUDE.md: {}", e)
        })?;

        println!("✓ Created CLAUDE.md with Legend instructions");
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
    let claude_dir = Path::new(".claude");
    let settings_path = claude_dir.join("settings.json");

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
            format!("Failed to read .claude/settings.json: {}", e)
        })?;

        let mut settings: Value = serde_json::from_str(&content).map_err(|e| {
            format!("Failed to parse .claude/settings.json: {}", e)
        })?;

        if has_legend_hooks(&settings) {
            println!("  Claude Code hooks already configured");
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
            format!("Failed to write .claude/settings.json: {}", e)
        })?;

        println!("✓ Added Legend hooks to existing .claude/settings.json");
    } else {
        fs::create_dir_all(claude_dir).map_err(|e| {
            format!("Failed to create .claude directory: {}", e)
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
            format!("Failed to write .claude/settings.json: {}", e)
        })?;

        println!("✓ Created .claude/settings.json with Legend hooks");
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
