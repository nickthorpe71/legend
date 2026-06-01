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
- [x] `src/types.rs::RegionStats` — replaced `mean: Vec<f32>` / `var: Vec<f32>` with `[f32; EMBEDDING_DIM]` (384). `RegionStats` lives only in `Hypergraph.region_stats`, which is `#[serde(skip)]` and rebuilt from `region_prototypes` on load — so the >32 array length never hits serde; dropped the now-dead `Serialize/Deserialize` derives. Updated `seed.rs` + `route_regions.rs` test constructors. `mahalanobis_similarity` unchanged (`.len()`/indexing identical on arrays).

## Phase 3 — Clarity / dead code

### Inline thin wrappers
- [x] `src/tick_pipeline/build_relations.rs::policy_default_conf_or_one` — inlined to `hypergraph.policy.default_conf` at all 4 call sites (bound to a local for borrow-cleanliness); kept the mint-time-confidence rationale as a comment
- [x] `src/tick_pipeline/build_relations.rs::cosine_unit` — inlined into `knn_attribute_name` as `crate::math::dot(...)` (unit-vector path, matching `pick_best_by_cosine`); fn removed
- [x] `src/tick_pipeline/build_relations.rs::confidence_for` — **kept** with docstring: 7 callers, one spec'd clamped expression; a named fn reads clearer than repeating the clamp 7×
- [x] `src/tick_pipeline/build_relations.rs::push_referenced` — renamed `push_referenced_unique` at all 5 references; dedup now visible at the call site

### Function decomposition
- [x] `src/tick_pipeline/build_relations.rs::build_relations` — extracted `apply_coref_phase`, `build_pattern_relations`, `build_novelty_relations`, `emit_source_meta_relations`; body now reads as a phase sequence (accumulator + span_cache thread by `&mut`; §7 pack-shape early-return preserved)
- [x] `src/tick_pipeline/frame.rs::assemble_frame` — extracted `gather_supporting_claims_and_history(&Hypergraph, &[RelationActivation]) -> (Vec<RelationId>, Vec<RelationId>)`
- [x] `src/tick_pipeline/frame.rs` next-actions loop — extracted `process_uncertainty_signals(&[UncertaintySignal]) -> Vec<AttentionAction>`

### Cleanup
- [x] `examples/verify_seed_yaml.rs`, `score_input.rs`, `score_per_prototype.rs` — deleted the "throwaway / toss before prod" comments; examples kept in place and functional (not moved)
- [x] `src/tick_pipeline/build_relations.rs::mint_novelty_relation` vs `mint_pattern_relation` — consolidated into one parameterized `mint_candidate_relation` (they differed only in status/confidence, already carried by `RelationCandidate`)
- [x] comment→code pass — conservative: removed only doubled-name rename artifacts (6 edits across 4 files); preserved every comment encoding a non-obvious why/invariant/perf rationale/citation

## Phase 4 — Larger moves (pick when you want to)

Each one is its own PR. Listed roughly by impact.

- [ ] **MemoryStats hot/cold split** — **not done / reverted.** The headline "best cache-locality win" is mechanically ineffective on the actual access patterns: the only sequential element scan reads no stats and is embedding-bandwidth-bound, while the stats-touching loops are random-access-by-id. The split also manufactured a hand-maintained parallel-array invariant plus a per-tick guard, violating the no-belt-and-suspenders rule. Unmeasured; reverted pending a benchmark proving real gain.
- [ ] **Defensive ID accessor sweep** — **incomplete.** Added `// invariant:` comments to a subset of `hypergraph.relations[rid.0]` / `hypergraph.elements[eid.0]` sites across `hebbian.rs`, `coref.rs`, `route_regions.rs`, `void_filter.rs`, `frame.rs`, `build_relations.rs`. Added ZERO `.get()` conversions — the intended "provable → comment; uncertain → `.get()`" dichotomy never materialized. Coverage is only ~1/3 of sites.
- [x] **RegionDelta no-copy prototype updates** — gated the prototype-update allocation behind `policy.hebbian_rate > 0` so the v0 default (rate 0) pays nothing; behavior for rate>0 unchanged.
- [x] **Atomic-save recovery** — daemon now latches `save_failed_since_tick` on `fs::rename` failure (still propagates the error), clears it on the next successful save, and surfaces it as a WARNING in `Status`. Added `sweep_orphan_tmp_files` at daemon startup (after the exclusive flock — single-writer invariant makes every matching temp an orphan). Two new unit tests.
- [ ] **`ConsciousAttentionFrame` lazy denormalization** — `src/types.rs` + `assemble_frame`. **BOUNCED → dedicated design-doc PR.** A borrowing `Resolver<'h>` forces a lifetime onto the frame, which breaks its `Serialize/Deserialize` derive and the "frame is the entire observable surface" contract; blast radius is 100+ destructuring sites across `render.rs` (4 pure-frame fns), three examples, and `v0_acceptance.rs` — far past a clean refactor. Needs the sketch→research→doc→sign-off flow.
- [x] **Once-per-tick BERT sequence cache** — split `embed_span_in_context` into `embed_sequence_with_offsets(text)` (one forward pass) + `embed_span_with_offsets(&sequence, &offsets, start, end)` (cheap pool); threaded the precomputed `(sequence, offsets)` through `build_relations` and `coref`. Span-embedding output is bit-identical (caching refactor only); forward pass now runs once per tick instead of 5-10×.
