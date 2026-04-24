# Tick prefix contract (#28)

**Recorded:** 2026-04-24

Closes queue item #28: "Define contract for structured tick prefixes
DECISION/BUG/ARCHITECTURE/PLAN." The user-facing prefixes have grown
organically. This doc states the contract explicitly.

## Recognized prefixes

| Prefix          | Storage             | Side effects                                                                  |
|-----------------|---------------------|-------------------------------------------------------------------------------|
| `PLAN: <name>`  | anterior PFC plans  | **Bypasses L1/L2/L3 entirely.** Body parsed as plan items.                    |
| `DECISION:`     | normal L1 → L2 → L3 | `classify_text` returns `MemoryCategory::Decision` (matches "decision" kw).    |
| `BUG:`          | normal pipeline     | `MemoryCategory::Bug` via "bug" kw; negative-valence salience boost.          |
| `BLOCKER:`      | normal pipeline     | Negative-valence salience boost; CLI `--blocker` flag prepends this prefix.   |
| `ARCHITECTURE:` | normal pipeline     | `MemoryCategory::Architecture`.                                               |
| `TODO:`         | normal pipeline     | `MemoryCategory::Todo`.                                                       |
| `COMPLETED:`    | normal pipeline     | **No special handling today.** Drops into General/Progress.                   |

## Contract

1. **`PLAN:` is the only structural prefix.** It changes the storage
   path: `anterior_pfc::strip_plan_prefix` peels it off and routes the
   body to `apply_plan` instead of `tick_impl`'s normal encoding.
   This is the contract that callers must honour — anything else
   that starts with `PLAN:` will not be stored as a normal memory.

2. **Other prefixes are advisory.** They influence classification
   and salience via keyword matches in `wernicke/lexicon.rs` and
   `amygdala::compute_emotional_valence`, but they do not change the
   pipeline. The tick still runs `chunk_text` → `embed_texts_batch` →
   L1/L2/L3 encoding.

3. **No prefix is required.** Plain free-form text is a valid tick.
   Classification falls through to `MemoryCategory::General` or
   `Progress` (when an action verb is present).

4. **Classification is keyword-driven, not prefix-driven.** The
   classifier matches against `DECISION_KEYWORDS`, `BUG_KEYWORDS`,
   etc., not the literal prefix string. `"DECISION: ..."` works
   because the lowercased "decision" matches. So does "we made a
   decision to ..." without the prefix.

5. **The CLI `--blocker` flag is the only one that injects a prefix.**
   `legend memory tick --blocker "..."` prepends `BLOCKER:` to the
   text. All other prefixes are user-supplied.

## What this contract is NOT

- It is not a typed schema. There's no enum check; unknown prefixes
  ("RFC:", "PROPOSAL:", "QUESTION:") just pass through as plain text
  and may match keyword classifiers if they happen to contain
  matching words.
- It is not a parsing contract for the rest of the body. After the
  prefix, the body is opaque to the tick pipeline (other than
  chunking and embedding).

## Gaps

1. **`COMPLETED:` has no special handling.** A natural addition:
   when a `COMPLETED:` tick mentions a queue item by number, mark
   it done. Not implemented; would need item-number extraction +
   plan lookup.
2. **No salience boost for `DECISION:` qua DECISION:.** Today the
   boost comes from the keyword matches in the body, not from the
   prefix itself. A `prefix-aware` salience pass could give all
   structured prefixes a baseline boost regardless of body content.
3. **No conformance test that locks the contract.** If someone
   accidentally changes the routing of `PLAN:`, only the
   `anterior_pfc` unit tests would catch it. A small
   `tests/conformance_tick_prefixes.rs` could verify each prefix's
   storage path and side effects end-to-end.

## Decision for this audit

- Document the current contract (this file).
- Do not change prefix semantics in this commit. Each gap is
  user-visible and should land with its own queue item, conformance
  test, and CHANGELOG note. Drive-by changes to the routing of
  user-supplied prefixes are how silent regressions enter the system.

## Future queue candidates

- Add `COMPLETED:` handler that ticks the relevant queue item to
  `done` (cross-reference to `legend memory plan set-status`).
- Add `tests/conformance_tick_prefixes.rs` to lock the routing.
- Consider a prefix-aware salience baseline (`DECISION:` text gets
  a small boost regardless of body).

## Related

- `src/memory/anterior_pfc.rs` — `PLAN:` parsing.
- `src/memory/mod.rs::classify_text` — keyword-driven categorization.
- `src/memory/wernicke/lexicon.rs` — keyword lists.
- `src/commands/mcp.rs:79` — the MCP tool description that documents
  the prefixes for LLMs (single source of truth for the user-facing
  list — keep this doc in sync).
- CLAUDE.md "Essential Commands" section.
