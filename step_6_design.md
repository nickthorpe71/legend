# Step 6 — Coreference Scoring

> **Status: design pass.** Spec source: `tick_pipeline_focus.md §11.8`.
> Step 10's `recent_focus` push (shipped) is the unblocker — coref
> was a stub for the whole pipeline until now because there were no
> candidates to score against.

## 1. What Step 6 is

Pure-Rust scorer — no model. For each **ambiguous span** in the
input (pronoun or definite description), pick a best-scoring
antecedent from the working-memory candidate set. Output a
`CorefDecision` that Step 8's existing override path (§4a, already
shipped) uses to bind the span to the antecedent element instead of
minting a fresh one.

Identity is **conservative** — pronouns/definites get bound only
when the score clears a threshold; below threshold, no decision is
emitted and the span gets minted as a fresh element.

## 2. Inputs

```rust
pub fn resolve_coref(
    input_text: &str,
    hg: &Hypergraph,
    ner_spans: &[LabeledSpan],   // from Step 5b — entities for non-pronoun spans
    policy: &Policy,
) -> Vec<CorefDecision>;
```

The current stub takes only `(input_text, &hg)`. Adding `ner_spans`
and `policy` so we can:
- skip spans that NER already labeled as a fresh entity (those
  don't need coref — they're new identities);
- read `policy.coref_threshold` (new — see §6) to gate decisions.

## 3. Ambiguous-span detection

Two kinds for v0:

1. **Pronouns** — closed-class list. `he | she | it | they | this |
   that | him | her | them | his | hers | its | their`. Detection
   is straightforward whole-word matching on `input_text` (the
   Step 5a chunker already enumerates content tokens; we can walk
   it or just regex once).

2. **Definite descriptions** — `the X` where `X` is a token that
   resolves via `by_name` to a Signal element. Detection is
   "find `the `, take the next word(s), check if the head noun is
   in `by_name`."

Anything NER tagged as a fresh entity span is excluded from both
sets — it's a new identity, not a reference.

## 4. Candidate set construction

Per §11.8:

```text
candidates =
    recent_focus (most recent first, deduped by element)
  ∪ region_members[R] for R in active_regions   // currently empty in v0
```

`region_members[R]` doesn't exist as an index today — there's
`region_prototypes[R]` but not a generic member list. For v0,
**candidate set = `recent_focus` only**. Once Step 4 builds a
proper region-membership index, the union grows automatically.

Deduplicate by element so two `recent_focus` entries pointing at
the same element (e.g., subject + target bound) collapse to one
candidate carrying both attribute slots.

## 5. Scoring

Per §11.8 the score is a six-term sum minus two penalties. v0
implements the four terms whose inputs are already on the
Hypergraph today; defers the two that need pieces not yet built.

### 5.1 v0 terms

**name_overlap (0..1)** — for definite descriptions, edit-distance
or lemma-match between the description's head noun and any of the
candidate's `names`. For pronouns this term is **0** (pronouns
don't carry surface form). Implementation: lowercase exact match
on any of `candidate.names` → 1.0; substring/contains → 0.5; else
0.

**embedding_similarity (0..1)** — cosine between
`embed_span_in_context(input, span_start, span_end)` and
`candidate.embedding`. Reuses `crate::math::dot` (already a pure
function over unit vectors).

**attribute_overlap (0..1)** — 1.0 if the `recent_focus` entry's
`attribute` slot matches the span's grammatical slot, else 0. The
grammatical slot of a pronoun in the input is hard to determine
without parsing; for v0, **use the most recently-pushed `attribute`
that matches** as a heuristic. The strong differentiation comes
when we have a real syntactic role for the pronoun — that's a
post-v0 hook. v0 boost is +0.5 when ANY `recent_focus` attribute
exists for the candidate (rewards "this element was focal in some
role"), 0 otherwise.

**recency_bonus (0..1)** — NOT in the §11.8 formula explicitly,
but the entries are in recency order in `recent_focus`. Compute as
`1.0 / (1.0 + ticks_ago)` where `ticks_ago = hg.clock.0 -
entry.tick.0`. Stays bounded; same-tick entries score 1.0.

### 5.2 Deferred terms

- **frame_overlap** — needs `active_frame` threaded through and a
  notion of "adjacent frame." `active_frame` is `None` everywhere
  today (Step 4 frame inheritance hasn't landed). Deferral cost:
  zero for now (the term contributes 0).

- **temporal_compatibility** — needs valid-time tracking on
  relations. Not in v0 substrate.

- **relation_support** — needs Step 8/9 history of the candidate's
  outgoing relations. Cheap-ish via `relations_by_element[cand]`
  but moot until we test coref on multi-tick scenarios where
  candidate neighborhoods matter. Defer.

- **contradiction_penalty** — needs the candidate's `Superseded`
  relations checked for re-fire compatibility. Defer.

- **distinct_instance_penalty** — needs `separate_pattern` from
  §14.3. Defer.

### 5.3 Aggregate

```text
v0 score(span, candidate) =
    name_overlap(span_text, candidate.names)        // 0..1
  + embedding_similarity(span_emb, cand_emb)        // 0..1
  + attribute_overlap_hint(candidate)               // 0 or 0.5
  + recency_bonus(entry.tick, hg.clock)             // 0..1
```

Max possible v0 score ≈ 3.5; threshold should be tuned. Initial
`policy.coref_threshold = 1.0` — must clear at least the recency
bonus plus one substantive signal. Conservative by design (false
positives are worse than false negatives: a missed coref mints a
provisional element that replay can merge; a wrong coref binds two
distinct entities together, which is hard to unwind).

## 6. Policy field

Add to `Policy`:

```rust
/// Minimum aggregate score for a coref decision to fire. Below
/// this, the span mints fresh. Tuned conservatively to favor
/// fresh-mint over wrong-bind (replay can merge later).
pub coref_threshold: f32,    // default 1.0
```

## 7. Decision output

`CorefDecision` already exists from Step 8's coref-override work:

```rust
pub struct CorefDecision {
    pub pronoun_text: String,
    pub pronoun_char_start: usize,
    pub pronoun_char_end: usize,
    pub antecedent_text: String,
    pub confidence: f32,
}
```

`antecedent_text` is the antecedent element's first `name`. Step 8's
`apply_coref_decisions` already looks this up via `by_name` —
matches one-to-one. `confidence` = `normalized_score / max_score`.

## 8. Phased rollout

### Phase 1 — Ambiguous-span detection

- `detect_ambiguous_spans(input_text, ner_spans, hg) -> Vec<AmbiguousSpan>`.
  Returns `(span_text, char_start, char_end, kind: Pronoun | DefiniteDescription)`.
- Skip spans NER already tagged as fresh entities.
- Unit tests: each pronoun, each definite-description shape,
  NER-overlap exclusion.

### Phase 2 — Candidate set + scoring helpers

- `collect_candidates(hg) -> Vec<Candidate>` — dedup recent_focus
  by element; carry forward attribute set per candidate.
- `score_candidate(span, candidate, hg, policy) -> f32` —
  implement the four v0 terms.
- Unit tests for each scoring component in isolation.

### Phase 3 — Decision aggregation + threshold gate

- `resolve_coref` body: for each ambiguous span, score all
  candidates, pick the max-scoring one, emit `CorefDecision` if
  score ≥ `policy.coref_threshold`.
- Add `coref_threshold` to `Policy::default()` = 1.0.

### Phase 4 — Integration into Step 5

- `run_extractors` already calls `resolve_coref` (stub). Update
  the call site to pass `ner_spans` and `policy`. The output
  pipeline through Step 8's `apply_coref_decisions` is already
  wired.

### Phase 5 — End-to-end test

- Two-tick scenario: tick 1 mints "Sarah called me."; tick 2 says
  "She emailed me too." — Step 6 should bind "She" to Sarah.
- Negative case: tick 2 with no compatible antecedent should not
  emit a decision.

## 9. Open questions

### Q1. How do we determine a pronoun's grammatical slot?

The full §11.8 attribute_overlap term needs to know if "it" sits
in a subject or object position so we can prefer focus entries
whose `attribute` matches. v0 punts: use the attribute_overlap
hint (+0.5 if any focal binding exists) instead of the precise
filter.

**Recommendation:** ship the hint for v0; add real syntactic-role
detection in v1 when we have a dependency parser (or when the
relation_patterns module is rich enough to expose object-position
hints).

### Q2. Definite descriptions — head noun only or full NP?

"the appointment" → head noun "appointment". "the dentist
appointment" → head noun "appointment" too? Both should match
the `appointment` Element. v0 takes the last token of the NP
(rightmost head, English convention).

### Q3. Threshold — is 1.0 right?

Empirically tunable. 1.0 means a recency bonus alone
(no name overlap, no embedding similarity above 0.5) doesn't fire
a decision. This favors precision; replay merges later if
warranted. Could revisit after running on broader inputs.

## 10. Out of scope (post-v0)

- **Centering-theory-style salience ordering** beyond pure
  recency.
- **Cross-tick coref state** (e.g., chains of pronouns across
  ticks) — `recent_focus` already buys us most of this with
  its capacity bound; explicit chain tracking is a v1 thing.
- **Plural pronouns binding to multiple antecedents** —
  "they" → multiple people from prior ticks. v0 treats every
  pronoun as a single-antecedent reference.
- **Quoted-speech coref** — "she said 'I'll be there'" — needs
  speaker-frame tracking.
