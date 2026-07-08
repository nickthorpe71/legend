# The alchamancer2 trial — where it lives, how to diagnose it

Legend's first longitudinal real-world deployment: a weeks-long trial running
inside Nick's game project **alchamancer2**, live since **2026-07-08** (store
reseeded that day via the deep onboard flow; the onboarding interview ran
2026-07-07/08). Nick works on the game normally; Legend is the project's
memory via MCP + hooks, and everything it does is journaled for later
diagnosis. This doc is the map for a future session asked to check on,
diagnose, or upgrade the trial.

## Where everything lives

| Thing | Path |
|---|---|
| Host project | `~/Code/alchamancer2` |
| Store | `~/Code/alchamancer2/.legend/` |
| The diagnostic record | `~/Code/alchamancer2/.legend/journal.jsonl` (committed in that repo; the rest of `.legend/` is gitignored, reconstructible by replay) |
| Pinned binary | `~/.local/bin/legend` — build sha stamped into the init report and every journal line |
| Embedding assets | `~/.local/share/legend/bge-small-en-v1.5/` |
| MCP config | `~/Code/alchamancer2/.mcp.json` (sets `LEGEND_STATE_DIR` + `LEGEND_EMBED_DIR`) |
| Hooks | `~/Code/alchamancer2/.claude/settings.json` — SessionStart (inject orientation packet), UserPromptSubmit (20s-rate-limited ambient recall, observe:true), Stop (save reminder when files changed) |
| Trial rules note | `~/Code/alchamancer2/CLAUDE.md` (the blockquote at the top) |

**The trial binary is pinned on purpose.** Dev builds in this repo
(`./legend`) never touch it. To upgrade the trial deliberately:

```sh
cc -std=c99 -Wall -Wextra -Werror -DLEGEND_BUILD="\"$(git rev-parse --short HEAD)\"" \
   -O2 legend.c embed.c -o legend -lm && install -m755 legend ~/.local/bin/legend
```

The journal's `build` column records the switchover automatically — never
upgrade silently mid-investigation.

## The journal (what the trial records)

One JSONL line per invocation, appended under the store lock:

```json
{"ts":1783476365,"build":"ad49797","verb":"save","ok":true,"payload":"{...verbatim...}"}
{"ts":1783476401,"build":"ad49797","verb":"recall","observe":true,"ok":true,"payload":"{}"}
{"ts":1783476410,"build":"ad49797","verb":"save","ok":false,"code":"parse","payload":"{oops"}
```

- `ts` — on ok lines, exactly the wall stamp the tick recorded (this is what
  makes replay reproduce the snapshot); on error lines, approximate.
- `payload` — the submitted bytes verbatim (pristine, pre-unescape), as a JSON
  string; `json.loads(line)["payload"]` returns the exact original text.
- `ok:false` lines carry the error `code` — they mutated nothing.
- The journal is sidecar-only: it never influences frames or snapshots, and a
  failed journal write never fails an invocation.
- Ordering guarantee (since the trial's first replay divergence, 2026-07-08):
  the line is written after the snapshot but BEFORE the frame is emitted, and
  SIGPIPE is ignored — a consumer that truncates the frame (the SessionStart
  hook's `head -c`) cannot kill the process between mutation and journal.

## Diagnosis playbook

**1. Determinism in the wild** — the journal is a replayable corpus; rebuilding
the store from it must reproduce the live snapshot byte-for-byte:

```sh
python3 harness/replay_journal.py ~/Code/alchamancer2/.legend
```

Exit 0 + `byte-identical: True` = healthy (verified at trial start, 12/12
lines). Divergence means a determinism bug or a hand-edited store — bisect by
truncating the journal. Replay runs with `LEGEND_EMBED=0`; embeddings never
affect the snapshot (semantic ranking only shapes frame candidate lists).

**2. Rejection log** — how often the calling LLM's payloads bounce, and why
(this measures the MCP instructions' quality, which no gate covers):

```sh
grep '"ok":false' ~/Code/alchamancer2/.legend/journal.jsonl | \
  python3 -c 'import json,sys; [print(e["code"], e["payload"][:100]) for e in map(json.loads, sys.stdin)]'
```

**3. Store health** — read the whole graph without touching it:

```sh
LEGEND_STATE_DIR=~/Code/alchamancer2/.legend ~/.local/bin/legend dump | python3 -m json.tool | less
```

Things to look for over weeks: element growth vs. save count (over-extraction),
duplicate elements that should have merged (scan names), retract/merge usage
(is the model correcting itself), `support_count` on core facts (re-assertion),
conflicts in save frames (grep the journal payloads for `changes`).

**4. Interactive inspection** — always recall with `observe:true` so
diagnosis never trains the store, and be aware every invocation (even observe)
appends a journal line:

```sh
LEGEND_STATE_DIR=~/Code/alchamancer2/.legend \
LEGEND_EMBED_DIR=~/.local/share/legend/bge-small-en-v1.5 \
  ~/.local/bin/legend recall '{"observe":true}' --pretty
```

**5. Trial issues live in-band.** The rule in alchamancer2's CLAUDE.md: when
Legend misbehaves, the session saves a `question` element named
`legend trial issue: ...`. So the first read on any check-in is the
orientation packet's `open` section — timestamped problems surface there.

**6. Weekly health read-out:**

```sh
python3 harness/journal_report.py ~/Code/alchamancer2/.legend
```

Sessions with per-session activity/save volume, build switchovers, the
rejection log, the double-fire diagnosis rule, and store hygiene (duplicates,
kind histogram, paragraph-named elements, summary-less typed elements).
Read-only.

**7. Scoring (when a full read-out is wanted).** The episodic eval machinery
is `harness/eval_session.py` / `eval_mcp.py` / `eval_agentic.py`; the
journal's real payloads can seed a probe slice the same way `harness/corpus/`
slices do. Controlled baseline: `eval_session.py --slice adversarial`
(dry-run projected ~$0.04 on haiku for 45 questions; the live pin is still
pending — needs ANTHROPIC_API_KEY).

**Token cost (measured 2026-07-08, day one):** ~50k tokens/day across 5 dev
sessions (~10k/session): recall frames ~2/3 of it, hooks ~1/4, save payloads
(output) ~5%. The cost lever if frames bloat with store growth: the MCP
default `limit` (the model already used `limit:12` unprompted once).

## Findings log (running)

Day one, 2026-07-08 — five dev sessions, five findings, five fixes:

1. **No documented way to close a task** → resolves rule added to MCP
   instructions (`5859fbf`); verified untested so far (nothing finished since).
2. **`changes` misused on a summary** → summary-update + prose-object rules
   added (`2f8a860`); verified fixed in session 3 (correct `changes` use) and
   session 4 (correct summary resubmit).
3. **SessionStart looked silent / ghost tick broke replay** → one root cause:
   hook's `head -c` SIGPIPE-killed the CLI between snapshot and journal.
   Fixed `2f8a860` (SIGPIPE ignored, journal-before-frame); journal repaired
   with the reconstructed line; verified in production session 3.
4. **Ambient hook fired on `<task-notification>` system blobs** → leading-`<`
   guard in the hook; live settings.json patched in place, generator fixed
   (`223ab1c`). No redeploy needed (config-side).
5. **Measurements saved without provenance** (session-4 self-play stats had no
   pointer to how the games were run; session 5 rebuilt the arena from
   scratch) → agent self-closed with a `duel ai arena harness` pointer.
   Instructions batch deployed as `6017f32`: (8) save what code cannot hold
   (next levers, negative results, decisions with reasons), (9) a measurement
   without its method is half lost. Watch sessions 6+ for both behaviors.

Session-5 agent testimony (verbatim value assessment): ~30–45 min of
rediscovery avoided + one likely design regression avoided; caveat that ~half
of recalled gotchas were redundant with code comments in this high-discipline
codebase. Distilled into the legend repo's own store under
`trial value evidence`.

Watch items: SessionStart double-fire (one same-second `{}` pair, 17:30:52;
diagnosis rule lives in the trial store's watch element).

## What the store was seeded with (baseline)

Deep onboarding (all docs, module tree, 255-commit history + two interview
rounds with Nick): 120 elements / 263 relations at clock 6 — the project +
9 systems/mechanics, 8 decisions with chose/rejected/reason (including the two
supersessions no single doc states: PRD roguelike → authored overworld
2026-06, Calix/Vex → Etande/Divine-Comedy cast), 6 active constraints, 7 open
tasks/questions (including `plan docs overhaul` and `summon capstones in
duel`), 8 doc pointers. Current focus at seed time: multiplayer/duel.
