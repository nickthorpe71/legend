# New Foundation

Status: living architecture spec for Legend v2 — a learnable hypergraph with a
vector subgraph, a single-verb tick API, and a from-scratch ultra-minimal Rust
implementation.

This document is the substrate spec. A solo developer should be able to read it
top to bottom and start coding against it without consulting prior versions of
Legend.

---

## 0. Reading Guide

- §1–3 set the goal, the core claim, and the hard invariants.
- §4 inventories every kind of atom in the hypergraph.
- §5 is the new **Core Data Model** section: the substrate spec the coder
  works against first.
- §6 is the new **Carry-Forward From Current Legend** section: which concepts
  (not code) we keep, and what we explicitly drop.
- §7 specifies the seed layer as concrete data, not prose.
- §8 specifies the semantic-region hierarchy and routing.
- §9 is the tick pipeline — the only input operation Legend has.
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
operation. Some inputs happen to be question-shaped and produce a
populated `answer` field in `A`; some are statement-shaped and produce
more durable writes in `G'`; the distinction is emergent in the output,
not an API choice.

In the substrate's Rust implementation, this function is encoded as
`tick(&mut Hypergraph, Input) -> ConsciousAttentionState`. The `&mut` is
the operational form of `G -> G'` — same semantics, in-place mutation
for performance.

### 1.2 What The Substrate Is For

Legend turns experience into fact-preserving memory. The target is a
small general substrate that learns concepts, relations, instances,
temporal state, semantic regions, and usefulness over time, by
processing a single stream of ticks.

Ideal behavior:

- Strip useless surface form.
- Preserve every **answer-bearing asserted/base fact** plus the evidence
  needed to derive entailed facts. (The full entailment closure is *not*
  materialized — see §13.5 — so "preserve every fact" is precise: bases
  and the evidence + rules that re-derive their consequences.)
- Preserve evidence needed to verify, revise, or answer from those facts.
- Learn concepts and relation patterns instead of hard-coding semantic
  strings.
- Organize semantic space with a vector hierarchy embedded in the same
  graph.
- Run brain-like processes over the same substrate: decay, reinforcement,
  Hebbian learning, prediction error, replay, consolidation.
- Return a current state of conscious attention from every tick.

### 1.3 The Attention Frame Is The Output

`A` (the `ConsciousAttentionState`, §9.14) is the core return value —
not an answer. It is a structured snapshot of: which atoms, regions,
columns, and frames activated; which claims are in focus, with
per-claim provenance for *why* (`LensSource` in §5.6); relevant
superseded history; structural uncertainty signals; what this tick
durably wrote and what it superseded; next-action hints for replay.

When the input is question-shaped, an `answer` field surfaces from the
focused claims as a byproduct of the frame, not its goal. Legend's
substrate is consumer-agnostic — an LLM is the common reader (converts
user input → `Input`, calls `tick`, renders the frame as natural
language), but any consumer of structured state works (CLI inspectors,
agents, dashboards, programmatic tools).

**Legend does not return answers; it returns the information needed
to produce answers.**

---

## 2. Core Claim

Use one physical hypergraph.

The hypergraph contains:

- evidence atoms
- concept/column atoms
- instance atoms
- event atoms
- claim atoms
- role atoms
- value atoms
- frame atoms
- semantic-region atoms
- schema/pattern atoms
- rule atoms

Runtime exposes views over the same structure:

- concept view
- instance view
- evidence view
- frame view
- value/time view
- vector-region view
- conscious-attention view

There is no separate vector database, graph database, concept graph, or
instance graph with weak synchronization between them. The vector hierarchy is
a subgraph inside the hypergraph.

---

## 3. Hard Invariants

1. **The event log is ground truth.** The hypergraph snapshot is a stamped,
   recomputable cache. This is event sourcing: `hypergraph = log.fold(apply_tick, seed)`.
   Storage cost is small (~75 MB/decade at 100 ticks/day with embeddings
   recomputed not stored).
2. Lossless evidence is ground truth *for content*. The log is ground truth
   *for time*. Together they are the only authoritative state.
3. Learned abstractions must point back to evidence.
4. Semantic strings are labels, aliases, or evidence; they do not drive
   control flow.
5. Control flow branches only on mechanical roles (`AtomRole`, `ClaimStatus`,
   `Polarity`, `Modality`, `ValueKind`) and learned affordances.
6. Compression must be answer-preserving.
7. **Bitemporal split.** `Tick` is **transaction time** (when Legend learned
   this). **Valid time** (when this was true in the world) lives on
   `Qualifiers.TimeScope`. Industry standard (Datomic, XTDB, Wikidata,
   SQL:2011, Graphiti). Do not conflate.
8. **Replay determinism is stamped.** Every snapshot and every log segment
   carries a `model_fingerprint` (embedding-model hash, extractor versions,
   tokenizer version, code version). On model upgrade: either keep the old
   snapshot authoritative and fold forward (cheap), or re-fold the kept tail
   under the new model (expensive, run as a background job).
9. If usefulness is uncertain, demote evidence to colder accessibility
   instead of deleting it. (v0 keeps all evidence in memory; cold storage is
   a v1 concern.)
10. Asserted, entailed, defeasible, superseded, and retracted claims remain
    distinct.
11. Vector closeness may merge semantic regions. It must never destructively
    merge facts, instances, events, or evidence.
12. Query success reinforces the exact access path that found the answer
    (path-aware reinforcement, not nearby-vector reinforcement).
13. Every answer is traceable to evidence.
14. **Cache claims carry a lineage pointer.** Cache claims (derived
    current-state claims) **must** carry a `derived_from` pointer to the
    event that produced them. They are recomputable. They are never written
    without that pointer. This is the PROV-O `wasDerivedFrom` discipline
    applied as a substrate-level invariant — equivalent to JTMS
    justification-pointer rigor (Doyle 1979) and incremental view
    maintenance (Cui & Widom 2000).
15. There is one input operation: `tick`. There is no query path.

---

## 4. What The Hypergraph Is Made Of

### 4.1 Atom

A memory atom is the smallest persistent thing Legend can activate, retrieve,
reinforce, decay, link, or use as evidence.

It is not a biological neuron. It is closer to an addressable engram component
or graph atom.

```rust
struct Atom {
    id: AtomId,
    role: AtomRole,
    labels: Vec<String>,
    aliases: Vec<String>,
    stats: AtomStats,
    evidence: Vec<EvidenceRef>,
    created_at: Tick,
}

enum AtomRole {
    Evidence,
    Column,
    SemanticRegion,
    Instance,
    Event,
    Claim,
    Role,
    Value,
    Frame,
    Schema,
    Rule,
}

struct AtomStats {
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

`AtomRole` is mechanical and is allowed to drive control flow. It is not a
world ontology. `Dentist`, `Project`, `Democracy`, and `uses_datastore` are
not mechanical roles — they are labels on `Column` or `Instance` atoms.

`created_at: Tick` is **transaction time** — when Legend learned this. It is
a monotonic `u64` counter incremented once per `tick()` call.
**Valid time** — when the fact was true in the world — is a separate axis
that lives on `Qualifiers.TimeScope` (§4.8). This bitemporal split is
industry standard (Datomic, XTDB, Wikidata, SQL:2011, Graphiti) and is
required by Invariant 7. Wall clock is a `Value::TimePoint`, not a system
primitive. This keeps replay deterministic and avoids time-zone issues in
the substrate.

### 4.2 Evidence

Evidence preserves the original experience.

```rust
struct EvidenceAtom {
    atom: AtomId,
    raw_text: String,
    normalized_text: String,
    source: SourceId,
    clock: Tick,
    spans: Vec<TextSpan>,
    embedding: Vec<f32>,
}
```

Evidence can decay in accessibility but is not deleted in v0.

### 4.3 Column (First-Class Computational Unit)

A column is a **learned local predictor** anchored by an atom. It is
Legend's cortical-column-inspired primitive: each column models a small
piece of the world, predicts what should come next given partial input,
and votes for claims through its outgoing wiring.

The metaphor: Mountcastle 1957 / Hawkins & George (HTM) / Hawkins 2021
*A Thousand Brains Theory*. A cortical column in the brain is a vertical
slice of cortex that runs the same algorithm everywhere — what differs is
what it's connected to. Legend's columns inherit the spirit (uniform
local predictor, voting via wiring, predict-next via expected slots and
sequence transitions) but **not** the strong claim that consensus voting
*is* inference. Voting is one signal feeding into `focused_claims`
weighting, not the entire pipeline. The strong-version cost (research-
grade consensus algorithms, O(N²) long-range messaging) is deferred.

```rust
struct Column {
    atom: AtomId,
    prototypes: Vec<Prototype>,            // 1-8 vector exemplars
    expected_slots: Vec<SlotExpectation>,  // what fillers this predicts
    slot_transitions: SlotTransitions,     // sequence prediction over slots
    learned_claims: Vec<ClaimId>,          // claims this column has accumulated
    outgoing_wiring: Vec<ColumnConnection>, // weighted edges to other atoms
    activation: f32,                       // current tick's activation level
    reliability: f32,                      // how often this column predicted correctly
    plasticity: f32,                       // how willing to update
    support_count: u32,                    // total evidence supporting this column
}

struct SlotExpectation {
    role: AtomId,                          // which Role atom this slot fills (e.g. `target`, `from`, `to`)
    type_hint: Option<AtomId>,             // expected concept/region (soft prior)
    required: bool,                        // must be filled for the column to "complete"
    fill_probability: f32,                 // Hebbian-learned likelihood
}

struct SlotTransitions {
    // Small transition matrix over slot indices.
    // transitions[i][j] = P(slot j fills next | slot i just filled).
    // For 3–8 slots this is 9–64 floats per column.
    matrix: Box<[[f32; MAX_SLOTS]; MAX_SLOTS]>,
    n_slots: u8,
}

struct ColumnConnection {
    target: TargetRef,                     // typed pointer — atom, claim, or value
    weight: f32,                           // Hebbian co-activation strength
    last_reinforced: Tick,
}

enum TargetRef {
    Atom(AtomId),                          // another atom (typically a Column)
    Claim(ClaimId),                        // a specific claim (column votes are usually here)
    Value(ValueId),                        // a stored value
}
```

A column becomes person-like, appointment-like, relation-like, or
democracy-like through evidence and usage. That classification is an output
of learning, not a hard-coded enum. The seed pack ships ~30 **domain-
neutral** columns that predict mechanical patterns (entity mentions, state
changes, temporal expressions, references); domain-specific columns like
`appointment` or `function_definition` emerge from evidence via replay
splitting.

**Per-tick lifecycle:** activate (score prototypes, §9.7.5) → predict-next
(rank expected slots by prior, bias extractor attention, §9.7.5) → vote
(boost `outgoing_wiring` targets, aggregated in §9.14) → learn (Hebbian
updates to slots, transitions, wiring, §13.9). Column scoring is
embarrassingly parallel — `par_iter` over `Vec<Column>` in the
read-mostly phase of the tick.

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

A claim is an evidence-backed role binding. It is a hyperedge.

```rust
struct Claim {
    id: ClaimId,
    predicate: AtomId,
    roles: Vec<RoleBinding>,
    qualifiers: Qualifiers,
    evidence: Vec<EvidenceRef>,
    status: ClaimStatus,
    confidence: f32,
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
    Atom(AtomId),
    Value(ValueId),
    Claim(ClaimId),                  // for nested / conditional claims
    Variable(VariableId),
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

- **Event reification** — predicate-argument claims with role bindings are
  *neo-Davidsonian event reification* (Parsons 1990; W3C N-ary 2006).
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
time, modality, uncertainty, and evidence — all in one structure.

### 4.7 Value

Values are typed regions in value spaces.

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

### 4.8 Qualifiers

Qualifiers scope claims.

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
    evidence_refs: Vec<EvidenceRef>,
    claim_refs: Vec<ClaimId>,
    column_refs: Vec<AtomId>,
    instance_refs: Vec<AtomId>,
}

struct Prototype {
    vector: Vec<f32>,
    weight: f32,
    support_count: u32,
    evidence_refs: Vec<EvidenceRef>,
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

The intent: keep the **hot substrate** (atoms, claims, evidence, indices,
the tick loop, region routing) free of dynamic dispatch and gratuitous
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
  Concrete types for `Atom`, `Claim`, `EvidenceAtom`, `Hypergraph`.
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
- All evidence in `Vec<EvidenceAtom>` indexed by `EvidenceId(u32)`.
- Indices (`HashMap<String, Vec<AtomId>>`, etc.) are derivable. Rebuild on
  load. Do not serialize them.
- Hot scalar fields (`activation`, `strength`) are candidates for split-out
  into parallel `Vec<f32>` arrays if profiling shows the wide `AtomStats`
  struct hurts cache. Decide on first profile, not earlier.

### 5.2 The Hypergraph Struct

The substrate uses **typed payload tables keyed by `AtomId`**. Every
atom lives in `atoms` with its `AtomRole` (§4.1); atoms whose role is
`Column`, `SemanticRegion`, or `Value` carry their role-specific fields
in a dedicated payload table. Lookup is `atoms[id].role` → which table
to consult. This keeps the headline `Atom` struct uniform while letting
role-specific data have its own concrete shape.

```rust
struct Hypergraph {
    // Core storage — every atom lives here, indexed by AtomId.
    atoms: Vec<Atom>,
    claims: Vec<Claim>,
    evidence: Vec<EvidenceAtom>,

    // Role-specific payload tables, keyed by AtomId.
    // The atom's `role` field tells you which table holds its payload.
    columns: HashMap<AtomId, Column>,           // AtomRole::Column
    regions: HashMap<AtomId, SemanticRegion>,   // AtomRole::SemanticRegion
    values: HashMap<ValueId, Value>,            // AtomRole::Value (ValueId is a u32 newtype, parallel to AtomId)

    // Tick clock — monotonic, incremented once per tick.
    clock: Tick,

    // Current policy (vigilance, plasticity, decay, thresholds).
    policy: Policy,

    // Working memory — recent focused atoms, used by coreference
    // and Hebbian co-activation. Capacity ~64.
    recent_focus: VecDeque<AtomId>,

    // Derived indices — rebuild on load, never serialize.
    by_label: HashMap<String, Vec<AtomId>>,
    by_alias: HashMap<String, Vec<AtomId>>,
    by_role: HashMap<AtomRole, Vec<AtomId>>,
    region_children: HashMap<AtomId, Vec<AtomId>>,
    region_parents: HashMap<AtomId, Vec<(AtomId, f32)>>,
    claims_by_subject: HashMap<AtomId, Vec<ClaimId>>,
    claims_by_predicate: HashMap<AtomId, Vec<ClaimId>>,
    supersession_chain: HashMap<ClaimId, ClaimId>,
}
```

**Why typed payload tables instead of one big `Atom` struct.** Putting
`Vec<Prototype>`, `Vec<SlotExpectation>`, `SlotTransitions`, etc. on
every atom would waste memory on the 90% of atoms that aren't columns
or regions. Keeping them in side tables means the hot `atoms: Vec<Atom>`
stays cache-friendly while role-specific data is one indirection away
when needed. `HashMap` is fine for v0; if the column count grows large,
swap to `Vec<Option<Column>>` indexed by `AtomId` — same `O(1)`
lookup, better cache locality.

When you see `&hg.columns[&atom_id]` in pseudocode for the column
functions (§9.7.5, §11.10, §13.9), it's reading the payload
table for an atom whose `role` is `Column`.

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

    // Column dynamics (§4.3, §9.7.5, §13.9)
    column_activation_threshold: f32,   // min similarity for a column to activate; 0.55 default
    column_voting_weight: f32,          // how much an active column's vote boosts focused_claims; 0.3 default
    slot_prediction_bias: f32,          // how strongly slot_transitions bias extractor attention; 0.4 default
    column_plasticity: f32,             // per-column update step size; 0.3 default
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

- All ids are `u32` newtypes (`AtomId(u32)`, `ClaimId(u32)`, `EvidenceId(u32)`,
  `ValueId(u32)`, `FrameId(u32)`, `SourceId(u32)`, `VariableId(u32)`,
  `RegionId = AtomId`).
- `Tick(u64)` for the monotonic clock.
- Reserve `u32::MAX` as `INVALID`. Never panic on bad ids; return
  `Result<_, HypergraphError>`.

### 5.6 Auxiliary Type Definitions

These types appear as fields in the structs above. Defined here once so
the substrate is fully concrete when you start coding.

```rust
// Pointer to an evidence atom. Same shape as EvidenceId; named
// differently in fields where the role is "this references evidence"
// rather than "this is an evidence-keyed lookup".
type EvidenceRef = EvidenceId;

// Pointer to a claim, used in attention-state output.
type ClaimRef = ClaimId;

// Byte offsets into raw evidence text.
struct TextSpan {
    start: u32,    // byte offset into EvidenceAtom.raw_text
    end: u32,
}

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

// Maximum slots a single column can have. Bounds the slot_transitions matrix.
const MAX_SLOTS: usize = 8;

// Activation records returned in ConsciousAttentionState.
struct RegionActivation {
    region: AtomId,
    activation: f32,
    surfaced_atoms: Vec<AtomId>,
}

struct ClaimActivation {
    claim: ClaimId,
    score: f32,                                // base_weight + vote_weight (§9.14)
    surfaced_by: Vec<LensSource>,              // which lens(es) brought this to focus
}

enum LensSource {
    RegionRouting(AtomId),                     // a region surfaced this claim
    ColumnVote(AtomId),                        // a column voted for this claim
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
    ConflictingColumnVotes,                    // active columns disagreed
    LowConfidenceExtraction,
    MissingExpectedSlot(AtomId),               // a column predicted a slot that didn't fill
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
    EmergeColumn { from_region: AtomId, prototypes: Vec<Prototype>, slots: Vec<SlotExpectation> },
    PruneColumn { column: AtomId },
    MergeColumns { columns: Vec<AtomId> },
    ResolveCoreference { provisional: AtomId, canonical: AtomId },
    EvictPrototype { region: AtomId, prototype_index: usize },
    PromoteFromVoid { atoms: Vec<AtomId>, into_region: AtomId },
}

enum ReplayJob {
    HighVarianceRegion(AtomId),
    SuspectedDuplicateColumn(AtomId, AtomId),
    StaleProvisionalInstance(AtomId),
}

// Used by activate_columns / predict_next.
struct SlotPrediction {
    column: AtomId,
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
    ColumnSlotsExceeded(AtomId),               // > MAX_SLOTS
    ModelFingerprintMismatch { expected: ModelFingerprint, found: ModelFingerprint },
    SerializationError(String),
}
```

These are intentionally light. Most can be expanded as the
implementation evolves; they're listed here so the type system is
closed when you start writing the substrate.

### 5.7 Serialization (Log-Primary Event Sourcing)

The event log is ground truth. The hypergraph snapshot is a stamped,
recomputable cache. This is **event sourcing** (Greg Young, 2006; Martin
Fowler, 2005); the formalization "the snapshot is a memoization of your
left fold" comes from Young's 2014 talk. Same pattern as Datomic's
transaction log + index, XTDB's Kafka topic + RocksDB cache, EventStoreDB's
log + projection.

#### 5.7.1 The Log

```rust
struct LogEntry {
    tick: Tick,
    input: Input,                    // raw text + source + wall-clock-at-write
    model_fingerprint: ModelFingerprint,
}

struct ModelFingerprint {
    embedding_model: String,         // e.g. "bge-small-en-v1.5"
    embedding_dim: u16,
    extractor_versions: Vec<(String, String)>,  // (name, version)
    tokenizer_version: String,
    code_version: String,            // git SHA
}
```

The log is **append-only**, written to disk via the WAL pattern, fsynced on
some policy (per current Legend's daemon-durability discipline; v0 keeps
this simple).

#### 5.7.2 The Snapshot

The on-disk hypergraph image is a stamped cache:

- Format: LZ4 + MessagePack (matches current Legend's proven choice).
- Serialized fields: `atoms`, `claims`, `evidence`, `clock`, `policy`,
  plus a **`stamped_at: Tick`** marker and the `ModelFingerprint` valid
  for that snapshot.
- Derived indices are rebuilt on load.
- v0 has no migrations because there is no v(-1). When the format changes
  in v1, add a 4-byte version header.

**Authoritative state = `(checkpoint snapshot) + (suffix log of entries with tick > snapshot.stamped_at)`.**

This is the one canonical rule, replacing earlier loose phrasings:

- A snapshot alone is not authoritative — it could be missing the
  ticks that landed after it was written.
- A log alone is not authoritative *unless* its entries reach all the
  way back to the empty hypergraph. Once compaction (§5.7.4) discards
  pre-checkpoint segments, full re-fold from genesis is impossible —
  but full re-fold from the most recent retained checkpoint always is.
- Boot path: load the latest checkpoint snapshot, then replay the
  suffix log on top of it.
- Crash recovery is the same path: the suffix log is what survived;
  the snapshot is the durable base it sits on.

Pre-checkpoint logs are retained only if explicitly opted into (e.g.
for the expensive-path model upgrade in §5.7.5). The default is to
truncate them once a fresh checkpoint is fsynced.

#### 5.7.3 Checkpoint Policy

Hybrid (well-precedented across RocksDB / Kafka Streams / Flink):

```text
checkpoint when (
  ticks_since_last_checkpoint > N    OR
  log_size_bytes > S                 OR
  time_since_last_checkpoint > T
)
```

v0 starting numbers: **N = 1000 ticks, S = 10 MB, T = 1 hour.** Tune from
profiling.

#### 5.7.4 Log Compaction

Snapshot + truncate (the EventStoreDB / Marten mainstream choice). After
a new snapshot lands and is fsynced:

- The snapshot becomes the new **base** for replay (it is *not* "tick
  0" — `clock` continues monotonically; the snapshot is just the
  durable state at `snapshot.stamped_at`).
- All log entries with `tick <= snapshot.stamped_at` are marked
  truncatable and garbage-collected on the next maintenance cycle —
  unless explicit retention is configured (see §5.7.5 expensive path).
- The authoritative state after compaction is still
  `(snapshot) + (suffix log with tick > stamped_at)` — only the *base*
  changed; the rule didn't.

After compaction, full re-fold from the empty hypergraph is no longer
possible (the early log segments are gone). Re-fold from the snapshot
forward is always possible, and is what replay/boot/crash-recovery
actually use.

#### 5.7.5 Replay Determinism Under Model Upgrade

This is the documented universal failure mode of event-sourced + ML
systems (Temporal.io's "non-determinism drift"; sakurasky.com agent
literature). When `model_fingerprint` changes, the authoritative-state
rule (`snapshot + suffix log`) still holds — but the *interpretation*
of the rule has two paths:

- **Cheap path (default).** The latest snapshot remains the
  authoritative base. New ticks fold forward under the new
  fingerprint. The suffix log past the snapshot may have been written
  under either the old or the new fingerprint and is replayed
  identically; the underlying *atoms/claims* don't change just
  because the model upgraded — only how *new* inputs get embedded
  changes. This is identical to Datomic's practice.
- **Expensive path (opt-in, background job).** Required only when an
  upgrade invalidates stored embeddings (e.g. embedding-dim change
  that breaks region prototypes). The job: (1) configure pre-
  checkpoint log retention so the relevant log tail is preserved
  before compaction, (2) re-fold the kept tail under the new
  fingerprint to produce a fresh authoritative snapshot, (3) the new
  snapshot replaces the old as the authoritative base. Run overnight.
  This is the only case where Legend *needs* logs older than the
  current checkpoint, and it must be opted into in advance.

#### 5.7.6 Storage Cost

At 200 B/tick (raw input + fingerprint, no embeddings) × 100 ticks/day
× 365 × 10 = **~75 MB/decade**. Embeddings recomputed at replay time, not
stored in the log. Snapshots dominate disk; logs are nearly free.

#### 5.7.7 Why Not CRDTs (For v0)

Legend's tick fold is not commutative — supersession depends on temporal
sequence; coreference depends on `recent_focus`. So the hypergraph itself
cannot be a CRDT. But the **log is a Grow-only Set, trivially a CRDT**.
v1 cross-device sync merges logs by union ordered with Lamport timestamps,
then re-folds locally. Don't make the hypergraph a CRDT — make the log a
CRDT. v0 is single-writer.

---

## 6. Carry-Forward From Current Legend

This is a fresh repo with a fresh data model. We bring forward **concepts**,
not code.

### 6.1 What We Keep (As Concepts)

- **Decay + reinforcement scalars on every atom.** Already in `AtomStats`.
  Constants worth cribbing from current Legend's basal-ganglia AdaGrad code.
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
- **Embedding model + auto-migration discipline.** If the embedding dim
  changes, re-embed everything. Current Legend's pattern; preserved.
- **Evidence-first reflex.** Always persist the raw input as an
  `EvidenceAtom` before anything else can fail.

### 6.2 What We Drop

- **L1/L2/L3 layering.** The substrate replaces it. Working memory is a small
  ring buffer; everything else is one hypergraph.
- **Brain-region module boundaries.** Brain processes are pure functions over
  `&mut Hypergraph` (§11), not modules with their own state.
- **The wernicke lexicon.** ~3400 lines of hand-coded entity logic. Replaced
  by the seed pack (§7) plus extractors (§12).
- **`TickResult`.** Replaced by `ConsciousAttentionState` (§9.14).
- **Persistence/WAL/daemon/MCP/CLI.** Out of scope for the core substrate.
  Reattach in v1 once the substrate is proven.
- **Anything Python or JVM.** No sidecars. No exceptions.

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

### 7.1 Code/Seed/Evidence Boundary

```text
Code owns mechanics.
Seeds own priors.
Evidence owns truth.
Replay owns learning.
```

Hard-coded code owns only substrate mechanics:

```text
AtomRole, ClaimStatus, Polarity, Modality, ValueKind
time/value comparison
evidence provenance
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
  labels: ["change/history patterns"]
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
Identity           two mentions may refer to the same thing (evidence required)
State              something has a value, property, relation, location, role, or status
Change             a state changes from old value to new value
Revision           text, code, plan, belief, preference, or artifact is edited
Decision           an option is selected with rationale and alternatives
Task               work is intended, active, blocked, completed, deferred, or superseded
Preference         user/project behavior should follow a stated style or rule
QuestionAnswer     input seeks information; output should select answer-bearing claims
Evidence           claim came from a source span, file, message, session, commit
Temporal           before/after/current/previous/next/latest/history
Quantification     count, amount, threshold, unit, comparison, range
```

**Concrete seed-schema example:**

```yaml
- atom_id: SCHEMA_CHANGE
  role: Schema
  labels: ["Change"]
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

### 7.4 Seed Columns (Domain-Neutral Mechanical Predictors)

Hard rule: **no seed column is a world entity.** No `appointment`,
`function`, `character`. Seeds are predicate-shaped predictors over
mechanical patterns that survive any domain. Domain-specific columns
emerge from evidence via replay splitting (§13.8).

The 30 seed columns (the 30 most reusable predicate-shaped patterns in
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

Each column ships with hand-authored prototypes (descriptor sentences
embedded at boot, like seed regions), expected slots, and a small seeded
slot-transition matrix derived from how these patterns most commonly
appear in English. **Concrete seed-column example:**

```yaml
- atom_id: COLUMN_CHANGE_EVENT
  role: Column
  labels: ["change_event"]
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
  outgoing_wiring:
    # Seeded with weak priors; learned by co-activation.
    - target: SCHEMA_CHANGE              # the Schema atom that emits cache claims
      weight: 0.7
    - target: COLUMN_STATE_ASSERTION     # often co-activates
      weight: 0.4
    - target: COLUMN_TEMPORAL_EXPRESSION # often co-activates
      weight: 0.3
  reliability: 0.8
  plasticity: 0.5
  provenance:
    source: built_in_seed
    status: defeasible
    user_confirmed: false
```

When tick 1 of §14 fires ("My dentist appointment with Dr. Rao changed
from Tuesday to Friday"), `COLUMN_CHANGE_EVENT` activates because its
prototypes match the embedded text. Its expected slots prime extractor
attention for `target` (the appointment) and `from`/`to` (Tuesday →
Friday). The slot-transition matrix says "after `from` fills, `to` is
85% likely next" — which biases the temporal parser to look for a second
weekday. The column votes for the resulting `reschedule_event_1` atom
via its `outgoing_wiring`, contributing to `focused_claims` weighting.

`appointment` does **not** appear in the seed pack. Replay can split out
a learned column called `appointment` later if evidence accumulates around
a particular `state_with_temporal_value` configuration with consistent
provider/participant slots — but that is earned by the corpus, not by us.

### 7.5 Seed Pack Manifest

The seed pack ships as one file:

```text
seed_v0.msgpack.lz4
  - 1 Genesis atom
  - 1 Void atom
  - 16 SemanticRegion atoms with descriptor-derived prototypes
  - 11 Schema atoms with trigger lemmas/patterns
  - 30 Column atoms with prototypes + slots + transitions (§7.4)
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
  the mutation phase of the tick (Step 9–11). Takes the proposed
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
    // Void attachments — when nothing matched.
    void_attachments: Vec<EvidenceRef>,
}

struct NewRegion {
    parent: AtomId,
    initial_prototype: Vec<f32>,
    refs: Vec<EvidenceRef>,
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
    let id = hg.allocate_atom(AtomRole::SemanticRegion)
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
- claim/evidence overlap is high,
- answer behavior is equivalent,
- merging does not collapse distinct instances,
- no contradiction or frame conflict appears.

Do not merge when:

- two regions answer different questions,
- one contains superseded state and the other contains current state,
- they share words but not roles,
- they belong to distinct frames,
- their evidence contradicts.

### 8.5 Region Split Rule

Split a region when:

- internal variance grows,
- repeated prediction errors occur,
- queries route into the region but need different answers,
- evidence forms distinct frames,
- a broad concept contains separable instances or sub-concepts.

Splitting improves routing. It does not duplicate or destroy evidence.

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

### 9.1 The Fifteen Steps

```text
0.  log entry                      -> append (Tick, Input, ModelFingerprint) to log
1.  preserve evidence              -> EvidenceAtom appended
2.  detect intent                  -> AttentionIntent
3.  adjust policy                  -> Policy updated for this tick
4.  segment text                   -> spans (sentence/clause/entity/value)
5.  embed every span               -> Vec<(span, embedding)>
                                      -- READ-MOSTLY PARALLEL PHASE BEGINS --
6a. route through region DAG       -> active regions per span
6b. activate columns + predict     -> active columns + slot predictions
                                      (par_iter over Vec<Column>; embarrassingly parallel)
7.  run extractors                 -> claim/event proposals with confidence
                                      (extractors biased by step 6b's slot predictions)
8.  coreference scoring            -> instance reuse vs. provisional new
                                      -- MUTATION PHASE BEGINS (single &mut Hypergraph) --
9.  build claims & events          -> appended to hypergraph with status
10. supersede prior cache          -> mark old current-state claims Superseded
11. derive current-state cache     -> new cache claims pointing at events
12. apply Hebbian + salience       -> AtomStats + Column wiring/transitions updated
                                      (active columns vote into focused_claims weighting)
13. apply decay                    -> all inactive atoms (incl. columns) weaken slightly
14. assemble attention state       -> ConsciousAttentionState returned
                                      (active_columns + focused_claims surfaced)
                                   -> enqueue heavy work for replay
```

Step 0 is the event-sourcing log append (§5.7). It happens *before* Step
1 so that even if Step 1 panics, the log entry is durable and the tick
can be replayed.

**Parallelism boundary.** Steps 5, 6a, 6b, 7 run with read-only access to
the hypergraph and parallelize cleanly via `rayon::par_iter`. Steps 9–14
require `&mut Hypergraph` and run sequentially. This is the
read-mostly-parallel, write-sequentially pattern — same shape Datomic and
FoundationDB use. Column scoring at scale (5K columns × 8 embeddings)
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
    StatsBumped(AtomId, AtomStatsDelta),
}
```

Downstream consumers (cache materialization, salience updates, the
attention assembler in Step 14, the replay queue) consume deltas, not full
state recomputation. This is **differential dataflow** discipline (McSherry,
Murray, Isaacs et al., CIDR 2013) / **semi-naive Datalog evaluation**
(Bancilhon-Ramakrishnan 1986). We do not import the
`differential-dataflow` crate; we adopt the discipline.

### 9.2 Step 1 — Preserve Evidence

Always store the raw tick first as an `EvidenceAtom`. This protects against
extractor failure: if every later step crashes, the raw text is still in the
hypergraph.

### 9.3 Step 2 — Detect Intent

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

### 9.4 Step 3 — Adjust Policy

PFC sets `vigilance`, `plasticity`, `merge_threshold`, etc. based on intent
(§8.3 table). The tick runs under the adjusted `Policy`.

### 9.5 Step 4 — Segment Text

Split into units: sentence, clause, quoted span, list item, code span,
entity-like span, time/value span. Each unit gets its own embedding and
evidence ref.

### 9.6 Step 5 — Embed Units

Embed every unit from Step 4 plus the full tick — never one averaged
vector for the whole memory, because later questions target small
facts. The substrate is dimension-agnostic but the seed pack's
prototypes are dim-specific; swapping dimensions requires re-embedding
the seed.

### 9.7 Step 6a — Route Through Regions

Each embedding runs `route_regions(...)` (§8.2 Phase A) against the DAG.
This is **read-only** and parallelizes across embeddings via `par_iter`.
Outputs:

- a `RegionDelta` describing the proposed structural changes (region
  attachments, prototype updates, new regions, void attachments)
- candidate concept columns surfaced during traversal
- candidate frames
- similar evidence
- likely duplicate claims
- novelty score
- noise score

The `RegionDelta` is held until the mutation phase (Steps 9–11), where
`apply_region_delta(...)` runs under `&mut Hypergraph` and commits the
attachments, prototype updates, and new regions. This split is what
preserves the read-mostly-parallel / mutation-sequential boundary
(§9.1).

### 9.7.5 Step 6b — Activate Columns + Predict Next

Per-column scoring against active embeddings:

```text
for each column c in hypergraph.columns (par_iter):
  similarity = max(cosine(embedding, p.vector) for p in c.prototypes)
  if similarity >= policy.column_activation_threshold:
    c.activation = similarity
    active_columns.push(c.atom)
```

Embarrassingly parallel — `par_iter` over `Vec<Column>`; every column
scoring is independent. With 5K columns × 8 embeddings on a 12-core
laptop, this is ~1ms per tick after SIMD-friendly cosine.

For each active column, **predict-next** — using **only** the column's
own expected-slot priors and its `slot_transitions` matrix. Step 7
(extractors) hasn't run yet at Step 6b, so no slots are filled yet for
this tick. Step 6b's job is purely *prior-driven*: rank slots by
expected fill probability and emit a `Vec<SlotPrediction>` to bias
extractor attention.

```text
for each active column c:
  # No slots are filled yet — extractors run in Step 7.
  # Predict from the column's own priors only.
  for slot s in c.expected_slots, sorted by s.fill_probability desc:
    emit SlotPrediction { column: c, role: s.role, type_hint: s.type_hint,
                          confidence: s.fill_probability * c.activation }
```

The slot-transition matrix is used **after** extraction (Step 12) for
Hebbian learning — it learns the observed ordering of fills. It is
*not* used at Step 6b because the ordering can't be known until
extraction has happened.

The emitted predictions are a soft prior on which slot types the
extractors should look for. They do **not** override extractor output:
if the temporal parser doesn't see a date, the prediction was wrong
and the column's `expected_slots[i].fill_probability` gets a Hebbian
downweight in Step 12 (§13.9.2).

Active columns also stage their **votes**: each active column's
`outgoing_wiring` targets become candidate boost weights for Step 14's
`focused_claims` aggregation. Multiple columns voting for the same
target compound (RRF-style aggregation; §13.9).

### 9.8 Step 7 — Run Extractors

The v0 extractor stack (§12 details what's native vs ONNX):

- **NER** — spans for names/orgs/places. Biased by Step 6b column
  predictions toward expected slot types.
- **Temporal parser** — dates, weekdays, durations, relative times.
  Biased by Step 6b toward expected `time` / `from` / `to` slots.
- **Zero-shot relation extraction (`gline-rs` / GLiNER2)** — typed
  triples driven by active column slot expectations.
- **Heuristic relation extractor** — pattern-matched fallback for
  patterns GLiNER2 doesn't cover; driven by seed schemas (§7.3).
- **Heuristic coref** — recency-based: pronouns resolve to the
  most-recently-focused atom whose role matches.

All extractor output carries confidence and evidence refs. Extractor
proposals that satisfy active columns' expected slots get a confidence
bump.

v1 upgrade points: real SRL, real coref, dependency parser.

### 9.9 Step 8 — Coreference Scoring

Identity is conservative. Score:

```text
score =
  alias_overlap
  + embedding_similarity
  + frame_overlap
  + role_overlap
  + temporal_compatibility
  + evidence_support
  - contradiction_penalty
  - distinct_instance_penalty
```

Rules:

- Reuse concepts broadly.
- Reuse instances only with coreference evidence.
- Create provisional instances when uncertain.
- Replay merges provisional instances later if evidence supports it.
- Never merge exact evidence.

Pattern separation (`separate_pattern`, ported from current Legend's dentate
gyrus) is the dampening function on the merge side: when two candidates are
close-but-distinct on a discriminating role, force them apart.

### 9.10 Step 9 — Build Claims and Events

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

### 9.11 Steps 10–11 — Supersession and Cache

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

### 9.12 Step 12 — Hebbian + Salience + Column Updates

Co-activated atoms (members of the focus set) have their pairwise wiring
strengthened. Amygdala bumps salience for:

- exact values/times/persons
- corrections / contradictions
- user-stated preferences
- claims that just answered something

**Column-specific updates this step:**

- **Slot-fill learning.** For each active column, slots that the
  extractors actually filled this tick get their `fill_probability`
  Hebbian-bumped. Slots that were predicted but did not fill get a
  small downweight.
- **Slot-transition learning.** For each active column, the observed
  ordering of slot fills updates the `slot_transitions` matrix.
  Transitions that matched prediction strengthen; transitions that
  diverged weaken.
- **Outgoing-wiring learning.** When two columns co-activate this tick,
  their `outgoing_wiring` to each other strengthens. This is the
  direct cortical-column voting analogue at the wiring level.
- **Reliability tracking.** Columns whose votes ended up in the final
  `focused_claims` set with high confidence get `reliability` bumped;
  columns whose votes did not survive aggregation get a small downweight.

### 9.13 Step 13 — Decay

Every atom not touched this tick has its `activation` decayed by
`policy.decay_rate`. Decay weakens **access paths**, never destroys
evidence or answer-bearing claims (Invariants 2, 11).

### 9.14 Step 14 — Assemble Attention State

```rust
struct ConsciousAttentionState {
    tick: Tick,
    evidence: EvidenceRef,         // the raw input, always
    intent: AttentionIntent,
    active_frame: Option<AtomId>,
    active_regions: Vec<RegionActivation>,
    active_columns: Vec<ColumnActivation>,
    focused_claims: Vec<ClaimActivation>,
    answer: Option<AnswerCandidate>,    // populated when input was answer-shaped
    supporting_evidence: Vec<EvidenceRef>,
    history: Vec<ClaimRef>,             // superseded claims relevant to focus
    uncertainty: Vec<UncertaintySignal>,
    durable_writes: Vec<AtomId>,        // what this tick added
    superseded: Vec<ClaimId>,           // what this tick demoted
    next_actions: Vec<AttentionAction>,
}
```

```rust
struct ColumnActivation {
    column: AtomId,
    activation: f32,                  // raw similarity-driven activation
    slots_filled: Vec<AtomId>,        // which slots got bound this tick
    slots_predicted_unfilled: Vec<AtomId>,  // slots column expected but extractors missed
    voted_for: Vec<(AtomId, f32)>,    // (target, weight) wiring votes contributed to focus
}
```

**Aggregation.** `focused_claims` is computed as a weighted score over
extractor proposals + column votes:

```text
for each candidate claim c:
  base_weight = c.confidence * c.salience
  vote_weight = sum over active columns col where col.outgoing_wiring -> c.atom:
                  col.activation * col.outgoing_wiring[c.atom].weight * policy.column_voting_weight
  focused_score(c) = base_weight + vote_weight

focused_claims = top-N by focused_score
```

Multiple columns voting for the same claim compound (RRF-like).
Disagreement surfaces in `uncertainty` rather than collapsing into a
forced consensus.

**Properties:**

- *Always returned* — even a bare statement gets the current focus
  including `active_columns`, `active_regions`, and related history.
- *Answer is opportunistic* — derived from focused claims when input
  is question-shaped; absent otherwise.
- *History is first-class* — superseded claims relevant to focus
  surface separately ("the dental appointment *used to* be Tuesday").
- *Column votes are inspectable* — `active_columns[i].voted_for`
  carries per-claim provenance for which columns endorsed it.

### 9.15 Replay Enqueue

Heavy work — region split/merge, coref resolution, cache pruning, prototype
eviction — is enqueued for the replay thread (§13.8). The synchronous tick
stays fast.

---

## 10. There Is No Query

What was a "query" in v1 Legend is a tick whose input is question-shaped.

```text
When is my appointment at the dentist?
```

This is a tick. Same fourteen steps. Step 7 detects question-shape via the
QuestionAnswer schema. Step 14 populates `answer` from focused claims.

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
fn activate_columns(input_embeddings: &[Vec<f32>], hg: &Hypergraph, p: &Policy) -> Vec<ColumnActivation>;
fn predict_next(active: &[ColumnActivation], filled: &[AtomId], hg: &Hypergraph) -> Vec<SlotPrediction>;
fn separate_pattern(candidate: &Atom, neighbors: &[&Atom], p: &Policy) -> Decision;
fn score_salience(claim: &Claim, ev: &[&EvidenceAtom], p: &Policy) -> f32;
fn detect_intent(input: &str, embeddings: &[Vec<f32>], recent: &VecDeque<AtomId>) -> AttentionIntent;
fn adjust_policy(intent: &AttentionIntent, base: &Policy) -> Policy;
fn aggregate_focus(candidates: &[ClaimCandidate], votes: &[ColumnActivation], p: &Policy) -> Vec<ClaimActivation>;

// Mutation (sequential, takes &mut Hypergraph).
fn apply_region_delta(hg: &mut Hypergraph, delta: RegionDelta);
fn reinforce_path(path: &[AtomId], hg: &mut Hypergraph);
fn update_columns(active: &[ColumnActivation], outcomes: &FocusOutcome, hg: &mut Hypergraph, p: &Policy);
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

Co-activated atoms wire together. After each tick, every pair of focused
atoms gets a small `outgoing_wiring` strength bump.

### 11.7 Path-Aware Reinforcement — `reinforce_path`

If a path answers a query, reinforce the **exact path**:

```text
query -> appointment region -> dental frame -> appointment_1 -> current_time -> Friday
```

Not every nearby vector. Path-aware, not vector-aware. Current Legend's
basal-ganglia AdaGrad code is the reference.

### 11.8 Decay — `decay_step`

Decay reduces retrieval priority, not evidence existence.

Decay targets:

- unused semantic-region links
- low-confidence inferred claims
- low-utility derived claims
- stale provisional instances
- noisy aliases
- weak access paths

Decay spares:

- exact evidence
- high-salience values
- claims with answer success
- contradictions/corrections
- supersession history
- user preferences

### 11.9 Replay — `replay` (background thread)

Offline learning. See §13.8.

### 11.10 Cortical Columns — `activate_columns` + `predict_next` + `aggregate_focus` + `update_columns`

The predict-vote-learn cycle. All four are pure functions over the
hypergraph (or `&mut` for `update_columns`); all `par_iter`-friendly.
Algorithms in §9.7.5 (activate + predict), §9.14 (aggregate), §13.9
(updates). Both predict_next and the slot-transitions update follow the
Mountcastle / Hawkins canonical-cortical-circuit principle at low
fidelity: every column runs the same algorithm; what differs is what
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
3. **Embedding model** — `bge-small-en-v1.5`, 384-dim, ~33 MB ONNX.
   **Pooling: CLS token** (per the official BAAI model card; the
   `sentence-transformers` packaging wraps it with mean pooling, but the
   raw HF model uses CLS). **L2-normalize** the output vector before
   storing or comparing. **Query instruction** for retrieval queries is
   the BGE-specific prompt — `"Represent this sentence for searching
   relevant passages: "` — prepended to *queries only*; passages get no
   instruction. The instruction is *mostly optional* for BGE-v1.5 (single-
   digit-percent quality drop when omitted) but **mandatory** if we later
   swap to E5 (`query:` / `passage:` style) or Nomic (`search_query:` /
   `search_document:`). **Keep an `EmbeddingWrapper` interface** that
   takes a typed `EmbedKind::{Query, Passage}` and applies the right
   prefix/pooling per model; this turns model swaps into one-line
   changes. Substrate is dimension-agnostic; swap within the 384-dim
   family is a binary change.
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
- **Embedding model upgrade** — `nomic-embed-text-v1.5` for Matryoshka
  Representation Learning (truncatable to 64/128/256/512/768 dims at
  single-digit-percent quality loss). The substrate is dimension-agnostic
  so the swap is mechanical.

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

Candidate scoring (§9.9). Pattern separation as the dampening function on the
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
`from`, `to`) follow **neo-Davidsonian event semantics** (Parsons 1990;
Davidson 1967) — the canonical n-ary reification pattern in W3C's
N-ary Note (2006) and the FrameNet / FrameBase tradition.

### 13.5 Claim Materialization Policy

Store:

- asserted base claims
- high-confidence entailed claims needed for retrieval
- current-state cache claims (with `derived_from`)
- supersession links
- evidence refs

Do not eagerly store:

- every paraphrase
- every weak implication
- every possible role assumption

Derived claims are computed on the fly or materialized during replay if they
prove useful. This is **incremental view maintenance** (Gupta & Mumick 1995;
PostgreSQL IVM wiki) discipline applied to the claim graph. Cache claims
are *self-maintainable views* (Quass et al., VLDB 1996): they can be
refreshed without re-querying base data, given the event chain.

### 13.6 Path-Aware Reinforcement

When a tick produces an answer, reinforce the exact path:

- query embedding region
- matched evidence
- selected claims
- selected instance
- selected column
- region-to-claim path
- frame/time filter path

Not nearby alternatives.

### 13.7 Utility-Based Decay

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

Decay weakens access paths first. Evidence deletion is a separate retention
policy and is not implemented in v0.

### 13.8 Replay (Background Thread)

Replay runs on a background thread under the snapshot/message-passing
protocol (§5.4). Replay jobs:

- split high-variance regions
- merge duplicate regions
- merge duplicate columns
- **emerge new columns from clustered evidence** — when a sub-region
  shows consistent slot-fill patterns across many ticks (e.g. a recurring
  `state_with_temporal_value` configuration with consistent
  provider/participant slots), replay can split out a learned column.
  This is how `appointment` (or `function_definition`, or `character`)
  emerges in a domain-specialized Legend instance from purely seed-
  domain-neutral starting columns.
- resolve provisional coreference
- compact repeated evidence
- materialize useful derived claims
- demote unused derived claims
- evict prototypes when a region exceeds 8
- prune low-utility columns (low `support_count`, low `reliability`)
- merge functionally-equivalent columns (high outgoing-wiring overlap +
  high prototype similarity)

**Replay must be benchmark-aware:** any candidate compression is rejected if
it would break recall on the §14 walkthrough.

### 13.9 Column Dynamics

This subsection consolidates the per-tick column update rules referenced
across §4.3, §9.7.5, §9.12, §11.10. All updates are bounded
(plasticity-modulated) Hebbian with explicit decay.

#### 13.9.1 Activation

```text
for column c, embedding e:
  sim = max(cosine(e, p.vector) for p in c.prototypes)
  if sim >= policy.column_activation_threshold:
    c.activation = sim
    active.push(c)
```

Cosine on unit-normalized vectors only (§8.7). Multi-prototype: the max
across prototypes is the column's activation, not the mean.

#### 13.9.2 Slot satisfaction

For each active column `c` and each extractor proposal `p`:

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
delta = policy.column_plasticity * (-prediction_error)
```

where `prediction_error = expected_fill_rate - observed_fill_rate`.

#### 13.9.3 Sequence prediction

```text
for column c with active slot fills [s_a, s_b, s_c, ...] (in order):
  for each consecutive pair (s_i, s_j):
    c.slot_transitions.matrix[s_i][s_j] ← bounded_hebbian_bump(...)
  for each non-observed transition (s_i, s_k) where s_k was predicted:
    c.slot_transitions.matrix[s_i][s_k] ← bounded_hebbian_decay(...)
```

The matrix rows re-normalize to sum to 1.0 after updates so they remain
proper probability distributions.

#### 13.9.4 Voting and outgoing wiring

When two columns `c1`, `c2` co-activate within a tick:

```text
strengthen c1.outgoing_wiring[c2.atom].weight
strengthen c2.outgoing_wiring[c1.atom].weight
```

When an active column `c` has `outgoing_wiring[claim.atom]` pointing at
a claim that ended up in `focused_claims`:

```text
c.reliability += policy.column_plasticity * (1 - c.reliability)
c.outgoing_wiring[claim.atom].weight ← bounded_hebbian_bump
```

When a column voted for a claim that did not make `focused_claims`:

```text
c.reliability *= (1 - policy.column_plasticity * 0.3)
c.outgoing_wiring[claim.atom].weight ← bounded_hebbian_decay
```

#### 13.9.5 Plasticity decay

A column's `plasticity` decays slowly with `support_count`:

```text
c.plasticity = initial_plasticity / (1 + log(1 + c.support_count))
```

Mature columns become harder to perturb. New columns (low
`support_count`) update fast.

#### 13.9.6 Bounded Hebbian operators

All updates use bounded operators that prevent runaway growth/collapse:

```text
bounded_hebbian_bump(x, rate=policy.column_plasticity):
  return x + rate * (1 - x)        # asymptotes to 1.0

bounded_hebbian_decay(x, rate=policy.column_plasticity * 0.3):
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

**Active seed columns this tick** (from §7.4 — none of these is
domain-specific):

```text
COLUMN_CHANGE_EVENT          activation 0.92
  predicted slots: target, from, to (top transition)
  filled this tick: target=appointment_1, from=Tuesday, to=Friday
COLUMN_ENTITY_MENTION        activation 0.88
  filled: name=DrRao
COLUMN_TEMPORAL_EXPRESSION   activation 0.81
  filled: kind=weekday, instances=[Tuesday, Friday]
COLUMN_STATE_WITH_TEMPORAL_VALUE  activation 0.74
  filled: subject=appointment_1, time_scope=Friday
COLUMN_REFERENCE_CHAIN       activation 0.62
  filled: mention="my dentist appointment", antecedent=null (first mention)
```

Hypergraph delta:

```text
added evidence:    e1
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
column updates:
  COLUMN_CHANGE_EVENT.slot_transitions[from][to] strengthened
  COLUMN_CHANGE_EVENT.outgoing_wiring[reschedule_event_1] strengthened
  COLUMN_TEMPORAL_EXPRESSION.outgoing_wiring[Tuesday], [Friday] strengthened
  Hebbian: COLUMN_CHANGE_EVENT <-> COLUMN_TEMPORAL_EXPRESSION wiring strengthened
```

Returned `ConsciousAttentionState`:

```text
intent: Statement
active_frame: user_schedule
active_regions: appointments, dental_appointments
active_columns:
  COLUMN_CHANGE_EVENT             voted for: reschedule_event_1, appointment_1
  COLUMN_ENTITY_MENTION           voted for: DrRao
  COLUMN_TEMPORAL_EXPRESSION      voted for: Tuesday, Friday
  COLUMN_STATE_WITH_TEMPORAL_VALUE voted for: appointment_1 current_time Friday
focused_claims:
  appointment_1 current_time Friday    (boosted by 2 column votes)
  reschedule_event_1 from Tuesday
  reschedule_event_1 to Friday
answer: None
durable_writes: e1, appointment_1, reschedule_event_1
next_actions: watch for future corrections to appointment_1
```

Note: `appointment` here is a *learned* atom emerged from this tick's
extractor proposals, not a seed column. The seed pack ships only the
mechanical predictors listed above — `COLUMN_CHANGE_EVENT` etc. — none
of which presume the appointment domain.

### Tick 2

Input:

```text
I have an appointment at the body shop on Tuesday.
```

Delta:

```text
added evidence:    e2
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
added evidence:   e3
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
answer: Friday (evidence: e1)
uncertainty: exact calendar date unknown
```

### Tick 4

Input:

```text
What do I have on Tuesday?
```

Delta:

```text
added evidence:   e4
no new atoms or claims
reinforced path: query -> Tuesday -> [filter current] -> appointment_2
```

Returned state:

```text
intent: Question
focused_claims:
  appointment_2 current_time Tuesday
answer: body shop appointment (evidence: e2)
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
added evidence:   e5
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

**Active columns this tick:**

```text
COLUMN_CHANGE_EVENT          activation 0.94 (highest yet — its
                             expected_slots fill_probabilities were
                             strengthened on Tick 1)
  predicted: target slot, from slot, to slot
  filled: target=appointment_1 (via coref), from=Friday, to=Monday
COLUMN_REFERENCE_CHAIN       activation 0.79
  filled: mention="it", antecedent=appointment_1 (from recent_focus)
COLUMN_ENTITY_MENTION        activation 0.71  (the dentist cue)
COLUMN_TEMPORAL_EXPRESSION   activation 0.83
```

`COLUMN_CHANGE_EVENT`'s `expected_slots[from].fill_probability` and
`expected_slots[to].fill_probability` rose during Tick 1's Step 12, so
Step 6b on Tick 5 emits stronger priors for both slots than it would
on a cold start. Extractor attention is biased accordingly. The
`slot_transitions` matrix is also stronger now, but it only fires at
this tick's Step 12 to update *future* learning — not at Step 6b
(extractors haven't run yet).

Returned state:

```text
intent: Correction
focused_claims:
  appointment_1 current_time Monday        (boosted by COLUMN_CHANGE_EVENT
                                             + COLUMN_STATE_WITH_TEMPORAL_VALUE votes)
  appointment_1 previous_time Friday  [Superseded]
answer: None
uncertainty: "it" resolved to dentist appointment via recent focus + dentist cue
             COLUMN_REFERENCE_CHAIN flagged ambiguity-then-resolved
```

### Tick 6

Input:

```text
When is my appointment with Dr. Rao now?
```

Delta:

```text
added evidence:   e6
no new atoms or claims
reinforced path: query -> DrRao -> appointment_1 -> current_time -> Monday
```

Returned state:

```text
intent: Question
focused_claims:
  appointment_1 current_time Monday
answer: Monday (evidence: e5)
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
added evidence:   e7
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
added evidence:   e8
no new atoms or claims
reinforced path: query -> body_shop -> appointment_2 -> purpose -> oil_leak
```

Returned state:

```text
intent: Question
focused_claims:
  appointment_2 purpose oil_leak
answer: oil leak (evidence: e7)
```

### Tick 9

Input:

```text
Dr. Rao is my dentist.
```

Delta:

```text
added evidence:   e9
matched existing: DrRao column
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
reinforced: DrRao column, dentist relationship
answer: None
```

### Tick 10

Input:

```text
What appointments do I have?
```

Delta:

```text
added evidence:   e10
no new atoms or claims
gathered: appointment_1, appointment_2
filtered: current non-Retracted, non-Superseded current_time claims
```

**Active columns this tick:**

```text
COLUMN_QUESTION              activation 0.86
  predicted: expected_answer_kind = enumeration over appointment-typed atoms
COLUMN_AGGREGATION           activation 0.74
  predicted: count/list over a typed set
COLUMN_STATE_WITH_TEMPORAL_VALUE  activation 0.69
  votes for: appointment_1.current_time, appointment_2.current_time
COLUMN_ENUMERATION           activation 0.55
```

After 10 ticks the wiring graph has settled enough that
`COLUMN_QUESTION` + `COLUMN_AGGREGATION` co-activating reliably retrieves
the right superset of claims. By Tick ~50 the aggregator-style query
path will be fast and direct.

Returned state:

```text
intent: Question
focused_claims:
  appointment_1 current_time Monday        (3 column votes)
  appointment_1 provider DrRao             (1 column vote)
  appointment_2 current_time Tuesday       (3 column votes)
  appointment_2 purpose oil_leak           (1 column vote)
answer:
  - dentist appointment with Dr. Rao: Monday
  - body shop appointment: Tuesday, for an oil leak
evidence: e5, e2, e7
uncertainty: exact calendar dates unknown unless Monday/Tuesday were grounded
```

This walkthrough is the **first conformance fixture**. The inspection
harness (§16) asserts the returned attention state, the internal
hypergraph state, *and* the active-column trace after each tick.

---

## 15. Evaluation

### 15.1 Co-Primary Metrics

The 2025 consensus stack: recall + faithfulness + abstention. v0 metric
floor is the first three:

1. **Evidence recall@k.** If the answer-bearing evidence is not retrieved,
   the system failed before any reader gets involved.
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
Can the compressed memory still recover the answer-bearing fact and evidence?
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

### Step 2 — Event Log + Snapshot Lifecycle (~1 wk)

**Build:** §5.7 — append-only log, snapshots stamped `Tick + fingerprint`,
hybrid checkpoint (N=1000 ∨ S=10 MB ∨ T=1 hr), snapshot+truncate
compaction, boot path replays log suffix.
**Done:** crash mid-corpus → restart → state matches; post-truncation
state still recoverable.

### Step 3 — Seed Pack (~2.5 wk)

**Build:** Hand-author §7's 16 regions + 11 schemas + **30 domain-neutral
columns** + Genesis/Void + ~12 Roles + ~8 Frames. Embed at boot.
Serialize as `seed_v0.msgpack.lz4`.
**Done:** boot shows ~80 atoms in expected configuration; 2D projection
of descriptor embeddings clusters sensibly; no column has a domain-
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

### Step 5.5 — Column Activation + Predict-Next (~1.5 wk)

**Build:** §11.10 + §9.7.5 + §9.14 — `activate_columns` (par_iter),
`predict_next` (slot priors only — Step 6b runs before extraction),
`aggregate_focus` (RRF-like).
**Done:** §14 Tick 1 activates `CHANGE_EVENT` / `TEMPORAL_EXPRESSION` /
`ENTITY_MENTION` with expected slot predictions; column votes appear
in `focused_claims` provenance; tick latency <5 ms for 50-atom seed
pack.

### Step 5.7 — Column Dynamics (~1.5 wk)

**Build:** §13.9 — `update_columns` (bounded Hebbian on expected slots,
slot transitions, wiring, reliability) + plasticity decay.
**Done:** across §14's 10 ticks, `slot_transitions[from][to]`
strengthens monotonically; mature plasticity decays; co-activation
wiring rises.

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

**Build:** §9.9 — recency-based pronoun resolution (Centering Theory
baseline) + pattern separation.
**Done:** Tick 5 "it" → `appointment_1`; `appointment_1` and
`appointment_2` stay separate; Tick 9 reinforces `DrRao` instead of
duplicating. §14 + the three §15.5 fixtures pass end-to-end.

### Step 9 — Lexical Index + Hybrid Retrieval (~1 wk)

**Build:** `tantivy` BM25 over evidence + labels + aliases; RRF fusion
of dense + sparse.
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
links decay, evidence is preserved.

### Step 11 — Replay (~2.5 wk)

**Build:** §5.4 + §13.8 — replay thread gets a snapshot clone, returns
`Vec<ReplayMutation>` for `&mut` apply on main; region split/merge,
coref resolution, prototype eviction, cache pruning, **column
emergence + merge/prune**. Reject any mutation that breaks §14 or §15.5.
**Done:** 100-tick corpus passes §14 + fixtures; region- and column-
creation rates flatten; an `appointment` column emerges in a heavy-
appointment fixture, a `function_def` column in a heavy-codebase
fixture, neither in the seed pack.

### Step 12 — External Benchmarks (~2 wk)

**Build:** wire LongMemEval `oracle`, MemoryAgentBench
FactConsolidation, RULER MK/MV-NIAH at 8K/32K into the harness.
**Done:** end-to-end numbers logged. Beating SOTA is *not* the v0 goal;
passing fixtures + credible external numbers is.

**v0 sign-off** = Steps 0–12 pass + §14 deterministic with column
traces + §15.5 fixtures + Step 9.5 fixture + LongMemEval +
MemoryAgentBench + RULER all produce credible numbers.

**Total: ~22 wk part-time.** Crate-reuse savings (Steps 0, 6, 9) cover
the column-dynamics additions (Steps 5.5, 5.7, +0.5 wk on Step 11).

### Reviewer Workflow

User writes code → runs the inspection harness → pastes diff → Claude
reviews diff + code, flags spec drift → user iterates. Step is done
when the harness shows zero unexpected diffs across the walkthrough up
to that step.

---

## 17. Source Map

Read in priority order. The first six are load-bearing for v0; the rest
are background and reference.

### Substrate / Algorithm

1. **DDVFA** — Brito da Silva, Elnabarawy & Wunsch, *Neural Networks* 116 (2019), arXiv 1901.00794. Closest published kin to §8 (two-level vigilance + multi-prototype + Merge-ART). **Read end-to-end before writing v0 region code.**
2. **ART Survey** — Brito da Silva et al. 2019, arXiv 1905.11437. Failure-mode catalogue used for §8.8.
3. **Adaptive Resonance Theory** — Carpenter & Grossberg. Vigilance / resonance / stable-plastic learning; conceptual backbone of §8.
4. **GNG + GWR** — Fritzke 1995; Marsland, Shapiro & Nehmzow 2002. GWR's activation-and-firing-counter add criterion is closer to Legend's `descend_threshold` than vanilla GNG.

### Cortical Columns

5. **Mountcastle 1957** — *J. Neurophysiol.* 20(4):408-434. Original cortical-column finding; foundation for §4.3.
6. **Hawkins & George (Numenta) — HTM papers.** Inspirational, also a cautionary tale about over-committing to consensus voting.
7. **Hawkins 2021 — *A Thousand Brains*.** Predict-next + voting framing taken at low fidelity (slots-only, voting-as-aggregation).
8. **Hawkins et al. 2017 — *Frontiers in Neural Circuits*.** Load-bearing TBT paper.
9. **Oja 1982 — *J. Math. Biol.* 15:267-273.** Bounded Hebbian operators for column updates (§13.9).

### Truth Maintenance / Temporal / Provenance

10. **Event Calculus** — Kowalski & Sergot, *New Generation Computing* 4(1), 1986. 40-year foundation for §13.4. Shanahan's modern formulation: doc.ic.ac.uk/~mpsha/ECExplained.pdf.
11. **PROV-O (W3C 2013)** — vocabulary for `derived_from` and `supersedes` (Invariant 14, §4.6).
12. **Wikidata data model** — statements/qualifiers/references/ranks; behind §4.6 + §4.8.
13. **JTMS / ATMS** — Doyle 1979; de Kleer 1986. Legend's claim-status discipline is JTMS-flavored.
14. **AGM + Hansson Base Revision** — Levi identity is the formal name for Legend's correction protocol.
15. **TimeML / TempEval-3** — temporal annotation standard; Legend adopts the 7-relation pragmatic subset.

### Event Sourcing / Materialized Views

16. **Greg Young — CQRS / Event Sourcing 2014.** "The snapshot is a memoization of your left fold." Foundation for §5.7.
17. **Datomic operational architecture** — reference event-sourced indexed-cache design.
18. **XTDB bitemporality** — two time axes; mandates Invariant 7.
19. **Differential Dataflow** — McSherry, Murray, Isaacs et al., CIDR 2013. Diff-passing discipline (§9.1.1).
20. **Salsa** — Rust pure-spec / `&mut`-impl pattern (rust-analyzer). Closest existing Rust analog to Legend's brain-processes-as-functions discipline.
21. **IVM** — PostgreSQL IVM wiki + Cui & Widom (TODS 2000). Background for `derived_from`.

### Comparable Memory Systems

22. **Graphiti / Zep** — Rasmussen et al. 2025, arXiv 2501.13956. Bi-temporal KG for agent memory; closest production competitor to §4.6 + §4.8 — read before finalizing supersession spec.
23. **HippoRAG 2** — Gutiérrez et al. 2025, arXiv 2502.14802. Dual-node KG + Personalized PageRank; path-aware reinforcement competitor.
24. **A-MEM** — NeurIPS 2025, arXiv 2502.12110. LLM-driven memory evolution.
25. **Mem0** — arXiv 2504.19413. Hybrid vector + graph + KV memory layer.

### NLP / Embedding / Retrieval

26. **Sentence-BERT** — Reimers & Gurevych 2019, arXiv 1908.10084. Why raw BERT is not an embedding model.
27. **BGE technical report** — arXiv 2309.07597. The v0 embedding model.
28. **GLiNER paper** — arXiv 2311.08526. Zero-shot NER used by `gline-rs`.
29. **`tokenizers`** — HuggingFace, Apache-2.0, pure-Rust.
30. **`tantivy`** — Quickwit-OSS, Lucene-grade BM25 in pure Rust.
31. **`gline-rs`** — fbilhaut, GLiNER inference on `ort`.
32. **`ort`** — pyke.io, Rust ONNX Runtime wrapper.

### Cognitive Background

33. **FrameNet** — frames + frame elements; informs §4.8 qualifiers.
34. **AMR paper** — sentence meaning as graph; design reference only, not v0.
35. **Centering Theory** — Grosz, Joshi, Weinstein 1995. Recency-based coreference baseline.

### Benchmarks

36. **LongMemEval** — ICLR 2025, arXiv 2410.10813. v0 evaluation gate.
37. **MemoryAgentBench** — ICLR 2026, arXiv 2507.05257. Fact Consolidation = supersession semantics test.
38. **RULER** — COLM 2024, arXiv 2404.06654. MK/MV-NIAH smoke tests.
39. **AbstentionBench** — FAIR 2025, arXiv 2506.09038. "Don't hallucinate when you don't know."

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
- What is the right cold-storage policy after v1? v0 keeps all evidence in
  memory.
- What is the right replay scheduling cadence? Per-tick? Every N ticks?
  Idle-only? Profile in v0 step 9.
- Should query success reinforce only the selected path, or also nearby
  alternatives at lower weight? v0 does selected-only; revisit once
  reinforcement metrics are visible.
- Should `HashMap` swap to `hashbrown` or a hand-rolled open-addressing
  table? Decide on first profile, not earlier.
- When does the wide `AtomStats` struct split into parallel `Vec<f32>`
  arrays for cache locality? Decide on first profile.
- When does `HNSW` (or another approximate-NN index) get added on top of
  the region DAG for fast lookup? When the DAG search becomes a measurable
  bottleneck.
