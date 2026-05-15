# LLM-driven extraction — FAILED EXPERIMENT

> **Status:** Abandoned. Implementation reverted at commit
> `811c935` after the small-model class proved unable to drive
> Step 5 extraction at acceptable latency. See `contextualized_embeddings_plan.md`
> for the replacement strategy (no LLM, uses per-token contextualized
> embeddings instead).
>
> Keeping this doc as a record so the same shape doesn't get re-tried
> without re-reading the lessons.

## What we tried

The hypothesis: replace degenerate `MiniLM(span_text)` element
embeddings with `MiniLM(description)` where the description comes
from a small generative LLM. Substrate routing, merging, and
supersession all get sharper because element vectors carry semantic
mass instead of just surface form.

Two architectures explored:

1. **Full LLM extraction (initial plan).** One generative-LLM pass
   per tick emits a structured payload of entities + relations +
   coref + descriptions. Replaces GLiNER, temporal regex, pattern
   RE, heuristic coref, and novelty_relations all at once.
2. **Hybrid (revised mid-way).** Keep GLiNER for span tagging. LLM
   handles only (a) per-tick label suggestion to widen GLiNER's
   label set, (b) per-novel-mint description writing.

Both architectures were validated against three model sizes:

| Model | Bundle | Speed (tok/s) | Quality on extraction tasks |
|---|---|---|---|
| SmolLM2-135M Q4_K_M | ~100 MB | ~87 | Useless. Copies few-shot example entities into output verbatim. Returns `["person"]` for every label-suggestion input regardless of content. |
| Qwen2-0.5B Q4_K_M | ~380 MB | ~31 | Tries to follow format. Degenerates into infinite loops past ~5 entities. Copies few-shot examples into descriptions for unfamiliar entities. |
| Qwen2.5-1.5B Q4_K_M | ~940 MB | ~10 | Actually does the task. World-knowledge-grounded descriptions. **But 3-5 s per call, 8.5 s for a full tick — order of magnitude over the subsecond budget.** |

## Why it failed

The 0.5B-class models can't reason about novel inputs — they
pattern-match against few-shot examples and emit the example's
content. The 1.5B class crosses the capability threshold but at
the cost of latency we couldn't afford for hot-path extraction.
This is a fundamental size-vs-capability tradeoff, not a prompt
engineering or backend issue. We tried:

- Tab-delimited vs pipe-delimited vs numbered-list output formats
- 0-shot vs 1-shot vs few-shot prompting
- Candle backend (~2 tok/s) vs `llama-cpp-2` (~30 tok/s)
- P-core pinning on hybrid Intel (Linux)

None bridged the gap. The path to "small + fast + capable" doesn't
exist at the 1B parameter class for structured extraction in 2026.

## What's load-bearing for future work

- **`embed_sequence_with_offsets` in `src/embed.rs`** (added during
  the experiment, kept after revert). Returns per-token contextualized
  vectors plus character offsets. This is the substrate for the
  replacement strategy.
- **`examples/route_element_test_v3.rs`** validates that
  contextualized span embeddings outperform bare-name embeddings
  for region routing. Strategy C from that file is the recommended
  path.

## Sunk-cost artifacts (now reverted)

- `llama-cpp-2`, `encoding_rs`, `libc`, `candle-*` deps
- `src/inference/qwen2.rs`
- `src/steps/llm_labels.rs`, `src/steps/llm_describe.rs`
- ~1.4 GB of GGUF model files
- Phase 1-3 implementation commits

All removed at the reset to `811c935`. LFS cache pruned.

## When to revisit

Revisit the LLM idea if any of the following land:

- A model emerges that does structured extraction reliably at
  < 200 ms per call (CPU). Probably requires either much smaller
  specialist models or hardware acceleration we don't have today.
- A concrete routing failure shows that contextualized-span
  embeddings (the replacement) are degenerate in some regime that
  rich descriptions would resolve. Specific evidence required, not
  a hypothetical.
- The substrate gains a "background description-write" off-tick
  path that decouples generation latency from the synchronous tick.
