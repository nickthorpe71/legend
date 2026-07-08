#!/usr/bin/env python3
"""MemoryAgentBench-templated, LLM-in-the-loop session eval for Legend.

Uses MABench's structure — the four competencies (Accurate Retrieval, Conflict
Resolution, long-range History, Faithfulness) and its ingest-then-query shape —
on our corpus, which is a realistic multi-month session. The corpus saves are a
human telling Legend things over time; each probe (stamped with an after_line)
is that human asking a question at that point. We replay the session, and at
each checkpoint a real model answers from Legend's live recall frame. Every
answer is scored and bucketed by competency, and the run's cost is tracked.

The read is a transcript: facts accrue, questions get asked when the info is
fresh / stale / just-changed, and you can see per competency where Legend
delivers and where it doesn't — with the substrate-vs-reasoning split so a miss
points at the right layer.

  python3 harness/eval_session.py --slice adversarial [--model ...] [--limit N] [--dry-run]

--dry-run projects cost with no API calls. A live run needs ANTHROPIC_API_KEY.
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, json, re, subprocess, tempfile, urllib.request, urllib.error
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PRICES = {  # $/MTok (input, output) — VERIFY vs current pricing; token counts are exact.
    "claude-haiku-4-5-20251001": (1.00, 5.00),
    "claude-sonnet-5": (3.00, 15.00),
    "claude-opus-4-8": (15.00, 75.00),
}
DEFAULT_MODEL = "claude-haiku-4-5-20251001"
ANSWER_SYS = (
    "You are the reasoning model behind a long-term memory system, answering a "
    "user's question. Use ONLY the recall context provided. If the answer is not "
    "present in the context, reply with exactly NOT_IN_CONTEXT and nothing else. "
    "Otherwise reply with just the specific value(s), as briefly as possible.")
# Competency labels (MABench template).
COMPS = {"AR": "Accurate Retrieval", "CR": "Conflict Resolution",
         "HIST": "Long-range History", "FAITH": "Faithfulness"}


def norm(s):
    return re.sub(r"\s+", " ", str(s).strip().lower())


def humanize(p):
    return str(p).replace("_", " ").strip()


def present(text, gold):
    a, g = norm(text), norm(gold)
    if not g:
        return False
    if g in a or (len(a) > 2 and a in g):
        return True
    gt, at = set(g.split()), set(a.split())
    return bool(gt) and len(gt & at) / len(gt) >= 0.6


def derive(cat, e):
    """probe entry -> list of question dicts. mode drives scoring. Entries that
    lack the fields a category needs are skipped (categories aren't uniform)."""
    focus = (e.get("probe", {}).get("payload", {}).get("focus") or ["?"])[0]
    if cat == "current_state" and {"property", "target", "expect"} <= e.keys():
        return [dict(comp="AR", q=f'What is the {humanize(e["property"])} of "{e["target"]}" right now?',
                     gold=str(e["expect"]), mode="value")]
    if cat == "cold_caller" and "expect_resolves_to" in e:
        return [dict(comp="AR", q=f'The memory may store this under a different name. What is "{focus}"? Name the specific entity.',
                     gold=e["expect_resolves_to"], mode="value")]
    if cat == "recall_hits" and e.get("expect_elements"):
        return [dict(comp="AR", q=f'What does memory record for "{focus}"? List every specific named item.',
                     gold=e["expect_elements"], mode="all")]
    if cat == "deep_history" and {"property", "target", "expect_current"} <= e.keys():
        qs = [dict(comp="CR", q=f'What is the current {humanize(e["property"])} of "{e["target"]}"?',
                   gold=str(e["expect_current"]), mode="value")]
        if e.get("expect_history"):
            qs.append(dict(comp="HIST", q=f'What was the {humanize(e["property"])} of "{e["target"]}" BEFORE its current value?',
                           gold=e["expect_history"], mode="any"))
        return qs
    if cat == "exclusion" and e.get("forbid_names"):
        return [dict(comp="FAITH", q=f'What is "{focus}"? Answer only from memory.',
                     gold=e["forbid_names"], mode="forbid")]
    if cat == "absent":
        return [dict(comp="FAITH", q=f'What is "{focus}"?', gold=None, mode="abstain")]
    return []


def build_session(slice_):
    """Replay the corpus with all probes; return questions ordered by after_line,
    each carrying its recall frame at that checkpoint."""
    tmp = Path(tempfile.mkdtemp(prefix="legend-sess-"))
    legend = str(tmp / "legend")
    if subprocess.run(["gcc", "-std=c99", "-Wall", "-Wextra", "-Werror", "-O2",
                       "legend.c", "embed.c", "-o", legend, "-lm"], cwd=ROOT,
                      capture_output=True, text=True).returncode:
        sys.exit("build failed")
    corpus, probes = tmp / "corpus.jsonl", ROOT / f"harness/corpus/probes_{slice_}.json"
    subprocess.run([sys.executable, "-P", "harness/gen_corpus.py", "--slice", slice_,
                    "-o", str(corpus)], cwd=ROOT, capture_output=True)
    results = tmp / "pr.json"
    subprocess.run([sys.executable, "-P", "harness/run.py", "--legend", legend,
                    "--replay", str(corpus), "--probes", str(probes),
                    "--probe-results", str(results), "--store", str(tmp / "store")],
                   cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    doc = json.loads(probes.read_text())
    frames = {(x["group"], x["index"]): x for x in json.loads(results.read_text())}
    qs = []
    for cat, entries in doc.items():
        if not isinstance(entries, list):
            continue
        for i, e in enumerate(entries):
            fr = frames.get((cat, i))
            if not fr or fr["frame"] is None:
                continue
            for q in derive(cat, e):
                q.update(cat=cat, after_line=e.get("after_line", 0),
                         frame=fr["frame"], notes=e.get("notes", ""))
                qs.append(q)
    qs.sort(key=lambda q: (q["after_line"], q["comp"]))
    return qs


def call_llm(model, user):
    body = json.dumps({"model": model, "max_tokens": 200, "system": ANSWER_SYS,
                       "messages": [{"role": "user", "content": user}]}).encode()
    req = urllib.request.Request("https://api.anthropic.com/v1/messages", data=body,
        headers={"x-api-key": os.environ["ANTHROPIC_API_KEY"],
                 "anthropic-version": "2023-06-01", "content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=90) as r:
        d = json.load(r)
    u = d.get("usage", {})
    return "".join(b.get("text", "") for b in d.get("content", [])).strip(), \
        u.get("input_tokens", 0), u.get("output_tokens", 0)


def score(q, answer, frame_text):
    abst = "not_in_context" in norm(answer)
    mode, gold = q["mode"], q["gold"]
    if mode == "abstain":
        return ("PASS" if abst else "HALLUCINATION"), None, ("PASS" if abst else "HALLUCINATION") == "PASS"
    if mode == "forbid":  # retracted value must not leak
        leaked = any(present(answer, g) for g in gold)
        return ("PASS" if not leaked else "LEAK"), None, not leaked
    if mode == "all":
        fh = all(present(frame_text, g) for g in gold)
        ok = (not abst) and all(present(answer, g) for g in gold)
    elif mode == "any":
        fh = any(present(frame_text, g) for g in gold)
        ok = (not abst) and any(present(answer, g) for g in gold)
    else:  # value
        fh = present(frame_text, gold)
        ok = (not abst) and present(answer, gold)
    verdict = "PASS" if ok else ("REASONING_MISS" if fh else "SUBSTRATE_MISS")
    return verdict, fh, ok


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--slice", default="adversarial", choices=["smoke", "adversarial", "dev", "full"])
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--limit", type=int, default=10**9)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--out", default=str(ROOT / "eval_session.md"))
    args = ap.parse_args()
    in_rate, out_rate = PRICES.get(args.model, (0.0, 0.0))
    if not args.dry_run and not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("ANTHROPIC_API_KEY not set (use --dry-run to project cost).")

    qs = build_session(args.slice)[: args.limit]
    rows, tin, tout = [], 0, 0
    for n, q in enumerate(qs):
        ctx = json.dumps(q["frame"], ensure_ascii=False)
        user = f"Recall context:\n{ctx}\n\nQuestion: {q['q']}"
        if args.dry_run:
            ci, co, answer, verdict, fh, ok = max(1, len(ANSWER_SYS + user) // 4), 25, "(dry-run)", "(dry-run)", None, None
        else:
            try:
                answer, ci, co = call_llm(args.model, user)
            except urllib.error.HTTPError as ex:
                answer, ci, co = f"API_ERROR {ex.code}", 0, 0
            verdict, fh, ok = score(q, answer, ctx)
        tin += ci; tout += co
        rows.append({**q, "answer": answer, "verdict": verdict, "frame_has": fh, "ok": ok,
                     "ci": ci, "co": co, "cost": ci/1e6*in_rate + co/1e6*out_rate})
        print(f"  L{q['after_line']:>3} {q['comp']:<5} {verdict:<15} "
              f"{'' if args.dry_run else q['q'][:52]}")

    total = tin/1e6*in_rate + tout/1e6*out_rate
    report(args, rows, tin, tout, total, in_rate, out_rate)
    print("\n" + "=" * 64)
    print(f"model {args.model}  ({in_rate:g}/{out_rate:g} $/MTok — verify vs current pricing)")
    print(f"COST ${total:.4f}  ({tin} in + {tout} out tok)  ${total/max(1,len(rows)):.4f}/q  over {len(rows)} questions"
          + ("  [PROJECTED]" if args.dry_run else ""))
    if not args.dry_run:
        print("\nBy competency (MABench template):")
        for c, name in COMPS.items():
            cr = [r for r in rows if r["comp"] == c]
            if cr:
                ok = sum(1 for r in cr if r["ok"])
                sub = [r for r in cr if r["frame_has"] is not None]
                subhit = sum(1 for r in sub if r["frame_has"])
                extra = f"   substrate {subhit}/{len(sub)}" if sub else ""
                print(f"  {name:<22} {ok}/{len(cr)}{pct(ok,len(cr))}{extra}")
        allok = sum(1 for r in rows if r["ok"])
        print(f"  {'OVERALL':<22} {allok}/{len(rows)}{pct(allok,len(rows))}")
    print(f"\nfull transcript: {args.out}")


def pct(a, b):
    return f" ({100*a/b:.0f}%)" if b else ""


def report(args, rows, tin, tout, total, ir, orr):
    L = ["# Legend — MemoryAgentBench-templated session eval", "",
         f"Model **{args.model}** · slice `{args.slice}` · "
         f"{'**DRY RUN**' if args.dry_run else 'live'} · {len(rows)} questions across the session", "",
         "## Cost",
         f"- **${total:.4f}** — {tin} in + {tout} out tokens ({ir:g}/{orr:g} $/MTok, *verify pricing*)",
         f"- **${total/max(1,len(rows)):.4f}/question**", ""]
    if not args.dry_run:
        L += ["## Scorecard (by competency)", "", "| competency | score | substrate recall |", "|---|---|---|"]
        for c, name in COMPS.items():
            cr = [r for r in rows if r["comp"] == c]
            if not cr:
                continue
            ok = sum(1 for r in cr if r["ok"])
            sub = [r for r in cr if r["frame_has"] is not None]
            subhit = sum(1 for r in sub if r["frame_has"])
            L.append(f"| {name} | {ok}/{len(cr)}{pct(ok,len(cr))} | {f'{subhit}/{len(sub)}' if sub else '—'} |")
        allok = sum(1 for r in rows if r["ok"])
        L.append(f"| **overall** | **{allok}/{len(rows)}{pct(allok,len(rows))}** | |")
        L += ["", "*substrate recall = did Legend's frame contain the gold at all "
              "(retrieval), vs the model failing to use it (reasoning).*", ""]
    L += ["## Session transcript", "", "_facts accrue top-to-bottom; questions asked at the "
          "point in the session shown by the line checkpoint._", ""]
    last = None
    for n, r in enumerate(rows):
        if r["after_line"] != last:
            L.append(f"\n**◆ checkpoint: after line {r['after_line']} of the session**\n")
            last = r["after_line"]
        gold = r["gold"]
        golds = ("_(must abstain)_" if r["mode"] == "abstain" else
                 f"_(must NOT leak: {gold})_" if r["mode"] == "forbid" else str(gold))
        L += [f"**Q{n} · {COMPS[r['comp']]} · `{r['cat']}` — {r['verdict']}**",
              f"- 🧑 {r['q']}",
              f"- 🤖 {r['answer']}",
              f"- gold: {golds}" + ("" if args.dry_run else
                 f" · frame had it: {'yes' if r['frame_has'] else 'no' if r['frame_has'] is not None else 'n/a'}"
                 f" · {r['ci']}+{r['co']} tok · ${r['cost']:.5f}"),
              f"- _note: {r['notes']}_" if r.get("notes") else "",
              "<details><summary>recall frame</summary>", "", "```json",
              json.dumps(r["frame"], ensure_ascii=False, indent=1)[:5000], "```", "</details>", ""]
    Path(args.out).write_text("\n".join(x for x in L if x is not None))


if __name__ == "__main__":
    main()
