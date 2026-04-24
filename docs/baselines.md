# Legend Performance Baselines

Reference numbers for build, test, and runtime latencies. Future changes
are measured against these to catch regressions and confirm wins.

Each baseline records the environment alongside the numbers — a baseline
without its environment is noise.

---

## Reference environment

| Field    | Value                                         |
|----------|-----------------------------------------------|
| Host     | Linux 6.18.2-arch2-1 (Arch Linux)             |
| CPU      | Intel Core i7-1365U (13th Gen), 12 threads    |
| Memory   | 32 GB                                         |
| rustc    | 1.92.0 (ded5c06cf 2025-12-08) — Arch package  |
| cargo    | 1.92.0 (344c4567c 2025-10-21) — Arch package  |
| sccache  | not installed; `RUSTC_WRAPPER` unset          |
| cargo profile flags | repo defaults (no overrides)       |

The i7-1365U is a mobile/ultrabook CPU and throttles under sustained
all-core load. This shows up in successive cold builds — later runs are
slower than earlier runs even though nothing changed. Interpret numbers
accordingly (see notes per baseline).

---

## #02 — `cargo build --release` (cold)

**Recorded:** 2026-04-23
**Commit:** `660376f` (master)
**Method:** `cargo clean && time cargo build --release`, repeated 3×,
bash builtin `time` with `TIMEFORMAT='real=%R user=%U sys=%S'`.
Clean target wiped ~7.7 GB each run.

| Run | Wall (real) | User      | Sys    | Parallelism (user+sys / real) |
|-----|-------------|-----------|--------|-------------------------------|
| 1   | 207.08 s    | 1895.73 s | 31.28 s | 9.30×                        |
| 2   | 261.75 s    | 2340.73 s | 49.97 s | 9.14×                        |
| 3   | 290.38 s    | 2548.66 s | 53.77 s | 8.97×                        |

**Summary:**
- Min: **207 s (3m 27s)**
- Median: **262 s (4m 22s)**
- Max: **290 s (4m 50s)**
- Range: **83 s** (~40% over min)
- All 3 runs: 0 warnings, 0 errors, `Finished release profile` emitted.

**Why the spread:** parallelism ratio drops 9.30× → 8.97× across runs —
that's thermal throttling on the i7-1365U, not randomness. The first
cold build after an idle period is the fastest; sustained rebuilds
slow as the package hits thermal limits.

**How to compare against this baseline in future:**
- Compare like-for-like: use the *first* cold build after a cooldown
  (≥5 min idle, CPU fan audibly settled), not runs 2/3 of a batch.
- Or: average runs 2 and 3 after discarding run 1 as a warmup —
  either convention is fine, but pick one and stick with it.
- A regression should be visible above the ~40 s noise floor. Anything
  smaller probably isn't real; re-run before acting.
- If the environment above changes (rustc upgrade, new dependency, Arch
  package rebuild), re-record the baseline before comparing.

---

## #03 — `cargo test --release` (1 cold + 2 warm)

**Recorded:** 2026-04-23
**Commit:** `8b2ea36` (master)
**Method:** `cargo clean && time cargo test --release` (run 1, cold — compile + run),
then `time cargo test --release` twice (runs 2 and 3, warm — binaries already built,
tests only). Same `TIMEFORMAT='real=%R user=%U sys=%S'` as #02. Raw log:
`.perf/cargo-test-release-2026-04-23.log` (git-ignored).

| Run | Type | Wall (real) | User     | Sys   | Parallelism (user+sys / real) |
|-----|------|-------------|----------|-------|-------------------------------|
| 1   | cold | 10,614.4 s  | 18,077.6 s | 73.8 s | 1.71×                       |
| 2   | warm | 5,918.4 s   |  6,350.2 s |  9.1 s | 1.07×                       |
| 3   | warm | 5,897.7 s   |  6,417.0 s |  8.3 s | 1.09×                       |

**Summary:**
- Warm test runtime: **~98 min** (median 5,918 s).
- Cold overhead (compile all 14 test binaries in release): **~78 min**
  (run 1 − run 2).
- Warm-to-warm variance: **21 s** — <0.4% of median. Test runtime itself is
  extremely stable; the dominant variance source is compile (in cold) and
  is absent from warm comparisons.

**Per-binary wall time (warm run 2), ranked:**

| Tests | Binary (inferred)                 | Wall    | % of warm run |
|-------|-----------------------------------|---------|---------------|
| 442   | unit tests (lib.rs)               | 2,858 s | 48.3%         |
| 505   | unit tests (main.rs)              | 2,799 s | 47.3%         |
|  17   | conformance_memory_commands       |    32 s |  0.5%         |
|  13   | conformance_keywords              |    54 s |  0.9%         |
|  10   | conformance_init / merge_driver   |    21–23 s each | ~0.8% |
|   7   | conformance_error_paths           |    21 s |  0.4%         |
|   6   | conformance_mcp                   |    21 s |  0.4%         |
|   3   | conformance_dev                   |    29 s |  0.5%         |
|   2   | conformance_discover              |    20 s |  0.3%         |
|   3   | conformance_memory_workflows      |    41 s | ⚠ FAILED — see below |

> **⚠ One consistent failure across all three runs:**
> `conformance_memory_workflows::start_query_context_and_dump_preserve_core_workflow_behavior`
> fails with an `insta` snapshot mismatch. The captured snapshot has 10
> memories in the `memories` array; the current `memory query`
> legitimately returns 1. This is stale-snapshot fallout from commit
> `faa2fca` (split retrieval into ReadOnly vs RecallStudy modes) — same
> class of issue as the one fixed in commit `660376f`, but this binary
> never ran during debug-mode testing because `conformance_memory_commands`
> failed first and cargo stopped there. Fix: update the inline snapshot to
> the new 1-memory shape. Intentionally deferred to the post-optimization
> pass so we don't need to re-run the 1.6-hour suite twice.
> Because cargo stops at the first failing test binary,
> `conformance_cli`, `conformance_recovery`, and
> `observability_pre_phase_2` **never ran** in any of the three measured
> runs. Their times are excluded from the baseline. Expect their addition
> to the warm number to be small (< 2 min combined based on debug-mode
> data) but this should be re-measured after the snapshot fix.

**Headline interpretation:**
- The two unit-test binaries (442 + 505 tests) together account for **~95%
  of warm wall time** — everything else is rounding error.
- `user / real = 1.07×` during warm runs on a **12-core CPU** proves the
  suite is running effectively single-threaded. The `SENTENCE_MODEL` mutex
  at `src/memory/entorhinal.rs:25` is the near-certain cause: every
  `embed_text` serializes through one lock, so of the 12 test threads
  rustc launches, only one at a time does useful work whenever an
  embedding is needed.
- Cold compile dwarfs the per-run runtime numbers from #02 (which only
  compiled the main binary): here we compile **14 release-profile
  binaries** (2 unit + 12 integration), explaining the 78-min gap.

**Optimization brief (for the follow-up item; see `docs/test-speed-optimization.md`).**
Do not re-run this suite to measure regressions until optimization lands.
Post-optimization, re-record this baseline in a new section (do not
overwrite) so the delta is visible.

---

## #03a — Post-optimization baseline + tiering (Phases 0-2)

**Recorded:** 2026-04-24
**Commit:** `dfce35d` (Phase 2: mutex drop + RwLock embedding cache)
**Method:** `cargo test --release --lib -- --test-threads=12` and
`cargo test --release --bins -- --test-threads=12` measured separately
(first cold-ish run after compile); partial run of full `cargo test --release`
then killed. Same `TIMEFORMAT` and rustc as #03.

### Per-command wall time

| Command | Tests | Wall | User+Sys | Parallelism |
|---------|-------|------|----------|-------------|
| `cargo test --release --lib` (first run)  | 443 | **145.70 s** |   421.88 s | 2.89× |
| `cargo test --release --bins` (first run) | 506 | **137.69 s** | 1,691.31 s | 12.29× |
| (same, later in same session — thermal throttled) | 443 / 506 | 165 s / 254 s | — | — |
| 3-module embedding subset (amygdala, anterior_pfc, dentate_gyrus) | 55 | **71.66 s** | 111.24 s | 1.54× |
| 3-module embedding subset (#03 baseline projection) | 55 | ~358 s estimated | — | ~1× |

### Delta vs #03

| Metric | #03 baseline | #03a Phase 2 | Speedup |
|--------|--------------|--------------|---------|
| `--lib` (443 tests) | 2,858 s | **145.70 s** | **19.6×** |
| `--bins` (506 tests) | 2,799 s | **137.69 s** | **20.3×** |
| Full suite (projected) | 5,918 s | **~780 s (~13 min)** | **~7.5×** |

Projected full suite is a conservative sum: lib (165 s thermal) + bins (254 s thermal) + integrations observed during the interrupted run (332 s across 7 of 11 binaries, remaining 4 not measured — estimate another ~80 s). The number tracks ≈ 800 s = 13 min.

### Tiering convention (new)

A single `cargo test --release` run no longer fits a tight dev loop. The following two-tier convention is now the norm:

- **Fast tier** — `cargo test --release --lib --bins` — unit tests only, ~4.7 min. Run on every meaningful change.
- **Full tier** — `cargo test --release` — unit tests + integration (`conformance_*`) + recovery + MCP. ~13 min. Run before merging, on release candidates, or when touching `tests/common/`, `tests/conformance_*`, `src/tool/persistence.rs`, or `src/commands/mcp.rs`.
- **Harness tier** — `cargo test --release -- --include-ignored` — also runs the observability benchmarks (`observability_pre_phase_2`, marked `#[ignore]`). For periodic harness gating.

The fast tier intentionally omits the ~260 s of conformance tests that dominate the integration cost, because they each spawn the `legend` binary as a subprocess and each subprocess pays the ONNX model parse/optimize cost fresh. The only way to collapse this without giving up subprocess isolation is a major `Harness` rewrite to invoke command handlers in-process; deferred until it's worth the effort.

### What changed in Phases 0–2

- **Phase 0** (commit `4932f67`): updated stale `insta` snapshot in `tests/conformance_memory_workflows.rs:64` — it was failing every run and stopping cargo before 3 other integration binaries could execute.
- **Phase 1** (commit `64a4aa8`): dropped `Mutex<SentenceModel>` at `src/memory/entorhinal.rs:25`. Upstream methods (`tract_core::plan::SimplePlan::run(&self)` and `tokenizers::Tokenizer::encode(&self)`) both take `&self`; the mutex was serializing all concurrent embed calls for no correctness reason. Added a compile-time `Send + Sync` assertion so future tract/tokenizer regressions fail the build.
- **Phase 2** (commit `dfce35d`): added a process-local `RwLock<HashMap<(String, usize), Arc<Vec<f32>>>>` memoization layer. ~48 unique strings feed ~1,400 runtime calls per test suite; hit rate is high enough that embedding CPU work collapses by an order of magnitude.

### Experiments tried and rejected

| Attempt | Result | Why it lost |
|---------|--------|-------------|
| Thread-local cache (`RefCell<HashMap>` per thread) | `--lib` **171 s** (+25 s vs Phase 2), user+sys **6.6×** higher | Each thread re-computed every unique embedding; the shared RwLock version was genuinely sharing populated entries despite write contention. |
| `main.rs → use legend::{memory, tool}` refactor to dedupe tests | Full suite **784 s** (+~240 s vs projection) | 506 tests moved from `--bins` into `--lib` ran ~2.6× slower in the unified binary for codegen / scheduler reasons I did not fully diagnose. The two-binary layout's separate statics apparently help somehow. |
| `cargo-nextest run --release` (default profile) | **386 s** | Nextest's default is one process per test, so every test paid ONNX model init (~1-2 s each × 1,028 tests). Isolating tests defeats our caching. |

### Stopping rule verdict

Target was `< 180 s`. Achieved `~780 s` full / `~284 s` fast-tier. Target not met; further cuts are architectural (Harness in-process rewrite, model swap) and warrant their own queue item. Phases 0-2 commit the biggest, safest wins; tiering absorbs the remaining gap for day-to-day development.

---

## #04 — Cold single-tick latency

**Recorded:** 2026-04-24
**Commit:** `f64df61` (master)
**Method:** 10 samples of `time legend memory tick "<input>"` against a
clone of the working memory state at `/tmp/legend_bench/.legend/`. Master
state restored before each sample so every run starts from the same
~2.4 MB `memory.lz4` / ~11k-entry starting point. Bash builtin `time`
with `TIMEFORMAT='real=%R user=%U sys=%S'`. Raw log:
`.perf/tick-cold-2026-04-24.log` (git-ignored).

**Only cold is measured.** The Legend CLI always spawns a fresh process,
so there is no "warm tick" path to measure — every tick in normal usage
is a cold tick. An in-process warm benchmark would measure a different
workload (library use) than the actual CLI dev loop.

**Input (117 chars):**
> `DECISION: Chose Redis for caching because pub/sub support is native and battle-tested for realtime notifications`

### Per-sample timings

| Sample | Wall (real) | User    | Sys     |
|--------|-------------|---------|---------|
| 1      | 6.595 s     | 6.472 s | 0.099 s |
| 2      | 6.467 s     | 6.355 s | 0.091 s |
| 3      | 6.284 s     | 6.162 s | 0.100 s |
| 4      | 6.255 s     | 6.140 s | 0.094 s |
| 5      | 6.406 s     | 6.287 s | 0.097 s |
| 6      | 6.507 s     | 6.374 s | 0.111 s |
| 7      | 6.199 s     | 6.088 s | 0.092 s |
| 8      | 6.180 s     | 6.052 s | 0.104 s |
| 9      | 6.498 s     | 6.371 s | 0.103 s |
| 10     | 6.506 s     | 6.388 s | 0.094 s |

### Summary

- **Min:** 6.180 s
- **Median (p50):** 6.436 s
- **Mean:** 6.390 s
- **Max:** 6.595 s
- **Stdev:** 0.148 s
- **Range:** 0.415 s (~6.7% over min)
- **Parallelism ratio (user+sys / real):** ~1.00× — single-threaded end-to-end.

### Interpretation

- A single tick from fresh process to final disk write takes **~6.4 s median** on this hardware against a realistic ~2.4 MB / ~11 k-entry state. That's the cost of *every* `legend memory tick` invocation today.
- Zero parallelism: the tick path is serial from binary load through ONNX init, state load, embedding, encoding, and save. Item #16 (profile tick latency by subsystem) will decompose this into its components; item #18 (fast encoding path + deferred queues) is the obvious consumer of that decomposition.
- Variance is low (6.7%), bounded by thermal and load noise on the host. The baseline is tight enough that optimizations targeting ≥ 200 ms savings should be visible without re-measuring many times.
- This baseline is state-dependent: the ~2.4 MB `memory.lz4` drives a significant fraction of startup cost through LZ4 decompression, MessagePack deserialization, and migration checks. If we revisit this after consolidation cuts state size, expect the number to move.

### How to compare future runs

Restore the master clone before each sample:
```bash
rm -rf /tmp/legend_bench/.legend
cp -r /tmp/legend_bench_master/.legend /tmp/legend_bench/
{ time legend memory tick "<same input>" ; }
```
Or regenerate a master from current `.legend/` if state has materially changed. Note the memory.lz4 size + entry count alongside the new numbers so the comparison is apples-to-apples.

---

## #04a — Post-daemon + ort tick latency

**Recorded:** 2026-04-24
**Commit:** `735e8b9` (master)
**Method:** Two complementary measurements against the same
`/tmp/legend_bench_master` state as #04 (~2.4 MB / ~11k entries):

1. **Cold daemon per sample** — kill daemon, restore master state,
   start daemon, wait 1.5 s for readiness, time *one* tick. Matches
   #04's "fresh process per sample" methodology as closely as the
   daemon architecture allows. This is the first-tick-of-session
   latency a user experiences after `memory start` fires.
2. **Warm daemon across a session** — single daemon, 10 consecutive
   ticks without restoring state. Represents the within-session dev
   loop after warm-up.

Raw log: `.perf/tick-daemon-warm-2026-04-24.log` (git-ignored).

**Input (same 117-char string as #04):**
> `DECISION: Chose Redis for caching because pub/sub support is native and battle-tested for realtime notifications`

### Cold daemon per sample (matches #04 methodology)

| Sample | Wall (real) |
|--------|-------------|
| 1 | 599 ms |
| 2 | 759 ms |
| 3 | 667 ms |
| 4 | 692 ms |
| 5 | 787 ms |
| 6 | 802 ms |
| 7 | 659 ms |
| 8 | 733 ms |
| 9 | 690 ms |
| 10 | 704 ms |

- **Min:** 599 ms
- **Median:** 698 ms
- **Mean:** 709 ms
- **Max:** 802 ms
- **Stdev:** 62 ms
- **Range:** 203 ms (≈ 34 % over min)

This includes: IPC connect (~1 ms) + daemon lazy state load (~500 ms
for 2.4 MB LZ4 → MessagePack → migration check) + ORT session init
triggered on first embed (~170 ms) + tick work (~30 ms).

### Warm daemon (10 ticks, single session, state grows)

| Sample | Wall (real) |
|--------|-------------|
| 1 | 508 ms |
| 2 | 246 ms |
| 3 | 292 ms |
| 4 | 654 ms |
| 5 | 54 ms |
| 6 | 57 ms |
| 7 | 180 ms |
| 8 | 69 ms |
| 9 | 57 ms |
| 10 | 182 ms |

- **Min:** 54 ms
- **Median:** 181 ms
- **Mean:** 230 ms
- **Max:** 654 ms
- **Stdev:** 206 ms
- **Samples 5–10 only (post-warm-up):** median **63 ms**

Variance is high because a tick on an 11k-entry graph can touch very
different code paths depending on which entries it resembles. A tick
whose embedding is a cache hit + doesn't trigger wholesale graph
decay lands at ~50–70 ms. A tick that triggers decay normalization +
graph spreading can climb to several hundred ms. The bimodal pattern
shows up across all observed sessions.

### Delta vs #04

| Metric | #04 (pre-daemon, tract) | #04a cold (daemon + ort) | #04a warm (samples 5-10) | Speedup |
|--------|-------------------------|--------------------------|--------------------------|---------|
| Min    | 6,180 ms | 599 ms | 54 ms | 10.3× / **114×** |
| Median | 6,436 ms | 698 ms | 63 ms | 9.2×  / **102×** |
| Max    | 6,595 ms | 802 ms | 654 ms | 8.2×  / 10.1× |
| Stdev  | 148 ms | 62 ms | — | — |

**Headline:** warm-session tick dropped from **~6.4 s → ~63 ms**
(> 100× faster). Cold first-tick-of-session is **~700 ms** — still
10× faster than the pre-daemon baseline, and the only sample users
see that's above 100 ms.

### What changed between #04 and #04a

All captured in commits between `f64df61` (pre-daemon baseline) and
`735e8b9` (Phase 5 ship):

1. **`89b72ee` Phase 1** — daemon IPC scaffolding.
2. **`4ae2517` + `d524e33` + `2cb7a78` Phase 2** — 11 CLI commands
   route through the daemon; fallback on NotImplemented /
   VersionMismatch.
3. **`bec6616` Phase 3b** — write-ahead log replaces full save on
   every mutation; checkpoint on Consolidate/Reset/shutdown.
4. **`e54037e`** — `tract-onnx` → `ort` (ONNX Runtime). Per-call
   inference: ~2 s → ~18 ms. Single biggest impact on absolute
   numbers.
5. **`91ff9fb` Phase 4** — `mcp-serve` also routes through the
   daemon (no state split across CLI and MCP).
6. **`735e8b9` Phase 5** — `SessionEnd` hook auto-stops the daemon
   on `/exit` for clean WAL/snapshot handoff between sessions.

### Caveats

- **State-size dependent.** These numbers are from an 11k-entry,
  2.4 MB state. A fresh project (empty state) would show lower cold
  times (no state load) but similar warm numbers.
- **Thermal variance.** i7-1365U laptop CPU. Sustained load pushes
  thermal throttling; the variance on warm samples partly reflects
  this.
- **The "slow tick" tail is real.** ~15–20 % of warm ticks cross
  100 ms on this state, mostly during decay-heavy runs. Item #22
  (profile tick latency by subsystem) is where we'd hunt down the
  exact path.
- **ORT binary dep.** `cargo install` now downloads libonnxruntime
  for the target at build time via `ort`'s `download-binaries`
  feature. Prebuilt distributions need the library shipped alongside
  the `legend` binary.

### How to compare future runs

```bash
# Cold per-sample (matches #04 methodology exactly)
for i in $(seq 1 10); do
  pkill -f 'legend daemon start'
  rm -rf /tmp/legend_bench/.legend
  cp -r /tmp/legend_bench_master/.legend /tmp/legend_bench/
  export LEGEND_SOCKET=/tmp/legend_bench.sock
  rm -f "$LEGEND_SOCKET"
  (cd /tmp/legend_bench && legend daemon start &) ; sleep 1.5
  (cd /tmp/legend_bench && { time legend memory tick "<same input>" ; })
  legend daemon stop
done
```

## #06 — Tick subsystem profile (sources of the 54–654 ms tail in #04a)

**Recorded:** 2026-04-24
**Commit:** `1badd15` (master)
**Method:** new `examples/tick_profile.rs` — builds with `--features instrument`,
initializes the `memory::trace` channel, runs 10 ticks against the
master-clone state at `/tmp/legend_bench/.legend/`, drains trace events
into per-step deltas. Pipeline steps are defined in
`src/memory/trace.rs::PipelineStep`; instrumentation call-sites live in
`src/memory/mod.rs` and the per-subsystem modules.

Raw log: `.perf/tick-profile-2026-04-24.log` (git-ignored).

### Raw wall-time per sample

| Sample | Wall (ms) | Note |
|--------|-----------|------|
| 0      | 858       | cold (first call; ORT session already in daemon is irrelevant here — `tick_profile` is in-process, so this sample pays full ORT init) |
| 1      | 549       | fires UpdateTermFrequencies |
| 2      | 532       | fires UpdateTermFrequencies |
| 3      | 1 431     | fires both UpdateTermFrequencies + ReplayConsolidation |
| 4      | 71        | baseline — no bulk update fires |
| 5      | 69        | baseline |
| 6      | 258       | partial — decay elevated, no bulk update |
| 7      | 81        | baseline |
| 8      | 78        | baseline |
| 9      | 234       | partial |

### The tail is NOT embedding cost

That was the earlier guess. Instead it's **two conditional operations** that
fire on a subset of ticks:

- **`UpdateTermFrequencies`: ~516 ms per fire** — rebuilds the global
  term-frequency tables used for salience and keyword scoring. Scales with
  the vocabulary / graph size (11 k nodes here). Fires when… (TBD, looks
  like it's based on ticks_since_rebuild or similar pressure counter).
- **Auto-consolidation: ~775 ms total when it fires**, broken down:
  - `ReplayConsolidation`: 670 ms
  - `SystemsConsolidation`: 40 ms
  - `CreateOrMergeSummaryNode`: 30 ms
  - `SemanticTopicExtraction`: 33 ms
  - `ClusterGroups`: 13 ms
  - `SummarizeGroup`: 2 ms
  - `MarkConsolidated`: <1 ms

These fire based on accumulated `ticks_since_consolidation` pressure
(`should_suggest_consolidation` + implicit trigger) inside
`tick_impl`. With ~11 k graph nodes, consolidation replay reads and
rewrites large chunks of the graph.

### Baseline per-tick cost (when bulk updates DON'T fire)

From the 4 "baseline" samples (69, 71, 78, 81 ms) and the per-step mean over
all warm samples, the irreducible per-tick work is:

| Step                     | Typical (µs) | Max (µs) | Notes |
|--------------------------|--------------|----------|-------|
| Decay                    | 6 000–17 000 | 32 000   | every tick; scales with graph edges |
| ChunkText                | 24 000       | 27 000   | **surprisingly high — small text should chunk in < 1 ms** |
| MergeEntryHigh           | 800–1 300    | —        | when an existing L2 entry is a near-match |
| Renormalize              | 222–862      | —        | |
| CpebTagging              | 365–570      | —        | |
| FindBestMatch            | 170–680      | —        | |
| ComputeEmotionalValence  | 80–200       | —        | |
| ComputeSalience          | 85–125       | —        | |
| ExtractMemoryRefs        | 30–60        | —        | |
| EmbedText                | 2–7          | —        | cache hit for repeated samples |

Sum of the baseline work is ~50–80 ms per tick on this ~11 k-node state,
dominated by decay (fixed cost scaling with graph size) and ChunkText
(investigate separately).

### Implications for #16 and #17

- **#16 — latency budgets:** separate steady-state from periodic. A
  reasonable split:
  - Normal tick:        ≤ 100 ms
  - Tick that runs term-freq rebuild:   ≤ 300 ms (if kept synchronous)
  - Tick that runs auto-consolidate:    ≤ 50 ms *sync* + deferred background
  - Explicit `memory consolidate`:      no budget (big operation by design)
- **#17 — fast + deferred split:** strong case for it. Auto-consolidation
  and term-frequency rebuilds are exactly the kind of wholesale updates
  that belong on a background queue. The tick's *sync* response can
  acknowledge the mutation and return; the heavy work runs behind the
  scenes. If we do this, #16's warm-tick budget of ≤ 100 ms is hit
  deterministically.
- **ChunkText at 24 ms** for short text is suspicious — `chunk_text` in
  `src/memory/entorhinal.rs` may be doing more work than needed. Small
  separate investigation.

### How to re-run

```bash
rm -rf /tmp/legend_bench/.legend
cp -r /tmp/legend_bench_master/.legend /tmp/legend_bench/
cargo run --release --features instrument --example tick_profile \
  > .perf/tick-profile-$(date +%F).log 2>&1
```

---

## #05 — `legend memory start` startup latency

**Recorded:** 2026-04-24
**Commit:** `16a5116` (master)
**Method:** Same state (`/tmp/legend_bench_master/.legend/` ~2.4 MB /
~11k entries) and same `time` framing as #04 / #04a. Two variants:

1. **Cold per sample** — kill daemon, restore master state, start
   daemon, wait 1.5 s for readiness, `time legend memory start`.
   Matches the "fresh session" path a user hits via Claude Code's
   SessionStart hook at the start of every new session.
2. **Warm** — preserve state across samples, run 10 consecutive
   invocations. Tests whether OS page cache / shared state reuse
   reduces latency.

Raw log: `.perf/memory-start-2026-04-24.log` (git-ignored).

### Per-sample timings

| Sample | Cold (ms) | Warm (ms) |
|--------|-----------|-----------|
| 1  | 288 | 265 |
| 2  | 268 | 266 |
| 3  | 288 | 276 |
| 4  | 264 | 272 |
| 5  | 275 | 252 |
| 6  | 274 | 238 |
| 7  | 275 | 225 |
| 8  | 270 | 207 |
| 9  | 263 | 267 |
| 10 | 278 | 263 |

### Summary

| Metric | Cold | Warm |
|--------|------|------|
| Min    | 263 ms | 207 ms |
| Median | 274 ms | 264 ms |
| Mean   | 274 ms | 253 ms |
| Max    | 288 ms | 276 ms |
| Stdev  | 8.7 ms | 22.7 ms |

**Warm-up effect is small** (~10–20 ms). Most of the cost is in
Rust binary startup + dynamic linking (including libonnxruntime, even
though `memory start` never invokes it) + LZ4 decompression +
MessagePack deserialization of the state file. None of these benefit
much from the OS page cache.

### Surprising finding: `memory start` doesn't use the daemon

`memory start` is deferred from Phase 2 (item #36 in the queue) —
it executes in-process every time, loading state fresh via
`load_or_default()` and saving after the L1→L2 flush. The "cold /
warm daemon" distinction doesn't matter for this command today
because the daemon isn't on the path. The numbers above are the
in-process path.

### Interpretation

- **Sub-300 ms is probably fine for the SessionStart hook.** Claude
  Code spawns the hook once per session; 274 ms is not user-visible
  friction.
- **If we wanted to drop this further**, the two biggest levers are
  (a) daemonize the command so state load is amortized (ports the
  same win we got on tick) and (b) stop linking libonnxruntime for
  the CLI binary path when the daemon handles all embedding-needing
  commands. Both are architecture-level, tracked by item #36.
- **No urgency.** The command is called once per session. Even a
  doubling to 500 ms would be acceptable for its role.

### Delta vs #04 (`memory tick`)

Worth noting because the two commands share a lot of machinery:
- `memory tick` cold (pre-daemon, #04): 6,436 ms median
- `memory start` cold (today): 274 ms median — **24× faster than a
  `tick` on the pre-daemon architecture,** because `memory start`
  doesn't call `embed_text` (the 2 s/call ORT inference, now 18 ms
  with ort, was the tick killer).

### How to compare future runs

```bash
# Cold per sample (same methodology as #04a cold)
for i in $(seq 1 10); do
  pkill -f 'legend daemon start' 2>/dev/null
  rm -rf /tmp/legend_bench/.legend
  cp -r /tmp/legend_bench_master/.legend /tmp/legend_bench/
  export LEGEND_SOCKET=/tmp/legend_bench_start.sock
  rm -f "$LEGEND_SOCKET"
  (cd /tmp/legend_bench && legend daemon start &) ; sleep 1.5
  (cd /tmp/legend_bench && { time legend memory start > /dev/null ; })
  legend daemon stop
done
```

