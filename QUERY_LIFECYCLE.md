# Legend Query Lifecycle

This document describes what happens when an LLM asks Legend for remembered
context, from the external query call through L1, L2, L3 retrieval, temporal
boosting, pattern completion, graph priming, reinforcement, output formatting,
event logging, and persistence.

It reflects the current code in this repo. When comments and implementation
disagree, this document follows implementation.

## Quick Map

Core files:

- `src/commands/memory/query.rs`: CLI command wrapper for
  `legend memory query`.
- `src/commands/mcp.rs`: MCP tool wrapper for `legend_memory_query`.
- `src/memory/mod.rs`: `retrieve_context()` and query-mode inference.
- `src/memory/hippocampus.rs`: L2 similarity search and CA3 pattern completion.
- `src/memory/neocortex.rs`: graph lookup, graph-informed L2 boosts, spreading
  activation, Hebbian reinforcement.
- `src/memory/entorhinal.rs`: query embedding and cosine similarity.
- `src/tool/persistence.rs`: `.legend/memory.lz4` load/save.
- `src/memory/basal_ganglia.rs`: explicit post-query reinforcement command.

Important thresholds:

- Base query similarity floor: `MIN_QUERY_SIMILARITY = 0.35`
- Query keyword bonus per match: `0.05`
- Query keyword bonus cap: `0.2`
- Pattern completion activates when fewer than 3 L2 snippets are found, or top
  snippet similarity is below `0.5`
- L3 Summary retrieval floor: `0.3`
- L3 Summary discount vs L2: `0.85x`
- TCM temporal proximity boost: `+0.03 * tcm_similarity`
- Date-affinity boost for temporal queries: `+0.05`
- Passive top-result auto-reinforcement scale: `0.03`
- Direct graph lookup result limit: `12`
- Associative priming hops: `2`
- Associative priming decay: `0.4`

Code pointers: `src/memory/mod.rs:102`, `src/memory/mod.rs:117`,
`src/memory/mod.rs:178`, `src/memory/mod.rs:203`.

## Entry Points

### CLI Query

The CLI path is:

```text
legend memory query [--reasons] <text>
```

The handler is `handle_query()` in `src/commands/memory/query.rs:28`.

Lifecycle:

1. Parse args.
   - Query text must be positional.
   - No stdin fallback exists.
   - `--reasons` or `-r` switches to detailed JSON output with simple
     explanation strings.

   Code: `src/commands/memory/query.rs:12`.

2. Load memory from `.legend/memory.lz4`.
   Load rebuilds graph edge index, rebuilds keyword cache from graph, and can
   migrate embeddings to the current 384-dimensional model.

   Code: `src/commands/memory/query.rs:34`,
   `src/tool/persistence.rs:23`.

3. Call:

   ```rust
   crate::memory::retrieve_context(&mut memory.brain, opts.query.trim())
   ```

   Code: `src/commands/memory/query.rs:35`.

4. Save memory after retrieval.

   This is necessary because query mutates memory: clock, decay, L1 rehearsal
   counts, L2 usage, L2 stability, L2 salience, `last_retrieved_ids`, and graph
   weights can all change.

   Code: `src/commands/memory/query.rs:36`.

5. Count primed L3 nodes.
   Any returned graph node with `edge_type.is_some()` is counted as primed.

   Code: `src/commands/memory/query.rs:38`.

6. Log a rich query event containing top L2 matches, top L3 graph nodes, and
   primed count.

   Code: `src/commands/memory/query.rs:44`,
   `src/commands/memory/query.rs:68`.

7. Print output:
   - default: compact JSON with `working_memory`, `memories`, `related_topics`
   - `--reasons`: JSON with `working_memory`, `short_term`, `long_term`,
     per-result reason strings, and a note about top-result auto-reinforcement

   Code: `src/commands/memory/query.rs:70`,
   `src/commands/memory/query.rs:78`,
   `src/commands/memory/query.rs:148`.

### MCP Query

The MCP tool path is `legend_memory_query`.

Lifecycle:

1. Read required `topic` argument.
2. Reject missing or empty topic.
3. Load memory.
4. Call `retrieve_context(&mut memory.brain, topic)`.
5. Save memory.
6. Log rich query event.
7. Return human-readable text grouped by:
   - `Working Memory (L1)`
   - `Episodic Memory (L2)`
   - `Knowledge Graph (L3)`

Code: `src/commands/mcp.rs:272`.

MCP output differs from CLI output:

- CLI default emits JSON.
- CLI `--reasons` emits richer JSON.
- MCP emits Markdown-like text with similarity scores for L2.
- MCP does not expose temporal metadata fields directly in output.

Code: `src/commands/mcp.rs:309`.

## Core Query: `retrieve_context()`

The brain query starts at `src/memory/mod.rs:930`.

It returns:

```rust
MemoryContext {
    short_term: Vec<MemorySnippet>,
    long_term: Vec<GraphNodeSummary>,
    working_memory: Vec<MemorySnippet>,
}
```

Query does not insert the user's query as a new memory, but it is not read-only.
It applies decay and reinforcement side effects.

## Step 1: Instrumentation

If compiled with `instrument`, query creates a trace context and emits
`QueryStart`, plus later pipeline events for L1 scan, L2 retrieval, graph lookup,
pattern completion, associative priming, Hebbian reinforcement, and query end.

Code: `src/memory/mod.rs:931`.

Without that feature, trace code is compiled out.

## Step 2: Clock Increment and Decay

Every query increments `state.clock` by 1.

Code: `src/memory/mod.rs:941`.

Then it applies global decay.

Code: `src/memory/mod.rs:942`,
`src/memory/mod.rs:1631`.

L2 effects:

- salience decays based on age since `last_access`
- semantic density slows decay
- Ebbinghaus stability slows decay
- emotional valence decays at half rate

Code: `src/memory/hippocampus.rs:261`.

L3 effects:

- graph node weight and salience decay based on age since `last_seen`
- graph edge weight decays based on age since `last_seen`
- edge stability slows edge decay

Code: `src/memory/neocortex.rs:670`.

Important: just asking a query ages memories that are not retrieved.

## Step 3: Query Mode Inference

Legend infers a `QueryMode`.

Code: `src/memory/mod.rs:943`,
`src/memory/mod.rs:876`.

Modes:

- `Temporal`
- `Diagnostic`
- `Structural`
- `Semantic`
- `Neutral`

Inference order matters.

### Temporal

If the lowercased query contains any marker from
`TEMPORAL_QUERY_MARKERS`, mode is `Temporal`.

Code: `src/memory/mod.rs:878`.

Examples:

```text
what happened before the migration?
chronological order of museum visits
what did we decide after March 15?
```

### Diagnostic

If query contains markers like:

```text
why, bug, crash, failure, regression, broke, error, panic
```

mode is `Diagnostic`.

Code: `src/memory/mod.rs:886`.

### Structural

If query contains structural markers:

```text
how does, where is, what calls, depends on, uses
```

or syntax:

```text
(), ::, /, _
```

mode is `Structural`.

Code: `src/memory/mod.rs:900`.

If entity extraction finds code-like entities such as `Symbol`, `Function`,
`Decorator`, `FilePath`, or `Tool`, mode is also `Structural`.

Code: `src/memory/mod.rs:906`.

### Semantic

If entity extraction finds any entities but no earlier mode matched, mode is
`Semantic`.

Code: `src/memory/mod.rs:915`.

### Neutral

If none of the above match, mode is `Neutral`.

Code: `src/memory/mod.rs:919`.

## Step 4: Query Embedding

Legend embeds the query using the embedded all-MiniLM-L6-v2 quantized model.

Code: `src/memory/mod.rs:950`,
`src/memory/entorhinal.rs:96`.

The embedding dimension defaults to 384.

This query embedding drives:

- L1 cosine similarity
- L2 cosine similarity
- L3 Summary-node similarity
- pattern completion direct similarity component

## Step 5: L1 Working Memory Scan

Legend scans L1 before L2.

Code: `src/memory/mod.rs:952`.

For every `WorkingMemoryEntry`:

1. Compute cosine similarity between L1 embedding and query embedding.
2. Lowercase query into whitespace tokens.
3. Add keyword bonus:
   - token length must be greater than 3
   - token must not be a stopword
   - entry text must contain token
   - each match adds `0.05`
   - total keyword bonus caps at `0.2`
4. Clamp effective similarity to at most `1.0`.
5. If effective similarity is at least `MIN_QUERY_SIMILARITY` (`0.35`):
   - increment L1 `rehearsal_count`
   - return a `MemorySnippet` with L1 text and similarity

Code: `src/memory/mod.rs:956`.

L1 query snippets have:

- `refs = []`
- `wall_clock = 0`
- `extracted_dates = []`
- `created_at_clock = 0`

Code: `src/memory/mod.rs:969`.

L1 snippets are sorted by similarity descending and not truncated.

Code: `src/memory/mod.rs:980`.

Important: L1 rehearsal currently affects the L1 entry's `rehearsal_count`, but
L1 promotion on displacement/flush only checks `promoted`, not rehearsal count.

## Step 6: L2 Candidate Search

Legend searches all L2 entries with `hippocampus::top_k_similar()`.

Code: `src/memory/mod.rs:988`,
`src/memory/hippocampus.rs:186`.

For every `ShortTermEntry`:

1. Compute cosine similarity with query embedding.
2. Extract lowercased query keywords:
   - trim punctuation
   - length greater than 1
   - not stopword
3. Add keyword bonus:
   - each keyword contained in entry text adds `0.05`
   - cap total keyword bonus at `0.2`
4. Add emotional boost:
   - `abs(emotional_valence) * 0.05`
5. Build `MemorySnippet` with text, similarity, refs, wall clock, extracted
   dates, and creation clock.
6. Retain only snippets with similarity at least `0.35`.
7. Sort descending by similarity.
8. Truncate to `k.max(50)`.

Code: `src/memory/hippocampus.rs:202`.

In current `retrieve_context()`, `k = usize::MAX`, so the safety truncate does
not practically cap results.

Code: `src/memory/mod.rs:992`.

Important: consolidated L2 entries are not filtered out. L3 summaries supplement
L2; they do not replace it.

Code: `src/memory/hippocampus.rs:204`.

## Step 7: Graph-Informed L2 Boost

Legend uses L3 associations to boost L2 candidates that mention graph-connected
entities.

Code: `src/memory/mod.rs:1002`,
`src/memory/neocortex.rs:381`.

Process:

1. Extract entities from the query.
2. Resolve query entities to L3 graph seed IDs.
3. Spread activation from seed nodes up to 6 hops.
4. Per hop, activation is multiplied by:
   - edge weight
   - square root of edge stability
   - query-mode edge-kind multiplier
   - hop decay (`0.5^hop`)
5. Stop paths below activation `0.01`.
6. For activated non-query nodes, compute a specificity-weighted label bonus:

   ```text
   0.05 * activation * (1 / max(0.1, node.weight))
   ```

7. For each L2 candidate, if its text contains an activated label, add the
   label bonuses, capped at `0.3`.

Code: `src/memory/neocortex.rs:399`,
`src/memory/neocortex.rs:419`,
`src/memory/neocortex.rs:466`,
`src/memory/neocortex.rs:481`.

Effect: rare graph-connected entities can pull up otherwise lower-similarity L2
memories.

## Step 8: L3 Summary Retrieval

Legend scans L3 Summary nodes with centroid embeddings.

Code: `src/memory/mod.rs:1016`.

For each graph node:

- node kind must be `Summary`
- node embedding must be non-empty
- cosine similarity with query embedding must be at least `0.3`
- node ID must not already be present in L2 candidate IDs

If matched, Legend returns a `MemorySnippet`:

- text is `full_text` if present, otherwise Summary label
- similarity is `sim * 0.85`
- no refs
- no wall clock
- no extracted dates
- creation clock `0`

Code: `src/memory/mod.rs:1022`.

L3 Summary hits are sorted and appended to the L2 candidate list.

Code: `src/memory/mod.rs:1045`,
`src/memory/mod.rs:1052`.

This is how consolidated neocortical memory can answer even when underlying L2
entries have decayed or been evicted.

## Step 9: Temporal Boosts

Temporal boosts happen after L2 candidates and L3 Summary hits are combined.

Code: `src/memory/mod.rs:1055`.

### TCM Proximity Boost

If `state.temporal_context` is non-empty, each candidate that corresponds to an
actual L2 entry and has a temporal-context snapshot gets:

```text
candidate.similarity += 0.03 * cosine(state.temporal_context, entry.temporal_context)
```

Code: `src/memory/mod.rs:1057`.

This boost is automatic and not gated by temporal query mode.

L3 Summary hits generally do not receive this boost because they do not
correspond to L2 entries in `state.short_term`.

### Date-Affinity Boost

Legend checks if the query has temporal signals:

- `extract_dates(query)` returns any dates, or
- query contains a temporal marker

Code: `src/memory/mod.rs:1070`.

If true, every L2 candidate with either wall-clock metadata or extracted dates
gets `+0.05`.

Code: `src/memory/mod.rs:1075`.

Again, L3 Summary hits generally do not receive this boost unless their ID also
matches an L2 entry, which normally it does not.

## Step 10: Adaptive L2 Relevance Threshold

Legend sorts candidates by similarity descending.

Code: `src/memory/mod.rs:1090`.

Then it computes:

```text
top_sim = first candidate similarity, or 0.0
adaptive_floor = max(top_sim * 0.65, MIN_QUERY_SIMILARITY)
```

It retains only candidates with similarity at least `adaptive_floor`.

Code: `src/memory/mod.rs:1091`.

Implications:

- Strong top result raises the floor for the whole result set.
- Weak top result falls back to the absolute floor of `0.35`.
- If no candidates exist, the list stays empty.
- There is no hard top-N cap here.

## Step 11: L2 Spaced-Repetition Updates

For every retained snippet whose ID matches an actual L2 entry, Legend updates
retrieval state.

Code: `src/memory/mod.rs:1096`.

For each matched L2 entry:

1. Compute `interval = state.clock - entry.last_access`.
2. If this is not the first retrieval interval:
   - if current interval is larger than previous interval, multiply stability by
     `1.3`
   - otherwise multiply stability by `1.05`
   - cap stability at `10.0`
3. Store `last_retrieval_interval = interval`.
4. Set `last_access = state.clock`.
5. Increment `usage`.

Code: `src/memory/mod.rs:1097`.

Effects:

- Retrieved L2 entries decay more slowly in the future if retrievals are spaced.
- Massed retrieval still helps, but much less.
- Query changes memory even before explicit reinforcement.

L3 Summary snippets do not get this update unless their ID happens to match an
L2 entry.

## Step 12: CA3 Pattern Completion

Pattern completion activates if:

```text
snippets.len() < 3 || top_similarity < 0.5
```

Code: `src/memory/mod.rs:1117`.

It calls `hippocampus::pattern_complete()`.

Code: `src/memory/hippocampus.rs:423`.

Pattern completion process:

1. Extract entities from the query.
2. Extract entities from current partial L2 snippets.
3. Resolve those entities to graph node IDs.
4. If no seeds exist, return empty.
5. Run graph spreading activation from seeds:
   - 2 hops
   - decay factor `0.5`
   - query-mode edge-kind multipliers
6. Collect `source_texts` from activated graph nodes.
7. Scan L2 entries not already in partial matches.
8. If an L2 entry text contains an activated source text, or an activated source
   text contains the entry text, compute:

   ```text
   completion_score = direct_query_similarity * 0.6 + activation * 0.4
   ```

9. Add the completed L2 entry.
10. Sort completed results by score.

Code: `src/memory/hippocampus.rs:429`,
`src/memory/hippocampus.rs:452`,
`src/memory/hippocampus.rs:466`,
`src/memory/hippocampus.rs:481`.

Back in `retrieve_context()`:

- completed snippets with IDs not already present are appended
- snippets are re-sorted descending by similarity

Code: `src/memory/mod.rs:1122`.

Important gotcha: the comment says completed results are above
`MIN_QUERY_SIMILARITY`, but the current implementation does not filter
`completion_score` by that threshold.

Code: `src/memory/hippocampus.rs:500`.

## Step 13: Chronological Sort for Temporal Queries

If the query has temporal signals, snippets are sorted by `created_at_clock`.

Code: `src/memory/mod.rs:1139`.

This changes the output order from relevance order to chronological order.

Since L3 Summary snippets have `created_at_clock = 0`, they can appear before L2
entries in temporal queries.

## Step 14: Record Last Retrieved IDs

Legend stores all returned snippet IDs in `state.last_retrieved_ids`.

Code: `src/memory/mod.rs:1145`.

This is used later by explicit reinforcement:

```text
legend memory reinforce <signal> <id1> [id2 ...]
```

If a later positive reinforcement call reinforces only some of the previously
retrieved IDs, basal ganglia contrastively penalizes the retrieved-but-not-
reinforced entries.

Code: `src/memory/basal_ganglia.rs:59`.

## Step 15: Passive Top-Result Auto-Reinforcement

Legend passively reinforces the top returned snippet.

Code: `src/memory/mod.rs:1148`.

If the top snippet similarity is greater than `0.2` and its ID matches an L2
entry:

```text
entry.salience = min(1.0, entry.salience + top_similarity * 0.03)
```

Code: `src/memory/mod.rs:1150`.

This reinforces useful memories even without explicit `memory reinforce`.

Temporal query gotcha: because temporal queries sort snippets by creation order
before auto-reinforcement, the "top" snippet is the earliest chronological
result, not necessarily the highest-similarity result.

L3 Summary snippets generally do not get auto-reinforced because they do not
match an L2 entry ID.

## Step 16: Direct L3 Graph Lookup

Legend queries the knowledge graph directly.

Code: `src/memory/mod.rs:1163`,
`src/memory/neocortex.rs:293`.

Graph lookup process:

1. Extract entities from the query.
2. For each entity whose lowercase label exists in graph index:
   - add direct graph node result with `edge_type = None`
   - add node as a seed
3. If seeds exist, run spreading activation:
   - max hops: `3`
   - decay factor: `0.5`
   - edge-kind multipliers from query mode
   - edge stability improves propagation by `sqrt(stability)`
   - minimum activation: `0.01`
4. Add activated graph node results with `edge_type = Some("activated")`.
5. Deduplicate by node ID, keeping highest weight.
6. Sort by weight descending.
7. Truncate to direct lookup limit, currently `12`.

Code: `src/memory/neocortex.rs:300`,
`src/memory/neocortex.rs:322`,
`src/memory/neocortex.rs:349`,
`src/memory/neocortex.rs:362`.

If no entities match graph index, there is no fallback dump of all graph nodes.

Code: `src/memory/neocortex.rs:345`.

## Step 17: Associative Priming

Legend expands graph results using entities from retrieved L2 snippets.

Code: `src/memory/mod.rs:1176`.

Process:

1. Extract entities from every returned L2 snippet.
2. Resolve those entities to graph node IDs.
3. Add direct graph lookup node IDs as additional seeds.
4. Deduplicate seed IDs.
5. Run spreading activation:
   - 2 hops
   - decay factor `0.4`
   - current query mode
6. For every activated node not already in direct graph results:
   - add it as a `GraphNodeSummary`
   - weight is `node.weight * 0.7 * activation`
   - `edge_type = Some("primed")`

Code: `src/memory/mod.rs:1179`,
`src/memory/mod.rs:1195`.

This means L3 output can include graph concepts not directly mentioned in the
query, if L2 matches imply them.

## Step 18: Adaptive L3 Weight Threshold

Legend sorts L3 graph results by weight descending.

Code: `src/memory/mod.rs:1220`.

Then:

```text
top_weight = first graph result weight, or 0.0
weight_floor = top_weight * 0.4
retain graph nodes where weight >= weight_floor
```

Code: `src/memory/mod.rs:1221`.

Implications:

- Strong graph result can prune weak graph topics from output.
- If graph result list is empty, it stays empty.
- There is no explicit minimum nonzero floor here.

## Step 19: Hebbian Reinforcement of Co-Retrieved L3 Nodes

Legend reinforces graph nodes and edges that were returned together.

Code: `src/memory/mod.rs:1226`,
`src/memory/neocortex.rs:625`.

If fewer than 2 graph node IDs are returned, nothing happens.

If 2 or more are returned:

- every edge connecting two co-retrieved IDs gets a dampened weight boost:

  ```text
  HEBBIAN_EDGE_BOOST / (1 + ln(edge.activation_count + 1))
  ```

- edge `last_seen` is updated
- every co-retrieved node gets `+0.02` weight, capped at `5.0`
- node `last_seen` is updated

Code: `src/memory/neocortex.rs:630`.

Important: this Hebbian path does not increment `activation_count`; activation
count is incremented in `upsert_edge()` during graph updates.

## Step 20: Return `MemoryContext`

Finally, `retrieve_context()` returns:

- L2/summary snippets in `short_term`
- L3 graph summaries in `long_term`
- L1 snippets in `working_memory`

Code: `src/memory/mod.rs:1238`.

The query string itself is not stored as a memory.

## Output Formatting

### CLI Default JSON

Default CLI output shape:

```json
{
  "working_memory": ["..."],
  "memories": ["..."],
  "related_topics": ["..."]
}
```

Code: `src/commands/memory/query.rs:148`.

If any returned short-term memory has temporal metadata, `memories` switches
from strings to objects:

```json
{
  "text": "...",
  "wall_clock": 1775928369,
  "dates": ["March 15th"],
  "seq": 123
}
```

Code: `src/commands/memory/query.rs:156`.

The temporal-object decision looks only at `context.short_term`, not L1 or L3
graph nodes.

### CLI `--reasons` JSON

`--reasons` output includes:

- L1 matches with reason `"matched in working memory (L1), rehearsal incremented"`
- L2 matches with coarse similarity reason labels
- L3 graph nodes with either direct-match or edge-reached reason
- `primed_via_edges`
- note about top result auto-reinforcement

Code: `src/commands/memory/query.rs:78`.

Reason labels are presentation heuristics. They do not expose the actual
component scores such as keyword bonus, TCM boost, graph boost, or date boost.

### MCP Text Output

MCP output is text grouped by layer:

```text
## Working Memory (L1)
- ...

## Episodic Memory (L2)
- [sim:0.72] ...

## Knowledge Graph (L3)
- label [kind] (primed)
```

Code: `src/commands/mcp.rs:309`.

If all layers are empty, MCP returns:

```text
No memories found for this topic.
```

Code: `src/commands/mcp.rs:340`.

## Explicit Reinforcement After Query

Although not part of `retrieve_context()` itself, query prepares for explicit
reinforcement by setting `state.last_retrieved_ids`.

Code: `src/memory/mod.rs:1145`.

The CLI reinforce command is:

```text
legend memory reinforce <signal> <id1> [id2 ...]
```

Code: `src/commands/memory/reinforce.rs:4`.

`signal` is clamped to `[-1.0, 1.0]`.

Code: `src/memory/basal_ganglia.rs:53`.

When signal is positive:

- entries retrieved in the prior query but not reinforced get
  `CONTRASTIVE_PENALTY = 0.02` subtracted from salience

For each reinforced L2 entry:

- salience changes by AdaGrad adaptive step
- accumulated gradient increases
- positive signal increments usage
- `last_access` updates
- extracted graph entities get graph node weight adjustment scaled by
  `REINFORCE_GRAPH_SCALE = 0.1`

Code: `src/memory/basal_ganglia.rs:59`,
`src/memory/basal_ganglia.rs:76`,
`src/memory/basal_ganglia.rs:102`.

This means query has a two-stage learning model:

1. Query itself passively updates retrieved memories and graph co-activations.
2. Optional reinforce gives explicit reward or penalty.

## Example Walkthroughs

### Example 1: Query Matches Recent L1 Only

Input:

```text
legend memory query "updated docs"
```

Possible lifecycle:

1. CLI loads memory.
2. `retrieve_context()` increments clock and decays L2/L3.
3. Query mode is likely `Semantic` or `Neutral`, depending on extracted
   entities.
4. Query embedding is computed.
5. L1 entry `updated docs` has enough cosine/keyword similarity.
6. L1 `rehearsal_count += 1`.
7. No L2 candidates pass floor.
8. Pattern completion may try graph seeds, but probably returns nothing.
9. Direct graph lookup likely returns nothing.
10. Result contains only `working_memory`.
11. Memory is saved because rehearsal count and decay changed.

### Example 2: Query Matches L2 Decision

Input:

```text
legend memory query "why did we choose SQLite?"
```

Possible lifecycle:

1. Query mode becomes `Diagnostic` because it contains `why`.
2. L1 is scanned.
3. L2 entries are scored by semantic similarity, keyword bonus, and emotional
   boost.
4. Graph-connected labels can boost entries mentioning related entities.
5. Adaptive floor keeps strong related entries.
6. Retrieved entries get usage incremented, `last_access` updated, and possibly
   stability increased.
7. Pattern completion may add related memories if direct matches are sparse.
8. Top result receives passive salience boost.
9. L3 graph lookup and priming return related topics.
10. Hebbian reinforcement strengthens co-retrieved graph nodes/edges.

### Example 3: Temporal Query

Input:

```text
legend memory query "what happened after March 15?"
```

Possible lifecycle:

1. Query mode becomes `Temporal`.
2. Query dates are extracted.
3. L2 candidates are scored normally.
4. L2 entries with wall-clock or extracted date metadata receive `+0.05`.
5. TCM proximity still applies.
6. After pattern completion, snippets are sorted by `created_at_clock`, not
   similarity.
7. CLI default output likely emits object memories with `wall_clock`, `dates`,
   and `seq`.

Gotcha: L3 Summary hits have `created_at_clock = 0`, so they can sort before
date-bearing L2 entries in temporal output.

### Example 4: Structural Query

Input:

```text
legend memory query "where is retrieve_context?"
```

Possible lifecycle:

1. Query mode becomes `Structural` because of `where is` and/or code-like
   entity extraction.
2. L2 search favors text semantically/lexically related to `retrieve_context`.
3. Graph boosting and graph lookup use structural edge priors:
   - `contains` and `represents`: strongest
   - `related`: moderate
   - `temporal`: downweighted
4. L3 graph output favors structural associations around the symbol.

### Example 5: Weak Query With Pattern Completion

Input:

```text
legend memory query "migration thing"
```

Possible lifecycle:

1. Direct L2 results are sparse or top similarity is below `0.5`.
2. Pattern completion extracts entities from the query and partial matches.
3. It spreads graph activation to related nodes.
4. It looks for L2 entries containing activated source texts.
5. It appends completed L2 memories even if direct similarity alone was weak.

## Layer Effects

### What Query Does to L1

Query:

- scans L1 first
- increments `rehearsal_count` for matching L1 entries
- returns matching L1 entries separately from L2

Query does not directly promote L1 entries to L2.

### What Query Does to L2

Query can:

- decay all L2 entries at query start
- return all candidates above adaptive relevance floor
- increment usage on retained L2 results
- update last access
- update retrieval interval
- increase stability through spaced retrieval
- passively boost top L2 salience
- set `last_retrieved_ids` for future contrastive reinforcement

Query does not insert the query text into L2.

### What Query Does to L3

Query can:

- decay all graph nodes/edges at query start
- use L3 to boost L2 candidates
- retrieve Summary nodes by embedding
- retrieve direct and activated graph nodes
- add primed graph nodes to output
- Hebbian-reinforce co-retrieved graph nodes and edges

Query does not create new graph nodes. New nodes are created by ticks,
keyword registration, and consolidation. Query does update existing graph
weights and `last_seen` through Hebbian reinforcement.

## Current Gotchas

1. Query is not read-only. It mutates clock, decay, L1 rehearsal count, L2
   usage/stability/salience, `last_retrieved_ids`, and L3 weights.

2. `retrieve_context()` is also called inside high-salience ticks. Tick therefore
   has query side effects.
   Code: `src/memory/mod.rs:741`.

3. The L2 candidate search comment mentions MMR, but MMR is currently removed.
   Current behavior returns threshold-passing candidates sorted by similarity.
   Code comment: `src/memory/mod.rs:988`.

4. `top_k_similar()` says "top-k", but when called from `retrieve_context()`
   with `usize::MAX`, the safety truncate is effectively disabled.
   Code: `src/memory/mod.rs:992`,
   `src/memory/hippocampus.rs:235`.

5. Pattern completion comments say completed results are above
   `MIN_QUERY_SIMILARITY`, but `pattern_complete()` does not filter by that
   threshold.
   Code: `src/memory/hippocampus.rs:500`.

6. Temporal query auto-reinforcement happens after chronological sorting, so the
   earliest result can receive the passive salience boost even if it is not the
   most semantically similar result.
   Code: `src/memory/mod.rs:1139`,
   `src/memory/mod.rs:1150`.

7. L3 Summary snippets are returned in `context.short_term`, because they use
   `MemorySnippet`. They are not actual L2 entries and usually do not receive
   L2 usage/stability/salience updates.
   Code: `src/memory/mod.rs:1022`.

8. CLI `--reasons` explanations are coarse labels derived from final similarity
   and edge presence. They do not explain actual score composition.
   Code: `src/commands/memory/query.rs:92`.

9. `graph_lookup()` has a stale comment saying activation threshold is `0.15`;
   implementation uses `0.01`.
   Code: `src/memory/neocortex.rs:281`,
   `src/memory/neocortex.rs:364`.

10. Query saves memory even when no results are found, because decay and clock
    still changed.

## End State Summary

After a successful query, Legend has potentially changed:

- `BrainState.clock`
- decayed L2 salience
- decayed L2 emotional valence
- decayed L3 node weight/salience
- decayed L3 edge weight
- L1 `rehearsal_count`
- L2 `usage`
- L2 `last_access`
- L2 `last_retrieval_interval`
- L2 `stability`
- L2 top-result salience
- `last_retrieved_ids`
- L3 graph node weights and `last_seen`
- L3 graph edge weights and `last_seen`
- `.legend/events.jsonl`
- `.legend/memory.lz4`

The query text itself is not stored unless the LLM separately ticks it.
