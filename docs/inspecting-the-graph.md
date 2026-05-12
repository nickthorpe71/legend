# Inspecting the hypergraph

Tooling for "what's actually in the graph right now?" — both the seeded
day-zero substrate and (eventually) post-tick mutated state.

## Current: Markdown snapshot

```bash
cargo run --release --example dump_hypergraph_md
# → writes inspect/seed.md  (gitignored)
```

The generator takes any `&Hypergraph` and writes a markdown report.
Open in any previewer; renders inline on GitHub via Mermaid.

### What the report contains

| Section          | What it shows                                                                                                                        |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Summary          | Element category counts (anchor / attribute-name / region / frame / class / prototype / minted) + per-attribute relation counts      |
| Anchors          | The four boot-time IDs (VOID, GENESIS, REGION_CLASS, REFERENCE_FRAME_CLASS)                                                          |
| Region DAG       | Mermaid diagram — GENESIS at root, region children, prototype attachments. Color-coded: regions blue, prototypes green, anchors gold |
| Regions          | Table: name, ID, parent(s) with weight, prototype(s)                                                                                 |
| Attribute Names  | Grouped by category (Ontology / Meta-relation / Region structural / Generic participant / Behavioral modal / Causal-relation)        |
| Reference Frames | Table: name, ID, all surface forms                                                                                                   |
| Prototypes       | Table: name, ID, owning region, embedding magnitude (sanity check: should be ≈ 1.0 since MiniLM normalizes)                          |
| Relations        | Grouped by non-subject attribute name; each entry `subject → object (status, conf)`                                                  |

### When to use

- **After regenerating `graph.bin`** — eyeball the new state to catch
  schema drift before it lands.
- **As a diff target** — commit a snapshot, regenerate, `git diff
inspect/seed.md` to see exactly what changed.
- **As a debugging aid** — when investigating why routing missed a
  region or why a prototype attached to the wrong parent.
- **Once tick mutations land** — pass a deserialized snapshot to
  `dump_hypergraph_md::render` and dump it. Same report shape; the
  "Minted" category will populate with extractor-born elements.

### Limitations

- Static. No pan/zoom/click/filter.
- Mermaid scales to ~50 nodes cleanly, ~200 nodes pushing it. Beyond
  that, switch to the planned HTML viewer.
- No tick streaming.

## Files

| Path                             | What                                                                                                     |
| -------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `examples/dump_hypergraph_md.rs` | Generator. `render(&Hypergraph) -> String` is reusable from anywhere                                     |
| `inspect/`                       | Output dir, gitignored. Add to it via `--name` flag if added later                                       |
| `src/types.rs`                   | `ElementId` / `RelationId` derive `Ord` + `PartialOrd` so the generator can produce deterministic output |

## Categorization

The generator categorizes elements by deriving from the relation graph,
not by ID range — so it'll continue to work after tick mutations
introduce new mints into the same ID space:

- **Anchor** — equals `hg.void` or `hg.genesis`
- **Class** — equals `hg.region_class` or `hg.reference_frame_class`
- **Region** — appears as a key in `hg.region_parents`
- **Frame** — has an `instance_of` relation pointing at
  `reference_frame_class`
- **Prototype** — appears in `hg.region_prototypes` values
- **Attribute name** — either appears as the `name` slot of any
  attribute, or its canonical name matches one of the 30 hardcoded seed
  attribute surface forms (catches seeded names that haven't been
  _used_ in a relation yet, like `valid_from`)
- **Minted** — anything left over (post-tick extractor output)

Attribute-name → group mapping (Ontology / Meta-relation / etc.) is
hardcoded in `attribute_group()` in the generator. When adding new
seed attribute names, update that table to keep grouping accurate.

## Roadmap

The MD report is the v0 inspection tool. Two larger pieces planned:

### Self-contained HTML viewer (next)

A single double-clickable `.html` file with Cytoscape.js + the data
embedded as a JS literal. Pan, zoom, click-to-highlight, filter by
attribute name, search by element name. DAG edges always rendered
distinctly (bold, colored) regardless of view mode. Cross-platform via
the browser; no install. Visual style will lift from the legend-v1
terminal aesthetic in `../legend-v1/legend-viz/frontend/src/index.css`
(amber `#f0a030` on `#0a0a0f` background, JetBrains Mono, scanline +
grid overlays).

### Tick streaming + scrubber (later)

Mirroring legend-v1's `legend-viz` shape — Rust+Axum+WebSocket backend
streams trace events; React+xyflow+Tailwind frontend renders a
timeline scrubber with playback controls (← LIVE → buttons + clickable
event bar). Each step's payload renders in a detail pane with
key-value tables, embedding previews, similarity scores. Useful once
the tick pipeline has multiple steps mutating substantive state.

For now, MD snapshots cover the "what's in the graph?" question
without committing to either of those tools.
