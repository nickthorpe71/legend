# Design — ingest restraint: fewer content-facts per entity

**Status:** SPEC / NEXT. The evidence-decided lever from the 2026-07-22 recall-harm
probe (`docs/substrate-literals.md` verdict, `scratchpad/recall_harm.py`). Replaces
the dropped `TERM_LIT` substrate fix.

## The problem, measured

Recall answers are **present but buried**. In the ingested SimpleQA store
(`benchmarks/simpleqa/store/.legend`):

- **David Sweet: 102 facts.** `date of birth: 1957-06-24` is fact #1, drowned among
  "tow truck business began: 1978", "age when confinement began: 13", "number of
  grandchildren: 4".
- **John Williams: 144 facts** — every Grammy nomination 1976–2026 stored as its own
  fact; the Hall-of-Fame answer is 1 of 144.
- Cold recall on the resulting store **times out (>110s)**.

The over-extraction that hurts is **content-fact COUNT per entity**, confirmed not to
be scalars-as-nodes (harmless leaves), provenance (invisible to recall), or predicate
identity. With ~100 facts/entity and a ~10–15/band recall cap, the answer only
surfaces if ranking is near-perfect — a fragile bet.

## The central tension (name it up front)

You **cannot know at ingest which fact a future query will ask for.** SimpleQA asks
about birth dates *and* obscure trivia ("tow truck business began 1978" is exactly
the shape of a SimpleQA question). So blunt volume-cutting drops answers — already
observed: the earlier "drop the exhaustive clause" subset re-ingest halved the count
(205→106 elem/page) but **over-corrected**, dropping needed facts (qids 2932/2952).
The prompt even contradicts itself today:

- `prompts.py:11-12` — "record only the facts worth keeping. Quality over coverage."
- `prompts.py:52-54` — "record **every other checkable claim**… we do not know what
  will be asked. A dense page may yield many facts; that is fine."

The greed clause wins. So the spec's job is **cut count WITHOUT cutting information** —
which is possible because much of the sprawl is *shape*, not *content*.

## Lever 1 (PRIMARY, low-risk): multi-attribute consolidation

**Much of the 102 is one fact fragmented into many.** David Sweet's riding tenure is
6 flat facts:

```
{represented: Ancaster—Dundas—Flamborough—Westdale}
{represented Ancaster—Dundas—Flamborough—Westdale from: 2006}
{represented Ancaster—Dundas—Flamborough—Westdale until: 2015}
{represented: Flamborough—Glanbrook}
{represented Flamborough—Glanbrook from: 2015}
{represented Flamborough—Glanbrook until: 2021}
```

That is TWO facts, each with a `from`/`until` attribute. Same for every committee
chair (term / predecessor / successor split into 3–4 facts × 5 committees ≈ 20 facts),
shadow-minister role (6 facts), travel (to / date / approval / purpose — 5 facts).

**Legend already supports this.** A relation holds up to **5 attributes**
(`legend.c:2165 Attr attrs[5]`), and a fact payload already accepts extra
property:value pairs beyond `s/p/o` (`read_attrs` at `legend.c:1643`). Recall renders
all attrs of a relation as **one unit** (`frame_put_rel_attrs`), so a consolidated
5-attr fact occupies **one** recall slot instead of six competing ones. The only thing
blocking it is the ingester prompt defining a fact as single-atomic-object
(`prompts.py:15`).

**Estimated effect on David Sweet:** ~102 → ~40–50 facts, **zero information lost**,
and the surviving facts recall as coherent units (riding-with-dates, role-with-term)
instead of fragments that bury each other.

**Change surface:** ingester-prompt only (emit `{s,p,o, attrA:…, attrB:…}` when
attributes qualify one predicate on one subject). NO Legend code change, NO golden
regen (goldens are separate fixtures; the smoke corpus already exercises multi-attr
relations). Deterministic gate unaffected.

## Lever 2 (SECONDARY, higher-risk): volume restraint

After consolidation, entities with many *genuinely distinct* facts (John Williams's
144 awards) still sprawl. Options, worst-to-best:

- **(a) Hard per-entity fact cap at ingest** — rejected. Arbitrary; drops answers;
  order-dependent (which N survive depends on chunk order).
- **(b) Salience filter at ingest** — keep only "checkable, entity-defining"
  facts. This is the prompt's line-11 intent; the risk is exactly the SimpleQA
  tension (an "unimportant" fact is someone's question). Resolve the prompt
  contradiction toward restraint *only for derivable/enumerable* classes (every
  individual Grammy nomination → one summary fact "27 Grammy wins" + keep the notable
  ones), never for singleton facts.
- **(c) Roll-up facts for enumerations** — when a page lists N homogeneous items
  (every nomination, every election opponent), emit one aggregate fact
  (`Grammy nominations: 76`, already present!) and keep only the *notable* members
  (wins, firsts, records). This is the honest middle: enumerations compress, singletons
  survive. Aligns with `project_counting_out_of_scope` (the consumer counts; Legend
  keeps distinct facts) — here the aggregate IS the distinct fact.

**Recommendation:** ship Lever 1 (pure win), measure, then apply Lever 2(c)
(enumeration roll-up) only where it demonstrably doesn't drop an answer.

## Recall-side complement (not a substitute)

Consolidation shrinks the pile; it does not remove ranking's job. After Lever 1, an
entity may still hold 40+ facts. F1 (query→fact relevance, shipped `f7b001f`) is the
mechanism that lifts the asked-for fact; the per-band **cap** (currently ~10–15) is
the other knob. Spec item: re-measure whether the cap should scale with neighborhood
size once consolidation lands (a 40-fact entity may need a larger cap than a 5-fact
one). Do NOT raise the cap as a substitute for consolidation — that re-introduces the
latency blowup.

## Revision & determinism considerations (for the reviewers)

- **Revision granularity:** if `until: 2021` on a multi-attr fact must later become
  `2023`, can `changes` target a single attr, or must the whole fact be
  re-asserted? Check `changes {target, property, to}` resolves to an attr slot, not
  just a whole relation. If it can't, consolidation trades recall clarity for revision
  friction — quantify before committing.
- **Dedup/supersession:** two facts that consolidate to the same (subject, predicate,
  attrs) must still dedup; a multi-attr fact with a differing single attr must NOT
  false-merge with its sibling. Confirm the canonical-key comparison
  (`legend.c:2358,2398`) includes all attrs.
- **Prefer prompt-side over Legend-side auto-merge:** doing consolidation in the
  ingester (LLM emits richer facts) needs no code and no golden regen. A Legend-side
  "auto-collapse same-subject-same-predicate facts" pass WOULD change frames → golden
  regen → defer unless the prompt approach underperforms.

## Validation plan (measure-first, reuse existing harness)

1. **Consolidation prompt** → re-ingest one page (`ingest_subset.py`, ~$1–2 or free
   if cached), `legend dump`, confirm facts/entity drops ~2x with the gold facts still
   present (re-run `scratchpad/analyze_store.py`).
2. **Answer-survival** (the real metric): re-run the recall-harm probe
   (`scratchpad/recall_harm.py`) on the consolidated store — do the buried answers
   (David Sweet DOB, John Williams 2004) now surface in the top-15?
3. **Latency:** `measure_tight.py` warm + cold recall — did the >110s cold-recall
   timeout resolve?
4. **No-regression:** the deterministic gate (`check.sh`) stays green (prompt change
   touches no C); smoke/adversarial baselines unchanged.
5. **Only if 1–2 pass:** apply Lever 2(c) enumeration roll-up, re-measure that no
   SimpleQA answer was dropped.

## Out of scope (tracked elsewhere)

- **Units/format canonicalization** — the Jensen `112 in` vs gold `2,845 mm` class of
  miss. Real, cheap, but a *distinct* lever (a normalizer, not an extraction change).
  Separate doc when picked up.
- The dropped `TERM_LIT` substrate fix (`docs/substrate-literals.md`).
