# Getting started — deploy Legend in a project

Legend is long-term memory for LLM coding sessions: a deduplicated, revisable knowledge
graph your agent recalls at the start of a session and saves durable facts into. It runs
as a single native binary + an MCP server + Claude Code / Codex hooks.

## 1. Install (Linux / WSL)

```sh
git clone <legend-repo> && cd legend
make install                 # builds + installs to ~/.local (binary + embedding model)
```

`make install` puts `legend` in `~/.local/bin` and the embedding model beside it
(`~/.local/bin/models/bge-small-en-v1.5/`), which the binary **self-locates** — no
environment variable, no hardcoded paths. Ensure `~/.local/bin` is on your `PATH`:

```sh
legend --help >/dev/null 2>&1 || echo 'add ~/.local/bin to PATH'
```

Custom prefix: `make install PREFIX=/opt/legend`. Override the model location with
`LEGEND_EMBED_DIR` if you keep it elsewhere.

> **WSL:** use the Linux install above — WSL runs the Linux binary unchanged.
> **Native Windows:** not yet supported (a port is in progress — see
> `docs/production-roadmap.md`). Use WSL on Windows for now.

## 2. Set up a project

From the project root:

```sh
legend init
```

This creates, beside the project, everything a session needs:
- `.legend/` — the store + `journal.jsonl` (the append-only, replayable record of every
  save/recall; **commit it** — it's your analytics stream and the audit trail)
- `.mcp.json` — registers the `legend` MCP server (tools `legend_recall`, `legend_save`,
  `legend_audit`) for Claude Code
- `.claude/settings.json` — three hooks: **SessionStart** (inject an orientation packet),
  **UserPromptSubmit** (rate-limited ambient recall), **Stop** (save reminder)
- `AGENTS.md` — the same recall-first / save-durable-facts guidance for Codex (which has
  no session hooks)

Commit `.legend/journal.jsonl`, `.mcp.json`, `.claude/settings.json`, and `AGENTS.md`.
The rest of `.legend/` (snapshot, vectors) is derived and reconstructible by replay.

## 3. First session — onboard

On the first real session, have the agent deep-ingest the project (docs, module tree,
recent history) so the store starts with real structure rather than empty. Recall
before you save; reuse canonical names; prefer few precise elements and durable facts.
(A generic onboarding recipe is being finalized — for now, point the agent at the repo's
docs and key modules and let it save the durable structure.)

## 4. Use it

Sessions use it automatically via the hooks (orientation at start, ambient recall on
prompts) and deliberately via the MCP tools:
- **recall first** — `legend_recall` to find the canonical name of anything that exists
- **save durable facts** — `legend_save`; to change a value use `changes` (supersedes +
  keeps history), `retract` to correct, `merge` to fold a duplicate
- Best saves are what code can't hold: decisions with reasons, negative results, next
  levers, cross-session measurements.

## 5. Analytics

Every invocation appends one line to `.legend/journal.jsonl` (verbatim payload,
build-stamped). To read a project's health at any time:

```sh
python3 harness/round_report.py            # activity, pollution rates, retrieval
                                           # quality (ambient replay), invariants
python3 harness/replay_journal.py .legend  # determinism: replay == live snapshot
```

Across multiple projects/machines, the journals are git-committed, so gather them (clone
/ pull the project repos) and run the cross-project aggregator (in progress — see
`docs/production-roadmap.md`, W4) for a rollup + comparison.

## Upgrading

The binary is version-pinned on purpose: the journal stamps the build sha per line.
Rebuild + `make install` to upgrade a machine; keep all deployments on the **same build**
so cross-project analytics stay comparable. Determinism replay stays byte-identical
within a build; a fix-bearing upgrade diverges at the boundary by design.
