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

## Results

_None yet._
