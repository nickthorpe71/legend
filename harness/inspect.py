#!/usr/bin/env python3
"""Inspection metrics for the Legend v3 replay corpus (spec §13, plan §9).

Consumes the artifacts of a probed replay:

    python3 harness/run.py --legend ./legend --replay smoke.jsonl \
        --probes harness/corpus/probes_smoke.json \
        --probe-results probes.json > frames.txt
    python3 harness/inspect.py --probes harness/corpus/probes_smoke.json \
        --results probes.json --frames frames.txt

and prints one plain JSON metrics report on stdout: spec §13's seven metrics.

Contract shared by every metric:
  - inputs are (a) probe payloads replayed via run.py and (b) their frames,
    plus the corpus replay's own frame stream;
  - EVERY probe payload MUST carry "observe": true (spec §3.1/§13) so that
    measurement never trains the store (run.py enforces this at injection);
  - metrics never open the snapshot directly; they read frames, so they work
    identically against the C and Rust builds.
"""

import os
import sys

# Python auto-prepends this script's directory to sys.path, which makes
# harness/inspect.py shadow the stdlib `inspect` module (argparse needs it).
sys.path = [p for p in sys.path
            if os.path.abspath(p if p else ".") != os.path.dirname(
                os.path.abspath(__file__))]

import argparse  # noqa: E402
import json      # noqa: E402

OBSERVE_REQUIRED = True  # every probe payload carries {"observe": true}

SEED_ELEMENTS = 32   # the S4 core ontology (plan §3.10)
SEED_RELATIONS = 10  # its expects relations

# ---------------------------------------------------------------- helpers

_COLLAPSE = set(b"-_.,;:!?'\"()") | {0x20, 0x09, 0x0A, 0x0D, 0x0B, 0x0C}


def normalize(s):
    """Plan §3.2 byte-level normalization (the binary's algorithm)."""
    out = bytearray()
    pending_space = False
    for b in s.encode("utf-8"):
        if 0x41 <= b <= 0x5A:
            b |= 0x20
        if b in _COLLAPSE:
            pending_space = bool(out)
            continue
        if pending_space:
            out.append(0x20)
            pending_space = False
        out.append(b)
    return bytes(out)


def trigrams(s):
    """Pin §3.19 trigram set over the normalized bytes."""
    b = normalize(s)
    if not b:
        return set()
    if len(b) < 3:
        v = b[0] << 16
        if len(b) == 2:
            v |= b[1] << 8
        return {v}
    return {(b[i] << 16) | (b[i + 1] << 8) | b[i + 2]
            for i in range(len(b) - 2)}


def jaccard(a, b):
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)


# non-ref fields of an instance-shaped entry ({ref, name, <attr>: <value>, ...} —
# the denormalized form typed/template-kind sections use); every other key's
# value is an element name
INSTANCE_META = {"ref", "summary", "date", "status", "confidence",
                 "support_count", "salience", "score", "via", "at",
                 "asserted", "asserted_at", "superseded_by", "superseded_at"}


def walk_names(node, out):
    """Collect every element name a frame surfaces: `name` fields, the values
    of `attrs` objects, and the attribute values of instance-shaped entries
    (template-kind sections denormalize relations into instance shape, so
    `offers: "tavern rest"` sits beside `name`, not under `attrs`)."""
    if isinstance(node, dict):
        instance = "ref" in node and "name" in node
        for key, val in node.items():
            if key == "name" and isinstance(val, str):
                out.add(val)
            elif key == "attrs" and isinstance(val, dict):
                for v in val.values():
                    if isinstance(v, str):
                        out.add(v)
                    elif isinstance(v, list):
                        out.update(x for x in v if isinstance(x, str))
            elif instance and key not in INSTANCE_META and isinstance(val, str):
                out.add(val)
            elif instance and key not in INSTANCE_META and isinstance(val, list):
                out.update(x for x in val if isinstance(x, str))
                walk_names([x for x in val if not isinstance(x, str)], out)
            else:
                walk_names(val, out)
    elif isinstance(node, list):
        for v in node:
            walk_names(v, out)


def frame_names(frame):
    out = set()
    walk_names(frame, out)
    return out


def probe_frame(results, group, index):
    for r in results:
        if r["group"] == group and r["index"] == index:
            return r["frame"]
    return None


def state_hit(frame, target, prop, expect):
    """True iff the frame's `state` carries the cache
    {subject: target, current_<prop>: expect}."""
    for entry in frame.get("state", []):
        attrs = entry.get("attrs", {})
        if (attrs.get("subject") == target
                and attrs.get("current_" + prop) == expect):
            return True
    return False


# ---------------------------------------------------------------- metrics

def dedup_quality(frames):
    """Element count vs distinct concepts; near-duplicate name rate
    (trigram over the minted-name population); predicate sprawl (distinct
    attribute-name elements over the relation population). Spec §13 bullet 1."""
    minted = {}  # ref -> name
    predicates = set()
    for f in frames:
        for e in f.get("writes", {}).get("minted_elements", []):
            minted[e["ref"]] = e["name"]

        def rels(node):
            if isinstance(node, dict):
                if "attrs" in node and isinstance(node["attrs"], dict):
                    predicates.update(node["attrs"].keys())
                for v in node.values():
                    rels(v)
            elif isinstance(node, list):
                for v in node:
                    rels(v)
        rels(f)
    names = sorted(minted.items())
    grams = {ref: trigrams(name) for ref, name in names}
    near = []
    for i in range(len(names)):
        for j in range(i + 1, len(names)):
            ra, na = names[i]
            rb, nb = names[j]
            score = jaccard(grams[ra], grams[rb])
            if score >= 0.6 and normalize(na) != normalize(nb):
                near.append({"a": na, "b": nb, "score": round(score, 2)})
    return {
        "minted_elements": len(minted),
        "near_duplicate_pairs": len(near),
        "near_duplicate_rate": round(len(near) / max(1, len(minted)), 4),
        "near_duplicates": near[:20],
        "distinct_predicates": len(predicates),
    }


def supersession_correctness(results, probes):
    """Current-state probes at checkpoints: for each (target, property) in
    the annotated ground truth, the frame's `state` cache must carry the
    expected value. Spec §13 bullet 2."""
    failures = []
    total = 0
    for idx, p in enumerate(probes.get("current_state", [])):
        total += 1
        frame = probe_frame(results, "current_state", idx)
        if frame is None or not state_hit(frame, p["target"], p["property"],
                                          p["expect"]):
            failures.append({"index": idx, "target": p["target"],
                             "property": p["property"], "expect": p["expect"]})
    return {"total": total, "hits": total - len(failures),
            "failures": failures}


def retrieval(results, probes):
    """Recall probes with expected subgraphs; hit rate at the default
    limit=40 cap. Spec §13 bullet 3."""
    per_probe = []
    found_total = expected_total = 0
    for idx, p in enumerate(probes.get("recall_hits", [])):
        frame = probe_frame(results, "recall_hits", idx)
        names = frame_names(frame) if frame else set()
        expected = p["expect_elements"]
        missing = [n for n in expected if n not in names]
        found_total += len(expected) - len(missing)
        expected_total += len(expected)
        per_probe.append({"index": idx, "expected": len(expected),
                          "missing": missing})
    return {"expected": expected_total, "found": found_total,
            "hit_rate": round(found_total / max(1, expected_total), 4),
            "per_probe": per_probe}


def cold_caller_resolution(results, probes):
    """The same project recalled in a different naming style: resolution hit
    rate per tier band — the measurement that decides whether resolve tier 3
    (Rust-only embeddings) earns its keep. Spec §13 bullet 4."""
    bands = {}
    for idx, p in enumerate(probes.get("cold_caller", [])):
        band = "far" if p.get("notes", "").startswith("far") else "mid"
        frame = probe_frame(results, "cold_caller", idx)
        outcome = "absent"
        for entry in (frame or {}).get("resolution", []):
            if entry.get("resolved") is not False:
                if entry.get("name") == p["expect_resolves_to"]:
                    outcome = "resolved"
                elif outcome == "absent":
                    outcome = "resolved_other"
            else:
                cands = [c.get("name") for c in entry.get("candidates", [])]
                if p["expect_resolves_to"] in cands and outcome == "absent":
                    outcome = "candidate_only"
        b = bands.setdefault(band, {"total": 0, "resolved": 0,
                                    "candidate_only": 0, "missed": []})
        b["total"] += 1
        if outcome == "resolved":
            b["resolved"] += 1
        else:
            if outcome == "candidate_only":
                b["candidate_only"] += 1
            b["missed"].append({"index": idx,
                                "phrase": p["probe"]["payload"]["focus"][0],
                                "wanted": p["expect_resolves_to"],
                                "outcome": outcome})
    return bands


def orientation_quality(results, probes):
    """Fresh-session `recall {}` probes: does the orientation frame surface
    standing constraints, open items, current values, and the genuinely
    most-active elements without a follow-up call? Spec §13 bullet 5."""
    per_probe = []
    sat_total = check_total = 0
    for idx, p in enumerate(probes.get("orientation", [])):
        frame = probe_frame(results, "orientation", idx) or {}
        exp = p["expect"]
        failures = []
        checks = 0

        def check(ok, what):
            nonlocal checks
            checks += 1
            if not ok:
                failures.append(what)

        if "scope" in exp:
            scope = frame.get("overview", {}).get("scope") or {}
            check(scope.get("name") == exp["scope"], "scope=" + exp["scope"])
        cons = [c.get("name") for c in frame.get("constraints", [])]
        for want in exp.get("constraints_include", []):
            check(want in cons, "constraint:" + want)
        opens = [o.get("name") for o in frame.get("open", [])]
        for want in exp.get("open_include", []):
            check(want in opens, "open:" + want)
        for tgt, prop, val in exp.get("current_state_include", []):
            check(state_hit(frame, tgt, prop, val),
                  "state:%s.%s=%s" % (tgt, prop, val))
        recent = frame_names({"recent": frame.get("recent", [])})
        for want in exp.get("recent_changes_include", []):
            check(want in recent, "recent:" + want)
        active = [a.get("name")
                  for a in frame.get("overview", {}).get("active", [])]
        for want in exp.get("active_should_rank", []):
            check(want in active, "active:" + want)
        sat_total += checks - len(failures)
        check_total += checks
        per_probe.append({"index": idx, "checks": checks,
                          "failures": failures})
    return {"checks": check_total, "satisfied": sat_total,
            "per_probe": per_probe}


def dynamics(results, probes):
    """Do frequently-recalled relations outrank stale ones; do spaced touches
    outlast massed ones (the §3.1 stability rule)? Spec §13 bullet 6. At smoke
    scale (48 ticks) the measurable slice is the orientation `active` ranking —
    activation x recency putting the recently-tuned elements on top; the
    spaced-vs-massed span needs the dev slice (corpus README) and is asserted
    at operator level by legend_test.c."""
    rank_checks = []
    for idx, p in enumerate(probes.get("orientation", [])):
        wants = p["expect"].get("active_should_rank")
        if not wants:
            continue
        frame = probe_frame(results, "orientation", idx) or {}
        active = [a.get("name")
                  for a in frame.get("overview", {}).get("active", [])]
        rank_checks.append({"index": idx, "active": active,
                            "expected": wants,
                            "hits": [w for w in wants if w in active]})
    ranked = sum(len(c["hits"]) for c in rank_checks)
    expected = sum(len(c["expected"]) for c in rank_checks)
    return {
        "active_rank_expected": expected,
        "active_rank_hits": ranked,
        "rank_checks": rank_checks,
        "spaced_vs_massed": None,
        "spaced_vs_massed_note": ("not measurable at smoke scale; "
                                  "operator-level coverage in legend_test.c, "
                                  "corpus coverage lands with the dev slice"),
    }


# frame keys that are retrieval machinery or provenance, not asserted content;
# everything else — including template-kind sections like `enemy` — is content
MACHINERY_SECTIONS = {"tick", "at", "store", "resolution", "writes",
                      "near_matches", "conflicts", "template_drift",
                      "overview", "pointers", "sources"}


def absent_integrity(results, probes):
    """Probes whose focus names something the graph has never held: every
    resolution entry must come back resolved: false (candidates are fine —
    they ARE the search results). A confident resolution of an absent concept
    is a hallucinated anchor: the frame then orients the caller around the
    wrong element. The false-resolution rate is the tier-2 lexical resolver's
    honest error bar — and the measurement tier 3 (embeddings) must beat."""
    per_probe = []
    false_resolutions = 0
    for idx, p in enumerate(probes.get("absent", [])):
        frame = probe_frame(results, "absent", idx)
        phrase = p["probe"]["payload"]["focus"][0]
        if frame is None:
            per_probe.append({"index": idx, "phrase": phrase, "outcome": "error"})
            false_resolutions += 1
            continue
        resolved_to = [entry.get("name") for entry in frame.get("resolution", [])
                       if entry.get("resolved") is not False]
        candidates = [c.get("name") for entry in frame.get("resolution", [])
                      for c in entry.get("candidates", [])]
        if resolved_to:
            false_resolutions += 1
            per_probe.append({"index": idx, "phrase": phrase,
                              "outcome": "false_resolution", "resolved_to": resolved_to})
        else:
            per_probe.append({"index": idx, "phrase": phrase, "outcome": "clean",
                              "candidates": candidates[:5]})
    total = len(per_probe)
    return {"total": total, "clean": total - false_resolutions,
            "false_resolutions": false_resolutions,
            "false_resolution_rate": round(false_resolutions / max(1, total), 4),
            "per_probe": per_probe}


def history_hit(frame, target, prop, value):
    """True iff the frame's `history` carries the superseded value — either
    the cache shape {subject: target, current_<prop>: value} or a healed
    plain relation {subject: target, <prop>: value} (a change supersedes any
    live same-property attr alongside the prior cache, spec §5)."""
    for entry in frame.get("history", []):
        attrs = entry.get("attrs", {})
        if attrs.get("subject") != target:
            continue
        if attrs.get("current_" + prop) == value or attrs.get(prop) == value:
            return True
    return False


def deep_history(results, probes):
    """Old values behind current ones: the current cache must hold AND each
    annotated prior must sit in `history` at the requested history_depth
    (expect_history_empty asserts depth 0 / since really suppress it)."""
    per_probe = []
    sat_total = check_total = 0
    for idx, p in enumerate(probes.get("deep_history", [])):
        frame = probe_frame(results, "deep_history", idx) or {}
        failures = []
        checks = 1
        if not state_hit(frame, p["target"], p["property"], p["expect_current"]):
            failures.append("current:%s.%s=%s" % (p["target"], p["property"],
                                                  p["expect_current"]))
        for value in p.get("expect_history", []):
            checks += 1
            if not history_hit(frame, p["target"], p["property"], value):
                failures.append("history:%s.%s=%s" % (p["target"], p["property"], value))
        if p.get("expect_history_empty"):
            checks += 1
            if frame.get("history"):
                failures.append("history not empty (%d entries)" % len(frame["history"]))
        sat_total += checks - len(failures)
        check_total += checks
        per_probe.append({"index": idx, "checks": checks, "failures": failures})
    return {"checks": check_total, "satisfied": sat_total, "per_probe": per_probe}


def exclusion_integrity(results, probes):
    """Dead content must stay dead: retracted values, merged-away names, and
    superseded caches must not surface in the frame's content sections (the
    retrieval machinery — resolution, near_matches — may still name them;
    asserting them as knowledge is the failure)."""
    per_probe = []
    leak_total = check_total = 0
    for idx, p in enumerate(probes.get("exclusion", [])):
        frame = probe_frame(results, "exclusion", idx) or {}
        content = {k: v for k, v in frame.items() if k not in MACHINERY_SECTIONS}
        names = frame_names(content)
        leaks = []
        checks = 0
        for name in p.get("forbid_names", []):
            checks += 1
            if name in names:
                leaks.append("name:" + name)
        for target, prop, value in p.get("forbid_state", []):
            checks += 1
            if state_hit(frame, target, prop, value):
                leaks.append("state:%s.%s=%s" % (target, prop, value))
        leak_total += len(leaks)
        check_total += checks
        per_probe.append({"index": idx, "checks": checks, "leaks": leaks})
    return {"checks": check_total, "clean": check_total - leak_total,
            "leaks": leak_total, "per_probe": per_probe}


def options_correctness(results, probes):
    """The §6 recall options under stress: `limit` must bound the untyped
    relation arrays (typed sections are always included; recent/related/
    history fill the remaining budget, so their union never exceeds limit)
    without starving the typed sections, and `since` must window `recent`
    by assertion date."""
    per_probe = []
    sat_total = check_total = 0
    for idx, p in enumerate(probes.get("options", [])):
        frame = probe_frame(results, "options", idx) or {}
        failures = []
        checks = 0

        def check(ok, what):
            nonlocal checks
            checks += 1
            if not ok:
                failures.append(what)

        if "max_untyped" in p:
            untyped = (len(frame.get("recent", [])) + len(frame.get("related", []))
                       + len(frame.get("history", [])))
            check(untyped <= p["max_untyped"],
                  "untyped %d > %d" % (untyped, p["max_untyped"]))
        for section in p.get("expect_sections_nonempty", []):
            check(bool(frame.get(section)), "empty:" + section)
        if p.get("expect_recent_empty"):
            check(not frame.get("recent"),
                  "recent not empty (%d entries)" % len(frame.get("recent", [])))
        if p.get("expect_recent_nonempty"):
            check(bool(frame.get("recent")), "recent empty")
        recent_names = frame_names({"recent": frame.get("recent", [])})
        for want in p.get("expect_recent_include", []):
            check(want in recent_names, "recent missing:" + want)
        for unwanted in p.get("expect_recent_exclude", []):
            check(unwanted not in recent_names, "recent leaked:" + unwanted)
        sat_total += checks - len(failures)
        check_total += checks
        per_probe.append({"index": idx, "checks": checks, "failures": failures})
    return {"checks": check_total, "satisfied": sat_total, "per_probe": per_probe}


def graph_health(frames, results):
    """Orphan-ish elements, status-lattice distribution, store growth per
    save. Spec §13 bullet 7 — measured from frames only: the final
    orientation probe supplies the arena totals, the probe frames supply the
    observed status distribution, and the replay stream supplies the minted
    population (elements a probe never surfaced are the orphan proxy)."""
    last_overview = {}
    for r in results:
        f = r.get("frame") or {}
        if "overview" in f:
            last_overview = f["overview"]
    statuses = {}

    def count_status(node):
        if isinstance(node, dict):
            if "status" in node and isinstance(node["status"], str):
                statuses[node["status"]] = statuses.get(node["status"], 0) + 1
            for v in node.values():
                count_status(v)
        elif isinstance(node, list):
            for v in node:
                count_status(v)
    for r in results:
        count_status(r.get("frame") or {})
    minted = {}
    saves = 0
    for f in frames:
        w = f.get("writes", {})
        if (w.get("minted_elements") or w.get("minted_relations")
                or w.get("reused_relations") or w.get("retracted")
                or w.get("merged")):
            saves += 1
        for e in w.get("minted_elements", []):
            minted[e["ref"]] = e["name"]
    observed = set()
    for r in results:
        observed |= frame_names(r.get("frame") or {})
    unseen = sorted(name for ref, name in minted.items()
                    if name not in observed)
    elements = last_overview.get("elements", 0)
    relations = last_overview.get("relations", 0)
    return {
        "elements": elements,
        "relations": relations,
        "clock": last_overview.get("clock", 0),
        "elements_per_save": round((elements - SEED_ELEMENTS) / max(1, saves), 2),
        "relations_per_save": round((relations - SEED_RELATIONS) / max(1, saves), 2),
        "status_distribution_observed": statuses,
        "minted_never_probed": len(unseen),
        "minted_never_probed_names": unseen[:20],
    }


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--probes", required=True,
                    help="probe annotations JSON (ground truth)")
    ap.add_argument("--results", required=True,
                    help="probe frames JSON from run.py --probe-results")
    ap.add_argument("--frames", required=True,
                    help="replay frame stream (run.py --replay stdout)")
    args = ap.parse_args(argv)

    with open(args.probes, "r", encoding="utf-8") as f:
        probes = json.load(f)
    with open(args.results, "r", encoding="utf-8") as f:
        results = json.load(f)
    frames = []
    with open(args.frames, "r", encoding="utf-8") as f:
        for ln in f:
            ln = ln.strip()
            if ln:
                frames.append(json.loads(ln))

    report = {
        "slice": probes.get("slice"),
        "corpus_lines": len(frames),
        "probes_fired": len(results),
        "probes_clean": sum(1 for r in results if r["exit"] == 0
                            and r["frame"] is not None),
        "metrics": {
            "dedup_quality": dedup_quality(frames),
            "supersession_correctness": supersession_correctness(results, probes),
            "retrieval": retrieval(results, probes),
            "cold_caller_resolution": cold_caller_resolution(results, probes),
            "orientation_quality": orientation_quality(results, probes),
            "dynamics": dynamics(results, probes),
            "graph_health": graph_health(frames, results),
        },
    }
    # adversarial-slice groups: reported only when the probes file carries them,
    # so the pinned smoke report keeps its exact shape
    for group, fn in (("absent", absent_integrity), ("deep_history", deep_history),
                      ("exclusion", exclusion_integrity), ("options", options_correctness)):
        if probes.get(group):
            report["metrics"][group] = fn(results, probes)
    print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
