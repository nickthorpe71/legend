# Phase 0 Implementation Plan — SimpleQA signal check

Status: DRAFT for review — 2026-07-11
Spec: [eval-simpleqa-distillation.md](eval-simpleqa-distillation.md) §0

Principle: smallest harness that produces the §0 read-out. Everything is built
to run at `n_questions: 1` first (one question through every stage), then the
same command runs at 50. No Batch API, no parallelism, no metrics machinery —
sync calls, JSONL artifacts, one report.

## Step 0 — Preflight (no code)

- [ ] Verify exact GPT-5.6 API model IDs (Sol, Terra) against OpenAI's models
      endpoint; one trivial tool-calling request each to confirm the key works.
- [ ] Download SimpleQA (`github.com/openai/simple-evals`); confirm per-question
      metadata has `topic` and source `urls`. Grab the official grader prompt
      verbatim while there.
- [ ] `./check.sh` green; note the dev `legend` binary path; confirm
      `models/bge-small-en-v1.5/` present (`LEGEND_EMBED=1` works).

Output: `benchmarks/simpleqa/config.json` — model IDs, binary path, dataset
path, seed, fixed `LEGEND_NOW`, `n_questions` (1 for smoke, 50 for the run).

## Step 1 — Scaffold

```
benchmarks/simpleqa/
  config.json
  common/
    legend_io.py     # init/save/recall/dump via subprocess
    oai.py           # OpenAI client: tool-call loop helper, retry w/ backoff
    schemas.py       # save/recall payloads (docs/cli.md) as OpenAI tool defs
    prompts.py       # ingester system prompt; answer+abstain instruction; grader prompt
  fetch_corpus.py  ingest.py  run_arms.py  grade.py  report.py
  run_all.sh
  requirements.txt   # openai, trafilatura, rapidfuzz
  corpus/ store/ runs/ results/        # gitignored
```

`legend_io.py`: every call sets `LEGEND_STATE_DIR`, `LEGEND_NOW` (from config),
`LEGEND_EMBED=1`; payload on stdin, JSON out; non-zero exit → raise with the
structured error `code`. Smoke test: temp store, init → save → recall → dump
round-trip.

`schemas.py`: save tool exposes `elements/facts/changes/retract/merge` +
required `src` on facts (grounding — cheap now, load-bearing in the full
study); recall tool exposes `focus/limit/history_depth`. Tool descriptions
carry the discipline text from `docs/mcp-server.md` (recall before save,
`changes` for updates, few precise elements).

## Step 2 — `fetch_corpus.py`

1. Deterministic sample: seed from config, filter to ≥1 Wikipedia source,
   take first `n_questions`. Write `corpus/questions.jsonl`.
2. Fetch each source URL → trafilatura extraction (full main content, no cap)
   → `corpus/pages/*.md` + `corpus/manifest.jsonl` (url, sha256, tokens).
   Fetch failure: log and continue (question survives if ≥1 source fetched).
3. Audit: gold answer in ≥1 of the question's pages (exact + rapidfuzz).
   Failures → `corpus/audit.json` as `corpus_unsupported`, question dropped.

Assert: ≥80% of sampled questions survive; else stop and inspect extraction
before spending ingestion tokens (the review's infobox-loss warning — if
tables are being stripped, this is where it shows up).

## Step 3 — `ingest.py` (Sol, one store)

- `legend init --reset` into `store/`.
- Pages in manifest order; split at ~2k tokens on paragraph boundaries; per
  chunk run the tool-call loop (recall/save available, max ~8 calls), feeding
  frames back verbatim.
- Journal every request/tool call/frame → `runs/ingest/journal.jsonl`; write a
  per-page high-water mark after each page completes; on restart, resume from
  it (store snapshot is durable per save, so re-entry at page N is safe).
- Failure policy: API/infra errors retry with backoff (never skip); a chunk
  whose loop still errors after 2 attempts keeps whatever partial saves landed,
  is logged, and the run continues.

Assert: dump non-empty; ≥1 save per page; log element/fact counts to
`runs/ingest/summary.json`. Then eyeball `legend dump --pretty` once before
running arms — 5 minutes of reading the store is Phase 0's real QA.

## Step 4 — `run_arms.py` (Terra ×2)

- Arm A: question only. Arm B: same + `legend_recall` tool (max 3 calls).
- One shared final instruction from `prompts.py`, identical across arms,
  including the abstain directive ("answer only if confident; otherwise say
  you don't know").
- Record everything: answers, full tool traces, recall frames shown, token
  usage → `runs/arms/answers.jsonl`.

Assert: both arms answered every surviving question; arm B frames non-error.

## Step 5 — `grade.py` (Sol as grader)

Verbatim SimpleQA grader prompt → `correct` / `incorrect` / `not_attempted`
per (question, arm), grader blind to arm. Output `results/grades.json` plus
the 3×3 transition table A→B.

## Step 6 — `report.py` + the hand-check worksheet

- `results/report.md`: transition table, per-outcome counts, net lift,
  attempted-rate per arm, store stats (elements/facts/aliases), token/cost
  totals from logged usage.
- **`results/flips.md`** — the manual-verification worksheet, one section per
  fixed flip, pre-assembled so the hand pass is minutes: question, gold, arm-B
  answer, the recall frames Terra saw (verbatim), grep hits for the gold answer
  in the store dump, and grep hits in the snapshot pages. Three checkboxes per
  flip: `in store` / `in frame shown` / `on page`. Same worksheet for broken
  and correct→abstain cases (what did the frame contain that hurt?).

## Step 7 — `run_all.sh`

Wipes `corpus/ store/ runs/ results/`, runs Steps 2–6, exits non-zero on any
stage assertion. First run at `n_questions: 1` end-to-end; flip config to 50
only when that's green.

## Estimates

| | |
|---|---|
| Sol ingestion (~60 pages, ~450k corpus tokens, loop overhead) | ~$8–15 |
| Terra arms (100 calls) + Sol grading (100 calls) | ~$2–4 |
| Wall-clock | ~1–2h, dominated by the sequential Sol pass |
| Human time | scaffold + scripts; then ~30 min reading `flips.md` and the store dump |

## Explicitly out of scope (full study only)

Dual ingester, RAG arms (D/D′/B0), ablations, Batch API, token parity, power
analysis, near-dup metric, Wayback fallback, mcp-serve warm process (revisit
only if per-call shell-out latency is annoying at 60 pages).
