# CLI reference

One binary, six verbs. Every verb reads/writes JSON on stdio and exits non-zero
with a structured error on failure.

```
legend save|recall|init|dump|audit|mcp-serve [payload] [--pretty] [--reset]
```

A payload can be passed as the trailing argument or piped on stdin. `--pretty`
formats output for humans; without it, output is compact single-line JSON.

## Verbs

### `init [--reset]`
Creates a `.legend` store in the target directory (idempotent — re-running is a
no-op that re-prints the status). The fresh store seeds the built-in ontology (32
elements, 10 relations). `--reset` wipes an existing store back to seed.

It also scaffolds the project for both agents, each written only if absent (never
clobbers): for **Claude Code** a `.mcp.json` (the `legend` MCP server) and
`.claude/settings.json` (SessionStart orientation-inject, UserPromptSubmit ambient
recall, Stop save-reminder); for **Codex CLI** a `.codex/config.toml`
(`[mcp_servers.legend]` + per-tool `approval_mode`) and a minimal `AGENTS.md`
(recall-at-start / save-durable-facts nudge — Codex has no session/prompt/stop
hooks, so AGENTS.md, injected on the first turn, is its only always-on channel).
The `init` status JSON reports each path and whether it was created.

### `save [payload]`
Runs one tick over the payload and prints the resulting frame. The graph is
resolved, mutated, persisted to the snapshot, and the vector sidecar is
refreshed — all under the store lock.

### `recall [payload]`
Resolves the requested focus and prints a frame without mutating the graph
(unless the payload sets `observe`, which records the access).

**Section caps.** The typed sections — `decisions`, `constraints`, `open`,
`causal`, and each custom-kind section — emit at most 10 entries, newest first.
`limit` budgets `recent`/`related`; it does not govern these. Whatever a cap
holds back is counted in a top-level `omitted` object, emitted only when
something was actually dropped:

```json
"decisions":[ ...10... ],
"omitted":{"decisions":93,"constraints":66,"open":13,"causal":26}
```

This is a bound on the *surface*, never on the store: nothing is pruned, and a
focused `recall` still reaches anything a cap held back. It exists because the
sections were previously uncapped on the reasoning that the typed sections are
the ones worth never truncating — which holds at 10 decisions and fails at 103.
The live trial packet reached 51KB (decisions alone 18KB, constraints 12KB)
against a SessionStart hook feeding the model its first 4000 bytes, so the cut
landed inside `overview` and the protected sections were exactly the ones being
dropped. Capped, that same packet is 18.9KB of JSON / 13.3KB pretty, and every
section reaches the model.

### `dump`
Prints a human-readable rendering of the whole graph (ids resolved to names).
Read-only.

### `audit`
Scans the store for things a human should look at and prints them ranked,
most-actionable first. Read-only and tick-free — like `observe`, it leaves the
snapshot byte-identical, so it is safe to run against a live store mid-session.

Every check is a deterministic graph query: no embeddings, no model, no
mutation. The oracle computes *suspicion*; a human decides what is actually
wrong. Nothing here repairs anything, and that is deliberate — a store can hold
a fact that is stale on purpose (design intent the code has since drifted from),
and the disagreement is the signal, so an automatic pruner would destroy exactly
the diagnostic value worth keeping. Repairs go back through the ordinary write
verbs: `retract`, `merge`, `changes`, `rename_to`, or resubmitting a summary.

| reason | what it means |
|---|---|
| `phantom_close` | a `resolves` fact whose target has no kind — it closed nothing and left a decoy beside the still-open item |
| `status_fact` | a status-flavored property (`status`, `standing`, `phase`, …) written as a plain fact, which accretes and goes stale; `changes` supersedes and keeps history |
| `near_dup` | two live names similar enough that dedup arguably should have folded them |
| `prose_name` | a whole sentence passed where a canonical name belongs (fact objects and attr values become element names) |
| `stale_open` | a question/task with no `resolves` edge, untouched for `50+` ticks |
| `orphan` | an element no live relation references at all — dangling provenance, or what a retraction left behind |
| `bloat` | a summary past ~280 chars, which should split into child elements |

Output carries the full tally in `counts` even when the printed list is capped
at five per reason; whatever the cap drops is named in `truncated`, so the list
is short but never quietly short.

The cap is per reason and overridable:

```
legend audit                    # 5 per reason
legend audit '{"limit":2}'      # 2 per reason
legend audit '{"limit":null}'   # all of them
```

Pass `null` when actually working through a group — triaging seventeen suspects
five at a time is not triage. `limit` is the only key `audit` accepts.

Thresholds were set by measurement, not taste. `near_dup` is the fussiest and
the least proven — it needs three guards before a similarity score means
anything:

- **Digits veto a pair.** Numbered siblings that must NOT pair (`trial round 1`
  vs `round 2`, 0.83) score *higher* than genuine typo duplicates that must
  (`abstention` vs `abstension`, 0.80), so no threshold alone separates them.
  Digits carry identity here, the same reading `rel_exception_protected` takes.
- **A `changes` from/to pair is never a duplicate.** An edited value's old and
  new forms are near-identical by nature, and folding them would destroy the
  history `changes` exists to keep.
- **Names under 12 bytes are not compared.** One differing byte in six still
  leaves ~0.6 Jaccard.

The bar itself is 0.72, set above the false positives a real store produced
(`regions design adversarial review` vs `loot design adversarial review`, 0.685
— same review type, different subsystem). Even so, `near_dup` found 0 confirmed
defects across 815 elements; treat it as advisory. The sharp checks are
`phantom_close` and `status_fact`. `prose_name` triggers past 120 bytes against
a measured median name of 29.

### Read `per_1k_elements`, not `counts`

Every count ships with a rate beside it:

```json
"counts":{"bloat":303,"status_fact":17,"stale_open":23,"flat_decision":1},
"per_1k_elements":{"bloat":287.7,"status_fact":16.1,"stale_open":21.8,"flat_decision":0.9}
```

**A raw count rises with the store and says nothing about health.** Read raw,
the trial store's numbers across two rounds looked like collapse — bloat 124 →
235 → 303, status_fact 12 → 17, stale_open 20 → 23. Per 1k elements the same
period reads: bloat 181 → 252 → 287 (a real but far smaller decay), status_fact
17.5 → 18.2 → 16.2 (**flat**), stale_open 29.2 → 24.6 → 21.9 (**improving**).

Everyone who read the old output drew the wrong conclusion, including the people
who wrote it — "predicate proliferation is accelerating" was recorded three
times while predicate density was in fact *falling*. The rate is the number that
carries the signal.

### `health.jsonl` — the series `audit` cannot give you

`audit` is a snapshot: it says bloat is 303 and cannot say whether that is
climbing, flat, or recovering. Direction is usually the actual question.

Each session-start recall appends one line to `.legend/health.jsonl`:

```json
{"ts":1785800000,"build":"a881751","clock":193,"elements":1053,"live_relations":4121,
 "counts":{...},"per_1k_elements":{...},
 "summaries":{"n":605,"mean":401,"p50":401,"p90":636,"max":1114}}
```

Session start is the cadence deliberately — per-save would be noise and would
put a scan on the write path; on demand only exists once someone already
suspects something. One line per session tracks the unit of work, and the file
is append-only, cheap, and safe to delete.

To read a trend, diff the first and last lines; the rates are directly
comparable across any two points, which the raw counts are not.

### The prose block — read this instead of the `bloat` count

`legend audit` closes with the summary-length distribution over the same
elements the `bloat` check ranges on:

```json
"prose":{"summaries":328,"chars":110776,"max":815,"p90":543,"mean":337,"over":400}
```

**The `bloat` count tracks store size, not store health, and should not be read
as a health signal.** Applying the documented remedy — split a wall into a short
core plus child elements — moves the count the *wrong* way, because the detail
still has to live somewhere and every child clears the threshold too (measured:
272 → 286). What improved on that same split was `max`, 4047 → 1405. So `max`,
`p90` and `chars` are what answer "are summaries becoming walls"; `over` just
echoes the threshold the count used.

Worked example: a store reporting `bloat: 108` alongside `max: 815, p90: 543,
mean: 337` is **healthy** — 108 summaries sit just past a 400-byte line and none
is a wall. Reading the count alone there produces a false alarm.

### The orientation tally

A `recall` with no focus carries a store-health tally in its `overview` header:

```json
"overview":{"elements":815,"relations":1936,"clock":251,
            "audit":{"status_fact":7,"prose_name":17,"stale_open":15,
                     "orphan":10,"bloat":267},
            "scope":{...}}
```

Emitted only when something is flagged — a clean store says nothing, so
maintenance stays a pull rather than a standing nag. Placement in the header is
deliberate: it predates the section caps, and at the time the hook's `head -c
4000` cut landed inside `overview`, so a counter further down would never have
reached the model. With the caps in place the whole packet now arrives, but the
header is still the right home for a health line.

The tally skips `near_dup`. That pair sweep is O(n²) and costs 58ms of a 59ms
scan on an 815-element store, against the 4.6ms an entire orientation recall
takes; running it at every session start would make boot an order of magnitude
slower and degrade as the store grows. Without it the scan is ~4ms — the tally
costs about 0.2ms — and since `near_dup` is also the least precise check, the
ambient tally gives up nothing it could act on. A deliberate `legend audit`
still runs all seven.

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
{ "focus": ["Inferno", "the fire spell"], "query": "cast time",
  "observe": false, "limit": 20, "since": 0, "history_depth": 3 }
```

`focus` terms are resolved through the tiered index (exact name → alias → lexical
→ embedding). The frame reports what resolved, near-misses, the focused
subgraph, and supporting bands (current state, decisions, constraints, history,
related, sources). `observe: true` records the access (used by the eval probes).

`query` (optional) is the F1 ranking signal: the focus neighborhood's facts are
ranked by relevance to it instead of by recency. It is **never resolved to an
element**, so passing intent here is safe — unlike stuffing intent into `focus`,
which forces a full-store tier-2 miss scan when it doesn't resolve. Give the
*discriminating* intent (`"date of birth"`), not the whole question with the
entity name repeated (that dilutes the lexical pre-filter). Omit it for a general
recall (recency order). No effect when the embedder is off (`LEGEND_EMBED=0`).

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
