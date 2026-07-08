#!/usr/bin/env python3
"""Rebuild a Legend store by replaying its invocation journal, then compare
snapshots — the in-the-wild determinism check for a long-running deployment.

Every successful journal line carries the tick's wall stamp (ts) and the
payload verbatim, so replaying `init` + each ok save/recall under
LEGEND_NOW=<ts> into a fresh store must reproduce the live legend.snapshot
byte-for-byte. Error lines never mutated and are skipped; observe recalls
replay as no-ops. Embeddings are irrelevant to the snapshot (semantic ranking
only shapes frame candidate lists), so the replay runs with LEGEND_EMBED=0.

  python3 harness/replay_journal.py <store-dir> [--legend BIN] [--keep]

Exit 0 = byte-identical; 1 = divergence (a determinism bug or a hand-edited
store — bisect by truncating the journal).
"""
import os, sys
_here = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path if os.path.abspath(p or ".") != _here]
import argparse, filecmp, json, subprocess, tempfile

p = argparse.ArgumentParser()
p.add_argument("store", help="live store dir holding journal.jsonl + legend.snapshot")
p.add_argument("--legend", default=os.path.expanduser("~/.local/bin/legend"))
p.add_argument("--keep", action="store_true", help="print and keep the rebuilt store dir")
args = p.parse_args()

lines = [json.loads(l) for l in open(os.path.join(args.store, "journal.jsonl"))]
tmp = tempfile.mkdtemp(prefix="legend-replay-")
env0 = dict(os.environ, LEGEND_STATE_DIR=tmp, LEGEND_EMBED="0")
subprocess.run([args.legend, "init"], env=env0, check=True, capture_output=True)

replayed = skipped = 0
for e in lines:
    if e["verb"] == "init" or not e["ok"]:
        skipped += 1
        continue
    r = subprocess.run([args.legend, e["verb"]],
                       input=e["payload"].encode(),
                       env=dict(env0, LEGEND_NOW=str(e["ts"])),
                       capture_output=True)
    if r.returncode != 0:
        sys.exit("replay diverged: %s at ts %d was ok live but failed now:\n%s"
                 % (e["verb"], e["ts"], r.stdout.decode()[:400]))
    replayed += 1

same = filecmp.cmp(os.path.join(args.store, "legend.snapshot"),
                   os.path.join(tmp, "legend.snapshot"), shallow=False)
print("replayed %d ok lines (%d skipped: init/error)" % (replayed, skipped))
print("snapshot byte-identical: %s" % same)
if args.keep:
    print("rebuilt store: %s" % tmp)
sys.exit(0 if same else 1)
