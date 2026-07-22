#!/usr/bin/env python3
"""Step 6: assemble results/report.md and the results/flips.md worksheet.

report.md is the numeric read-out: per-arm accuracy for every arm, the A->B
transition, net lift, the STRUCTURE DELTA (arm B minus each naive-RAG arm D, the
decisive claim-3 number) with a paired bootstrap CI, a token-parity audit, and
store / cost stats. flips.md is the manual-verification worksheet for arm B: each
A->B flip pre-loaded with the recall frames the model saw and grounding checks.

Grounding is format-aware: the store normalizes dates (August 16, 2008 ->
2008-08-16) and a naive substring grep misses that, so `grounded()` canonicalizes
dates on both sides before matching. A RapidFuzz score is reported as an advisory
for the human but never auto-grounds a flip.
"""

import datetime
import json
import random
import re
import sys
from pathlib import Path

from rapidfuzz import fuzz

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common.legend_io import from_config  # noqa: E402
from common.util import load_config, d, read_jsonl, est_tokens  # noqa: E402

STORE_DIR = "store/.legend"
OUTCOMES = ["correct", "incorrect", "not_attempted"]

_MONTHS = {m: i for i, m in enumerate(
    ["january", "february", "march", "april", "may", "june", "july",
     "august", "september", "october", "november", "december"], start=1)}
_MONTHS.update({m[:3]: i for m, i in list(_MONTHS.items())})


def norm(s):
    return re.sub(r"\s+", " ", s or "").casefold().strip()


def _canon_dates(text):
    """Rewrite common date spellings to ISO yyyy-mm-dd so store-normalized dates
    and gold answers match. Handles 'Month D, YYYY' and 'D Month YYYY'."""
    def iso(y, m, dd):
        return f"{int(y):04d}-{int(m):02d}-{int(dd):02d}"

    def repl_mdy(mo):
        mon = _MONTHS.get(mo.group(1).lower())
        return iso(mo.group(3), mon, mo.group(2)) if mon else mo.group(0)

    def repl_dmy(mo):
        mon = _MONTHS.get(mo.group(2).lower())
        return iso(mo.group(3), mon, mo.group(1)) if mon else mo.group(0)

    t = re.sub(r"\b([A-Za-z]+)\.?\s+(\d{1,2}),?\s+(\d{4})\b", repl_mdy, text)
    t = re.sub(r"\b(\d{1,2})\s+([A-Za-z]+)\.?,?\s+(\d{4})\b", repl_dmy, t)
    return t


def grounded(needle, haystack):
    """True if the gold answer is present, tolerant of date-format normalization.
    Conservative: exact (canonicalized) substring only — never fuzzy — so a flip
    is never auto-grounded on a coincidental near-match."""
    g = norm(_canon_dates(needle)).strip(" .,;:!?'\"")
    if not g:
        return False
    return g in norm(_canon_dates(haystack))


def fuzz_score(needle, haystack):
    """Advisory only: best partial-ratio of gold against text (0-100)."""
    n = norm(needle)
    return round(fuzz.partial_ratio(n, norm(haystack))) if n else 0


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


def median(xs):
    xs = sorted(xs)
    n = len(xs)
    if not n:
        return 0
    return xs[n // 2] if n % 2 else (xs[n // 2 - 1] + xs[n // 2]) / 2


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


def paired_delta_ci(outcomes_by_qid, arm_x, arm_y, iters=5000, seed=12345):
    """Point estimate and 95% bootstrap CI for acc(arm_x) - acc(arm_y), paired
    over the questions both arms answered. Returns (delta, lo, hi, x_only, y_only)."""
    diffs, x_only, y_only = [], 0, 0
    for arms in outcomes_by_qid.values():
        if arm_x not in arms or arm_y not in arms:
            continue
        cx = 1 if arms[arm_x] == "correct" else 0
        cy = 1 if arms[arm_y] == "correct" else 0
        diffs.append(cx - cy)
        x_only += cx and not cy
        y_only += cy and not cx
    n = len(diffs)
    if not n:
        return (0.0, 0.0, 0.0, 0, 0)
    point = sum(diffs) / n
    rnd = random.Random(seed)
    means = sorted(sum(diffs[rnd.randrange(n)] for _ in range(n)) / n for _ in range(iters))
    return (point, means[int(0.025 * iters)], means[int(0.975 * iters)], x_only, y_only)


def build_report(cfg, grades):
    per = grades["per_arm"]
    arms = grades["arms"]
    n = grades["n"]
    table = grades["transition_A_to_B"]
    obq = grades["outcomes_by_qid"]
    fixed = sum(table[a]["correct"] for a in ("incorrect", "not_attempted"))
    broken = sum(table["correct"][b] for b in ("incorrect", "not_attempted"))

    def acc(arm):
        return per[arm]["correct"] / n if n else 0.0

    def attempted_rate(arm):
        return (per[arm]["correct"] + per[arm]["incorrect"]) / n if n else 0.0

    def acc_given_attempted(arm):
        att = per[arm]["correct"] + per[arm]["incorrect"]
        return per[arm]["correct"] / att if att else 0.0

    # grounded fixed flips: gold present in a frame arm B actually saw
    b_rows = {r["qid"]: r for r in read_jsonl(d("runs", "arms", "answers.jsonl")) if r["arm"] == "B"}
    grounded_fixed = 0
    for p in grades["pairs"]:
        if p["A"] in ("incorrect", "not_attempted") and p["B"] == "correct":
            g = next((x for x in grades["graded"] if x["qid"] == p["qid"] and x["arm"] == "B"), None)
            frames_text = json.dumps(b_rows.get(p["qid"], {}).get("frames", []), ensure_ascii=False)
            if g and grounded(g["answer"], frames_text):
                grounded_fixed += 1

    ingest_u, arms_u, grade_u = stage_usages(cfg)
    pr = cfg["pricing_per_mtok"]
    c_ing, c_arm, c_grd = cost(ingest_u, pr["ingester"]), cost(arms_u, pr["consumer"]), cost(grade_u, pr["grader"])

    try:
        summ = json.loads(d("runs", "ingest", "summary.json").read_text())
    except FileNotFoundError:
        summ = {}

    m = cfg["models"]
    d_arms = [a for a in arms if a.startswith("D")]
    lines = [
        "# SimpleQA — results (arms A / B / naive-RAG D)",
        "",
        f"- questions (surviving audit): **{n}**",
        f"- ingester: `{m['ingester']}` · consumer: `{m['consumer']}` · grader: `{m['grader']}`",
        "",
        "## Headline",
        "",
        f"- accuracy — " + "  ·  ".join(f"{a}: **{per[a]['correct']}/{n}** ({acc(a):.0%})" for a in arms),
        f"- net lift arm B − arm A (store vs nothing): **{per['B']['correct'] - per['A']['correct']:+d}**",
        f"- A→B flips: fixed **{fixed}** (grounded **{grounded_fixed}**) · broken **{broken}** "
        f"· grounded lift **{grounded_fixed - broken:+d}**",
        "",
        "## Structure delta — arm B (Legend) vs naive RAG at matched token budget",
        "",
        "The claim-3 number: does the deduped graph earn anything over dumb retrieval "
        "over the same corpus? Paired bootstrap 95% CI (5000 resamples).",
        "",
        "| comparison | acc B | acc D | Δ (B−D) | 95% CI | B-only | D-only |",
        "|---|---|---|---|---|---|---|",
    ]
    for da in d_arms:
        delta, lo, hi, b_only, d_only = paired_delta_ci(obq, "B", da)
        lines.append(f"| B vs {da} | {acc('B'):.0%} | {acc(da):.0%} | {delta:+.0%} | "
                     f"[{lo:+.0%}, {hi:+.0%}] | {b_only} | {d_only} |")
    lines += [
        "",
        "## Per-arm",
        "",
        "| arm | correct | incorrect | not_attempted | attempted-rate | acc\\|attempted |",
        "|---|---|---|---|---|---|",
    ]
    for arm in arms:
        lines.append(
            f"| {arm} | {per[arm]['correct']} | {per[arm]['incorrect']} | {per[arm]['not_attempted']} | "
            f"{attempted_rate(arm):.0%} | {acc_given_attempted(arm):.0%} |"
        )

    # token-parity audit: did the D arms get the same context budget as B's recall?
    all_rows = read_jsonl(d("runs", "arms", "answers.jsonl"))
    b_frame_tok = [est_tokens(json.dumps(r.get("frames", []), ensure_ascii=False))
                   for r in all_rows if r["arm"] == "B"]
    lines += [
        "",
        "## Token-parity audit (median injected context tokens/question)",
        "",
        "| arm | median tokens |",
        "|---|---|",
        f"| B (recall frames) | {int(median(b_frame_tok))} |",
    ]
    for da in d_arms:
        inj = [r.get("injected_tokens", 0) for r in all_rows if r["arm"] == da]
        lines.append(f"| {da} (retrieved chunks) | {int(median(inj))} |")

    lines += [
        "",
        "## A → B transition",
        "",
        md_table(table),
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
        "See `flips.md` for the arm-B flip worksheet. Fixed flips are auto-grounded on "
        "canonicalized (date-aware) matching; confirm the ambiguous ones by hand.",
        "",
    ]
    return "\n".join(lines)


def build_flips(cfg, grades):
    graded = grades["graded"]
    by = {}
    for g in graded:
        by.setdefault(g["qid"], {})[g["arm"]] = g

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
        in_store = grounded(gold, dump_text)
        in_frame = grounded(gold, frames_text)
        page_hits = {p: grounded(gold, (d() / p).read_text(encoding="utf-8")) for p in pages_by_qid.get(qid, [])}
        auto = "✓ grounded" if (in_store and in_frame) else "✗ NOT grounded (gold not in a frame B saw)"

        block = [
            f"### qid={qid} — {kind}  (A={arms['A']['outcome']} → B={arms['B']['outcome']})  — {auto}",
            "",
            f"- **Q:** {g['problem']}",
            f"- **gold:** `{gold}`",
            f"- **arm A answer:** {arms['A']['response']!r}",
            f"- **arm B answer:** {arms['B']['response']!r}",
            "",
            f"- grounding (date-aware) — in store: **{in_store}** · in frames B saw: **{in_frame}** · "
            f"on pages: " + (", ".join(f"{Path(p).name}={v}" for p, v in page_hits.items()) or "—")
            + f"  · fuzz(gold, frames): {fuzz_score(gold, frames_text)}",
            "",
            f"  - [{'x' if in_store else ' '}] gold fact is **in the store**",
            f"  - [{'x' if in_frame else ' '}] gold was **in a frame Terra actually saw**",
            f"  - [{'x' if any(page_hits.values()) else ' '}] gold is **on a snapshot page**",
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

    out = ["# Flip verification worksheet (arm B vs arm A)", "",
           "A fixed flip only counts if the gold answer was in the store **and** in a frame "
           "Terra actually saw. Grounding is date-format-aware (August 16, 2008 == 2008-08-16). "
           "The checkboxes are pre-ticked from that automated grounding; the RapidFuzz score is "
           "an advisory for anything ambiguous — confirm those by hand.", ""]
    for kind in ("FIXED", "BROKEN"):
        out.append(f"## {kind} ({len(sections[kind])})")
        out.append("")
        out.extend(sections[kind] or ["_none_", ""])
    return "\n".join(out)


_SYM = {"correct": "✓", "incorrect": "✗", "not_attempted": "∅"}


def build_detail(cfg, grades):
    """Exhaustive per-question record: every arm's outcome + answer on every
    question, and the exact B-vs-D disagreement lists (where the graph beats RAG
    and where RAG beats the graph) — the diagnostic core of the structure delta."""
    obq = grades["outcomes_by_qid"]
    arms = grades["arms"]
    g_by = {}
    for g in grades["graded"]:
        g_by.setdefault(str(g["qid"]), {})[g["arm"]] = g

    m = cfg["models"]
    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    lines = [
        "# SimpleQA — per-question detail",
        "",
        f"- generated: {ts}",
        f"- models: ingester=`{m['ingester']}` consumer=`{m['consumer']}` grader=`{m['grader']}`  ·  seed: {cfg.get('seed')}",
        f"- questions: {grades['n']}  ·  legend={'on' if cfg.get('legend_embed') else 'off'}  ·  "
        f"rag: chunk_tokens={cfg.get('rag', {}).get('chunk_tokens')} floor={cfg.get('rag', {}).get('budget_floor_tokens')} embed={cfg.get('rag', {}).get('embed_model')}",
        "- key: ✓ correct · ✗ incorrect · ∅ not_attempted",
        "",
        "## Every question × every arm",
        "",
        "| qid | question | gold | " + " | ".join(arms) + " |",
        "|---|---|---|" + "---|" * len(arms),
    ]
    for qid in sorted(obq, key=lambda k: int(k)):
        any_g = next(iter(g_by[qid].values()))
        q = any_g["problem"].replace("|", "/").replace("\n", " ")[:80]
        gold = str(any_g["answer"]).replace("|", "/").replace("\n", " ").strip()[:44]
        cells = " | ".join(_SYM.get(obq[qid].get(a), "·") for a in arms)
        lines.append(f"| {qid} | {q} | `{gold}` | {cells} |")

    for da in [a for a in arms if a.startswith("D")]:
        def resp(qid, arm):
            return (g_by[qid].get(arm, {}).get("response", "") or "").replace("\n", " ")[:70]
        b_wins = [q for q in sorted(obq, key=lambda k: int(k))
                  if obq[q].get("B") == "correct" and obq[q].get(da) in ("incorrect", "not_attempted")]
        d_wins = [q for q in sorted(obq, key=lambda k: int(k))
                  if obq[q].get(da) == "correct" and obq[q].get("B") in ("incorrect", "not_attempted")]
        lines += ["", f"## B (Legend) beats {da} — graph wins ({len(b_wins)})", ""]
        for q in b_wins:
            lines.append(f"- **qid={q}** {g_by[q]['B']['problem'][:90]}  ·  gold `{g_by[q]['B']['answer']}`")
            lines.append(f"    - B: {resp(q, 'B')!r}  |  {da}: {resp(q, da)!r}")
        lines += ["", f"## {da} beats B (Legend) — RAG wins ({len(d_wins)})", ""]
        for q in d_wins:
            lines.append(f"- **qid={q}** {g_by[q]['B']['problem'][:90]}  ·  gold `{g_by[q]['B']['answer']}`")
            lines.append(f"    - B: {resp(q, 'B')!r}  |  {da}: {resp(q, da)!r}")
    return "\n".join(lines)


def main():
    cfg = load_config()
    grades = json.loads(d("results", "grades.json").read_text())
    d("results").mkdir(parents=True, exist_ok=True)
    d("results", "report.md").write_text(build_report(cfg, grades))
    d("results", "flips.md").write_text(build_flips(cfg, grades))
    d("results", "detail.md").write_text(build_detail(cfg, grades))
    print("wrote results/report.md, results/flips.md, results/detail.md")
    print(d("results", "report.md").read_text())


if __name__ == "__main__":
    main()
