# Init re-run behavior (#26)

**Recorded:** 2026-04-24

Closes queue item #26: "Decide init re-run behavior on already-
initialized repo." This doc captures the current behavior and the
deliberate non-decision (no `--reset` flag).

## Behavior matrix

| Invocation                          | Memory store      | Workspace keywords | Agent files (CLAUDE.md etc.) | Migrations |
|-------------------------------------|-------------------|--------------------|------------------------------|------------|
| `legend init` (first run)           | created           | seeded fresh       | created                      | n/a        |
| `legend init` (re-run, no flag)     | untouched         | untouched          | refreshed (marker-idempotent)| run        |
| `legend init --discover` (re-run)   | untouched         | additive seeding   | refreshed (marker-idempotent)| run        |
| `rm -rf .legend/ && legend init`    | recreated empty   | seeded fresh       | refreshed (marker-idempotent)| n/a        |

The "agent files" column refreshes via the `<!-- legend-start -->` /
`<!-- legend-end -->` markers in `write_legend_markdown`. Existing
user content outside the markers is preserved verbatim.

## Decision: no `--reset` flag

`legend init --reset` would be a one-line flag that wipes `.legend/`.
We are not adding it. Reasons:

1. **Footgun risk.** A typo on `--reset` (or a stale shell-history
   recall) could destroy weeks of accumulated decisions, plans, and
   graph state. The cost of one accidental wipe outweighs the
   convenience savings of avoiding `rm -rf .legend/`.
2. **No friction signal.** No user has reported needing this; the
   re-init path covers refresh and re-discover already. If demand
   shows up later, this decision is reversible.
3. **`memory reset` already exists** for the in-state case (clears
   the memory but keeps the directory + agent integrations). The
   wipe-and-redo case is different and should stay manual.

## Other rejected options

- **Auto-rebootstrap on every re-init.** Would add false-positive
  keywords (when a project pivots and old terms linger). The current
  `--discover`-gated behavior keeps the user in control.
- **Diff display.** Showing what changed during re-init is useful but
  out of scope; the markers already enforce idempotency, so re-runs
  rarely surprise.
- **Migrations only mode.** Currently `legend init` with no flag
  always runs migrations alongside agent file refresh. Splitting them
  into separate verbs (`legend init --migrations`) would be cleanly
  layered but isn't blocking anything today.

## What changed in this session

Updated the no-op re-run message in `src/commands/init.rs` to
explicitly call out:

- What re-init refreshes (agent integration files, idempotent)
- That `--discover` re-scans the workspace
- That a manual `rm -rf .legend/` is the path to start over
- A pointer to this doc

Changes verified by `tests/conformance_init.rs` (13 tests) and
`conformance_cli` (no help-text snapshot regression).

## Related

- `docs/init-bootstrap-scope.md` (#25): what gets scanned/seeded on
  first init or `--discover` re-run.
- `legend memory reset` (existing): clears state without touching
  agent integrations.
