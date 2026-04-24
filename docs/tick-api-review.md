# Tick API review — heredoc ergonomics, return value (#29)

**Recorded:** 2026-04-24

Closes queue item #29: "Review tick API shape — heredoc ergonomics,
return value usefulness". Audits the CLI tick surface and the
returned response.

## Surface today

```
legend memory tick [--blocker | -b] [<text>...]
```

- **Text source order**: positional args joined with spaces; if no
  positional args, read all of stdin.
- **`--blocker` / `-b`**: prepends `BLOCKER:` to the text.
- **Stdout**: a single JSON line. Three shapes:
  - `{"action":"created"|"merged"|"reconsolidated"|"working_memory_only"|"plan_updated", "entry_id": N}` for normal ticks.
  - `{"action":"keyword_only","keywords_registered": N}` when the tick
    only contained keyword directives.
- **Stderr**: `"[keyword registered: kw:<cat>:<term>]"` for each
  registered keyword directive (informational).

## Heredoc ergonomics

### What works

- `legend memory tick <<'EOF' ... EOF` — common pattern; CLAUDE.md
  documents this as the canonical shape for multi-line ticks.
- `legend memory tick "one-line decision"` — fine for a single
  sentence on the command line.
- `echo "..." | legend memory tick` — pipe input works because
  stdin is read when positional is empty.

### Friction

- **Multi-line on the command line is awkward.** Without a heredoc,
  you can't include `\n`. Workarounds (`printf "...\n..." | legend
  memory tick`) are clunky.
- **No `--file <path>`.** Reading prepared text from a file requires
  `cat path | legend memory tick`. Shell-friendly but easy to forget.
- **No `--text "..."` flag.** Some users prefer flag-based input
  for clarity in scripts. Today positional is the only flag-style
  way to pass inline text.
- **Quote-escaping.** Backticks inside quoted strings trigger shell
  command substitution; users have hit this writing commit messages
  with backticks. The heredoc form avoids this if the heredoc tag
  is single-quoted.

## Return value usefulness

The current response is a JSON line. For human consumption, this is
fine — short, scannable, the action verb is the primary signal.

For machine consumption it has rough edges:

- **`entry_id` is an internal handle.** It's not stable across
  consolidation. Scripts that store it and try to use it later may
  get a 404. There's no public surface that takes `entry_id` as
  input.
- **`action` strings are ad hoc.** The vocabulary
  (`created` / `merged` / `reconsolidated` / `working_memory_only` /
  `plan_updated` / `keyword_only`) is not enumerated anywhere user-
  visible. Scripts have to learn it by experiment.
- **No human-readable mode.** Even `--quiet` would help scripts
  that just want exit codes to mean "the tick succeeded."

## Recommendations

### Worth doing

1. **`--file <path>`**. Read text from a file. Single line of code,
   high payoff for CI / batch-import use cases.
2. **`--quiet`**. Suppress stdout entirely. Common request shape.
3. **Document the `action` enum** in the CLI help and CLAUDE.md.
   Lock it with a conformance test that exercises each action.
4. **Drop `entry_id` from the CLI shape OR rename to
   `entry_id_at_tick_time`** to flag its non-stability. The current
   field encourages misuse.

### Worth considering

- **`--text "..."` flag** as an alias for positional. Pure ergonomics;
  small UX win at the cost of one more flag in `--help`.
- **`--prefix DECISION|BUG|...`** as an alternative to `--blocker`,
  unifying the structured-prefix injection paths. Wait for the #28
  follow-up that adds salience-prefix awareness before committing.

### Not recommended

- **JSON-only output forever.** Currently the human and machine
  surfaces are the same. Splitting via `--json`/`--human` adds
  conditional logic without clear demand.
- **Returning the full `TickResult` from CLI.** MCP already does this
  via the structured payload; CLI keeps the compact JSON for
  scriptability. Mirroring MCP would bloat output.

## Decision for this audit

- Document the audit (this file).
- No code changes in this commit. The recommendations cluster into
  two future queue items:
  - "Add `--file` and `--quiet` flags to `legend memory tick`."
  - "Lock the action enum with a conformance test + CLI help text."

## Related

- `src/commands/memory/tick.rs` — current handler.
- `docs/tick-prefix-contract.md` (#28): structured prefix contract.
  This audit deliberately doesn't propose new prefix-injection flags
  until that contract evolves.
- `src/commands/mcp.rs::format_mcp_tick`: the richer response shape
  used by MCP — kept separate from CLI for good reasons.
- `tests/conformance_memory_commands.rs::tick_returns_entry_id`:
  the only test that locks any part of the current CLI shape.
