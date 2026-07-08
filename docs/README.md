# Legend docs

Legend is a single-file C oracle that gives an LLM long-term memory: a calling
model `save`s structured observations into a deduplicated reality graph and
`recall`s a focused frame. These pages are the **operational reference**; the
design and its rationale live in the root spec.

## Design (source of truth)

- [`../new_foundation.md`](../new_foundation.md) — the full v2 design (24 sections:
  architecture, the tick pipeline, the substrate, the data model, algorithms,
  durability, evaluation).
- [`../new_foundation_v0_core.md`](../new_foundation_v0_core.md) — the v0 core
  slice, the part to build and conform to first.

## Reference

- [cli.md](cli.md) — the verbs, JSON payloads and frames, the store, environment.
- [mcp-server.md](mcp-server.md) — the warm MCP server and how a model connects.
- [embeddings.md](embeddings.md) — the bundled BGE embedder and tiered recall,
  plus how to re-export the model blob.
- [harness.md](harness.md) — `check.sh`, the replay corpus, fixtures, and probes.

## Style

- [`../C-STAR.md`](../C-STAR.md) — the C style guide this codebase follows.
