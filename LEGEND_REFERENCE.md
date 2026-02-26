# Legend — Complete Technical Reference
**Version:** 0.3.0  **Author:** Nick Thorpe  **Language:** Rust (2021 edition)  **Date:** 2026-02-24

---

## Table of Contents

1. [What Legend Is](#1-what-legend-is)
2. [Why It Exists — The Problem It Solves](#2-why-it-exists)
3. [High-Level Architecture](#3-high-level-architecture)
4. [The Three-Layer Memory System](#4-the-three-layer-memory-system)
5. [The Embedding System](#5-the-embedding-system)
6. [Salience Scoring](#6-salience-scoring)
7. [Entity Extraction & The Knowledge Graph](#7-entity-extraction--the-knowledge-graph)
8. [Extractive Summarization](#8-extractive-summarization)
9. [The Memory Lifecycle](#9-the-memory-lifecycle)
10. [Memory Retrieval & Associative Priming](#10-memory-retrieval--associative-priming)
11. [Reinforcement — AdaGrad & Hebbian Learning](#11-reinforcement--adagrad--hebbian-learning)
12. [Consolidation](#12-consolidation)
13. [Decay & Garbage Collection](#13-decay--garbage-collection)
14. [CLI Command Reference](#14-cli-command-reference)
15. [The Hook System & AI Integration](#15-the-hook-system--ai-integration)
16. [Project Discovery & Feature Tracking](#16-project-discovery--feature-tracking)
17. [Dashboards](#17-dashboards)
18. [Persistence & Storage](#18-persistence--storage)
19. [Session Quality Metric](#19-session-quality-metric)
20. [Token Overhead Estimation](#20-token-overhead-estimation)
21. [Benefits](#21-benefits)
22. [Current Limitations](#22-current-limitations)
23. [Key Design Decisions & Rationale](#23-key-design-decisions--rationale)
24. [File Structure & Dependencies](#24-file-structure--dependencies)
25. [Performance Characteristics](#25-performance-characteristics)
26. [All Constants — Reference Table](#26-all-constants--reference-table)

---

## 1. What Legend Is

Legend is a **hierarchical, persistent memory system** for AI-assisted software development. It is a Rust CLI tool (`legend`) that runs alongside AI coding assistants — primarily Claude Code, but also Codex, Gemini CLI, VS Code Copilot, Cursor, and Zed — and provides long-term memory that persists across every conversation and session.

When Legend is installed in a project, it:

- **Automatically injects** project context at the start of every session via shell hooks.
- **Captures decisions, bugs, architecture insights, and preferences** as memory "ticks" during a session.
- **Retrieves the most relevant stored memories** when the AI is about to work on something new.
- **Builds a persistent knowledge graph** of code entities and their relationships over time.
- **Filters noise** from auto-generated telemetry to ensure high-signal context reaches the AI.
- **Visualizes** the memory state via a live terminal dashboard or a 3D Bevy application.

Legend is entirely self-contained: a single binary, zero network calls, deterministic n-gram embeddings, and LZ4-compressed binary storage.

---

## 2. Why It Exists — The Problem It Solves

Modern AI coding assistants are stateless. Each conversation begins with a blank context window.

**Without Legend:**
- The developer must manually re-explain project context at the start of each session.
- The AI repeats mistakes it already made and "fixed" in a prior session.
- Architectural decisions made three days ago are forgotten and contradicted.
- The developer spends 5–20% of each session just getting the AI back up to speed.

**With Legend:**
- Session start injects ~1,100 tokens of high-signal, pre-filtered context automatically.
- The AI knows what decisions were made, what was tried and rejected, and what the user prefers.

Real measured value: test-game estimated **+53,400 token net savings** over 88 sessions; spritec **+27,600** over 20 sessions.

---

## 3. High-Level Architecture

```
┌──────────────────────────────────────────────────────┐
│                   AI Assistant (Claude Code, etc.)    │
│  ┌───────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │SessionStart│  │UserPrompt   │  │PostToolUse /  │  │
│  │   Hook    │  │Submit Hook  │  │Stop Hook      │  │
│  └─────┬─────┘  └──────┬───────┘  └──────┬────────┘  │
└────────┼───────────────┼─────────────────┼────────────┘
         ▼               ▼                 ▼
┌──────────────────────────────────────────────────────┐
│                    Legend CLI Binary                  │
└──────────────────────┬───────────────────────────────┘
         ┌─────────────┼─────────────┐
         ▼             ▼             ▼
   .legend/        .legend/      .legend/
   memory.lz4      state.lz4     events.jsonl
   (MemoryState)   (Feature      (Event log)
   Layer 1-3       tracking)     Dashboard feed
```

---

## 4. The Three-Layer Memory System

### Layer 1 — Immediate Buffer
- **Type:** `VecDeque<String>` (FIFO ring buffer)
- **Capacity:** 256 entries
- **Purpose:** Working memory. No decay — FIFO eviction only. Evicted entries are embedded at 0.7x salience and inserted into Layer 2.

### Layer 2 — Short-Term Vector Store
- **Type:** `Vec<ShortTermEntry>`, **Capacity:** 1,024 entries
- Each entry: `id`, `text`, `summary`, `embedding` (256-dim), `salience`, `usage`, `last_access`, `reconsolidation_count`, `labile_until`, `refs`, `gradient_sq_sum`
- **Decay:** Exponential, `salience *= exp(-age x 0.001)`. Half-life ~693 ticks.

### Layer 3 — Long-Term Knowledge Graph
- **Nodes:** 2,048 cap. **Edges:** 8,192 cap.
- Node fields: `id`, `label`, `kind`, `weight`, `salience`, `last_seen`
- Edge fields: `from`, `to`, `weight`, `kind` (contains/depends-on/implements/co-defined/related), `last_seen`
- **Decay:** Half-life ~1,386 ticks (twice as durable as Layer 2).

### Supporting State
- `clock` — monotonic counter; "age" = `clock - last_access`
- `session_log` — chronological log of ticks, capped at 100 entries
- `current_task` — pinned task shown at session start
- `ticks_since_consolidation` — triggers auto-consolidation at 15

---

## 5. The Embedding System

Uses **n-gram hashing** — not neural embeddings. No external dependencies, sub-millisecond latency, fully deterministic.

### How `embed_text()` Works (256-dim output):
1. **Tokenize:** lowercase + split by whitespace
2. **Word unigrams:** FNV-1a hash -> bucket, `vector[index] += 1.0`
3. **Character trigrams:** 3-char window per token, `vector[index] += 0.5` (makes "memory"/"memories" similar)
4. **Word bigrams:** pairs of tokens, `vector[index] += 0.75`
5. **L2 normalize** -> unit vector; cosine similarity = dot product

### Dual Threshold System:
| Similarity | Word overlap >= 30% | Action |
|---|---|---|
| >= 0.92 | Yes | **Reinforce** — boost salience, no new entry |
| >= 0.55 | Yes | **Merge** — average embeddings |
| < 0.55 | Any | **Insert** — create new entry |

---

## 6. Salience Scoring

`compute_salience(text)` content-based heuristics:

| Pattern | Boost |
|---|---|
| Decision (2+ keywords: chose, decided, rejected...) | +0.5 |
| Decision + rationale ("because") | +0.65 total |
| Bug / incident (crash, panic, regression...) | +0.4 |
| TODO / blocker | +0.3 |
| User preference | +0.3 |
| Architecture | +0.25 |
| Code reference (``` fn struct) | +0.15 |
| Long text (50+ words) | +0.20 |

Clamped to [0.05, 1.0]. A DECISION tick scores ~0.95; a generic progress note ~0.05–0.15.

---

## 7. Entity Extraction & The Knowledge Graph

**Phase 1 — Code-aware:** Detects `fn`, `struct`, `enum`, `trait`, `impl`, `mod`, `use`, `def`, `class`, `function` prefixes -> creates Function/Struct/Enum/Trait/Module nodes.

**Phase 2 — Plain identifiers:** Extracts alphanumeric tokens > 2 chars, not stopwords, not numeric. Shape inference: `UpperCase` -> Type, `has_underscore` -> Symbol, `lowercase` -> Term.

**~170 stopwords** filter noise (the, and, tool, session, tick, memory, legend, etc.).

**Edge creation:** Co-occurring entities in a tick -> edge weight += 0.1. Edge kinds: contains / depends-on / implements / co-defined / related.

**Graph normalization** every 5 ticks prevents dominant clusters.

---

## 8. Extractive Summarization

Sentence scoring: `word_count + (has_code_symbol ? 5) + (has_key_symbol ? 3) + (has_decision_keyword ? 8)`

- `summarize_single` -> max 200 chars (best sentence)
- `summarize_group` -> top 3 entries by salience+usage, joined with ` | `, max 300 chars
- `chunk_text` -> ~200-char chunks respecting line boundaries

---

## 9. The Memory Lifecycle (tick pipeline)

1. Increment clock, apply decay, stabilize labile entries
2. Append to session log (capped at 100)
3. Chunk input into ~200-char pieces
4. For each chunk:
   - Push to L1 FIFO (evict oldest if full -> insert into L2 at 0.7x salience)
   - Embed + compute salience
   - **Reconsolidation check:** if a labile entry matches (sim >= 0.35, overlap >= 0.1) -> merge in-place, skip normal path
   - **Normal path:** find top-1 similar, apply dual-threshold -> reinforce / merge / insert
5. Prune L2 and L3

---

## 10. Memory Retrieval & Associative Priming

1. Embed query, apply decay
2. Search L2 by cosine similarity -> top 5. Mark returned entries as **labile** (`labile_until = clock + 5`).
3. Auto-reinforce top result: `salience += similarity x 0.03`
4. L3 graph lookup: match entities from query -> expand 1 hop
5. **Associative priming:** extract entities from top L2 results -> expand 1 hop through graph at 0.7x weight (surfaces structurally related concepts not in the query text)
6. Deduplicate, sort by weight, cap at 15 graph nodes
7. **Hebbian reinforcement:** co-retrieved node pairs get edge weight += 0.05; each node gets +0.02

---

## 11. Reinforcement — AdaGrad & Hebbian Learning

**Explicit:** `legend memory reinforce <signal> <id...>` (signal in [-1.0, 1.0])
- Contrastive descent: other retrieved entries get -0.02 penalty
- AdaGrad: `lr = 0.15 / sqrt(gradient_sq_sum + 1e-6)`, then `salience += signal x lr`
- Prevents saturation; entries with more feedback history get smaller updates

**Implicit Hebbian:** Every `retrieve_context()` co-reinforces edge weights and node weights automatically.

---

## 12. Consolidation

`legend memory consolidate` (also auto-runs at 15 ticks):
1. Cluster L2 entries by cosine similarity >= 0.55
2. Groups with > 1 member -> `summarize_group()` -> create L3 Summary node (weight = 1.0 + group_salience)
3. Prune L2 and L3

Summary nodes survive longer in L3 (high weight, slow decay) and surface in future queries.

---

## 13. Decay & Garbage Collection

- **L2 decay:** `salience *= exp(-age x 0.001)` every tick
- **L3 decay:** `weight *= exp(-age x 0.0005)` (half-life ~1,386 ticks)
- **L2 pruning:** `score = salience + (usage x 0.05) - (age x 0.001) < 0.1` -> remove
- **L3 pruning:** nodes where `(weight - age x 0.001) < 0.05` -> remove; enforce capacity limits; remove orphan edges
- **Eviction score:** `salience x 0.4 + ln(1+usage) x 0.3 + recency x 0.3` (lowest score evicted first)
- **Salience renormalization:** every 10 ticks, gentle EMA blend toward normalized values (10% blend)

---

## 14. CLI Command Reference

### Top-Level
| Command | Purpose |
|---|---|
| `legend init [--discover]` | Initialize, install hooks, write instruction files |
| `legend show` | Human-readable feature table |
| `legend get_state` | Export LegendState as JSON |
| `legend discover [path] [--apply]` | Scan project; with --apply, ingest high-signal files |
| `legend dashboard` | Launch TUI dashboard |
| `legend dashboard --3d` | Launch Bevy 3D dashboard |

### Memory Subcommands
| Command | Purpose |
|---|---|
| `legend memory start` | Session startup: protocol + recent activity + categorized memories |
| `legend memory start --tokens` | Show token overhead estimate |
| `legend memory start --category <name>` | decisions / architecture / bugs / todos / preferences |
| `legend memory tick "<text>"` | Record memory, run full tick pipeline |
| `legend memory tick --blocker` | Prepend "BLOCKER:", boost salience +0.4 |
| `legend memory tick --passive` | Prepend "EXPERIENCE:", halve salience, skip session log |
| `legend memory query "<text>"` | Retrieve top 5 L2 + 15 L3 nodes, auto-reinforce top |
| `legend memory reinforce <signal> <id...>` | Explicit feedback [-1.0, 1.0] |
| `legend memory consolidate` | Manual cluster-and-promote |
| `legend memory sessions [n]` | View session log |
| `legend memory stats` | Storage metrics + session quality score |
| `legend memory task set/clear` | Pin/clear current task |
| `legend memory dump` | Export full MemoryState as JSON |
| `legend memory reset` | Wipe memory store |

### Category System
Decision / Bug / Todo / Architecture / Preference / Progress / General. Progress and General are omitted from `memory start` output (low signal).

### ARCHITECTURE.md Auto-Generation
Any tick starting with "ARCHITECTURE:" auto-appends a timestamped entry to `ARCHITECTURE.md`.

---

## 15. The Hook System

### Hook 1 — SessionStart
```bash
legend memory start && touch .legend/.session_active
```

### Hook 2 — UserPromptSubmit (per prompt)
- If `.session_active` is > 2 hours old -> re-run `memory start`
- Else -> emit reflection checkpoint, detect task-starting verbs (implement/add/fix/create/...) and mandate a query, auto-run `memory query "$PROMPT"`

### Hook 3 — PostToolUse (per file edit)
- Detects write/edit/create tools
- Increments `.legend/.pending_ticks`
- Escalation: 1 = info, 2-3 = WARNING, 4+ = CRITICAL
- Resets to 0 on any `memory tick`

### Hook 4 — Stop / SessionEnd
- If no tick this session -> CRITICAL alert
- If ticks exist + uncommitted files -> summary mandate

### Supported Assistants
Claude Code, Codex, Gemini CLI (shell hooks) + VS Code Copilot, Cursor, Zed (instruction injection only).

---

## 16. Project Discovery & Feature Tracking

**Auto-Discovery:** Walks project tree, identifies high-signal files (README, ARCHITECTURE, manifests, entry points), detects features from subdirectory structure. `--apply` ingests files as passive ticks.

**Feature Tracking** (`.legend/state.lz4`): Each feature has `id`, `name`, `domain`, `tags`, `status` (Pending/InProgress/Blocked/Complete), `description`, `files_involved`, `recency_score` (7-day half-life exponential decay).

---

## 17. Dashboards

### Ratatui TUI (`legend dashboard`)
Three views (Tab to switch): Short-Term Memory list with salience bars | Knowledge Graph nodes by weight | Event log from events.jsonl. Search with `/`, inline query with `:`. Auto-refreshes every 2-3 seconds.

### Bevy 3D Dashboard (`legend dashboard --3d`)
Force-directed 3D graph. Nodes = sphere meshes with emissive glow proportional to salience, colored by kind. Edges = gizmo lines by type. Orbit camera, raycast click to inspect. In WSL: compiles to Windows .exe and launches via `cmd.exe`.

---

## 18. Persistence & Storage

### Serialization Stack
```
Rust struct -> bincode::serialize() -> lz4::block::compress() -> write .tmp -> atomic rename to .lz4
```

**Corruption recovery:** Corrupt file -> renamed to `.lz4.corrupt` -> fresh default returned.

**Migration:** Three format versions. `load_or_default()` tries each migration path transparently.

### Event Log (`events.jsonl`)
Append-only JSONL. Every tick/query/consolidation/start logs one line with `ts`, `cmd`, `detail`, `data`. Rotates to `events.jsonl.1` at 10,000 lines.

---

## 19. Session Quality Metric

`legend memory stats` computes quality score from events.jsonl since last "start" event:

```
signal_score        = (meaningful_ticks / total_ticks) x 50
query_score         = (queries / 3.0).min(1.0) x 30
consolidation_score = consolidations > 0 ? 20 : 0
quality_score       = sum (0-100)
```

Thresholds: < 40 [LOW] | 40-70 [OK] | >= 70 [GOOD]

---

## 20. Token Overhead Estimation

`legend memory start --tokens` prints estimates: session start injection (`len/4`), per-prompt hooks (~100 tokens each), per-edit hooks (~150 tokens each).

Observed: legend self ~1,099/session | test-game ~1,112/session | spritec ~345/session (sparse memory).

---

## 21. Benefits (Verified)

1. Session continuity at low cost (~1,100 tokens vs. 3,000-8,000 manual re-explanation)
2. Decision archaeology — day-1 decisions accessible on day 7
3. Rejected approach memory — dead ends documented, not retried
4. Bug root-cause preservation
5. Quantitative detail retention (metrics, dimensions, ratios)
6. User preference adaptation across sessions
7. Architectural pivot management across session boundaries
8. Zero infrastructure — single binary, no cloud, no API keys
9. Deterministic behavior — no variation, no rate limits
10. Living architecture documentation via ARCHITECTURE: ticks

---

## 22. Current Limitations

1. **Lexical-only similarity** — "auth" and "authentication" may not cluster
2. **No native token counting** — +/-30% uncertainty on estimates
3. **EXPERIENCE: tick noise** — hook can generate high-volume auto-noise (spritec: 97.9% noise)
4. **Session fragmentation overhead** — 30+ short sessions/day inflates startup costs
5. **Query under-utilization** — actual rate 0.10-0.73/session vs. target 1+
6. **No semantic deduplication** — near-duplicate ticks in different words create separate entries
7. **Zero `memory reinforce` calls observed** in practice across all three projects
8. **Single-process design** — no concurrent access support, no locking
9. **Coarse entity extraction** — no parser, generics/closures/decorators not captured
10. **No cross-session quality tracking** — only current session visible
11. **Fixed capacity** — all limits hard-coded, no config without recompile
12. **Dashboard requires separate build** — 3D Bevy crate built independently

---

## 23. Key Design Decisions & Rationale

1. **No external embedding model** — chosen for zero dependencies, determinism, sub-ms latency (lexical overlap is reasonable signal for technical dev notes)
2. **Dual threshold (0.92 / 0.55)** — arrived at empirically; allows nuanced reinforce-vs-merge handling
3. **Diversity gate (Jaccard >= 0.30)** — prevents hash collision false positives from merging unrelated memories
4. **Reconsolidation window (5 ticks)** — inspired by neuroscience; related follow-up updates merge into existing memory rather than duplicating
5. **Salience-driven eviction over LRU** — preserves important old decisions over recent-but-trivial notes
6. **Associative priming** — models human associative memory; bridges text similarity (L2) with structural relationships (L3)
7. **Atomic writes only** — write-to-temp-rename pattern; guaranteed valid or empty file on interrupt
8. **Bincode + LZ4 over JSON** — 10x faster serialization; 2x-31x compression ratio observed

---

## 24. File Structure & Dependencies

```
src/
├── main.rs                   (64 lines)   CLI routing
├── types.rs                 (169 lines)   Feature, LegendState
├── storage.rs                (91 lines)   LegendState persistence
├── commands/
│   ├── init.rs             (~700 lines)   Hook installation, instruction files
│   ├── memory.rs           (~900 lines)   All memory subcommands
│   ├── dashboard.rs        (107 lines)    Dashboard launcher
│   ├── discover.rs         (~400 lines)   Project scanning
│   └── ...others
├── memory/
│   ├── mod.rs             (2,725 lines)   Core three-layer engine
│   ├── embed.rs            (~200 lines)   N-gram embeddings, FNV hash
│   ├── extract.rs          (~300 lines)   Entity extraction
│   └── summarize.rs        (~200 lines)   Extractive summarization
└── tui/mod.rs              (~500 lines)   Ratatui TUI

dashboard/  (separate crate — Bevy 0.15 + bevy_egui 0.33)
```

**Total main binary: ~7,785 lines of Rust.**

**Runtime deps:** serde, serde_json, bincode, lz4, ratatui, crossterm
**Test coverage: 91 tests, all passing**

---

## 25. Performance Characteristics

| Operation | Target |
|---|---|
| `memory tick` | < 5ms |
| `memory query` | < 5ms |
| `memory start` | < 10ms |
| `memory consolidate` | < 50ms |
| Load `memory.lz4` (300KB) | < 3ms |
| Cosine similarity scan (1,024 entries) | < 1ms |

**Storage observed:** legend self: 302KB + 609KB events = ~911KB | test-game: 108KB + 980KB events = ~1.1MB | spritec: 12KB + 368KB events = ~380KB

---

## 26. All Constants — Reference Table

| Constant | Value | Purpose |
|---|---|---|
| `immediate_capacity` | 256 | L1 FIFO max |
| `short_term_capacity` | 1,024 | L2 max entries |
| `embedding_dim` | 256 | n-gram vector dimension |
| `theta_high` | 0.92 | Reinforce threshold |
| `theta_low` | 0.55 | Merge threshold |
| `SHORT_TERM_DECAY_RATE` | 0.001 | L2 decay per tick |
| `LONG_TERM_DECAY_RATE` | 0.0005 | L3 decay per tick |
| `HEBBIAN_EDGE_BOOST` | 0.05 | Edge weight per co-retrieval |
| `HEBBIAN_NODE_BOOST` | 0.02 | Node weight per co-retrieval |
| `PRUNE_THRESHOLD` | 0.1 | Min L2 retention score |
| `MERGE_WORD_OVERLAP_THRESHOLD` | 0.3 | Min Jaccard for merge |
| `SESSION_LOG_CAPACITY` | 100 | Max session log entries |
| `GRAPH_NODE_CAPACITY` | 2,048 | L3 node cap |
| `GRAPH_EDGE_CAPACITY` | 8,192 | L3 edge cap |
| `RECONSOLIDATION_THRESHOLD` | 0.35 | Min sim for labile update |
| `LABILE_WINDOW` | 5 | Ticks entry stays editable |
| `CONSOLIDATION_SUGGESTION_THRESHOLD` | 15 | Ticks between auto-consolidation |
| `ADAGRAD_BASE_LR` | 0.15 | Base learning rate |
| `RENORM_INTERVAL` | 10 | Ticks between EMA normalization |
| `EVENT_LOG_MAX_LINES` | 10,000 | Rotate events.jsonl at |
| Session TTL | 7,200 sec | After which hook re-runs memory start |
| Tick pending (warning) | 2 | at 2+ un-ticked edits |
| Tick pending (critical) | 4 | at 4+ un-ticked edits |
| Feature recency half-life | 7 days | Decay for feature recency score |

---

*End of report. Document generated 2026-02-24 from Legend v0.3.0 source code.*
