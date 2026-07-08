# CLI reference

One binary, five verbs. Every verb reads/writes JSON on stdio and exits non-zero
with a structured error on failure.

```
legend save|recall|init|dump|mcp-serve [payload] [--pretty] [--reset]
```

A payload can be passed as the trailing argument or piped on stdin. `--pretty`
formats output for humans; without it, output is compact single-line JSON.

## Verbs

### `init [--reset]`
Creates a `.legend` store in the target directory (idempotent — re-running is a
no-op that re-prints the status) and writes a project `.mcp.json` if one does not
already exist. The fresh store seeds the built-in ontology (32 elements, 10
relations). `--reset` wipes an existing store back to seed.

### `save [payload]`
Runs one tick over the payload and prints the resulting frame. The graph is
resolved, mutated, persisted to the snapshot, and the vector sidecar is
refreshed — all under the store lock.

### `recall [payload]`
Resolves the requested focus and prints a frame without mutating the graph
(unless the payload sets `observe`, which records the access).

### `dump`
Prints a human-readable rendering of the whole graph (ids resolved to names).
Read-only.

### `mcp-serve`
Starts the long-lived MCP server over stdio (see [mcp-server.md](mcp-server.md)).

## Save payload

```json
{
  "source": "session-note",
  "elements":  [ {"name": "Inferno", "kind": "spell", "summary": "burst fire", "aliases": ["Hellfire"]} ],
  "facts":     [ {"s": "Inferno", "p": "power", "o": "14", "confidence": 0.9, "src": "balance.md:12"} ],
  "changes":   [ {"target": "Inferno", "property": "power", "from": "12", "to": "14"} ],
  "templates": [ {"kind": "spell", "expects": ["power", "cost"], "summary": "..."} ],
  "retract":   [ {"s": "Inferno", "p": "power", "o": "14"} ],
  "merge":     [ {"from": "Hellfire", "into": "Inferno"} ],
  "intent":    {"conviction": 0.8, "arousal": 0.3, "curiosity": 0.0, "prediction_error": 0.0}
}
```

- **elements** mint or reuse nodes; reuse the canonical `name` (or an `alias`) to
  avoid duplicating an existing entity.
- **facts** are `(subject, property, object)` relations. Relations store element
  *ids*, not strings — `dump` resolves them back to names.
- **changes** supersede a value while preserving history: the prior value stays
  queryable, the current value becomes `to`.
- **retract** removes a fact; **merge** folds one element (and its aliases) into
  another.
- **intent** scalars modulate the tick (salience bump, novelty bias, gating).

## Recall payload

```json
{ "focus": ["Inferno", "the fire spell"], "observe": false,
  "limit": 20, "since": 0, "history_depth": 3 }
```

`focus` terms are resolved through the tiered index (exact name → alias → lexical
→ embedding). The frame reports what resolved, near-misses, the focused
subgraph, and supporting bands (current state, decisions, constraints, history,
related, sources). `observe: true` records the access (used by the eval probes).

## Environment

| var | meaning |
|---|---|
| `LEGEND_STATE_DIR` | store location (default: `.legend` discovered from cwd) |
| `LEGEND_NOW` | fixed clock in epoch seconds — the determinism seam for replay |
| `LEGEND_EMBED` | `0`/`1` — disable/enable the embedder (tier-2/3 recall) |
| `LEGEND_EMBED_DIR` | model directory (default `models/bge-small-en-v1.5`) |
| `LEGEND_TRACE` | trace store/graph reloads and warm hits to stderr |
| `LEGEND_EMBED_TRACE` | trace embedder sync/rank to stderr |

## Errors

Failures print a JSON object with a stable `code` (e.g. `no_store`, `parse`) and
exit non-zero, so callers can branch on the code rather than parse prose.
