#!/usr/bin/env python3
"""Read-only co-activation miner for a Legend store.

Reconstructs co-activation events from the store's JOURNAL (the always-on
replayable record) and surfaces two things the LLM/human can adjudicate:

  CLUSTERS   -- dense sets of elements that keep co-firing but aren't directly
                linked (the "same group keeps firing -> is a parent concept
                missing, or the structure wrong?" signal).
  DISTANT    -- un-related pairs that co-fire and are >=3 hops apart in the real
                graph: genuinely surprising candidate links, not topic-siblings.

A co-activation EVENT is the set of content elements brought together in one tick:
  * a deliberate multi-focus recall's resolved focus set, and
  * a save's co-mentioned elements (element names + fact s/o).
Filtered by: already-related (drop direct-relation pairs), kind-noise
(event/task/question/pointer/commit/reference + no-kind), and hub (drop the
highest-degree nodes that co-occur with everything).

READ-ONLY: recalls are replayed with observe:true; the store is never written.
No persisted table, no decay clock, no determinism surface -- this is
measurement; naming stays with the LLM/human. Re-run it anytime, on the archive
to retest or on the live store as it accumulates.

Usage:
  python3 harness/coactivation.py [store-dir] [--min-count N] [--embed]
                                  [--bin legend] [--json OUT] [--hub-cap D]
"""
import collections, itertools, json, os, subprocess, sys


def resolve_store(arg, binp):
    if arg:
        return os.path.abspath(arg)
    env = os.environ.get("LEGEND_STATE_DIR")
    if env:
        return env
    sys.exit("no store: pass a dir or set LEGEND_STATE_DIR")


def load_graph(store, binp, embed):
    env = {**os.environ, "LEGEND_STATE_DIR": store, "LEGEND_EMBED": "1" if embed else "0"}
    dump = json.loads(subprocess.run([binp, "dump"], env=env, capture_output=True, text=True).stdout)
    name = {e["ref"]: e["name"] for e in dump["elements"]}
    nm2ref = {e["name"]: e["ref"] for e in dump["elements"]}
    kind, adj, related = {}, collections.defaultdict(set), set()
    for r in dump["relations"]:
        a = r["attrs"]
        if a.get("instance_of") and a.get("subject") in nm2ref:
            kind[nm2ref[a["subject"]]] = a["instance_of"]
        mem = {nm2ref[v] for v in a.values() if v in nm2ref}
        for x, y in itertools.combinations(sorted(mem), 2):
            related.add(frozenset((x, y)))
            adj[x].add(y); adj[y].add(x)
    return dump, name, nm2ref, kind, adj, related, env


def bfs_dist(adj, a, b, cap=4):
    if a == b:
        return 0
    seen, frontier = {a}, {a}
    for d in range(1, cap):
        nxt = set().union(*(adj[u] for u in frontier)) if frontier else set()
        if b in nxt:
            return d
        nxt -= seen; seen |= nxt; frontier = nxt
        if not frontier:
            break
    return 99  # >= cap hops (or disconnected)


def _norm(s):
    return " ".join(s.lower().split())


def events_from_journal(store, nm2ref):
    """Co-activation events: (source, set-of-refs).

    Read-only: focus terms and save-mentioned names are resolved by exact then
    normalized NAME MATCH against the store's elements -- no recall replay, so
    the store's journal is never appended to (dump doesn't journal either).
    Replaying recalls with observe:true would pollute the journal it reads.
    """
    jpath = os.path.join(store, "journal.jsonl")
    if not os.path.exists(jpath):
        sys.exit(f"no journal at {jpath}")
    norm2ref = {}
    for nm, r in nm2ref.items():
        norm2ref.setdefault(_norm(nm), r)

    def resolve(term):
        if not isinstance(term, str):
            return None
        return nm2ref.get(term) or norm2ref.get(_norm(term))

    events = []
    for line in open(jpath):
        line = line.strip()
        if not line:
            continue
        e = json.loads(line)
        if not e.get("ok"):
            continue
        try:
            p = json.loads(e["payload"])
        except Exception:
            continue
        if e.get("verb") == "recall":
            f = p.get("focus")
            if not (isinstance(f, list) and len(f) >= 2):
                continue
            S = {r for r in (resolve(t) for t in f) if r}
            if len(S) >= 2:
                events.append(("recall", S))
        elif e.get("verb") == "save":
            S = set()
            for el in p.get("elements", []) or []:
                r = resolve(el.get("name"))
                if r:
                    S.add(r)
            for fa in p.get("facts", []) or []:
                for k in ("s", "o"):
                    r = resolve(fa.get(k))
                    if r:
                        S.add(r)
            if len(S) >= 2:
                events.append(("save", S))
    return events


NOISE_KINDS = {"event", "task", "question", "pointer", "commit", "reference"}


def mine(store, binp, min_count, embed, hub_cap):
    dump, name, nm2ref, kind, adj, related, env = load_graph(store, binp, embed)
    deg = {e: len(adj[e]) for e in adj}
    if hub_cap is None:  # default: drop the top ~1% by degree
        vals = sorted(deg.values())
        hub_cap = vals[int(0.99 * len(vals))] if vals else 10**9
    hubs = {e for e, d in deg.items() if d > hub_cap}

    def keep(ref):
        k = kind.get(ref, "")
        # no-kind nodes are predicates/provenance/session-markers -- the C2 noise
        # set. Dropping them costs the rare un-kinded content element (fix that at
        # save time, not here) but removes the session-marker co-fire noise.
        return bool(k) and k not in NOISE_KINDS and ref not in hubs

    events = events_from_journal(store, nm2ref)  # name-match resolution, no replay
    # count-based recurrence over UN-related, filtered pairs (no decay)
    pair = collections.Counter()
    for _, S in events:
        Sf = sorted(r for r in S if keep(r))
        for x, y in itertools.combinations(Sf, 2):
            pr = frozenset((x, y))
            if pr not in related:
                pair[pr] += 1

    # --- clusters: connected components of the count>=min_count co-activation graph ---
    cg = collections.defaultdict(set)
    for pr, c in pair.items():
        if c >= min_count:
            x, y = tuple(pr); cg[x].add(y); cg[y].add(x)
    seen, clusters = set(), []
    for node in cg:
        if node in seen:
            continue
        comp, stack = set(), [node]
        while stack:
            u = stack.pop()
            if u in comp:
                continue
            comp.add(u); seen.add(u)
            stack.extend(cg[u] - comp)
        if len(comp) >= 3:
            internal = sum(1 for x, y in itertools.combinations(sorted(comp), 2)
                           if pair.get(frozenset((x, y)), 0) >= min_count)
            weight = sum(pair[frozenset((x, y))] for x, y in itertools.combinations(sorted(comp), 2)
                         if frozenset((x, y)) in pair)
            # do they already share a common real-graph parent?
            common = set.intersection(*(adj[m] for m in comp)) if comp else set()
            clusters.append({"members": sorted(comp), "size": len(comp),
                             "internal_edges": internal, "coact_weight": weight,
                             "shared_parent": sorted(common)})
    clusters.sort(key=lambda c: (c["size"], c["coact_weight"]), reverse=True)

    # --- distant pairs: count>=min_count and >=3 hops apart (surprising, not siblings) ---
    distant = []
    for pr, c in pair.items():
        if c < min_count:
            continue
        x, y = tuple(pr); d = bfs_dist(adj, x, y)
        if d >= 3:
            distant.append({"a": x, "b": y, "count": c, "dist": d})
    distant.sort(key=lambda p: p["count"], reverse=True)

    dbreak = collections.Counter(bfs_dist(adj, *tuple(pr)) for pr, c in pair.items() if c >= 2)
    return {
        "store": store, "elements": len(dump["elements"]), "relations": len(dump["relations"]),
        "events": {"recall": sum(1 for t, _ in events if t == "recall"),
                   "save": sum(1 for t, _ in events if t == "save"), "total": len(events)},
        "hub_cap": hub_cap, "hubs_excluded": len(hubs),
        "pairs_ge": {k: sum(1 for v in pair.values() if v >= k) for k in (2, 3, 4, 5)},
        "distance_breakdown_ge2": {("1-related" if d == 1 else "2-sibling" if d == 2
                                    else "3+distant" if d == 99 else f"{d}hop"): n
                                   for d, n in sorted(dbreak.items())},
        "clusters": clusters, "distant": distant, "name": name, "kind": kind,
    }


def report(res, min_count):
    n, k = res["name"], res["kind"]
    print(f"store: {res['elements']} elements, {res['relations']} relations")
    print(f"events: {res['events']['recall']} recall + {res['events']['save']} save "
          f"= {res['events']['total']} co-activation events")
    print(f"un-related co-fire pairs at count>=: {res['pairs_ge']}  (hub cap deg>{res['hub_cap']}, "
          f"{res['hubs_excluded']} hubs excluded)")
    print(f"distance mix of the count>=2 set: {res['distance_breakdown_ge2']}\n")
    print(f"=== CLUSTERS (>=3 elements co-firing at count>={min_count}) ===")
    if not res["clusters"]:
        print("  (none yet -- needs more co-activation volume)")
    for c in res["clusters"][:8]:
        parent = ", ".join(n.get(p, p) for p in c["shared_parent"][:3]) or "NONE (no shared parent!)"
        print(f"  [{c['size']} elems, {c['internal_edges']} links, weight {c['coact_weight']}] "
              f"shared parent: {parent}")
        for m in c["members"][:10]:
            print(f"       - [{k.get(m,'?')}] {n.get(m,m)}")
    print(f"\n=== DISTANT candidate links (count>={min_count}, >=3 hops apart) ===")
    if not res["distant"]:
        print("  (none -- everything recurring is a topic-sibling)")
    for p in res["distant"][:15]:
        print(f"  {p['count']}x  {p['dist']}hop  [{k.get(p['a'],'?')}] {n.get(p['a'],p['a'])!r} "
              f"<-> [{k.get(p['b'],'?')}] {n.get(p['b'],p['b'])!r}")


def main():
    # hand-rolled parse: harness/inspect.py shadows stdlib inspect, which py3.14
    # argparse imports via _colorize -- so we avoid argparse entirely.
    args = sys.argv[1:]
    store = None; min_count = 3; embed = False; binp = "legend"; hub_cap = None; jout = None
    i = 0
    while i < len(args):
        a = args[i]
        if a in ("-h", "--help"):
            print(__doc__); return
        elif a == "--min-count":
            min_count = int(args[i + 1]); i += 2
        elif a == "--embed":
            embed = True; i += 1
        elif a == "--bin":
            binp = args[i + 1]; i += 2
        elif a == "--hub-cap":
            hub_cap = int(args[i + 1]); i += 2
        elif a == "--json":
            jout = args[i + 1]; i += 2
        elif not a.startswith("-"):
            store = a; i += 1
        else:
            sys.exit(f"unknown arg: {a}")
    store = resolve_store(store, binp)
    res = mine(store, binp, min_count, embed, hub_cap)
    report(res, min_count)
    if jout:
        slim = {kk: vv for kk, vv in res.items() if kk not in ("name", "kind")}
        json.dump(slim, open(jout, "w"), indent=2)
        print(f"\nfull result -> {jout}")


if __name__ == "__main__":
    main()
