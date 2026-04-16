# Legend Tick Lifecycle

This document describes what happens when an LLM records information with
Legend, from the external tick call through L1 working memory, L2 episodic
memory, L3 graph memory, decay, pruning, consolidation, and persistence.

It reflects the current code in this repo, not just the architectural intent.
Where comments and implementation disagree, this document follows the
implementation.

## Quick Map

Core files:

- `src/commands/memory/tick.rs`: CLI command wrapper for `legend memory tick`.
- `src/commands/mcp.rs`: MCP tool wrapper for `legend_memory_tick`.
- `src/tool/mod.rs`: tool-layer `tick()` and `tick_passive()` wrappers.
- `src/memory/mod.rs`: brain orchestrator, especially `tick_impl()` and
  `consolidate()`.
- `src/memory/prefrontal.rs`: L1 working memory.
- `src/memory/hippocampus.rs`: L2 episodic vector store.
- `src/memory/neocortex.rs`: L3 knowledge graph and consolidation helpers.
- `src/memory/entorhinal.rs`: chunking, embeddings, summaries.
- `src/memory/thalamus.rs`: salience scoring.
- `src/memory/amygdala.rs`: emotional valence.
- `src/tool/persistence.rs`: `.legend/memory.lz4` load/save.

Default capacities and thresholds:

- L1 working memory capacity: `10`
- L2 short-term capacity: `1024`
- Embedding dimension: `384`
- High merge threshold: `theta_high = 0.85`
- Low merge threshold: `theta_low = 0.75`
- L2 promotion threshold: `ATTENTION_GATE_THRESHOLD = 0.25`
- L2 decay rate: `HIPPOCAMPAL_DECAY_RATE = 0.001`
- L3 decay rate: `NEOCORTICAL_DECAY_RATE = 0.0005`
- Auto-consolidation tick threshold: `15`
- Emotional auto-consolidation threshold: `1.5`

Code pointers: `src/tool/types.rs:18`, `src/memory/mod.rs:74`,
`src/memory/mod.rs:82`, `src/memory/mod.rs:137`,
`src/memory/mod.rs:178`, `src/memory/mod.rs:205`.

## Entry Points

### CLI Tick

The CLI path is:

```text
legend memory tick [--blocker] [--passive] <text>
```

The handler is `handle_tick()` in `src/commands/memory/tick.rs:31`.

Lifecycle before brain processing:

1. Parse flags and text.
   If no positional text is present, it reads stdin.
   Code: `src/commands/memory/tick.rs:12`.

2. Reject empty input.
   Code: `src/commands/memory/tick.rs:37`.

3. Reject known noise ticks.
   Current noise filters reject:
   - text shorter than 10 characters
   - tool telemetry matching `Executed tool ... with status`
   - `EXPERIENCE:` completed-turn boilerplate
   - malformed empty experience quotes

   Code: `src/commands/memory/tick.rs:41`,
   `src/commands/memory/helpers.rs:57`.

4. Apply CLI-only prefixes:
   - `--blocker` prepends `BLOCKER: `
   - `--passive` prepends `EXPERIENCE: `

   Code: `src/commands/memory/tick.rs:46`.

5. Extract keyword directives from tick text.
   Lines like `KEYWORD:<category>:<term>` are removed from the memory text and
   converted into L3 Keyword nodes.

   Example:

   ```text
   Added Bevy ECS support
   KEYWORD:tool:bevy
   ```

   This stores the text `Added Bevy ECS support` and registers
   `kw:tool:bevy`.

   Code: `src/commands/memory/tick.rs:54`,
   `src/commands/memory/helpers.rs:13`.

6. Load memory from `.legend/memory.lz4`.
   Load also rebuilds the graph edge index and keyword cache, and can migrate
   embeddings to 384 dimensions if needed.

   Code: `src/commands/memory/tick.rs:61`,
   `src/tool/persistence.rs:23`.

7. Register keyword directives.
   If the tick contains only directives and no clean text, the command saves
   the keyword changes and exits with `{"action":"keyword_only"}`. No brain tick
   runs in that case.

   Code: `src/commands/memory/tick.rs:63`,
   `src/commands/memory/tick.rs:83`.

8. Call either:
   - `crate::memory::tick(&mut memory, &text)` for normal ticks
   - `crate::memory::tick_passive(&mut memory, &text)` for passive ticks

   Code: `src/commands/memory/tick.rs:96`.

9. After the brain returns, CLI applies extra salience changes to the target L2
   entry when one exists:
   - `--blocker`: add `0.4`, capped at `1.0`
   - `--passive`: multiply by `0.5`

   This happens after `tick_impl()`, so it is not part of salience gating.
   Code: `src/commands/memory/tick.rs:103`.

10. Save memory, reset `.legend/.pending_ticks`, log an event, print the JSON
    tick result, optionally append to `ARCHITECTURE.md`, and possibly run the
    older CLI consolidation suggestion path.

    Code: `src/commands/memory/tick.rs:116`,
    `src/commands/memory/tick.rs:118`,
    `src/commands/memory/tick.rs:146`,
    `src/commands/memory/tick.rs:148`,
    `src/commands/memory/tick.rs:150`,
    `src/commands/memory/tick.rs:154`.

### MCP Tick

The MCP tool path is `legend_memory_tick`.

Lifecycle before brain processing:

1. Read `description` from the MCP tool arguments.
2. Reject missing, empty, or noise input.
3. Load memory.
4. Call normal `crate::memory::tick(&mut memory, text)`.
5. Save memory.
6. Reset `.legend/.pending_ticks`.
7. Log a rich event.
8. Append to `ARCHITECTURE.md` if text starts with `ARCHITECTURE:`.
9. Return a text response with the action, related L2 memories, and graph topics.

Code: `src/commands/mcp.rs:183`.

Important MCP differences from CLI:

- MCP does not currently support `--passive`.
- MCP does not parse `KEYWORD:` directives.
- MCP does not apply the CLI `--blocker` post-hoc `+0.4` salience boost.
- MCP does not run the older `should_suggest_consolidation()` post-tick path.
  It relies on `tick_impl()` auto-consolidation.

## Tool-Layer Tick Wrappers

The public wrappers live in `src/tool/mod.rs`.

### Normal Tick

`tick(state, text)` does two things:

1. Calls `tick_impl(&mut state.brain, text, false)`.
2. Appends the original text to `state.session_log` with timestamp
   `state.brain.clock`, then caps the session log at 100 entries.

Code: `src/tool/mod.rs:97`.

Important: the session log timestamp is Legend's monotonic brain clock, not the
wall-clock timestamp. L2 entries separately store wall-clock seconds when they
are inserted.

### Passive Tick

`tick_passive(state, text)` calls `tick_impl(&mut state.brain, text, true)` and
does not write to the session log.

Code: `src/tool/mod.rs:114`.

Passive still mutates L1, L2, L3, clock, decay, and pruning. It skips only the
active-tick behaviors inside `tick_impl()`:

- no `ticks_since_consolidation` increment
- no rolling emotional-intensity decay/update
- no term-frequency auto-promotion
- no CPEB tagging
- no context-switch flush
- no auto-consolidation trigger
- no session-log append

## Brain Tick: `tick_impl()`

The core lifecycle starts at `src/memory/mod.rs:444`.

### 1. Instrumentation Context

If compiled with the `instrument` feature, `tick_impl()` creates a trace context
and emits `TickStart`. Most major stages emit trace events.

Code: `src/memory/mod.rs:445`.

Without the feature, this is compiled out.

### 2. Clock Increment

Every tick increments `state.clock` by 1.

Code: `src/memory/mod.rs:455`.

This clock drives:

- L2 age and decay
- L3 node and edge decay
- session-log timestamps
- temporal ordering (`created_at_clock`)
- consolidation windows
- replay temporal proximity
- graph edge spacing/stability

### 3. Active-Tick Counters

If `passive == false`:

- `ticks_since_consolidation += 1`
- rolling emotional intensity decays by `0.8`

Code: `src/memory/mod.rs:462`.

Passive ticks skip this.

### 4. Global Decay

Immediately after clock increment, Legend applies decay to existing L2 and L3.

Code: `src/memory/mod.rs:467`,
`src/memory/mod.rs:1631`.

L2 decay:

- Each L2 entry's salience is multiplied by
  `exp(-age * effective_decay_rate)`.
- `effective_decay_rate = HIPPOCAMPAL_DECAY_RATE / density_factor / stability`.
- Higher semantic density slows decay.
- Higher Ebbinghaus stability slows decay.
- Emotional valence decays at half the salience rate.

Code: `src/memory/hippocampus.rs:261`.

L3 decay:

- Each graph node's weight and salience decay by age since `last_seen`.
- Each edge's weight decays too.
- Edge stability slows edge decay.

Code: `src/memory/neocortex.rs:670`.

### 5. Periodic Normalization

Two periodic maintenance passes can run:

- Every 10 clock ticks: L2 salience is renormalized by basal ganglia.
- Every 5 clock ticks: graph weights are normalized so max node weight does
  not exceed the target maximum.

Code: `src/memory/mod.rs:476`,
`src/memory/mod.rs:479`.

### 6. Initialize Tick Result State

`tick_impl()` initializes a blank `MemoryContext` and result fields. The final
`TickResult` reports only the last chunk processed.

Code: `src/memory/mod.rs:483`.

This matters for multi-chunk ticks: earlier chunks may insert or merge, but the
returned `action` and `entry_id` reflect the final chunk's result.

### 7. Chunk Text

The full input text is split into chunks before embedding.

Code: `src/memory/mod.rs:495`,
`src/memory/entorhinal.rs:297`.

Chunking phases:

1. Split on paragraph breaks (`\n\n`) and pipe separators (`|`).
2. Split on topic-shift markers such as `by the way`, `separately`, `btw`,
   `one more thing`.
3. Split on sentence boundaries: `.`, `!`, `?` followed by whitespace or end.
   It avoids splitting common abbreviations.

Example:

```text
DECISION: Use SQLite because this is single-user. BUG: startup panics on empty config.
```

This becomes two chunks:

```text
DECISION: Use SQLite because this is single-user.
BUG: startup panics on empty config.
```

Each chunk independently goes through L1, salience, L2 promotion, graph updates,
and possible retrieval side effects.

### 8. Batch Embedding

All chunks are embedded together using the embedded all-MiniLM-L6-v2 quantized
model. Empty chunks get zero vectors.

Code: `src/memory/mod.rs:497`,
`src/memory/entorhinal.rs:134`.

The embedding dimension defaults to 384.

## Per-Chunk Lifecycle

The following steps run once per chunk.

### 1. Raw Embedding

`tick_impl()` reads the chunk's precomputed raw embedding from the batch result.

Code: `src/memory/mod.rs:509`.

### 2. Salience Scoring

The thalamus computes salience.

Code: `src/memory/mod.rs:516`,
`src/memory/thalamus.rs:25`.

Current scoring signals:

- Decision language:
  - one decision hit: `+0.3`
  - two or more decision hits: `+0.5`
  - rationale words (`because`, `rationale`, `reason`): extra `+0.15`
- Bug/incident language: `+0.4`
- TODO/blocker language: `+0.3`
- Architecture language: `+0.25`
- Preference/convention language: `+0.3`
- Learned domain vocabulary: `+0.1`
- Fenced code block: `+0.15`
- Code definition triggers:
  - one hit: `+0.2`
  - two or more hits: `+0.3`
- Long substantive text: `+0.15` for more than 25 words
- Explicit `error`: `+0.15`
- Final score is clamped to `[0.05, 1.0]`

Note: the word-count branch checks `>25` before `>50`, so the `>50` branch is
currently unreachable.

### 3. Emotional Valence

The amygdala computes emotional valence in `[-1.0, 1.0]`.

Code: `src/memory/mod.rs:523`,
`src/memory/amygdala.rs:20`.

Signals:

- negative keywords push toward `-1.0`
- positive keywords push toward `+1.0`
- urgency keywords (`blocker`, `critical`, `P0`, etc.) amplify magnitude by
  `0.15` per hit if valence is nonzero

Effects:

- Stored on L1 and L2 entries.
- In L2, emotional valence decays slower than salience.
- Emotional magnitude improves L2 query scoring.
- Emotional magnitude improves L2 eviction resistance.
- High valence can trigger CPEB tagging and auto-consolidation.

### 4. Source Reference Extraction

Legend extracts file references such as:

```text
src/memory/mod.rs#L444
src/game.rs#L10-20
```

Code: `src/memory/mod.rs:530`,
`src/memory/hippocampus.rs:561`.

L2 stores up to 8 refs per entry.

### 5. Dentate Gyrus Sparse Orthogonalization

Legend gathers existing L2 embeddings and pushes the new embedding away from
similar-but-distinct memories.

Code: `src/memory/mod.rs:537`,
`src/memory/dentate_gyrus.rs:31`.

Behavior:

- If similarity to an existing L2 embedding is between `theta_low` and
  `theta_high`, subtract a scaled projection from the new embedding.
- Similarity below `theta_low` is considered distinct enough.
- Similarity above `theta_high` is left alone so true duplicates can still
  merge.
- The result is renormalized.

This reduces interference before L2 matching.

### 6. Temporal Metadata

Legend updates the Temporal Context Model and extracts date strings.

Code: `src/memory/mod.rs:557`,
`src/memory/mod.rs:1883`.

The TCM process:

1. Project the 384-dimensional semantic embedding to 64 dimensions by
   mean-pooling groups of dimensions.
2. Detect event boundaries when:
   - salience is at least `0.7`, or
   - similarity to the previous tick embedding is below `0.2`
3. Use drift rate:
   - normal: `0.95`
   - boundary: `0.7`
4. Blend projected embedding into `state.temporal_context`.
5. Normalize the vector.
6. Clone the current temporal context as the L2 entry snapshot.

Date extraction runs on the chunk and stores strings like ISO dates, month/day,
month/year, and relative phrases.

Code: `src/memory/mod.rs:559`.

Wall-clock time is recorded as Unix seconds when the chunk is processed.

Code: `src/memory/mod.rs:561`.

### 7. Always Enter L1 Working Memory

Every chunk is pushed into L1 working memory, regardless of salience.

Code: `src/memory/mod.rs:566`,
`src/memory/prefrontal.rs:41`.

L1 entry fields:

- id
- text
- embedding
- salience
- tick_created
- rehearsal_count
- promoted
- emotional_valence

Code: `src/memory/prefrontal.rs:30`.

L1 ID allocation uses `state.next_id`, then increments it.

Code: `src/memory/prefrontal.rs:48`.

#### L1 Capacity and Displacement

If L1 is already at capacity (`immediate_capacity`, default 10), the oldest L1
entry is removed.

If the displaced entry has not already been promoted to L2:

1. Legend floors its salience to `PRUNE_THRESHOLD * 2.0`.
2. Extracts source refs.
3. Inserts it into L2.
4. Updates L3 graph from its text.

Code: `src/memory/prefrontal.rs:51`.

Important: displaced L1 entries promoted this way get `wall_clock = 0`,
`extracted_dates = []`, and `temporal_context = []`, because the L1 entry does
not currently carry that metadata.

### 8. Attention Gate

Legend checks:

```rust
salience >= ATTENTION_GATE_THRESHOLD
```

The threshold is `0.25`.

Code: `src/memory/mod.rs:587`,
`src/memory/mod.rs:609`.

Possibilities:

- Low-salience chunk: stays L1-only for now.
- High-salience chunk: promoted through L2/L3 encoding immediately.

Examples:

```text
updated docs
```

Likely salience: floor-ish, stays L1-only.

```text
DECISION: Use SQLite because the app is single-user.
```

Likely salience: decision + rationale, promotes to L2.

```text
BUG: startup panics on missing config.
```

Likely salience: bug signal, promotes to L2.

```text
The API layer interfaces with the schema module.
```

Likely salience: architecture signal, promotes at threshold.

## Low-Salience Path: L1 Only

If salience is below `0.25`:

1. No L2 match is attempted.
2. No L2 entry is inserted.
3. No L3 graph update runs for this chunk immediately.
4. `TickResult.action = "working_memory_only"`.
5. `TickResult.entry_id` is the L1 working memory ID.

Code: `src/memory/mod.rs:742`.

The entry can still reach L2 later:

- L1 capacity displacement promotes unpromoted entries.
- Session start flush promotes unpromoted entries.
- Context switch flush promotes unpromoted entries.

## High-Salience Path: L2 and L3 Encoding

If salience is at least `0.25`, Legend tries to merge into existing L2 or insert
a new L2 entry.

### 1. Find Best L2 Match

Legend computes cosine similarity between the chunk embedding and every L2
entry embedding.

Code: `src/memory/mod.rs:611`,
`src/memory/hippocampus.rs:168`.

If L2 is empty, best similarity is `-1.0`.

### 2. Diversity Gate

If best similarity is at least `theta_low`, Legend checks word overlap between
the existing entry text and new chunk text.

Code: `src/memory/mod.rs:621`,
`src/memory/dentate_gyrus.rs:85`.

The Jaccard word-overlap threshold is `0.4`.

This prevents semantically close but factually different memories from merging.

### 3. High-Similarity Merge

Condition:

```text
best_sim >= theta_high && diversity_ok
```

Default:

```text
best_sim >= 0.85 && word_overlap >= 0.4
```

Effects:

- Existing L2 `usage += 2`
- Existing L2 `salience = min(1.0, old_salience + new_salience)`
- Existing L2 `last_access = state.clock`
- Source refs are merged
- L3 graph is updated from the new chunk
- `TickResult.action = "merged"`
- `TickResult.entry_id = best_id`
- `TickResult.matched_existing = best_id`
- `TickResult.similarity = best_sim`

Code: `src/memory/mod.rs:640`.

Notably, high-similarity merge does not update text or embedding.

### 4. Low-Similarity Merge

Condition:

```text
best_sim >= theta_low && diversity_ok
```

Default:

```text
best_sim >= 0.75 && word_overlap >= 0.4
```

Effects:

- Existing L2 embedding is averaged with the new embedding.
- `usage += 1`
- `salience = min(1.0, old_salience + new_salience * 0.5)`
- Summary is recomputed from existing text + incoming chunk.
- `last_access = state.clock`
- Source refs are merged.
- L3 graph is updated from the new chunk.
- `TickResult.action = "merged"`

Code: `src/memory/mod.rs:660`.

Notably, low-similarity merge updates summary and embedding, but does not append
the incoming text to `entry.text`.

### 5. New L2 Insertion

If no merge path matches, Legend inserts a new `ShortTermEntry`.

Code: `src/memory/mod.rs:681`,
`src/memory/hippocampus.rs:346`.

Inserted fields include:

- `id = state.next_id`
- `text = chunk`
- `summary = summarize_single(chunk)`
- 384-dimensional embedding
- `last_access = state.clock`
- `usage = 1`
- clamped salience
- source refs
- semantic density
- `consolidated = false`
- emotional valence
- stability `1.0`
- last retrieval interval `0`
- `created_at_clock = state.clock`
- `wall_clock = now`
- extracted dates
- TCM temporal context snapshot

Code: `src/memory/hippocampus.rs:396`.

#### L2 Capacity Eviction

Before insertion, if L2 has reached `short_term_capacity` (default 1024), Legend
evicts the lowest-scoring L2 entry.

Code: `src/memory/hippocampus.rs:357`.

Eviction score balances:

- salience
- usage
- recency
- emotional valence resistance

If an entry is already consolidated into an L3 Summary node with an embedding,
its eviction resistance is reduced by `0.2`, because L3 can independently serve
some of its role.

Code: `src/memory/hippocampus.rs:362`,
`src/memory/hippocampus.rs:375`.

### 6. L3 Graph Update

Every immediate high-salience L2 merge or insert calls `neocortex::update_graph()`.

Code: `src/memory/mod.rs:648`,
`src/memory/mod.rs:669`,
`src/memory/mod.rs:711`,
`src/memory/neocortex.rs:687`.

Graph update process:

1. Extract entities from the chunk.
2. For each entity:
   - normalize label to lowercase for lookup
   - create a new graph node if absent
   - otherwise reuse existing node
   - update node weight, `last_seen`, salience, and possibly kind
3. Apply code-aware weighting:
   - `FilePath`: `2.0x`
   - function/struct/enum/trait/class: `1.5x`
   - symbol/type: `1.2x`
   - generic term: `0.5x`
4. Create or reinforce pairwise edges between entities in the same chunk.
5. Classify edge kinds from entity contexts:
   - `contains`
   - `depends-on`
   - `implements`
   - `co-defined`
   - `related`
6. Reinforce Keyword nodes whose terms appear in the text.
7. Create `keyword-co-occurs` edges between matched keyword nodes and active
   entity nodes.

Code: `src/memory/neocortex.rs:696`,
`src/memory/neocortex.rs:723`,
`src/memory/neocortex.rs:749`,
`src/memory/neocortex.rs:768`.

Edges are handled by `upsert_edge()`.

Code: `src/memory/neocortex.rs:572`.

If the edge already exists:

- update interval EMAs
- increase stability more for spaced reinforcement (`*1.3`) than massed
  reinforcement (`*1.05`)
- increment activation count
- add `EDGE_REINFORCE_DELTA`
- update `last_seen`
- upgrade kind from `related` to a more specific kind if applicable

If new:

- create edge with weight `EDGE_REINFORCE_DELTA`
- set kind
- set stability `1.0`
- register in edge index

### 7. Mark L1 Entry as Promoted

After L2 processing succeeds, the matching L1 working-memory entry gets
`promoted = true`.

Code: `src/memory/mod.rs:736`.

This prevents the same chunk from being inserted into L2 again during L1
displacement or flush.

### 8. Immediate Retrieval Side Effect

After every high-salience chunk, `tick_impl()` calls:

```rust
last_context = retrieve_context(state, &chunk);
```

Code: `src/memory/mod.rs:741`.

This is important: a tick internally performs a query-like retrieval for context.
That has side effects:

- increments `state.clock` again
- applies decay again
- scans L1
- retrieves L2 candidates
- boosts candidates using graph associations
- retrieves L3 Summary nodes
- applies temporal boosts
- updates L2 usage and retrieval intervals
- can increase L2 stability through spaced retrieval
- may auto-reinforce the top L2 result's salience
- performs graph lookup and associative priming
- applies Hebbian reinforcement to co-retrieved graph nodes/edges
- updates `state.last_retrieved_ids`

Full query behavior will be documented separately, but this immediate retrieval
is part of tick lifecycle because it happens inside `tick_impl()`.

Implication: a high-salience tick usually advances the clock by more than one:

- `+1` at tick start
- `+1` for each high-salience chunk's internal `retrieve_context()`
- `+1` more if auto-consolidation runs

Low-salience chunks do not call `retrieve_context()`.

## After All Chunks

### 1. Term Frequency and Keyword Auto-Promotion

For active ticks only, Legend updates term-frequency statistics from extracted
entities in the full original text.

Code: `src/memory/mod.rs:751`,
`src/memory/mod.rs:1747`.

For each extracted entity:

- lowercase label
- track distinct tick count
- track total appearances
- track first seen and last seen clocks
- record whether the tick co-occurred with any existing keyword

When a term has appeared in at least 5 distinct ticks, Legend may auto-promote
it to an L3 keyword node:

```text
kw:domain:<term>
```

Promotion filters:

- not already a domain keyword
- at least 3 characters
- not purely numeric
- not a stopword
- at least 5 distinct ticks
- came through entity extraction
- has co-occurred with an existing keyword

Code: `src/memory/mod.rs:1785`,
`src/memory/mod.rs:1810`.

If promotions happen, the keyword cache is rebuilt.

Code: `src/memory/mod.rs:1802`.

### 2. L2 Pruning

Legend prunes L2 entries whose composite score has fallen below threshold.

Code: `src/memory/mod.rs:763`,
`src/memory/hippocampus.rs:251`.

Prune condition:

```text
salience + usage * 0.05 - age * 0.001 > 0.1
```

Entries failing that condition are removed.

This is separate from L2 capacity eviction. Pruning can remove stale low-salience
entries even when L2 is below capacity.

### 3. L3 Pruning

Legend prunes the graph after each tick.

Code: `src/memory/mod.rs:778`,
`src/memory/neocortex.rs:497`.

Graph pruning:

1. Remove nodes whose effective weight falls below `GRAPH_PRUNE_WEIGHT`.
2. If graph node count still exceeds `GRAPH_NODE_CAPACITY`, remove lowest-weight
   nodes.
3. Remove edges pointing to deleted nodes.
4. If edge count exceeds `GRAPH_EDGE_CAPACITY`, keep highest-weight edges.
5. Rebuild edge index.

### 4. Active-Tick Smart Consolidation Signals

For active ticks only, Legend computes full-tick embedding and valence.

Code: `src/memory/mod.rs:795`.

#### Rolling Emotional Intensity

The absolute tick valence is added to `recent_valence_sum`.

Code: `src/memory/mod.rs:798`.

This was decayed earlier in the same tick by `0.8`.

#### CPEB Synaptic Tagging

If absolute tick valence is greater than `0.3`, Legend tags recently active
graph edges.

Code: `src/memory/mod.rs:802`,
`src/memory/neocortex.rs:650`.

Edges with `clock - edge.last_seen <= 5` get:

```text
edge.stability += 1.5 * abs(valence)
```

capped at `10.0`.

Effect: high-emotion events make recently active graph edges decay more slowly.

#### Context-Switch Detection and L1 Flush

If there is a previous tick embedding, Legend compares it with the current
full-tick embedding.

Code: `src/memory/mod.rs:827`.

If cosine similarity is below `0.15`, Legend treats it as a topic switch and
flushes L1.

Code: `src/memory/mod.rs:840`,
`src/memory/prefrontal.rs:89`.

Flush behavior:

- drain all L1 entries
- for each unpromoted entry:
  - floor salience to `PRUNE_THRESHOLD * 2.0`
  - extract refs
  - insert into L2
  - update L3 graph
- already-promoted L1 entries are discarded because their L2 representation
  already exists

Like L1 displacement, flush-inserted L2 entries currently use `wall_clock = 0`,
empty extracted dates, and empty temporal context.

Session start also flushes L1.

Code: `src/commands/memory/start.rs:115`.

MCP session start does the same.

Code: `src/commands/mcp.rs:152`.

#### Remember Last Tick Embedding

The full-tick embedding is stored as `state.last_tick_embedding`.

Code: `src/memory/mod.rs:859`.

This drives future context-switch detection and TCM event-boundary detection.

### 5. Auto-Consolidation

For active ticks only, `tick_impl()` automatically consolidates when either:

```text
ticks_since_consolidation >= 15
recent_valence_sum >= 1.5
```

Code: `src/memory/mod.rs:861`.

If triggered, it calls `consolidate(state)`.

Code: `src/memory/mod.rs:865`.

Passive ticks skip auto-consolidation.

## Consolidation Lifecycle

Consolidation can run from:

- `tick_impl()` auto-consolidation
- CLI `legend memory consolidate`
- CLI's older post-tick `should_suggest_consolidation()` block
- tests or library callers

The core function is `consolidate(state)`.

Code: `src/memory/mod.rs:1259`.

### 1. Consolidation Clock and Reset

Consolidation:

- increments clock by 1
- resets `ticks_since_consolidation` to 0
- applies decay

Code: `src/memory/mod.rs:1274`.

### 2. Replay Consolidation

Before grouping, Legend runs hippocampal replay.

Code: `src/memory/mod.rs:1277`,
`src/memory/neocortex.rs:808`.

Replay finds L2 entries that are temporally proximate by `last_access` and
reinforces graph edges between their entities. This creates or strengthens
temporal/associative structure from co-active episodic traces.

### 3. Group Similar L2 Entries

Legend clusters L2 entries by embedding similarity.

Code: `src/memory/mod.rs:1284`.

For each unused seed entry:

- create a group with the seed
- scan later entries
- if cosine similarity to seed is at least `theta_low` (`0.75`), add to group

Only groups with more than one entry produce Summary nodes.

Code: `src/memory/mod.rs:1323`.

### 4. Summarize Group

Group summary uses `summarize_group()`, which chooses the top 3 group entries by
`salience + usage * 0.1`, then joins their summaries up to 300 characters.

Code: `src/memory/mod.rs:1324`,
`src/memory/entorhinal.rs:263`.

### 5. Systems Consolidation for High-Salience Groups

If average group salience is at least `0.4`, Legend creates richer L3 encoding:

- centroid embedding: average of group embeddings, renormalized
- `full_text`: concatenated source texts up to 500 chars

Code: `src/memory/mod.rs:1348`.

If average salience is below `0.4`, Summary node is still created, but without
centroid embedding and `full_text`.

### 6. Create or Merge Summary Node

Legend checks for an existing Summary node with:

- exact lowercase label match, or
- word overlap at least `MERGE_WORD_OVERLAP_THRESHOLD`

Code: `src/memory/mod.rs:1412`.

If found:

- update weight
- update salience
- update `last_seen`
- extend `source_texts`, capped at 20
- update centroid embedding and full text if present

Code: `src/memory/mod.rs:1430`.

If not found:

- create a new GraphNode with kind `Summary`
- weight `1.0 + salience`
- source texts capped at 20
- optional centroid embedding
- optional full text
- insert into L3 index

Code: `src/memory/mod.rs:1452`.

### 7. Semantic Topic Extraction

Legend extracts entities from all entries in the group and counts them.

Code: `src/memory/mod.rs:1492`.

If an entity appears in at least half the group and more than once:

- create or reuse a Topic/entity node
- create or reinforce a `represents` edge from Summary to topic

Code: `src/memory/mod.rs:1526`.

### 8. Re-update Graph and Mark L2 Consolidated

For each L2 entry in the group:

- call `update_graph()` again with the entry text
- increment usage
- update `last_access`
- set `consolidated = true`

Code: `src/memory/mod.rs:1569`.

Important: consolidated L2 entries remain queryable. The current L2 retrieval
code no longer filters them out. The `consolidated` flag mainly affects eviction
resistance when an embedded Summary node can serve the memory's role.

### 9. Final Prune

Consolidation finishes by pruning L2 and L3 again.

Code: `src/memory/mod.rs:1596`.

Then it returns the Summary nodes produced.

## Persistence and Event Side Effects

### Save Format

After CLI or MCP tick completes, memory is saved to `.legend/memory.lz4`.

Code: `src/tool/persistence.rs:57`.

Save format:

1. Serialize `MemoryState` to MessagePack.
2. Prefix payload with magic `LGND` and format version byte.
3. Compress with LZ4.
4. Write to temp file.
5. Rename temp file over `.legend/memory.lz4`.

Code: `src/tool/persistence.rs:87`.

### Event Log

CLI and MCP both log rich tick events containing:

- entry ID
- top matched L2 snippets from returned context
- top graph nodes from returned context

CLI code: `src/commands/memory/tick.rs:120`.
MCP code: `src/commands/mcp.rs:205`.

### Pending Tick Counter

CLI and MCP reset `.legend/.pending_ticks` to `0` after successful tick.

CLI code: `src/commands/memory/tick.rs:118`.
MCP code: `src/commands/mcp.rs:202`.

### Architecture Append

If tick text starts with `ARCHITECTURE:`, both CLI and MCP append a dated
summary line to `ARCHITECTURE.md`.

CLI code: `src/commands/memory/tick.rs:150`.
MCP code: `src/commands/mcp.rs:234`.
Append helper: `src/commands/memory/helpers.rs:117`.

## Layer Outcomes

### When Does a Tick End Up in L1?

Always, per chunk, unless the command exits early as keyword-only or rejected
noise.

Even high-salience chunks first enter L1, then are marked `promoted = true`
after L2 processing.

### When Does a Tick Stay Only in L1?

When chunk salience is below `0.25` and the entry has not yet been displaced or
flushed.

It may still be queryable because query scans L1 first. Querying an L1 entry can
increase `rehearsal_count`, but promotion on displacement currently only checks
`promoted`, not rehearsal count.

### When Does a Tick Enter L2?

Immediate L2 entry or merge happens when salience is at least `0.25`.

Delayed L2 insertion happens when:

- an unpromoted L1 entry is displaced by capacity pressure
- session start flushes L1
- context switch flushes L1

### When Does a Tick Enter L3?

There are three paths:

1. Immediate entity graph update during high-salience merge/insert.
2. Delayed entity graph update during L1 displacement or L1 flush.
3. Summary-node systems consolidation during `consolidate()`.

Additionally, active ticks can promote recurring terms into L3 Keyword nodes
after enough repeated entity observations.

### When Is a Tick Lost?

Rejected before storage:

- empty input
- noise tick
- keyword-only tick stores only keyword graph nodes, not an L1/L2 memory

Pruned later:

- L2 entry can decay and fail prune threshold.
- L2 entry can be evicted at capacity.
- L3 nodes and edges can decay below graph prune threshold or be removed by
  hard capacity caps.

Not silently lost:

- L1 displacement promotes unpromoted entries.
- L1 flush promotes unpromoted entries.

## Example Walkthroughs

### Example 1: Low-Salience Tick

Input:

```text
updated docs
```

Expected lifecycle:

1. CLI accepts if length is at least 10 chars.
2. `tick_impl()` increments clock.
3. Existing L2/L3 decay.
4. One chunk.
5. Embedded.
6. Salience near floor.
7. Emotional valence near zero.
8. Pushed to L1.
9. Fails attention gate.
10. No immediate L2 insert.
11. No immediate L3 update.
12. L2/L3 prune runs.
13. Active tick updates term frequency if entities exist.
14. Smart consolidation checks run.
15. Saved to `.legend/memory.lz4`.

Final state: L1-only unless later displaced or flushed.

### Example 2: Decision Tick

Input:

```text
DECISION: Use SQLite because this project is single-user and needs zero setup.
```

Expected lifecycle:

1. Salience gets decision and rationale boosts.
2. Enters L1.
3. Passes attention gate.
4. Best L2 match is found.
5. If similar enough and word overlap passes, it merges.
6. Otherwise, a new L2 `ShortTermEntry` is inserted with wall-clock, extracted
   dates if any, TCM snapshot, salience, refs, density, stability `1.0`.
7. L3 graph extracts entities and creates/reinforces nodes/edges.
8. L1 entry is marked promoted.
9. Internal `retrieve_context()` runs and can reinforce related memories/graph.
10. Term frequencies update.
11. Pruning runs.
12. Consolidation may run if thresholds are reached.
13. Session log stores the original text.

Final state: L1 promoted copy plus L2 entry/merge plus L3 graph structure.

### Example 3: Multi-Sentence Tick

Input:

```text
DECISION: Use SQLite because setup matters. BUG: migrations fail on empty DB.
```

Expected lifecycle:

- Chunk 1 goes through the full per-chunk path.
- Chunk 2 goes through the full per-chunk path.
- Each chunk can independently create, merge, or remain L1-only.
- `TickResult` reports only the last chunk's action and entry ID.
- The session log stores the original combined text once.

### Example 4: Passive Tick

Input:

```text
legend memory tick --passive "Observed test runner latency around 250ms"
```

Expected lifecycle:

1. CLI prefixes `EXPERIENCE:`.
2. Brain clock still increments.
3. Decay still applies.
4. Chunk still enters L1.
5. If salience passes, it can still enter L2 and update L3.
6. No session-log append.
7. No active term-frequency promotion.
8. No CPEB/context-switch/auto-consolidation.
9. CLI halves the target L2 entry salience if there is one.

### Example 5: Keyword Directive Only

Input:

```text
KEYWORD:tool:bevy
```

Expected lifecycle:

1. CLI extracts directive.
2. Clean text is empty.
3. Adds or reinforces `kw:tool:bevy` graph node.
4. Rebuilds keyword cache if needed.
5. Saves memory.
6. Logs `keyword_register`.
7. Exits before `tick_impl()`.

Final state: L3 keyword changed. No L1/L2 tick entry.

## Current Gotchas

1. `ShortTermEntry` comments say consolidated entries are filtered from query
   results, but current retrieval intentionally does not filter them.
   Code comment is stale at `src/memory/hippocampus.rs:65`; behavior is in
   `src/memory/hippocampus.rs:203`.

2. `TickResult.action` comment says `"created"`, `"merged"`, or
   `"reconsolidated"`, but current `tick_impl()` can also return
   `"working_memory_only"`. Reconsolidation code exists but is disconnected from
   the tick path.
   Code: `src/tool/types.rs:89`, `src/memory/mod.rs:742`,
   `src/memory/hippocampus.rs:284`.

3. High-salience tick chunks call `retrieve_context()` internally, so tick can
   have query side effects and additional clock increments.
   Code: `src/memory/mod.rs:741`.

4. L1 displacement and flush promote entries without wall-clock/date/TCM
   metadata because `WorkingMemoryEntry` does not store those fields.
   Code: `src/memory/prefrontal.rs:59`,
   `src/memory/prefrontal.rs:95`.

5. The CLI still checks `should_suggest_consolidation()` after `tick_impl()`,
   although `tick_impl()` already auto-consolidates on the same thresholds.
   In normal current behavior this is mostly redundant because successful
   auto-consolidation resets the counter.
   Code: `src/commands/memory/tick.rs:101`,
   `src/commands/memory/tick.rs:154`,
   `src/tool/mod.rs:359`.

6. The salience word-count branch for `>50` words is unreachable because `>25`
   is checked first.
   Code: `src/memory/thalamus.rs:90`.

7. Multi-chunk ticks return only the last chunk's result, while all chunks still
   mutate memory.
   Code: `src/memory/mod.rs:502`,
   `src/memory/mod.rs:728`,
   `src/memory/mod.rs:869`.

## End State Summary

After a successful normal tick, Legend has potentially changed:

- `BrainState.clock`
- L1 `working_memory`
- L2 `short_term`
- L3 `long_term.nodes`, `long_term.edges`, `long_term.index`, `edge_index`
- L2 salience, emotional valence, usage, last access, stability
- L3 node/edge weights, salience, stability, activation counts
- rolling emotional intensity
- last tick embedding
- term-frequency statistics and keyword cache
- session log
- `.legend/events.jsonl`
- `.legend/.pending_ticks`
- `ARCHITECTURE.md` for architecture ticks
- `.legend/memory.lz4`

The exact outcome depends on chunking, salience, similarity thresholds, word
overlap, capacity pressure, emotional valence, context switching, and
consolidation triggers.
