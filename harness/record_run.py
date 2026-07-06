#!/usr/bin/env python3
"""Record a corpus run as one readable Markdown ledger for the C oracle.

For every step: the LLM's input payload, the frame returned, the wall-clock
time of the call, and a call-out whenever embeddings fired (model load / recall
rank / save sync, read from legend's LEGEND_EMBED_TRACE stderr). The full
hypergraph (via the `dump` verb) is printed every --every steps, not every step.

  python3 harness/record_run.py [--slice smoke] [--every 5] [--out corpus_run.md]

Builds legend.c + embed.c fresh (embeddings on) so the ledger reflects HEAD.
"""
import os, sys
# harness/ contains an inspect.py that shadows stdlib `inspect`; drop the script
# dir from sys.path before importing argparse (3.14 argparse -> _colorize ->
# dataclasses -> inspect.signature would otherwise pick up the wrong module).
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, json, subprocess, tempfile, time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def sh(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def build_legend(dst):
    r = sh(["gcc", "-std=c99", "-Wall", "-Wextra", "-Werror", "-O2",
            "legend.c", "embed.c", "-o", dst, "-lm"], cwd=ROOT)
    if r.returncode != 0:
        sys.exit(f"build failed:\n{r.stderr}")


def num(x):
    """Match the C %g / TS rendering: whole numbers bare, else trimmed float."""
    if isinstance(x, float) and x.is_integer():
        return str(int(x))
    return f"{x:g}" if isinstance(x, (int, float)) else str(x)


def stats(s):
    return (f"conf={num(s['conf'])} sal={num(s['sal'])} act={num(s['act'])} "
            f"stab={num(s['stab'])} acc={s['acc']} fsc={s['fsc']} "
            f"sup={s['sup']}/{s['div']} seen={s['seen']}")


def render_graph(g):
    out = [f"**Clock:** {g['clock']}  |  **Elements:** {len(g['elements'])}  "
           f"|  **Relations:** {len(g['relations'])}", "", "#### Elements"]
    for e in g["elements"]:
        if "merged_into" in e:
            out.append(f"- `{e['ref']}` \"{e['name']}\" → merged into {e['merged_into']}")
            continue
        kind = f" `[{e['kind']}]`" if "kind" in e else ""
        aka = f"  _(aka {', '.join(e['aliases'])})_" if e.get("aliases") else ""
        out.append(f"- `{e['ref']}` **{e['name']}**{kind}{aka} — {stats(e['stats'])}")
        if "summary" in e:
            out.append(f"    - _{e['summary']}_")
    out += ["", "#### Relations"]
    for r in g["relations"]:
        attrs = ", ".join(
            f"{k}: {v}" if str(v).startswith("rel:") else f'{k}: "{v}"'
            for k, v in r["attrs"].items())
        out.append(f"- `{r['ref']}` {{ {attrs} }}  — **{r['status']}** {stats(r['stats'])}")
    return "\n".join(out)


def embed_callout(stderr):
    lines = [l for l in stderr.splitlines() if l.startswith("[embed]")]
    return lines


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--slice", default="smoke",
                    choices=["smoke", "adversarial", "dev", "full"])
    ap.add_argument("--every", type=int, default=5, help="dump the graph every N steps")
    ap.add_argument("--out", default=str(ROOT / "corpus_run.md"))
    ap.add_argument("--start", type=int, default=0)
    ap.add_argument("--end", type=int, default=10**9)
    args = ap.parse_args()

    tmp = Path(tempfile.mkdtemp(prefix="legend-record-"))
    legend = str(tmp / "legend")
    build_legend(legend)
    corpus = tmp / "corpus.jsonl"
    r = sh([sys.executable, "-P", "harness/gen_corpus.py", "--slice", args.slice, "-o", str(corpus)], cwd=ROOT)
    if r.returncode != 0:
        sys.exit(f"gen_corpus failed:\n{r.stderr}")
    steps = [json.loads(l) for l in corpus.read_text().splitlines() if l.strip()]

    store = tmp / "store"
    base_env = dict(os.environ, LEGEND_STATE_DIR=str(store), LEGEND_EMBED_TRACE="1")
    sh([legend, "init"], env=base_env, cwd=ROOT)

    def call(verb, payload=None, now=None):
        env = dict(base_env)
        if now is not None:
            env["LEGEND_NOW"] = str(now)
        t = time.monotonic()
        r = subprocess.run([legend, verb], input=(json.dumps(payload) if payload is not None else ""),
                           capture_output=True, text=True, env=env, cwd=ROOT)
        return (time.monotonic() - t) * 1000, r

    def dump():
        _, r = call("dump")
        return json.loads(r.stdout)

    index = ["| step | verb | now | elem | rel | ms | embeddings |",
             "|---|---|---|---|---|---|---|"]
    bodies = []
    total_ms = 0.0
    embed_ms, plain_ms = [], []
    lo, hi = args.start, min(args.end, len(steps))
    for i in range(lo, hi):
        s = steps[i]
        ms, r = call(s["verb"], s["payload"], s.get("now"))
        total_ms += ms
        try:
            frame = json.loads(r.stdout)
        except json.JSONDecodeError:
            frame = {"PARSE_ERROR": r.stdout, "stderr": r.stderr}
        g = dump()  # authoritative counts (save frames report writes, not totals)
        ne, nr = len(g["elements"]), len(g["relations"])
        callouts = embed_callout(r.stderr)
        (embed_ms if callouts else plain_ms).append(ms)
        tag = "🔶 " + "; ".join(c.replace("[embed] ", "") for c in callouts) if callouts else "—"
        index.append(f"| [{i}](#step-{i}) | {s['verb']} | {s.get('now','')} | {ne} | {nr} | {ms:.1f} | {tag} |")

        block = [f'<a id="step-{i}"></a>',
                 f"## Step {i} — {s['verb']}  (now={s.get('now','')})  ·  **{ms:.1f} ms**", ""]
        if callouts:
            block += ["> **🔶 embeddings fired this step:**", ""] + [f"> - `{c}`" for c in callouts] + [""]
        block += ["### → Input (payload from the LLM)", "```json",
                  json.dumps(s["payload"], indent=2, ensure_ascii=False), "```", "",
                  "### ← Output (frame returned to the LLM)", "```json",
                  json.dumps(frame, indent=2, ensure_ascii=False), "```", ""]
        if (i - lo) % args.every == 0 or i == hi - 1:
            block += [f"### Hypergraph state after step {i}", render_graph(g), ""]
        block.append("---")
        bodies.append("\n".join(block))

    policy_note = ("Built from HEAD (`legend.c` + `embed.c`, embeddings **on**). "
                   f"Fresh store, `{args.slice}` slice replayed step by step. "
                   f"Full hypergraph every {args.every} steps.")
    doc = "\n".join([
        "# Legend corpus run — C oracle ledger", "",
        policy_note, "",
        "## Summary",
        f"- **{hi - lo} steps**, total call time **{total_ms/1000:.2f} s**",
        f"- **{len(plain_ms)} steps without embeddings**: median "
        f"**{sorted(plain_ms)[len(plain_ms)//2] if plain_ms else 0:.1f} ms** "
        f"(recall hits / cached saves — no model load)",
        f"- **{len(embed_ms)} steps that embedded** (🔶): median "
        f"**{sorted(embed_ms)[len(embed_ms)//2] if embed_ms else 0:.0f} ms**, "
        f"max **{max(embed_ms) if embed_ms else 0:.0f} ms** "
        f"(23 MB model load + ~one forward pass per new element)",
        "- Timing is full end-to-end per CLI call (spawn + snapshot load + tick + embed). "
        "The embedder is single-threaded (AVX2+FMA when available), so **absolute ms is "
        "machine-/load-dependent**; the recall-vs-embedding-save *ratio* is the stable signal.",
        "", "## Index", *index, "", *bodies,
    ])
    Path(args.out).write_text(doc)
    print(f"wrote {args.out}  ({doc.count(chr(10))+1} lines, {len(doc)/1024:.0f} KiB)")
    print(f"{hi-lo} steps · {total_ms/1000:.2f}s · {len(embed_ms)} embed-steps · store {store}")


if __name__ == "__main__":
    main()
