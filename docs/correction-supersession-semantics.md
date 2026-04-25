# Correction / supersession semantics — design (#37)

**Recorded:** 2026-04-24

Closes queue item #37: "Correction / supersession semantics."
Sibling to #36 (frame-relative contradictions) and concrete
follow-on to the truth-maintenance design in #35. Frames carve up
truth by *scope*; correction / supersession carves it up by *time*.

## The problem

Decisions revise other decisions. "DECISION: chose tokio" today
might be replaced by "DECISION: switched from tokio to async-std
because of compile-time regressions" tomorrow. Both are facts. The
older one is not *wrong*; it is *historical*. Naïve contradiction
counting would either:

- **Mis-flag the old decision as wrong** — destroying the audit trail.
- **Treat both as live beliefs** — leaving retrieval ambiguous.

The cognitive model: belief retraction is normal; the system
should mark which belief is current and keep the prior belief
queryable as historical context.

## What's in place today

- `FactPolarity::Corrective` (`src/memory/wernicke/extract.rs:304`)
  — extractor produces this when it sees correction / supersession
  language ("superseded", "supersedes", "now uses", "switched
  to" — see `wernicke/lexicon.rs`).
- `GraphEdgeSemantics::correction_count` (`neocortex.rs:117`).
  Incremented when `merge_edge_semantics` sees a Corrective
  polarity input (`neocortex.rs:948`).
- `conflict_state_for` returns `"Corrected"` whenever
  `correction_count > 0` (`neocortex.rs:990`).

So the *count* exists. What's missing is:

1. **Authority** — which evidence is the current belief vs the
   superseded one?
2. **Order** — in what order did supersessions occur?
3. **Read-side surfacing** — how does the LLM know the live
   belief vs the historical record at retrieval time?

## Design

### 1. Add a `supersession_chain` field

Extend `GraphEdgeSemantics`:

```rust
pub supersession_chain: Vec<SupersessionStep>,

pub struct SupersessionStep {
    pub clock: u64,            // brain clock at the time of correction
    pub evidence_ref: u64,     // L2 entry that supplied the correction
    pub previous_polarity: String,
    pub new_polarity: String,
}
```

Append-only. The latest entry is the current authority. The chain
preserves history without conflating it with live truth.

### 2. Promote `correction_count` to derived state

`correction_count` becomes `chain.len()`; the explicit field stays
as a back-compat cache populated from the chain on load. Same
pattern recommended for `confidence` in #35.

### 3. Polarity becomes "the polarity of the latest chain step"

`merge_polarity` today does ad-hoc string mixing (`neocortex.rs:976`).
With a chain, `polarity` is just `chain.last().map(|s| s.new_polarity)
.unwrap_or(initial_polarity)`. The merge function appends rather
than mutates.

### 4. Retrieval distinguishes live vs historical

Two retrieval modes:

- **Default**: query returns only the current belief. Older
  superseded beliefs are filtered out unless asked for.
- **Historical**: caller passes `--include-history` (or MCP
  equivalent); query returns the chain so the LLM can reason about
  *why* the belief changed.

Surfaces in `MemoryContext` as an optional `supersession_chain` on
the `GraphHit` shape. CLI gets a flag; MCP gets a parameter.

### 5. Salience interaction (#33 follow-on)

`compute_graph_prediction_error_score` sees a Corrective polarity
today and adds 0.20. With supersession-aware semantics:

- Corrective polarity that *agrees with the current chain head*:
  +0.05 (reaffirming the latest correction).
- Corrective polarity that *disagrees with the current chain head*:
  +0.40 (a correction *of* the correction is a strong signal).
- Corrective polarity introducing a new revision: +0.20 (today's
  behavior).

This keeps PE responsive to the evolving belief, not just the
count.

## Migration

Existing edges have no chain. On load:

- If `correction_count > 0` and `supersession_chain.is_empty()`,
  synthesize one synthetic step at clock 0 with
  `previous_polarity = ""` and `new_polarity = polarity`.
- Future writes use the chain directly.

Costs one synthetic entry per pre-migration corrected edge. Cheap.

## Phases

| Phase | Deliverable                                                |
|-------|------------------------------------------------------------|
| 1     | Add `supersession_chain`; migration shim; tests             |
| 2     | Switch `merge_edge_semantics` from string-mixing to append  |
| 3     | Retrieval flag (`--include-history`) and CLI / MCP surface  |
| 4     | PE-score updates in #33 to consult chain head               |

## What this doc is NOT

- Not implementation. Phases get their own queue items.
- Not a final commitment to `Vec<SupersessionStep>` — if a future
  phase finds the field bloats serialized state for the common
  case (`correction_count == 0`), wrapping in `Option<Vec<...>>`
  with `#[serde(default, skip_serializing_if)]` is acceptable.

## Related

- `docs/semantic-truth-maintenance.md` (#35): broader design.
- `docs/frame-relative-contradictions.md` (#36): scope axis.
- `src/memory/neocortex.rs::merge_edge_semantics` and
  `merge_polarity`: the current correction handling.
- `src/memory/wernicke/extract.rs::FactPolarity::Corrective` and
  `wernicke/lexicon.rs` correction cue list.
- `src/memory/thalamus.rs::compute_graph_prediction_error_score`
  (#33): the salience read-side.
