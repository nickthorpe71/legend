#!/usr/bin/env python3
"""Step 6: assemble results/report.md and the results/flips.md worksheet.

report.md is the numeric read-out (transition table, per-arm outcomes, net lift,
attempted rate, store stats, token/cost). flips.md is the manual-verification
worksheet: every flip pre-loaded with the recall frames the model saw and grep
hits for the gold answer in the store dump and the source pages, so the human
pass is minutes of reading, not archaeology.
"""

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common.legend_io import from_config  # noqa: E402
from common.util import load_config, d, read_jsonl  # noqa: E402

STORE_DIR = "store/.legend"
OUTCOMES = ["correct", "incorrect", "not_attempted"]


def norm(s):
    return re.sub(r"\s+", " ", s or "").casefold().strip()


def count_hits(needle, haystack):
    n = norm(needle).strip(" .,;:!?'\"")
    if not n:
        return 0
    return norm(haystack).count(n)


def cost(usage_sum, price):
    ti = usage_sum.get("prompt_tokens", 0)
    to = usage_sum.get("completion_tokens", 0)
    return ti / 1e6 * price["in"] + to / 1e6 * price["out"]


def sum_usage(usages):
    total = {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
    for u in usages:
        for k in total:
            total[k] += int((u or {}).get(k, 0) or 0)
    return total


def stage_usages(cfg):
    ingest = sum_usage([e.get("usage") for e in read_jsonl(d("runs", "ingest", "journal.jsonl"))
                        if e.get("event") == "chunk_done"])
    arms = sum_usage([r.get("usage") for r in read_jsonl(d("runs", "arms", "answers.jsonl"))])
    grades = json.loads(d("results", "grades.json").read_text())
    grade = sum_usage([g.get("usage") for g in grades.get("graded", [])])
    return ingest, arms, grade


def md_table(table):
    lines = ["| A \\ B | correct | incorrect | not_attempted |", "|---|---|---|---|"]
    for a in OUTCOMES:
        lines.append(f"| **{a}** | " + " | ".join(str(table[a][b]) for b in OUTCOMES) + " |")
    return "\n".join(lines)


def build_report(cfg, grades):
    per = grades["per_arm"]
    n = grades["n"]
    table = grades["transition_A_to_B"]
    fixed = sum(table[a]["correct"] for a in ("incorrect", "not_attempted"))
    broken = sum(table["correct"][b] for b in ("incorrect", "not_attempted"))
    lift = per["B"]["correct"] - per["A"]["correct"]

    def attempted_rate(arm):
        return (per[arm]["correct"] + per[arm]["incorrect"]) / n if n else 0.0

    def acc_given_attempted(arm):
        att = per[arm]["correct"] + per[arm]["incorrect"]
        return per[arm]["correct"] / att if att else 0.0

    ingest_u, arms_u, grade_u = stage_usages(cfg)
    pr = cfg["pricing_per_mtok"]
    c_ing, c_arm, c_grd = cost(ingest_u, pr["ingester"]), cost(arms_u, pr["consumer"]), cost(grade_u, pr["grader"])

    try:
        summ = json.loads(d("runs", "ingest", "summary.json").read_text())
    except FileNotFoundError:
        summ = {}

    m = cfg["models"]
    lines = [
        "# SimpleQA Phase 0 — results",
        "",
        f"- questions (surviving audit): **{n}**",
        f"- ingester: `{m['ingester']}` · consumer: `{m['consumer']}` · grader: `{m['grader']}`",
        "",
        "## Headline",
        "",
        f"- arm A correct: **{per['A']['correct']}/{n}**  ·  arm B correct: **{per['B']['correct']}/{n}**",
        f"- net lift (B − A correct): **{lift:+d}**",
        f"- fixed (A wrong/abstain → B correct): **{fixed}**  ·  broken (A correct → B wrong/abstain): **{broken}**",
        "",
        "## A → B transition",
        "",
        md_table(table),
        "",
        "## Per-arm",
        "",
        "| | correct | incorrect | not_attempted | attempted-rate | acc\\|attempted |",
        "|---|---|---|---|---|---|",
    ]
    for arm in ("A", "B"):
        lines.append(
            f"| arm {arm} | {per[arm]['correct']} | {per[arm]['incorrect']} | {per[arm]['not_attempted']} | "
            f"{attempted_rate(arm):.0%} | {acc_given_attempted(arm):.0%} |"
        )
    lines += [
        "",
        "## Store",
        "",
        f"- pages ingested: {summ.get('pages', '?')}  ·  elements: {summ.get('elements_total', '?')}  ·  "
        f"saves: {summ.get('saves', '?')}  ·  minted elements: {summ.get('minted_elements', '?')}",
        "",
        "## Tokens & cost",
        "",
        "| stage | prompt tok | completion tok | $ |",
        "|---|---|---|---|",
        f"| ingest ({m['ingester']}) | {ingest_u['prompt_tokens']:,} | {ingest_u['completion_tokens']:,} | {c_ing:.2f} |",
        f"| arms ({m['consumer']}) | {arms_u['prompt_tokens']:,} | {arms_u['completion_tokens']:,} | {c_arm:.2f} |",
        f"| grade ({m['grader']}) | {grade_u['prompt_tokens']:,} | {grade_u['completion_tokens']:,} | {c_grd:.2f} |",
        f"| **total** | | | **{c_ing + c_arm + c_grd:.2f}** |",
        "",
        "See `flips.md` for the per-flip hand-verification worksheet — the numbers above are not trustworthy until every fixed flip is confirmed grounded.",
        "",
    ]
    return "\n".join(lines)


def build_flips(cfg, grades):
    graded = grades["graded"]
    by = {}
    for g in graded:
        by.setdefault(g["qid"], {})[g["arm"]] = g

    # arm-B frames + page map
    b_rows = {r["qid"]: r for r in read_jsonl(d("runs", "arms", "answers.jsonl")) if r["arm"] == "B"}
    pages_by_qid = {}
    for man in read_jsonl(d("corpus", "manifest.jsonl")):
        if man.get("ok"):
            pages_by_qid.setdefault(man["qid"], []).append(man["path"])

    lg = from_config(cfg, d(STORE_DIR))
    try:
        dump_text = json.dumps(lg.dump(), ensure_ascii=False)
    except Exception:
        dump_text = ""

    def classify(a, b):
        if a in ("incorrect", "not_attempted") and b == "correct":
            return "FIXED"
        if a == "correct" and b in ("incorrect", "not_attempted"):
            return "BROKEN"
        return None

    sections = {"FIXED": [], "BROKEN": []}
    for qid, arms in by.items():
        if "A" not in arms or "B" not in arms:
            continue
        kind = classify(arms["A"]["outcome"], arms["B"]["outcome"])
        if not kind:
            continue
        g = arms["B"]
        gold = g["answer"]
        frames = b_rows.get(qid, {}).get("frames", [])
        frames_text = json.dumps(frames, ensure_ascii=False)
        in_store = count_hits(gold, dump_text)
        in_frame = count_hits(gold, frames_text)
        page_hits = {p: count_hits(gold, (d() / p).read_text(encoding="utf-8")) for p in pages_by_qid.get(qid, [])}

        block = [
            f"### qid={qid} — {kind}  (A={arms['A']['outcome']} → B={arms['B']['outcome']})",
            "",
            f"- **Q:** {g['problem']}",
            f"- **gold:** `{gold}`",
            f"- **arm A answer:** {arms['A']['response']!r}",
            f"- **arm B answer:** {arms['B']['response']!r}",
            "",
            f"- auto grep — gold in store dump: **{in_store}** · in frames shown to B: **{in_frame}** · "
            f"in pages: " + (", ".join(f"{Path(p).name}={h}" for p, h in page_hits.items()) or "—"),
            "",
            "  - [ ] gold fact is **in the store**",
            "  - [ ] gold was **in a frame Terra actually saw**",
            "  - [ ] gold is **on a snapshot page** (i.e. genuinely ingestible)",
            "",
            "<details><summary>recall frames arm B saw</summary>",
            "",
            "```json",
            json.dumps(frames, indent=2, ensure_ascii=False)[:6000],
            "```",
            "</details>",
            "",
        ]
        sections[kind].append("\n".join(block))

    out = ["# Flip verification worksheet", "",
           "A fixed flip only counts if the gold answer was in the store **and** in a frame "
           "Terra actually saw. A fixed flip that is NOT grounded (gold absent from store/frames) "
           "is Terra recalling from its own weights or the grader de-abstaining — not distillation. "
           "Confirm each one by hand.", ""]
    for kind in ("FIXED", "BROKEN"):
        out.append(f"## {kind} ({len(sections[kind])})")
        out.append("")
        out.extend(sections[kind] or ["_none_", ""])
    return "\n".join(out)


def main():
    cfg = load_config()
    grades = json.loads(d("results", "grades.json").read_text())
    d("results").mkdir(parents=True, exist_ok=True)
    d("results", "report.md").write_text(build_report(cfg, grades))
    d("results", "flips.md").write_text(build_flips(cfg, grades))
    print("wrote results/report.md and results/flips.md")
    print(d("results", "report.md").read_text())


if __name__ == "__main__":
    main()
