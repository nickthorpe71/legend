#!/usr/bin/env python3
"""Step 5: grade every (question, arm) answer with the SimpleQA grader (Sol).

The grader sees only question / gold target / predicted answer — it is blind to
which arm produced the answer — and returns A/B/C mapped to
correct / incorrect / not_attempted. We then build the 3x3 A->B transition
table, the object the whole Phase 0 read-out hangs on.
"""

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import oai  # noqa: E402
from common.prompts import GRADER_TEMPLATE, GRADE_LETTER  # noqa: E402
from common.util import load_config, d, read_jsonl  # noqa: E402

OUTCOMES = ["correct", "incorrect", "not_attempted"]


def grade_one(cfg, question, gold, predicted):
    prompt = GRADER_TEMPLATE.format(question=question, target=gold, predicted_answer=predicted)
    text, usage = oai.complete(cfg["models"]["grader"], "", prompt, seed=cfg.get("seed"))
    m = re.search(r"[ABC]", text.upper())
    letter = m.group(0) if m else "C"  # unparseable -> treat as not_attempted
    return GRADE_LETTER[letter], letter, text.strip(), usage


def main():
    cfg = load_config()
    if not cfg["models"].get("models_verified"):
        print("Refusing to spend tokens: models_verified is false. Run preflight.py --write first.")
        sys.exit(2)

    rows = read_jsonl(d("runs", "arms", "answers.jsonl"))
    if not rows:
        print("FATAL: no answers to grade — run run_arms.py first.")
        sys.exit(1)

    graded = []
    for r in rows:
        outcome, letter, raw, usage = grade_one(cfg, r["problem"], r["answer"], r["response"])
        graded.append({"qid": r["qid"], "arm": r["arm"], "outcome": outcome,
                      "letter": letter, "grader_raw": raw, "usage": usage,
                      "response": r["response"], "answer": r["answer"], "problem": r["problem"]})
        print(f"  qid={r['qid']} arm={r['arm']}: {outcome}")

    # per-question outcome map (arm -> outcome), and the A->B transition table
    by_qid = {}
    for g in graded:
        by_qid.setdefault(g["qid"], {})[g["arm"]] = g["outcome"]
    arms_present = sorted({g["arm"] for g in graded})
    table = {a: {b: 0 for b in OUTCOMES} for a in OUTCOMES}
    pairs = []
    for qid, arms in by_qid.items():
        if "A" in arms and "B" in arms:
            table[arms["A"]][arms["B"]] += 1
            pairs.append({"qid": qid, "A": arms["A"], "B": arms["B"]})

    out = {
        "n": len(by_qid),
        "arms": arms_present,
        "per_arm": {
            arm: {o: sum(1 for g in graded if g["arm"] == arm and g["outcome"] == o) for o in OUTCOMES}
            for arm in arms_present
        },
        "outcomes_by_qid": {str(qid): arms for qid, arms in by_qid.items()},
        "transition_A_to_B": table,
        "pairs": pairs,
        "graded": graded,
    }
    d("results").mkdir(parents=True, exist_ok=True)
    d("results", "grades.json").write_text(json.dumps(out, indent=2, ensure_ascii=False))

    print("\n  A\\B      correct  incorrect  not_att")
    for a in OUTCOMES:
        print(f"  {a:11}" + "".join(f"{table[a][b]:9}" for b in OUTCOMES))
    print(f"\n  arm A: {out['per_arm']['A']}")
    print(f"  arm B: {out['per_arm']['B']}")


if __name__ == "__main__":
    main()
