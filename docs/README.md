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

### Tick steps

- [intent-detection.md](intent-detection.md) — Step 1: per-dimension intent
  classifiers (conviction, prediction_error, arousal, curiosity). Pipeline,
  retraining, validation.
