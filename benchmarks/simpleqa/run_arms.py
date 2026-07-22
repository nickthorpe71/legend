#!/usr/bin/env python3
"""Step 4: answer every surviving question with the consumer model (Terra),
once per arm.

Arm A:      question only.
Arm B:      same question + a legend_recall tool over the ingested store (<=3 calls).
Arm D_bm25: same question + top passages from BM25 over the same corpus, stuffed.
Arm D_dense:same question + top passages from OpenAI-embedding cosine, stuffed.

The final answer+abstain instruction is identical across arms, so `not_attempted`
means the same thing everywhere and a flip is attributable to the context, not to
prompt wording. The D arms retrieve to a per-question token budget matched to what
arm B's recall injected (design claim 3: structure vs dumb retrieval at parity).

Output: runs/arms/answers.jsonl, one row per (qid, arm) with the response, the
tool trace / retrieved chunk ids, the recall frames the model saw, injected-token
counts, and token usage.
"""

import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import oai  # noqa: E402
from common import rag  # noqa: E402
from common.legend_io import from_config  # noqa: E402
from common.prompts import ANSWER_INSTRUCTION, ARM_B_PREAMBLE, ARM_D_PREAMBLE  # noqa: E402
from common.schemas import RECALL_TOOLS  # noqa: E402
from common.util import load_config, d, read_jsonl, est_tokens, Journal  # noqa: E402

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


def frame_budget(b_result, floor):
    """Tokens of the recall frames Terra actually saw in arm B, floored so a
    question where B recalled nothing still gives arm D a fighting chance."""
    seen = sum(est_tokens(json.dumps(f.get("frame", f), ensure_ascii=False))
               for f in b_result.get("frames", []))
    return max(floor, seen)


def arm_d(cfg, q, retriever, budget):
    scores = retriever.score(q["problem"])
    picked = rag.top_to_budget(retriever.chunks, scores, budget)
    context = "\n\n---\n\n".join(f"[{c['page']}] {c['text']}" for c in picked)
    system = ARM_D_PREAMBLE + "\n" + ANSWER_INSTRUCTION
    user = f"Reference passages:\n\n{context}\n\nQuestion: {q['problem']}"
    text, usage = oai.complete(cfg["models"]["consumer"], system, user, seed=cfg.get("seed"))
    return {"response": text, "tool_calls": 0, "frames": [],
            "retrieved": [c["id"] for c in picked],
            "injected_tokens": sum(c["tokens"] for c in picked),
            "budget": budget, "usage": usage, "stop_reason": "stop"}


def dry_arm_b(q, lg):
    """No-OpenAI: prove the store+recall path returns a frame for the question."""
    frame = lg.recall({"focus": [q["problem"]], "limit": 10})
    resolved = [f.get("name") for f in frame.get("focus", [])]
    return {"response": "[dry-run]", "tool_calls": 1,
            "frames": [{"args": {"focus": [q["problem"]]}, "resolved": resolved}],
            "usage": {}, "stop_reason": "dry"}


def dry_arm_d(q, retriever, budget):
    """No-OpenAI: prove BM25 retrieval + budgeting return chunks for the question."""
    scores = retriever.score(q["problem"])
    picked = rag.top_to_budget(retriever.chunks, scores, budget)
    return {"response": "[dry-run]", "tool_calls": 0, "frames": [],
            "retrieved": [c["id"] for c in picked],
            "injected_tokens": sum(c["tokens"] for c in picked),
            "budget": budget, "usage": {}, "stop_reason": "dry"}


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

    ragcfg = cfg.get("rag", {})
    floor = ragcfg.get("budget_floor_tokens", 300)
    chunks = rag.load_chunks(d("corpus", "pages"), ragcfg.get("chunk_tokens", 256))
    print(f"RAG index: {len(chunks)} chunks over {len(list((d('corpus','pages')).glob('*.md')))} pages")
    bm25 = rag.BM25(chunks)
    dense = None
    if not args.dry_run and ragcfg.get("dense", True):
        print(f"embedding {len(chunks)} chunks with {ragcfg.get('embed_model')} ...")
        dense = rag.Dense(chunks, ragcfg["embed_model"])

    arms_order = ["A", "B", "D_bm25", "D_dense"]
    rows = []
    out_path = d(OUT)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text("")  # start fresh (Journal appends)
    with Journal(out_path) as out:
        for q in questions:
            base = {"qid": q["qid"], "problem": q["problem"], "answer": q["answer"],
                    "model": cfg["models"]["consumer"]}
            if args.dry_run:
                b = dry_arm_b(q, lg)
                budget = frame_budget(b, floor)
                results = {"A": {"response": "[dry-run]", "tool_calls": 0, "frames": [],
                                 "usage": {}, "stop_reason": "dry"},
                           "B": b,
                           "D_bm25": dry_arm_d(q, bm25, budget),
                           "D_dense": dry_arm_d(q, bm25, budget)}
            else:
                a = arm_a(cfg, q)
                b = arm_b(cfg, q, lg)
                budget = frame_budget(b, floor)
                results = {"A": a, "B": b,
                           "D_bm25": arm_d(cfg, q, bm25, budget)}
                results["D_dense"] = (arm_d(cfg, q, dense, budget) if dense
                                      else {"response": "[skipped]", "tool_calls": 0, "frames": [],
                                            "usage": {}, "stop_reason": "skip"})
            for arm in arms_order:
                if arm not in results:
                    continue
                row = {**base, "arm": arm, **results[arm]}
                rows.append(row)
                out.write(row)
            print(f"  qid={q['qid']} budget={budget}: "
                  + " | ".join(f"{arm}={results[arm]['response'][:32]!r}" for arm in arms_order if arm in results))

    # assert: every arm answered every question; arm B frames were not errors
    by = {}
    for r in rows:
        by.setdefault(r["qid"], {})[r["arm"]] = r
    expect = {"A", "B", "D_bm25"} | ({"D_dense"} if (args.dry_run or dense) else set())
    for qid, arms in by.items():
        missing = expect - set(arms)
        if missing:
            print(f"WARN qid={qid} missing arms: {missing}")
        if not args.dry_run:
            for fr in arms["B"]["frames"]:
                if isinstance(fr.get("frame"), dict) and "error" in fr["frame"]:
                    print(f"WARN qid={qid} arm B saw an error frame")
    print(f"wrote {len(rows)} rows -> {OUT}")


if __name__ == "__main__":
    main()
