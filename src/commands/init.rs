use crate::commands::discover;
use crate::memory::MemoryState;
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

        // Migrate/refresh memory store to latest format
        migrate_memory_store();

        setup_claude_hooks()?;
        setup_claude_md()?;
        setup_codex_hooks()?;
        setup_codex_md()?;
        setup_copilot_instructions()?;
        setup_zed_rules()?;
        setup_gemini_styleguide()?;
        setup_gemini_md()?;
        setup_gemini_hooks()?;

        return Ok(());
    }

    fs::create_dir_all(legend_dir)
        .map_err(|e| format!("Failed to create .legend directory: {}", e))?;

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
                    format!(
                        "Auto-discovered from project structure ({} files)",
                        suggested.files.len()
                    ),
                );
                feature.files_involved = suggested.files;
                state.add_feature(feature);
            }
            if count > 0 {
                println!(
                    "  Auto-discovered {} features from project structure",
                    count
                );
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
    setup_zed_rules()?;
    setup_gemini_styleguide()?;
    setup_gemini_md()?;
    setup_gemini_hooks()?;

    Ok(())
}

/// Migrate memory store to latest format.
///
/// Loads memory (triggering any pending migrations), then re-saves it
/// in the current format. This ensures old memory files are upgraded.
fn migrate_memory_store() {
    match MemoryState::load_or_default() {
        Ok(state) => {
            let entries = state.short_term.len();
            let nodes = state.long_term.nodes.len();

            if let Err(e) = state.save() {
                eprintln!("  Warning: failed to save memory store: {}", e);
            } else if entries > 0 || nodes > 0 {
                println!("  Memory store OK ({} entries, {} graph nodes)", entries, nodes);
            }
        }
        Err(e) => {
            eprintln!("  Warning: failed to load memory store: {}", e);
        }
    }
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
    write_legend_markdown(
        &instructions_path,
        ".github/copilot-instructions.md",
        Some(github_dir),
    )
}

/// Generate or update .rules with Legend usage instructions
///
/// Zed editor reads .rules files for AI agent instructions.
/// Also supports CLAUDE.md and GEMINI.md which we create separately.
fn setup_zed_rules() -> Result<(), Box<dyn std::error::Error>> {
    write_legend_markdown(Path::new(".rules"), ".rules", None)
}

/// Generate or update .gemini/styleguide.md with Legend usage instructions
///
/// Gemini Code Assist reads styleguide.md from .gemini/ directory.
fn setup_gemini_styleguide() -> Result<(), Box<dyn std::error::Error>> {
    let gemini_dir = Path::new(".gemini");
    let styleguide_path = gemini_dir.join("styleguide.md");
    write_legend_markdown(&styleguide_path, ".gemini/styleguide.md", Some(gemini_dir))
}

/// Generate or update GEMINI.md with Legend usage instructions
fn setup_gemini_md() -> Result<(), Box<dyn std::error::Error>> {
    write_legend_markdown(Path::new("GEMINI.md"), "GEMINI.md", None)
}

/// Set up Gemini CLI hooks in .gemini/settings.json
fn setup_gemini_hooks() -> Result<(), Box<dyn std::error::Error>> {
    setup_agent_hooks(
        ".gemini",
        "Gemini CLI",
        "BeforeAgent",
        "SessionEnd",
        Some("AfterTool"),
        Some("AfterAgent"),
    )
}

// ---------------------------------------------------------------------------
// Shared instruction content & file I/O helpers
// ---------------------------------------------------------------------------

/// Returns the canonical Legend instructions that all agent instruction files share.
fn generate_legend_instructions() -> String {
    let cmd = get_legend_command();
    format!(
        r#"{LEGEND_MARKER_START}
# Legend — Your Long-Term Memory

You have access to a persistent hierarchical memory system called **Legend**. It stores context across sessions so you can pick up where you left off. **Use it actively and frequently.**

## CRITICAL: Memory Workflow

### On every session start
Run this FIRST before doing anything else:
```bash
{cmd} memory start
```
This returns everything in one call: stats, recent session log, top graph nodes, and relevant short-term memories. Read this to understand prior work, decisions, and open issues.

### During the session (frequently!)
After every significant action (writing code, making a decision, discovering something, completing a task), record it:
```bash
{cmd} memory tick "description of what just happened"
```
Tick **decisions with rationale** ("Chose X over Y because Z"), not just progress.

### Before starting unfamiliar work
Query for relevant context before diving in:
```bash
{cmd} memory query "topic you're about to work on"
```
The top result is automatically reinforced — frequently useful memories rise naturally.

### On session end
Summarize what was accomplished:
```bash
{cmd} memory tick "Session summary: what was done, what's next, any blockers"
```

## Memory Commands

| Command | When to Use |
|---------|-------------|
| `{cmd} memory start` | **Session start** — one call for full context |
| `{cmd} memory tick "<text>"` | Record decision, progress, discovery, blocker |
| `{cmd} memory query "<text>"` | Recall related context (auto-reinforces top result) |
| `{cmd} memory reinforce <signal> <id...>` | Explicit feedback: 1.0 = useful, -1.0 = irrelevant |
| `{cmd} memory stats` | Check storage usage |
| `{cmd} memory sessions [n]` | View chronological session log |
| `{cmd} memory consolidate` | Merge similar memories into long-term graph |

## Dashboard

Launch the live 3D memory visualization dashboard:
```bash
{cmd} dashboard
```
This opens a native Windows app (cross-compiled from WSL) showing:
- 3D force-directed graph of knowledge nodes (right-drag to orbit, scroll to zoom)
- Live event log of all memory operations
- Memory stats, short-term entries with salience bars, session log

Launch it at session start so the user can watch memory activity in real-time.

## Feature Tracking Commands

| Command | Purpose |
|---------|---------|
| `{cmd} get_state` | Load full project state as JSON |
| `{cmd} search <query>` | Search features by keyword |
| `{cmd} show` | Human-readable feature summary |
| `{cmd} update` | Update feature state (pipe JSON to stdin) |

## When to Tick

**Tick these (important context worth preserving):**
- Decisions with rationale: "DECISION: Chose X over Y because Z"
- Bug discoveries: "BUG: X fails when Y happens"
- Architecture insights: "Module X communicates with Y via Z"
- Blockers: "BLOCKER: Can't proceed until X is resolved"
- User preferences: "User prefers X approach"
- Completed features: "Implemented X in file Y"

**Don't tick these (noise that clutters memory):**
- Minor refactors or formatting changes
- Obvious changes that are self-documenting in code
- Routine operations like "reading file X"
- Redundant info already captured in recent ticks

**Tick frequency:** Aim for 3-8 ticks per session. After major decisions, discoveries, or completing substantial work.

## Understanding Tick Output

When you run `tick`, you get JSON feedback:
```json
{{
  "action": "created",       // "created", "merged", or "reconsolidated"
  "entry_id": 42,            // ID of the affected entry
  "matched_existing": null,  // ID if merged/reconsolidated, null if new
  "similarity": null,        // Match score if merged (0.0-1.0)
  "context": {{...}}         // Related memories
}}
```

- **created**: New memory entry added
- **merged**: Combined with similar existing entry (high similarity)
- **reconsolidated**: Updated a recently-accessed "labile" entry

## Hidden Behaviors

Legend does several things automatically:

1. **Auto-reinforce on query**: When you query, the top result gets a small salience boost (+3%). Frequently useful memories rise naturally.

2. **Exponential decay**: Unused memories decay over time. High-salience entries decay slower. Use `reinforce` to preserve important memories.

3. **Reconsolidation window**: After querying a memory, it enters a "labile" state for ~5 ticks. If you tick related content, it updates the existing memory instead of creating a duplicate.

4. **Auto-consolidation**: After ~20 ticks, similar short-term memories are merged into long-term graph nodes.

5. **Hebbian reinforcement**: When entities appear together, the graph edges between them strengthen.

## Understanding Start Output

The `memory start` command returns categorized memories. If a category has >5 items, it shows truncation info:
```json
"decisions": {{
  "items": [...],   // Top 5 by salience
  "showing": 5,
  "total": 12       // 12 total - use --category to see all
}}
```

Use `--category decisions` to fetch the full list for a specific category.

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

/// Detect whether to use 'legend' CLI or 'cargo run --quiet --' based on project context.
///
/// Returns "legend" if we're in a regular project, or "cargo run --quiet --" if we're
/// in the Legend source directory itself (detected by presence of Cargo.toml with name="legend").
fn get_legend_command() -> &'static str {
    // Check if we're in the Legend source directory
    if let Ok(cargo_toml) = fs::read_to_string("Cargo.toml") {
        if cargo_toml.contains("name = \"legend\"") {
            return "cargo run --quiet --";
        }
    }
    "legend"
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
        let existing = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", display_name, e))?;

        if existing.contains(LEGEND_MARKER_START) {
            // Replace existing Legend section with updated content
            if let (Some(start_idx), Some(end_idx)) = (
                existing.find(LEGEND_MARKER_START),
                existing.find(LEGEND_MARKER_END),
            ) {
                let end_idx = end_idx + LEGEND_MARKER_END.len();
                let before = &existing[..start_idx];
                let after = &existing[end_idx..];
                let updated = format!("{}{}{}", before.trim_end(), content, after);
                let updated = if before.is_empty() {
                    format!("{}\n", updated.trim())
                } else {
                    format!("{}\n\n{}\n", before.trim_end(), content.trim())
                };
                fs::write(path, updated)
                    .map_err(|e| format!("Failed to update {}: {}", display_name, e))?;
                println!("✓ Updated Legend instructions in {}", display_name);
            }
            return Ok(());
        }

        let updated = format!("{}\n\n{}\n", existing.trim_end(), content);
        fs::write(path, updated)
            .map_err(|e| format!("Failed to update {}: {}", display_name, e))?;

        println!(
            "✓ Appended Legend instructions to existing {}",
            display_name
        );
    } else {
        if let Some(dir) = parent_dir {
            fs::create_dir_all(dir)
                .map_err(|e| format!("Failed to create {} directory: {}", dir.display(), e))?;
        }

        fs::write(path, format!("{}\n", content))
            .map_err(|e| format!("Failed to create {}: {}", display_name, e))?;

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
    setup_agent_hooks(
        ".claude",
        "Claude Code",
        "UserPromptSubmit",
        "Stop",
        Some("PostToolUse"),
        Some("SubagentStop"),
    )
}

/// Set up Codex hooks in .codex/settings.json
fn setup_codex_hooks() -> Result<(), Box<dyn std::error::Error>> {
    setup_agent_hooks(
        ".codex",
        "Codex",
        "UserPromptSubmit",
        "Stop",
        Some("PostToolUse"),
        Some("SubagentStop"),
    )
}

/// Set up agent hooks in a settings.json for the given tool directory.
///
/// Creates or merges Legend hooks into the project's agent configuration.
/// Sets up three hooks:
/// - SessionStart: loads Legend state automatically
/// - UserPromptSubmit: reminds the agent to search Legend for context
/// - Stop: detects file changes and reminds the agent to update Legend
fn setup_agent_hooks(
    dir_name: &str,
    display_name: &str,
    prompt_event: &str,
    stop_event: &str,
    after_tool_event: Option<&str>,
    after_agent_event: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent_dir = Path::new(dir_name);
    let settings_path = agent_dir.join("settings.json");
    let cmd = get_legend_command();

    let legend_session_hook = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": format!("echo '== Legend Project Context =='; {cmd} project ls 2>/dev/null || echo 'Project state not found'; echo '== Legend Memory =='; {cmd} memory start --compact 2>/dev/null")
        }]
    });

    let legend_prompt_hook = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": format!("{cmd} memory query \"$PROMPT\" 2>/dev/null")
        }]
    });

    let legend_after_tool_hook = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": format!("{cmd} memory tick --passive \"Experience: Executed tool '$TOOL' with status '$STATUS'\" 2>/dev/null")
        }]
    });

    let legend_after_agent_hook = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": format!("{cmd} memory tick --passive \"Experience: Completed an agent turn. Current goal state updated.\" 2>/dev/null")
        }]
    });

    let legend_stop_hook = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": format!(r#"changed=$(git diff --name-only 2>/dev/null | head -5); if [ -n "$changed" ]; then count=$(echo "$changed" | wc -l); echo "Legend: $count file(s) changed."; echo "Remember to tick: {cmd} memory tick 'summary of work'"; fi"#)
        }]
    });

    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)
            .map_err(|e| format!("Failed to read {}/settings.json: {}", dir_name, e))?;

        let mut settings: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}/settings.json: {}", dir_name, e))?;

        // Clean up any old/outdated Legend hooks before adding new ones
        if remove_any_legend_hooks(&mut settings) {
            println!("✓ Updating existing {} hooks", display_name);
        }

        if has_legend_hooks(&settings, prompt_event, stop_event, after_tool_event, after_agent_event) {
            println!("  {} hooks already configured", display_name);
            return Ok(());
        }

        merge_legend_hooks(
            &mut settings,
            &legend_session_hook,
            &legend_prompt_hook,
            &legend_stop_hook,
            &legend_after_tool_hook,
            &legend_after_agent_hook,
            prompt_event,
            stop_event,
            after_tool_event,
            after_agent_event,
        );

        let output = serde_json::to_string_pretty(&settings)?;
        fs::write(&settings_path, output)
            .map_err(|e| format!("Failed to write {}/settings.json: {}", dir_name, e))?;

        println!(
            "✓ Added Legend hooks to existing {}/settings.json",
            dir_name
        );
    } else {
        fs::create_dir_all(agent_dir)
            .map_err(|e| format!("Failed to create {} directory: {}", dir_name, e))?;

        let mut hooks_map = serde_json::Map::new();
        hooks_map.insert("SessionStart".to_string(), json!([legend_session_hook]));
        if let Some(evt) = after_tool_event {
            hooks_map.insert(evt.to_string(), json!([legend_after_tool_hook]));
        }
        if let Some(evt) = after_agent_event {
            hooks_map.insert(evt.to_string(), json!([legend_after_agent_hook]));
        }
        hooks_map.insert(prompt_event.to_string(), json!([legend_prompt_hook]));
        hooks_map.insert(stop_event.to_string(), json!([legend_stop_hook]));
        let settings = json!({ "hooks": hooks_map });

        let output = serde_json::to_string_pretty(&settings)?;
        fs::write(&settings_path, output)
            .map_err(|e| format!("Failed to write {}/settings.json: {}", dir_name, e))?;

        println!("✓ Created {}/settings.json with Legend hooks", dir_name);
    }

    Ok(())
}

/// Check if Legend hooks are already configured correctly for this agent
fn has_legend_hooks(
    settings: &Value,
    prompt_event: &str,
    stop_event: &str,
    after_tool_event: Option<&str>,
    after_agent_event: Option<&str>,
) -> bool {
    let cmd = get_legend_command();

    let mut required: Vec<&str> = vec!["SessionStart", prompt_event, stop_event];
    if let Some(evt) = after_tool_event {
        required.push(evt);
    }
    if let Some(evt) = after_agent_event {
        required.push(evt);
    }

    // Check all required hook points
    for hook_type in &required {
        let mut found = false;
        if let Some(hook_entries) = settings
            .get("hooks")
            .and_then(|h| h.get(*hook_type))
            .and_then(|s| s.as_array())
        {
            for hook_entry in hook_entries {
                if let Some(hooks) = hook_entry.get("hooks").and_then(|h| h.as_array()) {
                    for hook in hooks {
                        if let Some(hook_cmd) = hook.get("command").and_then(|c| c.as_str()) {
                            if (hook_cmd.contains("legend get_state")
                                || hook_cmd.contains("legend memory start")
                                || hook_cmd.contains("legend memory query")
                                || hook_cmd.contains("legend memory tick"))
                                && hook_cmd.contains(cmd)
                            {
                                found = true;
                                break;
                            }
                        }
                    }
                }
                if found {
                    break;
                }
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// Remove any existing Legend hooks from settings for any agent event names.
/// Returns true if any hooks were removed.
fn remove_any_legend_hooks(settings: &mut Value) -> bool {
    let mut removed = false;
    // All possible hook event names across different tools
    let hook_types = [
        "SessionStart",
        "UserPromptSubmit",
        "Stop",
        "BeforeAgent",
        "SessionEnd",
        "AfterTool",
        "AfterAgent",
        "PostToolUse",
        "SubagentStop",
    ];

    if let Some(hooks_obj) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for hook_type in hook_types {
            let mut should_remove_key = false;
            if let Some(hook_entries) = hooks_obj.get_mut(hook_type).and_then(|s| s.as_array_mut()) {
                let initial_len = hook_entries.len();
                // Filter out entries that contain any Legend commands
                hook_entries.retain(|entry| {
                    let entry_str = serde_json::to_string(entry).unwrap_or_default();
                    // If it contains these core strings, it's likely a Legend hook
                    !(entry_str.contains("get_state")
                        || entry_str.contains("memory start")
                        || entry_str.contains("memory query")
                        || entry_str.contains("memory tick"))
                });
                if hook_entries.len() < initial_len {
                    removed = true;
                }
                if hook_entries.is_empty() {
                    should_remove_key = true;
                }
            }

            if should_remove_key {
                hooks_obj.remove(hook_type);
                removed = true;
            }
        }
    }
    removed
}

/// Merge Legend hooks into existing settings
fn merge_legend_hooks(
    settings: &mut Value,
    session_hook: &Value,
    prompt_hook: &Value,
    stop_hook: &Value,
    after_tool_hook: &Value,
    after_agent_hook: &Value,
    prompt_event: &str,
    stop_event: &str,
    after_tool_event: Option<&str>,
    after_agent_event: Option<&str>,
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

    // Add after-tool hook if the agent supports it
    if let Some(evt) = after_tool_event {
        if hooks.get(evt).is_none() {
            hooks[evt] = json!([]);
        }
        if let Some(arr) = hooks.get_mut(evt).and_then(|s| s.as_array_mut()) {
            arr.push(after_tool_hook.clone());
        }
    }

    // Add after-agent hook if the agent supports it
    if let Some(evt) = after_agent_event {
        if hooks.get(evt).is_none() {
            hooks[evt] = json!([]);
        }
        if let Some(arr) = hooks.get_mut(evt).and_then(|s| s.as_array_mut()) {
            arr.push(after_agent_hook.clone());
        }
    }

    // Add prompt hook (e.g. UserPromptSubmit or BeforeAgent)
    if hooks.get(prompt_event).is_none() {
        hooks[prompt_event] = json!([]);
    }
    if let Some(arr) = hooks.get_mut(prompt_event).and_then(|s| s.as_array_mut()) {
        arr.push(prompt_hook.clone());
    }

    // Add stop hook (e.g. Stop or SessionEnd)
    if hooks.get(stop_event).is_none() {
        hooks[stop_event] = json!([]);
    }
    if let Some(arr) = hooks.get_mut(stop_event).and_then(|s| s.as_array_mut()) {
        arr.push(stop_hook.clone());
    }
}
