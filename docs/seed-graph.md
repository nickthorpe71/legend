# Seed graph (boot-time hypergraph)

Legend boots with a seeded `Hypergraph` containing 622 elements and
610 relations — the substrate's day-zero state. Without this, Step 4
region routing has nothing to descend into, the token-level void
filter has no closed-class anchors to consult, and nothing minted by
extractors has a parent in the DAG. This doc describes the YAML →
bin → runtime pipeline and how to modify the seeds.

Source-of-truth design: `new_foundation.md` §10 (region topology) and
§12 (seed pack); `new_foundation_v0_core.md` for the v0 scope.

## Pipeline

```
seed_pack.yaml
 │
 │  cargo run --release --example gen_seed_graph
 │     ├── load_seed_pack()       (serde_yaml, dev-deps only)
 │     ├── mint elements          (VOID=0, GENESIS=1, attrs, regions, frames, classes, prototypes, void members)
 │     ├── embed_text()           (MiniLM INT8 BERT — one call per prototype + void member)
 │     ├── synth relations        (region_class_pins + frame_class_pins + region_parent_pins
 │     │                           + prototype_attach + void_member instance_of pins)
 │     └── write little-endian bin
 ▼
src/seed/graph.bin   (~1 MB; committed)
 │
 │  cargo build → include_bytes!()
 ▼
seed::load_seed_graph() → Hypergraph
 │
 ▼
seed::rebuild_indices(&mut hg)   (by_name, region_children, region_parents, region_prototypes, region_stats)
```

## What boots

```
2    anchors           VOID (id 0), GENESIS (id 1)
30   attribute names   ontology, meta-relation, region structural,
                       generic participant, behavioral modal, causal
14   signal regions    entities, events, change_history,
                       relationships, quantities, time, locations,
                       tasks, decisions, preferences, definitions,
                       provenance, domains, modal_negated
8    void regions      determiners, adpositions, coord_conj,
                       subord_conj, pronouns, auxiliaries,
                       particles, interjections
                       (children of VOID; carry Polarity::Void)
8    reference frames  user, project, domain, session, temporal_now,
                       temporal_past, temporal_future, meta
2    classes           REGION_CLASS, REFERENCE_FRAME_CLASS  (eagerly
                       minted; YAML treats them as lazy)
440  prototypes        20 examples per region (22 × 20); embedding =
                       MiniLM(example)
118  void members      closed-class stop words (the / of / and / …)
                       seeded under the 8 void regions; carry
                       Polarity::Void

610 relations         22 region_class_pins + 8 frame_class_pins
                    + 22 region_parent_pins + 440 prototype_attach
                    + 118 void_member instance_of pins
```

Element IDs are assigned sequentially in mint order, so the layout is
stable across regenerations as long as the YAML's element order is
stable. `void: ElementId(0)` and `genesis: ElementId(1)` are part of
the `Hypergraph` contract — the generator asserts this invariant.

## Files

| Path                         | What                                          |
| ---------------------------- | --------------------------------------------- |
| `seed_pack.yaml`             | Seed elements + relations + intent prototypes |
| `examples/gen_seed_graph.rs` | **Generator** — produces `graph.bin`          |
| `examples/shared/mod.rs`     | Serde DTOs (dev-only)                         |
| `src/seed.rs`                | Runtime loader + `rebuild_indices`            |
| `src/seed/graph.bin`         | Tightly-packed little-endian dump (committed) |
| `src/types.rs`               | `Hypergraph` definition + anchor ID fields    |

## Binary format

Documented in full at the top of `examples/gen_seed_graph.rs`. Quick
reference:

```
HEADER (10 × u32):
  format_version, element_count, relation_count,
  void_id, genesis_id, region_class_id, reference_frame_class_id,
  subject_attr_id, parent_region_attr_id, prototype_attr_id

ELEMENTS:
  id(u32) name_count(u32) [name_len(u32) bytes]*
  embedding(384 × f32) stats(49 bytes) created_at(u64)
  polarity(u8)        (0 = Signal, 1 = Void)

RELATIONS:
  id(u32) attribute_count(u32)
  [name_id(u32) term_kind(u8) value(u32)]*
  status(u8) stats(49 bytes) priority(i8) created_at(u64)
```

`format_version = 2`. Bumped by the generator if the byte layout
changes; the runtime panics on mismatch and points at the regen
command.

Derived indices (`by_name`, `region_children`, `region_parents`,
`region_prototypes`, `region_stats`) are **never** serialized — the
relation graph is authoritative. `rebuild_indices` repopulates them
after deserialization. `region_stats` (per-region mean + variance of
prototype embeddings) is computed in the same pass, consumed by
Step 4 Mahalanobis routing.

## Regenerate

```bash
cargo run --release --example gen_seed_graph
```

Output:

```
wrote 622 elements, 610 relations to src/seed/graph.bin
  void=0 genesis=1 region_class=... reference_frame_class=...
  subject_attr=... parent_region_attr=... prototype_attr=...
```

(Exact class/attr IDs depend on YAML order; the test
`seed::tests::loads_expected_counts` pins the 622/610 totals.)

The runtime picks up the new bin at the next `cargo build` because it's
pulled in via `include_bytes!`.

## Editing the seed pack

### Add a new region

1. Add a `RawRegion` block to `seed_pack.yaml` under `regions:`. Required
   fields: `element_id`, `names`, `parent_regions: [[parent, weight]]`,
   `descriptor`, `examples`, `rationale`.
2. Add a corresponding entry to `region_class_pins.relations`:
   `[NEW_REGION, instance_of, REGION_CLASS]`.
3. Add the parent edge to `region_parent_pins.relations`:
   `[NEW_REGION, parent_region, GENESIS, 1.0]` (or another parent —
   a child of `VOID` instead makes the region a void subtree, and
   every member it owns inherits `Polarity::Void`).
4. Update the count assertions in `gen_seed_graph.rs` (search for
   `assert_eq!(elements.len()` and `assert_eq!(relations.len()`).
5. Regenerate. Tests in `src/seed.rs` will fail on count mismatches —
   update those too.

Each `examples` entry becomes a prototype Element whose embedding
is `MiniLM(example)`; the per-region prototype set drives Step 4
routing (mean of top-K cosine + diagonal Mahalanobis against
`region_stats`). The `descriptor` field is documentation only at
runtime — useful for the inspection MD dump and as a human-readable
summary, but not embedded. Replay (§14.8) will drift prototypes
toward incoming inputs over time; boot quality matters for the
first dozen ticks.

### Add a new void member (closed-class stop word)

Void members live under one of the 8 void regions
(`REGION_DETERMINERS`, `REGION_PRONOUNS`, …) via the region's
`members:` list. They carry `Polarity::Void`, which is what
`src/steps/void_filter.rs` keys on to drop function-word tokens
from Step 5a's content stream.

1. Find the right void region in `seed_pack.yaml` and append a new
   entry to its `members:` list:
   `{ element_id: VOID_DET_FOO, names: ["foo"] }`. Each member
   becomes one Element (polarity Void) with a single
   `instance_of <REGION>` pin.
2. Update count assertions and tests; regenerate.

### Add a new attribute name

1. Add to `seed_pack.yaml` under `seeded_attribute_names:`. Required
   fields: `element_id` (e.g. `ATTR_FOO`), `names: ["foo"]`, `rationale`.
2. If the attribute belongs to a recognized group (ontology / meta /
   region structural / participant / modal / causal), add the surface
   form to `attribute_group()` in `examples/dump_hypergraph_md.rs` so
   inspection reports group it correctly.
3. Update count assertions and tests; regenerate.

### Add a new reference frame

1. Add a `RawElement` block to `seed_pack.yaml` under
   `reference_frames:`.
2. Add a corresponding entry to `reference_frame_class_pins.relations`:
   `[NEW_FRAME, instance_of, REFERENCE_FRAME_CLASS]`.
3. Update count assertions and tests; regenerate.

### Modifying confidence weights

`region_parent_pins` is the only `seeded_relations` block that carries
weights (4-element tuples `[A, attr, B, w]`). Changing `1.0` → `0.5`
will halve the parent-edge confidence; routing reads
`region_parents[child]`'s `(parent, weight)` tuples directly.

## Validate

```bash
cargo test --lib seed         # 6 tests covering counts, anchors, indices
cargo run --release -- "smoke text"   # boot the pipeline end-to-end
cargo run --release --example dump_hypergraph_md
                              # produces inspect/seed.md — eyeball it
```

The seed-loader test count assertions are the first thing to fail on
schema drift — update them deliberately when adding seeds, not by
"making the test pass."

## Why bake the bin

Same pattern as `intent_classifiers/*.bin`: build-time tools live in
`examples/` with `serde`/`serde_yaml` in dev-dependencies; the runtime
crate has no `serde` and no YAML. A downloaded `legend` ships the
seeded substrate as part of the binary — no first-run YAML parse, no
network, no startup latency, no production drift between what was
trained against and what's running.
