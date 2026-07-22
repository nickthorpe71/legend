#!/usr/bin/env python3
"""Re-run ONLY arm B (Legend recall) against the existing store, keeping arms A
and D frozen — isolates a recall-path change (e.g. F1 relevance ranking) at the
Terra price only, no re-ingest. Rewrites the B rows in runs/arms/answers.jsonl;
then run grade.py + report.py as usual.
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_arms  # noqa: E402
from common.legend_io import from_config  # noqa: E402
from common.util import load_config, d, read_jsonl, write_jsonl  # noqa: E402

STORE_DIR = "store/.legend"


def main():
    cfg = load_config()
    if not cfg["models"].get("models_verified"):
        print("Refusing to spend tokens: models_verified is false.")
        sys.exit(2)
    lg = from_config(cfg, d(STORE_DIR))
    questions = read_jsonl(d("corpus", "questions_supported.jsonl"))
    old = read_jsonl(d("runs", "arms", "answers.jsonl"))
    keep = [r for r in old if r["arm"] != "B"]
    if not keep:
        print("FATAL: no non-B rows to keep — run the full run_arms first.")
        sys.exit(1)

    new_b = []
    for q in questions:
        b = run_arms.arm_b(cfg, q, lg)
        new_b.append({"qid": q["qid"], "problem": q["problem"], "answer": q["answer"],
                      "model": cfg["models"]["consumer"], "arm": "B", **b})
        print(f"  qid={q['qid']}: B={b['response'][:56]!r} (calls={b['tool_calls']})")

    write_jsonl(d("runs", "arms", "answers.jsonl"), keep + new_b)
    print(f"rewrote answers.jsonl: {len(keep)} frozen (A/D) + {len(new_b)} fresh B")


if __name__ == "__main__":
    main()
