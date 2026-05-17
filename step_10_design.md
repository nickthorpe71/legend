# Step 10 — Hebbian + Salience

> **Status: design pass.** Spec source: `tick_pipeline_focus.md §11.11`
> + `new_foundation.md §14.7 / §14.9`. Step 9 (`supersede`) shipped;
> Step 10 is the last "no-model" mutation step before Step 11's
> bounded decay walk.

## 1. What Step 10 is

Pure arithmetic over `MemoryStats` — no model. Three pieces of
substrate maintenance plus one side effect that unblocks coref:

1. **Hebbian activation bump** (bounded Oja rule, asymptotes to 1.0)
   on every Relation produced or reinforced this tick.
2. **Salience computation + bump** with intent-modulated weighting.
3. **Defeasible → Asserted promotion check** for relations that
   have accumulated enough independent evidence within
   `policy.promotion_window_ticks`.
4. **Populate `Hypergraph.recent_focus`** with this tick's focal
   elements + their grammatical-slot binding. This is the side
   effect §11.7 explicitly defers to Step 10 — the empty deque is
   why `resolve_coref` is a stub today (no candidates to score).
   Once Step 10 lands, Step 6 (coref) and Step 4's frame inheritance
   become genuine consumers.

The `bounded_hebbian_bump` / `bounded_hebbian_decay` operators
from §14.9 land here too — Step 10 needs the first, Step 11 the
second, so both go in a shared `src/hebbian.rs` (or `src/math.rs`).

## 2. Inputs

```rust
pub fn hebbian_and_salience(
    hg: &mut Hypergraph,
    step8: &Step8Output,
    step9: &Step9Output,
    intent: &Intent,
    active_frame: Option<ElementId>,
    policy: &Policy,
) -> Step10Output;
```

Step 10 reads:
- `step8.minted_relations` — base relations from Step 8 (the
  reinforcement set).
- `step9.cache_relations` + `step9.meta_relations` — Step 9's
  contribution to the focus set.
- `step9.superseded` — exclude these from positive reinforcement
  (they were just flipped; we don't reinforce flipped state).
- `intent` — the salience formula uses `arousal` and
  `prediction_error` directly even though `policy.salience_multiplier`
  already folds them in (§10.6). v0 reads from policy, not intent.
- `policy.hebbian_rate`, `policy.salience_multiplier`,
  `policy.salience_floor`, `policy.promotion_*`.
- `active_frame` — needed to scope `recent_focus` entries.

## 3. Outputs

```rust
pub struct Step10Output {
    /// Relations whose `activation` got bumped via Oja (the full
    /// reinforcement set minus Step 9's flipped priors).
    pub reinforced: Vec<RelationId>,
    /// Salience bumps applied. Same set as `reinforced` in v0; the
    /// distinction is bookkeeping for future per-stat replay.
    pub salience_bumped: Vec<RelationId>,
    /// Relations that promoted Defeasible → Asserted this tick.
    pub promoted: Vec<RelationId>,
    /// `RecentFocusEntry` records pushed onto `hg.recent_focus`.
    pub focus_pushed: u32,
}
```

Frame's `durable_writes` already reads from Step 8 / Step 9;
Step 10's contribution is purely state mutation on existing
relations + the ring-buffer push. `promoted` is the one new piece
of frame-visible information — Step 12's frame walker may want to
surface "this tick promoted N relations."

## 4. Hebbian activation bump

For each relation `R` in the **reinforcement set**:

```text
reinforcement_set =
    step8.minted_relations
  ∪ step9.cache_relations
  ∪ step9.meta_relations
  \ step9.superseded
```

Apply:

```rust
R.stats.activation = bounded_hebbian_bump(
    R.stats.activation,
    policy.hebbian_rate,
);
```

where:

```rust
pub fn bounded_hebbian_bump(x: f32, rate: f32) -> f32 {
    x + rate * (1.0 - x)
}
```

Per §14.9, this asymptotes to 1.0 from below; never overshoots.
With default policy (`hebbian_rate = 0.0`) this is a no-op — same
gating story as Step 7's drift. Intent-modulation flows in
through `policy.hebbian_rate` (Step 2's adjustment).

**Why one bump per relation, not per pair?** §11.11 phrases the
rule as "for every pair (A, B) of elements that co-occurred in the
focus set this tick, walk to their connecting relation R." A
relation with 5 attributes generates 10 pairs all pointing back at
the same R — bumping 10× would burn through the Oja headroom in a
single tick. Step 10 dedups: each R in the reinforcement set gets
exactly one bump per tick.

**Why exclude Step 9's superseded?** Reinforcing a relation we
just flipped to `Superseded` is paradoxical — supersession says
"this is no longer the current state." Excluding them keeps the
salience signal honest.

## 5. Salience computation + bump

For each `R` in the reinforcement set:

```text
score = policy.salience_floor
      + 1.0  if R carries an exact-value attribute
             (date/number/named-entity-typed value)
      + 1.0  if R was just produced by supersession
             (in step9.cache_relations or step9.meta_relations)
      + 0.5  if R is a user-stated preference
             (active_frame == FRAME_USER AND attribute name is
              a preference-shaped seeded name)
      + 0.5  if R is in the reinforcement set
             (always true for R reaching this step; folds in
              the focus-bearing component)

bump = score * policy.salience_multiplier

R.stats.salience = bounded_hebbian_bump(
    R.stats.salience,
    bump * policy.hebbian_rate,
)
```

`policy.salience_multiplier` already carries
`base_salience + arousal + prediction_error` from §10.6 — emotionally
intense / surprising ticks land bigger bumps automatically.

**Detection helpers:**

- "Exact-value attribute": R has at least one Element-valued
  attribute whose value Element carries `instance_of` =
  weekday | month | time | quantity | person | place | org.
  (Reuses `kind_of` from `build_relations.rs`.)
- "Just produced by supersession": cheap O(1) — `step9.cache_relations
  .contains(&R)` or `step9.meta_relations.contains(&R)`.
  Use a `HashSet<RelationId>` for the lookup.
- "User-stated preference": `active_frame` matches
  `by_name["user"]` AND R has an attribute named `preference`,
  `like`, `prefer`, `want`, etc. v0 ships a small hardcoded
  list (`prefers`, `likes`, `wants`); these are NOT seeded as
  attribute names today, so the check defensively returns
  false when no match.

With default policy (`hebbian_rate = 0.0`), salience bumps are
no-ops. Once tuned, salience floors decay's effect (§14.7) so
preference + supersession-derived relations decay slowly even when
not actively accessed.

## 6. Defeasible → Asserted promotion

Per §11.11, three conditions must all hold:

1. `R.stats.support_count >= policy.promotion_min_count` (default 3)
2. `R.stats.support_diversity >= policy.promotion_min_diversity`
   (default 2)
3. No contradicting relation has been written within the window —
   one `meta_relations_by_object[R]` lookup filtered to `supersedes`
   attribute. If non-empty → blocked.

v0 implementation simplifications:

- **`support_count` bump**: every reinforcement-set relation gets
  `support_count += 1` this tick. That's how counts accumulate.
- **`support_diversity` bump**: in v0, +1 if this tick's source is
  distinct from the relation's prior source set. We don't have
  the per-relation source-tracking infrastructure yet (replay's
  job per §14.8); for v0, **`support_diversity` stays at 0** and
  the promotion check effectively only fires when
  `policy.promotion_min_diversity == 0`. Future work: replay-
  maintained `source_set` per relation.
- **Contradiction check**: walks `meta_relations_by_object[R]`
  filtered to relations whose attribute list contains
  `supersedes`. O(small) per `Defeasible` relation in the
  reinforcement set.

The promotion check runs only on **Defeasible** relations in the
reinforcement set — no full-graph sweep. Promotion in this step
is a focus-driven event.

Side effect: when R promotes, also bump `R.stats.confidence` to
`max(R.stats.confidence, policy.default_conf)` so downstream queries
see the lifted belief strength.

## 7. Populate `recent_focus`

For each newly-minted relation R this tick:
- Find its `subject_attr` slot value. Skip if no subject
  (meta-relations have `target` instead — those don't push focus).
- Construct a `RecentFocusEntry {
    element: subject_value,
    attribute: <slot that bound this entry — defaults to subject_attr>,
    frame: active_frame,
    tick: hg.clock,
  }`.
- Push to the front of `hg.recent_focus`; truncate from the back
  to `policy.recent_focus_capacity` (default 64).

**The `attribute` slot is critical.** Per §11.7's coref design, the
`attribute` lets Centering-style coref distinguish "it (focused as
target)" from "it (focused as actor)". For v0:
- For binary `[subject, X]` relations, `attribute = subject_attr`.
- For n-ary events `[subject: event, target, property, from, to]`,
  push an entry for the **target** (the thing the event happened
  to), with `attribute = target_attr`. The event element itself
  also pushes a subject-bound entry. So a reschedule event pushes
  TWO focus entries: the event (subject-bound) AND the appointment
  (target-bound).

**Deduplication**: don't push duplicate `(element, attribute)`
pairs within the same tick. If three relations all subject-bind
"Sarah", only one entry lands for Sarah-as-subject this tick.

**Recency invariant**: oldest entries fall off the back when the
deque exceeds capacity. Coref scoring (§11.8) walks from front
(most recent) to back; truncation preserves that order.

## 8. Order within the tick

The natural order is:
1. Hebbian activation bumps (touches `stats.activation` only).
2. Salience computation + bumps (reads `stats.activation`?
   No — independent of activation. Order doesn't matter here.).
3. `support_count` increment.
4. Promotion check (reads `support_count`).
5. `recent_focus` push.

Step 5 is independent of 1-4; could run first or last. Putting
it last keeps the loop boring and matches the order in `lib.rs::run`.

## 9. Phased rollout

### Phase 1 — `bounded_hebbian` helpers + module

- New `src/hebbian.rs` with `bounded_hebbian_bump` and
  `bounded_hebbian_decay`. Unit tests covering asymptotic
  behavior, idempotency at `rate = 0`, monotonicity.

### Phase 2 — Hebbian activation bumps

- `src/steps/hebbian.rs` (Step 10's module). Implement the
  reinforcement-set construction and the activation bump.
- `Step10Output` skeleton with `reinforced` populated.
- Tests: reinforcement set composition, default-policy no-op,
  non-zero-rate convergence over many ticks.

### Phase 3 — Salience computation + bump

- Add salience-detection helpers (exact-value, supersession-derived,
  preference-shape).
- Salience bump path under the same Step 10 entry point.
- Tests: each detection branch fires correctly; bump applies
  asymptotically; multiple branches stack additively.

### Phase 4 — Promotion check + support_count

- `support_count += 1` per reinforcement-set relation.
- Defeasible → Asserted promotion with the three-gate check.
- Document the v0 limitation: `support_diversity` always 0 →
  default `promotion_min_diversity = 2` never clears. Recommend
  setting `promotion_min_diversity = 0` in dev or a test fixture
  to exercise the path until replay infrastructure arrives.
- Tests: promotion fires when all gates pass; promotion blocked
  when superseding meta exists; promotion blocked by support_count
  shortfall.

### Phase 5 — Populate `recent_focus`

- Walk Step 8's `minted_relations`; for each, push focus entries
  (subject + target where applicable). Dedup intra-tick. Truncate
  to capacity.
- Tests: capacity bound, dedup, target-vs-subject for n-ary events.

### Phase 6 — Wire into `lib.rs::run()` + `print_step10`

- Call `hebbian_and_salience` after `supersede`.
- Print helper: shows reinforced count, salience bumps, promotion
  count, focus pushes. Per-relation rows for promotions (those
  are the highest-signal events).
- Smoke test against the dentist + rescheduled sentences end-to-end.

### Phase 7 — Integration test

- Two-tick scenario: first tick mints, second tick reinforces.
  Verify `activation` accumulates monotonically with non-zero
  rate, `support_count` accumulates, promotion fires when the
  conditions can be set up (custom policy override for the test).

## 10. Open questions

### Q1. Is `recent_focus` truly a Step 10 concern?

The spec's organization puts focus-related state under §11.11
(Step 10), but `recent_focus` doesn't actually drive any of
Step 10's own math — it's a side-effectful write for downstream
consumers (Step 6 coref, Step 4 frame inheritance). Could be its
own micro-step.

**Recommendation: keep it in Step 10.** No other step has a
natural reason to write it, and splitting it out adds plumbing
without a clear benefit. The unblocking-coref motivation is
recorded in the design.

### Q2. Do we want a global activation-bump cap?

Step 10 bumps every reinforcement-set relation once per tick. A
chatty tick could bump 50+ relations. With non-zero `hebbian_rate`
those all asymptote toward 1.0 — the bounded operator handles it
mathematically, but the *cost* scales linearly. For v0 inputs
(single-sentence ticks), this is bounded to ~30 relations.

**Recommendation: no cap for v0.** Profile after Step 11 lands.

### Q3. `support_diversity` — give up or stub?

The spec says diversity counts "topologically independent source
elements." We don't have the per-relation source set, and replay
is the §14.8 component that maintains it. Two options:

- **Stub at 0**: promotion gate effectively requires
  `promotion_min_diversity == 0` to ever fire. Documents the
  limitation cleanly.
- **Naive count**: bump diversity by 1 each tick the relation is
  reinforced. Too permissive — counts "the same Slack message
  twice" as two sources.

**Recommendation: stub at 0.** Honest about what's missing; v1
replay fills it in.

### Q4. Active-frame for `recent_focus` — defaulting?

Per §9.6 / §11.6, `active_frame` is `Option<ElementId>` and
defaults to `None` when no frame is established. Step 10 just
records whatever Step 4 set. No defaulting in Step 10.

## 11. Out of scope (post-v0)

- **Per-relation source tracking** for `support_diversity` (§14.8
  replay's job).
- **Cross-tick reinforcement** beyond what `bounded_hebbian_bump`
  naturally provides (which is just bump-on-touch).
- **Activation decay outside the focus radius** — that's Step 11.
- **Reinforcement-set membership for relations not minted this
  tick but co-referenced by re-mentioned elements** — would
  require the per-pair (A,B) walk the spec describes. v0 sticks
  to the simpler tick-minted set; replay can sweep wider.
