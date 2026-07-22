#!/usr/bin/env python3
"""No-code simulation of the raw-passage anchor (adversary-4's $2 test).

Everything is on disk from the corrected embed-on+F1 run:
  - arm B frames (the graph facts Terra saw) in runs/arms/answers.jsonl
  - arm D_dense's retrieved chunk ids per question (the exact passages RAG used)

Test 1 (ORACLE-GATE CEILING): for the D-only misses, hand Terra the arm-B graph
facts PLUS the D_dense passages, with the spec's "prefer the graph; use passages
only if the graph lacks the answer" instruction; re-answer and re-grade. Count
flips. This is the BEST case for the anchor (perfect gate + the right chunks).

Test 2 (GATE): max cosine(query, arm-B graph fact) for each question, misses vs a
reference set of hits. If misses' top-fact cosine is as high as hits', a relevance
threshold can't separate "graph has the answer" from "graph has a wrong neighbor"
-> the gate can't fire on the misses (the circular-gate critique).
"""
import json
import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import grade  # noqa: E402
import run_arms  # noqa: E402  (unused import kept for env parity)
from common import oai, rag  # noqa: E402
from common.prompts import ANSWER_INSTRUCTION  # noqa: E402
from common.util import load_config, d, read_jsonl  # noqa: E402

DONLY = [447, 820, 934, 1016, 1334, 2790, 2932, 2952, 3358, 3573, 3699, 3716, 4042]
INGEST8 = {1334, 2790, 2932, 2952, 3358, 3573, 3699, 4042}


def cos(a, b):
    dp = sum(x * y for x, y in zip(a, b))
    na = math.sqrt(sum(x * x for x in a)) or 1.0
    nb = math.sqrt(sum(y * y for y in b)) or 1.0
    return dp / (na * nb)


def frame_facts(rB):
    out = []
    for f in rB.get("frames", []):
        fr = f.get("frame", {})
        for band in ("recent", "related", "state"):
            for rr in fr.get(band, []):
                out.append(rr.get("attrs", {}))
    return out


def main():
    cfg = load_config()
    chunks = rag.load_chunks(d("corpus", "pages"), cfg["rag"]["chunk_tokens"])
    cid2text = {c["id"]: c["text"] for c in chunks}
    rows = [json.loads(l) for l in open(d("runs", "arms", "answers.jsonl"))]
    byqa = {(r["qid"], r["arm"]): r for r in rows}
    G = json.load(open(d("results", "grades.json")))["outcomes_by_qid"]
    qs = read_jsonl(d("corpus", "questions_supported.jsonl"))
    gold = {q["qid"]: q["answer"] for q in qs}
    qtext = {q["qid"]: q["problem"] for q in qs}

    results = []
    for qid in DONLY:
        rB = byqa[(qid, "B")]
        rD = byqa.get((qid, "D_dense"), {})
        q, g = qtext[qid], gold[qid]
        facts = frame_facts(rB)
        facts_txt = "\n".join(json.dumps(a, ensure_ascii=False) for a in facts) or "(none)"
        passages = [cid2text.get(cid, "") for cid in (rD.get("retrieved") or [])]
        pass_txt = "\n\n---\n\n".join(p for p in passages if p) or "(none)"
        system = ("You are given knowledge-base FACTS and, below them, reference PASSAGES. "
                  "Prefer the facts; use the passages only if the facts do not contain the answer.\n"
                  + ANSWER_INSTRUCTION)
        user = f"FACTS:\n{facts_txt}\n\nPASSAGES:\n{pass_txt}\n\nQuestion: {q}"
        ans, _ = oai.complete(cfg["models"]["consumer"], system, user, seed=cfg.get("seed"))
        outcome, _, _, _ = grade.grade_one(cfg, q, g, ans)
        base = G[str(qid)]["B"]
        flip = base != "correct" and outcome == "correct"
        # gate proxy: max cosine(query, graph fact) via OpenAI embeddings
        ftexts = [json.dumps(a, ensure_ascii=False) for a in facts] or ["(none)"]
        embs = oai.embed("text-embedding-3-small", [q] + ftexts)
        gate = max((cos(embs[0], e) for e in embs[1:]), default=0.0)
        results.append({"qid": qid, "ingest8": qid in INGEST8, "base": base,
                        "sim": outcome, "flip": flip, "gate_maxcos": round(gate, 3),
                        "gold": g, "sim_ans": ans[:70]})
        print(f"  qid={qid} ing8={qid in INGEST8} {base}->{outcome} flip={flip} "
              f"gate={gate:.3f} | gold={g!r} | sim={ans[:55]!r}")

    # reference gate scores on a few B-correct hits
    hits = [q["qid"] for q in qs if G[str(q["qid"])]["B"] == "correct"][:8]
    hit_gates = []
    for qid in hits:
        rB = byqa[(qid, "B")]
        facts = frame_facts(rB)
        ftexts = [json.dumps(a, ensure_ascii=False) for a in facts] or ["(none)"]
        embs = oai.embed("text-embedding-3-small", [qtext[qid]] + ftexts)
        hit_gates.append(round(max((cos(embs[0], e) for e in embs[1:]), default=0.0), 3))

    flips_all = sum(1 for r in results if r["flip"])
    flips_ing = sum(1 for r in results if r["flip"] and r["ingest8"])
    ing_gates = sorted(r["gate_maxcos"] for r in results if r["ingest8"])
    print("\n=== RESULT ===")
    print(f"ORACLE-GATE ceiling flips: {flips_all}/13 all D-only | {flips_ing}/8 ingest-target")
    print(f"gate max-cosine, 8 ingest MISSES: {ing_gates}")
    print(f"gate max-cosine, {len(hit_gates)} B-correct HITS (reference): {sorted(hit_gates)}")
    print("If miss-gates overlap hit-gates, no threshold separates them -> gate can't fire.")
    Path(d("results", "sim_raw_anchor.json")).write_text(json.dumps(
        {"flips_all": flips_all, "flips_ingest": flips_ing, "results": results,
         "hit_gates": hit_gates}, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
