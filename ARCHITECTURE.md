# Legend — Architecture & Internals

A technical reference for how Legend stores, creates, updates, and retrieves memories.
Current as of v0.3.9 (April 2026).

---

## Overview

Legend is a local persistent memory layer for AI coding agents. The codebase is split into two halves:

- **`src/memory/`** — Pure cognitive engine (no IO). Brain-region modules implement working memory, episodic memory, knowledge graph, emotional valence, pattern separation, reinforcement learning, and sensory encoding.
- **`src/tool/`** — IO and tool wrapper. Handles persistence (save/load), session logs, git sync, CLI/MCP integration, and workspace bootstrap.

All state lives in a `MemoryState` struct (wraps `BrainState` with session/tool fields) serialized atomically on every operation.

---

## Storage

### Data Structures

Legend uses a **three-layer hierarchical memory** modeled on cognitive neuroscience:

#### Layer 1 — Working Memory (Prefrontal Cortex)
- **Type:** `Vec<WorkingMemoryEntry>` with attention gating
- **Capacity:** 10 entries (~7±2, matching Miller's Law)
- **Purpose:** Limited-capacity buffer queried first during retrieval. Only entries with salience ≥ `ATTENTION_GATE_THRESHOLD` (0.25) are promoted to L2. On context switch (cosine drop below 0.15 between consecutive ticks), L1 is flushed and unpromoted entries get a final promotion opportunity.

#### Layer 2 — Episodic Memory (Hippocampus)
- **Type:** `Vec<ShortTermEntry>`, **Capacity:** 1,024 entries
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
  | `reconsolidation_count` | `u32` | Times this memory has been reconsolidated |
  | `labile_until` | `u64` | Clock tick until which this entry is labile after retrieval |
  | `emotional_valence` | `f32` | Amygdala signal: negative=threat, positive=reward |
  | `stability` | `f32` | Ebbinghaus forgetting resistance (1.0–10.0), grows with spaced retrieval |
  | `density` | `f32` | Weighted count of high-signal entities (modulates decay rate) |
  | `consolidated` | `bool` | Whether this entry has been promoted to L3 |
  | `gradient_sq_sum` | `f32` | AdaGrad accumulated squared gradient for reinforcement |
  | `refs` | `Vec<MemoryRef>` | Source file + line range references |
  | `last_retrieval_interval` | `u64` | Interval between two most recent retrievals |

- **Decay:** Exponential, `salience *= exp(-age × 0.001 / stability)`. Base half-life ≈ 693 ticks, extended by stability. Emotional valence decays at half the hippocampal rate.

#### Layer 3 — Knowledge Graph (Neocortex)
- **Type:** `GraphMemory` containing:
  - `nodes: HashMap<u64, GraphNode>` — up to 2,048 nodes
  - `edges: Vec<GraphEdge>` — up to 8,192 edges
  - `index: HashMap<String, u64>` — label → node ID lookup
- **Node fields:** `id`, `label`, `kind`, `weight`, `salience`, `last_seen`, `source_texts`, `embedding` (centroid for Summary nodes), `full_text` (rich text for consolidated memories)
- **Edge fields:** `from`, `to`, `weight`, `kind` (related/depends-on/implements/co-defined/contains/drives/represents), `last_seen`, `activation_count`, `stability` (caps 10.0), `recent_interval_avg` (STP, α=0.5), `historical_interval_avg` (LTP, α=0.1)
- **Decay:** Half-life ≈ 1,386 ticks (rate 0.0005, twice as durable as L2).

#### Supporting State
- **`clock: u64`** — Monotonic tick counter; "age" = `clock - last_access`.
- **`session_log: Vec<SessionEntry>`** — Chronological tick log (capped at 100). Preserves exact input for session review.
- **`current_task: Option<String>`** — Pinned task shown at session start.
- **`last_synced_sha: Option<String>`** — Git commit SHA for cold-start reconciliation.
- **`recent_valence_sum: f32`** — Rolling emotional intensity for amygdala-driven consolidation triggers.
- **`last_tick_embedding: Vec<f32>`** — Previous tick's embedding for context-switch detection.
- **`term_frequency: HashMap<String, TermStats>`** — L3 incremental keyword discovery.

### Serialization Stack

```
MemoryState (Rust struct)
    ↓ rmp_serde::to_vec()
LGND header (4 bytes) + format version (1 byte) + MessagePack bytes
    ↓ lz4::block::compress()
Compressed bytes
    ↓ fs::write(.tmp) + atomic rename
.legend/memory.lz4
```

- **MessagePack** — compact binary serialization via `rmp-serde`, with `LGND` magic header for format detection
- **LZ4** — fast compression (low CPU cost, good compression on repetitive text data)
- **Atomic writes** — data is written to a `.tmp` file first, then renamed, so a crash mid-write can't corrupt the store
- **Corruption recovery** — if deserialization fails on load, the corrupt file is renamed to `.corrupt` and a fresh default state is returned

All data lives in a single file: `.legend/memory.lz4`. There's also `.legend/events.jsonl` (append-only event log for the dashboard).

---

## The Tick Pipeline

Memory creation happens through `tick()` → `tick_impl()`, the primary write path.

```
Input text
    ↓
1. clock += 1, apply_decay(), stabilize_labile_entries()
   renormalize_salience() every 10 ticks, normalize_graph_weights() every 5 ticks
   decay rolling emotional intensity (× 0.8)
    ↓
2. Append raw text to session_log (non-passive ticks only)
    ↓
3. chunk_text() — split into ~200-char chunks (entorhinal cortex compression)
    ↓
For each chunk:
    ↓
4. Push to L1 working memory (prefrontal cortex)
    ↓
5. embed_text() — generate 256-dim n-gram vector (thalamus sensory encoding)
6. compute_salience() — score importance from keyword heuristics (thalamus)
7. compute_emotional_valence() — bipolar threat/reward signal (amygdala)
    ↓
8. Attention gate: salience ≥ 0.25 → promote to L2 path; else stay in L1 only
    ↓
9. sparse_orthogonalize() — push embedding away from similar-but-distinct L2 entries (dentate gyrus pattern separation)
    ↓
10. Reconsolidation check: if a labile entry matches (sim ≥ 0.35), update in-place
    ↓
11. Dual-threshold matching against L2:
    ├─ similarity ≥ 0.88 AND word overlap ≥ 40% → REINFORCE (bump usage+salience)
    ├─ similarity ≥ 0.72 AND word overlap ≥ 40% → MERGE (average embeddings)
    └─ otherwise → INSERT new entry
    ↓
12. update_graph() — extract entities (wernicke), create/update graph nodes and edges (neocortex)
    ↓
13. retrieve_context() — return relevant context (also marks entries labile)
    ↓
14. prune_short_term() + prune_graph() — garbage collect
    ↓
15. Context-switch detection: if cosine similarity to previous tick < 0.15, flush L1
```

### How Embeddings Work (Thalamus)

Legend uses **n-gram hashing** (not neural embeddings) for zero-dependency, deterministic similarity:

1. **Word unigrams** — each word hashes (FNV-1a) to a bucket in a 256-dim vector, weight 1.0
2. **Character trigrams** — sliding 3-char windows within each word, weight 0.3 (captures subword similarity)
3. **Word bigrams** — consecutive word pairs, weight 0.75 (captures phrase structure)
4. **L2 normalization** — the vector is normalized so cosine similarity works correctly

### How Salience Scoring Works (Thalamus)

`compute_salience()` assigns importance based on keyword heuristics from the dynamic `KeywordCache`:

| Content Pattern | Score Boost |
|----------------|-------------|
| Decision language (2+ keyword hits) | +0.5 |
| Decision (1 hit) | +0.3 |
| Decision + rationale ("because", "reason") | +0.15 |
| Bug/incident language | +0.4 |
| TODO/blocker language | +0.3 |
| Preference language | +0.3 |
| Architecture language | +0.25 |
| Domain-specific vocabulary (learned from workspace) | +0.1 |
| Multiple code definitions (2+) | +0.3 |
| Single code definition | +0.2 |
| Code block (```) | +0.15 |
| Error mentions | +0.15 |
| Substantive text (>25 words) | +0.15 |

Final score is clamped to [0.05, 1.0].

### Pattern Separation (Dentate Gyrus)

Before matching, new embeddings are pushed away from similar-but-distinct existing L2 embeddings via `sparse_orthogonalize()`. This reduces retrieval interference between related-but-different memories. A diversity gate (`word_overlap() ≥ 0.4` Jaccard) provides a second check beyond cosine similarity — entries must share enough actual vocabulary to merge.

### Emotional Valence (Amygdala)

`compute_emotional_valence()` produces a bipolar signal in [-1.0, 1.0]:
- **Negative** — threat/pain (bugs, crashes, security issues)
- **Positive** — reward (shipped, fixed, success)
- **Urgency amplifiers** (blocker, critical, P0) push magnitude toward extremes

Valence persists on L2 entries and decays at half the hippocampal rate, modeling how emotionally charged memories resist forgetting. Rolling `recent_valence_sum` triggers early consolidation when emotional intensity accumulates (≥ 1.5 threshold).

---

## Updating Memory

### 1. Reinforcement (High Similarity ≥ 0.88)
Existing entry is reinforced: `usage += 2`, `salience += new_salience` (capped at 1.0). No new entry created.

### 2. Merging (Medium Similarity ≥ 0.72)
Embeddings averaged, `usage += 1`, `salience += new_salience × 0.5`, summary regenerated from both texts.

### 3. Reconsolidation (Labile Memory Update)
Retrieved memories enter a labile window for 5 ticks. If a new tick matches (sim ≥ 0.35), the labile memory is updated in-place: text appended, embedding recomputed, salience boosted, `reconsolidation_count` incremented, entry re-stabilized.

### 4. Explicit Reinforcement (Basal Ganglia)
`legend memory reinforce <signal> <id...>` uses AdaGrad-adaptive learning: `lr = 0.15 / sqrt(gradient_sq_sum + ε)`. Contrastive descent: retrieved-but-unreinforced entries get a -0.02 penalty. Cascades to graph nodes via `REINFORCE_GRAPH_SCALE` (0.1).

### 5. Auto-Reinforcement
Top retrieval result gets `salience += similarity × 0.03` when similarity > 0.15.

### 6. Hebbian Reinforcement (Neocortex)
Co-retrieved graph nodes get edge weight += 0.05 (ceiling 10.0) and node weight += 0.02 (ceiling 5.0). "Neurons that fire together wire together." Enriched synaptic encoding tracks `activation_count`, `stability`, and dual-timescale interval averages (STP α=0.5, LTP α=0.1).

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
3. Scan L2 entries by cosine similarity + keyword bonus → top 5
   (min similarity 0.15 noise floor, keyword bonus up to 0.2)
    ↓
4. Mark retrieved entries as labile (labile_until = clock + 5)
    ↓
5. Auto-reinforce top result (salience += sim × 0.03)
    ↓
6. Pattern completion (CA3): if top result sim < 0.5 or < 3 results,
   extract entities from partial matches → spreading activation through graph →
   search L2 for entries containing activated entities
    ↓
7. Graph lookup: extract entities → multi-hop spreading activation (up to 3 hops, 0.5× decay per hop)
    ↓
8. Associative priming:
   - Extract entities from retrieved L2 results
   - Spreading activation through graph
   - Add neighbor nodes at decayed weight
   - Deduplicate, re-sort, cap at 15 nodes
    ↓
9. L3 Summary node retrieval: scan Summary nodes with embeddings by cosine similarity (≥ 0.3)
    ↓
10. Hebbian reinforce all co-retrieved graph nodes
    ↓
11. Return MemoryContext { short_term, long_term, working_memory }
```

### Cold-Start Summary (`memory start`)

Git-aware cold-start synchronization: compares current HEAD to `last_synced_sha`, reports intervening commits and uncommitted changes. Returns categorized high-signal memories (decisions, architecture, bugs, TODOs, preferences) plus retrieval results.

### Consolidation

Runs manually or auto-triggers after 15 active ticks. Two additional smart triggers:
1. **Emotional intensity** — when `recent_valence_sum ≥ 1.5` (amygdala-driven)
2. **Context switch** — when cosine similarity between consecutive ticks drops below 0.15

**Pipeline:**
1. **Sharp-wave ripple replay** — temporally co-active L2 pairs (within 5 ticks) reinforce shared graph edges (+0.08) and get salience boosts (+0.02)
2. **Cluster** L2 entries by cosine similarity ≥ θ_low (0.72)
3. **Summarize** each group → create L3 Summary node (weight = 1.0 + max_salience)
4. **Systems consolidation** — high-salience groups (avg ≥ 0.4) get centroid embeddings and rich text stored on Summary nodes, enabling L3 to serve queries independently after L2 entries decay
5. **Prune** L2 and L3

---

## Decay & Garbage Collection

### Exponential Decay (Ebbinghaus Forgetting Curve)
- **L2:** `salience *= exp(-age × 0.001 / stability)` — base half-life ≈ 693 ticks, extended by stability (1.0–10.0). Stability grows with spaced retrieval.
- **L3:** `weight *= exp(-age × 0.0005)` — half-life ≈ 1,386 ticks
- **Emotional valence:** decays at half the hippocampal rate

### L2 Pruning
Entries removed when composite score < 0.1: `score = salience + (usage × 0.05) - (age × 0.001)`. Consolidated entries whose Summary node has a valid embedding get an eviction score reduction of 0.2.

### L3 Pruning
Nodes with `(weight - age × 0.001) < 0.05` removed. Hard caps enforced: 2,048 nodes, 8,192 edges. Graph weight normalization every 5 ticks (ceiling 2.0).

### Eviction Scoring
When L2 hits 1,024 entries: `score = salience × 0.4 + ln(1+usage) × 0.3 + exp(-age × 0.002) × 0.3`

### Renormalization
Every 10 ticks, gentle EMA blend (10%) toward normalized salience values prevents score drift.

---

## Keyword System (Wernicke)

Three layers of vocabulary:

1. **Static keywords** (~288 total) — domain-independent lists: decision (~50), action (~80), architecture (~60), bug (~40), TODO (~20), preference (~20), code triggers (multi-language)
2. **Workspace bootstrap** — `bootstrap.rs` scans `Cargo.toml`/`package.json`/`requirements.txt` during init, extracts dependency names and project terms as domain keywords
3. **Incremental discovery** — `TermStats` tracks entity frequency across ticks. Terms appearing in ≥ 5 distinct ticks with keyword co-occurrence are auto-promoted to `kw:domain:<term>` graph nodes

The `KeywordCache` is rebuilt from graph + static fallbacks on every load.

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
| `legend memory stats` | Show current storage counts + session quality score |
| `legend memory context` | Structured context summary as JSON |
| `legend memory sessions [n]` | Show last n session log entries |
| `legend memory dump` | Export full memory state as JSON (used by dashboard) |
| `legend memory reset` | Delete memory store and start fresh |
| `legend memory task set/clear` | Pin/clear current task |
| `legend dashboard` | Launch TUI or 3D memory visualization |

---

## Key Design Decisions

1. **No external dependencies for embeddings.** N-gram hashing gives deterministic, zero-latency embeddings with no API calls or model files. Trade-off: purely lexical similarity.

2. **Dual-threshold matching (θ_high=0.88, θ_low=0.72).** High threshold reinforces without modification. Low threshold merges. Below low threshold, a new entry is created. Raised from 0.92/0.55 to reduce false merges.

3. **Word-overlap diversity gate (Jaccard ≥ 0.40).** Even at high cosine similarity, entries with different vocabulary stay separate. Prevents hash collision false positives.

4. **Reconsolidation window (5 ticks).** Retrieved memories become labile, modeling how biological retrieval makes memories malleable for update.

5. **Salience-driven survival.** Important memories (decisions, bugs, architecture) get higher initial salience and survive longer than routine noise.

6. **Multi-hop spreading activation.** Up to 3 hops with 0.5× decay per hop. Surfaces structurally related graph context beyond direct entity matches.

7. **Brain-region module architecture.** Each cognitive mechanism maps to a named brain region, enforcing separation of concerns and making the neuroscience analogy explicit in code.

8. **Atomic MessagePack + LZ4 persistence.** Write to temp file, rename. Compact binary format with magic header for format detection.

---

## File Map

| File | Purpose |
|------|---------|
| **Brain modules (`src/memory/`)** | |
| `mod.rs` | Orchestrator — routes ticks through brain regions, `tick_impl`, `retrieve_context`, `consolidate`, constants |
| `thalamus.rs` | Sensory encoding — n-gram embeddings, cosine similarity, salience scoring |
| `prefrontal.rs` | Working memory (L1) — attention gating, context-switch flushing |
| `hippocampus.rs` | Episodic memory (L2) — reconsolidation, pattern completion (CA3), SWR replay, forgetting curve |
| `neocortex.rs` | Knowledge graph (L3) — spreading activation, Hebbian learning, systems consolidation |
| `amygdala.rs` | Emotional valence — threat/reward scoring, intensity tracking |
| `dentate_gyrus.rs` | Pattern separation — sparse orthogonalization, diversity gating |
| `basal_ganglia.rs` | Reinforcement — AdaGrad optimization, contrastive descent, renormalization |
| `entorhinal.rs` | Compression gateway — text chunking, extractive summarization |
| `wernicke/` | Language comprehension — entity extraction, static vocabulary, dynamic keyword cache |
| **Tool modules (`src/tool/`)** | |
| `mod.rs` | IO orchestrator — tick/tick_passive, start summary, git sync, session log, task management |
| `persistence.rs` | Save/load — LZ4+MessagePack serialization, atomic writes, corruption recovery |
| `bootstrap.rs` | Workspace scanning — dependency parsing, domain keyword extraction |
| `types.rs` | Tool-layer types — SessionEntry, TickResult, MemoryContext, MemoryConfig, TermStats |
| **CLI (`src/commands/`)** | |
| `memory/` | CLI handlers for all `legend memory *` subcommands (start, tick, query, etc.) |
| `init.rs` | Project setup, hook installation, instruction file generation |
| `discover.rs` | Project scanning and feature detection |
| `mcp.rs` | MCP server — JSON-RPC 2.0 stdio loop with 6 tools |
| `dashboard.rs` | Dashboard launcher |
| **Other** | |
| `src/main.rs` | CLI routing |
| `src/cli.rs` | Command definition tree |
| `src/tui/mod.rs` | Ratatui TUI dashboard |
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
- **2026-04-04** — ARCHITECTURE: Completed Code Footprint Reduction Pass — 5 changes, reduced from 19,453 to 18,903 lines (550 lines saved). Changes: (A) ShortTermEntry test constructions compressed with ..Default::de…
- **2026-04-04** — ARCHITECTURE: Legend is a local persistent memory layer for AI coding agents. Its core split is src/memory/ as a pure cognitive engine (working memory L1, episodic L2, graph L3) and src/tool/ as the I…
- **2026-04-04** — ARCHITECTURE: The primary write path is tick -> tick_impl. Every tick is chunked, embedded with deterministic n-gram hashing, salience-scored, passed through working-memory attention gating, then eith…
- **2026-04-04** — ARCHITECTURE: Removed LLM module entirely (~1,390 lines deleted). Deleted src/commands/llm/ (mod.rs + helpers.rs), tests/conformance_llm.rs, LLM command statics from main.rs, auto_trigger_for_text cal…
- **2026-04-05** — ARCHITECTURE: Removed all bincode migration code from persistence.rs (~490 lines). Deleted V1-V5 migration types, migrate_v4(), migrate_v5(), migrate_corrupt_backup(), old_refs_to_current(). Simplifie…
- **2026-04-05** — ARCHITECTURE: Completed Change 11 — Terminology Alignment + Documentation Update. (A) Added comprehensive neuroscience-analog doc comments to thalamus.rs (sensory encoding relay) and entorhinal.rs (…
- **2026-04-06** — ARCHITECTURE: Completed revised Change 14 as CPEB-inspired synaptic tagging. High-valence ticks now selectively boost stability of recently active L3 edges via neocortex::cpeb_tag_edges, wired into ti…
- **2026-04-06** — ARCHITECTURE: Implemented Change 15 as query-mode gated retrieval instead of the old context-filter/multi-edge design. Added neocortex::QueryMode plus soft edge_kind_multiplier priors for structural, …
- **2026-04-07** — ARCHITECTURE: Refactored thalamus/entorhinal ownership split. Moved embed_text(), fnv_hash(), cosine_similarity(), merge_embeddings() from thalamus.rs to entorhinal.rs. Thalamus now owns only compute_…
