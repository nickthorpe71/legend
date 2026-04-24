# ML-based chunk boundary detection — investigation (#20)

**Recorded:** 2026-04-24
**Context:** Follow-on to #19's chunking evaluation. The queue item
asked whether Legend should replace its current rule-based splitter
(`src/memory/entorhinal.rs::chunk_text`) with an ML-based chunk
boundary detector.

## Recommendation

**No ML upgrade yet.** Address extraction quality first (see #19
findings). Revisit ML chunking only if a concrete failure after
extraction improves can be attributed to chunk boundaries.

## Why

#19 established empirically that the chunker is not the bottleneck:

- Median chunk size is 111 chars; boundaries respect `\n\n`, `|`,
  topic-shift markers, and sentence splits.
- The observability fixture failures all trace to per-chunk
  extraction over-generating (fragmented noun heads, verb-token
  entities, O(n²) relation pairs).
- Re-chunking does not change any of those failure modes.

An ML chunker would cost:

1. **Binary size.** We already embed `all-MiniLM-L6-v2-q` at ~23 MB
   via `include_bytes!` for sentence embeddings. Adding a second model
   (even a small ALBERT/DistilBERT segmenter) doubles that cost.
2. **Inference latency.** Current `ChunkText` step profiles at ~6 ms
   per tick (§#07). A neural segmenter would at best match MiniLM's
   ~3 ms/inference on short text; likely slower for longer inputs.
3. **Training/eval scaffolding.** We have no labeled sentence-boundary
   corpus specific to dev notes + structured tick prefixes. Off-the-
   shelf models trained on news / web text may not match.
4. **Deployment complexity.** Daemon startup already waits on one
   ORT session init; a second model doubles that cold cost.

None of these trade-offs are worth paying when the symptoms on the
observability fixtures come from downstream extraction.

## What we would do if chunking did become the bottleneck

Three options, in increasing cost:

1. **Semantic similarity splitter (reuse MiniLM).** Already have the
   model; compute sliding-window sentence embeddings and cut at
   similarity drops below a threshold. No new model. Estimated cost:
   1–2 days. Useful for long prose where sentence-level boundaries are
   too fine-grained.
2. **Tiny classifier ONNX (custom)**. Train a small BERT-classifier
   on a hand-labeled corpus of Legend tick inputs. Would ship as a
   second `include_bytes!` model (~10–25 MB). Weeks of dataset +
   training + eval work.
3. **External library (`unicode-segmentation` or `icu`)**. Pure-Rust,
   rule-based, no network dep. Slightly better Unicode handling than
   the current ad-hoc splitter, but not ML. Would replace
   `split_sentences` inside `chunk_text`. Low risk, modest upside —
   worth doing as part of a sentence-splitter refactor if it comes up.

## Where this leaves the chunking arc (#20–#23)

- **#20 (this item):** investigated and documented. Verdict: not
  worth the upgrade now.
- **#21 — batch vs whole-text embedding:** still an open and
  separate decision. Performance-driven, not quality-driven.
- **#22 was emptied** by a reorder glitch earlier; a historical
  placeholder exists but no active content. Treat as a no-op in the
  queue.
- **#23 — execute chosen chunking strategy:** should be gated on
  either #21 producing a concrete choice, or on a later finding that
  contradicts #19's "chunking is fine" conclusion.

## If extraction work uncovers a chunking regression

Re-run `cargo run --release --example chunking_eval > .perf/chunking-eval-YYYY-MM-DD.md`
and compare chunk boundaries against the current `.perf/chunking-eval-2026-04-24.md`.
A regression shows up as:
- single-paragraph inputs splitting into many small chunks (under-chunking)
- multi-paragraph inputs collapsing into one chunk (over-chunking)
- `|` or `\n\n` boundaries being crossed
