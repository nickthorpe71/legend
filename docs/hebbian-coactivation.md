# Design — Hebbian co-activation (associative edge formation)

> **⚠️ SUPERSEDED 2026-07-28 — the PERSISTED TABLE was killed; the CLUSTER framing
> shipped instead.** A 3-reviewer adversarial pass rejected the design below on
> three grounds: a decayed, hebb-bumped table is new persisted state on the
> determinism surface (byte-replay is a gate, `check.sh`); the recall-side signal
> was dead (co-activation never reached ranking); and 71% of the pairs it would
> have surfaced were already siblings under a shared parent, i.e. structure the
> graph holds.
>
> What survived is the **cluster** reading — dense co-firing SETS flagged
> `shared_parent: NONE`, which is a real "is a parent concept missing?" question.
> It shipped as **`harness/coactivation.py`** (`e8bd0a3`): read-only, journal-
> replayed with `observe:true`, name-matched so it never writes, no persisted
> table and no determinism surface. Run it against the live store as organic
> saves accumulate.
>
> Kept for the model and the measurement below, which the miner still uses. Do
> not build the table.


**Status:** designed, not built. Shaped by Phase 0 measurement (2026-07-28) on the
archived longitudinal store. Companion to `new_foundation.md §11.11`.

## The gap this fills

§11.11 (built: `dyn_reinforce_focus` + `hebb_bump`) reinforces an edge that
**already exists** — focus A and B together, their connecting relation's
activation bumps. What's missing, and what this specs, is **forming** a weak
associative edge between elements that co-activate repeatedly but have **no**
relation yet — so Legend can eventually ask the LLM: *"these keep firing together
and aren't linked — should they be?"*

**Direction (Nick, 2026-07-28):** keep it **out of the reality graph** — a
separate table, so these are unmistakably associative, never confused with
asserted facts. **Surface the weighted neighborhood** and let the **LLM** reason
about what relation(s) exist. Never auto-link.

## The model: a weighted co-activation graph

A separate table of weighted edges between elements. Edge `{A,B}` carries a
`strength ∈ [0,1]` (hebb-bumped on co-fire, decayed otherwise) and `last_event`.
**Edges are only the storage substrate; the surfaced and reasoned unit is always
the weighted _neighborhood_ (subgraph), never a bare pair.** This dissolves the
pairs-vs-groups question: the LLM does the grouping from the weighted data —
frequent-itemset detection handed to the model, not solved in C.

## Sources — what "firing together" means (Phase 0)

A co-activation **event** is the set of content elements brought together in one
tick. Two sources, both funneled through the same filters + accumulation:

1. **Recall focus sets** — the resolved `focus_elems` of a **deliberate
   (`observe:false`) multi-focus recall**. (Ambient `observe:true` recalls don't
   persist and are ~single-focus; they don't contribute — acceptable.)
2. **Save co-mention sets** — the content elements referenced in one save
   (element names + fact `s`/`o`) that the save did **not** relate.

**Phase 0 evidence:** recall alone is sparse (75 multi-focus events → 52 usable →
only **13 recurring un-related pairs**). Save co-mention is **15× richer** (**156
recurring**, kind-filtered) and semantically apt ("written together, never
connected"). So the firing event must include **both**, not recall alone.

## Filters (Phase 0 — mandatory, not optional)

Before forming/bumping edges within an event's element set `S`:

1. **Already-related** — drop any pair already sharing a real relation (never
   associate the connected). *(Nick's guard.)*
2. **Kind-filter** — exclude bookkeeping/temporal kinds `{event, task, question,
   pointer, commit, reference}` and no-kind nodes (predicate + provenance
   elements — the C2 noise set). They co-fire with everything in their session.
3. **Hub-filter** — exclude super-hubs: the project node and any element whose
   degree exceeds a cap. Unfiltered, `alchamancer2 ↔ X` dominated the list —
   everything "associates" with the project.

## Dynamics

- **bump:** on co-fire, `w = hebb_bump(w, coact_rate)` for every surviving pair in `S`.
- **decay:** `w = hebb_decay(w, coact_decay)` so one-offs fade.
- **prune:** drop edges below a floor — "dies if it only happened a few times."

## THE DECAY MODEL — the hard part (flagged for review)

Co-activation events are **sparse and spread across many ticks** (75 recalls over
weeks). If decay is **per-tick** (every recall/save), a recurring-but-spread pair
(co-fires at events 5, 40, 70) decays to nothing between its co-fires and never
accumulates — the whole layer under-fires. Options:

- **(a)** decay per co-activation **event** — a time step is a co-fire event, not
  a tick. Recurrence-preserving; decouples from tick/wall time.
- **(b)** very slow per-tick decay (half-life ≫ typical inter-event gap).
- **(c)** lazy decay on access, indexed by **events-since-`last_event`**.

**Recommendation: (a)/(c)** — the association's clock is co-activation events, not
ticks. **This is the make-or-break design choice; reviewers should attack it.**

## Surfacing (the maintain/audit channel)

A periodic check — scoped to a **transient dirty set** of recently-co-fired
elements (efficiency; Nick's point) — finds edges past a `surface_threshold` and
emits the weighted **neighborhood** via the **audit/maintain surface** ("computed
suspects, the LLM/human adjudicates" — a perfect fit), **not** the
orientation packet (bloat-sensitive). Format:

```
association suggestion:
  A ↔ B co-activate strongly (0.90); also co-fire with C(0.55), D(0.50), E(0.42).
  None are related. What relation(s) connect them?
```

Re-check already-related **at surface time** — a relation may have appeared after
the edge formed → retire instead of suggest.

## Retirement

When the LLM adds a real relation between A and B that carry an associative edge:
**retire the edge**, and **transfer its accumulated strength** into the new
relation's `activation`/`salience` — the association's history bootstraps the
explicit edge. *(Nick's "keep tracking the strength.")*

## Persistence & determinism (HARD constraint — the trial gate)

The trial requires **byte-identical journal replay**. The table is persisted
state; its evolution must be **deterministic** — pure arithmetic over inputs in a
fixed order, no map-iteration-order or float-order sensitivity. The table lives in
the snapshot (or a deterministic sidecar); **surfacing is a read-only computed
view (like `audit`), never persisted.** `observe:false` ticks mutate the table +
snapshot; `observe:true` don't (consistent with today).

## Parameters (Phase 0-informed starts; all in `Policy`, tunable)

| param | start | notes |
|---|---|---|
| `coact_rate` (bump) | 0.15 | |
| `coact_decay` | TBD | set by the decay model above |
| prune floor | 0.10 | |
| `surface_threshold` | 0.60 | ≈ 3–4 recurrences (Phase 0 top was 4×) |
| hub degree cap | top ~1% | exclude project + super-hubs |
| kind noise set | `{event,task,question,pointer,commit,reference, no-kind}` | C2 set |

## Open questions for review

1. **Decay model** (per-tick vs per-event vs lazy) — the make-or-break.
2. Is **save co-mention** legitimately "co-activation," or a distinct signal that
   deserves its own weight/table (a save is authorship, a recall is retrieval)?
3. **Storage bound**: pairwise edges are O(n²) worst case. Is prune-below-floor
   enough, or do we need bounded top-K co-partners per element?
4. **Determinism**: does anything here threaten byte-identical replay?
5. **Surface cadence**: per session start? per N events? on-demand (a verb)?
6. **Strength transfer** on retirement — meaningful, or cosmetic ceremony?
7. Does the whole thing **earn its keep** at realistic volume, or is it a
   slow-burn that only pays off after months (Phase 0 recall sparsity)?

## Out of scope for v1

- The fixed-clique → "missing parent concept" suggestion (a later, different surface).
- Auto-linking (never; always LLM-adjudicated).
- Cross-store / global associations.
