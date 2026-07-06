# Replay corpus (Track 2, Lane E — spec §13, plan §9)

Realistic `save`/`recall` traffic tracing a real project's life: **Alchamancer 2**
(`~/Code/alchamancer2`, 255 commits — alchemy spell system, levels, enemies,
balance tuning, a story engine). Episodes are hand-authored from the actual git
history and design docs — every number, rename, and reason is the commit's own;
timestamps are the commits' real author dates (pin §3.8: the runner exports each
line's `now` as `LEGEND_NOW`).

Build a slice:

```sh
python3 harness/gen_corpus.py --slice smoke -o /tmp/smoke.jsonl [--manifest]
```

`gen_corpus.py` is **not an LLM caller** — it is a curator's assembly tool. It
compiles `episodes/*.json` into corpus JSONL (one
`{"now": <unix>, "verb": "save"|"recall", "payload": {...}}` per line, sorted by
timestamp), validating every payload against the spec §5/§6 schema table:
unknown fields anywhere are hard errors (pin §3.1), 64-entry caps on every list
and attrs map, 64 KiB payload cap, `#<decimal>`/`rel:<decimal>` ref forms
(pin §3.9), refs that normalize (pin §3.2) to empty are rejected, saves need one
non-empty write list. It also cross-checks the slice's probes file (line refs in
range, probes are `observe: true` recalls, cold-caller phrasings are not stored
names).

## Episode file format

One file = one authoring unit (roughly a work session). Steps are merged across
files and sorted by `now`; timestamps must be unique corpus-wide so line numbers
(which probes reference) stay deterministic.

```json
{ "episode": "e09_balance_day",
  "slice": "smoke",                      // smallest slice that includes it: smoke ⊂ dev ⊂ full
  "provenance": ["commit f91f75e ...", "STORY.md"],
  "notes": "free text, ignored by the compiler",
  "steps": [
    { "now": 1782436844, "verb": "save", "payload": { ... }, "notes": "optional" } ] }
```

## Smoke slice — provenance (48 lines: 31 save, 17 recall)

| Episode | Derived from |
|---|---|
| e01_scaffold | e2a0ac0, a6e7ccf, 6aa5f51; README/PRD/PLAN — project element, standing constraints (pure sim core, assets are text, no game engine), review-day decisions; the PRD's 60–90 min run-length fact saved defeasible then **retracted by fact shape** when the review retunes it |
| e02_engine_spine | e0e8291, 9770ba5, 1ea50c1; PRD §5 — engine modules, mana economy (forage cap 2, per-color soft cap 6), `spell` template + starter book |
| e03_field_enemies | 7600861, 1ea50c1 — field/streak with the real x100 mods (150/70), `enemy` template + Hollow/Wraithling/Stoneborn, run loop (max_waves 5) |
| e04_reviews | review commits C1/C6/C8/C10 + E1 — decisions with only the reasons the commits state |
| e05_feel | bc67f83, 1bd71c6, fe23544, 56186d6, b8e14c1, 8bcb0a6; IDEAS.md — starting_mana → 0, the open name-stealing question, melee damage 6/9/4 |
| e06_campaign_story | a6e5789, 285263d, 1569233, adadd8b, b8dcadb, 26a878c, ae4e6e9, f44df22; STORY.md — campaign + story cast; **supersedes starting_mana** (carryover within realm); spell tempering (Planet Splitter 18→11, Inferno 20→14, Eternal Tao full-heal→heal 24); mints `the rival`/`Vex` as a deliberate near-duplicate |
| e07_renames | cd1a9ae, 5ae3f8d, 1794870, 018a9ad, 11d6c6b, 0e4421c — the Divine-Comedy rename pass (6 of 8 renames as aliases + `display_name` changes); the cost pass then targets spells **by their new aliases**; Cleanse cost 1B1R1Y→1G1W with an explicit `from` |
| e08_editor | 000828d, 36f89a2 — level editor, Bark Imp (range 3 as an attr — see warts), **merge** of the rival→Vex |
| e09_balance_day | 151499b, 826c1a0, c64de8c, f91f75e — the flagship chain: enemy hp multiplier unset→2x→**2.4x**, hero HP 300→200, lions move 3→5, Bark Imp range 3→5 and rename → Mischief Maker |

## Probes (`probes_smoke.json`)

Ground truth for the §13 metrics, all probes `observe: true` so measurement
never trains the store. `after_line: N` = run the probe against a store that has
replayed lines 1..N.

- **current_state** (12) — supersession correctness: `(target, property) = value`
  at a checkpoint, including both ends of the starting_mana and hp-multiplier
  chains and two alias-routed targets.
- **recall_hits** (5) — retrieval: focus X → element names that must appear in
  the frame.
- **cold_caller** (8) — resolve-tier quality: focus phrasings that deliberately
  never appear as any stored name/alias (the compiler enforces this), tagged
  far/mid by expected trigram reach; the far ones are the tier-3 case.
- **orientation** (2) — `recall '{}'` essentials: scope, standing constraints,
  the two never-resolved questions, recency of balance-day work.

## How slices grow

- **adversarial (58: 34 save, 24 recall)** — smoke plus e10/e11 and a
  pessimistic probe set; see the section below. Slice ordering is
  smoke ⊂ adversarial ⊂ dev ⊂ full.
- **dev (~500)** — the full 255-commit walk at the same grain: the VFX/absorb
  polish arcs, Purgatory terrace levels, story-point files, the duel ruleset,
  audio, more per-spell balance chains (STORY/ALCHEMY_MATRIX carry dozens), and
  recalls at every real session boundary in the log. Tag episodes `"slice": "dev"`.
- **full (~5k)** — transcript-grain detail: per-file `src` pointers for most
  facts, per-playtest observations, failed-experiment retractions, and heavier
  recall traffic (multiple recalls per save, the way long sessions actually run).

## Adversarial slice — the pessimistic corpus

The smoke slice is optimistic: every recall asks for something the graph
holds, by a name it can find, with default options. The adversarial slice asks
the other questions — for things that were never stored, for things stored
months ago, through every recall option the smoke slice left at its default.

Two episodes on top of smoke:

| Episode | Derived from |
|---|---|
| e10_mana_enemies | d461c8c, 15271d4 — the two real commits after the smoke window, at their real author dates: overworld per-color cap 10→7 (deliberately landing beside the existing `per_color_soft_cap` on the same target), passive refill removed, per-level startmana, Jestoad/Deucler/Fenghu, story-event engine, campaign from the shore, run state across screens, tavern rest. Seeds the `Deucler`/`Deucler's` near-duplicate wart. |
| e11_hiatus_return | simulated — the one departure from real author dates: the same store re-opened ~3 months later. Recall-heavy: orientation on a stale store, `history_depth`/`since` in live traffic, two asks for never-stored things, and one save that re-asserts two e01 facts (dedup reuse after months — a spaced touch). Note the wall gap itself moves no dynamics: decay is tick-based, so "old" here means old *values and stamps*, not decayed activation. |

Four probe groups beyond the smoke four (harness support in gen_corpus.py /
run.py / inspect.py):

- **absent (8)** — focus phrasings for things the graph has never held
  (validated against the stored lexicon *up to the checkpoint*, so a probe can
  ask for something that exists only later — line 48 asks for line 50's
  concept). Pass = every resolution entry `resolved: false`; a confident
  resolution of an absent concept is a hallucinated anchor, and the
  false-resolution rate is the number tier 3 (embeddings) must beat. Traps
  included: the predecessor project ("Alchamancer 1 duel balance"), summary
  adjacency ("save file format" vs story.sav/.lvl), and "the water realm",
  whose top candidate scores 0.62 — one notch above the resolve line.
- **deep_history (5)** — old values behind current ones: the four-month-old
  starting_mana prior, the full hp-multiplier chain, `history_depth: 0`
  suppression, a chain probed through a rename (Mischief Maker's range,
  written under "Bark Imp" — its healed attr relation sits in history in plain
  `{subject, range}` shape, which history_hit accepts alongside the cache
  shape), and the §5 no-fabricated-history rule: a change whose `from` had no
  prior cache must NOT invent a supersession (the old value stays reachable
  through the live change event — asserted by recall_hits instead).
- **exclusion (3)** — dead content stays dead months later: the retracted
  "60-90 minutes" and the merged-away "the rival" must not surface as a name
  or attr value in any content section (summary prose mentions are fine;
  template-kind sections count as content), and superseded values must never
  read as current.
- **options (6)** — `limit` 1/6/null on the hub (typed sections always
  included, recent+related+history ≤ limit) and three `since` windows,
  including the one that pins assertion-date stamping: the September re-assert
  dedup-reused June relations, so `since: 2026-09-01` shows nothing — support
  bumps do not re-date.

The smoke groups also re-fire in old-store form: day-1 values still current at
line 58, the spellbook's day-2 facts retrieved months later (under Riptide,
the post-rename canonical name), "the rival" routing through the merge fold,
cold-caller phrasings against the e10 cast, and orientation probes asserting
the two open questions are *still* open four months on.

Build and run:

```sh
python3 harness/gen_corpus.py --slice adversarial -o adversarial.jsonl
python3 harness/run.py --legend ./legend --replay adversarial.jsonl \
    --probes harness/corpus/probes_adversarial.json --probe-results aprobes.json \
    > aframes.txt
python3 harness/inspect.py --probes harness/corpus/probes_adversarial.json \
    --results aprobes.json --frames aframes.txt
```

### Adversarial baseline (C build, 2026-07-06; gated in check.sh)

All 58 payloads and all 43 probes exit 0; double replay is byte-identical in
frame stream and final snapshot.

| Metric | Baseline |
|---|---|
| Supersession | **7/7** — incl. day-1 run_length and the `overworld_per_color_cap` / `per_color_soft_cap` near-namesake pair, unclobbered |
| Retrieval | **9/9** — incl. from-event-only priors ("300", "10") and the post-rename spellbook |
| Absent integrity | **8/8 clean, 0 false resolutions** — "the water realm" candidate sits at 0.62, the guarded boundary |
| Deep history | **10/10** |
| Exclusion | **4/4** — no retracted/merged/superseded leaks at limit 1000 |
| Options | **12/12** |
| Orientation | **18/18** |
| Dynamics | 4/4 `active_should_rank` |
| Cold caller | far 0/1 (candidate-only); mid 2/5 resolved, 2 candidate-only, 1 absent ("healing at the inn" → tavern rest never reaches 0.3) — the tier-2 reach line, unpinned |
| Dedup | 339 minted, 13 near-dup pairs (rate 0.038): smoke's 8 + 4 new `X`/`current_X` cache shadows + the seeded `Deucler`/`Deucler's` wart |
| Graph | 371 elements / 878 relations / clock 58 |

What the probes taught while being authored (kept as probe semantics):
frames denormalize under the canonical-name-at-checkpoint (spellbook returns
Riptide, not Frost Lance); a `from` on a property with no prior cache mints
the cache with no flip and no history entry (§5) — the prior survives only in
the change event; a change *heals* a same-property plain attr into history in
its plain shape; template-kind instances surface relations in their kind
section in instance shape (`offers` beside `name`, not under `attrs`) — the
inspect.py name walk reads those positions too.

## What the smoke slice does NOT exercise (honest notes)

- **No error-path payloads** — every line is valid by construction (fixtures own
  the §9 error surface; a corpus replay should never error).
- **No `#<id>`/`rel:<id>` refs, no rel-ref retract, no `new: true` homonyms,
  no `intent`, no explicit `event` on changes, no general-form (non-triple)
  facts** — smoke exercises the name-based cold-caller path only.
- **No recall options** — `limit`/`history_depth`/`since` stay default except in
  probes; caps are never stressed (largest list is 6 entries).
- **Dynamics barely move** — 48 ticks over ~19 simulated days is too little for
  decay/spaced-vs-massed conclusions (§13 "dynamics" needs dev scale); the
  orientation `active` expectation is recency-dominated.
- **Deliberate warts, kept**: Bark Imp's range enters as an element attr (line
  39) and is later superseded by a change (line 45) — the attr relation and the
  `current_range` cache then coexist; `the rival`/`Vex` are a seeded
  near-duplicate the merge later folds. These are realistic LLM behavior and the
  graph-health metrics should see them, not have them curated away.
- **Small dedup surface** — ~70 distinct elements; predicate sprawl at this
  scale is anecdote, not measurement.

## Smoke-slice results — M4 baseline (C build, 2026-07-03)

Produced by the check.sh corpus gate:

```sh
python3 harness/gen_corpus.py --slice smoke -o smoke.jsonl
python3 harness/run.py --legend ./legend --replay smoke.jsonl \
    --probes harness/corpus/probes_smoke.json --probe-results probes.json \
    --store STORE > frames.txt
python3 harness/inspect.py --probes harness/corpus/probes_smoke.json \
    --results probes.json --frames frames.txt
```

All 48 payloads and all 27 observe probes exit 0. Double replay under the
pinned `LEGEND_NOW`s is byte-identical in both the frame stream (modulo the
echoed store path) and the final snapshot; a probe-less replay produces the
same snapshot byte-for-byte (observe never trains); the ASan/-O1 binary
reproduces the -O2 stream bit-for-bit.

| Metric (spec §13) | Smoke baseline |
|---|---|
| Dedup quality | 309 minted elements, 8 near-duplicate pairs (rate 0.026) — all 8 are `X`/`current_X` cache-name shadows or the seeded `range`/`ranged` wart, none a true concept twin; 32 distinct predicates |
| Supersession correctness | **12/12** — after the rename-episode resolution below |
| Retrieval hit rate | 12/12 expected elements at default limit 40 |
| Cold-caller resolution | far 0/3 resolved (1 candidate-only) — the tier-3 case, as designed; mid 2/5 resolved (`apple throwing enemy`→Bark Imp, `wave reward spell pick`→draft via summaries), 2 candidate-only, 1 absent (`the big slow tank monster`) |
| Orientation quality | 17/17 checks (scope, constraints, open items, current values, recent changes, active ranking) on both session-boot probes |
| Dynamics ordering | 2/2 `active_should_rank` — balance-day elements top `active` under activation x recency; spaced-vs-massed not measurable at 48 ticks (operator-level coverage in legend_test.c; corpus coverage lands with the dev slice) |
| Graph health | 341 elements / 823 relations / clock 48 (~10 elements, ~26 relations per save); snapshot 105,489 bytes; observed status distribution all-asserted at the probe surface (superseded live only in `history`, as specced) |

**Disputed probes — RESOLVED post-M4.** The two display_name misses were an
episode-style artifact: the renames were hand-composed as alias + `display_name`
change (the episodes predate pin 23), so pin 24 + tier-1 resolution made the
cache value the target element itself, denormalizing under the *old* canonical
name. Resolution: the rename episodes (e07, e09) now use `rename_to` — the
first-class primitive this very authoring friction created — which makes the
new name canonical, so caches and every frame section denormalize under it.
Probe annotations were re-keyed to the canonical-name-at-checkpoint rule
(a probe firing before a rename keys by the old name, after by the new one);
expected *values* were never edited. The corpus now exercises `rename_to`,
and the gate requires 12/12.

M4 concretizations this baseline depends on (candidates for §3 pins):
`active` ranks by `activation / (1 + clock - last_seen)` (ties last_seen desc,
id desc) over elements that carry a summary or sit in the subject slot of a
live non-`expects` relation (leaf values and kinds without summaries never
qualify); orientation `state` is capped to `limit/2` (spec §6 "store-wide,
capped") so `recent`/`related` keep budget room.

## Bake-off numbers — M5 baseline (C build; the Rust column comes later)

Recorded per plan §8 M5. Machine: Linux x86-64 (Arch, kernel 7.0.2),
gcc 15.2.1, `-O2` with the check.sh flags, 2026-07-03.

| Metric | Value |
|---|---|
| `legend.c` total | 6,356 lines (`wc -l`) |
| Stripped binary | 125,576 bytes (`-O2`, `strip`; 137,080 unstripped) |
| Cold start, observe recall (load + frame) | median 2.06 ms, p95 2.90 ms (100 spawns, smoke store) |
| Cold start, plain recall (load + tick + frame + save) | median 2.39 ms, p95 3.66 ms |
| Peak RSS, smoke replay | 2,684 KiB (worst single invocation, a save on the full store; recall 2,540 KiB) |
| Smoke replay wall total | 135.3 ms for the 48 payloads (runner-measured) |
| Snapshot at end of smoke | 105,489 bytes |

Per-section lines between the banner comments (file order; S7 sits before S6):

| S0 | S1 | S2 | S3 | S4 | S5 | S7 | S6 | S8 | S9 | S10 |
|---|---|---|---|---|---|---|---|---|---|---|
| 183 | 293 | 1,012 | 482 | 92 | 310 | 209 | 2,015 | 963 | 460 | 325 |

Latency is a 100-iteration spawn loop timed with `perf_counter_ns` (no
hyperfine on this machine), against the smoke-replayed store, medians
reported. RSS is `wait4` rusage from a minimal fork wrapper — `run.py`'s own
maxrss column reads ~18 MiB for the same calls because the forked Python
interpreter's copy-on-write pages count toward the child's high-water mark
before exec.

### M5 fuzz baseline (fuzz/, plan §8)

Full runs against the ASan/UBSan+float-cast-overflow build, all clean after
the three fixes below: **payload mutation 50,000 iterations × seeds 20260703
and 987654321; corrupt snapshot 20,000 iterations × the same two seeds.**
The check.sh gate replays deterministic 6,000/3,000-iteration slices of the
same seed (iteration i draws from `Random(f"{seed}:{i}")`, so slice verdicts
are a strict subset of the full runs').

Bugs found by fuzzing and fixed, each with a regression in `legend_test.c`:

1. **Invalid UTF-8 passed both string doors** (both targets; first hit by a
   directed probe, then payload it51 seed 20260703 and snapshot it8). Raw
   invalid bytes in a payload string minted elements whose names made every
   frame echoing them invalid JSON text; flipped snapshot string bytes
   reached frames the same way. String tokens and the snapshot string table
   now validate RFC 3629; the bounded escapers truncate by whole sequences.
2. **`fmt_unit_float` was partial** (directed probe while building target B;
   gcc's `-fsanitize=undefined` omits float-cast-overflow, so the gate never
   saw it). A snapshot only proves its stats finite, and a huge/negative
   confidence hit an undefined `double`→`u32` cast — gcc quietly printed
   `0` for 1e300 where Rust's `as` would print `1`. The quantizer now clamps
   to the nearest endpoint.
3. **`rename_to` on a core-vocabulary element bricked the store it saved**
   (payload seed 987654321, it 46287: a field swap grafted `"person"` into a
   rename episode). The save exited 0, then every later invocation died with
   `snapshot_corrupt: ontology element #28 is not person` — the reader
   verifies elements 0..31 by canonical name. Plan-phase `parse` error now.
