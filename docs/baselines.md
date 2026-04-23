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

## #03 — `cargo test --release` (cold)

*Not yet recorded. Queue item #03.*

## #04 — Cold + warm single-tick latency

*Not yet recorded. Queue item #04.*

## #05 — `legend memory start` startup latency

*Not yet recorded. Queue item #05.*
