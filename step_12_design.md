# Step 12 — Assemble Attention Frame

> **Status: complete (v0).** All five phases landed.
> `src/steps/frame.rs` ships `rrf_merge`, `assemble_frame`, and
> `print_step12`. Step 12 runs last in `lib.rs::run()` and produces
> a `ConsciousAttentionFrame` ready for the caller LLM. 18 frame
> tests + 217 lib tests total pass; clippy + fmt clean.
>
> Substrate touch-ups that shipped alongside:
>   - `RelationActivation.is_defeasible: bool` (status mirror)
>   - Step 10 now bumps `focus_success_count` per reinforced
>     relation so RRF has a real second signal (path-reinforced)
>     alongside dense (activation-based).
>
> Smoke verified on the dentist sentence: 8 focused relations,
> 1 supporting_claim (the cache's derived_from meta), RRF scores
> descending monotonically, 17 durable_writes, 2 active_regions
> (entities/events).
>
> Deferred per spec:
>   - **Tantivy BM25 sparse signal** — §15.1 dependency not built.
>   - **Mid-path-insertion replay flag** — §10.3.5 substrate doesn't
>     exist in v0.
>   - **Cross-tick relations** in focused_relations — flow through
>     routing + recent_focus, not the frame's reinforcement set.
>   - **`policy.frame_top_k`** — easy add when needed.
>   - **Uncertainty signal producers in Steps 5/6/8/9** — only
>     Step 4's `route.uncertainty` is threaded for v0.

## 1. What Step 12 is

No model. Two things Step 12 itself does:
1. **`focused_relations` RRF merge** over already-computed signals
   (Reciprocal Rank Fusion, Cormack 2009).
2. **`next_actions` emission** from uncertainty signals raised by
   earlier steps.

Everything else on the frame is **gathered** — earlier steps wrote
into per-tick buffers, Step 12 just packages them.

The frame is a **post-tick snapshot of the focused subgraph**, not
an answer. The calling LLM derives any natural-language response
off `focused_relations` + `supporting_claims` + `history`.

## 2. Inputs

```rust
pub fn assemble_frame(
    input_text: &str,
    hg: &Hypergraph,
    intent: &Intent,
    active_frame: Option<ElementId>,
    route: &RouteResult,
    step8: &Step8Output,
    step9: &Step9Output,
    step10: &Step10Output,
    policy: &Policy,
) -> ConsciousAttentionFrame;
```

`hg` is read-only — Step 12 must not mutate. All structural
mutation belongs to Steps 7-11.

## 3. Field assembly

Field-by-field map of where each comes from:

| Frame field | Source |
|---|---|
| `tick` | `hg.clock` |
| `input_echo` | `input_text.to_string()` |
| `intent` | passed in (Step 1) |
| `active_frame` | passed in (Step 4 / §11.6) — `None` today |
| `active_regions` | `route.active_regions.clone()` |
| `focused_relations` | RRF merge — see §4 |
| `supporting_claims` | for each focused R: walk `meta_relations_by_subject[R]`; filter to entries with `derived_from` or `source` attributes |
| `history` | for each focused R: walk `meta_relations_by_subject[R]` filtered to `supersedes`; collect linked Superseded relations |
| `uncertainty` | per-tick uncertainty buffer (see §6 — need to thread one through) |
| `durable_writes` | `step8.minted_elements.clone()` |
| `superseded` | `step9.superseded.clone()` |
| `next_actions` | derived from `uncertainty` and replay flags (see §5) |

## 4. `focused_relations` — RRF over what we actually have

§11.13 says RRF over three signals (Dense / Sparse / Path-
reinforced). v0 reality:

- **Dense (focus set)**: `step10.reinforced` — the union Step 10
  built. Ranked by `stats.activation` (post-Step-11 decay). This
  is the substrate's primary "what does this tick care about"
  signal.
- **Sparse (BM25)**: needs tantivy from §15.1 — not built. Defer
  to v0.1.
- **Path-reinforced**: spec means "relations whose
  `focus_success_count` was bumped this tick." Step 10 doesn't
  currently bump `focus_success_count` — it bumps `activation` +
  `support_count`. Adding a `focus_success_count` bump in Step 10
  is one line.

v0 RRF: over **two** signals — Dense (activation rank) +
Path-reinforced (focus_success_count rank). RRF with `k=60`:

```text
score(R) = 1/(60 + rank_dense(R)) + 1/(60 + rank_path(R))
```

If a relation only appears in one list, its missing-rank term is
0. RRF degrades gracefully — even with one signal it returns a
sensible ranking (just `1/(60+rank)`).

### Status filtering

Per §11.13:
- `Asserted` + `Entailed` → included in `focused_relations`.
- `Defeasible` → included, but mark `is_defeasible` (add field).
- `Superseded` → excluded from `focused_relations`; lands in
  `history`.
- `Retracted` → excluded entirely.

This requires adding `is_defeasible: bool` to `RelationActivation`:

```rust
pub struct RelationActivation {
    pub relation: RelationId,
    pub activation: f32,    // RRF-fused score
    pub is_defeasible: bool,
}
```

## 5. `next_actions` from uncertainty

§11.13 says Step 12 inspects `uncertainty` and raises:
- `EnqueueReplay { kind }` when this tick triggered a replay job.
- `FollowUpQuery(text)` when `AmbiguousCoref` could be resolved
  cheaply by asking.

v0 mapping:

```text
DiffuseRouting   → EnqueueReplay { kind: BackgroundSweep }
                   (replay should explore why routing dispersed)
UngroundedTime   → no v0 action (chrono parsing deferred)
AmbiguousCoref   → FollowUpQuery("Could you clarify what <pronoun> refers to?")
LowConfidence    → EnqueueReplay { kind: BackgroundSweep }
                   (low-conf extractions benefit from replay)
Contradiction    → FollowUpQuery("This seems to contradict <prior>. Which is correct?")
```

The mid-path-insertion replay flag (§11.6 / §11.7) doesn't exist
in v0 substrate; that arm of `next_actions` is wired but unused.

## 6. Uncertainty buffer

Currently `UncertaintySignal` is an enum on `types.rs`, but **no
per-tick buffer** exists on `Hypergraph` — earlier steps don't
push uncertainty signals anywhere. The `route` struct already
carries one (`route.uncertainty`), and Step 11.6 prints it via
`DiffuseRouting`.

Two paths for v0:

- **(A)** Add `Hypergraph.tick_uncertainty: Vec<UncertaintySignal>`
  with explicit `clear` at tick start. Steps 4-11 push when they
  detect uncertainty.
- **(B)** Pass `step4_uncertainty: &[UncertaintySignal]` through
  to Step 12 directly — no Hypergraph mutation needed.

**Recommendation: (B) for v0**. Only Step 4 actually produces
uncertainty signals today (`route.uncertainty`); Step 5/6/9 could
be wired later without changing the frame contract. Lower
complexity, no buffer-clearing footgun.

Other producers we'd thread in later:
- Step 5/8 `LowConfidence` from extraction confidences below
  `policy.ner_assertion_threshold`.
- Step 9 `Contradiction` when supersession gate failed.
- Step 6 `AmbiguousCoref` when no candidate cleared threshold.

For v0, Step 12 only consumes `route.uncertainty`.

## 7. `supporting_claims` and `history` walk

For each focused relation `R`:

```text
metas_on_R = hg.meta_relations_by_subject[R]
supporting_claims += metas_on_R filtered to attrs containing
                     `derived_from` or `source`
history += metas_on_R filtered to attrs containing `supersedes`,
           plus the actual Superseded R_old referenced
```

Deduplicate the resulting flat `Vec<RelationId>`. Both `supporting_claims`
and `history` are `Vec<ClaimRef>` (= `Vec<RelationId>`); order doesn't
matter for v0 — caller can sort if needed.

## 8. Outputs

`ConsciousAttentionFrame` already exists on `types.rs`. Step 12
just constructs and returns one. No new struct.

Print helper:

```text
attention frame (Step 12)
  tick                  N
  active_frame          <name or None>
  active_regions        K
  focused_relations     M  (top 5 shown below)
  supporting_claims     N
  history               D
  uncertainty           [DiffuseRouting, LowConfidence]
  durable_writes        N
  superseded            S
  next_actions          [EnqueueReplay { BackgroundSweep }]

  top focused (RRF-fused score):
    R<id>  score=0.032  Asserted    <subject> <attr> <value>
    ...
```

## 9. Phased rollout

### Phase 1 — Frame substrate touch-ups

- Add `is_defeasible: bool` to `RelationActivation` (struct change
  with `Default` impl).
- Add `focus_success_count` bump in Step 10's reinforcement loop
  (one-line addition; the field already exists on `MemoryStats`).
- Add `tick_uncertainty: &[UncertaintySignal]` parameter to the
  Step 12 entry (Step 4's `route.uncertainty` is the v0 source).

### Phase 2 — RRF helper

- `src/steps/frame.rs` with `Step12Output` (alias for
  `ConsciousAttentionFrame`) and `rrf_merge` helper.
- `rrf_merge(ranked_lists: &[Vec<RelationId>], k: u32) ->
  Vec<(RelationId, f32)>` — pure function, easy to unit test.
- Tests: RRF score for items in 1, 2, 3 lists; rank-1 in each list
  produces `3/(60+1) ≈ 0.0492`; missing-from-list = 0 contribution.

### Phase 3 — Frame assembly body

- `assemble_frame(...)` reads `step10.reinforced`, ranks by
  activation, ranks by `focus_success_count`, RRFs them, filters
  status, sets `is_defeasible`.
- Walks `meta_relations_by_subject` for each focused R to build
  `supporting_claims` and `history`.
- Maps uncertainty → next_actions per §5 table.
- Tests: status filter excludes Superseded/Retracted; Defeasible
  flagged; supporting_claims walks correctly; uncertainty →
  expected action.

### Phase 4 — Wire into `lib.rs::run()` + `print_step12`

- Call `assemble_frame` after `focus_radius_decay`.
- `print_step12` shows top-5 focused relations + counts.
- Smoke test on the dentist sentence; verify the frame's
  `superseded` matches Step 9's, `durable_writes` matches Step 8's.

### Phase 5 — End-to-end integration test

- Multi-tick scenario: tick 1 mints, tick 2 supersedes; frame
  should show:
  - `focused_relations`: tick 2's new cache (Asserted) at top
  - `history`: tick 1's superseded cache
  - `supporting_claims`: tick 2's `derived_from` event
  - `superseded`: tick 1's prior cache RelationId

## 10. Open questions

### Q1. v0 RRF over 2 signals vs 1 signal

If we punt path-reinforced (don't add the `focus_success_count`
bump in Step 10), RRF degenerates to a 1-signal sort by
activation. That's equivalent to just `step10.reinforced` sorted
by `r.stats.activation`. RRF adds no value with 1 signal.

**Recommendation:** add the 1-line `focus_success_count` bump in
Step 10 so RRF has 2 real signals. Path-reinforced will differ
from dense once Step 11's decay differentially weighs them.

### Q2. `top_k` cap on focused_relations?

Step 12 could trim `focused_relations` to a fixed top-K. The
spec doesn't say. For v0, no cap — the caller can slice. Adding
`policy.frame_top_k: u32` later is a one-line change.

### Q3. Should `focused_relations` include relations the caller
explicitly added via `recent_focus` re-mention?

Today, Step 10's reinforcement set is exactly Step 8 mints + Step
9 caches/metas. A relation from a prior tick that's still highly-
activated wouldn't be in this list. Recommendation: v0 ships
"this tick's work" only. Cross-tick focus is implicit through
`recent_focus` and Step 4 routing; the focused_relations list
reflects the substrate Legend changed THIS tick.

## 11. Out of scope (post-v0)

- **Tantivy BM25 sparse signal** — §15.1 dependency.
- **Mid-path-insertion replay flag** — §10.3.5 doesn't exist in
  v0 substrate.
- **Replay-side `next_actions` triggers** — §14.8 background
  sweep wiring.
- **`policy.frame_top_k`** — easy add when needed.
- **Cross-tick relations in focused_relations** — implicit
  through routing today.
- **Score-weighted Defeasible reranking** — v0 keeps Defeasible
  in the list with the flag; caller decides how to use.
