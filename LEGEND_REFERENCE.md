# Legend — Complete Technical Reference
**Version:** 0.3.9  **Author:** Nick Thorpe  **Language:** Rust (2021 edition)  **Date:** 2026-04-05

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
14. [Pattern Separation & Completion](#14-pattern-separation--completion)
15. [Emotional Valence](#15-emotional-valence)
16. [Keyword System](#16-keyword-system)
17. [CLI Command Reference](#17-cli-command-reference)
18. [The Hook System & AI Integration](#18-the-hook-system--ai-integration)
19. [Project Discovery & Feature Tracking](#19-project-discovery--feature-tracking)
20. [Dashboards](#20-dashboards)
21. [Persistence & Storage](#21-persistence--storage)
22. [Session Quality Metric](#22-session-quality-metric)
23. [Token Overhead Estimation](#23-token-overhead-estimation)
24. [Benefits](#24-benefits)
25. [Current Limitations](#25-current-limitations)
26. [Key Design Decisions & Rationale](#26-key-design-decisions--rationale)
27. [File Structure & Dependencies](#27-file-structure--dependencies)
28. [Performance Characteristics](#28-performance-characteristics)
29. [All Constants — Reference Table](#29-all-constants--reference-table)

---

## 1. What Legend Is

Legend is a **hierarchical, persistent memory system** for AI-assisted software development. It is a Rust CLI tool (`legend`) that runs alongside AI coding assistants — primarily Claude Code, but also Codex, Gemini CLI, VS Code Copilot, Cursor, and Zed — and provides long-term memory that persists across every conversation and session.

The codebase is organized around a **brain-region architecture**: each cognitive mechanism is implemented in a module named after its neuroscience analog (prefrontal cortex, hippocampus, neocortex, amygdala, dentate gyrus, basal ganglia, thalamus, entorhinal cortex, Wernicke's area). This split enforces separation of concerns between pure cognitive logic (`src/memory/`) and IO/persistence (`src/tool/`).

When Legend is installed in a project, it:

- **Automatically injects** project context at the start of every session via shell hooks.
- **Captures decisions, bugs, architecture insights, and preferences** as memory "ticks" during a session.
- **Retrieves the most relevant stored memories** with multi-hop spreading activation and pattern completion.
- **Builds a persistent knowledge graph** of code entities and their relationships over time.
- **Filters noise** through attention gating, emotional valence, and pattern separation.
- **Visualizes** the memory state via a live terminal dashboard or a 3D Bevy application.

Legend is entirely self-contained: a single binary, zero network calls, deterministic n-gram embeddings, and LZ4-compressed MessagePack storage.

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
│  ┌─────────────────┐  ┌──────────────────────────┐   │
│  │  src/memory/     │  │  src/tool/               │   │
│  │  (Pure Brain)    │  │  (IO + Persistence)      │   │
│  │  No filesystem   │  │  Save/load, git sync,    │   │
│  │  No network      │  │  session logs, CLI/MCP   │   │
│  └─────────────────┘  └──────────────────────────┘   │
└──────────────────────┬───────────────────────────────┘
                       ▼
                 .legend/
                 memory.lz4      events.jsonl
                 (MemoryState)   (Event log)
```

---

## 4. The Three-Layer Memory System

### Layer 1 — Working Memory (Prefrontal Cortex)
- **Type:** `Vec<WorkingMemoryEntry>` with attention gating
- **Capacity:** 10 entries (~7±2, matching Miller's Law)
- **Purpose:** Limited-capacity buffer queried first during retrieval. Only entries with salience ≥ `ATTENTION_GATE_THRESHOLD` (0.25) are promoted to L2.
- **Context-switch flushing:** When cosine similarity between consecutive ticks drops below 0.15, L1 is flushed and unpromoted entries get a final promotion opportunity.

### Layer 2 — Episodic Memory (Hippocampus)
- **Type:** `Vec<ShortTermEntry>`, **Capacity:** 1,024 entries
- Each entry: `id`, `text`, `summary`, `embedding` (256-dim), `salience`, `usage`, `last_access`, `reconsolidation_count`, `labile_until`, `refs`, `gradient_sq_sum`, `density`, `consolidated`, `emotional_valence`, `stability`, `last_retrieval_interval`
- **Decay:** Exponential, `salience *= exp(-age × 0.001 / stability)`. Base half-life ~693 ticks, extended by Ebbinghaus stability (1.0–10.0). Stability grows with spaced retrieval.
- **Emotional valence:** Decays at half the hippocampal rate (emotional memories resist forgetting).

### Layer 3 — Knowledge Graph (Neocortex)
- **Nodes:** 2,048 cap. **Edges:** 8,192 cap.
- Node fields: `id`, `label`, `kind`, `weight`, `salience`, `last_seen`, `source_texts`, `embedding` (centroid for Summary nodes), `full_text` (rich consolidated text)
- Edge fields: `from`, `to`, `weight`, `kind` (contains/depends-on/implements/co-defined/related/drives/represents), `last_seen`, `activation_count`, `stability` (caps 10.0), `recent_interval_avg` (STP α=0.5), `historical_interval_avg` (LTP α=0.1)
- **Decay:** Half-life ~1,386 ticks (rate 0.0005, twice as durable as Layer 2).

### Supporting State
- `clock` — monotonic counter; "age" = `clock - last_access`
- `session_log` — chronological log of ticks, capped at 100 entries
- `current_task` — pinned task shown at session start
- `last_synced_sha` — Git commit SHA for cold-start reconciliation
- `recent_valence_sum` — rolling emotional intensity for consolidation triggers
- `last_tick_embedding` — previous tick's embedding for context-switch detection
- `term_frequency` — L3 incremental keyword discovery tracking
- `ticks_since_consolidation` — triggers auto-consolidation at 15

---

## 5. The Embedding System

Uses **n-gram hashing** — not neural embeddings. No external dependencies, sub-millisecond latency, fully deterministic.

### How `embed_text()` Works (256-dim output):
1. **Tokenize:** lowercase + split by whitespace
2. **Word unigrams:** FNV-1a hash → bucket, `vector[index] += 1.0`
3. **Character trigrams:** 3-char window per token, `vector[index] += 0.3` (makes "memory"/"memories" similar)
4. **Word bigrams:** pairs of tokens, `vector[index] += 0.75`
5. **L2 normalize** → unit vector; cosine similarity = dot product

### Dual Threshold System:
| Similarity | Word overlap ≥ 40% | Action |
|---|---|---|
| ≥ 0.88 | Yes | **Reinforce** — boost salience, no new entry |
| ≥ 0.72 | Yes | **Merge** — average embeddings |
| < 0.72 | Any | **Insert** — create new entry |

---

## 6. Salience Scoring

`compute_salience(text, keyword_cache)` content-based heuristics using the dynamic `KeywordCache`:

| Pattern | Boost |
|---|---|
| Decision (2+ keywords: chose, decided, rejected...) | +0.5 |
| Decision (1 keyword) | +0.3 |
| Decision + rationale ("because") | +0.15 additional |
| Bug / incident (crash, panic, regression...) | +0.4 |
| TODO / blocker | +0.3 |
| User preference | +0.3 |
| Architecture | +0.25 |
| Domain-specific vocabulary (learned from workspace) | +0.1 |
| Multiple code definitions (2+) | +0.3 |
| Single code definition | +0.2 |
| Code block (```) | +0.15 |
| Error mention | +0.15 |
| Substantive text (25+ words) | +0.15 |
| Long text (50+ words) | +0.20 |

Clamped to [0.05, 1.0]. A DECISION tick with rationale scores ~0.95; a generic progress note ~0.05–0.15.

---

## 7. Entity Extraction & The Knowledge Graph

**Phase 1 — Code-aware (Wernicke's area):** Detects `fn`, `struct`, `enum`, `trait`, `impl`, `mod`, `use`, `def`, `class`, `function`, `func`, `interface` prefixes → creates Function/Struct/Enum/Trait/Module nodes. Also detects file paths, action verbs, and environment markers.

**Phase 2 — Plain identifiers:** Extracts alphanumeric tokens > 2 chars, not stopwords, not numeric. Shape inference: `UpperCase` → Type, `has_underscore` → Symbol, `lowercase` → Term.

**~288 static keywords** across decision, action, architecture, bug, TODO, preference, and code categories. Plus dynamic domain keywords learned from workspace.

**Edge creation:** Co-occurring entities in a tick → edge weight += 0.1. Seven edge kinds: contains / depends-on / implements / co-defined / related / drives / represents.

**Graph normalization** every 5 ticks prevents dominant clusters (weight ceiling 2.0).

---

## 8. Extractive Summarization (Entorhinal Cortex)

Sentence scoring: `word_count + (has_code_symbol ? 5) + (has_key_symbol ? 3) + (has_decision_keyword ? 8) + (has_arch_keyword ? 4)`

- `summarize_single` → max 200 chars (best sentence)
- `summarize_group` → top 3 entries by salience+usage, joined with ` | `, max 300 chars
- `chunk_text` → ~200-char chunks respecting line boundaries

---

## 9. The Memory Lifecycle (tick pipeline)

1. Increment clock, apply decay, stabilize labile entries, renormalize salience (every 10 ticks), normalize graph weights (every 5 ticks), decay rolling emotional intensity (×0.8)
2. Append to session log (capped at 100, non-passive ticks only)
3. Chunk input into ~200-char pieces (entorhinal cortex)
4. For each chunk:
   - Push to L1 working memory (prefrontal cortex)
   - Embed + compute salience (thalamus) + compute emotional valence (amygdala)
   - **Attention gate:** salience ≥ 0.25 → promote to L2 path; else stay in L1 only
   - **Pattern separation:** sparse_orthogonalize against similar-but-distinct L2 entries (dentate gyrus)
   - **Reconsolidation check:** if a labile entry matches (sim ≥ 0.35) → merge in-place, skip normal path
   - **Normal path:** find top-1 similar, apply dual-threshold → reinforce / merge / insert
5. Update graph (neocortex) — extract entities (Wernicke), create/update nodes and edges
6. Retrieve context for priming
7. Prune L2 and L3
8. Context-switch detection: if cosine sim to previous tick < 0.15, flush L1

---

## 10. Memory Retrieval & Associative Priming

1. Embed query, apply decay
2. Search L2 by cosine similarity + keyword bonus → top 5. Mark returned entries as **labile** (`labile_until = clock + 5`). Minimum similarity threshold: 0.15 (noise floor). Keyword bonus: up to 0.2.
3. Auto-reinforce top result: `salience += similarity × 0.03`
4. **Pattern completion (CA3):** If top result sim < 0.5 or < 3 results, extract entities from partial matches → spreading activation through graph → search L2 for entries containing activated entities
5. L3 graph lookup: match entities from query → **multi-hop spreading activation** (up to 3 hops, 0.5× decay per hop)
6. **Associative priming:** extract entities from top L2 results → spreading activation through graph at decayed weight (surfaces structurally related concepts not in the query text)
7. **L3 Summary retrieval:** scan Summary nodes with centroid embeddings by cosine similarity (≥ 0.3, up to 3 results)
8. Deduplicate, sort by weight, cap at 15 graph nodes
9. **Hebbian reinforcement:** co-retrieved node pairs get edge weight += 0.05 (ceiling 10.0); each node gets +0.02 (ceiling 5.0)

---

## 11. Reinforcement — AdaGrad & Hebbian Learning

**Explicit (Basal Ganglia):** `legend memory reinforce <signal> <id...>` (signal in [-1.0, 1.0])
- Contrastive descent: other retrieved entries get -0.02 penalty
- AdaGrad: `lr = 0.15 / sqrt(gradient_sq_sum + 1e-6)`, then `salience += signal × lr`
- Gradient squared sum capped at 1000.0 to prevent LR collapse
- Prevents saturation; entries with more feedback history get smaller updates
- Cascades to graph: node weight adjusted by signal × 0.1

**Implicit Hebbian (Neocortex):** Every `retrieve_context()` co-reinforces edge weights and node weights automatically. Enriched synaptic encoding tracks activation_count, stability, and dual-timescale interval averages.

---

## 12. Consolidation

`legend memory consolidate` (also auto-runs at 15 active ticks, or on emotional intensity / context-switch triggers):

1. **Sharp-wave ripple replay:** Temporally co-active L2 pairs (within 5 ticks of each other) reinforce shared graph edges (+0.08) and get salience boosts (+0.02)
2. **Cluster** L2 entries by cosine similarity ≥ 0.72
3. Groups with > 1 member → `summarize_group()` → create L3 Summary node (weight = 1.0 + group_salience)
4. **Systems consolidation:** High-salience groups (avg ≥ 0.4) get centroid embeddings and rich text (up to 500 chars) stored on Summary nodes. Topic anchors attached when entities recur across majority of group.
5. Prune L2 and L3

Summary nodes survive longer in L3 (high weight, slow decay) and surface in future queries via centroid embedding search.

---

## 13. Decay & Garbage Collection

- **L2 decay:** `salience *= exp(-age × 0.001 / stability)` every tick. Stability (1.0–10.0) grows with spaced retrieval.
- **Emotional valence decay:** half the hippocampal rate.
- **L3 decay:** `weight *= exp(-age × 0.0005)` (half-life ~1,386 ticks)
- **L2 pruning:** `score = salience + (usage × 0.05) - (age × 0.001) < 0.1` → remove. Consolidated entries with valid Summary embeddings get -0.2 eviction reduction.
- **L3 pruning:** nodes where `(weight - age × 0.001) < 0.05` → remove; enforce capacity limits; remove orphan edges
- **Eviction score:** `salience × 0.4 + ln(1+usage) × 0.3 + recency × 0.3` (lowest score evicted first)
- **Salience renormalization:** every 10 ticks, gentle EMA blend (10%) toward normalized values
- **Graph normalization:** every 5 ticks, weight ceiling of 2.0

---

## 14. Pattern Separation & Completion

### Pattern Separation (Dentate Gyrus)
At encoding time, new embeddings are pushed away from similar-but-distinct L2 embeddings via `sparse_orthogonalize()`. This creates sparser representations that reduce retrieval interference between related-but-different memories.

**Diversity gate:** Word-overlap Jaccard similarity ≥ 0.40 required beyond cosine similarity for merging. Entries must share enough actual vocabulary (not just hash-bucket overlap) to be considered the same memory.

### Pattern Completion (CA3 Autoassociative Network)
At retrieval time, when initial results are weak (top sim < 0.5 or < 3 results), pattern completion activates:
1. Extract entities from partial L2 matches
2. Run spreading activation through the graph
3. Search L2 for entries containing activated entities
4. Return additional results that the direct similarity search missed

This models how the hippocampal CA3 network reconstructs full memories from partial cues.

---

## 15. Emotional Valence (Amygdala)

`compute_emotional_valence()` produces a bipolar signal in [-1.0, 1.0]:
- **Negative** — threat/pain (bugs, crashes, security issues, regressions)
- **Positive** — reward (shipped, fixed, success, resolved)
- **Urgency amplifiers** (blocker, critical, P0) push magnitude toward extremes

Emotional valence persists on L2 entries and decays at half the hippocampal rate, modeling how emotionally charged memories resist forgetting.

**Consolidation trigger:** Rolling `recent_valence_sum` (decays 0.8× per tick) triggers early consolidation when ≥ 1.5.

---

## 16. Keyword System (Wernicke's Area)

Three layers of vocabulary:

1. **Static keywords (~288 total):** Decision ~50, action ~80, architecture ~60, bug ~40, TODO ~20, preference ~20, code triggers (Rust, Python, Go, TypeScript, Java, C++, Ruby, Elixir, Zig)
2. **Workspace bootstrap (`bootstrap.rs`):** Scans `Cargo.toml`/`package.json`/`requirements.txt` during init, extracts dependency names and project terms as domain keywords
3. **Incremental discovery:** `TermStats` tracks entity frequency across ticks. Terms appearing in ≥ 5 distinct ticks with keyword co-occurrence are auto-promoted to `kw:domain:<term>` graph nodes. Noise filters: minimum 3 characters, not a stopword.

The `KeywordCache` is rebuilt from graph nodes + static fallbacks on every memory load.

---

## 17. CLI Command Reference

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

## 18. The Hook System

### Hook 1 — SessionStart
```bash
legend memory start && touch .legend/.session_active
```

### Hook 2 — UserPromptSubmit (per prompt)
- If `.session_active` is > 2 hours old → re-run `memory start`
- Else → emit reflection checkpoint, detect task-starting verbs (implement/add/fix/create/...) and mandate a query, auto-run `memory query "$PROMPT"`

### Hook 3 — PostToolUse (per file edit)
- Detects write/edit/create tools
- Increments `.legend/.pending_ticks`
- Escalation: 1 = info, 2-3 = WARNING, 4+ = CRITICAL
- Resets to 0 on any `memory tick`

### Hook 4 — Stop / SessionEnd
- If no tick this session → CRITICAL alert
- If ticks exist + uncommitted files → summary mandate

### Supported Assistants
Claude Code, Codex, Gemini CLI (shell hooks) + VS Code Copilot, Cursor, Zed (instruction injection only).

---

## 19. Project Discovery & Feature Tracking

**Auto-Discovery:** Walks project tree, identifies high-signal files (README, ARCHITECTURE, manifests, entry points), detects features from subdirectory structure. `--apply` ingests files as passive ticks.

**Workspace Bootstrap (`bootstrap.rs`):** Parses `Cargo.toml`/`package.json`/`requirements.txt` to extract dependency names and project terms, seeding the domain keyword layer.

---

## 20. Dashboards

### Ratatui TUI (`legend dashboard`)
Three views (Tab to switch): Short-Term Memory list with salience bars | Knowledge Graph nodes by weight | Event log from events.jsonl. Search with `/`, inline query with `:`. Auto-refreshes every 2-3 seconds.

### Bevy 3D Dashboard (`legend dashboard --3d`)
Force-directed 3D graph. Nodes = sphere meshes with emissive glow proportional to salience, colored by kind. Edges = gizmo lines by type. Orbit camera, raycast click to inspect. In WSL: compiles to Windows .exe and launches via `cmd.exe`.

---

## 21. Persistence & Storage

### Serialization Stack
```
Rust struct → rmp_serde::to_vec() → LGND header + format version + MessagePack bytes → lz4::block::compress() → write .tmp → atomic rename to .lz4
```

- **MessagePack** — compact binary serialization via `rmp-serde`, with `LGND` magic header (4 bytes) for format detection
- **LZ4** — fast compression (low CPU cost, good compression on repetitive text data)
- **Atomic writes** — data written to `.tmp` first, then renamed, so crashes can't corrupt
- **Corruption recovery:** Corrupt file → renamed to `.lz4.corrupt` → fresh default returned

### Event Log (`events.jsonl`)
Append-only JSONL. Every tick/query/consolidation/start logs one line with `ts`, `cmd`, `detail`, `data`. Rotates to `events.jsonl.1` at 10,000 lines.

---

## 22. Session Quality Metric

`legend memory stats` computes quality score from events.jsonl since last "start" event:

```
signal_score        = (meaningful_ticks / total_ticks) × 50
query_score         = (queries / 3.0).min(1.0) × 30
consolidation_score = consolidations > 0 ? 20 : 0
quality_score       = sum (0-100)
```

Thresholds: < 40 [LOW] | 40-70 [OK] | ≥ 70 [GOOD]

---

## 23. Token Overhead Estimation

`legend memory start --tokens` prints estimates: session start injection (`len/4`), per-prompt hooks (~100 tokens each), per-edit hooks (~150 tokens each).

Observed: legend self ~1,099/session | test-game ~1,112/session | spritec ~345/session (sparse memory).

---

## 24. Benefits (Verified)

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
11. Emotional salience — threat/reward memories resist forgetting
12. Multi-hop retrieval — structurally related context surfaces automatically

---

## 25. Current Limitations

1. **Lexical-only similarity** — "auth" and "authentication" may not cluster (mitigated by character trigrams)
2. **No native token counting** — +/-30% uncertainty on estimates
3. **EXPERIENCE: tick noise** — hook can generate high-volume auto-noise
4. **Session fragmentation overhead** — 30+ short sessions/day inflates startup costs
5. **Query under-utilization** — actual rate often below target 1+/session
6. **Single-process design** — no concurrent access support, no locking
7. **Coarse entity extraction** — no parser, generics/closures/decorators not captured
8. **No cross-session quality tracking** — only current session visible
9. **Fixed capacity** — all limits hard-coded, no config without recompile
10. **Dashboard requires separate build** — 3D Bevy crate built independently

---

## 26. Key Design Decisions & Rationale

1. **No external embedding model** — chosen for zero dependencies, determinism, sub-ms latency (lexical overlap is reasonable signal for technical dev notes)
2. **Dual threshold (0.88 / 0.72)** — raised from 0.92/0.55 to reduce false merges. Dentate gyrus pattern separation and 0.40 Jaccard gate provide additional diversity protection.
3. **Brain-region module architecture** — each cognitive mechanism maps to a named neuroscience analog. Enforces separation of concerns and makes the system's design intent readable from file names alone.
4. **Reconsolidation window (5 ticks)** — inspired by neuroscience; related follow-up updates merge into existing memory rather than duplicating
5. **Salience-driven eviction over LRU** — preserves important old decisions over recent-but-trivial notes
6. **Multi-hop spreading activation (3 hops, 0.5× decay)** — models associative priming in human memory; bridges text similarity (L2) with structural relationships (L3)
7. **Atomic MessagePack + LZ4** — write-to-temp-rename pattern with magic header; guaranteed valid or empty file on interrupt
8. **Ebbinghaus forgetting curve** — stability field modulates decay rate; spaced retrieval builds memory durability
9. **Emotional valence** — bipolar threat/reward signal modulates both retention (slower decay) and consolidation timing
10. **Pattern separation + completion** — dentate gyrus orthogonalization at encoding, CA3 autoassociative recall at retrieval. Prevents interference between similar memories while enabling reconstruction from partial cues.

---

## 27. File Structure & Dependencies

```
src/
├── main.rs                          CLI routing
├── cli.rs                           Command definition tree
├── lib.rs                           Library re-exports
├── memory/                          Pure brain — no IO
│   ├── mod.rs                       Orchestrator: tick_impl, retrieve_context, consolidate, constants
│   ├── thalamus.rs                  Sensory encoding: n-gram embeddings, cosine similarity, salience scoring
│   ├── prefrontal.rs                Working memory (L1): attention gating, context-switch flushing
│   ├── hippocampus.rs               Episodic memory (L2): reconsolidation, pattern completion, SWR replay
│   ├── neocortex.rs                 Knowledge graph (L3): spreading activation, Hebbian learning, consolidation
│   ├── amygdala.rs                  Emotional valence: threat/reward scoring
│   ├── dentate_gyrus.rs             Pattern separation: sparse orthogonalization, diversity gating
│   ├── basal_ganglia.rs             Reinforcement: AdaGrad, contrastive descent, renormalization
│   ├── entorhinal.rs                Compression gateway: chunking, extractive summarization
│   └── wernicke/                    Language comprehension
│       ├── mod.rs                   Re-exports, KeywordCache
│       ├── extract.rs               Entity extraction (multi-language code patterns + identifiers)
│       ├── lexicon.rs               Static vocabulary tables (~288 keywords)
│       └── cache.rs                 Dynamic keyword cache (graph + static + domain)
├── tool/                            IO + persistence — all filesystem/network access
│   ├── mod.rs                       tick/tick_passive, start summary, git sync, session log, task mgmt
│   ├── persistence.rs               Save/load: LZ4 + MessagePack, atomic writes, corruption recovery
│   ├── bootstrap.rs                 Workspace scanning: dependency parsing, domain keyword extraction
│   └── types.rs                     SessionEntry, TickResult, MemoryContext, MemoryConfig, TermStats
├── commands/                        CLI handlers
│   ├── memory/                      All legend memory * subcommands
│   ├── init.rs                      Hook installation, instruction files
│   ├── discover.rs                  Project scanning
│   ├── mcp.rs                       MCP server (JSON-RPC 2.0 stdio, 6 tools)
│   └── dashboard.rs                 Dashboard launcher
└── tui/mod.rs                       Ratatui TUI dashboard

dashboard/  (separate crate — Bevy 0.15 + bevy_egui 0.33)
```

**Total main binary: ~16,700 lines of Rust.**

**Runtime deps:** serde, serde_json, rmp-serde, lz4, ratatui, crossterm
**Test coverage: 659 tests, all passing**

---

## 28. Performance Characteristics

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

## 29. All Constants — Reference Table

| Constant | Value | Purpose |
|---|---|---|
| `immediate_capacity` | 10 | L1 working memory max (Miller's Law ~7±2) |
| `short_term_capacity` | 1,024 | L2 max entries |
| `embedding_dim` | 256 | N-gram vector dimension |
| `theta_high` | 0.88 | Reinforce threshold (CA3 pattern completion) |
| `theta_low` | 0.72 | Merge threshold (dentate gyrus pattern separation) |
| `ATTENTION_GATE_THRESHOLD` | 0.25 | Min salience for L1→L2 promotion |
| `HIPPOCAMPAL_DECAY_RATE` | 0.001 | L2 base decay per tick (modulated by stability) |
| `NEOCORTICAL_DECAY_RATE` | 0.0005 | L3 decay per tick |
| `HEBBIAN_EDGE_BOOST` | 0.05 | Edge weight per co-retrieval |
| `HEBBIAN_NODE_BOOST` | 0.02 | Node weight per co-retrieval |
| `HEBBIAN_EDGE_CEILING` | 10.0 | Max edge weight |
| `HEBBIAN_NODE_CEILING` | 5.0 | Max node weight |
| `EDGE_REINFORCE_DELTA` | 0.1 | Weight increment on edge upsert |
| `NODE_WEIGHT_BASE` | 0.2 | Initial weight for new graph nodes |
| `PRUNE_THRESHOLD` | 0.1 | Min L2 retention score |
| `MERGE_WORD_OVERLAP_THRESHOLD` | 0.4 | Min Jaccard for merge (diversity gate) |
| `SESSION_LOG_CAPACITY` | 100 | Max session log entries |
| `GRAPH_NODE_CAPACITY` | 2,048 | L3 node cap |
| `GRAPH_EDGE_CAPACITY` | 8,192 | L3 edge cap |
| `GRAPH_PRUNE_WEIGHT` | 0.05 | Min node weight to survive pruning |
| `GRAPH_WEIGHT_TARGET_MAX` | 2.0 | Normalization ceiling |
| `RECONSOLIDATION_THRESHOLD` | 0.35 | Min sim for labile update |
| `RECONSOLIDATION_WINDOW` | 5 | Ticks entry stays labile |
| `CONSOLIDATION_SUGGESTION_THRESHOLD` | 15 | Ticks between auto-consolidation |
| `SPREADING_ACTIVATION_MAX_HOPS` | 3 | Graph traversal depth |
| `SPREADING_ACTIVATION_DECAY` | 0.5 | Activation decay per hop |
| `REPLAY_TEMPORAL_WINDOW` | 5 | Ticks for SWR co-activity |
| `REPLAY_EDGE_BOOST` | 0.08 | Edge boost during replay |
| `REPLAY_SALIENCE_BOOST` | 0.02 | Salience boost during replay |
| `EMOTIONAL_CONSOLIDATION_THRESHOLD` | 1.5 | Rolling valence sum trigger |
| `CONTEXT_SWITCH_THRESHOLD` | 0.15 | Cosine drop for topic change |
| `ADAGRAD_BASE_LR` | 0.15 | Base learning rate |
| `ADAGRAD_SQ_SUM_CAP` | 1000.0 | Prevent LR collapse |
| `CONTRASTIVE_PENALTY` | 0.02 | Penalty for unreinforced retrievals |
| `RENORM_INTERVAL` | 10 | Ticks between EMA normalization |
| `RENORM_BLEND` | 0.1 | EMA blend weight |
| `AUTO_REINFORCE_SCALE` | 0.03 | Top result salience boost scale |
| `MIN_QUERY_SIMILARITY` | 0.15 | Noise floor for retrieval |
| `KEYWORD_MATCH_BONUS` | 0.05 | Per-keyword retrieval bonus |
| `KEYWORD_MATCH_BONUS_CAP` | 0.2 | Max keyword bonus |
| `CONSOLIDATED_EVICTION_REDUCTION` | 0.2 | Eviction discount for consolidated entries |
| `SYSTEMS_CONSOLIDATION_SALIENCE_THRESHOLD` | 0.4 | Min avg salience for neocortical encoding |
| `SUMMARY_FULL_TEXT_MAX_LEN` | 500 | Max chars on Summary node full_text |
| `TERM_PROMOTION_MIN_TICKS` | 5 | Min distinct ticks for keyword auto-promotion |
| `EVENT_LOG_MAX_LINES` | 10,000 | Rotate events.jsonl at |
| Session TTL | 7,200 sec | After which hook re-runs memory start |
| Tick pending (warning) | 2 | at 2+ un-ticked edits |
| Tick pending (critical) | 4 | at 4+ un-ticked edits |

---

*End of report. Document generated 2026-04-05 from Legend v0.3.9 source code.*
