#!/usr/bin/env python3
"""Full-loop eval: the LLM does BOTH jobs it does in production.

  raw prose --[LLM extracts]--> Legend saves --> ...recall--> frame --[LLM reads]--> answer

Our earlier evals hand Legend a pre-structured corpus (perfect extraction). This
one closes the write side: each episode is deterministically rendered to a
developer-journal blob (no second LLM colluding on structure), the model extracts
Legend save payloads from that prose with only the schema to guide it, and those
saves build the store. Then the same competency questions are answered from
recall. Every miss is attributed to the layer that caused it:

  EXTRACTION_MISS : gold never made it into the substrate (`dump`) — the LLM
                    didn't capture it, or saved it under the wrong shape
  RETRIEVAL_MISS  : gold is in the substrate but not in the recall frame
  REASONING_MISS  : gold is in the frame but the model answered wrong
  PASS            : correct

Ingested at final state, so `absent` (not-yet) probes are dropped; everything
else (current values, paraphrase, history, retracted-no-leak) holds.

  python3 harness/eval_extract.py [--model ...] [--limit-episodes N] [--dry-run]
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, glob, json, re, subprocess, tempfile, urllib.request, urllib.error
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PRICES = {"claude-haiku-4-5-20251001": (1.00, 5.00), "claude-sonnet-5": (3.00, 15.00),
          "claude-opus-4-8": (15.00, 75.00)}
DEFAULT_MODEL = "claude-haiku-4-5-20251001"

EXTRACT_SYS = """You are the memory-writing side of an LLM using a structured long-term memory called Legend. You receive raw developer notes and must emit a Legend `save` payload capturing the durable facts, so they can be recalled later.

Output ONLY a JSON object (no prose, no markdown fence) of this shape:
{
  "source": "dev-notes",
  "elements": [
    {"name": "<canonical name>", "kind": "<one of: project function module system mechanic parameter constraint decision spell enemy character place question>",
     "aliases": ["<other names>"], "summary": "<one-line description>"}
  ],
  "facts": [
    {"s": "<subject element name>", "p": "<relationship or property>", "o": "<object element name OR literal value>"}
  ]
}
Rules:
- An element is a durable named thing. A fact is a triple: subject, property/relationship, object.
- Current-value facts use a property name (e.g. {"s":"mana economy","p":"starting_mana","o":"0"}). When a note says a value CHANGED, emit the NEW value with the SAME subject+property — Legend supersedes the old one automatically.
- If a note says a fact was retracted/withdrawn/wrong, DO NOT emit it.
- Prefer the exact names and values the notes use. Keep summaries short. Emit only what the notes state."""


def present(text, gold):
    a, g = re.sub(r"\s+", " ", str(text).lower()), re.sub(r"\s+", " ", str(gold).strip().lower())
    if not g:
        return False
    if g in a or (len(a) > 2 and a in g):
        return True
    gt = set(g.split())
    return bool(gt) and len(gt & set(a.split())) / len(gt) >= 0.6


def humanize(p):
    return str(p).replace("_", " ").strip()


def render_episode(ep):
    """Deterministic prose from an episode's save steps — developer notes a human
    might actually write, with the structure flattened out."""
    # NB: ep["notes"] is corpus-author meta ("...retracted in step 4...") — it
    # would hand the extractor the answers, so only element/fact content is used.
    out = [f"Dev notes — {ep.get('episode','session')}:"]
    for st in ep["steps"]:
        if st["verb"] != "save":
            continue
        for e in st["payload"].get("elements", []):
            s = e["name"]
            if e.get("aliases"):
                s += f" (also called {', '.join(e['aliases'])})"
            if e.get("kind"):
                s += f" is a {e['kind']}"
            if e.get("summary"):
                s += f": {e['summary']}"
            s += "."
            for k, v in (e.get("attrs") or {}).items():
                s += f" Its {humanize(k)} is {v}."
            if e.get("rename_to"):
                s += f" It has been renamed to {e['rename_to']}."
            out.append(s)
        for f in st["payload"].get("facts", []):
            tag = ""
            if f.get("status") in ("defeasible",):
                tag = " (tentative)"
            if f.get("status") in ("retracted",):
                tag = " — but this was retracted and is no longer true"
            out.append(f"{f['s']} {humanize(f['p'])}: {f['o']}{tag}.")
    return "\n".join(out)


def call_llm(model, system, user, max_tokens):
    body = json.dumps({"model": model, "max_tokens": max_tokens, "system": system,
                       "messages": [{"role": "user", "content": user}]}).encode()
    req = urllib.request.Request("https://api.anthropic.com/v1/messages", data=body,
        headers={"x-api-key": os.environ["ANTHROPIC_API_KEY"],
                 "anthropic-version": "2023-06-01", "content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as r:
        d = json.load(r)
    u = d.get("usage", {})
    return "".join(b.get("text", "") for b in d.get("content", [])).strip(), \
        u.get("input_tokens", 0), u.get("output_tokens", 0)


def parse_payload(text):
    """Pull a JSON object out of the model's reply (tolerate fences/prose)."""
    t = re.sub(r"^```(json)?|```$", "", text.strip(), flags=re.M).strip()
    try:
        return json.loads(t)
    except json.JSONDecodeError:
        m = re.search(r"\{.*\}", t, re.S)
        return json.loads(m.group(0)) if m else None


def derive_questions(slice_):
    """Final-state competency questions from the probes (absent dropped)."""
    doc = json.loads((ROOT / f"harness/corpus/probes_{slice_}.json").read_text())
    qs = []
    for e in doc.get("current_state", []):
        if {"property", "target", "expect"} <= e.keys():
            qs.append(dict(comp="AR", q=f'What is the {humanize(e["property"])} of "{e["target"]}" right now?',
                           gold=str(e["expect"]), mode="value", cat="current_state", payload=e["probe"]["payload"]))
    for e in doc.get("cold_caller", []):
        if "expect_resolves_to" in e:
            f = e["probe"]["payload"]["focus"][0]
            qs.append(dict(comp="AR", q=f'Memory may store this under another name. What is "{f}"? Name the specific entity.',
                           gold=e["expect_resolves_to"], mode="value", cat="cold_caller", payload=e["probe"]["payload"]))
    for e in doc.get("deep_history", []):
        if {"property", "target", "expect_current"} <= e.keys():
            qs.append(dict(comp="CR", q=f'What is the current {humanize(e["property"])} of "{e["target"]}"?',
                           gold=str(e["expect_current"]), mode="value", cat="deep_history", payload=e["probe"]["payload"]))
            if e.get("expect_history"):
                qs.append(dict(comp="HIST", q=f'What was the {humanize(e["property"])} of "{e["target"]}" BEFORE its current value?',
                               gold=e["expect_history"], mode="any", cat="deep_history", payload=e["probe"]["payload"]))
    for e in doc.get("exclusion", []):
        if e.get("forbid_names"):
            f = e["probe"]["payload"]["focus"][0]
            qs.append(dict(comp="FAITH", q=f'What is "{f}"? Answer only from memory.',
                           gold=e["forbid_names"], mode="forbid", cat="exclusion", payload=e["probe"]["payload"]))
    return qs


def check(mode, gold, text):
    if mode == "all":
        return all(present(text, g) for g in gold)
    if mode == "any":
        return any(present(text, g) for g in gold)
    if mode == "forbid":  # strict substring — no fuzzy, to avoid false leaks
        t = re.sub(r"\s+", " ", str(text).lower())
        return not any(re.sub(r"\s+", " ", str(g).strip().lower()) in t for g in gold)
    return present(text, gold)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--slice", default="adversarial")
    ap.add_argument("--limit-episodes", type=int, default=10**9)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--out", default=str(ROOT / "eval_extract.md"))
    args = ap.parse_args()
    ir, orr = PRICES.get(args.model, (0.0, 0.0))
    if not args.dry_run and not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("ANTHROPIC_API_KEY not set (use --dry-run to project cost).")

    tmp = Path(tempfile.mkdtemp(prefix="legend-extract-"))
    legend = str(tmp / "legend")
    if subprocess.run(["gcc", "-std=c99", "-O2", "legend.c", "embed.c", "-o", legend, "-lm"],
                      cwd=ROOT, capture_output=True, text=True).returncode:
        sys.exit("build failed")
    store = tmp / "store"
    env = dict(os.environ, LEGEND_STATE_DIR=str(store))
    subprocess.run([legend, "init"], env=env, capture_output=True)

    episodes = sorted(glob.glob(str(ROOT / "harness/corpus/episodes/*.json")))
    eps = [json.load(open(f)) for f in episodes]
    eps.sort(key=lambda e: e["steps"][0].get("now", 0))
    eps = eps[: args.limit_episodes]

    tin = tout = 0
    ing = []
    now = 1780000000
    for ep in eps:
        prose = render_episode(ep)
        if args.dry_run:
            ing.append({"ep": ep["episode"], "chars": len(prose), "saved": None,
                        "ci": max(1, len(EXTRACT_SYS + prose) // 4), "co": 220, "err": None})
            tin += ing[-1]["ci"]; tout += ing[-1]["co"]; continue
        try:
            reply, ci, co = call_llm(args.model, EXTRACT_SYS, prose, 2000)
            payload = parse_payload(reply)
        except (urllib.error.HTTPError, json.JSONDecodeError) as ex:
            ing.append({"ep": ep["episode"], "saved": False, "err": str(ex)[:120], "ci": 0, "co": 0})
            continue
        tin += ci; tout += co
        err = None
        if not payload or "elements" not in payload:
            err = "no valid payload"
        else:
            payload.setdefault("source", "dev-notes")
            now += 1000
            r = subprocess.run([legend, "save"], input=json.dumps(payload),
                               capture_output=True, text=True,
                               env=dict(env, LEGEND_NOW=str(now)))
            if r.returncode != 0:
                err = f"legend rejected: {r.stdout[:120]}"
        ing.append({"ep": ep["episode"], "ne": len((payload or {}).get("elements", [])),
                    "nf": len((payload or {}).get("facts", [])), "err": err, "ci": ci, "co": co})

    # substrate snapshot for extraction attribution
    substrate = ""
    if not args.dry_run:
        substrate = subprocess.run([legend, "dump"], env=env, capture_output=True, text=True).stdout

    qs = derive_questions(args.slice)
    rows = []
    for q in qs:
        rr = subprocess.run([legend, "recall"], input=json.dumps(q["payload"]),
                            capture_output=True, text=True, env=env)
        frame = rr.stdout
        user = f"Recall context:\n{frame}\n\nQuestion: {q['q']}"
        if args.dry_run:
            ci, co, ans = max(1, len(user) // 4), 25, "(dry-run)"
            verdict = "(dry-run)"
        else:
            try:
                ans, ci, co = call_llm(args.model,
                    "Answer ONLY from the recall context. If absent, reply exactly NOT_IN_CONTEXT. Else reply with just the value(s).",
                    user, 200)
            except urllib.error.HTTPError as ex:
                ans, ci, co = f"API_ERROR {ex.code}", 0, 0
            extracted = check(q["mode"], q["gold"], substrate) if q["mode"] != "forbid" else True
            retrieved = check(q["mode"], q["gold"], frame) if q["mode"] != "forbid" else True
            answered = check(q["mode"], q["gold"], ans) and "not_in_context" not in ans.lower()
            if q["mode"] == "forbid":
                verdict = "PASS" if check("forbid", q["gold"], ans) else "LEAK"
            elif answered:
                verdict = "PASS"
            elif retrieved:
                verdict = "REASONING_MISS"
            elif extracted:
                verdict = "RETRIEVAL_MISS"
            else:
                verdict = "EXTRACTION_MISS"
        tin += ci; tout += co
        rows.append({**q, "answer": ans, "verdict": verdict, "ci": ci, "co": co})
        if not args.dry_run:
            print(f"  {q['comp']:<5} {verdict:<16} gold={str(q['gold'])[:32]!r} ans={ans[:36]!r}")

    total = tin / 1e6 * ir + tout / 1e6 * orr
    write_report(args, ing, rows, tin, tout, total, ir, orr)
    print("\n" + "=" * 66)
    ok_saves = sum(1 for x in ing if not x.get("err"))
    print(f"INGEST: {ok_saves}/{len(ing)} episodes extracted+saved cleanly")
    print(f"model {args.model}  COST ${total:.4f}  ({tin} in + {tout} out tok)"
          + ("  [PROJECTED]" if args.dry_run else ""))
    if not args.dry_run:
        for v in ("PASS", "REASONING_MISS", "RETRIEVAL_MISS", "EXTRACTION_MISS", "LEAK"):
            c = sum(1 for r in rows if r["verdict"] == v)
            if c:
                print(f"  {v:<16} {c}/{len(rows)}")
    print(f"\nfull report: {args.out}")


def write_report(args, ing, rows, tin, tout, total, ir, orr):
    L = ["# Legend — full-loop eval (LLM extracts AND reads)", "",
         f"Model **{args.model}** · {'**DRY RUN**' if args.dry_run else 'live'}", "",
         "## Cost",
         f"- **${total:.4f}** — {tin} in + {tout} out tok ({ir:g}/{orr:g} $/MTok, *verify pricing*)", ""]
    if not args.dry_run:
        from collections import Counter
        c = Counter(r["verdict"] for r in rows)
        n = len(rows)
        L += ["## Where answers came from (three-way attribution)", "",
              "| outcome | count | layer |", "|---|---|---|",
              f"| PASS | {c.get('PASS',0)}/{n} | correct |",
              f"| REASONING_MISS | {c.get('REASONING_MISS',0)}/{n} | model misread a frame that HAD the answer |",
              f"| RETRIEVAL_MISS | {c.get('RETRIEVAL_MISS',0)}/{n} | Legend didn't surface a fact it HAD stored |",
              f"| EXTRACTION_MISS | {c.get('EXTRACTION_MISS',0)}/{n} | the fact never got into the store |",
              f"| LEAK | {c.get('LEAK',0)}/{n} | retracted value resurfaced (faithfulness) |", ""]
    L += ["## Ingest (LLM extraction of each episode)", "",
          "| episode | elements | facts | status |", "|---|---|---|---|"]
    for x in ing:
        L.append(f"| {x['ep']} | {x.get('ne','~')} | {x.get('nf','~')} | {x.get('err') or 'ok'} |")
    L += ["", "## Questions", ""]
    for i, r in enumerate(rows):
        L += [f"### Q{i} · {r['comp']} · `{r['cat']}` — {r['verdict']}",
              f"- 🧑 {r['q']}", f"- 🤖 {r['answer']}", f"- gold: {r['gold']}", ""]
    Path(args.out).write_text("\n".join(L))


if __name__ == "__main__":
    main()
