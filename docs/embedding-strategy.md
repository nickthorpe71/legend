# Embedding strategy — batch vs whole-text (#21)

**Recorded:** 2026-04-24
**Context:** Queue item #21 asks whether Legend should embed each
chunk independently (current) or embed whole tick text once with
extractive per-chunk indexing.

## Current behavior

`tick_impl` at `src/memory/mod.rs:1218`:

```rust
let chunks = chunk_text(text);
let chunk_refs: Vec<&str> = chunks.iter().map(|c| c.as_str()).collect();
let raw_embeddings = entorhinal::embed_texts_batch(&chunk_refs, dim);
```

- Each chunk becomes its own L2 entry with its own embedding.
- `embed_texts_batch` is a loop over `embed_text` (the ORT session is
  shaped for batch size 1). The name is aspirational — it doesn't fuse
  inferences into a single forward pass.
- Typical tick has N = 1 chunk (single DECISION:/sentence prefix).
  Long or paragraph ticks produce N = 2–5.
- Embedding cache (`EMBED_CACHE` in `entorhinal.rs`) memoizes by
  `(text, dim)` so identical chunks across ticks skip inference.

## Decision

**Keep the current approach (one embedding per chunk).**
No switch to whole-text embedding.

## Why

1. **Cost is already inside budget.** The §#07 profile shows `EmbedText`
   at ~3 ms per tick warm, dwarfed by `Decay` (~8 ms) and `ChunkText`
   (~6 ms). There is no latency motivation to collapse N embeddings
   into 1.
2. **N is usually 1.** For the typical tick shape the two strategies
   are indistinguishable. They only differ on paragraph-sized inputs,
   which are rare in LLM-session use.
3. **Retrieval granularity is a real win.** Chunk-level embeddings
   let `hippocampus::top_k_similar` match a query against the specific
   sentence/paragraph that's relevant, not the whole multi-topic tick.
   Whole-text embedding would average the topics together and dilute
   matches.
4. **Extractive indexing is already happening.** `extract_entities`
   and `extract_relations` run per chunk (see `neocortex::update_graph`
   at `src/memory/neocortex.rs:1193`). The graph layer is already the
   fine-grained index the alternative proposal would add.

## What "whole-text embedding with extractive indexing" would buy

- **Cheaper L2 write** (1 embedding instead of N). Negligible since
  N is usually 1 and `embed_text` is already ~3 ms.
- **Smaller L2 footprint.** One entry per tick instead of N. Small
  saving; L2 is already bounded by `hippocampus::prune_short_term`.
- **Potential "quote the relevant chunk" retrieval UX.** But this is
  already what per-chunk L2 entries give us — the proposal regresses
  on this axis, not improves.

The balance is negative; the current design is the right one.

## When to revisit

Three conditions would flip the decision:

1. **`EmbedText` leaves the latency budget.** If graph size grows to
   where the tick fires N ≥ 5 chunks per call AND the model slows,
   re-evaluate whether the cost per chunk matters.
2. **Proper batched ORT inference.** If the ORT session is re-exported
   with dynamic batch size, `embed_texts_batch` could become a true
   1-forward-pass batch — in which case the chunk-level approach gets
   even cheaper and the switch is even less appealing.
3. **A retrieval-quality regression** that traces back to per-chunk
   embedding (e.g. chunks matching on a topic that the larger tick
   would have filtered out). Not observed today.

## Related

- #19 (chunking evaluation): chunking boundaries look good; extraction
  is where noise enters.
- #20 (ML chunking): skipped in favor of extraction work.
- `docs/latency-budgets.md` (§ "EmbedText"): no-regret window for the
  current strategy.
