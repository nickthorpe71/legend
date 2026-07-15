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

**Byte-identical replay holds within one binary version, NOT across a
fix-bearing upgrade.** A bug fix that changes write semantics makes any journal
op that relied on the old behavior replay differently. Known divergence
(observed 2026-07-15, under BOTH the pre-causal `3bf188a` and causal `2c42c74`
binaries, so not caused by either): replay fails at **line 335 / ts
1784059486**, a `merge` whose `from` is a `source`-phantom element — minted by
the buggy `source`-without-facts path under `fcbb707` (line 334), then merged
away. Post-`ec197d7` (the source-phantom fix) that element is never minted, so
the merge can't find it. This is expected across the mid-trial fixes, not store
corruption: **the live snapshot stays the source of truth** (the warm server and
CLI load it directly; replay is only the diagnosis tool). The causal-vocab
upgrade (`2c42c74`) adds a second, benign cross-binary difference — extended
vocab is *appended* on an old store but *seeded at #32–41* on a fresh replay, so
element ids shift. To restore clean byte-replay one would re-seed the store from
its journal under a single binary; deferred (diagnostic-only, and blocked behind
the source-phantom divergence anyway).

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
pending). The API key goes in the repo-root `.env` (gitignored; the eval
scripts read it automatically when the env var is unset).

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

6. **Resolves fact minted a phantom** (2026-07-10, filed in-band by the agent
   itself): a `resolves` whose object never existed silently minted a bare
   leaf — nothing was closed, and the signal (the target listed in the
   frame's `writes.minted_elements`) went unread. Store repaired (rel
   retracted, decision annotated); instructions now state the target must
   already exist and to check `minted_elements` for it. Meta-note: my own
   first attempt to record this lesson minted a paragraph-named element the
   same way — the class catches careful writers.

Session-5 agent testimony (verbatim value assessment): ~30–45 min of
rediscovery avoided + one likely design regression avoided; caveat that ~half
of recalled gotchas were redundant with code comments in this high-discipline
codebase. Distilled into the legend repo's own store under
`trial value evidence`.

7. **Save-side miss: a decision disguised as a detail** (2026-07-10, caught
   by Nick's audit question "did you use legend after that last prompt?"):
   the agent skipped saving the tier-is-rarity decision because it arrived
   framed as a UI-polish request, reasoning "the plan file already records
   it" — but the plan file records the *what*; the *why* (rarity vocabulary,
   green-not-purple ramp, school-vs-rarity independence) was Legend material.
   Agent self-remediated: saved the decision with the miss noted in its
   provenance, and patched the design doc gap it exposed. First finding in
   the *missing-save* class — unmeasurable by the report, only auditable.
   Instruction candidate (batched): "a choice that settles a design question
   is a decision even when the request looks cosmetic."

Testimony 2 (adventure-mode session, 2026-07-10): recall self-rated ~3/10 —
a cold-start feature gives recall little to bite on — but the orientation
packet was load-bearing (the RL do-not-reintroduce guard shaped behavior);
save value deferred to next session by design. Verified fixed: summary
resubmit reused cleanly (finding 2 regression-free). New friction for the
watch list: ambient ranking separates poorly (~0.6 relevant beside ~0.4
postmortem noise; short prompts topped by in-band trial-diagnostic questions
— the trial's own bookkeeping pollutes game-prompt candidates).

8. **Two competing memory surfaces** (2026-07-12, legend-dev session; Testimony 3,
   saved as element `#48`): a full session of benchmark work went to the colocated
   Claude file-memory (`MEMORY.md`, auto-injected) and NONE to Legend — one
   orientation recall that didn't cover the task, zero saves — leaving the graph
   silently stale. Same "value capped by whoever isn't writing to it" class as an
   outside tool, but self-inflicted. Root cause: this dev repo has no
   `.claude/settings.json`, so none of Legend's hooks fire here, even though
   `legend init`'s `write_hooks_config` already writes all three (SessionStart
   orientation-inject, UserPromptSubmit ambient recall, Stop save-reminder). Fix
   filed as the `install legend hooks in legend dev repo` task (`#51`): run
   `legend init` at the repo root (no `--reset`). Principle: the always-on
   orientation index + a save-nudge (the hooks) are what let Legend win against a
   colocated file-memory.

**Dev-binary batch deployed 2026-07-12** (pending a deliberate trial-binary
upgrade): three instruction lines — finding 7 (missing-save decisions), watch #11
(status-via-changes), watch #1 (predicate-reuse) — into `MCP_INSTRUCTIONS` +
`legend_save`; a `bytes_out` recall-journal field (watch #3). Predicate dedup
(watch #1) resolved as instruction + merge-pass, NOT auto-fold: near-dup
predicates ≥0.6 Jaccard already surface in `near_matches` (the merge-pass signal),
while the real sub-0.6 near-dups (`validates`/`validated by`) can't be auto-folded
without false-merging distinct predicates. Retrieval separation (Testimony 2
friction, watch #8) deferred: the offline corpus already scores separation-perfect
(`retrieval.found`, `exclusion.leaks`, `absent.false_resolutions` all maxed), so a
cosine floor can only risk the pinned gate, not improve it — C1/C2 must be
developed against the live store where the symptom exists.

9. **A "read-only" harness mutated the live store** (2026-07-13, self-inflicted
   while building the C2 measurement). The retrieval-separation harness replayed
   the journal's focuses as recalls with `observe:false`, on the belief that was
   the read-only path. It is the opposite: `observe:false` is a *mutating* tick
   (advances the clock, reinforces, persists); `observe:true` is the read-only
   path — and the only one that fires the C2 ambient penalty, so the harness both
   corrupted the store and measured nothing. Damage: clock 121→774, 649 junk
   journal lines, activations perturbed — but **zero content lost** (recalls
   never mint; every save in the journal was real). Restore
   (`scratchpad/restore_trial_store.py`): drop the `build=="dev"` lines (an exact
   discriminator; each verified a recall), replay the real-only journal to
   rebuild the snapshot, keep `vectors.bin` (real element ids are unchanged since
   no dropped line was a save). Restored to clock 126 / 410 elements, replay
   byte-identical. It **self-healed into the still-live session**: the warm MCP
   server fingerprints the snapshot (size+mtime) and reloads on external change
   (`legend.c` warm-graph gate), so its next call picked up the clean snapshot —
   no clobber. Fix: the harness now runs `observe:true` against an isolated copy
   of the store, never the live one. Lesson for any future measurement — isolate
   to a copy; the live trial store is read-only-by-copy, not read-only-by-flag.

Testimony 4 (alchamancer session, 2026-07-14; **countdown session 1 on
`fcbb707`**; agent-saved in-band as `#422`). Orientation packet + saving
(decisions `#417` multi-school-summons, mechanics, the commit) solid and trusted
— the real payoff. Ambient per-prompt recall low-signal: every turn resolved
`false` with the SAME top-salience candidates regardless of prompt. Two readings,
both true: (a) **C2 landed in the wild** — those candidates were game content
(color-signature summons, cast-fizzle toast, procedural spell drops), NOT the
trial bookkeeping that topped the list pre-upgrade; (b) **the residual is a
different, deeper problem** — on an ~80%-art/animation/build-bug session, lexical
misses and BGE cosines are undifferentiated (~0.5–0.6 for everything), so the
same salient elements surface no matter what was typed. That motivates the next
lever (below). Recurring split: the session's most reusable lessons (the Sora
"prompt motion BIG or it renders static" rule; the asset-arena-OOM segfault
recipe) went to `.claude/` auto-memory, not Legend — procedural/debugging
knowledge is the two-surface boundary (finding 8) the verdict must rule on.

**Next lever — ambient abstention (`#63`).** The fix for "mostly clutter" is
knowing when to say nothing, not better ranking. NOT the deferred cosine floor
(BGE too compressed for an absolute threshold to separate a relevant 0.6 from an
irrelevant 0.6). Leading idea: require a **lexical anchor** — if nothing scores
above ~0.4 trigram, the prompt shares no vocabulary with the store, so suppress
the ambient block instead of offering salience defaults. Would go quiet on
"voodoo in tmp"/the segfault, stay active on "color signature summons." Prototype
+ measure on an isolated copy (observe:true); this is post-verdict unless it
blocks usefulness.

Testimony 5 (alchamancer trap-spell session, 2026-07-14; **countdown session 2
on `fcbb707`**; agent-saved in-band as `#433`). **Strongest positive datapoint
yet.** Resumed from compaction; the orientation packet's DEFERRED/NEXT-LEVERS
list recorded "EFF_BANISH built + priced + labelled but NOT yet rolled — no
teleport-trap yet," so when the user asked for the teleport trap the agent
already knew banish was 95 % wired and which function to touch. That is the "what
code cannot hold" value — code alone can't distinguish a deliberate deferred
lever from dead code. Recall accurate, nothing stale. Set against Testimony 4
(same day, low-signal on an uncovered art/debug domain) it's a clean controlled
contrast: **recall value tracks domain coverage.** Two issues raised, both now
addressed dev-side: (a) the source phantom-mint footgun (finding 10); (b) `#424`
summary bloat → split into children (`#66`, next-batch nudge).

10. **`source` without facts minted a phantom element** (2026-07-13/14, filed
    in-band as `#431`/`#432`; same class as the resolves-phantom `#221`/finding
    6, and it bit this Claude too via short source labels). `source` is
    provenance *for* facts minted in the call; the save path reified it
    unconditionally, so a `source` with no facts (elements-only, or nothing)
    minted a lone element named after the whole provenance sentence, costing a
    merge + a round-trip each time. **Fixed `ec197d7`**: the source is reified
    only once a listed relation exists to attach it to; `source` + facts is
    unchanged, `source` alone is now a clean no-op. check.sh green (the corpus
    only ever passes `source` with facts, so no metric moved). Ships in the next
    trial-binary upgrade.

## Watch list

Checked at every check-in; most are measured by `journal_report.py`. Each has
its trigger and its response so a future session doesn't re-derive them.

1. **Predicate proliferation** — the LLM mints its own relation predicates
   (19 by day two, 15 single-use; `validates`/`validated by` already an
   inverse-pair near-duplicate, `is` too vague). Dedup only catches exact
   names, so a fragmenting vocabulary silently weakens retrieval.
   *Measure:* the report's minted-predicates histogram. *Trigger:* the same
   concept under two names, or steady single-use growth. *Response:* one-line
   instruction nudge ("reuse an existing predicate; prefer short verbs") +
   a merge pass.
2. **SessionStart double-fire** — one same-second `{}` pair seen (07-08
   17:30), cause unresolved (possible client re-fire). *Measure:* the
   report's same-second boot pairs. *Trigger:* recurrence when only one
   session was started. *Response:* investigate the hook matcher.
3. **Frame-size / token-cost growth** — recall frames are ~2/3 of Legend's
   token cost and grow with the store. *Measure:* today only by re-running
   recalls; the `bytes_out` journal field (landed 2026-07-12 in the dev
   binary, on recall lines) makes it free. *Trigger:* orientation packet regularly past the hook's 4000-byte
   cap or deliberate frames past ~20KB. *Response:* lower the MCP default
   `limit`; frame curation.
4. **Resolves discipline** — finished work must close its task via a
   `resolves` fact, not a summary edit. *Measure:* open-list items whose
   summary claims done/RESOLVED. *Trigger:* any recurrence. *Response:*
   instruction wording pass.
5. **Duplicates & paragraph-named elements** — exact-name dups (report),
   plus prose-as-value mints paragraph names (8 as of day two, pre-fix).
   *Trigger:* dup pair surviving a session, or paragraph-name count growing
   after the instructions fix. *Response:* merge/retract repair + wording.
6. **Truth drift** — summaries going stale against the repo (counter-example
   so far: the RL removal was recorded exemplarily, `supersedes` fact and
   all). *Measure:* spot-check 2–3 old elements against the code at
   check-ins. *Response:* repair saves; consider a "verify on recall" nudge.
7. **Write-only store growth** — elements never reached by any focus walk
   (`fsc == 0`; 195/202 at day two — young store, watch the trend not the
   level). *Trigger:* ratio not falling as the store ages. *Response:*
   retrieval tuning, or acceptance (insurance memory has value).
8. **Hook noise ratio** — ambient recalls on prompts where the packet adds
   nothing ("still running"). *Measure:* ambient-vs-deliberate counts per
   session. *Trigger:* ambient ≫ deliberate with no behavioral sign of use.
   *Response:* raise the rate-limit interval / smarter prompt filter.
9. **Compounding vs plateau** — the strategic question. *Measure:* the
   session-5-style meta-question ("how useful was Legend this session?")
   asked every ~5 sessions; attributable wins vs cost. *Response:* decides
   week-two investment (breadth vs retrieval depth).
   Companion ritual (proved out 2026-07-10, finding 7): the **audit
   question** — "did you use legend after that last prompt?" — is the only
   instrument that catches *missing* saves; ask it occasionally right after
   a design-flavored exchange that looked cosmetic.
11. **Stale point-in-time facts** — plain facts used for changing values
    (e.g. `spellgen phase 0 build --status--> "M0+M1+M3a green"` saved
    mid-build, while the element's summary later says COMPLETE through M4).
    `changes` is the right verb for status-like properties; a plain fact
    accretes and goes stale. *Measure:* spot-check status/progress-flavored
    facts against their element summaries at check-ins. *Trigger:*
    contradictions surfacing in recall frames. *Response:* repair with
    `changes`/retract + an instruction line ("status-like values go through
    changes, not facts").
10. **Journal/store integrity** — replay determinism at every check-in
    (`replay_journal.py`; byte-identical at every check so far). *Trigger:*
    any divergence. *Response:* stop, bisect by truncating the journal —
    this one is a hard gate, not a trend.

## Roadmap & go/no-go criterion (locked 2026-07-13)

**Store-health check + wipe decision (2026-07-13).** Before continuing the
trial, the store was checked read-only: `replay_journal.py` reproduced the live
snapshot **byte-identical across all 265 ok lines — spanning five binary builds**
(`ad49797`, `5859fbf`, `2f8a860`, `6017f32`, `1e1d5b8`). 387 elements / 694
relations / clock 121; real domain content (33 decisions, 18 events, 12
mechanics, 8 constraints). Hygiene noise present but non-corrupting: ~15%
paragraph-named elements (watch #5), 66 predicates / 39 single-use (watch #1).
**Decision: do NOT wipe.** Three reasons: (a) the pending dev-binary upgrade
needs no wipe — the journal already replays clean across five builds, and the
batched changes (3 instruction lines + `bytes_out` + Codex init) touch no
snapshot format; (b) the accumulated bookkeeping noise IS the test corpus for
the retrieval-separation fix below — wiping deletes what we're measuring; (c) the
trial's entire value is longitudinal accumulation (`#37`) — a reset zeroes the
hypothesis. The store is journal-backed and committed, so it's never "lost."

**Retrieval separation — build sequence (Plan #2).** Develop against the live
store (the symptom only exists there; the offline corpus already scores
separation-perfect). Order: (1) **baseline measurement, no code — DONE
2026-07-13** (`harness/retrieval_separation.py`, a dedicated read-only script —
NOT folded into `journal_report.py`, whose contract is one `dump`; it replays
every journal `focus` at `observe:false` against the current store with
`LEGEND_EMBED=1` and scores the ranking). Then (2) **C1 margin/centering** in
`tier2_semantic`/`embed_rank_elements`; (3) **C2 bookkeeping down-weight** in
`build_embed_list`/`tier2_backfill`, **ambient recalls only**, kind resolved via
the `instance_of` relation (kind is not a struct field); (4) **dual-validate** —
the live baseline must improve AND `probes_adversarial.json` (`LEGEND_EMBED=1`)
re-pinned by inspecting the diff, better-not-different. C3 optional/last; B2
predicate-dedup starts report-only.

**Baseline result (2026-07-13, 128 ambient focus-sets / 130 terms replayed on
the 387-element store).** Ambient recalls barely resolve (10/130, all lexical) —
120 fall to candidate ranking, where the pollution lives:
- **C2 is severe.** The #1 candidate is trial bookkeeping **78%** of the time;
  META kinds (`event`/`task`/`question`/`pointer`) fill **6.4 of the top-10**;
  the first real game-content element (`system`/`mechanic`/…) doesn't appear
  until **rank ~5.6**. `event` alone dominates (541 top-10 appearances vs 68
  `system` + 55 `mechanic`) — the testimonies and session-marker events bury the
  game graph.
- **C2 is ambient-specific** (confirms watch #8): on the 6 *deliberate*
  canonical-focus recalls, rank-1-is-META drops to **0%** and first-domain-rank
  to 1.7. Precise focus → clean; short/conversational focus → buried.
- **C1 design correction.** A fixed cosine *floor* is the wrong tool: only ~1%
  of candidates sit below 0.5. BGE cosines compress into a narrow 0.5–0.7 band
  (197 @0.5, 631 @0.6, 254 @0.7) with a razor-thin **0.03** top1–top2 margin — so
  C1 must **center/rescale to spread that band**, not floor it. The baseline
  renamed the lever from "floor" to "margin/centering."

Baseline JSON is regeneratable (rerun the harness); headline numbers pinned here
are the pre-C1/C2 comparison point.

**C2 landed + validated (2026-07-13).** `legend.c`: `elem_is_ambient_noise` +
an ambient-only reorder in `tier2_focus_miss` (gated on `rec->observe`). On an
ambient miss, bookkeeping kinds (`event`/`task`/`question`/`pointer`) **and**
no-kind elements (relation predicates + prose-object mints — 238 of the store's
410) are scaled ×0.5 and the candidate pool is re-sorted. Honest A/B on the live
store (observe:true, isolated copy): **rank-1-is-noise 82.4% → 0%,
first-domain-rank 3.92 → 1.0, noise-in-top-10 7.6 → 3.3.** Reorder-only (no
cull) — so every pinned metric (presence/resolution-based; `exclusion` even skips
`resolution`) is unaffected: **check.sh stayed green with zero re-pin.** C1
(cosine floor/centering) deferred: cosmetic here (concatenated runs, only ~1%
below 0.5 pre-penalty), and a cull would risk the presence-based gate for only a
token trim.

**Deployed to the trial 2026-07-13 (`fcbb707`).** The pinned binary was upgraded
from `1e1d5b8`, bundling the whole undeployed C batch: the 3 MCP instruction
lines, the `bytes_out` recall field, the Codex `init` scaffold, and C2. Verified:
the new binary stamps `build=fcbb707` and carries all three instruction lines;
the store carried forward untouched (clock 126). The next alchamancer session
records the switchover in the journal. **This starts the go/no-go countdown** —
~3 real sessions on `fcbb707`, then render the `#37` verdict.

**Go/no-go verdict criterion.** Render the `#37` verdict **after C1+C2 lands and
~3 more real alchamancer sessions run on the upgraded binary** — ties the
judgement causally to the retrieval fix we're about to ship. Candidate branches:
**GO-deepen** (more MCP surface), **GO-breadth** (more host projects / the
Claude-vs-Codex axis), **WIND-DOWN**. The **PIVOT-to-distilled-store** branch
(pre-baked composable stores, trial demoted to eval-only) is **parked as a
separate track**, not gated on this verdict — so the go/no-go stays a clean test
of the live-capture hypothesis specifically. Instrumentation: testimony ritual
each substantive session (watch #9), `journal_report.py` pre/post the C1+C2
landing, and the Claude-vs-Codex axis once both agents drive the shared store.

## VERDICT RENDERED 2026-07-15 — GO (deepen) · `#68`

Three `fcbb707` countdown sessions (T4/T5/T6). **Legend earns its cost → GO, not
wind-down.** The evidence is consistent across all six testimonies:

- **What reliably delivers:** the orientation packet + durable saved knowledge —
  decisions, mechanics, **deferred levers**, **procedural recipes**. The trial's
  two highest-value moments (T5 deferred-banish lever → next request; T6 icon
  recipe → drove gen + the blur fix) both came from here and saved rediscovery
  the code *and* the compaction summary could not provide.
- **Where the value isn't:** in-flight state (the summary carries it better),
  uncovered domains (T4), and ambient per-prompt recall — the weakest surface.
- **Question the trial resolved:** the two-surface boundary is *durable-and-
  cross-session (→ Legend) vs in-flight (→ summary)*, NOT procedural-vs-domain.
- **Ceiling named:** recall quality ≠ utilization (T6 flood-fill under-applied).

**Branch: GO-DEEPEN** — polish the proven core, fix the known weak spots on
alchamancer, defer breadth (more projects / Codex axis) until the core is solid.
GO-deepen backlog: (1) **ambient abstention** (`#63`/task 11) — lexical-anchor
gating so ambient goes quiet off-domain; (2) **save papercuts** — `source`
phantom fixed (`ec197d7`), `changes.from` mismatch phantom queued (`legend.c`
~5037, reuse-only for `from`); (3) **summary-split nudge** (`#66`/task 12, next
instruction batch); (4) bundle all into the next deliberate trial-binary upgrade.

## ROUND 1 CLOSED 2026-07-15 · deployed `ecab48c` · `#69`

Round 1 = one full cycle: 6 testimonies → verdict → GO-deepen fixes → deploy.
Shipped this round and **live on the pinned binary `ecab48c`**:

| Fix | Commit | Effect |
|---|---|---|
| C2 ambient retrieval separation | `fcbb707` | rank-1-noise 82%→0%, first-domain-rank 3.9→1.0 |
| `source`-without-facts phantom | `ec197d7` | no lone provenance-sentence element |
| `changes.from` mismatch phantom | `d6058b0` | re-change reuses the cached prior, no phantom value |
| Ambient abstention (lexical anchor 0.6) | `ecab48c` | off-domain sweeps go quiet (~3% fire, modest but correct) |

**Deferred to Round 2:** summary-split instruction nudge (`#66`/task 12);
`changes.from` for fact-set (not change-cached) priors; a stronger off-domain
abstention signal than lexical overlap (common-word hits anchor most prompts).
**Round 2 = the next alchamancer sessions on `ecab48c`** — fresh testimonies
tell us whether the fixes moved the needle. Continue rounds until Legend is
amazing.

## ROUND 2 PRE-TEST FIXES 2026-07-15 · deployed `3bf188a`

Two of the three deferred items shipped before the user's Round-2 sessions;
the third is reclassified as a design pass, not a hotfix.

| Fix | Commit | Effect |
|---|---|---|
| Name-coverage abstention anchor | `3bf188a` | anchor signal swapped from scan score (name+summary, query-coverage) to `tier2_name_anchor` (name-only, name-coverage). Fixes the false-abstention of prompts that *name* an entity in passing ("lets keep working on the ai duel mode" now anchors). Live ambient workload (182 recalls): 11% resolve / 50.5% candidates / **38.5% abstain** — vs 81% over-suppression under the query-coverage trial. |
| Summary-split nudge | `3bf188a` | `#66`/task 12 — instruction in both `MCP_INSTRUCTIONS` and the `legend_save` tool description: split an overgrown summary into child elements, keep a short core. |

**`changes.from` for fact-set priors → deferred as a design pass, not a
hotfix.** The observed Round-1 papercut was the *change-cached* mismatch,
already fixed by `d6058b0`. The fact-set variant (a value first recorded as a
plain FACT, later hit with `changes`) is not a clean bug: a `changes` op
supersedes the `current_<prop>` cache, which plain facts never populate, so
`plan_prior_peek` correctly misses. The only correct fix is to have `changes`
*recognize and supersede a plain-fact prior* — a **core-semantics decision**
(should `changes` supersede `facts`? how to disambiguate a multi-valued
property?), with real corpus/ambiguity risk. Doing only the "recognize for
`from`-fill" half would be *worse* than today: the change caches the new value
while the stale fact stays live — a silent contradiction instead of a visible
phantom. Filed for a deliberate design pass; not blocking Round 2.

**Round 2 now runs on `3bf188a`.** Warm-server note: two pre-`ecab48c`
trial `mcp-serve` processes were left over from ended sessions (started Jul 12
and Jul 14 21:04) — a binary swap does not hot-reload a running process, but a
fresh Round-2 session spawns a new server from the deployed binary and bypasses
them, and the journal `build` column stamps the switchover.

## ROUND 2 ADDS: causal representation (Book of Why) — what to test

Shipped alongside Round 2 (three phases, on top of `3bf188a`): Legend can now
represent cause and effect as first-class structure. Full reference in
`docs/causal.md`; this is the trial test plan.

**What's new the trial should exercise:**
1. **Causal predicates** — save `{s, p, o}` facts with `p` = `caused`,
   `enables`, `prevents` (a real cause) or `correlated_with` (co-occurrence
   only). They dedup to one seeded element each. Test the invariant: use
   `correlated_with` when you only know two things co-occur and confirm Legend
   never surfaces it as a cause.
2. **Modality** — tag a fact with `modal`: `intervened` (an agent acted, vs the
   default observed), `non_actual` (counterfactual/desired), `negated`,
   `uncertain`, `general`. E.g. a design post-mortem: `{deploy, caused, outage,
   modal:[intervened]}`; a counterfactual: `{earlier_test, prevents, outage,
   modal:[non_actual]}`.
3. **Recall `causal` section** — recall a focus that participates in causal
   edges and read the new `causal` array: each edge with its `rung`
   (causal/correlational) and `modal`. These are consumed from `recent`/
   `related` (typed once, not buried).

**Signal we're looking for (testimony questions):**
- Does capturing *why* (cause/effect) as structure beat prose in a summary —
  does a later session actually use the `causal` section?
- Does the rung/modality distinction earn its keep, or is it ceremony the model
  won't populate correctly under real load? (Watch: does the calling LLM reach
  for `caused` vs `correlated_with` appropriately, or default to one?)
- Clutter check: does the typed `causal` section reduce noise vs the edges
  landing in `recent`/`related`, or add a section nobody reads?
- Utilization vs quality (the T6 ceiling): even if recall surfaces good causal
  structure, is it applied?

**Not tested this round (deferred):** interventional/counterfactual *queries*
(do()-projection, §24.9) — only the representation ships now.

## What the store was seeded with (baseline)

Deep onboarding (all docs, module tree, 255-commit history + two interview
rounds with Nick): 120 elements / 263 relations at clock 6 — the project +
9 systems/mechanics, 8 decisions with chose/rejected/reason (including the two
supersessions no single doc states: PRD roguelike → authored overworld
2026-06, Calix/Vex → Etande/Divine-Comedy cast), 6 active constraints, 7 open
tasks/questions (including `plan docs overhaul` and `summon capstones in
duel`), 8 doc pointers. Current focus at seed time: multiplayer/duel.
