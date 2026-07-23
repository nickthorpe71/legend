# Design — mechanical backstop for prose values that mint element names

> **✅ BUILT & VALIDATED 2026-07-23 (`77da3ef`).** Reject at the `changes.to` plan
> site (`legend.c:~5300`): if the value would MINT a new element
> (`pl->pends[to_pend].existing == NONE_U32`) and its **normalized** length exceeds
> `g_aud_name_chars` (hoisted so the gate and audit share one threshold), fail with
> the new `ERR_PROSE_VALUE`, guiding the model to a short value + summary detail.
> **Validated on real trial data** (`scratchpad/validate_backstop.py`): 32/32 prose
> `changes.to` rejected, 26/26 legit passed, **0 false positives**; mint-only lets a
> resolving 124-char reference through. Gate green. A 2-reviewer adversarial pass
> shaped the build — corrections applied: check at the **call site** (not the shared
> `plan_name_ref`, which `changes.to` doesn't use — it goes through `plan_ref`);
> hoisted the threshold; added the error code to the enum + names + **fuzz allowlist**
> (the fuzz gate caught the missing allowlist entry).
>
> **Deliberately partial — validated at build, NOT yet in the wild.** Reviewer B
> showed it likely RELOCATES pollution (prose the model can't put in `changes.to`
> moves into the target summary — feeding `bloat`, 11× more prevalent — or leaks
> through the ungated `fact.o`, already carrying 10 prose values). So the Round-6
> gauge MUST track `fact.o`-prose + `bloat` alongside `prose_name`; a `prose_name`
> win that just moves the pollution is a wash, and then this gets dropped/rethought.
> Do NOT sell it as fixing packet growth (that's summary/element-count driven).

**Status:** ~~SPEC~~ **BUILT (partial, wild-validation pending Round 6).** The
evidence-decided fix for the Round-5 trial finding that the `changes.to`-prose
instruction nudge is dead. From `docs/alchamancer-trial.md` Round-5 gauge + memory
`#149`/`#150`/`#152`.

## The problem, measured in the wild

Every fact object, attribute value, and `changes.to` value **becomes an element name**.
When that value is a prose progress-note, it mints a sentence-long element — the
`prose_name` audit pathology. The Round-4 remedy was an **instruction-only** warning
(MCP tool text: *"to: short canonical name; becomes an element, never prose"*,
`legend.c:9856`; the save-tool prose caveat, `9769`). There is **no code-level check.**

Round 5 (binary `9d745ee`, which carries that instruction) measured on the live store:

- **`prose_name` grew 20 → 36** despite the warning (the round's one gauge — FAIL).
- **16 of 27 Round-5 `changes` (59%) set a `to` value over 120 chars** — 168 to 1124
  chars — on `build_status` / `current_state`. E.g. `build_status →
  "BUILT 2026-07-21 sim-side, ART IS PLACEHOLDER (she wears Aine's frames…)"` (685).
- These mint elements `#1016`, `#1030`, `#1047`, `#1066`, … each a paragraph.

**Verdict: soft signals don't work under load.** The model knows the rule and ignores
it. The escalation is a mechanical gate — the same pattern that worked for Phase 1
`must_exist` (reject-with-guidance beats warn-and-hope).

## The measurement decided the scope (2026-07-23)

A free scan of the trial store + journal (no risk; a store copy) settled the two open
design questions — threshold and scope — and corrected the first draft of this spec:

- **`changes.to` length distribution, all 62 across the trial:** legitimate short
  canonical values top out ~100 chars (`'tier is rarity: Common/Magic/Rare/Fabled/
  Mythic'` 47; `'v1 approved as-generated, no rerolls'` 36; `'SHIPPED 2026-07-14'`).
  **Every one of the 32 values >120 is prose narrative** — `build_status`/
  `current_state`/`standing` paragraphs (168–1093 chars) with the real short value
  ("BUILT", "FIXED", "APPROVED", "SUPERSEDED") buried at the front and the rest
  belonging in the summary. So at the `changes.to` position, **120 has ≈0 false
  positives.**
- **But it is a CONTINUUM, not a bimodal split** — values slide smoothly 3→1093, so
  there is no "natural" gap; 120 is a judgment line (chosen to match the audit).
- **Broad scope is unsafe.** The borderline-band scan of *element names* 90–160 chars
  is full of legitimate long strings the model uses on purpose: session logs
  (`#484 "Session 2026-07-15 with Nick: Shift was dead in every text field…"`),
  code-location pointers (`#499 "src/main.c HERO_LUNGE_*, src/view/render.c…"`,
  `#566`), Nick quotes (`#741`, `#767`). Those arrive as subjects, fact-objects, and
  attr-values — gating all value positions at 120 would false-reject real work. Hence
  the scope narrows to `changes.to` alone.

## The mechanism (recommended)

**At plan time, reject a value that would MINT a NEW element AND whose name exceeds the
prose threshold**, with a guiding error that points the prose to where it belongs (the
target's summary) and asks for a short canonical value.

Three load-bearing design decisions:

1. **Mint-only, never on resolve.** The reject fires only when the value would create a
   *new* element. A long value that *resolves* to an existing element is a legitimate
   reference (e.g. a real long entity name already in the store) and must pass. This is
   exactly the mint-vs-resolve distinction the plan phase already computes (the Phase-1
   `must_exist` work threads it) — reuse it, don't reinvent. Concretely: the check
   belongs where a value position is materialized to an element and the plan knows
   `existing == NONE_U32` (a mint). Grep anchors: `plan_slot_value` / `plan_ref_ex`
   (the value path Phase 1 touched, ~`legend.c:4221-4270`), and the `changes.to`
   planning path off `read_change` (`legend.c:1724`, `SubChange`).

2. **One threshold, shared with the audit.** Use the audit's `g_aud_name_chars` (120,
   `legend.c:9247`) as the single definition of "this is prose." Then *save-reject* and
   *audit-flag* are the same line: the gate rejects exactly what the audit would later
   flag, so the two can never disagree. (Most legitimate element names are < 60 chars;
   120 is generous headroom. The 16 offenders are 168–1124.)

3. **Scope: `changes.to` ONLY — NOT fact objects or attr values.** *(Revised after
   measuring — see "The measurement decided the scope" below.)* The instinct was to
   gate every value-minting position, but the trial data kills that: legitimate
   fact-objects and attr-values go long routinely (code-location pointers, Nick
   quotes, design descriptions), on a smooth continuum with no clean cut, so a broad
   gate would false-reject real work. `changes.to` is the one position that is
   *supposed* to be a short canonical status value and where >120 is essentially
   always prose. Gate it alone; defer the broader prose-object question (it needs its
   own analysis and is far riskier).

**Error shape** (mirrors Phase 1's `unknown_ref` with candidates):
`code: prose_value`, `at: changes[0].to` (or `facts[i].o`, `elements[i].attrs.<k>`),
message: *"value is N chars; a value becomes an element NAME — use a short canonical
value and put the detail in the target's summary."*

## Alternatives considered (and why not)

- **Louder frame warning (a `writes.warnings` field).** Strictly stronger than the
  instruction, but still advisory — and the model already sees `minted_elements` and
  ignores it. The trial evidence is that advisory signals fail here. Rejected.
- **Auto-redirect prose → the target's summary, keep the change.** Clean in spirit
  ("prose belongs in summaries"), but a `changes.to` still needs *some* short value,
  and auto-deriving a good canonical value from a paragraph is lossy/magic. The model
  must supply the short value — which a reject forces cleanly. Rejected as primary;
  possible future ergonomic layer.
- **Truncate silently.** Destroys information and hides the problem. Rejected.

## Risks / open questions (for reviewers)

- **False rejects in the live trial.** This ships to a store Nick is actively using; a
  reject stalls a save until the model retries. **De-risked:** the retro scan (below)
  found 0 legitimate `changes.to` over 120 across the whole trial, and the scope was
  narrowed to `changes.to` precisely because broader positions DO go legitimately long.
  Plus mint-only (references pass) and the model already handles `must_exist` rejects.
- **Does the model recover well?** A reject is only good if the retry produces `to:
  "built"` + a summary update, not a loop. Worth checking against a couple of the real
  Round-5 payloads (does the guidance lead to the right restructure?).
- **`from`/`to` event-shaped facts** (`legend.c:5154`) reuse the `to` slot name for a
  different purpose — confirm the gate keys on *value length at a minting position*,
  not on the literal slot name `to`, so event facts aren't caught.
- **Threshold bikeshed.** 120 matches the audit. If false positives appear, the honest
  move is to raise the audit threshold too (keep them equal), not to fork them.

## Validation plan (measure-first, on a trial-store copy — free)

1. **Retro false-positive scan — DONE (2026-07-23).** All 62 trial `changes.to`:
   **0 legitimate values over 120** (legit tops out ~100); all 32 over 120 are prose.
   False-positive rate at the `changes.to` position ≈ 0. (Broad scope was rejected here
   — see "The measurement decided the scope".)
2. **Would-catch scan — DONE.** All 16 Round-5 prose `changes.to` (168–1124) trip a
   120 gate; the 11 legit Round-5 `changes.to` (< 120) pass.
3. **Unit tests** (`legend_test.c`): a `changes.to` over 120 that would mint → rejected
   with `prose_value` at `changes[0].to`; a short `to` → passes; a long `to` that
   *resolves* to an existing element → passes (mint-only); same for a prose `fact.o`
   and a prose attr value.
4. **Determinism gate** stays green (new reject path is a parse/plan error; no frame
   change for existing clean payloads → byte-identical replay). Add a fixture if useful.

## Ship plan

This is the last piece before the trial re-pin. Bundle into **one deliberate upgrade**:
Phase-1 `resolves.o must_exist` + F1 ranking + the recall `query` field + **this prose
backstop**, then re-pin `~/.local/bin/legend` and open a fresh round whose gauge is
"does `prose_name` stop growing" — this time with a mechanical guarantee, not a nudge.
