#!/usr/bin/env python3
"""Fuzz target B (plan §8 M5): corrupt snapshots against the §3.11 reader.

Takes a real replayed store's snapshot (the smoke corpus) plus a bare seeded
one, applies byte flips / truncations / extensions / structured u32 patches at
random offsets, and invokes the binary. The invariant: every run exits with a
clean single-JSON `snapshot_corrupt` error, or exits 0 (the damage hit a
don't-care byte) with the loaded graph re-serializing consistently — a
follow-up recall on the rewritten store must succeed and an observe recall
must leave it byte-identical. Never a crash, never UB, never a hang.

Deterministic: iteration i draws from random.Random(f"{seed}:{i}"), so any
--jobs value yields identical verdicts. Reproduce a failure with
  SEED=<seed> python3 fuzz/fuzz_snapshot.py --legend <bin> --repro <i>
"""

import argparse
import json
import multiprocessing
import os
import random
import shutil
import struct
import subprocess
import sys
import tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CHILD_ENV_BASE = {
    "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
    "ASAN_OPTIONS": "abort_on_error=1:detect_leaks=0",
    "UBSAN_OPTIONS": "print_stacktrace=1",
    "LEGEND_NOW": "1786000000",
    "LEGEND_EMBED": "0",  # snapshot fuzz is embeddings-independent; keep it fast
}
TIMEOUT_S = 10

G = {"legend": None, "bases": None, "workdir": None, "seed": None}


def build_base_snapshots(legend, workdir):
    """Base 0: the smoke-corpus replayed store. Base 1: a bare init'd store."""
    corpus = os.path.join(workdir, "smoke.jsonl")
    rstore = os.path.join(workdir, "rstore")
    subprocess.run([sys.executable, os.path.join(REPO, "harness", "gen_corpus.py"),
                    "--slice", "smoke", "-o", corpus],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run([sys.executable, os.path.join(REPO, "harness", "run.py"),
                    "--legend", legend, "--replay", corpus, "--store", rstore],
                   check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    bare = os.path.join(workdir, "bare")
    os.makedirs(bare)
    subprocess.run([legend, "init"], env=dict(CHILD_ENV_BASE, LEGEND_STATE_DIR=bare),
                   check=True, stdout=subprocess.DEVNULL, timeout=TIMEOUT_S)
    out = []
    for d in (rstore, bare):
        with open(os.path.join(d, "legend.snapshot"), "rb") as f:
            out.append(f.read())
    return out


def corrupt(rng, base):
    data = bytearray(base)
    op = rng.randrange(10)
    if op < 4:  # byte flips anywhere (the bulk of the diet)
        for _ in range(rng.randrange(1, 9)):
            data[rng.randrange(len(data))] ^= rng.randrange(1, 256)
    elif op == 4:  # flips concentrated in the header / policy / counts region
        for _ in range(rng.randrange(1, 5)):
            data[rng.randrange(min(250, len(data)))] ^= rng.randrange(1, 256)
    elif op == 5:  # truncate
        del data[rng.randrange(len(data)):]
    elif op == 6:  # extend with random bytes
        data.extend(rng.randrange(256) for _ in range(rng.randrange(1, 65)))
    elif op == 7:  # smash a window to 0x00 or 0xFF
        lo = rng.randrange(len(data))
        hi = min(len(data), lo + rng.randrange(1, 17))
        data[lo:hi] = bytes([rng.choice((0x00, 0xFF))]) * (hi - lo)
    elif op == 8:  # patch an aligned u32 with a boundary value
        off = rng.randrange(0, len(data) - 4)
        val = rng.choice((0, 1, 2, 63, 64, 0x7FFFFFFF, 0xFFFFFFFE, 0xFFFFFFFF,
                          rng.randrange(0, 1 << 32)))
        struct.pack_into("<I", data, off, val)
    else:  # swap two windows
        n = rng.randrange(1, 33)
        if len(data) > 2 * n:
            a = rng.randrange(len(data) - n)
            b = rng.randrange(len(data) - n)
            data[a:a + n], data[b:b + n] = data[b:b + n], data[a:a + n]
    return bytes(data)


def parse_single_json(raw):
    return json.loads(raw.decode("utf-8"))


def run_one(i):
    rng = random.Random(f"{G['seed']}:{i}")
    base = G["bases"][0] if rng.random() < 0.85 else G["bases"][1]
    data = corrupt(rng, base)
    store = os.path.join(G["workdir"], f"it{i}")
    os.makedirs(store, exist_ok=True)
    snap = os.path.join(store, "legend.snapshot")
    with open(snap, "wb") as f:
        f.write(data)
    env = dict(CHILD_ENV_BASE, LEGEND_STATE_DIR=store)

    def bad(reason):
        keep = os.path.join(G["workdir"], f"crash_it{i}")
        os.makedirs(keep, exist_ok=True)
        with open(os.path.join(keep, "legend.snapshot"), "wb") as f:
            f.write(data)
        with open(os.path.join(keep, "cmd.txt"), "w") as f:
            f.write(f"base={len(base)}B corrupted={len(data)}B\n"
                    f"repro: SEED={G['seed']} python3 fuzz/fuzz_snapshot.py "
                    f"--legend {G['legend']} --repro {i}\n")
        return (i, f"{reason} [artifacts={keep}]")

    try:
        p = subprocess.run([G["legend"], "recall"], env=env, input=b"{}",
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
        if set(doc.keys()) != {"error"} or doc["error"].get("code") != "snapshot_corrupt":
            return bad(f"expected snapshot_corrupt, got: {p.stdout[:200]!r}")
    elif p.returncode == 0:
        # don't-care damage: the rewritten store must re-load and stay stable
        try:
            parse_single_json(p.stdout)
        except (ValueError, UnicodeDecodeError) as e:
            return bad(f"success exit without valid JSON frame: {e}")
        with open(snap, "rb") as f:
            rewritten = f.read()
        try:
            q = subprocess.run([G["legend"], "recall"], env=env,
                               input=b'{"observe":true}',
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               timeout=TIMEOUT_S)
        except subprocess.TimeoutExpired:
            return bad("observe recall hang on the rewritten store")
        if q.returncode != 0 or q.stderr:
            return bad(f"rewritten store does not re-load (rc={q.returncode}): "
                       f"{(q.stderr or q.stdout)[:300]!r}")
        with open(snap, "rb") as f:
            if f.read() != rewritten:
                return bad("observe recall mutated the rewritten store")
    else:
        return bad(f"abnormal exit rc={p.returncode} stdout={p.stdout[:200]!r}")
    shutil.rmtree(store, ignore_errors=True)
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--legend", required=True)
    ap.add_argument("--iters", type=int,
                    default=int(os.environ.get("FUZZ_ITERS", "20000")))
    ap.add_argument("--seed", type=int,
                    default=int(os.environ.get("SEED", "20260703")))
    ap.add_argument("--jobs", type=int, default=min(os.cpu_count() or 1, 8))
    ap.add_argument("--repro", type=int, default=None,
                    help="run exactly this iteration, keep its store")
    args = ap.parse_args()

    G["legend"] = os.path.abspath(args.legend)
    G["seed"] = args.seed
    G["workdir"] = tempfile.mkdtemp(prefix="legend_fuzz_snapshot_")
    G["bases"] = build_base_snapshots(G["legend"], G["workdir"])

    if args.repro is not None:
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
        print(f"fuzz_snapshot: {len(failures)} failure(s) in {args.iters} iterations "
              f"(seed {args.seed}); workdir kept: {G['workdir']}", file=sys.stderr)
        sys.exit(1)
    shutil.rmtree(G["workdir"], ignore_errors=True)
    print(f"fuzz_snapshot: {args.iters} iterations clean (seed {args.seed}, "
          f"bases {len(G['bases'][0])}B and {len(G['bases'][1])}B)")


if __name__ == "__main__":
    main()
