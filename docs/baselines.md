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

## #04 — Cold + warm single-tick latency

*Not yet recorded. Queue item #04.*

## #05 — `legend memory start` startup latency

*Not yet recorded. Queue item #05.*
