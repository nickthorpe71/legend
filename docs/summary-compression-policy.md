# Summary compression policy (#24)

**Recorded:** 2026-04-24

Closes queue item #24: "Summary compression policy post
label/gist/evidence split". The post-split roles are defined in
commit `64f4ce0`:

- `label` — compact index/display handle (never compressed).
- `gist: Option<String>` — semantic extractive meaning of the
  consolidated group (never compressed; single string).
- `source_texts: Vec<String>` — evidence preserved from the
  consolidated source group. **This is the field the policy governs.**
- `full_text: Option<String>` — richer summary text (≤ 500 chars,
  already bounded by `SUMMARY_FULL_TEXT_MAX_LEN`).
- `coverage: SummaryCoverage` — tracks `source_count`,
  `evidence_count`, `omitted_source_count`, and
  `full_evidence_preserved` so callers can tell when the Summary is no
  longer a complete index.

## Pipeline

On every consolidation that creates or merges into a Summary node:

1. Clean semantic noise from the source texts.
2. **Exact-match dedup** (preserve first-occurrence order).
3. **Entity-pair compaction** — group observations by the (subject,
   object) proper-noun pair they mention. Groups of ≥
   `DUPLICATE_EVIDENCE_MIN_GROUP` (= 2) collapse into one
   `"<subject> / <object>: N supporting observations"` line.
4. **Hard cap** — if the compacted list still has more than
   `MAX_EVIDENCE_PER_SUMMARY` (= 24 = 2× `L3_EVIDENCE_LOAD_TARGET`)
   entries, drop the tail and record the delta as
   `coverage.omitted_source_count`.
5. `coverage.full_evidence_preserved` becomes `false` as soon as
   omission happens (either from exact-match dedup, compaction, or
   the cap).

All steps live in `compact_summary_source_texts` at
`src/memory/mod.rs:2583`.

## Why this policy

- **Exact-match dedup** is the cheapest possible compression and the
  least lossy. Always on.
- **Entity-pair compaction** preserves *distinct* observations and
  collapses surface variations of the same fact. Typed relations in
  `update_graph` already carry the reinforcement signal, so keeping
  every verbal restatement in evidence would duplicate information
  (and inflate retrieval cost) without adding facts.
- **Hard cap** prevents heavily-reinforced Summary nodes from
  accumulating unbounded evidence over a long session. 2× the load
  target gives the pressure signal room to function while bounding
  the worst case.
- **Coverage tracking** lets downstream callers (consolidation
  pressure, future retrieval UIs) detect when a Summary is
  incomplete. Today we surface `evidence_count` in the
  `SystemsConsolidation` trace; a future audit command could use the
  same field to flag over-compressed Summaries for re-consolidation.

## What this policy is NOT

- Not a replacement for the gist. The gist already carries the
  semantic meaning; evidence carries the source observations. Policy
  never touches gist.
- Not a quality-weighted prune. Tail drop is first-occurrence-order,
  not salience-weighted. Upgrading to a salience- or diversity-weighted
  prune is a future enhancement if evidence turnover becomes visible
  in retrieval regressions.
- Not applied outside Summary nodes. `source_texts` on non-Summary
  nodes (Keyword, Term, Type, etc.) stays untouched — those are bound
  already by the kind's own size norms.

## When to revisit

Three conditions would re-open this policy:

1. **`omitted_source_count` consistently > `evidence_count`.** That
   means the cap is dropping more than it keeps; time to raise the
   cap or add salience-weighted retention.
2. **Retrieval regressions on old facts.** If a Summary loses an
   observation the retrieval layer needed, the cap is too aggressive.
   Either raise it or add a "pinned evidence" mechanism.
3. **Label/gist contents become the source of truth in some
   retrieval path** (e.g. a future concept-extraction pass reads
   `gist` directly). Then a policy for compressing `gist` would live
   here too — currently unnecessary because gist is a single string.

## Related

- #19 (chunking evaluation): relation extraction over-generates,
  which would push noisy source_texts into Summary nodes. The cap
  bounds that fallout while extraction cleanup lands.
- `docs/latency-budgets.md` (auto-consolidation): cap keeps
  consolidation cost bounded as graph grows.
- Commit `81e45bc` (fixture leak removal): earlier special-case
  compaction for project_alpha/sqlite was removed; this doc defines
  the honest generalized replacement.
