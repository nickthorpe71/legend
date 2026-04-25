# Semantic truth maintenance — design (#35)

**Recorded:** 2026-04-24

Closes queue item #35: "Design semantic truth maintenance." Captures
the current state of belief-tracking machinery in Legend, what's
working, what's missing, and where the gaps fit on a phased roadmap.
Implementation lives behind future queue items so each piece lands
with its own conformance test and CHANGELOG note.

## What we already have

`GraphEdgeSemantics` (`src/memory/neocortex.rs:103`) carries:

- `kind` + `kinds`: the relation type(s) the edge represents
- `predicates`: surface predicates seen for this edge
- `evidence`: source phrases supporting the relation
- `contradictory_evidence`: source phrases that negated it
- `polarity`: `"affirmed"` / `"negated"` / etc.
- `support_count` and `contradiction_count`
- `correction_count`
- `conflict_state`: derived from the counts via `conflict_state_for`:
  - `"Corrected"` if any correction has occurred
  - `"Conflicted"` if both support and contradiction counts > 0
  - `"Contradicted"` if only contradictions
  - `"Supported"` otherwise
- `reference_frames`: scope qualifiers (e.g. project frame)
- `confidence`

`compute_graph_prediction_error_score` (#33, this session) reads the
above to amplify salience on incoming contradictions. So the *write*
side of truth maintenance is already richer than the *read* side
exposes.

## Roles a complete system needs

Following the AI/CS terminology for truth-maintenance systems
(JTMS / ATMS lineage), Legend's brain analog needs:

1. **Belief tracking** — for each edge, what is its current truth
   status (supported / contradicted / corrected / unknown)?
   *Status: present, via `conflict_state_for`.*
2. **Justification chains** — what evidence supports / opposes the
   belief, and where did each piece of evidence come from?
   *Status: partial. `evidence` and `contradictory_evidence` carry
   surface phrases but do not link back to the L2 entry IDs that
   produced them. A `evidence_refs: Vec<u64>` extension would close
   this gap.*
3. **Retraction propagation** — when belief X is retracted, beliefs
   that depended on X must be re-evaluated.
   *Status: not implemented. Today retraction increments
   `contradiction_count` on the directly-mentioned edge but does
   not walk the graph for derived implications.*
4. **Frame-relative truth** — facts true in one reference frame may
   be false in another (same SQLite is "the datastore" in Project
   Alpha, may not be in Project Beta).
   *Status: scaffolding present (`reference_frames`), but enforcement
   is fixture-style. #36 follows on this.*
5. **Temporal supersession** — newer beliefs override older ones
   when they contradict; older beliefs become "historical facts"
   rather than wrong ones.
   *Status: not implemented; tracked by #37.*
6. **Confidence calibration** — `confidence` should rise with
   support, fall with contradiction, but not collapse to 0 / 1
   prematurely.
   *Status: field exists; the compute path is ad hoc and lives
   inline in `merge_edge_semantics`. Worth extracting + testing.*

## Gaps to close, in priority order

### P1 — Justification IDs (small, high leverage)

Extend `GraphEdgeSemantics` with `evidence_refs: Vec<u64>` and
`contradictory_evidence_refs: Vec<u64>`. Populate from the
`MemoryRef` / L2 entry IDs that produced each evidence phrase.
Lets retrieval explain *why* it believes an edge: "Project Alpha
uses SQLite (supported by entries 1247, 1389, 1502)."

Cost: one field per edge, set at evidence-merge time. No new
inference logic. Unblocks every other gap.

### P2 — Confidence as a derived signal

Replace any inline mutation of `confidence` with a derived getter
on `GraphEdgeSemantics`:

```rust
fn confidence(&self) -> f32 {
    let s = self.support_count as f32;
    let c = self.contradiction_count as f32;
    if s + c == 0.0 { return 0.5; }
    let weighted = s.ln_1p() / (s.ln_1p() + c.ln_1p() + 1e-9);
    weighted.clamp(0.05, 0.95)
}
```

`confidence` becomes derived state; the field on the struct stays
as a cache but is recomputed on every merge. Avoids the current
ad-hoc updates and gives downstream callers a stable contract.

### P3 — Retraction propagation (graph-aware)

When a tick contradicts edge `(A --uses--> B)`:
1. Increment `contradiction_count` on `A--B` (already done).
2. Walk graph neighbors of `B` whose edges depend on the retracted
   fact (typed-relation chains: `A --uses--> B --backs--> C` should
   see C's belief in `A--C indirect-uses` weakened).
3. Update conflict state on each affected edge.

Gated behind a `retraction_propagation: bool` flag in
`MemoryConfig` so it's opt-in until it's measured. Risk: an
overzealous walker could ripple weakly-supported edges into
"Conflicted" en masse.

### P4 — Frame-relative beliefs (handed to #36)

Hook into `reference_frames`. Two beliefs with the same
`(subject, kind, object)` but distinct frames coexist as separate
edges; contradictions only fire across edges that share a frame.
Detailed design tracked in #36.

### P5 — Temporal supersession (handed to #37)

When a newer DECISION revokes an older DECISION on the same edge,
the older belief gets archived (not deleted) and the newer one
becomes authoritative. The older entry's `consolidated` /
`reference_frame.kind == "historical"` flag helps retrieval choose
the live answer while preserving the trail. Detailed design
tracked in #37.

## Phased roadmap

| Phase | What lands | Queue item |
|-------|-----------|------------|
| 1 | Evidence-ref IDs + derived confidence + tests | new, follow-on to #35 |
| 2 | Retraction propagation behind config flag    | new |
| 3 | Frame-relative truth                         | #36 |
| 4 | Temporal supersession                        | #37 |
| 5 | Public truth-status API surface (`legend memory truth <subject>`) | new |

## Read-side surface (future)

A new read-only query surface would let an LLM ask "what is the
current truth state of `(Project Alpha, uses_datastore, SQLite)`?"
and receive:

```json
{
  "subject": "Project Alpha",
  "kind": "uses_datastore",
  "object": "SQLite",
  "state": "Supported",
  "confidence": 0.83,
  "support_count": 12,
  "contradiction_count": 0,
  "supporting_evidence_ids": [1247, 1389, 1502],
  "contradicting_evidence_ids": [],
  "frame": "Project Alpha"
}
```

Out of scope for this design doc; tracked as Phase 5 above.

## What this doc is NOT

- Not an implementation. Each phase needs its own queue item, its
  own conformance test, and its own commit. Designing the whole
  system in one autonomous slice would land too much speculative
  code.
- Not a final spec. It's grounded in the current
  `GraphEdgeSemantics` shape; if a later phase finds that shape
  insufficient (e.g. multi-frame edges need a separate keying
  scheme), the roadmap shifts and this doc moves with it.

## Related

- `src/memory/neocortex.rs::GraphEdgeSemantics` and
  `merge_edge_semantics` — the scaffold this builds on.
- `src/memory/thalamus.rs::compute_graph_prediction_error_score`
  (#33) — the read-side that already exploits these counts.
- #36 (frame-relative contradictions): a direct follow-on.
- #37 (correction/supersession semantics): the time axis of this
  same problem.
