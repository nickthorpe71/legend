# Legend

**Lightweight context memory for AI-assisted development.**

Legend gives AI coding assistants persistent memory across sessions. It automatically tracks decisions, discoveries, and progress so your agent never loses context.

> For a deeper understanding of Legend's architecture and design principles, read the [Legend Paper](LEGEND_PAPER.md).

## Installation

### Quick Install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/nickthorpe71/legend/master/install.sh | bash
```

### Quick Install (Windows PowerShell)

```powershell
irm https://raw.githubusercontent.com/nickthorpe71/legend/master/install.ps1 | iex
```

### Download Binary

Prebuilt binaries are available on the [Releases page](https://github.com/nickthorpe71/legend/releases/latest):

| Platform | Binary |
|----------|--------|
| Linux x86_64 | `legend-linux-x86_64` |
| macOS x86_64 (Intel) | `legend-macos-x86_64` |
| macOS aarch64 (Apple Silicon) | `legend-macos-aarch64` |
| Windows x86_64 | `legend-windows-x86_64.exe` |

### Build from Source

Requires [Rust](https://rustup.rs/).

```bash
cargo install --git https://github.com/nickthorpe71/legend
```

### Update

Re-run the same install command to update to the latest version:

| Shell | Command |
|-------|---------|
| macOS / Linux | `curl -fsSL https://raw.githubusercontent.com/nickthorpe71/legend/master/install.sh \| bash` |
| PowerShell | `irm https://raw.githubusercontent.com/nickthorpe71/legend/master/install.ps1 \| iex` |
| Git Bash (Windows) | `curl -fsSL https://raw.githubusercontent.com/nickthorpe71/legend/master/install.sh \| bash` |
| CMD | `powershell -Command "irm https://raw.githubusercontent.com/nickthorpe71/legend/master/install.ps1 \| iex"` |

## Getting Started

### 1. Initialize Legend in your project

```bash
cd ~/my-project
legend init
```

This sets up:
- `.legend/` — compressed memory storage
- Agent hooks for **Claude Code**, **Codex**, and **Gemini CLI** (auto-loads context each session)
- Instruction injection for **VS Code Copilot**, **Cursor**, and **Zed**
- Protocol files (`CLAUDE.md`, `CODEX.md`, `GEMINI.md`, `AGENTS.md`) that teach your agent how to use Legend

Once initialized, your AI agent automatically loads Legend context at the start of every session.

### 2. Discover your codebase

When you start your first session with an AI agent, ask it:

> "Run `legend discover --apply` to scan this project and build initial context."

Legend will analyze your project's structure, manifests, entry points, git history, and documentation, then ingest the findings into memory. This gives your agent a head start on understanding the codebase.

### 3. Start working

That's it. Legend works in the background from here:

- **Session start** — your agent runs `legend memory start` to load context from prior sessions
- **During work** — your agent runs `legend memory tick "..."` to record decisions, bugs, and progress
- **Before unfamiliar tasks** — your agent runs `legend memory query "..."` to recall relevant context
- **Consolidation** — Legend automatically merges related memories into a long-term knowledge graph

Over time, Legend builds a rich, compressed history of your project that any compatible agent can draw from.

## Dashboard

Launch the memory visualization dashboard to explore your project's knowledge graph:

```bash
legend dashboard
```

## Memory Commands

These are primarily used by your AI agent automatically, but you can run them manually too:

| Command | Purpose |
|---------|---------|
| `legend memory start` | Load session context (run at session start) |
| `legend memory tick "<text>"` | Record a decision, discovery, or progress note |
| `legend memory query "<text>"` | Search memory for relevant context |
| `legend memory stats` | Check memory storage usage |
| `legend memory sessions` | View chronological session log |
| `legend memory consolidate` | Merge similar memories into long-term graph |
| `legend memory task set "<text>"` | Set the current task |

## How It Works

Legend stores memories in `.legend/memory.lz4` using LZ4 compression for fast (<5ms) reads. Memories are organized into three layers:

1. **Short-term** — recent ticks, high detail, decays over time
2. **Mid-term** — consolidated clusters of related memories
3. **Long-term** — a knowledge graph of entities, relationships, and architectural patterns

Salience-based retrieval ensures the most relevant memories surface first. Frequently accessed memories are automatically reinforced; stale ones decay naturally.

## License

MIT
