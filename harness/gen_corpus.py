#!/usr/bin/env python3
"""Compile hand-authored episode files into replayable corpus JSONL (plan §9).

Not an LLM caller: a curator's assembly tool. Episodes live in
harness/corpus/episodes/*.json — each a hand-authored session of save/recall
payloads derived from the Alchamancer 2 history — and this script:

  1. selects episodes by slice tag (smoke ⊂ dev ⊂ full),
  2. validates every payload against the spec §5/§6 schema table
     (field names, types, ref forms, 64-entry caps, 64 KiB payload cap) —
     rejecting unknown fields exactly like the binary will,
  3. sorts all steps by timestamp and emits one
     {"now": <unix>, "verb": "save"|"recall", "payload": {...}} per line,
  4. cross-checks the slice's probes file (line refs in range, probe payloads
     are valid observe:true recalls, cold-caller phrasings never appear as a
     stored element name or alias).

Usage:
  python3 harness/gen_corpus.py --slice smoke -o /tmp/smoke.jsonl [--manifest]
"""

import os
import sys

# harness/inspect.py shadows the stdlib `inspect` module (which argparse needs
# transitively); this tool is stdlib-only, so drop our own directory from the path.
sys.path = [p for p in sys.path
            if os.path.abspath(p or ".") != os.path.dirname(os.path.abspath(__file__))]

import argparse
import glob
import json
import re

SLICES = {"smoke": 0, "adversarial": 1, "dev": 2, "full": 3}
LIST_CAP = 64
PAYLOAD_CAP = 64 * 1024
REL_RE = re.compile(r"^rel:(0|[1-9][0-9]*)$")
HASH_RE = re.compile(r"^#(0|[1-9][0-9]*)$")
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")

# Byte set from plan §3.2: punctuation run-collapsed with whitespace.
_COLLAPSE = set(b"-_.,;:!?'\"()") | {0x20, 0x09, 0x0A, 0x0D, 0x0B, 0x0C}


def normalize(s):
    """Plan §3.2 byte-level normalization (the algorithm the binary uses)."""
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
    return bytes(out).decode("utf-8", errors="replace")


class Validator:
    """Schema table derived from spec §5 (save) / §6 (recall) + plan §3 pins."""

    def __init__(self):
        self.errors = []

    def err(self, path, msg):
        self.errors.append("%s: %s" % (path, msg))

    # -- scalar checks ------------------------------------------------------

    def ref(self, v, path):
        """A ref position: name, #<id>, or rel:<id>. Non-empty after normalize."""
        if not isinstance(v, str):
            self.err(path, "ref must be a string, got %s" % type(v).__name__)
            return
        if v == "":
            self.err(path, "empty string in ref position")
            return
        if v.startswith("#") and not HASH_RE.match(v):
            self.err(path, "malformed #id ref %r (pin §3.9: #<decimal>)" % v)
        elif v.startswith("rel:") and not REL_RE.match(v):
            self.err(path, "malformed rel ref %r (pin §3.9: rel:<decimal>)" % v)
        elif normalize(v) == "":
            self.err(path, "ref %r normalizes to the empty string" % v)

    def string(self, v, path, nonempty=True):
        if not isinstance(v, str):
            self.err(path, "must be a string")
        elif nonempty and v == "":
            self.err(path, "must be non-empty")

    def unit_float(self, v, path):
        if isinstance(v, bool) or not isinstance(v, (int, float)):
            self.err(path, "must be a number in [0,1]")
        elif not (0.0 <= v <= 1.0):
            self.err(path, "out of range [0,1]: %r" % v)

    def boolean(self, v, path):
        if not isinstance(v, bool):
            self.err(path, "must be a boolean")

    def capped_list(self, v, path):
        if not isinstance(v, list):
            self.err(path, "must be an array")
            return None
        if len(v) > LIST_CAP:
            self.err(path, "%d entries exceeds the 64-entry cap" % len(v))
        return v

    def known_fields(self, obj, allowed, path):
        for key in obj:
            if key not in allowed:
                self.err("%s.%s" % (path, key), "unknown field (hard parse error, pin §3.1)")

    # -- entry shapes (§5) --------------------------------------------------

    def element(self, e, path):
        if not isinstance(e, dict):
            self.err(path, "element must be an object")
            return
        self.known_fields(e, {"name", "kind", "aliases", "summary", "salience",
                              "attrs", "new", "src", "rename_to"}, path)
        if "name" not in e:
            self.err(path, "element requires 'name'")
        else:
            self.ref(e["name"], path + ".name")
        if "kind" in e:
            self.ref(e["kind"], path + ".kind")
        if "aliases" in e:
            lst = self.capped_list(e["aliases"], path + ".aliases")
            for i, a in enumerate(lst or []):
                self.ref(a, "%s.aliases[%d]" % (path, i))
        if "summary" in e:
            self.string(e["summary"], path + ".summary")
        if "salience" in e:
            self.unit_float(e["salience"], path + ".salience")
        if "new" in e:
            self.boolean(e["new"], path + ".new")
        if "rename_to" in e:
            self.ref(e["rename_to"], path + ".rename_to")
        if "src" in e:
            self.string(e["src"], path + ".src")
        if "attrs" in e:
            self.attrs_map(e["attrs"], path + ".attrs", min_slots=1)

    def attrs_map(self, attrs, path, min_slots, max_slots=LIST_CAP):
        if not isinstance(attrs, dict):
            self.err(path, "attrs must be an object")
            return
        if len(attrs) > LIST_CAP:
            self.err(path, "%d attrs exceeds the 64-entry cap" % len(attrs))
        if not (min_slots <= len(attrs) <= max_slots):
            self.err(path, "expected %d..%d slots, got %d" % (min_slots, max_slots, len(attrs)))
        for k, v in attrs.items():
            self.ref(k, "%s.%s (key)" % (path, k))
            if isinstance(v, list):
                lst = self.capped_list(v, "%s.%s" % (path, k))
                for i, item in enumerate(lst or []):
                    self.ref(item, "%s.%s[%d]" % (path, k, i))
            else:
                self.ref(v, "%s.%s" % (path, k))

    def fact_shape(self, f, path, allow_options):
        """Triple sugar {s,p,o} xor general form {attrs}; used by facts and retract."""
        if not isinstance(f, dict):
            self.err(path, "fact must be an object")
            return
        allowed = {"s", "p", "o", "attrs"}
        if allow_options:
            allowed |= {"status", "confidence", "salience", "src"}
        self.known_fields(f, allowed, path)
        triple = [k for k in ("s", "p", "o") if k in f]
        if "attrs" in f:
            if triple:
                self.err(path, "mixes triple sugar (%s) with general-form attrs" % ",".join(triple))
            self.attrs_map(f["attrs"], path + ".attrs", min_slots=2, max_slots=5)
            values = [v for v in f["attrs"].values() if isinstance(v, str)] if isinstance(f["attrs"], dict) else []
            if values and not any(not v.startswith("rel:") for v in values):
                self.err(path, "slots must include at least one element-valued ref (all are rel:)")
        elif len(triple) == 3:
            for k in ("s", "p", "o"):
                self.ref(f[k], "%s.%s" % (path, k))
            if all(isinstance(f[k], str) and f[k].startswith("rel:") for k in ("s", "o")):
                self.err(path, "slots must include at least one element-valued ref")
        else:
            self.err(path, "fact needs s+p+o or attrs (has: %s)" % (",".join(sorted(f)) or "nothing"))
        if allow_options:
            if "status" in f and f["status"] not in ("asserted", "entailed", "defeasible"):
                self.err(path + ".status", "must be asserted|entailed|defeasible, got %r" % f.get("status"))
            if "confidence" in f:
                self.unit_float(f["confidence"], path + ".confidence")
            if "salience" in f:
                self.unit_float(f["salience"], path + ".salience")
            if "src" in f:
                self.string(f["src"], path + ".src")

    def change(self, c, path):
        if not isinstance(c, dict):
            self.err(path, "change must be an object")
            return
        self.known_fields(c, {"target", "property", "to", "from", "event",
                              "intervened", "confidence", "src"}, path)
        for req in ("target", "property", "to"):
            if req not in c:
                self.err(path, "change requires '%s'" % req)
            else:
                self.ref(c[req], "%s.%s" % (path, req))
        if "from" in c:
            self.ref(c["from"], path + ".from")
        if "event" in c:
            self.ref(c["event"], path + ".event")
        if "intervened" in c:
            self.boolean(c["intervened"], path + ".intervened")
        if "confidence" in c:
            self.unit_float(c["confidence"], path + ".confidence")
        if "src" in c:
            self.string(c["src"], path + ".src")

    def retract_item(self, r, path):
        if isinstance(r, str):
            if not REL_RE.match(r):
                self.err(path, "retract string must be exactly rel:<decimal>, got %r" % r)
        elif isinstance(r, dict):
            self.fact_shape(r, path, allow_options=False)
        else:
            self.err(path, "retract entry must be a rel:<id> string or a fact shape")

    def merge_item(self, m, path):
        if not isinstance(m, dict):
            self.err(path, "merge entry must be an object")
            return
        self.known_fields(m, {"from", "into"}, path)
        for req in ("from", "into"):
            if req not in m:
                self.err(path, "merge requires '%s'" % req)
            else:
                self.ref(m[req], "%s.%s" % (path, req))

    def template(self, t, path):
        if not isinstance(t, dict):
            self.err(path, "template must be an object")
            return
        self.known_fields(t, {"kind", "expects", "summary"}, path)
        if "kind" not in t:
            self.err(path, "template requires 'kind'")
        else:
            self.ref(t["kind"], path + ".kind")
        if "expects" not in t:
            self.err(path, "template requires 'expects'")
        else:
            lst = self.capped_list(t["expects"], path + ".expects")
            for i, x in enumerate(lst or []):
                self.ref(x, "%s.expects[%d]" % (path, i))
        if "summary" in t:
            self.string(t["summary"], path + ".summary")

    def intent(self, it, path):
        if not isinstance(it, dict):
            self.err(path, "intent must be an object")
            return
        self.known_fields(it, {"conviction", "prediction_error", "arousal", "curiosity"}, path)
        for k, v in it.items():
            self.unit_float(v, "%s.%s" % (path, k))

    # -- payloads -----------------------------------------------------------

    def save_payload(self, p, path):
        self.known_fields(p, {"source", "templates", "elements", "facts", "changes",
                              "retract", "merge", "intent", "focus"}, path)
        if "source" in p:
            self.string(p["source"], path + ".source")
        for name, fn in (("templates", self.template), ("elements", self.element),
                         ("facts", lambda f, pp: self.fact_shape(f, pp, allow_options=True)),
                         ("changes", self.change), ("retract", self.retract_item),
                         ("merge", self.merge_item)):
            if name in p:
                lst = self.capped_list(p[name], "%s.%s" % (path, name))
                for i, entry in enumerate(lst or []):
                    fn(entry, "%s.%s[%d]" % (path, name, i))
        if "intent" in p:
            self.intent(p["intent"], path + ".intent")
        if "focus" in p:
            lst = self.capped_list(p["focus"], path + ".focus")
            for i, f in enumerate(lst or []):
                self.ref(f, "%s.focus[%d]" % (path, i))
        writes = ("templates", "elements", "facts", "changes", "retract", "merge")
        if not any(isinstance(p.get(w), list) and p[w] for w in writes):
            self.err(path, "save requires at least one non-empty write list")

    def recall_payload(self, p, path):
        self.known_fields(p, {"focus", "limit", "history_depth", "since",
                              "observe", "intent"}, path)
        if "focus" in p:
            lst = self.capped_list(p["focus"], path + ".focus")
            for i, f in enumerate(lst or []):
                self.ref(f, "%s.focus[%d]" % (path, i))
        if "limit" in p and p["limit"] is not None:
            if isinstance(p["limit"], bool) or not isinstance(p["limit"], int) or p["limit"] < 1:
                self.err(path + ".limit", "must be a positive integer or null")
        if "history_depth" in p and p["history_depth"] is not None:
            if isinstance(p["history_depth"], bool) or not isinstance(p["history_depth"], int) or p["history_depth"] < 0:
                self.err(path + ".history_depth", "must be a non-negative integer or null")
        if "since" in p:
            if not isinstance(p["since"], str) or not DATE_RE.match(p["since"]):
                self.err(path + ".since", "must be an ISO date YYYY-MM-DD")
        if "observe" in p:
            self.boolean(p["observe"], path + ".observe")
        if "intent" in p:
            self.intent(p["intent"], path + ".intent")

    def payload(self, verb, p, path):
        if not isinstance(p, dict):
            self.err(path, "payload must be an object")
            return
        raw = json.dumps(p, separators=(",", ":")).encode("utf-8")
        if len(raw) > PAYLOAD_CAP:
            self.err(path, "payload is %d bytes, exceeds 64 KiB cap" % len(raw))
        if verb == "save":
            self.save_payload(p, path)
        else:
            self.recall_payload(p, path)


# -- episode + probe loading ------------------------------------------------

def load_episodes(episodes_dir, slice_name, v):
    """Read every episode file, keep those tagged at or below the slice rank."""
    rank = SLICES[slice_name]
    steps = []
    episodes = []
    paths = sorted(glob.glob(os.path.join(episodes_dir, "*.json")))
    if not paths:
        v.err(episodes_dir, "no episode files found")
        return [], []
    for path in paths:
        name = os.path.basename(path)
        try:
            with open(path) as fh:
                ep = json.load(fh)
        except (OSError, ValueError) as exc:
            v.err(name, "unreadable episode: %s" % exc)
            continue
        if not isinstance(ep, dict):
            v.err(name, "episode must be an object")
            continue
        v.known_fields(ep, {"episode", "slice", "provenance", "notes", "steps"}, name)
        for req in ("episode", "slice", "provenance", "steps"):
            if req not in ep:
                v.err(name, "episode requires '%s'" % req)
        if ep.get("slice") not in SLICES:
            v.err(name, "slice must be one of %s" % "|".join(SLICES))
            continue
        if not isinstance(ep.get("provenance"), list) or not all(isinstance(x, str) for x in ep.get("provenance", [])):
            v.err(name, "provenance must be an array of strings")
        if SLICES[ep["slice"]] > rank:
            continue
        episodes.append(ep.get("episode", name))
        if not isinstance(ep.get("steps"), list) or not ep["steps"]:
            v.err(name, "steps must be a non-empty array")
            continue
        for i, step in enumerate(ep["steps"]):
            spath = "%s.steps[%d]" % (name, i)
            if not isinstance(step, dict):
                v.err(spath, "step must be an object")
                continue
            v.known_fields(step, {"now", "verb", "payload", "notes"}, spath)
            now = step.get("now")
            if isinstance(now, bool) or not isinstance(now, int) or now <= 0:
                v.err(spath + ".now", "must be a positive unix timestamp")
                continue
            verb = step.get("verb")
            if verb not in ("save", "recall"):
                v.err(spath + ".verb", "must be save|recall, got %r" % verb)
                continue
            v.payload(verb, step.get("payload"), spath + ".payload")
            steps.append({"now": now, "verb": verb, "payload": step["payload"],
                          "_ep": ep.get("episode", name), "_i": i})
    steps.sort(key=lambda s: (s["now"], s["_ep"], s["_i"]))
    seen = {}
    for s in steps:
        if s["now"] in seen:
            v.err("%s.steps[%d]" % (s["_ep"], s["_i"]),
                  "duplicate timestamp %d (also in %s) — line order would be ambiguous"
                  % (s["now"], seen[s["now"]]))
        seen[s["now"]] = s["_ep"]
    return steps, episodes


def stored_lexicon(steps, up_to_line=None):
    """Every element name and alias the corpus stores (for cold-caller checks).
    With up_to_line, only saves on lines 1..up_to_line contribute — the absent
    probes need temporal absence (a thing may exist later in the corpus)."""
    names = set()
    for line_no, s in enumerate(steps, 1):
        if s["verb"] != "save":
            continue
        if up_to_line is not None and line_no > up_to_line:
            break
        for e in s["payload"].get("elements", []):
            if isinstance(e, dict):
                if isinstance(e.get("name"), str):
                    names.add(normalize(e["name"]))
                for a in e.get("aliases", []) or []:
                    if isinstance(a, str):
                        names.add(normalize(a))
    return names


def probe_entry(entry, path, v, n_lines, extra_fields):
    if not isinstance(entry, dict):
        v.err(path, "probe entry must be an object")
        return
    v.known_fields(entry, {"after_line", "probe", "provenance", "notes"} | extra_fields, path)
    al = entry.get("after_line")
    if isinstance(al, bool) or not isinstance(al, int) or not (1 <= al <= n_lines):
        v.err(path + ".after_line", "must be a line number in 1..%d, got %r" % (n_lines, al))
    probe = entry.get("probe")
    if not isinstance(probe, dict) or set(probe) - {"verb", "payload"} or probe.get("verb") != "recall":
        v.err(path + ".probe", "must be {verb: recall, payload: {...}}")
        return
    v.payload("recall", probe.get("payload"), path + ".probe.payload")
    if not isinstance(probe.get("payload"), dict) or probe["payload"].get("observe") is not True:
        v.err(path + ".probe.payload", "probes must carry observe:true (§3.1: measurement never trains the store)")


def check_probes(probes_path, slice_name, steps, v):
    try:
        with open(probes_path) as fh:
            probes = json.load(fh)
    except (OSError, ValueError) as exc:
        v.err(probes_path, "unreadable probes file: %s" % exc)
        return None
    p = os.path.basename(probes_path)
    v.known_fields(probes, {"slice", "line_count", "current_state", "recall_hits",
                            "cold_caller", "orientation", "absent", "deep_history",
                            "exclusion", "options"}, p)
    if probes.get("slice") != slice_name:
        v.err(p + ".slice", "probes are for slice %r, generating %r" % (probes.get("slice"), slice_name))
    n = len(steps)
    if probes.get("line_count") != n:
        v.err(p + ".line_count", "declares %r lines but the slice compiles to %d" % (probes.get("line_count"), n))
    for entry_path, extras in (("current_state", {"target", "property", "expect"}),
                               ("recall_hits", {"expect_elements"}),
                               ("cold_caller", {"expect_resolves_to"}),
                               ("orientation", {"expect"}),
                               ("absent", set()),
                               ("deep_history", {"target", "property", "expect_current",
                                                 "expect_history", "expect_history_empty"}),
                               ("exclusion", {"forbid_names", "forbid_state"}),
                               ("options", {"expect_sections_nonempty", "max_untyped",
                                            "expect_recent_empty", "expect_recent_nonempty",
                                            "expect_recent_include", "expect_recent_exclude"})):
        for i, entry in enumerate(probes.get(entry_path, [])):
            probe_entry(entry, "%s.%s[%d]" % (p, entry_path, i), v, n, extras)
    for i, entry in enumerate(probes.get("current_state", [])):
        for req in ("target", "property", "expect"):
            if not isinstance(entry, dict) or not isinstance(entry.get(req), str) or not entry.get(req):
                v.err("%s.current_state[%d].%s" % (p, i, req), "required non-empty string")
    for i, entry in enumerate(probes.get("recall_hits", [])):
        ee = entry.get("expect_elements") if isinstance(entry, dict) else None
        if not isinstance(ee, list) or not ee or not all(isinstance(x, str) and x for x in ee):
            v.err("%s.recall_hits[%d].expect_elements" % (p, i), "required non-empty array of names")
    lexicon = stored_lexicon(steps)
    for i, entry in enumerate(probes.get("cold_caller", [])):
        if not isinstance(entry, dict):
            continue
        if not isinstance(entry.get("expect_resolves_to"), str) or not entry.get("expect_resolves_to"):
            v.err("%s.cold_caller[%d].expect_resolves_to" % (p, i), "required non-empty string")
        payload = entry.get("probe", {}).get("payload", {}) if isinstance(entry.get("probe"), dict) else {}
        for f in payload.get("focus", []) if isinstance(payload.get("focus"), list) else []:
            if isinstance(f, str) and normalize(f) in lexicon:
                v.err("%s.cold_caller[%d]" % (p, i),
                      "focus %r is a stored name/alias — not a cold phrasing" % f)
    for i, entry in enumerate(probes.get("orientation", [])):
        if isinstance(entry, dict) and not isinstance(entry.get("expect"), dict):
            v.err("%s.orientation[%d].expect" % (p, i), "required object of expectations")
        payload = entry.get("probe", {}).get("payload", {}) if isinstance(entry, dict) and isinstance(entry.get("probe"), dict) else {}
        if isinstance(payload, dict) and payload.get("focus"):
            v.err("%s.orientation[%d]" % (p, i), "orientation probes must not carry focus (§6)")
    for i, entry in enumerate(probes.get("absent", [])):
        if not isinstance(entry, dict):
            continue
        payload = entry.get("probe", {}).get("payload", {}) if isinstance(entry.get("probe"), dict) else {}
        focus = payload.get("focus") if isinstance(payload, dict) else None
        if not isinstance(focus, list) or not focus:
            v.err("%s.absent[%d]" % (p, i), "absent probes must carry focus (the phrase that must not resolve)")
            continue
        # temporal absence: the phrase must not be a stored name/alias at the checkpoint
        # (it may exist later in the corpus — that contrast is the point)
        al = entry.get("after_line")
        lexicon_here = stored_lexicon(steps, up_to_line=al if isinstance(al, int) else None)
        for f in focus:
            if isinstance(f, str) and normalize(f) in lexicon_here:
                v.err("%s.absent[%d]" % (p, i),
                      "focus %r is a stored name/alias by line %r — not absent" % (f, al))
    for i, entry in enumerate(probes.get("deep_history", [])):
        if not isinstance(entry, dict):
            continue
        for req in ("target", "property", "expect_current"):
            if not isinstance(entry.get(req), str) or not entry.get(req):
                v.err("%s.deep_history[%d].%s" % (p, i, req), "required non-empty string")
        eh = entry.get("expect_history")
        ehe = entry.get("expect_history_empty")
        if eh is not None and (not isinstance(eh, list) or not eh
                               or not all(isinstance(x, str) and x for x in eh)):
            v.err("%s.deep_history[%d].expect_history" % (p, i), "must be a non-empty array of prior values")
        if ehe is not None and not isinstance(ehe, bool):
            v.err("%s.deep_history[%d].expect_history_empty" % (p, i), "must be a boolean")
        if eh is None and ehe is None:
            v.err("%s.deep_history[%d]" % (p, i), "needs expect_history or expect_history_empty")
        if eh is not None and ehe:
            v.err("%s.deep_history[%d]" % (p, i), "expect_history contradicts expect_history_empty")
    for i, entry in enumerate(probes.get("exclusion", [])):
        if not isinstance(entry, dict):
            continue
        fn = entry.get("forbid_names")
        fs = entry.get("forbid_state")
        if fn is None and fs is None:
            v.err("%s.exclusion[%d]" % (p, i), "needs forbid_names or forbid_state")
        if fn is not None and (not isinstance(fn, list) or not fn
                               or not all(isinstance(x, str) and x for x in fn)):
            v.err("%s.exclusion[%d].forbid_names" % (p, i), "must be a non-empty array of names")
        if fs is not None and (not isinstance(fs, list) or not fs or not all(
                isinstance(t, list) and len(t) == 3
                and all(isinstance(x, str) and x for x in t) for t in fs)):
            v.err("%s.exclusion[%d].forbid_state" % (p, i),
                  "must be a non-empty array of [target, property, value] triples")
    option_sections = {"state", "decisions", "constraints", "open",
                       "recent", "history", "related"}
    for i, entry in enumerate(probes.get("options", [])):
        if not isinstance(entry, dict):
            continue
        expectations = [k for k in ("expect_sections_nonempty", "max_untyped",
                                    "expect_recent_empty", "expect_recent_nonempty",
                                    "expect_recent_include", "expect_recent_exclude")
                        if k in entry]
        if not expectations:
            v.err("%s.options[%d]" % (p, i), "needs at least one expectation field")
        sn = entry.get("expect_sections_nonempty")
        if sn is not None and (not isinstance(sn, list)
                               or not all(isinstance(x, str) and x in option_sections for x in sn)):
            v.err("%s.options[%d].expect_sections_nonempty" % (p, i),
                  "must be an array of section names (%s)" % "|".join(sorted(option_sections)))
        mu = entry.get("max_untyped")
        if mu is not None and (isinstance(mu, bool) or not isinstance(mu, int) or mu < 0):
            v.err("%s.options[%d].max_untyped" % (p, i), "must be a non-negative integer")
        for b in ("expect_recent_empty", "expect_recent_nonempty"):
            if b in entry and not isinstance(entry[b], bool):
                v.err("%s.options[%d].%s" % (p, i, b), "must be a boolean")
        if entry.get("expect_recent_empty") and entry.get("expect_recent_nonempty"):
            v.err("%s.options[%d]" % (p, i), "expect_recent_empty contradicts expect_recent_nonempty")
        for lst in ("expect_recent_include", "expect_recent_exclude"):
            val = entry.get(lst)
            if val is not None and (not isinstance(val, list) or not val
                                    or not all(isinstance(x, str) and x for x in val)):
                v.err("%s.options[%d].%s" % (p, i, lst), "must be a non-empty array of names")
    return probes


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--slice", choices=sorted(SLICES, key=SLICES.get), default="smoke")
    ap.add_argument("-o", "--out", required=True, help="output corpus JSONL path")
    ap.add_argument("--episodes", default=os.path.join(here, "corpus", "episodes"))
    ap.add_argument("--probes", default=None,
                    help="probes file (default: harness/corpus/probes_<slice>.json if present)")
    ap.add_argument("--manifest", action="store_true",
                    help="print line -> (episode, verb, now) mapping")
    args = ap.parse_args()

    v = Validator()
    steps, episodes = load_episodes(args.episodes, args.slice, v)

    probes_path = args.probes or os.path.join(here, "corpus", "probes_%s.json" % args.slice)
    probes = None
    if os.path.exists(probes_path):
        probes = check_probes(probes_path, args.slice, steps, v)
    elif args.probes:
        v.err(probes_path, "probes file not found")

    if v.errors:
        for e in v.errors:
            print("error: %s" % e, file=sys.stderr)
        print("FAIL: %d validation error(s); nothing written" % len(v.errors), file=sys.stderr)
        return 1

    with open(args.out, "w") as fh:
        for s in steps:
            fh.write(json.dumps({"now": s["now"], "verb": s["verb"], "payload": s["payload"]},
                                separators=(",", ":")))
            fh.write("\n")

    if args.manifest:
        for i, s in enumerate(steps, 1):
            print("%3d  %-22s %-6s now=%d" % (i, s["_ep"], s["verb"], s["now"]))

    saves = sum(1 for s in steps if s["verb"] == "save")
    recalls = len(steps) - saves
    print("wrote %s: %d lines (%d save, %d recall) from %d episodes [slice=%s]"
          % (args.out, len(steps), saves, recalls, len(episodes), args.slice))
    if probes is not None:
        counts = ", ".join("%d %s" % (len(probes.get(g, [])), g)
                           for g in ("current_state", "recall_hits", "cold_caller", "orientation",
                                     "absent", "deep_history", "exclusion", "options")
                           if probes.get(g))
        print("probes %s: %s — all valid" % (os.path.basename(probes_path), counts))
    return 0


if __name__ == "__main__":
    sys.exit(main())
