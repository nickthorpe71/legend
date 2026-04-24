# Latency Budgets

This document sets binding latency targets for the Legend tick path and
its read-only siblings. They reflect the current architecture (single
local user, daemon + ORT in-process embedding, ~10–20 k-node graph) and
should be kept in sync with `docs/baselines.md`.

Origin: queue item #16 (set after #15 profiled the bimodal tail).

## What "budget" means here

A budget is a **p50 / p95 wall-clock target** for the daemon-side
handler measured **inside the daemon** (does not include CLI startup).
Cold first-call budgets exist separately; treat them as bootstrap costs.

A budget is **violated** when p95 exceeds it on the master-clone
benchmark state in `/tmp/legend_bench_master/.legend/` for the
methodology in `docs/baselines.md` §#04 / §#06. Violations should
either:
1. Trigger a fix (the typical case), or
2. Trigger a budget revision with explicit rationale committed to this
   doc (rare; reserved for fundamental architecture shifts).

CLI startup (~6–8 ms on Linux for the bare client path) is excluded
because it is paid by the shell, not the daemon. End-to-end CLI calls
should add roughly 6–10 ms on top of every budget below.

## Budgets

| Path                                | p50      | p95      | Notes |
|-------------------------------------|----------|----------|-------|
| `memory tick` (steady-state)        | ≤ 100 ms | ≤ 200 ms | warm daemon, no bulk op fires |
| `memory tick` (with bulk op)        | ≤ 100 ms | ≤ 200 ms | enforced via deferred queue (#17) |
| `memory tick` (cold first of session) | ≤ 2 s  | ≤ 3 s    | daemon spawn + ORT init |
| `memory query` (read-only)          | ≤ 50 ms  | ≤ 100 ms | retrieval + ranking; no embedding cache miss should make this > 100 ms on its own |
| `memory start`                      | ≤ 150 ms | ≤ 300 ms | summary build over current state |
| `memory consolidate` (explicit)     | unbudgeted | unbudgeted | user-initiated; large operation by design |
| `discover` / `init` per-tick        | ≤ 200 ms | ≤ 400 ms | batch ingestion; bulk ops deferred to end-of-batch |
| `daemon checkpoint`                 | ≤ 500 ms | ≤ 1 s    | full snapshot + WAL truncate |

### Why two tick budgets are the same

`tick (with bulk op)` matches `tick (steady-state)` because **bulk ops
do not run synchronously on the tick path**. #17 shipped two changes
to enforce this:

1. `encoding_activation` (context recall) now skipped on the CLI path
   and computed lazily only on the MCP path (~40–670 ms saved).
2. Auto-consolidation deferred to a background worker thread in the
   daemon (`consolidation_worker` in `src/commands/daemon/server.rs`);
   non-daemon in-process fallbacks drain via
   `drain_deferred_consolidation` after save.

Post-#17 measurements (§#07) land all warm ticks under 100 ms p95 on
the master-clone state.

### Cold-tick clarification

The cold tick budget covers the **first tick of a new daemon process**:
ORT `Session::commit_from_memory` of the embedded all-MiniLM-L6-v2-q
model (~150–300 ms), state `load_or_default()` (~50–150 ms on the
~2.4 MB master state), plus normal tick work. SessionEnd hook + idle
timeout mean this is paid roughly once per Claude Code session.

### What's outside the budget

- LLM-side latency (model thinking time)
- Network I/O (none in current architecture)
- Disk fsync intervals — the WAL fsync runs on a 100 ms timer in the
  background and never blocks the tick handler. See
  `docs/daemon-durability.md`.

## Baseline measurements (anchor points)

From `docs/baselines.md` §#07 (2026-04-24, post-#17, 11 k-node state):

- Baseline tick: **15–21 ms** ✅ well inside budget
- Tick with larger replay batch: **89–94 ms** ✅ inside budget
- Cold first tick (in-process, full ORT init): **~197 ms** ✅ well inside 2 s budget
- Warm median: **20 ms** / mean: **42 ms** / max: **94 ms**

## How to verify

Re-run the §#06 methodology any time these budgets are at risk:

```bash
rm -rf /tmp/legend_bench/.legend
cp -r /tmp/legend_bench_master/.legend /tmp/legend_bench/
cargo run --release --features instrument --example tick_profile \
  > .perf/tick-profile-$(date +%F).log 2>&1
```

10-sample run reports per-step deltas; compare against the table above.
A future tick-budget enforcement test can build on this scaffolding —
deferred until #17 lands so the deferred-queue boundary is known.

## Revision history

- **2026-04-24** — initial budgets set after #15. Baseline ticks pass;
  the two bulk-op categories are knowingly out of budget pending #17.
- **2026-04-24** — #17 shipped: `compute_context` split + background
  consolidation worker. All warm ticks inside budget per §#07.
