# Retrieval redesign — fix backlog

Motivated by the SimpleQA B-vs-D result (`docs/eval-simpleqa-run-2026-07-22.md`):
naive RAG beat Legend recall by 37–40 points at matched token budget. Root cause,
grounded in real frames: **recall does semantic *entity resolution* then dumps the
entity's neighborhood ranked by activation/recency — it has no query→fact relevance
step and no path-finding between anchors.** The embedding is spent finding the
entity, then discarded.

Two failure frames that pin it (`legend.c`: `tick_recall` 6246, `rank_related`
6947, section assembly 7052, cap `g_frame_section_cap` 6592):
- *David Sweet's birth date?* — "David Sweet" resolved (person), "birth date"
  resolved to a bare node, and the `recent` band returned five election-winner
  facts. The birth-date fact never surfaced; the model confabulated. The neighbor-
  hoods were unioned and activation-ranked; **the path David Sweet →[born]→ date
  was never traversed.**
- *former NBA cheerleader (Bachelor S2)?* — the attribute-value focus term didn't
  resolve; the model was reduced to guessing contestant names.

## Fixes (recorded so we don't lose track)

Ordered by leverage-per-effort. F1 first.

**F1 — Query→fact relevance ranking (DO FIRST).** After resolving the focus
element(s), embed the query (the focus terms / question) and rank the resolved
elements' facts (rendered "predicate: object") by cosine similarity to the query,
returning top-k by **relevance** rather than by activation/recency. On the David
Sweet case this surfaces `born: 1957-06-24` for query "birth date" instead of five
election results. Smallest change, recovers most of the measured gap. Read-only
recall-path change → no re-ingest needed to test (see re-run economics below).

**F2 — Anchors are all elements, not just entities.** Confirm/enforce that focus
terms resolve against **all** elements — entities, predicates, values — as
first-class anchors. (Evidence suggests this already largely happens: "birth date"
resolved to a node. The gap is what we do *after* resolving, not the resolution
itself — but make the intent explicit and verify no kind-restriction sneaks in.)

**F3 — Walk between focus anchors (the semantic traversal).** When ≥2 anchors
resolve, find the connecting path(s) between them; the connecting edge is often the
answer (`David Sweet —[born]→ date`; `Sara Watkins —[spouse/married]→ Todd Cooper`,
with the date as an attribute of that edge). Walk **directionally from each anchor**
and bound the depth so hub predicate-nodes (shared "date"/"born" elements) don't
explode the path set. This is the primitive for relational / multi-hop queries —
Legend's actual differentiator, and where it could **beat** RAG rather than match
it.

**F4 — k-hop expansion around each anchor**, relevance-ranked (F1 applied to the
neighborhood), for surrounding context beyond the direct path.

**F5 — Multi-anchor disambiguation.** Use co-occurring anchors to constrain
resolution: `["Sara Watkins", "Todd Cooper"]` → the *person* Sara Watkins (linked
to Todd Cooper), not the *album*. Fixes the homonym failure (`tick_recall` silently
picks the first tier-1 match, 6292).

**F6 — Abstention.** When no reached fact matches the query well, signal "not
found" instead of returning an activation-ranked neighborhood the model will
confabulate from. Arm B never abstained (0/60 not_attempted) and stated confident
wrong neighbors.

**Coupling — over-extraction.** Every fix above is degraded by the 205-elements/
page over-extraction: it inflates neighborhoods (F1/F4 ranking harder), creates hub
nodes (F3 path pollution), and manufactures the near-miss facts recall confidently
serves. Pair the retrieval work with tightening ingest extraction.

## Sequence

1. **F1** — query→fact relevance ranking. **DONE 2026-07-22** (`rerank_relevance`
   in legend.c + `embed_rank_texts` in embed.c, gated on `embed_available()` so the
   determinism gate is untouched). Frame-level: gold-in-frame 42%→60% (+11, 0
   displaced). Answer-level, after also fixing the harness embed-dir bug that had
   recall running embeddings-off: **arm B 57%→75%**, structure delta B−D_dense
   −40%→−22%. See the CORRECTION in `eval-simpleqa-run-2026-07-22.md`.
2. **F5** — multi-anchor disambiguation (cheap, fixes homonyms).
3. **F2 + F3 + F4** — the full semantic traversal (anchors→paths→ranked expansion):
   Legend's relational-query strength, where it should beat RAG, not just match it.
4. **F6** — abstention, once relevance scores give a usable confidence signal.

## Re-prioritization after the D-only breakdown (2026-07-22)

The verified breakdown of the 13 questions RAG still wins (corrected embed-on+F1
run) reordered the levers — **ingest, not traversal, is the bigger remaining
lever**, and consumption is a non-issue:
- **~8/13 ingest misses** (fact never stored, or stored wrong) → the primary lever
  is the **raw-passage anchor**: [`ingest-raw-anchor.md`](ingest-raw-anchor.md).
  RAG wins these purely by keeping the verbatim source text.
- **~3–4/13 retrieval misses** (fact in store, not surfaced: alias 447, reverse-
  lookup 820) → **F3 traversal**: [`retrieval-f3-traversal.md`](retrieval-f3-traversal.md).
- **0 consumption** — when a fact reaches the frame, the consumer uses it.

So: build the raw-passage anchor first (biggest bucket, and the crisp answer to
"why not just RAG"), then F3 (Legend's relational-query differentiator). Both are
specced; **come back to them.**

## Re-run economics

A recall change is **read-only** — it does not touch the store. So testing F1
reuses the existing $63.89 store (no Sol re-ingest) and re-runs **only arm B**
(Terra recall) + re-grades arm B; arms A (bare) and D (RAG) are unaffected and
their answers are frozen in `answers.jsonl`. Cost of a re-run ≈ the Terra price
only, ~$1–2. (F1 embeds facts at recall time with the local BGE model — no new
persisted state, no re-ingest.)
