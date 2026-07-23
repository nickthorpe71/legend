# Design — literals-not-nodes (+ must_exist refs)

> **⚠️ VERDICT 2026-07-22 — PHASE 2 (`TERM_LIT`) DROPPED. Measurement below.**
> A free local analysis of the ingested SimpleQA store (`dump` → shape + degree,
> `scratchpad/analyze_store.py`) showed the literal fix targets the wrong thing:
> - **Scalars are ~15% of elements, ~11% of fact-objects, and 77% of all
>   object-values are degree-1** (avg degree ~2). They never dedup and are never
>   traversal hubs — they are *harmless leaves*. Literal-izing them is real surgery
>   (persistence bump, golden regen, forward-ref caveat, ~10-site blast radius) for
>   a cleanup of the one bucket that already costs nothing. **Keeping scalars as
>   elements is nearly free** — the "every scalar should still be an element"
>   position (Nick) is correct.
> - **The traversal hubs are ENTITIES + kind-labels**, not scalars: `Barack Obama`
>   (deg 1159), `Google Chrome` (1084), `person` (1288), `organization` (741) — all
>   must stay nodes. Literals wouldn't touch a single hub. The "scalar hairball"
>   worry was wrong.
> - **Real inflation, ranked:** (a) provenance overhead — **58% of all facts are
>   `src`+`source` meta-facts** (2 per content fact); (b) predicate sprawl (~5,973
>   distinct predicates, one-offs not reused); (c) section-title composites as object
>   values (`Bikini_Atoll: Trust funds and failed claims`).
>
> Phase 1 (`must_exist` refs) still shipped and stands. The rest of this doc is
> retained as the *original* (now-superseded) scoping.
>
> **RECALL-HARM PROBE (2026-07-22, `scratchpad/recall_harm.py`) — the real root:**
> recalled 5 SimpleQA head entities against the store and looked at what fills the
> frame. Findings:
> - **Provenance is INVISIBLE to recall** — `src`/`source` meta-facts appeared in
>   **0** of the frames (they're subject=`rel:` facts-about-facts; bands are
>   element-anchored). So the 58% provenance overhead is a *store-size* cost, **not
>   a recall-quality lever.** Ruled out.
> - **The answers are PRESENT but BURIED under extractor greed.** David Sweet has
>   **102 facts** (`date of birth: 1957-06-24` is fact #1, drowned by "tow truck
>   business began 1978", "number of grandchildren 4"…); John Williams has **144**
>   (every Grammy nom 1976–2026 as its own fact; the Hall-of-Fame answer is 1 of
>   144). The real over-extraction is **content-fact count per entity**, not
>   scalars, not provenance, not predicate identity.
> - **Cold recall on the polluted store times out (>110s)** — over-extraction also
>   destroys latency, not just precision.
> - **Bonus:** one "miss" (Jensen wheelbase, gold `2,845 mm`) is present as
>   `wheelbase: 112 in` — a **units/canonicalization** gap, not a retrieval miss.
>   True retrieval accuracy was understated by unit/format mismatches.
>
> **So the real lever = reduce content facts per entity** (ingest-side: extract
> fewer, salient facts — already ½'d the count in the subset re-ingest) **backed by
> F1 ranking + cap** (recall-side). Both about *content* facts. NOT this doc's
> substrate change.

**Status:** ~~scoped, not built~~ **Phase 2 dropped (see verdict above).** The *free,
permanent* version of the over-extraction fix (`retrieval-redesign.md` #3,
`session-2026-07-22-retrieval.md` #B): the paid extractor-prompt tightening is a
band-aid; this stops the reification at the substrate so the store *structurally
cannot* over-extract. From Sofia's DX review (`analysis-2026-07-21.md`). This was
believed the deepest lever — measurement redirected it.

## The problem, confirmed at the type level

`Term` (one relation slot's value) is `{u8 tag; u32 id}` with `enum { TERM_ELEM=0,
TERM_REL=1 }` (`legend.c:2108,2126`). **There is no literal type.** So every value —
`green`, `1957-06-24`, `2,845 mm` — becomes a minted *element node*:
`plan_slot_value` (`legend.c:4217`) parses `rel:<id>` → `TERM_REL`, and *everything
else* → `plan_ref` (`legend.c:4182`), which resolves-or-**mints** an element. That
single default is why the SimpleQA store hit **205 elements/page** (14,730 elements
from 72 pages), why recall neighborhoods are polluted, why F1 latency scaled with
pollution, and why 5 of the 7 audit checks (`prose_name`, `bloat`, `status_fact`,
`phantom_close`, `orphan`) exist — they detect the wreckage of reifying strings.

## The hard decision: when is a value a node vs a literal?

This is the crux, not the mechanism. Some fact objects are **scalars** (dates,
numbers, statuses) that should be literals; some are **entities** (`spouse: Todd
Cooper`, `planet: Pluto`) that must stay nodes so relational/multi-hop queries (F3)
can traverse to them. Legend can't cheaply know which. Three candidate rules:

- **(a) Literal-by-default** (Sofia's phrasing): value positions are literals unless
  the payload explicitly gives an element ref. **Rejected** — it turns `Todd Cooper`
  into a string, breaking every entity-valued fact and the F3 traversal that is
  Legend's differentiator.
- **(b) Reify-only-if-it-resolves-or-is-an-explicit-ref** *(recommended)*: a value
  becomes an element **only if it already resolves to an existing element** (the
  recall-before-save discipline already requires this) **or** is an explicit
  `{ref: "..."}` / `#id`. Otherwise it is a literal. Result:
  - `spouse: Todd Cooper` where Todd Cooper exists → `TERM_ELEM` (traversable). ✓
  - `date of birth: 1957-06-24` (no such element) → `TERM_LIT` — **no new node.** ✓
  - `status: green` (no such element) → `TERM_LIT`. ✓
  This stops the minting (over-extraction) while preserving existing-entity links.
  **Caveat — order dependence:** if Pluto is not yet in the store when `planet:
  Pluto` is saved, "Pluto" becomes a literal and won't link once Pluto is later
  minted. Mitigations: the ingester recalls entities first (already the discipline);
  and an optional "promote a literal to an element when a matching element appears"
  pass. Accept the caveat; it degrades to today's behavior only for
  forward-referenced entities.
- **(c) Type heuristic** (numbers/dates → literal, proper-noun → element): fragile,
  locale-specific; use at most as a tie-breaker on top of (b).

**Recommendation: build (b).** It needs no new caller syntax for the common case,
and it directly implements "stop minting nodes for values that aren't entities."

## The mechanism

> **Line numbers below are approximate and drift with every edit** (Phase 1 already
> shifted everything past ~`plan_ref`). The **grep target is authoritative**: `grep -n
> 'value.tag' legend.c` finds every blast-radius site; `plan_slot_value` /
> `plan_ref_ex` / the `VT_` enum (`enum { VT_EPEND, VT_RCONST, VT_RPEND }`, note the
> third) are the named anchors. Verify by name, not by number.


1. **Term:** add `TERM_LIT = 2`; when `tag == TERM_LIT`, `id` is a **string-arena
   id** (the interned literal), not an element id. Literals intern like everything
   else, so equal strings share an id → dedup and supersession keep working by
   `(name, tag, id)` comparison.
2. **Plan value tags:** add `VT_LIT` beside `VT_RCONST`/`VT_EPEND`
   (`legend.c:4223-4227`). `plan_slot_value` (only for **value** positions, never
   `subject`): if it resolves to an existing element or is an explicit ref →
   `VT_EPEND`/existing; else `VT_LIT` (intern the raw span). Apply materializes
   `VT_LIT` → `TERM_LIT`.
3. **`changes.to`** flows through the same value path → literal by default (kills the
   `changes.to`-reifies-prose papercut directly).

## Blast radius — everything that reads `value.tag == TERM_ELEM`

Each must handle `TERM_LIT` (grep `value.tag`):
- **Dedup** (`legend.c:2358,2398`): compare literals by interned id (already works if
  interned).
- **Current-value cache / supersession** (`2598-2620,4524,4590,4711`): compare/emit
  literal values; a literal `to` supersedes a prior literal.
- **Retrieval / traversal** (`3704,3760,3788`): these push `value.id` (an element)
  into the neighborhood — for `TERM_LIT` **skip** (a literal is not a node). This is
  a *win*: literals stop polluting neighborhoods (the whole point).
- **Frame rendering** (`frame_put_rel_attrs`, `frame_put_instance`, ~`6466/6725`):
  render a `TERM_LIT` as its string, not an element name.
- **F1 fact text** (`frame_rel_text`): render `TERM_LIT` object as its string.
- **Persistence** (`snapshot_serialize/load`, ~`7947/8049`): serialize the tag; for
  `TERM_LIT` the id is a string-table id (already persisted in the string table).
  Bump the snapshot version; old stores are all-`TERM_ELEM` and load unchanged
  (forward-compat).

## Sub-fix: `must_exist` reference positions — **SHIPPED as Phase 1** (see tracker)

*Historical framing; Phase 1 corrected it.* Of the three positions originally
grouped here, only **`resolves.o`** turned out to be a true must-exist invariant and
is the one that shipped: an unknown one now **errors with near-miss candidates**
instead of minting a phantom. **`changes.target` is NOT must-exist** — spec fixtures
f05/f08 legitimately mint the change target in one shot (a typo there is a future
*warning*, not an error). **`merge` operands were already** must-exist
(`resolve_precise_elem`). See the phase tracker for the shipped scope.

## Validation plan

Measure-first, like the rest of this session:
1. **Unit tests** (`legend_test.c`): a scalar-object fact mints **no** element and
   stores `TERM_LIT`; an entity-object fact (entity pre-saved) stays `TERM_ELEM`;
   dedup collapses two identical literal facts; supersession replaces a literal `to`;
   the frame renders the literal; `changes.target` to an unknown ref **errors** (with
   candidates) rather than minting.
2. **Determinism gate**: frames change (literals render differently), so the golden
   fixtures + corpus baselines (`tests/fixtures`, smoke/adversarial `inspect.py`
   baselines) **must be regenerated** — a real, expected cost; review the diff to
   confirm every change is a value-turned-literal, nothing else.
3. **Over-extraction metric** (the payoff): re-ingest one SimpleQA page (free-ish,
   or reuse `ingest_subset.py`) and confirm **elem/page drops sharply** (scalar
   values no longer nodes) while gold facts survive; re-run `measure_tight.py` for
   retrieval + latency.
4. **Retrieval regression**: entity-valued facts still traverse (spot-check an F3
   case); scalar facts still surface via F1 (it ranks the rendered `"pred: literal"`
   text).

## Risks / open questions

- **The reify decision (b)'s order dependence** — forward-referenced entities become
  literals. Decide whether the "promote literal → element" pass is in scope or
  deferred.
- **Golden/baseline regen** is unavoidable and touches many fixtures — budget for it
  and diff carefully (this is where a subtle bug hides).
- **Backward-compat**: existing over-extracted stores keep their value-nodes (no
  migration) — the cleanup only applies to *new* writes unless a migration pass is
  built (complex; defer). So the live trial store improves only going forward, or on
  a re-ingest.
- **Aggregation/counting** (`project_counting_out_of_scope`): literals are still
  distinct per fact, so "average N ages" still works (each is a distinct literal on a
  distinct relation) — confirm this holds.

## Build order / phase tracker

- [x] **Phase 1 — `must_exist` refs** *(DONE 2026-07-23)*. Added a `must_exist`
  mode to `plan_name_ref` (`plan_ref_ex`/`plan_slot_value` thread it) that errors
  with near-miss "did you mean" candidates instead of minting. **Scope corrected
  while building:** only **`resolves.o`** is a true must-exist invariant (you can't
  close a question that never existed). **`changes.target` is NOT** — spec fixtures
  f05/f08 legitimately mint the target to set a property in one shot (the phantom
  there is a typo footgun for a future *warning*, not an error). **`merge` was
  already must-exist** (`resolve_precise_elem`). Net: `resolves.o` phantom_close is
  now prevented at the source (test updated), gate green.
- [x] ~~**Phase 2 — `TERM_LIT` + rule (b)**~~ **DROPPED 2026-07-22** (verdict at
  top). Measured harmless; scalars stay elements.
- [x] ~~**Phase 3 — regenerate goldens; validate over-extraction drop**~~ **MOOT**
  (was Phase 2's validation).
- [ ] **Milestone** — re-pin the trial on a build with F1 (+ Phase 1). No longer
  waits on literals.
- [ ] **NEXT LEVER (decided by the recall-harm probe)** — **reduce content-facts
  per entity.** The probe ruled out provenance (invisible to recall) and confirmed
  answers are buried under 100+ facts/entity. Primary = ingest-side extraction
  restraint (already ½'d the count, `ingest_subset.py`); secondary = F1 ranking +
  cap robustness for 100+-fact entities. Separately: a units/format canonicalization
  pass (the Jensen `112 in` vs `2,845 mm` class of miss).

Loose ends to interleave (free, ~1hr): Elena's 3 science-doc corrections;
`apply_plan` non-reentrancy comment. Dropped: paid extractor re-ingest (superseded
by Phase 2).
