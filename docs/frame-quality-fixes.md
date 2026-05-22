# Frame quality fixes — working plan

**Status:** working plan, not durable docs. Captures the five issues
surfaced by the two-turn conversation smoke test:

> "The language we're using is Rust" → "what language are we using?"

Sections for items 1 and 5 are full design-first proposals (research,
citations, open questions); items 2–4 are short because the fix is
direct. Retire this doc once items land — split design content into
durable docs (`docs/extraction-attribute-name-quality.md`,
`docs/query-shaped-retrieval.md`) if the work needs ongoing reference.

The five issues, sorted by scope (smallest first):

| # | Item | Scope | Design? |
|---|---|---|---|
| 4 | Clock never advances | one-liner | no |
| 2 | Seed anchors crowd focus | Step 12 filter | no |
| 3 | `active_frame` stale across sessions | Step 10 update | no |
| 1 | Bad relation extraction | Steps 5/6/8 | **yes** |
| 5 | No question→answer linking | Step 12 / new step | **yes** |

The smoke-test observation behind each item is in
[the original analysis](frame-as-surface.md) — not repeated here.

---

## Item 4: clock never advances

**Scope:** one-liner.

`hg.clock` is mutated in `merge.rs::merge_hypergraphs` (`src/merge.rs:128`)
and in test setup (`coref.rs::tests`, `merge.rs::tests`). Read at
`src/steps/build_relations.rs:271, 334, 465, 472, 494, 503, 892`
(via `stats.last_seen = hg.clock`, `created_at: hg.clock`,
event-name templating, etc.) and in `src/steps/frame.rs:380`
(`frame.tick = hg.clock`). **No pipeline step ever increments it.**
Every tick reads `clock = 0`; every persisted relation gets
`created_at = Tick(0)` and `last_seen = Tick(0)`.

Fix: bump `hg.clock` once per tick. Right place is at the top of
`lib::tick` (the in-process path) and `daemon::tick` (the daemon
path) — before any step reads `hg.clock`, so freshly-minted
relations carry the *new* tick value, not the prior one.

```rust
hg.clock = Tick(hg.clock.0 + 1);
```

Verify: two consecutive ticks show `tick 1` then `tick 2`. Existing
persistence round-trip test (`tests/persistence_round_trip.rs`) already
covers clock serialization, so no new test needed there — but add a
single integration test in `tests/v0_acceptance.rs` that asserts
`frame.tick` increments across two ticks.

---

## Item 2: seed anchors crowd the focus list

**Scope:** small, localized to Step 12.

Diagnosis. Every tick's focused_relations is dominated by relations
of shape `<seed-anchor> → instance_of → region_class` (R0–R29
roughly: `tasks`, `provenance`, `meta`, `project`, etc.). These are
seed-graph scaffolding — they exist to make the routing tree work,
not to be surfaced to consumers. They sit at conf=1.00 and
stats.activation hovers in the same band as content relations
because `policy.hebbian_rate = 0` in v0 (no activation
differentiation).

In the tick-2 example, 9 of 15 focused relations are scaffolding.

Fix: filter "structural" relations from `focused_relations`
(and `current_state`) in Step 12, where structural is:

```
relation has exactly one attribute whose name is "instance_of"
AND whose value (Element) is one of:
  - region_class
  - reference_frame_class
  - attribute_name_class
  - <any other class anchor declared in the seed>
```

The set of class anchors is stable and indexable from
`hg.by_name`. A helper:

```rust
fn is_structural_relation(hg: &Hypergraph, rid: RelationId) -> bool {
    let r = &hg.relations[rid.0 as usize];
    let mut instance_of_count = 0;
    let mut class_object = false;
    for attr in &r.attributes {
        if attr.name == hg.instance_of_attr {
            instance_of_count += 1;
            if let Term::Element(eid) = attr.value {
                let name = hg.elements[eid.0 as usize].names.first();
                if matches!(name.map(|s| s.as_str()),
                    Some("region_class")
                    | Some("reference_frame_class")
                    | Some("attribute_name_class")
                ) {
                    class_object = true;
                }
            }
        }
    }
    instance_of_count == 1 && class_object
}
```

Lives next to `is_cache_relation` in `src/steps/build_relations.rs`
or in a new `src/steps/anchor_classification.rs`. Called from
`src/steps/frame.rs::assemble_frame` to filter both the dense and
path-reinforced input lists *before* RRF.

Open question: should this be a substrate-side tag instead of a
runtime check? An "anchor" flag on the relation set at seed-load.
Cleaner, but adds a serialization field — defer to v0.1 unless
the runtime filter shows up in profiling.

**Caveat:** keep the relations themselves in the substrate.
They're load-bearing for routing. Just don't surface them in the
frame.

---

## Item 3: `active_frame` stale across sessions

**Scope:** moderate — `src/steps/hebbian.rs:422` (`derive_active_frame`).

Diagnosis. `derive_active_frame` walks `hg.recent_focus`
(`src/steps/hebbian.rs:423`) and returns the first non-pronoun,
non-question-word subject. It does not check tick distance and
doesn't see the current input's intent. So:

- A `recent_focus` entry pushed N ticks ago (potentially a different
  session) stays as the active_frame indefinitely unless overwritten.
- A query input (curiosity high, conviction low) inherits a topical
  active_frame that's not what the speaker is asking about.

Note: `derive_active_frame` already has a hardcoded pronoun /
wh-word list (`is_frame_unsuitable`, `src/steps/hebbian.rs:443-461`).
This is in tension with the [[no-hardcoded-language-wordlists]]
feedback. Probably worth a separate cleanup — but out of scope for
this plan.

Fix, two parts:

1. **Recency gate.** Reject recent_focus entries older than N ticks
   from the current `hg.clock`. v0 value of N: the recent_focus
   depth invariant (likely 8 — confirm in `Policy`). Anything
   pushed > N ticks ago is stale by construction.

   Requires item 4 (clock advancing) to be useful. Without it
   "older than N ticks" never fires.

2. **Intent gate.** Pass `&Intent` into `derive_active_frame`.
   When the input is query-shaped (curiosity > 0.6, conviction <
   0.4 — tune in `Policy`), return `None`. Better to surface no
   active_frame than to lie with a stale topic.

   The Intent is already computed in Step 1 and is available at
   the call sites (`src/lib.rs:149`, `src/daemon.rs:312`). Just
   needs threading through.

Open question: how does the caller (Step 12) know an active_frame
*should* exist for this query? It doesn't — the consumer
treats `None` as "no current topical anchor." Acceptable for v0.
Future work: extract the topic from the query itself (item 5
territory).

---

## Item 1: relation extraction quality — design proposal

**Scope:** substantial. Touches Steps 5/6/8.
**Design-first per [[feedback-design-first]] convention.**

### Observed failure

Tick 1 input: `"The language we're using is Rust"` mints relation
`R617`:

```
subject = "language we're"
attribute_name = "using is"
value = "Rust"
status = Defeasible
confidence = 0.25
```

Both `language we're` and `using is` are minted as fresh elements
because they don't exist in `hg.by_name`. The attribute-name
"using is" becomes a permanent element in the substrate, with
`Polarity::Signal` (per `resolve_attribute_name` at
`src/steps/build_relations.rs:359`).

### Where it comes from

The bad relation has `PatternSource::Svo` and is emitted by
`extract_surface_relations` in `src/steps/relation_patterns/svo.rs:46-178`.
The SVO extractor's algorithm:

1. Find a subject span (NER-tagged or chunked).
2. Find the next verb token sequence.
3. The text *between* the subject's end and the next verb (or
   between verbs for multi-verb sentences) becomes the
   `attribute_name`. See `src/steps/relation_patterns/svo.rs:156`:
   ```rust
   let attr_text = text[prev_tail_end..obj_first.char_start].trim().to_string();
   ```
4. Object is the span between the verb and the next verb / EOS.

For `"The language we're using is Rust"`:
- Subject candidate: `language we're` (orthographic chunking pulled
  "language" + "we're" into one phrase — see `src/steps/run_extractors.rs`
  and the chunker)
- Verbs detected: `using`, `is`
- For verb `is`: subject_end…verb_start = `we're using`
- That becomes attribute_name; "is" is the verb; object after = `Rust`
- Result: `(language we're, using is, Rust)` — wait, but the
  observed output is `(language we're, using is, Rust)`, which
  matches if SVO took the verb chunk between subjects/objects as
  attribute, plus the previous verb included.

Either way: the extractor treats free text between content
boundaries as a relation predicate. That's a fundamental
mismatch with how Legend models predicates.

### The schema mismatch

Legend's substrate models relations as `(subject, attribute_name,
value)` triples where `attribute_name` is a **slot name** — a
canonical identifier like `instance_of`, `at`, `from`, `to`,
`current_tech`, `subject`, `target`. Slot names are single-token
in the seed; multi-token slot names only exist when explicitly
declared (e.g. `derived_from`, `is_a`).

SVO produces `(subject, verb_phrase, object)` — verb phrases are
*action descriptions*, not slot names. There's no clean mapping
from "is using" to a slot. The candidate slot name "using is"
isn't even a verb phrase — it's the wrong side of the boundary
because the algorithm split at the wrong verb.

### Three converging fixes

(a) and (b) constrain what attribute names are admissible; (c)
improves the upstream chunking.

#### (a) Gate `resolve_attribute_name` for novel multi-word labels

The right narrow fix: don't mint multi-word attribute-name elements
unless they're already in the lexicon. In
`src/steps/build_relations.rs:359` (`resolve_attribute_name`), before
the mint:

```rust
if !is_admissible_attribute_name(label, hg) {
    // Either reject the relation, OR fall back to a
    // pre-canonicalized predicate. v0: reject (caller drops the
    // relation candidate).
    return Err(AttrNameRejected);
}
```

Where `is_admissible_attribute_name`:

- Single whitespace-separated token: **admissible**.
- Multi-token but already in `hg.by_name` (seeded or
  promoted): **admissible**.
- Multi-token, novel: **inadmissible** in v0.

This matches the seed reality (canonical names are single-token)
and the typed-relation-lexicon Phase 2 work (commit `1b57274`,
`patterns: unify pattern outputs into one RelationCandidate type`)
— predicates flow through a lexicon, not free text.

#### (b) PMI / repetition gating for novel single-token attribute names

Even single-token novel attributes deserve some confidence weight.
Per [[no-hardcoded-language-wordlists]]: don't blocklist by
identity, gate by statistical signal. The signal that makes a
token a legitimate predicate (vs noise):

- **Repetition.** A token that's appeared in N+ extracted
  relations across different sessions is more predicate-like.
- **PMI with subject/object.** If `using` co-occurs with content
  words in a way that's not explained by base rate, it's a
  predicate-shaped token.

v0 likely doesn't have enough corpus to measure PMI well. Two
options:

- **Conservative v0:** novel single-token attributes mint with
  confidence floor (e.g. 0.15); they accumulate via `support_count`
  with each re-extraction. Cache promotion to higher confidence
  uses standard Step 9/10 logic.
- **Hold for v0.1:** add the PMI statistics now (cheap to compute,
  cheap to store) but don't gate on them until v0.1.

#### (c) Span chunking around contractions

`"language we're"` becoming a single phrase candidate is a chunker
artifact. The orthographic chunker (see `src/steps/extract_chunks.rs`
or wherever the "Phrase Polaris is in Rust" output line came
from — verify on read) treats "we're" as one token but doesn't
prevent it from joining "language" in a phrase boundary.

Fix: split phrase boundaries at apostrophe-containing tokens
unless the apostrophe is in a known multi-word lexeme
(`it's-a-trap` style). This is a small chunking rule; doesn't
need its own design doc.

### Citations

- Phase 2 typed-relation lexicon work: commit `1b57274`
  (`patterns: unify pattern outputs into one RelationCandidate type`).
- SVO extractor: `src/steps/relation_patterns/svo.rs`. Read fully
  before changing — `subj_pronoun_end` handling at line 152 has
  precedent for "treat synthesized pronouns differently."
- `resolve_attribute_name`: `src/steps/build_relations.rs:359`.
  Note the warning counter (`attr_names_minted` ←
  `policy.attribute_name_mint_warning_count`) — the system
  already tracks this metric, just doesn't gate on it.
- `new_foundation.md` §11.4 (token budget) and the typed-relation
  lexicon section — confirm the "predicates flow through a
  lexicon" framing is the intended design.

### Open questions

1. **What's the right confidence floor for novel attributes?**
   Today `policy.default_conf` after intent adjustment lands
   around 0.2–0.5. The bad relation came out at 0.25 (just under
   the assertion threshold). A novel-attribute relation maybe
   wants confidence pinned at `min(default_conf, 0.15)` so it's
   always Defeasible.
2. **Should rejected candidates surface as uncertainty signals?**
   When `is_admissible_attribute_name` rejects, the system
   silently drops a candidate the user actually said. Maybe push
   `UncertaintySignal::LowConfidence` so the frame surfaces
   "something was said we couldn't parse."
3. **How aggressive on multi-token slot names long-term?** The
   lexicon will accumulate `derived_from`, `from_when`, etc. Is
   there a v0.1 mechanism where a multi-token candidate gets
   *promoted* to the lexicon based on repetition?
4. **What about the chunker boundary issue?** Worth a separate
   pass-through pull request, or fold into the same change set?

### Output

Either retire this section into a permanent
`docs/extraction-attribute-name-quality.md` doc, or keep it as
working notes until the work lands. Permanent doc preferred
because the lexicon strategy is going to be referenced again.

---

## Item 5: query-shaped retrieval — design proposal

**Scope:** large. New retrieval logic in Step 12 (or a new step).
**Design-first per convention.**

### Observed failure

Tick 2 input: `"what language are we using?"`. The frame surfaces:

- `R611 Rust → instance_of → language` (in `current_state`,
  because "language" was a referenced element)
- A pile of seed scaffolding (mitigated by item 2)
- `R617` (the bad extraction from item 1)

None of these is *answer-shaped* — there's no relation explicitly
saying "the language we are using is Rust." The substrate has the
answer scattered across two relations (`R611 Rust → instance_of →
language` and the missing-or-malformed `we → uses → Rust`); a
downstream LLM would have to chain them to answer.

### The principled framing

The system today treats every input the same way: extract entities
and relations, walk activation, surface what's been most recently
or strongly engaged. That works for statements ("X is Y") because
the extraction *is* the answer.

It doesn't work for queries ("what X is Y?"). A query needs a
different operation: **pattern-match the current substrate against
the question shape, and surface the matching tuples**.

This is well-known graph-DB territory (SPARQL, Cypher) — query is
not retrieval-by-activation, query is constraint satisfaction.
Legend's substrate is already a knowledge graph; the query path is
just the missing operation.

### Where to land it

Three architectural options:

#### (i) Promote-in-Step-12

In `src/steps/frame.rs::assemble_frame`, when intent is
query-shaped (curiosity high, conviction low), run a query-match
pass over the substrate and *promote* matching relations to the
top of `focused_relations`.

- **Pros:** Smallest delta. Same frame shape; same code paths.
- **Cons:** Query-matches don't have an activation score natural
  to the RRF inputs. We'd shoehorn them in.

#### (ii) New frame field

Add `query_answers: Vec<ResolvedRelation>` to
`ConsciousAttentionFrame`. Step 12 populates it when intent is
query-shaped; empty otherwise.

- **Pros:** Honest semantic separation. Consumers know "this is
  what answers your question" vs "this is what's currently in
  focus."
- **Cons:** Grows the frame contract. Aligns with the
  [[legend-as-function]] principle (output goes on the frame) so
  the cost is one-time.

#### (iii) New step

A query-retrieval step that runs only when intent is query-shaped,
populating Step 12's input. Cleaner separation; bigger pipeline
diff.

**Lean (ii).** Honest field, frame-as-output principle holds, and
the Step 12 implementation is simple (call a query-matcher,
attach results).

### The query-match algorithm (v0 sketch)

For a query like "what language are we using?":

1. **Extract the query shape.** Parse the question into a
   pattern. v0 minimal parse:
   - Question word: `what` (the unknown slot)
   - Anchor noun: `language` (the type of the answer)
   - Verb phrase: `using` (the predicate)
   - Subject: `we` (the entity asked about)
   - Pattern: `(?value of type language, used by we)`
   - Or more abstractly: `(?x, instance_of, language) ∧ (we, ?, ?x)`

2. **Match the substrate.** Index lookup against:
   - `relations_by_attribute_name["instance_of"]` filtered to
     `value = language_element` → candidate ?x values.
   - For each candidate ?x: walk `relations_by_element[?x]` for
     relations where `we` (or its coref) is the subject.

3. **Surface matching tuples.** The matched relations land in
   `query_answers`.

For v0 this is mostly *triple-pattern matching* with one variable
binding step. Not full SPARQL. The query-parse is the hard part.

### Query parsing — the hard part

v0 doesn't have a query parser. Options:

#### (a) Tiny rule-based query templates

Detect wh-question shape ("what X are Y Z-ing?", "what is X's Y?",
"where is X?") with a small set of regex / token-pattern templates.
Each template extracts the pattern variables.

- **Pros:** Ships fast. Works for the common cases.
- **Cons:** Brittle. Misses paraphrases ("what's the language" vs
  "which language" vs "tell me the language").

#### (b) Embedding-based pattern matching

For each query, embed it and find the nearest neighbor in a small
set of canonical query forms with known patterns. Reuses the
existing MiniLM embedding.

- **Pros:** More robust than (a). Scales to new query types via
  the canonical set.
- **Cons:** Needs a curated canonical set. Embedding-based
  pattern matching for structured extraction is a known-fragile
  area (similar to the intent classifier work).

#### (c) Defer to extraction

Treat queries as a special case of extraction: run the same
relation extractor (Step 5), interpret the slot it didn't fill as
the query variable. So `"what language are we using?"` extracts
`(we, using, ?language_class)` — the unfilled slot is the
question.

- **Pros:** Reuses existing machinery. No new parser.
- **Cons:** Requires the extractor to recognize question shape
  (so it doesn't try to mint "what" as an element). And requires
  it to emit an "unfilled slot" marker, which is new.

**Lean (c) for v0, (b) as the v0.1 path.** (c) lets us ship a
working query path quickly by extending the existing extractor;
(b) is the structural upgrade once we have evidence about which
query shapes matter.

### Citations

- `Intent.curiosity` semantics:
  `new_foundation_v0_core.md:545` ("retrieval-shape vs assertion-shape").
- `adjust_policy.rs:34` uses curiosity to soften writes — there's
  already precedent for "treat queries differently," just on the
  write side.
- `new_foundation_v0_core.md:949` — search for "query" /
  "retrieval" in the foundation docs to confirm the v0 stance.
- `relations_by_attribute_name` and `relations_by_element` indices
  in `src/types.rs::Hypergraph` already give us the substrate
  lookups query-match needs.
- SPARQL / Cypher prior art: not necessary to cite, but the
  triple-pattern-matching framing is the standard graph-DB
  approach. Worth one sentence in the durable doc.

### Open questions

1. **Where does the unfilled-slot marker live?** If we go with
   (c), the extractor needs to emit "this slot is the question."
   New `Term` variant? Or a side-channel from Step 5 to Step 12?
2. **What about queries without a clean triple shape?** E.g.
   "tell me about Polaris" — this isn't a triple-pattern match,
   it's an entity-centric retrieval. Should query_answers handle
   both, or is there a separate `entity_dossier` field?
3. **Does `current_state` change?** Today `current_state` is
   "what's true about referenced entities." A query references
   *concepts* (language, we), not entities-in-the-usual-sense.
   The field may not need to change — query_answers is the
   query-specific bucket, current_state stays.
4. **What's the relationship to the active_frame fix (item 3)?**
   If item 3 sets active_frame to None on queries, item 5's
   pattern-matcher uses the query parse instead. Clean
   composition.
5. **Coreference for "we" / "us" / "I."** Step 6 already
   resolves pronouns to recent_focus elements; the query
   pattern-matcher can reuse that.

### Output

Promote to `docs/query-shaped-retrieval.md` once signed off.

---

## Suggested order

1. **Item 4** — clock fix. Today. Unlocks recency gating in item 3.
2. **Item 2** — seed anchor filter. Today or next session. Big
   quality win on every tick output.
3. **Item 3** — active_frame staleness. Depends on item 4. Capture
   N (tick distance threshold) and intent-gate thresholds in commit
   message.
4. **Item 1 design doc** — review, sign off, then code. May surface
   followups; budget time for that.
5. **Item 5 design doc** — review, sign off, then code. Best done
   after item 1 lands (better extraction makes the retrieval
   tractable), but the doc can be reviewed in parallel.

## Out of scope

- Reworking the seed graph itself.
- `is_frame_unsuitable` cleanup (hardcoded pronoun/wh-word list at
  `src/steps/hebbian.rs:443`) — flagged but deferred.
- Adding any output channel that isn't on the frame
  ([[legend-as-function]]).
- Coreference improvements beyond what Step 6 already does.
