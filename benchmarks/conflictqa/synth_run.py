#!/usr/bin/env python3
"""Synthetic-entity conflict_resolution run.

Deterministically loads a synthetic corpus (facts + latest-wins `changes`) into a
Legend store — no LLM ingester, so the store is guaranteed correct and free to
build — then answers each question with arm A (no store) and arm B (+recall),
exact-match graded. Synthetic entities have no parametric prior, so arm A cannot
answer from world knowledge and any lift is attributable to the store. This
isolates the one thing the famous-entity variant couldn't: does a consumer WITHOUT
a competing prior actually use Legend's memory?

    python synth_run.py              # arms need OPENAI_API_KEY
    python synth_run.py --dry-run    # store-load plumbing only, no OpenAI
    python synth_run.py --n 6        # questions per hop (default 4)
"""
import argparse
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "simpleqa"))
from common.legend_io import Legend  # noqa: E402
from common.util import Journal  # noqa: E402

sys.path.insert(0, str(HERE))
from run import arm_a, arm_b, grade, cfg_load, cost, OUTCOMES, ingest  # noqa: E402
from synth import generate, to_factlist  # noqa: E402

STORE = "store_synth/.legend"


def load_store(lg, corpus):
    """Build the store deterministically: elements, then facts, then conflicts."""
    lg.init(reset=True)
    B = 50  # Legend caps save lists at 64 entries
    els = [{"name": e["name"], "kind": e["kind"], "summary": e["kind"]}
           for e in corpus["elements"]]
    bf, ch = corpus["base_facts"], corpus["changes"]
    for i in range(0, len(els), B):
        lg.save({"source": "synth", "elements": els[i:i + B]})
    for i in range(0, len(bf), B):
        lg.save({"source": "synth", "facts": bf[i:i + B]})
    for i in range(0, len(ch), B):
        lg.save({"source": "synth", "changes": ch[i:i + B]})
    return len(bf), len(ch)


def pick(questions, n):
    out, seen = [], {"single": 0, "multi": 0}
    for q in questions:
        if seen[q["hop"]] < n:
            out.append(q)
            seen[q["hop"]] += 1
    return out


def report(cfg, graded, n_facts, n_conf, arms_usage, ingest_usage=None, ingest_counts=None):
    per_arm = {a: {o: 0 for o in OUTCOMES} for a in ("A", "B")}
    per_hop = {h: {"A": 0, "B": 0, "n": 0} for h in ("multi", "single")}
    table = {a: {b: 0 for b in OUTCOMES} for a in OUTCOMES}
    broken = []
    for g in graded:
        per_arm["A"][g["A_outcome"]] += 1
        per_arm["B"][g["B_outcome"]] += 1
        table[g["A_outcome"]][g["B_outcome"]] += 1
        ph = per_hop[g["hop"]]
        ph["n"] += 1
        ph["A"] += g["A_outcome"] == "correct"
        ph["B"] += g["B_outcome"] == "correct"
        if g["A_outcome"] == "correct" and g["B_outcome"] != "correct":
            broken.append(g)
    n = len(graded)
    c_arm = cost(arms_usage, cfg["pricing_per_mtok"]["consumer"])
    if ingest_counts is not None:
        build = (f"LLM ingester `{cfg['models']['ingester']}` "
                 f"({ingest_counts['saves']} saves, {ingest_counts['changes']} conflict updates)")
    else:
        build = "built deterministically (no ingester)"
    L = ["# conflict_resolution (synthetic entities) — results\n",
         f"- questions: **{n}**  ·  consumer `{cfg['models']['consumer']}`  ·  store {build}",
         f"- store: {n_facts} facts, {n_conf} latest-wins conflicts\n",
         "## Headline",
         f"- arm A (no store) correct: **{per_arm['A']['correct']}/{n}**",
         f"- arm B (+recall) correct: **{per_arm['B']['correct']}/{n}**",
         f"- net lift: **{per_arm['B']['correct'] - per_arm['A']['correct']:+d}**\n",
         "## By hop type", "| hop | n | A correct | B correct |", "|---|---|---|---|"]
    for h in ("multi", "single"):
        ph = per_hop[h]
        L.append(f"| {h} | {ph['n']} | {ph['A']} | {ph['B']} |")
    L += ["\n## A → B transition", "| A \\ B | correct | incorrect | not_attempted |", "|---|---|---|---|"]
    for a in OUTCOMES:
        L.append(f"| **{a}** | " + " | ".join(str(table[a][b]) for b in OUTCOMES) + " |")
    if ingest_usage is not None:
        c_ing = cost(ingest_usage, cfg["pricing_per_mtok"]["ingester"])
        L.append(f"\n## Cost\n- ingest: ${c_ing:.3f}  ·  arms: ${c_arm:.3f}  ·  **total ${c_ing + c_arm:.3f}**\n")
    else:
        L.append(f"\n## Cost\n- arms: **${c_arm:.3f}** (store build free)\n")
    if broken:
        L.append("## Broken flips (store hurt)")
        for g in broken[:20]:
            L.append(f"- [{g['hop']}] {g['q']}  gold={g['gold']}  B={g['B_response']!r}")
    (HERE / "results").mkdir(exist_ok=True)
    (HERE / "results" / "report_synth.md").write_text("\n".join(L))
    print("\n".join(L))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--n", type=int, default=4, help="questions per hop")
    ap.add_argument("--ingest", action="store_true",
                    help="build the store via the LLM ingester (Sol) instead of deterministically")
    ap.add_argument("--chunk", type=int, default=0,
                    help="ingest chunk size; 0 = boundary at n_base (every correction cross-chunk)")
    args = ap.parse_args()
    cfg = cfg_load()
    corpus = generate()
    questions = pick(corpus["questions"], args.n)
    lg = Legend(cfg["legend_binary"], HERE / STORE, now=cfg.get("legend_now"),
                embed=cfg.get("legend_embed", 1))
    n_conf = len(corpus["changes"])
    ingest_usage = ingest_counts = None

    if args.ingest and not args.dry_run:
        if not cfg["models"].get("models_verified"):
            print("models_verified is false — verify model IDs first.")
            sys.exit(2)
        context, n_base = to_factlist(corpus)
        cfg["ingest_chunk_facts"] = args.chunk or n_base
        lg.init(reset=True)
        (HERE / "runs_synth").mkdir(exist_ok=True)
        with Journal(HERE / "runs_synth" / "ingest.jsonl") as journal:
            ingest_usage, ingest_counts, n_facts = ingest(lg, context, cfg, journal)
        print(f"LLM-ingested: {n_facts} statements (chunk={cfg['ingest_chunk_facts']}) -> "
              f"{len(lg.dump().get('elements', []))} elements, "
              f"{ingest_counts['changes']} conflict updates, saves={ingest_counts['saves']}")
    else:
        n_facts, _ = load_store(lg, corpus)
        print(f"loaded synthetic store: {n_facts} facts, {n_conf} conflicts, "
              f"{len(lg.dump().get('elements', []))} elements")

    if args.dry_run:
        graded = [{**q, "A_response": "I don't know", "B_response": q["gold"][0],
                   "A_outcome": grade("I don't know", q["gold"]),
                   "B_outcome": grade(q["gold"][0], q["gold"])} for q in questions]
        report(cfg, graded, n_facts, n_conf, {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0})
        return

    if not cfg["models"].get("models_verified"):
        print("models_verified is false — verify model IDs first.")
        sys.exit(2)

    arms_usage = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    graded = []
    (HERE / "runs_synth").mkdir(exist_ok=True)
    with Journal(HERE / "runs_synth" / "answers.jsonl") as ans:
        for q in questions:
            a = arm_a(cfg, q)
            b = arm_b(cfg, q, lg)
            for rr in (a, b):
                for k in arms_usage:
                    arms_usage[k] += rr["usage"].get(k, 0)
            row = {**q, "A_response": a["response"], "B_response": b["response"],
                   "A_outcome": grade(a["response"], q["gold"]),
                   "B_outcome": grade(b["response"], q["gold"]),
                   "B_tool_calls": b["tool_calls"], "B_frames": b["frames"]}
            graded.append(row)
            ans.write(row)
            print(f"  {q['qid']}: A={row['A_outcome'][:4]} B={row['B_outcome'][:4]}  "
                  f"gold={q['gold']}  B_ans={b['response'][:40]!r}")
    report(cfg, graded, n_facts, n_conf, arms_usage, ingest_usage, ingest_counts)


if __name__ == "__main__":
    main()
