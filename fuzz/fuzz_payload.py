#!/usr/bin/env python3
"""Fuzz target A (plan §8 M5): payload mutation against the plan/apply invariant.

Mutates valid seed payloads (tests/fixtures steps + harness/corpus episodes)
with byte flips, truncations, splices, deep nesting, huge numbers, invalid
UTF-8, and structure-aware field swaps, feeding each to the sanitizer binary
against a fresh store. The invariant (plan §4): every input either exits 1
with a single valid error-JSON object on stdout (store untouched), or exits 0
having applied fully — and a post-run recall on the same store still works.

Deterministic: iteration i draws from random.Random(f"{seed}:{i}"), so any
--jobs value yields identical verdicts. Reproduce a failure with
  SEED=<seed> python3 fuzz/fuzz_payload.py --legend <bin> --repro <i>
"""

import argparse
import json
import multiprocessing
import os
import random
import shutil
import subprocess
import sys
import tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ERROR_CODES = {"parse", "unknown_ref", "ambiguous_ref", "limit_exceeded",
               "no_store", "lock_timeout", "snapshot_corrupt", "store_full",
               "prose_value"}
CHILD_ENV_BASE = {
    "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
    "ASAN_OPTIONS": "abort_on_error=1:detect_leaks=0",
    "UBSAN_OPTIONS": "print_stacktrace=1",
    "LEGEND_EMBED": "0",  # fuzz tests core parsing/invariants; embedding per iter is too slow
}
TIMEOUT_S = 10

# Globals shared with fork()ed pool workers (set in main before Pool()).
G = {"legend": None, "seeds": None, "stores": None, "workdir": None}


def collect_seed_payloads():
    """Every (verb, payload-bytes) step from the fixtures and the corpus episodes."""
    seeds = []
    dirs = [os.path.join(REPO, "tests", "fixtures"),
            os.path.join(REPO, "harness", "corpus", "episodes")]
    for d in dirs:
        for fn in sorted(os.listdir(d)):
            if not fn.endswith(".json"):
                continue
            doc = json.load(open(os.path.join(d, fn)))
            for step in doc.get("steps", []):
                if "payload" in step and step.get("verb") in ("save", "recall"):
                    text = json.dumps(step["payload"], separators=(",", ":"))
                    seeds.append((step["verb"], text.encode()))
    if len(seeds) < 20:
        sys.exit("fuzz_payload: too few seed payloads found")
    return seeds


def build_template_stores(legend, workdir):
    """Two pre-states: a bare init'd store, and one with real episode history."""
    stores = []
    for name, steps in (("bare", []), ("warm", warm_steps())):
        d = os.path.join(workdir, "tmpl_" + name)
        os.makedirs(d)
        env = dict(CHILD_ENV_BASE, LEGEND_STATE_DIR=d)
        subprocess.run([legend, "init"], env=env, check=True,
                       stdout=subprocess.DEVNULL, timeout=TIMEOUT_S)
        for now, verb, payload in steps:
            env["LEGEND_NOW"] = str(now)
            p = subprocess.run([legend, verb], env=env, input=payload,
                               stdout=subprocess.DEVNULL, timeout=TIMEOUT_S)
            if p.returncode != 0:
                sys.exit(f"fuzz_payload: template step failed: {payload[:80]!r}")
        with open(os.path.join(d, "legend.snapshot"), "rb") as f:
            stores.append(f.read())
    return stores


def warm_steps():
    """The first steps of two corpus episodes, replayed with their pinned nows."""
    steps = []
    for ep, take in (("e01_scaffold.json", 8), ("e05_feel.json", 6)):
        doc = json.load(open(os.path.join(REPO, "harness", "corpus", "episodes", ep)))
        for step in doc["steps"][:take]:
            payload = json.dumps(step["payload"], separators=(",", ":")).encode()
            steps.append((step["now"], step["verb"], payload))
    steps.sort(key=lambda s: s[0])
    return steps


# ---- mutations ----

SPECIAL_SCALARS = [
    0, -1, 1, 64, 65, 2**31, 2**32, 2**63 - 1, -2**63, 10**25, -10**25,
    1e308, -1e308, 5e-324, 0.30000000000000004, 1.0000000000000002,
    True, False, None, "", " ", "a" * 300, "a" * 70000, "\u0000",
    "rel:0", "rel:4294967295", "rel:99999999999999999999", "#0", "#-1",
    "  --  ", "....", "\t\n\r", "é" * 40, "𐀀", [], {},
]


def deep_nest(rng):
    depth = rng.randrange(2, 220)
    v = rng.choice([0, "x", None])
    for _ in range(depth):
        v = [v] if rng.random() < 0.7 else {"k": v}
    return v


def all_paths(node, prefix=()):
    yield prefix
    if isinstance(node, dict):
        for k, v in node.items():
            yield from all_paths(v, prefix + (k,))
    elif isinstance(node, list):
        for i, v in enumerate(node):
            yield from all_paths(v, prefix + (i,))


def get_path(node, path):
    for p in path:
        node = node[p]
    return node


def set_path(root, path, value):
    if not path:
        return value
    node = get_path(root, path[:-1])
    node[path[-1]] = value
    return root


def mutate_tree(rng, tree, other):
    """One structure-aware mutation; returns the (possibly new) root."""
    paths = list(all_paths(tree))
    op = rng.randrange(8)
    if op == 0 and len(paths) >= 3:  # field/value swap
        a, b = rng.sample(paths[1:], 2)
        va, vb = get_path(tree, a), get_path(tree, b)
        tree = set_path(tree, a, vb)
        tree = set_path(tree, b, va)
    elif op == 1:  # replace a node with a special scalar
        tree = set_path(tree, rng.choice(paths), rng.choice(SPECIAL_SCALARS))
    elif op == 2:  # replace a node with deep nesting
        tree = set_path(tree, rng.choice(paths), deep_nest(rng))
    elif op == 3:  # graft a random subtree from another seed
        opaths = list(all_paths(other))
        tree = set_path(tree, rng.choice(paths), get_path(other, rng.choice(opaths)))
    elif op == 4:  # typo a key
        dicts = [p for p in paths if isinstance(get_path(tree, p), dict) and get_path(tree, p)]
        if dicts:
            d = get_path(tree, rng.choice(dicts))
            k = rng.choice(list(d.keys()))
            v = d.pop(k)
            if k:
                i = rng.randrange(len(k))
                k = k[:i] + rng.choice("abcs_ZF0é ") + k[i + 1:]
            d[k + ("" if rng.random() < 0.8 else "s")] = v
    elif op == 5:  # delete a member
        containers = [p for p in paths if isinstance(get_path(tree, p), (dict, list)) and get_path(tree, p)]
        if containers:
            c = get_path(tree, rng.choice(containers))
            if isinstance(c, dict):
                del c[rng.choice(list(c.keys()))]
            else:
                del c[rng.randrange(len(c))]
    elif op == 6:  # oversize an array (list cap is 64)
        lists = [p for p in paths if isinstance(get_path(tree, p), list)]
        if lists:
            lst = get_path(tree, rng.choice(lists))
            filler = lst[-1] if lst else {"name": "x"}
            lst.extend([filler] * rng.randrange(60, 90))
    else:  # promote a subtree to the root, or wrap the root
        if rng.random() < 0.5 and len(paths) > 1:
            tree = get_path(tree, rng.choice(paths[1:]))
        else:
            tree = rng.choice([[tree], {"payload": tree}])
    return tree


def mutate_bytes(rng, data, other):
    """One byte-level mutation."""
    data = bytearray(data)
    op = rng.randrange(6)
    if op == 0:  # flip 1..8 bytes
        for _ in range(rng.randrange(1, 9)):
            if data:
                data[rng.randrange(len(data))] ^= rng.randrange(1, 256)
    elif op == 1 and data:  # truncate
        del data[rng.randrange(len(data)):]
    elif op == 2:  # insert raw bytes (invalid UTF-8 included)
        chunk = bytes(rng.randrange(256) for _ in range(rng.randrange(1, 17)))
        pos = rng.randrange(len(data) + 1)
        data[pos:pos] = chunk
    elif op == 3:  # splice with another seed
        cut_a = rng.randrange(len(data) + 1)
        cut_b = rng.randrange(len(other) + 1)
        data = bytearray(data[:cut_a] + other[cut_b:])
    elif op == 4 and data:  # duplicate a chunk in place
        lo = rng.randrange(len(data))
        hi = min(len(data), lo + rng.randrange(1, 64))
        pos = rng.randrange(len(data) + 1)
        data[pos:pos] = data[lo:hi]
    else:  # inject a huge number literal at a random position
        lit = b"1" + b"0" * rng.randrange(1, 400)
        pos = rng.randrange(len(data) + 1)
        data[pos:pos] = lit
    return bytes(data)


def build_input(rng, seeds):
    verb, payload = rng.choice(seeds)
    if rng.random() < 0.35:  # sometimes fuzz the verb choice independently
        verb = rng.choice(["save", "recall"])
    other_verb, other = rng.choice(seeds)
    if rng.random() < 0.02:  # rare: pure garbage / nesting bombs
        data = rng.choice([
            b"[" * rng.randrange(1, 5000),
            b"{" * rng.randrange(1, 5000),
            bytes(rng.randrange(256) for _ in range(rng.randrange(0, 300))),
            b'{"focus":' + b"[" * 200 + b"]" * 200 + b"}",
        ])
        return verb, data
    data = payload
    for _ in range(rng.randrange(1, 4)):
        if rng.random() < 0.55:
            try:
                tree = json.loads(data.decode())
                tree = mutate_tree(rng, tree, json.loads(other.decode()))
                data = json.dumps(tree, separators=(",", ":")).encode("utf-8", "surrogatepass")
            except (ValueError, RecursionError, UnicodeDecodeError,
                    TypeError, KeyError, IndexError):
                # not JSON anymore, or a stale path after a structural swap
                data = mutate_bytes(rng, data, other)
        else:
            data = mutate_bytes(rng, data, other)
    return verb, data


# ---- one iteration ----

def parse_single_json(raw):
    """stdout must be exactly one JSON document in valid UTF-8."""
    text = raw.decode("utf-8")  # strict: invalid UTF-8 output is a failure
    return json.loads(text)


def run_one(i):
    rng = random.Random(f"{G['seed']}:{i}")
    verb, data = build_input(rng, G["seeds"])
    template = G["stores"][0] if rng.random() < 0.4 else G["stores"][1]
    store = os.path.join(G["workdir"], f"it{i}")
    os.makedirs(store, exist_ok=True)
    snap = os.path.join(store, "legend.snapshot")
    with open(snap, "wb") as f:
        f.write(template)
    now = 1790000000 + i if rng.random() < 0.9 else rng.randrange(0, 2**33)
    env = dict(CHILD_ENV_BASE, LEGEND_STATE_DIR=store, LEGEND_NOW=str(now))

    def bad(reason):
        keep = os.path.join(G["workdir"], f"crash_it{i}")
        os.makedirs(keep, exist_ok=True)
        with open(os.path.join(keep, "input.bin"), "wb") as f:
            f.write(data)
        with open(os.path.join(keep, "cmd.txt"), "w") as f:
            f.write(f"verb={verb} LEGEND_NOW={now} template={len(template)}B\n"
                    f"repro: SEED={G['seed']} python3 fuzz/fuzz_payload.py "
                    f"--legend {G['legend']} --repro {i}\n")
        return (i, f"{reason} [verb={verb} artifacts={keep}]")

    try:
        p = subprocess.run([G["legend"], verb], env=env, input=data,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                           timeout=TIMEOUT_S)
    except subprocess.TimeoutExpired:
        return bad("hang: killed after timeout")
    if p.stderr:
        return bad(f"stderr output (rc={p.returncode}): {p.stderr[:400]!r}")
    if p.returncode == 1:
        try:
            doc = parse_single_json(p.stdout)
        except (ValueError, UnicodeDecodeError) as e:
            return bad(f"error exit without valid JSON: {e}; stdout={p.stdout[:200]!r}")
        if set(doc.keys()) != {"error"} or doc["error"].get("code") not in ERROR_CODES:
            return bad(f"malformed error envelope: {p.stdout[:200]!r}")
        with open(snap, "rb") as f:
            if f.read() != template:
                return bad("error exit mutated the store")
    elif p.returncode == 0:
        try:
            doc = parse_single_json(p.stdout)
        except (ValueError, UnicodeDecodeError) as e:
            return bad(f"success exit without valid JSON frame: {e}; stdout={p.stdout[:200]!r}")
        if "error" in doc:
            return bad("exit 0 but an error object on stdout")
        # the applied store must still load, tick, and save
        try:
            q = subprocess.run([G["legend"], "recall"], env=env, input=b"{}",
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               timeout=TIMEOUT_S)
        except subprocess.TimeoutExpired:
            return bad("post-run recall hang")
        if q.returncode != 0 or q.stderr:
            return bad(f"post-run recall failed (rc={q.returncode}): "
                       f"{(q.stderr or q.stdout)[:300]!r}")
        try:
            parse_single_json(q.stdout)
        except (ValueError, UnicodeDecodeError) as e:
            return bad(f"post-run recall frame invalid: {e}")
    else:
        return bad(f"abnormal exit rc={p.returncode} stdout={p.stdout[:200]!r}")
    shutil.rmtree(store, ignore_errors=True)
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--legend", required=True)
    ap.add_argument("--iters", type=int,
                    default=int(os.environ.get("FUZZ_ITERS", "50000")))
    ap.add_argument("--seed", type=int,
                    default=int(os.environ.get("SEED", "20260703")))
    ap.add_argument("--jobs", type=int, default=min(os.cpu_count() or 1, 8))
    ap.add_argument("--repro", type=int, default=None,
                    help="run exactly this iteration, keep its store")
    args = ap.parse_args()

    G["legend"] = os.path.abspath(args.legend)
    G["seed"] = args.seed
    G["workdir"] = tempfile.mkdtemp(prefix="legend_fuzz_payload_")
    G["seeds"] = collect_seed_payloads()
    G["stores"] = build_template_stores(G["legend"], G["workdir"])

    if args.repro is not None:
        rng = random.Random(f"{args.seed}:{args.repro}")
        verb, data = build_input(rng, G["seeds"])
        print(f"iteration {args.repro}: verb={verb} input={data!r}", file=sys.stderr)
        res = run_one(args.repro)
        print(res[1] if res else "OK: invariant held", file=sys.stderr)
        print(f"workdir kept: {G['workdir']}", file=sys.stderr)
        sys.exit(1 if res else 0)

    failures = []
    it = range(args.iters)
    if args.jobs > 1:
        # fork, explicitly: workers must inherit G (3.14 defaults to forkserver)
        with multiprocessing.get_context("fork").Pool(args.jobs) as pool:
            for res in pool.imap_unordered(run_one, it, chunksize=64):
                if res:
                    failures.append(res)
                    if len(failures) >= 10:
                        break
    else:
        for i in it:
            res = run_one(i)
            if res:
                failures.append(res)
                if len(failures) >= 10:
                    break

    if failures:
        for i, reason in sorted(failures):
            print(f"FAIL it{i}: {reason}", file=sys.stderr)
        print(f"fuzz_payload: {len(failures)} failure(s) in {args.iters} iterations "
              f"(seed {args.seed}); workdir kept: {G['workdir']}", file=sys.stderr)
        sys.exit(1)
    shutil.rmtree(G["workdir"], ignore_errors=True)
    print(f"fuzz_payload: {args.iters} iterations clean (seed {args.seed}, "
          f"{len(G['seeds'])} seed payloads)")


if __name__ == "__main__":
    main()
