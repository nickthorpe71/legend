#!/usr/bin/env python3
"""Legend v3 conformance runner (plan §9, Track 2).

Spawns a legend binary once per step/line: payload via stdin then close
(the binary is EOF-delimited, plan §3.13), LEGEND_NOW from the step,
LEGEND_STATE_DIR pointing at a fresh mkdtemp store per fixture/replay.
Collects stdout, exit code, wall time, and maxrss per spawn.

Modes:
    run.py --legend PATH --fixture tests/fixtures/f03_compact.json
        Replays the fixture's steps; non-null `expect` values are diffed
        against stdout via harness/diff.py (errors: subset match on the
        keys present in the expected error object; frames: full diff after
        substituting the "<store>" placeholder).
    run.py --legend PATH --replay corpus.jsonl
        Inits a fresh store, then replays JSONL lines
        {"now": N, "verb": "...", "payload": {...}}; no assertions; frames
        echoed to stdout, stats to stderr.
        --probes FILE fires the probe file's observe-recalls at their
        after_line checkpoints (spec §13; plan §9 probe injection); probe
        frames go to --probe-results FILE (JSON), never to stdout, so the
        replayed frame stream stays byte-comparable across runs.
        --store DIR replays into DIR (created, kept) instead of a mkdtemp —
        the double-replay snapshot byte-identity gate reads it afterwards.
    add --dry-run to either: print would-be invocations, spawn nothing.

Fixture format: tests/fixtures/README.md.
Exit: 0 all steps clean, 1 divergence/failure, 2 usage error.
"""

import argparse
import json
import os
import resource
import shutil
import subprocess
import sys
import tempfile
import time

# Python auto-prepends this script's directory to sys.path, which makes
# harness/inspect.py shadow the stdlib `inspect` module (argparse needs it).
# Scrub it, and load the sibling differ by explicit path instead.
_HERE = os.path.dirname(os.path.abspath(__file__))
sys.path = [p for p in sys.path
            if os.path.abspath(p if p else ".") != _HERE]

import importlib.util as _ilu  # noqa: E402

_spec = _ilu.spec_from_file_location(
    "legend_diff", os.path.join(_HERE, "diff.py"))
differ = _ilu.module_from_spec(_spec)
_spec.loader.exec_module(differ)

GARBAGE = b"LEGEND-HARNESS-CORRUPTION\x00\xff\xfe" * 8
VERBS = ("save", "recall", "init")


def stdin_bytes(step):
    raw = step.get("payload_raw")
    if raw is not None:
        return raw.encode("utf-8")
    payload = step.get("payload")
    if payload is None:
        return b""
    return json.dumps(payload, ensure_ascii=False,
                      separators=(",", ":")).encode("utf-8")


def corrupt_store(store):
    """Overwrite every regular file except legend.lock (plan §S9 sweeps
    skip the lock by name; corrupting it would test flock, not the
    snapshot reader)."""
    for name in sorted(os.listdir(store)):
        path = os.path.join(store, name)
        if name == "legend.lock" or not os.path.isfile(path):
            continue
        with open(path, "wb") as f:
            f.write(GARBAGE)


def spawn(legend, verb, data, store, now):
    env = dict(os.environ)
    env["LEGEND_STATE_DIR"] = store
    env["LEGEND_NOW"] = str(now)
    before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    t0 = time.monotonic()
    proc = subprocess.run([legend, verb], input=data, env=env,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    wall_ms = (time.monotonic() - t0) * 1000.0
    after = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    # ru_maxrss is a high-water mark across all children (KiB on Linux);
    # per-step attribution is only exact for the largest child so far.
    maxrss_kib = max(after, before)
    return proc, wall_ms, maxrss_kib


def substitute_store(node, store):
    """Replace the '<store>' placeholder in an expected frame."""
    if isinstance(node, dict):
        return {k: substitute_store(v, store) for k, v in node.items()}
    if isinstance(node, list):
        return [substitute_store(v, store) for v in node]
    if isinstance(node, str) and not isinstance(node, differ.Num):
        return node.replace("<store>", store)
    return node


def check_step(step, proc, store):
    """Returns None if the step is clean, else a failure string."""
    expect = step.get("expect")
    stdout = proc.stdout.decode("utf-8", "replace")
    if expect is None:
        if proc.returncode != 0:
            return ("prelude step exited %d\nstderr: %s\nstdout: %s"
                    % (proc.returncode, proc.stderr.decode("utf-8", "replace"),
                       stdout))
        return None
    if isinstance(expect, dict) and "error" in expect:
        if proc.returncode == 0:
            return "expected error %s but exit was 0; stdout: %s" % (
                json.dumps(expect["error"]), stdout)
        try:
            actual = differ.loads_raw(stdout)
        except ValueError as e:
            return "error output is not JSON (%s): %s" % (e, stdout)
        if not isinstance(actual, dict) or "error" not in actual:
            return "expected an error envelope, got: %s" % stdout
        # subset match: only the keys authored in the fixture are compared
        # (spec §9 pins codes and envelope shape, not message text)
        exp_err = differ.loads_raw(json.dumps(expect["error"]))
        for key, want in exp_err.items():
            if key not in actual["error"]:
                return "error.%s missing; got: %s" % (key, stdout)
            d = differ.compare(want, actual["error"][key],
                               "/error/" + key)
            if d:
                return d.render()
        return None
    # frame expectation: exit 0 + full structural diff
    if proc.returncode != 0:
        return ("expected a frame but exit was %d\nstderr: %s\nstdout: %s"
                % (proc.returncode, proc.stderr.decode("utf-8", "replace"),
                   stdout))
    expected = substitute_store(
        differ.loads_raw(json.dumps(expect, ensure_ascii=False)), store)
    d = differ.check_shapes(expected)
    if d:
        return "(in EXPECTED fixture) " + d.render()
    try:
        actual = differ.loads_raw(stdout)
    except ValueError as e:
        return "frame output is not JSON (%s): %s" % (e, stdout)
    d = differ.check_shapes(actual)
    if d:
        return d.render()
    d = differ.compare(expected, actual)
    if d:
        return d.render()
    return None


def describe(step, i, store_label):
    data = stdin_bytes(step)
    preview = data[:120].decode("utf-8", "replace")
    if len(data) > 120:
        preview += "...(%d bytes)" % len(data)
    flags = "".join(
        " [%s]" % f for f in ("no_store", "corrupt_store") if step.get(f))
    return ("step %-2d LEGEND_NOW=%s LEGEND_STATE_DIR=%s legend %s%s\n"
            "        stdin: %s"
            % (i, step.get("now"), store_label, step.get("verb"), flags,
               preview if data else "(empty)"))


def run_fixture(args):
    with open(args.fixture, "r", encoding="utf-8") as f:
        fixture = json.load(f)
    steps = fixture.get("steps", [])
    name = fixture.get("name", os.path.basename(args.fixture))
    print("fixture %s: %d step(s)" % (name, len(steps)))

    if args.dry_run:
        for i, step in enumerate(steps):
            label = "<mkdtemp:empty>" if step.get("no_store") else "<mkdtemp>"
            print(describe(step, i, label))
        print("dry-run: nothing spawned")
        return 0

    store = tempfile.mkdtemp(prefix="legend-fixture-")
    scratch = []
    failed = 0
    try:
        for i, step in enumerate(steps):
            verb = step.get("verb")
            if verb not in VERBS:
                print("step %d: bad verb %r" % (i, verb))
                return 2
            step_store = store
            if step.get("no_store"):
                step_store = tempfile.mkdtemp(prefix="legend-nostore-")
                scratch.append(step_store)
            if step.get("corrupt_store"):
                corrupt_store(step_store)
            proc, wall_ms, rss = spawn(args.legend, verb, stdin_bytes(step),
                                       step_store, step.get("now", 0))
            failure = check_step(step, proc, step_store)
            asserted = "asserted" if step.get("expect") is not None else "prelude"
            status = "ok" if failure is None else "FAIL"
            print("step %-2d %-6s %-8s %-4s %7.1fms maxrss=%dKiB"
                  % (i, verb, asserted, status, wall_ms, rss))
            if failure:
                print(failure)
                failed += 1
                break  # store state is unreliable past a failed step
    finally:
        if args.keep:
            print("store kept at %s" % store)
        else:
            shutil.rmtree(store, ignore_errors=True)
            for d in scratch:
                shutil.rmtree(d, ignore_errors=True)
    if failed:
        print("fixture %s: FAIL" % name)
        return 1
    print("fixture %s: clean" % name)
    return 0


def load_probes(path):
    """Probe file -> {after_line: [(group, index, probe_entry), ...]}, in
    file order per checkpoint. Every probe payload must be observe: true —
    measurement never trains the store (spec §3.1/§13)."""
    with open(path, "r", encoding="utf-8") as f:
        doc = json.load(f)
    groups = ("current_state", "recall_hits", "cold_caller", "orientation",
              "absent", "deep_history", "exclusion", "options")
    by_line = {}
    for group in groups:
        for idx, entry in enumerate(doc.get(group, [])):
            payload = entry["probe"]["payload"]
            if payload.get("observe") is not True:
                raise SystemExit("probe %s[%d] is not observe: true" % (group, idx))
            by_line.setdefault(entry["after_line"], []).append((group, idx, entry))
    return by_line


def fire_probes(args, probes_at, line_no, now, store, results):
    for group, idx, entry in probes_at.get(line_no, ()):
        probe = entry["probe"]
        proc, wall_ms, _ = spawn(args.legend, probe["verb"],
                                 json.dumps(probe["payload"], ensure_ascii=False,
                                            separators=(",", ":")).encode("utf-8"),
                                 store, now)
        out = proc.stdout.decode("utf-8", "replace").strip()
        try:
            frame = json.loads(out)
        except ValueError:
            frame = None
        results.append({"group": group, "index": idx,
                        "after_line": entry["after_line"],
                        "exit": proc.returncode, "frame": frame,
                        "raw": None if frame is not None else out})
        sys.stderr.write("probe %-13s[%d] after_line=%d exit=%d %7.1fms\n"
                         % (group, idx, entry["after_line"], proc.returncode,
                            wall_ms))


def run_replay(args):
    with open(args.replay, "r", encoding="utf-8") as f:
        lines = [ln for ln in f.read().splitlines() if ln.strip()]
    if args.dry_run:
        for i, ln in enumerate(lines):
            step = json.loads(ln)
            print(describe(step, i, "<mkdtemp>"))
        print("dry-run: nothing spawned")
        return 0
    probes_at = load_probes(args.probes) if args.probes else {}
    probe_results = []
    if args.store:
        store = args.store
        os.makedirs(store, exist_ok=True)
    else:
        store = tempfile.mkdtemp(prefix="legend-replay-")
    total_ms = 0.0
    worst_rss = 0
    rc = 0
    try:
        # the corpus is save/recall traffic only; the store is created here
        first_now = json.loads(lines[0]).get("now", 0) if lines else 0
        proc, _, _ = spawn(args.legend, "init", b"", store, first_now)
        if proc.returncode != 0:
            sys.stderr.write("replay: init failed: %s\n"
                             % proc.stdout.decode("utf-8", "replace"))
            return 1
        for i, ln in enumerate(lines):
            step = json.loads(ln)
            proc, wall_ms, rss = spawn(args.legend, step.get("verb"),
                                       stdin_bytes(step), store,
                                       step.get("now", 0))
            total_ms += wall_ms
            worst_rss = max(worst_rss, rss)
            sys.stdout.write(proc.stdout.decode("utf-8", "replace").strip()
                             + "\n")
            sys.stderr.write("line %-4d %-6s exit=%d %7.1fms\n"
                             % (i, step.get("verb"), proc.returncode, wall_ms))
            if proc.returncode != 0:
                rc = 1
            fire_probes(args, probes_at, i + 1, step.get("now", 0), store,
                        probe_results)
    finally:
        if args.store or args.keep:
            sys.stderr.write("store kept at %s\n" % store)
        else:
            shutil.rmtree(store, ignore_errors=True)
    if args.probe_results:
        with open(args.probe_results, "w", encoding="utf-8") as f:
            json.dump(probe_results, f, ensure_ascii=False, indent=1)
            f.write("\n")
        sys.stderr.write("probe results: %d probe(s) -> %s\n"
                         % (len(probe_results), args.probe_results))
    sys.stderr.write("replay: %d line(s), %.1fms total, maxrss=%dKiB\n"
                     % (len(lines), total_ms, worst_rss))
    return rc


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--legend", required=True,
                    help="path to the legend binary under test")
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--fixture", help="fixture JSON (payload stream)")
    mode.add_argument("--replay", help="corpus JSONL to replay")
    ap.add_argument("--dry-run", action="store_true",
                    help="print would-be invocations without spawning")
    ap.add_argument("--keep", action="store_true",
                    help="keep the temp store dir for inspection")
    ap.add_argument("--probes",
                    help="probe annotations JSON; fired at their after_line "
                         "checkpoints during --replay")
    ap.add_argument("--probe-results",
                    help="write collected probe frames to this JSON file")
    ap.add_argument("--store",
                    help="replay into this store dir (created, kept) instead "
                         "of a temp dir")
    args = ap.parse_args(argv)
    if (args.probes or args.probe_results or args.store) and not args.replay:
        ap.error("--probes/--probe-results/--store apply to --replay only")
    if not args.dry_run and not (os.path.isfile(args.legend)
                                 and os.access(args.legend, os.X_OK)):
        ap.error("--legend %s is not an executable file" % args.legend)
    if args.fixture:
        return run_fixture(args)
    return run_replay(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
