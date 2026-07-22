# SimpleQA — results (arms A / B / naive-RAG D)

- questions (surviving audit): **60**
- ingester: `gpt-5.6-sol` · consumer: `gpt-5.6-terra` · grader: `gpt-5.6-sol`

## Headline

- accuracy — A: **23/60** (38%)  ·  B: **45/60** (75%)  ·  D_bm25: **56/60** (93%)  ·  D_dense: **58/60** (97%)
- net lift arm B − arm A (store vs nothing): **+22**
- A→B flips: fixed **25** (grounded **22**) · broken **3** · grounded lift **+19**

## Structure delta — arm B (Legend) vs naive RAG at matched token budget

The claim-3 number: does the deduped graph earn anything over dumb retrieval over the same corpus? Paired bootstrap 95% CI (5000 resamples).

| comparison | acc B | acc D | Δ (B−D) | 95% CI | B-only | D-only |
|---|---|---|---|---|---|---|
| B vs D_bm25 | 75% | 93% | -18% | [-30%, -8%] | 1 | 12 |
| B vs D_dense | 75% | 97% | -22% | [-32%, -12%] | 0 | 13 |

## Per-arm

| arm | correct | incorrect | not_attempted | attempted-rate | acc\|attempted |
|---|---|---|---|---|---|
| A | 23 | 34 | 3 | 95% | 40% |
| B | 45 | 15 | 0 | 100% | 75% |
| D_bm25 | 56 | 3 | 1 | 98% | 95% |
| D_dense | 58 | 2 | 0 | 100% | 97% |

## Token-parity audit (median injected context tokens/question)

| arm | median tokens |
|---|---|
| B (recall frames) | 870 |
| D_bm25 (retrieved chunks) | 2180 |
| D_dense (retrieved chunks) | 2151 |

## A → B transition

| A \ B | correct | incorrect | not_attempted |
|---|---|---|---|
| **correct** | 20 | 3 | 0 |
| **incorrect** | 22 | 12 | 0 |
| **not_attempted** | 3 | 0 | 0 |

## Store

- pages ingested: 72  ·  elements: 14730  ·  saves: 185  ·  minted elements: 14234

## Tokens & cost

| stage | prompt tok | completion tok | $ |
|---|---|---|---|
| ingest (gpt-5.6-sol) | 4,687,867 | 1,348,216 | 63.89 |
| arms (gpt-5.6-terra) | 528,168 | 31,712 | 1.80 |
| grade (gpt-5.6-sol) | 331,607 | 1,314 | 1.70 |
| **total** | | | **67.38** |

See `flips.md` for the arm-B flip worksheet. Fixed flips are auto-grounded on canonicalized (date-aware) matching; confirm the ambiguous ones by hand.
