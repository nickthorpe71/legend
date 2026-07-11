# SimpleQA Phase 0 harness

Signal check for the distillation hypothesis: does a Legend store built by a
strong model (Sol) let a consumer model (Terra) answer SimpleQA questions it
otherwise gets wrong? Design: [`docs/eval-simpleqa-distillation.md`](../../docs/eval-simpleqa-distillation.md) §0.
Plan: [`docs/eval-simpleqa-phase0-implementation.md`](../../docs/eval-simpleqa-phase0-implementation.md).

Pipeline: `fetch_corpus` (scrape Wikipedia sources, audit that gold answers are
present) → `ingest` (Sol builds one store) → `run_arms` (Terra answers ± a
`legend_recall` tool) → `grade` (SimpleQA grader, 3-way) → `report` (transition
table + `flips.md` hand-verification worksheet). No LLM uses web search; the
scrape is dumb HTTP.

## Run it

```bash
python3 -m venv .venv && . .venv/bin/activate
pip install -r requirements.txt

# 1. put an OpenAI key in the repo-root .env (gitignored):
#      OPENAI_API_KEY=sk-...
# 2. resolve real GPT-5.6 model IDs and edit config.json models.{ingester,consumer,grader}:
python preflight.py            # prints the gpt-5* models your key can see
# 3. end-to-end (preflight --write flips models_verified once IDs resolve):
./run_all.sh
```

Start with `config.json` `n_questions` small (1–5, smoke) to prove the plumbing,
then set it to 50 for the real run.

## Validate without spending tokens

Every OpenAI-dependent stage has a `--dry-run` that exercises the plumbing
(chunking, journaling, resume, the store round-trip) with no API calls:

```bash
python fetch_corpus.py          # real scrape, no LLM — safe to run anytime
python ingest.py --dry-run      # one deterministic save per chunk
python run_arms.py --dry-run    # recall against the store, stubbed answers
```

## Layout

- `config.json` — models, binary path, seed, fixed `LEGEND_NOW`, `n_questions`, pricing.
- `common/` — `legend_io` (subprocess wrapper), `oai` (the one place OpenAI is called),
  `schemas` (save/recall tool defs), `prompts` (ingester / answer / grader), `util`.
- `corpus/ store/ runs/ results/` — generated, gitignored. `run_all.sh` wipes them.

The numbers in `results/report.md` are not trustworthy until every flip in
`results/flips.md` is confirmed grounded (gold in the store **and** in a frame
Terra actually saw) — that hand pass is the point of Phase 0.
