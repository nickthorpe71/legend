# Golden fixtures (Track 2, Lane D)

Fixtures are payload *streams* (plan §6): prelude steps build the pre-state, later
steps assert frames. Run with `python3 harness/run.py --fixture tests/fixtures/fNN_*.json --legend <binary>`.

## Format

```json
{ "name": "f03_compact",
  "notes": "optional, ignored by the runner",
  "steps": [
    { "now": 1781222400,
      "verb": "save" | "recall" | "init",
      "payload": { ... } | null,
      "payload_raw": "literal stdin bytes (overrides payload; for parse-error tests)",
      "expect": <frame object> | { "error": { ... } } | null,
      "no_store": true,
      "corrupt_store": true,
      "notes": "optional" } ] }
```

- `now` — unix seconds; the runner exports it as `LEGEND_NOW` (plan §3.8).
- `payload` — sent on stdin as compact JSON, then the pipe is closed (EOF-delimited).
  `null` (or absent) = empty stdin (used by `init`).
- `expect: null` — unasserted prelude step; the runner only requires exit 0.
- `expect: {frame}` — exit must be 0 and stdout must diff clean against the frame
  (harness/diff.py: key-order-insensitive, array order significant, §3.7 number shapes).
- `expect: {"error": {...}}` — exit must be nonzero; only the keys *present* in the
  expected error object are compared (`code`, `at`, `candidates` as authored). Rationale:
  spec §9 pins codes and the envelope, not `message` text.
- `"<store>"` — placeholder string in expected frames; the runner substitutes the actual
  mkdtemp store path before diffing (the store is part of the determinism triple, but its
  path is chosen by the runner).
- `no_store: true` — the runner points `LEGEND_STATE_DIR` at a fresh *empty* mkdtemp for
  this step only (no `init` ever ran there). This is the no-init flag: spec §4 says only
  `legend init` creates a store, so an empty `LEGEND_STATE_DIR` must yield `no_store`.
- `corrupt_store: true` — before spawning, the runner overwrites every regular file in the
  store dir *except* `legend.lock` (plan §S9 orphan-sweep skips it by name) with garbage
  bytes. Must be the last step of its fixture; the store is unusable afterwards.

Each fixture gets a fresh `mkdtemp` store; steps within a fixture share it (except
`no_store` steps). Errors must not advance the clock or mutate the store (spec §9), so
error steps may be interleaved with asserted frames.

## Not fixturable

- `lock_timeout` — requires a concurrent process holding the flock for the full 5s backoff
  window (plan §3.12); timing-dependent and multi-process, not a deterministic replay.
  Covered instead by the in-process unit test at M0 (plan §8).
- `store_full` — requires driving an element/relation/string/tick count to its u32 bound
  (plan §3.11); constructing ~2^32 mints is resource-prohibitive as a fixture.

## Assumptions to reconcile at M0/M1

Every id, tick, and date in these fixtures derives from the assumptions below. They are
Lane D's best reading of the spec + §3 pins; where the implementation lands differently,
fix the assumption here and regenerate the fixture values — the *shapes* are the contract.

1. **Tick numbering**: `init` seeds the ontology at tick 0; the first save/recall is
   tick 1; each non-`observe` invocation advances by exactly 1. Errors never advance.
2. **Seed element ids** — RECONCILED at M2/M3 (ids are 0-based): `init` mints the §12
   vocabulary in its listed order — subject #0 through part_of #16; metas source #17,
   src #18, derived_from #19, supersedes #20, intervened #21; template kinds decision
   #22, constraint #23, question #24, task #25, pointer #26, project #27, person #28,
   event #29, file #30, commit #31. First user mint is #32. (Authored 1-based; every
   fixture id was renumbered.)
3. **Seed relations** — RECONCILED at M2/M3: `expects` relations only, rel:0–rel:9 —
   decision 5 (chose, rejected, reason, about, resolves), constraint 3 (applies_to,
   reason, standing), question 1 (about), task 1 (about); `resolves` attaches to
   question/task as a *target* semantic, not an expects edge, so they seed one each.
   pointer/project/person/event/file/commit seed no expects. First user relation is
   rel:10. (Authored as 12 seed expects; f02's overview count updated 17 → 15.)
4. **Relation mint (id-assignment) order per tick** — RATIFIED by amended pin §3.17 +
   §4: templates(expects) → element attr relations + kind side-effect caches (in
   element order, attrs in schema order) → facts → changes (event then cache) →
   instance_of relations → metas. Still assumed: the instance_of/meta *position* (after
   all base relations) — the spec only pins that they are never listed, and the §7
   example's contiguous rel:733–740 is consistent with either placement.
5. **minted_relations content**: base relations only (facts, change events, `current_*`
   caches, element attr relations, template `expects` relations). instance_of relations
   and metas (source/src/derived_from/supersedes) never appear, per §7 field notes.
   `sources`/`pointers` aggregate over exactly the minted base relations.
6. **source element**: the `source` string mints an element in the metas phase; it is
   never listed in `minted_elements`/`reused_elements` (§7 excludes it from reused; we
   assume the same for minted).
7. **Constraint side-effect**: minting a constraint-kind element mints attr-name element
   `current_<standing-first-name>` = `current_standing`, then value element `active`,
   then the cache relation, during the element-attrs phase.
8. **Date rendering**: `date`/`asserted` fields = UTC calendar date of the tick's
   `LEGEND_NOW` (`YYYY-MM-DD`); frame `at` = RFC3339 UTC `YYYY-MM-DDTHH:MM:SSZ` of this
   invocation's `LEGEND_NOW`. All fixture `now` values are UTC midnights except f01's
   final step (1782942007 = 2026-07-01T21:40:07Z, copied from the spec example).
9. **Similarity scores** — RECONCILED at M3. The pinned tier-2 math: read positions
   (focus) score by max trigram *containment* |Q∩T|/|Q| over the element's names,
   aliases, and summary (≥ 0.6 joins the walk via "lexical", ≥ 0.3 reports as a
   candidate on a miss, candidate list capped at 8); `near_matches` scores by max
   symmetric *Jaccard* over names+aliases only (summaries would false-positive every
   mint whose name appears inside an existing summary — the spec's own example needs
   near_matches [] while its focus resolves through a summary). f01's 0.83 became 1:
   "jump feel" sits verbatim inside the jump_physics summary. Tier-1 exact/homonym
   hits score `1`.
10. **Homonym resolution reporting**: a write-position tier-1 multi-match picks
    prefer-kind-then-last_seen and reports in `resolution` with `via: "homonym"`,
    `score: 1` (the `via` vocabulary beyond `"lexical"` is unpinned).
11. **near_matches scope**: compared against elements that existed *before* this tick
    only (else f01's "raycast ground check" vs "ground check via raycast", both minted
    same tick, would self-report; the spec example shows `[]`). A `new: true` mint is
    also exempt — its tier-1 twin is exact (score 1), but the caller explicitly forced
    the homonym, so reporting it would be noise (f07).
12. **Section dedup**: `recent` excludes `current_*` caches (they live in `state`) and
    relations rendered inside a kind section (decision/constraint/character attrs);
    `related` = live one-hop relations not already placed in any other section.
    RECONCILED at M3, the recent/related split: on a *recall* the whole focus
    neighborhood is recency (f04's part_of fact lands in `recent`); on a *save* recent
    holds the tick's minted base relations plus neighborhood change events, and older
    neighborhood edges stay in `related` (f01/spec §7: rel:512 in `related`). A reused
    relation outside the focus neighborhood surfaces only through
    `writes.reused_relations`.
13. **Relation-entry shape** — RESOLVED by new pin §3.22 (uniform relation-object
    shape): every `state`/`recent`/`related` entry carries exactly
    `{ref, attrs, status, confidence, support_count, date}` (confidence defaults to
    0.7, support_count starts at 1); `history` entries carry exactly
    `{ref, attrs, asserted_at, asserted, superseded_by, superseded_at}`. No per-kind
    omission remains.
14. **`since` filtering**: history entries filter by their *asserted* date; recent
    entries by their date. Boundary is inclusive (date ≥ since).
15. **Idempotent echoes**: re-retracting a Retracted rel:id still lists it in
    `writes.retracted`; re-merging an already-merged pair still echoes
    `writes.merged: [{"from": ..., "into": ...}]` (from/to echoed as submitted).
16. **Orientation `active`** — RECONCILED at M3: ranked focus_success_count desc, then
    last_seen desc, then id desc (activation is the M4 signal; with no focus successes
    yet, f02 degenerates to the authored last_seen/id ranking), capped at 5, excluding
    ontology-seeded elements and attribute-name elements (any element serving as a slot
    name in any relation). Activation joins the ranking at M4; the fixture pins the
    *shape* (focus-shaped entries, `kind`/`summary` omitted when absent).
17. **Orientation `overview` counts** — RECONCILED at M3: `elements`/`relations` are
    arena totals (seeds included, tombstones included); `clock` = the frame's tick.
    `scope` = the lowest-id live element of kind `project`, else null (no payload field
    exists to declare a scope element — noted, not invented).
18. **Unknown-field `at`**: for a top-level unknown key, `at` is the bare key name
    (`"focuss"`); nested unknowns use the path form (`"facts[0].xyz"`).
19. **limit_exceeded `at`**: the offending list's name (`"elements"`).
20. **candidates on ambiguous_ref**: candidate shape {ref, name, kind?, score}, ordered
    score desc then id asc (plan §5).
21. **Templates walk first** — RATIFIED by amended pin §3.17: the payload walk is
    templates → elements → facts → changes → retract/merge, for both writes-array
    ordering and relation-id assignment.
22. **`observe` frames are unfixtured**: tick/`at` semantics of a no-clock-advance frame
    are unpinned; deferred to M4. The M3 invariant is tested outside the fixtures
    (check.sh + unit tests): an observe recall leaves the snapshot byte-identical —
    no clock, no stamps, no reinforcement, no snapshot write; the frame's `tick`
    reports the unadvanced clock.
23. **Multi-value attr rendering**: an attr with several live value relations renders as
    an array in kind sections (§5 shape rule), array order = relation id ascending.
24. **Event-shaped general-form facts**: their `property` slot value is excluded from
    `reused_elements` exactly as a change's `property` is (§7 "folds in here"); as
    relations they render per the uniform pin §3.22 shape like any other entry.

## Spec contradictions found — all four RESOLVED in the spec (2026-07-02)

Lane D flagged these; the spec/plan were amended and confirmed Lane D's reading:

- **`writes.merged`** — resolved: §7 example now carries `"merged": []`; pin §3.22's
  amendment note covers it. Fixtures already emitted it everywhere.
- **`confidence`/`support_count` inconsistency** — resolved by new pin §3.22 (uniform
  relation-object shape); §7 example reconciled. Fixtures updated (assumption 13).
- **Relation id order** — resolved: amended pin §3.17 + §4 pin apply order as
  templates → elements → facts → changes → retract/merge, ids assigned in that order;
  §7 example's id narrative rewritten to match. Fixtures updated (assumption 4).
- **`templates` in the §3.17 walk** — resolved: templates now lead the pinned walk,
  as the fixtures assumed (assumption 21).

## Inventory

| Fixture | Exercises |
|---|---|
| f09_errors | every reachable error code: no_store, parse ×2 (raw + unknown field w/ `at`), unknown_ref, ambiguous_ref w/ candidates, limit_exceeded (65 entries), snapshot_corrupt |
| f03_compact | focus-less save → compact frame (§7): tick/at/store/resolution/writes/near_matches/conflicts/template_drift only |
| f04_pure_recall | recall frame carries `writes` with all-empty arrays (§3.14) |
| f07_new_homonym | `new: true` forced mint; later bare-name write resolves prefer-kind-then-last_seen, reported in `resolution` |
| f06_idempotent | the three §9 no-ops: fact re-submit (support_count 2), re-retract, re-merge |
| f08_event_fact | general-form fact with from+to triggers supersession like a change |
| f02_orientation | focus-less recall: `overview` object (§6), store-wide sections |
| f05_history_since | 4-value supersession chain; history_depth 1/2/null; `since` cutoff on history+recent |
| f10_templates | create-on-save `character` template, same-save instance, multi-value attr, template_drift, kind section |
| f01_worked_example | the §5/§7 example re-based onto a minimal prelude (plan §6); frame matches the spec field-for-field in shape/ordering |
