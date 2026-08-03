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
import re
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
    args = sys.argv[1:]
    binary = os.path.join(REPO, "legend")
    if "--binary" in args:
        i = args.index("--binary")
        binary = args[i + 1]
        del args[i:i + 2]  # its VALUE is not a positional store path
    # the store copy is read with cwd=tmp, so a relative binary would not resolve
    binary = os.path.abspath(os.path.expanduser(binary))
    argv = [a for a in args if not a.startswith("--")]
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

    # 6. the self anchor (docs/self-node.md). Three independent readings, because
    #    the failure modes pull in opposite directions: a dead node (nobody writes
    #    themselves in), a mega-hub (everything anchors on `me`), and leftover
    #    unresolved deixis (pronouns still sitting in prose with no referent).
    SELF = {"me", "the assistant", "the agent", "myself"}
    self_rels = [r for r in rels
                 if any(str(v) in SELF for v in r["attrs"].values())]
    subj_of = sum(1 for r in self_rels if str(r["attrs"].get("subject")) in SELF)
    share = 100.0 * len(self_rels) / len(rels) if rels else 0.0
    print(f"\n[6] self-anchored live facts        : {len(self_rels)} "
          f"({share:.1f}% of live)   BOUND <5%")
    print(f"      as subject {subj_of} · as object {len(self_rels) - subj_of}")
    seen = Counter()
    for r in self_rels:
        for k, v in r["attrs"].items():
            if k != "subject" and str(v) in SELF or k == "subject":
                continue
            seen[k] += 1
    for p, c in seen.most_common(5):
        print(f"      {p} x{c}")

    # authorship must NOT anchor on self — source stays the material drawn on
    src_self = sum(1 for r in self_rels
                   for k, v in r["attrs"].items()
                   if k in ("source", "src") and str(v) in SELF)
    print(f"      on source/src (must stay 0)   : {src_self}   TARGET 0")

    # unresolved first person: pronouns in a NAME or SUMMARY with no referent.
    # Quoted source strings are opaque text and are excluded — they are not refs.
    srcobjs = {str(v) for r in dump["relations"]
               for k, v in r.get("attrs", {}).items() if k in ("source", "src")}
    pron = re.compile(r"\b(I|me|my|myself|we|our|us)\b")
    unresolved = [e for e in els
                  if e["name"] not in srcobjs
                  and e["name"] not in SELF
                  and (pron.search(e["name"] or "")
                       or pron.search(e.get("summary") or ""))]
    print(f"\n[7] unresolved first person         : {len(unresolved)}   "
          f"(round 9 baseline 37 at the 2026-08-03 boundary; should FALL)")
    for e in unresolved[:5]:
        print(f"      {e['ref']} {e['name'][:60]!r}")

    # provenance share: the self node must not make this worse (it replaces
    # nothing today — this is the no-regression read, Watch on inflation).
    prov = sum(1 for r in rels
               for k in r["attrs"] if k in ("source", "src"))
    print(f"\n[8] provenance share of live facts  : {prov} "
          f"({100.0 * prov / len(rels) if rels else 0:.1f}%)   must not RISE")

    # 9. nested statements (docs/nested-statements.md). A statement carried as
    #    the CONTENT of another. `subject: rel` is a meta ABOUT a statement, not
    #    nesting; derived_from/supersedes are versioning plumbing. Everything
    #    else pointing at a statement is content.
    PLUMBING = {"derived_from", "supersedes"}
    isrel = lambda v: isinstance(v, str) and re.match(r"^rel:\d+$", str(v))
    byref = {r["ref"]: r for r in dump["relations"]}
    nested = [(r, k, v) for r in rels for k, v in r["attrs"].items()
              if k != "subject" and k not in PLUMBING and isrel(v)]
    print(f"\n[9] nested content statements       : {len(nested)}   "
          f"(baseline 0 at 2026-08-02; rises iff adopted)")
    # the directive shape done right: the inner statement marked not-yet-true
    marked = sum(1 for _, _, v in nested
                 if "non_actual" in (byref.get(str(v), {}).get("modal") or []))
    print(f"      inner marked non_actual       : {marked} of {len(nested)}")
    for r, k, v in nested[:5]:
        print(f"      {r['ref']} {k} -> {v}")

    # 10. wanted-to-nest-but-didn't. A speech act whose object is the agent but
    #     that carries NO content slot is a directive with its content dropped —
    #     the signal that the two-save flow (not the shape) is what blocks
    #     adoption. Distinguishes "models won't nest" from "models can't".
    SPEECH = {"asked", "said", "told", "requested", "wants", "directed",
              "instructed", "decided", "reported", "claimed"}
    stranded = [r for r in rels
                if any(k in SPEECH for k in r["attrs"])
                and not any(k != "subject" and k not in PLUMBING and isrel(v)
                            for k, v in r["attrs"].items())]
    print(f"\n[10] speech acts with no content    : {len(stranded)}   "
          f"(high + [9] low => the two-save flow is the blocker, not the shape)")
    for r in stranded[:5]:
        print(f"      {r['ref']} {json.dumps(r['attrs'])[:88]}")


if __name__ == "__main__":
    main()
