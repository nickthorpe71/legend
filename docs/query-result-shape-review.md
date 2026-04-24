# Query result shape review (#30)

**Recorded:** 2026-04-24

Closes queue item #30: "Review query result shape." Audits the JSON
returned by `legend memory query` (and the structured payload that
flows through MCP / IPC).

## Two shapes today

### Default (compact)

```json
{
  "working_memory": ["<text>", "..."],
  "memories": ["<text>", "..."],
  "related_topics": ["<label>", "..."]
}
```

When **any** short-term entry carries temporal metadata, `memories`
switches to objects:

```json
{
  "memories": [{"text": "...", "wall_clock": 12345, "dates": [...], "seq": N}, "..."]
}
```

So the same field can hold a string array OR an object array
depending on the result set.

### With `--reasons`

```json
{
  "working_memory": [{"id": N, "text": "...", "similarity": 0.81, "reason": "..."}],
  "short_term":     [{"id": N, "text": "...", "similarity": 0.74, "reason": "..."}],
  "long_term":      [{"id": N, "label": "...", "kind": "...", "weight": 0.4, "reason": "..."}],
  "primed_via_edges": 3,
  "note": "Read-only retrieval: no recall-time reinforcement or clock advance"
}
```

## Findings

### Schema asymmetry between the two modes

- The default mode names the L2 array `memories`; `--reasons` names
  it `short_term`. Same data, different field names. Scripts have to
  branch on which mode they used.
- The default mode names the L3 array `related_topics` (string list
  of labels); `--reasons` names it `long_term` (object list with
  metadata). Different name and different shape.
- `working_memory` is the only field that keeps the same name across
  modes — but its shape changes (strings vs objects).

This is the biggest gap. A consumer parsing the output has to switch
schemas based on a CLI flag, which is friction-inducing.

### `memories` field is polymorphic

Whether `memories` is `[string]` or `[object]` depends on whether
ANY entry has temporal metadata. A consumer can't write a stable
parser; they have to inspect the first element to decide.

A simple fix: always emit objects, and let the consumer ignore the
fields they don't need. Backwards-incompatible but unambiguous.

### `note` field is decorative

`"note": "Read-only retrieval: ..."` in `--reasons` is informational
text inside a JSON response. Useful to LLMs reading the payload as
prose, but it would belong as part of a CLI human-readable mode, not
the JSON.

### `primed_via_edges` only in `--reasons`

The number of L3 nodes reached via graph edges (vs direct entity
match) is a useful signal — but it only surfaces in `--reasons`.
Worth adding to the default shape.

### Similarity rounding vs weight rounding

Both are rounded via `crate::tool::round3` to 3 decimals. Consistent;
no friction.

## Recommended target shape

A single, mode-flag-independent schema:

```json
{
  "working_memory": [{"text": "...", "id": N?, "similarity": 0.81?}, "..."],
  "short_term":     [{"text": "...", "id": N?, "similarity": 0.74?, "wall_clock": N?, "dates": [...]?, "seq": N?, "reason": "..."?}],
  "long_term":      [{"label": "...", "id": N?, "kind": "...", "weight": 0.4?, "edge_type": "...", "reason": "..."?}],
  "primed_via_edges": N
}
```

Field-presence rules:

- All `_text`/`_label` and ID fields always emitted.
- Similarity / weight / temporal fields emitted when populated.
- `reason` emitted only with `--reasons`.

Would replace the existing branching: same field names regardless of
mode, presence-based metadata.

## Recommendations

### Worth doing

1. **Unify field names across modes.** Rename default-mode `memories`
   to `short_term`, default-mode `related_topics` to `long_term`. One
   schema, mode-dependent enrichment.
2. **Make `memories` always emit objects.** Drop the string shorthand;
   `{"text": "..."}` is barely longer and unambiguous.
3. **Add `primed_via_edges` to the default shape.** Useful signal; no
   reason to gate it on `--reasons`.
4. **Drop the `note` field from JSON.** Move to a `--quiet`/`--human`
   complement on the CLI side if we want a human banner.

### Worth considering

- **Schema versioning**: add `"version": 1` so future shape changes
  are explicit. Cheap insurance.
- **A documented JSON Schema** committed to `docs/`. Locks contract
  for both the CLI and MCP surfaces (the structured payload that
  flows through IPC).
- **LLM-readable prose mode** for MCP. Today MCP returns markdown for
  tick but the same JSON for query. A "Related memories: ..." prose
  shape (parallel to `format_mcp_tick`) might be worth adding.

### Not recommended

- **Removing `--reasons`.** It's the diagnostic mode for understanding
  retrieval; humans use it during debugging. Keep it.
- **Adding more knobs.** The two-mode CLI is already at the
  complexity limit. Schema unification reduces friction without
  adding flags.

## Decision for this audit

- Document (this file).
- Apply no breaking change in this commit. Field renames (`memories`
  → `short_term`, `related_topics` → `long_term`) and
  always-object-mode are user-visible JSON contract breaks. They need
  their own queue item with a CHANGELOG note + conformance test.

## Future queue candidates

- "Unify `legend memory query` result schema across `--reasons` and
  default modes" — combines findings 1, 2, 3 above.
- "Add JSON-Schema doc + conformance test for query results."
- "MCP query: render Markdown response in addition to structured
  payload (parallel to `format_mcp_tick`)."

## Related

- `src/commands/daemon/handlers.rs::render_query` — entry point.
- `format_query_context` and `format_query_with_reasons` —
  the two formatters.
- `tests/conformance_memory_commands.rs::query_with_reasons_shows_*`:
  the only place locking parts of the current shape.
- `docs/tick-api-review.md` (#29): same audit pattern, sibling task.
