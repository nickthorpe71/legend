# Frame as the observable surface

Proposal: `ConsciousAttentionFrame` is the entire observable result of
a tick. Every consumer — the CLI, benches, anything that comes later
— reads only the frame, never the `Hypergraph` behind it. The tick
pipeline lives in library code (`src/lib.rs`, `src/steps/`,
`src/render.rs`); the daemon and the CLI are thin entry points that
call into it. Status: design sketch, not implemented.

## The principle

`ConsciousAttentionFrame` is the **entire observable surface** from a
tick. The substrate (`Hypergraph`) is internal state owned by whichever
process is running the library at the moment; it is not part of the
contract any consumer sees. Anything a consumer needs from a tick has
to be on the frame.

This is stricter than what holds today:

- The CLI looks behind the frame — it loads `.legend/memory.lz4`
  after each tick to resolve `ElementId`/`RelationId` into human
  names.
- Benches in `examples/` look behind the frame — they call
  `persistence::load` and walk the `Hypergraph` directly to score.

Both have to stop. The frame must be **self-contained** (every ID
ships with its resolved name + relation metadata inline) and
**complete enough** (any signal a downstream scorer needs appears as
a frame field, not as a substrate lookup).

## Today's shape, briefly

- `src/lib.rs` exposes the tick pipeline. `src/steps/` is the Step
  1–12 chain. `src/render.rs` renders snapshots and the flat-frame
  view.
- `src/daemon.rs` is a long-lived host: TCP-loopback socket, port
  file, `fs2` flock, MessagePack framing. Holds substrate in RAM,
  calls into the lib to run ticks.
- `src/main.rs` is the CLI. Sends `Tick` to the daemon by default;
  the `LEGEND_INPROC=1` path skips the daemon and calls the lib
  directly. Used by both humans and LLM callers that shell out per
  tick.

What's missing for the principle to hold:

1. The frame is not denormalized — consumers need a `Hypergraph` to
   make sense of it.
2. The CLI's render path requires substrate access.
3. The wire protocol wraps the frame in `DaemonResponse::TickResult
   { frame, elements, relations }` rather than shipping the frame
   directly.
4. Benches read substrate state to score, treating the frame as one
   signal among many.

## Why now

Measured on a warm release build, an end-to-end tick is ~220ms:

| Stage | Cost |
|---|---|
| Rust binary launch (per CLI invocation) | ~30–50ms |
| Socket connect + framed Tick request | a few ms |
| Daemon tick (NER dominates: ~95ms) | ~125–150ms |
| LZ4 disk reload on CLI response side | ~10–50ms |
| Render + print | a few ms |

Half of every warm tick is per-CLI-process overhead the daemon can't
amortize. That overhead exists *because* the principle doesn't hold —
the CLI needs its own copy of the substrate. Fix the principle and
that cost disappears.

It also matters at scale: any caller running ticks back-to-back
(batch evaluator, LLM session shelling out per turn), and substrates
that grow past a few MB where LZ4 decode crosses ~100ms.

## Proposal

Three coordinated changes, ordered by dependency:

### 1. Denormalize `ConsciousAttentionFrame`

The frame must be renderable + scorable in isolation. For every
`ElementId` referenced (in `active_frame`, in attribute lists,
anywhere), the canonical name ships inline. For every `RelationId`
referenced, the relation's resolved attributes + status + confidence
ship inline.

Two shapes worth comparing at implementation time:

- **Parallel name arrays** — keep IDs, add a `names: HashMap<...>`
  or parallel `Vec<String>` to each field. Smaller diff to the frame
  type.
- **`Resolved*` structs** — replace `ElementId` with `ResolvedElement
  { id, name }`, replace `RelationId` with `ResolvedRelation { id,
  attributes: Vec<(String, ResolvedTerm)>, status, confidence, … }`
  in frame fields. Cleaner type contract; bigger refactor.

Lean Resolved structs unless they bloat the frame in a way that bites
serialization. The denormalization happens once during frame assembly
(Step 12); it's library work, not consumer work.

### 2. `ConsciousAttentionFrame` is the daemon's tick response

`DaemonResponse::TickResult` goes away. The tick response variant is
`DaemonResponse::Frame(Box<ConsciousAttentionFrame>)`. The
`elements` / `relations` substrate-size counts drop — they're
substrate-scoped, not frame-scoped. A consumer who wants them queries
`Status`.

### 3. Render is a pure function over `&ConsciousAttentionFrame`

The CLI's `print_tick_summary`, `print_frame_contents`,
`print_flat_frame`, and `print_relation_line` move from `src/lib.rs`
into `src/render.rs` as functions that return `String` and take only
`&ConsciousAttentionFrame`. No `Hypergraph` parameter. They are
library code, called by anything that wants rendered text.

The CLI's `tick_via_daemon` collapses to: connect, send Tick, read
`Frame`, `print!("{}", render_frame(&frame))`, exit. No
`persistence::load`. No `Hypergraph` import.

## Scope: where logic lives, who reads disk

- **Library code (`src/lib.rs`, `src/steps/`, `src/render.rs`)
  owns tick + render.** The daemon is one host for it; the CLI's
  `LEGEND_INPROC=1` path is another; an in-process bench is a third.
  All three call the same functions.
- **The daemon is the sole writer of `.legend/memory.lz4` during
  normal operation.** Ticks go through it; the substrate mutates
  in one place. The `fs2` flock prevents two daemons from racing.
- **Anyone can read `.legend/memory.lz4` for inspection.** Dev tools
  (`examples/dump_hypergraph_md`), the `LEGEND_INPROC` debug path —
  these read post-hoc and don't race meaningfully with the daemon.
- **Benches stop reading the substrate.** Today they call
  `persistence::load` and walk the `Hypergraph` to score. After this
  proposal they invoke the tick pipeline (in-process is fine), read
  the returned frame, score against frame fields only. If a bench
  needs a signal that isn't on the frame, that signal moves onto
  the frame — the frame is the contract.
- **Git merge driver is the explicit second writer.** When git
  invokes `legend git-merge-driver %O %A %B`, the writes go to a
  git-managed temp path (`%A`). Git is in charge of when that file
  becomes `.legend/memory.lz4`. The daemon should be stopped during
  a merge; that's a user contract, not a lock-protected invariant.

## Phased plan

1. **Denormalize `ConsciousAttentionFrame`.** Inline names + relation
   metadata in frame fields. Step 12 (`assemble_frame`) gains the
   denormalization pass. Consumer types start working without a
   `Hypergraph`. This is the foundation; everything else depends on
   it.
2. **Move `print_*` into `src/render.rs` as frame-only functions.**
   Pure refactor: lift the bodies, drop the `Hypergraph` parameter,
   keep the existing call sites working. After this step the CLI
   still does `persistence::load` — we just no longer need it.
3. **Drop `DaemonResponse::TickResult`; ship `Frame` directly. CLI
   becomes a relay.** Update the wire enum, the daemon's handler,
   `tick_via_daemon`. Lose `persistence::load` from the CLI's tick
   path. Lose the `elements` / `relations` fields from the wire.
4. **Audit benches.** Replace `persistence::load` + `Hypergraph`
   reads with frame-only scoring. Where the frame is missing a
   signal a bench needs, add it to the frame in step 1's shape.

Steps 1–3 are blocking-sequential. Step 4 can land in parallel with
step 3 once the frame is self-contained.

## Cross-cutting concerns

- **Test surface.** Conformance tests that exec `legend <text>` and
  match on stdout shouldn't care about the rearrangement — same
  text, different code path. Worth a once-over.
- **Bench correctness after step 4.** If a current bench reads
  substrate state that isn't observable from the frame, switching it
  cold would change the bench's signal. Audit each bench and either
  (a) confirm the frame already exposes what it reads, or (b) add
  the missing field. Don't quietly drop signal.

## Open questions

- **Denormalization shape.** Parallel names vs `Resolved*` structs
  (above, §1). Decide at implementation time based on the
  serialization size + ergonomics trade.
- **Frame fields that benches need but the frame doesn't expose.**
  This is the step-4 audit output, not a design question — but
  worth noting that step 1 might grow if the audit surfaces gaps.

## Prior art

- The daemon docstring (`src/daemon.rs:1-26`) already calls out the
  per-invocation reload cost — this proposal acts on that
  observation rather than introducing the diagnosis.

## Out of scope

- An MCP stdio server for LLM consumers. Worth doing later — drives
  Legend over the same `Frame` contract, so the denormalized frame
  here is the right shape for that future work too. Not part of this
  proposal.
- Multi-client concurrency on the daemon (still single-threaded).
- Remote (non-loopback) connections — TCP loopback + port file +
  flock is local-only by intent.
