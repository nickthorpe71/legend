#!/usr/bin/env python3
"""Minimal v2-adapted LongMemEval runner.

The v1 harness in this directory (run_longmemeval.py on master) expects
`legend memory tick` / `legend memory query` subcommands. v2's CLI is
flatter — every invocation is `legend <text>` and produces a write tick
(loads the snapshot, runs the pipeline, persists). There is no query
verb; retrieval is what the tick's frame shows in `focused_relations`
and `current_state`.

This runner adapts:
- INGEST each haystack turn by invoking the binary with its content.
- QUERY by invoking the binary with the question text and inspecting
  the printed frame for keyword hits from the expected answer.

Scoring is intentionally crude: we look for substantive (≥4-char)
words from the expected answer in the frame's stdout. A real eval
would route the retrieved context through a reading LLM (per the v1
harness); for a v2 sanity check, keyword overlap is enough to tell
"did Legend remember the right entity?" from "did it miss entirely."

Usage:
    python3 benchmarks/run_longmemeval_v2.py [--questions N]
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

LEGEND_BIN = Path(__file__).resolve().parent.parent / "target/release/legend"
DATASET = Path(__file__).resolve().parent / "longmemeval_oracle.json"

STOPWORDS = {
    "the", "and", "for", "with", "that", "this", "from", "have", "had",
    "has", "was", "were", "are", "you", "your", "their", "they", "them",
    "his", "her", "him", "she", "but", "not", "all", "any", "what", "when",
    "where", "why", "how", "who", "which", "into", "about", "after", "before",
    "first", "last", "during",
}


def key_terms(answer: str) -> list[str]:
    """Extract substantive words from an expected answer. Keeps
    uppercase acronyms (e.g. GPS, CPU) regardless of length —
    they're typically the proper-noun salient terms."""
    out: list[str] = []
    for raw in re.split(r"[^A-Za-z0-9]+", answer):
        if not raw:
            continue
        lower = raw.lower()
        if lower in STOPWORDS:
            continue
        # Keep uppercase acronyms (≥2 chars, all upper, alpha) verbatim.
        if raw.isupper() and len(raw) >= 2 and raw.isalpha():
            out.append(lower)
            continue
        # Otherwise require ≥4 chars to filter "not", "had", "got", etc.
        if len(lower) >= 4:
            out.append(lower)
    return out


def run_legend(workspace: Path, text: str, timeout: int = 30,
               frame_json: bool = False) -> subprocess.CompletedProcess:
    env = os.environ.copy()
    env["LEGEND_STATE_DIR"] = str(workspace / ".legend")
    if frame_json:
        env["LEGEND_FRAME_JSON"] = "1"
    return subprocess.run(
        [str(LEGEND_BIN), text],
        cwd=str(workspace),
        env=env,
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def parse_frame_json(stdout: str) -> dict | None:
    """Extract the frame JSON block printed when LEGEND_FRAME_JSON=1."""
    begin = stdout.find("--- LEGEND_FRAME_JSON_BEGIN ---")
    end = stdout.find("--- LEGEND_FRAME_JSON_END ---")
    if begin < 0 or end < 0 or end <= begin:
        return None
    payload = stdout[begin:end].split("\n", 1)[1]
    try:
        return json.loads(payload)
    except json.JSONDecodeError:
        return None


def ingest_turns(workspace: Path, sessions: list, max_turns: int, user_only: bool) -> tuple[int, int]:
    """Tick each haystack turn through Legend. Cap by max_turns total.

    `user_only=True` skips assistant turns. Assistant responses are
    typically 1500-2800 chars of verbose recommendations and exceed
    the binary's 480-token limit; they also take 40+ seconds each
    when they do fit because NER scales with text length. User
    turns are the ones that carry actual facts the question asks
    about anyway.
    """
    ingested = rejected = 0
    for session in sessions:
        for turn in session:
            if ingested + rejected >= max_turns:
                return ingested, rejected
            if user_only and turn.get("role") != "user":
                continue
            content = (turn.get("content") or "").strip()
            if not content:
                continue
            try:
                r = run_legend(workspace, content)
                if r.returncode == 0:
                    ingested += 1
                else:
                    rejected += 1
            except subprocess.TimeoutExpired:
                rejected += 1
    return ingested, rejected


def query_frame(workspace: Path, question: str) -> subprocess.CompletedProcess:
    return run_legend(workspace, question, frame_json=True)


def flatten_frame_text(frame: dict) -> str:
    """Flatten the JSON frame into a single searchable text blob —
    subject names, attribute names, and values from every focused
    relation, current_state, history, supporting_claims entry, plus
    the active_frame."""
    parts: list[str] = []
    if frame.get("active_frame"):
        parts.append(frame["active_frame"])
    for section in ("focused_relations", "current_state", "history", "supporting_claims"):
        for r in frame.get(section, []):
            parts.append(r.get("subject", ""))
            for a in r.get("attrs", []):
                parts.append(a.get("name", ""))
                parts.append(a.get("value", ""))
    return " | ".join(parts)


def score_hits(frame: dict | None, fallback_stdout: str, terms: list[str]) -> list[str]:
    """Return the answer-terms that appeared in the frame. Uses the
    JSON dump when available (the full top-N retrieval set) and falls
    back to scanning the printed truncated-top-5 stdout."""
    haystack = flatten_frame_text(frame).lower() if frame else fallback_stdout.lower()
    return [t for t in terms if t in haystack]


def run_one_question(q: dict, max_turns: int, verbose: bool, user_only: bool) -> dict:
    workspace = Path(tempfile.mkdtemp(prefix="legend_lm_v2_"))
    try:
        t0 = time.time()
        ingested, rejected = ingest_turns(
            workspace, q.get("haystack_sessions", []), max_turns, user_only
        )
        ingest_secs = time.time() - t0

        t1 = time.time()
        result = query_frame(workspace, q["question"])
        query_secs = time.time() - t1

        frame = parse_frame_json(result.stdout) if result.returncode == 0 else None
        terms = key_terms(q["answer"])
        hits = score_hits(frame, result.stdout, terms) if result.returncode == 0 else []
        focused_count = len(frame.get("focused_relations", [])) if frame else 0

        return {
            "qid": q["question_id"],
            "type": q.get("question_type"),
            "question": q["question"],
            "answer": q["answer"],
            "answer_terms": terms,
            "hits": hits,
            "ingested": ingested,
            "rejected": rejected,
            "ingest_secs": round(ingest_secs, 1),
            "query_secs": round(query_secs, 2),
            "query_exit": result.returncode,
            "focused_count": focused_count,
            "active_frame": frame.get("active_frame") if frame else None,
            "frame": frame if verbose else None,
            "query_stdout": result.stdout if verbose else "",
            "query_stderr": result.stderr if verbose else "",
        }
    finally:
        shutil.rmtree(workspace, ignore_errors=True)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--questions", type=int, default=1,
                   help="Number of questions to run (default 1)")
    p.add_argument("--max-turns", type=int, default=20,
                   help="Per-question ingest cap (default 20). Set to "
                        "a large number to ingest the full haystack.")
    p.add_argument("--verbose", action="store_true",
                   help="Include query stdout/stderr in the result")
    p.add_argument("--all-roles", action="store_true",
                   help="Ingest assistant turns too. Slow — assistant "
                        "responses are typically 1500-2800 chars and "
                        "each takes 40+ seconds in NER. Defaults to "
                        "user turns only.")
    args = p.parse_args()

    if not LEGEND_BIN.is_file():
        print(f"binary not found: {LEGEND_BIN} — run `cargo build --release`")
        return 1

    dataset = json.load(open(DATASET))
    subset = dataset[: args.questions]
    print(f"running {len(subset)} questions (max {args.max_turns} ingested turns each)\n")

    results = []
    for i, q in enumerate(subset):
        print(f"[{i + 1}/{len(subset)}] {q['question_id']} ({q.get('question_type')})")
        print(f"  Q: {q['question'][:100]}")
        print(f"  expected: {q['answer'][:100]}")
        r = run_one_question(q, args.max_turns, args.verbose, user_only=not args.all_roles)
        results.append(r)
        print(f"  ingested: {r['ingested']} turns ({r['rejected']} rejected) in {r['ingest_secs']}s")
        print(f"  query: exit={r['query_exit']} in {r['query_secs']}s, focused={r['focused_count']}, active_frame={r['active_frame']!r}")
        print(f"  answer terms: {r['answer_terms']}")
        if r["hits"]:
            print(f"  ✓ hits: {r['hits']}")
        else:
            print(f"  ✗ no answer terms surfaced in frame")
        print()

    hit_count = sum(1 for r in results if r["hits"])
    print(f"summary: {hit_count}/{len(results)} questions had at least one answer-term hit")

    if args.verbose:
        out_path = Path("benchmarks/last_run_verbose.json")
        out_path.parent.mkdir(exist_ok=True)
        out_path.write_text(json.dumps(results, indent=2))
        print(f"verbose dump: {out_path}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
