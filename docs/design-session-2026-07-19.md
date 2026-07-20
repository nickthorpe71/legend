# Design session brief — 2026-07-19

Written at the end of the 2026-07-18 session so the next one starts informed
rather than re-deriving. Everything here is also in the store as elements with
reasons; this is the reading order, not a second source of truth.

## Where the project stands

Legend v3_minimal: single-file C oracle plus a warm MCP server, gated by
`check.sh` (6797 unit checks, 10 golden fixtures, corpus replay determinism,
fuzz, ASan/UBSan, `-Werror`). The go/no-go is **settled** — `#68 trial verdict:
GO-deepen` resolves `#37`. The trial is no longer asking whether Legend earns
its keep; it is a findings machine, and nearly every commit in the last eleven
days traces to something the deployment surfaced.

Round 3 closed 2026-07-18: 18 sessions, 110 invocations, 31 saves, **zero
rejections**. Round 4 is pending a manual re-pin (below).

## Update — 2026-07-19 session (read before the forks below)

The forks were re-examined against the store this session. **Three claims in the
original brief were verified WRONG**; the corrections here supersede the framing
that follows.

1. **Modality is NOT at zero adoption.** "Zero `modal` payloads" was a grep
   artifact — `payload` is an escaped JSON string, so `grep '"modal"'` finds
   nothing while the structural count is **5 accepted, unprompted `modal` facts**
   (journal lines 383/385/391/403/455), each using `negated`/`general` correctly
   where something is inert. The rung-2 causal predicates are **adopted**:
   `enables` 22, `caused` 10, `prevents` 6 across 509 facts. Only `modal` (5) and
   `correlated_with` (0) are rare/dead. (A naïve substring search overcounts to
   29 — this is a game codebase full of the UI word "modal"; 5 is the structural
   number.)
2. **The `#66` "280 nudge" was never built.** `280` existed *only* as the audit
   threshold `g_aud_bloat_chars`; no write-path instruction ever carried a
   number. "290 summaries over threshold" measured a numberless instruction
   against a constant the model was never shown. Fixed this session (below).
3. Scorecard / `near_dup` counts drift with the store — read them as of their
   measurement date, not standing truth.

### Decisions taken
- **`#71` → instruction only.** No engine change; `changes` does not supersede
  plain facts. The instruction already said "status-like values go through
  changes, not facts"; this session added the phantom warning below.
- **`#127` → leave `modal`, stop counting it as shipped value.** Correct and
  cheap-when-unused; 5 correct uses is real but rare. Causal predicates stay.
  Re-scope the fork to `modal` alone. Do **not** cut.

### Shipped this session (committed; trial re-pinned)
- `bloat` threshold **`280 → 400`** (`#122`: a terse core + its result lands
  ~300). The number now also appears in the write-path instructions, so guidance
  and audit finally agree. Fixtures resized; check.sh green.
- Trial re-pinned to this build; SessionStart hook →
  `recall '{"limit":16}' --pretty | head -c 20000`. Round 4 now sees the capped
  packet and `legend audit`.

### New finding — `phantom_change` (adjacent to `#71`, deferred to the audit bucket)
A `changes` (or `resolves`) whose target does not exist **silently mints a
phantom**: the `current_<prop>` cache lands on the phantom and the real element
keeps its stale value. No audit check catches it — `orphan` needs `refs==0`,
`near_dup` skips names under 12 chars, `phantom_close` checks `resolves` only.
Verified live this session with a typo'd target. Instruction mitigation shipped
(target-must-exist warning on `changes`); a `phantom_change` detector mirroring
`phantom_close` is deferred to the next maintenance pass alongside the
`#118`/`#122` audit retune, per the "don't retune on one store" discipline.

## The forks

### 1. `#127` — modality has zero adoption

**The finding.** _[SUPERSEDED — see Update above: the structural count is 5, not
zero; "zero" was a grep artifact.]_ Round 3 existed to be the first clean test of
working modality after the `#616` fix. Modal adoption is **low but nonzero**: 5
unprompted `modal` facts, all correct. The fix is correct by unit test and
lightly exercised in the wild.

**Why it matters beyond modality.** Causal representation shipped three phases:
rung-2 predicates (`caused`/`enables`/`prevents` vs `correlated_with`), the
`modal` array reified as meta-relations, and a `causal` recall section. The
calling model has reached for none of it unprompted. So the question is not "is
modality broken" but "does a model writing facts ever reach for negation,
counterfactuals, or intervention marking without being made to?"

**The fork.** Three honest options, and they are genuinely different bets:
- *Push harder* — strengthen the instructions, or have recall prompt for
  modality when a fact looks like a correction. Costs instruction budget, which
  is already the scarcest surface.
- *Leave it* — a capability with no cost when unused, waiting for a domain that
  needs it. But then stop counting it as shipped value.
- *Cut it* — reclaim the vocabulary and the instruction lines. Irreversible-ish,
  and the store already carries `wk_ext` vocab that would need handling.

**Blocked on:** a judgment about what Legend is for, not on effort.

### 2. `#71` — does `changes` supersede a plain fact?

Deferred twice (filed 2026-07-15, Round 2). `changes` supersedes only the
`current_<prop>` cache, which plain facts never populate — a fact writes
property `standing`, a change writes `current_standing`. So a value first set as
a FACT and later hit with `changes` leaves the stale fact live: a **silent
contradiction**.

**Why it is still open.** Half-fixing is worse than today: recognising a fact
for the `from`-fill without superseding it turns a visible phantom into a silent
contradiction. It needs a semantics call — does `changes` supersede `facts`, and
how does a multi-valued property disambiguate?

**Fresh evidence from 2026-07-18.** The maintenance pass found exactly this
shape live: 5 constraints carried a plain `standing: active` fact *alongside* a
correct `current_standing` cache, inert but contradiction-ready. 79 constraints
used the cache alone. So the failure is real, rare, and self-inflicted by the
model rather than by the design — which argues for instruction over machinery,
but that is the call to make.

**Blocked on:** you. This is the one genuinely waiting on a decision rather than
on someone doing the work.

### 3. `#66` — summary discipline, the last `#91` lever

`#91` is fixed on levers (a) section caps and (c) hook retune: the packet went
51KB → 13.3KB pretty and now arrives whole. Lever (b) is untouched and is now
**the largest remaining driver**: `overview` alone is 5.3KB for five active
entries, because those entries carry ~1KB summaries.

Two pieces of evidence collide here and should be resolved together:
- `#66` residual (b): a save-frame signal when a resubmitted summary exceeds
  ~280 chars. The instruction nudge shipped (`3bf188a`) and is **not holding** —
  the trial store has 290 summaries over the threshold.
- `#122`: the 280 threshold is **miscalibrated**. Splitting the five worst
  summaries into short cores plus children produced cores of 278–308 chars — a
  deliberately terse core stating a decision and its test result lands near 300.
  Four of five still flagged. Measured answer: ~400.

**Blocked on:** nothing. This is effort, once the threshold question is settled.

### 4. `#118` + `#122` — the audit's own precision limits

Both found by *using* the audit for real, not by reading it. One data point each
— enough to discuss, not enough to retune on.

- `#118` **`stale_open` conflates three states**: genuinely open, parked on
  purpose (`#111`'s summary literally says "PARKED on purpose"), and
  answered-but-saved-as-`question` (four usefulness assessments that each
  already contain their verdict). 6 of 15 flags were the latter two.
- `#122` **`bloat` count punishes its own documented remedy**: splitting moves
  the count 272 → 286 because the detail must live somewhere and every child
  clears the threshold too. What actually improved was distribution — max
  summary 4047 → 1405, and a recall of `trap spells` returns 306 chars instead
  of 4047. Count is the wrong metric; max or p90 is honest.

**Blocked on:** a second maintenance pass for more evidence. Deliberately not
retuned on one store.

## Audit precision scorecard (one real store, 823 elements)

Worth having in front of you before deciding anything about the audit:

| check | found | real | note |
|---|---|---|---|
| `phantom_close` | 1 | 1 | already hand-retracted; live count 0 |
| `status_fact` | 7 | 7 | incl. the trial doc §11 example verbatim |
| `prose_name` | 17 | 17 | independently surfaced `#615`'s wreckage |
| `stale_open` | 15 | 9 | 6 false — see `#118` |
| `orphan` | 10 | 0 | all inert; 7 are `ec197d7` residue |
| `near_dup` | 4→2 | 0 | 0 real across three runs |
| `bloat` | 272 | — | a practice signal, not a defect list |

## Settled — do not relitigate

- `#68` GO-deepen. The verdict is in.
- `#89` stale intent has diagnostic value; never auto-prune facts to match code.
- `#97` suspects are computed, judgments are human — no unrequested repair.
- `#103` a missing summary is not a defect signal (119 hits vs ~5 real).
- `#106` digits carry identity in name matching; differing digits veto a pair.
- `#113` the expensive audit check is also the least useful one.
- `#91` levers (a) and (c) — fixed and deployed to the repo.

## Pending action

Round 4 runs on `8fbeedc` and sees neither the capped packet nor `legend audit`
until the trial binary is re-pinned:

```sh
cp ~/Code/legend/legend ~/.local/bin/legend
# then ~/Code/alchamancer2/.claude/settings.json, SessionStart command:
#   ... recall '{"limit":16}' --pretty 2>/dev/null | head -c 20000
```

`legend init` never clobbers an existing `settings.json`, so the hook edit is
manual. Pre-maintenance store snapshot preserved at
`.trial-backups/alchamancer2-pre-maintenance-2026-07-18` (gitignored); the
`8fbeedc` binary was not kept because `git archive 8fbeedc` rebuilds it
byte-for-byte, which is what the build-aware replay already does.
