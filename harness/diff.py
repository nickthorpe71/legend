#!/usr/bin/env python3
"""Structural frame differ for the Legend v3 conformance harness (plan §9).

Rules (spec §5 determinism + plan §3.7/§3.14):
  - dict comparison is key-order-insensitive;
  - array order is SIGNIFICANT;
  - numbers are compared by their literal source text (byte-identity is the
    product contract), via a parse hook that never converts to float;
  - fields named "score" or "confidence" must match ^(0|1|0\\.\\d\\d?)$;
    every other number must be a plain integer literal;
  - the first divergence is reported as a JSON pointer plus both values.

Importable API:
    loads_raw(text) -> obj            # numbers become Num (str subclass)
    compare(expected, actual) -> Divergence | None
    check_shapes(obj) -> Divergence | None
    diff_texts(expected_text, actual_text) -> Divergence | None

CLI:
    diff.py EXPECTED.json ACTUAL.json    # exit 0 clean, 1 divergent, 2 usage
    diff.py --self-test
"""

import json
import re
import sys

SCORE_RE = re.compile(r"^(0|1|0\.\d\d?)$")
INT_RE = re.compile(r"^(0|-?[1-9][0-9]*)$")
SCORE_KEYS = ("score", "confidence")


class Num(str):
    """A JSON number kept as its literal source text (never a float)."""


def loads_raw(text):
    """json.loads that preserves number literals as Num strings."""
    return json.loads(text, parse_float=Num, parse_int=Num,
                      parse_constant=Num)


class Divergence:
    """kind: value | missing | extra | length | type | shape"""

    def __init__(self, kind, pointer, expected=None, actual=None, note=""):
        self.kind = kind
        self.pointer = pointer
        self.expected = expected
        self.actual = actual
        self.note = note

    def __repr__(self):
        return "Divergence(%s, %s)" % (self.kind, self.pointer)

    def render(self):
        def show(v):
            if isinstance(v, Num):
                return str(v)
            return json.dumps(v, ensure_ascii=False, default=str)
        lines = ["%s at %s" % (self.kind.upper(), self.pointer or "/")]
        if self.note:
            lines.append("  note:     %s" % self.note)
        if self.kind != "shape":
            lines.append("  expected: %s" % show(self.expected))
            lines.append("  actual:   %s" % show(self.actual))
        else:
            lines.append("  value:    %s" % show(self.actual))
        return "\n".join(lines)


def _escape(token):
    """RFC 6901 token escaping."""
    return str(token).replace("~", "~0").replace("/", "~1")


def _type_name(v):
    if isinstance(v, Num):
        return "number"
    if isinstance(v, dict):
        return "object"
    if isinstance(v, list):
        return "array"
    if isinstance(v, bool):
        return "bool"
    if isinstance(v, str):
        return "string"
    if v is None:
        return "null"
    return type(v).__name__


def check_shapes(obj, pointer="", key=None):
    """Validate the §3.7 number contract over a loads_raw tree.

    Returns the first violation as a Divergence(kind="shape") or None.
    """
    if isinstance(obj, Num):
        if key in SCORE_KEYS:
            if not SCORE_RE.match(obj):
                return Divergence(
                    "shape", pointer, actual=obj,
                    note='"%s" must match ^(0|1|0\\.\\d\\d?)$' % key)
        elif not INT_RE.match(obj):
            return Divergence(
                "shape", pointer, actual=obj,
                note='non-score/confidence numbers must be integers')
        return None
    if isinstance(obj, dict):
        for k, v in obj.items():
            d = check_shapes(v, pointer + "/" + _escape(k), k)
            if d:
                return d
        return None
    if isinstance(obj, list):
        for i, v in enumerate(obj):
            # elements inherit the nearest enclosing key (e.g. an array of
            # scores would still be score-shaped; frames don't have one today)
            d = check_shapes(v, pointer + "/" + str(i), key)
            if d:
                return d
    return None


def compare(expected, actual, pointer=""):
    """First divergence between two loads_raw trees, or None."""
    te, ta = _type_name(expected), _type_name(actual)
    if te != ta:
        return Divergence("type", pointer, te, ta)
    if isinstance(expected, dict):
        for k in expected:
            p = pointer + "/" + _escape(k)
            if k not in actual:
                return Divergence("missing", p, expected[k], None)
            d = compare(expected[k], actual[k], p)
            if d:
                return d
        for k in actual:
            if k not in expected:
                return Divergence("extra", pointer + "/" + _escape(k),
                                  None, actual[k])
        return None
    if isinstance(expected, list):
        if len(expected) != len(actual):
            return Divergence("length", pointer, len(expected), len(actual),
                              note="array order is significant")
        for i, (e, a) in enumerate(zip(expected, actual)):
            d = compare(e, a, pointer + "/" + str(i))
            if d:
                return d
        return None
    # Num compares as its literal text; str/bool/None compare directly.
    if expected != actual:
        return Divergence("value", pointer, expected, actual)
    return None


def diff_texts(expected_text, actual_text):
    """Parse both sides, validate shapes on both, then compare."""
    expected = loads_raw(expected_text)
    actual = loads_raw(actual_text)
    d = check_shapes(expected)
    if d:
        d.note = "(in EXPECTED fixture) " + d.note
        return d
    d = check_shapes(actual)
    if d:
        return d
    return compare(expected, actual)


# ---------------------------------------------------------------- self-test

_BASE = """{
  "tick": 41, "at": "2026-07-01T21:40:07Z", "store": "/tmp/s/.legend",
  "resolution": [
    {"at": "focus[0]", "submitted": "jump feel", "ref": "#87",
     "name": "jump_physics", "via": "lexical", "score": 0.83}
  ],
  "writes": {
    "minted_elements": [{"ref": "#110", "name": "coyote_time"}],
    "reused_elements": [], "minted_relations": ["rel:733", "rel:734"],
    "reused_relations": [{"ref": "rel:501", "support_count": 2,
                          "promoted": false}],
    "retracted": [], "merged": []
  },
  "near_matches": [], "conflicts": [], "template_drift": [],
  "state": [{"ref": "rel:735", "confidence": 0.7, "support_count": 1}]
}"""

# identical content, different key order inside objects
_REORDERED_KEYS = """{
  "at": "2026-07-01T21:40:07Z", "store": "/tmp/s/.legend", "tick": 41,
  "writes": {
    "reused_elements": [], "retracted": [], "merged": [],
    "minted_relations": ["rel:733", "rel:734"],
    "reused_relations": [{"promoted": false, "ref": "rel:501",
                          "support_count": 2}],
    "minted_elements": [{"name": "coyote_time", "ref": "#110"}]
  },
  "resolution": [
    {"score": 0.83, "at": "focus[0]", "submitted": "jump feel",
     "ref": "#87", "name": "jump_physics", "via": "lexical"}
  ],
  "conflicts": [], "near_matches": [], "template_drift": [],
  "state": [{"support_count": 1, "confidence": 0.7, "ref": "rel:735"}]
}"""


def _self_test():
    failures = []

    def case(name, want_kind, want_pointer, expected_text, actual_text):
        d = diff_texts(expected_text, actual_text)
        got = (d.kind, d.pointer) if d else (None, None)
        ok = got == (want_kind, want_pointer)
        print("%s %s -> %s" % ("PASS" if ok else "FAIL", name, got))
        if not ok:
            failures.append(name)

    # must pass: key order is insensitive
    case("identical/reordered-keys", None, None, _BASE, _REORDERED_KEYS)
    # deliberately mutated value
    case("mutated-ref", "value", "/writes/minted_elements/0/ref",
         _BASE, _BASE.replace('"#110"', '"#111"'))
    # array order is significant
    case("array-reorder", "value", "/writes/minted_relations/0",
         _BASE, _BASE.replace('["rel:733", "rel:734"]',
                              '["rel:734", "rel:733"]'))
    # missing key
    case("missing-key", "missing", "/near_matches",
         _BASE, _BASE.replace('"near_matches": [], ', ""))
    # extra key
    case("extra-key", "extra", "/answer",
         _BASE, _BASE.replace('"tick": 41,', '"tick": 41, "answer": "no!",'))
    # array length
    case("array-length", "length", "/writes/minted_relations",
         _BASE, _BASE.replace('"rel:733", ', ""))
    # score with three decimals violates §3.7
    case("score-shape", "shape", "/resolution/0/score",
         _BASE, _BASE.replace("0.83", "0.830"))
    # confidence printed as 1.0 must be "1"
    case("confidence-1.0", "shape", "/state/0/confidence",
         _BASE, _BASE.replace('"confidence": 0.7', '"confidence": 1.0'))
    # integer field carrying a float
    case("float-tick", "shape", "/tick",
         _BASE, _BASE.replace('"tick": 41', '"tick": 41.0'))
    # bad shape in the EXPECTED side is also caught
    case("expected-side-shape", "shape", "/tick",
         _BASE.replace('"tick": 41', '"tick": 41.5'), _BASE)

    if failures:
        print("self-test: %d case(s) failed: %s"
              % (len(failures), ", ".join(failures)))
        return 1
    print("self-test: all cases behaved as expected")
    return 0


def main(argv):
    if "--self-test" in argv:
        return _self_test()
    if len(argv) != 2:
        sys.stderr.write(
            "usage: diff.py EXPECTED.json ACTUAL.json | --self-test\n")
        return 2
    with open(argv[0], "r", encoding="utf-8") as f:
        expected_text = f.read()
    with open(argv[1], "r", encoding="utf-8") as f:
        actual_text = f.read()
    d = diff_texts(expected_text, actual_text)
    if d:
        print(d.render())
        return 1
    print("clean")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
