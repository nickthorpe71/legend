# TODO — Carmack-Pass Punch List

Full audit findings in `~/.claude/plans/iterative-mapping-puppy.md`. Grouped by phase; tackle in order.

## Phase 1 — Free wins (low risk, high readability ROI)

### Comment sweep — missed "Step N" refs
- [x] `src/inference/deberta/predict.rs:5, 12` — rewrote to self-describe (no pipeline-position framing)
- [x] `src/inference/deberta/preprocess.rs:49` — rewrote to self-describe
- [x] `src/inference/deberta/mod.rs:14` — "Step 5" → "callers"
- [x] `src/tick_pipeline/frame.rs:120, 548, 864` — function names + generic phrasing
- [x] `examples/verify_seed_yaml.rs:4` — "downstream of Step 1" → "the YAML's own structural sanity"

### Honest panic messages
- [x] `src/tick_pipeline/route_regions.rs:122` — `partial_cmp(&a.1).unwrap()` → `.unwrap_or(Ordering::Equal)` with explanatory comment (skipped the debug_assert per the no-belt-and-suspenders rule)
- [x] `src/intent_classifiers.rs:85-89` — rewrote with actionable "regenerate with ..." message
- [x] `src/tick_pipeline/coref.rs:279` — `.expect("by_element must contain every id from `order`; both are built from the same recent_focus walk in the same pass")`
- [x] `src/tick_pipeline/temporal.rs` — added `lazy_regexes_compile` test that forces all three `LazyLock<Regex>` to evaluate

### Silent error logging
- [x] `src/daemon.rs:218` — both cleanup paths now log on Err
- [x] `src/persistence.rs` — promoted from "log cleanup errors" to the real fix: `TmpFileGuard` RAII removes the orphan `.tmp.<pid>` file on any early exit from `save()` (write/sync/rename failure). Resolves the silent-leak that prompted Phase 4 "atomic-save recovery"; remaining Phase 4 piece is the startup sweep for tmp files from crashed processes.

### Broken test
- [x] `tests/persistence_session_to_session.rs::support_count_accumulates_across_sessions` — rewrote with explicit reuse-path mechanics: tick same input twice in-session (bumps via reuse), cross session boundary, tick a third time (bumps post-load). Asserts exact counts (0 → 1 → 1 across load → 2) instead of weak `>= 1`.

## Phase 2 — Cheap perf wins

- [x] `src/tick_pipeline/topical.rs:40` — switched to bounded `BinaryHeap<Reverse<ScoredId>>` of size k+1 with a local `ScoredId` wrapper that gives `f32` a total ordering (high-score-wins, tie-break on lower id). Allocation drops from O(N) to O(k), sort cost from O(N·log N) to O(N·log k). Output identical; 5 unit tests pass.
- [ ] ~~`src/tick_pipeline/build_relations.rs` span resolution — defer `embed_span_in_context(...)` until after the span-cache lookup misses; reuse paths get to skip the embed~~ — **plan finding was wrong**: the embed IS used on the reuse path (`fold_streaming_centroid` at `build_relations.rs:281`). Real issue is bigger (BERT forward pass repeats per span). See new Phase 4 entry.
- [ ] ~~`src/tick_pipeline/frame.rs` focused-relations RRF — eliminate the two `Vec` clones~~ — **not worth it**: the clones are on a `Vec<RelationId>` of ~20-50 `u32`s (80-200 bytes each), and replacing with index-based sorting + rank lookup tables adds more code than it saves. Plan agent overstated impact as 2-5 KB/tick. Reading is cleaner as-is.
- [ ] `src/types.rs::RegionStats` — replace `mean: Vec<f32>` / `var: Vec<f32>` with `[f32; EMBEDDING_DIM]`, or move all regions into one flat row-major buffer keyed by region id

## Phase 3 — Clarity / dead code

### Inline thin wrappers
- [ ] `src/tick_pipeline/build_relations.rs::policy_default_conf_or_one` — inline (2-line wrapper, 6 callers)
- [ ] `src/tick_pipeline/build_relations.rs::cosine_unit` — inline into `knn_attribute_name` (single caller) OR document why it deserves a name
- [ ] `src/tick_pipeline/build_relations.rs::confidence_for` — decide: keep with docstring explaining when it's not the right path, or inline. 7 callers, one expression.
- [ ] `src/tick_pipeline/build_relations.rs::push_referenced` — rename `push_referenced_unique` (or `push_dedup`) so the dedup is visible at the call site

### Function decomposition
- [ ] `src/tick_pipeline/build_relations.rs::build_relations` (180 lines) — extract `apply_coref_phase`, `build_pattern_relations`, `build_novelty_relations`, `emit_source_meta_relations`
- [ ] `src/tick_pipeline/frame.rs::assemble_frame` (150 lines) — extract `gather_supporting_claims_and_history`
- [ ] `src/tick_pipeline/frame.rs` next-actions loop — extract `process_uncertainty_signals(&[UncertaintySignal]) -> Vec<AttentionAction>`

### Cleanup
- [ ] `examples/verify_seed_yaml.rs`, `score_input.rs`, `score_per_prototype.rs` — "Throwaway — toss before prod" comments. Either delete the comment (they're functional) or move under `examples/diagnostic/`
- [ ] `src/tick_pipeline/build_relations.rs::mint_novelty_relation` vs `mint_pattern_relation` — consolidate into one parameterized helper OR accept the duplication with a comment

## Phase 4 — Larger moves (pick when you want to)

Each one is its own PR. Listed roughly by impact.

- [ ] **MemoryStats hot/cold split** — `src/types.rs`. Split into `MemoryStatsHot` (activation, salience, focus_success_count, support_count) inline on Element/Relation + `MemoryStatsCold` parallel array indexed by id. Touches persistence format + every step that reads stats. Best cache-locality win in the codebase.
- [ ] **Defensive ID accessor sweep** — methodically classify every `hypergraph.relations[rid.0 as usize]` / `hypergraph.elements[eid.0 as usize]` in `hebbian.rs`, `coref.rs`, `route_regions.rs`, `void_filter.rs`, `frame.rs`, `build_relations.rs`. Keep direct indexing where invariant holds (with a `// invariant: ...` comment); switch to `.get()` elsewhere and propagate via `UncertaintySignal` or drop.
- [ ] **RegionDelta no-copy prototype updates** — `src/tick_pipeline/route_regions.rs:189` + `RegionDelta` shape + `apply_region_delta`. Gate the allocation behind `policy.hebbian_rate > 0` OR change `RegionDelta.prototype_updates` to carry a shared `Arc<[f32]>` / index. v0 default makes this a pure no-op cost today.
- [ ] **Atomic-save recovery** — `src/persistence.rs:224-227`. On `fs::rename` failure, mark the in-RAM substrate as "save failed since tick N"; surface in next `Status` response. Sweep stale `.tmp` files at daemon startup.
- [ ] **`ConsciousAttentionFrame` lazy denormalization** — `src/types.rs` + `assemble_frame`. Return relation IDs + a borrowing `Resolver` keyed to the Hypergraph; resolution becomes lazy. Public API change. ~150 String allocations per tick reclaimed.
- [ ] **Once-per-tick BERT sequence cache** — `embed_span_in_context` (`src/embed.rs:124`) calls `embed_sequence_with_offsets(text)` internally, which is a full BERT forward pass over the entire input. Each span resolution repeats this — a typical tick runs the forward pass 5-10x over the same text. Fix: split into `embed_sequence_with_offsets(text)` + `embed_span_with_offsets(&sequence, &offsets, start, end)`; thread the precomputed `(sequence, offsets)` through `build_relations` and `coref` (~5 call sites across 3 files). Significant wall-clock win — eliminates the dominant per-span cost.
