# Spec — raw-passage anchor (recall fallback to source text)

> **ADVERSARIAL REVIEW VERDICT (2026-07-22) — DO NOT BUILD AS SPECCED.** A 5-agent
> panel found the plan broken in its core mechanism and net-negative for Legend's
> actual (revisable) use case. Status downgraded from "primary lever" to
> **deferred, revise-heavy**; F6 abstention + F3 traversal + extraction-tightening
> move ahead of it (`retrieval-redesign.md`). Findings:
> - **The confidence gate is broken four ways**, all rooted in reusing F1's cosine
>   (which can't separate "answer present" from "topically-adjacent wrong
>   neighbor"): (1) can't catch wrong-*value* misses — "Moral Sciences" scores high,
>   so the flagship 2790 case never flips; (2) *fires more reliably in stale cases*
>   — a supersession pushes the new value away from a query that echoes the old, so
>   the gate resurrects superseded/retracted values; (3) the BGE cosine band is
>   already measured dead by the trial (`alchamancer-trial.md:468-472, 270-272`) —
>   no floor separates a relevant 0.6 from a noise 0.6; (4) it is *circular* — the
>   same score that made arm B never abstain gates the fallback. Only rescue: a
>   **lexical/token-coverage** trigger (the trial's validated abstention mechanism),
>   not a cosine threshold.
> - **Net-negative for revisable stores** (`changes`/`retract`): the sidecar has no
>   status link and reasserts values the graph buried. Safe only for immutable/
>   append-only factoid corpora, hard-gated on a store having no supersessions.
> - **Wouldn't fix what it claims:** ~3–4 of 8 flip (not 8), ceiling is a *tie*
>   with RAG; several "ingest misses" are really retrieval (F3/F5) or wrong-value
>   (extraction) misses needing different fixes.
> - **The agentic-relevant win is F6 abstention — needs no raw text.** Only the
>   raw-text-*return* half requires the gist concession, and it serves factoid QA
>   alone.
> - **Spec errors:** "reuses F1's scores, no new machinery" is false (F1 discards
>   cosines); "recall stays read-only" is loosely false (`embed_rank_elements`
>   persists `vectors.bin`); the "relevant 0.6 / noise 0.5" band is a misread.
> - **Before any code:** run the ~$1–2 no-code simulation — append the exact
>   D_dense chunks arm D already retrieved to arm B's frames for the 8 target qids,
>   re-run Terra, re-grade; pre-registered bar: **≥6/8 flip with the gate
>   auto-firing** → build, else the mechanism is broken as designed.
> **SIMULATION RESULT (2026-07-22, `benchmarks/simpleqa/sim_raw_anchor.py`).** Ran
> the $1-2 no-code test. **Bar NOT met.** Split outcome:
> - **Content works — oracle-gate ceiling 7/8 ingest (12/13 overall) flip.** Handed
>   the exact chunks RAG used, Terra readily uses them (refuting the "consumption is
>   the bottleneck" prediction). The one non-flip (2790 Lygia Pape) is instructive:
>   "prefer graph" made Terra keep the graph's WRONG "Moral Sciences" over the
>   passage's correct "Philosophy."
> - **Gate dead — confirmed.** Miss top-fact relevance `[0.03,0.14,0.51,0.53,0.67,
>   0.70,0.72,0.80]` overlaps hits `[0.59,0.60,0.64,0.69,0.72,0.73,0.75,0.77]`; 4/8
>   misses score ≥0.67, inside the hit band. A ~0.55 threshold catches only the 4
>   low-gate misses → **cosine-gated reality ~4/8 (B→~82%, still loses); always-on
>   ~7/8 but = RAG + staleness on revisable stores.**
> Conclusion: the content approach works; the whole problem is the trigger. Viable
> only as an opt-in **always-on** (immutable factoid corpora, where staleness can't
> occur) or with a working **lexical-coverage** trigger (untested — the next
> experiment if a factoid store is pursued). NOT a general lever.
>
> Reopen only behind an explicit factoid-store product decision.

**Status:** proposed, not built. ~~Primary lever for closing the RAG gap.~~
**DEFERRED — see adversarial-review verdict above.**
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
