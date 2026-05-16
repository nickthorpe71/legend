# Step 8 — Build Relations and Events

> **Status: complete (v0).** All six phases landed.
> Implementation lives in `src/steps/build_relations.rs`; design
> notes below are the canonical record. Production tick output
> shows minted elements + relations via the `print_step8` helper
> in `lib.rs::run()`. 15 unit tests cover binary mints, n-ary
> merge, novelty branch, coref override, source meta, indices,
> and an end-to-end dentist-sentence integration.
>
> Future work (post-v0): `property` slot inference in n-ary
> events, span-level seed prototypes, cosine-dedup for spans
> against `region_members`, Tantivy lexical index for
> attribute-name resolution. See §13's "Out of scope" list.

## 1. What Step 8 is

The first **structural mutation** step. Step 7 mutated prototype
embeddings and region indices; Step 8 mints the Elements and
Relations that record the tick's claims about the world.

No model — pure HashMap inserts + index updates over what Step 5
already proposed. The "work" is:

1. Resolve every span Step 5 mentioned to an existing Element, or
   mint a fresh one. Fold the contextualized vector into the
   element's running centroid if it already exists.
2. Resolve each `attr_label` to an attribute-name Element. Reuse
   exact-match hits; cosine-dedup near misses; mint on miss.
3. Build one Relation per surviving proposal — attribute list
   assembled from the extractor's frame.
4. Stamp status (Entailed/Defeasible) from confidence vs
   `policy.ner_assertion_threshold`; stamp `stats.confidence` from
   `policy.default_conf × extractor_confidence`.
5. Write the optional `source` meta-relation if the tick's source
   is `Some`.
6. Incrementally update the indices in §3.

## 2. Inputs

```rust
fn build_relations(
    input_text: &str,
    hg: &mut Hypergraph,
    out: &ExtractionOutput,    // from Step 5
    route: &RouteResult,       // from Step 4 — active regions inform polarity
    policy: &Policy,           // adjusted, from Step 2
    source: Option<ElementId>, // tick's source (§11.1)
) -> Step8Output;
```

`ExtractionOutput` is the existing two-branch struct:

- `known.instance_of` — `(span, instance_of, K)` from NER + Temporal.
- `known.relations` — `(subj, attr, obj)` from pattern RE.
- `known.coref` — antecedent decisions (stub today; honored once live).
- `novelty.chunks` — Phrase/Token chunks. Always populated for
  non-empty input.
- `novelty.relations` — `(subj_text, attr_text, obj_text)` candidate
  triples from pattern OpenIE. Always Defeasible-bound.

## 3. Outputs

```rust
pub struct Step8Output {
    /// Elements minted this tick. Surface in the frame's
    /// `durable_writes` field (§11.12).
    pub minted_elements: Vec<ElementId>,
    /// Relations minted this tick (base + meta).
    pub minted_relations: Vec<RelationId>,
    /// Per-tick mint count — fed into the
    /// `attribute_name_mint_warning_count` observability check
    /// (§11.7).
    pub attr_names_minted: u32,
}
```

Side effects on `&mut Hypergraph`:

- `elements`, `relations` extended with mints.
- `by_name` updated per minted element.
- **New indices** (none exist yet — see §7):
  - `relations_by_element: HashMap<ElementId, Vec<RelationId>>`
  - `relations_by_attribute_name: HashMap<ElementId, Vec<RelationId>>`
  - `meta_relations_by_subject: HashMap<RelationId, Vec<RelationId>>`
  - `meta_relations_by_object:  HashMap<RelationId, Vec<RelationId>>`
  - `attribute_value_counts: HashMap<(ElementId, ElementId), u32>` — keyed by `(attr_name, value_element)`.
  - `attribute_co_counts: HashMap<(ElementId, ElementId), u32>` — keyed by ordered pair of attribute names co-occurring on the same relation.
  - `meta_relation_presence: HashMap<(RelationId, ElementId), bool>` — does relation R carry a meta-attribute with this name? Cheap "is R `intervened`?" lookup.

## 4. Element minting + dedup

For every proposal that names a span (`instance_of` subjects,
pattern RE subj/obj, novelty triples), resolve `span_text` to an
Element by:

1. **Exact-name hit** via `by_name[normalized(span_text)]`.
   Normalization = lowercase + trim. If multiple IDs share the
   name, pick the one whose embedding has the highest cosine to
   the span's contextualized vector (`embed_span_in_context`).
   Cheap because `Vec<ElementId>` per name is small.
2. **Cosine hit** if exact miss. Embed the span. Search element
   candidates within the **active regions' members** (Step 4's
   `route.active_regions` → `region_members[R]`). Hit if cosine ≥
   `policy.attribute_name_dedup_threshold`. Reuse the top hit.
   Cosine search is bounded — only active regions contribute, so
   it's at most a few hundred elements per tick.
3. **Mint** on both misses. New `Element` with:
   - `id = ElementId(elements.len() as u32)`
   - `names = vec![span_text]`
   - `embedding = embed_span_in_context(input_text, start, end)`
     fall-back to `embed_text(span_text)` if `None`.
   - `polarity` = inherited from the dominant active region (see §6).
   - `stats = MemoryStats { confidence: policy.default_conf, plasticity: 1.0, ..Default::default() }`
   - `created_at = hg.clock`.

On **reuse**, fold the contextualized vector into the element's
running centroid:

```rust
fold_streaming_centroid(
    &mut element.embedding,
    &observed,
    element.stats.access_count,
);
element.stats.access_count += 1;
element.stats.last_seen = hg.clock;
```

`fold_streaming_centroid` already lives in `src/embed.rs` from
Phase 4 of the contextualized-embeddings work.

### 4a. Coref override

If `known.coref` has a decision binding `span_text → antecedent`,
**skip steps 1–3** and reuse the antecedent element directly. Bump
its access count + fold the new mention's vector (same as the
reuse path). The antecedent itself is named in the decision; the
binding is the whole point.

## 5. Attribute-name resolution

`attr_label` ∈ {`instance_of`, `from`, `to`, `with`, `at`,
`property`, …} from extractors. Resolve to an `ElementId` by the
algorithm in §11.7 of the spec:

1. **Exact-name** lookup against `by_name`. Hit → reuse.
2. **Universal cosine** search across all attribute-name Elements
   (every Element whose ID appears as `Attribute.name` somewhere in
   `relations`). On hit ≥ `policy.attribute_name_dedup_threshold`,
   reuse the top hit. Relation marked `Defeasible` (surface form ≠
   canonical name).
3. **Mint** on miss. Mark every relation using this new attribute
   name as `Defeasible` until replay reinforces or prunes.

**Tantivy index (§15.1) is deferred** — exact-match on
`by_name` covers step 1 today; the universal cosine over
attribute-name elements is `O(P)` with `P` ≈ 30 (seeded
attribute names), tractable inline.

**Open issue: `with` and `at` are seeded only as Void
adpositions.** `relation_patterns.rs` emits them as `attribute_name`
strings, but `by_name["with"]` resolves to `VOID_ADP_WITH`, not a
Signal attribute-name element. Two options:

- **(A)** Mint Signal siblings on first use (so the seed pack
  gains `with`/`at`/`from`/`to` as Signal attribute names through
  v0 use). `from`/`to` already exist as Signal; only `with`/`at`
  are gaps.
- **(B)** Patch the seed pack to seed Signal `with` and `at`
  attribute-name elements alongside the Void adposition entries.
  No surface-form collision because `by_name` already keys
  `Vec<ElementId>` and Step 4 routes to Signal vs Void by region.

**Recommendation: (B).** Cleaner; keeps attribute-name vocabulary
deliberate rather than emerging from a routing accident.

## 6. Relation assembly

Each surviving proposal becomes **one** `Relation`. The attribute
list shape depends on the proposal arity.

### 6.1 Binary `instance_of` proposals (NER + Temporal)

```text
Relation {
    attributes: [
        Attribute { name: subject_attr,     value: Term::Element(span_elem) },
        Attribute { name: instance_of_attr, value: Term::Element(label_elem) },
    ],
    status,        // from confidence vs policy.ner_assertion_threshold
    stats: MemoryStats {
        confidence: policy.default_conf * confidence,
        plasticity: 1.0,
        ..Default::default()
    },
    priority: 0,
    created_at: hg.clock,
}
```

`label_elem` is resolved by the same path as element minting (§4),
with the label text as the surface form. Type-class elements
(`person`, `place`, etc.) are seeded under their respective regions
so the resolver hits exact-name early.

### 6.2 Binary pattern RE proposals (`RelationProposal`)

```text
Relation {
    attributes: [
        Attribute { name: subject_attr,  value: Term::Element(subj_elem) },
        Attribute { name: attr_name_id,  value: Term::Element(obj_elem)  },
    ],
    ...
}
```

`attr_name_id` resolved via §5. `subj_elem`/`obj_elem` already exist
as Elements minted from the NER pass for the same spans.

### 6.3 N-ary event relations

The dentist worked example wants one Relation with five attributes
(subject + four role slots — the §11.9 shorthand
`reschedule_event_1 [target, property, from, to]` expands to
`[subject: reschedule_event_1, target, property, from, to]`):

```text
Relation {
    attributes: [
        { subject,  reschedule_event_1 },   // freshly minted event element
        { target,   appointment_1      },
        { property, date               },
        { from,     Tuesday            },
        { to,       Friday             },
    ],
    status: Asserted,
    stats: {
        confidence: policy.default_conf * min(from_conf, to_conf),
        plasticity: 1.0,
        ..Default::default()
    },
}
```

Plus the companion typing relation:

```text
Relation {
    attributes: [
        { subject,     reschedule_event_1 },
        { instance_of, change_event       },   // or reschedule_event if seeded
    ],
    status: Entailed,
    ...
}
```

Pattern RE today emits **separate** binary `from`/`to` proposals
(two `RelationProposal` quads). Step 8 adds a **merge pass** that
groups verb-anchored proposals over the same subject and synthesizes
the n-ary Relation.

**Required change to `RelationProposal`** (in `src/steps/relation_patterns.rs`):

```rust
pub struct RelationProposal {
    // ...existing fields...
    /// Surface verb that anchored this proposal, when the template was
    /// verb-anchored (e.g., "changed", "moved", "rescheduled"). `None`
    /// for templates like `X's Y` that have no verb. The merge pass in
    /// Step 8 reads this to group co-event proposals.
    pub event_anchor: Option<String>,
}
```

**Merge rule (Step 8 inner pass):**

1. Group `known.relations` by `(subject_elem, event_anchor)` keeping
   only groups where `event_anchor.is_some()` AND the group contains
   **both** `from` AND `to` attributes. (One-sided `from`-only or
   `to`-only stays binary — it's not a state-change frame.)
2. For each surviving group:
   - Mint a fresh event element. Name: `<verb>_event_<tick>_<seq>`
     (e.g., `change_event_42_0`). Embedding from
     `embed_span_in_context` over the anchor verb's span.
   - Build the n-ary Relation with `[subject: event_elem, target:
     subj, property: <inferred>, from: val1, to: val2]`.
   - Build the typing Relation: `[subject: event_elem, instance_of:
     <verb-kind>]` where `<verb-kind>` resolves to a seeded event
     kind (`change_event`, `reschedule_event`) if `event_anchor`
     maps to one (lookup table in `build_relations.rs`), else mints
     a new kind element.
   - **Drop** the source `from`/`to` `RelationProposal`s — they're
     subsumed.
3. Apply the `intervened` convention (§11.7 in spec): if
   `event_anchor` ∈ {`reschedule`, `move`, `set`, `configure`,
   `decide`, `cancel`, `ship`, `revert`, `merge`, `deploy`,
   `delete`}, emit a meta-relation `[target: <n-ary rel>,
   intervened: <verb_elem>]`.

**`property` inference.** For v0:
- If `from`/`to` values are both weekday or month elements →
  `property = date` element (seed it if missing).
- If both are time-of-day elements → `property = time`.
- If both are quantity elements → `property = amount`.
- Otherwise → `property = value` (generic seeded element).
This is a lookup table in `build_relations.rs`; precise enough for
seed-pack frames, replay can refine.

**Non-event `from`/`to`** (e.g., "Flight from JFK to LAX") have
`event_anchor: None` because no verb-anchored template matched —
they stay as two binary relations. Same path as Q1 single-span
mints: replay can promote later.

### 6.4 Novelty relations

`NoveltyRelation` carries text-only subject/attribute/object —
nothing's been resolved against the graph yet. Run §4 mint/dedup
on the subject + object spans, run §5 on the attribute text,
assemble a Relation exactly like §6.2 but **always `Defeasible`**.
The novelty branch is "we noticed surface structure that looks
relational; replay will decide if it's real."

## 7. Source meta-relation

If `source` is `Some(source_id)`, every base Relation built this
tick gets a companion:

```text
Relation {
    attributes: [
        Attribute { name: target_attr, value: Term::Relation(R)         },
        Attribute { name: source_attr, value: Term::Element(source_id)  },
    ],
    status: RelationStatus::Entailed,
    stats: MemoryStats {
        confidence: 1.0,    // trust the wiring, not the extractor
        plasticity: 0.5,
        ..Default::default()
    },
    ...
}
```

Implementation: build the base relations first, accumulate their
IDs in a `Vec<RelationId>`, then sweep over that vec to emit the
companions. Keeps the loop boring.

## 8. Index updates

Strictly incremental. Every base + meta relation runs this on the
way out:

```rust
fn index_relation(hg: &mut Hypergraph, r_id: RelationId, r: &Relation) {
    for attr in &r.attributes {
        // relations_by_attribute_name
        hg.relations_by_attribute_name.entry(attr.name).or_default().push(r_id);
        match attr.value {
            Term::Element(e) => {
                hg.relations_by_element.entry(e).or_default().push(r_id);
                *hg.attribute_value_counts.entry((attr.name, e)).or_insert(0) += 1;
            }
            Term::Relation(parent) => {
                // This is a meta-attribute. Cross-reference both sides.
                if attr.name == hg.target_attr {
                    hg.meta_relations_by_subject.entry(parent).or_default().push(r_id);
                } else {
                    hg.meta_relations_by_object.entry(parent).or_default().push(r_id);
                }
                hg.meta_relation_presence.insert((parent, attr.name), true);
            }
        }
    }
    // attribute_co_counts: every ordered pair within the attribute list.
    for i in 0..r.attributes.len() {
        for j in 0..r.attributes.len() {
            if i == j { continue; }
            *hg.attribute_co_counts
                .entry((r.attributes[i].name, r.attributes[j].name))
                .or_insert(0) += 1;
        }
    }
}
```

`target_attr` is cached on `Hypergraph` like `subject_attr` (already
present). Add the field at seed-load time.

These indices are **derived** — the seed bin format doesn't change,
and `rebuild_indices` adds the build-from-scratch path for tests
and replay.

## 9. Polarity inheritance

Newly minted elements inherit `Polarity` from the region(s) Step 4
routed the input into:

- **Signal active** dominant (cosine sum across active Signal
  regions > Void) → mint as `Polarity::Signal`.
- **Void active** dominant → mint as `Polarity::Void`.
- **All routes failed leaf_vigilance** (`branch_unrouted`) → mint
  as `Polarity::Signal` and push a `DiffuseRouting` uncertainty
  signal.

In practice, content extractors only see content tokens (Step 5a
already stripped Void surface forms via `void_filter`), so this
should almost always resolve to Signal. The check exists to keep
the rule explicit and prevent surprise mints into the Void region.

## 10. Confidence + status

Status from extractor confidence:

```text
status = if conf >= policy.ner_assertion_threshold {
    RelationStatus::Entailed
} else {
    RelationStatus::Defeasible
}
```

(NER and Temporal already set this in `ExtractionProposal.status`.
Pattern RE and Novelty also set it. Step 8 honors what's there
rather than recomputing.)

Confidence stamped on the Relation:

```text
stats.confidence = policy.default_conf * extractor_confidence
```

With default policy (`default_conf = 1.0`), this is just the
extractor confidence. Intent-modulated `default_conf` (Step 2)
shifts this — hedged inputs drop it, confident statements raise.

## 11. Streaming-centroid call-site

§3.3 of `contextualized_embeddings_plan.md` deferred the call to
Step 7 or 8. **Step 8 owns it.** Every element reuse (§4.1 / §4.2 /
§4a) folds the new mention's contextualized vector via
`fold_streaming_centroid`. That's the function the embedding plan
described and the centroid update lands here.

## 12. Open questions

### Q1. Span-element minting for compound spans — RESOLVED

NER returns `"My dentist appointment"` as one span; Step 8 mints
**one** element with that surface form as its name. This is
intentional — the span is one logical thing in the input, and
keeping it whole preserves the speaker's framing. Compositional
decomposition (`dentist` + `appointment` as separate elements
linked via a compound-noun relation, §24.8) is a post-v0
refinement.

### Q2. Coref + element-fold interaction

If coref resolves "Dr. Rao" in tick 2 to the same element minted
in tick 1, the fold uses tick 2's contextualized vector (which
reflects tick 2's surrounding text). If tick 1 said "Dr. Rao
called me" and tick 2 says "I rescheduled my meeting with Dr.
Rao", the centroid drifts toward the "meeting"-flavored context.
That's intentional — the element's embedding represents how it's
used over time. Just want to flag that **coref ≠ no-op** for the
embedding.

### Q3. Mint-rate observability

§11.7 says "log priority flag if mint count > 5". v0 implements
this as `Step8Output.attr_names_minted` and an `eprintln!` in
`lib.rs::run()` when it exceeds the threshold. Real telemetry
(replay queue priority flag) lands when replay arrives.

## 13. Phased rollout

### Phase 1 — Indices + plumbing

- Add the 7 new indices to `Hypergraph` (§3). All `HashMap`-based,
  defaults to empty in `Hypergraph::default()`.
- Cache `target_attr` on `Hypergraph` alongside the existing three.
- Extend `rebuild_indices` to populate the new indices from
  `hg.relations`. Add an idempotency test mirroring the existing
  one.
- Seed-pack patch: add Signal `with` and `at` attribute-name
  elements. Bump pack version to v4 (binary layout unchanged;
  element count changes by 2).

### Phase 2 — `build_relations` skeleton

- Create `src/steps/build_relations.rs`.
- Implement §4 (element mint + dedup) and §5 (attribute-name
  resolution) as standalone helpers.
- Implement §6.1 + §6.2 (binary instance_of + pattern RE) and §7
  (source meta-relation).
- Wire into `lib.rs::run()` after `apply_region_delta`.
- Print helper for `Step8Output` so the dev-time tick output
  shows what landed.

### Phase 3 — N-ary event merging

- Add `event_anchor: Option<String>` to `RelationProposal`.
- Patch `relation_patterns.rs` so verb-anchored templates
  (`X <verb> from Y to Z`) populate `event_anchor`; non-verb
  templates leave it `None`.
- Seed-pack patch: add `date`, `time`, `amount`, `value`,
  `change_event`, `reschedule_event` as Signal Element kinds
  under appropriate regions if not already seeded (bundle with
  the Phase 1 v3 → v4 bump).
- Implement the merge pass in `build_relations.rs` per §6.3.
- Emit the `intervened` meta-relation when the verb matches the
  intervention lexicon.

### Phase 4 — Novelty branch

- Implement §6.4 — mint elements + relations from
  `NoveltyExtractions`. All `Defeasible`. Novelty triples don't
  go through the n-ary merge pass — they ship as binary triples
  pending replay confirmation.

### Phase 5 — Coref override

- Implement §4a — coref short-circuits the mint path.
- No-op today (coref is a stub), but the wiring lands so when
  §11.11 lights up `recent_focus`, coref decisions immediately
  start binding correctly.

### Phase 6 — Integration test

- A worked example test against the dentist sentence (§11.9 spec
  example). Assert:
  - 4 minted entities (`Dr. Rao`, `appointment`, `Tuesday`,
    `Friday`) — plus whatever else NER catches.
  - 1 minted event element (`change_event_<tick>_0`).
  - 4 `instance_of` relations on the entities + 1 for the event.
  - 1 n-ary event relation with 5 attributes.
  - 1 `with` binary relation (`appointment with Dr. Rao`).
  - 1 `intervened` meta-relation (`changed` ∉ intervention
    lexicon → actually omitted; verify it's NOT present).
  - All indices updated; `relations_by_element[appointment]`
    contains the expected relation set.

### Out of scope (post-v0)

- **Compound-noun decomposition** (Q1 option A, §24.8) — Q1 confirms
  whole-span minting is intentional for v0; decomposition is a
  later refinement.
- **Multi-event sentences** — "moved from A to B and from C to D"
  emits two n-ary events. The merge pass groups by
  `(subject, event_anchor)` so this *should* work mechanically,
  but the test surface is thin until we have such inputs.
- **Replay-driven attribute-name dedup** — the synchronous cosine
  check is the v0 dedup; replay does the deeper sweep later.
- **Tantivy lexical index** — exact-match on `by_name` is enough
  for v0; tantivy lands when lexical recall starts mattering.
