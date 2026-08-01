#!/usr/bin/env python3
"""Gauges for the 2026-08-01 save-path bundle (docs/fix-roster-2026-08-01.md).

round_report.py covers activity, the prose backstop, audit health and ambient
recall. These are the five this bundle is supposed to move, and they need the
NEW binary because two of them read the audit's `prose` block.

READ-ONLY: copies the store to a temp dir and reads the copy. Never writes the
live trial store.

    python3 harness/bundle_gauges.py [store-dir] [--binary PATH]
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections import Counter, defaultdict

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def run(binary, cwd, *args):
    r = subprocess.run([binary, *args], cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"{args[0]} failed: {r.stderr[:400]}")
    return json.loads(r.stdout)


def main():
    argv = [a for a in sys.argv[1:] if not a.startswith("--")]
    binary = os.path.join(REPO, "legend")
    if "--binary" in sys.argv:
        binary = sys.argv[sys.argv.index("--binary") + 1]
    store = argv[0] if argv else os.path.expanduser("~/Code/alchamancer2/.legend")
    store = os.path.abspath(store)

    tmp = tempfile.mkdtemp(prefix="legend_gauges_")
    try:
        shutil.copytree(store, os.path.join(tmp, ".legend"))
        dump = run(binary, tmp, "dump")
        audit = run(binary, tmp, "audit", '{"limit":1}')
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    els = dump["elements"]
    rels = [r for r in dump["relations"] if r.get("status") == "asserted"]
    print(f"BUNDLE GAUGES — {store}")
    print(f"  clock={dump['clock']} elements={len(els)} live_relations={len(rels)}")

    # 1. kind clobber (e6973ae): a kind is a one-word noun; anything longer is a
    #    claim, and accepting one supersedes the element's real kind.
    bad = [(e["ref"], e["name"], e["kind"])
           for e in els if len(str(e.get("kind", "")).split()) > 2]
    print(f"\n[1] clobbered kinds (>2 words)      : {len(bad)}   TARGET 0")
    for ref, name, kind in bad[:6]:
        print(f"      {ref} {name!r} -> kind {kind!r}")

    # 3. current_* written as a plain fact (f76d3e6). The post-hoc symptom is more
    #    than one LIVE cache relation for the same (subject, current_<prop>).
    cur = defaultdict(list)
    for r in rels:
        a = r["attrs"]
        subj = a.get("subject")
        for k, v in a.items():
            if k != "subject" and str(k).startswith("current_"):
                cur[(subj, k)].append((r["ref"], v))
    dup = {k: v for k, v in cur.items() if len(v) > 1}
    print(f"\n[3] duplicate live current_* caches : {len(dup)}   TARGET 0")
    for (subj, prop), vals in list(dup.items())[:6]:
        print(f"      {subj!r}.{prop} = {[v for _, v in vals]}")

    # 2. multi-valued (subject, predicate) groups: what the old pin-25 heal would
    #    have wiped wholesale on a single `changes` (bbc9c8c).
    groups = defaultdict(set)
    for r in rels:
        a = r["attrs"]
        subj = a.get("subject")
        if subj is None or len(a) != 2:
            continue
        for k, v in a.items():
            if k != "subject":
                groups[(subj, k)].add(str(v))
    multi = {k: v for k, v in groups.items() if len(v) > 1}
    exposed = sum(len(v) for v in multi.values())
    print(f"\n[2] multi-valued (subj,pred) groups : {len(multi)} "
          f"covering {exposed} facts   (exposure, not a defect)")
    for (subj, pred), vals in sorted(multi.items(), key=lambda kv: -len(kv[1]))[:5]:
        print(f"      {subj!r} {pred} x{len(vals)}")

    # 4. predicate sprawl (trial Watch #1) — the cause of flat_decision's false
    #    positives; unchanged by this bundle, tracked so it stays visible.
    preds = Counter()
    for r in rels:
        for k in r["attrs"]:
            if k != "subject":
                preds[k] += 1
    single = [p for p, c in preds.items() if c == 1]
    print(f"\n[4] distinct predicates             : {len(preds)}  "
          f"single-use {len(single)}   (watch, not a target)")

    # 5. prose distribution (ade943b) — read this instead of the bloat count.
    prose = audit.get("prose", {})
    print(f"\n[5] prose   {prose}")
    print(f"    bloat count {audit['counts'].get('bloat')} "
          f"(size signal, NOT health — see docs/cli.md)")
    print(f"\n    audit: {json.dumps(audit['counts'])}")


if __name__ == "__main__":
    main()
