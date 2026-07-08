# MCP server

`legend mcp-serve` runs a long-lived [Model Context Protocol](https://modelcontextprotocol.io)
server: JSON-RPC 2.0 over newline-delimited stdio. It exposes two tools to any
MCP client (Claude Code, etc.):

- **`legend_save`** — ingest a structured payload (see [cli.md](cli.md#save-payload)).
- **`legend_recall`** — resolve a focus and return the frame.

Clients namespace the tool names; in Claude Code they appear as
`mcp__legend__legend_save` and `mcp__legend__legend_recall`.

## Provisioning

`legend init` writes a project `.mcp.json` (unless one already exists) pointing at
the binary and the store:

```json
{ "mcpServers": { "legend": {
    "command": "/abs/path/to/legend",
    "args": ["mcp-serve"],
    "env": { "LEGEND_STATE_DIR": "/abs/path/to/.legend" }
} } }
```

So the whole setup is one command: a model told "run `legend init`" can
self-provision — create the store and drop the config an MCP client discovers.

## Warm process

The server stays resident, and two things exploit that:

- **Warm model.** The embedding model loads once at startup (`embed_warm`), so no
  call pays the cold-start cost.
- **Warm graph.** The graph is reloaded from the snapshot only when its on-disk
  fingerprint (size + mtime + nanoseconds) changes. Back-to-back calls that
  nothing else wrote skip the reload; an external `legend save` (a separate CLI
  process writing the same store) is still picked up on the next call.

## Failure isolation

Each call takes the per-store flock and runs under a `setjmp` error trap: a bad
call returns an MCP `isError` result rather than taking the process down, and any
error clears the warm-graph flag so the next call reloads from a clean snapshot.
One malformed request never corrupts the next.

## Notes for the calling model

- **Recall before save.** Resolve the entities you're about to write so you reuse
  canonical names instead of minting duplicates.
- **Update with `changes`, not new facts.** A changed value goes through `changes`
  (target/property/from/to) so history is preserved and the current value is
  unambiguous.
- Tool *semantics* like these are carried in the server's MCP `initialize`
  instructions, so a client picks them up automatically.
