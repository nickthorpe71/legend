use crate::cli::{parse_args, print_command_help, CommandDef};
use crate::commands::discover;
use crate::memory::wernicke::lexicon as keywords;
use crate::tool::bootstrap as keyword_bootstrap;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

const LEGEND_MARKER_START: &str = "<!-- legend-start -->";
const LEGEND_MARKER_END: &str = "<!-- legend-end -->";

/// Initialize a new Legend project
///
/// Creates `.legend/` directory, auto-discovers features, sets up
/// agent hooks, and generates agent instruction files.
/// Safe to run multiple times - won't error if directory already exists.
pub fn handle_init(args: &[String], def: &CommandDef) -> Result<(), Box<dyn std::error::Error>> {
    let parsed = parse_args(args, def);

    if parsed.has("help") {
        print_command_help(def);
        return Ok(());
    }

    let discover_requested = parsed.has("discover");

    let legend_dir = Path::new(".legend");
    let first_init = !legend_dir.exists();

    if !first_init {
        println!("Legend already initialized in this directory");
        println!("  .legend/ directory exists");
        println!("  Use 'legend memory start' to view current memory context");

        if discover_requested {
            println!("  Re-scanning project context...");
            if let Ok(report) = discover::run_discovery(Path::new(".")) {
                let _ = discover::onboard_project(Path::new("."), &report);
                println!(
                    "✓ Project onboarding complete. High-signal context ingested into Legend."
                );
                // Re-bootstrap workspace keywords from fresh discovery
                if let Ok(mut memory) = crate::memory::load_or_default() {
                    let high_signal: Vec<(String, String)> = report
                        .high_signal_files
                        .iter()
                        .map(|f| (f.path.clone(), f.kind.clone()))
                        .collect();
                    let languages: Vec<(String, usize)> = report
                        .languages
                        .iter()
                        .map(|(k, v)| (k.clone(), *v))
                        .collect();
                    let br = keyword_bootstrap::bootstrap_keywords_from_workspace(
                        Path::new("."),
                        &high_signal,
                        &languages,
                        &report.metadata.tech_stack,
                        &mut memory,
                    );
                    if br.total > 0 {
                        crate::memory::rebuild_keyword_cache(&mut memory.brain);
                        let _ = crate::memory::save(&memory);
                        println!("✓ Bootstrapped {} workspace keywords", br.total);
                    }
                }
            }
        }

        // Migrate/refresh memory store to latest format
        migrate_memory_store();

        // One-time keyword migration: if graph has no Keyword nodes yet, seed them
        migrate_keywords_if_needed();

        setup_git_merge_driver()?;
        setup_claude_hooks()?;
        setup_claude_mcp()?;
        setup_claude_md()?;
        setup_codex_hooks()?;
        setup_codex_mcp()?;
        setup_codex_md()?;
        setup_agents_md()?;
        setup_copilot_instructions()?;
        setup_vscode_mcp()?;
        setup_zed_rules()?;
        setup_zed_mcp()?;
        setup_gemini_styleguide()?;
        setup_gemini_md()?;
        setup_gemini_hooks()?;
        add_mcp_to_gemini_settings()?;
        setup_cursor_rules()?;
        setup_cursor_mcp()?;

        return Ok(());
    }

    // First time initialization
    fs::create_dir_all(legend_dir)
        .map_err(|e| format!("Failed to create .legend directory: {}", e))?;

    let report = discover::run_discovery(Path::new(".")).ok();

    if let Some(ref report) = report {
        // Run discovery automatically on first init
        println!("  First-time initialization: Ingesting high-signal context into memory...");
        let _ = discover::onboard_project(Path::new("."), report);
        println!("✓ Project onboarding complete. High-signal context ingested into Legend.");
        println!("\nNext Steps for AI Agent:");
        println!("1. Run 'legend memory start' to see the ingested context.");
        println!("2. Use 'legend memory query' to explore specific modules or features.");
        println!("3. If significant architectural details are missing, manually 'tick' them.");
    }

    // Seed keywords based on project context
    if let Some(ref report) = report {
        seed_tier1_keywords(report);
        print_tier2_prompt(report);
    } else {
        // No discovery report — seed universal keywords only
        seed_universal_keywords();
    }

    println!("✓ Initialized Legend");
    println!("  Created .legend/ directory");

    setup_git_merge_driver()?;
    setup_claude_hooks()?;
    setup_claude_mcp()?;
    setup_claude_md()?;
    setup_codex_hooks()?;
    setup_codex_mcp()?;
    setup_codex_md()?;
    setup_agents_md()?;
    setup_copilot_instructions()?;
    setup_vscode_mcp()?;
    setup_zed_rules()?;
    setup_zed_mcp()?;
    setup_gemini_styleguide()?;
    setup_gemini_md()?;
    setup_gemini_hooks()?;
    add_mcp_to_gemini_settings()?;
    setup_cursor_rules()?;
    setup_cursor_mcp()?;

    Ok(())
}

/// Set up Git merge driver for Legend memory files.
///
/// This tells Git to use 'legend git-merge-driver' to resolve conflicts
/// in .legend/memory.lz4 and .legend/events.jsonl.
fn setup_git_merge_driver() -> Result<(), Box<dyn std::error::Error>> {
    use std::process::Command;

    // 1. Configure local git merge driver
    let cmd = get_legend_command();
    let git_config_cmd = "merge.legend.driver".to_string();
    let git_config_val = format!("{} git-merge-driver %O %A %B %P", cmd);

    let status = Command::new("git")
        .args(["config", "--local", &git_config_cmd, &git_config_val])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("✓ Configured local Git merge driver for Legend");
        }
        _ => {
            eprintln!("  Warning: failed to configure Git merge driver via 'git config'");
        }
    }

    // 2. Add to .gitattributes
    let attr_lines: &[&str] = &[
        ".legend/memory.lz4 merge=legend",
        ".legend/events.jsonl merge=legend",
    ];
    let attr_path = Path::new(".gitattributes");

    let existing = if attr_path.exists() {
        fs::read_to_string(attr_path)?
    } else {
        String::new()
    };
    let mut kept: Vec<&str> = existing
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && trimmed != ".legend/*.lz4 merge=legend"
                && trimmed != ".legend/state.lz4 merge=legend"
                && trimmed != ".legend/memory.lz4 merge=legend"
                && trimmed != ".legend/events.jsonl merge=legend"
        })
        .collect();
    for line in attr_lines {
        kept.push(line);
    }
    let content = kept.join("\n") + "\n";
    fs::write(attr_path, content)?;
    println!("✓ Updated .gitattributes with Legend merge driver rules");

    Ok(())
}

/// Migrate memory store to latest format.
///
/// Loads memory (triggering any pending migrations), then re-saves it
/// in the current format. This ensures old memory files are upgraded.
fn migrate_memory_store() {
    match crate::memory::load_or_default() {
        Ok(mut state) => {
            let entries = state.brain.short_term.len();
            let nodes = state.brain.long_term.nodes.len();

            crate::memory::rebalance_weights(&mut state.brain);

            // Scan manifests for dependencies and add to graph
            crate::memory::scan_ecosystem_dependencies(&mut state);

            if let Err(e) = crate::memory::save(&state) {
                eprintln!("  Warning: failed to save memory store: {}", e);
            } else if entries > 0 || nodes > 0 {
                println!(
                    "  Memory store OK ({} entries, {} graph nodes)",
                    entries, nodes
                );
            }
        }
        Err(e) => {
            eprintln!("  Warning: failed to load memory store: {}", e);
        }
    }
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

/// Generate or update AGENTS.md with Legend usage instructions
fn setup_agents_md() -> Result<(), Box<dyn std::error::Error>> {
    write_legend_markdown(Path::new("AGENTS.md"), "AGENTS.md", None)
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
        None,
    )
}

/// Generate or update .cursorrules with Legend usage instructions
fn setup_cursor_rules() -> Result<(), Box<dyn std::error::Error>> {
    write_legend_markdown(Path::new(".cursorrules"), ".cursorrules", None)
}

// ---------------------------------------------------------------------------
// Shared instruction content & file I/O helpers
// ---------------------------------------------------------------------------

/// Returns the canonical Legend instructions that all agent instruction files share.
fn generate_legend_instructions() -> String {
    let cmd = get_legend_command();
    format!(
        r#"{LEGEND_MARKER_START}
# SYSTEM_MANDATE: Legend — Your Long-Term Memory

You MUST use **Legend** to maintain context across sessions.

## Session Start — CRITICAL
The **SessionStart hook** automatically runs `{cmd} memory start` and injects the output into your first system-reminder. **Your very first message MUST:**
1. Acknowledge the Legend context you received (current task, recent activity, key memories)
2. If the hook output is missing or empty, run `{cmd} memory start` manually

**NEVER skip or ignore the Legend session context.** It contains your task continuity, architectural decisions, and user preferences from prior sessions. Failing to acknowledge it wastes the user's time re-explaining context.

## Essential Commands
- **Record decisions:** `{cmd} memory tick <<'EOF'` ... `EOF` — tick decisions with rationale (DECISION:, BUG:, ARCHITECTURE:, BLOCKER: prefixes). Aim for 3-8 ticks per session.
- **Recall context:** `{cmd} memory query <<'EOF'` ... `EOF` — query before starting new topics. Top result auto-reinforced.
{LEGEND_MARKER_END}"#,
        LEGEND_MARKER_START = LEGEND_MARKER_START,
        LEGEND_MARKER_END = LEGEND_MARKER_END,
        cmd = cmd
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
            // Prefer pre-built binary for speed in hooks (~40ms vs ~200ms)
            if Path::new("./target/release/legend").exists() {
                return "./target/release/legend";
            }
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
        None,
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
        None,
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
    post_tool_event: Option<&str>,
    _after_agent_event: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let agent_dir = Path::new(dir_name);
    let settings_path = agent_dir.join("settings.json");
    let cmd = get_legend_command();

    let legend_session_hook = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": format!("{cmd} memory start")
        }]
    });

    // Auto-query Legend with the user's prompt text (10s cooldown, 800 char cap)
    let legend_prompt_hook = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": format!(
                "input=$(cat); now=$(date +%s); stamp=.legend/.last_query; last=$(cat \"$stamp\" 2>/dev/null || echo 0); if [ $((now - last)) -lt 10 ]; then exit 0; fi; prompt=$(echo \"$input\" | sed -n 's/.*\"prompt\": *\"\\([^\"]*\\)\".*/\\1/p' | head -c 200); if [ -n \"$prompt\" ]; then echo \"$now\" > \"$stamp\"; result=$({cmd} memory query \"$prompt\" 2>/dev/null | head -c 800); if [ -n \"$result\" ]; then printf '{{\"additionalContext\":\"[Legend Context] %s\"}}' \"$(echo \"$result\" | tr '\"' \"'\" | tr '\\n' ' ')\"; fi; fi",
                cmd = cmd
            )
        }]
    });

    // Remind LLM to tick after file modifications (no binary invocation — zero latency)
    let legend_post_tool_hook = json!({
        "matcher": "Edit|Write|NotebookEdit",
        "hooks": [{
            "type": "command",
            "command": format!(
                "input=$(cat); file=$(echo \"$input\" | sed -n 's/.*\"file_path\": *\"\\([^\"]*\\)\".*/\\1/p'); if [ -n \"$file\" ]; then printf '{{\"additionalContext\":\"[Legend] You modified %s. Tick your decision: {cmd} memory tick '\\''DECISION: ...'\\''\"}}' \"$file\"; fi",
                cmd = cmd
            )
        }]
    });

    // Show changed file count at session end
    let legend_stop_hook = json!({
        "matcher": "*",
        "hooks": [{
            "type": "command",
            "command": format!(
                "changed=$(git diff --name-only 2>/dev/null | head -5); if [ -n \"$changed\" ]; then count=$(echo \"$changed\" | wc -l | tr -d ' '); printf '{{\"additionalContext\":\"[Legend] %s file(s) changed this session. Tick final decisions: {cmd} memory tick\"}}' \"$count\"; fi",
                cmd = cmd
            )
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

        if has_legend_hooks(
            &settings,
            prompt_event,
            stop_event,
            post_tool_event,
            _after_agent_event,
        ) {
            println!("  {} hooks already configured", display_name);
            return Ok(());
        }

        merge_legend_hooks(
            &mut settings,
            LegendHooks {
                session_hook: &legend_session_hook,
                prompt_hook: &legend_prompt_hook,
                stop_hook: &legend_stop_hook,
                prompt_event,
                stop_event,
                post_tool_hook: post_tool_event.map(|_| &legend_post_tool_hook),
                post_tool_event,
            },
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
        hooks_map.insert(prompt_event.to_string(), json!([legend_prompt_hook]));
        hooks_map.insert(stop_event.to_string(), json!([legend_stop_hook]));
        if let Some(event) = post_tool_event {
            hooks_map.insert(event.to_string(), json!([legend_post_tool_hook]));
        }
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
                            if (hook_cmd.contains("legend memory start")
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
            if let Some(hook_entries) = hooks_obj.get_mut(hook_type).and_then(|s| s.as_array_mut())
            {
                let initial_len = hook_entries.len();
                // Filter out entries that contain any Legend commands or markers
                hook_entries.retain(|entry| {
                    let entry_str = serde_json::to_string(entry).unwrap_or_default();
                    !(entry_str.contains("memory start")
                        || entry_str.contains("memory query")
                        || entry_str.contains("memory tick")
                        || entry_str.contains("[Legend]")
                        || entry_str.contains("[Legend Context]"))
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

struct LegendHooks<'a> {
    session_hook: &'a Value,
    prompt_hook: &'a Value,
    stop_hook: &'a Value,
    prompt_event: &'a str,
    stop_event: &'a str,
    post_tool_hook: Option<&'a Value>,
    post_tool_event: Option<&'a str>,
}

/// Merge Legend hooks into existing settings
fn merge_legend_hooks(settings: &mut Value, hooks_config: LegendHooks) {
    if settings.get("hooks").is_none() {
        settings["hooks"] = json!({});
    }

    let hooks = settings.get_mut("hooks").unwrap();

    // Add SessionStart hook
    if hooks.get("SessionStart").is_none() {
        hooks["SessionStart"] = json!([]);
    }
    if let Some(arr) = hooks.get_mut("SessionStart").and_then(|s| s.as_array_mut()) {
        arr.push(hooks_config.session_hook.clone());
    }

    // Add prompt hook (e.g. UserPromptSubmit or BeforeAgent)
    if hooks.get(hooks_config.prompt_event).is_none() {
        hooks[hooks_config.prompt_event] = json!([]);
    }
    if let Some(arr) = hooks
        .get_mut(hooks_config.prompt_event)
        .and_then(|s| s.as_array_mut())
    {
        arr.push(hooks_config.prompt_hook.clone());
    }

    // Add stop hook (e.g. Stop or SessionEnd)
    if hooks.get(hooks_config.stop_event).is_none() {
        hooks[hooks_config.stop_event] = json!([]);
    }
    if let Some(arr) = hooks
        .get_mut(hooks_config.stop_event)
        .and_then(|s| s.as_array_mut())
    {
        arr.push(hooks_config.stop_hook.clone());
    }

    // Add post-tool hook if configured (e.g. PostToolUse or AfterTool)
    if let (Some(hook), Some(event)) = (hooks_config.post_tool_hook, hooks_config.post_tool_event)
    {
        if hooks.get(event).is_none() {
            hooks[event] = json!([]);
        }
        if let Some(arr) = hooks.get_mut(event).and_then(|s| s.as_array_mut()) {
            arr.push(hook.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// MCP config file generation
// ---------------------------------------------------------------------------

/// Get the Legend command split into (command, args) for MCP JSON configs.
///
/// Returns ("legend", ["mcp-serve"]) normally, or
/// ("cargo", ["run", "--quiet", "--", "mcp-serve"]) in the Legend source dir.
fn get_legend_mcp_command() -> (&'static str, Vec<&'static str>) {
    let cmd = get_legend_command();
    if cmd == "cargo run --quiet --" {
        ("cargo", vec!["run", "--quiet", "--", "mcp-serve"])
    } else {
        ("legend", vec!["mcp-serve"])
    }
}

/// Merge a key into a JSON object file, creating the file if it doesn't exist.
/// If the file exists, merges the key at the top level (does not overwrite other keys).
/// Idempotent: skips if the key already exists.
fn merge_json_config(
    path: &Path,
    dir: Option<&Path>,
    key: &str,
    value: Value,
    display_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", display_name, e))?;
        let mut settings: Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", display_name, e))?;

        if settings
            .get(key)
            .and_then(|v| v.get("legend-memory"))
            .is_some()
        {
            println!("  {} MCP already configured", display_name);
            return Ok(());
        }

        // Merge: if key exists as object, insert into it; otherwise create it
        if let Some(obj) = settings.get_mut(key).and_then(|v| v.as_object_mut()) {
            obj.insert("legend-memory".to_string(), value);
        } else {
            settings[key] = json!({ "legend-memory": value });
        }

        let output = serde_json::to_string_pretty(&settings)?;
        fs::write(path, output).map_err(|e| format!("Failed to write {}: {}", display_name, e))?;
        println!("✓ Added Legend MCP to existing {}", display_name);
    } else {
        if let Some(d) = dir {
            fs::create_dir_all(d)
                .map_err(|e| format!("Failed to create {} directory: {}", d.display(), e))?;
        }
        let settings = json!({ key: { "legend-memory": value } });
        let output = serde_json::to_string_pretty(&settings)?;
        fs::write(path, output).map_err(|e| format!("Failed to write {}: {}", display_name, e))?;
        println!("✓ Created {} with Legend MCP config", display_name);
    }
    Ok(())
}

/// Set up MCP config for Claude Code — .mcp.json
fn setup_claude_mcp() -> Result<(), Box<dyn std::error::Error>> {
    let (cmd, args) = get_legend_mcp_command();
    let server = json!({ "command": cmd, "args": args });
    merge_json_config(
        Path::new(".mcp.json"),
        None,
        "mcpServers",
        server,
        ".mcp.json",
    )
}

/// Set up MCP config for VS Code Copilot — .vscode/mcp.json
fn setup_vscode_mcp() -> Result<(), Box<dyn std::error::Error>> {
    let (cmd, args) = get_legend_mcp_command();
    let server = json!({ "command": cmd, "args": args });
    let vscode_dir = Path::new(".vscode");
    merge_json_config(
        &vscode_dir.join("mcp.json"),
        Some(vscode_dir),
        "servers",
        server,
        ".vscode/mcp.json",
    )
}

/// Set up MCP config for Cursor — .cursor/mcp.json
fn setup_cursor_mcp() -> Result<(), Box<dyn std::error::Error>> {
    let (cmd, args) = get_legend_mcp_command();
    let server = json!({ "command": cmd, "args": args });
    let cursor_dir = Path::new(".cursor");
    merge_json_config(
        &cursor_dir.join("mcp.json"),
        Some(cursor_dir),
        "mcpServers",
        server,
        ".cursor/mcp.json",
    )
}

/// Add MCP config to Gemini CLI — .gemini/settings.json
/// Must run AFTER setup_gemini_hooks() which creates the file.
fn add_mcp_to_gemini_settings() -> Result<(), Box<dyn std::error::Error>> {
    let (cmd, args) = get_legend_mcp_command();
    let server = json!({ "command": cmd, "args": args });
    let gemini_dir = Path::new(".gemini");
    merge_json_config(
        &gemini_dir.join("settings.json"),
        Some(gemini_dir),
        "mcpServers",
        server,
        ".gemini/settings.json (MCP)",
    )
}

/// Set up MCP config for Codex — .codex/config.toml
fn setup_codex_mcp() -> Result<(), Box<dyn std::error::Error>> {
    let codex_dir = Path::new(".codex");
    let config_path = codex_dir.join("config.toml");
    let (cmd, args) = get_legend_mcp_command();

    let mcp_section = format!(
        "\n[mcp_servers.legend-memory]\ncommand = \"{}\"\nargs = [{}]\n",
        cmd,
        args.iter()
            .map(|a| format!("\"{}\"", a))
            .collect::<Vec<_>>()
            .join(", ")
    );

    if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("Failed to read .codex/config.toml: {}", e))?;

        if content.contains("[mcp_servers.legend-memory]") {
            println!("  .codex/config.toml MCP already configured");
            return Ok(());
        }

        let updated = format!("{}{}", content.trim_end(), mcp_section);
        fs::write(&config_path, updated)
            .map_err(|e| format!("Failed to write .codex/config.toml: {}", e))?;
        println!("✓ Added Legend MCP to existing .codex/config.toml");
    } else {
        fs::create_dir_all(codex_dir)
            .map_err(|e| format!("Failed to create .codex directory: {}", e))?;
        fs::write(&config_path, mcp_section.trim_start())
            .map_err(|e| format!("Failed to write .codex/config.toml: {}", e))?;
        println!("✓ Created .codex/config.toml with Legend MCP config");
    }

    Ok(())
}

/// Set up MCP config for Zed — .zed/settings.json
fn setup_zed_mcp() -> Result<(), Box<dyn std::error::Error>> {
    let (cmd, args) = get_legend_mcp_command();
    let server = json!({ "command": cmd, "args": args });
    let zed_dir = Path::new(".zed");
    merge_json_config(
        &zed_dir.join("settings.json"),
        Some(zed_dir),
        "context_servers",
        server,
        ".zed/settings.json",
    )
}

// ---------------------------------------------------------------------------
// Keyword Seeding
// ---------------------------------------------------------------------------

/// Seed universal classification keywords from static arrays into the knowledge graph.
/// Shared by `seed_tier1_keywords` and `seed_universal_keywords`.
fn seed_static_keywords(brain: &mut crate::memory::BrainState) -> usize {
    let mut count = 0;
    let categories: &[(&str, &[&str])] = &[
        ("decision", keywords::DECISION_KEYWORDS),
        ("bug", keywords::BUG_KEYWORDS),
        ("todo", keywords::TODO_KEYWORDS),
        ("preference", keywords::PREFERENCE_KEYWORDS),
        ("architecture", keywords::ARCHITECTURE_KEYWORDS),
        ("environment", keywords::ENVIRONMENT_KEYWORDS),
    ];
    for (category, kws) in categories {
        for kw in *kws {
            if crate::memory::add_keyword_node(brain, category, kw, Vec::new()) {
                count += 1;
            }
        }
    }
    for (verb, _kind) in keywords::ACTION_KEYWORDS {
        if crate::memory::add_keyword_node(brain, "action", verb, Vec::new()) {
            count += 1;
        }
    }
    count
}

/// Seed tier-1 keywords from discovery report (no LLM needed).
///
/// Seeds universal keywords (decision, bug, todo, etc.) from static arrays,
/// plus language-specific code keywords based on detected languages.
fn seed_tier1_keywords(report: &discover::DiscoveryReport) {
    let mut memory = match crate::memory::load_or_default() {
        Ok(m) => m,
        Err(_) => return,
    };

    let mut count = seed_static_keywords(&mut memory.brain);

    // 2. Workspace bootstrap — environmental imprinting layer
    //    Seeds: dependencies → tools, doc headings → architecture,
    //    recurring terms → domain, language code keywords, config keys → environment
    let high_signal: Vec<(String, String)> = report
        .high_signal_files
        .iter()
        .map(|f| (f.path.clone(), f.kind.clone()))
        .collect();
    let languages: Vec<(String, usize)> = report
        .languages
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    let bootstrap_result = keyword_bootstrap::bootstrap_keywords_from_workspace(
        Path::new("."),
        &high_signal,
        &languages,
        &report.metadata.tech_stack,
        &mut memory,
    );
    count += bootstrap_result.total;

    if count > 0 {
        crate::memory::rebuild_keyword_cache(&mut memory.brain);
        if let Err(e) = crate::memory::save(&memory) {
            eprintln!("  Warning: failed to save keyword seeds: {}", e);
        } else {
            println!("✓ Seeded {} keyword nodes into knowledge graph", count);
            if bootstrap_result.total > 0 {
                println!(
                    "  ({} innate + {} workspace: {} tools, {} architecture, {} domain, {} code, {} env)",
                    count - bootstrap_result.total,
                    bootstrap_result.total,
                    bootstrap_result.tools,
                    bootstrap_result.architecture,
                    bootstrap_result.domain,
                    bootstrap_result.code,
                    bootstrap_result.environment,
                );
            }
        }
    }
}

/// Seed only universal keywords (no discovery report available).
fn seed_universal_keywords() {
    let mut memory = match crate::memory::load_or_default() {
        Ok(m) => m,
        Err(_) => return,
    };

    let count = seed_static_keywords(&mut memory.brain);

    if count > 0 {
        crate::memory::rebuild_keyword_cache(&mut memory.brain);
        if let Err(e) = crate::memory::save(&memory) {
            eprintln!("  Warning: failed to save keyword seeds: {}", e);
        } else {
            println!(
                "✓ Seeded {} universal keyword nodes into knowledge graph",
                count
            );
        }
    }
}

/// Print a structured prompt for the LLM to enrich keywords via KEYWORD: ticks.
fn print_tier2_prompt(report: &discover::DiscoveryReport) {
    let detected_langs: Vec<&String> = report
        .languages
        .iter()
        .filter(|(_, &count)| count > 0)
        .map(|(lang, _)| lang)
        .collect();

    eprintln!();
    eprintln!("## Keyword Enrichment (LLM-Assisted)");
    eprintln!();
    eprintln!(
        "Legend has seeded base keywords for project '{}' ({}). To add domain-specific keywords,",
        report.metadata.name,
        detected_langs
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    eprintln!("use KEYWORD: directives in your ticks:");
    eprintln!();
    eprintln!("  legend memory tick \"KEYWORD:tool:<framework_name>\"");
    eprintln!("  legend memory tick \"KEYWORD:architecture:<domain_term>\"");
    eprintln!("  legend memory tick \"Added new framework KEYWORD:tool:bevy\"");
    eprintln!();
    eprintln!("Keywords are reinforced automatically when they appear in ticks.");
}

/// Check if graph has any Keyword nodes; if not, seed tier-1 universals.
fn migrate_keywords_if_needed() {
    let memory = match crate::memory::load_or_default() {
        Ok(m) => m,
        Err(_) => return,
    };

    let has_keywords = memory
        .brain
        .long_term
        .nodes
        .values()
        .any(|n| n.kind == "Keyword");

    if !has_keywords {
        println!("  Migrating: seeding keyword nodes into knowledge graph...");
        // Try with discovery report if available
        if let Ok(report) = discover::run_discovery(Path::new(".")) {
            seed_tier1_keywords(&report);
        } else {
            seed_universal_keywords();
        }
    }
}

/// Count keyword nodes in the graph.
#[allow(dead_code)]
pub fn count_keyword_nodes() -> usize {
    match crate::memory::load_or_default() {
        Ok(m) => m
            .brain
            .long_term
            .nodes
            .values()
            .filter(|n| n.kind == "Keyword")
            .count(),
        Err(_) => 0,
    }
}
