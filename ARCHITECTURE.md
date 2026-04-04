# Legend — Architecture & Internals

A technical reference for how Legend stores, creates, updates, and retrieves memories.

---

## Storage

### Data Structures

Legend uses a **three-layer hierarchical memory** modeled loosely on biological memory systems. All three layers live inside a single `MemoryState` struct that is loaded and saved atomically on every operation.

#### Layer 1 — Immediate Buffer
- **Type:** `VecDeque<String>` (FIFO ring buffer)
- **Capacity:** 256 entries
- **Purpose:** Raw text of every tick, in order. When full, the oldest entry is evicted. Think of this as the "working memory" — the most recent raw inputs available for quick scanning.

#### Layer 2 — Short-Term Vector Store
- **Type:** `Vec<ShortTermEntry>`
- **Capacity:** 1,024 entries
- **Each entry contains:**
  | Field | Type | Purpose |
  |-------|------|---------|
  | `id` | `u64` | Unique monotonic ID |
  | `text` | `String` | Full text of the memory |
  | `summary` | `String` | Extractive summary (best sentence) |
  | `embedding` | `Vec<f32>` | 256-dim n-gram vector for similarity search |
  | `salience` | `f32` | Importance score (0.05–1.0), determines survival priority |
  | `usage` | `u32` | How many times this entry has been accessed or reinforced |
  | `last_access` | `u64` | Clock tick of last read/write |
  | `reconsolidation_count` | `u32` | How many times this memory has been reconsolidated |
  | `labile_until` | `u64` | Clock tick until which this entry is "unstable" after retrieval |

- **Purpose:** The workhorse of the system. Every query searches this layer by cosine similarity. Entries decay over time and get pruned when their composite score drops below threshold.

#### Layer 3 — Long-Term Knowledge Graph
- **Type:** `GraphMemory` containing:
  - `nodes: HashMap<u64, GraphNode>` — up to 2,048 nodes
  - `edges: Vec<GraphEdge>` — up to 8,192 edges
  - `index: HashMap<String, u64>` — label → node ID lookup
- **Each node contains:** `id`, `label`, `kind` (Function/Struct/Type/Term/etc.), `weight`, `salience`, `last_seen`
- **Each edge contains:** `from`, `to`, `weight`, `kind` (related/depends-on/implements/co-defined/contains)
- **Purpose:** Captures *relationships* between entities. Entities are extracted from text using code-aware parsing (recognizes `fn`, `struct`, `class`, `def`, etc.) and plain identifier scanning. Co-occurring entities get edges between them. Frequently co-retrieved nodes strengthen their shared edges (Hebbian reinforcement).

#### Supporting State
- **`session_log: Vec<SessionEntry>`** — Chronological log of every tick's raw text (capped at 100 entries). Preserves exact input for session review.
- **`clock: u64`** — Monotonically increasing tick counter. Every `tick()`, `retrieve_context()`, `consolidate()`, and `reinforce()` call increments it.
- **`next_id: u64`** — Auto-incrementing ID for new short-term entries and graph nodes.

### Serialization Stack

```
MemoryState (Rust struct)
    ↓ bincode::serialize()
Raw bytes
    ↓ lz4::block::compress()
Compressed bytes
    ↓ fs::write()
.legend/memory.lz4
```

- **bincode** — fast binary serialization (no schema overhead, ~10x faster than JSON)
- **LZ4** — fast compression (low CPU cost, good compression on repetitive text data)
- **Atomic writes** — data is written to a `.tmp` file first, then renamed, so a crash mid-write can't corrupt the store
- **Corruption recovery** — if deserialization fails on load, the corrupt file is renamed to `.corrupt` and a fresh default state is returned

All data lives in a single file: `.legend/memory.lz4`. There's also `.legend/events.jsonl` (append-only event log for the dashboard) and `.legend/state.lz4` (feature tracking state, separate from memory).

---

## Creating Memory Nodes

Memory creation happens through the `tick()` method, which is the primary write path.

### The Tick Pipeline

```
Input text
    ↓
1. clock += 1, apply_decay(), stabilize_labile_entries()
    ↓
2. Append raw text to session_log
    ↓
3. chunk_text() — split into ~200-char chunks (respects line boundaries)
    ↓
For each chunk:
    ↓
4. Push to immediate buffer (FIFO)
    ↓
5. embed_text() — generate 256-dim n-gram vector
6. compute_salience() — score importance from content heuristics
    ↓
7. Reconsolidation check (see "Updating" section below)
    ↓
8. Match against existing short-term entries:
    ├─ similarity ≥ 0.92 AND word overlap ≥ 30% → REINFORCE (bump usage+salience, no new entry)
    ├─ similarity ≥ 0.55 AND word overlap ≥ 30% → MERGE (average embeddings, combine summaries)
    └─ otherwise → INSERT new entry
    ↓
9. update_graph() — extract entities, create/update graph nodes and edges
    ↓
10. retrieve_context() — return relevant context (also marks entries labile)
    ↓
11. prune_short_term() + prune_graph() — garbage collect
```

### How Embeddings Work

Legend uses **n-gram hashing** (not neural embeddings) for zero-dependency, deterministic similarity:

1. **Word unigrams** — each word hashes (FNV-1a) to a bucket in a 256-dim vector, weight 1.0
2. **Character trigrams** — sliding 3-char windows within each word, weight 0.5 (captures subword similarity)
3. **Word bigrams** — consecutive word pairs, weight 0.75 (captures phrase structure)
4. **L2 normalization** — the vector is normalized so cosine similarity works correctly

This means "memory system" and "memory systems" have high overlap (shared trigrams), but "memory system" and "cooking recipes" don't.

### How Salience Scoring Works

`compute_salience()` assigns importance based on keyword heuristics:

| Content Pattern | Score Boost |
|----------------|-------------|
| Decision language ("chose", "decided", "instead of") × 2+ hits | +0.5 |
| Decision × 1 hit | +0.3 |
| Decision + rationale ("because", "reason") | +0.15 |
| Bug/incident language ("bug", "crash", "regression") | +0.4 |
| TODO/blocker language | +0.3 |
| Preference language ("user prefers", "convention") | +0.3 |
| Architecture language ("module", "component", "api") | +0.25 |
| Code references (``` or `fn ` or `struct `) | +0.15 |
| Error mentions | +0.15 |
| Substantive text (>25 words) | +0.15 |

Final score is clamped to [0.05, 1.0]. This means decisions with rationale can score 0.65+ out of the box, while generic progress notes start around 0.05–0.15.

### How Entity Extraction Works

`extract_entities()` in `extract.rs` parses text for:

1. **Code patterns** — `fn name`, `struct Name`, `class Name`, `def name`, `impl Name`, `mod name`, `use path` → extracted with appropriate kind (Function, Struct, Class, etc.) and context (defines, uses, implements)
2. **Plain identifiers** — any alphanumeric+underscore token that passes the stopword filter, with kind inferred from casing:
   - `UpperCase` → Type
   - `has_underscore` → Symbol
   - `lowercase` → Term

Extracted entities become graph nodes. When multiple entities appear in the same text, they get edges between them.

### How Graph Edges Are Typed

Edge kinds are inferred from entity context:
- defines + mentions → `contains`
- either uses → `depends-on`
- either implements → `implements`
- both define → `co-defined`
- everything else → `related`

---

## Updating Memory Nodes

Memory is updated through several mechanisms:

### 1. Reinforcement (High Similarity Match)

When a tick's embedding has cosine similarity ≥ 0.92 to an existing entry (and word overlap ≥ 30%), the existing entry is reinforced:
- `usage += 2`
- `salience = min(salience + new_salience, 1.0)`
- `last_access = current clock`

No new entry is created — this prevents duplicates for repeated or very similar information.

### 2. Merging (Medium Similarity Match)

When similarity is between 0.55 and 0.92 (with word overlap ≥ 30%):
- `embedding = average(old_embedding, new_embedding)` — the vector drifts toward the combined meaning
- `usage += 1`
- `salience += new_salience × 0.5`
- `summary` is regenerated from both texts (extractive — picks the best sentence)

### 3. Reconsolidation (Labile Memory Update)

Inspired by neuroscience: when a memory is **retrieved**, it enters a "labile" (unstable) state for 5 ticks. If the next tick contains related information (similarity ≥ 0.35 AND word overlap ≥ 10%), instead of creating a new entry, the labile memory is **updated in-place**:

- Text is appended (pipe-delimited): `"original text | new information"`
- If combined text exceeds 500 chars, it's replaced with an extractive summary
- Embedding is recomputed from the merged text
- Salience gets a 30% boost from the new text's salience
- `reconsolidation_count` increments
- Entry re-stabilizes (`labile_until = 0`)

This is the primary mechanism for memories to evolve over time rather than proliferate.

### 4. Explicit Reinforcement

`legend memory reinforce <signal> <id1> [id2 ...]` applies a manual signal (-1.0 to 1.0):
- **Positive signal:** salience += signal × 0.15, usage += 1
- **Negative signal:** salience -= |signal| × 0.15 (entry decays faster)
- **Cascades to graph:** entities from the entry's text get weight adjusted by signal × 0.1

### 5. Auto-Reinforcement

When `retrieve_context()` runs, the **top result** (if similarity > 0.2) automatically gets a small salience boost: `salience += similarity × 0.03`. This means frequently-useful memories naturally rise in importance without manual intervention.

### 6. Hebbian Reinforcement (Graph)

When multiple graph nodes are co-retrieved in the same query, their shared edges get weight += 0.05 and each node gets weight += 0.02. "Neurons that fire together wire together."

---

## Retrieving Memory

### Query Path (`retrieve_context`)

```
Query text
    ↓
1. clock += 1, apply_decay()
    ↓
2. embed_text(query) → 256-dim vector
    ↓
3. Scan ALL short-term entries by cosine similarity → top 5
    ↓
4. Mark retrieved entries as labile (labile_until = clock + 5)
    ↓
5. Auto-reinforce top result (salience += sim × 0.03)
    ↓
6. Graph lookup: extract entities from query → find matching nodes → expand 1-hop
    ↓
7. Associative priming:
   - Extract entities from the retrieved short-term results
   - Look up those entities in the graph
   - Follow edges 1-hop to neighbors (edge weight ≥ 0.15)
   - Add neighbor nodes at 0.7× weight discount
   - Deduplicate, re-sort by weight, cap at 15 nodes
    ↓
8. Hebbian reinforce all co-retrieved graph nodes
    ↓
9. Return MemoryContext { short_term: [...], long_term: [...] }
```

### Cold-Start Summary (`memory start`)

A single call that returns everything an LLM needs at session start:

1. **Context summary** — stats (buffer/store/graph sizes, clock), last 10 session log entries, top 15 graph nodes by weight
2. **Categorized memories** — short-term entries grouped by detected category:
   - Decisions (sorted by salience, top 10)
   - Architecture notes
   - TODOs / blockers
   - Bugs / incidents
   - Preferences / conventions
3. **Retrieval** — runs `retrieve_context("recent work decisions architecture")` to surface the most broadly relevant memories and graph nodes

### Consolidation (`memory consolidate`)

Groups similar short-term entries (cosine similarity ≥ 0.55), picks the top 3 by salience+usage from each group, summarizes them, and creates a new long-term graph node of kind "Summary" with weight = 1.0 + max_salience.

---

## Decay & Garbage Collection

### Exponential Decay

Applied on every operation:
- **Short-term entries:** `salience *= exp(-age × 0.001)` — half-life ≈ 693 ticks
- **Long-term nodes:** `weight *= exp(-age × 0.0005)`, `salience *= exp(-age × 0.0005)` — half-life ≈ 1,386 ticks

### Short-Term Pruning

After every tick, entries are removed if their composite score drops below 0.1:
```
score = salience + (usage × 0.05) - (age × 0.001)
```
High-salience, frequently-used entries survive much longer.

### Graph Pruning

After every tick:
1. Remove nodes whose effective weight (weight − age × 0.001) falls below 0.05
2. If still over 2,048 nodes, evict lowest-weight nodes
3. Remove edges referencing deleted nodes
4. If still over 8,192 edges, keep highest-weight edges only

### Eviction Scoring (Capacity Full)

When the short-term store hits 1,024 entries, the entry with the lowest composite eviction score is removed:
```
score = salience × 0.4 + ln(1 + usage) × 0.3 + exp(-age × 0.002) × 0.3
```

---

## CLI Commands

| Command | What It Does |
|---------|--------------|
| `legend init` | Create `.legend/` directory, auto-discover features, set up AI tool hooks |
| `legend memory start` | Cold-start summary — one call for full context at session start |
| `legend memory tick "<text>"` | Record a memory (decision, progress, discovery, blocker) |
| `legend memory query "<text>"` | Search memory by similarity (auto-reinforces top result, marks entries labile) |
| `legend memory reinforce <signal> <id...>` | Explicit feedback: 1.0 = useful, -1.0 = irrelevant |
| `legend memory consolidate` | Merge similar short-term entries into long-term graph summaries |
| `legend memory stats` | Show current storage counts |
| `legend memory context` | Structured context summary as JSON |
| `legend memory sessions [n]` | Show last n session log entries |
| `legend memory dump` | Export full memory state as JSON (used by dashboard) |
| `legend memory reset` | Delete memory store and start fresh |
| `legend dashboard` | Launch 3D memory visualization (Bevy app, cross-compiled to Windows from WSL) |

---

## Key Design Decisions

1. **No external dependencies for embeddings.** N-gram hashing gives deterministic, zero-latency embeddings with no API calls or model files. The trade-off is purely lexical similarity — it doesn't understand synonyms or meaning.

2. **Dual-threshold matching (θ_high=0.92, θ_low=0.55).** High threshold reinforces without modification (preserves original text). Low threshold merges (evolves the embedding). Below low threshold, a new entry is created.

3. **Word-overlap diversity gate.** Even at high cosine similarity, if the actual words are different enough (Jaccard < 0.30), entries aren't merged. Prevents unrelated entries from collapsing due to hash collisions.

4. **Reconsolidation window.** Retrieved memories become editable for 5 ticks, then re-stabilize. This mimics how biological memory works — retrieval makes memories malleable, and new context can update them.

5. **Salience-driven survival.** Decisions, bugs, and architecture notes get higher initial salience than generic progress updates. Combined with decay, this means important context outlives routine noise.

6. **Associative priming.** Retrieved short-term entries "activate" related graph nodes, surfacing context the query text alone wouldn't have found. Bridges the gap between text similarity and relational knowledge.

7. **Atomic persistence.** Write to temp file, rename. No corruption from interrupted writes. Corrupt files are auto-backed-up and the system recovers with a fresh state.

---

## File Map

| File | Purpose |
|------|---------|
| `src/memory/mod.rs` | Core memory engine — all three layers, tick, retrieve, consolidate, reconsolidate, prune, decay |
| `src/memory/embed.rs` | N-gram embeddings, cosine similarity, salience scoring |
| `src/memory/extract.rs` | Code-aware entity extraction (Rust/Python/JS patterns + plain identifiers) |
| `src/memory/summarize.rs` | Extractive summarization (best-sentence selection, decision keyword boosting) |
| `src/commands/memory.rs` | CLI handler for all `legend memory *` subcommands |
| `src/commands/init.rs` | `legend init` — project setup, hook installation |
| `src/commands/dashboard.rs` | Dashboard launcher (WSL → Windows cross-compiled Bevy app) |
| `src/storage.rs` | Feature state persistence (bincode + LZ4, separate from memory) |
| `src/types.rs` | Feature tracking types (LegendState, Feature, FeatureStatus) |
| `dashboard/` | Bevy 0.15 + bevy_egui 0.33 native 3D visualization app |
- **2026-03-09** — ARCHITECTURE: commands/memory.rs handle_tick() now auto-appends to ARCHITECTURE.md when tick starts with ARCHITECTURE: prefix
- **2026-03-15** — ARCHITECTURE: Thoroughly explored MCP server implementation in Legend. JSON-RPC 2.0 stdio loop with 6 tools (start, tick, query, task_get, task_set, stats). Config generation for 6 platforms: Claude .…
- **2026-03-17** — ARCHITECTURE: Added `children: &'static [&'static CommandDef]` field to CommandDef struct, encoding the command tree hierarchy structurally. Parents (MEMORY, LLM) reference their subcommands via child…
- **2026-03-17** — ARCHITECTURE: Completed CommandDef pass-down refactor — eliminated all upward crate:: references from handler files. TopCommand.handler signature changed from fn(&[String]) to fn(&[String], &Command…
- **2026-03-30** — ARCHITECTURE: Reworked L1 as neuroscience-aligned working memory. Replaced immediate: VecDeque<String> FIFO with working_memory: Vec<WorkingMemoryEntry> (capacity 10). Added attention gate in tick_imp…
- **2026-03-31** — ARCHITECTURE: Implemented multi-hop spreading activation (Change 4). Added spreading_activation() BFS method with configurable max_hops and decay_factor. Replaced 1-hop inline loops in both graph_look…
- **2026-03-31** — ARCHITECTURE: Implemented sharp-wave ripple replay consolidation (Change 5). Added replay_consolidation() method called at start of consolidate(). Finds temporally proximate L2 pairs (within 5 ticks),…
- **2026-04-01** — ARCHITECTURE: Implemented two smart consolidation triggers. (1) Emotional intensity — rolling recent_valence_sum (decays 0.8x/tick), triggers consolidation when >= 1.5 (amygdala-driven). (2) Context…
- **2026-04-01** — ARCHITECTURE: Implemented CA3 pattern completion (Change 6). Added pattern_complete() method: extracts entities from partial matches, runs spreading_activation through graph, searches L2 for entries c…
- **2026-04-01** — ARCHITECTURE: Completed Change 7 — Enriched Synaptic Encoding. GraphEdge now has 4 new fields: activation_count (u32), stability (f32, caps 10.0), recent_interval_avg (fast EMA α=0.5), historical_i…
- **2026-04-01** — ARCHITECTURE: Completed Change 8 — Systems Consolidation (Hippocampal Independence). GraphNode now has embedding (Vec<f32>) and full_text (Option<String>) fields. During consolidate(), high-salience…
- **2026-04-01** — ARCHITECTURE: Completed Change 9A — Comprehensive Domain-Independent Static Keywords. Expanded all keyword lists to 288 total (from 116): DECISION ~50, ACTION ~80, ARCHITECTURE ~60, BUG ~40, TODO ~2…
- **2026-04-01** — ARCHITECTURE: Completed Change 9B — Domain Category + Workspace Bootstrap (Environmental Layer). New keyword_bootstrap.rs module scans workspace during init: (1) parses Cargo.toml/package.json/requi…
- **2026-04-01** — ARCHITECTURE: Completed Change 9C — Incremental Discovery with Noise Reduction (Statistical Learning Layer). Added TermStats struct (tick_count, total_count, first_seen, last_seen, has_keyword_coocc…
- **2026-04-02** — ARCHITECTURE: Completed full 8-phase Data-Oriented Design refactor. All 46 methods removed from impl MemoryState. Structs are now pure data with zero methods. Brain-region modules (hippocampus.rs, neo…
- **2026-04-03** — ARCHITECTURE: Completed Phase 7 — Brain/Tool Module Separation. Moved 14 tool functions from memory/mod.rs to tool/mod.rs: get_git_summary, tick, tick_passive, build_start_summary, build_start_summa…
- **2026-04-03** — ARCHITECTURE: Completed all 9 phases of Brain/Tool Module Separation. Final state: src/memory/ is pure brain (no IO), src/tool/ handles persistence/CLI/IO. Brain modules renamed to neuroscience region…
