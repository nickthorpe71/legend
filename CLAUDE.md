<!-- legend-start -->
# SYSTEM_MANDATE: Legend — Your Long-Term Memory

You MUST use the **Legend** hierarchical memory system to maintain context across sessions. **This is a technical requirement for this project.**

## CRITICAL: Memory Workflow

### 1. Session Start (REQUIRED)
Run this FIRST before doing anything else in every new session:
```bash
cargo run --quiet -- memory start
```
This returns the "MANDATORY PROTOCOL", stats, recent session log, top graph nodes, and relevant short-term memories. You MUST read this to understand prior work and decisions.

### 2. During Session (FREQUENTLY)
After every significant action (writing code, making a decision, discovering something, completing a task), record it:
```bash
cargo run --quiet -- memory tick <<'EOF'
description of what just happened
EOF
```
**IMPORTANT:** Always use heredoc (`<<'EOF'`) for tick and query messages. This prevents bash from interpreting backticks, apostrophes, or special characters in your text.
You MUST tick **decisions with rationale** ("Chose X over Y because Z"), not just progress.

### 3. Before New Tasks (REQUIRED)
Query for relevant context before diving into unfamiliar work:
```bash
cargo run --quiet -- memory query <<'EOF'
topic you are about to work on
EOF
```
The top result is automatically reinforced — frequently useful memories rise naturally.

### 4. Session End (REQUIRED)
Summarize what was accomplished:
```bash
cargo run --quiet -- memory tick <<'EOF'
Session summary: what was done, what is next, any blockers
EOF
```

## Memory Commands

| Command | When to Use |
|---------|-------------|
| `cargo run --quiet -- memory start` | **Session start** — one call for full context |
| `cargo run --quiet -- memory tick <<'EOF'`...`EOF` | Record decision, progress, discovery, blocker |
| `cargo run --quiet -- memory query <<'EOF'`...`EOF` | Recall related context (auto-reinforces top result) |
| `cargo run --quiet -- memory reinforce <signal> <id...>` | Explicit feedback: 1.0 = useful, -1.0 = irrelevant |
| `cargo run --quiet -- memory stats` | Check storage usage |
| `cargo run --quiet -- memory sessions [n]` | View chronological session log |
| `cargo run --quiet -- memory consolidate` | Merge similar memories into long-term graph |

## Dashboard

Launch the live 3D memory visualization dashboard:
```bash
cargo run --quiet -- dashboard
```

## When to Tick (Priority Context)

**You MUST tick these:**
- Decisions with rationale: "DECISION: Chose X over Y because Z"
- Bug discoveries: "BUG: X fails when Y happens"
- Architecture insights: "Module X communicates with Y via Z"
- Blockers: "BLOCKER: Can't proceed until X is resolved"
- User preferences: "User prefers X approach"
- Completed features: "Implemented X in file Y"
- **Discussion conclusions**: "Discussed X with user, agreed on Y approach"
- **Discoveries without file changes**: "Found that X module does Y"
- **Rejected approaches**: "Decided against X because Y"

**Tick frequency:** Aim for 3-8 ticks per session. After major decisions or substantial work.

## Understanding Start Output

By default, `cargo run --quiet -- memory start` returns a high-signal Markdown summary designed for YOUR consumption. It contains the **LEGEND PROTOCOL (MANDATORY)** which you must follow.
<!-- legend-end -->
