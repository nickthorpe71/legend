# Contextualized-span-pool element embeddings

> **Status: complete (v0).** All five phases landed in commits
> `87fbf89` → `6d51466`. Final measurement: production routing on
> the regenerated seed pack hits **67% top-1** on the v3 18-case
> fixture (up from 44% baseline). Polarity stays — the 85%
> cosine-only separation isn't clean enough to deprecate it.
> The remaining gap to v3's 83% comes from prototypes being
> phrase-level rather than span-level; closing it is a future
> seed_pack.yaml refactor (mark focal entity per example phrase).
>
> This document is kept as the design record. The code is canonical
> (see `src/embed.rs::{embed_span_in_context, fold_streaming_centroid}`,
> `examples/{route_seed_pack_test, polarity_separation_test}.rs`).

## 1. Goal

Replace each `Element`'s degenerate `embedding = MiniLM(name)`
with `embedding = mean-pool of contextualized span tokens` —
pulled from the same MiniLM forward pass we already run for
input-level routing. Over multiple mentions of the same entity,
fold each mention's contextualized vector into the stored
embedding via a streaming centroid.

The element vector ends up representing **how this entity is
typically used in real input**, not just what its surface form
looks like. Substrate routing, merging, and supersession all get
sharper without any of the cost of LLM-authored descriptions.

## 2. Why this works (empirically)

`examples/route_element_test_v3.rs` runs the same 18 test cases
against three anchor strategies. Top-1 accuracy:

| Strategy | What the anchor is | Top-1 |
|---|---|---|
| A | `embed_text(region_name)` — current baseline | 8/18 (44%) |
| B | mean of `embed_text(member)` — bare-word centroid | 14/18 (78%) |
| C | **mean of contextualized span pools** | **15/18 (83%)** |

Strategy C's remaining 3 failures are arguably legitimate
("`Dr. Rao` in `Dr. Rao changed the appointment`" routes to
`change_history` — `changed` is the surface-dominant signal in
that sentence). The lift from A → C is the architectural win.

## 3. End-state architecture

### 3.1 Span pool helper

A single new helper in `src/embed.rs`:

```rust
/// Embed the input text once, then mean-pool the contextualized
/// token vectors that fall inside `[char_start, char_end)`. Returns
/// `None` if the span doesn't intersect any tokenized region (e.g.,
/// span sits inside a multi-byte character or the model dropped it).
///
/// The returned vector is L2-normalized so callers can use plain
/// dot-product as cosine.
pub fn embed_span_in_context(
    text: &str,
    char_start: usize,
    char_end: usize,
) -> Option<Vec<f32>>;
```

Implementation: ~30 lines lifted directly from
`route_element_test_v3.rs::contextualized_span_embedding`,
generalized to take char offsets instead of substring search.

### 3.2 Element embedding at mint time

`mint_element` (and equivalents — there are 3-4 callsites across
`src/seed.rs`, `examples/gen_seed_graph.rs`, test helpers) takes
a new shape:

```rust
fn mint_element_in_context(
    elements: &mut Vec<Element>,
    symbol_to_id: &mut HashMap<String, ElementId>,
    symbol: &str,
    names: Vec<String>,
    /// The full input/seed-phrase the span appeared in.
    context_text: &str,
    /// Span offsets inside `context_text`.
    span_char_start: usize,
    span_char_end: usize,
    polarity: Polarity,
) -> ElementId;
```

The embedding is computed as:
- If `embed_span_in_context(context_text, start, end)` returns
  `Some(v)` → use `v`.
- Otherwise fall back to `embed_text(name)` — preserves the
  current behavior when tokenization can't recover the span.

### 3.3 Streaming centroid on re-mention

When an input re-mentions an existing element (matched by name
lookup or near-perfect cosine), fold the new mention's
contextualized vector into the stored embedding:

```rust
// Pseudocode — actual code lives in Step 7 / 8 once those phases
// land. `n` is the element's observation count (lives on
// MemoryStats.access_count or similar).
let new_obs = embed_span_in_context(input_text, start, end)?;
let n = element.stats.access_count as f32;
for i in 0..EMBEDDING_DIM {
    element.embedding[i] = (n * element.embedding[i] + new_obs[i])
                         / (n + 1.0);
}
// L2-renormalize.
```

This is a vanilla streaming mean. Over time, the embedding
stabilizes around the entity's typical usage centroid; outlier
mentions move it less as `n` grows. Replay can refine further.

### 3.4 Seed pack regeneration

Every seed prototype in `seed_pack.yaml` already lives inside a
seed phrase. The generator currently does:

```rust
embed_text(&proto_name)   // ← degenerate
```

The new generator does:

```rust
embed_span_in_context(&example_phrase, span_start, span_end)
    .unwrap_or_else(|| embed_text(&proto_name))
```

For prototypes that *are* the whole phrase (e.g., the existing
20-examples-per-region pattern), the contextualized embedding
becomes the phrase-pooled embedding — slightly different from
`embed_text(phrase)` because the latter pools over all tokens
including `[CLS]`/`[SEP]`, while ours mean-pools only content
tokens within the span. The substantive change is for **named
void members** (`the`, `of`, `and`, …): instead of degenerate
single-word `MiniLM("the")`, each gets a contextual `the`
embedding pulled from one of the seed phrases that contains it.

Seed pack format bump: **v3**. Binary layout unchanged; only the
embedding values differ.

## 4. Phased rollout

### Phase 1 — `embed_span_in_context` helper

- Lift the `contextualized_span_embedding` body from
  `route_element_test_v3.rs` into `src/embed.rs`.
- Switch from `substring search → fall-back` to
  `(start, end) → match against offsets`.
- Unit tests: round-trip a few cases against the existing v3
  fixtures, verify cosine ≥ 0.99 between the new helper and the
  v3 strategy C output.

### Phase 2 — Wire into `gen_seed_graph`

- Each region's `examples:` list provides the context text for
  the prototype it spawns. The prototype's name IS the whole
  example, so the "span" is the whole phrase. Use
  `embed_span_in_context(phrase, 0, phrase.len())`.
- For void-region `members:` entries: pick the first example
  phrase that contains the member's surface form, find the span
  via case-insensitive search, embed with the new helper. If no
  example contains the surface form, fall back to
  `embed_text(name)` and warn.
- Regenerate the seed pack. Sanity check: bytes-different from
  v2 pack, element counts unchanged at 622.

### Phase 3 — Validate routing quality on the real pack

- Run a routing test on the regenerated pack mirroring
  `route_element_test_v3.rs`'s 18-case fixture, but against the
  full seeded graph (not just the 14 hand-picked regions in v3).
- Compare top-1 accuracy: regenerated pack vs current pack. Goal:
  ≥ 75% top-1 (matches the v3 80%-ish floor, accounting for the
  larger region count).
- If accuracy drops, debug before proceeding. We don't ship a
  worse pack.

### Phase 4 — Streaming-centroid update on re-mention

- Add the streaming-mean update to whatever code path will
  eventually mint or update Elements based on Step 5 output.
- For v0 (no Step 8 yet): hold the update logic in a function but
  don't call it. Tests cover correctness; the call-site lands
  with Step 8.

### Phase 5 — Polarity / void filter re-evaluation

With description-rich (contextualized) embeddings, function words
("the" gets pooled from "the cat sat on the mat" context — a
contextual vector that smears across all of English) and content
words (`London` gets a vector specific to geographic context) may
embed naturally far apart.

- Run a clustering test on every Polarity::Void member's embedding
  vs every Polarity::Signal element's embedding. If void elements
  cluster cleanly via cosine alone (i.e., a knee in the cosine
  distance distribution separates them), deprecate the `Polarity`
  field and `void_filter` module.
- If clustering is ambiguous, keep Polarity as a low-cost backup
  filter.

## 5. Risks and open questions

### What if contextualized embeddings don't actually help in
production?

Strategy C's 83% v.s baseline 44% is on hand-picked test cases.
Production inputs may have:
- Spans that drift across many contexts (e.g., `Sarah` mentioned
  in 50 different settings) — the centroid loses sharpness.
- Spans with degenerate context (single-word inputs, just an
  entity name).

Mitigations available:
- The streaming-centroid weighting can be tuned (early mentions
  weighted higher, plateau at some max `n`).
- Fall back to `embed_text(name)` when context is too sparse.

### Multi-mention disambiguation

Two distinct elements with the same name (e.g., two "Sarah"s)
both get streaming-averaged into one embedding under the current
plan, which is wrong. Disambiguation is Step 8's job (it'll mint
a second `Sarah` element if the contextual cosine is far enough
from the existing one). For phase 1-4 we assume one name → one
element, same as today.

### Embedding stability over replay

Streaming centroid drifts continuously, which can interfere with
replay-determinism. Two fixes possible:
- Snapshot embeddings at WAL boundaries so replay reproduces
  exact byte-identical pack.
- Use a finite weighting (only last N observations contribute).
The simpler answer for v0: snapshot at WAL boundaries. Replay
reproduces from snapshot, not by re-deriving.

## 6. Out of scope

- **Per-tick LLM calls** — see `llm_extraction_plan.md` for the
  abandoned attempt.
- **Replacing GLiNER** — stays as the entity-tagger.
- **Replacing MiniLM** — stays as the embedder. The contextualized
  vectors come from MiniLM's hidden states, not a new model.
