#!/bin/sh
# check.sh — the WP0/M0 gate (v3_c_plan.md §8): strict build + ASan/UBSan
# build, unit tests, then end-to-end error-path checks against ./legend.
# Every store used is a fresh temp dir; the repo's own .legend is never touched.
set -u
cd "$(dirname "$0")"

CC="${CC:-cc}"
CFLAGS="-std=c99 -Wall -Wextra -Werror"  # no -ffast-math, ever (plan §1: §3.7 needs strict IEEE)
SAN="-fsanitize=address,undefined,float-cast-overflow -fno-sanitize-recover=undefined,float-cast-overflow -fno-omit-frame-pointer"
ROOT="$(pwd)"
BUILD_ID="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
STAMP=-DLEGEND_BUILD=\"$BUILD_ID\"   # journal + init report record the building sha

fails=0
fail() { echo "FAIL: $*"; fails=$((fails+1)); }
pass() { echo "  ok: $*"; }

echo "== build =="
# -lm: S7's stability soft cap uses tanh (pin 6)
$CC $CFLAGS $STAMP -O2 legend.c      embed.c -o legend      -lm || { echo "FATAL: legend build failed"; exit 1; }
$CC $CFLAGS $STAMP -O2 legend_test.c embed.c -o legend_test -lm || { echo "FATAL: legend_test build failed"; exit 1; }
$CC $CFLAGS $STAMP -g -O1 $SAN legend.c      embed.c -o legend.asan      -lm || { echo "FATAL: legend asan build failed"; exit 1; }
$CC $CFLAGS $STAMP -g -O1 $SAN legend_test.c embed.c -o legend_test.asan -lm || { echo "FATAL: legend_test asan build failed"; exit 1; }
pass "4 binaries built with -Werror"

# Embeddings are ON by default in the binary (committed blob). The core gates
# (fixtures, smoke determinism, fuzz) test embeddings-independent logic and must
# stay fast + model-agnostic, so disable there; only the adversarial gate runs
# with embeddings ON (LEGEND_EMBED=1) to exercise + pin the semantic retrieval.
export LEGEND_EMBED=0

echo "== unit tests =="
./legend_test      || fail "unit tests"
./legend_test.asan || fail "unit tests under ASan/UBSan"

echo "== end-to-end error paths =="
TMPROOT="$(mktemp -d)"
trap 'rm -rf "$TMPROOT"' EXIT

# no store -> no_store JSON on stdout, nonzero exit
out="$(printf '{"focus":["x"]}' | LEGEND_STATE_DIR="$TMPROOT/nowhere" ./legend recall)"
rc=$?
if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q '"code":"no_store"'; then
    pass "no store -> no_store (exit $rc)"
else
    fail "no store: exit=$rc out=$out"
fi

# init from a scratch cwd (generated configs land beside the store): the
# first run creates .mcp.json + .claude/settings.json, and init is then
# idempotent -- the second and third reports must be byte-identical
S1="$TMPROOT/store1"
out0="$(LEGEND_STATE_DIR="$S1" ./legend init)"; rc0=$?
out1="$(LEGEND_STATE_DIR="$S1" ./legend init)"; rc1=$?
out2="$(LEGEND_STATE_DIR="$S1" ./legend init)"; rc2=$?
if [ $rc0 -eq 0 ] && [ $rc1 -eq 0 ] && [ $rc2 -eq 0 ] && [ "$out1" = "$out2" ] \
   && printf '%s' "$out0" | grep -q '"mcp_config_created":true' \
   && printf '%s' "$out0" | grep -q '"hooks_created":true' \
   && printf '%s' "$out1" | grep -q '"hooks_created":false' \
   && printf '%s' "$out1" | grep -q "\"store\":\"$S1\"" \
   && printf '%s' "$out1" | grep -q '"version":1' \
   && printf '%s' "$out1" | grep -q '"elements":32,"relations":10,"clock":0' \
   && [ -s "$TMPROOT/.mcp.json" ] && [ -s "$TMPROOT/.claude/settings.json" ] \
   && python3 -c 'import json,sys; json.load(open(sys.argv[1])); json.load(open(sys.argv[2]))' \
        "$TMPROOT/.mcp.json" "$TMPROOT/.claude/settings.json"; then
    pass "init: configs created beside the store once, then idempotent (32 ontology elements)"
else
    fail "init: rc0=$rc0 rc1=$rc1 rc2=$rc2 out0=$out0 out1=$out1 out2=$out2"
fi
# every invocation journals: 3 inits so far -> 3 ok init lines
if [ "$(grep -c '"verb":"init","ok":true' "$S1/journal.jsonl" 2>/dev/null)" = 3 ]; then
    pass "init journals one line per invocation"
else
    fail "init journal: $(cat "$S1/journal.jsonl" 2>/dev/null)"
fi

# oversize stdin -> limit_exceeded before parsing
out="$(head -c 70000 /dev/zero | LEGEND_STATE_DIR="$S1" ./legend save)"
rc=$?
if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q '"code":"limit_exceeded"'; then
    pass "oversize stdin -> limit_exceeded"
else
    fail "oversize stdin: exit=$rc out=$out"
fi

# malformed JSON -> parse
out="$(printf '{oops' | LEGEND_STATE_DIR="$S1" ./legend save)"
rc=$?
if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q '"code":"parse"'; then
    pass "malformed JSON -> parse"
else
    fail "malformed JSON: exit=$rc out=$out"
fi

# unknown payload field -> parse with at (plan §3.1)
out="$(printf '{"focuss":["x"]}' | LEGEND_STATE_DIR="$S1" ./legend save)"
rc=$?
if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q '"at":"focuss"'; then
    pass "unknown field -> parse with at"
else
    fail "unknown field: exit=$rc out=$out"
fi

# empty stdin -> parse
out="$(printf '' | LEGEND_STATE_DIR="$S1" ./legend save)"
rc=$?
if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q '"code":"parse"'; then
    pass "empty stdin -> parse"
else
    fail "empty stdin: exit=$rc out=$out"
fi

# a focus-less recall is the orientation frame: overview object, no focus key
S2="$TMPROOT/store2"
LEGEND_STATE_DIR="$S2" ./legend init >/dev/null || fail "init store2"
out="$(printf '{}' | LEGEND_STATE_DIR="$S2" LEGEND_NOW=1780272000 ./legend recall)"
rc=$?
if [ $rc -eq 0 ] && printf '%s' "$out" | python3 -c '
import json, sys
f = json.load(sys.stdin)
ov = f["overview"]
assert ov["elements"] == 32 and ov["relations"] == 10 and ov["clock"] == 1, ov
assert ov["scope"] is None and ov["active"] == [], ov
assert "focus" not in f
assert f["state"] == [] and f["recent"] == [] and f["related"] == []
'; then
    pass "focus-less recall -> orientation frame with overview"
else
    fail "focus-less recall: exit=$rc out=$out"
fi

# argv payload sugar runs through the same validation
out="$(LEGEND_STATE_DIR="$S2" ./legend save '{"focuss":1}')"
rc=$?
if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q '"code":"parse"'; then
    pass "argv payload sugar -> same validation"
else
    fail "argv sugar: exit=$rc out=$out"
fi

echo "== end-to-end M1: facts round-trip (plan §8) =="
PAYLOAD='{"source":"check","elements":[{"name":"coyote_time","kind":"mechanic","summary":"grace window"}],"facts":[{"s":"player_jump","p":"uses","o":"coyote_time","confidence":0.9,"src":"src/j.rs:1"}]}'

# the same sequence gates both the strict and the ASan/UBSan binaries
for BIN in ./legend ./legend.asan; do
    S3="$TMPROOT/m1$(basename "$BIN")"
    LEGEND_STATE_DIR="$S3" "$BIN" init >/dev/null || fail "$BIN init"

    # save -> a real compact frame that python3 can parse, with the mints listed
    out="$(printf '%s' "$PAYLOAD" | LEGEND_STATE_DIR="$S3" LEGEND_NOW=1780272000 "$BIN" save)"
    rc=$?
    if [ $rc -eq 0 ] && printf '%s' "$out" | python3 -c '
import json, sys
f = json.load(sys.stdin)
assert f["tick"] == 1, f["tick"]
assert f["at"] == "2026-06-01T00:00:00Z", f["at"]
assert f["resolution"] == []
names = [e["name"] for e in f["writes"]["minted_elements"]]
assert "coyote_time" in names and "player_jump" in names, names
assert len(f["writes"]["minted_relations"]) == 1
assert f["writes"]["reused_relations"] == []
assert f["writes"]["retracted"] == [] and f["writes"]["merged"] == []
assert f["near_matches"] == [] and f["conflicts"] == [] and f["template_drift"] == []
'; then
        pass "$BIN save -> compact frame (json.load clean)"
    else
        fail "$BIN save frame: exit=$rc out=$out"
    fi

    # the identical payload again -> dedup reuse with support_count 2
    out="$(printf '%s' "$PAYLOAD" | LEGEND_STATE_DIR="$S3" LEGEND_NOW=1780358400 "$BIN" save)"
    rc=$?
    if [ $rc -eq 0 ] && printf '%s' "$out" | python3 -c '
import json, sys
f = json.load(sys.stdin)
assert f["tick"] == 2, f["tick"]
assert f["writes"]["minted_elements"] == []
assert f["writes"]["minted_relations"] == []
rr = f["writes"]["reused_relations"]
assert len(rr) == 1 and rr[0]["support_count"] == 2 and rr[0]["promoted"] is False, rr
'; then
        pass "$BIN identical save -> reused_relations support_count 2"
    else
        fail "$BIN dedup frame: exit=$rc out=$out"
    fi

    # a dd-corrupted snapshot -> clean snapshot_corrupt, never a crash
    dd if=/dev/zero of="$S3/legend.snapshot" bs=1 count=8 conv=notrunc 2>/dev/null
    out="$(printf '%s' "$PAYLOAD" | LEGEND_STATE_DIR="$S3" "$BIN" save)"
    rc=$?
    if [ $rc -ne 0 ] && [ $rc -lt 126 ] && printf '%s' "$out" | grep -q '"code":"snapshot_corrupt"'; then
        pass "$BIN corrupted snapshot -> snapshot_corrupt (exit $rc)"
    else
        fail "$BIN corrupt snapshot: exit=$rc out=$out"
    fi
done

echo "== end-to-end M2: full write semantics (plan §8) =="
for BIN in ./legend ./legend.asan; do
    S4="$TMPROOT/m2$(basename "$BIN")"
    LEGEND_STATE_DIR="$S4" "$BIN" init >/dev/null || fail "$BIN init"

    # a change flips state cleanly: conflicts stays empty
    out="$(printf '{"changes":[{"target":"jump_height","property":"value","to":"3.5"}]}' \
        | LEGEND_STATE_DIR="$S4" LEGEND_NOW=1780272000 "$BIN" save)"
    rc=$?
    if [ $rc -eq 0 ] && printf '%s' "$out" | python3 -c '
import json, sys
f = json.load(sys.stdin)
assert f["conflicts"] == [], f["conflicts"]
assert len(f["writes"]["minted_relations"]) == 2, f["writes"]  # event + cache
'; then
        pass "$BIN change -> event + cache, conflicts empty"
    else
        fail "$BIN change frame: exit=$rc out=$out"
    fi

    # a contradicting low-confidence change fails the gate -> conflicts entry
    out="$(printf '{"changes":[{"target":"jump_height","property":"value","to":"9.9","confidence":0.1}]}' \
        | LEGEND_STATE_DIR="$S4" LEGEND_NOW=1780358400 "$BIN" save)"
    rc=$?
    if [ $rc -eq 0 ] && printf '%s' "$out" | python3 -c '
import json, sys
f = json.load(sys.stdin)
assert len(f["conflicts"]) == 1, f["conflicts"]
c = f["conflicts"][0]
assert c["property"] == "value" and c["values"] == ["3.5", "9.9"], c
assert len(f["writes"]["minted_relations"]) == 1  # the event only, no flip
'; then
        pass "$BIN contradicting low-confidence change -> conflicts entry, no flip"
    else
        fail "$BIN conflict frame: exit=$rc out=$out"
    fi

    # retract, then re-retract: idempotent, both acknowledged
    out1="$(printf '{"retract":["rel:10"]}' | LEGEND_STATE_DIR="$S4" LEGEND_NOW=1780444800 "$BIN" save)"
    out2="$(printf '{"retract":["rel:10"]}' | LEGEND_STATE_DIR="$S4" LEGEND_NOW=1780531200 "$BIN" save)"
    r1="$(printf '%s' "$out1" | grep -o '"retracted":\[[^]]*\]')"
    r2="$(printf '%s' "$out2" | grep -o '"retracted":\[[^]]*\]')"
    if [ -n "$r1" ] && [ "$r1" = "$r2" ] && printf '%s' "$r1" | grep -q '"rel:10"'; then
        pass "$BIN retract -> re-retract acknowledges identically (incl. cascade)"
    else
        fail "$BIN retract idempotency: out1=$out1 out2=$out2"
    fi

    # merge, then a ref by the folded element's old name resolves to into
    printf '{"elements":[{"name":"colour"},{"name":"color variant"}]}' \
        | LEGEND_STATE_DIR="$S4" LEGEND_NOW=1780618000 "$BIN" save >/dev/null || fail "$BIN merge prep"
    out="$(printf '{"merge":[{"from":"color variant","into":"colour"}]}' \
        | LEGEND_STATE_DIR="$S4" LEGEND_NOW=1780704000 "$BIN" save)"
    printf '%s' "$out" | grep -q '"merged":\[{"from":"color variant","into":"colour"}\]' \
        || fail "$BIN merge echo: $out"
    out="$(printf '{"facts":[{"s":"color variant","p":"uses","o":"pigment"}]}' \
        | LEGEND_STATE_DIR="$S4" LEGEND_NOW=1780790400 "$BIN" save)"
    rc=$?
    if [ $rc -eq 0 ] && printf '%s' "$out" | python3 -c '
import json, sys
f = json.load(sys.stdin)
names = [e["name"] for e in f["writes"]["reused_elements"]]
assert "colour" in names and "color variant" not in names, names
minted = [e["name"] for e in f["writes"]["minted_elements"]]
assert "color variant" not in minted, minted
'; then
        pass "$BIN merged old name resolves through the fold"
    else
        fail "$BIN post-merge resolution: exit=$rc out=$out"
    fi
done

echo "== end-to-end M3: orientation, observe, --pretty (plan §8) =="
for BIN in ./legend ./legend.asan; do
    S5="$TMPROOT/m3$(basename "$BIN")"
    LEGEND_STATE_DIR="$S5" "$BIN" init >/dev/null || fail "$BIN init"
    printf '{"elements":[{"name":"platformer","kind":"project","summary":"a 2D game"},{"name":"no lag","kind":"constraint"}]}' \
        | LEGEND_STATE_DIR="$S5" LEGEND_NOW=1780272000 "$BIN" save >/dev/null || fail "$BIN m3 prelude"

    # orientation: overview carries scope + active; constraint surfaces store-wide
    out="$(printf '{}' | LEGEND_STATE_DIR="$S5" LEGEND_NOW=1780358400 "$BIN" recall)"
    rc=$?
    if [ $rc -eq 0 ] && printf '%s' "$out" | python3 -c '
import json, sys
f = json.load(sys.stdin)
ov = f["overview"]
assert ov["scope"]["name"] == "platformer" and ov["scope"]["kind"] == "project", ov
assert any(e["name"] == "no lag" for e in ov["active"]), ov
assert f["constraints"][0]["name"] == "no lag" and f["constraints"][0]["standing"] == "active"
assert len(f["state"]) == 1  # the current_standing cache
'; then
        pass "$BIN orientation packet (scope, active, store-wide constraints)"
    else
        fail "$BIN orientation: exit=$rc out=$out"
    fi

    # observe: the store file is byte-identical after an observe recall...
    before="$(cksum "$S5/legend.snapshot")"
    printf '{"focus":["platformer"],"observe":true}' \
        | LEGEND_STATE_DIR="$S5" LEGEND_NOW=1780444800 "$BIN" recall >/dev/null || fail "$BIN observe recall"
    printf '{"observe":true}' \
        | LEGEND_STATE_DIR="$S5" LEGEND_NOW=1780444800 "$BIN" recall >/dev/null || fail "$BIN observe orientation"
    after="$(cksum "$S5/legend.snapshot")"
    if [ "$before" = "$after" ]; then
        pass "$BIN observe recall leaves the snapshot byte-identical"
    else
        fail "$BIN observe mutated the store: $before -> $after"
    fi
    # ...and a plain recall advances it
    printf '{"focus":["platformer"]}' \
        | LEGEND_STATE_DIR="$S5" LEGEND_NOW=1780531200 "$BIN" recall >/dev/null || fail "$BIN plain recall"
    after="$(cksum "$S5/legend.snapshot")"
    if [ "$before" != "$after" ]; then
        pass "$BIN non-observe recall advances the store"
    else
        fail "$BIN non-observe recall left the store untouched"
    fi

    # --pretty renders headed sections and exits 0; errors render too
    out="$(printf '{"focus":["platformer"]}' | LEGEND_STATE_DIR="$S5" LEGEND_NOW=1780617600 "$BIN" recall --pretty)"
    rc=$?
    if [ $rc -eq 0 ] && printf '%s' "$out" | grep -q '^focus (1):' \
       && printf '%s' "$out" | grep -q '^tick: ' \
       && printf '%s' "$out" | grep -q 'name=platformer'; then
        pass "$BIN --pretty frame renders headed sections"
    else
        fail "$BIN --pretty frame: exit=$rc out=$out"
    fi
    out="$(printf '{"focuss":1}' | LEGEND_STATE_DIR="$S5" "$BIN" save --pretty)"
    rc=$?
    if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q '^error: parse'; then
        pass "$BIN --pretty error renders human form"
    else
        fail "$BIN --pretty error: exit=$rc out=$out"
    fi
done

echo "== golden fixtures (harness, plan §8 M3 gate: all ten) =="
for FX in f01_worked_example f02_orientation f03_compact f04_pure_recall f05_history_since \
          f06_idempotent f07_new_homonym f08_event_fact f09_errors f10_templates; do
    for BIN in ./legend ./legend.asan; do
        if python3 harness/run.py --fixture "tests/fixtures/$FX.json" --legend "$BIN" >/dev/null 2>&1; then
            pass "$FX via $BIN"
        else
            fail "$FX via $BIN"
            python3 harness/run.py --fixture "tests/fixtures/$FX.json" --legend "$BIN" | tail -12
        fi
    done
done

echo "== corpus smoke replay (plan §8 M4 gate) =="
CORPUS="$TMPROOT/smoke.jsonl"
PROBES="harness/corpus/probes_smoke.json"
if python3 harness/gen_corpus.py --slice smoke -o "$CORPUS" >/dev/null 2>&1; then
    pass "gen_corpus.py compiled the smoke slice"
else
    fail "gen_corpus.py could not compile the smoke slice"
fi

# replay twice under the corpus's pinned LEGEND_NOWs: every payload must
# succeed, the frame streams must be byte-identical modulo the echoed store
# path, and the final snapshots must be byte-identical
for N in 1 2; do
    if python3 harness/run.py --legend ./legend --replay "$CORPUS" \
        --probes "$PROBES" --probe-results "$TMPROOT/probes$N.json" \
        --store "$TMPROOT/rstore$N" > "$TMPROOT/frames$N.txt" 2>/dev/null; then
        pass "replay $N: all 48 payloads + 27 observe probes exited 0"
    else
        fail "replay $N had failing payloads"
    fi
    sed "s|$TMPROOT/rstore$N|<store>|g" "$TMPROOT/frames$N.txt" > "$TMPROOT/norm$N.txt"
done
if cmp -s "$TMPROOT/norm1.txt" "$TMPROOT/norm2.txt"; then
    pass "double replay -> byte-identical frame streams"
else
    fail "double replay frame streams diverge"
fi
if cmp -s "$TMPROOT/rstore1/legend.snapshot" "$TMPROOT/rstore2/legend.snapshot"; then
    pass "double replay -> byte-identical final snapshots"
else
    fail "double replay snapshots diverge"
fi

# inspect.py: the §13 metrics report on the replayed store. Baselines pinned
# in harness/corpus/README.md. The two probes disputed at M4 were resolved by
# converting the rename episodes to rename_to (pin 23): 12/12 required.
if python3 harness/inspect.py --probes "$PROBES" \
    --results "$TMPROOT/probes1.json" --frames "$TMPROOT/frames1.txt" \
    > "$TMPROOT/metrics.json" 2>/dev/null \
   && python3 - "$TMPROOT/metrics.json" <<'EOF'
import json, sys
m = json.load(open(sys.argv[1]))
mm = m["metrics"]
assert m["probes_clean"] == m["probes_fired"] == 27, (m["probes_clean"], m["probes_fired"])
sup = mm["supersession_correctness"]
assert sup["total"] == 12 and sup["hits"] == 12, sup
ret = mm["retrieval"]
assert ret["found"] == ret["expected"] == 12, ret
ori = mm["orientation_quality"]
assert ori["satisfied"] == ori["checks"] == 17, ori
dyn = mm["dynamics"]
assert dyn["active_rank_hits"] == dyn["active_rank_expected"] == 2, dyn
EOF
then
    pass "inspect.py metrics match the pinned smoke baseline"
else
    fail "inspect.py metrics diverge from the pinned smoke baseline"
    cat "$TMPROOT/metrics.json" 2>/dev/null | tail -30
fi

echo "== corpus adversarial replay (pessimistic recall probes) =="
ACORPUS="$TMPROOT/adversarial.jsonl"
APROBES="harness/corpus/probes_adversarial.json"
if python3 harness/gen_corpus.py --slice adversarial -o "$ACORPUS" >/dev/null 2>&1; then
    pass "gen_corpus.py compiled the adversarial slice"
else
    fail "gen_corpus.py could not compile the adversarial slice"
fi
if LEGEND_EMBED=1 python3 harness/run.py --legend ./legend --replay "$ACORPUS" \
    --probes "$APROBES" --probe-results "$TMPROOT/aprobes.json" \
    > "$TMPROOT/aframes.txt" 2>/dev/null; then
    pass "adversarial replay: all 58 payloads + 51 observe probes exited 0 (embeddings on)"
else
    fail "adversarial replay had failing payloads"
fi
# baseline pinned in harness/corpus/README.md; cold_caller stays informational
if python3 harness/inspect.py --probes "$APROBES" \
    --results "$TMPROOT/aprobes.json" --frames "$TMPROOT/aframes.txt" \
    > "$TMPROOT/ametrics.json" 2>/dev/null \
   && python3 - "$TMPROOT/ametrics.json" <<'EOF'
import json, sys
m = json.load(open(sys.argv[1]))
mm = m["metrics"]
assert m["probes_clean"] == m["probes_fired"] == 51, (m["probes_clean"], m["probes_fired"])
sup = mm["supersession_correctness"]
assert sup["hits"] == sup["total"] == 7, sup
ret = mm["retrieval"]
assert ret["found"] == ret["expected"] == 9, ret
ori = mm["orientation_quality"]
assert ori["satisfied"] == ori["checks"] == 18, ori
ab = mm["absent"]
assert ab["false_resolutions"] == 0 and ab["total"] == 8, ab
dh = mm["deep_history"]
assert dh["satisfied"] == dh["checks"] == 10, dh
ex = mm["exclusion"]
assert ex["leaks"] == 0 and ex["checks"] == 4, ex
op = mm["options"]
assert op["satisfied"] == op["checks"] == 12, op
dyn = mm["dynamics"]
assert dyn["active_rank_hits"] == dyn["active_rank_expected"] == 4, dyn
EOF
then
    pass "inspect.py metrics match the pinned adversarial baseline"
else
    fail "inspect.py metrics diverge from the pinned adversarial baseline"
    cat "$TMPROOT/ametrics.json" 2>/dev/null | tail -30
fi

echo "== M5 fuzz gates (plan §8): payload mutation + corrupt snapshots =="
# The fuzz build adds float-cast-overflow: gcc's -fsanitize=undefined omits
# that check, and the formatter-totality bug was invisible without it.
$CC $CFLAGS -g -O1 -fsanitize=address,undefined,float-cast-overflow \
    -fno-sanitize-recover=undefined,float-cast-overflow -fno-omit-frame-pointer \
    legend.c embed.c -o legend.fuzz -lm || fail "legend fuzz build"

# Deterministic seeded slices sized for the gate; the full M5 runs were
# 2x50,000 payload + 2x20,000 snapshot iterations (seeds 20260703 and
# 987654321 — recorded in harness/corpus/README.md). Iteration i draws from
# Random(f"{seed}:{i}"), so the slice verdicts are a strict subset of the
# full runs'. SEED / FUZZ_JOBS / FUZZ_*_ITERS override for reproduction.
FUZZ_JOBS="${FUZZ_JOBS:-4}"
if python3 fuzz/fuzz_payload.py --legend ./legend.fuzz \
       --iters "${FUZZ_PAYLOAD_ITERS:-6000}" --jobs "$FUZZ_JOBS" \
       > "$TMPROOT/fuzz_payload.log" 2>&1; then
    pass "$(tail -1 "$TMPROOT/fuzz_payload.log")"
else
    fail "payload mutation fuzz (plan §4 invariant)"
    tail -12 "$TMPROOT/fuzz_payload.log"
fi
if python3 fuzz/fuzz_snapshot.py --legend ./legend.fuzz \
       --iters "${FUZZ_SNAPSHOT_ITERS:-3000}" --jobs "$FUZZ_JOBS" \
       > "$TMPROOT/fuzz_snapshot.log" 2>&1; then
    pass "$(tail -1 "$TMPROOT/fuzz_snapshot.log")"
else
    fail "corrupt snapshot fuzz (plan §3.11 reader)"
    tail -12 "$TMPROOT/fuzz_snapshot.log"
fi

echo "=="
if [ $fails -ne 0 ]; then
    echo "check.sh: $fails failure(s)"
    exit 1
fi
echo "check.sh: all green"
