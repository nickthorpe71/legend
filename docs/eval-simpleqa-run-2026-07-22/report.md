# SimpleQA — results (arms A / B / naive-RAG D)

- questions (surviving audit): **60**
- ingester: `gpt-5.6-sol` · consumer: `gpt-5.6-terra` · grader: `gpt-5.6-sol`

## Headline

- accuracy — A: **23/60** (38%)  ·  B: **34/60** (57%)  ·  D_bm25: **56/60** (93%)  ·  D_dense: **58/60** (97%)
- net lift arm B − arm A (store vs nothing): **+11**
- A→B flips: fixed **16** (grounded **13**) · broken **5** · grounded lift **+8**

## Structure delta — arm B (Legend) vs naive RAG at matched token budget

The claim-3 number: does the deduped graph earn anything over dumb retrieval over the same corpus? Paired bootstrap 95% CI (5000 resamples).

| comparison | acc B | acc D | Δ (B−D) | 95% CI | B-only | D-only |
|---|---|---|---|---|---|---|
| B vs D_bm25 | 57% | 93% | -37% | [-50%, -23%] | 1 | 23 |
| B vs D_dense | 57% | 97% | -40% | [-52%, -28%] | 0 | 24 |

## Per-arm

| arm | correct | incorrect | not_attempted | attempted-rate | acc\|attempted |
|---|---|---|---|---|---|
| A | 23 | 34 | 3 | 95% | 40% |
| B | 34 | 26 | 0 | 100% | 57% |
| D_bm25 | 56 | 3 | 1 | 98% | 95% |
| D_dense | 58 | 2 | 0 | 100% | 97% |

## Token-parity audit (median injected context tokens/question)

| arm | median tokens |
|---|---|
| B (recall frames) | 2043 |
| D_bm25 (retrieved chunks) | 2180 |
| D_dense (retrieved chunks) | 2151 |

## A → B transition

| A \ B | correct | incorrect | not_attempted |
|---|---|---|---|
| **correct** | 18 | 5 | 0 |
| **incorrect** | 14 | 20 | 0 |
| **not_attempted** | 2 | 1 | 0 |

## Store

- pages ingested: 72  ·  elements: 14730  ·  saves: 185  ·  minted elements: 14234

## Tokens & cost

| stage | prompt tok | completion tok | $ |
|---|---|---|---|
| ingest (gpt-5.6-sol) | 4,687,867 | 1,348,216 | 63.89 |
| arms (gpt-5.6-terra) | 730,541 | 34,415 | 2.34 |
| grade (gpt-5.6-sol) | 331,638 | 1,200 | 1.69 |
| **total** | | | **67.92** |

See `flips.md` for the arm-B flip worksheet. Fixed flips are auto-grounded on canonicalized (date-aware) matching; confirm the ambiguous ones by hand.
