#!/usr/bin/env python3
"""
LongMemEval benchmark harness for Legend.

For each question:
1. Replay haystack sessions through Legend's tick pipeline
2. Query Legend for relevant context
3. Use Claude to answer the question from retrieved context
4. Write hypothesis to JSONL

Usage:
    python run_longmemeval.py --dataset longmemeval_oracle.json --output results.jsonl
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def parse_args():
    parser = argparse.ArgumentParser(description="Run LongMemEval benchmark against Legend")
    parser.add_argument("--dataset", default="longmemeval_oracle.json", help="Path to LongMemEval JSON")
    parser.add_argument("--output", default="results.jsonl", help="Output hypothesis JSONL")
    parser.add_argument("--model", default="claude-sonnet-4-5-20250929", help="Claude model for reading step")
    parser.add_argument("--limit", type=int, default=0, help="Only run first N questions (0 = all)")
    parser.add_argument("--legend", default="../target/release/legend", help="Path to Legend binary")
    parser.add_argument("--verbose", action="store_true", help="Print progress details")
    return parser.parse_args()


def load_dataset(path: str) -> list:
    with open(path) as f:
        return json.load(f)


def run_legend(legend_bin: str, cwd: str, args: list[str], stdin_text: str | None = None) -> str:
    """Run a Legend CLI command and return stdout."""
    cmd = [legend_bin] + args
    result = subprocess.run(
        cmd,
        cwd=cwd,
        input=stdin_text,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if result.returncode != 0:
        raise RuntimeError(f"Legend command failed: {' '.join(args)}\nstderr: {result.stderr}")
    return result.stdout


def setup_workspace(legend_bin: str, workspace: str):
    """Initialize a fresh Legend workspace."""
    # Create .legend dir and git repo (Legend needs a git context)
    os.makedirs(os.path.join(workspace, ".legend"), exist_ok=True)
    subprocess.run(["git", "init", "-q"], cwd=workspace, check=True, capture_output=True)
    subprocess.run(
        ["git", "commit", "--allow-empty", "-m", "init", "-q"],
        cwd=workspace,
        check=True,
        capture_output=True,
        env={**os.environ, "GIT_AUTHOR_NAME": "bench", "GIT_AUTHOR_EMAIL": "b@b",
             "GIT_COMMITTER_NAME": "bench", "GIT_COMMITTER_EMAIL": "b@b"},
    )


def ingest_sessions(legend_bin: str, workspace: str, sessions: list, verbose: bool = False):
    """Tick all turns from haystack sessions into Legend."""
    tick_count = 0
    rejected = 0
    for session in sessions:
        for turn in session:
            content = turn.get("content", "").strip()
            if not content:
                continue
            try:
                run_legend(legend_bin, workspace, ["memory", "tick", content])
                tick_count += 1
            except RuntimeError:
                rejected += 1
    if verbose:
        print(f"    Ingested {tick_count} ticks ({rejected} rejected)")


def query_legend(legend_bin: str, workspace: str, question: str) -> str:
    """Query Legend and return the retrieved context as plain text."""
    output = run_legend(legend_bin, workspace, ["memory", "query", question])
    raw = output.strip()

    # Parse JSON and format as readable text
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        return raw

    lines = []
    for i, mem in enumerate(data.get("memories", []), 1):
        if isinstance(mem, dict):
            text = mem.get("text", "")
            wc = mem.get("wall_clock", 0)
            seq = mem.get("seq", 0)
            parts = []
            if wc:
                from datetime import datetime, timezone
                dt = datetime.fromtimestamp(wc, tz=timezone.utc)
                parts.append(f"recorded:{dt.strftime('%Y-%m-%d %H:%M UTC')}")
            if seq:
                parts.append(f"seq:{seq}")
            prefix = f"[{', '.join(parts)}] " if parts else ""
            lines.append(f"[Memory {i}] {prefix}{text}")
        else:
            lines.append(f"[Memory {i}] {mem}")
    topics = data.get("related_topics", [])
    if topics:
        lines.append(f"[Related Topics] {', '.join(topics)}")
    for i, mem in enumerate(data.get("working_memory", []), 1):
        lines.append(f"[Active Memory {i}] {mem}")

    return "\n\n".join(lines) if lines else raw


def answer_with_llm(question: str, context: str, model: str) -> str:
    """Use Claude to answer the question given retrieved context."""
    try:
        import anthropic
    except ImportError:
        print("ERROR: pip install anthropic")
        sys.exit(1)

    client = anthropic.Anthropic()
    prompt = f"""You are answering questions based on memories retrieved from past conversations.
Use ONLY the provided context. Answer concisely and directly.
If the context doesn't contain enough information, say "I don't know".
Memories may include temporal metadata: "date:" is the wall-clock date, "seq:" is a monotonic sequence number (lower = earlier in time). Use these to reason about chronological order, duration, and temporal relationships. When memories are listed in order, earlier memories happened before later ones.

## Retrieved Memories
{context}

## Question
{question}

## Answer"""

    response = client.messages.create(
        model=model,
        max_tokens=256,
        messages=[{"role": "user", "content": prompt}],
    )
    return response.content[0].text.strip()


def run_benchmark(args):
    legend_bin = str(Path(args.legend).resolve())

    # Verify Legend binary exists
    if not os.path.isfile(legend_bin):
        print(f"ERROR: Legend binary not found at {legend_bin}")
        print("Run: cargo build --release")
        sys.exit(1)

    # Load dataset
    print(f"Loading dataset from {args.dataset}...")
    questions = load_dataset(args.dataset)
    if args.limit > 0:
        questions = questions[: args.limit]
    print(f"Running {len(questions)} questions")

    # Track results
    results = []
    correct = 0
    errors = 0
    start_time = time.time()

    with open(args.output, "w") as out_f:
        for i, q in enumerate(questions):
            qid = q["question_id"]
            question = q["question"]
            qtype = q.get("question_type", "unknown")

            if args.verbose:
                print(f"\n[{i + 1}/{len(questions)}] {qid} ({qtype})")
                print(f"  Q: {question[:80]}...")

            # Create fresh workspace for each question
            workspace = tempfile.mkdtemp(prefix="legend_bench_")
            try:
                setup_workspace(legend_bin, workspace)

                # 1. Ingest haystack sessions
                sessions = q.get("haystack_sessions", [])
                ingest_sessions(legend_bin, workspace, sessions, verbose=args.verbose)

                # 2. Query Legend
                context = query_legend(legend_bin, workspace, question)
                if args.verbose:
                    context_preview = context[:200].replace("\n", " ")
                    print(f"  Context: {context_preview}...")

                # 3. Answer with LLM
                hypothesis = answer_with_llm(question, context, args.model)
                if args.verbose:
                    print(f"  A: {hypothesis[:100]}...")
                    print(f"  Expected: {str(q['answer'])[:100]}...")

                # 4. Write result
                entry = {"question_id": qid, "hypothesis": hypothesis}
                out_f.write(json.dumps(entry) + "\n")
                out_f.flush()
                results.append({"qid": qid, "type": qtype, "hypothesis": hypothesis})

            except Exception as e:
                print(f"  ERROR on {qid}: {e}")
                entry = {"question_id": qid, "hypothesis": "Error during processing"}
                out_f.write(json.dumps(entry) + "\n")
                out_f.flush()
                errors += 1

            finally:
                # Clean up workspace
                shutil.rmtree(workspace, ignore_errors=True)

            # Progress
            if (i + 1) % 10 == 0:
                elapsed = time.time() - start_time
                rate = (i + 1) / elapsed
                eta = (len(questions) - i - 1) / rate if rate > 0 else 0
                print(f"  Progress: {i + 1}/{len(questions)} ({rate:.1f} q/s, ETA: {eta:.0f}s)")

    elapsed = time.time() - start_time
    print(f"\nDone. {len(results)} results written to {args.output}")
    print(f"Errors: {errors}")
    print(f"Time: {elapsed:.1f}s ({len(results) / elapsed:.1f} q/s)")
    print(f"\nNext: score with the official LongMemEval evaluator (see README.md)")


if __name__ == "__main__":
    args = parse_args()
    run_benchmark(args)
