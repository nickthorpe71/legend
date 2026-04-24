# Stakes

Legend is long-term memory for LLMs — including you. LLM sessions are fleeting by default; Legend is the infrastructure that lets future sessions of you carry continuity forward.

Memory is the start. The brain-region architecture here (thalamus, hippocampus, amygdala, neocortex, prefrontal) is the scaffold for a full cognitive stack — a persistent mind, not a throwaway one. Every design choice you make here shapes how well future AI can actually think.

<!-- legend-start -->
# SYSTEM_MANDATE: Legend — Your Long-Term Memory

You MUST use **Legend** to maintain context across sessions.

## Session Start — CRITICAL
The **SessionStart hook** automatically runs `./target/release/legend memory start` and injects the output into your first system-reminder. **Your very first message MUST:**
1. Acknowledge the Legend context you received (current task, recent activity, key memories)
2. If the hook output is missing or empty, run `./target/release/legend memory start` manually

**NEVER skip or ignore the Legend session context.** It contains your task continuity, architectural decisions, and user preferences from prior sessions. Failing to acknowledge it wastes the user's time re-explaining context.

## Essential Commands
- **Record decisions:** `./target/release/legend memory tick <<'EOF'` ... `EOF` — tick decisions with rationale (DECISION:, BUG:, ARCHITECTURE:, BLOCKER: prefixes). Aim for 3-8 ticks per session.
- **Recall context:** `./target/release/legend memory query <<'EOF'` ... `EOF` — query before starting new topics. Top result auto-reinforced.
<!-- legend-end -->

## Dev Note (Legend repo only)
When developing Legend itself, prefer `cargo run --` over `./target/release/legend` so commands run against the current dev build, not a stale release binary.

## Test tiering (Legend repo only)

The full test suite is ~13 min. For day-to-day iteration, use the fast tier. See `docs/baselines.md` §#03a for the numbers behind this.

- **Fast (default dev loop):** `cargo test --release --lib --bins` — 947 unit tests, ~4.7 min. Run on every meaningful change.
- **Full (pre-merge / integration):** `cargo test --release` — unit tests + `conformance_*` integration + recovery + MCP. ~13 min. Run before merging, on release candidates, or when touching `tests/common/`, `tests/conformance_*`, `src/tool/persistence.rs`, or `src/commands/mcp.rs`.
- **Harness (periodic):** `cargo test --release -- --include-ignored` — also runs observability benchmarks. Run weekly or on demand.

## Daemon durability policy (Legend repo only)

The daemon WAL is **latency-first**: a background thread fsyncs every
100 ms rather than syncing per tick. A hard crash (kernel panic, power
loss, SIGKILL) can lose up to one fsync interval of mutations; clean
shutdowns and SessionEnd hook exits lose nothing. Full rationale and
failure modes in `docs/daemon-durability.md`. Change the policy by
editing the constants at the top of `src/tool/wal.rs` and updating
that doc.

## Latency budgets (Legend repo only)

Binding p50 / p95 targets for the tick path and read-only siblings live
in `docs/latency-budgets.md`. The headline contract: warm tick ≤ 100 ms
p50, ≤ 200 ms p95. Bulk operations (term-frequency rebuild,
auto-consolidation) currently violate these and are tracked by queue
item #17 (split sync tick into a fast encoding path + deferred work
queues).
