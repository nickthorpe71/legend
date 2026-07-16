# Causal relationships in Legend

Legend represents cause and effect as first-class, queryable structure rather
than prose buried in summaries. The design follows Pearl's *Book of Why* /
`new_foundation.md` §16.3: keep the ladder of causation legible so a future
session never mistakes a correlation for a cause, and can ask "what caused this?"
directly. Shipped in three phases (commits `3e55393`, `ea7c65e`, `35d835f`).

## The two things it represents

Causality lives in two different places, and they are not the same:

1. **Causal edges between things** — ordinary FACTS with a causal predicate
   (`{s, p, o}` where `p` is `caused` / `enables` / `prevents` /
   `correlated_with`). The graph of these edges *is* the causal model.
2. **Modality on a claim** — whether an edge (or any fact) is observed vs acted
   on, actual vs counterfactual, negated, etc. This is a `modal` array on the
   fact, reified as meta-relations on the claim.

You need both: (1) to have a graph at all, (2) to know the *level of causal
commitment* each edge carries.

## The causal predicates (rung matters)

| Predicate | Pearl rung | Meaning |
|---|---|---|
| `caused` | rung-2 (causal) | X brought about Y |
| `enables` | rung-2 (causal) | X made Y possible |
| `prevents` | rung-2 (causal) | X blocked Y |
| `correlated_with` | rung-1 (correlational) | X and Y co-occur; **no cause claimed** |

These are seeded, deduplicated vocabulary: every `{X, caused, Y}` reuses the one
`caused` predicate element, so causal structure never fragments across synonyms.
The critical invariant — the whole reason the rungs are distinct predicates —
is that **Legend never promotes `correlated_with` to `caused`.** If you only know
two things co-occur, say `correlated_with`; do not claim a cause you don't have.

Also seeded: `subclass_of` (taxonomy) and `antecedent_of` (conditional shape,
"Y holds if X" — substrate for later forward-chaining / counterfactual queries).

## Modality — the `modal` array on a fact

```json
{"facts": [
  {"s": "deploy", "p": "caused", "o": "outage", "modal": ["intervened"]},
  {"s": "migration", "p": "prevents", "o": "outage", "modal": ["non_actual"]}
]}
```

| Modal | Meaning |
|---|---|
| `intervened` | an agent *acted* (Pearl rung-2 evidence) — vs the default, an *observed* fact |
| `non_actual` | claim is not about actual world state — counterfactual ("would have") or merely desired |
| `negated` | polarity flip |
| `uncertain` | a graded hedge |
| `general` | habitual / generic ("X generally causes Y") |

Each set modal reifies a meta-relation `[subject: <fact-rel>, <modal>: <modal>]`
on the claim. Absence of `intervened` means observation; absence of `non_actual`
means the claim is about actual state. An unknown modal string is a parse error.
Modality attaches to plain facts, event-shaped facts, and general-form facts.

## What recall returns — the `causal` section

A focused recall gathers the causal edges touching the focus into a dedicated
`causal` section, each tagged with its rung and modality, and keeps them out of
`recent`/`related` so they surface once, typed:

```jsonc
// recall {"focus": ["outage"]}
"causal": [
  {"ref": "rel:11", "attrs": {"subject": "migration", "prevents": "outage"},
   "rung": "causal", "modal": ["non_actual"]},
  {"ref": "rel:10", "attrs": {"subject": "deploy", "caused": "outage"},
   "rung": "causal", "modal": ["intervened"]},
  {"ref": "rel:13", "attrs": {"subject": "spike", "correlated_with": "outage"},
   "rung": "correlational", "modal": []}
]
```

This is the "what caused the outage?" query: the observed-vs-intervened and
actual-vs-counterfactual distinctions are on every edge, so the consumer never
reads a hedged or counterfactual claim as a settled cause.

### Modality surfaces on every fact, not only causal edges

A `modal` array is legal on **any** fact, not just a causal predicate — a
`justifies` or `depends_on` claim can be `negated` or `uncertain` too. So a
fact's modality renders wherever the fact renders: an entry in `state`,
`recent`, `related`, or `history` carries a `modal` field **when it has one**
(a plain fact with no modal omits the key, so the common case pays no bytes):

```jsonc
// recall {"focus": ["assets are text"]}  -- a NEGATED non-causal claim
"recent": [
  {"ref": "rel:10", "attrs": {"subject": "llm readability", "justifies": "assets are text"},
   "status": "asserted", "confidence": 0.7, "support_count": 1, "date": "2026-07-16",
   "modal": ["negated"]}
]
```

Negation is *modal*, not a status (there is no `Negated` status): the relation
is still `asserted` into the graph, and `modal: ["negated"]` says the claim it
asserts is **false**. A consumer must read the modal — a `negated` fact whose
modal is ignored inverts the record. (Regression fix, 2026-07-16: before this,
`modal` rendered only in the `causal` section, so a negated *non-causal* fact
recalled as a plain assertion — trial issue #616.)

## Storage & migration (implementation notes)

- The ten extended-vocabulary names are seeded contiguously on a fresh init and
  **appended on load** of a store created before the vocabulary existed
  (`seed_ext_vocab`), so upgrading a live store is additive: existing element ids
  and data are byte-preserved; the store just gains the vocab elements at the
  end. Every use resolves through `g->wk_ext[]`, never a fixed id.
- Causal predicates and modal names are protected from `rename`/`merge`
  (`is_core_vocab`), like the legacy core vocabulary.

## Not yet built (design present)

Interventional / counterfactual *queries* — Pearl rung 2/3 as a do()-projection
or a `non_actual` counterfactual tick (`new_foundation.md` §24.9) — are
deliberately deferred. The substrate above (causal edges, the modal metas, the
`derived_from` DAG) is what makes them possible later without new shape.
