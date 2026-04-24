# Legend Daemon Durability Policy

## TL;DR

The daemon prioritizes **latency** over **last-few-mutations durability**.
On an unclean shutdown (kernel panic, SIGKILL, power loss) the daemon may
lose up to **~100 ms of mutations** — at most a few ticks on a busy
session, usually zero. On a clean shutdown (`legend daemon stop`,
SessionEnd hook, SIGTERM) nothing is lost.

Ticks that have returned a successful response to the CLI are durable to
OS buffer cache, not necessarily to disk.

## The decision

When the daemon + WAL design was committed (Phase 3b of
`/home/nickthorpe71/.claude/plans/we-need-to-make-hidden-hickey.md`), we
had a pure tradeoff to resolve:

- **Safety**: fsync the WAL after every append. Tick wall latency lands
  at ~150–300 ms (fsync is the dominant cost on a busy SSD, more on HDD).
  Every successful tick is durable across any crash.
- **Latency**: buffer WAL writes to OS cache; have a background thread
  fsync on a timer (e.g. every 100 ms) or when the WAL buffer grows past
  a threshold. Tick wall latency lands at **<100 ms**. A hard crash can
  lose up to one fsync-interval of mutations.

The user explicitly chose latency. Legend's primary workload is an LLM
session ticking 5–20 times per minute while a developer works; losing a
single tick on a power loss is a recoverable nuisance, not a data
integrity problem. Sub-100 ms feedback on every tick, in contrast, is
load-bearing for the product feel — the whole daemon exists to hit that
number. Optimizing for the happy path is the right call.

## What "durable" means in practice

| Event | What survives |
|-------|---------------|
| `legend daemon stop` (explicit) | Everything. Final fsync before exit. |
| Claude Code SessionEnd hook fires | Everything. Same path as `stop`. |
| `kill -TERM` (graceful) | Everything. SIGTERM handler does final fsync. |
| `kill -9` (hard) | Entries up to the last periodic fsync. |
| Kernel panic / power loss | Entries up to the last periodic fsync. |
| User command returns "ok" and daemon then crashes | May be lost. |

The last case is the one users need to understand. When a `legend memory
tick` returns a successful `entry_id`, the state change is committed to
the in-memory `MemoryState` and written to the WAL buffer in the kernel
page cache. It is durable to anything short of a power loss. On a clean
shutdown it lands on disk within ~100 ms of return.

## Periodic fsync details

- **Timer interval**: 100 ms. Tuned to be short enough that visible data
  loss is effectively zero, long enough that the fsync cost is amortized
  across many mutations (on a ticking-heavy session, each fsync covers
  multiple tick entries at once).
- **Buffer-size trigger**: also fsync eagerly if the pending WAL bytes
  exceed 64 KB. This caps worst-case memory sitting in the page cache
  and keeps OS flushes predictable.
- **Implementation**: a single background thread in the daemon holds a
  handle to the WAL file and sleeps on a Mutex/Condvar. Mutation path
  does not block on fsync; it only holds the file write lock long
  enough to `write` (not `sync`).
- **Checkpoint path**: still forces fsync before renaming the new
  snapshot over `memory.lz4`. The snapshot is the "strong durability"
  anchor; WAL is the "recent mutations" overlay.
- **Shutdown path**: SIGTERM / explicit `Shutdown` command triggers a
  final fsync before the daemon exits. `Drop` on the WAL writer also
  tries a best-effort fsync.

## What gets replayed on restart

1. Load snapshot (`memory.lz4`) — this is the strong-durability anchor.
2. Replay WAL entries on top of the snapshot, in order, via the same
   mutation handlers the live path uses (deterministic).
3. Validate each entry's XXHash trailer. On mismatch, truncate WAL at
   the last good boundary and log a warning; surface via `legend daemon
   status` so operators can see it happened.
4. Take a fresh checkpoint (full save + truncate WAL) to consolidate.

Replay is idempotent because the mutation functions (`tool::tick`,
`memory::consolidate`, `basal_ganglia::reinforce`, `set_task`,
`clear_task`, `reset_memory`) are deterministic given `(state, args)`.
Decay is clock-driven; clock advances per tick so replay reproduces the
same curve.

## Why not "choose per command"

One option we considered was flagging some commands durable
(`consolidate`, `reset`) and others eventual (`tick`). We rejected this
because:
1. Every mutation ends up in the same WAL; the fsync timer doesn't know
   about command types.
2. Adding a "force-flush" parameter to specific commands adds complexity
   for a tiny gain: `consolidate` is rare and its output naturally
   triggers a snapshot checkpoint anyway; `reset` is even rarer.
3. Simplicity wins here.

If a specific workflow turns out to need strict durability (e.g.
automated test harnesses that expect a crashed process to have already
persisted), we can add `LEGEND_DAEMON_FSYNC_MODE=sync` as an env-var
escape hatch.

## How to change this policy

1. Edit `src/tool/wal.rs` — the `FSYNC_INTERVAL_MS` and
   `FSYNC_BUFFER_BYTES` constants at the top.
2. If switching to per-append fsync, just call `sync_data()` inside
   `append()` and delete the background thread.
3. Update `docs/baselines.md` §#04a entry to note the new mode so
   historical comparisons stay apples-to-apples.
4. Update this doc's TL;DR.

## Failure-mode summary for reference

- Rare (hard crash with unflushed WAL): lose up to one fsync interval
  of ticks (normally 0, worst case ~100 ms of activity ≈ 1-2 ticks on a
  very busy session).
- Common (clean shutdown): lose nothing.
- Always recoverable (corrupt WAL entry at tail): truncate to last good
  boundary, continue. Previous snapshot + earlier WAL entries remain
  valid.
- Not recoverable (corrupt snapshot itself): `memory.lz4` is already
  backed up to `memory.lz4.corrupt` on any load failure today
  (`src/tool/persistence.rs:34-38`), and replay would start from a
  fresh default state. This pre-existed WAL.
