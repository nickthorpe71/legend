# Fix roster — 2026-08-01 (post-review)

## PHASE 1 SHIPPED 2026-08-01 — four commits, `check.sh` green on each

| commit | item | what changed |
|---|---|---|
| `e6973ae` | C1 / N1 | 2-word cap on `elements[].kind`. Measured first: all 18 kinds in real use are exactly one word, so a word cap is the right shape — `must_exist` would have rejected `spell` on first use. |
| `78416af` | C3 | A constraint's supplied `standing` seeds its `current_standing` cache instead of being written beside it. Removes the source of all 12 `status_fact` suspects. |
| `bbc9c8c` | C2 / N2 | The pin-25 heal supersedes a single prior, not a whole value set, and never touches `rel:0..9`. Closes the multi-value loss (57 groups / 139 facts) and the one-save ontology wipe. |
| `f76d3e6` | C2 / N3 | A plain fact can no longer be written into the `current_*` cache namespace, at all four payload predicate positions. |

Corpus and adversarial baselines matched throughout — no fixture regen was
needed. Backward compatible: a copy of the live trial store loads clean and the
pre-existing defects stay readable, since these are all save-path guards.

**Not shipped from C2:** the `missing` half of `plan_check_drift`. It is a soft
signal, and this roster's own thesis predicts soft signals do not land; it is
worth having but only alongside a gauge that tests whether it gets read.

**C2 was built smaller than reviewed.** The proposed `WK_TEMPLATES` cardinality
bitmask was dropped in favour of counting live priors in the store: the count is
measured rather than declared, so it also covers properties no template lists —
which is exactly the `expects` case that produced the ontology wipe.

## PHASE 2 SHIPPED 2026-08-01 — three commits, `check.sh` green on each

| commit | item | what changed |
|---|---|---|
| `401b82a` | R6a | Hebbian spec committed with a `SUPERSEDED` banner (same handling `3d2191f` gave the killed ingest-restraint spec); `.gitignore` widened to `benchmarks/simpleqa/store*/`. The roster's "gitignore **or delete**" was wrong — that directory is a 3.0 MB store `measure_tight.py` reads, built with paid ingest. Not deleted. |
| `ade943b` | R4a | `legend audit` emits `prose{summaries,chars,max,p90,mean,over}` — `#136`'s explicitly-open metric half. Threshold stays 400; the 600 raise was dropped as relitigating `#122`/`#153`. Documented in `docs/cli.md` with the false-alarm worked example. |
| `00a0d52` | R5 | `graph_sync` compares the journal's last `build` to its own on a reload it did not cause, and puts a mismatch in the frame as `foreign_build`. |

R5 was **relocated, not built as specified.** The roster's start-time check is
blind to the hazard: the stale server's build *matched* the store when it
started, and the mismatch only appeared 1.3 days later at the re-pin. It would
also have false-positived on every legitimate upgrade. Verified end to end using
the previously pinned binary as a real foreign writer.

Measured on a copy of the live store, the new metric makes the R4 argument
concrete: `bloat: 108` alongside `max 815 · p90 543 · mean 337`.

## Still pending

Phase 3 (live-store repair of the 4 clobbered kinds and `#602`'s duplicate
cache) must run against the re-pinned binary. Phase 0's baseline and round-open
come **before** the re-pin. C4, C5, `changes.target`, the predicate
consolidation and the `#110` near-dup call are the round after.


**v1 was written by the agent that diagnosed most of it. Four independent
adversarial reviewers then read the code and measured the stores. Most of v1 did
not survive.** This is the corrected roster; v1's dead items are preserved below
with the evidence that killed them, per the `docs/ingest-restraint.md` /
`docs/substrate-literals.md` house pattern.

Review lenses: (1) does each item need fixing, (2) would each fix work, (3) what
consolidates, (4) what does this relitigate. Findings that ≥2 reviewers reproduced
independently are marked **[converged]**.

---

## What v1 got wrong

| v1 claim | reality |
|---|---|
| R1: "the trap arms when a constraint is lifted" | **False [converged ×3].** `plan_heal_plain_attrs` (`legend.c:4579-4643`, called at `:4748`) already supersedes the plain fact when a change flips the property. Reproduced end-to-end in fresh stores: `status_fact` goes 1→0 on its own. |
| R2: "zero rejections observed" | **False [converged ×3].** 11 rejections in 82–84 saves (13%): `prose_value` ×8, `unknown_ref` ×2, `parse` ×1. The "0" was a Round-7 figure meaning *the backstop was never exercised* — recorded as absence of evidence (`alchamancer-trial.md:929-931, 961-963`), cited as proof of the opposite. |
| R2: "all 8 are named `<X> decision`" | **False.** None are. The roster's own audit-dump script printed `ref, name, kind` unlabelled; the author read the `kind` column as part of the name. |
| R2: the 8 decisions are flat | **False [converged ×3].** All 8 carry content relations. `#510` and `#554` record `selected:` alternatives outright. `dec_content` only counts `chose\|rejected\|about\|resolves` (`legend.c:9635-9657`) and ignores `selected`/`justifies`/`applies_to`. |
| R7: "83 orphans", "kills ~29" | **Wrong framing [converged ×4].** 149–150 today, and `legend audit` reports orphan **0**. The viz number counts `Relation.supporters` (a `U32Vec`, `legend.c:5606-5612`) as unattached. The 2026-07-28 decision itself recorded "36 on-screen orphans = ZERO orphaned content." |
| R11: "three candidates" | **Misread tool output.** The top cluster is 31 elements / 92 links, not the 3-element triple. The author quoted the first three names printed. |
| R8: "two agents from opposite directions" | **Overstated.** `#157` and `#159` are both legend-dev agents reading alchamancer transcripts — same evidence channel, different sessions. |
| Thesis: "every check fixed in code went to zero" | **Partly an artifact.** `audit_mark_structural` (`legend.c:9399-9419`) excludes all provenance from every check; the store holds 65 over-5-word provenance names the `f83b436` cap never sees. And see N1 — clobbered kinds remove elements from every kind-keyed check. **Every count in v1 is a floor.** |

---

# NEW — three defects nobody had listed

These outrank most of v1.

## N1 · `kind` is an unconstrained reference position, and `ddb9f7b` made it destructive

`legend.c:5099-5101` — `kind_pend = plan_ref(...)` with no `must_exist` and no
kind-vocabulary check. The 5-word cap (`:5091-5096`) applies **only** to
`elements[].name`. Any string becomes a kind; a 9-word claim was minted cleanly in
test.

Since `ddb9f7b` added `plan_supersede_old_kind` (`legend.c:4648-4674`), a bad kind
no longer accumulates — it **overwrites the correct one**:

```
save {"name":"Bio Weapon","kind":"spell"}                      → instance_of: spell
save {"name":"Bio Weapon","kind":"nothing resolves on cast"}   → instance_of: spell SUPERSEDED
```

Four live elements carry a claim or another element's name as `instance_of`:
`Bio Weapon`→`nothing resolves on cast`, `Winter's price`→`costs are an open
space`, `Nice Try`→`the mask layer`, `the family photo album`→`the Hunter x Hunter
register`.

**This silently disables the audit.** `flat_decision` keys off
`g->elem_kind[e] == WK_KIND_DECISION` (`:9693`); `status_fact` and the recall
`constraints`/`decisions` bands key off kind too. A clobbered kind removes that
element from every check.

**Fix:** wire the existing `must_exist` plumbing (`legend.c:4204`, `4238`,
`stage_must_exist_cands` at `:4172`) into `elements[].kind`, constrained to
elements that are kinds; extend the 5-word cap to the slot. Cost **S**; the route
is proven — it is what took `phantom_close` and `orphan` to 0.

## N2 · Supersession has no cardinality model — silent multi-value loss

`plan_heal_plain_attrs` is cardinality-blind. `WK_TEMPLATES` (`legend.c:2826-2858`)
stores `expects` as attribute *names* only (`u8 expects[5]`, `:2822-2823`).

```
constraint applies_to [the editor, the sim, the view layer]
changes {property:"applies_to", to:"the audio engine"}
→ all three superseded; recall reports one scope
```

Live blast radius: **57 multi-valued (subject, predicate) groups covering 139
facts** (`applies_to` 17 groups, max 6; `part_of` 8; `enables` 6; `caused` 5).

Worst case, the ontology is unprotected — `retract` guards it (`legend.c:5457`)
but the heal path bypasses that guard:

```
changes {target:"decision", property:"expects", to:"vibes"}
→ rel:0..rel:4 ALL superseded — the decision template, gone in one save
```

## N3 · `current_*` written as a plain fact bypasses supersession *and* the audit

`audit_name_is_status_flavored` (`:9426-9430`) exempts the `current_` prefix on the
theory that it is the cache `changes` wrote. Nothing stops an agent writing
`current_standing` directly as a plain fact. Live, right now:

```
#602 the brake count
  rel:2071 asserted {current_standing: active}
  rel:2096 asserted {current_standing: settled}
```

Both surface in the `state` band — the highest-authority read surface — and
`legend audit` reports **all checks clean**. The audit flags the harmless copy
(v1's R1) and exempts the harmful one.

---

# The consolidated roster

## C1 · Close the reference position `must_exist` never reached
**Subsumes N1, part of R2, the long-deferred `changes.target` phantom.**

`must_exist` was built and wired to `resolves.o` only (`legend.c:5222`) — which is
exactly why `phantom_close` and `orphan` are 0. It stopped one slot short of the
slot that matters most. Wire it into `elements[].kind` now; hold
`changes.target` (`:5364`) for a later round — that half legitimately mints in
fixtures `f05`/`f08` and needs a `new: true` escape or a warn-only first pass.

**Cost** S · **Risk** M (rejects writes) · **Ship first** — every audit count is a
floor until it lands.

## C2 · Give `WK_TEMPLATES` cardinality + a required-set
**Subsumes R1 (retargeted), R2, N2, N3.**

Add `u8 single` and `u8 required` bitmasks beside `expects[5]` at `legend.c:2822`.
In the C struct, **not** as graph attributes — so no seed-id shift and replay
determinism survives. Three consumers:

- `plan_heal_plain_attrs` heals only single-valued properties → kills N2 and the
  ontology-wipe hole. Failure mode becomes under-healing (visible) rather than
  over-healing (silent).
- `plan_check_drift` (`:4802-4870`) reports `missing` alongside `unexpected` →
  **kills R2 without a hard error.** Verified: a decision with no `chose`/`resolves`
  currently produces `template_drift: []`. `expects` is a one-way lint; this is its
  missing half.
- A single-valued plain-fact write to a `current_*` name routes through
  supersession → kills N3.

**Cost** M · **Risk** M (supersession semantics; needs corpus + fixture regen).
Ship as a **separate commit from C1** — C1 is a proven resolution-path change, C2
carries corpus risk; bundling gates the safe fix behind the risky one.

## C3 · R1, retargeted — the auto-mint must honor the caller
The real bug next door to v1's R1: `legend.c:5163-5183` hardcodes `active` and
ignores a supplied `standing`.

```
save {"name":"dead rule","kind":"constraint","attrs":{"standing":"retired"}}
→ rel:10 {standing: retired}   rel:11 {current_standing: active}   ← contradicts at mint
```

`constraint_is_active` (`:7004-7036`) reads only `current_standing`, so a
constraint declared retired renders as live in the recall `constraints` band.

**Fix (~15 lines):** in the `elements[].attrs` loop, when the key resolves to
`WK_STANDING` on a new constraint, capture the value pend, skip the plain
`plan_relation`, and feed the pend into the auto-mint block in place of `"active"`.
No new event, no derivmeta, no walk-order shift.

**Do not** implement v1's R1 as written — reviewers showed `plan_curov_push`
(`:4559-4568`) is bookkeeping only; the work is in `plan_change_state` (`:4692`),
which requires an `event_idx`, and passing `NONE_U32` indexes `pl->rels[0xFFFFFFFF]`
out of bounds.

## C4 · One actuation seam, two call sites (was R3 + R8)
Both are the same shape: a computed set with no consumer.

**R3 shrinks to a comparator change.** The `open` section already exists
(`legend.c:7891-7893`), capped at 10 (`:6788`), sorted **newest-first**
(`elem_recency_cmp`, `:7657`). The packet reports `"omitted": {"open": 11}` and
those 11 omitted *are* precisely the stale set. Reserve slots for the oldest, or
sort by `clock - last_seen`. No new section, no `FRAME_RESERVED_KEYS` edit. The
budget worry was unfounded — measured packet is **9.8–10.3 KB** against a 20 KB
valve, and every section is capped so it does not climb with store size.

**Gated on `#118`** (see Missing, below) — 30–40% of the current stale set is
parked-on-purpose or answered-but-still-`kind=question`, and pushing those is a
standing nag about undecidable questions.

**R8 is not XL.** The `constraints` band already returns the exact rule on a plain
focus. But naive scoping is dead on arrival — matching any name in the payload
yields **median 13 constraints per save**; restricting to `elements[]` declarations
yields **median 0, mean 1.1, fires on 30/84**. That is the scoping to prototype.

**R8's cited evidence does not support it** and the roster must say so: both misses
were knowledge Legend did not hold, and the `"Subtle"` word went into a *file*, not
a save — a save-path field could not have caught it. That class needs a
`PreToolUse` hook. Build R8 only as an instrumented experiment with a
**pre-registered gauge**: does the warning fire on a write a later constraint would
have caught?

## C5 · R7, re-scoped and demoted
Real: folding `source` metas onto the statement takes relations **2161 → 1301
(−39.8%)**, mean arity 2.03 → 2.69. Provenance is **48%** of relations (860
`source` + 160 `src` + 17 `derived_from`) from only 64 distinct source strings.

But: **zero measured retrieval benefit.** Provenance appeared in 0 of 5 recall
probes (`sources: 0` on every one) — independently reproducing the 2026-07-22
probe that dropped literals-not-nodes. And one live blocker no one had noticed:
folding `source` onto the statement **destroys `support_diversity`**, since the
dedup key is the attr set — `{s,p,o,source:A}` and `{s,p,o,source:B}` would stop
folding into one relation with two supporters, and `support_diversity >= 2` is a
promotion gate (`legend.c:2165-2166`, `5931-5932`). Also `changes` already consumes
all five slots (`:5410-5430`), so folding onto a change statement is impossible at
arity 5.

**Bill it as store-size and graph hygiene. It subsumes nothing. Do not lead with
it.** Cost L, and the arity bump — not persistence — is the L (the snapshot format
is already length-prefixed, `:8132`, `:8537-8540`).

---

# Survives as one-offs

- **R4a — emit `max`/`p90`/total-prose in the audit.** This is `#136`'s explicitly
  open "metric half." Ship it. **R4b (400→600) is DROPPED** — see below.
- **R5, relocated.** Evidence is far stronger than v1 stated: the stale image
  served **60 journal lines over 2.5 days**, ~27.5% of post-re-pin saves, with 17
  build alternations. But a **start-time check fires backwards** [converged ×2] —
  the stale server's build *matched* when it started, and the check would false-
  positive on every legitimate new server. Put the probe in `graph_sync`'s reload
  branch (`legend.c:10386`), which fires exactly when another process wrote. Open
  question: `mcp-serve` speaks JSON-RPC on stdout and stderr is unread, so the
  warning must be a frame field.
- **R6a — hebbian `SUPERSEDED` header + commit.** Matches `3d2191f`'s handling of
  a killed spec.
- **R9, cheap.** `harness/round_report.py:159-192` already does what v1 proposed;
  cost is one command, not "M". Reviewers ran it: **75–85% surfaced vs Round 7's
  77% — the distribution did not degrade**, and there is no tension with Round 7
  (different store, pre- vs post-wipe). `resolved:false` on a prose prompt is *by
  design* (tier 1 is normalized-exact only, `:2909-2916`) — the testimony read
  designed behavior as failure. **The real finding is elsewhere:** every miss list
  is exactly `TIER2_LIST_CAP = 100` (`:3267`) with a median of **8** nonzero
  scores — 92% zero-score padding. The ambient hook's `head -c 1500` hides it; the
  **MCP path does not truncate**, so `legend_recall` returns ~6.8 KB whose scored
  content is ~640 bytes. Fix the payload, not the dial.
- **R11 — one candidate, not three.** The `C/Rust/SDL` and spell clusters have
  shared parents and need no judgment. The 31-element no-shared-parent blob is a
  hub-cap artifact. The three 99-hop pairs between major systems (`the duel`,
  `the audio engine`, `the spell tree`, `board mana`) are real disconnected
  components — a structural fact, not a judgment call.

---

# DROPPED

- **R2 as written.** Contradicts `#103` ("absence of description is a style
  observation, not a defect" — on the *do-not-relitigate* list). Would have
  rejected **31 of 55 real decision writes (56%)** and **8 of Legend's own 16
  decisions**, including `#89`, `#103`, `#106`, `#108`, `#113` — four of them
  settled entries. `#103` itself could not have been written. It is also stricter
  than the check it enforces (`chose|resolves` vs the audit's four). Rejects are
  payload-wide: `fail()` (`:339-378`) kills every element in the save.
  **Replaced by C2's `missing` drift signal**, plus a read-only `dec_content`
  vocabulary pass.
- **R4b, 400→600.** Relitigates `#122`→`#136`→`#153`. `#122` already measured the
  answer ("a terse core lands ~300 … **Suggest ~400**"); `#153` closed it as "not
  being tuned further." v1's new data measures *what the model writes*, not what a
  terse core costs, so it does not refute the calibration. And bloat is already
  zeroed in the session tally (`:9851`) — the "false alarm" came from a deliberate
  `legend audit` call, so raising the line only makes a hand-pulled diagnostic read
  nicer.
- **R10's "close recipe 114/115."** Contradicts `#97`, `#89`, `#118`. Their own
  summaries read *"Open design slot … **No direction is chosen**"* and *"has not
  chosen it."* A placeholder was implemented; the design slot is deliberately open.
  Closing them asserts a choice Nick has not made.
- **R10's `standing` retracts.** Unnecessary — self-healing (see C3). The genuine
  live repairs are **the 4 clobbered kinds** and **`#602`'s dual `current_standing`**,
  neither of which v1 listed.
- **R6b's "gitignore or delete `store_tight/`."** It is not "one stray AGENTS.md" —
  it is a **3.0 MB initialized store** that `benchmarks/simpleqa/measure_tight.py:51`
  reads as a live input, built with paid OpenAI ingest (precedent: that budget
  already overran $38→$64). **Gitignore only, never delete** — mirror
  `.gitignore:105`'s `benchmarks/conflictqa/store*/` pattern.

---

# MISSING from v1 — add these

1. **`#118` stale_open conflates three states.** Open, unfixed, and a
   *prerequisite* to C4's R3 half. Its recorded blocker is "a second maintenance
   pass for more evidence" — this roster **is** that pass, and the three-state split
   still holds on the fresh store (`#254` "Parked, not killed", `#253`
   "deliberately", `#250` "not been played enough yet", `#246` "Being tested now").
   A parked/answered marker comes first.
2. **`#66` summary bloat as the *retrieval* ceiling.** Recorded twice as the next
   real lever (Round 7 close; `project_ambient_hook_findings`): bloated summaries
   embed diffusely → weak semantic matches. v1 has a bloat item framed purely as
   metrology and an ambient item framed as measurement, and never joins them. This
   is the highest-value recorded lever and it was not on the list.
3. **Watch #1, predicate proliferation.** **79 distinct predicates, 34 single-use**
   in 100 ticks on a 626-element store. `decided_by` (49) and `selected` (5)
   duplicate seeded `chose`/`about` semantics — and this is the direct cause of
   R2's false positives. `project_literals_dropped` already recorded provenance +
   predicate sprawl as the real inflation; C5 covers only the provenance half.
4. **`#110` near_dup** — open, needs Nick's recall-vs-precision call. The current
   single hit (`wire the spell library` vs `the spell library`, 0.75) is the most
   plausibly-real one the check has produced.
5. **No round is open and no gauges are pre-registered.** Round 7 closed
   2026-07-27. Every prior re-pin was a round boundary with pre-registered gauges
   (playbook step 0, `harness/round_report.py`). Shipping a bundle without one means
   nothing measures whether it worked — which is this roster's own thesis about why
   instruction fixes failed unnoticed. **Open the round before the re-pin.**
6. **`#137` correlated_with invariant** — still 0, but v1 carried no invariant
   checks at all.

---

# Recommended first bundle

**C1 (kind half) → C3 → C2 → R4a → R5(relocated) → R6a**

C1 first: a clobbered kind silently removes elements from every kind-keyed check,
so every number here is a floor until it ships. C3 is ~15 lines and fixes a live
contradiction. C2 then retires v1's R1 and R2 together while closing two
silent-data-loss paths. R4a/R5/R6a are XS and independent.

Hold for the round after: `changes.target` (C1's second half), C4 (needs `#118`
first and a pre-registered gauge), C5 (cost L, no retrieval payoff, and the
`support_diversity` blocker needs an answer).

**Open a trial round with gauges before any of it ships.**

---

## Review note

One reviewer ran a single non-`observe` `legend recall` against the live trial
store before switching to a scratch copy, appending journal line 307 and ticking
the clock 99→100. No content mutation, no save. Recorded here because trial-store
contamination has a precedent (`#61`) and the journal should not carry an
unexplained legend-dev tick.
