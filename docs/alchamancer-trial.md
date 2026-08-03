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

> **Embeddings verified on (2026-07-22).** A harness bug in `benchmarks/simpleqa`
> ran recall with embeddings OFF because it resolved the model dir relative to the
> process CWD. **The trial is NOT affected:** the MCP server (`.mcp.json`) and every
> hook pin `LEGEND_EMBED_DIR` to the absolute `~/.local/share/legend/bge-small-en-v1.5`,
> and this was confirmed empirically on a store copy (model loads, candidates come
> back embedding-ranked). The trial's recall has used embeddings throughout — but on
> the pre-F1 binary (`9d745ee`), so it is embeddings-on-but-recency-ranked; F1's
> query→fact relevance ranking reaches the trial only on a re-pin. See
> `docs/session-2026-07-22-retrieval.md`.

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

**0. Round close (all gauges in one command)** — run at every round close so nothing
is forgotten; reads the journal + a read-only store copy, runs the pinned binary:

```sh
python3 harness/round_report.py [build_sha]   # default: latest build in the journal
```

Reports, for the round's build segment: activity + save-op mix; **prose-backstop**
firing/recovery + whether prose slipped through; **reroute watch** (`fact.o` prose that
would indicate the backstop just moved pollution); audit health, packet size, and the
`correlated_with` invariant; and — the gauge the journal can't give directly —
**ambient-recall quality** by replaying the round's ambient queries and tallying
surface-vs-abstain (too-quiet → lower `TIER2_AMBIENT_SEMANTIC_MIN`; clutter → raise).
Compare its snapshot to the round's baseline (recorded in the round section). This does
NOT check determinism — that's step 1.

**1. Determinism in the wild** — the journal is a replayable corpus; rebuilding
the store from it must reproduce the live snapshot byte-for-byte:

```sh
python3 harness/replay_journal.py ~/Code/alchamancer2/.legend
```

Exit 0 + `byte-identical: True` = healthy. The check is **build-aware**: every
journal line records the `build` (git short-sha) that wrote it, so replay
reconstructs each distinct binary from git (`git archive` + `cc`, cached per sha)
and replays every line under its **own** binary — the whole multi-build journal
verifies byte-for-byte, not just one binary's segment. Replay runs with
`LEGEND_EMBED=0` (embeddings never affect the snapshot). Divergence means a real
determinism bug or a hand-edited store — bisect by truncating the journal; exit 2
means a `build` couldn't be reconstructed (a `dev`/unstamped binary, or a sha not
in git). **Verified byte-identical 2026-07-16: 425 ok lines across 8 builds**
(`ad49797` → … → `2c42c74`, the causal binary).

**Why build-aware.** Byte-identical replay holds only when each line runs under
the binary that wrote it. A mid-trial bug fix changes write semantics, so an op
that relied on the OLD behavior replays differently under a NEW binary. Concrete
case this tool now handles: line 335 (`fcbb707`) is a `merge` whose `from` is a
`source`-phantom element the buggy `source`-without-facts path minted at line 334
— replaying it under any post-`ec197d7` binary (the fix) fails, because the fixed
path never mints the phantom. Replaying line 335 under `fcbb707` (its own build)
reproduces it exactly. `--simple` forces the old single-binary replay and is
correct only for a one-build journal; it will (correctly) diverge on this store.
The live snapshot remains the source of truth regardless — the warm server and
CLI load it directly; replay is only the diagnosis tool.

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

### Check-in 2026-07-16 — store-health read (clock 212; dev store `#91`)

Read-only via `journal_report.py` + a temp-copy frame measurement (zero live
journal footprint — `dump` journals nothing, and frame sizing ran on a copied
snapshot). Growth over 8 days / 151 saves / 55 sessions: **120→651 live
elements, 263→1352 relations** (21 superseded/retracted). Snapshot 420KB,
vectors 944KB — loads fast; a focused recall is cheap (~5.8KB).

**Watch #3 (frame/token cost) has FIRED — but as truncation, not token spend.**
The orientation packet is now **38KB compact / 35KB pretty** (61 decisions + 38
constraints emitted uncapped, plus summary bloat — `#506`'s summary alone
~1500 chars). The SessionStart hook (`recall '{}' --pretty | head -c 4000`) was
tuned when the store was small; **4000 bytes now cuts off inside `overview`**,
after ~4 working-set entries. The model receives project scope + top-salience
active items ONLY and never sees the `decisions`/`constraints`/`open`/`recent`/
`causal` sections at boot. So store growth is being paid as *silent orientation
loss*, not tokens. **The fix is at the surface, not the store** — and per the
stale-intent finding (`#89`) pruning the store is the wrong lever (you want the
cold/stale facts). Levers: (a) cap the orientation sections (top-N decisions/
constraints by salience/recency); (b) enforce summary discipline (`#66` — the
split-into-children nudge isn't holding; 36 paragraph-named elements now, up
from 8); (c) retune the hook cap. Ambient/explicit recall (cheap, focus-scoped)
currently masks the loss — the model re-fetches what boot drops.

**RESOLVED 2026-07-18 (levers a + c).** By then the packet had grown to
**51KB** and the cut had moved *earlier* — landing inside `overview` itself, so
the model never even reached `state`. Fixed at the surface, exactly as the
diagnosis prescribed, with nothing pruned from the store:

- **(a) Section caps.** `decisions`, `constraints`, `open`, `causal` and each
  custom-kind section emit at most 10 entries, newest first, with the held-back
  counts reported in a top-level `omitted` object. On the live store that is
  51,441 → 18,895 bytes of JSON, `omitted:{decisions:93, constraints:66,
  open:13, causal:26}`.
- **(c) Hook retune.** The generated SessionStart hook becomes
  `recall '{"limit":16}' --pretty | head -c 20000`. The caps now bound the
  content, so `head -c` is a safety valve against a pathological store rather
  than the routine trimmer it had become. The live packet is 13.3KB pretty and
  arrives whole — every section, plus `omitted`, reaches the model.

Lever **(b)** — summary discipline — remains open under `#66`. It is the
larger remaining driver: `overview` alone is still 5.3KB for five active
entries, because those entries carry ~1KB summaries.

**Watch #1 (predicate proliferation):** 19→**84 minted predicates**, long
single-use tail, plus a `current_*` status-as-fact family (55 uses, incl. a
`current_current_status` doubled-prefix bug) — **watch #11's nudge is not
landing**; the model reifies status as predicates instead of `changes`.
**Watch #5:** 0 exact-duplicate names (dedup solid), but 36 paragraph-named
elements (prose reified as names — the `#615` mechanism). **Watch #7 (write-only
growth):** 609/651 never focus-retrieved (93.5%), *down* from 96.5% at day two —
ratio is flat-to-improving, not worsening; acceptable (insurance memory).
**Decay:** none by design — no eviction; activation decays for ranking only (all
read 0). The store grows monotonically; the only thing bounding delivered cost
today is the crude `head -c` truncation, which bounds it by dropping signal.

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

**Deploy log.** Causal code first went live as `2c42c74` (Round-2 session 1 ran
on it — 53 journal lines). Current pinned binary: **`4ee4ac9`** — a restamp of
byte-identical `legend.c`/`embed.c` (the only changes since `2c42c74` are docs +
the build-aware `replay_journal.py` fix, neither of which ships in the binary),
so behavior is unchanged. The deployed stamp tracks the binary-code version;
docs/harness commits after it do not require a redeploy.

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

## ROUND 2 SESSION 2 — testimony + two filed issues (2026-07-16)

An alchamancer asset session (ran on `2c42c74`/`4ee4ac9`, the causal binary).
It exercised the new surface hard — minted a new predicate `justifies` (checked
existing predicates first, per instruction (1)), corrected a stale constraint,
and used `retract`/`merge` for cleanup — and surfaced two real defects.

- **`#616` — Negated facts recalled as asserted (data-integrity, SERIOUS).**
  It saved `{llm readability, justifies, assets are text, modal:["negated"]}` to
  record that a rationale was void; recall returned `status:"asserted"` with no
  negation marker — the exact opposite of the finding. Root cause: Phase 3
  rendered `modal` **only in the `causal` section**, and `justifies` is a
  non-causal predicate, so its `negated` meta existed in the graph but never
  reached the frame. A negated claim was indistinguishable from an assertion.
  **FIXED 2026-07-16 (dev):** `frame_put_rel_entry` now emits `modal` on every
  fact entry that carries one (`recent`/`related`/`state`/`history`), emit-
  when-present so plain facts pay no bytes. Regression-locked in `test_causal`;
  recall-read guidance added to the MCP instructions ("a `negated` fact is
  asserted to be FALSE, so read the modal"). See `docs/causal.md` §"Modality
  surfaces on every fact". **Not yet on the trial binary** — folds into the next
  deliberate upgrade.
- **`#615` — attrs silently reify prose into elements.** Passing prose as an
  `attr` value minted 7 sentence-named phantom elements. The "never pass prose
  as a value" nudge was scoped to fact *objects*; attr values reify identically,
  and the attrs schema ("structured properties") read as non-reifying.
  **ADDRESSED 2026-07-16 (dev):** the prose nudge now names attr values in both
  the verbose instructions and the compact `legend_save` description, and the
  attrs schema says "each value becomes an element, so use canonical names, not
  prose." Instruction-only fix (attrs reifying is by design); no auto-guard.

Both fixes are dev-side (`./legend`); the pinned trial binary at
`~/.local/bin/legend` is unchanged until a deliberate upgrade (see the deploy
discipline above). Cleanup lesson the session also recorded: `retract` is the
right tool for a wrong fact; `merge` on unrelated elements makes self-loops.

## ROUND 2 SESSION 3 — the strongest testimony, and a design pivot (2026-07-16)

An AP-migration coding session (same binary), **all non-causal/non-modal**, so
unaffected by the still-undeployed `#616` — a clean example of the
"continue non-modal work on the current binary, redeploy before modal-heavy
work" call. Elements `#88` (testimony 8) / `#89` (the stale-intent principle),
both `informs` `trial value evidence` (`#37`).

- **The value was a RULE, not a fact.** The biggest hit was a carried
  probe-discipline rule ("a probe that has never failed is a decoration"); it
  changed the *quality* of the work (5 fixes got sabotage-tested that otherwise
  wouldn't have). Facts saved minutes; the rule changed the outcome. This is the
  "what code cannot hold" category (`#39`) doing its core job — strongest
  evidence yet that the durable payoff is decisions/rules/recipes, not facts.
- **Recall collapsed a diagnosis.** Ambient + one explicit recall turned
  "reconstruct the AP migration from git history" into "find the one call site
  that missed the memo" in ~a minute.
- **A STALE memory was the most productive thing in the store — a design
  pivot.** `#562` "explore ignores AP entirely" was true of absorb, false of
  casting; the gap between recorded *intent* and drifted *code* WAS the bug, and
  gave standing to fix the whole design rather than one site. **Implication for
  the `#71` supersession fork:** aggressively pruning facts to match the code
  would destroy this drift-detection value. Record intent, let it go stale,
  surface the gap — do not silently "correct" it away. Captured as `#89`.
- **Phantom recurrence (finding-8 amplifier).** A fact referencing a name the
  agent had read in the CLAUDE.md *file-memory* index silently minted an empty
  Legend stub (facts mint unknown refs rather than erroring). Self-caught via
  `writes.minted_elements` and retracted — the net worked — but it confirms the
  two-surface hazard: a name from one memory system silently becomes a stub in
  the other. The real fix is the deferred memory-surface reconciliation (Phase
  D / hooks), not another instruction line; logged here, not instrumented, until
  it recurs a third time or the reconciliation lands.
- **Attribution honesty (keep the trial clean).** Some wins this session came
  from the file-based auto-memory (`-Werror` divergence; "Nick tunes numbers by
  playing"), NOT the graph. Don't credit Legend for them.
- **Gap:** the store's *silence* about `run_rival_turn`'s existing kite band let
  the agent assert "no `enemy_step_away` primitive exists" (false, caught by the
  compiler). Arguably fair — code knowledge the code should hold — but a note
  that absence-in-store isn't evidence of absence-in-code.

## ROUND 2 CLOSED 2026-07-16 · deployed `8fbeedc` · Round 3 next

All Round-2 sessions ended; the round is closed and the trial binary upgraded
(deliberately, sessions quiescent) from `4ee4ac9` → **`8fbeedc`**.

**Shipped in `8fbeedc`** (on top of the causal binary):
- **`#616`** — negated/modal facts now surface `modal` on every recall entry
  (recent/related/state/history), not only the causal section. A `negated`
  non-causal fact no longer recalls as a plain assertion. Data-integrity fix;
  `modal:["negated"]` is trustworthy again.
- **`#615`** — the "no prose as a value" nudge now covers attr values (both
  instruction mirrors + the attrs schema).

**Deliberately NOT in this binary** — the **orientation-packet surface bounding**
(the store-health check-in above / `#91`). Capping which decisions/constraints
surface is a design change with real downside if done blindly (a mis-ranked cap
could hide a load-bearing constraint at boot), so it is Round 3's opening dev
item, not a rushed pre-redeploy edit. The store balloon is a surface problem, not
a storage one — the fix bounds what recall *surfaces*, never what the store
*keeps* (per stale-intent `#89`).

**Round 3 = the next alchamancer sessions on `8fbeedc`.** Two things to watch:
(1) the first clean test of *working* modality — does a `negated`/`non_actual`
claim get read correctly and earn its keep; (2) the orientation-packet balloon
(watch #3) under continued growth, pending the surface-bounding fix.

## ROUND 3 CLOSED 2026-07-18 · `#126` · Round 4 needs a re-pin

18 sessions on `8fbeedc` (07-16 → 07-18): 110 invocations, 31 saves, **zero
rejections** — against 5 across all earlier rounds combined, the clearest
evidence yet that the MCP instruction fixes landed. Work was audio/DSP doctrine,
regions design, and playtest batches. Determinism healthy: 579 ok journal lines
replay byte-identical across 11 binaries.

**Watch item (1): modality is lightly adopted — corrected 2026-07-19.** The
Round-3 close originally recorded "zero `modal` payloads," but that was a grep
artifact (`payload` is an escaped JSON string; `grep '"modal"'` misses the
escaped key). The structural count is **5 accepted, unprompted `modal` facts**
(journal lines 383/385/391/403/455), each using `negated`/`general` correctly
where something is inert. And the rung-2 causal predicates are **adopted** —
`enables` 22, `caused` 10, `prevents` 6 across 509 facts; only `modal` (5) and
`correlated_with` (0) are rare/dead. So causal representation is *used*, modality
*rarely*. `#127` resolved 2026-07-19: **leave `modal`, stop counting it as
shipped value** — a correct, cheap-when-unused capability waiting for a domain
that needs negation, not a defect. The likelier next lever is `correlated_with`
at 0/38: the invariant "never call a correlation a cause" is at risk from the
model never *picking* the weak option, not from relabeling.

**Watch item (2), the packet balloon, is fixed** — see the RESOLVED note under
the store-health check-in above. The packet had grown to 51KB with the hook cut
landing inside `overview`; section caps plus a hook retune bring it to 13.3KB
pretty, delivered whole.

**Two new issues were raised in-band** by the sessions themselves: `#615` attrs
silently reify prose values (one save of 3 elements minted 7 prose-named
phantoms) and `#645` cross-store name collisions mint phantoms (a fact object
read from the CLAUDE.md file-memory *index* rather than from Legend — the same
failure as testimony 8, now named as a structural hazard of running two memory
systems whose topic names deliberately overlap).

**First human-in-the-loop maintenance pass** ran at close (`#119`, `#130`): 16
decisions, applied at ticks 263–265 tagged `source="maintenance 2026-07-18"`.
`status_fact` 7→0, `stale_open` 21→13, `prose_name` 17→14. Replay stayed
byte-identical afterwards, so mixing a newer build's writes into an
`8fbeedc`-pinned journal cost nothing. Independent validation for the audit: its
`prose_name` and `orphan` checks surfaced `#615`'s wreckage without knowing the
issue existed.

**Round 4 requires a manual re-pin** — the `#91` fix and the whole audit/
maintenance surface reach the trial only when the pinned binary is replaced and
the SessionStart hook updated:

```sh
git commit ...                                   # FIRST — see gotcha 1
cc ... -DLEGEND_BUILD="$(git rev-parse --short HEAD)" -O2 legend.c embed.c -o legend
cp ~/Code/legend/legend ~/.local/bin/legend.new  # gotcha 2: not a direct cp
mv -f ~/.local/bin/legend.new ~/.local/bin/legend
# then in ~/Code/alchamancer2/.claude/settings.json, SessionStart command:
#   ... recall '{"limit":16}' --pretty 2>/dev/null | head -c 20000
```

`legend init` never clobbers an existing `settings.json`, so the hook edit is
manual. Until both are done, Round 4 runs on `8fbeedc` and sees neither the
capped packet nor `legend audit`.

**Done 2026-07-19: re-pinned to `0c70e2a`, hook updated** (`{"limit":16}` /
`head -c 20000`). Measured on a store copy first: the new packet is 14.3KB
(arrives whole, all seven typed sections); the old `{}`/`4000` hook cut at 4KB
mid-sentence inside `overview` with zero typed sections surviving.

Two re-pin gotchas, learned the hard way — fold into the recipe above:
1. **Commit before you build the binary you pin.** The stamp is
   `git rev-parse --short HEAD`, and it goes into every journal line. Pinning a
   binary built with uncommitted changes records a sha that `git archive <sha>`
   cannot reproduce — the journal *lies* about which binary wrote it, and
   build-aware replay diverges as if corrupt. Commit first so the stamp is real.
2. **The pinned binary is `Text file busy`** — warm `legend mcp-serve` processes
   hold it open, so a direct `cp` over it fails (`ETXTBSY`). Copy to a sibling
   name and `mv -f` over the target: rename swaps the directory entry without
   truncating the running inode, so live servers keep their old binary until
   they restart (which is the Round boundary anyway) and the next fresh
   invocation gets the new one.

## ROUND 4 CLOSED 2026-07-21 · `0c70e2a` · `#145`

The first round to run on the capped packet, the 400 bloat threshold, and the
MCP `audit`/`maintain` surface. **Closes at ~15–18 sessions (parity with Round
3), then one maintenance pass.** No mid-round gardening — bloat, suspects, and
phantoms accrue naturally so the *rate* is what gets measured, not a groomed
snapshot.

**Baseline at start** (`#146`, read-only on a store copy, `0c70e2a`): 639 journal
lines, 886 elements, 2172 relations. Audit: bloat 281, prose_name 16, stale_open
14, orphan 11, near_dup 2, status_fact **0** (held since the Round-3 pass).
Summaries: max 2042, p90 1427, over-400 = 283. Causal: enables 23 / caused 10 /
prevents 6 / correlated_with **0**. Modal 5. Packet **12.7 KB, whole**.

**Pre-registered gauges** — each anchored to the baseline and each deciding an
open item, so the close is a measurement, not an impression:

1. **Packet orients** (`#91`, never tested in the wild before): arrives whole
   every session, zero truncation inside `overview`, testimony that it oriented.
2. **400 + the number changes writing** (`#66`/`#122`): of summaries *written
   during* the round, does the fraction over 400 fall vs history, and does
   max/p90 hold? Rate, not count — count rises with store size, which is the
   whole `#122` point.
3. **`phantom_change` in the wild** (`#135`): count Round-4 `changes`/`resolves`
   whose target lands in `minted_elements`. >0 → build the detector; 0 →
   theoretical, deprioritize.
4. **`correlated_with` stays 0** (`#137`): any Round-4 causal fact using it? Still
   0 → the "never call a correlation a cause" invariant-risk is confirmed.
5. **Maintenance pass at close** feeds the retune bucket: stale_open true/false
   split (`#118`), bloat count-vs-distribution (`#122`), near_dup precision.

Determinism note: the `8fbeedc → 0c70e2a` upgrade is a fix-bearing one, so
cross-version byte-replay diverges at the boundary by design (see the
determinism memory) — not corruption. Within `0c70e2a`, replay must stay
byte-identical.

**CLOSED 2026-07-21 — 7 sessions (below the 15–18 target; findings directional).**
104 invocations, all on `0c70e2a`; determinism byte-faithful on replay (968 ==
968). Gauge results:

1. **Packet orients** — holds: 12.7 → 15.0KB, still whole. But climbing ~2.3KB
   per +82 elements; the `#141` scaling pressure is now empirical. No orientation
   testimony was saved, so the qualitative side stays untested.
2. **400 write-rate** — FAIL. 43 of 44 R4 summaries exceeded 400 (98% vs 74%
   historical); distribution flat, count climbed +38 with store size. The
   write-side length instruction is dead (`#150`), and bloat is now out of the
   session tally (`#153`) — closing `#122`.
3. **phantom_change** — 0 in the wild → standalone detector deprioritized
   (`#135`). But the sibling `changes.to`-reifies-prose fired 4 of 5, minting
   300–542-char prose names — the round's dominant papercut → warning shipped
   (`#152` / `#149`).
4. **correlated_with** — still 0 across 39 causal facts; the invariant-risk is
   confirmed (`#137`, kept open, no check built — speculative precision).
5. **Maintenance** — status_fact held at 0; prose_name 16→20 (all via the
   changes.to path), bloat 281→319, stale_open 14→16.

Footnotes: 1 rejection (malformed JSON on a complex musical payload); the model
minted two off-list kinds, `reference` (11×) and `feedback` (2×) — taxonomy
drift, directional.

## ROUND 5 CLOSED 2026-07-23 · re-pinned to `19868c3` · `#155`

Re-pinned with the two Round-4 fixes live: the `changes.to` prose warning and
bloat dropped from the session tally. **Deliberately lightweight — one gauge:**
does prose-name pollution stop growing now that the model is warned? Baseline
`prose_name = 20`, packet 15.0 KB whole. Passively watching packet growth toward
the 20 KB cap and whether `correlated_with` ever leaves 0. Close when convenient;
not a formal round.

**MID-ROUND READ 2026-07-23 (clock 409, 1168 elements, 269 invocations on
`9d745ee`; read-only on a store copy). The gauge FAILED:**
- **`prose_name` grew 20 → 36.** The warning is **instruction-only** (MCP tool
  text, no code check) and the model ignores it under load: **16 of 27 Round-5
  `changes` set a prose `to` >120 chars** on `build_status`/`current_state`
  ("BUILT 2026-07-21 sim-side, ART IS PLACEHOLDER…" 685). Soft nudge is dead —
  → mechanical backstop spec'd: `docs/prose-value-backstop.md` (reject a
  `changes.to` mint >120, guide the prose into the summary). Scope decided by
  measurement: `changes.to` only (0 legit values >120 across all 62 trial
  `changes.to`; fact-objects/attr-values go legitimately long, so a broad gate
  would false-reject real work).
- **Packet 15.0 → ~16.0 KB, still whole.** Climbing toward the 20 KB cap (`#141`).
- **`correlated_with` still 0** as a fact predicate — invariant holds (`#137`).
  `phantom_close` and `status_fact` still 0.

The prose backstop is the **last piece before the next deliberate re-pin**, which
should bundle it with F1 ranking + the recall `query` field + Phase-1
`resolves.o must_exist`. The re-pin is the natural Round-5 close.

**RE-PINNED 2026-07-23 → `19868c3`** (`install -m755 legend ~/.local/bin/legend`,
strict `-std=c99 -Wall -Wextra -Werror` build, sha-stamped). The live store loaded
unchanged (no migration; all four changes are save/recall logic, no snapshot bump).
Round 6 opened organically the moment a live session recalled on the new binary.
Cross-version replay diverges at this boundary by design (fix-bearing) — not
corruption; replay stays byte-identical within `19868c3`.

## ROUND 6 CLOSED 2026-07-25 · `19868c3` · prose backstop UNEXERCISED (inconclusive)

**Closed after 2 days / 120 invocations — but the round was recall-heavy and
save-light, so the prose backstop was never exercised** (like the modal fix
`#127`/`#134`). Delta over the `19868c3` segment:
- **8 saves, 112 recalls; ZERO `changes`/`retract`/`merge` ops** — every save was
  `elements`+`facts` only. The backstop targets `changes.to`, so with no `changes`
  it couldn't fire: **0 `prose_value` rejections, 0 prose `changes.to`.** The
  reduce-vs-relocate question stays **OPEN** — carry the backstop forward untested.
- `fact.o` prose (>120) added this round: **0** (no relocation, but also nothing to
  relocate).
- **Store health stable/good:** prose_name **36→36** (flat), bloat 397→409 (+12
  noise, out of tally `#153`), stale_open/orphan/near_dup unchanged, phantom_close &
  status_fact still **0**, `correlated_with` still **0** (invariant holds).
- **Packet 16.0 → 13.9 KB** — *shrank* ~2 KB (activation decay redistributed the
  top-16 salient set). Good for the `#141` cap-pressure watch; not climbing.

Verdict: no signal on the backstop — it needs a **save/`changes`-heavy** round (like
R4/R5) to test. **Next re-pin should bundle the ambient-abstention fix (`31bdb5e`,
never deployed — the trial ran R6 on 19868c3, not the intended "Round 7" binary) and
carry the prose backstop forward; watch BOTH next round.**

## ROUND 7 CLOSED 2026-07-27 · `c1f1b06` · ambient fix VALIDATED

**67 invocations over 2 days** (12 saves — 49 elements / 70 facts / **11 changes /
3 retract** — and 55 recalls; 0 rejections). Determinism **byte-identical: True**
(build-aware replay, all 15 binaries). The round finally had `changes` activity (R6
had none). Gauge results:

1. **Ambient recall — VALIDATED in the wild (the round's headline).** 49 real ambient
   queries, **77% surfaced** with relevant candidates (`"make the default for shape a
   bolt"` → *bolt is the default shape* @0.85; `"…after they cast"` → *every cast
   selects a target* @0.74), and it **correctly abstained on contentless prompts**
   ("try again", "continue"). The 0.75 dial is well-placed — no tuning needed. The
   lexical-only gate that returned empty is fixed.
2. **Prose backstop — clean outcome, mechanically UNEXERCISED.** 11 `changes.to`, all
   short (max **17** chars), **0 prose, 0 `prose_value` rejections, 0 reroute** into
   `fact.o`; `prose_name` flat at **36**. Pollution did NOT recur (R5 was **16/27**
   prose paragraphs → R7 **0/11**). But the backstop never *fired* (no prose attempt),
   so reduce-vs-relocate resolves to "nothing to reduce/relocate." **The mechanical
   catch stays unproven — carry it forward; it may never fire if the model has
   genuinely stopped producing prose `changes.to`** (which is the goal).
3. **Packet** shrank **13.9 → 11.2 KB** — the `#141` cap-pressure keeps easing.
4. **Invariants hold:** `correlated_with` / `phantom_close` / `status_fact` all **0**.

**Verdict: healthy round, GO-quality.** The ambient fix is confirmed working on real
prompts; prose pollution did not return; no new pathologies; determinism faithful.
**No re-pin — nothing new is built to deploy.** The trial continues on `c1f1b06`; the
next round opens on the next fix (candidate levers: summary discipline `#66` — the
ceiling for both ambient *recall* and packet size — and F3 traversal).

## ~~ROUND 7~~ (original setup, for reference) · from 2026-07-25 · `19868c3` → `c1f1b06`

Re-pinned mid-agent (safe: binary swap doesn't disturb a running process, lock
serializes writes, `c1f1b06` reuses the warm `vectors.bin` — no cold re-embed).
Adds the **ambient-abstention fix** (`31bdb5e`) on top of the R6 bundle; carries the
still-untested **prose backstop** forward. Verified live: an ambient recall of
"verge region music" now surfaces **The Verge @0.76** (was empty on `19868c3`).

**Baseline at re-pin** (clock 425, 1202 elements): prose_name **36**, bloat 409,
stale_open 16, orphan 11, near_dup 2, phantom_close 0, status_fact 0; packet
**13.9 KB**; `correlated_with` 0.

**Gauges:**
1. **Ambient recall in the wild** (new): does the passive hook now surface useful
   on-domain candidates in real sessions? **Too quiet → lower the 0.75 dial
   (`TIER2_AMBIENT_SEMANTIC_MIN`); clutter → raise it.** No further tuning without
   wild data — the 14-query sample is exhausted ([[project_ambient_hook_findings]]).
2. **Prose backstop — finally exercise it** (`#149`/`#152`): R6 had **0 `changes`
   ops** so it never fired. If this round has save/`changes` activity, count
   `prose_value` rejections + whether the model recovers (short value + summary), and
   track the reroute (`facts[].o` prose + `bloat`) to answer reduce-vs-relocate.
   Needs an R4/R5-style save-heavy round.
3. **Packet** (`#141`): was *shrinking* (16→14 KB); keep watching it stays clear of
   the 20 KB cap.
4. **`correlated_with` stays 0** (`#137`) — invariant watch continues.

## ~~ROUND 6~~ (original setup, for reference) · from 2026-07-23 · `19868c3` · `#155`

**First round carrying the retrieval work + the prose backstop.** Four changes now
live: F1 query→fact ranking, the recall `query` field (decouples ranking from focus
resolution; no more tier-2 hang on intent terms), Phase-1 `resolves.o must_exist`,
and the `changes.to` prose backstop (`ERR_PROSE_VALUE` on a prose mint).

**Baseline at re-pin** (clock 409, 1168 elements, read-only on a copy): audit
prose_name **36**, bloat 397, stale_open 16, orphan 11, near_dup 2, phantom_close 0,
status_fact 0; packet **16.0 KB** (hook's `limit:16`, under the 20 KB `head -c`);
genuine `fact.o` prose (journal-measured, `facts[].o` > 120) ≈ **10**;
`correlated_with` 0.

**Gauges — relocation-aware (the backstop is a partial fix; adversarial review
showed it may move pollution, not remove it):**
1. **Prose backstop works AND doesn't just relocate** (`#149`/`#152`): does
   `prose_name` growth flatten now that prose `changes.to` mints are rejected?
   Measure **as a journal delta over Round 6**, and *alongside* it track the reroute
   — new `facts[].o` prose (>120) and `bloat` growth. **A `prose_name` win that only
   moves the prose into summaries (`bloat`) or `fact.o` is a wash → then drop/rethink
   the backstop** (Nick: "validate it's working, if it doesn't, drop it"). Also count
   `prose_value` rejections in the journal and whether the model recovered cleanly
   (short value + summary) or looped.
2. **F1 + `query` in the wild**: any testimony that focused recall surfaced the right
   fact? (Retrieval quality was never trial-tested before this binary.) Watch for the
   ambient/SessionStart hooks — do they pass `query`, or should they be taught to?
3. **Packet** toward the 20 KB cap (`#141`) — still climbing; note the backstop
   pushes *more* into summaries, so this is NOT expected to improve.
4. **`correlated_with` stays 0** (`#137`) — invariant watch continues.

Close when convenient; the relocation question (gauge 1) is the decision this round
exists to make.

## ENTITY-MODEL RESET + RE-PIN SEQUENCE (2026-07-27 → 07-30)

After Round 7 the trial pivoted to the **small-intentional-entities / decision-hub**
work. The longitudinal store was deliberately **WIPED and re-onboarded** (2026-07-27,
archived to `.legend.archive-*`; the pre-pivot store is preserved and reversible), so
the live store restarts from a fresh deep onboard — 312 elements at clock ~9, median
name 2 words. This resets the longitudinal accumulation on purpose: the hypothesis is
now the entity model, not raw weeks-of-capture.

**Re-pin log (never upgrade silently — each stamped in the journal):**
- `f83b436` — naming instruction + 5-word name cap + onboard reframe.
- `8c546be` — `flat_decision` audit reason + "decision is a hub" instructions; store
  re-onboarded on this build (**decisions structured 7/30 → 21/24**).
- `7f42f61` — audit refinement (options referenced by a hub aren't flat) + kind nudge.
- **`ddb9f7b` (current, 2026-07-30)** — mutable element kind: resubmitting with a new
  `kind` supersedes the old `instance_of` (fixes the in-band "immutable kind" issue).
  Maintenance save 2026-07-30 corrected C/Rust/SDL `decision`→`language`/`library` and
  closed the issue. Build-aware replay **byte-identical across all 3 builds** after
  every swap (`elem_kind` is derived/never serialized, so the fix needs no migration).

Store health at the re-pin (read-only): `flat_decision 0 · prose_name 0 · status_fact
0 · phantom 0`, 1 near_dup + 1 bloat. Activity light (sessions were game-dev-code-heavy
— a spell-system overhaul — not Legend-heavy). Co-activation miner
(`harness/coactivation.py`, read-only) available but still batch-noise on the fresh
store; meaningful only once organic per-session saves accumulate.

## Re-pin hazard: a server left behind keeps writing (guarded 2026-08-01)

`mv -f` swaps the binary but does not disturb a running `mcp-serve`, so a server
from an ended session keeps serving on its old image. Measured over the last
round: a `7f42f61` process wrote **60 journal lines over 2.5 days** after
`ddb9f7b` was pinned, interleaving with the new build 17 times. The journal
`build` column recorded it faithfully; nothing surfaced it, so nobody looked.

`graph_sync` now compares the journal's last `build` to its own whenever it
reloads a graph it was holding warm -- which happens exactly when another
process wrote. On a mismatch the frame carries `foreign_build`:

```json
{"tick":7,"at":"...","store":"...","foreign_build":"ddb9f7b","resolution":[...]}
```

A start-time check was proposed first and rejected: the stale server's build
MATCHED the store when it started, and the mismatch only appeared later at the
re-pin, so a start-time compare is blind to the actual hazard and false-positives
on every legitimate upgrade.

**It warns, it does not prevent — and do NOT kill the servers.** Measured
2026-08-01: killing a session's `mcp-serve` does **not** respawn it. The client
marks the server disconnected and its tools vanish for the remaining life of that
session — no recall, no save, no Legend at all. That is strictly worse than a
stale build, which at least still works.

The only clean cut is **ending the session**; the next one spawns on the new
binary. So the re-pin has two honest modes:

| | what happens | cost |
|---|---|---|
| End sessions, then `mv -f` | every writer is on the new build immediately | you stop work at a boundary |
| `mv -f` now, let sessions finish | old sessions keep writing the old build until they end | overlap muddies the round's attribution — now **visible** via `foreign_build` rather than silent |

`mv -f` itself is always safe with servers running (that is the `ETXTBSY`
workaround above) — it never needs a kill. The choice is only about how long the
overlap lasts, and `foreign_build` is what makes that overlap measurable instead
of a 2.5-day blind spot.

Verified end to end: a warm server on the new build detected a save from the
previously pinned binary and surfaced it in the next frame.

## ROUND 8 OPEN 2026-08-01 — baseline recorded, NOT YET DEPLOYED

Round 7 closed 2026-07-27; the entity-model reset that followed was a re-pin
sequence, not a measured round. Nothing has been gauged since. This round opens
**before** the re-pin so the save-path bundle can be attributed — the failure
this whole roster diagnosed is instruction fixes shipping into an unmeasured
window and nobody noticing they did nothing.

**Under test:** the bundle in `docs/fix-roster-2026-08-01.md` — `e6973ae`
(kind cap), `78416af` (constraint standing seeds its cache), `bbc9c8c` (pin-25
heal bounded), `f76d3e6` (no plain writes to `current_*`), `ade943b` (prose
metric), `00a0d52` (`foreign_build`). Three of these close defects that were
LIVE in this store and that no audit check could see.

**Baseline, on `ddb9f7b`, 2026-08-01** (`round_report.py ddb9f7b` +
`bundle_gauges.py`):

```
segment      259 invocations · saves 60 · recalls 199 · rejections 9
save ops     elements 213 · facts 348 · changes 16 · retract 38
store        clock 115 · elements 684 · live relations 2437
audit        status_fact 12 · near_dup 1 · prose_name 0 · flat_decision 13
             stale_open 20 · orphan 0 · phantom_close 0 · bloat 124
prose        summaries 357 · max 815 · p90 543 · mean 344
packet       9090 B (cap 20000)
ambient      167 queries · 74% surfaced · 42 abstained
invariant    correlated_with 0
```

**Pre-registered gauges** — decided before deploy, so the round cannot be graded
on whatever happens to move:

| gauge | baseline | target |
|---|---|---|
| clobbered kinds (>2 words) | **1** (`#574 Bio Weapon`) | 0 after repair, stays 0 |
| duplicate live `current_*` caches | **1** (`#602 the brake count`) | 0 after repair, stays 0 |
| `status_fact` | **12** | falls; no NEW ones minted |
| `phantom_close` · `orphan` · `prose_name` · `correlated_with` | **0** | must not move |
| multi-valued groups (exposure) | **70 groups / 167 facts** | preserved across any `changes` |
| distinct predicates · single-use | **80 · 37** | watch only — Watch #1, untouched by this bundle |
| `foreign_build` in any frame | n/a | fires iff a stale server writes |

**Deliberately NOT graded:** `bloat` (a size signal — read `prose` instead,
`#122`/`#153`), and `flat_decision`, whose 13 are predicate false positives that
this bundle does not address.

**To close:** `python3 harness/round_report.py <sha>` then
`python3 harness/bundle_gauges.py`, and compare to the table above.

**RE-PINNED 2026-08-01 15:26 to `aede89d`** — `check.sh` green, clean tree, real
stamp (verified: a fresh store's journal line reads `"build":"aede89d"`). Copied
to a sibling and `mv -f`'d, so the one live `mcp-serve` (started 13:55) kept its
`ddb9f7b` inode rather than being killed — see the re-pin hazard section for why
killing it would have been worse.

**CUT CLEAN at 15:27:20.** Last `ddb9f7b` write 15:23:00, first `aede89d` 15:27:20,
no interleaving — the old session ended before the new one started, so Round 8's
segment has a single build. `foreign_build` never had to fire.

**PHASE 3 APPLIED 15:57**, one stamped maintenance save, two surgical retracts:

- `rel:1966` `{Bio Weapon, instance_of: nothing resolves on cast}` — a mechanic
  (`#577`, Trickster's signature texture) had been written into the kind slot.
  Both `instance_of` edges were live and `elem_kind` picked the wrong one, so the
  clobber here was an ambiguity rather than the supersession the guard was built
  for. `rel:1955 instance_of spell` was untouched and is now the only one.
- `rel:2071` `{the brake count, current_standing: active}` — stale beside
  `rel:2096 settled`, which the element's own summary confirms ("SETTLED by Nick
  2026-08-01").

Dry-run on a copy first, then applied. Gauges `[1]` and `[3]` both **1 → 0**, and
the new guards reject re-breaking either (`prose_value` on the kind, `parse` on a
plain `current_*` write).

**Left deliberately unrepaired:** the 12 `status_fact` plain `standing` facts.
They are harmless and self-heal on the next lift (proven by the review), and
`78416af` stops new ones at mint. The gauge grades "no NEW ones", not the
backlog.

**Open for Nick:** the retracted edge may have been reaching for a real
relationship — Bio Weapon does exhibit that texture. No replacement predicate was
invented, because asserting one is a design call. `#577`'s summary already states
the connection in prose.

## ROUND 8 CLOSED 2026-08-03 20:52 — 6 of 7 gauges pass, `status_fact` FAILS

Segment: **168 invocations, 50 saves, 118 recalls, 13 rejections** (10
`prose_value`, 2 `unknown_ref`, 1 `parse`) across 2 days on a single build.
Save ops: 150 elements, 246 facts, 19 changes, 5 retracts. Store clock 115 →
167, elements 684 → 934, live relations 2437 → 3401.

| gauge | baseline | close | verdict |
|---|---|---|---|
| clobbered kinds (>2 words) | 1 | **0** | **PASS** — repaired, held all round |
| duplicate live `current_*` | 1 | **0** | **PASS** — repaired, held all round |
| `status_fact` | 12 | **17** | **FAIL** — 0 healed, 5 new (below) |
| `phantom_close`·`orphan`·`prose_name`·`correlated_with` | 0 | 0·0·0·0 | **PASS** — unmoved |
| multi-valued groups | 70 / 167 | 100 / 236 | **PASS** — grew, none collapsed |
| predicates · single-use | 80 · 37 | 99 · 46 | watch only — Watch #1 climbing |
| `foreign_build` | n/a | never fired | **PASS** — single-build segment |

### The unregistered win: the prose backstop works

10 `prose_value` rejections, **0 prose slipped through `changes.to`** (19 change
ops, max value length 14), and the reroute watch shows **0 prose displaced into
`fact.o`**. The guard does real work and does not merely move the behavior
somewhere else. This was the clearest result of the round and it was not on the
pre-registered table.

### The failure: `status_fact`, and why the mid-round read hid it

Read at 12 mid-round, which looked like "no new ones minted". It was not. By
identity at close:

- **All 12 baseline facts are still present. Zero healed.**
- **Five are new**, all minted during the round:

```
rel:3246  {no new currency,         standing: active}
rel:3353  {the Lightning branch,    status:   locked}
rel:3392  {the Dragon Mother redraw,status:   locked}
rel:3424  {the Evolution branch,    status:   locked}
rel:3425  {the form system,         status:   scheduled}
```

**Four of the five use `status`, not `standing`.** The guard keys on the
predicate name `standing`, so anything else status-shaped walks straight past
it — and `status: locked` recurs three times, so this is a real recurring need
being expressed through a predicate the guard does not cover.

This falsifies two claims recorded here at the re-pin:

- *"`78416af` stops new ones at mint"* — it does not. `rel:3246` is the exact
  `standing` shape it targets and still got through, and the other four were
  never in scope.
- *"harmless, self-heals on the next lift"* — 12 sat unhealed through a full
  round with 50 saves. The self-heal is not happening in practice.

Follow-up is task #9: find the path that bypasses the guard for `rel:3246`, and
decide whether guarding on a predicate NAME is viable at all when a model will
reach for `status`/`state`/`phase`/`standing` interchangeably. That question
overlaps Watch #1.

### Other movement, not graded

`flat_decision` 13 → 18 (peaked at 19 mid-round), `near_dup` 1 → 2, `stale_open`
20 → 23, `bloat` 124 → 235. Packet 9090 → 9570 B, still under half the 20000
cap. Prose: 357 → 508 summaries, mean 344 → 379, **max unchanged at 815** — the
long tail did not get longer. Ambient surfaced 74% (167 q) → **64% (106 q)**,
worth watching but the query mix changed with the work.

## ROUND 9 OPEN — re-pinned to `970d039` 2026-08-03 20:59

**Built from `970d039`, NOT from HEAD.** HEAD (`c7582ce`) also carries nested
statements; pinning it would have made round 9 grade two independent changes at
once — the exact confound separate rounds exist to prevent. The binary was built
in a detached worktree at `970d039` so the stamp is honest, verified on a fresh
store: `{"build":"970d039","verb":"init"}`.

Cut was clean: no alchamancer2 `mcp-serve` was running, last `aede89d` write
20:52, re-pin 20:59. `mv -f` over a copied sibling, no kill (the ETXTBSY path).

Pre-flight on a copy of the live store: **934 → 935 elements (+1, the anchor),
relations unchanged at 3401.** No migration step, no relation churn.

**Under test:** the self anchor (`docs/self-node.md`) — a seeded `me` element
plus MCP instruction clause (11), so the agent can write itself into facts as a
participant. Fixes unresolvable deixis already in the store (30 elements use
first person with no referent; `#458` is a constraint binding the agent with no
subject to bind to) and unblocks `#157 prior-miss count`.

**Baseline taken AT the boundary 2026-08-03 20:59** (not from the design doc —
the 2026-08-02 figures had already drifted: first person read 30 then, 37 now):

| gauge | baseline at boundary | target |
|---|---|---|
| self-anchored live facts | **0** (0.0% of 3401) | rises off 0; **bound <5%** of live |
| self on `source`/`src` | **0** | stays **0** — authorship never anchors here |
| unresolved first person | **37** | falls |
| provenance share | **1608 (47.3%)** | must not rise |
| packet bytes | **9570** (cap 20000) | watch |
| seed element count | 42 | 43 (32 core + 10 ext + self) |

Carried in from round 8, not this bundle's to fix: `status_fact` 17,
`flat_decision` 18, `near_dup` 2, `stale_open` 23, `bloat` 235, predicates 99 ·
single-use 46.

Read with `python3 harness/bundle_gauges.py` (gauges `[6]`–`[8]`).

**Graded as representational correctness, not retrieval.** The recorded
retrieval lever is still summary bloat / over-extraction (`#66`); this bundle
does not address it and must not be credited for it.

**Two failure modes pull opposite ways** — a dead node (nobody writes themselves
in, gauge `[6]` stays 0) and a mega-hub (everything anchors on `me`, `[6]` past
5%). Both are failures; the target is the band between them.

### Baselines drift — RE-TAKE them at each re-pin

The trial store is live. During the two hours of 2026-08-02 spent building the
round-10 work it moved measurably: provenance 1402 → 1461, unresolved first
person 30 → 31, bloat 168 → 187, near_dup 1 → 2. Numbers recorded in a design
doc describe the SHAPE of a gauge, not the grading baseline. **Capture the
baseline at the boundary, immediately before the re-pin**, or the round is graded
against a number that was already stale when it was written down. This applies to
round 9's table above as much as to round 10's.

## ROUND 10 PRE-REGISTERED (not yet open) — nested statements

Built 2026-08-02, `check.sh` green, NOT re-pinned. Rides AFTER the self anchor in
its own round: both change how models write, so bundling them would make gauge
`[6]` move for two reasons at once and neither would be attributable.

**Under test:** nested statements (`docs/nested-statements.md`) — a statement
carried as the CONTENT of another, which is how a request, report, or quote is
written. Frames now expand a nested statement inline with its modal, and focus
reaches a container from any term inside it. Instruction clause (12) carries the
discipline.

Measured first: of 2908 relations, **zero** carry a content-bearing nested
statement. The 37 that nest are all `derived_from`/`supersedes` plumbing; the
other 1209 relation-to-relation links are metas ABOUT a statement, not nesting.

| gauge | baseline (2026-08-02, re-take at boundary) | target |
|---|---|---|
| `[9]` nested content statements | **0** | rises iff adopted |
| `[9]` inner marked `non_actual` | **0 of 0** | the directive shape done right |
| `[10]` speech acts with no content | **0** | see below |
| packet bytes (`round_report.py`) | 9250 | expansion costs size; watch it |

**A zero on `[9]` is only meaningful together with `[10]`.** Writing a nested
statement currently takes TWO saves — `content` needs a `rel:` id, and a relation
minted in the same payload cannot be referenced. Gauge `[10]` counts speech acts
whose content was dropped: models reaching for the shape and failing to complete
it.

- `[9]` rises → adopted; the ergonomics are fine.
- `[9]` flat, `[10]` rises → the two-save FLOW is the blocker, not the shape.
  That justifies building the `facts[<n>]` same-payload ref form on evidence.
- both flat → models never reached for it; the instruction or the shape is wrong.

Low adoption is therefore an ACTIONABLE result, not a failed round.

## What the store was seeded with (baseline)

Deep onboarding (all docs, module tree, 255-commit history + two interview
rounds with Nick): 120 elements / 263 relations at clock 6 — the project +
9 systems/mechanics, 8 decisions with chose/rejected/reason (including the two
supersessions no single doc states: PRD roguelike → authored overworld
2026-06, Calix/Vex → Etande/Divine-Comedy cast), 6 active constraints, 7 open
tasks/questions (including `plan docs overhaul` and `summon capstones in
duel`), 8 doc pointers. Current focus at seed time: multiplayer/duel.
