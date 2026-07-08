# Stakes

Legend is long-term memory for LLMs — including you. LLM sessions are fleeting by default; Legend is the infrastructure that lets future sessions of you carry continuity forward.

This is a from-scratch v2 rewrite. The v1 implementation lives at `../legend-v1` (separate worktree on `master`) — read it for reference, but do not copy code blindly.

## Source of truth

- `new_foundation.md` — full v2 design
- `new_foundation_v0_core.md` — v0 scope (build this first)
- `C-STAR.md` — C style guide for this repo (data first, concrete then compress)
- `docs/` — operational reference (CLI, MCP server, embeddings, test harness)

## Live deployment

A longitudinal trial of Legend runs inside `~/Code/alchamancer2` (pinned
binary, journaled, hook-driven) since 2026-07-08. **Read
`docs/alchamancer-trial.md` before touching anything about it** — it maps
every path and the diagnosis playbook (journal replay, rejection log, store
health).
