// Init command - creates .legend directory and initializes state
//
// R* principle: Working code first
// Layer 3: Create directory structure
// Layer 4: Add serialization (bincode + LZ4) ✓
// Layer 11: Claude Code hooks setup ✓

use crate::storage;
use crate::types::LegendState;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

/// Initialize a new Legend project
///
/// Creates `.legend/` directory and sets up initial state structure.
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
    // R* principle: Add context to errors - tell user what failed
    fs::create_dir_all(legend_dir).map_err(|e| {
        format!("Failed to create .legend directory: {}", e)
    })?;

    // Create initial state
    // For now, we'll use a default project name
    // Later (Layer 6), we can accept --name flag or detect from git
    let project_name = "My Project".to_string();
    let state = LegendState::new(project_name);

    // Save the initial state to disk (bincode + LZ4)
    // This serializes and compresses the state
    storage::save_state(&state)?;

    println!("✓ Initialized Legend");
    println!("  Created .legend/ directory");
    println!("  Saved initial state to .legend/state.lz4");

    // Set up Claude Code hooks in this project
    setup_claude_hooks()?;

    Ok(())
}

/// Set up Claude Code hooks in .claude/settings.json
///
/// Creates or merges Legend hooks into the project's Claude Code configuration.
/// Handles edge cases: existing settings, existing hooks, permission errors.
fn setup_claude_hooks() -> Result<(), Box<dyn std::error::Error>> {
    let claude_dir = Path::new(".claude");
    let settings_path = claude_dir.join("settings.json");

    // The Legend hooks configuration
    let legend_session_hook = json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": "echo '== Legend Context =='; legend get_state 2>/dev/null || echo 'Legend state not found'"
        }]
    });

    let legend_prompt_hook = json!({
        "matcher": "",
        "hooks": [{
            "type": "command",
            "command": "echo '{\"additionalContext\": \"Legend available. Use legend search <keyword>, legend get_state, or pipe JSON to legend update.\"}'"
        }]
    });

    // Check if settings.json exists
    if settings_path.exists() {
        // Read and parse existing settings
        let content = fs::read_to_string(&settings_path).map_err(|e| {
            format!("Failed to read .claude/settings.json: {}", e)
        })?;

        let mut settings: Value = serde_json::from_str(&content).map_err(|e| {
            format!("Failed to parse .claude/settings.json: {}", e)
        })?;

        // Check if Legend hooks already exist
        if has_legend_hooks(&settings) {
            println!("  Claude Code hooks already configured");
            return Ok(());
        }

        // Merge Legend hooks into existing settings
        merge_legend_hooks(&mut settings, &legend_session_hook, &legend_prompt_hook);

        // Write back the merged settings
        let output = serde_json::to_string_pretty(&settings)?;
        fs::write(&settings_path, output).map_err(|e| {
            format!("Failed to write .claude/settings.json: {}", e)
        })?;

        println!("✓ Added Legend hooks to existing .claude/settings.json");
    } else {
        // Create .claude directory if it doesn't exist
        fs::create_dir_all(claude_dir).map_err(|e| {
            format!("Failed to create .claude directory: {}", e)
        })?;

        // Create new settings.json with Legend hooks
        let settings = json!({
            "hooks": {
                "SessionStart": [legend_session_hook],
                "UserPromptSubmit": [legend_prompt_hook]
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
    // Check SessionStart hooks for legend get_state
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
fn merge_legend_hooks(settings: &mut Value, session_hook: &Value, prompt_hook: &Value) {
    // Ensure hooks object exists
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
}
