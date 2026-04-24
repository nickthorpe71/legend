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

## #04 — Cold + warm single-tick latency

*Not yet recorded. Queue item #04.*

## #05 — `legend memory start` startup latency

*Not yet recorded. Queue item #05.*
