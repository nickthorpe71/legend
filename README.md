# Legend

Long-term memory for LLMs. Legend is a single-file C oracle that a calling model
drives over MCP (or the CLI): it ingests structured `save` payloads into a
deduplicated reality graph and answers `recall` with a focused frame. LLM
sessions are fleeting by default — Legend is the substrate that carries
continuity across them.

This is the **v2 rewrite**: pure C99, no runtime dependencies beyond `libm` and
a bundled embedding model. (The v1 Rust implementation lives at `../legend-v1`.)

## Build

```bash
./check.sh        # build + full gate: unit tests, fixtures, replay slices, fuzz
```

or just the binary:

```bash
cc -std=c99 -O2 legend.c embed.c -o legend -lm
```

No network, no package manager, no codegen. The only runtime asset is the
bundled embedder under `models/bge-small-en-v1.5/` (an int8 blob + vocab).

## Use it — CLI

```bash
legend init                              # create a .legend store here (+ writes .mcp.json)
echo '{"elements":[...]}' | legend save  # ingest a payload; prints the resulting frame
echo '{"focus":["..."]}'  | legend recall  # query; prints the focused frame
legend dump                              # human-readable graph dump
legend mcp-serve                         # long-lived MCP server over stdio
```

Payloads and frames are JSON; add `--pretty` for readable output. Full reference
in [docs/cli.md](docs/cli.md).

## Use it — MCP (the primary path)

`legend init` writes a project `.mcp.json` pointing at the binary and store,
exposing two tools — `legend_save` and `legend_recall` — to any MCP client
(e.g. Claude Code). The server is long-lived and warm: the embedding model loads
once at startup, and the graph reloads only when the snapshot changes on disk.
See [docs/mcp-server.md](docs/mcp-server.md).

## How it works

A `save` runs one **tick**: the payload is parsed, resolved against existing
elements (reuse canonical names, don't duplicate), folded into the graph
(new elements/relations, value supersessions, retractions, merges), persisted to
a binary snapshot, and the vector sidecar is refreshed. A `recall` resolves the
requested focus through a tiered index (exact name → alias → lexical → embedding)
and returns a frame: the focused subgraph plus supporting bands (current state,
decisions, constraints, history, related). The design is in
[`new_foundation.md`](new_foundation.md).

## Store & environment

- `.legend/legend.snapshot` — the binary graph snapshot; written atomically after
  every `save`. `.legend/legend.lock` is the per-store single-writer flock.
- `LEGEND_STATE_DIR` — override the store location (default: `.legend` discovered
  from the cwd).
- `LEGEND_NOW` — inject a fixed clock (epoch seconds) for deterministic replay.
- `LEGEND_EMBED` — `0`/`1` to disable/enable the embedder (tier-2/3 recall).
- `LEGEND_EMBED_DIR` — model directory (default `models/bge-small-en-v1.5`).
- `LEGEND_TRACE`, `LEGEND_EMBED_TRACE` — diagnostic tracing to stderr.

## Docs

- [`new_foundation.md`](new_foundation.md) / [`new_foundation_v0_core.md`](new_foundation_v0_core.md)
  — the design (source of truth)
- [`docs/`](docs/) — operational reference (CLI, MCP server, embeddings, harness)
- [`C-STAR.md`](C-STAR.md) — the C style guide for this repo

## License

See [LICENSE](LICENSE).
