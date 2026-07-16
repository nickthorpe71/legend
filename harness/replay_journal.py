#!/usr/bin/env python3
"""Rebuild a Legend store by replaying its invocation journal, then compare
snapshots — the in-the-wild determinism check for a long-running deployment.

Every successful journal line carries the tick's wall stamp (ts), the payload
verbatim, and the `build` (the git short-sha of the binary that wrote it), so
replaying `init` + each ok save/recall under LEGEND_NOW=<ts> reproduces the live
legend.snapshot byte-for-byte. Error lines never mutated and are skipped;
embeddings are irrelevant to the snapshot, so replay runs with LEGEND_EMBED=0.

BUILD-AWARE by default. A long-running journal spans binary upgrades, and a
semantic fix makes an op that relied on the OLD behavior replay differently
under a NEW binary (e.g. the source-phantom fix ec197d7: a merge whose `from`
was a phantom the buggy path minted). Byte-identical replay therefore holds only
when each line runs under the binary that wrote it. This tool reconstructs each
distinct `build` from git (git archive + cc, cached per sha) and replays every
line under its own build's binary — so the WHOLE multi-build journal stays a
verifiable replayable corpus, not just one binary's segment.

  python3 harness/replay_journal.py <store-dir> [--keep] [--simple [--legend BIN]]

--simple restores the old single-binary behavior (init + replay all under one
BIN); use it only for a single-build journal. Exit 0 = byte-identical; 1 =
divergence (a real determinism bug or a hand-edited store — bisect by truncating
the journal); 2 = a build could not be reconstructed.
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, filecmp, json, shutil, subprocess, tempfile

REPO = os.path.dirname(_here)

p = argparse.ArgumentParser()
p.add_argument("store", help="live store dir holding journal.jsonl + legend.snapshot")
p.add_argument("--legend", default=os.path.expanduser("~/.local/bin/legend"),
               help="binary for --simple mode (ignored in build-aware mode)")
p.add_argument("--simple", action="store_true",
               help="single-binary replay (old behavior); only valid for a one-build journal")
p.add_argument("--keep", action="store_true", help="print and keep the rebuilt store dir")
args = p.parse_args()

lines = [json.loads(l) for l in open(os.path.join(args.store, "journal.jsonl"))]
ok_lines = [e for e in lines if e.get("ok") and e["verb"] != "init"]
builds = []
for e in ok_lines:
    b = e.get("build")
    if b and b not in builds:
        builds.append(b)

# ---- resolve a binary per build sha (git archive <sha> legend.c embed.c | cc) ----
bincache = {}
build_root = tempfile.mkdtemp(prefix="legend-builds-")

def binary_for(build):
    """Path to the replayed binary for git short-sha `build`, built on demand.
    LEGEND_EMBED=0 at replay time, so the embedder model is never loaded and the
    binary only needs to compile; built without -Werror so an old sha that trips
    a newer compiler's lint still runs."""
    if build in bincache:
        return bincache[build]
    if not build or build == "dev":
        sys.exit("build %r is not a git sha (a dev/unstamped build cannot be "
                 "reconstructed); replay is not verifiable for it" % build)
    if subprocess.run(["git", "-C", REPO, "cat-file", "-e", build + "^{commit}"],
                      capture_output=True).returncode != 0:
        print("build %s is not in git — cannot reconstruct" % build, file=sys.stderr)
        sys.exit(2)
    src = os.path.join(build_root, build)
    os.makedirs(src, exist_ok=True)
    # translation units + every root header at that sha (embed.h and any others)
    tree = subprocess.run(["git", "-C", REPO, "ls-tree", "--name-only", build],
                          capture_output=True, text=True).stdout.split()
    need = ["legend.c", "embed.c"] + [f for f in tree if f.endswith(".h")]
    for f in need:
        blob = subprocess.run(["git", "-C", REPO, "show", "%s:%s" % (build, f)],
                              capture_output=True)
        if blob.returncode != 0:
            print("build %s: %s missing at that sha" % (build, f), file=sys.stderr)
            sys.exit(2)
        open(os.path.join(src, f), "wb").write(blob.stdout)
    out = os.path.join(src, "legend")
    cc = subprocess.run(["cc", "-std=c99", "-O2",
                         "-DLEGEND_BUILD=\"%s\"" % build,
                         os.path.join(src, "legend.c"), os.path.join(src, "embed.c"),
                         "-o", out, "-lm"], capture_output=True)
    if cc.returncode != 0:
        print("build %s failed to compile:\n%s" % (build, cc.stderr.decode()[:800]),
              file=sys.stderr)
        sys.exit(2)
    bincache[build] = out
    return out

# ---- replay ----
tmp = tempfile.mkdtemp(prefix="legend-replay-")
env0 = dict(os.environ, LEGEND_STATE_DIR=tmp, LEGEND_EMBED="0")

if args.simple:
    if len(builds) > 1:
        print("WARNING: journal spans %d builds %s but --simple replays all under "
              "one binary; divergence is expected across a fix-bearing upgrade."
              % (len(builds), builds), file=sys.stderr)
    init_bin = args.legend
    line_bin = lambda e: args.legend
else:
    # init under the build that created the store (the first line's build), then
    # every line under its own build.
    init_bin = binary_for(ok_lines[0].get("build")) if ok_lines else args.legend
    for b in builds:
        binary_for(b)
    line_bin = lambda e: binary_for(e.get("build"))

subprocess.run([init_bin, "init"], env=env0, check=True, capture_output=True)

replayed = skipped = 0
for e in lines:
    if e["verb"] == "init" or not e.get("ok"):
        skipped += 1
        continue
    r = subprocess.run([line_bin(e), e["verb"]],
                       input=e["payload"].encode(),
                       env=dict(env0, LEGEND_NOW=str(e["ts"])),
                       capture_output=True)
    if r.returncode != 0:
        sys.exit("replay diverged: %s at ts %d (build %s) was ok live but failed "
                 "now:\n%s" % (e["verb"], e["ts"], e.get("build"),
                               r.stdout.decode()[:400]))
    replayed += 1

same = filecmp.cmp(os.path.join(args.store, "legend.snapshot"),
                   os.path.join(tmp, "legend.snapshot"), shallow=False)
mode = "simple (1 binary)" if args.simple else "build-aware (%d binaries: %s)" % (
    len(builds), ", ".join(builds))
print("mode: %s" % mode)
print("replayed %d ok lines (%d skipped: init/error)" % (replayed, skipped))
print("snapshot byte-identical: %s" % same)
if args.keep:
    print("rebuilt store: %s" % tmp)
else:
    shutil.rmtree(tmp, ignore_errors=True)
shutil.rmtree(build_root, ignore_errors=True)
sys.exit(0 if same else 1)
