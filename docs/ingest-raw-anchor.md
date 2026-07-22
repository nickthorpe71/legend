# Spec — raw-passage anchor (recall fallback to source text)

**Status:** proposed, not built. Primary lever for closing the RAG gap.
Motivated by the SimpleQA breakdown (`eval-simpleqa-run-2026-07-22.md` +
`retrieval-redesign.md`): of the 13 questions RAG got that Legend missed, **~8 are
ingest misses** — the answer fact never made it into the graph (Sol didn't extract
it, or extracted a wrong value: Sara Watkins' marriage date absent, Lygia Pape
stored as "Moral Sciences" not "Philosophy", the chess £20 prize absent). RAG wins
these for one reason: it keeps the **verbatim source passage**, so the exact
answer is a literal substring. Legend discards raw text at ingest by design (gist,
not verbatim). This spec adds the missing capability back as a **fallback tier**,
not a replacement for the graph.

## The idea

Two-tier recall:
- **Tier A — the graph (primary).** Resolve focus → traverse (F3) → rank facts by
  relevance (F1). This is unchanged and stays the default answer surface.
- **Tier B — raw passages (fallback).** Keep an embedded index of the source
  chunks that were ingested; when Tier A does not surface a fact relevant to the
  query, embed the query and return the top-k raw chunks in a new frame section.

The consumer gets structured facts when the graph has them, and the raw passage
when it doesn't — i.e. Legend = revisable/deduped/causal graph **+** RAG's verbatim
recall, unified behind one `recall`. This is also the crisp answer to "why not just
use RAG": you get the graph's revision/supersession/structure *and* a raw-text
safety net for what the graph didn't capture.

## What to store

- A per-store **`chunks` sidecar** (`chunks.bin`), parallel to `vectors.bin`:
  `{chunk_id, src_label, text, embedding}`. Written at save/ingest time from the
  same source text the ingester read; read at recall time.
- **Sidecar, not snapshot.** Keep it out of the `snapshot` binary format (like
  `vectors.bin`) so there is **no snapshot-format change and no determinism/replay
  impact** — the snapshot stays byte-reproducible; the sidecar is a rebuildable
  cache. `LEGEND_EMBED=0` disables Tier B entirely (same gate as F1).
- Storage cost is real (raw text > triples). Mitigations: cap chunk size (~256
  tok), dedup identical chunks by `fnv64(text)`, and make it **opt-in per store**
  (`LEGEND_RAW_ANCHOR=1` or a policy flag) — a pure agentic-memory store may not
  want it (see the strategic note).

## When to fall back (the hard part)

Do **not** always append passages (that is just bolting RAG on, and it burns the
token budget). Gate it on a confidence signal we already compute:
- After Tier A ranks the neighborhood facts (F1), take the **top fact's relevance
  score** to the query. If it is below a threshold (the graph has nothing that
  looks like an answer), attach a `passages` band from Tier B.
- Threshold is tunable and store-specific; start with the BGE cosine band the
  trial already characterized (relevant ~0.6+, noise ~0.5). Log the decision.

This makes Tier B a *miss detector*: it fires exactly when the graph would
otherwise hand the consumer a confident wrong neighbor (the failure the eval
caught). It reuses F1's scores, so no new confidence machinery.

## Frame shape

A new capped section, e.g.:
```
"passages": [ {"src": "<label>", "score": 0.71, "text": "<raw chunk>"}, ... ]
```
Capped like other bands; counted in `omitted`. The consumer instruction gains one
line: prefer a fact from the graph; use `passages` only when the graph lacks the
answer.

## Open questions

1. **Store everything, or only weak sources?** Storing every chunk is simplest and
   most robust; storing only chunks whose facts were sparse saves space but risks
   dropping the one we need. Lean: store all, cap size, dedup.
2. **Chunk granularity** for retrieval (256 tok is the eval's arm-D value; smaller
   = sharper answers, more chunks).
3. **Consumer discipline** — will the model over-rely on passages and stop using
   the graph? Measure the graph-vs-passage answer mix; if passages dominate, the
   graph isn't earning its keep and that is itself a finding.
4. **Provenance** — passages tie to `src`; keep the existing fact→src links so a
   passage and its extracted facts are cross-referable.

## Rejected alternative

*Just improve extraction.* Make Sol extract the missing values. Rejected as the
primary lever: the trial showed extraction is a treadmill (205 elem/page of
over-extraction that still misses the one needed fact), and a non-deterministic
extractor cannot be made complete. The raw anchor is robust to imperfect
extraction — it does not require the ingester to have anticipated the question.
(Still worth tightening extraction independently; it just is not the fix for the
ingest-miss bucket.)

## Strategic note — this is a philosophical concession

Legend's core bet is "compress to gist, discard raw." The raw anchor partially
reverses that. Justified for **factoid / knowledge-store** use (where verbatim
recall is the job), and it is what makes Legend competitive with RAG on SimpleQA.
But for the **agentic cross-session memory** pitch (the trial: decisions, recipes,
next-levers carried across sessions), verbatim source passages matter far less —
the value there is the revisable graph, not raw text. So: build this if factoid
QA / a queryable knowledge artifact is a target use case; treat it as optional
(off by default) for pure agentic-memory stores. Decide the use case before
building — do not compromise the gist design globally to win one benchmark.

## Success metric

Re-run the eval with Tier B on: the ~8 ingest-miss questions should flip to correct
when the raw passage surfaces the answer, closing most of the remaining B−D gap.
Watch the graph-vs-passage answer mix (see open question 3).
