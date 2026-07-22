# SimpleQA — Legend vs naive RAG at matched token budget (2026-07-22)

> **CORRECTION (2026-07-22, same day, later).** The run below was measured with a
> configuration bug: the harness (`common/legend_io.py`) resolved Legend's embed
> model dir relative to the benchmark CWD (where `models/` does not exist) and
> never set `LEGEND_EMBED_DIR`, so **every arm-B recall ran with embeddings OFF** —
> lexical entity resolution + recency fact ordering only, no semantic retrieval.
> The store was fully embedded at ingest (`vectors.bin` is real); recall just never
> used it. So the "RAG dominates by 37–40 points" headline was Legend competing
> with its own semantic retrieval switched off, against embedding-based RAG arms.
>
> **Fixed** (`from_config` now pins `LEGEND_EMBED_DIR` to the model dir beside the
> binary) and re-ran arm B with embeddings ON + the new F1 relevance ranking. The
> corrected numbers (store unchanged, A/D frozen):
>
> | metric | embed OFF (original) | embed ON + F1 (corrected) |
> |---|---|---|
> | arm B accuracy | 34/60 = 57% | **45/60 = 75%** |
> | B − D_bm25 | −37% [−50,−23] | **−18% [−30,−8]** |
> | B − D_dense | −40% [−52,−28] | **−22% [−32,−12]** |
> | correct & store-grounded | 18 | **30** |
> | A→B grounded lift | +8 | **+19** |
>
> RAG still leads by ~18–22 pts (D-only = 13; the remaining misses are facts
> outside the 1-hop hood → need the F3 traversal, multi-part answers, and residual
> consumption failures), but the gap halved and the store now does real work.
> **F1's isolated retrieval contribution** (deterministic frame-level replay, same
> recall calls, embeddings on both sides): gold-in-frame **42% → 60%**, +11
> questions, **0 displaced**. Corrected artifacts: `eval-simpleqa-run-2026-07-22-corrected/`.
> The original embed-off analysis below is kept for the record.

---


The decisive experiment the six-perspective review converged on (Dana's
falsifiable bar, Priya's #6): does Legend's deduplicated graph earn anything over
dumb retrieval over the *same* corpus, at a *matched* token budget? Run to
completion on 60 questions. **Answer: no — naive RAG dominates Legend by 37–40
points, and even BM25 (pure lexical, no ML) beats it 93% to 57%.**

Raw artifacts (this run, frozen): `docs/eval-simpleqa-run-2026-07-22/` — `report.md`,
`detail.md` (every question × every arm + the full B-vs-D disagreement lists),
`flips.md`, `grades.json`, `answers.jsonl`, `summary.json`, `config.json`,
`questions_supported.jsonl`. Harness: `benchmarks/simpleqa/`.

## Setup

- **Task:** SimpleQA (OpenAI), single-shot fact-seeking questions. 70 drawn, **60
  survived** the answer-present audit (gold findable on ≥1 scraped source page).
- **Models:** ingester `gpt-5.6-sol`, consumer `gpt-5.6-terra`, grader
  `gpt-5.6-sol` (verbatim SimpleQA 3-way grader). seed 12345.
- **Store:** one Legend store, Sol built it via recall-before-save over 72 pages →
  **14,730 elements, 20,583 facts** (205 elements/page — severe over-extraction,
  worse than the smoke's 148).
- **Arms** (identical answer/abstain instruction, so `not_attempted` is comparable):
  - **A** — bare Terra, no context.
  - **B** — Terra + a `legend_recall` tool over the store (≤3 calls).
  - **D_bm25** — Terra + top passages from BM25 over the same corpus, stuffed.
  - **D_dense** — Terra + top passages from `text-embedding-3-small` cosine, stuffed.
- **Token parity:** each D arm retrieved to a per-question budget = tokens of the
  recall frames arm B actually saw. Verified: median B = 2043 tok, D_bm25 = 2180,
  D_dense = 2151. D did **not** win on more context.

## Headline

| arm | accuracy | acc\|attempted | not_attempted |
|---|---|---|---|
| A (bare) | 23/60 = **38%** | 40% | 3 |
| B (Legend) | 34/60 = **57%** | 57% | 0 |
| D_bm25 (lexical RAG) | 56/60 = **93%** | 95% | 1 |
| D_dense (dense RAG) | 58/60 = **97%** | 97% | 0 |

**Structure delta (claim 3), paired bootstrap 95% CI, 5000 resamples:**

| comparison | Δ (B − D) | 95% CI | B-only | D-only |
|---|---|---|---|---|
| B vs D_bm25 | **−37%** | [−50%, −23%] | 1 | 23 |
| B vs D_dense | **−40%** | [−52%, −28%] | 0 | 24 |

Both CIs exclude zero decisively. There is essentially **nothing the graph
retrieves that RAG misses** (B-only = 1 vs BM25, 0 vs dense), and 23–24 questions
RAG gets that Legend misses.

Legend *does* beat nothing: B − A = **+11** (grounded lift +8; A→B fixed 16, of
which 13 grounded, 3 consumer-from-weights fakes correctly flagged; broken 5). But
"memory beats amnesia" is the near-tautology the review predicted — not the value
claim. The value claim is B vs D, and it lost.

## Why Legend lost — the mechanism

Two lossy stages compound, and the failure is worse than "misses" — it is
**confident wrong answers.**

**Where accuracy dies (evidence-based attribution, from the ingest journal + store
membership of each gold; measurement noise noted):**
- **Legend RETRIEVAL — ~19–23 of the 26 misses (the majority).** The fact is
  provably in the store (Sol extracted it, Legend kept it), but recall surfaces a
  wrong neighbor and Terra answers confidently wrong: David Sweet `June 24` (B said
  26), Jensen wheelbase stored as `2845` (B said 2667), both Constable children
  present (B wrong). This is Legend's recall, not the ingester. (Range because the
  automated store-membership check misses comma-numbers/multi-part golds and thus
  undercounts this bucket — the true share is at the high end.)
- **Ingester LLM miss — ~6–10.** Sol never emitted the fact in any form: Sara
  Watkins' marriage date (entity stored, the date never extracted), Masaki Tsuji's
  award date, the Javits answer. This is the LLM, not Legend's storage.
- **Legend storage rejection — 0.** The harness atomic-object filter rejected 266
  of Sol's facts (enforcing Legend's "no prose object" discipline), but **none were
  gold answers.** "Legend's discipline threw the answer away" did not happen here.
- Arm B's ceiling was 83% (50/60 golds reached the store); it hit 57%, so most of
  the gap is retrieval, not ingestion.

**The killer pattern (from the 23–24 B-vs-D disagreements in `detail.md`):** arm B
almost never abstains (**0 not_attempted**) — instead it retrieves a
plausible-but-wrong adjacent fact from the 14,730-element store and states it
confidently:

- *David Sweet born?* gold **June 24, 1957** — B: "**June 26, 1957**" (RAG: correct)
- *Jensen Interceptor wheelbase?* gold **2,845 mm** — B: "**2,667 mm**"
- *Padma Bhushan year?* gold **1954** — B: "**1955**"
- *NBA-cheerleader contestant?* gold **Lori Todd** — B: "**Hayley Crittenden**"
- *von Kármán's research assistant?* gold **Frank Wattendorf** — B: "**Clark
  Blanchard Millikan**"
- *2015 Matia Mahal MLA's father?* gold **Shamim Ahmed Khan** — B retrieved the
  *wrong MLA* ("Asim Ahmed Khan") and answered his father
- *Sara Watkins marriage date?* gold **August 16, 2008** — B: "**September 9,
  2016**" (in the 5-q smoke B accidentally got this right from its own weights; in
  the full run, with a bigger, noisier store, recall surfaced/confabulated a wrong
  date)

RAG wins these because it keeps the **verbatim passage** — the exact date/number/
name is a literal substring of a retrieved chunk. Legend by design keeps gist
triples and discards the raw source text; it over-extracted 205 elements/page of
near-miss facts, and for these questions recall served a confident wrong neighbor
(or, for ~6–10, Sol never extracted the fact at all). **The over-extraction is not
just noise — it manufactures the wrong-neighbor facts Legend confidently reports.**
Note the precise attribution above: the loss is at *recall* and at *LLM
extraction*, not at Legend's storage (which rejected 0 gold answers).

## What this shows — and what it doesn't

**Shows (decisively):** On single-shot verbatim factoid QA over a static corpus,
naive RAG beats Legend by 37–40 points at matched token budget, both CIs excluding
zero, with **BM25 alone** — Dana's "twenty lines of cosine similarity" — sufficing
at 93%. The design doc's own decision rule (§57: "If RAG matches Legend, the
structure isn't paying for itself on this task shape") is triggered hard: RAG
doesn't match, it dominates. Dana's falsifiable bar has its verdict.

**Does NOT show:** that Legend is worthless. SimpleQA is the **worst-case task
shape** for Legend and best-case for RAG — verbatim single-fact lookup where the
answer is a literal substring of a retrievable chunk, single-shot, no revision, no
cross-session horizon. It tests **none** of Legend's actual pitch: revisable
memory, supersession, dedup-over-time, causal structure, multi-session agentic
continuity. A loss here does not refute that pitch.

**But the sting is real:** this kills the specific "Legend as a transferable
knowledge artifact / distillation store that beats RAG" framing this eval was built
to test, and it exposes a failure mode — **confident wrong-neighbor retrieval from
an over-extracted store, with no abstention** — that would hurt in the agentic
setting too. The most actionable findings, in priority order:
1. **Over-extraction is actively harmful.** 205 elements/page fills the store with
   near-miss facts; recall serves them confidently. Fewer, cleaner facts would help
   both this task and store health (ties to Sofia's "no literal type" root cause and
   the trial's `prose_name`/bloat papercuts).
2. **Ingest discards the verbatim anchor.** SimpleQA answers are exact values;
   Legend keeps gist and loses them. A design that keeps a retrievable
   verbatim/source pointer alongside the gist would recover much of the 17%+26%.
3. **Recall never abstains.** 0/60 not_attempted; B states a wrong neighbor rather
   than "I don't know." Abstention-on-low-confidence would convert confident-wrong
   into honest-null (which the grader scores as not_attempted, not incorrect).

## Grounding & the smoke fake-positive, resolved

The 5-question smoke's lone "win" (Sara Watkins) was a fake positive — the store's
date was ISO-normalized (`2008-08-16`) and the flips grep literal-matched "August
16, 2008", so it mis-reported "gold not in store," and Terra had answered from its
own weights. This run's grounding is **date-format-aware** (`report.grounded`
canonicalizes both sides), so the 13 grounded fixed flips are genuine and 3 fakes
are correctly flagged. And in the full run Sara Watkins flips to a *loss* for B
(wrong date), confirming the smoke win was never real.

## Cost

| stage | prompt tok | completion tok | $ |
|---|---|---|---|
| ingest (Sol) | 4,687,867 | 1,348,216 | **63.89** |
| arms (Terra ×4) | 730,541 | 34,415 | 2.34 |
| grade (Sol ×240) | 331,638 | 1,200 | 1.69 |
| **total** | | | **67.92** |

Ingest dominated (reasoning/completion tokens at $30/mtok). Estimated at ~$38 from
a 4-page smoke; came in at $63.89 — see the cost-estimation lesson in memory.

## Reproducibility

`config.json` (archived) pins models, seed (12345), `LEGEND_NOW`, rag block
(chunk_tokens 256, budget_floor 300, embed text-embedding-3-small). Note: GPT-5.6
is a reasoning model and is **not** bit-deterministic even with a seed, so exact
numbers will vary slightly on re-run; the 37–40 point gap will not. To reproduce:
`benchmarks/simpleqa/run_all.sh` with these config values (draws a fresh sample —
set the same seed for the same draw).
