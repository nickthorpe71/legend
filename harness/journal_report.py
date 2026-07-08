#!/usr/bin/env python3
"""Health report over a deployed Legend store: the journal (what happened)
plus the graph dump (what it produced). The weekly trial read-out.

  python3 harness/journal_report.py <store-dir> [--legend BIN]

Reads only; the one legend invocation is `dump` (no store mutation, no
journal line). Session boundaries = gaps > 30 min between journal lines.
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, collections, datetime, json, subprocess

p = argparse.ArgumentParser()
p.add_argument("store")
p.add_argument("--legend", default=os.path.expanduser("~/.local/bin/legend"))
args = p.parse_args()

lines = [json.loads(l) for l in open(os.path.join(args.store, "journal.jsonl"))]
fmt = lambda t: datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%m-%d %H:%M")

# ---- sessions: split on >30 min gaps ----
sessions, cur = [], [lines[0]]
for e in lines[1:]:
    if e["ts"] - cur[-1]["ts"] > 1800:
        sessions.append(cur)
        cur = []
    cur.append(e)
sessions.append(cur)

def classify(e):
    if e["verb"] == "init":
        return "init"
    if not e["ok"]:
        return "rejected"
    if e["verb"] == "save":
        return "save"
    pay = e.get("payload") or ""
    if e.get("observe"):
        return "ambient" if '"focus"' in pay else "observe"
    return "boot" if pay == "{}" else "recall"

print("== sessions (gap > 30 min) ==")
tot = collections.Counter()
for i, s in enumerate(sessions):
    c = collections.Counter(classify(e) for e in s)
    tot += c
    span = (s[-1]["ts"] - s[0]["ts"]) / 60
    builds = ",".join(sorted({e["build"] for e in s}))
    save_b = sum(len(e.get("payload") or "") for e in s if classify(e) == "save")
    print(f"  {i+1:2}. {fmt(s[0]['ts'])}  {span:5.0f} min  {builds:8} "
          f"boot:{c['boot']} ambient:{c['ambient']} recall:{c['recall']} "
          f"save:{c['save']} ({save_b}B) rejected:{c['rejected']}")

print("\n== totals ==")
print("  lines:", len(lines), dict(tot))
print("  builds seen:", ", ".join(sorted({e['build'] for e in lines})))
rej = [e for e in lines if not e["ok"]]
for e in rej:
    print(f"  rejected: {fmt(e['ts'])} {e['verb']} {e['code']}: {(e.get('payload') or '')[:80]}")

# double-fire diagnosis rule (trial watch item): same-second non-observe {} pairs
boots = collections.Counter(e["ts"] for e in lines if classify(e) == "boot")
pairs = [(fmt(ts), n) for ts, n in boots.items() if n > 1]
print("  same-second boot pairs:", pairs or "none")

# ---- graph state ----
env = dict(os.environ, LEGEND_STATE_DIR=args.store)
d = json.loads(subprocess.run([args.legend, "dump"], env=env,
                              capture_output=True, check=True).stdout)
els = [e for e in d["elements"] if "merged_into" not in e]
print("\n== store ==")
print(f"  clock {d['clock']}: {len(els)} live elements, {len(d['relations'])} relations "
      f"({sum(1 for r in d['relations'] if r['status'] in ('superseded','retracted'))} superseded/retracted)")
kinds = collections.Counter(e.get("kind", "(none)") for e in els)
print("  kinds:", dict(kinds.most_common()))
norm = collections.Counter(" ".join("".join(c if c.isalnum() else " " for c in e["name"].lower()).split())
                           for e in els)
dups = {k: v for k, v in norm.items() if v > 1}
print("  duplicate names:", dups or "none")
longnames = [e["name"] for e in els if len(e["name"]) > 120]
print(f"  paragraph-named elements (>120 chars): {len(longnames)}")
nosum = sum(1 for e in els if e.get("kind") in ("system", "mechanic", "decision", "constraint")
            and not e.get("summary"))
print(f"  typed elements missing a summary: {nosum}")
