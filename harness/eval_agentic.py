#!/usr/bin/env python3
"""Agentic full-loop eval: the model drives Legend through real tool-use.

The naive eval_extract.py cold-parses a JSON blob per episode — which (a) has no
recall-before-save loop, so entity names fragment across episodes, and (b) fishes
JSON out of prose, which drops malformed replies. Both are artifacts of NOT using
the production scaffolding (MCP tool interface + hooks). This harness fixes both:
`legend_recall` and `legend_save` are real tools with input schemas, and the model
runs an agentic loop per episode — recall to find what already exists, reuse those
canonical names, then save new/changed facts. Structured tool inputs mean no
JSON-from-text parsing.

Then the same competency questions are answered from recall, with three-way
attribution (extraction / retrieval / reasoning). The gap between this and the
naive harness is the measured value of the recall-before-save loop.

  python3 harness/eval_agentic.py [--model ...] [--limit-episodes N] [--dry-run]
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, glob, json, re, subprocess, tempfile, time, urllib.request, urllib.error
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
PRICES = {"claude-haiku-4-5-20251001": (1.00, 5.00), "claude-sonnet-5": (3.00, 15.00),
          "claude-opus-4-8": (15.00, 75.00)}
DEFAULT_MODEL = "claude-haiku-4-5-20251001"

# The orientation a real MCP server + session hook would give the model.
GUIDE = (ROOT / "harness/legend_guide.md").read_text()
WRITE_SYS = GUIDE + """

## Your task now
You are given raw developer notes. Update Legend so memory reflects the durable facts they
state. Recall the entities the notes mention FIRST (to find existing canonical names), then
save new or changed facts reusing those names. Work only through the tools; do not write prose."""
ANSWER_SYS = GUIDE + """

## Your task now
Answer the user's question using ONLY the recall context provided. If the answer is not in the
context, reply with exactly NOT_IN_CONTEXT and nothing else. Otherwise reply with just the value(s)."""

TOOLS = [
    {"name": "legend_recall",
     "description": "Look up what memory already knows about one or more entities/topics. Returns the focused memory frame. Use before saving to find existing canonical names.",
     "input_schema": {"type": "object", "properties": {
         "focus": {"type": "array", "items": {"type": "string"},
                   "description": "entity names or topics to look up"}},
         "required": ["focus"]}},
    {"name": "legend_save",
     "description": "Write entities and facts to memory. Reuse existing canonical names for known entities. Saving a new value for the same subject+property supersedes the old one.",
     "input_schema": {"type": "object", "properties": {
         "elements": {"type": "array", "items": {"type": "object", "properties": {
             "name": {"type": "string"}, "kind": {"type": "string"},
             "aliases": {"type": "array", "items": {"type": "string"}},
             "summary": {"type": "string"}}, "required": ["name"]}},
         "facts": {"type": "array", "items": {"type": "object", "properties": {
             "s": {"type": "string"}, "p": {"type": "string"}, "o": {"type": "string"}},
             "required": ["s", "p", "o"]}}},
         "required": ["elements", "facts"]}},
]


def present(text, gold):
    a = re.sub(r"\s+", " ", str(text).lower())
    g = re.sub(r"\s+", " ", str(gold).strip().lower())
    if not g:
        return False
    # short or numeric golds ("0", "14", "2.4x", "5") match only on a token
    # boundary, so "0" can't hit inside "10 minutes" and "2x" can't hit "12x".
    if len(g) <= 4 or re.fullmatch(r"[0-9]+(\.[0-9]+)?x?", g):
        return re.search(r"(?<![\w.])" + re.escape(g) + r"(?![\w.])", a) is not None
    if g in a:
        return True
    gt = set(g.split())
    return bool(gt) and len(gt & set(a.split())) / len(gt) >= 0.6


def humanize(p):
    return str(p).replace("_", " ").strip()


def render_episode(ep):
    """Reconstruct the episode's gold as natural developer notes — the prose a
    dev might jot after a session, not a structured fact list. Same facts, so the
    gold stays derivable; the model still has to extract and shape them itself."""
    out = []
    for st in ep["steps"]:
        if st["verb"] != "save":
            continue
        for e in st["payload"].get("elements", []):
            name = e["name"]
            alias = f" (aka {', '.join(e['aliases'])})" if e.get("aliases") else ""
            kind, summ, attrs = e.get("kind"), e.get("summary"), (e.get("attrs") or {})
            if kind == "decision" and (attrs.get("chose") or attrs.get("rejected")):
                s = f"Decided on {name}{alias}"
                if attrs.get("about"):
                    s += f" (for {attrs['about']})"
                s += f": going with {attrs.get('chose', 'it')}"
                if attrs.get("rejected"):
                    s += f" instead of {attrs['rejected']}"
                if attrs.get("reason"):
                    s += f", since {attrs['reason']}"
                out.append(s + ".")
                continue
            if kind == "constraint":
                s = f"Hard rule — {name}{alias}"
                s += f": {summ}" if summ else ""
                s += f" (the point: {attrs['reason']})" if attrs.get("reason") else ""
                out.append(s + ".")
                continue
            s = f"{name}{alias}" + (f" is a {kind}" if kind else "")
            if summ:
                s += (": " if kind else " — ") + summ
            out.append(s + ".")
            for k, v in attrs.items():
                out.append(f"Its {humanize(k)} is {v}.")
            if e.get("rename_to"):
                out.append(f"Renamed {name} to {e['rename_to']}.")
        for f in st["payload"].get("facts", []):
            tag = " (still tentative)" if f.get("status") == "defeasible" else \
                  " — scratch that, no longer true" if f.get("status") == "retracted" else ""
            out.append(f"{f['s']}'s {humanize(f['p'])} is {f['o']}{tag}.")
        for c in st["payload"].get("changes", []):
            tgt, prop, to, frm = c.get("target"), humanize(c.get("property", "")), c.get("to"), c.get("from")
            out.append(f"Retuned {tgt}'s {prop} to {to} (was {frm})." if frm
                       else f"{tgt}'s {prop} is now {to}.")
        rt = st["payload"].get("retract")
        for r in (rt if isinstance(rt, list) else [rt] if rt else []):
            out.append(f"{r['s']}'s {humanize(r['p'])} is no longer {r['o']}.")
        mg = st["payload"].get("merge")
        for m in (mg if isinstance(mg, list) else [mg] if mg else []):
            out.append(f"{m.get('from')} turned out to be the same thing as {m.get('into')} — merge them.")
    return " ".join(out)


def api(model, system, messages, tools=None, max_tokens=1024):
    body = {"model": model, "max_tokens": max_tokens, "system": system, "messages": messages}
    if tools:
        body["tools"] = tools
    data = json.dumps(body).encode()
    last = None
    for attempt in range(5):  # transient timeouts/429/5xx shouldn't nuke a 40-call run
        try:
            req = urllib.request.Request("https://api.anthropic.com/v1/messages", data=data,
                headers={"x-api-key": os.environ["ANTHROPIC_API_KEY"],
                         "anthropic-version": "2023-06-01", "content-type": "application/json"})
            with urllib.request.urlopen(req, timeout=180) as r:
                d = json.load(r)
            u = d.get("usage", {})
            return d, u.get("input_tokens", 0), u.get("output_tokens", 0)
        except urllib.error.HTTPError as e:
            if e.code < 500 and e.code != 429:  # 400-class: retrying won't help
                raise
            last = e
        except (urllib.error.URLError, TimeoutError, ConnectionError, json.JSONDecodeError) as e:
            last = e
        time.sleep(3 * (attempt + 1))
    raise last


def legend(store, verb, payload=None, now=None):
    env = dict(os.environ, LEGEND_STATE_DIR=str(store))
    if now:
        env["LEGEND_NOW"] = str(now)
    r = subprocess.run(["./legend_bin", verb] if False else [str(store.parent / "legend"), verb],
                       input=(json.dumps(payload) if payload is not None else ""),
                       capture_output=True, text=True, env=env, cwd=ROOT)
    return r.stdout, r.returncode


SAVE_LOG = []  # every payload the model actually wrote, for volume-vs-form diagnosis


def ingest_episode(model, store, prose, now, ep_name=""):
    """Agentic recall-before-save loop for one episode. Returns a rich trace record.

    Saves are buffered and flushed as ONE `legend save` per episode (before any recall
    that must see them, or at end-of-reasoning), so a single legend process loads the
    embedding model once and embeds the whole batch warm instead of reloading per call."""
    messages = [{"role": "user", "content": f"{prose}\n\nUpdate memory to reflect these notes."}]
    tin = tout = saves = 0
    rounds = []
    pend_el, pend_fa = [], []

    def flush(rnd):
        nonlocal saves, pend_el, pend_fa
        if not pend_el and not pend_fa:
            return
        pl = {"source": "dev-notes", "elements": pend_el, "facts": pend_fa}
        SAVE_LOG.append({"episode": ep_name, "elements": pend_el, "facts": pend_fa})
        t = time.perf_counter()
        out, rc = legend(store, "save", pl, now=now)
        saves += 1
        if rnd is not None:
            rnd["tools"].append({"name": "save", "input": pl, "ms": (time.perf_counter() - t) * 1000,
                                 "output": ("saved ok" if rc == 0 else f"error: {out[:200]}")})
        pend_el, pend_fa = [], []

    for _ in range(8):  # cap tool rounds per episode
        t0 = time.perf_counter()
        d, ci, co = api(model, WRITE_SYS, messages, TOOLS, max_tokens=1500)
        api_ms = (time.perf_counter() - t0) * 1000
        tin += ci; tout += co
        blocks = d.get("content", [])
        messages.append({"role": "assistant", "content": blocks})
        tool_uses = [b for b in blocks if b.get("type") == "tool_use"]
        rnd = {"api_ms": api_ms, "in": ci, "out": co, "tools": [],
               "note": "".join(b.get("text", "") for b in blocks if b.get("type") == "text").strip()}
        if not tool_uses:
            flush(rnd)  # end of reasoning — commit the batch
            rounds.append(rnd)
            break
        results = []
        for tu in tool_uses:
            if tu["name"] == "legend_recall":
                flush(rnd)  # commit pending saves so this recall can see them
                t1 = time.perf_counter()
                out, _ = legend(store, "recall", {"focus": tu["input"].get("focus", []), "observe": True})
                rnd["tools"].append({"name": "recall", "input": tu["input"],
                                     "ms": (time.perf_counter() - t1) * 1000, "output": out})
                content = out[:4000]
            else:  # legend_save — buffer for the batch, don't invoke legend yet
                pend_el.extend(tu["input"].get("elements", []))
                pend_fa.extend(tu["input"].get("facts", []))
                content = "saved ok"
            results.append({"type": "tool_result", "tool_use_id": tu["id"], "content": content})
        rounds.append(rnd)
        messages.append({"role": "user", "content": results})
    flush(rounds[-1] if rounds else None)  # round-cap safety: commit anything still buffered
    return {"episode": ep_name, "prose": prose, "rounds": rounds, "in": tin, "out": tout, "saves": saves}


def _norm(s):
    return re.sub(r"[^a-z0-9 ]", "", re.sub(r"\s+", " ", str(s).lower())).strip()


def diagnose(eps):
    """Compare what the model wrote (SAVE_LOG) against the gold saves — same payload shape."""
    # gold: final object per (subject, property), and the set of gold subjects
    gold_fp, gold_subj = {}, set()
    for ep in eps:
        for st in ep["steps"]:
            if st["verb"] != "save":
                continue
            for e in st["payload"].get("elements", []):
                gold_subj.add(_norm(e["name"]))
                for a in e.get("aliases", []):
                    gold_subj.add(_norm(a))
            for f in st["payload"].get("facts", []):
                if f.get("status") == "retracted":
                    gold_fp.pop((_norm(f["s"]), _norm(f["p"])), None)
                else:
                    gold_fp[(_norm(f["s"]), _norm(f["p"]))] = _norm(f["o"])
    # llm: subjects it named, (s,p) pairs it wrote, final object per (s,p)
    llm_subj, llm_sp, llm_fp = set(), set(), {}
    for s in SAVE_LOG:
        for e in s["elements"]:
            llm_subj.add(_norm(e["name"]))
            for a in e.get("aliases", []):
                llm_subj.add(_norm(a))
        for f in s["facts"]:
            k = (_norm(f.get("s", "")), _norm(f.get("p", "")))
            llm_sp.add(k)
            llm_fp[k] = _norm(f.get("o", ""))
    subj_all = {_norm(s) for (s, _) in gold_fp}
    subj_covered = {s for s in subj_all if any(s in ls or ls in s for ls in llm_subj)}
    sp_match = {k for k in gold_fp if k in llm_sp}
    obj_match = {k for k in sp_match if gold_fp[k] == llm_fp.get(k) or gold_fp[k] in llm_fp.get(k, "") or llm_fp.get(k, "x") in gold_fp[k]}
    print(f"\n=== VOLUME vs FORM (LLM saves vs gold) ===")
    print(f"gold facts (s,p pairs):            {len(gold_fp)}")
    print(f"llm total facts written:           {sum(len(s['facts']) for s in SAVE_LOG)}")
    print(f"gold SUBJECTS the llm covered:     {len(subj_covered)}/{len(subj_all)}   <- volume of entities")
    print(f"gold (s,p) the llm keyed exactly:  {len(sp_match)}/{len(gold_fp)}   <- FORM: right property key")
    print(f"  ...of those, object also right:  {len(obj_match)}/{len(sp_match)}   <- value/supersession")
    miss_subj = sorted(subj_all - subj_covered)
    miss_form = sorted(k for k in gold_fp if _norm_subj_covered(k[0], llm_subj) and k not in llm_sp)
    print(f"\nsubjects the llm NEVER saved ({len(miss_subj)}): {miss_subj[:12]}")
    print(f"\nfacts where subject WAS saved but property key differs (form) — sample:")
    for (s, p) in miss_form[:14]:
        near = [lp for (ls, lp) in llm_sp if ls == s]
        print(f"  gold ({s} | {p})  ->  llm has props for this subject: {near[:6]}")


def _norm_subj_covered(s, llm_subj):
    return any(s in ls or ls in s for ls in llm_subj)


def derive_questions(slice_):
    # Prefer an eval-only probe file (probes_<slice>_mcp.json) when present: it
    # may carry lenient fields (deep_history, question overrides) that the strict
    # check.sh gate schema rejects. Fall back to the gate's shared probe file.
    _mcp = ROOT / f"harness/corpus/probes_{slice_}_mcp.json"
    _pf = _mcp if _mcp.exists() else ROOT / f"harness/corpus/probes_{slice_}.json"
    doc = json.loads(_pf.read_text())
    qs = []
    for e in doc.get("current_state", []):
        if {"property", "target", "expect"} <= e.keys():
            q = e.get("question") or f'What is the {humanize(e["property"])} of "{e["target"]}" right now?'
            qs.append(dict(comp="AR", q=q, gold=str(e["expect"]), mode="value",
                           cat="current_state", payload=e["probe"]["payload"]))
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
    if mode == "forbid":
        t = re.sub(r"\s+", " ", str(text).lower())
        return not any(re.sub(r"\s+", " ", str(g).strip().lower()) in t for g in gold)
    return present(text, gold)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default=DEFAULT_MODEL)
    ap.add_argument("--slice", default="adversarial")
    ap.add_argument("--limit-episodes", type=int, default=10**9)
    ap.add_argument("--out", default=str(ROOT / "eval_agentic.md"))
    ap.add_argument("--ingest-only", action="store_true", help="write phase + volume/form diagnosis, skip queries")
    ap.add_argument("--limit-q", type=int, default=10**9, help="cap questions (cheap validation)")
    args = ap.parse_args()
    ir, orr = PRICES.get(args.model, (0.0, 0.0))
    if not os.environ.get("ANTHROPIC_API_KEY"):
        sys.exit("ANTHROPIC_API_KEY not set.")

    tmp = Path(tempfile.mkdtemp(prefix="legend-agentic-"))
    legend_bin = tmp / "legend"
    if subprocess.run(["gcc", "-std=c99", "-O2", "legend.c", "embed.c", "-o", str(legend_bin), "-lm"],
                      cwd=ROOT, capture_output=True, text=True).returncode:
        sys.exit("build failed")
    store = tmp / "store"
    subprocess.run([str(legend_bin), "init"], env=dict(os.environ, LEGEND_STATE_DIR=str(store)), capture_output=True)

    eps = [json.load(open(f)) for f in sorted(glob.glob(str(ROOT / "harness/corpus/episodes/*.json")))]
    eps.sort(key=lambda e: e["steps"][0].get("now", 0))
    eps = eps[: args.limit_episodes]

    tin = tout = tsaves = 0
    now = 1780000000
    ing = []
    for ep in eps:
        now += 1000
        rec = ingest_episode(args.model, store, render_episode(ep), now, ep["episode"])
        t = time.perf_counter()
        dump_raw, _ = legend(store, "dump")
        rec["dump_ms"] = (time.perf_counter() - t) * 1000
        try:
            dj = json.loads(dump_raw)
            rec["n_elements"], rec["n_relations"] = len(dj.get("elements", [])), len(dj.get("relations", []))
        except Exception:
            rec["n_elements"] = rec["n_relations"] = -1
        rec["dump"] = dump_raw
        ing.append(rec)
        tin += rec["in"]; tout += rec["out"]; tsaves += rec["saves"]
        print(f"  ingest {ep['episode']:<20} {rec['saves']} saves  el={rec['n_elements']} rel={rec['n_relations']}  (+{rec['in']+rec['out']} tok)")

    Path(str(args.out) + ".saves.json").write_text(json.dumps(SAVE_LOG, indent=1))
    diagnose(eps)
    if args.ingest_only:
        total = tin / 1e6 * ir + tout / 1e6 * orr
        print(f"\nmodel {args.model}  INGEST-ONLY  COST ${total:.4f}  ({tin} in + {tout} out tok)")
        print(f"raw payloads: {args.out}.saves.json")
        return

    substrate, _ = legend(store, "dump")
    qs = derive_questions(args.slice)[: args.limit_q]
    rows = []
    for q in qs:
        t = time.perf_counter()
        frame, _ = legend(store, "recall", q["payload"])
        rec_ms = (time.perf_counter() - t) * 1000
        t = time.perf_counter()
        d, ci, co = api(args.model, ANSWER_SYS,
            [{"role": "user", "content": f"Recall context:\n{frame}\n\nQuestion: {q['q']}"}], max_tokens=150)
        ans_ms = (time.perf_counter() - t) * 1000
        ans = "".join(b.get("text", "") for b in d.get("content", []) if b.get("type") == "text").strip()
        tin += ci; tout += co
        if q["mode"] == "forbid":
            verdict = "PASS" if check("forbid", q["gold"], ans) else "LEAK"
        else:
            extracted = check(q["mode"], q["gold"], substrate)
            retrieved = check(q["mode"], q["gold"], frame)
            answered = check(q["mode"], q["gold"], ans) and "not_in_context" not in ans.lower()
            verdict = "PASS" if answered else "REASONING_MISS" if retrieved else \
                      "RETRIEVAL_MISS" if extracted else "EXTRACTION_MISS"
        rows.append({**q, "answer": ans, "verdict": verdict, "frame": frame,
                     "rec_ms": rec_ms, "ans_ms": ans_ms, "in": ci, "out": co})
        print(f"  {q['comp']:<5} {verdict:<16} gold={str(q['gold'])[:30]!r} ans={ans[:34]!r}")

    total = tin / 1e6 * ir + tout / 1e6 * orr
    from collections import Counter
    c = Counter(r["verdict"] for r in rows)
    write_ledger(args, ing, rows, c, tin, tout, total, ir, orr, tsaves)
    print("\n" + "=" * 66)
    print(f"model {args.model}  {tsaves} saves during ingest  COST ${total:.4f}  ({tin} in + {tout} out tok)")
    for v in ("PASS", "REASONING_MISS", "RETRIEVAL_MISS", "EXTRACTION_MISS", "LEAK"):
        if c.get(v):
            print(f"  {v:<16} {c[v]}/{len(rows)}")
    print(f"\nledger: {args.out}")


def write_ledger(args, ing, rows, c, tin, tout, total, ir, orr, tsaves):
    n = len(rows)
    L = ["# Legend — agentic full-loop ledger", "",
         f"**Model** `{args.model}` · **{tsaves}** save-calls · **{len(ing)}** episodes · **{n}** questions  ",
         f"**Cost ${total:.4f}** ({tin:,} in + {tout:,} out tok @ {ir:g}/{orr:g} $/MTok — *verify pricing*)", "",
         "The model is handed raw dev-notes per episode and must drive Legend through `recall`/`save` tools to keep",
         "memory correct. Each episode below shows what it saw, every tool call it made (with latency), and the",
         "hypergraph after. The query phase then asks competency questions against the memory it built.", "",
         "## Attribution", "", "| outcome | count |", "|---|---|",
         f"| ✅ PASS | {c.get('PASS',0)}/{n} |",
         f"| 🧠 REASONING_MISS (frame had it, model missed) | {c.get('REASONING_MISS',0)}/{n} |",
         f"| 🔍 RETRIEVAL_MISS (stored, not surfaced) | {c.get('RETRIEVAL_MISS',0)}/{n} |",
         f"| ✍️ EXTRACTION_MISS (never stored) | {c.get('EXTRACTION_MISS',0)}/{n} |",
         f"| ⚠️ LEAK (retracted resurfaced) | {c.get('LEAK',0)}/{n} |", "",
         "## Performance per ingest step", "",
         "| episode | saves | tok in | tok out | model ms | tool ms | dump ms | elems | rels |",
         "|---|---|---|---|---|---|---|---|---|"]
    for r in ing:
        api_ms = sum(rd["api_ms"] for rd in r["rounds"])
        tool_ms = sum(t["ms"] for rd in r["rounds"] for t in rd["tools"])
        L.append(f"| {r['episode']} | {r['saves']} | {r['in']} | {r['out']} | {api_ms:.0f} | {tool_ms:.0f} "
                 f"| {r['dump_ms']:.0f} | {r['n_elements']} | {r['n_relations']} |")
    L += ["", "## Ingest phase (per episode)", ""]
    for r in ing:
        L += [f"### {r['episode']} — {r['saves']} saves → {r['n_elements']} elems / {r['n_relations']} rels", "",
              "<details><summary>raw dev-notes the model saw</summary>", "", "```", r["prose"], "```", "</details>", ""]
        for rd in r["rounds"]:
            if rd["note"]:
                L.append(f"> 💭 {rd['note'][:400]}")
            for t in rd["tools"]:
                if t["name"] == "recall":
                    L += [f"**recall** `{t['input'].get('focus', [])}` · {t['ms']:.0f} ms",
                          "<details><summary>frame returned</summary>", "", "```", t["output"][:2500], "```", "</details>", ""]
                else:
                    L += [f"**save** · {t['ms']:.0f} ms → _{t['output']}_", "```json",
                          json.dumps(t["input"], indent=2), "```", ""]
        L += ["<details><summary>🗺️ hypergraph after this episode</summary>", "", "```json",
              r["dump"][:6000], "```", "</details>", "", "---", ""]
    L += ["## Query phase", ""]
    for i, r in enumerate(rows):
        L += [f"### Q{i} · {r['comp']} · `{r['cat']}` — **{r['verdict']}**  · recall {r['rec_ms']:.0f} ms + answer {r['ans_ms']:.0f} ms",
              f"- 🧑 **{r['q']}**", f"- 🔎 focus: `{r['payload'].get('focus')}`",
              f"- 🤖 {r['answer']}", f"- 🎯 gold: `{r['gold']}`", "",
              "<details><summary>recall frame the answerer saw</summary>", "", "```", r["frame"][:2500], "```", "</details>", ""]
    Path(args.out).write_text("\n".join(L))


if __name__ == "__main__":
    main()
