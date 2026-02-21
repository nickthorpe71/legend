<!-- legend-start -->
# Legend — Your Long-Term Memory

You have access to a persistent hierarchical memory system called **Legend**. It stores context across sessions so you can pick up where you left off. **Use it actively and frequently.**

## CRITICAL: Memory Workflow

### On every session start
Run this FIRST before doing anything else:
```bash
cargo run --quiet -- memory start
```
This returns everything in one call: stats, recent session log, top graph nodes, and relevant short-term memories. Read this to understand prior work, decisions, and open issues.

### During the session (frequently!)
After every significant action (writing code, making a decision, discovering something, completing a task), record it:
```bash
cargo run --quiet -- memory tick "description of what just happened"
```
Tick **decisions with rationale** ("Chose X over Y because Z"), not just progress.

### Before starting unfamiliar work
Query for relevant context before diving in:
```bash
cargo run --quiet -- memory query "topic you're about to work on"
```
The top result is automatically reinforced — frequently useful memories rise naturally.

### On session end
Summarize what was accomplished:
```bash
cargo run --quiet -- memory tick "Session summary: what was done, what's next, any blockers"
```

## Memory Commands

| Command | When to Use |
|---------|-------------|
| `cargo run --quiet -- memory start` | **Session start** — one call for full context |
| `cargo run --quiet -- memory tick "<text>"` | Record decision, progress, discovery, blocker |
| `cargo run --quiet -- memory query "<text>"` | Recall related context (auto-reinforces top result) |
| `cargo run --quiet -- memory reinforce <signal> <id...>` | Explicit feedback: 1.0 = useful, -1.0 = irrelevant |
| `cargo run --quiet -- memory stats` | Check storage usage |
| `cargo run --quiet -- memory sessions [n]` | View chronological session log |
| `cargo run --quiet -- memory consolidate` | Merge similar memories into long-term graph |

## Dashboard

Launch the live 3D memory visualization dashboard:
```bash
cargo run --quiet -- dashboard
```
This opens a native Windows app (cross-compiled from WSL) showing:
- 3D force-directed graph of knowledge nodes (right-drag to orbit, scroll to zoom)
- Live event log of all memory operations
- Memory stats, short-term entries with salience bars, session log

Launch it at session start so the user can watch memory activity in real-time.

## Feature Tracking Commands

| Command | Purpose |
|---------|---------|
| `cargo run --quiet -- get_state` | Load full project state as JSON |
| `cargo run --quiet -- search <query>` | Search features by keyword |
| `cargo run --quiet -- show` | Human-readable feature summary |
| `cargo run --quiet -- update` | Update feature state (pipe JSON to stdin) |

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
{
  "action": "created",       // "created", "merged", or "reconsolidated"
  "entry_id": 42,            // ID of the affected entry
  "matched_existing": null,  // ID if merged/reconsolidated, null if new
  "similarity": null,        // Match score if merged (0.0-1.0)
  "context": {...}         // Related memories
}
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
"decisions": {
  "items": [...],   // Top 5 by salience
  "showing": 5,
  "total": 12       // 12 total - use --category to see all
}
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
<!-- legend-end -->
