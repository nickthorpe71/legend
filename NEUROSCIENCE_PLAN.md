# Plan: Neuroscience-Aligned Memory Improvements (Phase 2)

## Context

The L1 working memory rework is complete (510 tests passing). Legend now has a 3-layer architecture:
- **L1 (Prefrontal Cortex)**: `Vec<WorkingMemoryEntry>`, capacity 10, attention-gated, rehearsal tracking
- **L2 (Hippocampus)**: `Vec<ShortTermEntry>`, capacity 1024, reconsolidation, labile windows
- **L3 (Neocortex)**: `GraphMemory` (HashMap nodes + Vec edges), Hebbian learning, 1-hop priming

The user wants to improve query result quality and align more closely with neuroscience. Each change below is independently testable and committable. Ordered by dependency and value.

---

## Change 1: Pattern Separation (Dentate Gyrus)

**Problem**: `theta_low=0.55` merges similar-but-distinct memories. "Rust memory model" and "Legend memory system" collapse because they share "memory."

**Files**: `src/memory/mod.rs`

**Changes**:
- `theta_low`: 0.55 → 0.72 (`MemoryConfig::default()`, line 115)
- `theta_high`: 0.92 → 0.88 (line 114) — narrows reinforce-only band
- `MERGE_WORD_OVERLAP_THRESHOLD`: 0.3 → 0.4 (line 40)
- Add doc comments referencing dentate gyrus pattern separation

**Tests**:
- `test_pattern_separation_preserves_similar_but_distinct`: tick "Rust memory model borrow checker" and "Legend memory system three-layer architecture" → assert both exist as separate L2 entries
- `test_near_identical_still_merges`: tick near-identical sentences → assert they merge
- Verify existing `test_tick_reinforces_similar` and `test_diversity_prevents_merge_of_unrelated` still pass

**Value**: Prevents the most impactful quality problem — distinct topics with shared vocabulary collapsing into one muddled entry.

---

## Change 2: Emotional Tagging (Amygdala)

**Problem**: `compute_salience` conflates urgency, importance, and emotional weight into one score. No separate emotional dimension that persists independently and resists decay.

**Files**: `src/memory/mod.rs`, `src/memory/embed.rs`

**Changes**:

In `embed.rs` — add new function:
```rust
pub fn compute_emotional_valence(text: &str, kw: &KeywordCache) -> f32
```
- Negative: bug/crash/panic/error/regression/breaking/failure → -0.3 to -0.8
- Positive: fixed/resolved/shipped/completed/success → +0.3 to +0.8
- Urgency amplifier: BLOCKER/critical/P0/urgent → pushes magnitude toward ±1.0
- Neutral: 0.0

In `mod.rs`:
- Add `emotional_valence: f32` to `ShortTermEntry` and `WorkingMemoryEntry` (with `#[serde(default)]`)
- In `tick_impl`: compute `emotional_valence` alongside salience, store on both L1 and L2 entries
- In `apply_decay`: emotional_valence decays at half rate (`SHORT_TERM_DECAY_RATE * 0.5`)
- In `top_k_similar`: add `|emotional_valence| * 0.05` to effective similarity score
- In `eviction_score`: add `emotional_valence.abs() * 0.15` to composite score

**Tests**:
- `compute_emotional_valence("BUG: server crashes on null input")` → < -0.3
- `compute_emotional_valence("SHIPPED: v2.0 released successfully")` → > 0.3
- `compute_emotional_valence("updated docs")` → ~0.0
- High-emotion entry retrieves better than neutral entry at similar cosine similarity
- Emotional valence decays slower than salience over 100 ticks

**Value**: Bug reports and critical decisions persist longer and surface more reliably. "What bugs did I fix?" returns better results.

---

## Change 3: Ebbinghaus Forgetting Curve + Spaced Repetition

**Problem**: Flat exponential decay (`salience *= exp(-age * rate)`) doesn't model how spaced retrieval strengthens retention.

**Files**: `src/memory/mod.rs`

**Changes**:
- Add `stability: f32` to `ShortTermEntry` (`#[serde(default = "default_stability")]`, initial 1.0)
- Add `last_retrieval_interval: u64` to `ShortTermEntry` (`#[serde(default)]`)
- In `apply_decay`: replace `salience *= exp(-age * rate)` with `salience *= exp(-age * (rate / stability))` — higher stability = slower decay
- In `retrieve_context` (entry access block, ~line 1016):
  - Compute `interval = clock - entry.last_access`
  - If `interval > entry.last_retrieval_interval`: `stability *= 1.3` (capped at 10.0) — spaced retrieval
  - If `interval <= entry.last_retrieval_interval`: `stability *= 1.05` — massed/cramming
  - Update `last_retrieval_interval = interval`

**Tests**:
- Memory retrieved at intervals [5, 10, 20, 40] develops higher stability than one at [1, 1, 1, 1]
- High-stability entry retains more salience after 200 ticks of no access
- Backward compat: old entries get stability=1.0 (identical decay behavior)

**Value**: Memories that prove useful over time persist; rarely-accessed memories fade faster. Self-curating toward high-value content.

---

## Change 4: Multi-Hop Spreading Activation

**Problem**: Only 1-hop graph walk in `graph_lookup` and associative priming. Indirect associations never surface.

**Files**: `src/memory/mod.rs`

**Changes**:

Add new method:
```rust
fn spreading_activation(&self, seed_ids: &[u64], max_hops: usize, decay_factor: f32) -> Vec<(u64, f32)>
```
- BFS-style outward spread from seeds
- Hop N activation = parent_activation * edge.weight * decay_factor^(hop-1)
- Visited set prevents cycles
- Returns (node_id, activation) sorted descending

Add constants:
```rust
const SPREADING_ACTIVATION_DECAY: f32 = 0.5;
const SPREADING_ACTIVATION_MAX_HOPS: usize = 3;
```

Replace the inline 1-hop loop in `graph_lookup` (lines ~1801-1823) with `spreading_activation(&seed_ids, 3, 0.5)`.

Replace the inline 1-hop priming loop (lines ~1062-1087) with `spreading_activation(&priming_seed_ids, 2, 0.4)` — lower hops and stronger decay for the secondary pass.

**Tests**:
- A→B→C chain: query A → C appears in results with reduced weight
- Hop-2 activation < hop-1 activation at equal edge weight
- Cycle A→B→A doesn't loop
- 3-hop chain A→B→C→D: query A → D appears with progressively lower weight
- Existing priming test still passes

**Value**: Queries surface indirectly related concepts. "authentication" → "JWT" → "token expiry" — 2 hops away — now appears in results.

---

## Change 5: Sharp-Wave Ripple Replay Consolidation

**Problem**: Consolidation just clusters by cosine similarity. No replay of temporal co-occurrence patterns.

**Files**: `src/memory/mod.rs`

**Changes**:

Add method `fn replay_consolidation(&mut self)` called at start of `consolidate()`:
1. Find temporally proximate L2 entry pairs (last_access within `REPLAY_TEMPORAL_WINDOW` ticks)
2. Extract entities from both entries
3. Reinforce edges between shared entities with `REPLAY_EDGE_BOOST`
4. Create new "temporal" edges between unconnected co-active entities (weight 0.05)
5. Boost salience of replayed entries by `REPLAY_SALIENCE_BOOST`

Add constants:
```rust
const REPLAY_TEMPORAL_WINDOW: u64 = 5;
const REPLAY_EDGE_BOOST: f32 = 0.08;
const REPLAY_SALIENCE_BOOST: f32 = 0.02;
```

**Tests**:
- Two entries ticked within 5 ticks have shared entities' edge weights increased after consolidation
- Temporally distant entries (50+ ticks) don't get replay reinforcement
- Replay creates "temporal" edges between previously unconnected co-active entities
- Existing consolidation tests pass

**Value**: Querying one topic from a work session surfaces related topics from that same session, even if semantically different. Worked on "database migrations" and "API endpoints" together → temporal links improve associative recall.

---

## Change 6: Pattern Completion (CA3 Recall)

**Problem**: Partial/vague queries return weak results because cosine similarity alone doesn't reconstruct full memories from fragments.

**Files**: `src/memory/mod.rs`

**Depends on**: Change 4 (uses `spreading_activation`)

**Changes**:

Add method:
```rust
fn pattern_complete(&self, query: &str, partial_matches: &[MemorySnippet]) -> Vec<MemorySnippet>
```
1. From partial_matches, extract entities
2. Run `spreading_activation` to find related graph nodes
3. For activated nodes with `source_texts`, search L2 for entries containing those texts
4. Return with `completion_score = original_sim * 0.6 + activation * 0.4`

In `retrieve_context` (~line 1013): if fewer than 3 L2 results OR top result similarity < 0.5, call `pattern_complete` and merge into results.

Add constant: `const PATTERN_COMPLETION_MIN_RESULTS: usize = 3;`

**Tests**:
- Query single entity "Config" → retrieves longer memory containing "Config" via graph completion
- Pattern completion doesn't activate when 5 strong results already exist
- Completed results score lower than direct matches

**Value**: Vague queries like "that database thing" or just "Config" retrieve richer results by leveraging graph structure.

---

## Change 7: Enriched Synaptic Encoding on Edges

**Problem**: `GraphEdge` only has weight/kind/last_seen. No tracking of co-activation frequency or temporal patterns.

**Files**: `src/memory/mod.rs`

**Benefits from**: Change 4 (spreading activation uses edge properties)

**Changes**:

Add to `GraphEdge` (with `#[serde(default)]`):
```rust
pub activation_count: u32,       // times reinforced
pub temporal_pattern: f32,       // EMA of inter-activation intervals
```

In `upsert_edge`: increment `activation_count`, update `temporal_pattern` as EMA: `0.3 * interval + 0.7 * temporal_pattern`.

In `hebbian_reinforce`: logarithmic dampening based on activation_count: `boost = HEBBIAN_EDGE_BOOST / (1.0 + (activation_count as f32).ln())`.

In `spreading_activation` (from Change 4): modulate by temporal_pattern: `effective_weight = edge.weight * (1.0 / (1.0 + temporal_pattern * 0.1))` — frequently co-activated edges propagate more.

**Tests**:
- `activation_count` increases on repeated reinforcement
- `temporal_pattern` reflects recent intervals
- Frequently-activated edges have higher effective weight in spreading activation
- Existing Hebbian tests pass

**Value**: Spreading activation prefers paths through frequently co-activated edges. Many-session reinforcement > one-time coincidence.

---

## Change 8: Systems Consolidation (Hippocampal Independence)

**Problem**: Consolidated L2 entries get `consolidated=true` but L3 Summary nodes don't carry enough detail to replace them. Old memories become unretrievable.

**Files**: `src/memory/mod.rs`, `src/memory/summarize.rs`

**Changes**:

Add to `GraphNode` (with `#[serde(default)]`):
```rust
pub embedding: Vec<f32>,          // centroid embedding for direct similarity search
pub full_text: Option<String>,    // richer summary (up to 500 chars)
```

In `consolidate()`: when creating Summary nodes, compute centroid embedding (average + renormalize) and store richer `full_text`.

In `retrieve_context()`: after L2 retrieval, also scan L3 Summary nodes by cosine similarity using their embedding field. Include matches as additional results (up to 3, min similarity 0.3).

In pruning: consolidated L2 entries whose Summary node has a valid embedding get eviction score reduced by 0.2 (L3 can serve their role).

**Tests**:
- Consolidation produces Summary nodes with non-empty embeddings
- Query topics that exist only in consolidated form (L2 pruned) → still returns results via L3
- Centroid embedding is cosine-similar to each group member (>0.5)

**Value**: Old consolidated memories remain retrievable via L3 even after L2 entries decay. Solves the "I know I decided this months ago but can't find it" problem.

---

## Change 9: Neuroscience Terminology Alignment

**Problem**: Code uses generic terms. Should use standard neuroscience terminology for self-documenting code.

**Files**: `src/memory/mod.rs`

**Changes** (pure rename/doc pass, no behavioral changes):

Constants:
- `PROMOTION_SALIENCE_THRESHOLD` → `ATTENTION_GATE_THRESHOLD`
- `LABILE_WINDOW` → `RECONSOLIDATION_WINDOW`
- `SHORT_TERM_DECAY_RATE` → `HIPPOCAMPAL_DECAY_RATE`
- `LONG_TERM_DECAY_RATE` → `NEOCORTICAL_DECAY_RATE`

Doc comments on structs:
- L2 `short_term` → "Hippocampal episodic store"
- L3 `long_term` → "Neocortical semantic memory graph"
- `consolidate()` → "Systems consolidation (hippocampus → neocortex transfer)"
- `try_reconsolidate()` → "Memory reconsolidation — labile retrieval window"

Module-level doc comment mapping architecture to brain regions.

**Tests**: All 510+ tests pass unchanged (behavioral no-op). Grep for old names to verify full replacement.

**Value**: Codebase reads as a cognitive architecture document.

---

## Execution Order

```
1. Pattern Separation     ← highest value, no deps
2. Emotional Tagging      ← no deps
3. Forgetting Curve       ← no deps
4. Spreading Activation   ← no deps, unlocks 5/6
5. SWR Replay             ← benefits from 4
6. Pattern Completion     ← depends on 4
7. Synaptic Encoding      ← benefits from 4
8. Systems Consolidation  ← benefits from 5/6/7
9. Terminology            ← always last
```

Changes 1-3 are independent — can be done in any order. Change 4 unlocks 5, 6, 7. Change 8 is best after behavioral changes settle. Change 9 is always last.

## Migration

All new fields use `#[serde(default)]` — no new migration structs needed. MessagePack handles missing fields gracefully (proven by `test_msgpack_backward_compat_missing_fields`). Old files load with neutral defaults (emotional_valence=0.0, stability=1.0, activation_count=0).

## Verification

After each change:
1. `cargo test` — all tests pass
2. Manual smoke test with `cargo run --quiet -- memory tick/query/start`
3. Commit
