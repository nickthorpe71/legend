# New Foundation

Status: living architecture spec for Legend v2 — a learnable hypergraph with a
vector subgraph, a single-verb tick API, and a from-scratch ultra-minimal Rust
implementation.

This document is the substrate spec. A solo developer should be able to read it
top to bottom and start coding against it without consulting prior versions of
Legend.

---

## 0. Reading Guide

- §1 sets the goal.
- §2 is the **walk-through**: what Legend is, the components in order,
  and what `tick` actually does. Read this first; it's the shape.
- §3 is the hard invariants — non-negotiables.
- §4 inventories every kind of atom in the hypergraph. **§4.0 names the
  mathematical foundations the substrate rests on** (typed hypergraph,
  predicates with named role-fillers, bitemporal time, algebraic
  provenance, weighted formulas). Read §4.0 once; later sections
  reference it by number.
- §5 is the new **Core Data Model** section: the substrate spec the coder
  works against first.
- §6 is the new **Carry-Forward From Current Legend** section: which concepts
  (not code) we keep, and what we explicitly drop.
- §7 specifies the seed layer as concrete data, not prose.
- §8 specifies the semantic-region hierarchy and routing.
- §9 is the tick pipeline — the only operation Legend has.
- §10 explains why there is no separate query path.
- §11 reframes brain processes as pure functions over the hypergraph.
- §12 is the model stack (Rust-only, pragmatic-reuse).
- §13 specifies the algorithms (insertion, coreference, reinforcement, decay,
  replay).
- §14 is the worked ten-tick conformance walkthrough.
- §15 is evaluation.
- §16 is the build order — written for a solo coder with Claude as reviewer.
- §17 is the source map.
- §18 is genuinely deferred questions.

---

## 1. Goal

**Legend is a memory engine, not a chatbot.** It maintains a persistent,
structured understanding of information over time and exposes that
understanding through an interpretable activation trace.

### 1.1 The Functional Signature

Legend is, formally, a single function:

```text
Legend(G, x) -> (G', A)
```

- **G** = current memory (a hypergraph)
- **x** = input (raw text + source + wall-clock)
- **G'** = updated memory
- **A** = attention frame (a `ConsciousAttentionState`)

This is the entire public surface. There is no separate query path, no
separate write path, no read/update duality. Every input is the same
operation. Some inputs happen to be question-shaped in which case
`A` will contain the answer; some are statement-shaped and produce
more durable writes in `G'`; the distinction is emergent in the output,
not an API choice.

In the substrate's Rust implementation, this function is encoded as
`tick(&mut Hypergraph, Input) -> ConsciousAttentionState`. The `&mut` is
the operational form of `G -> G'` — same semantics, in-place mutation
for performance.

### 1.2 What The Substrate Is For

Legend turns experience into a tiny, fast model of the world. The target
is a small general substrate that learns concepts, relations, instances,
temporal state, semantic regions, and usefulness over time, by
processing a single stream of ticks. The brain analogy is load-bearing:
human memory is reconstructive and lossy by design, and that is the
feature. Legend aims to be a more stable, less drift-prone version of
the same idea.

Ideal behavior:

- **Distill, don't transcribe.** Strip surface form. Keep what an
  answer would reach for; drop the rest. No raw text, no normalized
  forms, no duplicated bytes. (The full unbounded log retains raw
  inputs for development builds only — §5.7.)
- Preserve every **answer-bearing asserted/base fact**. Entailed facts
  are not materialized eagerly (§13.5); the rules that derive them are.
- Every claim points at a `SourceId` so users can re-read the original
  source if it still exists; Legend itself does not store the source
  bytes.
- Learn concepts and relation patterns instead of hard-coding semantic
  strings.
- Organize semantic space with a vector hierarchy embedded in the same
  graph.
- Run brain-like processes over the same substrate: decay, reinforcement,
  Hebbian learning, prediction error, replay, consolidation.
- Return a current state of conscious attention from every tick.
- **Tiny footprint, sub-100 ms ticks** — see Invariant 16.

### 1.3 The Attention Frame Is The Output

`A` (the `ConsciousAttentionState`, §9.13) is the core return value —
not an answer. It is a structured snapshot of: which atoms, regions,
archetypes, and frames activated; which claims are in focus, with
per-claim provenance for *why* (`LensSource` in §5.6); relevant
superseded history; structural uncertainty signals; what this tick
durably wrote and what it superseded; next-action hints for replay.

When the input is question-shaped, an `answer` surfaces from the
focused claims as a byproduct of the frame, not its goal. Legend's
substrate is consumer-agnostic — an LLM is the common reader (converts
user input → `Input`, calls `tick`, renders the frame as natural
language), but any consumer of structured state works (CLI inspectors,
agents, dashboards, programmatic tools).

**Legend does not return answers; it returns the information needed
to produce answers.**

---

## 2. How Legend Works

This section is a walk-through of *what Legend is* before
diving into the spec.

A **tick** is one call into Legend: one input (some string), one updated hypergraph,
one attention frame returned. Legend has no separate query path or
write path — every interaction (user message, file event, agent
observation) becomes an `Input`, gets handed to a single function, and
produces a structured snapshot of what's now in focus. The term
"tick" appears throughout this walk-through; §2.3 formalizes the
function signature, §2.4 enumerates the sub-functions inside each
tick, and §9 spells out every step. For now: when you read "a tick
arrives," picture one input flowing through Legend top-to-bottom and
changing the hypergraph by exactly the amount that input warrants.

### 2.1 The Substrate, a Hypergraph

Everything Legend remembers lives in **one physical hypergraph**. There
is no separate vector database, no separate graph database, no concept
graph synchronized to an instance graph. The vector hierarchy is a
subgraph *inside* the hypergraph. One structure, queried through
different lenses.

The hypergraph is built from **atoms** and **claims**, both first-class
memory citizens — both decay, reinforce, accumulate stats, and
contribute to attention.

An **atom** is a node. Its mechanical
`kind` says which: an atom can be a learned predictor of structure
(an archetype), a semantic region in vector space, a specific instance
in context, an event marking change, a value, a role slot, a frame, a
schema, or a rule. Same shape, different kind; kind-specific detail
lives in dedicated payload tables (§5.2). An atom is **indivisible**
from the rest of the substrate's perspective — claims reference atoms
by ID and never decompose them (§4.1).

A **claim** is a hyperedge. It binds N atoms into a single statement:
predicate + role bindings + qualifiers + status + supersession links.
Triples, n-ary events, nested conditionals — all one structure. Claims
have their own ID space (`ClaimId`) and their own stats; they are not
atoms, but they participate in the same memory dynamics.

Together: a graph of richly typed atoms, hyperedge-bound by claims,
with both sides alive — both able to decay, both able to be
reinforced, both routed by attention.

### 2.2 The Components, In Order Of When You Meet Them

Walk an input through Legend, and you encounter each component in turn.

**No input record.** When a tick arrives, Legend embeds the input,
routes it, extracts atoms and claims — and stores the *result*, not
the input. There is no per-input audit record. Memory is the
distilled pattern; the brain analogy is load-bearing here. Where a
source pointer matters (a user-cited URL, a file path, a chat message
id), it lives on the relevant claim's `Qualifiers.source`; otherwise
the field is `None` and the input was never going to be re-readable
anyway. (§4.2)

**Semantic Regions.** Embeddings don't sit in a flat vector space —
they're routed through a **weighted DAG of regions**. Each region holds
1–8 prototype vectors (the centroids of what's been routed to it).
Walking down from the root (`Genesis`) by best-matching prototype
identifies which regions an embedding "lives in." Multiple parents are
allowed; this is what makes the structure a DAG rather than a tree, and
it's how Legend handles polysemy. Inputs that match nothing fall to a
`Void` sink. (§4.9, §8)

**Archetypes.** An archetype is Legend's **predictor of structure** —
a small atom that has learned a canonical *shape* an input can take.
Each archetype has prototype vectors (when does it fire?), expected
slots (what fillers it predicts — `target`, `from`, `to`, etc.), a
slot-transition matrix (what fills next given what just filled), and a
small **structural vocabulary** of role atoms and claim templates it
endorses (the canonical patterns it predicts when it activates).
Archetypes activate in parallel against incoming embeddings, predict
which slots extractors should look for, and contribute their
vocabulary's strength to `focused_claims` aggregation. Endorsements
compound when archetypes agree, and surface as uncertainty when they
disagree. The seed pack ships ~30 *domain-neutral* archetypes
(`change_event`, `entity_mention`, `temporal_expression`, …);
domain-specific archetypes (`appointment_event`, `function_definition`)
emerge from replay over time. (§4.3)

**Instances.** Specific things in context: this appointment, this
person, this file. Instances are reused only with coreference evidence
— if there's any doubt, Legend creates a separate provisional instance
and lets replay merge later. Pattern separation in, premature
collapsing out. (§4.4)

**Events.** First-class atoms representing *change*: a reschedule, a
correction, a state transition. Events are the ground truth for "what
changed when"; current-state cache claims always point back to the
event that produced them via `derived_from`. This is **Event Calculus**
(Kowalski & Sergot 1986) made structural. (§4.5, §13.4)

**Claims.** The hyperedges. Every fact Legend knows is a claim:
predicate + role bindings + qualifiers + status (`Asserted`,
`Entailed`, `Defeasible`, `Superseded`, `Retracted`) + supersession
chain + its own `MemoryStats`. Claims decay, reinforce, and accumulate
access stats just like atoms. A correction doesn't delete the old
claim — it *supersedes* it (PROV-O `wasRevisionOf`), so the history
walks both directions. (§4.6)

**Values.** Typed regions in value spaces — text, numbers with units,
time points and intervals, locations, booleans, probabilities, vectors.
"Tuesday" is a weekday-concept value; `2026-04-28` is a grounded
time-point. The relation between them is itself a claim. (§4.7)

**Qualifiers.** Every claim is scoped: a `frame` (context), a
`time_scope` (valid time, separate from when Legend learned it), a
source, a polarity, a modality (`Actual`, `Possible`, `Desired`,
`Obligatory`, `Hypothetical`, `Counterfactual`), a rank
(`Preferred` / `Normal` / `Deprecated` — Wikidata-style), and an
optional `condition` for "X if Y." Modality and condition are how
Legend handles uncertainty and conditionality without splitting into
separate stores. (§4.8)

**Schemas, Roles, Frames, Rules.** Supporting atoms. Schemas are
extraction templates; roles are the named slots of a claim
(`target`, `from`, `to`); frames scope claims to a context
(`user_schedule`, `codebase_legend`); rules are the inference templates
that derive entailed claims from base claims.

**Working Memory.** A `VecDeque` of the most recently focused atoms
(~64). This is what coreference resolves "it" against, and what Hebbian
co-activation strengthens edges between. Not a store — a bounded
attention window. (§5.2)

### 2.3 The Function Is `Legend(G, x) → (G', A)`

Legend's entire public surface is one function:

```
Legend(G, x) → (G', A)
```

- `G` = the hypergraph before this tick
- `x` = the input (text + source pointer + wall-clock)
- `G'` = the hypergraph after
- `A` = the **attention frame** — a structured snapshot of what fired,
  what was in focus, what changed, what's uncertain, what to replay
  next

In Rust this is `tick(&mut Hypergraph, Input) -> ConsciousAttentionState`.
The `&mut` is the operational form of `G → G'`.

There is no separate query path. Question-shaped inputs and
statement-shaped inputs go through the same pipeline; the *output*
differentiates. A question populates `A` from claims in focus;
a statement produces durable writes in `G'`. The distinction is
emergent in the result, not an API choice.

### 2.4 What `tick` Actually Does — The Sub-Functions

A tick is ~15 sequential steps, each one a pure function over the
hypergraph (or `&mut` for the few that mutate). Pseudocode shape:

```
fn tick(hg: &mut Hypergraph, input: Input) -> ConsciousAttentionState {
    // --- Read-mostly parallel phase ---
    let intent  = detect_intent(&input, hg);                 // Step 1
    let policy  = adjust_policy(&intent, &hg.policy);        // Step 2
    let units   = segment(&input);                           // Step 3
    let embeds  = embed(&units);                             // Step 4
    let (active_regions, region_delta)
                = route_regions(&embeds, hg, &policy);       // Step 5a
    let active_cols
                = activate_archetypes(&embeds, hg, &policy);    // Step 5b
    let preds   = predict_next(&active_cols, &[], hg);

    // --- Mutation phase ---
    apply_region_delta(hg, region_delta);
    let extractions
                = run_extractors(&units, &preds, &policy);   // Step 6
    let coref   = score_coreference(&extractions, hg);       // Step 7
    let (claims, events)
                = build_claims(&extractions, &coref, hg);    // Step 8
    apply_supersession_and_cache(hg, &claims, &events);      // Steps 9–10
    update_archetypes(&active_cols, &outcomes, hg, &policy);    // Step 11
    reinforce_path(hg, &focused_path);
    decay_step(hg, &policy);                                 // Step 12
    let attn = aggregate_focus(&claims, &active_cols,
                               &policy);                     // Step 13
    enqueue_replay(hg, &attn);                               // Step 14
    attn
}
```

Each named function is one of Legend's brain processes:

- `detect_intent` — PFC. Classifies the input shape (statement,
  question, correction, identity, temporal update, brainstorming).
- `adjust_policy` — PFC. Sets per-tick thresholds and modulators
  (vigilance, plasticity, salience floor) from intent.
- `segment` + `embed` — entorhinal. Split input into sentences/clauses,
  run BGE-small, L2-normalize.
- `route_regions` — thalamus, read-only. Walks the region DAG; emits
  the active regions and a `RegionDelta` of structural changes pending.
  Parallel-safe across embeddings.
- `apply_region_delta` — thalamus, mutation. Commits prototype updates,
  region creation, void attachments.
- `activate_archetypes` — neocortex, read-only. Scores every
  archetype's prototypes against active embeddings; returns the
  archetypes above threshold. Embarrassingly parallel.
- `predict_next` — neocortex, read-only. Per active archetype, ranks
  expected slots by `fill_probability * activation`; emits
  `SlotPrediction`s that bias extractors.
- `run_extractors` — wernicke. NER, temporal parsing, relation
  extraction. Biased by archetype slot predictions but never
  overridden by them.
- `score_coreference` — hippocampus + dentate gyrus. Resolves "it"
  against working memory; pattern-separates close-but-distinct
  instances.
- `build_claims` — composes extractor output + coref into hyperedges
  and event atoms.
- `apply_supersession_and_cache` — Event Calculus update. New events
  initiate fluents (cache claims with `derived_from`); previous
  current-state claims get marked `Superseded` with `superseded_by`
  pointers.
- `update_archetypes` — neocortex, mutation. Bounded Hebbian on
  expected slots, slot transitions, and vocabulary entry strengths;
  plasticity decays with `support_count` (mature archetypes harden).
- `reinforce_path` — basal ganglia. If the tick produced an answer,
  strengthen the *exact* path that found it (region → claim → instance
  → frame), not nearby vectors.
- `decay_step` — basal ganglia. Utility-based decay on access paths;
  v0 never deletes atoms or claims, only weakens them.
- `aggregate_focus` — assembles `ConsciousAttentionState`. Combines
  extractor proposals with archetype vocabulary endorsements using
  reciprocal-rank fusion (a standard way to merge multiple ranked
  lists into one) into `focused_claims`; surfaces disagreement as
  `UncertaintySignal` rather than collapsing.
- `enqueue_replay` — hands a snapshot to the replay thread, which runs
  offline (region split/merge, archetype emergence, vocabulary
  pruning, coref resolution, cache pruning) and returns mutations to
  apply later.

That's the whole engine. Every sub-function is pure or `&mut`, none
own state of their own, and every brain region is a function — not a
module — over the same hypergraph. (Full pipeline in §9; brain-process
signatures in §11; algorithms in §13.)

### 2.5 Durability — The Hypergraph On Disk

The hypergraph is the system of record. Alongside it Legend keeps a
**bounded write-ahead log** (WAL): a small queue of recent inputs (raw
text + wall-clock + model fingerprint), capped at 10 MB, segmented (~1
MB per segment, LZ4 hot, zstd-19 closed), oldest segment dropped when
the cap is exceeded. The WAL exists for two purposes only: (a) crash
recovery between checkpoints, (b) bug reproduction during development.

Development builds may set `LEGEND_WAL_UNBOUNDED=1` to retain every
segment for as long as wanted; production builds enforce the cap. The
WAL is **not** an event store, **not** the system of record, and
**not** consulted on the hot path. Boot path: `load latest snapshot,
replay WAL suffix on top`. Crash recovery: same.

The embedder is **pinned for Legend's lifetime** — see §12.1.
`ModelFingerprint` is a boot-time consistency check that refuses to
start against the wrong model. Because Legend does not retain raw text
or input records (§4.2 — the brain analogy: distill, don't transcribe),
a model swap is a deliberate one-way door: re-ingest from any
`Qualifiers.source` pointers still reachable, accept loss where not.
Treat the pin commitment as hard.

The bitemporal split holds regardless: transaction time =
`created_at: Tick`; valid time = `Qualifiers.TimeScope`. Full
durability lifecycle in §5.7.

### 2.6 Recap

- One hypergraph. Atoms (richly-typed nodes — archetypes, regions,
  instances, events, values, roles, frames, schemas, rules) bound by
  claims (hyperedges). Both decay and reinforce; both first-class
  memory citizens.
- Vector hierarchy is a region DAG inside the same hypergraph.
- Archetypes are predictors of structure: prototype + expected slots +
  slot transitions + a small vocabulary of role atoms and claim
  templates they endorse. They activate in parallel, predict slot
  fills, and contribute their vocabulary's strength to focused claims.
- Events are first-class; corrections supersede via PROV-O chains
  rather than overwriting.
- Every input is one call to `tick`, which threads through ~15 pure
  sub-functions and returns an attention frame.
- Brain regions are functions, not modules. None own state.
- Snapshot + bounded WAL; pinned embedder; no transcript storage.

The rest of the doc — Hard Invariants, the deep type spec, the
algorithms, the worked walkthrough, the build order — fills in this
shape.

---

## 3. Hard Invariants

1. **The hypergraph is the substrate and the system of record.** A bounded
   write-ahead log (10 MB cap, queue-style, oldest dropped) sits alongside
   it for crash recovery between checkpoints; it is not an event store
   and is not read on the hot path. Boot = snapshot + replay WAL suffix.
   Development builds may retain a full unbounded log for debugging;
   production builds enforce the cap.
2. **No duplication, no fluff.** Every byte in the hypergraph earns its
   place. Legend distills what matters from inputs and drops the rest
   (raw text, normalized text, span offsets, derivable forms). The goal
   is a minimal hypergraph that contains all useful information from
   sources, not a transcript with a graph index. This is the brain
   analogy taken seriously: human memory is reconstructive and lossy by
   design; that is the feature.
3. Learned abstractions must point back to other atoms or claims (via
   `derived_from`, supersession, or extractor lineage). Nothing is born
   without ancestry inside the hypergraph.
4. Semantic strings are atom names; they do not drive control flow.
5. Control flow branches only on mechanical roles (`AtomKind`, `ClaimStatus`,
   `Polarity`, `Modality`, `ValueKind`) and learned affordances.
6. Compression must be answer-preserving.
7. **Bitemporal split.** `Tick` is **transaction time** (when Legend learned
   this). **Valid time** (when this was true in the world) lives on
   `Qualifiers.TimeScope`. Industry standard (Datomic, XTDB, Wikidata,
   SQL:2011, Graphiti). Do not conflate.
8. **`ModelFingerprint` is a boot-time consistency check.** Every snapshot
   and WAL segment carries a fingerprint (embedding-model hash, tokenizer
   vocab hash, extractor versions, code version). On boot the fingerprint
   must match the running binary's pinned model exactly; if not, Legend
   refuses to start and points at the offline embedding-migration program.
   Mixing embeddings from two models in one vector space is silently
   broken; the fingerprint check makes the failure loud.
9. If usefulness is uncertain, weaken atoms and claims through decay
   rather than deleting them. (v0 keeps everything in memory; cold
   storage is a v1 concern.)
10. Asserted, entailed, defeasible, superseded, and retracted claims remain
    distinct.
11. Vector closeness may merge semantic regions. It must never destructively
    merge facts, instances, or events.
12. Query success reinforces the exact access path that found the answer
    (path-aware reinforcement, not nearby-vector reinforcement).
13. Where an external source pointer exists, the claims that depend on
    it carry it via `Qualifiers.source: Option<SourceId>`. Legend does
    not store the raw text of the source; if the user wants to re-read
    it, they follow the pointer. Most ticks (agent-internal, ephemeral)
    have no source pointer and lose nothing — the claim graph itself is
    the record of what Legend understood.
14. **Cache claims carry a lineage pointer.** Cache claims (derived
    current-state claims) **must** carry a `derived_from` pointer to the
    event that produced them. They are recomputable. They are never written
    without that pointer. This is the PROV-O `wasDerivedFrom` discipline
    applied as a substrate-level invariant — equivalent to JTMS
    justification-pointer rigor (Doyle 1979) and incremental view
    maintenance (Cui & Widom 2000).
15. There is one input operation: `tick`. There is no query path.
16. **Tiny footprint, fast tick.** Legend targets a hypergraph that is
    small enough to fit comfortably in RAM for any realistic personal
    use (target: dwarfed by a typical photo library — orders of
    magnitude smaller than naive transcript-with-index designs) and a
    tick that completes in **under 100 ms** end-to-end on commodity
    hardware. Every design choice is judged against these two numbers.
    A design that breaks either is wrong by definition.

---

## 4. What The Hypergraph Is Made Of

### 4.0 Mathematical Foundations

The substrate is not a Legend-invented data structure. It is a small
composition of well-studied formalisms. Naming them up front lets the
rest of §4 lean on existing theory instead of re-deriving it
informally, and lets a reader with a background in programming
languages, databases, or knowledge representation place each Legend
concept onto a formalism they already know, instead of learning
Legend's primitives from scratch.

Neuroscience references in this document live one layer up — at the cognitive
operations (§6.3, §11). The substrate itself is mathematical.

**1. Typed hypergraph with attributes** (Habel 1992; Ehrig et al. 2006).
A *hypergraph* is just a graph where an edge can connect any number of
nodes instead of always two. *Typed* means each node and edge carries
a tag saying what kind it is. *With attributes* means each node and
edge can carry an attached record of properties (numbers, vectors,
flags). Legend's atoms are the nodes; claims are the edges; `AtomKind`
and `ClaimStatus` are the type tags; `MemoryStats`, `Qualifiers`, and
embeddings are the attribute records; a claim's named slots are how
each connection point on the edge is labeled (so the graph knows
which atom plays the `agent` role, which plays `patient`, etc.).
Naming this formalism gives §4.1's indivisibility rule for free —
in a typed hypergraph, edges connect to nodes by reference; they
don't reach inside. Replay's bulk rewrites (§13) are a known kind
of structured graph rewriting under this formalism, with results that
do not depend on rule application order — important for replay
determinism.

**2. Predicates with named role-fillers** (Parsons 1990; Fillmore 1976;
Baker et al. 1998).
A claim `P(role₁ → t₁, role₂ → t₂, …)` is a predicate applied to
**named** arguments, not positional ones. This is how natural-language
event semantics is formalized — verbs have role slots like `agent`,
`patient`, `instrument`, and each slot is filled by a specific atom or
value. FrameNet (Berkeley, 1998) is the largest catalogue of this
shape: ~1,200 frames, each a predicate with its expected role
inventory. An archetype (§4.3) is the same idea applied to Legend's
substrate: a predicate plus the slots it expects, schematic over which
specific atoms fill them. The archetype's vocabulary is the frame's
role inventory.

**3. Bitemporal data model** (Snodgrass 1995; SQL:2011; Datomic).
Every fact has two time axes: **transaction time** (when the system
learned it) and **valid time** (when it was true in the world). Legend
uses `Tick` for transaction time and `Qualifiers.TimeScope` for valid
time (§3 Inv 7). This split is industry-standard — relational DBs,
Datomic, XTDB, Wikidata all use it — and is required to handle
late-arriving information and supersession correctly.

**4. Algebraic provenance** (Green, Karvounarakis, Tannen 2007).
When a derived fact is computed from base facts, the derivation
carries an algebraic record of *how* it was derived (which base facts,
combined how). The algebra you choose determines the aggregation rule:
counting derivations, picking the most-trusted source, propagating
trust levels, etc. Legend's combination of confidence × evidence-
strength × age-decay for derived claims (§4.7 `derived_from`, §6.1
cache claims, §11 supersession) is one such choice. Naming the
formalism makes "how does a cache claim's confidence update when a
base claim's confidence drops?" a closed-form question with a known
answer, rather than a per-case judgment call.

**5. Weighted formulas with online updates** (Richardson & Domingos
2006; Domingos & Lowd 2009).
A Markov logic network (MLN) is a set of logical formulas, each tagged
with a real-valued weight: higher weight means stronger belief, infinite
weight means a hard rule. To answer a query, you find the most
consistent assignment of values given those weights. Legend's
"claims with confidence + reinforce/decay + replay-time consolidation"
(§9.13, §13) is structurally an MLN whose weights update online with
every tick. Calling this out explicitly closes off a common
misreading: Legend is *not* a Bayesian network computing joint
probability distributions. It is weighted-formula reasoning, which
has a different complexity profile and a different citation graph.

What this section does **not** do: rename anything, change behavior, or
add formal proofs. It tells the reader where each piece of Legend lives
in the math literature so the rest of §4 / §13 can reference these
frameworks by name.

### 4.1 Atom

A memory atom is the smallest persistent thing Legend can activate,
retrieve, reinforce, decay, or link.

It is not a biological neuron. It is closer to an addressable engram component
or graph node. Formally: a node in a typed hypergraph (§4.0 (1)).

Atoms are **indivisible from the hypergraph's perspective** — claims reference
atoms by ID, never decompose them. `AtomKind` tags an atom's type; it does
not partition the atom into subparts. An `AtomKind::Event` atom is no more
"made of smaller atoms" than an `AtomKind::Instance` atom is — both are single
nodes the rest of the substrate operates on by reference. Claims connect to
atoms; they never reach inside them.

```rust
struct Atom {
    id: AtomId,
    kind: AtomKind,
    names: Vec<String>,           // canonical + variant; both decay if unused
    stats: MemoryStats,
    created_at: Tick,
}

enum AtomKind {
    Archetype,
    SemanticRegion,
    Instance,
    Event,
    Role,
    Value,
    Frame,
    Schema,
    Rule,
}

// Carried by both atoms and claims (§4.6) — both decay, reinforce,
// and accumulate access stats. Atoms are nodes; claims are hyperedges;
// memory dynamics apply to both.
struct MemoryStats {
    activation: f32,
    strength: f32,
    stability: f32,
    confidence: f32,
    plasticity: f32,
    salience: f32,
    access_count: u32,
    answer_success_count: u32,
    prediction_error: f32,
    last_seen: Tick,
    last_accessed: Option<Tick>,
}
```

`AtomKind` is mechanical and is allowed to drive control flow. It is not a
world ontology. `Dentist`, `Project`, `Democracy`, and `uses_datastore` are
not mechanical kinds — they are labels on `Archetype` or `Instance` atoms.

Claims are not atoms; they are hyperedges with their own ID space
(§4.6). They carry their own `MemoryStats` and decay/reinforce alongside
atoms — the dynamics are uniform; the type identity is not.

`created_at: Tick` is **transaction time** — when Legend learned this. It is
a monotonic `u64` counter incremented once per `tick()` call.
**Valid time** — when the fact was true in the world — is a separate axis
that lives on `Qualifiers.TimeScope` (§4.8). This bitemporal split is
industry standard (Datomic, XTDB, Wikidata, SQL:2011, Graphiti) and is
required by Invariant 7. Wall clock is a `Value::TimePoint`, not a system
primitive. This keeps replay deterministic and avoids time-zone issues in
the substrate.

### 4.2 Provenance (No Evidence Citizen)

Legend does **not** store inputs as a separate citizen. There is no
`Evidence` struct, no per-input record, no audit trail of "Legend saw
something at tick T." Memory IS the distilled pattern: the atoms and
claims an input produced. If extraction failed completely, there is
nothing to remember — the input is dropped (the dev-only WAL still has
the raw text for debugging; production has only the patterns).

This is the brain analogy taken seriously. The brain does not store
events; it stores patterns. Reconstruction rebuilds context from cues,
not from retrieved audit records. Legend does the same.

Where source matters — a user-cited URL, a file/commit, a chat message
id — it lives on the relevant claim's `Qualifiers.source: Option<SourceId>`
(§4.8). Most ticks (agent observations, internal turns) leave it `None`
and lose nothing — those inputs were never going to be re-readable
anyway.

What you trade by not having Evidence:

- No "what was input #4127?" — atoms with shared `created_at: Tick` came
  from the same input, but there is no input-as-thing to retrieve.
- No re-running extraction over old inputs — replay (§13.8) operates on
  the existing atom/claim graph (Hebbian, supersession, archetype
  emergence), not on stored input embeddings. We dropped raw text
  earlier, so re-extraction was already mostly impossible.

What you keep:

- Per-tick ordering via `created_at: Tick` on every atom and claim.
- Source provenance where it exists, via `Qualifiers.source`.
- A smaller substrate honoring Inv 2 (no fluff) more rigorously.

### 4.3 Archetype (Predictor Of Structure)

An **archetype** is a learned canonical *shape* an input can take —
the structural vocabulary Legend has accumulated for predicting what
comes next when something looks a certain way. Archetypes are
addressable atoms (so they decay, reinforce, and route attention like
everything else in the substrate), but their job is not to *be* a
piece of the world — it is to recognize *kinds* of structure and bias
extraction toward the slots and patterns those structures imply.

Each archetype carries three pieces of expertise:

1. **Sensory profile** — prototype vectors that say "I activate on
   inputs that look like this."
2. **Predictive shape** — `expected_slots` plus a `slot_transitions`
   matrix that say "when I activate, look for these slots in this
   order."
3. **Structural vocabulary** — a small, explicit, Hebbian-strength-
   weighted set of role atoms and claim templates the archetype
   endorses. When the archetype activates, those vocabulary entries
   gain activation weight in `focused_claims` aggregation (§9.13).
   Entries are not arbitrary "things this archetype has weights to" —
   they are the canonical structural elements the archetype predicts.

The lineage: Mountcastle 1957 / Hawkins & George (HTM) / Hawkins 2021
*A Thousand Brains Theory*. The cortical-column hypothesis says
"every cortex column runs the same algorithm; what differs is what
it's connected to." Legend's archetypes inherit the spirit (uniform
local predictor, predict-next, voting via a small canonical
vocabulary) but **not** the strong claim that consensus voting *is*
inference. Voting is one signal feeding `focused_claims`, not the
entire pipeline.

```rust
struct Archetype {
    atom: AtomId,
    prototypes: Vec<Prototype>,            // 1-8 vector exemplars
    expected_slots: Vec<SlotExpectation>,  // what fillers this predicts
    slot_transitions: SlotTransitions,     // sequence prediction over slots
    vocabulary: ArchetypeVocabulary,       // canonical structural elements
    activation: f32,                       // current tick's activation level
    reliability: f32,                      // how often this archetype predicted correctly
    plasticity: f32,                       // how willing to update
    support_count: u32,                    // total evidence supporting this archetype
}

// The structural vocabulary the archetype endorses when it activates.
// Kept small by replay's pruning of low-strength entries (§13.8) —
// no hard cap in v0; profile if it ever grows pathological.
struct ArchetypeVocabulary {
    role_atoms: Vec<(AtomId, f32)>,        // role slots this archetype expects (with strength)
    claim_templates: Vec<(ClaimId, f32)>,  // canonical claim shapes it endorses (with strength)
}

struct SlotExpectation {
    role: AtomId,                          // which Role atom this slot fills (e.g. `target`, `from`, `to`)
    type_hint: Option<AtomId>,             // expected concept/region (soft prior)
    required: bool,                        // must be filled for the archetype to "complete"
    fill_probability: f32,                 // Hebbian-learned likelihood
}

struct SlotTransitions {
    // Small transition matrix over slot indices.
    // transitions[i][j] = P(slot j fills next | slot i just filled).
    // For 3–8 slots this is 9–64 floats per archetype.
    matrix: Box<[[f32; MAX_SLOTS]; MAX_SLOTS]>,
    n_slots: u8,
}
```

The seed pack ships ~30 **domain-neutral** archetypes that predict
mechanical patterns (entity mentions, state changes, temporal
expressions, references); domain-specific archetypes like
`appointment_event` or `function_definition` emerge from evidence via
replay splitting (§13.8).

**Per-tick lifecycle:** activate (score prototypes, §9.6.5) →
predict-next (rank expected slots by prior, bias extractor attention,
§9.6.5) → endorse (vocabulary entries gain weight in `focused_claims`
aggregation, §9.13) → learn (Hebbian updates to slots, transitions,
vocabulary strengths, §13.9). Archetype scoring is embarrassingly
parallel — `par_iter` over `Vec<Archetype>` in the read-mostly phase
of the tick.

### 4.4 Instance

An instance is a specific thing, event, or situation in context.

Concepts are reused broadly. Instances are reused only with coreference
evidence (§13.3). When uncertain, create a provisional instance and let
replay merge it later.

### 4.5 Event

Events are first-class atoms in the same hypergraph. They are the source of
record for change.

```text
reschedule_event_1 target appointment_1
reschedule_event_1 property date
reschedule_event_1 from Tuesday
reschedule_event_1 to Friday
```

A flat edge like `appointment_1 date Friday` loses history. The event preserves
it. A cached current-state claim (§4.6) may exist for fast retrieval, but it
**must** point back at the event via `derived_from`.

### 4.6 Claim

A claim is a role binding — a hyperedge — and a first-class memory
citizen, not a passive label on a graph. Claims decay, reinforce,
accumulate access stats, and contribute to attention exactly the way
atoms do; the dynamics are uniform.

Formally: a predicate applied to named, role-tagged arguments, with a
real-valued weight (§4.0 (2)). The `predicate` field is the predicate;
each `RoleBinding` names one slot and points at the atom that fills it;
the weight lives in `stats.confidence`.

```rust
struct Claim {
    id: ClaimId,
    predicate: AtomId,
    roles: Vec<RoleBinding>,
    qualifiers: Qualifiers,          // includes Option<SourceId> when an external source exists
    status: ClaimStatus,
    stats: MemoryStats,              // §4.1 — same struct atoms carry
    priority: i8,                    // defeasible-logic priority (Antoniou 2002)
    supersedes: Option<ClaimId>,     // PROV-O wasRevisionOf
    superseded_by: Option<ClaimId>,
    derived_from: Option<AtomId>,    // PROV-O wasDerivedFrom — lineage to producing event
    created_at: Tick,                // transaction time only
}

struct RoleBinding {
    role: AtomId,
    term: Term,
}

enum Term {
    Atom(AtomId),                    // covers value-kind atoms; their content lives in the values payload table (§5.2)
    Claim(ClaimId),                  // for nested / conditional claims
    Variable(VariableId),            // for archetype templates only
}

enum ClaimStatus {
    Asserted,
    Entailed,
    Defeasible,
    Superseded,
    Retracted,
}
```

Standard names for the moves this struct makes (adopt the vocabulary so we
inherit the literature for free):

- **Event reification** — turning an event ("the appointment was rescheduled")
  into a first-class atom with role-tagged arguments (target, from, to)
  rather than a verb tied to a single sentence. Standard reference:
  Parsons 1990, *Events in the Semantics of English*; same shape as W3C's
  N-ary Relations note (2006).
- **Supersession chain** — `supersedes` / `superseded_by` are
  `prov:wasRevisionOf` (PROV-O) / `schema.org/supersededBy`. Walk backward
  to recover any prior current-state.
- **Lineage pointer** — `derived_from` is `prov:wasDerivedFrom` (PROV-O).
  Non-`None` for cache claims (current-state derivations); `None` for
  asserted base claims and for events themselves. Required by Invariant 14.
- **Defeasible priority** — `priority: i8` follows Antoniou's defeasible
  logic with dynamic priorities (2002). Two `Defeasible` claims that
  contradict resolve by higher priority. `Asserted` claims always outrank
  `Defeasible` regardless of priority.
- **Belief revision via supersession** is the **Levi identity** —
  contraction-of-negation followed by expansion (Alchourrón-Gärdenfors-
  Makinson 1985). Legend's correction protocol is base belief revision
  (Hansson 1999) made operational.

A claim can represent binary triples, n-ary events, nested claims,
conditional claims (via `Term::Claim` antecedents in `Qualifiers.condition`),
time, modality, and uncertainty — all in one structure.

### 4.7 Value

A value is the **content payload** for an atom of `AtomKind::Value` —
typed regions in value spaces. Values are atoms (with `AtomId`,
`MemoryStats`, qualifiers); the payload below is what lives in the
`values` table keyed by that atom's id (§5.2). The same side-table
pattern Archetype and SemanticRegion use.

```rust
enum Value {
    Text(String),
    Number { value: f64, unit: Option<UnitId> },
    TimePoint(TimeExpr),
    TimeInterval { start: TimeExpr, end: TimeExpr },
    Location(LocationExpr),
    Boolean(bool),
    Probability(f32),
    Vector(Vec<f32>),
}
```

Ungrounded weekdays are not exact dates. `Tuesday` is a weekday-concept value;
`2026-04-28` is a grounded time-point value. The relation between them is a
claim, not a substitution.

Slot fillers reference values via `Term::Atom(AtomId)` like any other
atom — there is no separate `Term::Value` variant. The atom's
`AtomKind::Value` discriminator tells you to consult the `values`
payload table for its content.

### 4.8 Qualifiers

Qualifiers scope claims. `time_scope` is the valid-time half of the
bitemporal split (§4.0 (3)); `polarity` and `modality` are modal
annotations on the predication; `derived_from` and supersession on the
parent `Claim` carry the semiring-provenance trail (§4.0 (4)).

```rust
struct Qualifiers {
    frame: Option<FrameId>,
    time_scope: Option<TimeScope>,
    source: Option<SourceId>,
    polarity: Polarity,
    modality: Modality,
    rank: ClaimRank,
    condition: Option<ClaimId>,      // antecedent for "X if Y"
}

enum Polarity {
    Affirmed,
    Negated,
}

enum Modality {
    Actual,
    Possible,           // expresses both "maybe true" and the old Polarity::Unknown
    Desired,            // deontic
    Obligatory,         // deontic
    Hypothetical,
    Counterfactual,
}

enum ClaimRank {
    Preferred,          // current/best — Wikidata-style preferred
    Normal,             // historically valid but not current
    Deprecated,         // known-false but kept for context
}

enum TimeScope {
    Instant(TimeExpr),
    Interval { start: Option<TimeExpr>, end: Option<TimeExpr> },   // Wikidata-style
    Always,
    Never,
}
```

Notes locked from research:

- **Polarity is truth-functional only.** `Polarity::Unknown` was dropped —
  it overlapped `Modality::Possible`. Express "unknown" as
  `Modality::Possible` over `Polarity::Affirmed` (or `Negated`). This
  matches modal logic.
- **Modality covers deontic distinctions** (`Desired`, `Obligatory`)
  per Standard Deontic Logic.
- **`condition` is the conditional antecedent slot.** Required because
  flag-style modality cannot represent "X is true if Y." `Modality::Hypothetical`
  marks a claim as conditional; `condition` names the claim Y on which X
  depends.
- **`rank` follows Wikidata's three-tier model** (preferred / normal /
  deprecated). It is auto-derivable from the supersession chain
  (leaf-of-chain → preferred, mid-chain → normal, retracted-or-defeated →
  deprecated) but stored explicitly as a query-time hint to skip the chain
  walk. `ClaimStatus` is mechanical (drives control flow per Invariant 5);
  `rank` is the query-facing summary; the tick pipeline keeps them
  synchronized.
- **`time_scope` is valid time, not transaction time.** Transaction time is
  `created_at` on the claim. This is the bitemporal split (Invariant 7).
  `TimeScope` follows Wikidata's start/end/point primitives. Allen-relations
  between claims are computed on demand using the **TempEval-3 pragmatic
  subset** `{BEFORE, AFTER, OVERLAP, INCLUDES, IS_INCLUDED, EQUALS, VAGUE}`
  rather than the full 13-relation algebra (NP-complete satisfiability, not
  worth it for v0).

Qualifiers are how Legend represents "I used to think X," "X if Y," "X
according to source S at time T," and "I do not want X." Without them every
claim collapses to flat assertion.

### 4.9 Semantic Region

A semantic region is a vector-space cluster inside the hypergraph.

It is the formal version of the Genesis/Void hierarchy.

```rust
struct SemanticRegion {
    atom: AtomId,
    parent_regions: Vec<(AtomId, f32)>,   // weighted DAG, not a tree
    child_regions: Vec<AtomId>,
    lateral_regions: Vec<AtomId>,
    prototypes: Vec<Prototype>,           // up to 8 in v0
    radius: f32,
    vigilance: f32,
    density: f32,
    variance: f32,
    utility: f32,
    noise_score: f32,
    claim_refs: Vec<ClaimId>,
    archetype_refs: Vec<AtomId>,
    instance_refs: Vec<AtomId>,
}

struct Prototype {
    vector: Vec<f32>,
    weight: f32,
    support_count: u32,
}
```

Topology is a **weighted DAG**, not a tree: an atom embedding can attach to
multiple parents with different weights. This handles polysemy and
cross-domain concepts naturally.

`Genesis` is the structural root. It covers known semantic space.

`Void` is the low-value / unknown / noise sink. It is not an embedding of
nothing — it is the place inputs land when they fail every threshold. Replay
decides whether anything in Void ever gets promoted.

---

## 5. Core Data Model

This section pins down the concrete substrate the coder works against first,
before any pipeline code, before any NLP. The substrate must serialize
round-trip and the inspection harness (§16) must dump it before anything else
is written.

### 5.1 Style Constraints (Ultra-Minimal Rust)

Beyond the project's R\* style, this v2 codebase commits to a stricter subset.

The intent: keep the **hot substrate** (atoms, claims, indices, the
tick loop, region routing) free of dynamic dispatch and gratuitous
abstraction. The constraint is **no custom dynamic polymorphism or
unconstrained generic abstractions in the hot substrate**, not "no
generics anywhere" — `Vec<T>`, `Option<T>`, `Result<T, E>`, `HashMap<K, V>`,
and serde derive macros (`#[derive(Serialize, Deserialize)]`) are
generic-by-construction and are obviously fine.

**Allowed:**

- Plain `struct` and `enum`.
- `Vec<T>`, `&[T]`, `&mut [T]`, `Box<[T]>`, `String`, `&str`,
  `Option<T>`, `Result<T, E>`, `HashMap<K, V>`, `VecDeque<T>`.
- `HashMap<K, V>` swap to `hashbrown` or hand-rolled open-addressing if
  profiling demands.
- Newtype wrappers around primitives (`AtomId(u32)`, `ClaimId(u32)`,
  `Tick(u64)`).
- `match`, `if let`, basic iterators (`iter`, `map`, `filter`, `fold`,
  `for`).
- Integer indices into `Vec<T>`. Never pointers between atoms.
- Concrete error enums.
- Derive macros: `#[derive(Serialize, Deserialize, Debug, Clone, Copy,
  PartialEq, Eq, Hash)]`.
- A small number of well-justified generic helper functions where they
  remove duplication without introducing trait machinery (e.g.
  `fn upsert<T: Eq>(vec: &mut Vec<T>, item: T)`).

**Disallowed unless a specific perf or correctness need is documented in
code:**

- **Custom traits with `impl` blocks** in the hot substrate. (Standard
  derive traits are fine — they're inert.)
- **`dyn` anything.** No trait objects, no `Box<dyn Error>`. Concrete
  enums for everything that would otherwise want polymorphism.
- **Unconstrained generic abstractions in the substrate.** Don't write
  `fn process<T: SomeTrait>(...)` to "future-proof" the hypergraph.
  Concrete types for `Atom`, `Claim`, `Hypergraph`.
- `Rc`, `Arc`, `RefCell`, `Mutex` inside the substrate.
- `async fn` / `tokio` / futures. Replay is a thread, not a future.
- Builder patterns.
- Derive macros beyond the allowed list.
- `clone()` in hot paths.
- `String` allocations per tick where `&str` works.
- Procedural macros we do not write ourselves.
- `lazy_static`, `once_cell`. Use plain `static` or explicit init.
- `serde_json` in the tick loop. `rmp-serde` or hand-written binary for
  hot serialization.
- Iterator chains longer than three steps. Use a `for` loop.

**Memory layout discipline:**

- All atoms in `Vec<Atom>` indexed by `AtomId(u32)`.
- All claims in `Vec<Claim>` indexed by `ClaimId(u32)`.
- Indices (`HashMap<String, Vec<AtomId>>`, etc.) are derivable. Rebuild on
  load. Do not serialize them.
- Hot scalar fields (`activation`, `strength`) are candidates for split-out
  into parallel `Vec<f32>` arrays if profiling shows the wide `MemoryStats`
  struct hurts cache. Decide on first profile, not earlier.

### 5.2 The Hypergraph Struct

The substrate uses **typed payload tables keyed by `AtomId`**. Every
atom lives in `atoms` with its `AtomKind` (§4.1); atoms whose kind is
`Archetype`, `SemanticRegion`, or `Value` carry their kind-specific fields
in a dedicated payload table. Lookup is `atoms[id].kind` → which table
to consult. This keeps the headline `Atom` struct uniform while letting
kind-specific data have its own concrete shape.

```rust
struct Hypergraph {
    // Core storage — every atom lives here, indexed by AtomId.
    atoms: Vec<Atom>,
    claims: Vec<Claim>,

    // Kind-specific payload tables, keyed by AtomId.
    // The atom's `kind` field tells you which table holds its payload.
    archetypes: HashMap<AtomId, Archetype>,     // AtomKind::Archetype
    regions: HashMap<AtomId, SemanticRegion>,   // AtomKind::SemanticRegion
    values: HashMap<AtomId, Value>,             // AtomKind::Value — content payload for value-kind atoms

    // Tick clock — monotonic, incremented once per tick.
    clock: Tick,

    // Current policy (vigilance, plasticity, decay, thresholds).
    policy: Policy,

    // Working memory — recent focused atoms, used by coreference
    // and Hebbian co-activation. Capacity ~64.
    recent_focus: VecDeque<AtomId>,

    // Derived indices — rebuild on load, never serialize.
    by_name: HashMap<String, Vec<AtomId>>,
    by_kind: HashMap<AtomKind, Vec<AtomId>>,
    region_children: HashMap<AtomId, Vec<AtomId>>,
    region_parents: HashMap<AtomId, Vec<(AtomId, f32)>>,
    claims_by_subject: HashMap<AtomId, Vec<ClaimId>>,
    claims_by_predicate: HashMap<AtomId, Vec<ClaimId>>,
    supersession_chain: HashMap<ClaimId, ClaimId>,
}
```

**Why typed payload tables instead of one big `Atom` struct.** Putting
`Vec<Prototype>`, `Vec<SlotExpectation>`, `SlotTransitions`, etc. on
every atom would waste memory on the 90% of atoms that aren't archetypes
or regions. Keeping them in side tables means the hot `atoms: Vec<Atom>`
stays cache-friendly while role-specific data is one indirection away
when needed. `HashMap` is fine for v0; if the archetype count grows large,
swap to `Vec<Option<Archetype>>` indexed by `AtomId` — same `O(1)`
lookup, better cache locality.

When you see `&hg.archetypes[&atom_id]` in pseudocode for the archetype
functions (§9.6.5, §11.10, §13.9), it's reading the payload
table for an atom whose `role` is `Archetype`.

### 5.3 Policy

```rust
struct Policy {
    descend_threshold: f32,             // 0.65 default
    merge_threshold: f32,               // 0.80 default
    void_threshold: f32,                // 0.30 default
    vigilance: f32,                     // 0.5 default
    plasticity: f32,                    // 0.5 default
    decay_rate: f32,                    // per-tick decay applied to inactive atoms
    salience_floor: f32,                // minimum salience to write a claim
    fanout_k: u8,                       // top-k DAG children considered per insert; 3 default
    max_prototypes_per_region: u8,      // 8 default; replay decides on overflow

    // Archetype dynamics (§4.3, §9.6.5, §13.8, §13.9)
    archetype_activation_threshold: f32,        // min similarity for an archetype to activate; 0.55 default
    archetype_voting_weight: f32,               // how much an active archetype's endorsement boosts focused_claims; 0.3 default
    slot_prediction_bias: f32,                  // how strongly slot_transitions bias extractor attention; 0.4 default
    archetype_plasticity: f32,                  // per-archetype update step size; 0.3 default
    archetype_vocabulary_min_strength: f32,     // replay drops vocabulary entries below this; 0.05 default
}
```

PFC (a function, not a module — see §11) adjusts `Policy` based on detected
intent before each tick's main pipeline runs.

### 5.4 Concurrency Model

- **One owner of the hypergraph.** Synchronous tick takes
  `&mut Hypergraph`.
- **Replay is a background thread.** It does not share the hypergraph. The
  replay thread is given a snapshot (clone of the slices it needs), computes
  proposed mutations, sends a batch back via a channel, and the main thread
  applies them under `&mut`.
- **No `Arc<RwLock<Hypergraph>>`.** No interior mutability in the substrate.
  Rust's borrow checker enforces single-writer at compile time. This is the
  reason we chose Rust over C.

### 5.5 Identifier Discipline

- All ids are `u32` newtypes (`AtomId(u32)`, `ClaimId(u32)`,
  `FrameId(u32)`, `SourceId(u32)`, `VariableId(u32)`,
  `RegionId = AtomId`). Value-kind atoms use `AtomId` like every other kind;
  their content lives in the `values` payload table (§5.2).
- `Tick(u64)` for the monotonic clock.
- Reserve `u32::MAX` as `INVALID`. Never panic on bad ids; return
  `Result<_, HypergraphError>`.

### 5.6 Auxiliary Type Definitions

These types appear as fields in the structs above. Defined here once so
the substrate is fully concrete when you start coding.

```rust
// Pointer to a claim, used in attention-state output.
type ClaimRef = ClaimId;

// Time expression — a parsed datetime or weekday-concept reference.
// The temporal parser (§12.1 #5) emits these.
enum TimeExpr {
    GroundedDate(NaiveDate),                   // 2026-04-28
    GroundedDateTime(NaiveDateTime),           // 2026-04-28T14:30
    Weekday(Weekday),                          // Tuesday — ungrounded
    Relative { anchor: Tick, offset: Duration },  // "in two weeks"
    Duration(Duration),
    Unresolved(String),                        // raw text we couldn't parse
}

// Newtype for measurement units (kg, USD, lines-of-code, etc.).
// The Value::Number variant uses this. Unit atoms live in the
// hypergraph; this id refers to one.
type UnitId = AtomId;

// Geographic / spatial reference. Symbolic for v0; v1 may add geocoding.
enum LocationExpr {
    Named(String),                             // "the body shop"
    Coords { lat: f64, lon: f64 },
    AtomRef(AtomId),                           // resolved to an atom
}

// The raw input to a tick.
struct Input {
    text: String,
    source: SourceId,
    wall_clock: SystemTime,                    // for logs only — never drives substrate logic
}

// Maximum slots a single archetype can have. Bounds the slot_transitions matrix.
const MAX_SLOTS: usize = 8;

// Activation records returned in ConsciousAttentionState.
struct RegionActivation {
    region: AtomId,
    activation: f32,
    surfaced_atoms: Vec<AtomId>,
}

struct ClaimActivation {
    claim: ClaimId,
    score: f32,                                // base_weight + vote_weight (§9.13)
    surfaced_by: Vec<LensSource>,              // which lens(es) brought this to focus
}

enum LensSource {
    RegionRouting(AtomId),                     // a region surfaced this claim
    ArchetypeEndorsement(AtomId),                        // a archetype endorsementd for this claim
    LexicalMatch,                              // BM25 hit
    SupersessionWalk,                          // appeared during chain traversal
    RecentFocus,                               // already in working memory
}

struct AnswerCandidate {
    text: String,                              // human-readable answer
    backing_claims: Vec<ClaimId>,
    confidence: f32,
}

struct UncertaintySignal {
    kind: UncertaintyKind,
    message: String,
}

enum UncertaintyKind {
    UngroundedTime,                            // weekday without a date
    AmbiguousCoref,                            // pronoun could resolve multiple ways
    ConflictingArchetypeEndorsements,                    // active archetypes disagreed
    LowConfidenceExtraction,
    MissingExpectedSlot(AtomId),               // an archetype predicted a slot that didn't fill
}

enum AttentionAction {
    WatchForCorrection(AtomId),                // future ticks should bias to update this
    EnqueueReplay(ReplayJob),
    PromoteFromVoid(AtomId),
}

// Used by `aggregate_focus` and replay.
struct ClaimCandidate {
    claim_id: ClaimId,
    base_confidence: f32,
    base_salience: f32,
}

struct FocusOutcome {
    final_focused: Vec<ClaimId>,               // what made it into focused_claims
    rejected: Vec<ClaimId>,                    // what didn't
}

// Read-only snapshot for the replay thread (§5.4).
type HypergraphSnapshot = Hypergraph;          // cloned slices; identical layout

// The set of operations a replay batch can propose.
enum ReplayMutation {
    SplitRegion { region: AtomId, into: Vec<Vec<AtomId>> },
    MergeRegions { regions: Vec<AtomId> },
    EmergeArchetype { from_region: AtomId, prototypes: Vec<Prototype>, slots: Vec<SlotExpectation> },
    PruneArchetype { archetype: AtomId },
    MergeArchetypes { archetypes: Vec<AtomId> },
    ResolveCoreference { provisional: AtomId, canonical: AtomId },
    EvictPrototype { region: AtomId, prototype_index: usize },
    PromoteFromVoid { atoms: Vec<AtomId>, into_region: AtomId },
}

enum ReplayJob {
    HighVarianceRegion(AtomId),
    SuspectedDuplicateArchetype(AtomId, AtomId),
    StaleProvisionalInstance(AtomId),
}

// Used by activate_archetypes / predict_next.
struct SlotPrediction {
    archetype: AtomId,
    role: AtomId,
    type_hint: Option<AtomId>,
    confidence: f32,
}

// Used by the EmbeddingWrapper (§12.1 #3) to apply the right
// prefix/pooling per model.
enum EmbedKind {
    Query,
    Passage,
}

// Concrete error enum returned by hypergraph operations.
enum HypergraphError {
    InvalidAtomId(AtomId),
    InvalidClaimId(ClaimId),
    CacheClaimMissingDerivedFrom(ClaimId),     // Invariant 14 violation
    SupersessionCycle(ClaimId),
    ArchetypeSlotsExceeded(AtomId),               // > MAX_SLOTS
    ModelFingerprintMismatch { expected: ModelFingerprint, found: ModelFingerprint },
    SerializationError(String),
}
```

These are intentionally light. Most can be expanded as the
implementation evolves; they're listed here so the type system is
closed when you start writing the substrate.

### 5.7 Serialization (Snapshot + Bounded WAL)

The hypergraph snapshot is the system of record. A small bounded
write-ahead log sits alongside it for crash recovery between checkpoints
and bug reproduction during development. This is **WAL-style durability**
(every relational DB ever shipped), not full event sourcing — Legend
neither stores nor consults a full event history at runtime.

#### 5.7.1 The Snapshot

The on-disk hypergraph image is the canonical state.

- Format: LZ4 + MessagePack.
- Serialized fields: `atoms`, `claims`, `clock`, `policy`, plus a
  `stamped_at: Tick` marker and the `ModelFingerprint` in force when
  it was written.
- Derived indices are rebuilt on load.
- v0 has no format migrations. When the format changes in v1, add a
  4-byte version header.

#### 5.7.2 The Bounded WAL

```rust
struct LogEntry {
    tick: Tick,
    input: Input,                    // raw text + source + wall-clock
    model_fingerprint: ModelFingerprint,
}

struct ModelFingerprint {
    embedding_model: String,         // e.g. "bge-small-en-v1.5"
    embedding_dim: u16,
    tokenizer_vocab_hash: u64,       // load-bearing — silent drift killer
    extractor_versions: Vec<(String, String)>,
    code_version: String,            // git SHA
}
```

WAL on-disk layout:

- **Segmented** — ~1 MB segments. Active segment uses LZ4 fast-path on
  every append (latency-first). Closed segments are recompressed with
  zstd-19 in a background thread (size-first).
- **Bounded** — total on-disk size is capped at **10 MB** in production
  builds. When the cap is exceeded, the oldest closed segment is dropped.
  The cap holds roughly 4–7 years of inputs at 100 ticks/day under heavy
  compression.
- **Development override** — a `LEGEND_WAL_UNBOUNDED=1` env var disables
  the cap and retains every segment, for bug reproduction during
  development. Production builds ignore this flag.

**Boot path:** load latest snapshot, replay every WAL entry with
`tick > snapshot.stamped_at` on top.

**Crash recovery:** identical path. Whatever survived in the WAL since
the last checkpoint is replayed.

#### 5.7.3 Checkpoint Policy

Hybrid (well-precedented across RocksDB / Kafka Streams / Flink):

```text
checkpoint when (
  ticks_since_last_checkpoint > N    OR
  wal_size_bytes > S                 OR
  time_since_last_checkpoint > T
)
```

v0 starting numbers: **N = 1000 ticks, S = 5 MB, T = 1 hour.** The S
threshold is half the WAL cap so checkpoints fire well before
queue-eviction does. Tune from profiling.

After a checkpoint lands and is fsynced, all WAL entries with
`tick <= snapshot.stamped_at` are dropped. Combined with the 10 MB cap,
day-to-day WAL size is bounded by whichever is smaller.

#### 5.7.4 Boot-Time Fingerprint Check (Invariant 8)

On boot, the running binary's pinned `ModelFingerprint` is compared
against the snapshot's. Mismatch → refuse to start.

The embedder is pinned for Legend's lifetime (§12.1). Because Legend
does not retain raw text and does not store inputs as a separate
citizen, there is no in-place re-embedding migration. If the model is
ever changed (a deliberate, one-time event), the only path is to
re-ingest from any `Qualifiers.source` pointers still reachable, and
accept lost atoms where they are not. This makes the pin-for-life
commitment hard, not aspirational — treat it that way at decision
time.

There is no "replay-under-different-fingerprint" mode at runtime, no
"cheap path / expensive path" branch, and no time-travel.

#### 5.7.5 Storage Cost

The hypergraph is dominated by **embeddings** (one ~1.5 KB f32 vector
per concept/region/archetype atom; int8 quantization in v1 cuts this
4×). Claims and concept atoms are ~100–500 B each. With raw text and
input records dropped, typical hypergraph sizes are orders of magnitude
smaller than naive transcript-with-index designs. The WAL is bounded
at 10 MB. Latency, not disk, is the primary scarcity (Invariant 16).

---

## 6. Carry-Forward From Current Legend

This is a fresh repo with a fresh data model. We bring forward **concepts**,
not code.

### 6.1 What We Keep (As Concepts)

- **Decay + reinforcement scalars on every memory citizen.** In
  `MemoryStats` (§4.1), shared by atoms and claims. Constants worth
  cribbing from current Legend's basal-ganglia AdaGrad code.
- **Salience scoring at write time.** Becomes the function that decides
  amygdala protection and initial atom strength. Not a module — a function.
- **Pattern separation.** The "do not collapse close-but-distinct" rule used
  inside coreference scoring (§13.3). Current `dentate_gyrus.rs` is the
  reference implementation; we re-derive from scratch.
- **Working-memory ring buffer.** A `VecDeque<AtomId>` of the last ~64
  focused atoms. Used by coreference ("it" resolves against recent focus) and
  by Hebbian co-activation (§11.6).
- **Neurochemistry-style policy modulators.** Not the names (NE/DA/ACh/etc.
  are noise to a new reader), but the *idea* — global scalars that flex based
  on intent. Now lives in `Policy` (§5.3) and is set by PFC (§11.5).

### 6.2 What We Drop

- **L1/L2/L3 layering.** The substrate replaces it. Working memory is a small
  ring buffer; everything else is one hypergraph.
- **Brain-region module boundaries.** Brain processes are pure functions over
  `&mut Hypergraph` (§11), not modules with their own state.
- **The wernicke lexicon.** ~3400 lines of hand-coded entity logic. Replaced
  by the seed pack (§7) plus extractors (§12).
- **`TickResult`.** Replaced by `ConsciousAttentionState` (§9.13).
- **Persistence/WAL/daemon/MCP/CLI.** Out of scope for the core substrate.
  Reattach in v1 once the substrate is proven.
- **Anything Python or JVM.** No sidecars. No exceptions.
- **Per-input audit records.** No `Evidence` struct, no input-as-thing.
  Memory is the distilled pattern; if extraction failed, there is
  nothing to remember. Source pointers (where they exist) live on
  `Qualifiers.source`.

### 6.3 What "Brain Regions" Means In v2

Each brain region from current Legend maps to a **function**, not a module —
none own state, the hypergraph is the only owned thing. The full mapping with
signatures lives in §11. Names are retained as descriptive shorthand, not
architectural boundaries.

---

## 7. Seed Layer (Concrete Data Spec)

The seed layer is **not** a runtime system. It is an **initialized
hypergraph state** shipped as a serialized file. Boot loads it. Ticks then
evolve it.

This makes the seed layer **data**, not code. Replaceable, version-controlled,
inspectable. Customizable per Legend instance without recompilation.

### 7.1 Code / Seed / Input Boundary

```text
Code owns mechanics.
Seeds own priors.
Inputs own truth — but Legend keeps the patterns, not the inputs.
Replay owns learning.
```

Hard-coded code owns only substrate mechanics:

```text
AtomKind, ClaimStatus, Polarity, Modality, ValueKind
time/value comparison
source provenance on Qualifiers
decay/reinforcement/replay mechanics
the tick pipeline
the embedding interface
```

Seeded hypergraph data owns priors:

```text
Genesis, Void
broad seed regions (§7.2)
generic role atoms
core schema atoms (§7.3)
```

Seeded priors carry provenance:

```text
source: built_in_seed
status: defeasible
user_confirmed: false
```

### 7.2 Seed Regions

Genesis starts with sixteen broad children. Each is a `SemanticRegion` atom
with one hand-authored descriptor sentence, embedded at boot.

```text
Genesis
  reference-like patterns
  change/history patterns
  time/order patterns
  quantity/measure patterns
  space/location patterns
  actor/agency patterns
  artifact/object patterns
  communication/discourse patterns
  task/goal/work patterns
  preference/constraint patterns
  decision/rationale patterns
  affect/salience patterns
  text-structure patterns
  code/software patterns
  narrative/creative-work patterns
  music/poetic-form patterns
```

These are starter access paths. They can split, merge, weaken, or be ignored.

**Concrete seed-region example** — the file format the seed pack ships in:

```yaml
- atom_id: REGION_CHANGE_HISTORY
  role: SemanticRegion
  names: ["change/history patterns"]
  parent_regions:
    - [GENESIS, 1.0]
  descriptor: >
    Something that was one way and is now different. A value moved from
    an old state to a new state. A revision, an edit, a correction, a
    supersession of a previous claim. Before and after. Was and is.
  prototype_source: descriptor   # embed the descriptor at boot
  vigilance: 0.7
  provenance:
    source: built_in_seed
    status: defeasible
    user_confirmed: false
```

`appointment` is **not** a required seed. It emerges from evidence under
`task/goal/work` + `time/order` + `change/history` if the user's text
contains appointments. If the corpus is mostly appointments, replay splits
out rich appointment-specific subregions.

### 7.3 Seed Schemas

Schemas are reusable extraction and interpretation patterns. Each is a
`Schema` atom with a `predicate` set and a small extraction template.

Eleven core schemas:

```text
Reference          something is mentioned, named, pointed to, or re-mentioned
Identity           two mentions may refer to the same thing (coreference required)
State              something has a value, property, relation, location, role, or status
Change             a state changes from old value to new value
Revision           text, code, plan, belief, preference, or artifact is edited
Decision           an option is selected with rationale and alternatives
Task               work is intended, active, blocked, completed, deferred, or superseded
Preference         user/project behavior should follow a stated style or rule
QuestionAnswer     input seeks information; output should select answer-bearing claims
Provenance         a claim carries an external source pointer (URL, file, msg id) on its qualifiers
Temporal           before/after/current/previous/next/latest/history
Quantification     count, amount, threshold, unit, comparison, range
```

**Concrete seed-schema example:**

```yaml
- atom_id: SCHEMA_CHANGE
  role: Schema
  names: ["Change"]
  trigger_lemmas: ["change", "move", "update", "shift", "switch"]
  trigger_patterns:
    - "from {old} to {new}"
    - "changed to {new}"
    - "moved {target} to {new}"
    - "{target} is now {new}"
  emits:
    event:
      role: Event
      bindings:
        target: "{target}"
        property: inferred
        from: "{old}"
        to: "{new}"
    cache_claim:
      predicate: current_value
      derived_from: $event
      supersedes: previous current_value claim for {target}
  provenance:
    source: built_in_seed
    status: defeasible
```

Schemas produce claim/event **proposals** with confidence. They do not decide
truth on their own. The pipeline (§9) reconciles proposals against existing
state.

### 7.4 Seed Archetypes (Domain-Neutral Mechanical Predictors)

Hard rule: **no seed archetype is a world entity.** No `appointment`,
`function`, `character`. Seeds are predicate-shaped predictors over
mechanical patterns that survive any domain. Domain-specific archetypes
emerge from evidence via replay splitting (§13.8).

The 30 seed archetypes (the 30 most reusable predicate-shaped patterns in
language):

```text
ENTITY & REFERENCE
  entity_mention       — something is named/pointed-to
  reference_chain      — coreference between mentions
  bridging_reference   — "the X" referring to a previously-mentioned X
  paraphrase           — restatement of an existing claim
  attribution          — "X said Y" / source-tagged claim

STATE & CHANGE
  state_assertion      — subject has property/value
  state_with_temporal_value  — state scoped to a time
  change_event         — state transition (from/to)
  edit_event           — diff/revision/correction
  negated_state        — explicit absence of a state
  causal_chain         — A caused B / A because B

EVENT STRUCTURE
  event_with_participants    — actor + patient + roles
  event_with_outcome         — action + result
  enumeration                — list of items

TEMPORAL
  temporal_expression  — date / weekday / interval / relative time
  sequence             — before/after ordering of events

QUANTITATIVE
  quantity             — number + unit + comparator
  comparison           — X greater/less/equal Y
  aggregation          — count/sum/avg over a set

INTENT & MODALITY
  preference_assertion — holder wants/dislikes target
  desire               — modal `Desired`
  obligation           — modal `Obligatory`
  condition            — "X if Y" antecedent structure
  question             — interrogative + topic + expected answer kind

DISCOURSE
  decision_point       — option selected from alternatives with rationale
  task_state           — work intended/active/blocked/done/superseded
  definition           — naming + describing a new term
  quotation            — verbatim citation
  location_expression  — spatial reference / where
  code_construct       — generic syntactic pattern (function/class/module/import)
                         -- this one is generic, not language-specific
```

Each archetype ships with hand-authored prototypes (descriptor sentences
embedded at boot, like seed regions), expected slots, and a small seeded
slot-transition matrix derived from how these patterns most commonly
appear in English. **Concrete seed-archetype example:**

```yaml
- atom_id: ARCHETYPE_CHANGE_EVENT
  role: Archetype
  names: ["change_event"]
  descriptor: >
    A state that was one way is now different. Something moved, shifted,
    transitioned, was updated, was corrected. Has a target, a property,
    an old value, a new value, optionally a time and an actor.
  prototypes:
    - "from {old} to {new}"
    - "changed from {old} to {new}"
    - "{target} is now {new} instead of {old}"
  expected_slots:
    - role: ROLE_TARGET           # what changed
      type_hint: null             # any concept
      required: true
      fill_probability: 1.0
    - role: ROLE_PROPERTY         # which property of target changed
      type_hint: null
      required: false
      fill_probability: 0.7
    - role: ROLE_FROM             # old value
      type_hint: null
      required: false
      fill_probability: 0.85
    - role: ROLE_TO               # new value
      type_hint: null
      required: true
      fill_probability: 0.95
    - role: ROLE_TIME             # when the change happened
      type_hint: null
      required: false
      fill_probability: 0.4
    - role: ROLE_ACTOR            # who caused the change
      type_hint: null
      required: false
      fill_probability: 0.3
  slot_transitions:
    # P(slot j fills next | slot i just filled)
    # Hand-seeded; refined by Hebbian co-activation as evidence accumulates.
    target -> property:  0.40
    target -> from:      0.45
    target -> to:        0.10
    property -> from:    0.50
    property -> to:      0.40
    from -> to:          0.85
    to -> time:          0.30
    to -> actor:         0.10
  vocabulary:
    # The structural vocabulary this archetype endorses when it
    # activates. Small, canonical, replay-pruned (§13.8). Seeded with
    # the role atoms this archetype's slots use plus a couple of
    # template claim shapes — Hebbian-strengthened by experience.
    role_atoms:
      - role: ROLE_TARGET     ; strength: 0.9
      - role: ROLE_FROM       ; strength: 0.85
      - role: ROLE_TO         ; strength: 0.9
      - role: ROLE_PROPERTY   ; strength: 0.6
    claim_templates:
      - template: TEMPLATE_CHANGE_EVENT  ; strength: 0.8
        # canonical claim: predicate=change, roles={target, from, to}
  reliability: 0.8
  plasticity: 0.5
  provenance:
    source: built_in_seed
    status: defeasible
    user_confirmed: false
```

When tick 1 of §14 fires ("My dentist appointment with Dr. Rao changed
from Tuesday to Friday"), `ARCHETYPE_CHANGE_EVENT` activates because
its prototypes match the embedded text. Its expected slots prime
extractor attention for `target` (the appointment) and `from`/`to`
(Tuesday → Friday). The slot-transition matrix says "after `from`
fills, `to` is 85% likely next" — which biases the temporal parser to
look for a second weekday. The archetype's vocabulary
(`ROLE_TARGET`, `ROLE_FROM`, `ROLE_TO`, `TEMPLATE_CHANGE_EVENT`)
contributes endorsement weight to the resulting `reschedule_event_1`
claim during `focused_claims` aggregation.

`appointment_event` does **not** appear in the seed pack. Replay can
split out a learned archetype called `appointment_event` later if
evidence accumulates around a particular `state_with_temporal_value`
configuration with consistent provider/participant slots — but that is
earned by the corpus, not by us.

### 7.5 Seed Pack Manifest

The seed pack ships as one file:

```text
seed_v0.msgpack.lz4
  - 1 Genesis atom
  - 1 Void atom
  - 16 SemanticRegion atoms with descriptor-derived prototypes
  - 11 Schema atoms with trigger lemmas/patterns
  - 30 Archetype atoms with prototypes + slots + transitions (§7.4)
  - ~12 generic Role atoms (target, source, agent, patient, time, ...)
  - ~8 generic Frame atoms (user_schedule, codebase, narrative, chat, ...)
```

Roughly 80 atoms. Hand-authored. Version-controlled. Replaceable per
Legend instance. None of them encode a domain entity.

---

## 8. Semantic Region Hierarchy

The hierarchy organizes semantic space. It does not replace the fact graph.

### 8.1 Topology

**Weighted DAG, not a strict tree.** An atom embedding may attach to multiple
parent regions with different weights. This handles polysemy ("appointment"
under both `task/goal/work` and `time/order`) and cross-domain concepts.

`Genesis` is the structural root. `Void` is a sink for inputs that fail every
threshold; replay decides whether anything in Void escapes.

### 8.2 Region Routing (Read-Only) + Application (Mutation)

Region traversal happens in **two phases** to preserve the
read-mostly-parallel / mutation-sequential boundary (§9.1):

- **Phase A — `route_regions` (read-only, parallel).** Walks the DAG,
  computes which regions an embedding *would* merge into, descend into,
  attach to as a sibling, or land in Void. Emits a `RegionDelta`
  describing the proposed structural change. **Does not mutate.**
  Runs under `&Hypergraph` and parallelizes across embeddings.
- **Phase B — `apply_region_delta` (mutation, sequential).** Runs in
  the mutation phase of the tick (Step 8–10). Takes the proposed
  `RegionDelta` and applies it: creates new region atoms, attaches
  parents, updates prototypes via spherical-k-means, attaches refs.
  Uses `&mut Hypergraph`.

```rust
struct RegionDelta {
    // Multi-parent attachments — DAG, not tree.
    parent_attachments: Vec<(AtomId, AtomId, f32)>,  // (child_region, parent_region, weight)
    // Prototype updates to existing regions.
    prototype_updates: Vec<(AtomId, Vec<f32>)>,      // (region, embedding_to_fold_in)
    // New regions to create.
    new_regions: Vec<NewRegion>,
    // Number of inputs that fell to Void this tick. Bookkeeping only;
    // not stored in the hypergraph (no input citizen).
    void_count: u32,
}

struct NewRegion {
    parent: AtomId,
    initial_prototype: Vec<f32>,
}
```

Algorithm (Phase A — read-only):

```text
route_regions(x, refs, hg, policy) -> RegionDelta:
  delta = RegionDelta::empty()
  walk(x, refs, node = Genesis, delta)
  return delta

walk(x, refs, node, delta):
  # Top-k child consideration, not best-only — this is what makes the
  # topology a DAG, not a tree.
  candidates = top_k_children_by_similarity(node, x, k = policy.fanout_k)
                 # k defaults to 3; tuned by Policy

  if candidates is empty:
    delta.new_regions.push(NewRegion { parent: node, initial_prototype: x, refs })
    return

  any_attached = false
  for c in candidates:
    sim = similarity(c, x)

    if sim >= policy.merge_threshold:
      delta.prototype_updates.push((c, x))
      delta.parent_attachments.push((refs.region_atom, c, sim))
      any_attached = true
      walk(x, refs, c, delta)        # recurse to find finer-grained matches

    elif sim >= policy.descend_threshold:
      delta.parent_attachments.push((refs.region_atom, c, sim))
      any_attached = true
      walk(x, refs, c, delta)

    # below descend_threshold: skip; void decision is global, below.

  # If nothing attached and best was below void, send to Void.
  best_sim = similarity(candidates[0], x)
  if !any_attached and best_sim < policy.void_threshold:
    delta.void_attachments.push(refs)
    return

  # Above void but nothing close enough: create a sibling under this node.
  if !any_attached:
    delta.new_regions.push(NewRegion { parent: node, initial_prototype: x, refs })
```

Algorithm (Phase B — mutation, applied in pipeline mutation phase):

```text
apply_region_delta(hg: &mut Hypergraph, delta: RegionDelta):
  for (child, parent, weight) in delta.parent_attachments:
    hg.region_parents[child].push((parent, weight))
    hg.region_children[parent].push(child)

  for (region, x) in delta.prototype_updates:
    spherical_k_means_update(hg.regions[region].prototypes, x)

  for new_region in delta.new_regions:
    let id = hg.allocate_atom(AtomKind::SemanticRegion)
    hg.regions.insert(id, SemanticRegion::new(new_region.initial_prototype))
    hg.region_parents.insert(id, vec![(new_region.parent, 1.0)])

  for refs in delta.void_attachments:
    hg.attach_to_void(refs)
```

Notes:

- **Top-k considered, not best-only.** This is the DAG generalization of
  the BIRCH/ART best-child traversal. With `k = 1` the algorithm reduces
  to a tree.
- **Multi-parent attach when ambiguity is high.** Any candidate that
  clears `descend_threshold` (and any that clears `merge_threshold`)
  becomes a parent with similarity-weighted edge. This handles polysemy
  ("appointment" under both `task/goal/work` and `time/order`) by
  construction rather than after-the-fact.
- **Reinforcing a region updates prototypes and access paths** but does
  not merge the underlying claims or instances (Invariant 11).
- **Per-candidate decision short-circuits at merge.** If the top candidate
  merges (sim ≥ merge_threshold), don't keep descending into worse
  candidates beyond their own descend cut.
- **Void is a global fallback**, not a per-candidate decision.
- Replay can later split, merge, or rebalance regions, including pruning
  redundant DAG edges.

`fanout_k` defaults to 3 in v0. High-vigilance intents (correction,
identity) push `k` to 1 to force precise routing; brainstorming relaxes
to 5.

### 8.3 Thresholds

Three thresholds, all on `Policy`:

```text
descend_threshold   0.65   close enough to route deeper
merge_threshold     0.80   close enough to update the region prototype
void_threshold      0.30   too weak/noisy to promote into active structure
```

Empirical anchoring (sentence-transformer cosine space, BGE/MiniLM/E5
families):

- `sentence-transformers` library default for community detection: 0.75
- Reimers & Gurevych paraphrase-mining: 0.78–0.84
- Paraphrase-class similarity: 0.85+
- Topical-relatedness: 0.55–0.75

`merge_threshold = 0.80` is the field's working middle ground. Vigilance
pushes it to 0.90 for `correction` / `identity` intents (§8.3 vigilance
table) and relaxes it to 0.75 for brainstorming.

Vigilance flexes thresholds per intent (§11.5):

```text
intent              vigilance   effect
correction          0.9         tighter merge_threshold; force precise routing
identity            0.9         tighter merge_threshold
temporal_update     0.9         tighter merge_threshold
question            0.7         slightly tighter than baseline
statement           0.5         baseline
brainstorming       0.3         looser; allow broader cluster updates
```

### 8.4 Region Merge Rule

Regions may merge when:

- prototypes are close,
- claim overlap is high,
- answer behavior is equivalent,
- merging does not collapse distinct instances,
- no contradiction or frame conflict appears.

Do not merge when:

- two regions answer different questions,
- one contains superseded state and the other contains current state,
- they share words but not roles,
- they belong to distinct frames,
- their claim sets contradict.

### 8.5 Region Split Rule

Split a region when:

- internal variance grows,
- repeated prediction errors occur,
- queries route into the region but need different answers,
- routed claims form distinct frames,
- a broad concept contains separable instances or sub-concepts.

Splitting improves routing. It does not duplicate or destroy claims.

### 8.6 Multi-Prototype

Each region stores up to **8 prototypes** (`Policy.max_prototypes_per_region`,
not a constant), not one centroid. Reasons:

- one average vector turns broad regions into mush;
- concepts can be polysemous;
- a region may need exemplars for different frames.

When a 9th prototype would be added, replay decides whether to split the
region or evict the lowest-weight prototype.

### 8.7 Cosine-Specific Update Rule

Sentence-transformer embeddings are unit-normalized cosine-space vectors,
not Euclidean. **Do not naively port Fuzzy ART's complement-coding update**
— it assumes inputs in [0,1]^d. Use the **spherical k-means update** for
prototype updates: running mean weighted by `support_count`, then
re-normalize to unit length. This is the standard cosine-native equivalent.

Prototypes drift toward density modes on cosine space; cap update step size
(`Policy.plasticity` regulates this) and tag prototypes with `support_count`
to detect stale ones for replay-time eviction.

### 8.8 Failure Modes To Plan Around

Drawn from the ART survey (Brito da Silva et al. 2019, arXiv 1905.11437)
and from GHSOM / DPMM literature:

1. **Category proliferation** — vigilance too high → every input creates a
   new region. Mitigation: monitor region creation rate per tick; if it
   doesn't decay, vigilance is too high.
2. **Order dependence** — same data in different presentation order →
   different clusters. Universal across online algorithms. Mitigation:
   DDVFA's Merge-ART module (§13.1); periodic replay-driven re-clustering
   of buffered Void/low-utility regions.
3. **Prototype drift / centroid collapse** — long-running prototype
   accretes toward the density mode. Mitigation: multi-prototype helps;
   cap update step size; track `support_count`.
4. **Catastrophic forgetting** — plasticity overwhelms stability.
   Mitigation: per-intent vigilance flex is the right knob.
5. **Cluster collapse to one super-region** — vigilance too low →
   everything merges. Mitigation: enforce a minimum-vigilance floor;
   `void_threshold` alone isn't enough.
6. **Adversarial Void growth** — noise inputs land in Void; replay must
   use `noise_score` and `support_count`, not just embedding closeness,
   when deciding whether to promote anything from Void.

The inspection harness (§16) ships with a region-proliferation-rate
dashboard from day one. Most ART pathologies show up there before they
show up in eval.

---

## 9. Tick Pipeline (The Only Verb)

Every tick is both a write opportunity and a perception update. There is no
separate query path.

```text
tick = update the current model of reality
```

Some ticks are write-heavy:

```text
My dentist appointment moved from Tuesday to Friday.
```

Some ticks are read-heavy:

```text
When is my dentist appointment?
```

Some are both:

```text
Actually, it moved again to Monday. What do I have Tuesday now?
```

All flow through the same pipeline and return a `ConsciousAttentionState`.

### 9.1 The Fourteen Steps

```text
0.  log entry                      -> append (Tick, Input, ModelFingerprint) to WAL
1.  detect intent                  -> AttentionIntent
2.  adjust policy                  -> Policy updated for this tick
3.  segment text                   -> spans (sentence/clause/entity/value)
4.  embed every span               -> Vec<(span, embedding)>
                                      -- READ-MOSTLY PARALLEL PHASE BEGINS --
5a. route through region DAG       -> active regions per span
5b. activate archetypes + predict  -> active archetypes + slot predictions
                                      (par_iter over Vec<Archetype>; embarrassingly parallel)
6.  run extractors                 -> claim/event proposals with confidence
                                      (extractors biased by step 5b's slot predictions)
7.  coreference scoring            -> instance reuse vs. provisional new
                                      -- MUTATION PHASE BEGINS (single &mut Hypergraph) --
8.  build claims & events          -> appended to hypergraph with status
9.  supersede prior cache          -> mark old current-state claims Superseded
10. derive current-state cache     -> new cache claims pointing at events
11. apply Hebbian + salience       -> MemoryStats (atoms + claims) + archetype vocabulary/transitions
                                      (active archetypes vote into focused_claims weighting)
12. apply decay                    -> inactive atoms and claims weaken (incl. archetypes)
13. assemble attention state       -> ConsciousAttentionState returned
                                      (active_archetypes + focused_claims surfaced)
14. enqueue replay                 -> hand snapshot to replay thread
```

Step 0 is the WAL append (§5.7). It happens *before* Step 1 so that
even if a later step panics, the WAL entry is durable and the tick can
be replayed in dev (production discards on extraction failure — there
is no input citizen to preserve).

**Parallelism boundary.** Steps 4, 5a, 5b, 6 run with read-only access to
the hypergraph and parallelize cleanly via `rayon::par_iter`. Steps 8–13
require `&mut Hypergraph` and run sequentially. This is the
read-mostly-parallel, write-sequentially pattern — same shape Datomic and
FoundationDB use. Archetype scoring at scale (5K archetypes × 8 embeddings)
benefits the most: ~5–7× tick speedup on a modern multicore CPU vs.
fully sequential.

### 9.1.1 Diff-Passing Discipline

Steps 9–11 produce **deltas**, not full recomputes. Each modification is
emitted as a `(record, ±1)` triple keyed by `Tick`:

```rust
enum HypergraphDelta {
    AtomAdded(AtomId),
    ClaimAdded(ClaimId),
    ClaimSuperseded(ClaimId, ClaimId),    // (old, new)
    StatusChanged(ClaimId, ClaimStatus),
    AtomStatsBumped(AtomId, MemoryStatsDelta),
    ClaimStatsBumped(ClaimId, MemoryStatsDelta),
}
```

Downstream consumers (cache materialization, salience updates, the
attention assembler in Step 13, the replay queue) consume deltas, not full
state recomputation. This is **differential dataflow** discipline (McSherry,
Murray, Isaacs et al., CIDR 2013) / **semi-naive Datalog evaluation**
(Bancilhon-Ramakrishnan 1986). We do not import the
`differential-dataflow` crate; we adopt the discipline.

### 9.2 Step 1 — Detect Intent

```rust
enum AttentionIntent {
    Statement,
    Question,
    Correction,
    Identity,
    TemporalUpdate,
    Brainstorming,
    Mixed(Box<[AttentionIntent]>),
}
```

Intent detection is a function over the input embedding + recent focus. It
does **not** branch on hard-coded keywords. (A v0 heuristic: punctuation +
embedding-similarity to a small bank of intent-prototype embeddings shipped in
the seed pack.)

### 9.3 Step 2 — Adjust Policy

PFC sets `vigilance`, `plasticity`, `merge_threshold`, etc. based on intent
(§8.3 table). The tick runs under the adjusted `Policy`.

### 9.4 Step 3 — Segment Text

Split into units: sentence, clause, quoted span, list item, code span,
entity-like span, time/value span. Each unit gets its own embedding;
units flow through the rest of the tick by value, not as stored
records.

### 9.5 Step 4 — Embed Units

Embed every unit from Step 4 plus the full tick — never one averaged
vector for the whole memory, because later questions target small
facts. The substrate is dimension-agnostic but the seed pack's
prototypes are dim-specific; swapping dimensions requires re-embedding
the seed.

### 9.6 Step 5a — Route Through Regions

Each embedding runs `route_regions(...)` (§8.2 Phase A) against the DAG.
This is **read-only** and parallelizes across embeddings via `par_iter`.
Outputs:

- a `RegionDelta` describing the proposed structural changes (region
  attachments, prototype updates, new regions, void counts)
- candidate concept archetypes surfaced during traversal
- candidate frames
- similar atoms (by region neighborhood)
- likely duplicate claims
- novelty score
- noise score

The `RegionDelta` is held until the mutation phase (Steps 9–11), where
`apply_region_delta(...)` runs under `&mut Hypergraph` and commits the
attachments, prototype updates, and new regions. This split is what
preserves the read-mostly-parallel / mutation-sequential boundary
(§9.1).

### 9.6.5 Step 5b — Activate Archetypes + Predict Next

Per-archetype scoring against active embeddings:

```text
for each archetype c in hypergraph.archetypes (par_iter):
  similarity = max(cosine(embedding, p.vector) for p in c.prototypes)
  if similarity >= policy.archetype_activation_threshold:
    c.activation = similarity
    active_archetypes.push(c.atom)
```

Embarrassingly parallel — `par_iter` over `Vec<Archetype>`; every archetype
scoring is independent. With 5K archetypes × 8 embeddings on a 12-core
laptop, this is ~1ms per tick after SIMD-friendly cosine.

For each active archetype, **predict-next** — using **only** the archetype's
own expected-slot priors and its `slot_transitions` matrix. Step 6
(extractors) hasn't run yet at Step 5b, so no slots are filled yet for
this tick. Step 5b's job is purely *prior-driven*: rank slots by
expected fill probability and emit a `Vec<SlotPrediction>` to bias
extractor attention.

```text
for each active archetype c:
  # No slots are filled yet — extractors run in Step 6.
  # Predict from the archetype's own priors only.
  for slot s in c.expected_slots, sorted by s.fill_probability desc:
    emit SlotPrediction { archetype: c, role: s.role, type_hint: s.type_hint,
                          confidence: s.fill_probability * c.activation }
```

The slot-transition matrix is used **after** extraction (Step 11) for
Hebbian learning — it learns the observed ordering of fills. It is
*not* used at Step 5b because the ordering can't be known until
extraction has happened.

The emitted predictions are a soft prior on which slot types the
extractors should look for. They do **not** override extractor output:
if the temporal parser doesn't see a date, the prediction was wrong
and the archetype's `expected_slots[i].fill_probability` gets a Hebbian
downweight in Step 11 (§13.9.2).

Active archetypes also stage their **endorsements**: each active
archetype's `vocabulary` (role atoms + claim templates) becomes
candidate boost weight for Step 13's `focused_claims` aggregation.
Multiple archetypes endorsing the same target compound RRF-style
(§9.13, §13.9).

### 9.7 Step 6 — Run Extractors

The v0 extractor stack (§12 details what's native vs ONNX):

- **NER** — spans for names/orgs/places. Biased by Step 5b archetype
  predictions toward expected slot types.
- **Temporal parser** — dates, weekdays, durations, relative times.
  Biased by Step 5b toward expected `time` / `from` / `to` slots.
- **Zero-shot relation extraction (`gline-rs` / GLiNER2)** — typed
  triples driven by active archetype slot expectations.
- **Heuristic relation extractor** — pattern-matched fallback for
  patterns GLiNER2 doesn't cover; driven by seed schemas (§7.3).
- **Heuristic coref** — recency-based: pronouns resolve to the
  most-recently-focused atom whose role matches.

All extractor output carries confidence and (where available) a
source pointer that flows into `Qualifiers.source` on the resulting
claims. Extractor proposals that satisfy active archetypes' expected
slots get a confidence bump.

v1 upgrade points: real SRL, real coref, dependency parser.

### 9.8 Step 7 — Coreference Scoring

Identity is conservative. Score:

```text
score =
  name_overlap
  + embedding_similarity
  + frame_overlap
  + role_overlap
  + temporal_compatibility
  + claim_support
  - contradiction_penalty
  - distinct_instance_penalty
```

Rules:

- Reuse concepts broadly.
- Reuse instances only with coreference support.
- Create provisional instances when uncertain.
- Replay merges provisional instances later if claim support
  accumulates.

Pattern separation (`separate_pattern`, ported from current Legend's dentate
gyrus) is the dampening function on the merge side: when two candidates are
close-but-distinct on a discriminating role, force them apart.

### 9.9 Step 8 — Build Claims and Events

Build compact base claims. Do not materialize the full entailment closure.

For:

```text
My dentist appointment with Dr. Rao changed from Tuesday to Friday.
```

Base atoms created or reused:

```text
user, Dr. Rao, dentist, appointment, appointment_1,
Tuesday, Friday, reschedule_event_1
```

Base claims:

```text
DrRao instance_of person                         [defeasible]
DrRao has_role dentist                           [asserted]
appointment_1 instance_of appointment            [entailed]
appointment_1 participant user                   [entailed]
appointment_1 provider DrRao                     [asserted]
appointment_1 domain dental                      [entailed]
reschedule_event_1 instance_of reschedule_event  [entailed]
reschedule_event_1 target appointment_1          [asserted]
reschedule_event_1 property date                 [asserted]
reschedule_event_1 from Tuesday                  [asserted]
reschedule_event_1 to Friday                     [asserted]
```

### 9.10 Steps 9–10 — Supersession and Cache

If a prior cache claim exists for `appointment_1 current_time`, mark it
`Superseded`, set its `superseded_by` to the new cache claim, and write the
new cache claim:

```text
appointment_1 current_time Friday   [Asserted, derived_from=reschedule_event_1]
```

Plus a history cache claim:

```text
appointment_1 old_time Tuesday      [Superseded, derived_from=reschedule_event_1]
```

Cache claims **always** carry `derived_from`. Invariant 14.

### 9.11 Step 11 — Hebbian + Salience + Archetype Updates

Co-activated atoms (members of the focus set) have their pairwise wiring
strengthened. Amygdala bumps salience for:

- exact values/times/persons
- corrections / contradictions
- user-stated preferences
- claims that just answered something

**Archetype-specific updates this step:**

- **Slot-fill learning.** For each active archetype, slots that the
  extractors actually filled this tick get their `fill_probability`
  Hebbian-bumped. Slots that were predicted but did not fill get a
  small downweight.
- **Slot-transition learning.** For each active archetype, the observed
  ordering of slot fills updates the `slot_transitions` matrix.
  Transitions that matched prediction strengthen; transitions that
  diverged weaken.
- **Vocabulary strength updates.** For each active archetype, vocabulary
  entries (role atoms and claim templates) whose target ended up in
  `focused_claims` get their `strength` Hebbian-bumped; entries whose
  target did not make focus get a small downweight. New candidate
  entries are *not* added here — that is a deliberate replay-time
  decision (§13.8) so the per-tick path stays fast and vocabulary
  growth stays disciplined.
- **Reliability tracking.** Archetypes whose vocabulary endorsements
  landed in the final `focused_claims` set get `reliability` bumped;
  archetypes whose endorsements did not survive aggregation get a small
  downweight.

### 9.12 Step 12 — Decay

Every atom not touched this tick has its `activation` decayed by
`policy.decay_rate`. Decay weakens **access paths**, never destroys
answer-bearing claims (Invariants 2, 11).

### 9.13 Step 13 — Assemble Attention State

```rust
struct ConsciousAttentionState {
    tick: Tick,
    intent: AttentionIntent,
    active_frame: Option<AtomId>,
    active_regions: Vec<RegionActivation>,
    active_archetypes: Vec<ArchetypeActivation>,
    focused_claims: Vec<ClaimActivation>,
    answer: Option<AnswerCandidate>,    // populated when input was answer-shaped
    supporting_claims: Vec<ClaimRef>,   // claims that backed the answer
    history: Vec<ClaimRef>,             // superseded claims relevant to focus
    uncertainty: Vec<UncertaintySignal>,
    durable_writes: Vec<AtomId>,        // what this tick added
    superseded: Vec<ClaimId>,           // what this tick demoted
    next_actions: Vec<AttentionAction>,
}
```

```rust
struct ArchetypeActivation {
    archetype: AtomId,
    activation: f32,                  // raw similarity-driven activation
    slots_filled: Vec<AtomId>,        // which slots got bound this tick
    slots_predicted_unfilled: Vec<AtomId>,  // slots archetype expected but extractors missed
    endorsed: Vec<(AtomId, f32)>,     // (target, contribution) vocabulary endorsements that landed in focus
}
```

**Aggregation.** `focused_claims` is computed as a weighted score over
extractor proposals + archetype vocabulary endorsements:

```text
for each candidate claim c:
  base_weight = c.stats.confidence * c.stats.salience
  vote_weight = 0
  for each active archetype a:
    # Role-atom endorsements: archetype expects this role to participate.
    for (role_atom, strength) in a.vocabulary.role_atoms:
      if claim_uses_role(c, role_atom):
        vote_weight += a.activation * strength * policy.archetype_voting_weight
    # Claim-template endorsements: this is one of the canonical shapes.
    for (template_id, strength) in a.vocabulary.claim_templates:
      if matches_template(c, template_id):
        vote_weight += a.activation * strength * policy.archetype_voting_weight
  focused_score(c) = base_weight + vote_weight

focused_claims = top-N by focused_score
```

Multiple archetypes endorsing the same claim compound (RRF-like).
Disagreement surfaces in `uncertainty` rather than collapsing into a
forced consensus.

**Properties:**

- *Always returned* — even a bare statement gets the current focus
  including `active_archetypes`, `active_regions`, and related history.
- *Answer is opportunistic* — derived from focused claims when input
  is question-shaped; absent otherwise.
- *History is first-class* — superseded claims relevant to focus
  surface separately ("the dental appointment *used to* be Tuesday").
- *Archetype endorsements are inspectable* — `active_archetypes[i].voted_for`
  carries per-claim provenance for which archetypes endorsed it.

### 9.14 Replay Enqueue

Heavy work — region split/merge, coref resolution, cache pruning, prototype
eviction — is enqueued for the replay thread (§13.8). The synchronous tick
stays fast.

---

## 10. There Is No Query

What was a "query" in v1 Legend is a tick whose input is question-shaped.

```text
When is my appointment at the dentist?
```

This is a tick. Same fourteen steps. Step 6 detects question-shape via the
QuestionAnswer schema. Step 13 populates `answer` from focused claims.

```text
Do I have anything Tuesday?
```

Same. The pipeline runs, the Tuesday concept activates, claims with Tuesday
in their roles are gathered, current ones are separated from superseded ones,
the answer surfaces in the attention state.

This collapses a real wart in v1 Legend (separate write/query paths, separate
return types, separate state coupling) and is the conceptual core of v2's
"every tick is a perception update."

---

## 11. Brain Processes As Functions

No modules. Each brain process is a function over the hypergraph + policy.
None of them own state of their own.

```rust
// Read-only (parallel-safe under &Hypergraph).
fn route_regions(input_embeddings: &[Vec<f32>], hg: &Hypergraph, p: &Policy) -> (Vec<ActiveRegion>, RegionDelta);
fn activate_archetypes(input_embeddings: &[Vec<f32>], hg: &Hypergraph, p: &Policy) -> Vec<ArchetypeActivation>;
fn predict_next(active: &[ArchetypeActivation], filled: &[AtomId], hg: &Hypergraph) -> Vec<SlotPrediction>;
fn separate_pattern(candidate: &Atom, neighbors: &[&Atom], p: &Policy) -> Decision;
fn score_salience(claim: &Claim, p: &Policy) -> f32;
fn detect_intent(input: &str, embeddings: &[Vec<f32>], recent: &VecDeque<AtomId>) -> AttentionIntent;
fn adjust_policy(intent: &AttentionIntent, base: &Policy) -> Policy;
fn aggregate_focus(candidates: &[ClaimCandidate], votes: &[ArchetypeActivation], p: &Policy) -> Vec<ClaimActivation>;

// Mutation (sequential, takes &mut Hypergraph).
fn apply_region_delta(hg: &mut Hypergraph, delta: RegionDelta);
fn reinforce_path(path: &[AtomId], hg: &mut Hypergraph);
fn update_archetypes(active: &[ArchetypeActivation], outcomes: &FocusOutcome, hg: &mut Hypergraph, p: &Policy);
fn decay_step(hg: &mut Hypergraph, p: &Policy);

// Background-thread (snapshot in, mutation list out).
fn replay(hg_snapshot: &HypergraphSnapshot, p: &Policy) -> Vec<ReplayMutation>;
```

### 11.1 Thalamus — `route_regions` + `apply_region_delta`

Read-only DAG routing produces `(ActiveRegion list, RegionDelta)`;
mutation commits the delta in the sequential phase. Full algorithm in
§8.2. Neither function decides semantic truth by labels.

### 11.2 Hippocampus — embedded in pipeline + `reinforce_path`

Preserves exact traces and separates close-but-distinct experiences. Used to
prevent over-merging:

```text
dentist appointment != body shop appointment
```

### 11.3 Dentate Gyrus — `separate_pattern`

Pattern separation as a callable function inside coreference scoring.
Re-derived from current Legend's `dentate_gyrus.rs`.

### 11.4 Amygdala — `score_salience`

Boost when:

- user states a preference,
- correction/supersession occurs,
- contradiction appears,
- exact value/time/person is present,
- answer-bearing fact is likely,
- retrieval previously depended on the item.

### 11.5 PFC — `detect_intent` + `adjust_policy`

High vigilance: precise question, identity decision, date/time answer,
correction, contradiction.

Low vigilance: broad search, concept discovery, brainstorming, background
consolidation.

### 11.6 Hebbian Learning

Co-activated memory citizens reinforce their connections. After each
tick: archetype vocabulary entries whose targets landed in
`focused_claims` get a small `strength` bump (§13.9.4); claims and
atoms on the focused path get a small `MemoryStats` bump via
`reinforce_path` (§13.6, §11.7).

### 11.7 Path-Aware Reinforcement — `reinforce_path`

If a path answers a query, reinforce the **exact path**:

```text
query -> appointment region -> dental frame -> appointment_1 -> current_time -> Friday
```

Not every nearby vector. Path-aware, not vector-aware. Current Legend's
basal-ganglia AdaGrad code is the reference.

### 11.8 Decay — `decay_step`

Decay reduces retrieval priority; v0 deletes nothing.

Decay targets:

- unused semantic-region links
- low-confidence inferred claims
- low-utility derived claims
- stale provisional instances
- noisy names
- weak access paths

Decay spares:

- value atoms with exact, durable content (times, ids, numbers)
- high-salience claims
- claims with answer success
- contradictions/corrections
- supersession history
- user preferences

### 11.9 Replay — `replay` (background thread)

Offline learning. See §13.8.

### 11.10 Archetypes — `activate_archetypes` + `predict_next` + `aggregate_focus` + `update_archetypes`

The predict-vote-learn cycle. All four are pure functions over the
hypergraph (or `&mut` for `update_archetypes`); all `par_iter`-friendly.
Algorithms in §9.6.5 (activate + predict), §9.13 (aggregate), §13.9
(updates). Both predict_next and the slot-transitions update follow the
Mountcastle / Hawkins canonical-cortical-circuit principle at low
fidelity: every archetype runs the same algorithm; what differs is what
they connect to.

---

## 12. Model Stack (Rust-Only, Pragmatic-Reuse)

No Python. No JVM. No managed runtimes. **Compiled C/C++ libraries linked
into the Rust binary are allowed** (e.g. ONNX Runtime via `ort`). **Pure-Rust
crates with mature ecosystems are preferred over from-scratch reimplementation
when the from-scratch version buys nothing.**

The rule: **own the data model and the substrate; use mature crates for the
well-defined inputs to it.**

### 12.1 The v0 Stack — Seven Components

1. **Tokenizer** — `tokenizers` 0.22 (HuggingFace's official Rust crate,
   Apache-2.0, no Python in the build path — the Python `tokenizers`
   package is just PyO3 bindings on top of this Rust crate). Reads
   `tokenizer.json` exports straight from any HF model card. Covers BPE,
   WordPiece, Unigram, byte-level BPE.
2. **ONNX Runtime via `ort`** — 2.0.0-rc.12+ (March 2026). Wraps ONNX
   Runtime 1.20+ via its C API. Compiled C++ library linked into our
   binary; not a managed-runtime sidecar. Same category as `lz4`.
3. **Embedding model — pinned for Legend's lifetime.**
   `bge-small-en-v1.5`, 384-dim, ~33 MB ONNX. Pooling: CLS token. Output
   L2-normalized. Query instruction: `"Represent this sentence for
   searching relevant passages: "` prepended to queries only; passages
   get no instruction.

   **Pin discipline.** Legend is built to behave like a C program: written
   once, runs for decades. The model artifact, the tokenizer vocab, the
   `ort` runtime, and the ONNX runtime native library are all vendored
   into the binary and never auto-updated. The tokenizer vocab hash is
   stamped into `ModelFingerprint` because vocab drift silently corrupts
   embeddings; the boot-time check (Inv 8) refuses to start on mismatch.

   Why this works: Legend stores the language *around* work — decisions,
   rationale, preferences — which general-purpose English embedders
   already handle well. Domain-specialized embedders (code, biomed) buy
   nothing here. There is no "stay current with SOTA" pressure.

   **A model swap is a deliberate, hard, one-time event.** Legend does
   not retain raw text or per-input records (§4.2) — the brain analogy
   says: distill, don't transcribe. So there is no in-place
   re-embedding migration. The only path on swap is to re-ingest from
   any `Qualifiers.source` pointers where the source is still reachable,
   and accept atom loss where it is not. Treat this as a one-way door
   at decision time. This is the right tradeoff for a tiny, fast
   hypergraph; it is the wrong tradeoff if Legend ever needs to be a
   transcript store. That role belongs elsewhere.

   Keep an `EmbeddingWrapper` interface that takes a typed
   `EmbedKind::{Query, Passage}` and applies the right prefix/pooling —
   useful for clean code organization and for the rare hypothetical
   re-ingest, not for runtime swaps.
4. **BM25 lexical index** — `tantivy` 0.25 (Lucene-grade, MIT, pure Rust,
   ~2× faster than Lucene in benchmarks). Mandatory for proper-noun /
   identifier / file-path retrieval that dense embeddings systematically
   underweight. Fallback if `tantivy` feels heavy: the `bm25` micro-crate.
5. **Temporal parser** — `chrono` + `chrono-english` for the easy 80%
   (date arithmetic, "next Tuesday", "two weeks ago") + a thin custom
   layer for grounding uncertainty (Tuesday is a weekday-concept, not a
   date until tied to a week). Carries grounding uncertainty.
6. **NER + zero-shot relation extraction** — `gline-rs` (pure Rust on
   `ort`, GLiNER and GLiNER2 models). Zero-shot: pass entity-type strings
   at inference, no fine-tuning. ~130–208 ms/call across 5–50 labels;
   ~4× faster than Python GLiNER on CPU. Collapses what was previously
   "ONNX'd NER + from-scratch BIO decoder + heuristic relation extractor"
   into one component.
7. **Heuristic coreference** — write from scratch in Rust. Recency-based:
   pronouns resolve to the most-recently-focused atom whose role matches.
   Defensible per Centering Theory + Hobbs' algorithm baselines (within
   5–8 F1 of neural ECR within-document). v0 only.

### 12.2 What We Drop In v0

- **OpenIE.** Stanford CoreNLP is JVM-only; no Rust port; SRL covers the
  same ground better when we add it.
- **AMR / UMR.** No portable implementation; not worth writing from scratch.
- **Cross-encoder reranker.** The substrate's path-aware reinforcement is
  the reranker.
- **Dependency parser.** Not on the §14 walkthrough's critical path.
- **From-scratch tokenizer / BM25 / NER+BIO decoder.** Pure-Rust mature
  crates exist (`tokenizers`, `tantivy`, `gline-rs`); writing our own buys
  nothing in 2026.

### 12.3 v1 Upgrade Points

- **SRL** — only if GLiNER2's relation extraction proves insufficient on
  general corpora. ONNX'd transformer + from-scratch BIO/role decoder.
- **Real coreference** — `anno` (Rust IE crate that already wires GLiNER +
  coref + relation) or ONNX-export of `maverick-coref` (170× faster than
  prior SOTA, 2024). Don't reimplement fastcoref's decoder from scratch.
- **Dependency parser** — only if SRL alone doesn't carry. Biaffine parser
  via ONNX + Eisner / Chu-Liu-Edmonds decoder.
- **(Removed.)** No embedding-model upgrade is planned. The embedder is
  pinned for Legend's lifetime (§12.1). If ever changed, it's a one-time
  offline migration, not a v1 plan.

### 12.4 Optional v1 Extension — Local LLM as Unified Extractor

Tempting (Qwen2.5-0.5B-Instruct via `ort` covers NER + relation + temporal +
coref in one model) but for v0 GLiNER's deterministic BIO output is
testable bit-for-bit; LLM JSON output isn't (sampling is non-deterministic
across hardware even at temperature=0). **Keep GLiNER as the deterministic
extractor; add Qwen2.5-0.5B as a v1 fallback for inputs GLiNER's schema
doesn't cover.** Tag any claim it produces with a lower base-confidence.
This matches the SMALLM hybrid pattern (2025).

### 12.5 Storage Quantization

- **v0:** store FP32 embeddings in the hypergraph. INT8-quantize the
  embedding *model* (`BGESmallENV15Q` in fastembed-rs) for inference
  latency only. 2.7–3.4× faster CPU inference at 1–3% MTEB drop.
- **v1:** introduce stored INT8 vectors with FP32 rescore for top-K.
  Match Snowflake's "128 bytes/vec" target as the design point.

Test before committing: e5-base-v2 specifically suffers "dimension
collapse" under quantization (Vespa). Calibrate against a held-out set of
seed prototypes; require ≤ 2% recall@10 drop.

### 12.6 Honest Estimates (Revised)

Solo developer, evenings/weekends:

| Component | Estimate |
|---|---|
| `tokenizers` integration + golden-vector tests | ~0.25 wk |
| `ort` integration + BGE-small round-trip | ~2 wk |
| `tantivy` integration + Legend's index schema | ~0.5 wk |
| Temporal parser (`chrono-english` + uncertainty layer) | ~2.5 wk |
| `gline-rs` integration + zero-shot relation calls | ~0.5 wk |
| Heuristic coref | ~1 wk |
| **v0 model-stack total** | **~7 wk** |

Plus substrate, seed pack, pipeline, replay (§16): ~10–12 wk additional.

Realistic v0 horizon: **~10–12 wk part-time**, **~3–4 wk full-time**. The
prior estimate (~21 wk model stack, ~23 wk substrate) was correct *in
2023*; the 2026 Rust ecosystem makes it ~3× too pessimistic.

v1 (SRL if needed + real coref via `anno` + MRL embeddings) adds another
**~3 months part-time**.

---

## 13. Algorithms

The mutating algorithms in this section — region insertion, archetype
emergence, supersession, replay — are all instances of structured
**graph rewriting** over the typed hypergraph (§4.0 (1)). Each rule
has three parts: a *match* pattern (what to look for), an *interface*
(what to keep unchanged), and a *replacement* (what to write in
place). When rules are written this way, there is a known result
about *which* rule sets give the same final hypergraph regardless of
the order rules are applied. That result is what makes replay's
mutations deterministic (§13.8) and lets the parallel match-only
phases (§9 read-mostly window) be order-free. We do not import a
graph-rewriting library; we adopt the discipline.

Replay's claim-weight updates (§13.8, §13.9.4) are online updates to
the weighted-formula model from §4.0 (5) — strengthen patterns that
fired together; weaken patterns the world contradicted.

### 13.1 Online Region Insertion

The §8.2 algorithm is **DDVFA-derived** (Brito da Silva et al. 2019; see
§17 for the literature trail). DDVFA already provides what we need:
two-level vigilance, multi-prototype F2 nodes, and a Merge ART module
for input-order robustness. **Read DDVFA before writing v0 region
code** — saves weeks of reinvention.

Legend-specific deltas on top of DDVFA:

- **Weighted DAG region topology with multi-parent attachment.** No
  published online-clustering algorithm produces this; every cataloged
  one yields strict trees or flat partitions. This is where Legend
  earns its keep — eval (§15) specifically measures whether multi-
  parent attachment improves polysemy handling over a strict-tree
  ablation.
- "Facts don't merge" (Invariant 11) — enforced at the data-model level,
  not in clustering.
- Replay-driven split (§13.8).
- The Void sink for sub-threshold inputs.
- Cosine prototype updates (spherical k-means), not Fuzzy ART complement
  coding.

### 13.2 Multi-Prototype Clustering

Up to 8 prototypes per region. When a 9th would be added, replay decides
whether to split the region or evict the lowest-weight prototype.

### 13.3 Conservative Coreference

Candidate scoring (§9.8). Pattern separation as the dampening function on the
merge side. When uncertain, create separate provisional instances and link
them as possible coreference candidates; replay resolves later.

### 13.4 Event Reification (Event-Calculus-Style Fluent Update)

§4.5 introduced events as atoms; this section names the algorithm.
Legend's update protocol is **Event Calculus** (Kowalski & Sergot 1986;
Shanahan's "The Event Calculus Explained" for the modern formulation),
with the events-initiate-fluents mapping made structural:

| Event Calculus | Legend |
|---|---|
| `Happens(e, t)` | event atom asserted at tick t |
| `Initiates(e, f, t)` | new current-state claim with `derived_from = e` |
| `Terminates(e, f, t)` | prior current-state claim marked `Superseded` |
| `HoldsAt(f, t)` | walk supersession chain to find non-Superseded leaf |

This is a 40-year-old logical foundation; adopt the vocabulary, don't
reinvent under different names. Role bindings (`target`, `property`,
`from`, `to`) follow the standard treatment of events as objects with
named role-fillers (Parsons 1990; Davidson 1967) — the same pattern
W3C's N-ary Relations Note (2006) and the FrameNet / FrameBase
tradition use.

### 13.5 Claim Materialization Policy

Driven by Invariant 2 (no duplication, no fluff). Every claim must earn
its bytes.

Store:

- asserted base claims
- high-confidence entailed claims that are answer-bearing
- current-state cache claims (with `derived_from`)
- supersession links
- `Qualifiers.source` on claims that came with an external pointer

Do not store:

- raw or normalized text (lives in the dev-only WAL, never in the
  hypergraph)
- per-input audit records (no `Evidence` citizen — see §4.2)
- paraphrases of existing claims
- weak implications
- speculative role assumptions
- any field that is derivable from another field on demand

Derived claims are computed on the fly or materialized during replay
when they prove answer-bearing. This is **incremental view maintenance**
(Gupta & Mumick 1995) applied to the claim graph; cache claims are
self-maintainable views (Quass et al., VLDB 1996) refreshable from the
event chain without re-querying base data.

### 13.6 Path-Aware Reinforcement

When a tick produces an answer, bump `MemoryStats` along the **exact
path** that produced it — every atom *and every claim* on the path:

- query embedding region (atom)
- matched concept atoms in that region
- selected claims (the hyperedges themselves)
- selected instance (atom)
- selected archetype (atom)
- region-to-claim edges and frame/time qualifier path

Not nearby alternatives. Claims and atoms reinforce uniformly; the
path-aware discipline is what keeps memory durable for things that
actually answered something, instead of merely things that sit near
the answer in vector space.

### 13.7 Utility-Based Decay

Decay applies to atoms and claims uniformly — both carry `MemoryStats`,
both compute utility the same way:

```text
utility =
  answer_success
  + support_count
  + salience
  + exact_value_bonus
  + correction_or_contradiction_bonus
  + source_quality
  - noise_score
  - redundancy
  - age_without_access
```

Decay weakens access paths first (the cheapest reversible move). A
heavily decayed claim becomes harder to retrieve but is not deleted in
v0; superseded claims are kept and walked via the supersession chain.
Atom and claim deletion is a separate retention policy and is not
implemented in v0.

### 13.8 Replay (Background Thread)

Replay runs on a background thread under the snapshot/message-passing
protocol (§5.4). Replay jobs:

- split high-variance regions
- merge duplicate regions
- merge duplicate archetypes
- **emerge new archetypes from clustered claim patterns** — when a
  sub-region shows consistent slot-fill patterns across many ticks
  (e.g. a recurring `state_with_temporal_value` configuration with
  consistent provider/participant slots), replay can split out a
  learned archetype. This is how `appointment` (or `function_definition`,
  or `character`) emerges in a domain-specialized Legend instance from
  purely seed domain-neutral starting archetypes.
- resolve provisional coreference
- compact redundant claims
- materialize useful derived claims
- demote unused derived claims
- evict prototypes when a region exceeds 8
- prune low-utility archetypes (low `support_count`, low `reliability`)
- merge functionally-equivalent archetypes (high vocabulary overlap +
  high prototype similarity)
- **prune low-strength vocabulary entries** — for each archetype, drop
  vocabulary entries whose `strength` is below
  `policy.archetype_vocabulary_min_strength`. This is what keeps each
  archetype's vocabulary a small canonical set without a hard cap
  (§13.9.4).
- **promote co-occurring vocabulary candidates** — when an archetype's
  active ticks consistently include a role atom or claim template that
  isn't yet in its vocabulary, replay adds it (with low initial
  strength); subsequent ticks will reinforce or prune.

**Replay must be benchmark-aware:** any candidate compression is rejected if
it would break recall on the §14 walkthrough.

### 13.9 Archetype Dynamics

This subsection consolidates the per-tick archetype update rules referenced
across §4.3, §9.6.5, §9.11, §11.10. All updates are bounded
(plasticity-modulated) Hebbian with explicit decay.

#### 13.9.1 Activation

```text
for archetype c, embedding e:
  sim = max(cosine(e, p.vector) for p in c.prototypes)
  if sim >= policy.archetype_activation_threshold:
    c.activation = sim
    active.push(c)
```

Cosine on unit-normalized vectors only (§8.7). Multi-prototype: the max
across prototypes is the archetype's activation, not the mean.

#### 13.9.2 Slot satisfaction

For each active archetype `c` and each extractor proposal `p`:

```text
if p.role matches some slot s in c.expected_slots:
  if p.type_hint is compatible with s.type_hint or s.type_hint is None:
    c.slots_filled.push(s.role)
    p.confidence *= (1 + policy.slot_prediction_bias * c.activation)
    s.fill_probability ← bounded_hebbian_bump(s.fill_probability)
```

Slots predicted but not filled get a small downweight on
`fill_probability` (Rescorla-Wagner style):

```text
delta = policy.archetype_plasticity * (-prediction_error)
```

where `prediction_error = expected_fill_rate - observed_fill_rate`.

#### 13.9.3 Sequence prediction

```text
for archetype c with active slot fills [s_a, s_b, s_c, ...] (in order):
  for each consecutive pair (s_i, s_j):
    c.slot_transitions.matrix[s_i][s_j] ← bounded_hebbian_bump(...)
  for each non-observed transition (s_i, s_k) where s_k was predicted:
    c.slot_transitions.matrix[s_i][s_k] ← bounded_hebbian_decay(...)
```

The matrix rows re-normalize to sum to 1.0 after updates so they remain
proper probability distributions.

#### 13.9.4 Vocabulary endorsement and updates

When an active archetype `c` has a vocabulary entry pointing at a role
atom or claim template that ended up in `focused_claims`:

```text
c.reliability += policy.archetype_plasticity * (1 - c.reliability)
c.vocabulary[entry].strength ← bounded_hebbian_bump
```

When a vocabulary entry's target did not make `focused_claims`:

```text
c.reliability *= (1 - policy.archetype_plasticity * 0.3)
c.vocabulary[entry].strength ← bounded_hebbian_decay
```

When a new role atom or claim template appears in `focused_claims`
that this archetype activated for, replay (§13.8) considers adding it
as a new vocabulary entry on the next pass — vocabulary growth is a
deliberate replay-time decision, not a free per-tick write, so the
vocabulary stays a *small canonical set* rather than a sprawling
weighted graph.

Replay also prunes vocabulary entries whose `strength` has decayed
below `policy.archetype_vocabulary_min_strength`, keeping the
"structured vocabulary" property over time without a hard cap.

#### 13.9.5 Plasticity decay

A archetype's `plasticity` decays slowly with `support_count`:

```text
c.plasticity = initial_plasticity / (1 + log(1 + c.support_count))
```

Mature archetypes become harder to perturb. New archetypes (low
`support_count`) update fast.

#### 13.9.6 Bounded Hebbian operators

All updates use bounded operators that prevent runaway growth/collapse:

```text
bounded_hebbian_bump(x, rate=policy.archetype_plasticity):
  return x + rate * (1 - x)        # asymptotes to 1.0

bounded_hebbian_decay(x, rate=policy.archetype_plasticity * 0.3):
  return x * (1 - rate)             # asymptotes to 0.0
```

This is the standard Oja-rule-derived bounded Hebbian. Prevents
prototype/wiring/transition values from leaving [0, 1].

---

## 14. Ten-Tick Conformance Walkthrough

This is the executable conformance fixture. Each tick's expected output is
both the returned `ConsciousAttentionState` and the hypergraph delta (atoms
added, claims added, claims superseded). The inspection harness (§16) diffs
both.

### Tick 1

Input:

```text
My dentist appointment with Dr. Rao changed from Tuesday to Friday.
```

**Active seed archetypes this tick** (from §7.4 — none of these is
domain-specific):

```text
ARCHETYPE_CHANGE_EVENT          activation 0.92
  predicted slots: target, from, to (top transition)
  filled this tick: target=appointment_1, from=Tuesday, to=Friday
ARCHETYPE_ENTITY_MENTION        activation 0.88
  filled: name=DrRao
ARCHETYPE_TEMPORAL_EXPRESSION   activation 0.81
  filled: kind=weekday, instances=[Tuesday, Friday]
ARCHETYPE_STATE_WITH_TEMPORAL_VALUE  activation 0.74
  filled: subject=appointment_1, time_scope=Friday
ARCHETYPE_REFERENCE_CHAIN       activation 0.62
  filled: mention="my dentist appointment", antecedent=null (first mention)
```

Hypergraph delta:

```text
added atoms:       user, Dr. Rao (DrRao), dentist, appointment,
                   appointment_1, Tuesday, Friday, reschedule_event_1
added claims:
  DrRao has_role dentist                          [Asserted, e1]
  appointment_1 instance_of appointment           [Entailed, e1]
  appointment_1 provider DrRao                    [Asserted, e1]
  appointment_1 participant user                  [Entailed, e1]
  reschedule_event_1 target appointment_1         [Asserted, e1]
  reschedule_event_1 property date                [Asserted, e1]
  reschedule_event_1 from Tuesday                 [Asserted, e1]
  reschedule_event_1 to Friday                    [Asserted, e1]
  appointment_1 current_time Friday               [Asserted, derived_from=reschedule_event_1, e1]
  appointment_1 old_time Tuesday                  [Superseded, derived_from=reschedule_event_1, e1]
archetype updates:
  ARCHETYPE_CHANGE_EVENT.slot_transitions[from][to] strengthened
  ARCHETYPE_CHANGE_EVENT.vocabulary[TEMPLATE_CHANGE_EVENT].strength bumped
  ARCHETYPE_CHANGE_EVENT.vocabulary[ROLE_FROM, ROLE_TO].strength bumped
  ARCHETYPE_TEMPORAL_EXPRESSION.vocabulary[ROLE_FROM, ROLE_TO].strength bumped
  reliability bumped on both archetypes (their endorsements landed in focus)
```

Returned `ConsciousAttentionState`:

```text
intent: Statement
active_frame: user_schedule
active_regions: appointments, dental_appointments
active_archetypes:
  ARCHETYPE_CHANGE_EVENT             voted for: reschedule_event_1, appointment_1
  ARCHETYPE_ENTITY_MENTION           voted for: DrRao
  ARCHETYPE_TEMPORAL_EXPRESSION      voted for: Tuesday, Friday
  ARCHETYPE_STATE_WITH_TEMPORAL_VALUE voted for: appointment_1 current_time Friday
focused_claims:
  appointment_1 current_time Friday    (boosted by 2 archetype endorsements)
  reschedule_event_1 from Tuesday
  reschedule_event_1 to Friday
answer: None
durable_writes: e1, appointment_1, reschedule_event_1
next_actions: watch for future corrections to appointment_1
```

Note: `appointment` here is a *learned* atom emerged from this tick's
extractor proposals, not a seed archetype. The seed pack ships only the
mechanical predictors listed above — `ARCHETYPE_CHANGE_EVENT` etc. — none
of which presume the appointment domain.

### Tick 2

Input:

```text
I have an appointment at the body shop on Tuesday.
```

Delta:

```text
reused atoms:      user, appointment, Tuesday
added atoms:       appointment_2, body_shop_1
added claims:
  appointment_2 instance_of appointment           [Asserted, e2]
  appointment_2 participant user                  [Entailed, e2]
  appointment_2 location_or_provider body_shop_1  [Asserted, e2]
  appointment_2 current_time Tuesday              [Asserted, e2]
```

Critical: do **not** merge `appointment_1` and `appointment_2`. Pattern
separation fires on the discriminating role (`provider` vs
`location_or_provider`).

Returned state:

```text
intent: Statement
active_frame: user_schedule
focused_claims:
  appointment_2 current_time Tuesday
  appointment_2 location_or_provider body_shop_1
answer: None
uncertainty: exact calendar date for Tuesday is unknown
```

### Tick 3

Input:

```text
When is my appointment at the dentist?
```

Delta:

```text
no new atoms or claims (read-shaped tick)
reinforced path: query -> appointments -> dental_appointments
                 -> DrRao -> appointment_1 -> current_time -> Friday
```

Returned state:

```text
intent: Question
active_frame: user_schedule
active_regions: appointments, dental_appointments
focused_claims:
  appointment_1 current_time Friday
answer: Friday
uncertainty: exact calendar date unknown
```

### Tick 4

Input:

```text
What do I have on Tuesday?
```

Delta:

```text
no new atoms or claims
reinforced path: query -> Tuesday -> [filter current] -> appointment_2
```

Returned state:

```text
intent: Question
focused_claims:
  appointment_2 current_time Tuesday
answer: body shop appointment
history:
  appointment_1 old_time Tuesday  [Superseded — for context only]
```

### Tick 5

Input:

```text
Actually, the dentist moved it again to Monday.
```

Delta:

```text
coref: "it" -> appointment_1 (most-recently-focused with dentist context)
added atom:       Monday, reschedule_event_2
added claims:
  reschedule_event_2 target appointment_1         [Asserted, e5]
  reschedule_event_2 from Friday                  [Asserted, e5]
  reschedule_event_2 to Monday                    [Asserted, e5]
  appointment_1 current_time Monday               [Asserted, derived_from=reschedule_event_2, e5]
superseded claims:
  appointment_1 current_time Friday               [Superseded by current_time Monday]
added claim:
  appointment_1 previous_time Friday              [Superseded, derived_from=reschedule_event_2, e5]
preserved: appointment_1 old_time Tuesday (already Superseded)
```

**Active archetypes this tick:**

```text
ARCHETYPE_CHANGE_EVENT          activation 0.94 (highest yet — its
                             expected_slots fill_probabilities were
                             strengthened on Tick 1)
  predicted: target slot, from slot, to slot
  filled: target=appointment_1 (via coref), from=Friday, to=Monday
ARCHETYPE_REFERENCE_CHAIN       activation 0.79
  filled: mention="it", antecedent=appointment_1 (from recent_focus)
ARCHETYPE_ENTITY_MENTION        activation 0.71  (the dentist cue)
ARCHETYPE_TEMPORAL_EXPRESSION   activation 0.83
```

`ARCHETYPE_CHANGE_EVENT`'s `expected_slots[from].fill_probability` and
`expected_slots[to].fill_probability` rose during Tick 1's Step 12, so
Step 5b on Tick 5 emits stronger priors for both slots than it would
on a cold start. Extractor attention is biased accordingly. The
`slot_transitions` matrix is also stronger now, but it only fires at
this tick's Step 12 to update *future* learning — not at Step 5b
(extractors haven't run yet).

Returned state:

```text
intent: Correction
focused_claims:
  appointment_1 current_time Monday        (boosted by ARCHETYPE_CHANGE_EVENT
                                             + ARCHETYPE_STATE_WITH_TEMPORAL_VALUE votes)
  appointment_1 previous_time Friday  [Superseded]
answer: None
uncertainty: "it" resolved to dentist appointment via recent focus + dentist cue
             ARCHETYPE_REFERENCE_CHAIN flagged ambiguity-then-resolved
```

### Tick 6

Input:

```text
When is my appointment with Dr. Rao now?
```

Delta:

```text
no new atoms or claims
reinforced path: query -> DrRao -> appointment_1 -> current_time -> Monday
```

Returned state:

```text
intent: Question
focused_claims:
  appointment_1 current_time Monday
answer: Monday
history:
  appointment_1 old_time Tuesday
  appointment_1 previous_time Friday
```

### Tick 7

Input:

```text
The body shop appointment is for an oil leak.
```

Delta:

```text
coref: "the body shop appointment" -> appointment_2
added atom:       oil_leak
added claim:
  appointment_2 purpose oil_leak                  [Asserted, e7]
```

Returned state:

```text
intent: Statement
focused_claims:
  appointment_2 purpose oil_leak
answer: None
```

### Tick 8

Input:

```text
Why am I going to the body shop?
```

Delta:

```text
no new atoms or claims
reinforced path: query -> body_shop -> appointment_2 -> purpose -> oil_leak
```

Returned state:

```text
intent: Question
focused_claims:
  appointment_2 purpose oil_leak
answer: oil leak
```

### Tick 9

Input:

```text
Dr. Rao is my dentist.
```

Delta:

```text
matched existing: DrRao archetype
no new atoms (do not create a new DrRao instance)
reinforced claim: DrRao has_role dentist (incremented confidence + answer_success)
reinforced path: user -> dentist -> DrRao
```

Returned state:

```text
intent: Statement
focused_claims:
  DrRao has_role dentist
  user has_dentist DrRao  [Entailed]
reinforced: DrRao archetype, dentist relationship
answer: None
```

### Tick 10

Input:

```text
What appointments do I have?
```

Delta:

```text
no new atoms or claims
gathered: appointment_1, appointment_2
filtered: current non-Retracted, non-Superseded current_time claims
```

**Active archetypes this tick:**

```text
ARCHETYPE_QUESTION              activation 0.86
  predicted: expected_answer_kind = enumeration over appointment-typed atoms
ARCHETYPE_AGGREGATION           activation 0.74
  predicted: count/list over a typed set
ARCHETYPE_STATE_WITH_TEMPORAL_VALUE  activation 0.69
  votes for: appointment_1.current_time, appointment_2.current_time
ARCHETYPE_ENUMERATION           activation 0.55
```

After 10 ticks the wiring graph has settled enough that
`ARCHETYPE_QUESTION` + `ARCHETYPE_AGGREGATION` co-activating reliably retrieves
the right superset of claims. By Tick ~50 the aggregator-style query
path will be fast and direct.

Returned state:

```text
intent: Question
focused_claims:
  appointment_1 current_time Monday        (3 archetype endorsements)
  appointment_1 provider DrRao             (1 archetype endorsement)
  appointment_2 current_time Tuesday       (3 archetype endorsements)
  appointment_2 purpose oil_leak           (1 archetype endorsement)
answer:
  - dentist appointment with Dr. Rao: Monday
  - body shop appointment: Tuesday, for an oil leak
supporting_claims: appointment_1.current_time, appointment_2.current_time, appointment_2.purpose
uncertainty: exact calendar dates unknown unless Monday/Tuesday were grounded
```

This walkthrough is the **first conformance fixture**. The inspection
harness (§16) asserts the returned attention state, the internal
hypergraph state, *and* the active-archetype trace after each tick.

---

## 15. Evaluation

### 15.1 Co-Primary Metrics

The 2025 consensus stack: recall + faithfulness + abstention. v0 metric
floor is the first three:

1. **Claim recall@k.** If the answer-bearing claim is not retrieved,
   the system failed before any reader gets involved. (Benchmarks that
   ship "gold evidence" annotations are mapped onto the corresponding
   claims in Legend's hypergraph.)
2. **Update / supersession accuracy.** When a fact is superseded across
   ticks, does the system answer with the *current* (post-update) fact?
   This is what `MemoryAgentBench FactConsolidation` directly measures and
   what `LongMemEval`'s `knowledge-update` slice tests.
3. **Abstention recall.** When the answer isn't in memory, does the
   system correctly say "I don't know" instead of hallucinating? Tested by
   `LongMemEval` `*_abs` variants and `AbstentionBench`.

### 15.2 Secondary Metrics

- answer accuracy on grounded questions
- temporal accuracy (current vs historical disambiguation)
- instance-separation accuracy (no false merges across name collisions)
- compression safety (replay does not break recall on the §14 walkthrough)
- retrieval path stability across reruns
- faithfulness / unsupported-claim rate (deferred — needs an LLM judge)

### 15.3 v0 Evaluation Gates

Three benchmarks adopted as v0 evaluation gates:

1. **§14 ten-tick walkthrough** — the conformance fixture. Hypergraph +
   attention state must match the predicted deltas exactly.
2. **LongMemEval** (Wang et al., ICLR 2025; arXiv 2410.10813; MIT) —
   `longmemeval_oracle.json` (gold-evidence-only) is the first run.
   `longmemeval_s_cleaned.json` (~115k tokens) once routing stabilizes.
   Categories Legend should pass first: `single-session-*`,
   `knowledge-update`, `temporal-reasoning`, `*_abs` abstention.
   Categories that will lag in v0: `multi-session` aggregation,
   `single-session-preference` (until a `Preference` schema lands in the
   seed pack).
3. **MemoryAgentBench FactConsolidation** (HUST-AI, ICLR 2026; HF
   `ai-hyz/MemoryAgentBench`) — single-hop and multi-hop counterfactual
   updates. Structurally identical to Legend's supersession semantics.
   Multiple-choice format means string-match scoring suffices.

### 15.4 Smoke-Test Benchmark (Embedding / Routing)

**RULER MK-NIAH and MV-NIAH at 8K and 32K** (NVIDIA, COLM 2024;
arXiv 2404.06654; Apache 2.0). Synthetic, deterministic, license-clean.
Use as embedding/region-routing smoke test in CI. Failures here mean §8
routing is broken before you even touch memory.

### 15.5 Custom Conformance Fixtures (Companions to §14)

Three more, each ~15 minutes to author in §14 format:

1. **"Two Sarahs" — instance separation.** Two entities with identical
   name, divergent attributes (e.g. Sarah the teacher vs Sarah Chen the
   nurse). Asserts pattern separation fires on at least one role mismatch.
   Catches over-merging.
2. **"Forgotten correction" — supersession blindness.** Three reschedule
   events on the same appointment over 20 ticks of unrelated content.
   Asserts the answer reflects the *third* time, not the most recent
   surface text. Catches recency-without-supersession.
3. **"Frame drift" — frame disambiguation.** User asks "do I have anything
   Tuesday?" then 30 ticks later asks "was Tuesday on the schedule?"
   referring to a *past* week. Asserts `active_frame` switches from
   `user_schedule:current` to `user_schedule:historical`. Catches frame
   collapse.

§14 + these three are the v0 conformance gate. LongMemEval +
MemoryAgentBench are the v0 generalization gate.

### 15.6 Benchmarks We're Not Adopting

- **LoCoMo** (Snap, 2024) — documented scoring controversy (Zep's re-eval
  shows reported 84% scores drop to ~58% after fixes). Skip.
- **MTEB / BEIR** — only when the embedding model swap is on the table.
- **SOTOPIA** — interactive LLM-vs-LLM rollouts, not memory-shaped.
- **NIAH variants beyond RULER** — RULER subsumes them.

### 15.7 What "Compression Safety" Means

LongMemEval-style conservation test:

```text
Can the compressed memory still recover the answer-bearing fact and the
claim path that supports it?
```

Replay is benchmark-aware (§13.8): any candidate replay mutation that
would break recall on the §14 walkthrough is rejected before it lands.

The §14 walkthrough is the v0 conformance gate. A second-domain test
(MemoryAgentBench EventQA, codebase corpus, or chat corpus) is the v0
sign-off gate (§16 step 10).

---

## 16. Build Order

Solo coder with Claude as reviewer. Every step's done-criterion is the
inspection-harness diff: hypergraph + attention state after each tick
must match the predicted state. Spec sections in parens are the source
of truth; this section gives sequence and gates only.

### Step 0 — Foundation Infrastructure (~1 wk)

**Build:** Add v0 crates (`ort`, `tokenizers`, `tantivy`, `gline-rs`,
`chrono-english`, `rayon`, `hashbrown`, `lz4`, `rmp-serde`, `serde`).
Round-trip BGE-small via `EmbeddingWrapper` (§12.1 #3) against a
`sentence-transformers` parity oracle. Wire the inspection harness
(serialize → deserialize → pretty-print, including region-proliferation
over time per §8.8).
**Done:** bit-identical round-trip; embedding parity; harness prints
region creation rate.

### Step 1 — Substrate (~2 wk)

**Build:** §4 + §5 types + indices + supersession-chain walk.
**Done:** 50-atom round-trip; chains walk both directions;
debug-asserts fire on cache-claims-without-`derived_from` (Inv 14) and
on snapshot/log without `ModelFingerprint` (Inv 8).

### Step 2 — Snapshot + Bounded WAL (~1 wk)

**Build:** §5.7 — segmented WAL (1 MB segments, LZ4 hot, zstd-19 closed,
10 MB cap with oldest-segment eviction; `LEGEND_WAL_UNBOUNDED=1` for dev
builds), snapshot serializer stamped with `Tick` + `ModelFingerprint`,
hybrid checkpoint (N=1000 ∨ S=5 MB ∨ T=1 hr), boot-time fingerprint
check that refuses startup on mismatch.
**Done:** crash mid-corpus → restart → state matches; post-checkpoint
WAL truncation works; binary built against a different model refuses to
boot against an existing snapshot.

### Step 3 — Seed Pack (~2.5 wk)

**Build:** Hand-author §7's 16 regions + 11 schemas + **30 domain-neutral
archetypes** + Genesis/Void + ~12 Roles + ~8 Frames. Embed at boot.
Serialize as `seed_v0.msgpack.lz4`.
**Done:** boot shows ~80 atoms in expected configuration; 2D projection
of descriptor embeddings clusters sensibly; no archetype has a domain-
specific label.

### Step 4 — Manual Ten-Tick Test (~1 wk)

**Build:** Hard-code §14 via direct `add_atom`/`add_claim` (no NLP).
**Done:** §14 walkthrough passes; `ConsciousAttentionState` shape is
right.

### Step 5 — Embeddings + Region Routing (~1.5 wk)

**Build:** §8.2 — `route_regions` (read-only, parallel, top-k DAG) +
`apply_region_delta` (spherical k-means, §8.7). Diff-passing discipline
(§9.1.1).
**Done:** every span lands in the expected region; multi-prototype
bounded at 8; region-creation rate decays after first 20 ticks.

### Step 5.5 — Archetype Activation + Predict-Next (~1.5 wk)

**Build:** §11.10 + §9.6.5 + §9.13 — `activate_archetypes` (par_iter),
`predict_next` (slot priors only — Step 5b runs before extraction),
`aggregate_focus` (RRF-like).
**Done:** §14 Tick 1 activates `CHANGE_EVENT` / `TEMPORAL_EXPRESSION` /
`ENTITY_MENTION` with expected slot predictions; archetype endorsements appear
in `focused_claims` provenance; tick latency <5 ms for 50-atom seed
pack.

### Step 5.7 — Archetype Dynamics (~1.5 wk)

**Build:** §13.9 — `update_archetypes` (bounded Hebbian on expected slots,
slot transitions, vocabulary endorsements, reliability) + plasticity
decay.
**Done:** across §14's 10 ticks, `slot_transitions[from][to]`
strengthens monotonically; mature plasticity decays; vocabulary
strengths track repeated co-occurrence.

### Step 6 — Temporal Parser + NER + Relation Extraction (~2 wk)

**Build:** `chrono-english` for the 80% + thin uncertainty-grounding
layer; `gline-rs` zero-shot NER + relations.
**Done:** Tick 1 emits `Tuesday`, `Friday`, `DrRao`, and the reschedule
triple without hand-coding.

### Step 7 — Event Reification + Supersession Cache (~1.5 wk)

**Build:** §13.4 Event Calculus mapping; supersession chains; cache
claims with `derived_from`; auto-`ClaimRank` derivation.
**Done:** Ticks 1/2/5/7 build correct events; chain
`Tuesday → Friday → Monday` walks both directions.

### Step 8 — Heuristic Coreference + Conservative Instances (~1 wk)

**Build:** §9.8 — recency-based pronoun resolution (Centering Theory
baseline) + pattern separation.
**Done:** Tick 5 "it" → `appointment_1`; `appointment_1` and
`appointment_2` stay separate; Tick 9 reinforces `DrRao` instead of
duplicating. §14 + the three §15.5 fixtures pass end-to-end.

### Step 9 — Lexical Index + Hybrid Retrieval (~1 wk)

**Build:** `tantivy` BM25 over atom names + claim role fillers; RRF
fusion of dense + sparse.
**Done:** rare proper nouns / identifiers retrieve correctly even when
dense similarity is low.

### Step 9.5 — Domain-Neutrality Smoke Test (~1 wk)

**Lands before reinforcement and replay** so heuristics can't calcify
around appointments.

**Build:** hand-author one ≥10-tick fixture in a non-appointment
domain (codebase rename, chat preference shift, or novel character
revision). Run Steps 0–9 against it unchanged.
**Done:** fixture passes with the same code path that passes §14, no
domain-specific shortcuts. If it fails, fix the seed pack (§7) or the
heuristic extractor (§12.1 #6) here.

### Step 10 — Hebbian + Salience + Decay + Path-Aware Reinforcement (~1.5 wk)

**Build:** §13.6 path-aware reinforcement; §13.7 utility decay; §11.4
salience; co-activation strengthening.
**Done:** across a 100-tick corpus, accessed paths strengthen, unused
links decay, no answer-bearing claim is destructively removed.

### Step 11 — Replay (~2.5 wk)

**Build:** §5.4 + §13.8 — replay thread gets a snapshot clone, returns
`Vec<ReplayMutation>` for `&mut` apply on main; region split/merge,
coref resolution, prototype eviction, cache pruning,
**archetype emergence + merge/prune + vocabulary pruning**. Reject any
mutation that breaks §14 or §15.5.
**Done:** 100-tick corpus passes §14 + fixtures; region- and
archetype-creation rates flatten; an `appointment_event` archetype
emerges in a heavy-appointment fixture, a `function_definition`
archetype in a heavy-codebase fixture, neither in the seed pack.

### Step 12 — External Benchmarks (~2 wk)

**Build:** wire LongMemEval `oracle`, MemoryAgentBench
FactConsolidation, RULER MK/MV-NIAH at 8K/32K into the harness.
**Done:** end-to-end numbers logged. Beating SOTA is *not* the v0 goal;
passing fixtures + credible external numbers is.

**v0 sign-off** = Steps 0–12 pass + §14 deterministic with archetype
traces + §15.5 fixtures + Step 9.5 fixture + LongMemEval +
MemoryAgentBench + RULER all produce credible numbers.

**Total: ~22 wk part-time.** Crate-reuse savings (Steps 0, 6, 9) cover
the archetype-dynamics additions (Steps 5.5, 5.7, +0.5 wk on Step 11).

### Reviewer Workflow

User writes code → runs the inspection harness → pastes diff → Claude
reviews diff + code, flags spec drift → user iterates. Step is done
when the harness shows zero unexpected diffs across the walkthrough up
to that step.

---

## 17. Source Map

Two passes, read in priority order. **Mathematical Foundations** ground
the substrate (§4.0). **Substrate / Algorithm** + **Cortical Columns**
are load-bearing for v0; the rest are background and reference.

### Mathematical Foundations (§4.0)

- **Habel 1992** — *Hyperedge Replacement: Grammars and Languages.* LNCS 643. Formal definition of typed hypergraphs whose edges have labeled connection points (called "tentacles" in the literature; Legend calls them slots). Backbone for §4.0 (1).
- **Ehrig, Ehrig, Prange & Taentzer 2006** — *Fundamentals of Algebraic Graph Transformation.* DPO graph rewriting; the formalism behind replay-as-transformation (§13 preamble, §13.8).
- **Parsons 1990** — *Events in the Semantics of English.* Neo-Davidsonian event semantics; the formal account of role-tagged predications used in §4.6.
- **Fillmore 1976; Baker, Fillmore & Lowe 1998** — Frame semantics / FrameNet. Archetypes-as-frames (§4.3); also cited under Cognitive Background below for §4.8 qualifiers.
- **Snodgrass 1995** — *Developing Time-Oriented Database Applications in SQL.* Bitemporal model reference; pairs with Datomic/XTDB (Durability section below) for §4.0 (3).
- **Green, Karvounarakis & Tannen 2007** — *Provenance Semirings*, PODS. Closed-form aggregation rules for derived data; backbone for §4.0 (4) and §4.7 `derived_from` / supersession.
- **Richardson & Domingos 2006; Domingos & Lowd 2009** — *Markov Logic Networks.* Weighted FOL with online weight updates; structural match to Legend's claim weights and replay (§4.0 (5), §13 preamble, §9.13).

### Substrate / Algorithm

1. **DDVFA** — Brito da Silva, Elnabarawy & Wunsch, *Neural Networks* 116 (2019), arXiv 1901.00794. Closest published kin to §8 (two-level vigilance + multi-prototype + Merge-ART). **Read end-to-end before writing v0 region code.**
2. **ART Survey** — Brito da Silva et al. 2019, arXiv 1905.11437. Failure-mode catalogue used for §8.8.
3. **Adaptive Resonance Theory** — Carpenter & Grossberg. Vigilance / resonance / stable-plastic learning; conceptual backbone of §8.
4. **GNG + GWR** — Fritzke 1995; Marsland, Shapiro & Nehmzow 2002. GWR's activation-and-firing-counter add criterion is closer to Legend's `descend_threshold` than vanilla GNG.

### Cortical Columns (Archetypes)

5. **Mountcastle 1957** — *J. Neurophysiol.* 20(4):408-434. Original cortical-column finding; foundation for §4.3.
6. **Hawkins & George (Numenta) — HTM papers.** Inspirational, also a cautionary tale about over-committing to consensus voting.
7. **Hawkins 2021 — *A Thousand Brains*.** Predict-next + voting framing taken at low fidelity (slots-only, voting-as-aggregation).
8. **Hawkins et al. 2017 — *Frontiers in Neural Circuits*.** Load-bearing TBT paper.
9. **Oja 1982 — *J. Math. Biol.* 15:267-273.** Bounded Hebbian operators for archetype updates (§13.9).

### Truth Maintenance / Temporal / Provenance

10. **Event Calculus** — Kowalski & Sergot, *New Generation Computing* 4(1), 1986. 40-year foundation for §13.4. Shanahan's modern formulation: doc.ic.ac.uk/~mpsha/ECExplained.pdf.
11. **PROV-O (W3C 2013)** — vocabulary for `derived_from` and `supersedes` (Invariant 14, §4.6).
12. **Wikidata data model** — statements/qualifiers/references/ranks; behind §4.6 + §4.8.
13. **JTMS / ATMS** — Doyle 1979; de Kleer 1986. Legend's claim-status discipline is JTMS-flavored.
14. **AGM + Hansson Base Revision** — Levi identity is the formal name for Legend's correction protocol.
15. **TimeML / TempEval-3** — temporal annotation standard; Legend adopts the 7-relation pragmatic subset.

### Durability / Materialized Views

16. **Write-Ahead Logging** — every relational DB ever shipped. Legend's §5.7 follows the WAL pattern, not full event sourcing.
17. **Datomic / XTDB** — referenced only for the bitemporal data model (§4.6 + Invariant 7), not for log-as-ground-truth.
18. **Differential Dataflow** — McSherry, Murray, Isaacs et al., CIDR 2013. Diff-passing discipline (§9.1.1).
19. **Salsa** — Rust pure-spec / `&mut`-impl pattern (rust-analyzer). Closest existing Rust analog to Legend's brain-processes-as-functions discipline.
20. **IVM** — PostgreSQL IVM wiki + Cui & Widom (TODS 2000). Background for `derived_from`.

### Comparable Memory Systems

21. **Graphiti / Zep** — Rasmussen et al. 2025, arXiv 2501.13956. Bi-temporal KG for agent memory; closest production competitor to §4.6 + §4.8 — read before finalizing supersession spec.
22. **HippoRAG 2** — Gutiérrez et al. 2025, arXiv 2502.14802. Dual-node KG + Personalized PageRank; path-aware reinforcement competitor.
23. **A-MEM** — NeurIPS 2025, arXiv 2502.12110. LLM-driven memory evolution.
24. **Mem0** — arXiv 2504.19413. Hybrid vector + graph + KV memory layer.

### NLP / Embedding / Retrieval

25. **Sentence-BERT** — Reimers & Gurevych 2019, arXiv 1908.10084. Why raw BERT is not an embedding model.
26. **BGE technical report** — arXiv 2309.07597. The v0 embedding model.
27. **GLiNER paper** — arXiv 2311.08526. Zero-shot NER used by `gline-rs`.
28. **`tokenizers`** — HuggingFace, Apache-2.0, pure-Rust.
29. **`tantivy`** — Quickwit-OSS, Lucene-grade BM25 in pure Rust.
30. **`gline-rs`** — fbilhaut, GLiNER inference on `ort`.
31. **`ort`** — pyke.io, Rust ONNX Runtime wrapper.

### Cognitive Background

32. **FrameNet** — frames + frame elements; informs §4.8 qualifiers.
33. **AMR paper** — sentence meaning as graph; design reference only, not v0.
34. **Centering Theory** — Grosz, Joshi, Weinstein 1995. Recency-based coreference baseline.

### Benchmarks

35. **LongMemEval** — ICLR 2025, arXiv 2410.10813. v0 evaluation gate.
36. **MemoryAgentBench** — ICLR 2026, arXiv 2507.05257. Fact Consolidation = supersession semantics test.
37. **RULER** — COLM 2024, arXiv 2404.06654. MK/MV-NIAH smoke tests.
38. **AbstentionBench** — FAIR 2025, arXiv 2506.09038. "Don't hallucinate when you don't know."

### Deferred / Not v0

- **BIRCH (1996)** — threshold-gated descent pattern only; CF-tree breaks under multi-prototype + cosine + DAG.
- **HNSW** — possibly a fast-lookup index *over* regions later.
- **RDF 1.1** — triple baseline; n-ary reification is the practical model.

Considered and dropped: Stanford OpenIE (JVM), AllenNLP SRL docs (Python), BERT paper (not an embedding model — see Sentence-BERT), MiniLM (subsumed by BGE-small lineage), LoCoMo (scoring controversy — §15.6).

---

## 18. Deferred Questions

These remain open. None block v0.

- When (if ever) does AMR/UMR earn its way back into the pipeline? Likely
  trigger: §15 metrics show consistent failures on document-level temporal
  reasoning that SRL + temporal parser cannot recover.
- What is the right cold-storage policy after v1? v0 keeps the full
  hypergraph in memory.
- What is the right replay scheduling cadence? Per-tick? Every N ticks?
  Idle-only? Profile in v0 step 9.
- Should query success reinforce only the selected path, or also nearby
  alternatives at lower weight? v0 does selected-only; revisit once
  reinforcement metrics are visible.
- Should `HashMap` swap to `hashbrown` or a hand-rolled open-addressing
  table? Decide on first profile, not earlier.
- When does the wide `MemoryStats` struct split into parallel `Vec<f32>`
  arrays for cache locality? Decide on first profile.
- When does `HNSW` (or another approximate-NN index) get added on top of
  the region DAG for fast lookup? When the DAG search becomes a measurable
  bottleneck.
