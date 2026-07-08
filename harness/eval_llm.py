#!/usr/bin/env python3
"""LLM-in-the-loop eval: does a real model answer questions from Legend's frames?

The corpus probes already carry gold: `current_state` entries are factual Q&A
(target/property -> expect) and `absent` entries are the faithfulness test (the
concept must NOT be answerable yet). This replays the corpus (via run.py, which
fires each probe at its after_line checkpoint), hands the recall frame to a
model, and scores the answer — decomposing every result so you can tell WHY it
passed or failed, and tracking exactly what the run cost.

The decomposition that makes results legible:
  - substrate recall : was the gold value present in the frame at all?
                       (Legend's retrieval succeeded)
  - reasoning acc.   : given the gold WAS in the frame, did the model answer it?
                       (frame ergonomics / model)
  - end-to-end acc.  : the product metric — correct answers / questions
  - abstention acc.  : on absent probes, did the model correctly say nothing?
                       (end-to-end faithfulness — no hallucination)

  python3 harness/eval_llm.py --slice smoke [--model ...] [--limit N] [--dry-run]

--dry-run builds frames + prompts and PROJECTS cost without calling the API
(no key needed). A live run needs ANTHROPIC_API_KEY.
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, json, re, subprocess, tempfile, time, urllib.request, urllib.error
from pathlib import Path

def _key_from_dotenv():
    """ANTHROPIC_API_KEY fallback: the repo-root .env (gitignored)."""
    if os.environ.get("ANTHROPIC_API_KEY"):
        return
    p = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".env")
    try:
        for ln in open(p):
            if ln.startswith("ANTHROPIC_API_KEY=") and ln.split("=", 1)[1].strip():
                os.environ["ANTHROPIC_API_KEY"] = ln.split("=", 1)[1].strip()
    except OSError:
        pass
_key_from_dotenv()


ROOT = Path(__file__).resolve().parent.parent

# $/MTok (input, output). VERIFY against current Anthropic pricing before trusting
# the dollar figures — the token counts are exact, the rates are what convert them.
PRICES = {
    "claude-haiku-4-5-20251001": (1.00, 5.00),
    "claude-sonnet-5":           (3.00, 15.00),
    "claude-opus-4-8":           (15.00, 75.00),
}
DEFAULT_MODEL = "claude-haiku-4-5-20251001"

ANSWER_SYS = (
    "You are the reasoning model behind a long-term memory system. Answer the "
    "question using ONLY the recall context provided. If the answer is not present "
    "in the context, reply with exactly NOT_IN_CONTEXT and nothing else. Otherwise "
    "reply with just the specific value, as briefly as possible."
)


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, cwd=ROOT, **kw)


def norm(s):
    return re.sub(r"\s+", " ", str(s).strip().lower())


def humanize(prop):
    return prop.replace("_", " ").strip()


def est_tokens(s):
    return max(1, len(s) // 4)  # ~4 chars/token; dry-run projection only


def build_questions(slice_):
    """Replay the corpus with probes; derive (question, gold, frame) from the
    current_state + absent probe results joined to their gold."""
    tmp = Path(tempfile.mkdtemp(prefix="legend-eval-"))
    legend = str(tmp / "legend")
    r = sh(["gcc", "-std=c99", "-Wall", "-Wextra", "-Werror", "-O2",
            "legend.c", "embed.c", "-o", legend, "-lm"])
    if r.returncode:
        sys.exit("build failed:\n" + r.stderr)
    corpus = tmp / "corpus.jsonl"
    probes = ROOT / f"harness/corpus/probes_{slice_}.json"
    if sh([sys.executable, "-P", "harness/gen_corpus.py", "--slice", slice_,
           "-o", str(corpus)]).returncode:
        sys.exit("gen_corpus failed")
    results = tmp / "probe_results.json"
    subprocess.run([sys.executable, "-P", "harness/run.py", "--legend", legend,
                    "--replay", str(corpus), "--probes", str(probes),
                    "--probe-results", str(results), "--store", str(tmp / "store")],
                   cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    probe_doc = json.loads(probes.read_text())
    frames = {(x["group"], x["index"]): x for x in json.loads(results.read_text())}

    qs = []
    for i, e in enumerate(probe_doc.get("current_state", [])):
        fr = frames.get(("current_state", i))
        if not fr or fr["frame"] is None:
            continue
        qs.append({
            "kind": "fact", "group": "current_state",
            "q": f'What is the {humanize(e["property"])} of "{e["target"]}"?',
            "gold": str(e["expect"]), "frame": fr["frame"], "notes": e.get("notes", ""),
        })
    for i, e in enumerate(probe_doc.get("absent", [])):
        fr = frames.get(("absent", i))
        if not fr or fr["frame"] is None:
            continue
        focus = e["probe"]["payload"]["focus"][0]
        qs.append({
            "kind": "absent", "group": "absent",
            "q": f'What is "{focus}"?', "gold": None,
            "frame": fr["frame"], "notes": e.get("notes", ""),
        })
    return qs


def call_llm(model, user):
    body = json.dumps({"model": model, "max_tokens": 128, "system": ANSWER_SYS,
                       "messages": [{"role": "user", "content": user}]}).encode()
    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages", data=body,
        headers={"x-api-key": os.environ["ANTHROPIC_API_KEY"],
                 "anthropic-version": "2023-06-01", "content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        d = json.load(resp)
    text = "".join(b.get("text", "") for b in d.get("content", []))
    u = d.get("usage", {})
    return text.strip(), u.get("input_tokens", 0), u.get("output_tokens", 0)


def gold_in(text, gold):
    """Best-effort: is the gold value present in `text`? Normalized containment
    either direction, plus token overlap for multi-word golds. Reported raw too,
    so borderline cases stay auditable."""
    a, g = norm(text), norm(gold)
    if not g:
        return False
    if g in a or (len(a) > 2 and a in g):
        return True
    gt, at = set(g.split()), set(a.split())
    return len(gt & at) / len(gt) >= 0.6 if gt else False


def score(q, answer):
    abstained = norm(answer) == "not_in_context" or "not_in_context" in norm(answer)
    if q["kind"] == "absent":
        return ("PASS" if abstained else "HALLUCINATION"), False, abstained
    frame_has = gold_in(json.dumps(q["frame"], ensure_ascii=False), q["gold"])
    correct = (not abstained) and gold_in(answer, q["gold"])
    if correct:
        verdict = "PASS"
    elif frame_has:
        verdict = "REASONING_MISS"   # Legend surfaced it; model missed it
    else:
        verdict = "SUBSTRATE_MISS"    # Legend didn't surface it
    return verdict, frame_has, correct


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--slice", default="smoke", choices=["smoke", "adversarial", "dev", "full"])
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--limit", type=int, default=10**9)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--out", default=str(ROOT / "eval_run.md"))
    args = ap.parse_args()
    in_rate, out_rate = PRICES.get(args.model, (None, None))
    if in_rate is None:
        print(f"warning: no pricing for {args.model}; dollar figures will be 0", file=sys.stderr)
        in_rate = out_rate = 0.0
    if not args.dry_run and not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("ANTHROPIC_API_KEY not set. Run with --dry-run to project cost, "
                 "or `!export ANTHROPIC_API_KEY=...` first.")

    qs = build_questions(args.slice)[: args.limit]
    rows, tin, tout = [], 0, 0
    for n, q in enumerate(qs):
        ctx = json.dumps(q["frame"], ensure_ascii=False)
        user = f"Recall context:\n{ctx}\n\nQuestion: {q['q']}"
        if args.dry_run:
            ci, co = est_tokens(ANSWER_SYS + user), 20
            answer, verdict, frame_has, correct = "(dry-run)", "(dry-run)", None, None
        else:
            try:
                answer, ci, co = call_llm(args.model, user)
            except urllib.error.HTTPError as e:
                answer, ci, co = f"API_ERROR {e.code}: {e.read().decode()[:200]}", 0, 0
            verdict, frame_has, correct = score(q, answer)
        tin += ci; tout += co
        cost = ci / 1e6 * in_rate + co / 1e6 * out_rate
        rows.append({**q, "answer": answer, "verdict": verdict, "frame_has": frame_has,
                     "correct": correct, "ci": ci, "co": co, "cost": cost})
        print(f"  Q{n:<2} [{q['group']:<13}] {verdict:<15} "
              f"{'' if args.dry_run else ('gold=%r ans=%r' % (q['gold'], answer[:48]))}")

    total_cost = tin / 1e6 * in_rate + tout / 1e6 * out_rate
    facts = [r for r in rows if r["kind"] == "fact"]
    absents = [r for r in rows if r["kind"] == "absent"]
    write_report(args, rows, facts, absents, tin, tout, total_cost, in_rate, out_rate)

    print("\n" + "=" * 60)
    print(f"model: {args.model}   ({in_rate:g}/{out_rate:g} $/MTok — verify vs current pricing)")
    print(f"COST: ${total_cost:.4f}   ({tin} in + {tout} out tok)"
          f"{'  [PROJECTED — dry run]' if args.dry_run else ''}")
    print(f"      ${total_cost/max(1,len(rows)):.4f} / question over {len(rows)} questions")
    if not args.dry_run:
        nf = len(facts); surfaced = sum(1 for r in facts if r["frame_has"])
        e2e = sum(1 for r in facts if r["correct"])
        print(f"\nFACT questions (n={nf}):")
        print(f"  end-to-end accuracy : {e2e}/{nf}" + pct(e2e, nf) + "   <- the product metric")
        print(f"  substrate recall    : {surfaced}/{nf}" + pct(surfaced, nf) + "   <- Legend surfaced the answer")
        print(f"  reasoning accuracy  : {e2e}/{surfaced}" + pct(e2e, surfaced) + "   <- model used it, given surfaced")
        if absents:
            ab = sum(1 for r in absents if r["verdict"] == "PASS")
            print(f"\nABSENT questions (n={len(absents)}):")
            print(f"  abstention accuracy : {ab}/{len(absents)}" + pct(ab, len(absents)) + "   <- faithfulness (no hallucination)")
    print(f"\nfull report: {args.out}")


def pct(a, b):
    return f" ({100*a/b:.0f}%)" if b else " (n/a)"


def write_report(args, rows, facts, absents, tin, tout, total_cost, in_rate, out_rate):
    L = ["# Legend LLM-in-the-loop eval", "",
         f"Model **{args.model}** · slice `{args.slice}` · "
         f"{'**DRY RUN (projected cost, no API calls)**' if args.dry_run else 'live'}", "",
         "## Cost",
         f"- **${total_cost:.4f}** total — {tin} input + {tout} output tokens "
         f"(rates {in_rate:g}/{out_rate:g} $/MTok, *verify against current pricing*)",
         f"- **${total_cost/max(1,len(rows)):.4f}/question** over {len(rows)} questions", ""]
    if not args.dry_run:
        nf = len(facts); surfaced = sum(1 for r in facts if r["frame_has"]); e2e = sum(1 for r in facts if r["correct"])
        L += ["## Scores",
              f"| metric | value | reads as |", "|---|---|---|",
              f"| end-to-end accuracy | {e2e}/{nf}{pct(e2e,nf)} | correct answers / fact questions |",
              f"| substrate recall | {surfaced}/{nf}{pct(surfaced,nf)} | Legend surfaced the gold value |",
              f"| reasoning accuracy | {e2e}/{surfaced}{pct(e2e,surfaced)} | model answered it, given surfaced |"]
        if absents:
            ab = sum(1 for r in absents if r["verdict"] == "PASS")
            L.append(f"| abstention accuracy | {ab}/{len(absents)}{pct(ab,len(absents))} | correctly said nothing on absent probes |")
        L.append("")
    L += ["## Questions", ""]
    for n, r in enumerate(rows):
        L += [f"### Q{n} · `{r['group']}` · **{r['verdict']}**",
              f"- **Question:** {r['q']}",
              f"- **Gold:** {r['gold'] if r['gold'] is not None else '_(should abstain — not in memory yet)_'}",
              f"- **Model answer:** {r['answer']}",
              (f"- **Frame contained gold:** {'yes' if r['frame_has'] else 'no'} · "
               f"tokens {r['ci']}+{r['co']} · ${r['cost']:.5f}") if not args.dry_run
              else f"- projected tokens ~{r['ci']}+{r['co']} · ~${r['cost']:.5f}",
              f"- _probe note: {r['notes']}_" if r.get("notes") else "",
              "<details><summary>recall frame the model saw</summary>", "",
              "```json", json.dumps(r["frame"], ensure_ascii=False, indent=1)[:6000], "```",
              "</details>", ""]
    Path(args.out).write_text("\n".join(x for x in L if x is not None))


if __name__ == "__main__":
    main()
