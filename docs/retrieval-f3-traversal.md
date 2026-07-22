# Design — F3: semantic traversal (anchor→path→ranked expansion)

**Status:** designed, not built. Secondary lever (after the raw-passage anchor).
**⟵ COME BACK TO THIS.** Recovers the in-store retrieval misses the SimpleQA
breakdown found (`retrieval-redesign.md`, `eval-simpleqa-run-2026-07-22.md`) and is
Legend's *relational-query* differentiator — the place it should **beat** RAG, not
just match it. F1 (query→fact relevance ranking) is done; F3 builds on it.

## What it fixes

From the verified breakdown of the 13 D-only misses, the retrieval bucket
(fact is in the store, recall didn't surface it):
- **`447` Yamraj→Pluto** — `(Yama) planet = Pluto` is in the store; Terra focused
  "Yamraj", an **alias** of Yama that resolution didn't follow. → alias resolution.
- **`820` US equestrian** — `(United States) equestrian rank at 2004 Olympics = 1`
  is in the store, but the answer entity (US) is unknown to the asker; you focus on
  "2004 Olympics" + "equestrian" and must reach the country with the top rank. →
  **reverse-lookup / aggregation** via traversal.

Plus the general case F1 can't reach: facts **outside the focus element's 1-hop
hood**, and multi-hop relational questions ("how are X and Y related", "X's
spouse's employer") — the agentic queries that are Legend's reason to exist.

## The design (composing the discussion + F1)

Focus terms are of two kinds — **anchors** (entities/predicates/values to resolve
and walk from) and **targets** (what to find). All of them resolve against **all
elements**, not just entities (predicates and values are elements in Legend's
reify-everything model — that is what makes "entity + predicate" a two-anchor
query whose connecting edge is the answer: `David Sweet —[born]→ 1957-06-24`).

1. **Resolve anchors, with aliases.** Extend resolution to follow the **alias
   index** — "Yamraj" resolves to the canonical "Yama". (447's bug: the alias link
   either was not recorded or resolution did not check it. Verify both.)
2. **Multi-anchor disambiguation** (F5, folded in). When ≥2 anchors resolve, use
   co-occurrence to pick the right sense: `["Sara Watkins","Todd Cooper"]` → the
   *person* linked to Todd Cooper, not the *album*.
3. **Walk between anchors.** BFS from each anchor toward the others; the connecting
   relation(s) are the answer for attribute/relational questions. Collect the
   relations on the connecting paths.
4. **k-hop expansion** around each anchor (depth 2–3), collecting nearby relations
   for context beyond the direct path.
5. **Rank everything by relevance** — reuse F1's `embed_rank_texts` over the
   collected relations vs the query; return top-k by relevance, not recency.

## Mechanics / guards

- **Hub explosion is the main risk.** Over-extraction (205 elem/page) creates
  high-degree predicate/value nodes (a shared "date"/"1976" element linking
  hundreds of facts); a naive BFS walks through them and returns meaningless short
  paths. Guards: (a) cap fan-out per node; (b) down-weight edges through
  high-degree nodes (specificity weighting); (c) bounded depth (2–3). This couples
  F3 to the ingest-quality work — fewer/cleaner facts → fewer spurious hubs.
- **Degrade to one anchor.** Most queries have a single meaningful anchor; the
  single-anchor path must be first-class (it is just F1 over the k-hop
  neighborhood), not a fallback.
- **Reverse-lookup / aggregation** (820): traversal surfaces the *candidate* facts
  (each country's equestrian rank at 2004); the consumer does the "which is #1"
  aggregation. Legend's job is to surface the distinct comparable facts, not to
  compute the max (consistent with the "operations in the consumer" stance).
- **Frame shape:** collected facts feed `recent`/`related` (relevance-ranked as in
  F1). For genuinely multi-hop results, consider a `paths` section rendering the
  connecting chain so the consumer sees the relation, not just endpoints.

## Determinism

The graph walk is deterministic (fixed traversal order by id); the relevance
ranking is gated on `embed_available()` exactly like F1, so the `LEGEND_EMBED=0`
determinism gate is unaffected. Frames change only when embeddings are on.

## Composition with F1

F1 = *rank* the facts already in the 1-hop hood by query relevance (done).
F3 = *gather* a larger, path-aware candidate set (between-anchor paths + k-hop +
aliases), then hand it to F1's ranker. F3 without F1's ranking would just dump a
bigger unranked neighborhood; F1 without F3 can't reach facts outside 1-hop. They
compose: F3 gathers, F1 ranks.

## Open questions

1. Path depth / fan-out caps — tune against the hub-pollution risk.
2. How many connecting paths to return (1 shortest? top-k by specificity?).
3. Alias coverage — are aliases reliably recorded at ingest? (447 suggests not.)
4. Does `paths` as a frame section earn its token cost, or is relevance-ranked
   `related` enough?

## Success metric

`447` (alias) and `820` (reverse-lookup) flip to correct; multi-hop conflictqa-style
questions improve. Measure at the **frame level** (gold-in-frame, deterministic) as
well as answer level — the consumer-noise floor (~±5 questions) hides small
answer-level gains, as it did for F1.
