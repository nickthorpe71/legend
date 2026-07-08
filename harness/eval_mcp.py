#!/usr/bin/env python3
"""Episodic eval through the REAL production stack.

Each episode is a fresh headless `claude -p` session that connects to a live
`legend mcp-serve` over MCP and drives the tools itself. Chat context does NOT
carry across episodes — the only continuity is Legend's memory — so this tests
exactly the cross-session scenario Legend exists for. After ingest, probe
questions are answered from cold recall (each its own fresh session) and scored.
Produces a reviewable markdown ledger.

  python3 harness/eval_mcp.py [--model claude-sonnet-5] [--episodes 5]
                              [--slice smoke] [--max-probes N] [--out FILE]

Reuses render_episode / derive_questions / check / diagnose from eval_agentic.
"""

import argparse
import contextlib
import glob
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

_here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, _here)
import eval_agentic as ea  # noqa: E402

ROOT = ea.ROOT
LEG = ROOT / "legend"
MODELS = ROOT / "models/bge-small-en-v1.5"

# Minimal, production-faithful framing. Tool *semantics* (recall-before-save,
# `changes` to update, reuse canonical names) come from the MCP server's own
# initialize `instructions`; here we only set the session's task.
INGEST_SYS = (
    "You maintain long-term memory across separate work sessions using the Legend tools. "
    "You have no memory of prior sessions except what Legend holds. Given today's developer "
    "notes: first RECALL the entities they mention to find existing canonical names and current "
    "values, then record new facts, decisions, renames, retractions, and changed values through "
    "the tools — reuse canonical names and update a value with a change rather than duplicating it."
)
ANSWER_SYS = (
    "Answer using ONLY your Legend memory. Recall what you need first, then reply with just the "
    "value(s) — no prose. If memory does not contain it, reply exactly NOT_IN_CONTEXT."
)
ALLOWED = ["mcp__legend__legend_recall", "mcp__legend__legend_save"]


def write_mcp_config(path, store, now):
    path.write_text(
        json.dumps(
            {
                "mcpServers": {
                    "legend": {
                        "command": str(LEG),
                        "args": ["mcp-serve"],
                        "env": {
                            "LEGEND_STATE_DIR": str(store),
                            "LEGEND_EMBED_DIR": str(MODELS),
                            "LEGEND_NOW": str(now),
                        },
                    }
                }
            }
        )
    )


def run_claude(prompt, cfg, model, append_sys, cwd, timeout=360):
    """One headless claude session over the MCP server. Parses the stream."""
    cmd = [
        "claude",
        "-p",
        prompt,
        "--mcp-config",
        str(cfg),
        "--strict-mcp-config",
        "--allowedTools",
        *ALLOWED,
        "--dangerously-skip-permissions",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        model,
        "--append-system-prompt",
        append_sys,
    ]
    t0 = time.perf_counter()
    try:
        r = subprocess.run(
            cmd,
            cwd=str(cwd),
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        out, err, rc = r.stdout, r.stderr, r.returncode
    except subprocess.TimeoutExpired as e:
        out, err, rc = (e.stdout or ""), "TIMEOUT", 124
    ms = (time.perf_counter() - t0) * 1000
    calls, final, usage, cost = [], "", {}, 0.0
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        if ev.get("type") == "assistant":
            for b in ev.get("message", {}).get("content", []):
                if b.get("type") == "tool_use" and b.get("name", "").startswith(
                    "mcp__legend__"
                ):
                    calls.append(
                        {"tool": b["name"].split("__")[-1], "input": b.get("input", {})}
                    )
        elif ev.get("type") == "result":
            final = ev.get("result", "") or ""
            usage = ev.get("usage", {}) or {}
            cost = ev.get("total_cost_usd", 0.0) or 0.0
    return dict(
        calls=calls, final=final.strip(), usage=usage, cost=cost, ms=ms, rc=rc, err=err
    )


def cli(store, verb, payload=None):
    env = dict(os.environ, LEGEND_STATE_DIR=str(store), LEGEND_EMBED_DIR=str(MODELS))
    r = subprocess.run(
        [str(LEG), verb] + ([] if payload is None else [json.dumps(payload)]),
        capture_output=True,
        text=True,
        env=env,
        cwd=ROOT,
    )
    return r.stdout


def dump_graph(store):
    try:
        return json.loads(cli(store, "dump"))
    except json.JSONDecodeError:
        return {"elements": [], "relations": []}


def load_episodes():
    return [
        json.load(open(f))
        for f in sorted(glob.glob(str(ROOT / "harness/corpus/episodes/*.json")))
    ]


def episode_now(ep):
    return min(st["now"] for st in ep["steps"])


def record_saves(ep_name, saves):
    """Feed eval_agentic.SAVE_LOG so diagnose() can compare vs gold. A `change`
    is folded into a synthetic (target, property)->to fact so supersession
    counts as the current value."""
    el = [e for c in saves for e in c["input"].get("elements", [])]
    fa = [f for c in saves for f in c["input"].get("facts", [])]
    for c in saves:
        for ch in c["input"].get("changes", []):
            fa.append(
                {
                    "s": ch.get("target", ""),
                    "p": ch.get("property", ""),
                    "o": ch.get("to", ""),
                }
            )
    ea.SAVE_LOG.append({"episode": ep_name, "elements": el, "facts": fa})


def j(x):
    return json.dumps(x, indent=2, ensure_ascii=False)


def write_ledger(path, args, eps_all, ing, rows, wall):
    n_ep, slice_total = (
        len(ing),
        sum(1 for e in eps_all if e.get("slice") == args.slice),
    )
    tcost = sum(r["res"]["cost"] for r in ing) + sum(r["res"]["cost"] for r in rows)
    passed = sum(1 for r in rows if r["ok"])
    diag = io.StringIO()
    with contextlib.redirect_stdout(diag):
        ea.diagnose([e for e in eps_all if e["episode"] in {i["ep"] for i in ing}])

    L = [
        f"# MCP episodic eval — `{args.slice}` slice",
        "",
        f"**Model** `{args.model}` · **{n_ep}** episodes ingested (of {slice_total} in slice) · "
        f"**{len(rows)}** probes · **${tcost:.2f}** · **{wall:.0f}s** wall",
        "",
        "Each episode is a *separate* headless `claude -p` session with no chat memory of the "
        "others — the only continuity is Legend, driven live over MCP. Probes are answered from "
        "cold recall in their own fresh sessions.",
        "",
        "## Ingest summary",
        "",
        "| episode | recalls | saves | elems | rels | $ | ms | rc |",
        "|---|--:|--:|--:|--:|--:|--:|--:|",
    ]
    for r in ing:
        L.append(
            f"| {r['ep']} | {r['n_recall']} | {r['n_save']} | {r['n_el']} | {r['n_rel']} "
            f"| {r['res']['cost']:.3f} | {r['res']['ms']:.0f} | {r['res']['rc']} |"
        )

    L += [
        "",
        "## Probe results",
        "",
        f"**{passed}/{len(rows)} passed**"
        + (
            ""
            if n_ep >= slice_total
            else f"  ⚠️ only {n_ep}/{slice_total} episodes ingested — probes needing later episodes will miss"
        ),
        "",
        "| ✓ | category | question | model answer | gold |",
        "|:-:|---|---|---|---|",
    ]
    for r in rows:
        q = r["q"]
        mark = "✅" if r["ok"] else "❌"
        ans = (r["res"]["final"] or "∅").replace("|", "\\|").replace("\n", " ")[:70]
        gold = str(q["gold"]).replace("|", "\\|")[:40]
        L.append(
            f"| {mark} | {q['cat']} | {q['q'][:64].replace('|', '\\|')} | {ans} | {gold} |"
        )

    L += [
        "",
        "## Volume vs form (LLM saves vs gold)",
        "",
        "```",
        diag.getvalue().strip(),
        "```",
    ]

    L += ["", "## Per-episode detail", ""]
    for r in ing:
        res = r["res"]
        L += [
            f"### {r['ep']} — {r['n_recall']} recalls / {r['n_save']} saves "
            f"→ {r['n_el']} elems / {r['n_rel']} rels",
            "",
            "<details><summary>dev notes the model saw</summary>",
            "",
            "```",
            r["prose"],
            "```",
            "</details>",
            "",
            "<details><summary>tool calls (recall focuses + save payloads)</summary>",
            "",
        ]
        for c in res["calls"]:
            if c["tool"] == "legend_recall":
                L.append(
                    f"- **recall** `{json.dumps(c['input'].get('focus', c['input']))}`"
                )
            else:
                L += [f"- **save**:", "```json", j(c["input"]), "```"]
        L += [
            "</details>",
            "",
            f"**model said:** {res['final'][:400] or '(no text)'}",
            "",
            "<details><summary>hypergraph after this episode</summary>",
            "",
            "```json",
            j(r["graph"]),
            "```",
            "</details>",
            "",
        ]

    path.write_text("\n".join(L))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", default="claude-sonnet-5")
    ap.add_argument("--slice", default="smoke")
    ap.add_argument("--episodes", type=int, default=5)
    ap.add_argument("--max-probes", type=int, default=0, help="0 = all")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    subprocess.run(
        ["gcc", "-std=c99", "-O2", "legend.c", "embed.c", "-o", "legend", "-lm"],
        cwd=ROOT,
        check=True,
    )
    tmp = Path(tempfile.mkdtemp(prefix="legend-mcpeval-"))
    store, cwd, cfg = tmp / "store", tmp / "cwd", tmp / "mcp.json"
    cwd.mkdir()
    cli(store, "init")

    eps_all = load_episodes()
    eps = eps_all[: args.episodes]  # e01..eN of the shared timeline
    t_start = time.perf_counter()

    print(f"== ingest {len(eps)} episodes (model {args.model}) ==")
    ing = []
    for ep in eps:
        write_mcp_config(cfg, store, episode_now(ep))
        prose = ea.render_episode(ep)
        prompt = f"Developer notes for this session:\n\n{prose}\n\nUpdate your memory to reflect them."
        res = run_claude(prompt, cfg, args.model, INGEST_SYS, cwd)
        graph = dump_graph(store)
        saves = [c for c in res["calls"] if c["tool"] == "legend_save"]
        record_saves(ep["episode"], saves)
        ing.append(
            dict(
                ep=ep["episode"],
                prose=prose,
                res=res,
                graph=graph,
                n_el=len(graph["elements"]),
                n_rel=len(graph["relations"]),
                n_recall=sum(1 for c in res["calls"] if c["tool"] == "legend_recall"),
                n_save=len(saves),
            )
        )
        print(
            f"  {ep['episode']:<22} recalls={ing[-1]['n_recall']} saves={ing[-1]['n_save']} "
            f"el={ing[-1]['n_el']} rel={ing[-1]['n_rel']} ${res['cost']:.3f} {res['ms']:.0f}ms rc={res['rc']}"
        )

    qs = ea.derive_questions(args.slice)
    if args.max_probes:
        qs = qs[: args.max_probes]
    probe_now = (max(episode_now(e) for e in eps) + 3600) if eps else 0
    write_mcp_config(cfg, store, probe_now)
    print(f"== {len(qs)} probes ==")
    rows = []
    for q in qs:
        res = run_claude(q["q"], cfg, args.model, ANSWER_SYS, cwd)
        ok = ea.check(q["mode"], q["gold"], res["final"])
        rows.append(dict(q=q, res=res, ok=ok))
        print(
            f"  [{'PASS' if ok else 'FAIL'}] {q['cat']:<14} {q['q'][:56]:<56} -> {res['final'][:44]!r}"
        )

    wall = time.perf_counter() - t_start
    out = Path(args.out) if args.out else ROOT / f"mcp_ledger_{args.slice}.md"
    write_ledger(out, args, eps_all, ing, rows, wall)
    tcost = sum(r["res"]["cost"] for r in ing) + sum(r["res"]["cost"] for r in rows)
    print(
        f"\n== {sum(1 for r in rows if r['ok'])}/{len(rows)} probes passed · "
        f"${tcost:.2f} · {wall:.0f}s ==\nledger: {out}"
    )
    shutil.rmtree(tmp)


if __name__ == "__main__":
    main()
