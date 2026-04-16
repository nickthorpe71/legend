# Tick Improvement Plan

Status key: `[ ]` todo, `[~]` in progress, `[x]` done, `[?]` needs research first

---

## Phase 1 — Cleanup & Simplification ✓ COMPLETE
Remove dead code and side effects. Low risk, clears the path for deeper work.

- [x] Remove passive ticks entirely (`tick_passive`, `passive` param, EXPERIENCE prefix, all callers/tests)
- [x] Remove ARCHITECTURE.md auto-append (CLI and MCP)
- [x] Remove pending tick counter (`.pending_ticks`)
- [x] Remove `--blocker` flag (CLI def, tick handler, conformance test)
- [x] Remove character limit and noise exclusions — only block empty ticks (`is_noise_tick` removed)
- [x] Slim CLI and MCP to thin wrappers that just call legend core
- [x] Fix gotcha 1: stale comment about consolidated entries filtered from query (test comment already corrected)
- [x] Fix gotcha 2: stale `TickResult.action` comment — added `working_memory_only`
- [x] Fix gotcha 5: redundant `should_suggest_consolidation()` post-tick CLI check already removed (function kept)
- [x] Fix gotcha 6: unreachable `>50` word-count branch in thalamus — inverted condition order
- [x] Fix gotcha 7: multi-chunk ticks return last chunk result — confirmed intentional by design
- [~] Fix gotcha 4: L1 displacement/flush should carry wall_clock/date/TCM metadata → **moved to Phase 3** (requires WorkingMemoryEntry struct changes + temporal propagation design)

## Phase 2 — Understand & Document ✓ COMPLETE
Research sessions to answer open questions before changing core logic.

Verdict key: **OK** = working correctly, **CONCERN** = works but has design issues for later phases, **BUG** = needs fixing.

### 1. Tick Result State Initialization — **OK**

**Purpose**: The caller (CLI, MCP, or viz bridge) needs a single answer: "what did Legend do with my tick?" Did it create a new memory? Merge into an existing one? Keep it only in working memory? The result also carries back related memories so the LLM sees what context was activated by this tick.

**How it works**: `tick_impl` initializes four tracking variables (`mod.rs:488-491`):
- `result_action` — starts as `"created"`, overwritten to `"merged"` if a chunk merged, or `"working_memory_only"` if salience was too low for L2
- `result_entry_id` — the ID of the created or merged entry (so the caller can reference it)
- `result_matched` — `Some(id)` if merged into an existing entry, `None` if created new
- `result_similarity` — the similarity score that triggered a merge (useful for debugging/logging)
- `last_context` — the `MemoryContext` from `retrieve_context()`, giving the caller related L1/L2/L3 memories

For multi-chunk ticks, each chunk overwrites the previous values. The final `TickResult` reflects the last chunk's outcome. This is intentional — the caller gets one summary, but all chunks independently mutate memory.

### 2. Batch Embedding — **OK**

**Purpose**: Embedding is the slowest step in a tick (~80% of latency). Without batching, a 3-chunk tick would lock the ONNX model mutex 3 times and run 3 separate forward passes. Batching amortizes this to 1 lock + 1 pass, cutting tick latency roughly in half for multi-chunk inputs.

**How it works**: Text is chunked first by `chunk_text()` (sentence-boundary splitting, ~200 word target per chunk). Then all chunks are embedded in a single `embed_texts_batch()` call — one mutex acquisition, one ONNX session forward pass. Each chunk still gets its own embedding vector for independent L2 storage, similarity matching, and merge decisions.

**Why chunk at all (instead of embedding whole text)?** MiniLM's tokenizer truncates at 512 tokens — long texts lose information. Per-chunk embeddings also enable fine-grained merge: chunk A might merge with existing entry X while chunk B creates a new entry. Each chunk gets independent salience scoring too.

**Alternatives**: Whole-text embedding with extractive indexing (one embedding, keyword/position index for sub-retrieval). Would reduce storage but lose per-chunk merge/salience granularity.

### 3. Emotional Valence — **CONCERN**

**Purpose**: Not all memories are equal — emotionally charged events (bugs, breakthroughs, shipped features) should be harder to forget, easier to retrieve, and should trigger faster consolidation. This models how the amygdala modulates memory formation in the brain.

**How it works** (`amygdala.rs`): Keyword matching against `KeywordCache` lists:
- `negative_valence`: "crash" (-0.5), "bug" (-0.3), "vulnerability" (-0.5), etc.
- `positive_valence`: "shipped" (+0.5), "fixed" (+0.4), "success" (+0.3), etc.
- `urgency` amplifier: "blocker", "critical", "P0" push magnitude toward ±1.0
- Final value clamped to [-1.0, 1.0]

**Where it flows** (5 downstream effects):
1. **Slower forgetting**: Decays at half the hippocampal rate — emotional memories resist time-based fade
2. **Retrieval boost**: `|valence| * 0.05` added to similarity during query — emotional memories surface more readily
3. **Eviction resistance**: `|valence| * 0.15` added to eviction score — emotional memories harder to displace from L2
4. **Consolidation trigger**: `|valence|` accumulated into rolling intensity sum → triggers early consolidation when threshold hit
5. **CPEB tagging**: High valence events strengthen recently-active graph edges

**Concern**: Keyword lists are code/programming-biased. Non-programming emotional signals miss entirely. Phase 3/5 territory.

### 4. CPEB Synaptic Tagging — **CONCERN**

**Purpose**: When something emotionally significant happens, you don't just remember the event — you also strengthen your memory of what you were doing right before it. If you're working on Redis caching and then discover a critical bug, the Redis knowledge should be reinforced too. CPEB models this: a high-emotion event "captures" recently-active neural connections.

**How it works** (`neocortex::cpeb_tag_edges`, `mod.rs:799`): After all chunks are processed, if `|tick_valence| > 0.3`:
- Scans all graph edges
- If `clock - edge.last_seen <= 5` (recently active), boosts `edge.stability += 1.5 * |valence|`
- Stability capped at 10.0

**What stability does**: (1) Slows edge decay (higher stability = more persistent connections), (2) Amplifies spreading activation propagation (`activation *= edge.weight * stability.sqrt()`), (3) Interacts with spaced repetition tracking.

**Concern**: The 10.0 cap is arbitrary — should be distribution-based normalization (Phase 6). Also, tagging ALL edges seen in the last 5 ticks is very broad — could tag irrelevant edges that happen to be recent.

### 5. Rolling Emotional Intensity — **OK**

**Purpose**: A single mildly emotional tick shouldn't trigger consolidation, but a *burst* of emotional ticks should. If you're debugging a cascade of crashes, that cluster of stress signals should cause Legend to consolidate and organize what it's learning. This models stress-induced memory formation.

**How it works**: `state.recent_valence_sum` is a rolling accumulator:
- Each tick: `recent_valence_sum *= 0.8` (exponential decay)
- Then: `recent_valence_sum += |tick_valence|`
- When `recent_valence_sum >= 1.5`, auto-consolidation fires

**Example**: 3 consecutive crash-related ticks each contributing ~0.5 valence → sum ≈ 0.5 + 0.4 + 0.32 = 1.22. One more emotional tick pushes past 1.5 → consolidation.

**Half-life**: After 5 idle ticks, sum drops to ~33%. After 10, ~11%. So the burst has to be relatively concentrated.

### 6. Dentate Gyrus Sparse Orthogonalization — **OK**

**Purpose**: When you learn "chose Redis for caching" and then "chose Postgres for persistence," these are related but distinct decisions. Without orthogonalization, their embeddings are so similar that querying one often returns the other as noise. The dentate gyrus pushes them apart just enough that retrieval can distinguish them, while still allowing genuine duplicates to merge.

**How it works** (`dentate_gyrus.rs`): For each existing L2 embedding, check cosine similarity with the new embedding:
- `sim < theta_low (0.75)`: **Leave alone** — already different enough
- `theta_low ≤ sim < theta_high (0.85)`: **Orthogonalize** — subtract a scaled projection of the existing embedding from the new one, then renormalize. Strength 0.3.
- `sim ≥ theta_high (0.85)`: **Leave alone** — these should merge, not separate

**Concrete example**:
- Existing: "chose Redis for caching because pub/sub" (embedding B)
- New: "chose Postgres for persistence because ACID" (embedding A)
- Similarity: ~0.75 (confusable zone)
- Action: A' = normalize(A - 0.3 × (A·B) × B)
- Result: A' is pushed away from B, retrieval can now distinguish them

### 7. Context-Switch Detection and L1 Flush — **OK**

**Purpose**: L1 (working memory) holds the last ~10 ticks. If you suddenly switch topics (e.g., from "Redis architecture" to "planning vacation"), the old L1 entries would just sit there taking up slots until displaced. Context-switch detection catches this and flushes all L1 entries to L2, ensuring nothing is lost when the conversation topic changes.

**How it works** (`mod.rs:822-852`): After all chunks are processed, compares the current tick's embedding against `state.last_tick_embedding`. If cosine similarity < 0.15 → topic shift detected → `flush_working_memory()` promotes all unpromoted L1 entries to L2.

**Why 0.15**: MiniLM embeddings are normalized, so even vaguely related texts score >0.3. A threshold of 0.15 means "almost completely unrelated." This is conservative — better to miss a context switch than to flush too aggressively and interfere with normal topic drift.

### 8. L2 Capacity Eviction — **CONCERN**

**Purpose**: L2 (episodic memory) has a capacity limit (default 200 entries). When full, Legend must decide which memory to forget. The eviction score balances importance (salience), usage frequency, recency, and emotional charge — mimicking how the brain prioritizes memories.

**How it works** (`hippocampus::insert_short_term`, triggered at capacity):
```
score = salience * 0.4 + ln(1+usage) * 0.3 + e^(-age * 0.002) * 0.3 + |valence| * 0.15
```
Lowest-scoring entry is evicted. Consolidated entries with embedded Summary nodes get -0.2 to their score (safer to evict since L3 can independently serve them).

**Concern**: Evicted entries are simply removed. There's no "distill facts before evicting" step. If consolidation hasn't happened yet, information is lost. In practice, auto-consolidation fires every 15 ticks, so most entries get consolidated before L2 fills up. But under heavy load (many rapid ticks), eviction could outpace consolidation.

### 9. Consolidation Grouping — **CONCERN**

**Purpose**: Consolidation transforms many similar episodic memories (L2) into organized knowledge (L3). Grouping is the first step — find which memories are about the same topic so they can be summarized together. Like how sleeping on a problem helps organize scattered observations into coherent understanding.

**How it works** (`consolidate`, line 1280): Single-pass greedy clustering:
1. Take first unused entry as seed
2. Add all entries with `cosine_similarity(seed, entry) >= theta_low (0.75)` to the group
3. Repeat for next unused entry
4. Only multi-entry groups (size > 1) are processed. Singletons are skipped.

**Risk**: Greedy approach means seed choice matters. Entry i might match j and k, but j and k might not match each other — they all get grouped anyway. Distant entries within a group could lose nuance in the summary.

**Mitigation**: theta_low = 0.75 is strict — entries must be quite similar. Also, L2 entries are NOT deleted — they're marked `consolidated = true` and remain retrievable. So even a bad grouping doesn't lose data.

### 10. Summarize Group — **OK (no data loss)**

**Purpose**: After grouping similar L2 entries, create a concise label for the Summary node. This label is used for graph indexing and deduplication — it's how Legend finds existing summaries to merge into rather than creating duplicates.

**How it works** (`entorhinal::summarize_group`): Extracts keywords from each entry in the group, deduplicates, and constructs a label like `"keyword1 | keyword2 | keyword3"`. This is keyword extraction, NOT LLM summarization.

**No data loss**: Original L2 entries remain in `short_term` (marked consolidated). Source texts are copied to the Summary node's `source_texts` field (capped at 20). The summary label is a keyword digest for graph indexing, not a replacement for the original text.

### 11. Systems Consolidation — **OK**

**Purpose**: Make important memory groups independently queryable even after their L2 entries decay or get evicted. Without this, consolidated memories can only be found via graph traversal (entity → edge → Summary). With a centroid embedding, the Summary node can be found by direct similarity search — like how deeply learned knowledge can be recalled directly without needing to reconstruct the original experience.

**How it works** (line 1346): Only for groups where `avg_salience >= 0.4`:
- Computes centroid embedding (average of all group embeddings, renormalized to unit length)
- Stores `full_text` (concatenated source texts, capped at 500 chars) on the Summary node
- During retrieval, Summary nodes with embeddings are directly compared against the query embedding

**Why 0.4**: Only "important" memory groups deserve full neocortical encoding. Low-salience groups get a Summary node but no embedding — they can only be found via graph traversal.

### 12. Create/Merge Summary Node — **OK**

**Purpose**: Prevent Summary node proliferation. If you consolidate memories about "Redis caching" three times, you should have one strong Summary node, not three weak ones. Merging strengthens existing knowledge rather than fragmenting it.

**How it works**: During consolidation, for each multi-entry group:
- **Merge** (if existing Summary has same label or ≥0.4 Jaccard word overlap): Update weight/salience, extend source_texts (cap 20), update embedding/full_text
- **Create** (otherwise): New Summary node with label = keyword summary, weight = 1.0 + salience

### 13. Semantic Topic Extraction — **OK**

**Purpose**: Connect Summary nodes to the broader knowledge graph. If a group of memories all mention "Redis," then "Redis" should be linked to the group's Summary. This means queries about "Redis" can find the consolidated knowledge even without exact text matches.

**How it works** (line 1486): Counts entity frequency within the group. Entities appearing in >50% of entries (and count > 1) become Topic nodes linked to the Summary via "represents" edges.

### 14. Re-update Graph and Mark Consolidated — **OK**

**Purpose**: Two goals. (1) Ensure L3 captures the full group's entity relationships. During the original ticks, chunking boundaries might have split entities across chunks — re-running extraction on each entry's full text catches what was missed. (2) Mark L2 entries as consolidated so eviction scoring can discount them.

**How it works** (line 1562): For each entry in the group: `update_graph()` re-extracts entities and reinforces edges. Then updates the L2 entry: `usage += 1`, `last_access = clock`, `consolidated = true`.

### 15. Event Log Performance — **OK**

**Purpose**: Debugging and benchmark analysis. The event log records what Legend did (tick, query, start) with rich metadata.

**Impact**: Append to `.legend/events.jsonl` — one `serde_json::to_string` + file append per tick/query. Measured overhead: <1ms. File grows unboundedly but is only read by external tools, not by Legend itself.

### 16. Why Prune Multiple Times? — **OK**

**Purpose**: Each prune operates on a different state. The post-tick prune catches entries that decayed during the tick. The post-consolidation prune catches entries that (a) further decayed during consolidation time (clock incremented again), and (b) now qualify for removal because their Summary node has a valid embedding. Not redundant — they serve different decay windows.

### 17. What Makes a Tick High Salience — **OK**

**Purpose**: The attention gate (`ATTENTION_GATE_THRESHOLD = 0.25`) decides whether a tick promotes to L2 or stays in L1 only. Understanding the scoring clarifies which ticks Legend considers worth remembering long-term.

| Signal | Score | Running total |
|---|---|---|
| Floor (all text) | 0.05 | 0.05 |
| 1 decision keyword ("decided") | +0.3 | 0.35 ✓ promotes |
| + rationale ("because") | +0.15 | 0.50 |
| Bug keyword ("crash") | +0.4 | 0.45 alone ✓ |
| TODO/blocker keyword | +0.3 | 0.35 alone ✓ |
| Architecture keyword | +0.25 | 0.30 ✓ |
| Preference keyword | +0.3 | 0.35 ✓ |
| 1 code definition | +0.2 | 0.25 ✓ barely |
| >50 words | +0.20 | — additive |

Plain text with no keywords, short, no code → 0.05 → L1 only. Any single decision/bug/preference keyword → promotes.

### 18. Should retrieve_context Return Results to the LLM? — **OPEN**

**Purpose of the question**: `retrieve_context()` is called inside `tick_impl` (line 739) for high-salience chunks. The results go into `TickResult.context`, which CLI/MCP formats and returns. But this also has side effects.

**Pro**: The LLM immediately sees what related memories exist — useful for continuity.
**Con**: It's an implicit side effect inside a "write" operation: increments clock, applies decay, auto-reinforces top result, does Hebbian reinforcement.

**Verdict**: Keep for now. The context return is useful. Side effects are beneficial (reinforcing related memories during encoding). Consider making side effects optional in Phase 6.

### 19. Clock Increment During Tick — **CONCERN**

**Purpose of the question**: `tick_impl` increments clock once (line 455). Then each high-salience chunk's `retrieve_context()` (line 739) increments again. N high-salience chunks = N+1 clock increments per tick.

**Impact**: All decay, eviction scoring, normalization, and temporal ordering depend on clock. Extra increments mean faster decay and temporal context that doesn't accurately represent wall-clock time.

**Verdict**: Phase 6 item. Options: (1) retrieve_context skips clock increment when called from tick, (2) separate "query for context" from "full retrieve with side effects."

### 20. Gotcha 3: retrieve_context Side Effects During Tick — **CONCERN**

**Purpose of the question**: Same as #18/#19. The internal `retrieve_context()` during tick causes 5 side effects: clock increment, decay, auto-reinforce, Hebbian reinforcement, and contrastive descent recording. These are "read path" effects during a "write path" operation. Not a bug — intentionally models hippocampal pattern completion during encoding. But adds complexity and makes tick behavior harder to reason about.

## Phase 3 — Salience & Scoring Overhaul
Fix signal quality — the foundation everything else depends on.

- [ ] Remove code/programming bias from salience scoring in thalamus
- [ ] Make learned domain vocabulary the dominant salience signal
- [ ] Replace `min(1.0, ...)` salience clamping with gradient-based normalization (backprop-style)
- [ ] Replace `min(1.0, ...)` in low-similarity merge salience with same approach
- [ ] Rethink source reference extraction: extract quantitative data, not just code file refs
- [ ] Final salience score should be normalized, not clamped to [0.05, 1.0]
- [ ] Review term-frequency promotion filters (purely numeric exclusion, co-occurrence requirement)
- [ ] L1 displacement/flush should carry wall_clock/date/TCM metadata (moved from Phase 1)

## Phase 4 — Chunking & Embedding Strategy
Improve how we split and represent text before storage.

- [ ] Rethink chunking to keep related content together instead of splitting on sentences
- [ ] Investigate using embedded ML model for smarter chunking boundaries
- [ ] Clarify batch embedding strategy: why split then embed together, what alternatives exist
- [ ] Consider whether chunking is even the right approach vs. whole-text embedding with extractive indexing

## Phase 5 — Graph & Entity Extraction Overhaul
The "find signal in noise" work. Defines what Legend considers a fact.

- [ ] Define what constitutes a fact vs. general information (not code-biased)
- [ ] Thorough entity extraction logic — pull out key facts from any domain
- [ ] Edge creation and reinforcement: make it brain-inspired (not just co-occurrence)
- [ ] Review edge types: are chunk-based types sufficient? What about causal, temporal, contradicts?
- [ ] Should keyword lexicon live in the graph instead of separate?
- [ ] L3 node capacity: pre-allocate (NASA-style) vs unbounded with pruning?
- [ ] L3 edge capacity: same question — bounded or pruning-only?
- [ ] Ensure consolidation grouping doesn't lose data (maybe append instead of summarize for low-sim merge)
- [ ] L3 graph update should extract important facts, not be code-biased

## Phase 6 — Normalization & Timing
Fix arbitrary intervals and capping with principled approaches.

- [ ] Signal-based normalization intervals (not every N clock ticks)
- [ ] Clock increment strategy: should retrieve_context during tick advance the clock?
- [ ] Remove CPEB stability cap (10.0) — use proper normalization instead
- [ ] Remove salience clamping throughout — use distribution-based normalization

---

## Previously Addressed

- [x] Temporal metadata extraction (TCM, wall_clock, extracted_dates)
- [x] Adaptive relevance thresholds for query output (MIN_QUERY_SIMILARITY raised, adaptive floor)
- [x] Remove text truncation from start/query output (relevance-based filtering instead)
