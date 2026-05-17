# Step 11 — Focus-Radius Decay

> **Status: design pass.** Spec source: `tick_pipeline_focus.md §11.12`
> + `new_foundation.md §14.7 / §14.9`. Step 10 (`hebbian + salience`)
> shipped; Step 11 is the last "no-model" mutation step before
> Step 12's frame assembly. `bounded_hebbian_decay` already lives
> in `src/hebbian.rs` from Step 10 Phase 1.

## 1. What Step 11 is

Pure arithmetic over `MemoryStats` — no model. Bounded BFS outward
from this tick's focus set, decaying `stats.activation` on each
element/relation reached. Decay is **utility-modulated**: high-
utility relations decay slowly; sub-radius low-utility ones decay
fast. The bounded radius keeps the tick's latency budget
predictable; everything outside the radius is the background
sweep's job (§14.7, post-v0 replay thread).

Two key spec invariants:
- **Activation only.** `support_count`, `confidence`, `salience`,
  `support_diversity` — none of these decay in Step 11. They're
  promotion-counting and belief-strength signals that should only
  reset via explicit replay or supersession.
- **Status untouched.** Decayed relations stay live; they're just
  harder to retrieve via activation-weighted ranking. `Asserted`,
  `Entailed`, `Defeasible` statuses don't change.

## 2. Inputs

```rust
pub fn focus_radius_decay(
    hg: &mut Hypergraph,
    step8: &Step8Output,
    step9: &Step9Output,
    step10: &Step10Output,
    policy: &Policy,
) -> Step11Output;
```

The focus set is what Steps 8/9/10 just touched — the union of
`minted_relations`, `cache_relations`, and Step 10's `reinforced`
list. The seed element set for BFS is the **subjects and
object-elements** of those relations.

## 3. Outputs

```rust
pub struct Step11Output {
    /// Elements reached during the BFS walk (deduped).
    pub elements_walked: u32,
    /// Relations whose `stats.activation` was decayed this tick.
    pub relations_decayed: u32,
    /// Maximum BFS depth actually traversed (≤ focus_decay_radius).
    pub max_depth_reached: u32,
}
```

Counts only — Step 11 doesn't surface IDs because the work is
diffuse (a chatty tick can walk hundreds of relations). Frame's
observability reads the counts; per-relation introspection lives
on the relations themselves.

## 4. BFS construction

The seed set:
1. Walk `step8.minted_relations ∪ step9.cache_relations ∪
   step9.meta_relations ∪ step10.reinforced` (deduped).
2. For each relation, collect every `Term::Element(e)` value from
   its attribute list. That's the BFS seed Element set.

The BFS itself:
- **Queue**: `VecDeque<(ElementId, u32)>` where the `u32` is
  current depth.
- **Visited**: `HashSet<ElementId>`. Seed elements enter visited
  with depth 0; they themselves don't decay (they're focus-bearing).
- **Expand**: at depth `d` < `radius`, for each element, look up
  `relations_by_element[e]` to find all relations mentioning `e`.
  For each such relation, decay it (unless already decayed) and
  collect its other Element-valued attribute targets to push at
  depth `d + 1`.

`policy.focus_decay_radius` default is **0** today, which means
**Step 11 is a no-op under default policy** — same gating story as
Step 7's drift and Step 10's bumps. Tests use a custom `radius=2`
or `radius=3` to exercise the walk.

## 5. Per-relation decay

For each relation `R` reached at depth `d ≥ 1` (depth 0 = the
seed elements themselves; we don't decay the seed set):

```rust
let utility = compute_utility(R, policy);
let normalized = normalize_utility(utility);
let rate = policy.decay_rate * (1.0 - normalized);
R.stats.activation = bounded_hebbian_decay(R.stats.activation, rate);
```

`policy.decay_rate` default is **0.0** — another no-op under
default policy. Tests use 0.1-0.3.

## 6. Utility formula (v0 subset)

The full §14.7 formula:

```text
utility = focus_success
        + support_count
        + salience
        + exact_value_bonus
        + correction_or_contradiction_bonus
        + source_quality
        - noise_score
        - redundancy
        - age_without_access
```

v0 reads only the substrate we already maintain:

```rust
fn compute_utility(r: &Relation, policy: &Policy) -> f32 {
    r.stats.focus_success_count as f32
        + r.stats.support_count as f32
        + r.stats.salience
        // exact_value_bonus reuses the same detection Step 10
        // uses for salience scoring. v0 skips it — Step 10's
        // salience already folds it in via the +1.0 component,
        // so it's already in `r.stats.salience`. Double-counting
        // would over-protect typed-value relations.
        // (correction, source_quality, noise, redundancy,
        //  age_without_access) → deferred to replay's §14.8 pass.
}
```

Returns a non-negative `f32`. `normalize_utility` maps it to
`[0, 1]` via:

```rust
fn normalize_utility(raw: f32) -> f32 {
    // Soft cap via sigmoid-style mapping: u / (u + k) where
    // k = 5.0 (so utility=5 maps to 0.5, utility=20 maps to 0.8).
    // Keeps the decay rate from going negative or above policy.decay_rate.
    raw / (raw + 5.0)
}
```

Tunable. The constant `5.0` is empirically derived from "a relation
with support_count=3 + salience=0.5 should land in the middle of
the decay-rate range" — adjust after running on real workloads.

## 7. What does NOT decay

Step 11 deliberately leaves these alone:
- **Elements' activation** — the spec says "element/relation
  uniformly" but v0 ships relation-only. Element activation only
  exists today as a field; no Step has populated it yet. Adding
  Element decay is trivial once we have a use for the value.
- **Salience** — decays much more slowly per §14.7. Replay's
  background sweep, not Step 11.
- **Confidence, support_count, support_diversity** — belief
  signals. Only Step 9 (supersession) and Step 10 (promotion)
  touch these.
- **status** — Asserted/Entailed/Defeasible/Superseded/Retracted
  is set by explicit events, never by decay.

## 8. Order within the tick

Step 11 runs **last among mutators**. After Step 10's bumps land,
Step 11 decays the diffuse periphery. Then Step 12 (frame) reads
the current activation state for ranking.

Step-order: 8 (mint) → 9 (supersede) → 10 (reinforce) → 11
(decay) → 12 (frame). Activation flows: bumped in 10, decayed
in 11, ranked in 12.

## 9. Phased rollout

### Phase 1 — Step 11 skeleton + utility helpers

- `src/steps/decay.rs` with `Step11Output` + `focus_radius_decay`
  entry point.
- `compute_utility` + `normalize_utility` helpers (testable in
  isolation).
- No BFS yet; default policy → no-op via early return.

### Phase 2 — BFS walk + per-relation decay

- Build seed Element set from Step 8/9/10 inputs.
- BFS via `relations_by_element` with depth cap.
- Decay each relation reached at depth ≥ 1.
- Tests: depth-0 seed not decayed; depth-1 relations decayed;
  high-utility decays less than low-utility; `radius=0` no-op;
  visited set prevents cycle re-decay.

### Phase 3 — Wire into `lib.rs::run()` + `print_step11`

- Call `focus_radius_decay` after `hebbian_and_salience`.
- Print helper: elements_walked / relations_decayed /
  max_depth_reached. Note the no-op when rate or radius is 0.
- Smoke test with a custom policy (`focus_decay_radius=2`,
  `decay_rate=0.3`) to verify decay actually fires on a real
  sentence.

### Phase 4 — Integration test

- Multi-tick fixture: tick 1 mints with non-zero rate; tick 2's
  Step 11 walks outward and decays the periphery. Assert
  activation dropped on periphery, stayed bumped on focus,
  status untouched, support_count untouched.

## 10. Open questions

### Q1. Should Element activation decay in v0?

The spec says "element and relation uniformly." `Element.stats`
exists; no Step writes to it today. Adding element decay is
trivial in the BFS (decay each element at depth ≥ 1 alongside
its relations). v0 deferral: only decay relations until something
populates element activation.

**Recommendation: skip element decay for v0.** Trivially added
later; nothing reads Element activation today, so decaying it
would be cosmetic.

### Q2. Normalize utility — sigmoid or linear?

Spec says "utility-modulated rate"; doesn't specify shape. Linear
clamping (`min(utility / K, 1.0)`) is simpler but creates a hard
ceiling at K. Sigmoid-style (`u / (u + K)`) is smoother and never
saturates. I picked sigmoid because the latency budget makes
exact tuning of K less important than smooth gradients.

### Q3. Visited set vs. visited-and-decayed set

A relation R might be reachable from two different seed elements
at the same depth. Decaying R twice in one tick would double-
penalize it. Solution: track decayed relations separately and
skip re-decay even if R is re-walked.

**Resolution:** maintain `decayed_relations: HashSet<RelationId>`
alongside `visited_elements`. Walk relations through the visited
set but only decay each one once.

## 11. Out of scope (post-v0)

- **Element activation decay** — see Q1.
- **Salience decay** — replay's job (§14.7).
- **Background sweep over the full graph** — replay thread (§14.8).
- **Utility's `noise_score`, `redundancy`, `age_without_access`** —
  need observability infrastructure that doesn't exist yet.
- **`exact_value_bonus` as a separate utility term** — would
  double-count with Step 10's salience contribution. Wait until
  we separate decay-utility from salience-bump-utility (probably
  never — they're meant to be the same signal).
