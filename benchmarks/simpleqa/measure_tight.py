#!/usr/bin/env python3
"""Compare the tight store (store_tight/) vs the over-extracted store (store/) on
the 13 D-only miss questions: did tightening keep the answer facts, do they now
surface in recall, and is recall faster?
"""
import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import report  # noqa: E402
from common.util import d, read_jsonl  # noqa: E402

MISSES = [447, 820, 934, 1016, 1334, 2790, 2932, 2952, 3358, 3573, 3699, 3716, 4042]
MD = "/home/nickthorpe71/.local/share/legend/bge-small-en-v1.5"
BIN = "/home/nickthorpe71/Code/legend/legend"


def env(store, embed=1):
    return dict(os.environ, LEGEND_STATE_DIR=store, LEGEND_EMBED_DIR=MD,
                LEGEND_EMBED=str(embed), LEGEND_NOW="1720000000")


def recall(store, args):
    t = time.time()
    try:
        p = subprocess.run([BIN, "recall", json.dumps(args)], capture_output=True,
                           text=True, env=env(store), timeout=90)
        return p.stdout, time.time() - t
    except subprocess.TimeoutExpired:
        return "", 90.0


def dump_text(store):
    return subprocess.run([BIN, "dump"], capture_output=True, text=True,
                          env=env(store, embed=0)).stdout


def main():
    gold = {q["qid"]: q["answer"] for q in read_jsonl(d("corpus", "questions_supported.jsonl"))}
    rows = [json.loads(l) for l in open(d("runs", "arms", "answers.jsonl"))]
    focus = {}
    for r in rows:
        if r["arm"] == "B":
            focus[r["qid"]] = [f["args"] for f in r.get("frames", []) if "args" in f] \
                or [{"focus": [r["problem"]], "limit": 10}]

    TIGHT = os.path.abspath("store_tight/.legend")
    ORIG = os.path.abspath("store/.legend")
    tdump = dump_text(TIGHT)
    tstore_n = len(json.loads(tdump).get("elements", []))
    print(f"tight store elements: {tstore_n}\n")

    hdr = f"{'qid':>5} {'gold':<26} {'t_store':>7} {'t_frame':>7} {'t_ms':>6}"
    print(hdr)
    ts = tf = 0
    lat = []
    for qid in MISSES:
        g = gold[qid]
        in_store = report.grounded(g, tdump)
        in_frame = False
        qlat = 0.0
        for a in focus[qid]:
            out, dt = recall(TIGHT, a)
            qlat += dt
            if report.grounded(g, out):
                in_frame = True
        ts += in_store
        tf += in_frame
        lat.append(qlat)
        print(f"{qid:>5} {g[:26]!r:<28} {str(in_store):>7} {str(in_frame):>7} {int(qlat*1000):>6}")
    print(f"\nTIGHT store: gold-in-store {ts}/13, gold-in-frame {tf}/13, "
          f"median recall {int(sorted(lat)[len(lat)//2]*1000)}ms")
    print("(over-extracted store: gold-in-frame was measured lower; the polluted "
          "Pluto/Yama recall timed out at 40s+)")


if __name__ == "__main__":
    main()
