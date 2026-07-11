# SimpleQA Distillation Eval — Design Doc

Status: DRAFT — 2026-07-11. **Active scope is Phase 0 (§0).** Sections 1–12 are
the full study, to run only if Phase 0 shows signal; adversarial-review findings
(2026-07-11) get folded into §1–12 before the full study runs.
Depends on: `v3_minimal` branch (pure tool + deduped reality graph, MCP `legend_save` / `legend_recall`)

## 0. Phase 0 — minimal signal check (current step)

One question: **does a Sol-built Legend store lift Terra on SimpleQA at all?**
Two arms, one ingester, no RAG, no dual stores. If this shows nothing, nothing
downstream matters; if it shows lift, the full study's job is attribution.

- **Questions:** ~50, deterministic sample (fixed seed, ≥1 Wikipedia source).
- **Corpus:** fetch each question's source pages in full (boilerplate stripped,
  no length cap), snapshot to disk. Cheap string-match audit: gold answer
  findable in ≥1 page, else drop the question and report the count.
- **Ingest:** Sol runs the recall-before-save loop over the snapshot,
  sequentially, into one fresh store. Journal every tool call; resume = replay
  high-water mark from the journal.
- **Arms:** Terra answers each question (a) bare, (b) with a `legend_recall`
  tool against the store, max 3 calls. Same final answer/abstain instruction.
- **Grade:** verbatim SimpleQA 3-way grader (Sol as grader — cheap at this
  volume, and not the consumer).
- **Read-out:** the full baseline→legend transition table (fixed, broken,
  correct→abstain, abstain→correct — all three outcomes, both directions), and
  **manual inspection**: read the store dump; for every fixed flip, check by
  hand that the answering fact (1) exists in the store, (2) appeared in the
  recall frame Terra saw, and (3) is on the snapshot page (grounding — catches
  de-abstention artifacts and Sol writing memorized answers, the two ways a
  fake positive happens).
- **Decision:** real net lift with hand-verified surfaced facts → proceed to
  the full study (§1–12, hardened per the review). No lift → triage the ~50
  misses by hand before spending anything more.
- **Cost:** ~$10–20 total; wall-clock dominated by one sequential Sol pass over
  ~60 pages.

What Phase 0 deliberately does not tell us: whether lift comes from structure
vs extraction (no RAG arm), or whether it transfers from a stronger ingester
(no second store). Those are the full study's questions.

Implementation: [eval-simpleqa-phase0-implementation.md](eval-simpleqa-phase0-implementation.md).

## 1. Purpose

Test whether a Legend store works as a **transferable knowledge artifact**: a strong
model ingests a corpus into a fresh store once, and a weak model consumes that store
at inference time. Three claims, each with its own measurement:

| Claim | Measurement |
|---|---|
| **Distillation** — the ingester's intelligence is captured in the store and transfers downstream | acc(Luna + Sol-built store) − acc(Luna + Luna-built store) |
| **Self-bootstrap** — the tool adds capability even with no strong model in the loop | acc(Luna + Luna-built store) − acc(Luna alone) |
| **Structure** — the deduped graph earns something over dumb retrieval | acc(Luna + Sol-built store) − acc(Luna + naive RAG over the same corpus) |

A negative result on any row is informative. If the two stores perform the same,
ingestion is mechanical and the expensive model is wasted there. If RAG matches
Legend, the structure isn't paying for itself on this task shape.

## 2. Benchmark

**SimpleQA** (OpenAI, arXiv:2411.04368; dataset in `github.com/openai/simple-evals`).
4,326 short fact-seeking questions, adversarially collected so models fail; each
question carries reference answer(s) plus **source URLs from two independent
trainers**. Grading is a fixed judge prompt with three outcomes: `correct`,
`incorrect`, `not_attempted`.

Why it fits:

- **Headroom.** Weak-model baselines are low, so the flippable set is most of the
  benchmark, not a sliver (contrast: ChemBench, where Haiku-class models already
  hold the knowledge parametrically and fail mostly on arithmetic).
- **One fact per question** — maps directly onto Legend's `{s,p,o}` triples. No
  calculation confound.
- **The corpus comes with it** — source URLs make corpus construction mechanical.

**Subset:** 500 questions, stratified by the dataset's topic metadata, fixed seed,
frozen as `questions.jsonl` before any ingestion run. (Alternative: *SimpleQA
Verified*, Google's cleaned 1,000-question subset — decide in review; see §12.)

**Pilot gate:** run the full pipeline on 100 questions first. Proceed to 500 only
if the pilot shows (a) baseline Luna accuracy < 40% on the subset and (b) at least
one Legend arm shows a fixed-flip rate meaningfully above its broken-flip rate.

## 3. Corpus construction

1. For each sampled question, collect the union of both trainers' source URLs.
2. Dedupe by URL across the whole subset (many questions share Wikipedia pages).
3. Fetch each page once; extract **full main content** (readability-style
   extraction). Strip boilerplate only — navigation, references markup,
   external-link lists — never body content. **No length cap**: SimpleQA's tail
   facts often live deep in long pages (tables, filmographies, award lists), and
   truncation would manufacture fake ingestion failures that pollute the §7
   triage. If corpus size blows past the §10 estimate, the fallback is a high
   soft cap (~15k) with audit-driven (step 5) uncapped re-fetch of the pages
   that fail it — not a blanket cap.
4. **Snapshot to disk** (`corpus/pages/*.md` + `corpus/manifest.jsonl` with URL,
   fetch date, token count). All arms and both ingesters read from the snapshot,
   never the live web — pages drift, and reproducibility matters more than
   freshness here.
5. **Corpus-support audit.** After snapshotting, verify each question's gold
   answer is actually findable (string/fuzzy match, Terra-confirmed on
   ambiguous cases) in at least one of its snapshot pages. Questions that fail
   are marked `corpus_unsupported` and excluded from scoring (count reported).
   This catches page drift — the benchmark was collected in 2024 and live pages
   have been edited since; some answers no longer exist on the current revision.
   The audit is strictly corpus-side: ingesters still never see questions, and
   no page content is altered based on it.

**Question-blindness.** Ingesters see pages only, never questions. Page *selection*
is question-derived (unavoidable; disclosed), but every extraction decision is
question-blind: the ingester must compress a ~6k-token page into elements and facts
without knowing which fact will be probed. That is the real task — did the needed
fact survive compression, naming, and dedup?

Expected size: ~500–800 unique pages, uncapped ≈ 4–8M corpus tokens.

## 4. Models

All experiment models are OpenAI GPT-5.6 family (released 2026-07-09). Exact API
model IDs TBD — verify against OpenAI's model docs before coding; do not guess.

| Role | Model | Pricing (per M, in/out) | Why |
|---|---|---|---|
| Strong ingester | **Sol** | $5 / $30 | Flagship; the "comprehension once" arm |
| Weak ingester + consumer | **Luna** | $1 / $6 | Budget tier; the model we're trying to lift |
| Judge + error triage | **Terra** | $2.50 / $15 | Mid tier; runs the fixed SimpleQA grader prompt |
| RAG embeddings | text-embedding-3-small (or current equivalent) | ~negligible | Keeps the stack all-OpenAI |

Optional third ingester point: **Terra** (~+$25–30) — turns the two-point
comparison into a capability curve. Decide in review (§12).

Legend itself is model-agnostic (C binary); nothing Anthropic-specific is in the
loop, which also keeps the eval provider-clean.

## 5. Arms

Four consumption arms, all with **Luna as the answering model**, identical answer
prompt, temperature/default settings identical across arms:

| Arm | Context provided |
|---|---|
| A. Baseline | Question only |
| B. Legend (Sol store) | Question + `legend_recall` tool access against the Sol-built store |
| C. Legend (Luna store) | Question + `legend_recall` tool access against the Luna-built store |
| D. Naive RAG | Question + top-k chunks retrieved from the snapshot corpus |

**RAG arm spec:** chunk snapshot pages at ~500 tokens with overlap, embed with the
OpenAI embedding model, cosine top-k. **Token-budget parity:** tune k so the RAG
context ≈ the median tokens injected by recall in arms B/C. Otherwise we're
measuring context budget, not structure.

**Legend arms:** Luna gets a function-calling tool mirroring the `legend_recall`
MCP schema (executed by shelling to the legend binary with `LEGEND_STATE_DIR`
pointing at the arm's store). Max 3 recall calls per question, then it must
answer. `recall` is non-mutating by default (`observe: false`) — no saves during
eval. Recall runs with `LEGEND_EMBED=1` (full tier chain: exact → alias →
lexical → embedding). An optional **ablation** re-runs arm B with
`LEGEND_EMBED=0` to isolate the embedding tier's contribution — same store, same
questions, recall-only, so it costs only eval tokens.

## 6. Ingestion protocol

One fresh store per ingester: `stores/sol/`, `stores/luna/`.

- **Sequential, fixed page order** (same order for both ingesters) — dedup
  discipline requires each page's recall-before-save to see prior pages' saves,
  so ingestion cannot be batched or parallelized.
- Per page: the ingester model runs an agentic loop with `legend_recall` and
  `legend_save` function-calling tools (mirroring the MCP schemas), instructed
  with Legend's standard discipline: recall before save, reuse canonical names
  verbatim, `changes` for updated values, few precise elements over many vague
  ones.
- **Identical prompt and loop for both ingesters.** The only variable is the model.
- Journal every tool call per ingester (`runs/<ingester>/journal.jsonl`) — this is
  the raw material for the artifact metrics (§8) and post-hoc debugging.
- Failure handling: a page whose loop errors after 2 retries is logged and skipped
  **for both ingesters** (keep corpora identical); skipped pages' questions are
  excluded from scoring.

## 7. Grading and primary metrics

Grade all four arms with the **unmodified SimpleQA grader prompt** on Terra
(`correct` / `incorrect` / `not_attempted`).

Primary metrics, computed per arm and per topic stratum:

- **Accuracy** (3-way outcomes reported; headline = %correct).
- **Flips vs baseline, both directions:**
  - *fixed*: baseline `incorrect`/`not_attempted` → arm `correct`
  - *broken*: baseline `correct` → arm `incorrect`
  Broken-flips are the number nobody reports and the one that decides whether a
  memory system distracts more than it helps. Report fixed − broken as net lift.
- **The three claim deltas** from §1, with bootstrap confidence intervals over
  questions (resample questions, 10k iterations).

`not_attempted` is tracked separately throughout — GPT-5.6 models may be tuned to
abstain rather than hallucinate, and abstain→correct is a cleaner win than
wrong→correct.

### Error triage (the diagnostic core)

Every Legend-arm `incorrect`/`not_attempted` gets classified into exactly one of:

1. **Ingestion failure** — the fact never made it into the store. Check: string/
   alias search for the gold answer in the store dump (`--pretty` export), judge-
   confirmed by Terra.
2. **Retrieval failure** — fact present in the store but recall didn't surface it
   in what Luna saw (compare the fact against the recorded recall outputs).
3. **Consumption failure** — the fact was surfaced and Luna still answered wrong.

This decomposition tells us *where* the pipeline leaks — and specifically
quantifies the known recall gap (§9.1) instead of letting it masquerade as an
ingestion-quality result.

## 8. Store-artifact metrics

The stores are inspectable artifacts; compare them directly, before any question
runs:

| Metric | How |
|---|---|
| Element count, fact count, facts/element | Store dump |
| Alias usage, merge count, retract/change counts | Journal + dump |
| Near-duplicate rate | Embed element names+summaries with `embed.c` (in-repo BGE-small-en-v1.5 — the same embedder recall uses); count pairs above cosine ~0.9 that share no alias/merge link |
| Store size (bytes), tokens-in → facts-out compression ratio | Manifest + dump |

Prediction to test: the Luna store shows higher near-dup rate and higher
facts/element noise; if it *doesn't*, but still underperforms, the gap is in fact
selection/summarization quality rather than dedup discipline.

## 9. Known risks

1. **Embedding-tier recall is unproven at this scale.** The full tier chain
   (exact → alias → lexical → embedding, BGE-small asymmetric retrieval) is live
   in the binary, but it has only been exercised on stores of a few hundred
   elements. At 15–50k facts, embedding rank quality and lexical-tier noise are
   unmeasured. The §7 triage quantifies the cost (retrieval-failure bucket), and
   the `LEGEND_EMBED=0` ablation (§5) separates the embedding tier's
   contribution from the exact/alias/lexical tiers.
2. **Benchmark contamination.** SimpleQA is public since late 2024 and OpenAI
   publishes scores on it; GPT-5.6 may have unusually strong parametric coverage
   or tuned abstention. Mitigation: the pilot gate (§2) checks baseline headroom
   before we spend on full ingestion; if Luna's baseline is already high,
   substitute SimpleQA Verified or a held-out slice, or accept reduced power.
3. **Teach-to-test critique.** Ingesting the questions' own source pages is
   deliberate and disclosed; the RAG arm sees the identical corpus, so the
   *comparative* claims (§1) are unaffected. Only the absolute accuracy numbers
   carry the caveat.
4. **Store scale.** ~4–8M corpus tokens may yield 15–50k facts — well beyond
   anything the store has held. Watch ingestion latency per page as the store grows (recall
   cost scales with store size) and orientation-packet/recall-limit behavior.
5. **Judge bias.** One judge (Terra) grades all arms with the fixed public
   prompt; arms are graded blind (judge never sees which arm produced an answer).

## 10. Cost estimate

Assumes ~4–8M corpus tokens (uncapped pages, §3), agentic-loop overhead ~2.3× on
input, output ≈ 0.27× corpus per ingestion pass; 500 questions × 4 arms; prices
from §4. OpenAI batch (50%) applies to eval, grading, and the audit (independent
calls) but **not** ingestion (sequential); prompt caching reduces ingestion input
further (not counted below).

| Item | Est. |
|---|---|
| Ingestion — Sol (~9–18M in / 1–2.2M out) | ~$90–150 |
| Ingestion — Luna | ~$18–30 |
| Eval — 4 arms × 500 q on Luna (~7M in / 0.6M out, batched) | ~$6–11 |
| Grading + triage + corpus audit on Terra (batched) | ~$6–10 |
| Embeddings (RAG + near-dup metric) | <$1 |
| **Total (two ingesters, 500 q)** | **~$125–200** |
| Pilot (100 q, both ingesters) | ~$30–45 |
| Optional Terra ingester | +$45–75 |

## 11. Harness layout

```
benchmarks/simpleqa/     # sibling of benchmarks/memoryagentbench
  fetch_corpus.py        # sample questions, fetch+snapshot sources, audit, manifest
  ingest.py              # agentic loop: one (model, store) pass over the snapshot
  run_arms.py            # arms A–D via OpenAI Batch where possible
  grade.py               # SimpleQA grader + error triage on Terra
  report.py              # metrics tables, deltas + CIs, artifact metrics
  corpus/  stores/  runs/  results/
```

Python (OpenAI SDK + batch API); shells out to the **dev build** of the legend
binary (per-store isolation via `LEGEND_STATE_DIR`, fixed `LEGEND_NOW` for
reproducibility). Stores and snapshots are gitignored (store-commit policy TBD);
manifests, configs, and results are committed. Run order: `fetch_corpus` → pilot
(`ingest` ×2 → `run_arms` → `grade` → `report`) → review pilot gate → full run.

## 12. Open questions for review

1. **Subset:** 500 stratified from original SimpleQA, or SimpleQA Verified (1,000,
   cleaned, but confirm it retains source URLs)?
2. **Third ingester (Terra)** for a three-point capability curve — worth +$30?
3. ~~Harness location~~ — resolved: `benchmarks/simpleqa/`, sibling of the
   existing `benchmarks/memoryagentbench`.
4. **OpenAI account:** API key with Batch API access available? Exact GPT-5.6 API
   model IDs need verifying at coding time.
5. **`LEGEND_EMBED=0` ablation** (§5): include in the walking skeleton, or defer
   to the pilot?
