#!/usr/bin/env python3
"""Measure ambient-recall ranking separation on a Legend store — the baseline
the C1 (cosine floor/margin) + C2 (bookkeeping down-weight) retrieval fixes must
beat.

The store's own journal is the query workload: every focus ever recalled,
replayed as observe:true (the passive-hook path) so the numbers reflect how the
graph ranks the real prompts the trial has seen — the exact surface C1/C2 change.
C2 only fires on observe:true, so the measurement MUST use it, not observe:false.

ISOLATION (learned the hard way): the recall path persists on a mutating tick,
and observe:false IS a mutating tick — replaying it against a live store inflates
the clock and reinforces activations, i.e. corrupts the trial. So this script
copies the snapshot + vector sidecar into a throwaway dir and runs every recall
there. observe:true is itself non-mutating (no reinforce/persist), so one copy
serves all recalls with no cross-recall contamination; the copy is discarded.

  python3 harness/retrieval_separation.py <store-dir> [--legend BIN]
      [--embed-dir DIR] [--top-k K] [--floor F] [--json OUT]

For a clean A/B: run the PRE-change binary (e.g. ~/.local/bin/legend) for the
baseline and the post-change ./legend for the result — both observe:true against
the copy. --json emits machine-diffable aggregates; the embedder is forced on.
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, collections, json, shutil, subprocess, tempfile

# Real domain content — game systems, mechanics, entities, design decisions.
# Everything else is ambient-recall noise: process kinds (event/task/question/
# pointer) AND no-kind elements ("(none)": relation predicates + prose-object
# mints), which the harness must count as noise, not domain, or junk sitting at
# the top flatters the ranking. C2 down-weights all of it on ambient recalls.
DOMAIN_KINDS = {"system", "mechanic", "character", "place", "spell", "enemy",
                "parameter", "decision", "constraint", "project", "person",
                "commit", "function", "module", "file"}
def is_noise(kind):
    return kind not in DOMAIN_KINDS

p = argparse.ArgumentParser()
p.add_argument("store")
p.add_argument("--legend", default=os.path.expanduser("~/.local/bin/legend"))
p.add_argument("--embed-dir",
               default=os.path.expanduser("~/.local/share/legend/bge-small-en-v1.5"))
p.add_argument("--top-k", type=int, default=10, help="candidates inspected per focus term")
p.add_argument("--floor", type=float, default=0.5,
               help="cosine floor to report weak-candidate share against (C1 signal)")
p.add_argument("--json", help="write aggregate metrics as JSON to this path")
args = p.parse_args()

# ---- isolate: copy snapshot + vector sidecar into a throwaway store ----
work = tempfile.mkdtemp(prefix="legend-retsep-")
for fn in ("legend.snapshot", "vectors.bin"):
    src = os.path.join(args.store, fn)
    if os.path.exists(src):
        shutil.copyfile(src, os.path.join(work, fn))
env = dict(os.environ, LEGEND_STATE_DIR=work,
           LEGEND_EMBED="1", LEGEND_EMBED_DIR=args.embed_dir)

# ---- ref -> kind map from a single read-only dump ----
dump = json.loads(subprocess.run([args.legend, "dump"], env=dict(env, LEGEND_EMBED="0"),
                                 capture_output=True, check=True).stdout)
ELS = dump["elements"]                      # id == list index (dense, per journal_report)
def kind_of(ref):
    try:
        return ELS[int(ref.lstrip("#"))].get("kind") or "(none)"
    except (ValueError, IndexError):
        return "(unknown)"

# ---- pull focus recalls from the journal, split ambient vs deliberate ----
groups = {"ambient": [], "deliberate": []}  # each item: (focus_tuple)
seen = {"ambient": collections.Counter(), "deliberate": collections.Counter()}
for l in open(os.path.join(args.store, "journal.jsonl")):
    e = json.loads(l)
    if e["verb"] != "recall" or not e.get("ok"):
        continue
    pay = e.get("payload") or ""
    if '"focus"' not in pay:
        continue
    focus = json.loads(pay).get("focus") or []
    g = "ambient" if e.get("observe") else "deliberate"
    seen[g][tuple(focus)] += 1                 # frequency weight
for g in groups:
    groups[g] = list(seen[g])                  # unique focus-sets, measured once each

# ---- replay each unique focus-set read-only, score the ranking ----
def recall(focus):
    r = subprocess.run([args.legend, "recall"],
                       input=json.dumps({"focus": list(focus), "observe": True}).encode(),
                       env=env, capture_output=True)
    if r.returncode != 0:
        return None
    return json.loads(r.stdout)

def measure_group(name, focus_sets, weights):
    # per-focus-term records over the unresolved (candidate-ranked) population
    resolved_via = collections.Counter()
    n_terms = n_resolved = n_candidated = n_empty = 0
    top1s, margins = [], []
    below_floor_share = []        # per unresolved term: frac of top-k candidates < floor
    rank1_meta = 0                # unresolved terms whose #1 candidate is META
    meta_in_topk = []             # per unresolved term: count of META in top-k
    first_domain_rank = []        # per unresolved term: 1-based rank of first non-META/dual cand
    topk_kind_hist = collections.Counter()
    score_hist = collections.Counter()   # 0.1-width buckets over all candidate scores

    for focus in focus_sets:
        fr = recall(focus)
        if fr is None:
            continue
        for entry in fr.get("resolution", []):
            n_terms += 1
            if "candidates" not in entry:       # resolved (exact/alias/lexical/embedding)
                n_resolved += 1
                resolved_via[entry.get("via", "?")] += 1
                continue
            cands = entry["candidates"][:args.top_k]
            if not cands:
                n_empty += 1
                continue
            n_candidated += 1
            scores = [c.get("score", 0.0) for c in cands]
            kinds = [kind_of(c["ref"]) for c in cands]
            top1s.append(scores[0])
            margins.append(scores[0] - (scores[1] if len(scores) > 1 else 0.0))
            below_floor_share.append(sum(1 for s in scores if s < args.floor) / len(scores))
            for s in scores:
                score_hist[min(int(s * 10), 9)] += 1
            for k in kinds:
                topk_kind_hist[k] += 1
            if is_noise(kinds[0]):
                rank1_meta += 1
            meta_in_topk.append(sum(1 for k in kinds if is_noise(k)))
            dom = next((i for i, k in enumerate(kinds) if not is_noise(k)), None)
            first_domain_rank.append(dom + 1 if dom is not None else 0)  # 0 = none in top-k

    mean = lambda xs: sum(xs) / len(xs) if xs else 0.0
    return {
        "focus_sets": len(focus_sets),
        "focus_terms": n_terms,
        "resolved": n_resolved,
        "candidated": n_candidated,
        "empty": n_empty,
        "resolved_via": dict(resolved_via),
        "mean_top1": round(mean(top1s), 4),
        "mean_margin": round(mean(margins), 4),
        "mean_below_floor_share": round(mean(below_floor_share), 4),
        "rank1_meta_frac": round(rank1_meta / n_candidated, 4) if n_candidated else 0.0,
        "mean_meta_in_topk": round(mean(meta_in_topk), 3),
        "mean_first_domain_rank": round(mean([r for r in first_domain_rank if r]), 3),
        "no_domain_in_topk_frac": round(
            sum(1 for r in first_domain_rank if r == 0) / len(first_domain_rank), 4)
            if first_domain_rank else 0.0,
        "topk_kind_hist": dict(topk_kind_hist.most_common()),
        "score_hist_tenths": {f"0.{b}": score_hist[b] for b in sorted(score_hist)},
    }

report = {"store": args.store, "legend": args.legend, "floor": args.floor, "top_k": args.top_k}
for g in ("ambient", "deliberate"):
    report[g] = measure_group(g, groups[g], seen[g])

# ---- print ----
def show(name, m):
    print(f"\n== {name} ==")
    print(f"  {m['focus_sets']} unique focus-sets -> {m['focus_terms']} terms: "
          f"{m['resolved']} resolved, {m['candidated']} candidate-ranked, {m['empty']} empty")
    if m["resolved_via"]:
        print(f"  resolved via: {m['resolved_via']}")
    if not m["candidated"]:
        print("  (no candidate-ranked terms — nothing to separate)")
        return
    print(f"  C1 signal  top1={m['mean_top1']}  margin(top1-top2)={m['mean_margin']}  "
          f"share<{args.floor}={m['mean_below_floor_share']}")
    print(f"  C2 signal  rank-1 is noise: {m['rank1_meta_frac']}  "
          f"noise in top-{args.top_k}: {m['mean_meta_in_topk']}  "
          f"first domain rank: {m['mean_first_domain_rank']}  "
          f"no domain in top-{args.top_k}: {m['no_domain_in_topk_frac']}")
    print(f"  top-{args.top_k} kind histogram: {m['topk_kind_hist']}")
    print(f"  candidate score buckets (tenths): {m['score_hist_tenths']}")

print(f"store: {args.store}\nbinary: {args.legend}\n"
      f"noise = any kind NOT in domain set {sorted(DOMAIN_KINDS)} (incl. (none)/process kinds)")
for g in ("ambient", "deliberate"):
    show(g, report[g])

if args.json:
    with open(args.json, "w") as f:
        json.dump(report, f, indent=2)
    print(f"\nwrote {args.json}")

shutil.rmtree(work, ignore_errors=True)
