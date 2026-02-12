# Legend v2: Epics & Tasks

> Each task is a small, self-contained unit of work that leaves the system in a
> buildable, testable state. Run `cargo test` and `cargo build` after every task.
> Read all the code — this is a learning exercise as much as a build.

## Status Key

- [ ] Not started
- [~] In progress
- [x] Complete

---

## Epic 1: Core Data Model

> Replace Feature-centric types with layered memory nodes. Foundation for everything.

### Task 1.1: Add NodeState and LayerType enums
**File:** `src/types.rs`
**Work:**
- Add `NodeState` enum: `Draft`, `Active`, `Final`, `Deprecated`
- Add `LayerType` enum: `L1Working`, `L2Semantic`, `L3Decision`, `L4Procedural`, `L5Executive`
- Derive `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`, `Eq`, `Hash`
- Keep all existing types untouched
**Test:** Unit tests for serde round-trip on both enums.
**Status:** [ ]

### Task 1.2: Add MemoryNode struct
**File:** `src/types.rs`
**Work:**
- Add `MemoryNode` struct with fields:
  `id: String`, `layer: LayerType`, `state: NodeState`, `title: String`,
  `content: String`, `source: String` (user/llm/system),
  `created_at: i64`, `updated_at: i64`,
  `retrieval_count: u32`, `last_retrieved_at: Option<i64>`,
  `salience: f64`, `associations: Vec<String>`
- Keep all existing types untouched
**Test:** Unit test creating a node, serializing to bincode and back, verifying all fields.
**Status:** [ ]

### Task 1.3: Add layer-specific content types
**File:** `src/types.rs`
**Work:**
- `L5Content` struct: `kind: String` (goal/non-goal/phase), `description: String`, `priority: Option<u8>`
- `L3Content` struct: `decision: String`, `rationale: String`, `alternatives_rejected: Vec<String>`
- `L2Content` struct: `surface_type: String` (api/module/datamodel/service), `files: Vec<String>`, `dependencies: Vec<String>`
- `L4Content` struct: `trigger: String`, `constraint: String`, `applies_to_phase: Option<String>`
- All derive Serialize/Deserialize
- These are serialized into `MemoryNode.content` as JSON strings
**Test:** Unit tests for each content type serialization.
**Status:** [ ]

### Task 1.4: Add LegendState V2
**File:** `src/types.rs`
**Work:**
- Add `LegendStateV2` struct:
  `project_name: String`,
  `nodes: Vec<MemoryNode>`,
  `bootstrap_complete: bool`,
  `created_at: i64`,
  `last_updated: i64`
- Keep old `LegendState` and `Feature` for now
**Test:** Create state with nodes across multiple layers, serde round-trip.
**Status:** [ ]

### Task 1.5: Add ContextBundle and DriftWarning types
**File:** `src/types.rs`
**Work:**
- `DriftWarning` struct: `kind: String` (non-goal-conflict/final-reopened/phase-violation), `message: String`, `severity: String` (warning/block), `node_id: String`
- `ContextBundle` struct: `nodes: Vec<MemoryNode>`, `warnings: Vec<DriftWarning>`, `token_estimate: u32`, `is_bootstrap: bool`
**Test:** Serde round-trip tests.
**Status:** [ ]

### Task 1.6: Add MemoryDelta types
**File:** `src/types.rs`
**Work:**
- `MemoryDelta` struct: `action: String` (create/update/deprecate), `layer: LayerType`, `title: String`, `content: String`, `rationale: String`, `node_id: Option<String>`, `confidence: String` (high/medium/low)
- `DeltaResult` struct: `accepted: bool`, `reason: String`, `node_id: Option<String>`
**Test:** Serde round-trip.
**Status:** [ ]

### Task 1.7: Remove legacy types, rename V2
**Files:** `src/types.rs`
**Work:**
- Delete `Feature`, `FeatureStatus`, `LegendState`
- Rename `LegendStateV2` → `LegendState`
- Find and fix all compilation errors in other files (will cascade to storage, commands)
- This will temporarily break commands — that's fine, we'll rebuild them
**Test:** `cargo build` succeeds (commands may be stubbed/commented).
**Status:** [ ]

---

## Epic 2: Storage Layer

> Update storage to handle multi-layer state with <5ms reads.

### Task 2.1: Update save/load for new LegendState
**File:** `src/storage.rs`
**Work:**
- Update `save_state()` to accept new `LegendState`
- Update `load_state()` to return new `LegendState`
- Same bincode + LZ4 compression strategy
- Atomic write (temp file + rename)
**Test:** Round-trip test with 100 nodes across 5 layers. Timing assertion <5ms read.
**Status:** [ ]

### Task 2.2: Add node query helpers
**File:** `src/storage.rs`
**Work:**
- `get_layer(state, LayerType) -> Vec<&MemoryNode>` — filter by layer
- `get_by_state(state, NodeState) -> Vec<&MemoryNode>` — filter by state
- `get_by_layer_and_state(state, LayerType, NodeState) -> Vec<&MemoryNode>`
- `get_finals(state) -> Vec<&MemoryNode>` — all FINAL nodes across layers
- `find_node(state, id) -> Option<&MemoryNode>` — lookup by ID
**Test:** Create state with varied nodes, verify all queries return correct subsets.
**Status:** [ ]

### Task 2.3: Add node mutation helpers
**File:** `src/storage.rs`
**Work:**
- `add_node(state, node) -> Result` — append node, validate no duplicate ID
- `update_node_state(state, id, new_state) -> Result` — enforce: FINAL cannot change, DEPRECATED cannot promote
- `deprecate_node(state, id) -> Result` — set state to Deprecated
- `update_node_content(state, id, content) -> Result` — reject if FINAL
- All mutations update `last_updated` timestamp on both node and state
**Test:** Test each mutation, especially FINAL enforcement and error cases.
**Status:** [ ]

### Task 2.4: Add retrieval tracking helpers
**File:** `src/storage.rs`
**Work:**
- `mark_retrieved(state, node_ids: Vec<String>)` — increment `retrieval_count`, update `last_retrieved_at`
- `add_association(state, node_id_a, node_id_b)` — bidirectional link
**Test:** Mark nodes retrieved, verify counts. Add associations, verify both directions.
**Status:** [ ]

---

## Epic 3: Init & Bootstrap

> Zero-config initialization that auto-infers as much as possible.

### Task 3.1: Rewrite init for new state structure
**File:** `src/commands/init.rs`
**Work:**
- Create `.legend/` directory
- Auto-detect project name from directory name (keep existing logic)
- Create empty `LegendState` with `bootstrap_complete: false`
- Save state via storage layer
- Print confirmation to stderr
**Test:** Run `legend init` in a temp dir, verify `.legend/state.bin` created with correct structure.
**Status:** [ ]

### Task 3.2: Auto-discover L2 surfaces on init
**File:** `src/commands/init.rs`
**Work:**
- Scan for `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml` → extract project metadata
- Walk `src/`, `lib/`, `app/` directories → create DRAFT L2 nodes for major directories
- Read `README.md` first paragraph → store as project description in an L2 node
- All auto-discovered nodes get `source: "system"` and `state: Draft`
**Test:** Run init on a Rust project, verify L2 DRAFT surfaces match directory structure.
**Status:** [ ]

### Task 3.3: Auto-detect L4 style rules on init
**File:** `src/commands/init.rs`
**Work:**
- Scan for `.eslintrc*`, `clippy.toml`, `.rustfmt.toml`, `.prettierrc`, `.editorconfig`
- If found, create DRAFT L4 nodes: "Project uses [tool] for style enforcement"
- Scan for `CONTRIBUTING.md`, `.github/PULL_REQUEST_TEMPLATE.md` → create L4 node if found
**Test:** Run init on project with clippy.toml, verify L4 DRAFT node created.
**Status:** [ ]

### Task 3.4: Generate Claude Code hooks
**File:** `src/commands/init.rs`
**Work:**
- Create/update `.claude/settings.json` with hooks:
  - SessionStart: `legend preflight --bootstrap`
  - UserPromptSubmit: `legend preflight "$PROMPT"`
  - Stop: detect changes, prompt LLM to run `legend propose`
- Create/append `CLAUDE.md` with Legend usage instructions, delta JSON schema, layer descriptions
- Idempotent (safe to re-run) using marker comments
**Test:** Run init, verify hook file and CLAUDE.md content.
**Status:** [ ]

### Task 3.5: Make init fully idempotent
**File:** `src/commands/init.rs`
**Work:**
- If `.legend/state.bin` exists, don't overwrite — print "already initialized"
- If `.claude/settings.json` exists, merge hooks (don't clobber existing hooks)
- If `CLAUDE.md` exists, only append if marker not found
- Add `--force` flag to re-initialize from scratch
**Test:** Run init twice, verify no duplicates. Run with `--force`, verify clean reset.
**Status:** [ ]

---

## Epic 4: Layer Commands (L5 Executive)

> User-only commands for managing goals, non-goals, and project phase.

### Task 4.1: Scaffold executive command module
**Files:** `src/commands/executive.rs` (new), `src/commands/mod.rs`, `src/main.rs`
**Work:**
- Create `executive.rs` with subcommand enum: `AddGoal`, `AddNonGoal`, `SetPhase`, `List`
- Wire into `mod.rs` and `main.rs` CLI parser
- Stub each subcommand with `todo!()` or a println
**Test:** `cargo build`, `legend executive --help` prints subcommands.
**Status:** [ ]

### Task 4.2: Implement `executive add-goal`
**File:** `src/commands/executive.rs`
**Work:**
- Parse: `legend executive add-goal "Title" --description "..." --priority 1-5`
- Create `MemoryNode` with `layer: L5Executive`, `state: Active`, `source: "user"`
- Content: `L5Content { kind: "goal", description, priority }`
- Generate unique ID (timestamp + random suffix or similar)
- Load state, add node, save state
**Test:** Run command, then `legend get-state | grep` to verify node exists.
**Status:** [ ]

### Task 4.3: Implement `executive add-non-goal`
**File:** `src/commands/executive.rs`
**Work:**
- Parse: `legend executive add-non-goal "Title" --rationale "..."`
- Same pattern as add-goal but `kind: "non-goal"`
**Test:** Run command, verify node in state.
**Status:** [ ]

### Task 4.4: Implement `executive set-phase`
**File:** `src/commands/executive.rs`
**Work:**
- Parse: `legend executive set-phase "building" --description "..."`
- Deprecate any existing phase node (only one active phase allowed)
- Create new phase node with `kind: "phase"`, `state: Active`
**Test:** Set phase, verify. Set again, verify old phase deprecated.
**Status:** [ ]

### Task 4.5: Implement `executive list`
**File:** `src/commands/executive.rs`
**Work:**
- Query all L5 nodes (non-deprecated)
- Print human-readable table to stdout:
  - Current Phase: [phase]
  - Goals (by priority): [list]
  - Non-Goals: [list]
**Test:** Add goals, non-goals, phase, then list. Verify readable output.
**Status:** [ ]

---

## Epic 5: Layer Commands (L3 Decisions)

### Task 5.1: Scaffold decision command module
**Files:** `src/commands/decisions.rs` (new), `src/commands/mod.rs`, `src/main.rs`
**Work:**
- Create `decisions.rs` with subcommands: `Record`, `Finalize`, `List`
- Wire into CLI
**Test:** `cargo build`, `legend decision --help`.
**Status:** [ ]

### Task 5.2: Implement `decision record`
**File:** `src/commands/decisions.rs`
**Work:**
- Parse: `legend decision record "Title" --rationale "..." --rejected "alt1,alt2" [--salience 0.8]`
- Create ACTIVE node (user-recorded decisions start active, not draft)
- Content: `L3Content { decision, rationale, alternatives_rejected }`
- Optional `--finalize` flag to create as FINAL
**Test:** Record decision, verify in state. Record with `--finalize`, verify FINAL state.
**Status:** [ ]

### Task 5.3: Implement `decision finalize`
**File:** `src/commands/decisions.rs`
**Work:**
- Parse: `legend decision finalize <id>`
- Load state, find node, promote to FINAL
- Reject if already deprecated
- Print confirmation
**Test:** Record ACTIVE decision, finalize, verify. Try finalizing already-final — no error but no change.
**Status:** [ ]

### Task 5.4: Implement `decision list`
**File:** `src/commands/decisions.rs`
**Work:**
- Parse: `legend decision list [--state draft|active|final] [--all]`
- Filter by state, default shows ACTIVE + FINAL
- Human-readable table: ID (short), Title, State, Salience, Rationale (truncated)
**Test:** Create decisions in various states, verify filtering.
**Status:** [ ]

---

## Epic 6: Layer Commands (L2 Semantic, L4 Procedural, L1 Working)

### Task 6.1: Implement `semantic add`
**File:** `src/commands/semantic.rs` (new)
**Work:**
- Wire into CLI
- Parse: `legend semantic add "Name" --type api|module|datamodel|service --files "f1,f2" --description "..."`
- Create ACTIVE L2 node
**Test:** Add surface, verify in state.
**Status:** [ ]

### Task 6.2: Implement `semantic list`
**File:** `src/commands/semantic.rs`
**Work:**
- Show all non-deprecated L2 nodes
- Table: Name, Type, Files, State
**Test:** Add surfaces, verify list.
**Status:** [ ]

### Task 6.3: Implement `procedural add-rule`
**File:** `src/commands/procedural.rs` (new)
**Work:**
- Wire into CLI
- Parse: `legend procedural add-rule "Name" --trigger "when..." --constraint "must..." [--phase "building"]`
- Create ACTIVE L4 node
**Test:** Add rule, verify in state.
**Status:** [ ]

### Task 6.4: Implement `procedural list`
**File:** `src/commands/procedural.rs`
**Work:**
- Show all non-deprecated L4 nodes
- Optional `--phase` filter
**Test:** Add rules, verify list and filtering.
**Status:** [ ]

### Task 6.5: Implement `working set` and `working clear`
**File:** `src/commands/working.rs` (new)
**Work:**
- Wire into CLI
- `legend working set "context text"` — create/replace single L1 node
- `legend working clear` — deprecate all L1 nodes
- Only one active L1 node at a time
**Test:** Set, verify. Clear, verify empty. Set again, verify old one replaced.
**Status:** [ ]

---

## Epic 7: Preflight Pipeline

> The read path: classify intent, query layers, detect drift, assemble context bundle.

### Task 7.1: Create preflight module scaffold
**File:** `src/preflight.rs` (new), `src/main.rs`
**Work:**
- Create module with placeholder functions: `classify_intent()`, `query_layers()`, `detect_drift()`, `assemble_bundle()`
- Add `preflight` subcommand to CLI that calls through to module
- Initially just load state and return it as-is
**Test:** `cargo build`, `legend preflight "test"` outputs something.
**Status:** [ ]

### Task 7.2: Implement bootstrap detection
**File:** `src/preflight.rs`
**Work:**
- Count total non-deprecated nodes across L2–L5
- If <5 nodes: set `is_bootstrap: true` on bundle
- In bootstrap mode: include a system prompt telling the LLM to explore the project and propose L2/L4 nodes
- Skip drift detection in bootstrap mode
**Test:** Init fresh project, run preflight, verify bootstrap flag and prompt included.
**Status:** [ ]

### Task 7.3: Implement intent classification
**File:** `src/preflight.rs`
**Work:**
- `Intent` enum: `Feature`, `Refactor`, `Debug`, `Exploration`, `Question`
- Keyword heuristic for MVP:
  - "add", "create", "implement", "new" → Feature
  - "refactor", "clean", "reorganize", "extract" → Refactor
  - "fix", "bug", "error", "broken", "failing" → Debug
  - "explore", "investigate", "research", "understand" → Exploration
  - "what", "how", "why", "explain" → Question
- Default to `Feature` if ambiguous
- Intent affects which layers get heavier weight in bundle assembly
**Test:** Unit tests for 10+ prompt strings → expected intents.
**Status:** [ ]

### Task 7.4: Implement layer querying
**File:** `src/preflight.rs`
**Work:**
- `query_layers(state, intent, prompt) -> Vec<MemoryNode>`
- Always include: all L5 ACTIVE nodes, all L3 FINAL nodes
- Include L4 rules matching current phase
- Include L1 working context
- Include L2/L3 ACTIVE nodes with keyword relevance (case-insensitive substring match on title/content vs prompt words)
- Sort by: layer authority → state authority → salience → recency
**Test:** Create state with nodes across layers, query with prompt, verify correct nodes returned in priority order.
**Status:** [ ]

### Task 7.5: Implement associative priming
**File:** `src/preflight.rs`
**Work:**
- After initial query, check `associations` on each retrieved node
- Pull in associated nodes if not already in result set
- Cap at +5 primed nodes to prevent explosion
**Test:** Create nodes with associations, verify priming pulls linked nodes.
**Status:** [ ]

### Task 7.6: Implement drift detection
**File:** `src/preflight.rs`
**Work:**
- `detect_drift(prompt, nodes) -> Vec<DriftWarning>`
- Check each L5 non-goal: does prompt contain keywords from non-goal title/content? → WARNING
- Check each L3 FINAL node: does prompt suggest revisiting this decision? (keyword overlap) → HARD BLOCK
- Check L4 phase rules: does prompt violate current phase constraints? → WARNING
- Return list of warnings with severity
**Test:** Unit tests for each drift type with crafted prompts and nodes.
**Status:** [ ]

### Task 7.7: Implement bundle assembly with token budget
**File:** `src/preflight.rs`
**Work:**
- `assemble_bundle(nodes, warnings, budget) -> ContextBundle`
- Estimate tokens: ~4 chars per token, count node title + content lengths
- Allocation: L5 10%, L3-FINAL 25%, L4 15%, L3-other 20%, L2 20%, L1 10%
- Fill each bucket up to its allocation, highest-priority nodes first
- If a bucket isn't full, redistribute to next priority bucket
- Set `token_estimate` on bundle
- Default budget: 4096, configurable via `--budget` flag
**Test:** Create state with many nodes, verify bundle respects budget and priority ordering.
**Status:** [ ]

### Task 7.8: Wire preflight command end-to-end
**File:** `src/commands/preflight.rs` (new), `src/main.rs`
**Work:**
- `legend preflight "<prompt>" [--budget N] [--bootstrap]`
- Run full pipeline: load state → classify → query → prime → drift → assemble
- Mark retrieved nodes (update retrieval_count/last_retrieved_at) and save state
- Output: JSON context bundle to stdout
- Output: Warnings/blocks to stderr
- Exit code: 0 = green, 1 = warning, 2 = block
**Test:** End-to-end test with populated state. Verify JSON output parses, exit codes correct.
**Status:** [ ]

---

## Epic 8: Memory Delta System

> The write path: LLM proposes memory updates, Legend validates and applies.

### Task 8.1: Implement delta validation
**File:** `src/delta.rs` (new)
**Work:**
- `validate_delta(state, delta) -> DeltaResult`
- Reject if `delta.layer == L5` and source is LLM
- Reject if action is "update" and target node is FINAL
- Reject if action is "deprecate" and target node is FINAL (require user escalation)
- Reject if action is "create" and layer is invalid
- Accept otherwise, return reason
**Test:** Unit tests for each rejection case and valid cases.
**Status:** [ ]

### Task 8.2: Implement delta application
**File:** `src/delta.rs`
**Work:**
- `apply_delta(state, delta) -> Result<String>` (returns new node ID)
- For "create": generate ID, create node as DRAFT with `source: "llm"`
- For "update": find node, update content (reject FINAL), bump updated_at
- For "deprecate": set state to Deprecated
**Test:** Apply each action type, verify state changes.
**Status:** [ ]

### Task 8.3: Implement propose command
**File:** `src/commands/propose.rs` (new), `src/main.rs`
**Work:**
- `legend propose` — reads JSON array of `MemoryDelta` from stdin
- Validate each delta, apply valid ones, collect results
- Output: JSON array of `DeltaResult` to stdout
- Report rejected deltas to stderr with reasons
**Test:** Pipe valid and invalid deltas, verify correct acceptance/rejection.
**Status:** [ ]

### Task 8.4: Implement review command
**File:** `src/commands/propose.rs`
**Work:**
- `legend review` — list all DRAFT nodes (pending review)
- `legend review --accept <id>` — promote DRAFT → ACTIVE
- `legend review --reject <id>` — deprecate DRAFT node
- `legend review --accept-all` — promote all DRAFTs to ACTIVE
**Test:** Create drafts via propose, review list, accept some, reject others.
**Status:** [ ]

---

## Epic 9: Consolidation Engine

> Post-session processing: promotion, pruning, association building.

### Task 9.1: Implement auto-promotion logic
**File:** `src/consolidation.rs` (new)
**Work:**
- `auto_promote(state) -> Vec<String>` (returns promoted node IDs)
- DRAFT nodes with `retrieval_count >= 3` → promote to ACTIVE
- Log each promotion to stderr
**Test:** Create drafts with varied retrieval counts, run promote, verify correct ones promoted.
**Status:** [ ]

### Task 9.2: Implement auto-pruning logic
**File:** `src/consolidation.rs`
**Work:**
- `auto_prune(state, now) -> Vec<String>` (returns pruned node IDs)
- DRAFT nodes with `last_retrieved_at` older than 7 days (or never retrieved + created >7 days ago) → deprecate
- Respect layer-specific decay rates (L3 FINAL and L5 are immune)
**Test:** Create old unretrieved drafts, run prune, verify deprecated.
**Status:** [ ]

### Task 9.3: Implement association building
**File:** `src/consolidation.rs`
**Work:**
- `build_associations(state, retrieval_log) -> u32` (returns count of new associations)
- Track which nodes were co-retrieved in the same preflight call (need to log this)
- If two nodes have been co-retrieved 3+ times, add bidirectional association
- Add retrieval log to state: `Vec<(i64, Vec<String>)>` (timestamp, node_ids)
**Test:** Simulate multiple preflight retrievals, run association builder, verify links created.
**Status:** [ ]

### Task 9.4: Implement consolidate command
**File:** `src/commands/consolidate.rs` (new), `src/main.rs`
**Work:**
- `legend consolidate` — runs all three passes: promote, prune, associate
- Output summary to stderr: "Promoted: N, Pruned: N, Associations: N"
- Add to Claude hooks Stop event: suggest running consolidate at session end
**Test:** End-to-end with populated state, verify all three passes run.
**Status:** [ ]

---

## Epic 10: CLI Cleanup & Polish

> Remove legacy commands, update help text, polish output.

### Task 10.1: Update get-state for new structure
**File:** `src/commands/get_state.rs`
**Work:**
- Output new `LegendState` as JSON
- Maintain <5ms performance target
- Add `--layer L2` flag to filter output by layer
- Add `--compact` flag for minimal JSON (no whitespace)
**Test:** Benchmark read time. Verify JSON output parses correctly.
**Status:** [ ]

### Task 10.2: Remove legacy command files
**Files:** `src/commands/show.rs`, `src/commands/search.rs`, `src/commands/update.rs`, `src/commands/discover.rs`
**Work:**
- Delete files
- Remove from `src/commands/mod.rs`
- Remove from `src/main.rs` CLI parser
- Clean up any dead imports
**Test:** `cargo build` succeeds. `legend --help` shows only v2 commands.
**Status:** [ ]

### Task 10.3: Add comprehensive help text
**File:** `src/main.rs`
**Work:**
- Add description and examples for every subcommand
- `legend --help` shows overview of the memory model
- Each subcommand `--help` shows usage examples
**Test:** Manual review of all help text.
**Status:** [ ]

### Task 10.4: Add `--json` flag to all list commands
**Files:** `src/commands/executive.rs`, `src/commands/decisions.rs`, `src/commands/semantic.rs`, `src/commands/procedural.rs`
**Work:**
- All `list` subcommands default to human-readable table
- `--json` flag outputs machine-readable JSON (for LLM consumption)
**Test:** Verify both output formats for each list command.
**Status:** [ ]

---

## Epic 11: Documentation

### Task 11.1: Update README.md
**File:** `README.md`
**Work:**
- New overview: Legend as memory system (not feature tracker)
- Quick start: `cargo install`, `legend init`, done
- Memory layer explanations with examples
- Command reference with examples
- How it integrates with Claude Code
**Status:** [ ]

### Task 11.2: Update CLAUDE.md template
**File:** Generated by `src/commands/init.rs`
**Work:**
- Document all Legend commands the LLM should use
- Include delta JSON schema with examples
- Explain layer model so LLM understands what to propose
- Explain DRAFT → ACTIVE → FINAL lifecycle
- Include "don't contradict FINAL decisions" instruction
**Status:** [ ]

### Task 11.3: Performance validation
**File:** `PERFORMANCE.md`
**Work:**
- Benchmark get-state (<5ms)
- Benchmark preflight (<50ms) with 100, 500, 1000 nodes
- Benchmark propose (<500ms)
- Document results and optimization notes
**Status:** [ ]

---

## Execution Order

The tasks should be completed in this order. Each task is independently testable.

### Phase 1: Foundation (Epic 1 + 2)
1. Task 1.1 → 1.2 → 1.3 → 1.4 → 1.5 → 1.6 (types, no breakage)
2. Task 1.7 (remove legacy types — things break)
3. Task 2.1 → 2.2 → 2.3 → 2.4 (storage works with new types)

### Phase 2: Bootstrap (Epic 3)
4. Task 3.1 → 3.2 → 3.3 → 3.4 → 3.5 (init works again)

### Phase 3: Layer Commands (Epics 4, 5, 6)
5. Task 4.1 → 4.2 → 4.3 → 4.4 → 4.5 (L5 executive)
6. Task 5.1 → 5.2 → 5.3 → 5.4 (L3 decisions)
7. Task 6.1 → 6.2 (L2 semantic)
8. Task 6.3 → 6.4 (L4 procedural)
9. Task 6.5 (L1 working)

### Phase 4: Intelligence (Epics 7, 8, 9)
10. Task 7.1 → 7.2 → 7.3 → 7.4 → 7.5 → 7.6 → 7.7 → 7.8 (preflight)
11. Task 8.1 → 8.2 → 8.3 → 8.4 (delta system)
12. Task 9.1 → 9.2 → 9.3 → 9.4 (consolidation)

### Phase 5: Polish (Epics 10, 11)
13. Task 10.1 → 10.2 → 10.3 → 10.4 (CLI cleanup)
14. Task 11.1 → 11.2 → 11.3 (documentation)

---

## Task Count Summary

| Epic | Tasks | Estimate |
|------|-------|----------|
| 1. Core Data Model | 7 | 3–4 hours |
| 2. Storage Layer | 4 | 2–3 hours |
| 3. Init & Bootstrap | 5 | 3–4 hours |
| 4. L5 Executive | 5 | 2–3 hours |
| 5. L3 Decisions | 4 | 2–3 hours |
| 6. L2/L4/L1 Commands | 5 | 2–3 hours |
| 7. Preflight Pipeline | 8 | 5–6 hours |
| 8. Delta System | 4 | 3–4 hours |
| 9. Consolidation | 4 | 3–4 hours |
| 10. CLI Cleanup | 4 | 2–3 hours |
| 11. Documentation | 3 | 2–3 hours |
| **Total** | **53** | **~30–40 hours** |
