# Plan: Neuroscience-Aligned Memory Improvements (Phase 2)

## Context

The L1 working memory rework is complete (510 tests passing). Legend now has a 3-layer architecture:
- **L1 (Prefrontal Cortex)**: `Vec<WorkingMemoryEntry>`, capacity 10, attention-gated, rehearsal tracking
- **L2 (Hippocampus)**: `Vec<ShortTermEntry>`, capacity 1024, reconsolidation, labile windows
- **L3 (Neocortex)**: `GraphMemory` (HashMap nodes + Vec edges), Hebbian learning, multi-hop spreading activation

The user wants to improve query result quality and align more closely with neuroscience. Each change below is independently testable and committable. Ordered by dependency and value.

---

## Change 1: Pattern Separation (Dentate Gyrus) — DONE

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

## Change 2: Emotional Tagging (Amygdala) — DONE

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

## Change 3: Ebbinghaus Forgetting Curve + Spaced Repetition — DONE

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

## Change 4: Multi-Hop Spreading Activation — DONE

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

## Change 5: Sharp-Wave Ripple Replay Consolidation — DONE

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

## Change 6: Pattern Completion (CA3 Recall) — DONE

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

## Change 7: Enriched Synaptic Encoding on Edges — DONE

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

## Change 9: Dynamic Keyword Bootstrapping (Workspace-Adaptive Lexicon)

**Problem**: The keyword system relies heavily on hardcoded static arrays in `keywords.rs` — all software-engineering-centric. If Legend is used on a non-coding project (game design, writing, research), the keyword cache is filled with irrelevant terms like `fn `, `struct `, `pytest` while missing domain-specific vocabulary entirely. The `seed_tier1_keywords` function during `init` only seeds the static arrays + detected languages/tools; it never scans actual project content.

**Files**: `src/commands/init.rs`, `src/memory/keyword_cache.rs`, new `src/memory/keyword_bootstrap.rs`

**Current state**:
- `seed_tier1_keywords(report)`: seeds static keyword arrays + language-specific code keywords + matching tool keywords from tech_stack
- `seed_universal_keywords()`: seeds only static arrays (when no discovery report)
- `KeywordCache::from_graph()`: builds cache from graph `kw:*` nodes, falls back to static arrays per empty category
- `print_tier2_prompt()`: prints a manual KEYWORD: directive hint to stderr (rarely seen)

**Changes**:

### Phase A: Content-Driven Keyword Extraction
In new `src/memory/keyword_bootstrap.rs`:
```rust
pub fn bootstrap_keywords_from_workspace(report: &DiscoveryReport, memory: &mut MemoryState) -> usize
```
1. Read high-signal files from `report.high_signal_files` (README, docs, manifests, entry points)
2. Extract recurring nouns/phrases using lightweight NLP heuristics:
   - Term frequency across documents (TF threshold to filter noise)
   - Capitalized multi-word phrases (proper nouns → likely domain entities)
   - Heading text from markdown files (## headings = architecture/feature names)
   - Dependency names from manifests (Cargo.toml, package.json, requirements.txt, etc.)
   - Config key names from config files (.env.example, settings files)
3. Classify extracted terms into categories using context heuristics:
   - Terms from dependency manifests → `tool`
   - Terms from README headings / feature descriptions → `architecture`
   - Terms from CI/deployment configs → `environment`
   - Recurring capitalized terms in code → `code` (with entity_kind/context metadata)
   - Domain nouns that don't fit other categories → new `domain` category
4. Seed as `kw:<category>:<term>` graph nodes with source_text metadata for traceability

### Phase B: Domain Category
Add new `domain` keyword category to `keywords.rs`, `KeywordCache`, `keyword_cache.rs`:
- `DOMAIN_KEYWORDS: &[&str]` — initially empty (purely graph-driven)
- `KeywordCache.domain: Vec<String>` — populated from graph only
- Used in `compute_salience` and `compute_emotional_valence` as a neutral-weight signal
- Allows Legend to learn project-specific vocabulary: game design terms, biology terms, etc.

### Phase C: Incremental Learning During Ticks
In `tick_impl`, after entity extraction:
- If an extracted entity appears 3+ times across L2 entries but isn't in the keyword cache → auto-promote to a `kw:domain:<term>` graph node
- Threshold-based self-expansion: the keyword system grows as the user works

### Phase D: Workspace Re-scan Command
Add `legend memory rescan` command:
- Re-runs workspace discovery + bootstrap
- Merges new keywords without removing user-added ones
- Useful after major project changes (new dependencies, restructuring)

**Tests**:
- Bootstrap from a fixture workspace with README.md + Cargo.toml → extracts project name, dependencies as tool keywords, README headings as architecture keywords
- Bootstrap from a non-code project (markdown-only workspace) → extracts domain keywords from headings and recurring terms
- Incremental learning: tick the same entity 3+ times → auto-promoted to domain keyword
- `rescan` merges without duplicating existing keywords
- Empty workspace → falls back to static arrays gracefully
- Existing keyword_cache tests still pass

**Value**: Legend becomes genuinely workspace-adaptive. A game designer gets keywords like "sprite", "tilemap", "collision"; a researcher gets "hypothesis", "methodology", "dataset". The hardcoded software keywords serve as a reasonable fallback but are no longer the ceiling.

---

## Change 10: Brain-Region Module Structure

**Problem**: `mod.rs` is a 4700+ line monolith. The codebase should be organized by brain region so each module is a distinct neural subsystem with clear inputs/outputs.

**Files**: `src/memory/mod.rs` → split into brain-region modules

**Changes**:

Extract from `mod.rs` into dedicated modules:
- `src/memory/dentate_gyrus.rs` — **DONE** (pattern separation, orthogonalization, diversity gating)
- `src/memory/amygdala.rs` — emotional valence computation (from `embed.rs` after Change 2)
- `src/memory/hippocampus.rs` — L2 episodic store: `ShortTermEntry`, `insert_short_term`, `find_best_match`, `top_k_similar`, `try_reconsolidate`, labile window logic, reconsolidation
- `src/memory/neocortex.rs` — L3 graph: `GraphMemory`, `GraphNode`, `GraphEdge`, `update_graph`, `graph_lookup`, `spreading_activation`, `hebbian_reinforce`, consolidation
- `src/memory/prefrontal.rs` — L1 working memory: `WorkingMemoryEntry`, `push_working_memory`, `flush_working_memory`, attention gate
- `src/memory/basal_ganglia.rs` — procedural/habit learning: `reinforce()`, AdaGrad, contrastive descent

Rename constants:
- `PROMOTION_SALIENCE_THRESHOLD` → `ATTENTION_GATE_THRESHOLD`
- `LABILE_WINDOW` → `RECONSOLIDATION_WINDOW`
- `SHORT_TERM_DECAY_RATE` → `HIPPOCAMPAL_DECAY_RATE`
- `LONG_TERM_DECAY_RATE` → `NEOCORTICAL_DECAY_RATE`

`mod.rs` becomes a thin orchestrator that imports from brain-region modules and wires them together (like the thalamus routing signals between regions).

**Tests**: All tests pass unchanged (behavioral no-op, pure restructure).

**Value**: Codebase reads as a cognitive architecture document. Each future change lands in the obvious module.

---

## Change 11: Neuroscience Terminology Alignment

**Problem**: After module split, add doc comments and rename remaining generic terms.

**Files**: All brain-region modules

**Changes** (pure doc/rename pass, no behavioral changes):

Module-level doc comments mapping each file to its brain region and function.
Remaining constant renames that weren't handled in Change 9.

**Tests**: All tests pass unchanged.

**Value**: Final polish — code is fully self-documenting as a cognitive architecture.

---

## Change 12: Emotional Intensity Consolidation Trigger (Amygdala-Driven) — DONE

**Problem**: Consolidation fires on a fixed tick count (every 15). The brain consolidates more aggressively after emotionally significant events — a burst of bug reports or critical decisions should trigger consolidation sooner.

**Files**: `src/memory/mod.rs`

**Changes**:

Track rolling emotional intensity in `tick_impl`:
- Add `recent_valence_sum: f32` to `MemoryState` (`#[serde(default)]`)
- In `tick_impl`: add `|emotional_valence|` to `recent_valence_sum`; decay sum each tick by `*= 0.8`
- Add constant `EMOTIONAL_CONSOLIDATION_THRESHOLD: f32 = 1.5`
- If `recent_valence_sum >= threshold` after a tick, set `consolidation_suggested = true` regardless of tick count

**Tests**:
- 3 high-valence ticks in a row → consolidation suggested (even though < 15 ticks)
- 3 neutral ticks → no early suggestion
- Rolling decay prevents stale accumulation

**Value**: Bug storms and critical decisions trigger immediate consolidation, strengthening memory of important events. Routine ticks wait for the normal 15-tick cycle.

---

## Change 13: Context Switch Consolidation Trigger (Hippocampal Novelty Detection) — DONE

**Problem**: When the user switches topics dramatically (from "database migrations" to "CSS styling"), the previous context should be consolidated before the new one overwrites working memory. The hippocampus detects novelty via pattern mismatch.

**Files**: `src/memory/mod.rs`

**Changes**:

Track topic coherence in `tick_impl`:
- Add `last_tick_embedding: Vec<f32>` to `MemoryState` (`#[serde(default)]`)
- In `tick_impl`: compute cosine similarity between new tick's embedding and `last_tick_embedding`
- Add constant `CONTEXT_SWITCH_THRESHOLD: f32 = 0.15` — if similarity drops below this, it's a topic shift
- On context switch: set `consolidation_suggested = true` and optionally call `flush_working_memory()` to grade L1 entries from the old context
- Update `last_tick_embedding` after each tick

**Tests**:
- Tick "database migration schema" then "CSS flexbox layout" → consolidation suggested
- Tick "database migration schema" then "database index optimization" → no suggestion (same topic)
- First tick ever (no previous embedding) → no suggestion

**Value**: Natural "micro-nap" when switching tasks. Previous work session's memories are consolidated before the new topic floods working memory.

---

## Change 16: Neural Network Feasibility Review

**Problem**: The current system uses a graph with entity nodes and edge weights as the substrate for spreading activation, pattern completion, Hebbian learning, and associative recall. While effective and debuggable, real brain regions (CA3, CA1, entorhinal cortex) use actual recurrent neural networks — attractor dynamics, weight matrices, and iterative convergence. Some subsystems may produce better results as small neural nets.

**Scope**: This is a research/evaluation change, not a code change. After all behavioral changes (1-13) are settled and the system has real usage data, systematically review each component and assess whether replacing the graph-based implementation with a small neural network would improve quality.

**Components to evaluate**:

1. **CA3 Pattern Completion** (Change 6) — Currently graph-based spreading activation. Could be a Hopfield network / modern continuous Hopfield net. Depends heavily on embedding quality — bag-of-words embeddings may produce shallow attractor basins. Most viable if Legend moves to LLM-generated embeddings.

2. **Dentate Gyrus Pattern Separation** (Change 1) — Currently threshold + sparse orthogonalization. Could be a competitive learning network that learns to maximally separate similar inputs. Would need training signal (when do two memories collide in practice?).

3. **Entorhinal Cortex Encoding** — Currently `embed_text` (bag-of-words hash). A learned encoder (even a tiny single-layer net trained on the user's corpus) could produce higher-quality embeddings that better capture semantic relationships specific to the workspace.

4. **Hebbian Learning** — Currently additive weight updates on edges. Could be replaced with a proper Hebbian/Oja rule operating on a weight matrix, with built-in normalization and competition.

5. **Salience/Attention Gating** — Currently keyword-based scoring. Could be a small classifier trained on which memories the user actually retrieves (implicit labels from usage patterns).

**Evaluation criteria for each**:
- Does the neural net approach measurably improve retrieval quality?
- What's the capacity vs interference tradeoff?
- Is it debuggable enough for a developer tool?
- What's the storage/compute overhead?
- Does it require training data the system naturally produces?

**Output**: A decision document per component: keep graph-based, switch to neural net, or hybrid approach. Implement the winners as follow-up changes.

**Value**: Ensures Legend's architecture evolves toward genuine neural computation where it helps, while keeping graph-based implementations where they're sufficient. Avoids premature complexity.

---

## Change 14: Structural Synaptic Plasticity (Multi-Edge Connections)

**Problem**: Currently each node pair has a single edge with one weight. In the brain, Kandel (Nobel 2000, *Aplysia* studies) showed that learning physically grows new synaptic terminals between neurons — it's not just one connection getting stronger, it's structurally denser wiring. Our single edge collapses all contextual relationships into one weight. "Config" ↔ "JWT" connected via authentication AND token expiry are indistinguishable.

**Brain analogy**: Structural plasticity — habituation retracts synaptic terminals, sensitization grows new ones. Multiple parallel synapses between the same neuron pair encode different facets of the relationship. Selective pruning of one synapse doesn't destroy the others.

**Files**: `src/memory/mod.rs`

**Depends on**: Change 15 (contextual queries make multi-edges useful)

**Changes**:
- Modify `upsert_edge` to match on `(from, to, kind)` instead of just `(from, to)` — allows multiple edges between the same pair with different kinds
- When a new kind of relationship is discovered between an existing pair, create a new edge rather than upgrading the existing one's kind
- Preserve kind-upgrade logic for genuinely hierarchical kinds (e.g., "related" → "contains")
- Update `spreading_activation` to traverse all edges from a node (naturally works with multiple edges)
- Update `prune_graph` edge pruning to handle multiple edges per pair
- Each edge independently tracks its own activation_count, stability, and interval EMAs

**Tests**:
- Same node pair can have "contains" and "temporal" edges simultaneously
- Reinforcing one edge doesn't affect the other's stability/activation_count
- Pruning a low-weight edge between a pair preserves the other edge(s)
- Spreading activation traverses all edges from a node
- Existing edge behavior unchanged for single-kind pairs

**Value**: Different contextual associations between concepts are preserved independently. A temporal work-session link can decay without destroying the structural "contains" relationship. Enables context-aware queries (Change 16).

---

## Change 15: Context-Aware Spreading Activation

**Problem**: Spreading activation treats all edges equally regardless of kind. A query about "authentication" follows "temporal" edges (same work session) with the same weight as "contains" edges (structural code relationships). This dilutes results with irrelevant associations.

**Brain analogy**: Different neurotransmitter systems (dopamine, serotonin, acetylcholine) modulate which neural pathways are active. Context biases which connections propagate — emotional context activates amygdala pathways, spatial context activates hippocampal place cells. The same neurons participate in different circuits depending on the active neuromodulatory state.

**Files**: `src/memory/mod.rs`

**Depends on**: Change 14 (multi-edge makes this meaningful)

**Changes**:
- Add optional `context_filter: Option<&[&str]>` parameter to `spreading_activation` — list of preferred edge kinds
- When context_filter is set, preferred-kind edges get full weight, other kinds get a dampening factor (e.g., 0.3)
- Infer context from query: extract entities, check their dominant edge kinds in the graph, use those as the context filter
- Add `fn infer_query_context(&self, query: &str) -> Vec<String>` method
- Wire into `retrieve_context`: pass inferred context to spreading activation and priming

**Tests**:
- Query about code ("fn handle_auth") preferentially follows "contains"/"represents" edges
- Query about a work session preferentially follows "temporal" edges
- Without context filter, behavior identical to current (backward compat)
- Context inference extracts dominant edge kinds from query entities

**Value**: Queries return more relevant results by following contextually appropriate graph pathways. "What did I work on yesterday?" follows temporal edges. "How does auth work?" follows structural edges.

---

## Execution Order

```
1. Pattern Separation            ← DONE (+ dentate_gyrus.rs module)
2. Emotional Tagging             ← DONE (+ amygdala.rs module, 568 tests passing)
3. Forgetting Curve              ← DONE (stability + spaced repetition, 578 tests passing)
4. Spreading Activation          ← DONE (multi-hop BFS, 590 tests passing)
5. SWR Replay                    ← DONE (temporal co-occurrence, 600 tests passing)
6. Pattern Completion            ← DONE (CA3 autoassociative recall, 622 tests)
7. Synaptic Encoding             ← DONE (dual-timescale EMA, 634 tests passing)
8. Systems Consolidation         ← benefits from 5/6/7
9. Dynamic Keyword Bootstrap     ← no deps, benefits from init/discover
10. Brain-Region Modules         ← after all behavioral changes settle
11. Terminology                  ← always last
12. Emotional Consolidation      ← DONE (amygdala-driven, 605 tests passing)
13. Context Switch Consolidation ← DONE (novelty detection, 605 tests passing)
14. Structural Synaptic Plasticity ← depends on 7, unlocks 15
15. Context-Aware Spreading Activation ← depends on 14
16. Neural Net Feasibility Review ← after all behavioral changes, with usage data
```

Changes 1-3 are independent — can be done in any order. Change 4 unlocks 5, 6, 7. Change 8 is best after behavioral changes. Change 9 (keyword bootstrap) can be done at any point but benefits from having the emotional valence keywords (Change 2) and architecture settled. Changes 10-11 are structural/doc cleanup, always last. Changes 12-13 are smart consolidation triggers — can be done any time after the SWR replay foundation (Change 5) is in place. Change 14 is a research review — always after all behavioral and structural changes are settled.

## Migration

All new fields use `#[serde(default)]` — no new migration structs needed. MessagePack handles missing fields gracefully (proven by `test_msgpack_backward_compat_missing_fields`). Old files load with neutral defaults (emotional_valence=0.0, stability=1.0, activation_count=0).

## Verification

After each change:
1. `cargo test` — all tests pass
2. Manual smoke test with `cargo run --quiet -- memory tick/query/start`
3. Commit
