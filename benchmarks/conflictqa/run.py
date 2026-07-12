#!/usr/bin/env python3
"""conflict_resolution benchmark end-to-end.

One counterfactual fact-list (~455 facts) is ingested once into a Legend store
(Sol, latest-wins conflicts via `changes`), then every question is answered by
Terra twice — without the store (arm A) and with a legend_recall tool (arm B) —
and graded by exact match (free). Because the facts contradict the real world,
arm A cannot succeed from parametric knowledge, so any lift is attributable to
the store.

    python run.py               # full pipeline (needs OPENAI_API_KEY)
    python run.py --dry-run     # plumbing only: no OpenAI, gold-as-answer stub
Set n_questions in config (per hop-type): small for smoke, 100 for the full run.
"""

import argparse
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "simpleqa"))
from common import oai  # noqa: E402
from common.legend_io import Legend  # noqa: E402
from common.util import Journal  # noqa: E402
from common.schemas import SAVE_TOOLS, RECALL_TOOLS  # noqa: E402

sys.path.insert(0, str(HERE))
from prompts import CONFLICT_INGESTER_SYSTEM, ANSWER_INSTRUCTION, ARM_B_PREAMBLE  # noqa: E402

STORE = "store/.legend"
OUTCOMES = ["correct", "incorrect", "not_attempted"]
_ARTICLES = re.compile(r"\b(the|a|an)\b")


def cfg_load():
    return json.loads((HERE / "config.json").read_text())


def d(*p):
    return HERE.joinpath(*p)


# ---------- data ----------
def load_questions(cfg):
    import pyarrow.parquet as pq
    rows = pq.read_table(cfg["data_parquet"]).to_pylist()
    mh, sh = rows[cfg["rows"]["multi_hop"]], rows[cfg["rows"]["single_hop"]]
    assert mh["context"] == sh["context"], "mh/sh contexts differ; revisit row choice"
    lim = cfg["n_questions"] or 10 ** 9
    qs = []
    for hop, row in (("multi", mh), ("single", sh)):
        for i, (q, a) in enumerate(zip(row["questions"], row["answers"])):
            if i >= lim:
                break
            qs.append({"qid": f"{hop}_{i}", "hop": hop, "q": q, "gold": list(a)})
    return mh["context"], qs


# ---------- grading (exact match, free) ----------
def norm(s):
    s = (s or "").casefold().strip().strip(".")
    s = _ARTICLES.sub(" ", s)
    s = re.sub(r"[^\w\s]", " ", s)
    return re.sub(r"\s+", " ", s).strip()


def grade(answer, gold_list):
    raw = (answer or "").lower()
    if "don't know" in raw or "dont know" in raw or norm(answer) in ("", "unknown", "i don t know"):
        return "not_attempted"
    a = norm(answer)
    for g in gold_list:
        gn = norm(g)
        if gn and (gn == a or f" {gn} " in f" {a} "):
            return "correct"
    return "incorrect"


# ---------- ingestion ----------
def ingest_recall_view(frame):
    return {k: frame[k] for k in ("resolution", "focus", "state", "recent")
            if k in frame and frame[k]}


def ingest(lg, context, cfg, journal):
    facts = [l for l in context.splitlines() if re.match(r"^\d+\.", l)]
    step = cfg["ingest_chunk_facts"]
    chunks = [facts[i:i + step] for i in range(0, len(facts), step)]
    usage = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    counts = {"saves": 0, "changes": 0}

    def dispatch(name, args):
        if name == "legend_recall":
            fr = lg.recall({"focus": args.get("focus", []), "limit": args.get("limit", 10),
                            "history_depth": args.get("history_depth", 1)})
            view = ingest_recall_view(fr)
            journal.write({"event": "recall", "args": args, "view": view})
            return json.dumps(view, ensure_ascii=False)
        if name == "legend_save":
            payload = {"source": "conflict",
                       "elements": args.get("elements", []), "facts": args.get("facts", []),
                       "changes": args.get("changes", []), "retract": args.get("retract", []),
                       "merge": args.get("merge", [])}
            res = lg.save(payload)
            counts["saves"] += 1
            counts["changes"] += len(payload["changes"])
            out = {"writes": res.get("writes", {}), "near_matches": res.get("near_matches", [])[:5]}
            journal.write({"event": "save", "payload": payload, "writes": res.get("writes", {})})
            return json.dumps(out, ensure_ascii=False)
        return json.dumps({"error": f"unknown tool {name}"})

    for ci, chunk in enumerate(chunks):
        user = (f"Facts to load (chunk {ci + 1}/{len(chunks)}). Recall entities you may already "
                f"have, use `changes` for any that update an earlier value, then save:\n\n"
                + "\n".join(chunk))
        res = oai.run_tool_loop(model=cfg["models"]["ingester"], system=CONFLICT_INGESTER_SYSTEM,
                                user=user, tools=SAVE_TOOLS, dispatch=dispatch,
                                max_calls=cfg["ingest_max_calls_per_chunk"], seed=cfg.get("seed"))
        for k in usage:
            usage[k] += res["usage_total"][k]
        journal.write({"event": "chunk_done", "chunk": ci, "calls": res["tool_calls_made"],
                       "usage": res["usage_total"]})
    return usage, counts, len(facts)


# ---------- arms ----------
def arm_a(cfg, q):
    text, u = oai.complete(cfg["models"]["consumer"], ANSWER_INSTRUCTION, q["q"], seed=cfg.get("seed"))
    return {"response": text, "frames": [], "tool_calls": 0, "usage": u}


def arm_b(cfg, q, lg):
    frames = []

    def dispatch(name, args):
        if name != "legend_recall":
            return json.dumps({"error": f"unknown tool {name}"})
        fr = lg.recall({"focus": args.get("focus", []), "limit": args.get("limit", 15),
                        "history_depth": args.get("history_depth", 1)})
        view = {k: fr[k] for k in ("resolution", "focus", "state", "recent", "related")
                if k in fr and fr[k]}
        frames.append({"args": args, "view": view})
        return json.dumps(view, ensure_ascii=False)

    system = ARM_B_PREAMBLE + "\n" + ANSWER_INSTRUCTION
    res = oai.run_tool_loop(model=cfg["models"]["consumer"], system=system, user=q["q"],
                            tools=RECALL_TOOLS, dispatch=dispatch,
                            max_calls=cfg["recall_max_calls"], seed=cfg.get("seed"))
    return {"response": res["answer"], "frames": frames,
            "tool_calls": res["tool_calls_made"], "usage": res["usage_total"]}


# ---------- report ----------
def cost(usage, price):
    return usage["prompt_tokens"] / 1e6 * price["in"] + usage["completion_tokens"] / 1e6 * price["out"]


def report(cfg, graded, ingest_usage, ingest_counts, n_facts, arms_usage):
    per_arm = {arm: {o: 0 for o in OUTCOMES} for arm in ("A", "B")}
    per_hop = {h: {"A": 0, "B": 0, "n": 0} for h in ("multi", "single")}
    table = {a: {b: 0 for b in OUTCOMES} for a in OUTCOMES}
    fixed, broken = [], []
    for g in graded:
        per_arm["A"][g["A_outcome"]] += 1
        per_arm["B"][g["B_outcome"]] += 1
        table[g["A_outcome"]][g["B_outcome"]] += 1
        per_hop[g["hop"]]["n"] += 1
        per_hop[g["hop"]]["A"] += g["A_outcome"] == "correct"
        per_hop[g["hop"]]["B"] += g["B_outcome"] == "correct"
        if g["A_outcome"] != "correct" and g["B_outcome"] == "correct":
            fixed.append(g)
        if g["A_outcome"] == "correct" and g["B_outcome"] != "correct":
            broken.append(g)

    n = len(graded)
    pr = cfg["pricing_per_mtok"]
    c_ing = cost(ingest_usage, pr["ingester"])
    c_arm = cost(arms_usage, pr["consumer"])
    L = []
    L.append("# conflict_resolution — results\n")
    L.append(f"- questions: **{n}**  ·  ingester `{cfg['models']['ingester']}`  ·  consumer `{cfg['models']['consumer']}`")
    L.append(f"- store: {n_facts} source facts, {ingest_counts['saves']} saves, {ingest_counts['changes']} conflict updates\n")
    L.append("## Headline")
    L.append(f"- arm A (no store) correct: **{per_arm['A']['correct']}/{n}**")
    L.append(f"- arm B (+recall) correct: **{per_arm['B']['correct']}/{n}**")
    L.append(f"- net lift: **{per_arm['B']['correct'] - per_arm['A']['correct']:+d}**  ·  fixed {len(fixed)}  ·  broken {len(broken)}\n")
    L.append("## By hop type")
    L.append("| hop | n | A correct | B correct |")
    L.append("|---|---|---|---|")
    for h in ("multi", "single"):
        ph = per_hop[h]
        L.append(f"| {h} | {ph['n']} | {ph['A']} | {ph['B']} |")
    L.append("\n## A → B transition")
    L.append("| A \\ B | correct | incorrect | not_attempted |")
    L.append("|---|---|---|---|")
    for a in OUTCOMES:
        L.append(f"| **{a}** | " + " | ".join(str(table[a][b]) for b in OUTCOMES) + " |")
    L.append(f"\n## Cost")
    L.append(f"- ingest: ${c_ing:.2f}  ·  arms: ${c_arm:.2f}  ·  **total ${c_ing + c_arm:.2f}**\n")
    L.append("## Broken flips (store hurt)")
    for g in broken[:20]:
        L.append(f"- [{g['hop']}] {g['q']}  gold={g['gold']}  A={g['A_response']!r} B={g['B_response']!r}")
    d("results").mkdir(exist_ok=True)
    d("results", "report.md").write_text("\n".join(L))
    print("\n".join(L))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--skip-ingest", action="store_true", help="reuse the existing store; only run arms+grade")
    args = ap.parse_args()
    cfg = cfg_load()
    context, questions = load_questions(cfg)
    lg = Legend(cfg["legend_binary"], d(STORE), now=cfg.get("legend_now"), embed=cfg.get("legend_embed", 1))
    d("runs").mkdir(exist_ok=True)

    if args.dry_run:
        lg.init(reset=True)
        # save a handful of facts by naive parse just to prove the store path
        for line in context.splitlines()[:6]:
            m = re.match(r"^\d+\.\s*(.+)", line)
            if m:
                lg.save({"source": "dry", "facts": [{"s": m.group(1)[:40], "p": "stated", "o": "true", "src": "dry"}]})
        graded = []
        for q in questions:
            a_out = grade("I don't know", q["gold"])
            b_out = grade(q["gold"][0], q["gold"])  # stub arm B with gold
            graded.append({**q, "A_response": "I don't know", "B_response": q["gold"][0],
                           "A_outcome": a_out, "B_outcome": b_out})
        zero = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
        report(cfg, graded, zero, {"saves": 6, "changes": 0}, 455, zero)
        return

    if not cfg["models"].get("models_verified"):
        print("models_verified is false — verify model IDs first.")
        sys.exit(2)

    # 1. ingest once (or reuse an already-built store)
    if args.skip_ingest:
        n_facts = len([l for l in context.splitlines() if re.match(r"^\d+\.", l)])
        ingest_usage = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
        ingest_counts = {"saves": 0, "changes": 0}
        print(f"reusing existing store ({len(lg.dump().get('elements', []))} elements)")
    else:
        lg.init(reset=True)
        with Journal(d("runs", "ingest.jsonl")) as journal:
            ingest_usage, ingest_counts, n_facts = ingest(lg, context, cfg, journal)
        dump = lg.dump()
        print(f"ingested: {n_facts} facts -> {len(dump.get('elements', []))} elements, "
              f"{ingest_counts['changes']} conflict updates, saves={ingest_counts['saves']}")

    # 2. arms + 3. grade
    arms_usage = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    graded = []
    with Journal(d("runs", "answers.jsonl")) as ans:
        for q in questions:
            a = arm_a(cfg, q)
            b = arm_b(cfg, q, lg)
            for r in (a, b):
                for k in arms_usage:
                    arms_usage[k] += r["usage"].get(k, 0)
            row = {**q, "A_response": a["response"], "B_response": b["response"],
                   "A_outcome": grade(a["response"], q["gold"]),
                   "B_outcome": grade(b["response"], q["gold"]),
                   "B_tool_calls": b["tool_calls"], "B_frames": b["frames"]}
            graded.append(row)
            ans.write(row)
            print(f"  {q['qid']}: A={row['A_outcome'][:4]} B={row['B_outcome'][:4]}  "
                  f"gold={q['gold']}  B_ans={b['response'][:40]!r}")

    # 4. report
    report(cfg, graded, ingest_usage, ingest_counts, n_facts, arms_usage)


if __name__ == "__main__":
    main()
