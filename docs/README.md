# Legend docs

Build/operate notes for v2. Source-of-truth design lives in
`new_foundation.md` and `new_foundation_v0_core.md` at the repo root.

## Index

### Substrate

- [seed-graph.md](seed-graph.md) — Day-zero hypergraph: YAML → bin →
  runtime loader. Format, regen workflow, how to add regions /
  attribute names / frames.
- [inspecting-the-graph.md](inspecting-the-graph.md) — Markdown
  snapshot tool for substrate state. Current capability + planned
  HTML viewer / tick scrubber.

### Inference

- [inference-engine.md](inference-engine.md) — INT8 BERT forward pass
  in pure Rust. Pipeline, file map, quantization scheme, the three
  matmul kernels (scalar / AVX2 / AVX-VNNI), 2×4 register tile, the
  +128 shift trick, performance ceiling, portability matrix.

### Tick steps

- [intent-detection.md](intent-detection.md) — Step 1: per-dimension intent
  classifiers (conviction, prediction_error, arousal, curiosity). Pipeline,
  retraining, validation.

### Proposals (design sketches, pre-implementation)

- [frame-as-surface.md](frame-as-surface.md) — `ConsciousAttentionFrame`
  is the entire observable surface of a tick. Logic lives in lib
  code; the daemon and CLI are thin entry points. Denormalize the
  frame, drop `TickResult`, stop benches from reading the substrate.
