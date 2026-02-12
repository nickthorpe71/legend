# Legend: Vision & Architecture

> A layered memory system for LLM-assisted software development that cures AI amnesia.

## The Problem

Every time an LLM coding assistant starts a new session, it forgets everything:
architectural decisions, rejected approaches, project goals, style preferences,
what it just spent 3 hours building yesterday. Engineers repeat themselves endlessly,
the LLM re-proposes already-rejected ideas, and hard-won project knowledge evaporates.

## The Solution

Legend is a **stateful memory system** that sits between the developer, the LLM, and
the codebase. It provides persistent, layered memory modeled on human cognition —
not a database to query, but a **memory system that shapes behavior**.

Drop it into any project. Run `legend init`. The LLM gains persistent memory
automatically — no configuration, no forms, no setup wizards.

## Design Principles

1. **Zero-config by default.** `legend init` should work with no user input. Everything
   is auto-inferred from the codebase and progressively enriched by the LLM.
2. **Memory is for future decisions, not storage.** Every node must answer: "How does
   knowing this change what the LLM does next?"
3. **Finality beats freshness.** A battle-tested FINAL decision outranks any new proposal.
4. **LLM proposes, Legend disposes.** The LLM suggests memory updates; Legend enforces
   rules, permissions, and promotion gates.
5. **Forgetting is a feature.** Stale, unretrieved nodes decay and get pruned. Relevance
   over recall.
6. **Memory dynamics, not just storage.** Consolidation, priming, retrieval strengthening,
   and decay make this a *memory system*, not a filing cabinet.

---

## Memory Architecture: Five Layers

| Layer | Name | Human Analog | Decay Rate | LLM Write | Purpose |
|-------|------|-------------|------------|-----------|---------|
| **L1** | Working Memory | Short-term attention | Hours | Yes | Current task context, recent files, active focus |
| **L2** | Semantic Memory | Mental model | Months | Propose | System structure, surfaces, APIs, data models |
| **L3** | Decision Memory | Scar tissue | Never (FINAL) | Propose | Decisions, rejections, rationale, lessons learned |
| **L4** | Procedural Memory | Habit/taste | Months | Propose | Style rules, phase rules, behavioral constraints |
| **L5** | Executive Memory | Prefrontal cortex | Never | No (user-only) | Goals, non-goals, priorities, current phase |

### Node States

Every memory node (L2–L5) has a lifecycle state:

```
DRAFT  →  ACTIVE  →  FINAL
  ↓         ↓
DEPRECATED  DEPRECATED
```

- **DRAFT**: Proposed (usually by LLM). Unconfirmed. Low authority. Subject to auto-pruning.
- **ACTIVE**: Confirmed and in use. High authority. Shapes behavior.
- **FINAL**: Immutable. Cannot be contradicted without explicit escalation. Highest authority.
- **DEPRECATED**: Soft-deleted. Kept for history. Not included in context bundles.

L1 nodes have no state — they are ephemeral and auto-cleared.

### Node Metadata

Every node carries:

```
id:               Unique identifier
layer:            L1–L5
state:            Draft | Active | Final | Deprecated
content:          Layer-specific structured data
created_at:       Timestamp
updated_at:       Timestamp
source:           User | LLM | System (auto-inferred)
retrieval_count:  How many times preflight has included this node
last_retrieved_at: Last time this node appeared in a context bundle
salience:         0.0–1.0 weight (painful lessons > trivial preferences)
associations:     Vec<NodeId> — linked nodes that co-activate during priming
```

---

## Memory Dynamics

These mechanisms make Legend a *memory system* rather than a 5-folder database.

### Consolidation

After each session (or via `legend consolidate`):
- DRAFT nodes retrieved 3+ times auto-promote to ACTIVE
- DRAFT nodes unretrieved for 7+ days are auto-deprecated
- Duplicate/overlapping nodes within a layer are merged
- Nodes frequently retrieved together get associated (priming links)

### Retrieval Strengthening

Each time preflight includes a node in a context bundle:
- `retrieval_count` increments
- `last_retrieved_at` updates
- Decay timer resets (the node stays relevant longer)

This mirrors how recalling a memory strengthens it in the brain.

### Associative Priming

When preflight retrieves a node, it also retrieves associated nodes:
- If "PostgreSQL decision" (L3) is relevant, automatically include
  "database schema" (L2) and "SQL style rules" (L4)
- Associations are built from co-retrieval patterns during consolidation
- Prevents missing related-but-not-keyword-matching context

### Layer-Specific Decay

| Layer | Half-Life | Rationale |
|-------|-----------|-----------|
| L1 | 4 hours | Working memory is ephemeral by design |
| L2 | 90 days | System structure changes slowly |
| L3 DRAFT | 14 days | Unconfirmed decisions lose relevance quickly |
| L3 ACTIVE | 180 days | Confirmed decisions are long-lived |
| L3 FINAL | ∞ (no decay) | Final decisions are permanent |
| L4 | 90 days | Style rules evolve with the project |
| L5 | ∞ (no decay) | Goals and phase are always relevant |

Decay affects retrieval priority, not deletion. Low-relevance nodes drop out of
context bundles but remain in storage until consolidation prunes them.

### Salience Weighting

Not all memories are equal. A painful revert after 2 weeks of wrong-direction work
matters more than a cosmetic naming preference.

- **High salience (0.8–1.0)**: Production incidents, major reverts, architectural pivots
- **Medium salience (0.4–0.7)**: Technology choices, API design decisions
- **Low salience (0.1–0.3)**: Style preferences, minor conventions
- Salience is set by the user or inferred from node content keywords
  ("reverted", "broke", "critical", "lesson learned" → high salience)

---

## Runtime Flows

### Preflight (Read Path)

Triggered before the LLM processes a user prompt.

```
User Prompt
    ↓
[Intent Heuristic] — feature / refactor / debug / exploration
    ↓
[Layer Query]
    L5: Always include goals, non-goals, phase
    L4: Include rules matching current phase
    L3: Include FINAL always, ACTIVE by keyword relevance
    L2: Include surfaces matching affected files/keywords
    L1: Include current working context
    ↓
[Associative Priming] — follow node associations for related context
    ↓
[Drift Detection]
    - Prompt conflicts with non-goals? → WARNING
    - Prompt reopens FINAL decision? → HARD BLOCK
    - Prompt violates phase rules? → WARNING
    ↓
[Bundle Assembly] — prioritize by layer authority, respect token budget
    ↓
Context Bundle (JSON to stdout)
Warnings/blocks (to stderr)
Exit code: 0=green, 1=warning, 2=block
```

### Token Budget Allocation

Default budget: 4096 tokens (configurable via `--budget`).

| Layer | Allocation | Rationale |
|-------|-----------|-----------|
| L5 | 10% (~400 tokens) | Small but always present |
| L3 FINAL | 25% (~1024 tokens) | Highest authority content |
| L4 | 15% (~600 tokens) | Rules are concise |
| L3 ACTIVE/DRAFT | 20% (~800 tokens) | Relevant decisions |
| L2 | 20% (~800 tokens) | System structure |
| L1 | 10% (~400 tokens) | Current task context |

Within each layer, nodes rank by: `state_authority × salience × recency × retrieval_strength`

### Postflight (Write Path)

After the LLM completes work:

```
LLM proposes Memory Deltas (JSON)
    ↓
[Validate]
    - L5 writes rejected (user-only)
    - FINAL node modifications rejected
    - Layer-appropriate content check
    ↓
[Apply]
    - New nodes created as DRAFT
    - Updates applied to non-FINAL nodes
    - Deprecations marked
    ↓
[Consolidation Trigger] — if session ending, run consolidation pass
```

### Bootstrap Mode (Cold Start)

When preflight detects <5 total nodes across L2–L5:

1. Skip drift detection (nothing to detect against)
2. Include a bootstrap prompt instructing the LLM to:
   - Read key project files (README, config, entry points)
   - Propose L2 surfaces from what it finds
   - Propose L4 style rules from existing linter/CI config
3. Ask the user one natural question: "What are you working on right now?"
   (seeds L1 and gives context for L5 goal inference)
4. After 5+ total nodes exist, exit bootstrap mode

---

## Integration Model

### Primary: Claude Code Hooks

Three lifecycle hooks in `.claude/settings.json`:

| Hook | Trigger | Legend Command |
|------|---------|---------------|
| **SessionStart** | Session begins | `legend preflight --bootstrap` |
| **UserPromptSubmit** | User sends prompt | `legend preflight "<prompt>"` |
| **Stop** | LLM finishes | `legend propose` (reads deltas from LLM output) |

### Future: MCP Server

Legend as an MCP tool provider, exposing:
- `legend_preflight(prompt)` — returns context bundle
- `legend_propose(deltas)` — accepts memory updates
- `legend_query(layer, filter)` — direct layer queries

This enables Cursor, Continue, Cody, Aider, and any MCP-compatible tool to use Legend.

---

## CLI Command Summary

| Command | Purpose |
|---------|---------|
| `legend init` | Zero-config setup: create .legend, scan codebase, setup hooks |
| `legend preflight "<prompt>"` | Assemble context bundle, detect drift |
| `legend propose` | Accept memory delta proposals from stdin |
| `legend review` | List/accept/reject DRAFT nodes |
| `legend consolidate` | Run consolidation pass (promotion, pruning, association) |
| `legend get-state` | Dump full state as JSON (<5ms) |
| `legend executive list` | Show L5 goals, non-goals, phase |
| `legend executive add-goal <title>` | Add an L5 goal (user-only) |
| `legend executive add-non-goal <title>` | Add an L5 non-goal (user-only) |
| `legend executive set-phase <phase>` | Set current project phase |
| `legend decision list [--state X]` | Show L3 decisions |
| `legend decision finalize <id>` | Promote decision to FINAL |
| `legend semantic list` | Show L2 surfaces |
| `legend procedural list` | Show L4 rules |
| `legend working set "<context>"` | Set L1 working context |
| `legend working clear` | Clear L1 |

---

## Performance Targets

| Operation | Target | Rationale |
|-----------|--------|-----------|
| `get-state` | <5ms | LLM reads on every prompt |
| `preflight` | <50ms | Runs before every LLM response |
| `propose` | <500ms | Write path, less latency-sensitive |
| `consolidate` | <2s | Runs post-session, not blocking |
| State file size | <1MB | Even for large projects with 500+ nodes |
