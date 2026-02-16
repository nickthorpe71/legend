<!-- legend-start -->
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
<!-- legend-end -->
