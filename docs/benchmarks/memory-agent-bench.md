# MemoryAgentBench

Chunk-by-chunk ingest, then
question — this is exactly Legend's tick/encode loop, and the four
competencies map cleanly onto the v2 design.

## Status

Not started.

## What it tests

Four memory competencies, evaluated incrementally:

1. **Accurate Retrieval (AR)** — recall specific facts from
   incrementally-seen context.
2. **Test-Time Learning (TTL)** — pick up a classification or
   recommendation policy from in-context examples.
3. **Long-Range Understanding (LRU)** — summarize / reason over the
   full ingested history.
4. **Conflict Resolution (CR)** — handle later facts that contradict
   earlier ones. The headline competency for any memory system that
   claims to *update* rather than *append*.

## Source

- Paper: [Evaluating Memory in LLM Agents via Incremental Multi-Turn Interactions](https://arxiv.org/abs/2507.05257) (Hu et al., ICLR 2026)
- Code: <https://github.com/HUST-AI-HYZ/MemoryAgentBench>
- Dataset: <https://huggingface.co/datasets/ai-hyz/MemoryAgentBench>

## Task format

Each sample is a sequence `(c₁, c₂, ..., cₙ)` of chunks followed by
questions `(q₁, ..., qₘ)`. Chunks arrive with an instruction prompting
the agent to memorize their contents. The agent must:

1. Ingest each chunk in order, updating memory.
2. After all chunks are ingested, answer each `qᵢ` using only memory
   (the chunks are no longer in context).

Chunk size is typically 512 or 4096 tokens. Multiple questions per
context to amortize ingest cost.

## Datasets

| Competency | Datasets |
|---|---|
| Accurate Retrieval | RULER-QA, RULER-NIAH-MQ, ∞-Bench-QA, LongMemEval (S*), **EventQA** (new) |
| Test-Time Learning | BANKING77, CLINC150, NLU, TREC-Coarse, TREC-Fine, Movie Recommendation (Redial) |
| Long-Range Understanding | ∞-Bench-Sum |
| Conflict Resolution | **FactConsolidation-SH**, **FactConsolidation-MH** (both new) |

EventQA and FactConsolidation are this paper's contributions; the
rest are repurposed long-context evals.

## Metrics

| Dataset | Metric |
|---|---|
| RULER-QA, FactConsolidation | SubEM (substring exact match) |
| RULER-NIAH-MQ | Recall |
| ∞-Bench-QA | ROUGE F1 |
| LongMemEval | GPT-4o-as-judge accuracy |
| EventQA, classification | Accuracy |
| Movie Recommendation | Recall@5 |
| ∞-Bench-Sum | GPT-4o-as-judge F1 |

## Baselines

- **Long-context LLMs:** GPT-4o, GPT-4o-mini, GPT-4.1-mini, Gemini-2.0-Flash, Claude-3.7-Sonnet
- **Simple RAG:** BM25
- **Embedding RAG:** Contriever, Text-Embed-3-Small/Large, NV-Embed-v2
- **Structure-augmented RAG:** RAPTOR, GraphRAG, HippoRAG-v2, Mem0, Cognee
- **Agentic memory:** Self-RAG, MemGPT

Mem0, HippoRAG-v2, GraphRAG, and Cognee are direct competitors — this
is the table Legend wants to be on.

## Integration shape

No documented "implement this trait" interface. Existing integrations
live as sibling directories:

```
methods/   — memory implementations
cognee/    — Cognee agent wrapper
letta/     — Letta wrapper
mem0/      — Mem0 wrapper
configs/   — agent + dataset YAMLs
bash_files/ — run scripts (e.g. run_memagent_rag_agents.sh)
main.py    — entry point
agent.py   — core loop
```

Path of least resistance: add a `legend/` sibling that exposes the
same shape as `mem0/`, talking to the Legend daemon over its TCP
MessagePack protocol.

Install: Python 3.10.16 (conda), `torch + requirements.txt + numpy<2`,
plus framework-specific deps for any baseline you also want to run.
API keys for OpenAI/Anthropic/Google via `.env`.

## Why this is the bench

- **Shape match.** Chunks-in, questions-out is what `tick` does.
- **FactConsolidation is the strongest case for the relation model.**
  Append-only RAG baselines should struggle with conflicting facts; a
  hypergraph that revises relations should win. If Legend doesn't
  beat append-only baselines on FactConsolidation, the v2 design
  isn't earning its complexity.
- **The competitor list is the right room.** Mem0, HippoRAG-v2,
  GraphRAG, Cognee are public, published, and beatable.
- **Headline finding from the paper:** long-context baselines stay
  competitive. The interesting cuts are CR and the harder AR cases
  where context doesn't fit. That's where Legend has to land its
  claim.

## Status

Harness exists for FactConsolidation: `examples/bench_memoryagentbench_fc.rs`.
Reads the `Conflict_Resolution-00000-of-00001.parquet` row that matches
`--variant`, spawns one Legend daemon per run, ingests each fact line as
a separate tick, then ticks the question and SubEM-matches the flattened
focused frame against the row's gold answers.

Dataset on disk: `benchmarks/memoryagentbench/conflict_resolution.parquet`
(downloaded once from the HF dataset).

Run:

```bash
cargo run --release --example bench_memoryagentbench_fc -- \
  --variant sh_6k --questions 10
```

## Results

### FactConsolidation-SH, 6k context, all 100 questions (2026-05-21)

| Run | Score | Change |
|---|---|---|
| v0 — out of the box | 77/100 (77.0%) | initial harness |
| v1 — drop `is_cache_relation` gate in `current_state` | 90/100 (90.0%) | `frame.rs:271–306` |
| v2 — SVO excludes proper-noun-run tokens from verb candidates | **91/100 (91.0%)** | `svo.rs:42–80` |

**v0 baseline (77.0%).** First end-to-end run, no Legend changes.

**v1 (90.0%).** Diagnostic on v0 revealed 22 of 23 misses had the gold
in the substrate but not the focused frame. Root cause: `current_state`
gated relation walks on `is_cache_relation`, which silently dropped
the underlying assertions when no `current_<property>` cache had been
minted for a `(subject, attribute)` bucket. Dropping that gate (one
edit in `src/steps/frame.rs`) surfaced the live assertions on referenced
elements and closed 13 of the 22 substrate-only misses. Status filter
remains (Asserted/Entailed/Defeasible only), so the supersede chain
still culls older retracted facts.

**v2 (91.0%).** Inspecting v1 misses with `--verbose-misses` showed
the Dave Filoni BBC case had the fact stored as fragments — `British |
Broadcasting | Corporation` as separate values — instead of one
`British Broadcasting Corporation` element. Root cause: SVO's
`is_verb_shape` classifies any token ending in `-ed`/`-ing`/`-en` as
a verb, so "Broadcasting" (-ing) was treated as a verb mid-phrase,
splitting object spans. General bug, not bench-specific — same break
on "Lockheed Martin" (-ed), "Working Class Hero" (-ing), "Wired
Magazine" (-ed). Fix: in `svo.rs`, compute proper-noun-run ranges
once per call and (a) skip phrases that *are* proper-noun runs from
clause iteration, (b) exclude verb candidates whose char range falls
inside any run. Net: 4 new hits on multi-word-object cases (Dave
Filoni BBC, Hal Needham USA, Stephen McNeil journalist, Karen
Armstrong Church of Scotland) minus 3 regressions (compound-subject
questions whose v1 hits depended on fragmented parses) = +1.

**Tradeoffs.**
- v1: flat-frame size grew from ~2k chars → ~11k chars after the gate-drop.
  More text for a downstream verbalizer to handle. Worth revisiting once
  a reading LLM is wired in — at that point the gate may want to come
  back with a different filter, or move behind a per-tick-mode flag.
- v2: substrate is smaller and cleaner (2,272 → ~2,306 → 2,306 elements
  for the same row in v0/v1/v2). Fewer fragmented relations = fewer
  "incidental substring" hits. The 3 regressions in v2 illustrate that
  some of v1's score came from noisy fragments rather than real
  retrieval. v2's hits are more honest.

**Why we stopped at 91%.** The remaining 9 misses don't share a
single root cause — compound subjects ("head of state of X"),
apostrophe titles ("Lady Windermere's Fan"), special characters
("Shinzō Abe"), and one genuine extractor gap ("Walter Chrysler's
child → Charles Frederick, Duke of Holstein-Gottorp"). Going further
means separate investigations per case with diminishing returns
relative to mh_6k and the larger context sizes, which are the better
next investments.

### FactConsolidation-MH, 6k context, all 100 questions (2026-05-21)

**81/100 SubEM hits (81.0%)** on a fresh substrate. Same harness, same
v2 substrate, just `--variant mh_6k`.

- Ingest: 455 facts in 127.3s (same row size as sh_6k, just different
  questions)
- Diagnostic split: **81 frame hits, 16 substrate-only, 3 absent**

The 3 absent gold answers are all comma-containing compound proper
nouns the chunker doesn't capture as one entity: "Philippe, Duke of
Orléans"; "Mansourah, Algeria"; "Charles Frederick, Duke of
Holstein-Gottorp" (same one missed on sh_6k for Walter Chrysler).
Same extractor gap, different question.

### Multi-hop validity caveat (important — read before quoting 81%)

The 81% number is real *for what it measures*: does the gold answer
appear in the flattened focused frame after the question tick? But
**it is not directly comparable** to the published baselines on
FactCon-MH:

- Published baselines feed their memory format to an **LLM that
  generates a text response**. SubEM checks the LLM's response. The
  LLM has to actually emit the answer string. GPT-4o at mh_6k = 28.0
  means GPT-4o composed and emitted the right answer for 28 of 100
  questions.
- Legend has **no LLM in the answer loop yet**. SubEM checks the
  flattened frame — the focused subgraph dump. The 81 means the gold
  *entity* appears somewhere in the surfaced relations.

For these multi-hop questions, Legend isn't composing the chain.
Example: "Which country is the birthplace of the sport associated
with Steve Sax?" → gold "Italy". Legend's frame for this question
references multiple noun phrases (Steve Sax, sport, country,
birthplace). Each routes to elements; `current_state` walks all live
relations on each (after the v1 gate-drop). The frame ends up
carrying many relations across many entities. "Italy" appears in
that soup because at least one relation among them contains it as a
value. SubEM substring-matches it.

What this number *does* mean:
- Legend's substrate retains the right entities for 97/100 mh_6k
  questions (substrate-only + frame hits = 97).
- Legend's retrieval surfaces those entities into the focused frame
  for 81 of 100.
- That's a real claim about substrate + retrieval coverage on
  compositional questions. Strong enough that a downstream
  verbalizer would have the right entity available to compose with.

What this number *does not* mean:
- Legend "solved multi-hop reasoning."
- Legend's 81 beats GPT-4o's 28 in any directly comparable sense.

The proper apples-to-apples comparison is **frame → reading LLM
verbalizer → SubEM**. Until that's wired in, treat the mh_6k 81 as a
retrieval-coverage signal, not as an answer-generation score. Project
memory `project_attention_frame_no_answer.md` is the design rationale:
the tick output is a focused subgraph, not an answer.

### What both sh_6k and mh_6k reveal together

| | sh_6k | mh_6k |
|---|---:|---:|
| Frame hits | 91 | 81 |
| Substrate-only (retrieval gap) | 8 | 16 |
| Absent (extractor gap) | 1 | 3 |
| Substrate coverage (hits + substrate-only) | 99 | 97 |

The substrate-coverage row is the most informative cut. **Legend's
extractor + supersede + storage layers are doing 97-99% of the work
on FactConsolidation at 6k.** The losses sit in retrieval (which
relations surface in the focused frame) and in extracting comma-
separated multi-clause proper nouns.

### FactConsolidation-SH, 32k context, all 100 questions (2026-05-21)

**90/100 SubEM hits (90.0%).** Same harness, same v2 substrate,
`--variant sh_32k` — 2,310 facts (5× more than sh_6k) packed into
the same 100-question row.

| | sh_6k | sh_32k |
|---|---:|---:|
| Frame hits | 91 | **90** |
| Substrate-only | 8 | 8 |
| Absent | 1 | 2 |
| Ingest time | 130s | 859s |
| Facts ingested | 455 | 2,310 |
| Substrate coverage (hits + substrate-only) | 99 | 98 |

**Scaling story vs the published baselines (paper Table 3):**

| System | sh_6k → sh_32k | Δ |
|---|---|---:|
| O4-mini | 100.0 → 61.0 | **-39.0** |
| GPT-4o | 92.0 → 88.0 | -4.0 |
| **Legend** | **91.0 → 90.0** | **-1.0** |

Legend's per-row score barely moves between 6k and 32k context. The
substrate-only count holds at 8; the only regression is one extra
extractor absence. Same caveats as before — Legend's score measures
"gold entity in focused frame," not "model emits the right answer."
But the *shape* of the scaling curve (very shallow drop) is a real
signal that the substrate isn't suffering interference from carrying
5× more facts.

Per-fact ingest cost climbs modestly: 287 ms/fact → 372 ms/fact.
That's ~30% slower per fact at 5× substrate size, mostly attributable
to the by-name lookup paths (HashMap lookups in `hg.by_name` slow
down with more elements) and the linear scans inside the supersede
gate. Both are addressable but not bottlenecks for the bench.

### FactConsolidation-MH, 32k context, all 100 questions (2026-05-21)

**78/100 SubEM hits (78.0%).** Same 2,310-fact row as sh_32k, just
the multi-hop question variant.

| | mh_6k | mh_32k |
|---|---:|---:|
| Frame hits | 81 | **78** |
| Substrate-only | 16 | 22 |
| Absent | 3 | **0** |
| Substrate coverage (hits + substrate-only) | 97 | **100** |

**100% substrate coverage on mh_32k** — every gold answer is in the
substrate; the only misses are retrieval. Extractor caught
everything across 2,310 facts.

**Scaling vs published baselines (paper Table 3, FactCon-MH):**

| System | mh_6k → mh_32k | Δ |
|---|---|---:|
| O4-mini | 80.0 → 14.0 | **-66.0** |
| GPT-4o | 28.0 → 10.0 | -18.0 |
| **Legend** | **81.0 → 78.0** | **-3.0** |

Same validity caveat — Legend surfaces, doesn't compose. But the
shape of the curve is the real story: O4-mini's multi-hop reasoning
collapses by 66 points going from 6k to 32k context; Legend's
substrate-mediated retrieval drops 3.

### Aggregate-shaped table after 6 runs (2026-05-21)

|  | sh_6k | sh_32k | sh_64k | mh_6k | mh_32k | mh_64k |
|---|---:|---:|---:|---:|---:|---:|
| Frame hits | **91** | **90** | **89** | **81** | **78** | **74** |
| Substrate-only (retrieval gap) | 8 | 8 | 10 | 16 | 22 | 25 |
| Absent (extractor gap) | 1 | 2 | 1 | 3 | 0 | 1 |
| Substrate coverage | 99 | 98 | 99 | 97 | 100 | 99 |
| Facts ingested | 455 | 2,310 | 4,580 | 455 | 2,310 | 4,580 |
| Ingest time | 130s | 859s | 1,602s | 127s | 872s | 1,543s |

The "substrate coverage" row is what to point at when discussing
Legend's claim: **the substrate retains 97–100% of all gold answers
across this 3×2 grid of context size × hop count.** Everything else
is retrieval cost, which is in scope to keep improving.

### Scaling curve vs published baselines

Per-row drop in score going from 6k to higher context. Paper Table 3
only publishes per-context-size numbers for GPT-4o and O4-mini at 6k
and 32k, so the rightmost columns are unpublished for baselines.

| System | SH drop 6k→32k | SH drop 6k→64k | MH drop 6k→32k | MH drop 6k→64k |
|---|---:|---:|---:|---:|
| O4-mini | -39 (100→61) | _unpublished_ | -66 (80→14) | _unpublished_ |
| GPT-4o | -4 (92→88) | _unpublished_ | -18 (28→10) | _unpublished_ |
| **Legend** | **-1** (91→90) | **-2** (91→89) | **-3** (81→78) | **-7** (81→74) |

Validity caveat from the earlier mh_6k discussion still applies —
Legend's number measures "gold entity surfaces in focused frame,"
not "model composes the right answer." But the *shape* of the curve
is a real claim: Legend's drop with context expansion is materially
smaller than any published baseline's drop on the same expansion.

### Per-fact ingest cost grows with substrate size

| Variant | Facts | Ingest | ms/fact |
|---|---:|---:|---:|
| sh_6k | 455 | 130s | 287 |
| sh_32k | 2,310 | 859s | 372 |
| sh_64k | 4,580 | 1,602s | 350 |

Per-fact cost climbs ~30% from 6k to 32k but doesn't keep growing
linearly — sh_64k actually came in a hair faster per-fact than
sh_32k. Substrate-size-dependent costs (by-name lookups, supersede
prior scans) appear to plateau or sublinear in practice. A full
262k run would take ~3+ hours on this hardware, which is why it's
not yet wired into the regression test.

### EventQA 65k — not a comparable claim (2026-05-21)

**Reported number:** 100/100 frame hits on EventQA at 65K-token
context. Ingested 2,485 sentence chunks in 2,500s.

**Why this number is not comparable to the published baselines:**

| | What's measured | Scoring |
|---|---|---|
| Paper's EventQA | LLM picks one of 6 continuation options | Accuracy on multiple-choice |
| Legend (this harness) | Gold sentence appears as substring of focused frame | SubEM |

EventQA is a **6-way multiple-choice** task per the paper. The
published numbers (GPT-4o 77.2, BM25 74.6, NV-Embed-v2 72.8, Mem0
37.5, Cognee 26.8, etc.) measure "did the LLM pick the correct
continuation from 6 options." Legend's 100/100 measures "is the
gold sentence anywhere in the focused frame" — trivially satisfied
because we chunked at sentence level on ingest, so every source
sentence is in the substrate verbatim and gets pulled into the
frame when the question references the same entities.

**What this run *does* tell us:**
- Legend's sentence-level ingest scales to ~2,500 chunks per row
  without losing sentences (substrate coverage stays at 100%).
- The retrieval/focus path surfaces source sentences for *every*
  EventQA question — the v1 gate-drop + referenced-element walk is
  pulling enough of the substrate into the frame to satisfy SubEM
  for any prose-style question.

**What it does not tell us:**
- Whether Legend, with a downstream LLM verbalizer, could pick the
  correct option from 6. That requires (a) the multiple-choice
  options (the dataset has them but the harness doesn't load them),
  and (b) an LLM to read the focused frame and choose. Neither
  exists yet.

**Cost note:** 2,485 sentences took 2,500s to ingest (~1.0s/sentence)
vs ~287 ms/fact for CR. Prose sentences are longer than CR fact lines
and the substrate-growth tax compounds: at 2,485 elements minted
by the end, every new tick pays more per-name lookup cost.

### Headline conflict-resolution hits (Legend picks the *later* fact)

| Q | Original fact | Contradicting fact (gold) | Legend |
|---|---|---|---|
| Microsoft CEO | Satya Nadella | **Steve Jobs** | ✓ |
| BBC director | Tony Hall | **Narendra Modi** | ✓ |
| Sidereus Nuncius author | Galileo | **Samuel Beckett** | ✓ |
| Tunisia football team sport | association football | **basketball** | ✓ |
| Goaltender associated sport | hockey | **pesäpallo** | ✓ |
| Robert Parish position | center | **quarterback** | ✓ |

### Miss patterns (23 misses)

Two failure modes mixed together — worth distinguishing before
deciding what to fix:

**(a) Likely retrieval-cue mismatch — fact in substrate but not
focused.** Examples: "Ferdowsi famous for" → Shahnameh, "Endymion
author" → John Keats, "Lady Windermere's Fan author" → Oscar Wilde.
These look like single-hop lookups on proper nouns; the data should
be in the substrate from ingest. The frame just didn't surface them.

**(b) Phrasing-distance / long-answer misses.** "Joan Didion educated
at" → "University of California, Berkeley" (long multi-word with
comma), "Dave Filoni employer" → "British Broadcasting Corporation"
(four-word proper noun). SubEM is a strict substring match — if Legend
stored "UC Berkeley" instead of the full canonical form, it misses
even with the right concept.

Investigation priority: dump the substrate after ingest and check
whether the (a)-class misses are actually present. If they are, this
is a retrieval-frame problem, not a substrate problem.

### Published baselines (paper Tables 2 & 3)

Paper aggregates SH scores across all four context sizes (6k / 32k /
64k / 262k) into one number — that's the column most baselines report.
Only GPT-4o and O4-mini have per-size validation runs (Table 3).

| System | FactCon-SH (agg) | FactCon-MH (agg) | FactCon-SH @ 6k | FactCon-MH @ 6k |
|---|---:|---:|---:|---:|
| O4-mini | — | — | **100.0** | **80.0** |
| GPT-4o | 60.0 | 5.0 | **92.0** | 28.0 |
| **Legend (this work, v2)** | _pending_ | _pending_ | **91 / 90 / 89**¹ | **81 / 78 / 74**¹ ⚠ |
| Legend (this work, v1) | _pending_ | _pending_ | 90.0 | _pending_ |
| Legend (this work, v0) | _pending_ | _pending_ | 77.0 | _pending_ |

⚠ See "Multi-hop validity caveat" below — Legend's mh_6k number is
not directly comparable to baselines on the same row.

¹ Legend's values reported as <6k> / <32k> / <64k>. Paper Table 3
only publishes per-context-size numbers for GPT-4o and O4-mini and
only at 6k and 32k:
- SH: GPT-4o 92→88 (-4), O4-mini 100→61 (-39), **Legend 91→90→89 (-1, -2)**.
- MH: GPT-4o 28→10 (-18), O4-mini 80→14 (-66), **Legend 81→78→74 (-3, -7)**.
| BM25 | 56.0 | 3.0 | — | — |
| NV-Embed-v2 | 55.0 | 6.0 | — | — |
| HippoRAG-v2 | 54.0 | 5.0 | — | — |
| GPT-4o-mini | 45.0 | 5.0 | — | — |
| Claude-3.7-Sonnet | 43.0 | 2.0 | — | — |
| GPT-4.1-mini | 36.0 | 5.0 | — | — |
| Gemini-2.0-Flash | 30.0 | 3.0 | — | — |
| Text-Embed-3-Small | 28.0 | 3.0 | — | — |
| Text-Embed-3-Large | 28.0 | 4.0 | — | — |
| Cognee | 28.0 | 3.0 | — | — |
| MemGPT | 28.0 | 3.0 | — | — |
| Self-RAG | 19.0 | 3.0 | — | — |
| Mem0 | 18.0 | 2.0 | — | — |
| Contriever | 18.0 | 7.0 | — | — |
| RAPTOR | 14.0 | 1.0 | — | — |
| GraphRAG | 14.0 | 2.0 | — | — |

**Paper's headline finding on CR:** *"all methods fail on the
multi-hop situation (with achieving at most 6% accuracy). Only long
context agents can achieve fairly reasonable results on single-hop
scenarios."* The strongest CR system in the entire baseline table is
GPT-4o (60.0 aggregate, 92.0 at sh_6k). Every dedicated memory system
— Mem0, HippoRAG-v2, Cognee, MemGPT, RAPTOR, GraphRAG — is below
60 aggregate.

### How Legend's 77% lands

Two ways to read it. Both matter.

**Per-context-size (sh_6k vs sh_6k).** Legend's 77.0 sits between
the small-model long-context tier and GPT-4o's 92.0. We're not in
GPT-4o's tier yet at this single size, but we're well above any
small-model long-context baseline. The other RAG / memory baselines
don't publish per-size numbers, so the closest comparison points
are their aggregates.

**Aggregate (Legend at 6k vs baselines' avg over all sizes).** Not a
fair comparison — the baselines are dragged down by 32k/64k/262k.
But it's the only column the rest of the table has, and at 77 Legend
is currently above every dedicated memory baseline (HippoRAG-v2 54,
Cognee 28, Mem0 18, MemGPT 28, GraphRAG 14, RAPTOR 14, Self-RAG 19).
That gap will narrow once we run Legend across the larger contexts.

To make the comparison clean: **run Legend across sh_32k, sh_64k,
sh_262k and report Legend's own aggregate.** That's the apples-to-
apples number against the baseline aggregates.

### Where the multi-hop opportunity is

Every published baseline scores ≤7% on FactCon-MH aggregate. Even
GPT-4o is at 5.0 aggregate (28.0 at mh_6k). If Legend's relation
model can compose facts across the hypergraph, this is the
competency where it has the most room to claim a real win. mh_6k
is the next bench run.

### Remaining 10 misses after v1 (1 substrate-absent + 9 frame-misses)

The verbose-misses dump shows the question's subject IS reaching
`referenced_elements` (e.g. "Dave Filoni" appears 8× in the flattened
frame for the Dave Filoni question) — but only the *original* fact
relation surfaces, not the contradicting later one. So the remaining
gap looks like a **supersede / conflict-resolution failure**, not a
referenced-elements failure. Substrate has both old and new; the new
one is in the substrate's elements but is not making it into the
focused relation set for the question tick.

The one true substrate-absent miss is "Walter Chrysler's child →
Charles Frederick, Duke of Holstein-Gottorp." That's a long
multi-clause proper noun the extractor never captured as one entity.
Genuine extractor gap, 1/100.

Going from 90% → 95+ on sh_6k looks like Step 9 (supersede) work.
That's substrate-level — bigger surface area than the v1 fix.

### Caveats

- **SubEM is the metric, but it's harsh.** Strict substring miss for
  paraphrased-but-correct answers is the bench's standard scoring
  rule. We don't soften it.
- **One row only.** sh_6k is the smallest variant.
- **No multi-hop number yet.** That's the real test for the relation
  model. Coming next.

### Next steps

1. **mh_6k.** The competency where every published baseline fails.
   Single highest-information next run.
2. **sh_32k, sh_64k, sh_262k** — to compute Legend's own aggregate
   and compare apples-to-apples against the baseline aggregate
   column.
3. **`--dump-misses` flag** — for each miss, also dump whether the
   gold string appears anywhere in the full substrate (not just the
   focused frame). That cleanly separates (a) vs (b) above.
