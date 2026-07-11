#!/usr/bin/env python3
"""Step 4: answer every surviving question twice with the consumer model (Terra).

Arm A: question only.
Arm B: same question + a legend_recall tool over the ingested store (<=3 calls).

The final answer+abstain instruction is identical across arms — the only
difference arm B sees is the recall tool and a one-line note that it exists — so
`not_attempted` means the same thing in both and a flip is attributable to the
store, not to prompt wording.

Output: runs/arms/answers.jsonl, one row per (qid, arm) with the response, the
full tool trace, the recall frames the model saw, and token usage.
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import oai  # noqa: E402
from common.legend_io import from_config  # noqa: E402
from common.prompts import ANSWER_INSTRUCTION, ARM_B_PREAMBLE  # noqa: E402
from common.schemas import RECALL_TOOLS  # noqa: E402
from common.util import load_config, d, read_jsonl, Journal  # noqa: E402

STORE_DIR = "store/.legend"
OUT = "runs/arms/answers.jsonl"


def arm_a(cfg, q):
    text, usage = oai.complete(
        cfg["models"]["consumer"], ANSWER_INSTRUCTION, q["problem"], seed=cfg.get("seed")
    )
    return {"response": text, "tool_calls": 0, "frames": [], "usage": usage, "stop_reason": "stop"}


def arm_b(cfg, q, lg):
    frames = []

    def dispatch(name, args):
        if name != "legend_recall":
            return json.dumps({"error": f"unknown tool {name}"})
        frame = lg.recall({
            "focus": args.get("focus", []),
            "limit": args.get("limit", 20),
            "history_depth": args.get("history_depth", 3),
        })
        view = {k: frame[k] for k in
                ("resolution", "near_matches", "focus", "state", "constraints",
                 "decisions", "recent", "history", "related", "sources")
                if k in frame and frame[k]}
        frames.append({"args": args, "frame": view})
        return json.dumps(view, ensure_ascii=False)

    system = ARM_B_PREAMBLE + "\n" + ANSWER_INSTRUCTION
    res = oai.run_tool_loop(
        model=cfg["models"]["consumer"], system=system, user=q["problem"],
        tools=RECALL_TOOLS, dispatch=dispatch, max_calls=cfg["recall_max_calls"],
        seed=cfg.get("seed"),
    )
    return {"response": res["answer"], "tool_calls": res["tool_calls_made"],
            "frames": frames, "usage": res["usage_total"], "stop_reason": res["stop_reason"]}


def dry_arm_b(q, lg):
    """No-OpenAI: prove the store+recall path returns a frame for the question."""
    frame = lg.recall({"focus": [q["problem"]], "limit": 10})
    resolved = [f.get("name") for f in frame.get("focus", [])]
    return {"response": "[dry-run]", "tool_calls": 1,
            "frames": [{"args": {"focus": [q["problem"]]}, "resolved": resolved}],
            "usage": {}, "stop_reason": "dry"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    cfg = load_config()
    if not args.dry_run and not cfg["models"].get("models_verified"):
        print("Refusing to spend tokens: models_verified is false. Run preflight.py --write first.")
        sys.exit(2)

    lg = from_config(cfg, d(STORE_DIR))
    questions = read_jsonl(d("corpus", "questions_supported.jsonl"))
    if not questions:
        print("FATAL: no surviving questions — run fetch_corpus.py first.")
        sys.exit(1)

    rows = []
    out_path = d(OUT)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text("")  # start fresh (Journal appends)
    with Journal(out_path) as out:
        for q in questions:
            base = {"qid": q["qid"], "problem": q["problem"], "answer": q["answer"],
                    "model": cfg["models"]["consumer"]}
            if args.dry_run:
                a = {"response": "[dry-run]", "tool_calls": 0, "frames": [], "usage": {}, "stop_reason": "dry"}
                b = dry_arm_b(q, lg)
            else:
                a = arm_a(cfg, q)
                b = arm_b(cfg, q, lg)
            for arm, r in (("A", a), ("B", b)):
                row = {**base, "arm": arm, **r}
                rows.append(row)
                out.write(row)
            print(f"  qid={q['qid']}: A={a['response'][:50]!r} | B={b['response'][:50]!r} (B calls={b['tool_calls']})")

    # assert: both arms answered every question; arm B frames were not errors
    by = {}
    for r in rows:
        by.setdefault(r["qid"], {})[r["arm"]] = r
    for qid, arms in by.items():
        if set(arms) != {"A", "B"}:
            print(f"WARN qid={qid} missing an arm: {set(arms)}")
        if not args.dry_run:
            for fr in arms["B"]["frames"]:
                if isinstance(fr.get("frame"), dict) and "error" in fr["frame"]:
                    print(f"WARN qid={qid} arm B saw an error frame")
    print(f"wrote {len(rows)} rows -> {OUT}")


if __name__ == "__main__":
    main()
