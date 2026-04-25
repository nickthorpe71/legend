# Frame-relative contradictions — design (#36)

**Recorded:** 2026-04-24

Closes queue item #36: "Distinguish frame-relative contradictions."
Builds on the truth-maintenance design in #35.

## The problem

A statement like "SQLite is the canonical datastore" is true *in
the Project Alpha frame* but may be false elsewhere — Project Beta
might use Postgres, in which case "Project Beta uses_datastore
SQLite" should not contradict "Project Alpha uses_datastore SQLite".

Today, `merge_edge_semantics` (`src/memory/neocortex.rs:926`)
collapses all evidence for `(A --uses_datastore--> B)` into a
single `GraphEdgeSemantics` regardless of frame. Frames *are*
recorded (the `reference_frames: Vec<GraphReferenceFrame>` field
exists) but they don't gate which evidence rolls up into which
semantics bucket. Two ticks talking about different projects can
incorrectly increment each other's `support_count` or
`contradiction_count`.

## What "frame" means here

`GraphReferenceFrame` (`src/memory/neocortex.rs:122`) carries
`kind`, `label`, `relation`, `confidence`. In practice the frame
that matters is whatever scopes a fact: a project name, a
service identifier, a time window, a deployment environment.

This design treats frame identity as the tuple `(kind, label)` —
e.g. `(Project, "Project Alpha")` or `(Environment, "staging")`.
Two facts with overlapping frames can support / contradict each
other; two facts with no shared frame are independent.

## Design

### 1. Frame-keyed semantics

`merge_edge_semantics` keys evidence by **edge + frame** rather
than just by edge. Three keying options, in order of complexity:

| Option | Storage | Lookup | Contradiction scope |
|--------|---------|--------|---------------------|
| A. Single semantics, frame-tagged evidence | one `GraphEdgeSemantics` per edge | unchanged | always per-edge (today) |
| B. Multiple semantics per edge, indexed by frame | `HashMap<frame, GraphEdgeSemantics>` per edge | new `(edge, frame)` lookup | per-frame |
| C. Separate edges per frame | edges become `(from, to, kind, frame)` quadruples | major change to edge model | per-frame |

**Recommendation: Option B**, with frame keying logic in a small
helper module. It's the smallest change that gives correct
semantics; the `evidence_count` cap stays per-frame so heavily
discussed frames don't crowd out rarely discussed ones.

### 2. Default frame

Most existing ticks have no explicit frame. We need a
convention for those:

- **Default frame**: `(kind: "default", label: "")`. Used when
  the extractor returns no frames.
- Two unframed facts continue to behave exactly as today
  (back-compat for the existing test suite).
- A framed fact and an unframed fact never directly contradict;
  the unframed fact is treated as ambient (could be either
  frame).

### 3. Contradiction predicate

```text
fn contradicts(a: &GraphEdgeSemantics, b: &GraphEdgeSemantics) -> bool {
    let polarity_disagrees = a.polarity != b.polarity
        && a.polarity != "Unknown"
        && b.polarity != "Unknown";
    let frames_overlap = a.reference_frames.iter().any(|fa| {
        b.reference_frames.iter().any(|fb| fa.kind == fb.kind && fa.label == fb.label)
    }) || a.reference_frames.is_empty() || b.reference_frames.is_empty();
    polarity_disagrees && frames_overlap
}
```

`reference_frames.is_empty()` falling through means an unframed
fact still counts toward contradiction so we don't silently drop
the existing semantics — but only one side may be unframed.

### 4. Salience changes

`compute_graph_prediction_error_score` (#33) currently picks the
edge by node IDs alone. Once frame-keyed semantics ship, it
should:

1. Extract incoming relation's frames (today: `relation.reference_frames`).
2. Look up the per-frame `GraphEdgeSemantics` bucket if one
   exists; fall back to the unframed bucket if not.
3. Apply the contradiction / novel / reaffirmation logic against
   that bucket.

That keeps PE scoring consistent with the storage model.

## Migration path

Existing graphs have one `GraphEdgeSemantics` per edge. Migration:

1. On load: if `edge_semantics` is the legacy single-bucket shape,
   wrap it under the default frame key.
2. New writes hit per-frame buckets.
3. A maintenance pass during consolidation (idle worker, like #32)
   walks any edge with both an `evidence` entry and an
   `evidence_with_frame` entry and rolls the legacy single bucket
   forward.

This avoids a one-shot migration that would lock the loader.

## What this doc is NOT

- Not implementation. Implementation lands behind a follow-on
  queue item with its own conformance test.
- Not a final decision on Option B. If migration cost or query
  complexity blows up, falling back to Option A with frame-tagged
  evidence (single bucket, but evidence carries its frame) is
  acceptable. Option C is too invasive without strong demand.

## Phases

| Phase | Deliverable |
|-------|-------------|
| 1     | Add per-frame keying to `merge_edge_semantics` (Option B); migration shim; tests |
| 2     | Update `compute_graph_prediction_error_score` to consult per-frame buckets |
| 3     | Public read surface that reports truth state per frame |

## Related

- `docs/semantic-truth-maintenance.md` (#35): the broader design.
- `src/memory/neocortex.rs::GraphEdgeSemantics` and
  `merge_edge_semantics` — current semantics merge.
- `src/memory/wernicke/extract.rs::ExtractedRelation::reference_frames`
  — extractor's frame output.
- #37 (correction/supersession semantics): the time-axis sibling
  of frames.
