#!/usr/bin/env python3
"""Round report for the alchamancer trial — one command per round close, so no gauge
is forgotten. Given a build sha (default: the latest build in the journal), compute
every relevant metric for that round from the journal + a read-only copy of the store.

Usage:
    python3 harness/round_report.py [build_sha]

Reads ~/Code/alchamancer2/.legend (journal + store), copies the store to a tempdir so
the live one is never touched, and runs the PINNED binary (~/.local/bin/legend) against
the copy. Determinism is a SEPARATE check: python3 harness/replay_journal.py <store>.
"""
import json, os, sys, subprocess, shutil, tempfile

LIVE = os.path.expanduser("~/Code/alchamancer2/.legend")
LEGEND = os.path.expanduser("~/.local/bin/legend")
EMBED_DIR = os.path.expanduser("~/.local/share/legend/bge-small-en-v1.5")
PROSE = 120  # g_aud_name_chars: a value/name past this is prose


def load_journal():
    out = []
    for line in open(f"{LIVE}/journal.jsonl"):
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return out


def legend(store, verb, payload=None, embed=True):
    env = dict(os.environ, LEGEND_STATE_DIR=store)
    if embed:
        env["LEGEND_EMBED_DIR"] = EMBED_DIR
    else:
        env["LEGEND_EMBED"] = "0"
    args = [LEGEND, verb] + ([payload] if payload else [])
    p = subprocess.run(args, capture_output=True, text=True, env=env, timeout=120)
    try:
        return json.loads(p.stdout)
    except json.JSONDecodeError:
        return {}


def section(t):
    print(f"\n== {t} ==")


def main():
    jr = load_journal()
    builds = []
    for d in jr:
        b = d.get("build")
        if b and b not in builds:
            builds.append(b)
    target = sys.argv[1] if len(sys.argv) > 1 else (builds[-1] if builds else None)
    if not target:
        print("no journal / builds found")
        return
    seg = [d for d in jr if d.get("build") == target]
    ts = [d.get("ts", 0) for d in seg if d.get("ts")]

    print(f"ROUND REPORT — build {target}")
    print(f"  segment: {len(seg)} invocations"
          + (f", ts {min(ts)}..{max(ts)}" if ts else ""))

    # ---- activity ----
    section("Activity")
    saves = [d for d in seg if d.get("verb") == "save"]
    recs = [d for d in seg if d.get("verb") == "recall"]
    rej = [d for d in seg if not d.get("ok")]
    print(f"  saves={len(saves)}  recalls={len(recs)}  rejections={len(rej)} "
          + (str([d.get("code") for d in rej]) if rej else ""))
    opcount = {}
    for d in saves:
        if not d.get("ok"):
            continue
        try:
            p = json.loads(d["payload"])
        except Exception:
            continue
        for k in ("elements", "facts", "changes", "retract", "merge", "templates"):
            if p.get(k):
                opcount[k] = opcount.get(k, 0) + len(p[k])
    print(f"  save ops: {opcount or '(none)'}")

    # ---- prose backstop ----
    section("Prose backstop (changes.to)")
    prose_rej = [d for d in rej if (d.get("code") or "") == "prose_value"]
    chg_to, slipped = [], 0
    for d in saves:
        if not d.get("ok"):
            continue
        try:
            p = json.loads(d["payload"])
        except Exception:
            continue
        for c in p.get("changes", []):
            t = c.get("to", "")
            if isinstance(t, str):
                chg_to.append(len(t))
                if len(t) > PROSE:
                    slipped += 1
    print(f"  changes.to ops: {len(chg_to)}"
          + (f"  (max len {max(chg_to)})" if chg_to else "")
          + f"  | prose SLIPPED THROUGH (>{PROSE}): {slipped}")
    print(f"  prose_value rejections: {len(prose_rej)}")
    if prose_rej:
        # recovery: did a save by the same session shortly after succeed?
        idx = {id(d): i for i, d in enumerate(jr)}
        for d in prose_rej:
            print(f"    rejected at ts={d.get('ts')} — check the next save recovered "
                  f"(short value + summary)")
    elif len(chg_to) == 0:
        print("  -> backstop UNEXERCISED this round (no changes ops)")

    # ---- reroute watch ----
    section("Reroute watch (does blocking changes.to just move prose?)")
    facto = 0
    for d in saves:
        if not d.get("ok"):
            continue
        try:
            p = json.loads(d["payload"])
        except Exception:
            continue
        for f in p.get("facts", []):
            o = f.get("o", "")
            if isinstance(o, str) and len(o) > PROSE:
                facto += 1
    print(f"  fact.o prose (>{PROSE}) added this round: {facto}")

    # ---- store snapshot (audit + packet + invariants) on a copy ----
    section("Store snapshot (audit / packet / invariants)")
    tmp = tempfile.mkdtemp(prefix="round_report_")
    store = os.path.join(tmp, ".legend")
    shutil.copytree(LIVE, store)
    try:
        os.remove(os.path.join(store, "legend.lock"))
    except OSError:
        pass
    try:
        aud = legend(store, "audit", embed=False)
        print(f"  clock={aud.get('clock')} elements={aud.get('elements')}")
        print(f"  audit counts: {json.dumps(aud.get('counts', {}))}")
        pk = subprocess.run([LEGEND, "recall", '{"limit":16}', "--pretty"],
                            capture_output=True, env=dict(os.environ,
                            LEGEND_STATE_DIR=store, LEGEND_EMBED="0"), timeout=120)
        print(f"  packet (hook's limit:16): {len(pk.stdout[:20000])} bytes "
              f"(cap 20000)")
        dump = legend(store, "dump", embed=False)
        cw = sum(1 for r in dump.get("relations", [])
                 for k in r.get("attrs", {}) if k == "correlated_with")
        print(f"  correlated_with facts: {cw}  (invariant: stays 0)")

        # ---- ambient recall quality: replay the round's ambient queries ----
        section("Ambient recall quality (replay — final-store proxy)")
        amb_q = []
        for d in seg:
            if d.get("verb") != "recall" or not d.get("observe"):
                continue
            try:
                p = json.loads(d["payload"])
            except Exception:
                continue
            f = p.get("focus")
            if f and isinstance(f, list) and f:
                amb_q.append(f[0])
        if not amb_q:
            print("  (no ambient recall queries in this segment)")
        else:
            surfaced, examples = 0, []
            for q in amb_q:
                d = legend(store, "recall", json.dumps({"focus": [q],
                           "observe": True}))
                res = (d.get("resolution") or [{}])[0]
                cs = res.get("candidates", [])
                if cs:
                    surfaced += 1
                if len(examples) < 6:
                    top = (cs[0].get("name", "?")[:26], round(cs[0].get("score", 0), 2)) if cs else None
                    examples.append((q[:34], top))
            print(f"  ambient queries: {len(amb_q)}  surfaced candidates: "
                  f"{surfaced}  abstained: {len(amb_q) - surfaced}  "
                  f"({100*surfaced//max(1,len(amb_q))}% surfaced)")
            print("  too-quiet -> lower TIER2_AMBIENT_SEMANTIC_MIN; clutter -> raise")
            for q, top in examples:
                print(f"    {q:36} -> {top or 'ABSTAIN'}")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    section("Determinism (run separately)")
    print(f"  python3 harness/replay_journal.py {LIVE}")


if __name__ == "__main__":
    main()
