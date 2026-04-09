# LongMemEval Benchmark for Legend

Evaluates Legend's memory system against the [LongMemEval](https://github.com/xiaowu0162/LongMemEval) benchmark (ICLR 2025). 500 questions testing 5 memory abilities: information extraction, multi-session reasoning, knowledge updates, temporal reasoning, and abstention.

## Setup

```bash
# 1. Install dependencies
pip install anthropic   # for the reading/answering step

# 2. Download the dataset
cd benchmarks/
curl -L -o longmemeval_oracle.json \
  "https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned/resolve/main/longmemeval_oracle.json"

# 3. Build Legend
cd ..
cargo build --release

# 4. Set API key (for the LLM reading step)
export ANTHROPIC_API_KEY=your_key_here
```

## Run

```bash
cd benchmarks/
python run_longmemeval.py --dataset longmemeval_oracle.json --output results.jsonl
```

Options:
- `--dataset` — path to LongMemEval JSON file (default: `longmemeval_oracle.json`)
- `--output` — path for hypothesis JSONL output (default: `results.jsonl`)
- `--model` — Claude model for the reading step (default: `claude-haiku-4-5-20251001`)
- `--limit` — only run first N questions (for testing)
- `--legend` — path to Legend binary (default: `../target/release/legend`)

## Score

Use the official LongMemEval evaluator:

```bash
git clone https://github.com/xiaowu0162/LongMemEval.git
cd LongMemEval/src/evaluation
python evaluate_qa.py gpt-4o ../../benchmarks/results.jsonl ../../data/longmemeval_oracle.json
python print_qa_metrics.py gpt-4o ../../benchmarks/results.jsonl.log ../../data/longmemeval_oracle.json
```

## How It Works

For each of the 500 questions:

1. **Ingest** — Replay all `haystack_sessions` through Legend's `tick` pipeline (fresh BrainState per question)
2. **Query** — Use Legend's `retrieve_context` to find relevant memories
3. **Read** — Pass retrieved context + question to Claude Haiku to generate an answer
4. **Write** — Output `{"question_id": ..., "hypothesis": answer}` to JSONL

The benchmark measures Legend's encoding + retrieval quality. The LLM reading step is standardized (same model, same prompt) so the variable is Legend's memory system.

## Expected Challenges

- **Multi-session reasoning** (133 questions) — Legend consolidates, which may lose scattered details
- **Knowledge updates** (78 questions) — Legend's merge/reconsolidate should help here
- **Temporal reasoning** (133 questions) — Legend doesn't do date-aware retrieval yet

## Baselines

| System | Score |
|--------|-------|
| Full-context GPT-4o | 60-64% |
| MemPalace (raw) | 96.6% |
| MemPalace (hybrid) | 100% |
| Hindsight | 91.4% |
| agentmemory | 96.2% |
