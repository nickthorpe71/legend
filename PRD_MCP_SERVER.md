# PRD: Legend MCP Server

## Problem

Legend's memory system relies on AI models voluntarily running `legend memory start/tick/query` via Bash. In practice, models frequently ignore these instructions — even when CLAUDE.md mandates them and hooks inject reminders. The model sees legend as "some CLI command I should probably run" rather than a tool in its toolbox.

MCP (Model Context Protocol) solves this by registering legend's memory operations as **first-class tools** — identical in status to Read, Write, Edit, and Bash. The model sees `legend_memory_tick` in its tool list and calls it directly, with no Bash intermediary.

## Goal

1. Add a `legend mcp-serve` subcommand that runs a stdio MCP server exposing legend's memory operations as tools
2. Update `legend init` to create MCP config files for all supported platforms
3. Zero additional dependencies for end users — `legend mcp-serve` is part of the existing binary

## Non-Goals

- HTTP/SSE MCP transport (stdio is sufficient for local use)
- Exposing LLM task orchestration commands (`legend llm`) via MCP (future consideration)
- Replacing hooks entirely (hooks and MCP tools complement each other)

---

## Architecture

### How MCP Stdio Works

```
┌──────────────┐     stdin (JSON-RPC)      ┌──────────────────┐
│  Claude Code  │ ──────────────────────── │  legend mcp-serve │
│  / Gemini CLI │                          │                    │
│  / Copilot    │ ◄──────────────────────  │  (same binary,     │
└──────────────┘     stdout (JSON-RPC)     │   new subcommand)  │
                                           └──────────────────┘
```

- The AI tool (Claude Code, Gemini CLI, Copilot) spawns `legend mcp-serve` as a child process at session start
- Communication is JSON-RPC 2.0 over newline-delimited JSON on stdin/stdout
- The server stays alive for the duration of the session
- No network, no HTTP — purely local process I/O
- `legend mcp-serve` calls legend's existing memory logic internally (same binary, direct function calls)

### Tool Definitions

The MCP server exposes these tools:

| Tool Name | Arguments | Description |
|-----------|-----------|-------------|
| `legend_memory_start` | `category?: string` | Start a memory session. Returns context, recent memories, protocol instructions. Optional category filter. |
| `legend_memory_tick` | `description: string` | Record a decision, discovery, progress update, or blocker. |
| `legend_memory_query` | `topic: string` | Query memory for relevant context. Auto-reinforces top result. |
| `legend_memory_task_get` | _(none)_ | Get the current task. |
| `legend_memory_task_set` | `task: string` | Set the current task. |
| `legend_memory_stats` | _(none)_ | Show memory storage stats. |

#### Tool Descriptions (what the model sees)

These descriptions are critical — they tell the model **when** to use each tool:

```
legend_memory_start:
  "Start a Legend memory session. MUST be called at the beginning of every
   session. Returns prior decisions, architectural context, and recent
   activity. Provides the context needed to avoid repeating past work."

legend_memory_tick:
  "Record a significant decision, discovery, or completed action to Legend
   memory. Call after: making architectural decisions, fixing bugs,
   completing features, discovering important patterns, or when the user
   makes a preference known. Include rationale: 'DECISION: Chose X over Y
   because Z'."

legend_memory_query:
  "Search Legend memory for context about a topic before starting work.
   Call before: working on unfamiliar modules, investigating bugs, or
   making design decisions. Returns related prior decisions and context."
```

### Protocol Flow

```
1. Session starts → AI tool spawns `legend mcp-serve`
2. Handshake:
   Client → {"method":"initialize","params":{...}}
   Server → {"result":{"capabilities":{"tools":{}},"serverInfo":{"name":"legend-memory"}}}
   Client → {"method":"notifications/initialized"}

3. Tool discovery:
   Client → {"method":"tools/list"}
   Server → {"result":{"tools":[{name,description,inputSchema}...]}}

4. During session (model decides to call):
   Client → {"method":"tools/call","params":{"name":"legend_memory_tick","arguments":{"description":"..."}}}
   Server → {"result":{"content":[{"type":"text","text":"Memory recorded."}]}}

5. Session ends → Client closes stdin → Server exits
```

---

## Changes to `legend init`

### Current Files Created

```
.legend/                          # Memory storage
  memory.lz4
  events.jsonl
CLAUDE.md                         # Claude Code instructions
.claude/settings.json             # Claude hooks
GEMINI.md                         # Gemini instructions
.gemini/settings.json             # Gemini hooks
.github/copilot-instructions.md   # Copilot instructions
CODEX.md                          # Codex instructions
.codex/settings.json              # Codex hooks
AGENTS.md                         # Generic agent instructions
.cursorrules                      # Cursor instructions
.rules                            # Generic rules
.gitattributes                    # Git merge driver
```

### New Files to Create

```
.mcp.json                         # Claude Code MCP config
.vscode/mcp.json                  # VS Code Copilot MCP config
```

### New File: `.mcp.json` (Claude Code)

```json
{
  "mcpServers": {
    "legend-memory": {
      "command": "legend",
      "args": ["mcp-serve"]
    }
  }
}
```

### New File: `.vscode/mcp.json` (VS Code Copilot)

```json
{
  "servers": {
    "legend-memory": {
      "command": "legend",
      "args": ["mcp-serve"]
    }
  }
}
```

### Update: `.gemini/settings.json`

Currently contains hooks only. Add `mcpServers` key alongside existing content:

```json
{
  "mcpServers": {
    "legend-memory": {
      "command": "legend",
      "args": ["mcp-serve"]
    }
  },
  "hooks": {
    ...existing hooks...
  }
}
```

### Init Output (Updated)

```
✓ Initialized Legend
  Created .legend/ directory
✓ Created .mcp.json with Legend MCP server              ← NEW
✓ Created .vscode/mcp.json with Legend MCP server        ← NEW
✓ Created .claude/settings.json with Legend hooks
✓ Created CLAUDE.md with Legend instructions
✓ Updated .gemini/settings.json with Legend MCP server   ← UPDATED
✓ Created .gemini/settings.json with Legend hooks
✓ Created GEMINI.md with Legend instructions
  ...etc
```

---

## Implementation: `legend mcp-serve`

### Subcommand Behavior

```
legend mcp-serve [--cwd <path>]
```

- Reads JSON-RPC messages from stdin, writes responses to stdout
- Logs to stderr (never pollute stdout)
- Runs in the current working directory (or `--cwd` override)
- Exits cleanly when stdin closes
- All memory operations use the same internal code paths as `legend memory *`

### Required JSON-RPC Methods

| Method | Type | Handler |
|--------|------|---------|
| `initialize` | Request | Return capabilities (tools only) |
| `notifications/initialized` | Notification | No-op |
| `tools/list` | Request | Return tool definitions |
| `tools/call` | Request | Dispatch to memory commands |
| `ping` | Request | Return `{}` |
| `resources/list` | Request | Return empty array |
| `prompts/list` | Request | Return empty array |
| Unknown method | Request | Return `-32601 Method not found` |

### Tool Call Dispatch

When `tools/call` is received, dispatch based on `params.name`:

| Tool Name | Internal Call |
|-----------|---------------|
| `legend_memory_start` | Same logic as `legend memory start` |
| `legend_memory_tick` | Same logic as `legend memory tick` with description from args |
| `legend_memory_query` | Same logic as `legend memory query` with topic from args |
| `legend_memory_task_get` | Same logic as `legend memory task` |
| `legend_memory_task_set` | Same logic as `legend memory task set` |
| `legend_memory_stats` | Same logic as `legend memory stats` |

### Error Handling

- Unknown tool → JSON-RPC error `-32602`
- Memory operation fails → Tool result with `isError: true` and error message
- Malformed JSON input → JSON-RPC error `-32700` (parse error)
- Missing required arguments → JSON-RPC error `-32602` (invalid params)

---

## Integration with Legend Bench

Once `legend mcp-serve` exists, the benchmark runner (`src/provider.rs`) can leverage it for the legend mode:

```rust
// In Provider::Claude, for legend mode:
if mode == "legend" {
    cmd.arg("--mcp-config")
       .arg(r#"{"mcpServers":{"legend-memory":{"command":"legend","args":["mcp-serve"]}}}"#);
}
```

This replaces the current approach where the model must remember to run Bash commands. The model sees `legend_memory_tick` as a tool and uses it naturally.

Note: `--allowedTools` may be needed in `-p` mode:
```rust
cmd.arg("--allowedTools")
   .arg("mcp__legend-memory__legend_memory_start,mcp__legend-memory__legend_memory_tick,mcp__legend-memory__legend_memory_query");
```

---

## Rollout Plan

### Phase 1: Build `legend mcp-serve`
- Implement the stdio JSON-RPC server as a new subcommand in the legend Rust binary
- Dispatch tool calls to existing memory internals (no code duplication)
- Handle all required MCP lifecycle methods
- Test manually: `echo '{"jsonrpc":"2.0","id":1,"method":"initialize",...}' | legend mcp-serve`

### Phase 2: Update `legend init`
- Generate `.mcp.json` for Claude Code
- Generate `.vscode/mcp.json` for VS Code Copilot
- Update `.gemini/settings.json` to include `mcpServers`
- Print new files in init output

### Phase 3: Test with AI tools
- Verify Claude Code discovers and uses legend tools via `.mcp.json`
- Verify Gemini CLI discovers tools via `.gemini/settings.json`
- Verify VS Code Copilot discovers tools via `.vscode/mcp.json`
- Verify tools work in non-interactive/print mode (`-p`)

### Phase 4: Update legend-bench
- Add `--mcp-config` to provider.rs for Claude legend mode
- Add `--allowedTools` for `-p` mode if required
- Verify TUI shows MCP tool calls (they appear as `tool_use` in stream-json)
- Compare benchmark results: hooks-only vs MCP tools

---

## Success Criteria

1. `legend mcp-serve` starts, completes MCP handshake, and responds to tool calls
2. `legend init` creates all MCP config files without errors
3. Claude Code auto-discovers legend tools when opening a project with `.mcp.json`
4. Model calls `legend_memory_start` at session start without explicit prompting
5. Model calls `legend_memory_tick` after significant actions without explicit prompting
6. Legend bench TUI shows `[Legend]` events from MCP tool calls during benchmark runs
7. No performance regression — MCP stdio adds < 10ms overhead per tool call

---

## Open Questions

1. **Should hooks be kept alongside MCP tools?** Hooks provide enforcement (nag reminders), MCP provides discoverability. Recommendation: keep both — they serve different purposes.

2. **Should `legend_memory_consolidate` be exposed?** Consolidation is expensive and rarely needed mid-session. Recommendation: omit from initial MCP tools, add later if models request it.

3. **Cursor MCP support?** Cursor uses `.cursor/mcp.json` — should `legend init` create this too? Need to verify Cursor's MCP config format.

4. **Tool descriptions tuning.** The tool descriptions heavily influence when models call them. May need iteration based on observed behavior in benchmarks.
