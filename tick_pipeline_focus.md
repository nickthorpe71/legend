## Steps

### Step 1 — Detect Intent

Intent is a **4-dimensional weight vector** scoring how much this tick should change the substrate.

Conviction:
Speaker certainty. High = "absolutely / definitely / I know";
low = "maybe / I think / not sure". Drives default confidence
for new relations and the Asserted/Defeasible threshold.

Prediction Error:
Informational surprise. High when the input _suggests_ it contradicts a
prior belief OR introduces a concept far from existing
regions. Drives salience boost + supersession-lookup trigger.

Arousal:
Magnitude of importance signal. Caps, exclamation, intensifying
vocabulary, emotional language. Drives salience independent
of conviction or prediction-error.

Curiosity:
Retrieval-shape vs assertion-shape. High = "what is X / find
when / show me"; low = "X is Y / X happened". Drives
default-confidence reduction (the question's content shouldn't
elevate as much as a statement's would) while still firing
path reinforcement. No direct neuromodulator analog —
Legend-specific because we have a single tick verb that
covers both encoding and retrieval.

### Step 2 — Adjust Policy

Pure scalar arithmetic — no model. PFC reads the `Intent`
vector from Step 1 and computes the adjusted `Policy` by combining
each dimension into the substrate knobs it drives. The base
mappings (§10.6 specifies the formulas in full):

```text
default_conf       = base_conf * conviction * (1.0 - 0.7 * curiosity)
                     // high conviction non-questions write at high
                     // confidence; questions and hedges write low

salience_multiplier = base_salience
                    + 1.0 * arousal                  // NE-analog
                    + 1.0 * prediction_error         // DA-analog
                    // both extremes (emotional + surprising) bump

leaf_vigilance     = base_vigilance
                   + 0.20 * prediction_error
                   + 0.20 * conviction
                   // contradictions and confident claims tighten
                   // routing so we don't blur entities; brainstorming
                   // (low conviction, low prediction_error) loosens

hebbian_rate       = base_rate * (1.0 - 0.5 * curiosity)
                                * (1.0 + 0.3 * arousal)
                   // questions reinforce paths but at lower magnitude
                   // than statements; arousal slightly amplifies

supersession_threshold = base_threshold * (1.0 - prediction_error)
                       // high prediction-error ⇒ search prior cache
                       // relations to supersede; low ⇒ skip the lookup
```

Every tick-internal subroutine reads `&Policy`; only PFC writes it.
The base `Policy` on `Hypergraph` is the inter-tick rest state; the
per-tick adjusted copy is what Steps 4–12 see.

### Step 3 — Route Through Regions

**The job.** Identify which regions of the DAG are _active for this
tick_. This is a **fast predictive prefilter**, not the
substrate's authoritative answer about where new elements belong.

The window embedding gives a
~5 ms semantic prefilter that captures the input's _gestalt_ (the
verb shape, the from/to construction, the change-vs-state cue —
things that don't reduce to any single extracted element) and lets
extraction proceed with a correctly tuned label set on its first
pass. Step 7 then does the authoritative placement once the actual
elements exist and have their own persistent embeddings.

This is why the substrate's "where does X live?" answer comes from
Step 7, not Step 4 — and why the window embedding is correctly
ephemeral while element embeddings persist.

**Mechanics.** Read-only DAG descent — no model, just cosine
similarity over already-computed vectors. Each window's embedding
runs `route_regions(...)` (§10.2) against the DAG. Parallelizes
across windows via `rayon::par_iter`. The active-regions set
surfaced to Step 5 is the **union** of each window's routing
results — multi-window inputs touch multiple regions, which is the
desired behavior.

**Algorithm.** Starting from `GENESIS`, for each window's embedding:

1. Look up candidate children via `region_children[current]`.
2. For each candidate region, fetch its prototype Elements via
   `region_prototypes[region]` and read each prototype Element's
   inline `embedding` field directly (FP32, no indirection).
3. Score each candidate as the **max cosine** over its prototype
   Elements' embeddings (multi-prototype regions preserve modes,
   §10.4; max-pooling reflects "any prototype matches").
4. Descend into the top-k children whose score exceeds
   `policy.descend_threshold`.
5. Stop when no child exceeds the threshold (leaf reached) or
   when no child clears `policy.leaf_vigilance` (sub-threshold
   input → routed to `VOID`).

Each comparison is O(prototypes-in-region); the prototype set is
kept small by `policy.merge_threshold` (collapses near-duplicates)
and `policy.split_variance` (splits high-scatter regions). The `RegionDelta`
returned alongside the `ActiveRegion` list captures proposed
parent attachments, prototype-vector updates (k-means targets),
and any newly-minted regions (§10.3.5 mid-path insertions); it is
**held** through the read-mostly phase and committed by Step 7
(§11.8a).

The `active_regions` set seeds extractor attention in Step 5 —
when a region is active, attribute names authored within relations
whose participants are members of that region (per the
`region_members` index over `member_of` relations) get a small
label-set priority, so GLiNER2 prefers the lexicon's "warm"
attribute names over cold ones.

### 11.7 Step 5 — Run Extractors

One call per tick over the whole input. he extractor sees the
entire input at once; sentence boundaries inside the input are not
consulted, which is what makes cross-sentence relations
recoverable without pre-segmentation hazards. The output is the
(entity, relation) candidate stream that feeds Step 6.

The v0 extractor stack runs five passes sequentially within the
step (the step itself is the latency long pole because of GLiNER
NER):

- **Step 5a — Orthographic chunker** (`src/steps/orthographic.rs`,
  `src/steps/void_filter.rs`). Pure Rust, no model. Always runs
  first; always emits at least one chunk for non-empty input.
  Emits `OrthographicChunk` records at three scales: `Phrase`
  (punctuation/whitespace-delimited spans), `Repeated` (any
  2..=5-token n-gram appearing ≥ 2× in the input), `Token`
  (content tokens — every whitespace-and-slash atom whose
  lowercased form does **not** resolve to a `Polarity::Void`
  element via `by_name`). The 118 closed-class void members
  seeded under the 8 void regions cover the high-frequency
  English function-word tail. Output lands in
  `ExtractionOutput.unconditional_chunks`; Step 8 mints an
  Element per entry regardless of whether NER labels it.
- **Step 5b — NER** — hand-rolled GLiNER1 span tagger on the
  in-house INT8 BERT engine (encoder = DeBERTa-v3-small, INT8).
  Spec originally called for `gline-rs` on `ort`, but `ort` is
  disqualified for portability and GLiNER2's disentangled-attention
  `Clip` op blocks naive tract loading; the span head is bespoke.
  Labels passed in are the seed kinds (`person`, `org`, `place`,
  `weekday`, `quantity`, `event`, `role`, `state`, `time`) plus
  warm-bias names from Step 4's active regions. Returns
  `(span, kind, confidence)` triples. Each tagged span auto-emits
  `(span_element, instance_of, K)`. Auto-emitted `instance_of`
  relations dedup against existing equivalents; reinforcement
  flows through the normal Step 8 path. Confidence
  ≥ `policy.ner_assertion_threshold` → `Entailed`; below →
  `Defeasible`. Anonymous spans are minted with
  `name = "<kind>_<counter>"`.
- **Step 5c — Temporal parser** (`src/steps/temporal.rs`). Pure
  `regex` over weekdays / months / `today` / `tomorrow` /
  `yesterday` / `tonight`. Each match becomes a value-Element with
  the surface form as a name; spans overlapping a 5b NER hit are
  dropped to avoid duplicate typing. Confidence is fixed at 0.95
  (regex precision). **`chrono` / `chrono-english` relative-phrase
  parsing is deferred** until we need to ground `"next Tuesday"`
  to a concrete datetime; typed comparison still re-parses the
  surface name on demand (§7.3).
- **Step 5d — Pattern-based relation extraction**
  (`src/steps/relation_patterns.rs`). Pure-Rust surface templates
  (§15.1 / §24.1 fast-path) over Step 5b spans: `X from A to B`,
  `X at Y`, `X with Y`, `X's Y`. Emits canonical
  `(subj_span, attr_name, obj_span, confidence)` quads with
  hardcoded attribute names (`from`, `to`, `with`, `at`,
  `property`). Covers the seed-pack frames. **The GLiNER2
  multi-task zero-shot RE model is deferred**; broader coverage
  grows through replay and warm-region labels.
- **Step 5e — Heuristic coref** (`src/steps/coref.rs`). Pure Rust,
  recency-based, no model. Pronouns (he / she / it / they / this /
  that) and definite descriptions ("the dentist") resolve to the
  most-recently-focused `RecentFocusEntry` whose `attribute`
  matches the span's grammatical slot (Centering Theory + Hobbs'
  baselines, §15.1). **Currently a stub returning no decisions** —
  wired into Step 5's return shape so the downstream API is stable,
  but `Hypergraph.recent_focus` isn't populated until §11.11
  lands, so there are no candidates to score against yet.

All extractor output carries confidence. The tick's `source`
parameter (§9.6, §11.1) flows into `(R, source, source)`
meta-relations on the resulting relations during Step 8.

**Causal-shape extraction conventions (§6 (8)).** Three conventions
sit on top of the four extractors above; none requires a new model
in v0, only that the relation-extraction step bias its label set
and emit the right meta-relations:

- **Tag agent-action verbs with `intervened`.** When the
  relation-extractor's verb belongs to the surface lexicon for
  agent action (`reschedule`, `move`, `set`, `configure`, `decide`,
  `cancel`, `ship`, `revert`, `merge`, `deploy`, `delete`), emit a
  `[target: R, intervened: <verb-element>]` meta-relation alongside
  the base relation. Verb elements link to `intervened` via
  `subclass_of` (§16.3); recognition walks the cone, so the
  surface lexicon stays open. Default — no tag, no `intervened`
  meta-relation — means observation. Steps 9 and 10 read the tag
  for do()-shaped supersession and reinforcement (§11.10, §11.11).
- **Prefer causal vs. correlational attribute names from
  linguistic markers.** "X caused Y" / "Y because X" / "X led to Y" /
  "due to X" → `caused`. "X enabled Y" / "made it possible for Y"
  → `enables`. "X prevented Y" / "blocked Y" → `prevents`.
  "X tends to happen with Y" / "X and Y both" / "comes with X" →
  `correlated_with`. The four canonical names are seeded (§16.3);
  surface variants emerge as ordinary attribute-name elements
  pinned via `subclass_of`. Without these conventions, GLiNER2
  collapses correlational and causal claims onto whatever surface
  attr_label the input used and Legend loses the rung distinction
  the seed pack created the names for.
- **Capture conditionals as `antecedent_of`, not as flattened
  base relations.** "If X happens, Y" / "Y holds when X" / "Y
  unless X" → emit a base relation for Y plus a meta-relation
  `[target: R_y, antecedent_of: R_x]`. Forward-chaining is §24.6's
  job to add later; v0's job is to _record the shape_ so v1 has
  substrate to read from on day one (§16.3).

**Attribute-name label set.** GLiNER2's relation-extraction labels come
from, in order:

1. Seed-pack canonical attribute names (`instance_of`, `subclass_of`,
   participant attribute names) — always included.
2. The "warm" attribute names: attribute-name elements whose
   `MemoryStats` activation is above a floor. This is what active
   regions modulate — when Step 4 returned active regions, attribute
   names whose participants live in those regions get included even if
   their activation has decayed somewhat. Bounds open-vocabulary drift
   without freezing extraction to seed coverage.

**Resolving a proposed attribute-name label to an ElementId.** Each
extractor proposal arrives as
`(subj_span, attr_label, obj_span, confidence)`. Resolve `attr_label`
to an `ElementId` by:

1. Exact-match lookup against element names in the lexical index
   (tantivy, §15.1). On hit, reuse the attribute-name element.
2. On miss, embed `attr_label` and run a cosine search across **all**
   attribute-name elements (not just warm ones — the warm set is
   used for GLiNER2's label-set bias above, not for dedup). On any hit
   with cosine ≥ `policy.attribute_name_dedup_threshold`, reuse the top
   hit. The relation is marked `Defeasible` (the surface label didn't
   match the canonical name, so the binding carries some uncertainty
   even though the attribute name is right); replay can reinforce the
   alias later.
3. On miss, mint a new attribute-name element with the label as its
   name. Every relation that uses it this tick is `Defeasible` until
   replay either reinforces it (≥ N independent ticks within a window)
   or prunes it.

Note: there is no privileged "predicate" position in the resulting
Relation. The `attr_label` becomes the _attribute name_ of one slot in
the relation's attribute list; the subject and object Elements are
bound via separately seeded attribute names (typically `subject` for
the head, `attr_label` itself for the object — see §7.2 worked example).
Step 8 assembles the full attribute list.

**Why the cosine search is universal, not warm-only.** Attribute-name
synonyms ("rescheduled to" / "moved to" / "changed to") embed close
together regardless of which is currently warm. Restricting dedup to
the warm set lets cold-but-equivalent attribute names pile up and re-
mint on every tick, defeating recognition that counts by attribute-
name id. The universal cosine search is `O(P)` per proposed label —
tractable at v0 attribute-name counts (low thousands).

**Mint-rate observability.** When a single tick mints more than
`policy.attribute_name_mint_warning_count` new attribute-name elements
(default 5), the inspection harness logs the tick id and replay
receives a priority flag for attribute-name dedup on this tick's
outputs (§14.8). Not a hard cap — a tick that legitimately introduces
several new attribute names is allowed — but the warning surfaces the
cases where synchronous dedup didn't catch a synonym cluster.

**Optional accelerator (post-v0):** a _lexicon-paired-noun_ rule that
proposes intermediate DAG nodes upfront when both components of a
compound noun are already in the lexicon. See §24.8 for the v1+ form.

### 11.8 Step 6 — Coreference Scoring

Pure Rust scorer — no model. Operates on **entity-mention spans
returned by Step 5's NER and relation extractor** —
A "span" here is a contiguous slice of source text that an extractor
identified as referring to something — a pronoun ("it", "they"), a
definite description ("the dentist"), a partial name ("Doc Rao"),
or a freshly-tagged entity. Identity is conservative. For each
ambiguous span, build the candidate set from `recent_focus` (working
memory) plus elements within the active regions' neighborhood (via
`region_members[R]`), then score each candidate:

```text
score(span, candidate) =
    name_overlap(span, candidate.names)         // 0..1, edit distance / lemma match
  + embedding_similarity(span_emb, cand_emb)    // cosine
  + frame_overlap(active_frame, candidate)      // 1.0 if same frame, 0.5 if adjacent
  + attribute_overlap(span_slot, candidate)     // 1.0 if recent_focus entry's `attribute` matches the span's grammatical slot
  + temporal_compatibility                      // 1.0 if no valid-time conflict
  + relation_support                            // 0..1, fraction of candidate's relations consistent with span's neighborhood
  - contradiction_penalty                       // 1.5 if candidate has a Superseded relation that would re-fire
  - distinct_instance_penalty                   // §14.3 pattern_separation output
```

The attribute-overlap term is what `recent_focus` carrying
`RecentFocusEntry { element, attribute, frame, tick }` (§9.6) buys
us: "it" + most-recently-focused-as-`target` = the right element;
"it" + most-recently-focused-as-`actor` = a different one. Without
attribute-tagged focus, a flat `VecDeque<ElementId>` would resolve
all "it" pronouns to the most-recent-anything, which fails on
multi-attribute ticks like §19's Tick 5 ("the dentist moved it again
to Monday" — "it" should resolve to the appointment, not to the
dentist or the previous date).

Filtering the candidate set by `attribute` (the grammatical slot
the candidate was bound under in its prior focus) is **back-door-
style adjustment** (Pearl, §6 (8)): "most recent thing" confounds
"what the speaker just referred to" through "what was just
mentioned in any role"; conditioning on the slot blocks that
confounding path so the remaining signals (name, embedding, frame,
relation support) carry causal information about identity rather
than slot-mixed correlation.

Rules:

- Reuse concepts broadly (concept-recognition fires per §3.4 / §8.1).
- Reuse instances only with coreference support (multi-term score
  above a threshold).
- Create provisional instances when uncertain.
- Replay merges provisional instances later if support accumulates.

Pattern separation (`separate_pattern`, ported from current Legend's
dentate gyrus) is the dampening function on the merge side: when two
candidates are close-but-distinct on a discriminating role, force them
apart.

### 11.8a Step 7 — Apply Region Delta

The first mutation step — no model. This is the **authoritative**
phase of region routing (the predictive prefilter ran in Step 4,
§11.6). Step 4 used the window's ephemeral embedding to identify
which regions are active for biasing extraction; Step 7 now uses
the actual _element_ embeddings (each minted Element's persistent
inline `embedding` field) to update the substrate's belief about
where elements belong. After this step, region membership is the
DAG's source of truth.

The `RegionDelta` returned by Step 4 (held through the read-mostly
phase) is now committed:

- **Parent attachments.** Each `(child, parent, weight)` becomes (or
  reinforces) a `(child, parent_region, parent)` relation with
  `stats.confidence = weight`.
- **Prototype updates.** Each `(prototype_element, new_embedding)`
  overwrites the prototype Element's inline embedding via the
  spherical k-means update rule (§10.5). The
  `(R, prototype, prototype_element)` relation already exists.
- **New regions.** Each `NewRegion` mints a region Element plus a
  prototype Element with the supplied initial vector as its inline
  embedding, then writes the seed structural relations
  (`instance_of REGION_CLASS`, `parent_region`, `prototype`).
  Mid-path insertions (§10.3.5) write the `parent_region` relation
  as `Defeasible`.
- **New members.** Each `(member, region)` becomes a
  `(member, member_of, region)` relation.

The region indices (`region_parents`, `region_children`,
`region_lateral`, `region_prototypes`, `region_members`) update
incrementally as these relations land. After this step the DAG
reflects whatever §10.2 routing decided, and Steps 8–12 see the
updated topology.

### 11.9 Step 8 — Build Relations and Events

No model — pure HashMap inserts + index updates. Each surviving
extractor proposal becomes a Relation whose **attribute list** is
assembled from the extractor's emitted slots. For a binary triple
`(subj_span, attr_label, obj_span)` the resulting relation has two
attributes — one binding the head Element under a participant
attribute-name appropriate to the extractor's frame (default
`subject`, or a frame-specific slot like `actor` for animate event
participants), and one binding the object Element under the attribute
name resolved from `attr_label`. `subject` is a seeded participant
attribute (§16.3) with no structural privilege — it is not indexed
separately, and recognition does not read it. For n-ary events the
attribute list grows: a reschedule event becomes one Relation with
`[target: appointment_1, property: date, from: Tuesday, to: Friday]`.

Per relation:

- **Status** set from extractor confidence vs
  `policy.ner_assertion_threshold` (Entailed / Defeasible).
- **`stats.confidence`** initialized as `policy.default_conf`
  (intent-modulated) × extractor confidence.
- **Source meta-relation** — a separate Relation
  `[target: R, source: source_id]` written iff the tick's `source`
  parameter is `Some` (§11.1).

`relations_by_element` / `relations_by_attribute_name` /
`meta_relations_by_subject` / `meta_relations_by_object` /
`attribute_value_counts` / `attribute_co_counts` /
`meta_relation_presence` all update incrementally — one HashMap
insert per (relation × attribute) pair per index. Build compact base
relations only; entailment closure is computed on demand (§14.5).

For: _"My dentist appointment with Dr. Rao changed from Tuesday to
Friday."_

Base elements created or reused:

```text
user, Dr. Rao, dentist, appointment, appointment_1,
Tuesday, Friday, reschedule_event_1
```

Base relations (shorthand `S attr O` ≡ a Relation with attribute list
`[subject: S, attr: O]` — `subject` is the seeded generic head
participant (§16.3), used here unless a frame-specific slot fits
better. The n-ary `reschedule_event_1` row is one Relation with four
attributes — `[target: appointment_1, property: date, from: Tuesday,
to: Friday]` — not four separate triples):

```text
DrRao instance_of person                         [Defeasible]
DrRao has_role dentist                           [Asserted]
appointment_1 instance_of appointment            [Entailed]
appointment_1 participant user                   [Entailed]
appointment_1 provider DrRao                     [Asserted]
appointment_1 domain dental                      [Entailed]
reschedule_event_1 instance_of reschedule_event  [Entailed]
reschedule_event_1 [target: appointment_1,
                    property: date,
                    from: Tuesday,
                    to: Friday]                  [Asserted]
```

### 11.10 Step 9 — Supersession and Cache

No model — index lookups + status flips. For each new event-shaped
relation (Event Calculus fluent update, §14.4) whose attribute list
includes `target`, `property`, `from`, and `to`:

1. **Look up prior cache relations** for the same target+property
   pair via `relations_by_element[target]` filtered to entries whose
   attribute list contains both `target` (with this value) and
   `property` (matching the event's `property` value).
2. **Mark each prior cache `Superseded`** — status flip in place,
   no delete.
3. **Write the new cache relation** `R_new` with
   `MemoryStats.confidence` carried from the event.
4. **Write the linking meta-relations** (themselves Relations whose
   attributes target `R_new`): one with `(target: R_new, derived_from:
event)` and one per superseded cache with
   `(target: R_new, supersedes: R_old)`.

Worked example for `appointment_1 current_time`:

```text
R_new: appointment_1 current_time Friday   [Asserted]
R_old: appointment_1 current_time Tuesday  [Superseded]

(R_new, derived_from, reschedule_event_1)  [Entailed]
(R_new, supersedes,   R_old)               [Entailed]
```

`meta_relations_by_subject` and `meta_relations_by_object` update
incrementally as the new `supersedes` and `derived_from` meta-
relations land, so chain walks (forward via
`meta_relations_by_subject[R]` filtered to entries with a `supersedes`
attribute, inverse via `meta_relations_by_object[R]` filtered the same
way) remain O(chain-length) — one HashMap lookup plus a 0–3-element
filter per hop.

**`intervened` modulates the trigger threshold (§6 (8), §16.3).**
When the event carries an `intervened` meta-relation (the agent
acted on the world — `rescheduled`, `set`, `cancelled`, …), prior
cache supersession fires unconditionally regardless of confidence
gaps: do() severs prior causes by definition, so the previous
state's confidence is irrelevant. When the event lacks
`intervened` (default observation), the existing
`policy.supersession_threshold` (§10.6, intent-modulated) applies —
an observed contradiction may indicate the substrate's prior model
was wrong somewhere upstream rather than a clean state transition,
so high prior confidence raises the bar for flipping. Both paths
produce the same `Superseded` flip + `supersedes` meta-relation;
only the gate condition differs.

### 11.11 Step 10 — Hebbian + Salience

Pure arithmetic over `MemoryStats` — no model. Two updates fire:

**Hebbian co-activation.** For every pair (A, B) of elements that
co-occurred in the focus set this tick, walk to their connecting
relation R via `relations_by_element[A]` (filter to relations whose
attribute list also mentions B) and bump `R.stats.activation` via the
bounded Oja rule (§14.9):

```text
new = old + rate * (1 - old)
where rate = policy.hebbian_rate * intent.plasticity_multiplier
```

Asymptotes to 1.0; never overshoots. The intent multiplier from
Step 2 (§10.6 table) is already baked into `policy.hebbian_rate`,
so a question's reinforcement lands at lower weight than a
statement's on the same path.

**Salience formula.** For each relation R produced or reinforced this
tick, compute:

```text
score_salience(R, p) =
    p.salience_floor
  + 1.0  if R has an exact-value attribute (date-named, number-named, named entity)
  + 1.0  if R was just produced by supersession (Step 9) — preserve correction history
  + 0.5  if R is a user-stated preference (frame-scoped FRAME_USER + an attribute name is preference-shaped)
  + 0.5  if R is in this tick's focus set (focus-bearing on this tick)
  + 0.0  otherwise

bump = score * p.salience_multiplier
       // p.salience_multiplier already carries the +1.0*arousal +
       // 1.0*prediction_error contributions from §10.6, so emotionally
       // intense or surprising ticks land bigger bumps automatically.
R.stats.salience = bounded_hebbian_bump(R.stats.salience, bump * p.hebbian_rate)
```

The bump uses `bounded_hebbian_bump` (§14.9) so salience asymptotes
to 1.0 rather than running away. Salience floors decay's effect:
high-salience relations decay much more slowly than low-salience
ones (§14.7 utility formula).

**Promotion check (Defeasible → Asserted).** A `Defeasible` relation
is promoted to `Asserted` in this step when _all three_ hold:

1. `stats.support_count >= policy.promotion_min_count` (default 3) —
   the relation has been observed across at least N independent ticks
   within `policy.promotion_window_ticks`.
2. `stats.support_diversity >= policy.promotion_min_diversity`
   (default 2) — the supporting ticks come from at least D
   _topologically distinct_ evidence sources. Distinctness is
   measured across: different `(R, source, S)` source elements,
   different `Intent` regions (e.g. high-conviction-statement vs.
   curiosity vs. high-prediction-error mention), and different
   `active_frame` scopes — _and_ the source elements themselves
   must be topologically independent in the source DAG. Step 10
   reads a replay-maintained annotation that maps each source
   element to its independent-root id (replay walks `derived_from`
   chains; §14.8), so two sources that trace back to the same
   root event count as one rather than two. Two Slack messages
   from the same user reposting the same git commit are downstream
   of one root event and don't clear the bar.
3. No contradicting relation has been written within the window
   (one `meta_relations_by_object[R]` lookup + `supersedes` filter).

The diversity check is what distinguishes "repeated assertion" from
"converging evidence" — and the topological-independence extension
is what distinguishes "converging evidence" from "echo chamber."
Without it, an extractor that rephrases the same input three times
auto-promotes wrong content; with it, promotion requires actually
independent evidence in the Pearl sense (§6 (8) — independent
causes, not just nominally distinct ids).

### 11.12 Step 11 — Focus-Radius Decay

No model — bounded BFS (Breadth-First Search) + scalar multiplies. Decay during the tick
is **bounded to the focus radius** so the read-mostly-then-mutate
phase stays under the latency budget. Walk outward from the focus
set up to `policy.focus_decay_radius` hops via
`relations_by_element`; for each element/relation reached, decay
`activation` via `bounded_hebbian_decay` (§14.9):

```text
new = old * (1 - rate * (1 - normalize(utility)))
```

where utility is the §14.7 score (focus_success + support_count +
salience − noise_score − redundancy − age_without_access).
High-utility relations decay slowly; sub-radius low-utility ones
decay quickly.

Everything outside the radius is decayed by the **background sweep**
(§14.7), scheduled by `enqueue_replay`. The sweep runs in the replay
thread, computes a delta against a snapshot, and the next tick applies
it under `&mut`. Decay weakens **access paths**, never destroys
focus-bearing relations (Invariants 2, 8).

### 11.13 Step 12 — Assemble Attention Frame

No model. The frame is a **post-tick snapshot of the focused
subgraph** — not a pre-assembled answer. Most fields are not
_computed_ in Step 12; they are _gathered_ from per-tick buffers
that earlier steps populated as a side effect of doing their own
work. The two things Step 12 itself produces are (a) the
`focused_relations` RRF (Reciprocal Rank Fusion; Cormack et al. 2009) over three already-computed signals plus a single tantivy
BM25 (Best Match 25) query, and (b) the `next_actions` suggestions.
Output shape:

```rust
struct ConsciousAttentionFrame {
    tick: Tick,
    input: InputEcho,
    intent: Intent,
    active_frame: Option<ElementId>,
    active_regions: Vec<RegionActivation>,
    focused_relations: Vec<RelationActivation>,
    supporting_claims: Vec<ClaimRef>,
    history: Vec<ClaimRef>,
    uncertainty: Vec<UncertaintySignal>,
    durable_writes: Vec<ElementId>,
    superseded: Vec<RelationId>,
    next_actions: Vec<AttentionAction>,
}
```
