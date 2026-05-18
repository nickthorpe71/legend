# Step 9 — Supersession and Cache

> **Status: complete (v0).** All six phases landed.
> Implementation lives in `src/steps/supersede.rs`; design notes
> below are the canonical record. Step 9 runs in `lib.rs::run()`
> after `build_relations` and surfaces cache + supersession output
> via `print_step9`. 17 unit tests cover property inference, cache
> mint, prior supersession + linking metas, intervened/observed
> gating, and a two-tick end-to-end integration. Total lib test
> count: 128.
>
> Two deferred items from the initial v0 land have since shipped:
> property inference moved upstream into Step 8's `mint_event_relations`
> so events self-describe (Step 9 reads the `property` slot and falls
> back to inference only when absent), and `meta_relation_presence`
> was fixed to record `(parent, sibling_attr)` instead of the
> tautological `(parent, target_attr)`, so the intervened gate is
> now an O(1) HashMap lookup. Transitive supersession remains a
> deliberate read-time chain walk via `meta_relations_by_subject`,
> not eagerly closed.

## 1. What Step 9 is

The first **history-aware** step. Step 8 wrote a new event relation
saying _"the appointment moved from Tuesday to Friday"_; Step 9 walks
the existing graph to find what that event invalidates, flips the
stale relations to `Superseded`, and links the chain via
`supersedes` / `derived_from` meta-relations.

No model. The whole step is index lookups + status flips + two or
three relation appends. Everything it needs already exists:

- `relations_by_element[target]` — finds candidate prior caches.
- `meta_relation_presence[(event_rel, intervened_attr)]` — the
  `intervened` gate from §11.7.
- `policy.supersession_threshold` — intent-modulated by Step 2.
- Seeded attribute-name elements: `supersedes`, `derived_from`,
  `target` — all live on `seed_pack.yaml` and resolve via `by_name`.

## 2. Inputs

```rust
pub fn supersede(
    hg: &mut Hypergraph,
    new_event_relations: &[RelationId],   // from Step8Output.minted_relations
    policy: &Policy,
) -> Step9Output;
```

The minted-relations vector carries _everything_ Step 8 wrote, not
just events. Step 9 filters internally for event-shaped relations
(see §3) so callers don't have to pre-filter.

## 3. Identifying an event-shaped relation

§11.10 defines event-shaped as "attribute list includes `target`,
`property`, `from`, and `to`." Step 8 ships all four (the `property`
slot was moved upstream during v0), so the filter is:

**An event relation has both `from` AND `to` attribute names.** The
`subject` slot points at the event element; the `target` slot points
at what changed; the `property` slot carries the inferred kind label
(date / time / amount / location / value); `from`/`to` carry the
value transition.

The from/to-only filter is deliberately loose: Step 9 still falls
back to per-value inference (§4) when an event somehow lacks a
property slot (defensive — pre-step8-property events from older
code paths). The looser filter is a superset of the stricter one.

## 4. Property inference (Step-9-local)

For each event relation, derive a `property_kind` from the value
types of `from` and `to`:

```text
both weekday OR both month                 → "date"
both time-of-day                           → "time"
both quantity OR both unit                 → "amount"
both location-tagged                       → "location"
otherwise                                  → "value"   (generic)
```

Implementation: walk each value Element's `instance_of` relations
via `relations_by_element[value_id]` filtered to attribute name ==
`instance_of`. The kind label is the value at the `instance_of`
slot. Cheap — each value Element usually carries one or two
`instance_of` relations.

The property kind element resolves through the existing
`resolve_label_element` from Step 8 — mint on first sight, dedup on
repeat. (`date`, `time`, `amount`, `value` will be minted by the
first event that uses them; future seed-pack patch can pre-seed if
mint volume becomes a concern.)

## 5. Cache relation shape

Per the §11.10 worked example
`R_new: appointment_1 current_time Friday`, the cache is a binary
relation `[subject: <target>, current_<property>: <to_value>]`:

```text
R_cache_new: [
    Attribute { name: subject_attr,         value: Element(appointment) },
    Attribute { name: current_date_attr,    value: Element(Friday) },
]
status: Asserted
stats.confidence = event.stats.confidence
```

`current_<property>` is itself a new attribute-name element
(e.g. `current_date`, `current_time`, `current_amount`). Minted on
first use through `resolve_attribute_name`.

**Why encode the property in the attribute name vs. a separate
`property` slot?** Two reasons:

- §11.10's worked example uses this shape literally.
- Supersession lookup reduces to "relations on this target with
  attribute name `current_date`" — one `relations_by_element` +
  `relations_by_attribute_name` filter. No third axis.

## 6. Supersession algorithm

For each event relation `E`:

1. **Extract** `target_id`, `from_id`, `to_id`, `from_attr`, `to_attr`
   from `E.attributes`. Skip if any missing (defensive — not all
   minted relations are events).
2. **Infer** `property_kind_label` from value types (§4).
3. **Resolve** `current_<property>_attr` element via
   `resolve_attribute_name`.
4. **Look up priors**: candidate Vec is
   `relations_by_element[target_id]` filtered to relations whose
   attribute list contains `current_<property>_attr`. Drop those
   already `Superseded` / `Retracted`.
5. **Decide gate**:
   - If `meta_relation_presence[(E_id, intervened_attr)] == true` →
     supersession is **unconditional**. Skip the threshold check.
   - Else → require `E.stats.confidence ≥ policy.supersession_threshold`.
     If not, mint the cache anyway but don't flip priors. (The new
     observation isn't confident enough to overturn settled state.)
6. **Flip each prior to `Superseded`** (in-place status update).
7. **Mint** the new cache relation `R_cache_new`.
8. **Mint linking meta-relations**:
   - `[target: R_cache_new, derived_from: <event_id>]` — Entailed
   - One `[target: R_cache_new, supersedes: R_old]` per `R_old`
     flipped in step 6 — Entailed

`meta_relations_by_subject` / `meta_relations_by_object` update
incrementally through Step 8's `index_relation` helper (which Step 9
reuses) — chain walks stay O(chain length).

## 7. `intervened` gate semantics

§11.10's spec is explicit:

- **Observed** event (no `intervened`): high prior confidence
  raises the bar for flipping. Threshold gates supersession.
- **Intervened** event (do()): prior is invalidated by definition.
  No threshold check.

Implementation: O(1) via `meta_relation_presence`. If the boolean
flag is `true` for `(E_id, intervened_attr)`, skip the gate.

The cache mint **always** happens, gate or no gate. The gate only
governs whether the prior gets flipped. If the gate fails:

- Mint `R_cache_new` as `Defeasible` (not `Asserted`) — the new
  observation is recorded but doesn't dominate.
- Don't write the `supersedes` meta-relations.
- Still write the `derived_from` meta-relation — the cache _did_
  derive from this event, regardless of whether it superseded
  anything.

This keeps the audit trail intact while letting Step 12's frame
walker prefer the higher-confidence prior.

## 8. Outputs

```rust
pub struct Step9Output {
    /// Cache relations minted this tick (one per event that fired).
    pub cache_relations: Vec<RelationId>,
    /// Prior cache relations flipped to Superseded this tick.
    pub superseded: Vec<RelationId>,
    /// `(supersedes | derived_from)` meta-relations written.
    pub meta_relations: Vec<RelationId>,
}
```

Frame's `superseded` (`Vec<RelationId>` per §11.12) is gathered
directly from this.

## 9. Index maintenance

Every status flip leaves the relation in `hg.relations` but flips
its `RelationStatus`. The derived indices stay consistent because
they index by `RelationId`, not by status — Step 12's focus walker
filters status at read time. No index rebuilds needed.

The cache mint + linking meta-relations all run through Step 8's
`mint_relation` helper, which already calls `index_relation` to
update all 7 indices. No new index code.

## 10. Edge cases

### 10a. Multiple events on the same target

Two events on the same target, same property — both fire
independently. Each mints its own cache. The second one's
supersession lookup finds the first one's cache (just minted this
tick) and supersedes it. Order matters; iterate `new_event_relations`
in their minted order.

### 10b. Multiple events from over-extraction

Pattern RE's over-extraction (Sarah/meeting from-to) fires multiple
n-ary events. Each fires its own supersession lookup. Different
subjects → independent supersession chains. Same target via
different paths → second event supersedes the first event's cache
(same target, same property). This is correct behavior: the more
recent event wins, even if both ran in the same tick.

### 10c. Cache cycle prevention

The cache walker recurses indefinitely if `R_new` accidentally lists
itself in `supersedes`. Defensively, skip `R_old == R_new` in step 6.
Shouldn't happen in practice (we just minted `R_new`) but the guard
is one line.

### 10d. Property inference failure

If `from` and `to` values have no `instance_of` relations, property
defaults to `"value"`. The cache attribute becomes `current_value`,
which is generic but correct — and dedup still works on subsequent
ticks with the same fallback.

## 11. Phased rollout

### Phase 1 — Property inference helper

- New helper `infer_property_kind` in `src/steps/supersede.rs`.
- Reads `relations_by_element[value_id]` + filters to `instance_of`.
- Returns a static `&str` label (or `"value"` as fallback).
- Unit tests with hand-built Hypergraphs covering weekday/month,
  quantity, mixed, and no-instance-of fallback.

### Phase 2 — Event filter + skeleton

- New `supersede` function reading `Step8Output.minted_relations`.
- Filters to event-shaped (has `from` AND `to` attrs).
- For each event: extract target/from/to, infer property kind,
  resolve `current_<property>` attribute name.
- Mint the cache relation (Asserted by default, no priors yet).
- Tests covering: event recognition, cache shape, attribute-name
  reuse across multiple events.

### Phase 3 — Prior supersession + linking

- Add the `relations_by_element[target]` lookup filtered to
  `current_<property>` attribute name.
- Flip prior caches to `Superseded` in place.
- Mint `supersedes` and `derived_from` meta-relations.
- Tests: one prior → flips. Two priors → both flip. No priors →
  no flip.

### Phase 4 — `intervened` gate

- Read `meta_relation_presence[(event_id, intervened_attr)]`.
- If true → unconditional.
- If false → require `event.stats.confidence ≥
  policy.supersession_threshold`.
- Failed gate → mint cache as `Defeasible`, skip `supersedes`
  metas, still write `derived_from`.
- Tests: intervened event with low conf still supersedes; observed
  event with low conf doesn't supersede.

### Phase 5 — Wire into `lib.rs::run()` + print helper

- Run `supersede(...)` after `build_relations(...)`.
- `print_step9(...)` shows cache mints + flips + chain ops.
- Smoke-test the rescheduled-meeting sentence: prior tick mints
  `appointment current_date Tuesday`; second tick supersedes to
  Friday with `intervened` (because `rescheduled` is in the
  lexicon).

### Phase 6 — Integration test

- Two-tick worked example. Tick 1: mint appointment + first
  cache. Tick 2: `appointment moved from Tuesday to Friday`.
  Assert:
  - Tick 1's `current_date Tuesday` cache flips to `Superseded`.
  - Tick 2's `current_date Friday` cache lands `Asserted`.
  - `supersedes` chain has exactly one link.
  - `derived_from` points at the n-ary event.

## 12. Open questions

### Q1. Where does property inference belong long-term?

The current plan puts inference in Step 9. The alternative — push
it back into Step 8's merge pass so the event itself carries
`property` — has two upsides: events stay self-describing in
isolation, and Step 9 simplifies to a pure index walk. Downside:
Step 8's merge pass gets longer, and we already shipped it without
inference.

**Recommendation: Step 9 for v0.** Moving inference to Step 8 is a
clean refactor once Step 12's frame walker actually consumes
events, since that's when we'll see whether the missing `property`
on the event hurts downstream queries. Until then, the Step-9-local
inference is cheap and keeps Step 8 stable.

### Q2. Should `current_<property>` attribute-names be seeded?

The mint-on-first-use plan grows the attribute-name population
organically. Cleaner alternative: pre-seed `current_date`,
`current_time`, `current_amount`, `current_value` so the first
event doesn't trigger an attribute-name mint warning.

**Recommendation: mint-on-first-use for v0.** The
`policy.attribute_name_mint_warning_count` default is 5, and v0
won't hit it from cache attribute names alone in any normal
workload. If it becomes noisy, pre-seed in a future seed-pack
patch.

### Q3. Cache relations and source meta-relations

Step 8 emits a `source` meta-relation for every base relation when
the tick's `source` is `Some`. Should Step 9's cache relations also
get a `source` meta? They're derived, not directly observed —
arguably `derived_from` already carries the provenance.

**Recommendation: skip `source` for cache relations.**
`derived_from` already points at the event, and the event has the
`source` meta. Step 12 can walk one extra hop if it wants to
attribute the cache to the original input source.

## 13. Out of scope (post-v0)

- **Multi-property events**: a single event with multiple
  `property` slots in its attribute list (e.g., "rescheduled from
  Tuesday at 3pm to Friday at 4pm" — both date AND time). v0
  treats this as two separate events upstream; Step 9 sees one
  property per event.
- **Transitive supersession**: if A supersedes B and B supersedes
  C, A doesn't auto-supersede C. The chain is walked at read time
  via `meta_relations_by_subject`, not eagerly closed.
- **Cross-tick property inference**: today's property inference
  uses only `instance_of` relations visible at the moment Step 9
  runs. A value Element minted this tick whose `instance_of`
  hasn't been written yet (defensive ordering) would fall back to
  `"value"`. Step 8's mint order already writes `instance_of`
  before the n-ary event, so this isn't a practical issue, but a
  ordering guarantee in the doc would be nice.
- **Tantivy or vector-search property dedup**: today's resolution
  is exact-name only. Synonym dedup (`current_date` vs `date`) is
  replay's job.
