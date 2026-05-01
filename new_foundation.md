# New Foundation

> **Status:** Living architecture spec for **Legend v2** — long-term memory
> for LLMs, built as a hypergraph that accumulates discoveries about its
> world. One primitive (Elements), typed connections (Relations), and a
> single-verb API (`tick`). 
> **Audience:** A solo developer should be able to read this top to bottom
> and start coding against it without consulting prior versions of Legend.

---

## 0. Reading Guide

This document is structured in three layers. Read them in order; jump back
when something later refers to something earlier.

**Layer 1 — Orientation (§1–§5).** By the end of §5 you will have a
complete mental model of Legend.

- §1 — **Executive Summary.** The whole design, compressed. If you read
  nothing else, read this.
- §2 — **Goal.** What Legend exists to do.
- §3 — **The Four-Piece Architecture.** Elements, Relations, Discoveries,
  Emergence. The conceptual frame the rest of the doc rests on.
- §4 — **How A Tick Works (Conceptual).** Walk an input through Legend
  end to end without dropping into types yet.
- §5 — **Hard Invariants.** Non-negotiables.

**Layer 2 — Technical deep dive (§6–§18).** Spec-level detail. Read in
order if implementing; skim if reviewing.

- §6 — Mathematical Foundations.
- §7 — The Substrate (Element, Relation, Payload Tables).
- §8 — Recognition Indices.
- §9 — Core Data Model.
- §10 — Semantic Regions.
- §11 — The Tick Pipeline.
- §12 — There Is No Query.
- §13 — Brain Processes As Functions.
- §14 — Algorithms.
- §15 — Model Stack.
- §16 — The Seed Pack.
- §17 — Carry-Forward From Current Legend.
- §18 — Durability.

**Layer 3 — Examples + Reference (§19–§24).**

- §19 — Worked Ten-Tick Walkthrough.
- §20 — Evaluation.
- §21 — Build Order.
- §22 — Source Map.
- §23 — Deferred Questions.
- §24 — Beyond v0.

---

## 1. Executive Summary

This section gives you the entire design, compressed. The rest of the doc
is detail.

### 1.1 What Legend Is

Legend is **long-term memory for LLMs** — including future sessions of
the model reading this document. LLM sessions are fleeting by default;
Legend is the persistent substrate that lets continuity carry across
them. It is not a chatbot, not a RAG store, not a knowledge graph
database in any conventional sense. It is a **living model of the slice
of reality Legend has been told about** — the project, the user, the
domain, the history.

Legend's substrate is **one hypergraph**. Everything Legend remembers
lives there: facts, events, concepts, vector regions, meta-claims about
its own claims. There is no separate vector database, no separate
symbolic store, no synchronization between layers. One structure,
queried through different lenses.

### 1.2 The Four-Piece Story

Legend is built from four conceptual pieces. Each is load-bearing.

**1. Elements — the one primitive.** An Element is a bare identity Legend
can refer to. It has an id, one or more names, and bookkeeping (memory
stats, creation tick). It has no kind tag, no fixed type. Elements are
addressable identities and nothing more.

**2. Relations — typed connections, plus everything else.** A Relation
is an evidence-weighted hyperedge connecting one or more elements
through named role bindings. A relation has a predicate (which is
itself an element), one or more slots (each binding a role-name to a
filler), a confidence weight, a status (asserted, defeasible,
superseded, retracted, entailed), and a creation tick. Anything that
*modifies* a relation — its frame scope, valid-time, source pointer,
modality, supersession links, lineage, conditional antecedents — is
itself a relation whose subject is the modified relation. There is
no annotation layer; there are only relations. Relations are how
Legend says anything about anything, *and* how Legend says anything
about its own claims.

**3. Discoveries — each tick is one.** Legend has one operation: `tick`.
Each tick is a **discovery** — new information arriving, distilled into
elements and relations (including meta-relations on those relations),
evolving Legend's model of its world. There is no separate query path;
question-shaped inputs and statement-shaped inputs flow through the
same pipeline. The output of every tick is an **attention frame**
describing what Legend now knows about the input.

**4. Emergence — kinds are read from the relation graph.** What other
systems pre-declare as types — concept, instance, event, frame —
Legend recognizes through derived indices over relations (§8). An
element that is the target of many `instance_of` relations is
**functioning as** a concept. An element with valid-time meta-relations
and from/to slot relations is **functioning as** a state-change
event. An element that scopes other relations (i.e. many relations
carry `(R, frame, this_element)`) is **functioning as** a reference
frame. Kinds are not stored on elements; they are read from the
structure of relations the element participates in.

This is the deepest commitment of the design. Legend does not pre-decide
what categories exist in the world. It accumulates discoveries; ontology
is what stabilizes.

### 1.3 What A Tick Looks Like (One Example)

User says: *"My dentist appointment with Dr. Rao changed from Tuesday to
Friday."*

Legend processes this as a single discovery:

1. **Embed and route.** The input is segmented into clauses, each
   embedded by a pinned BGE-small encoder, then routed through the
   semantic-region DAG. Active regions: `appointments`,
   `dental_appointments`, `change_history`.
2. **Run extractors.** NER, temporal parsing, zero-shot relation
   extraction produce element/relation candidates: elements for `user`,
   `Dr. Rao`, `dentist`, `appointment_1`, `Tuesday`, `Friday`,
   `reschedule_event_1`; relations binding them. Active regions bias
   the predicate label set toward warm predicates from this part of
   the graph.
3. **Coreference.** Heuristics resolve "my appointment" to a new
   `appointment_1` (no prior anchor); pattern separation prevents false
   merges with any nearby same-name elements.
4. **Build relations and update structure.** New relations:
   `DrRao has_role dentist`, `appointment_1 instance_of appointment`,
   `appointment_1 provider DrRao`, `reschedule_event_1 target
   appointment_1`, `reschedule_event_1 from Tuesday`,
   `reschedule_event_1 to Friday`. A current-state cache relation
   `appointment_1 current_time Friday` is materialized with
   `derived_from = reschedule_event_1`.
5. **Reinforce, decay, surface.** Memory stats bump along the focused
   path. Elements within the focus radius decay slightly; the rest is
   handled by the background sweep. The returned attention frame
   reports the focused relations, durable writes, supersessions, and
   any uncertainty signals — a snapshot of the slice of the world
   Legend now understands after the tick.

Three minutes later the user asks *"When is my appointment with Dr. Rao
now?"* That's also a tick. Same pipeline. The attention frame surfaces
`appointment_1 current_time Friday` (and its derivation chain) as a
focused relation; the calling LLM reads `Friday` off that frame. There
is no separate retrieval path and no pre-assembled answer field.

### 1.4 How The Substrate Is Stored

The hypergraph is one in-memory struct, durably mirrored by snapshot +
WAL.

```rust
struct Hypergraph {
    elements: Vec<Element>,
    relations: Vec<Relation>,

    // Optional payload tables for elements that need structured payloads.
    // An element appears in zero, one, or more. Membership IS the
    // implicit "kind" of the element.
    embeddings: HashMap<ElementId, Vec<f32>>,
    regions:    HashMap<ElementId, RegionPayload>,
    values:     HashMap<ElementId, Value>,

    clock: Tick,
    policy: Policy,
    recent_focus: VecDeque<ElementId>,

    // Derived indices — rebuild on load, never serialize.
    // Includes meta-relation lookups (relation_frame, relation_supersedes,
    // relation_derived_from, ...) and recognition indices (inbound /
    // outbound predicate counts, meta-relation presence). Full list in §9.2.
    by_name:                HashMap<String, Vec<ElementId>>,
    region_children:        HashMap<ElementId, Vec<ElementId>>,
    region_parents:         HashMap<ElementId, Vec<(ElementId, f32)>>,
    relations_by_subject:   HashMap<ElementId, Vec<RelationId>>,
    relations_by_predicate: HashMap<ElementId, Vec<RelationId>>,
    // ... meta-relation and recognition indices ...
}
```

**Durability.** A bounded write-ahead log (10 MB, segmented, queue-style
oldest-eviction; LZ4 hot, zstd-19 closed) sits alongside the hypergraph
for crash recovery between snapshots. Snapshots are stamped with a
`ModelFingerprint` (embedding-model hash, tokenizer vocab hash, code
version) that is checked at boot — refuse to start on mismatch. Boot =
load latest snapshot, replay WAL suffix on top.

**Embedder pin.** The embedder (BGE-small-en-v1.5) is pinned for
Legend's lifetime. A model swap is a deliberate one-way door requiring
re-ingest from any reachable `source` meta-relations. Legend stores
distilled understanding, not transcripts; there is no in-place
re-embedding migration.

### 1.5 Brain Inspiration, Honestly Applied

Legend takes brain function as design inspiration *where it accurately
describes function*, not as decoration:

- **Hippocampus** — episodic encoding and consolidation; in Legend, the
  per-tick discovery write path.
- **Neocortex** — long-term semantic graph; in Legend, the persistent
  hypergraph.
- **Thalamus** — attentional gating; in Legend, region routing.
- **Amygdala** — salience and protection; in Legend, the salience scorer
  that decides what to reinforce.
- **Prefrontal cortex** — intent classification and policy modulation;
  in Legend, the per-tick policy adjuster.

These are functions of the substrate, not modules. None of them own
state. The hypergraph is the only owned thing.

The deeper brain analogy — the one that's load-bearing for the whole
design — is this: **the brain doesn't store events; it stores
distilled abstractions.** Reconstruction rebuilds context from cues,
not from retrieved audit records. Legend does the same. There is no
per-input record. Inputs that produce no extractable atoms are
discarded (the dev-only WAL keeps raw text for debugging; production
has only the distilled relations).

### 1.6 Why One Primitive

Legend's job is to model the entire **written** world that a project
sits in. That means new ontological categories will keep arriving:
new domains, new subject matters, new conceptual frameworks. A typed
substrate (`AtomKind::Concept`, `AtomKind::Event`, …) locks in a
predetermined ontology and forces every new structure to be slotted
into existing categories.

Legend instead bets on **emergent ontology**: one primitive (Element)
plus typed Relations is enough to express anything; categories
crystallize as recurring relation structures stabilize. This matches:

- How **embeddings** already work — meaning emerges from relational
  position, not from a type tag on the vector.
- How the **brain** works — neurons don't have a `NeuronKind::Concept`
  vs `NeuronKind::Event` tag; functional differentiation emerges from
  connectivity.
- How **Wolfram Physics** bets the universe can be modeled — bare
  elements + rewrite rules.
- How **neural networks** succeed at world modeling — typed primitives
  are an obstacle, not a help.

The cost of one primitive is that Legend has to *recognize* kinds
rather than *declare* them — done by reading derived indices (§8) over
the relation graph rather than inspecting a tag.

### 1.7 Legend As A Bounded Observer

There is a deeper reason the design is shaped the way it is, drawn from
Wolfram's notion of **computational irreducibility** (*A New Kind of
Science*, 2002; sharpened in the Wolfram Physics Project). Some
computations cannot be shortcut: the only way to know the outcome at
step N is to actually run all N steps. The computation is its own
simplest description.

The accumulated understanding of a project — what's true, what changed,
what recurring structures the work has produced — is computationally
irreducible in this sense. There is no closed-form way to know "what
should this LLM session know about the project at tick 4127" without
having actually run the prior 4126 ticks of discovery. You have to live
through the discoveries to have the model.

Legend's design accepts this and turns it into discipline. The substrate
is structured to let a **computationally bounded observer** (the LLM
that consumes Legend's output) navigate an irreducible information
stream tractably. Two moves do the work in v0; a third (replay-driven
compression of recurring structures) is on the v1 horizon (§22).

**1. Forward compression: the hypergraph as memoized state.** Each tick
is a step in the irreducible computation. The hypergraph is the running
result. Future ticks, and especially future *queries*, read the running
state instead of replaying history — O(graph traversal) instead of
O(N inputs replayed). This is exactly what the brain does: your
connectome at age 30 encodes the cumulative effect of every prior
experience, so recall doesn't require re-experiencing.

**2. Emergent ontology, not declared ontology.** A typed substrate
(pre-declared concepts, instances, events) would amount to claiming we
already know which slices of the world are reducible. We don't. Which
structures recur, which categories stabilize — that's what the
irreducible computation reveals over time. Pre-declaring would be a
cheat, and the cheat would produce wrong models. Emergent kinds let
Legend *recognize* category membership through derived indices over
the relation graph instead of asserting it on the element. This is the
deepest reason the one-primitive design is the right design.

Legend, then, is an instrument for being a bounded observer of an
LLM-driven project: it runs the irreducible discovery computation,
keeps a compressed running state queryable in real time, and uses
replay to extract the structure that makes future computation cheaper.

### 1.8 What Comes Next In The Doc

If you want to **understand the design**, read §2–§5.
If you want to **implement it**, read §6–§18.
If you want to **see it work**, read §19.
If you want to **build it in order**, read §21.

---

## 2. Goal

**Legend is a memory engine, not a chatbot.** It maintains a persistent,
queryable model of an LLM-driven project's accumulated context — what
was said, what was decided, what changed, what came before — and serves
that context to whatever LLM session needs it next.

### 2.1 The Functional Signature

Legend's entire public surface is one function:

```
Legend(G, x) → (G', A)
```

- `G` — the hypergraph before this tick.
- `x` — the input (text + optional source pointer + wall-clock).
- `G'` — the hypergraph after this tick.
- `A` — the **attention frame** (a `ConsciousAttentionFrame`): a
  structured snapshot of what fired, what's in focus, what changed,
  what's uncertain, what to replay next.

In Rust: `fn tick(&mut Hypergraph, Input) -> ConsciousAttentionFrame`.
The `&mut` is the operational form of `G → G'`.

There is **no separate query path.** Question-shaped inputs and
statement-shaped inputs go through the same pipeline; the output
differentiates. A question produces an attention frame whose
`focused_relations` already contain what the caller needs to read off;
a statement additionally produces durable writes in `G'`. The
distinction is emergent in the result, not an API choice.

### 2.2 What The Substrate Is For

Legend exists to do these things, in order of importance:

1. **Carry continuity across LLM sessions.** A session opens; Legend
   hands it the relevant model of its world. The session works; Legend
   ingests its outputs. The session closes; Legend persists. The next
   session opens against the same model.
2. **Surface the relevant slice of accumulated context on demand** —
   not by retrieving passages, but by walking relations and returning
   the focused subgraph the caller needs to act or speak.
3. **Track change over time** — supersession, current-state, history.
4. **Support multiple frames simultaneously** — a project's frame, a
   user's frame, a domain's frame; relations are scoped, not flat.
5. **Stay small enough to live in RAM and fast enough to feel
   responsive** — v0 ticks ~200–300 ms p50 on commodity hardware; v1
   horizon sub-100 ms via the §24.1 / §24.7 extractor changes.

### 2.3 The Attention Frame Is The Output

Legend's tick returns a **ConsciousAttentionFrame**: a snapshot of the
slice of the world Legend now understands *after* this tick has been
applied. It is not an answer; it is the relevant subgraph and the
metadata the caller needs to read meaning off it. Fields include:

- `input` — an echo of the input that produced this frame (raw text or
  pointer, plus source kind), so the caller has the question/statement
  in hand alongside Legend's response. Read-only, not a hypergraph
  citizen.
- `focused_relations` — the relations Legend believes are most relevant
  to this input, post-tick.
- `active_regions` — the semantic-DAG regions that activated during
  routing, with similarity scores.
- `supporting_claims` — claims behind the focused relations (provenance
  for any fact the caller chooses to surface).
- `history` — superseded relations relevant to the focus, for context.
- `uncertainty` — discrepancies, ambiguities, low-confidence states.
- `durable_writes` — what this tick added to the hypergraph.
- `superseded` — what this tick demoted.
- `next_actions` — replay enqueues, follow-ups.

The attention frame is consumed by whatever LLM session called Legend.
The caller derives any natural-language answer from this frame; Legend
does not pre-assemble one. The frame is the *result* of the discovery,
not a side effect.

### 2.4 Out Of Scope For v0

- Multi-tenant authentication.
- Distributed replication.
- A query language (Legend's only verb is `tick`).
- LLM-internal RAG (Legend serves the model, not the other way around).
- Anything Python or JVM (no sidecars, no exceptions).

### 2.5 First Consumer: A Notes App

The first thing Legend ships against is a minimal notes app — input
field in, rendered output out, CLI to start. It exists to prove Legend
works end-to-end against a real consumer, before it's wired into
larger agentic systems.

The shape:

1. The user types a thought.
2. The frontend hands the raw text to Legend as one tick. One thought
   per tick — no batching, no segmentation in the frontend.
3. Legend returns a `ConsciousAttentionFrame`.
4. A **tiny render LLM** (Qwen-0.5B class) verbalizes the frame as
   natural language for display. It does not reason over the frame; it
   formats it. Substantive content lives in `focused_relations` /
   `supporting_claims` / `history` — the render LLM picks readable
   phrasing.

Two things this scopes for v0:

- **The render LLM's job is open.** Render-only is the starting
  configuration. Whether the tiny LLM should also do light reasoning,
  follow-up question generation, or formatting choices specific to
  intent will be decided by playing with the notes app — and the
  answer will shape the system-prompt-style instructions any larger
  caller (Claude Code, an agent harness) gets for how to use Legend.
- **Coding-project use is the parallel test.** The notes app exercises
  Legend on free-form personal text. Coding-project use exercises it
  on file events, decision ticks, BUG/DECISION/ARCHITECTURE prefixes.
  Both run against the same substrate; divergence in quality between
  them is a signal about the substrate, not about either frontend.

The notes app is not a deliverable of the substrate spec — it has its
own repo and its own surface — but it is the first end-to-end gate the
substrate is judged against in §21.

### 2.6 What This Buys You

§2.2 names what Legend *is for*; this section names what the user, the
LLM, and the surrounding system actually get out of having Legend.

**For the LLM (cognitive):**

1. **Cross-session continuity.** A session opens against the same
   model of the world the previous session left. The headline benefit.
2. **Reduced hallucination on grounded claims.** Factual responses
   come from a verifiable retrieved subgraph with provenance, not from
   parametric recall. Supersession labels stale facts as superseded
   rather than letting them surface as current. Same mechanism that
   gives RAG its hallucination-reduction edge, applied to accumulated
   session state instead of static corpora.
3. **Correct temporal reasoning.** Bitemporal split + supersession
   chains let the LLM answer "what's true now?", "what *was* true?",
   and "when did it change?" — three questions a flat doc store
   conflates.
4. **Compound improvement over time.** Path-aware reinforcement and
   replay-driven mid-path insertion mean the hundredth tick of a
   recurring topic is faster and more confident than the first.
   Memory that gets smarter, not just bigger.

**For the user (human):**

5. **No re-explanation tax.** You don't restate context to a new
   session. The "let me catch you up" overhead is absorbed by Legend.
   This is human minutes saved, not just tokens.
6. **Auditable provenance — structural, not textual.** `derived_from`,
   source pointers, and supersession lineage let you trace *why*
   Legend believes anything it believes: which event reified which
   cache relation, which input introduced which element, which
   correction superseded which prior claim. The audit trail is the
   chain of relations, not a stored transcript — Legend does not
   double-store source content. Recovering "what the user actually
   typed" depends on the source's own retention (Slack history, git
   commits, the file's current content); Legend's audit trail tells
   you the lineage and points you at where the original lives.
7. **Privacy and locality.** The hypergraph is in-memory + local
   snapshot. No vendor sees your accumulated context. Most "memory"
   products ship your data to a third party; this one doesn't.
8. **Model-agnostic.** The same Legend works against Claude, GPT,
   Qwen, local Llama. Switch models without losing accumulated
   context. The model is interchangeable; the memory is yours.

**For the system (economics):**

9. **Token and latency economy.** Today's norm is "stuff CLAUDE.md,
   recent git log, and grep results into every session" — input tokens
   proportional to project size, paid every session. Legend turns that
   into "fetch the focused subgraph for *this* input": O(focus) instead
   of O(project). Tick latency budget is ~200–300 ms p50 in v0,
   dominated by zero-shot relation extraction (GLiNER2, §15.1). The
   v1 horizon targets sub-100 ms via pattern fast-paths (§24.1) and
   the unified tiny-LLM extractor (§24.7); read-path/background-work
   splitting (§24.2) helps but is not the long-pole solution. Either
   way it replaces minutes of cold ingestion. Savings compound as the
   project grows.
10. **Multi-tool / multi-agent coordination.** Multiple consumers
    (two Claude Code sessions, an agent harness, the §2.5 notes app,
    an MCP client) read and write the same Legend. Continuity isn't
    just one agent across time — it's across agents at once.
11. **Personalization without fine-tuning.** Preferences, style,
    vocabulary, project conventions accumulate as relations and
    reinforcement weights. Personalization lives in the substrate, not
    in model weights. Cheaper, reversible, auditable, portable across
    models.
12. **Tiny consumer models become viable.** Because Legend produces
    the substantive cognitive output (focus, supersession, structural
    reasoning), the consumer LLM can be small enough to run locally —
    see §2.5, where Qwen-0.5B verbalizes the attention frame as the
    notes-app render layer.

**What this is not:**

- Not perfect recall. Legend is reconstructive and lossy by design
  (Inv 2).
- Not a replacement for document-corpus RAG. Legend is an
  accumulated-state substrate; static document retrieval is a
  different problem.
- Not a hallucination cure-all. Hallucinations on ungrounded or
  out-of-scope claims are unaffected; only the grounded-fact subset
  benefits.

---

## 3. The Four-Piece Architecture

This section names the four pieces of Legend's design and what each
does. Read this before §4 (the conceptual walkthrough) and §7 (the
substrate spec) — both refer back here.

### 3.1 Elements: The One Primitive

An **Element** is a bare identity Legend can refer to. It has:

- An id (`ElementId`, `u32`).
- Zero or more names (strings — canonical, variant, alias all in one
  list; lifecycle is uniform).
- Memory stats (activation, strength, stability, confidence,
  plasticity, salience, access count, last seen, etc.).
- A creation tick (transaction time).

That is everything an Element has structurally. No kind tag. No fixed
type. Elements are addressable identities; their meaning emerges from
the relations they participate in and from any payload-table entries
they accumulate.

Why one primitive: §1.6 explains the design bet. The short version is
that meaning emerges from relational position, not from a type
declaration; pre-declaring categories locks in an ontology Legend has
no business locking in.

### 3.2 Relations: Typed Connections

A **Relation** is a typed hyperedge between elements. Relations are
*how Legend says anything about anything* — and *how Legend says
anything about its own claims*. A Relation has:

- A **predicate** — itself an Element. The predicate's names tell you
  what kind of relation this is (`instance_of`, `provider`,
  `current_time`, `target`, `from`, `to`, or any meta-predicate like
  `frame`, `valid_from`, `source`, `modality`, `supersedes`,
  `derived_from`).
- One or more **role bindings** — each binding a role-name (an
  Element) to a filler (a `Term`: Element, Relation, or Variable).
- A **confidence weight** in memory stats.
- A **status** — asserted, entailed, defeasible, superseded, retracted.
- A **priority** for defeasible tie-breaking.
- Memory stats — relations decay and reinforce just like elements.

That's it. Relations have no qualifier struct, no `supersedes` /
`derived_from` fields, no status sub-fields. Anything that *modifies*
a relation — frame scope, valid-time, source, modality, supersession
chain, lineage, conditional antecedent — is itself a relation whose
subject is the modified relation, via `Term::Relation(RelationId)`.

Relations are first-class memory citizens. They are not passive labels
on edges; they participate in attention, decay, reinforcement, and
supersession. The dynamics of memory apply uniformly to elements and
relations — the only difference is structural (relations have endpoints;
elements don't).

A relation can represent anything an n-ary predicate can represent:
binary triples, multi-arg events, nested claims, conditional rules,
time-scoped facts, modal assertions. One structure, one mechanism for
both world claims and the meta-claims that contextualize them.

**Claims about claims about claims.** Because a meta-relation is
itself a Relation, it can carry its own meta-relations — recursively,
without bound. The substrate does not cap depth, and nothing in the
type system distinguishes level-1 claims from level-N claims. This is
not an accident; it falls out of `Term::Relation(RelationId)` being a
first-class filler.

What the recursion buys:

- **Uncertainty about meta-claims.** `(R, frame, dental)` can itself be
  marked `Defeasible`, modalized via `(meta_R, modality,
  MODAL_POSSIBLE)`, or superseded. "I'm not sure that scope assertion
  was right" is just a modality on a frame meta-relation.
- **Belief revision uniform at every level.** Revising "appointment_1
  is on Friday" and revising "this scope was actually medical, not
  dental" are the same operation: mark old `Superseded`, write new,
  link with `(R_new, supersedes, R_old)`. Whether the relation being
  revised is a world claim or a claim about a claim doesn't matter to
  the supersession machinery.
- **Provenance is fully addressable.** Every relation has a
  `RelationId`. Anything the substrate can say, it can say about
  anything else the substrate has said — including its own past
  decisions. Replay can record `(merge_decision_relation, derived_from,
  evidence_cluster_relation)` to make its own history queryable.
- **Conditional meta-relations.** "This frame scope only holds while
  X" is a meta-relation with its own `antecedent_of` meta-relation.
  The contingency is in the graph, not in special-case code.

What the recursion costs in practice: nothing the substrate doesn't
already cost. Most relations will have zero meta-relations; of those
that have meta-relations, most won't have meta-meta-relations. The
substrate supports depth; it doesn't require it. Storage and retrieval
scale with what's actually written, not with what could be.

Two structural consequences worth flagging:

1. **Indices are flat, not recursive.** `relation_frame[R]` returns
   the frame *value* (an ElementId), not the meta-relation that asserted
   it. To reason about the meta-relation itself — its status, source,
   modality — query `relations_by_subject[R]` and filter to
   `predicate = frame`. Hot path uses the value index; reflective
   reasoning walks the graph.
2. **Cycles need a resolution story.** `(R₁, derived_from, R₂)` and
   `(R₂, derived_from, R₁)` is structurally well-formed but
   semantically inconsistent. Replay detects provenance cycles
   (§14.8) and retracts the lowest-confidence relation in the cycle;
   ties are broken by older `created_at` (the older claim loses,
   since the newer one is more likely the current-state-bearing
   version). v0 does not enforce write-time acyclicity (cheaper to
   fix in the background than to add ceremony to the hot path).
   Codified as Invariant 15.

**v0 reads depth-1 only.** The substrate stores arbitrary-depth
meta-relations via `Term::Relation`, but every v0 behavior — recognition
indices, supersession, region routing, frame-scoped retrieval — reads
depth-1 via the flat indices above. Depth-N traversal exists only as a
private replay helper used by cycle resolution (§14.8). Depth-2+
reasoning shapes (supersession of meta-relations, modal-on-meta,
provenance walks across meta-meta-relations) are deferred to v1, where
empirical signal from real use will guide the semantics. Storage is
unbounded; behavior is depth-1.

### 3.3 Discoveries: Each Tick Is One

A **discovery** is what we call a tick when we want to emphasize what it
*means* rather than how it *executes*. A discovery is one piece of new
information arriving and evolving Legend's model.

Discoveries are the dynamic of Legend. Every tick:

1. **Brings new information** — a user message, a tool observation, a
   file event, an LLM session output.
2. **Distills it** — extracting elements and relations (including
   meta-relations on those relations); not preserving the raw text.
3. **Reconciles it with prior beliefs** — coreference merges, pattern
   separation prevents false merges, supersession marks what changed.
4. **Reinforces what fired and decays what didn't** — the focused path
   strengthens; idle relations weaken slightly.
5. **Returns an attention frame** — a snapshot of what was just
   discovered and what's now in focus, ready for the caller to read
   meaning off of.

The hypergraph is not a database Legend writes to. It is Legend's
*current best model of its world*, updated by each discovery. Reinforce,
decay, supersession aren't three independent mechanics — they are three
faces of how a discovery interacts with prior beliefs:

- **Reinforce** — this discovery confirms what I already knew; the path
  strengthens.
- **Supersede** — this discovery contradicts what I held; revise.
- **Decay** — this knowledge wasn't reinforced; weaken its grip until
  something brings it back.

This is belief evolution under new evidence, expressed mechanically.

### 3.4 Emergence: Recognition Through Indices

Legend has no `ElementKind` enum. The "kinds" that other knowledge
systems pre-declare — concept, instance, event, reference frame —
are **structures of relations** here, recognized by reading derived
indices over the relation graph. Nothing is stored as a kind tag;
nothing fires a "this is now a concept" event. Behavior conditions
on index values directly.

The four canonical recognitions:

- **Concept** — element with `inbound_predicate_counts[E][instance_of]
  >= policy.concept_recognition_threshold` (default 3). Many other
  elements point at it as their type.
- **Instance** — element with non-zero
  `outbound_predicate_counts[E][instance_of]`. It is itself an instance
  of one or more concepts.
- **Event** — element appearing as the subject of role-binding relations
  (`target`/`from`/`to`/`actor`/`time`) where those relations carry
  valid-time meta-relations (`(R, valid_from, T)` / `(R, valid_to, T)`).
- **Reference frame** — element with `inbound_predicate_counts[E][frame]
  >= policy.frame_recognition_threshold` (default 5).

A given element can be functioning as multiple kinds at once.
`healthcare_provider` is both an instance (of `concept`) and a concept
(`dentist` is an instance of it). Both index entries are non-zero;
both behaviors apply.

**How recognition affects behavior.** Each recognition reads an index
threshold and routes a specific behavior:

- **Coreference** (§14.3) reuses concept-like elements broadly and
  treats instance-like elements with pattern separation. The merge
  bias is computed from `inbound_predicate_counts` directly.
- **Supersession** (§11.10) fires when a relation's
  meta-relation-presence indicates state-change shape (`from`/`to`
  on a `property` plus valid-time).
- **Frame-relative scoping** uses `relation_frame[R]` to filter
  retrieval. Elements with many inbound `(?r, frame, ?)` references
  are recognized as frames at query time. **Frame scope is flat in
  v0.** A relation either is or is not in a given frame; there is no
  transitive inheritance — a relation scoped to `FRAME_PROJECT`
  doesn't automatically inherit `FRAME_USER` even if the project is
  the user's. Cross-frame visibility must be expressed by explicit
  meta-relations. (Hierarchical/composed frames are a v1+ idea, §22.)
- **Decay** treats event-like elements (valid-time-bounded) and
  persistent state differently.

These are observed properties of the graph, not stored tags.
Recognition is a function of the indices; the indices are derived
from the relations.

**The seed pack is bootstrapping, not a closed vocabulary.** Predicates,
concepts, and roles emerge by default — Step 6 (§11.7) mints new
predicate elements from extractor proposals. The handful of predicates
that *are* seeded (`instance_of`, `subclass_of`, role predicates, and
the eight meta-relation predicates — see §16) are seeded because they're
**load-bearing for emergence recognition itself**: the recognition
rules in this section refer to `instance_of` by name, so without it
present at boot, replay would have to rediscover one of its own
logical primitives before any other emergence could be observed.
Seeding fixes those anchors so the rest of the lattice can be
discovered cheaply. Seeded predicates are not privileged in code —
there's no `if predicate == INSTANCE_OF` branch — only privileged in
being *present at boot*.

### 3.5 Side Tables: Where Structured Payloads Live

Some elements need structured payloads — vectors, prototypes, typed
values. These live in **side tables** keyed by `ElementId`:

- `embeddings` — `HashMap<ElementId, Vec<f32>>`. Any element that has a
  semantic anchor (a region, a value-typed atom, a labeled concept).
- `regions` — `HashMap<ElementId, RegionPayload>`. Elements that
  function as semantic regions; payload holds prototype vectors,
  vigilance, density, etc.
- `values` — `HashMap<ElementId, Value>`. Elements whose meaning is a
  typed value (text, number, time-point, weekday, location, etc.);
  payload holds the typed content.

An element appears in **zero, one, or more** side tables. The
intersection of its side-table memberships **is** its emergent
structured kind. An element with a `regions[id]` entry is a region;
most elements have no side-table entries and live entirely in the
relation graph.

Side tables are pre-defined in the substrate (embeddings, regions,
values). Most relations and elements have no side-table entries and
live entirely in the relation graph.

---

## 4. How A Tick Works (Conceptual)

This section walks an input through Legend without dropping into types
yet. §11 specifies the same pipeline at the type level.

A tick is one call into Legend: one input, one updated hypergraph, one
attention frame returned. Legend has no separate query path — every
interaction (user message, file event, agent observation) becomes an
`Input`, gets handed to a single function, and produces a structured
snapshot of what's now in focus.

### 4.1 Input

An `Input` carries:

- `text: String` — the new information.
- `source: Option<ElementId>` — pointer to a source element (Slack
  message id, file path, URL — modeled as ordinary elements). Optional
  because most ticks (agent-internal, ephemeral) have no useful
  external pointer.
- `wall_clock: SystemTime` — for log entries only; never drives
  substrate logic.

Inputs are one stream. Discoveries arrive in tick order; tick order is
transaction time.

### 4.2 The Fourteen Steps

Each tick threads through 14 steps (0–13). Steps 1–7 are read-mostly
and parallelize where possible under `&Hypergraph`. Steps 8–13 are
sequential under `&mut Hypergraph`. Every tick — statement, question,
correction — runs the full pipeline; intent modulates *policy*, never
which steps run.

```
0.  log entry                  -> append (Tick, Input, ModelFingerprint) to WAL
                                  -- READ-MOSTLY PHASE BEGINS (&Hypergraph) --
1.  detect intent              -> AttentionIntent
2.  adjust policy              -> Policy updated for this tick
3.  segment text               -> spans (sentence/clause/entity/value)
4.  embed every span           -> Vec<(span, embedding)>
5.  route through region DAG   -> active regions per span + RegionDelta (held)
6.  run extractors             -> element/relation proposals with confidence
7.  coreference scoring        -> element reuse vs. provisional new
                                  -- MUTATION PHASE BEGINS (single &mut Hypergraph) --
8.  apply region delta         -> commit prototype updates / new regions / attachments
9.  build relations and events -> appended to hypergraph with status
10. supersede + derive cache   -> mark old current-state Superseded; write new with derived_from
11. apply Hebbian + salience   -> MemoryStats (elements + relations)
12. apply focus-radius decay   -> nearby idle elements/relations weaken
                                  (full-graph sweep runs in replay)
13. assemble attention frame   -> ConsciousAttentionFrame returned
    enqueue replay             -> hand snapshot to replay thread
```

Step 0 is the WAL append. It happens *before* Step 1 so that even if a
later step panics, the WAL entry is durable in dev (production discards
inputs that produce no extractable atoms — there is no input citizen to
preserve).

### 4.3 Pre-Mutation Diagnosis: What We Learn Before Changing State

Every tick has a clean phase split. Steps 0–7 *diagnose* the input
without touching hypergraph state. Steps 8–13 *commit* what falls out
of the diagnosis. The diagnosis is read-only and parallelizable; the
commit is sequential under `&mut`. Nothing in the diagnosis decides
"should we mutate" — every tick mutates. The diagnosis decides *what*
to commit and *at what weight*.

Step-by-step, what the diagnosis extracts and where each piece pays
off:

```text
step  what it extracts                         why                                                         pays off in
0     WAL entry (Tick, Input,                  durability — if we crash mid-tick, the input is recoverable boot-time replay
      ModelFingerprint)                        in dev; stamps the model fingerprint for boot checks         (§18.4)
1     AttentionIntent                          classify the input shape (statement / question / etc.)      Step 2 (sole consumer)
                                               so policy can be tuned to it
2     adjusted Policy                          turn intent into the four knobs that govern this tick:      Steps 5, 9, 11, 12
                                               vigilance, plasticity, salience, default confidence         (every weighted op)
                                               (§10.6 table)
3     spans (sentence/clause/entity/value)     give every meaningful unit its own embedding so later       Step 4, Step 6
                                               questions can target small facts, not averaged blobs
4     embedding per span + tick-level vector   dense semantic anchors for routing, similarity, salience    Steps 5, 6, 7,
                                                                                                            and stored in
                                                                                                            embeddings table
                                                                                                            for elements
                                                                                                            written in Step 9
5     active_regions + RegionDelta (held)      identify the conceptual locale (which DAG branches the      Step 6 (warm-predicate
                                               input lives in); compute proposed DAG structural changes    bias on label set),
                                               but DO NOT commit them yet                                   Step 8 (commit the
                                                                                                            held delta), Step 13
                                                                                                            (active_regions in frame)
6     element + relation proposals             turn diagnosed text into structural candidates              Step 9 (build base
      (with confidence per proposal)                                                                        relations and events),
                                                                                                            Step 10 (recognize
                                                                                                            from/to/property
                                                                                                            shape for supersession)
7     coreference decisions (reuse vs. mint)   reconcile each candidate with prior identity; decide        Step 9 (which
                                               whether "Dr. Rao" = existing DrRao element or a new one      ElementIds bind to
                                                                                                            each role), Step 11
                                                                                                            (reinforcement target —
                                                                                                            reused elements get
                                                                                                            their existing path
                                                                                                            strengthened)
```

Two things to notice:

1. **Steps 1–7 cannot be skipped.** Even on a question that mints
   nothing, we still classify intent (so reinforcement weight is
   right), run extractors (a question can introduce new entities),
   and score coreference (so "it" resolves correctly for the focus
   set). The full diagnosis runs every tick.
2. **Steps 1–7 do not commit.** `route_regions` returns a
   `RegionDelta` rather than applying it; extractor output is a
   `Vec<Proposal>`, not a write; coreference produces a decision
   table, not a merge. All of these are values held in tick-local
   state until Step 8 opens the mutation phase.

The parallelism story falls out of (2): because Steps 4–7 hold
`&Hypergraph` only, embedding + region routing + extractor calls run
under `rayon::par_iter` per span at no risk of conflict — roughly
5–7× tick speedup on a modern multicore CPU vs. fully sequential.
This is the same read-mostly-parallel, write-sequentially shape
Datomic and FoundationDB use; here it falls naturally out of the
diagnosis/commit split rather than being a separate concurrency
design.

### 4.4 Each Sub-Step As A Brain Process

Step labels map to brain processes (§13 specifies the function
signatures):

- `detect_intent`, `adjust_policy` — prefrontal cortex.
- `segment`, `embed` — entorhinal cortex.
- `route_regions`, `apply_region_delta` — thalamus.
- `run_extractors` — wernicke (language comprehension).
- `score_coreference` — hippocampus + dentate gyrus.
- `score_salience` — amygdala.
- `reinforce_path`, `decay_step` — basal ganglia.
- `aggregate_focus` — assembles `ConsciousAttentionFrame`.
- `enqueue_replay` — hands a snapshot to the replay thread.

None of these own state. The hypergraph is the only owned thing.

### 4.5 The Discovery Frame

Each tick is a discovery — Legend's model of its world evolving as new
information arrives. The mechanics above (segment, embed, extract,
build, reinforce, decay) are how the discovery is processed. The
*meaning* is: Legend just learned something, possibly contradicting or
confirming what it knew, and its updated model is reflected in the
returned attention frame.

### 4.6 Recap

- One hypergraph. Elements (richly-typed only through emergent
  structure read from indices) bound by Relations (typed hyperedges).
  Both decay and reinforce; both first-class memory citizens.
- Vector hierarchy is a region DAG inside the same hypergraph.
- Recognition (concept, instance, event, frame) is read from derived
  indices over the relation graph (§8) — no kind tag on Element.
- Events are first-class; corrections supersede via PROV-O-style
  chains rather than overwriting.
- Every input is one call to `tick`, which threads through 12 pure
  sub-functions and returns an attention frame; replay enqueue is
  the post-tick handoff.
- Brain regions are functions, not modules. None own state.
- Snapshot + bounded WAL; pinned embedder; no transcript storage.

The rest of the doc — Hard Invariants, the deep type spec, the
algorithms, the worked walkthrough, the build order — fills in this
shape.

---

## 5. Hard Invariants

Substrate-level non-negotiables. A v0 build that violates any of these
is wrong. Mechanism, sizing, and rationale live in the relevant deeper
sections; this list should be readable in one pass.

1. **The hypergraph is Legend's model of its world.** The WAL exists
   only for crash recovery between checkpoints — it is not an event
   store and is not read on the hot path.
2. **No duplication, no fluff.** Every byte earns its place. Inputs are
   distilled into elements and relations (including meta-relations on
   those relations); raw text, normalized text, span offsets, and
   per-input audit records are not substrate citizens.
3. **Every learned abstraction points back to ancestry.** Emergent
   elements and relations carry a `derived_from` meta-relation, a
   `supersedes` meta-relation, or extractor lineage to something
   earlier in the graph. Nothing is born parentless.
4. **Semantic strings do not drive control flow.** Branching uses
   recognition indices, payload-table membership, `RelationStatus`,
   and meta-relation indices — never element name strings.
5. **No fixed kind enum on elements.** Type discrimination is
   relational and structural, not a stored tag. New categories emerge
   from new relation structures without substrate changes.
6. **Bitemporal split.** `Tick` is transaction time (when Legend
   learned this). Valid time (when it was true in the world) lives on
   `(R, valid_from, T)` / `(R, valid_to, T)` meta-relations. Never conflate them.
7. **Status enum distinctions are mechanical and durable.** Asserted,
   entailed, defeasible, superseded, and retracted relations remain
   distinct in the substrate.
8. **Vector closeness may merge regions; it never destructively merges
   facts, instances, or events.**
9. **Cache relations carry `derived_from`.** Derived current-state
   relations are recomputable and are never written without a pointer
   to the event element that produced them.
10. **External source pointers, when they exist, live on
    the `(R, source, S)` meta-relation.** Legend does not store source text; the
    pointer is the record.
11. **Path-aware reinforcement.** Focus success bumps the exact path
    that produced the focused subgraph — not nearby vectors.
12. **Decay weakens; deletion is rare.** When usefulness is uncertain,
    let memory dynamics handle it.
13. **Compression must be focus-preserving.** Replay may consolidate;
    it may not destroy a focused subgraph that conformance gates rely
    on.
14. **One input operation: `tick`. There is no query path.**
15. **Provenance cycles are resolved by replay, not rejected at
    write time.** `(R₁, derived_from, R₂)` and `(R₂, derived_from,
    R₁)` is structurally well-formed but semantically inconsistent;
    replay detects the cycle and marks one side `Retracted`. The hot
    path does not enforce acyclicity.

---

## 6. Mathematical Foundations

The substrate is not a Legend-invented data structure. It is a small
composition of well-studied formalisms. Naming them up front lets the
rest of the doc lean on existing theory instead of re-deriving it
informally, and lets a reader with a background in programming
languages, databases, or knowledge representation place each Legend
concept onto a formalism they already know.

Brain references in this document live one layer up — at the cognitive
operations (§17.3, §13). The substrate itself is mathematical.

**1. Hypergraph with attributes and typed relations** (Habel 1992;
Ehrig et al. 2006). A *hypergraph* is a graph where an edge can connect
any number of nodes instead of always two. Legend's elements are the
nodes; relations are the edges. Elements carry attributes
(names, stats, optional payload-table entries); relations carry
typed predicates and named role bindings. Replay's bulk rewrites
(§14) are a known kind of structured graph rewriting under this
formalism, with results that do not depend on rule application order
when the rules are written correctly — important for replay
determinism. Note: unlike Wikidata or Cyc, Legend does **not** type
elements; only relations carry types. This is the Wolfram-Physics
end of the design spectrum (untyped primitive + typed
connections).

**2. Predicates with named role-fillers** (Parsons 1990; Fillmore 1976;
Baker et al. 1998). A relation `P(role₁ → t₁, role₂ → t₂, …)` is a
predicate applied to **named** arguments, not positional ones. This is
how natural-language event semantics is formalized — verbs have role
slots like `agent`, `patient`, `instrument`, and each slot is filled
by a specific element. FrameNet (Berkeley, 1998) is the largest
catalogue of this shape: ~1,200 frames, each a predicate with its
expected role inventory. Legend's role-fillers may be concrete
(`Term::Element`) or nested (`Term::Relation`); the latter is what
makes meta-relations work — any relation can take another relation
as a filler, and that recursion is unbounded by the substrate.

**3. Bitemporal data model** (Snodgrass 1995; SQL:2011; Datomic). Every
fact has two time axes: **transaction time** (when the system learned
it) and **valid time** (when it was true in the world). Legend uses
`Tick` for transaction time and `(R, valid_from, T)` / `(R, valid_to, T)` meta-relations for valid time.
This split is industry-standard and is required to handle
late-arriving information and supersession correctly.

**4. Algebraic provenance** (Green, Karvounarakis, Tannen 2007). When
a derived fact is computed from base facts, the derivation carries an
algebraic record of *how* it was derived. The algebra you choose
determines the aggregation rule: counting derivations, picking the
most-trusted source, propagating trust levels, etc. Legend's
combination of confidence × evidence-strength × age-decay for derived
relations (`derived_from`, cache relations, supersession) is one such
choice. Naming the formalism makes "how does a cache relation's
confidence update when a base relation's confidence drops?" a
closed-form question with a known answer.

**5. Weighted formulas with online updates** (Richardson & Domingos
2006; Domingos & Lowd 2009). A Markov logic network (MLN) is a set of
logical formulas, each tagged with a real-valued weight. To answer a
query, you find the most consistent assignment of values given those
weights. Legend's "relations with confidence + reinforce/decay +
replay-time consolidation" is structurally an MLN whose weights update
online with every tick. This is *not* a Bayesian network computing
joint distributions — the complexity profile and citation graph are
different.

**6. Emergent ontology vs. declared ontology.** Wikidata, Cyc, OWL, and
RDFS all *declare* types up front (Class, Concept, Individual). Legend
does not. This commits Legend to a different design family — one closer
to neural-network and Wolfram-Physics modeling, where structure
emerges from accumulated relational structure. The cost is that
recognition of "this element is functioning as a concept" must happen
through index reads rather than tag inspection. The benefit is
that no pre-committed ontology limits what Legend can model.

**7. Computational irreducibility and bounded observers** (Wolfram
2002, *A New Kind of Science*; Wolfram Physics Project). The
accumulated understanding of a project is computationally irreducible:
there is no closed-form way to know "what should be in Legend's
hypergraph at tick N" without having actually run the prior N−1 ticks.
Legend accepts this and serves an LLM that is itself a computationally
bounded observer. The substrate provides forward compression (the
hypergraph as queryable running state); a v1+ inward-compression
mechanism (recurring structures promoted to template relations that
shortcut future computation) is sketched in §22. §1.7 develops this
framing; it is the deep reason the one-primitive + emergent-kinds
design is the right one rather than just an aesthetic choice.

§22 (Source Map) gives full citations for each formalism above.

---

## 7. The Substrate

The concrete spec for Legend's two primitives and their attached
structures.

### 7.1 Element

```rust
struct Element {
    id: ElementId,
    names: Vec<String>,           // canonical + variant; both decay if unused
    stats: MemoryStats,
    created_at: Tick,
}
```

That is the entire Element struct. No kind enum. No type tag. No
embedding (lives in the `embeddings` side table for elements that need
one). No fixed payload (lives in side tables when applicable).

**`names`** is the unified list of strings that refer to this element.
The seed pack gives some elements canonical names ("Change",
"change_event"); extractors add variant forms as inputs use them
("Doc Rao" appears alongside "Dr. Rao"). All names share the same
lifecycle; noisy or unused names decay.

**`stats: MemoryStats`** governs decay, reinforcement, salience.
Elements and Relations share the same stats struct — memory dynamics
are uniform.

```rust
struct MemoryStats {
    activation: f32,             // current tick's activation level
    strength: f32,               // long-term durability
    stability: f32,              // resistance to drift
    confidence: f32,             // belief strength
    plasticity: f32,             // willingness to update
    salience: f32,               // amygdala-protected importance
    access_count: u32,
    focus_success_count: u32,
    prediction_error: f32,
    last_seen: Tick,
    last_accessed: Option<Tick>,
}
```

**`created_at: Tick`** is **transaction time** — when Legend learned of
this element. Monotonic `u64` counter, incremented once per `tick()`
call. **Valid time** lives in `(R, valid_from, T)` / `(R, valid_to, T)`
meta-relations on the relations that mention this element (§7.2).

Elements are **indivisible from the rest of the substrate's
perspective**: relations reference elements by id and never reach
inside them. The names field can grow; the stats field updates; but
nothing decomposes an element into sub-elements.

### 7.2 Relation

```rust
struct Relation {
    id: RelationId,
    predicate: ElementId,
    roles: Vec<RoleBinding>,
    status: RelationStatus,
    stats: MemoryStats,
    priority: i8,
    created_at: Tick,
}

struct RoleBinding {
    role: ElementId,                   // role-name (e.g. "agent", "from", "to")
    term: Term,
}

enum Term {
    Element(ElementId),                // concrete filler; covers value-payload elements
    Relation(RelationId),              // nested or meta-relation reference
}

enum RelationStatus {
    Asserted,
    Entailed,
    Defeasible,
    Superseded,
    Retracted,
}
```

Seven fields. Everything that scopes, provenances, modalizes,
supersedes, or otherwise modifies a relation is expressed as a
**meta-relation**: an ordinary relation whose subject is another
relation, via `Term::Relation(RelationId)`. There is no qualifier
struct; there are only relations.

Formally a Relation is a predicate applied to named, role-tagged
arguments, with a real-valued weight. The `predicate` field is the
predicate Element; each `RoleBinding` names one slot and points at the
filler; the weight lives in `stats.confidence`.

**Meta-relations replace the qualifier struct.** Anything that scopes,
provenances, modalizes, supersedes, or otherwise modifies how a
relation should be read is itself a relation:

| What it carries | Meta-relation form |
|---|---|
| Frame (contextual scope) | `(R, frame, F)` |
| Valid-time start | `(R, valid_from, T1)` |
| Valid-time end   | `(R, valid_to, T2)` |
| External source pointer | `(R, source, S)` |
| Modality (actual / possible / desired / obligatory / counterfactual / negated) | `(R, modality, M)` where M is one of six seeded modal-elements |
| Supersession backward link | `(R, supersedes, R')` |
| Lineage (derived-from) | `(R, derived_from, X)` where X is an element or another relation |
| Conditional antecedent | `(R, antecedent_of, R')` |

Each meta-relation is just a Relation. It has its own status, stats,
priority, decay, and can itself carry meta-relations (e.g. a
frame-scoping fact can have its own valid-time, source, etc.) — the
same recursion that nested relations always supported. There is no
substrate-level annotation layer; there are only relations.

**Hot-path access is via derived indices** (§9.2):

```rust
relation_frame:        HashMap<RelationId, ElementId>
relation_valid_from:   HashMap<RelationId, ElementId>
relation_valid_to:     HashMap<RelationId, ElementId>
relation_source:       HashMap<RelationId, ElementId>
relation_modality:     HashMap<RelationId, ElementId>
relation_supersedes:   HashMap<RelationId, RelationId>
relation_superseded_by: HashMap<RelationId, RelationId>   // inverse
relation_derived_from: HashMap<RelationId, Term>
```

These are **derived state**, rebuilt on load and updated incrementally
during Steps 9–10. The relation graph is the source of truth. Reading
"what's the frame of R?" is a single HashMap lookup — same speed as a
struct-field access, no special-casing required.

**Why no struct field for these.** The asymmetry that broke the old
design: `supersedes` was a `RelationId` field while `frame` was
"morally also a `RelationId` reference but stored in a struct called
Qualifiers." There was never a structural reason for the split — only
hot-path performance, which the indices preserve. Removing the
asymmetry lets §3.1's "one primitive (Element) + typed connections
(Relations)" claim be strictly true: there is no second-level
annotation layer.

**What makes an Element a predicate.** Nothing structural — there is no
`is_predicate` flag. An Element is *functioning as* a predicate exactly
when it appears in the `predicate` field of one or more relations
(§3.4). Predicate identity is established by names and by incoming
`subclass_of` / `instance_of` relations that place it in the predicate
lattice (e.g. `(provider, subclass_of, role_predicate)`). New
predicates enter the system either via the seed pack (§16) or via
Step 6 extractor proposals; minting policy and label-set resolution
are specified in §11.7.

**`instance_of` vs. `has_role` convention.** `instance_of` is reserved
for **ontological kind** — what something fundamentally *is* (e.g.
`DrRao instance_of person`, `appointment_1 instance_of appointment`).
`has_role` carries **situational role** — a function the element plays
in a frame (e.g. `DrRao has_role dentist` in the dental-appointment
frame). Both can hold simultaneously on the same element with no
contradiction; the same person can have multiple roles across frames
without any of them changing what the person *is*. Extractors and seed
schemas must respect this split: NER and the `Reference` schema emit
`instance_of`; situational predicates (`has_role`, `provider`,
`participant`) come from frame- and event-shaped extraction. The
emergence rules in §3.4 read `instance_of` for concept/instance
recognition; they do not read `has_role`.

**Status** — `RelationStatus` is mechanical and is allowed to drive
control flow. `Asserted` outranks `Defeasible` regardless of priority;
priority breaks ties between same-status relations.

**Supersession** is meta-relations: `(R, supersedes, R')` form chains
(PROV-O `wasRevisionOf` / `schema.org/supersededBy`). The
`relation_supersedes` index makes chain-walks fast.

**Lineage** is `(R, derived_from, X)` — `prov:wasDerivedFrom`. Present
for cache relations (current-state derivations) and for relations
derived from another relation (X is `Term::Relation(parent_id)`);
absent for asserted base relations. Required by Invariant 9.

**Defeasible priority** — `priority: i8` follows Antoniou's defeasible
logic with dynamic priorities (2002). Stored as a field because tie-
breaking happens on every comparison and must be branchless.

**Belief revision via supersession** is the **Levi identity** —
contraction-of-negation followed by expansion (Alchourrón-Gärdenfors-
Makinson 1985). Legend's correction protocol is base belief revision
(Hansson 1999) made operational.

A relation can represent binary triples, n-ary events, nested relations,
conditional relations (via `(R, antecedent_of, R')` meta-relations),
time-scoping, modality, and uncertainty — all in one structure, all
expressed as relations.

### 7.3 Payload Tables

Some elements and relations need structured payloads beyond what plain
relations can express. Payloads live in side tables keyed by
`ElementId` or `RelationId`. An element or relation appears in zero,
one, or more side tables; the intersection of its memberships is its
emergent structured kind.

#### 7.3.1 `embeddings: HashMap<ElementId, Vec<f32>>`

The semantic anchor for this element. Used by region routing, similarity
search, and salience scoring. Not every element has one — only those
that need a vector position (regions, named concepts, specific
instances with retrievable surface forms).

Embeddings are FP32.

#### 7.3.2 `regions: HashMap<ElementId, RegionPayload>`

An element with a `regions[id]` entry is **functioning as a semantic
region** — a node in the vector-space DAG.

```rust
struct RegionPayload {
    parent_regions: Vec<(ElementId, f32)>,    // weighted DAG, not tree
    child_regions: Vec<ElementId>,
    lateral_regions: Vec<ElementId>,
    prototypes: Vec<Prototype>,               // up to 8 in v0
    radius: f32,
    vigilance: f32,
    density: f32,
    variance: f32,
    utility: f32,
    noise_score: f32,
    relation_refs: Vec<RelationId>,
    instance_refs: Vec<ElementId>,
}

struct Prototype {
    vector: Vec<f32>,
    weight: f32,
    support_count: u32,
}
```

§10 specifies the region DAG's topology, routing algorithm, and merge/
split rules.

#### 7.3.3 `values: HashMap<ElementId, Value>`

An element with a `values[id]` entry is **functioning as a typed
value**.

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

`Tuesday` is a weekday-concept value; `2026-04-30` is a grounded
time-point value. The relation between them is itself a relation, not
a substitution. Relations can target value-payload elements via
`Term::Element(ElementId)` like any other element.

### 7.4 What Else An Element Doesn't Have

Several things you might expect on an element are deliberately not
there:

- **No raw text.** Inputs are distilled into elements and relations;
  the original text lives only in the dev-only WAL (§18) and in
  whatever the source's own system retains. This is the Inv 2 (no
  fluff) and brain analogy commitment.
- **No per-input record.** There is no `Evidence` citizen in the
  hypergraph. Memory IS the distilled relations; if extraction failed
  completely, the input is dropped. Source pointers live on the
  `(R, source, S)` meta-relation for relations that need them.
- **No `kind` field.** §1.6 covers the design bet; §8 covers how kinds
  are observed.

---

## 8. Recognition Indices

Recognition (§3.4) has one mechanism: read derived indices that
summarize each element's relation neighborhood, then condition
behavior on count thresholds. Those indices are this section's
subject.

**Why this is short.** Earlier drafts enumerated six "emergent kinds"
(concept, instance, event, frame, schema, rule), each with its own
structural rule and recognition path. That collapsed: there is no
stored kind, no recognition function. There is a small set of indices
and a few thresholds the pipeline reads. New behaviors are added by
maintaining a new index, not by adding a new kind.

### 8.1 The Indices

Per-element derived state, rebuilt on load and updated incrementally
during `apply_supersession_and_cache` (Steps 9–10):

- `inbound_predicate_counts: HashMap<ElementId, HashMap<ElementId, u32>>`
  — for element `E`, counts of inbound relations grouped by predicate.
  Concept-recognition reads `[E][instance_of]`; reference-frame
  recognition reads `[E][frame]`; any "how many relations point at E
  with predicate P" query reads here.

- `outbound_predicate_counts: HashMap<ElementId, HashMap<ElementId, u32>>`
  — symmetric for outbound. Instance-recognition reads `[E][instance_of]`.

- `meta_relation_presence: HashMap<ElementId, MetaRelationMask>` —
  which meta-relations (valid_from / valid_to / frame / source /
  modality / etc.) appear on relations involving this element.
  Substrate for event-shape recognition (an event is an element on
  whose role-cone many relations carry valid-time meta-relations).

These are **derived, not authoritative.** The relation graph is the
source of truth; indices are caches. They can be rebuilt from scratch
at any time.

**Counted statuses.** Indices count only `Asserted` and `Entailed`
relations. `Defeasible`, `Superseded`, and `Retracted` relations are
excluded — superseded facts must not keep an element looking
concept-like once they're revised.

### 8.2 How Behaviors Read Them

The pipeline conditions on index thresholds:

- **Coreference merge bias** (§14.3) reads
  `inbound_predicate_counts[E][instance_of]` and treats counts at or
  above `policy.concept_recognition_threshold` (default 3) as
  concept-like (broad reuse); counts below that with non-zero outbound
  `instance_of` are instance-like (pattern separation).
- **Supersession trigger** (§11.10) reads
  `meta_relation_presence[E]` for valid-time-bounded role-binding
  shape.
- **Frame-relative scoping** reads `relation_frame[R]` and
  `inbound_predicate_counts[E][frame]`; elements at or above
  `policy.frame_recognition_threshold` (default 5) are recognized as
  reference-frame elements at query time.
- **Decay** reads `meta_relation_presence` to differentiate
  event-bounded from persistent state.

There is no separate "is this a concept?" function. There is a count
threshold, read from the index. Thresholds are hardcoded in v0 (see
§9.3) and calibrated against §19 + §20.5 after Step 8; adaptive
(percentile-based) recognition is deferred (§23).

### 8.3 Why Recognition Indices, Not Stored Tags

Three properties matter:

1. **An element's recognition profile changes over time.** A name
   first encountered as a one-off may later attract many `instance_of`
   inbounds — its index entry rises and the concept-like behavior
   starts firing. No migration, no tag rewrite.
2. **Multiple recognitions at once.** `healthcare_provider` is both
   an instance (of `concept`) and a concept (`dentist` is an instance
   of it). Both index entries are non-zero; both behaviors apply.
3. **New recognitions without substrate changes.** Future recognitions
   (e.g. "plan-shaped elements" defined by `outbound_predicate_counts
   [E][step_n]`) plug into the same index types; no new tables, no new
   code paths.

---

## 9. Core Data Model

Concrete substrate types and the Hypergraph struct. This is the spec
the coder works against first, before any pipeline code, before any
NLP. The substrate must serialize round-trip and the inspection harness
(§21) must dump it before anything else is written.

### 9.1 Style Constraints (Ultra-Minimal Rust)

Beyond the project's R\* style, this v2 codebase commits to a stricter
subset.

The intent: keep the **hot substrate** (elements, relations, indices,
the tick loop, region routing) free of dynamic dispatch and gratuitous
abstraction. The constraint is **no custom dynamic polymorphism or
unconstrained generic abstractions in the hot substrate**, not "no
generics anywhere" — `Vec<T>`, `Option<T>`, `Result<T, E>`,
`HashMap<K, V>`, and serde derive macros are obviously fine.

**Allowed:**

- Standard collections (`Vec`, `HashMap`, `VecDeque`).
- Concrete enums for closed sums.
- Concrete structs everywhere.
- Concrete error enums.
- Derive macros: `#[derive(Serialize, Deserialize, Debug, Clone, Copy,
  PartialEq, Eq, Hash)]`.
- A small number of well-justified generic helper functions where they
  remove duplication without introducing trait machinery.

**Disallowed unless a specific perf or correctness need is documented
in code:**

- **Custom traits with `impl` blocks** in the hot substrate. (Standard
  derive traits are fine.)
- **`dyn` anything.** No trait objects, no `Box<dyn Error>`. Concrete
  enums for everything that would otherwise want polymorphism.
- **Unconstrained generic abstractions in the substrate.** Don't write
  `fn process<T: SomeTrait>(...)` to "future-proof" the hypergraph.
  Concrete types for `Element`, `Relation`, `Hypergraph`.
- `Rc`, `Arc`, `RefCell`, `Mutex` inside the substrate.
- `async fn` / `tokio` / futures. Replay is a thread, not a future.
- Builder patterns.
- Derive macros beyond the allowed list.
- `clone()` in hot paths.
- `String` allocations per tick where `&str` works.
- Procedural macros we do not write ourselves.
- `lazy_static`, `once_cell`. Use plain `static` or explicit init.
- `serde_json` in the tick loop. `rmp-serde` or hand-written binary
  for hot serialization.

Memory layout discipline:

- All elements in `Vec<Element>` indexed by `ElementId(u32)`.
- All relations in `Vec<Relation>` indexed by `RelationId(u32)`.
- Indices (`HashMap<String, Vec<ElementId>>`, etc.) are derivable.
  Rebuild on load. Do not serialize them.
- Hot scalar fields (`activation`, `strength`) are candidates for
  split-out into parallel `Vec<f32>` arrays if profiling shows the
  wide `MemoryStats` struct hurts cache. Decide on first profile, not
  earlier.

### 9.2 The Hypergraph Struct

```rust
struct Hypergraph {
    // Core storage — two primitives, nothing else.
    elements: Vec<Element>,
    relations: Vec<Relation>,

    // Optional payload tables.
    embeddings: HashMap<ElementId, Vec<f32>>,
    regions:    HashMap<ElementId, RegionPayload>,
    values:     HashMap<ElementId, Value>,

    // Tick clock — monotonic, incremented once per tick.
    clock: Tick,

    // Current policy (vigilance, plasticity, decay, thresholds).
    policy: Policy,

    // Working memory — recent focused elements, used by coreference
    // and Hebbian co-activation. Capacity ~64.
    recent_focus: VecDeque<ElementId>,

    // Derived indices — rebuild on load, never serialize.
    by_name:                HashMap<String, Vec<ElementId>>,
    region_children:        HashMap<ElementId, Vec<ElementId>>,
    region_parents:         HashMap<ElementId, Vec<(ElementId, f32)>>,
    relations_by_subject:   HashMap<ElementId, Vec<RelationId>>,
    relations_by_predicate: HashMap<ElementId, Vec<RelationId>>,

    // Meta-relation indices — fast access for what was previously
    // Qualifiers. Each entry is "for relation R, the element/relation
    // pointed at by the (R, predicate, _) meta-relation."
    relation_frame:         HashMap<RelationId, ElementId>,
    relation_valid_from:    HashMap<RelationId, ElementId>,
    relation_valid_to:      HashMap<RelationId, ElementId>,
    relation_source:        HashMap<RelationId, ElementId>,
    relation_modality:      HashMap<RelationId, ElementId>,
    relation_supersedes:    HashMap<RelationId, RelationId>,
    relation_superseded_by: HashMap<RelationId, RelationId>,
    relation_derived_from:  HashMap<RelationId, Term>,

    // Recognition indices (§8) — derived predicate counts. Reading
    // these is how we tell concept from instance from event without
    // a kind enum.
    inbound_predicate_counts:  HashMap<ElementId, HashMap<ElementId, u32>>,
    outbound_predicate_counts: HashMap<ElementId, HashMap<ElementId, u32>>,
    meta_relation_presence:    HashMap<RelationId, HashSet<ElementId>>,
}
```

**Why optional payload tables.** Each table holds the structured
content for elements playing that role. The shapes are different
enough that putting them all on `Element` would balloon the headline
struct (everyone pays for what only some need). Side tables let the
core `Element` stay small and uniform; structured payloads attach via
`HashMap<ElementId, _>` to elements that need them.

### 9.3 Policy

Per-tick modulators set by PFC (`adjust_policy`, §13.5):

```rust
struct Policy {
    // Region routing
    descend_threshold: f32,
    leaf_vigilance: f32,
    merge_threshold: f32,
    split_variance: f32,
    max_prototypes_per_region: u32,
    void_threshold: f32,

    // Mid-path DAG insertion (§10.3.5). All tick-time insertions are
    // Defeasible; replay confirms / re-parents / retracts.
    // - confirm_gap: minimum cosine gap between node-to-child and
    //   node-to-parent for replay to flip Defeasible → Asserted.
    //   Default: 0.05.
    // - confirm_evidence: minimum independent ticks routed-against
    //   the provisional node before confirmation. Default: 3.
    // - reparent_gap: minimum cosine gap to a cross-subtree parent
    //   before replay re-parents. Wider than confirm_gap to prevent
    //   flapping. Default: 0.10.
    midpath_confirm_gap: f32,
    midpath_confirm_evidence: u32,
    midpath_reparent_gap: f32,

    // Predicate dedup (entity collapse threshold for predicate elements).
    // Universal cosine search at mint time (§11.7); hits at or above
    // this threshold reuse instead of mint. Default: 0.85.
    predicate_dedup_threshold: f32,

    // Mint-rate observability (§11.7). If a single tick mints more than
    // this many new predicate elements, the inspection harness logs
    // the tick and replay priority-bumps predicate dedup for it. Not
    // a hard cap. Default: 5.
    predicate_mint_warning_count: u32,

    // Recognition thresholds (§3.4, §8.2). Hardcoded in v0; calibrated
    // against §19 + §20.5 after Step 8. Adaptive (percentile-based)
    // recognition is deferred until v0 corpus data shows the
    // distributional shape per recognition kind (§23).
    concept_recognition_threshold: u32,   // default: 3
    frame_recognition_threshold:   u32,   // default: 5

    // Memory dynamics
    decay_rate: f32,
    salience_floor: f32,
    hebbian_rate: f32,

    // Focus radius for in-tick decay; everything outside decays in
    // the background sweep (§14.7).
    focus_decay_radius: u32,

    // Replay
    replay_cadence: ReplayCadence,
}

enum ReplayCadence {
    EveryNTicks(u32),
    Idle,
    OnDemand,
}
```

Policy is updated each tick by `adjust_policy(intent, base_policy)`.
Tick-internal subroutines read `&Policy`; only PFC writes it.

### 9.4 Concurrency Model

- **Read-mostly parallel phase** (Steps 4–7): `&Hypergraph` shared
  across `rayon::par_iter` workers. No interior mutability. No
  locking.
- **Mutation phase** (Steps 8–13): single `&mut Hypergraph` owned by
  the tick driver. Sequential.
- **Replay** (background thread): receives a snapshot clone of
  `Hypergraph`, computes proposed mutations, sends a batch back via a
  channel, and the main thread applies them under `&mut`.
- **No `Arc<RwLock<Hypergraph>>`.** No interior mutability in the
  substrate. Rust's borrow checker enforces single-writer at compile
  time. This is the reason we chose Rust over C.

### 9.5 Identifier Discipline

- All ids are `u32` newtypes (`ElementId(u32)`, `RelationId(u32)`).
  `RegionId = ElementId`. Frames and sources are ordinary elements (no
  `FrameId` / `SourceId` newtypes); they're referenced by `ElementId`
  like every other element.
- `Tick(u64)` for the monotonic clock.
- Reserve `u32::MAX` as `INVALID`. Never panic on bad ids; return
  `Result<_, HypergraphError>`.

### 9.6 Auxiliary Type Definitions

```rust
type ClaimRef = RelationId;

enum TimeExpr {
    GroundedDate(NaiveDate),
    GroundedDateTime(NaiveDateTime),
    Weekday(Weekday),
    Relative { anchor: Tick, offset: Duration },
    Duration(Duration),
}

enum LocationExpr {
    Named(String),
    Coords { lat: f64, lon: f64 },
    ElementRef(ElementId),
}

struct Input {
    text: String,
    source: Option<ElementId>,    // pointer to a source element if known
    wall_clock: SystemTime,
}

const MAX_SLOTS: usize = 8;

struct ModelFingerprint {
    embedder_hash: [u8; 32],
    tokenizer_vocab_hash: [u8; 32],
    extractor_versions: Vec<(String, String)>,
    code_version: String,
}
```

### 9.7 Differential Updates

The mutation phase produces deltas, not full state recomputes. Each
modification is emitted as a tagged record:

```rust
enum HypergraphDelta {
    ElementAdded(ElementId),
    RelationAdded(RelationId),
    RelationSuperseded(RelationId, RelationId),    // (old, new)
    StatusChanged(RelationId, RelationStatus),
    ElementStatsBumped(ElementId, MemoryStatsDelta),
    RelationStatsBumped(RelationId, MemoryStatsDelta),
    PayloadAttached(ElementId, PayloadKind),
    PayloadUpdated(ElementId, PayloadKind),
}
```

Downstream consumers (cache materialization, salience updates, the
attention assembler, the replay queue) consume deltas, not full state
recomputation. This is **differential dataflow** discipline (McSherry,
Murray, Isaacs et al., CIDR 2013) / **semi-naive Datalog evaluation**
(Bancilhon-Ramakrishnan 1986). We do not import the
`differential-dataflow` crate; we adopt the discipline.

---

## 10. Semantic Regions

The vector subgraph that lives inside the hypergraph. Regions are
elements with `regions` payload entries (§7.3.2); their payloads hold
prototype vectors and DAG topology metadata.

### 10.1 Topology

Regions form a **weighted directed acyclic graph** rooted at `Genesis`
with a `Void` sink for sub-threshold inputs.

- Every region has 1–8 prototype vectors.
- Multi-parent attachment is allowed (this is what makes the topology
  a DAG rather than a tree). Parent edges are weighted; the same child
  region can attach to multiple parents at different strengths.
- Lateral edges may connect sibling regions for fast-pivot retrieval.

The DAG topology is what makes Legend handle polysemy — `Tuesday` in
the `user_schedule:current` region is reachable from multiple parent
regions (`weekday`, `appointment_slot`).

### 10.2 Region Routing (Read-Only) + Application (Mutation)

Region routing happens in the **read-mostly parallel phase** of the
tick (Step 5a). The algorithm walks the DAG from Genesis, considering
top-k children at each node by cosine similarity to a bounded
`max_prototypes_per_region` set.

```rust
fn route_regions(
    embeddings: &[Vec<f32>],
    hg: &Hypergraph,
    p: &Policy,
) -> (Vec<ActiveRegion>, RegionDelta);
```

Outputs:

- a list of active regions per embedding (with similarity scores);
- a `RegionDelta` describing the proposed structural changes (region
  attachments, prototype updates, new regions).

```rust
struct RegionDelta {
    parent_attachments: Vec<(ElementId, ElementId, f32)>,
    prototype_updates: Vec<(ElementId, Vec<f32>)>,
    new_regions: Vec<NewRegion>,
    void_count: u32,
}

struct NewRegion {
    parent: ElementId,
    initial_prototype: Vec<f32>,
}
```

`RegionDelta` is held until the mutation phase, where
`apply_region_delta` commits via spherical k-means prototype updates.

### 10.3 Thresholds, Merge, Split

Regions may merge when prototypes are close, claim overlap is high,
focus behavior is equivalent, merging does not collapse distinct
instances, and no contradiction or frame conflict appears.

Split a region when internal variance grows, prediction errors
accumulate, queries route into the region but need different focused
subgraphs, routed claims form distinct frames, or a broad concept
contains separable sub-concepts.

Splitting improves routing. It does not duplicate or destroy claims.
Merging never destructively merges facts, instances, or events
(Inv 8).

### 10.3.5 DAG Refinement: Mid-Path Insertion

The DAG is never assumed to be a complete taxonomy. It is an evolving
cluster topology that gets refined as evidence accumulates. Tick-time
placement is best-effort given current DAG state; replay restructures
when accumulated evidence reveals missing intermediates.

**Vector used:** the **span-level embedding** of the new concept (the
embedding of the extracted entity string itself, e.g. `embed("js
object")`), not the sentence-level embedding of the input that
introduced it. Span vectors give crisp concept boundaries;
sentence-level vectors mix multiple concepts together and would cause
the DAG to accrete fuzzy mixed-concept nodes.

**Candidate set:** the tick's **sentence-level region routing** acts
as a *structural prior* that filters which branches of the DAG are
eligible for insertion. Only branches under the tick's active
region(s), plus their immediate parents and children, are candidates.
This is how polysemy is handled without blending vectors: "object" in
a coding sentence routes the tick to the coding region, so the span's
DAG placement happens within the coding branch; "object" in a
philosophy sentence routes to a different branch and places there.
Same span vector, different candidate sets, no contradiction.

**The signal** (within the candidate set): a new element or region is
**more similar to an existing child than to that child's parent**.
That pattern means an intermediate concept exists between them.

Concrete example. Suppose the DAG already contains:

```
object ─────────────► my js object that represents a car
```

A new input introduces `js object`. Its **span-level embedding** scores
(against candidates filtered by sentence-level routing into the
coding branch):

- cosine to `object` parent prototype: 0.78
- cosine to the specific element: 0.85

The specific element is closer to `js object` than to `object`. Replay
detects the pattern, inserts `js object` as a region between them, and
re-parents the specific element:

```
object ─► js object ─► my js object that represents a car
```

This is the same machinery as §10.3 split — a parent region whose
children form a cluster gets a new node inserted, and the cluster's
elements re-parent to it. Mid-path insertion is the case where the
"cluster" has size 1 (a single specific element with no siblings) and
the new intermediate comes from outside the parent's existing
children.

Conditions for tick-time mid-path insertion:

- A new element/region's **span-level embedding** scores higher cosine
  similarity to an existing specific element than to that element's
  current parent — within the candidate set filtered by sentence-level
  region routing.
- The new node's embedding sits within the parent's region radius
  (otherwise it's just a different region, not an intermediate).
- The split does not violate Inv 8 (no destructive merge of distinct
  instances).

**All tick-time mid-path insertions are provisional.** The new node's
parent meta-relation `(node, parent_region, R)` is written
`Defeasible` regardless of whether sentence-level routing was crisp
or diffuse. The DAG benefits from the refined topology immediately
for routing — region recognition reads `RegionPayload` structurally,
not the parent meta-relation's `RelationStatus`, so Defeasible-parent
regions are routable on the next tick. The *claim* that the new node
belongs there is what stays provisional until replay confirms.

Replay (§14.8) accumulates evidence across ticks and resolves each
provisional insertion to one of three outcomes:

- **Confirm.** Cosine gap between node-to-current-child and
  node-to-current-parent ≥ `policy.midpath_confirm_gap`, and the
  node has been routed-against in ≥ `policy.midpath_confirm_evidence`
  ticks without contradiction. Parent meta-relation flips to
  `Asserted`.
- **Re-parent (cross-subtree allowed).** When a node's cosine to a
  parent in a different subtree exceeds its current parent by
  ≥ `policy.midpath_reparent_gap`, replay moves it. Wider gap than
  `confirm_gap` to avoid flapping. Emits
  `(node, supersedes_parent_region, old_parent)` for lineage. This is
  the recovery path for wrong-subtree placements that came out of
  weak or wrong sentence-level routing on the introducing tick.
- **Retract.** A Defeasible insertion that fails to accumulate
  evidence within the window is pruned; the node's children re-parent
  to the original (pre-insertion) parent.

The stability gate is essential at BGE-small's 384 dimensions, where
adjacent cosine differences of 0.02–0.05 are routinely within
embedding noise. `confirm_gap` (default 0.05) and the multi-tick
evidence requirement keep replay from churning provisional nodes in
and out across passes on noise-driven signals.

**Anaphoric spans are not DAG-insertion candidates.** Spans like "it",
"this approach", "the pattern" must resolve to an existing element via
the coref cascade (§11.8), not become new DAG nodes. Enforcing this is
the extractor stack's job at §11.7 (GLiNER + lexicon should not
propose anaphoric/deictic spans as entity candidates).

### 10.4 Multi-Prototype

Each region stores up to **8 prototypes**
(`Policy.max_prototypes_per_region`). Reasons:

- Region centroids are averages; prototypes preserve modes.
- Multi-modal regions (e.g. `appointment` matches both medical and
  vehicle service contexts) need multiple anchor points.
- Replay decides whether a 9th prototype triggers a split.

### 10.5 Cosine-Specific Update Rule

Prototype updates use spherical k-means (cosine on unit-normalized
vectors). The DDVFA reference implementation (Brito da Silva et al.
2019) is the closest published kin to Legend's region machinery and
should be read end-to-end before writing v0 region code.

Legend-specific deltas on top of DDVFA:

- Weighted DAG region topology with multi-parent attachment (no
  published online-clustering algorithm produces this).
- "Facts don't merge" (Inv 8) enforced at the data-model level.
- Replay-driven split (§14.8).
- The `Void` sink for sub-threshold inputs.
- Cosine prototype updates (spherical k-means), not Fuzzy ART
  complement coding.

### 10.6 Failure Modes To Plan Around

- **Region proliferation.** Unchecked region creation explodes memory.
  Mitigation: replay merges on schedule; the inspection harness
  reports region creation rate; alarms when rate exceeds threshold.
- **Vigilance set wrong.** Too high = no merging, too low = wrong
  merging. Mitigation: per-frame vigilance from the policy table
  below.
- **Prototype dimension collapse** (e.g. e5-base-v2 under
  quantization). Mitigation: smoke-test against a held-out set of seed
  prototypes; require ≤ 2% recall@10 drop after any quantization
  change.

Per-intent policy modulators (the canonical "what does intent change"
table — referenced from §11.2 and §11.3). All values are v0 starting
points; calibrate against §19 + §20.5 after Step 8.

```text
intent          vigilance  plasticity  salience  default_conf  notes
correction        0.85        1.2        1.5        1.0         protect against false merge; high-salience event
identity          0.85        0.9        1.2        1.0         names matter; don't blur entities; bump salience
statement         0.50        1.0        1.0        1.0         baseline
temporal_update   0.70        1.0        1.2        1.0         slightly tighter; current-state writes get a salience bump
question          0.70        0.6        0.7        0.5         tighter routing, lower plasticity, smaller salience bump,
                                                                lower default confidence on any new writes (entities the
                                                                question introduces enter Defeasible by default)
brainstorming     0.30        1.3        0.8        0.7         looser routing, more plastic, slightly lower salience and
                                                                confidence — let exploration happen without solidifying
```

Read columns as multipliers/overrides on the corresponding `Policy`
fields:

- `vigilance` → `policy.leaf_vigilance` (absolute).
- `plasticity` → multiplier applied to `policy.hebbian_rate` before
  Step 11.
- `salience` → multiplier applied to amygdala salience bumps in Step 11
  (interacts with `policy.salience_floor`).
- `default_conf` → initial `MemoryStats.confidence` for relations
  built in Step 9 from this tick's extractor proposals; also shifts
  the `Entailed` ↔ `Defeasible` threshold for NER auto-emit.

The question row is the load-bearing one: lower plasticity and lower
default confidence are how a question can run the full pipeline
without solidifying as much as a statement. Mutation still happens —
reinforcement, decay, any newly-introduced entities — just at a
weight that reflects the input being a query rather than an
assertion.

---

## 11. The Tick Pipeline

This section specifies the 14 steps (0–13) `tick` runs through. §4
covered the conceptual shape; this section is the typed spec. Every
tick runs every step regardless of intent; intent modulates `Policy`,
not which steps execute.

### 11.0 Per-Step Latency Budget

v0 budget table on commodity CPU (4-core, INT8 ONNX, BGE-small +
GLiNER2-small via gline-rs). Numbers are p50 targets; p95 typically
runs 1.5–2× p50 driven by GLiNER2 variance.

```text
step  name                              p50 budget   notes
0     log entry (WAL append)            <1 ms        LZ4 hot segment append
1     detect_intent                     1–3 ms       small-bank cosine vs intent prototypes
2     adjust_policy                     <1 ms        scalar copy + multiplier apply
3     segment                           1–3 ms       sentence/clause/value splitting
4     embed                             5–20 ms      BGE-small INT8 over all spans (parallel)
5     route_regions                     5–15 ms      DAG descent over hundreds of regions (parallel)
6     run_extractors                    130–208 ms   ★ GLiNER2 — the long pole; one inference call
7     score_coreference                 2–5 ms       small candidate sets, recency-based
8     apply_region_delta                2–5 ms       k-means prototype updates
9     build_relations                   3–8 ms       hashmap inserts + index updates
10    supersession + cache              2–5 ms       chain walks via relation_supersedes index
11    reinforce_hebbian + salience      2–5 ms       Oja-rule bumps along focused path
12    decay_focus_radius                3–8 ms       bounded by policy.focus_decay_radius
13    aggregate_focus + enqueue_replay  2–5 ms       RRF merge + handoff to replay thread
                                        ─────────
                                        ~160–290 ms p50

★ GLiNER2 is v0's binding latency constraint. Steps 4–5 parallelize
across spans via rayon::par_iter; Step 6 is one call and does not
parallelize. The p50 floor moves with whichever zero-shot extractor
is in slot 6, not with infrastructure changes elsewhere.
```

The path to sub-100 ms p50 is replacing or augmenting Step 6:

- **Pattern fast-path (§24.1).** Surface-pattern templates handle
  common shapes (~5–20 ms) with GLiNER2 fallback for novel inputs.
  Average tick latency drops as pattern hit-rate climbs on mature
  corpora. This is the cheapest win.
- **Unified tiny-LLM extractor (§24.7).** A single Qwen-0.5B / Phi-3-
  mini class call replaces NER + RE + temporal + heuristic coref.
  ~50–150 ms on CPU INT8. Disruptive but flexible.
- **Smaller GLiNER variant.** A calibration-only change during
  Step 6 of the build (§21). Floor ~50–80 ms if the smaller variant
  passes §19 + §20.5 quality gates.

Read-path / background-work splitting (§24.2) addresses non-extractor
contributions to tick latency, not the GLiNER2 long pole. It is
secondary, not primary, on the path to sub-100 ms.

### 11.1 The Function

```rust
fn tick(hg: &mut Hypergraph, input: Input) -> ConsciousAttentionFrame {
    // --- Read-mostly phase (Steps 1–7, &Hypergraph) ---
    let intent  = detect_intent(&input, hg);                  // Step 1
    let policy  = adjust_policy(&intent, &hg.policy);         // Step 2
    let units   = segment(&input);                            // Step 3
    let embeds  = embed(&units);                              // Step 4
    let (active_regions, region_delta)
                = route_regions(&embeds, hg, &policy);        // Step 5  (delta held, not applied)
    let extractions
                = run_extractors(&units, &active_regions,
                                 &policy, hg);                // Step 6
    let coref   = score_coreference(&extractions, hg);        // Step 7

    // --- Mutation phase (Steps 8–13, &mut Hypergraph) ---
    apply_region_delta(hg, region_delta);                     // Step 8
    let (relations, events)
                = build_relations(&extractions, &coref, hg);  // Step 9
    apply_supersession_and_cache(hg, &relations, &events);    // Step 10
    reinforce_hebbian(hg, &focused_path, &policy);            // Step 11
    decay_focus_radius(hg, &focused_path, &policy);           // Step 12
    let attn = aggregate_focus(&relations, &policy);          // Step 13
    enqueue_replay(hg, &attn);     // also schedules background decay sweep
    attn
}
```

The phase boundary is strict: Steps 1–7 take `&Hypergraph` and produce
proposals (`region_delta`, `extractions`, `coref`); no hypergraph state
changes during this window. Step 8 onward takes `&mut Hypergraph` and
commits all proposals together. `apply_region_delta` is the first
mutation, not part of the read-mostly phase.

### 11.2 Step 1 — Detect Intent

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

Intent detection is a function over the input embedding + recent
focus. It does **not** branch on hard-coded keywords. v0 heuristic:
punctuation + embedding-similarity to a small bank of intent-prototype
embeddings shipped in the seed pack.

**What intent does and does not change.** Intent feeds Step 2
(`adjust_policy`, §11.3) and through it modulates exactly four things:

- **Vigilance** — region-routing crispness (§10.6 table).
- **Plasticity** — how much new writes update existing weights vs. land
  fresh (`policy.hebbian_rate`).
- **Salience floor** — the amygdala-bump magnitude that fires in
  Step 11 (`policy.salience_floor`).
- **Default confidence** — the initial `MemoryStats.confidence` written
  on new relations this tick, and the threshold separating `Asserted`
  from `Defeasible` for extractor proposals.

Intent does **not** affect: which pipeline steps run, what gets
extracted, whether `apply_region_delta` commits, whether
`build_relations` writes, whether `reinforce_hebbian` or
`decay_focus_radius` fire, the shape of the returned frame, or any
structural decision about elements/relations. Every tick runs Steps
0–13 regardless of intent. The only knobs intent turns are the
weights inside those steps.

### 11.3 Step 2 — Adjust Policy

PFC reads the per-intent modulator table in §10.6 and produces the
adjusted `Policy` for this tick — vigilance (absolute), plasticity
(multiplier on `hebbian_rate`), salience (multiplier on amygdala
bumps in Step 11), and default confidence (initial weight on writes
in Step 9). Every tick-internal subroutine reads `&Policy`; only PFC
writes it. The base `Policy` on `Hypergraph` is the inter-tick rest
state; the per-tick adjusted copy is what Steps 3–13 see.

### 11.4 Step 3 — Segment Text

Split into units: sentence, clause, quoted span, list item, code span,
entity-like span, time/value span. Each unit gets its own embedding;
units flow through the rest of the tick by value, not as stored
records.

### 11.5 Step 4 — Embed Units

Embed every unit from Step 3 plus the full tick — never one averaged
vector for the whole memory, because later questions target small
facts. The substrate is dimension-agnostic but the seed pack's
prototypes are dim-specific; swapping dimensions requires re-embedding
the seed.

### 11.6 Step 5 — Route Through Regions

Each embedding runs `route_regions(...)` (§10.2) against the DAG.
**Read-only** and parallelizes across embeddings via `par_iter`. The
returned `active_regions` set seeds extractor attention in Step 6 —
when a region is active, predicates and roles authored within
relations whose participants live in that region get a small label-set
priority, so GLiNER2 prefers the lexicon's "warm" predicates over
cold ones.

### 11.7 Step 6 — Run Extractors

The v0 extractor stack (§15 details what's native vs ONNX):

- **NER** — spans for names/orgs/places. Each tagged span auto-emits
  an `instance_of` relation: `(span_element, instance_of, K)` where `K`
  is the seed-pack kind that matches the NER tag (`person`, `org`,
  `place`, etc.). Auto-emitted `instance_of` relations dedup against
  existing `(span_element, instance_of, K)` — if one is already present
  on this element, no new relation is created and the existing one's
  `MemoryStats` are reinforced via the normal Step 10 path. NER
  confidence ≥ `policy.ner_assertion_threshold` → `Entailed`; below →
  `Defeasible`. Anonymous spans (no surface name) are minted with
  `name = "<kind>_<counter>"` where `<kind>` is taken from the
  highest-confidence `instance_of` proposal in the same tick.
- **Temporal parser** — dates, weekdays, durations, relative times.
- **Zero-shot relation extraction** (`gline-rs` / GLiNER2) — emits
  typed `(subj, pred, obj, confidence)` triples.
- **Heuristic coref** — recency-based: pronouns resolve to the
  most-recently-focused element whose role matches.

All extractor output carries confidence and (where available) a source
pointer that flows into a `(new_relation, source, source_element)`
meta-relation on the resulting relations.

**Predicate label set.** GLiNER2's relation-extraction labels come from,
in order:

1. Seed-pack canonical predicates (`instance_of`, `subclass_of`, role
   predicates) — always included.
2. The "warm" predicates: predicate elements whose `MemoryStats`
   activation is above a floor. This is what active regions modulate —
   when Step 5 returned active regions, predicates whose participants
   live in those regions get included even if their activation has
   decayed somewhat. Bounds open-vocabulary drift without freezing
   extraction to seed coverage.

**Resolving a proposed predicate label to an ElementId.** Each extractor
proposal arrives as `(subj_span, pred_label, obj_span, confidence)`.
Resolve `pred_label` to an `ElementId` by:

1. Exact-match lookup against element names in the lexical index
   (tantivy, §15.1). On hit, reuse the predicate.
2. On miss, embed `pred_label` and run a cosine search across **all**
   predicate elements (not just warm ones — the warm-predicate set is
   used for GLiNER2's label-set bias above, not for dedup). On any hit
   with cosine ≥ `policy.predicate_dedup_threshold`, reuse the top hit.
   The relation is marked `Defeasible` (the surface label didn't match
   the canonical name, so the binding carries some uncertainty even
   though the predicate is right); replay can reinforce the alias
   later.
3. On miss, mint a new predicate element with the label as its name.
   Every relation that uses it this tick is `Defeasible` until replay
   either reinforces it (≥ N independent ticks within a window) or
   prunes it.

**Why the cosine search is universal, not warm-only.** Predicate
synonyms ("rescheduled to" / "moved to" / "changed to") embed close
together regardless of which is currently warm. Restricting dedup to
the warm set lets cold-but-equivalent predicates pile up and re-mint
on every tick, defeating recognition that counts by predicate id. The
universal cosine search is `O(P)` per proposed label — tractable at v0
predicate counts (low thousands).

**Mint-rate observability.** When a single tick mints more than
`policy.predicate_mint_warning_count` new predicates (default 5), the
inspection harness logs the tick id and replay receives a
priority flag for predicate dedup on this tick's outputs (§14.8). Not
a hard cap — a tick that legitimately introduces several new
predicates is allowed — but the warning surfaces the cases where
synchronous dedup didn't catch a synonym cluster.

**Optional accelerator (post-v0):** a *lexicon-paired-noun* rule that
proposes intermediate DAG nodes upfront when both components of a
compound noun are already in the lexicon. See §22 for v1+ ideas.

### 11.8 Step 7 — Coreference Scoring

Identity is conservative. Score:

```text
score =
    name_overlap
  + embedding_similarity
  + frame_overlap
  + role_overlap
  + temporal_compatibility
  + relation_support
  - contradiction_penalty
  - distinct_instance_penalty
```

Rules:

- Reuse concepts broadly.
- Reuse instances only with coreference support.
- Create provisional instances when uncertain.
- Replay merges provisional instances later if support accumulates.

Pattern separation (`separate_pattern`, ported from current Legend's
dentate gyrus) is the dampening function on the merge side: when two
candidates are close-but-distinct on a discriminating role, force them
apart.

### 11.9 Step 8 — Build Relations and Events

Build compact base relations. Do not materialize the full entailment
closure.

For: *"My dentist appointment with Dr. Rao changed from Tuesday to
Friday."*

Base elements created or reused:

```text
user, Dr. Rao, dentist, appointment, appointment_1,
Tuesday, Friday, reschedule_event_1
```

Base relations:

```text
DrRao instance_of person                         [Defeasible]
DrRao has_role dentist                           [Asserted]
appointment_1 instance_of appointment            [Entailed]
appointment_1 participant user                   [Entailed]
appointment_1 provider DrRao                     [Asserted]
appointment_1 domain dental                      [Entailed]
reschedule_event_1 instance_of reschedule_event  [Entailed]
reschedule_event_1 target appointment_1          [Asserted]
reschedule_event_1 property date                 [Asserted]
reschedule_event_1 from Tuesday                  [Asserted]
reschedule_event_1 to Friday                     [Asserted]
```

### 11.10 Steps 9–10 — Supersession and Cache

If a prior cache relation exists for `appointment_1 current_time`,
mark it `Superseded` and write the new cache relation plus the two
linking meta-relations:

```text
R_new: appointment_1 current_time Friday   [Asserted]
R_old: appointment_1 current_time Tuesday  [Superseded]

(R_new, derived_from, reschedule_event_1)  [Entailed]
(R_new, supersedes,   R_old)               [Entailed]
```

The `relation_supersedes` and `relation_superseded_by` indices update
incrementally so chain walks (`walk backward to recover any prior
current state`) remain O(chain-length) HashMap lookups.

### 11.11 Step 11 — Hebbian + Salience

Co-activated elements (members of the focus set) have their pairwise
co-activation strengthened via the bounded Oja rule (§14.6). The
update magnitude reads `policy.hebbian_rate` — already scaled by the
intent's plasticity multiplier from Step 2 (§10.6 table), so a
question's reinforcement lands at lower weight than a statement's on
the same path.

Amygdala bumps salience for:

- exact values/times/persons
- corrections / contradictions
- user-stated preferences
- relations that were focus-bearing on this tick

Bump magnitude is `base_bump * policy.salience_multiplier` where the
multiplier is the intent's salience column from §10.6 (correction =
1.5, question = 0.7, brainstorming = 0.8, etc.). Salience floor is
`policy.salience_floor`. This is the only place intent affects how
strongly a discovery is protected from later decay.

Promotion check: any `Defeasible` relation whose
`stats.support_count >= 3` (independent ticks within a window) is
promoted to `Asserted` in this step.

### 11.12 Step 12 — Focus-Radius Decay

Decay during the tick is **bounded to the focus radius** so the
read-mostly-then-mutate phase stays under the latency budget. Walk
outward from the focus set up to `policy.focus_decay_radius` hops; for
every element on the way, decay `activation` by `policy.decay_rate`.

Everything outside the radius is decayed by the **background sweep**
(§14.7), scheduled by `enqueue_replay`. The sweep runs in the replay
thread, computes a delta against a snapshot, and the next tick applies
it under `&mut`. Decay weakens **access paths**, never destroys
focus-bearing relations (Invariants 2, 8).

### 11.13 Step 13 — Assemble Attention Frame

```rust
struct ConsciousAttentionFrame {
    tick: Tick,
    input: InputEcho,
    intent: AttentionIntent,
    active_frame: Option<ElementId>,
    active_regions: Vec<RegionActivation>,
    focused_relations: Vec<RelationActivation>,
    supporting_claims: Vec<ClaimRef>,
    history: Vec<ClaimRef>,
    uncertainty: Vec<UncertaintySignal>,
    durable_writes: Vec<ElementId>,
    superseded: Vec<RelationId>,
    next_actions: Vec<AttentionAction>,
}
```

`input: InputEcho` carries the input that produced this frame — the
raw text (or pointer, for non-text inputs) plus its source kind. This
is read-only, not durable: it is *not* a hypergraph citizen and is
discarded after the calling LLM consumes the frame. It exists so the
caller has the question/statement in hand alongside Legend's response
to it, without having to thread it separately through its own state.

The frame is a **post-tick snapshot of the focused subgraph**, not a
pre-assembled answer. The calling LLM reads any natural-language
response off `focused_relations` (with `supporting_claims` for
provenance and `history` for superseded context), in light of `input`.
Legend does not assemble or rank answer candidates; that is the
caller's job.

`focused_relations` aggregates extractor-proposed relations with the
focus set's path-reinforced relations via reciprocal-rank fusion. Each
relation's `score` carries its base weight + vote weight from cone
neighbors that reinforced this tick.

### 11.14 Replay Enqueue (post-tick)

`enqueue_replay` hands a snapshot to the background replay thread.
Replay returns a `Vec<ReplayMutation>` that the main thread applies on
the next tick boundary under `&mut`. See §14.8.

---

## 12. There Is No Query

A reminder, restated as its own section because it is the design move
most likely to be misread.

Legend has **one input operation: `tick`**. Every input — statement,
question, correction, identity, temporal update, brainstorming —
flows through the same pipeline and runs every step. The output
(`ConsciousAttentionFrame`) is always the same shape: a post-tick
snapshot of the focused subgraph. There is no separate query API and
no separate memory store. Retrieval is differential — path traversal
with reinforcement — not a parallel index alongside the substrate.

What differs across input shapes is *what the discovery contained*,
which surfaces in the frame's contents:

- A **statement** typically writes new elements/relations, so
  `durable_writes` is non-empty; `focused_relations` reflects the slice
  now updated.
- A **question** typically writes little — usually just reinforcement
  bumps along the focused path, plus any newly-introduced entities the
  question mentioned. `focused_relations` carries the relations the
  caller can read its response off of.
- A **correction** typically combines both: new writes for the new
  state, supersession for the old, and `focused_relations` showing the
  resolved current state alongside the just-superseded history.

These are tendencies, not API contracts. The frame reflects what was
actually discovered, not an intent-driven gate on what could be
written. Intent (§11.2) modulates `Policy` (vigilance, plasticity,
salience floor) — it does not gate which steps run.

There is no `query()` function. There is one verb. The caller — the
LLM session — synthesizes any natural-language response from the
frame; Legend does not pre-assemble one and the frame carries no
`answer` field by design (§2.3, §11.13).

Why this matters for implementation:

- **Retrieval is differential, not absolute.** Each tick reinforces the
  exact path that produced focus; future ticks reach the same focused
  subgraph faster because the path is stronger, not because a separate
  index was rebuilt.
- **Question + statement in one input is one tick.** *"Actually it
  moved to Monday. What do I have Tuesday now?"* — one tick produces
  both a supersession and a focused subgraph that exposes the new
  Tuesday state.
- **Latency budget is uniform.** v0 200–300 ms p50 applies to all
  ticks, dominated by zero-shot relation extraction (§15.1). v1 sub-
  100 ms is the goal via pattern fast-paths (§24.1) and the unified
  tiny-LLM extractor (§24.7).

---

## 13. Brain Processes As Functions

Each brain process is a function over the hypergraph + policy. None of
them own state.

```rust
// Read-only (parallel-safe under &Hypergraph).
fn route_regions(input_embeddings: &[Vec<f32>], hg: &Hypergraph, p: &Policy)
    -> (Vec<ActiveRegion>, RegionDelta);
fn separate_pattern(candidate: &Element, neighbors: &[&Element], p: &Policy)
    -> Decision;
fn score_salience(relation: &Relation, p: &Policy) -> f32;
fn detect_intent(input: &str, embeddings: &[Vec<f32>], recent: &VecDeque<ElementId>)
    -> AttentionIntent;
fn adjust_policy(intent: &AttentionIntent, base: &Policy) -> Policy;
fn aggregate_focus(candidates: &[RelationCandidate],
                   path: &[ElementId],
                   p: &Policy)
    -> Vec<RelationActivation>;

// Mutation (sequential, takes &mut Hypergraph).
fn apply_region_delta(hg: &mut Hypergraph, delta: RegionDelta);
fn reinforce_path(path: &[ElementId], hg: &mut Hypergraph);
fn decay_focus_radius(hg: &mut Hypergraph, focus: &[ElementId], p: &Policy);

// Background-thread (snapshot in, mutation list out).
fn replay(hg_snapshot: &HypergraphSnapshot, p: &Policy)
    -> Vec<ReplayMutation>;
```

### 13.1 Thalamus — `route_regions` + `apply_region_delta`

Entry to the cortex. Routes embeddings through the region DAG; emits
proposed structural changes. Read-only phase parallelizes; mutation
phase sequential.

### 13.2 Hippocampus — embedded in pipeline + `reinforce_path`

Episodic encoding lives in the tick mutation phase; per-tick
"episodes" are the durable writes. Path-aware reinforcement is the
hippocampal consolidation analog.

### 13.3 Dentate Gyrus — `separate_pattern`

Pattern separation. When two coreference candidates are close in
embedding but distinct in role, dampen the merge.

### 13.4 Amygdala — `score_salience`

Salience scoring. Bumps protection for exact values, corrections,
preferences, and focus-bearing relations.

### 13.5 PFC — `detect_intent` + `adjust_policy`

Intent classification and policy modulation. Sets per-tick vigilance,
plasticity, salience floor.

### 13.6 Hebbian Learning

Fires inside the co-activation step (Step 11). Bounded by
`Policy.hebbian_rate` and the Oja-rule operators (§14.9).

### 13.7 Path-Aware Reinforcement — `reinforce_path`

When a tick produces a focused subgraph, bump `MemoryStats` along the
**exact path** that produced it — every element *and every relation*
on the path:

- query embedding region (element)
- matched concept elements in that region
- selected relations (the hyperedges themselves)
- selected instance (element)
- region-to-relation edges and frame/time meta-relation path

Not nearby alternatives. Path-aware discipline is what keeps memory
durable for things that actually got focused on, instead of merely
things that sit near them in vector space.

### 13.8 Decay — `decay_step`

Decay reduces retrieval priority; v0 deletes nothing.

Decay targets:

- unused semantic-region links
- low-confidence inferred relations
- low-utility derived relations
- stale provisional instances
- noisy names
- weak access paths

Decay spares:

- value-payload elements with exact, durable content (times, ids,
  numbers)
- high-salience relations
- relations with focus success
- contradictions/corrections
- supersession history
- user preferences

### 13.9 Replay — `replay` (background thread)

Offline learning. See §14.8.

---

## 14. Algorithms

The mutating algorithms — region insertion, supersession, replay —
are all instances of structured **graph rewriting** over the substrate
(§6 (1)). Each rule has three parts: a *match* shape (what to look
for), an *interface* (what to keep unchanged), and a *replacement*
(what to write in place). The DPO graph-rewriting literature
characterizes which rule sets give the same final hypergraph
regardless of application order (confluence); Legend's replay rules
are **designed** to be order-independent in this sense, and the
property is **tested** as such (§21 Step 11 determinism fixture: two
replay passes over the same starting hypergraph with different rule
orderings must produce bit-identical final state). v0 does not
attempt formal confluence proofs — those are out of scope. The
two-pass test is the v0 contract; confluence violations surface as
test failures, not as cryptic divergence elsewhere. We do not import
a graph-rewriting library; we adopt the discipline.

Replay's relation-weight updates are online updates to the
weighted-formula model from §6 (5) — strengthen relations that fired
together; weaken relations the world contradicted.

### 14.1 Online Region Insertion

The §10.2 algorithm is **DDVFA-derived** (Brito da Silva et al. 2019).
DDVFA already provides what we need: two-level vigilance, multi-
prototype F2 nodes, and a Merge ART module for input-order robustness.
**Read DDVFA before writing v0 region code** — saves weeks of
reinvention.

Legend-specific deltas: weighted DAG topology, "facts don't merge"
enforcement, replay-driven split, the Void sink, cosine prototype
updates (§10.5).

### 14.2 Multi-Prototype Clustering

Up to 8 prototypes per region. When a 9th would be added, replay
decides whether to split the region or evict the lowest-weight
prototype.

### 14.3 Conservative Coreference

Candidate scoring (§11.8). Pattern separation as the dampening function
on the merge side. When uncertain, create separate provisional
instances and link them as possible coreference candidates; replay
resolves later.

### 14.4 Event Reification (Event-Calculus-Style Fluent Update)

§7.2 introduced relations; this section names the algorithm for
state-change events specifically. Legend's update protocol is **Event
Calculus** (Kowalski & Sergot 1986; Shanahan's "The Event Calculus
Explained" for the modern formulation), with the events-initiate-
fluents mapping made structural:

| Event Calculus | Legend |
|---|---|
| `Happens(e, t)` | event element asserted at tick t |
| `Initiates(e, f, t)` | new current-state cache relation `R_new` plus paired meta-relation `(R_new, derived_from, e)` |
| `Terminates(e, f, t)` | prior current-state relation `R_old` marked `Superseded`; meta-relation `(R_new, supersedes, R_old)` written |
| `HoldsAt(f, t)` | walk `relation_supersedes` index to find non-Superseded leaf |

This is a 40-year-old logical foundation; adopt the vocabulary, don't
reinvent under different names. Role bindings (`target`, `property`,
`from`, `to`) follow the standard treatment of events as objects with
named role-fillers (Parsons 1990; Davidson 1967).

### 14.5 Relation Materialization Policy

Driven by Invariant 2 (no duplication, no fluff). Every relation must
earn its bytes.

Store:

- asserted base relations
- high-confidence entailed relations that are focus-bearing
- current-state cache relations (paired with `derived_from`
  meta-relations)
- `supersedes` / `superseded_by` meta-relations
- `source` meta-relations on relations that came with an external
  pointer

Do not store:

- raw or normalized text (lives in the dev-only WAL, never in the
  hypergraph)
- per-input audit records (no `Evidence` citizen — see §1.4 / §7.4)
- paraphrases of existing relations
- weak implications
- speculative role assumptions
- any field that is derivable from another field on demand

Derived relations are computed on the fly or materialized during replay
when they prove focus-bearing. This is **incremental view
maintenance** (Gupta & Mumick 1995); cache relations are
self-maintainable views (Quass et al., VLDB 1996) refreshable from the
event chain without re-querying base data.

### 14.6 Path-Aware Reinforcement

§13.7. When a tick produces a focused subgraph, bump `MemoryStats`
along the exact path — every element and relation on it.

### 14.7 Utility-Based Decay

Decay applies to elements and relations uniformly — both carry
`MemoryStats`, both compute utility the same way:

```text
utility =
    focus_success
  + support_count
  + salience
  + exact_value_bonus
  + correction_or_contradiction_bonus
  + source_quality
  - noise_score
  - redundancy
  - age_without_access
```

Decay runs in two passes:

1. **Focus-radius decay (in tick, Step 12).** Walk outward from the
   focus set up to `policy.focus_decay_radius` hops; decay each
   element/relation's `activation` by a utility-modulated rate:
   `decay = policy.decay_rate * (1 - normalize(utility))`. This is
   bounded and stays under the latency budget.

2. **Background sweep (replay thread).** A periodic full-graph pass —
   scheduled by `enqueue_replay`, not the tick — applies the same
   formula to elements outside the focus radius. Like every replay
   job, it sees a `Hypergraph` snapshot, computes a delta, and the
   next tick applies it under `&mut`.

Decay weakens access paths first (the cheapest reversible move). A
heavily decayed relation becomes harder to retrieve but is not deleted
in v0; superseded relations are kept and walked via the supersession
chain. Element and relation deletion is a separate retention policy
and is not implemented in v0.

### 14.8 Replay (Background Thread)

Replay runs on a background thread under the snapshot/message-passing
protocol (§9.4). Replay jobs:

- **background decay sweep (§14.7)** — periodic full-graph utility-based
  decay for everything outside the per-tick focus radius.
- split high-variance regions.
- merge duplicate regions.
- **resolve provisional mid-path insertions (§10.3.5)** — every
  tick-time mid-path insertion is `Defeasible`. Replay walks
  Defeasible parent meta-relations and resolves each to one of:
  (a) **confirm** — gap between node-to-child and node-to-parent
  cosine ≥ `policy.midpath_confirm_gap` and the node was routed-against
  in ≥ `policy.midpath_confirm_evidence` ticks without contradiction;
  flip parent meta-relation to `Asserted`; (b) **re-parent across
  subtrees** — node's cosine to a parent in a different subtree
  exceeds its current parent by ≥ `policy.midpath_reparent_gap`; move
  the node and emit `(node, supersedes_parent_region, old_parent)`
  for lineage. Available regardless of `Asserted` / `Defeasible`
  status — this is the recovery path for wrong-subtree placements
  driven by weak sentence-level routing on the introducing tick;
  (c) **retract** — Defeasible insertion failed to accumulate
  evidence within the window; prune the node and re-parent its
  children to the pre-insertion parent. This is how the DAG resolves
  intermediate concepts (`object → js object → my js object that
  represents a car`) from accumulated evidence rather than upfront
  NP decomposition, and how it recovers from noise-driven or
  wrong-subtree initial placements.
- **resolve cycles among meta-relations** — when a chain of
  `supersedes` / `derived_from` meta-relations forms a cycle (e.g.
  R1 supersedes R2 supersedes R1), retract the lowest-confidence
  relation in the cycle. Tie-break by older `created_at` (the older
  claim loses, since the newer one is more likely the
  current-state-bearing version). Emit a retraction meta-relation
  for lineage; the retracted relation flips to
  `RelationStatus::Retracted` rather than being deleted.
- merge duplicate predicates — Step 6 mint-time dedup (§11.7) is the
  primary defense; this replay job is **cleanup-only**, catching
  predicates whose embeddings drifted into convergence after their
  initial mint or whose surface labels were too dissimilar to trigger
  synchronous dedup. Merge when two predicate elements' embeddings
  converge within `policy.predicate_dedup_threshold`. Priority-bumped
  for ticks that fired the §11.7 mint-rate warning.
- resolve provisional coreference.
- compact redundant relations.
- materialize useful derived relations.
- demote unused derived relations.
- evict prototypes when a region exceeds 8.

**Replay must be benchmark-aware:** any candidate compression is
rejected if it would break recall on the §19 walkthrough.

### 14.9 Bounded Hebbian Operators

The Oja-rule-derived bounded Hebbian update used by §14.6 and Step 11
(§11.11):

```text
bounded_hebbian_bump(x, rate):
  return x + rate * (1 - x)        // asymptotes to 1.0

bounded_hebbian_decay(x, rate):
  return x * (1 - rate)             // asymptotes to 0.0
```

Used wherever activation, support strength, or co-activation weights
need to update without leaving [0, 1].

---

## 15. Model Stack

Pure Rust plus deterministic ONNX. No Python, no JVM, no sidecars.

### 15.1 v0 Components

1. **`tokenizers` (HuggingFace)** — tokenization. Apache-2.0, pure
   Rust. Golden-vector tests against reference outputs.
2. **`ort` (pyke.io)** — ONNX runtime. The single inference path.
3. **BGE-small-en-v1.5** as the embedder, INT8-quantized for inference
   latency only (FP32 stored in v0). Pinned for Legend's lifetime
   (§18.4). A model swap is a deliberate, hard, one-time event.
   Legend does not retain raw text or per-input records, so there is
   no in-place re-embedding migration. The only path on swap is to
   re-ingest from `(R, source, S)` meta-relation pointers where the
   source is still reachable, and accept element loss where it is
   not.

   **Recoverability by source class.** This is what "model swap" costs
   in practice — not a uniform "re-ingest from sources," but a
   stratified loss based on what kind of source each relation cites:

   | Source class | Recoverability on swap |
   |---|---|
   | User-as-source (notes app, direct prompts) | Recoverable by re-prompting; tedious but possible. |
   | Git history (commits, file content at SHA) | Recoverable but expensive — every cited commit is a re-fetch and re-extract. |
   | File events (paths, edits) | Partial — files that have moved or been deleted are unrecoverable; current-file references are recoverable. |
   | Slack / chat messages | Partial-to-ephemeral — edited and deleted messages are gone; archived channels may be reachable; private DMs depend on retention. |
   | Agent-internal observations | Unrecoverable — no source pointer, no transcript. |

   For coding-project use, the practical share unrecoverable on a
   swap is significant — file moves, deleted Slack messages, and
   agent-internal observations dominate over time. v0 accepts this.
   The "one-way door" framing is honest about the magnitude: pinning
   the embedder for life is what the design costs, not a conservative
   default. v1 will design a recovery path informed by which source
   classes dominate in real Legend instances; candidate approaches
   include secondary-embedder rotation, opt-in source-text retention,
   or hybrid (§23). Until then, treat the pinned model as
   load-bearing infrastructure that does not get swapped.
4. **`tantivy` 0.25** — BM25 lexical index over element names + relation
   role fillers. Mandatory for proper-noun / identifier / file-path
   retrieval that dense embeddings systematically underweight.
5. **Temporal parser** — `chrono` + `chrono-english` for the easy 80% +
   thin uncertainty-grounding layer. Carries grounding uncertainty.
6. **`gline-rs`** — pure Rust GLiNER and GLiNER2 inference on `ort`.
   Zero-shot NER + relation extraction. ~130–208 ms/call across 5–50
   labels.

   **★ Binding latency constraint in v0.** GLiNER2 is one inference
   call per tick and does not parallelize. It owns ~60–80% of the
   tick budget by itself; everything else (embedding, routing, coref,
   supersession, reinforcement, decay, frame assembly) sums to
   ~30–60 ms. The path to sub-100 ms p50 ticks runs through replacing
   or augmenting this slot — pattern fast-paths (§24.1), unified
   tiny-LLM extractor (§24.7), or a smaller GLiNER variant if it
   passes §19 + §20.5. See §11.0 for the per-step budget table.
7. **Heuristic coreference** — write from scratch in Rust. Recency-
   based, defensible per Centering Theory + Hobbs' algorithm baselines.

### 15.2 What We Drop In v0

- **OpenIE.** Stanford CoreNLP is JVM-only.
- **AMR / UMR.** No portable implementation.
- **Cross-encoder reranker.** Path-aware reinforcement IS the reranker.
- **Dependency parser.** Not on the §19 walkthrough's critical path.
- **From-scratch tokenizer / BM25 / NER+BIO decoder.** Pure-Rust mature
  crates exist; writing our own buys nothing in 2026.

### 15.3 Beyond v0

Substantive v1+ ideas (patterns, latency optimization, hierarchical
frames, INT8 stored embeddings, HNSW over regions, forward-chaining
inference, local-LLM unified extractor, lexicon-paired-noun
acceleration) live in §24.

### 15.4 Honest Estimates

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

Plus substrate, seed pack, pipeline, replay (§21): ~9–11 wk
additional (lighter than earlier estimates because patterns are
deferred to v1, §24).

Realistic v0 horizon: **~9–11 wk part-time**, **~3–4 wk full-time**.

---

## 16. The Seed Pack

The seed pack is **data, not code** — replaceable, version-controlled,
inspectable. Customizable per Legend instance without recompilation.
Seeded as `seed_v0.msgpack.lz4`, embedded at boot.

The full enumeration with per-element rationale lives in
`seed_pack.yaml` at the repo root. This section gives the criterion,
the categories, and one example per category; the yaml gives every
seeded atom with a one-sentence reason for being there.

**Seeding Criterion.** Seed an element only if it is **load-bearing
for recognition or for the v0 extraction machinery** — i.e. one of
the §3.4 recognition rules, the Step 6 extractor stack (§11.7), or
replay (§14.8) must be able to read its name or its prototype to
function. Everything else emerges via extraction and replay (§3.4:
"the seed pack is bootstrapping, not a closed vocabulary").

The seed pack has three categories:

- **Anchors** — `Genesis` and `Void`. Roots of the region DAG.
- **Predicates** — the names §3.4's recognition rules and §11.7's
  extractor stack read by name (`instance_of`, `subclass_of`, the
  eight meta-relation role predicates). Without these present at
  boot, recognition has nothing to count.
- **Regions** — broad shape priors that bias routing (§10.2).
  Without seed regions, every input lands in an unparented cluster
  and routing has nothing to descend through.

Roles, reference frames, and modal elements round out the pack so
extractors have valid `RoleBinding.role` targets and so
`(R, modality, M)` meta-relations can be written without minting
fresh elements per tick.

### 16.1 Code / Seed / Input Boundary

```text
Code owns mechanics.
Seeds own priors.
Inputs own truth — Legend keeps the distilled relations, not the
  inputs.
Replay owns consolidation.
```

Hard-coded code owns only substrate mechanics:

```text
RelationStatus, Term variants
time/value comparison
meta-relation index maintenance (frame, source, valid_from/to,
  modality, supersedes, derived_from)
decay/reinforcement/replay mechanics
the tick pipeline
the embedding interface
```

Seeded hypergraph data owns priors:

```text
Genesis, Void
seeded predicates (§16.3)
broad seed regions (§16.4)
generic role elements
seed reference frames
modal elements
```

Inputs own truth — Legend keeps the distilled relations
(elements + relations), not raw inputs. Source pointers live on
the `(R, source, S)` meta-relation for relations that need them.

Replay owns consolidation — region splits/merges, mid-path inserts,
cycle resolution, predicate dedup, the background decay sweep.

### 16.2 Seed Regions

Shape priors that bias routing (§10.2). Hand-author 15 broad regions
rooted at `Genesis` with a `Void` sink. Example:

```yaml
- element_id: REGION_CHANGE_HISTORY
  payload_kind: region
  names: ["change/history"]
  parent_regions:
    - [GENESIS, 1.0]
  descriptor: >
    Something that was one way and is now different. A value moved from
    an old state to a new state. A revision, an edit, a correction, a
    rescheduling, an update.
```

Regions are seeded with descriptor strings; their initial prototype
embeddings are computed from the descriptor at boot. Refined by online
clustering as inputs flow through.

There is **no** question region. Question-shape is *intent* (Step 1
`AttentionIntent::Question`, §11.2), not content. A question routes
through the same regions as a statement on the same topic — "what
time is my appointment?" goes through REGION_EVENTS / REGION_TIME.

### 16.3 Seeded Predicates

Predicate names §3.4's recognition rules and §11.7's Step 6 read by
name:

- **`instance_of`, `subclass_of`** — concept-hierarchy predicates.
- **Meta-relation role predicates** (8): `frame`, `valid_from`,
  `valid_to`, `source`, `modality`, `supersedes`, `derived_from`,
  `antecedent_of`. These are the predicates of meta-relations
  maintained by hot-path indices (§9.2).
- **Modal elements** (6): `MODAL_ACTUAL`, `MODAL_POSSIBLE`,
  `MODAL_DESIRED`, `MODAL_OBLIGATORY`, `MODAL_COUNTERFACTUAL`,
  `MODAL_NEGATED` — pointed at by `(R, modality, M)` meta-relations.

All other predicates emerge per §11.7's mint-new-predicate path.

### 16.4 Seed Pack Manifest

Total seed atoms: ~52 (2 anchors + 10 seeded predicates + 6 modal
elements + 15 regions + 11 roles + 8 reference frames).

Seeded relations:

- `instance_of` relations pinning seed elements into their structural
  roles (e.g. `(REGION_CHANGE_HISTORY, instance_of, REGION_CLASS)`).
- `subclass_of` relations establishing seed concept hierarchy.

`appointment`, `function_definition`, `plan`, and other domain
concepts are **not** seeded. They emerge from extraction.

The full per-element enumeration with rationales lives in
`seed_pack.yaml`.

---

## 17. Carry-Forward From Current Legend

This is a fresh repo with a fresh data model. We bring forward
**concepts**, not code.

### 17.1 What We Keep (As Concepts)

- **Decay + reinforcement scalars on every memory citizen.** In
  `MemoryStats`, shared by elements and relations. Constants worth
  cribbing from current Legend's basal-ganglia AdaGrad code.
- **Salience scoring at write time.** Becomes the function that
  decides amygdala protection and initial element strength. Not a
  module — a function.
- **Pattern separation.** The "do not collapse close-but-distinct"
  rule used inside coreference scoring (§14.3). Current
  `dentate_gyrus.rs` is the reference implementation; we re-derive
  from scratch.
- **Working-memory ring buffer.** A `VecDeque<ElementId>` of the last
  ~64 focused elements. Used by coreference ("it" resolves against
  recent focus) and by Hebbian co-activation (§13.6).
- **Neurochemistry-style policy modulators.** Not the names
  (NE/DA/ACh/etc. are noise to a new reader), but the *idea* — global
  scalars that flex based on intent. Now lives in `Policy` (§9.3) and
  is set by PFC (§13.5).

### 17.2 What We Drop

- **L1/L2/L3 layering.** The substrate replaces it. Working memory is
  a small ring buffer; everything else is one hypergraph.
- **Brain-region module boundaries.** Brain processes are pure
  functions over `&mut Hypergraph` (§13), not modules with their own
  state.
- **The wernicke lexicon.** ~3400 lines of hand-coded entity logic.
  Replaced by the seed pack (§16) plus extractors (§15).
- **`TickResult`.** Replaced by `ConsciousAttentionFrame` (§11.13).
- **Persistence/WAL/daemon/MCP/CLI.** Out of scope for the core
  substrate. Reattach in v1 once the substrate is proven.
- **Anything Python or JVM.** No sidecars. No exceptions.
- **Per-input audit records.** No `Evidence` struct, no input-as-thing.
  Memory is the distilled relations.
- **Typed atom kinds.** No `AtomKind` enum. Kinds are emergent
  structures of relations and payload-table memberships (§8).
- **Concept/Instance distinction at the type level.** Both are just
  Elements; recognition is via the
  `inbound_predicate_counts[E][instance_of]` index (§8.1).

### 17.3 What "Brain Regions" Means In v2

Each brain region from current Legend maps to a **function**, not a
module — none own state, the hypergraph is the only owned thing. The
full mapping with signatures lives in §13. Names are retained as
descriptive shorthand, not architectural boundaries.

---

## 18. Durability

### 18.1 The Snapshot

The on-disk hypergraph image is the canonical state.

- Format: LZ4 + MessagePack.
- Serialized fields: `elements`, `relations`, `embeddings`, `regions`,
  `values`, `clock`, `policy`, plus a `stamped_at: Tick` marker and
  the `ModelFingerprint` in force when written.
- Derived indices are rebuilt on load.
- v0 has no format migrations. When the format changes in v1, add a
  4-byte version header.

### 18.2 The Bounded WAL

A segmented write-ahead log sits alongside the snapshot for crash
recovery between checkpoints.

- 1 MB segments.
- Hot segment: LZ4 (fast write).
- Closed segments: zstd-19 (aggressive compression).
- 10 MB total cap, queue-style oldest-segment eviction.
- `LEGEND_WAL_UNBOUNDED=1` for development builds (full unbounded log
  for debugging). Production builds reject this flag.

The WAL is *not* an event store. It is read only on crash recovery
between snapshots. Boot path: `load latest snapshot, replay WAL suffix
on top`. Crash recovery: same. In production, the bounded WAL is
purely a durability mechanism; in dev with `LEGEND_WAL_UNBOUNDED=1`,
it doubles as the full input-history record for replay-through-the-
extractor debugging.

### 18.2a Extraction-Failure Quarantine (Dev Only)

Successful ingestion produces only distilled relations — `(R, source, S)`
points at a source-element id; no transcript, no per-input record,
no exceptions. The §1.5 / §7.4 commitment holds in production
without softening.

In **development builds only**, a bounded in-memory ring captures
inputs that emitted no relations after Step 9, for diagnosing
"why didn't Legend learn about X?" failures during extractor
calibration. Gated behind `LEGEND_DEV_QUARANTINE=1`; production
builds compile this out and reject the flag.

- Capacity: 100 entries (configurable via env).
- Stored: `(Tick, Input, Reason)` where `Reason` names the step that
  emitted nothing (e.g. `NoExtractorProposals`, `AllProposalsBelowThreshold`,
  `AllProposalsResolvedToExistingNoOp`).
- Eviction: FIFO when the ring is full.
- Lifetime: in-memory only. Not serialized in the snapshot. Cleared
  on restart.
- Inspected via `legend memory show-failures` (dev CLI) — production
  builds do not ship this command.

This is **not** memory. The quarantine is a debug artifact in the
same category as the dev WAL — bounded, dev-only, off in production.
Successful inputs do not enter it. The §1.5 brain-analogy commitment
holds: production memory IS distilled relations and nothing else.

### 18.3 Hybrid Checkpoint Triggers

Checkpoints fire when any of:

- `N = 1000` ticks have elapsed since last checkpoint, OR
- `S = 5 MB` of WAL growth since last checkpoint, OR
- `T = 1 hour` of wall-clock has elapsed.

After a successful checkpoint, all WAL segments stamped with `tick <=
snapshot.stamped_at` are dropped.

### 18.4 Boot-Time Fingerprint Check

Every snapshot and WAL segment carries a `ModelFingerprint` (embedding-
model hash, tokenizer vocab hash, extractor versions, code version).
On boot, the running binary's pinned fingerprint is compared against
the snapshot's. Mismatch → refuse to start.

Because Legend does not retain raw text or per-input records (§7.4),
there is no in-place re-embedding migration. If the model is ever
changed (a deliberate, one-time event), the only path is to re-ingest
from `(R, source, S)` meta-relation pointers where the source is
still reachable, and accept element loss where it is not.

The recoverability matrix is stratified by source class — not every
source survives a swap, and for coding-project use the unrecoverable
share (file moves, deleted Slack messages, agent-internal
observations) tends to dominate over time. See §15.1 for the full
breakdown. The pin-for-life commitment is what the design costs, not
a conservative default; treat it as load-bearing infrastructure that
does not get swapped without an explicit recovery plan. v1 will
revisit with empirical signal on which source classes dominate (§23).

### 18.5 Storage Cost

The hypergraph is dominated by **embeddings** (one ~1.5 KB f32 vector
per concept/region/pattern element; INT8 quantization in v1 cuts
this 4×). Relations and concept elements are ~100–500 B each. With raw
text and input records dropped, typical hypergraph sizes are orders of
magnitude smaller than naive transcript-with-index designs. The WAL is
bounded at 10 MB. Latency, not disk, is the primary scarcity (§2.2).

---

## 19. Ten-Tick Conformance Walkthrough

This is the executable conformance fixture. Each tick's expected
output is both the returned `ConsciousAttentionFrame` and the
hypergraph delta. The inspection harness (§21) diffs both.

The walkthrough uses the appointment-rescheduling domain because it
exercises every substrate mechanic in a small number of ticks:
extraction, coreference, supersession, recognition through indices,
current-state caching, path-aware reinforcement, and the discovery
frame.

### Tick 1

Input:

```text
My dentist appointment with Dr. Rao changed from Tuesday to Friday.
```

**Active regions this tick:**

```text
REGION_CHANGE_HISTORY  similarity 0.92
REGION_EVENTS          similarity 0.85
REGION_ENTITIES        similarity 0.81  (covers Dr. Rao mention)
REGION_TIME            similarity 0.78  (covers Tuesday/Friday)
```

The active regions bias the predicate label set in Step 6 toward
warm predicates from these parts of the graph.

Hypergraph delta:

```text
added elements:    user, Dr. Rao (DrRao), dentist, appointment,
                   appointment_1, Tuesday, Friday, reschedule_event_1

added relations (concrete world claims):
  R1:  DrRao has_role dentist                       [Asserted]
  R2:  appointment_1 instance_of appointment        [Entailed]
  R3:  appointment_1 provider DrRao                 [Asserted]
  R4:  appointment_1 participant user               [Entailed]
  R5:  reschedule_event_1 target appointment_1      [Asserted]
  R6:  reschedule_event_1 property date             [Asserted]
  R7:  reschedule_event_1 from Tuesday              [Asserted]
  R8:  reschedule_event_1 to Friday                 [Asserted]
  R9:  appointment_1 current_time Friday            [Asserted]
  R10: appointment_1 old_time Tuesday               [Superseded]

added meta-relations (modifications to the above):
  (R9,  derived_from, reschedule_event_1)           [Entailed]
  (R10, derived_from, reschedule_event_1)           [Entailed]
  (R9,  supersedes,   R10)                          [Entailed]

stats updates (Step 11):
  MemoryStats reinforced along the focused path —
  reschedule_event_1, appointment_1, DrRao, Tuesday, Friday, and the
  relations binding them.
```

Returned `ConsciousAttentionFrame`:

```text
intent: Statement
active_frame: user_schedule
active_regions: REGION_CHANGE_HISTORY, REGION_EVENTS, REGION_ENTITIES,
                REGION_TIME
focused_relations:
  appointment_1 current_time Friday
  reschedule_event_1 from Tuesday
  reschedule_event_1 to Friday
durable_writes: appointment_1, reschedule_event_1, ...
next_actions: watch for future corrections to appointment_1
```

Note: `appointment` here is a *learned* element emerged from this
tick's extractor proposals, not a seeded concept. The seed pack ships
only the load-bearing predicates and broad regions; the appointment
domain was never presumed.

### Tick 2

Input:

```text
I have an appointment at the body shop on Tuesday.
```

Delta:

```text
reused elements:   user, appointment, Tuesday
added elements:    appointment_2, body_shop_1
added relations:
  appointment_2 instance_of appointment           [Asserted]
  appointment_2 participant user                  [Entailed]
  appointment_2 location_or_provider body_shop_1  [Asserted]
  appointment_2 current_time Tuesday              [Asserted]
```

Critical: do **not** merge `appointment_1` and `appointment_2`. Pattern
separation fires on the discriminating role (`provider` vs
`location_or_provider`).

Returned state:

```text
intent: Statement
active_frame: user_schedule
focused_relations:
  appointment_2 current_time Tuesday
  appointment_2 location_or_provider body_shop_1
uncertainty: exact calendar date for Tuesday is unknown
```

### Tick 3

Input:

```text
When is my appointment at the dentist?
```

Delta:

```text
no new elements or relations (read-shaped tick)
reinforced path: query -> appointments -> dental_appointments
                 -> DrRao -> appointment_1 -> current_time -> Friday
```

Returned state:

```text
intent: Question
active_frame: user_schedule
active_regions: appointments, dental_appointments
focused_relations:
  appointment_1 current_time Friday
uncertainty: exact calendar date unknown
```

### Tick 4

Input:

```text
What do I have on Tuesday?
```

Delta:

```text
no new elements or relations
reinforced path: query -> Tuesday -> [filter current] -> appointment_2
```

Returned state:

```text
intent: Question
focused_relations:
  appointment_2 current_time Tuesday
  appointment_2 location_or_provider body_shop_1
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
added elements:    Monday, reschedule_event_2
added relations:
  reschedule_event_2 target appointment_1         [Asserted]
  reschedule_event_2 from Friday                  [Asserted]
  reschedule_event_2 to Monday                    [Asserted]
  R20: appointment_1 current_time Monday          [Asserted]
  R21: appointment_1 previous_time Friday         [Superseded]

added meta-relations:
  (R20, derived_from, reschedule_event_2)         [Entailed]
  (R21, derived_from, reschedule_event_2)         [Entailed]
  (R20, supersedes,   R9)                         [Entailed]
       // R9 was the prior current_time = Friday from Tick 1
```

**Active regions this tick:**

```text
REGION_CHANGE_HISTORY  similarity 0.94 (stronger than Tick 1 — path
                                        was reinforced by the prior
                                        change-shaped tick)
REGION_EVENTS          similarity 0.86
REGION_TIME            similarity 0.83
```

The change-history region's prior reinforcement on Tick 1 raised the
activation margin this tick, biasing extractor attention toward the
warm change/from/to predicates already present in the graph.

Returned state:

```text
intent: Correction
focused_relations:
  appointment_1 current_time Monday
  appointment_1 previous_time Friday  [Superseded]
uncertainty: "it" resolved to dentist appointment via recent focus
             + dentist cue (heuristic coref)
```

### Tick 6

Input:

```text
When is my appointment with Dr. Rao now?
```

Delta:

```text
no new elements or relations
reinforced path: query -> DrRao -> appointment_1 -> current_time -> Monday
```

Returned state:

```text
intent: Question
focused_relations:
  appointment_1 current_time Monday
  appointment_1 provider DrRao
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
added element:     oil_leak
added relation:
  appointment_2 purpose oil_leak                  [Asserted]
```

Returned state:

```text
intent: Statement
focused_relations:
  appointment_2 purpose oil_leak
```

### Tick 8

Input:

```text
Why am I going to the body shop?
```

Delta:

```text
no new elements or relations
reinforced path: query -> body_shop -> appointment_2 -> purpose -> oil_leak
```

Returned state:

```text
intent: Question
focused_relations:
  appointment_2 purpose oil_leak
  appointment_2 location_or_provider body_shop_1
```

### Tick 9

Input:

```text
Dr. Rao is my dentist.
```

Delta:

```text
matched existing: DrRao element
no new elements (do not create a new DrRao instance)
reinforced relation: DrRao has_role dentist (incremented confidence + focus_success)
reinforced path: user -> dentist -> DrRao
```

Returned state:

```text
intent: Statement
focused_relations:
  DrRao has_role dentist
  user has_dentist DrRao  [Entailed]
reinforced: DrRao element, dentist relationship
```

### Tick 10

Input:

```text
What appointments do I have?
```

Delta:

```text
no new elements or relations
gathered: appointment_1, appointment_2
filtered: current non-Retracted, non-Superseded current_time relations
```

**Active regions this tick:**

```text
REGION_EVENTS          similarity 0.84
REGION_TIME            similarity 0.76
```

Intent classification fires `AttentionIntent::Question`; aggregate
focus walks all `appointment instance_of` elements with non-superseded
`current_time` relations and returns them.

Returned state:

```text
intent: Question
focused_relations:
  appointment_1 current_time Monday
  appointment_1 provider DrRao
  appointment_2 current_time Tuesday
  appointment_2 purpose oil_leak
  appointment_2 location_or_provider body_shop_1
supporting_claims: appointment_1.current_time, appointment_2.current_time, appointment_2.purpose
uncertainty: exact calendar dates unknown unless Monday/Tuesday were grounded
```

This walkthrough is the **first conformance fixture**. The inspection
harness (§21) asserts the returned attention frame and the internal
hypergraph state after each tick.

---

## 20. Evaluation

### 20.1 Co-Primary Metrics

The 2025 consensus stack: recall + faithfulness + abstention. v0
metric floor is the first three:

1. **Relation recall@k.** If the focus-bearing relation is not in
   `focused_relations`, the system failed before any reader gets
   involved. Benchmarks that ship "gold evidence" annotations are
   mapped onto the corresponding relations in Legend's hypergraph.
2. **Update / supersession accuracy.** When a fact is superseded
   across ticks, does the focused subgraph reflect the *current*
   (post-update) fact? Tested by `MemoryAgentBench FactConsolidation`
   and `LongMemEval`'s `knowledge-update` slice. Scored against the
   reader LLM's response derived from the frame.
3. **Abstention recall.** When the relevant fact isn't in memory,
   does `focused_relations` come back empty (or low-confidence)
   strongly enough that the reader says "I don't know" instead of
   hallucinating? Tested by `LongMemEval` `*_abs` variants and
   `AbstentionBench`.

### 20.2 Secondary Metrics

- end-to-end answer accuracy on grounded questions (reader LLM reading off the frame)
- temporal accuracy (current vs historical disambiguation)
- instance-separation accuracy (no false merges across name collisions)
- compression safety (replay does not break recall on §19)
- retrieval path stability across reruns
- faithfulness / unsupported-claim rate (deferred — needs an LLM judge)

### 20.3 v0 Evaluation Gates

Three benchmarks adopted as v0 evaluation gates:

1. **§19 ten-tick walkthrough** — the conformance fixture. Hypergraph
   + attention frame must match the predicted deltas exactly.
2. **LongMemEval** (Wang et al., ICLR 2025; arXiv 2410.10813; MIT) —
   `longmemeval_oracle.json` (gold-evidence-only) is the first run.
   `longmemeval_s_cleaned.json` (~115k tokens) once routing
   stabilizes. Categories Legend should pass first:
   `single-session-*`, `knowledge-update`, `temporal-reasoning`,
   `*_abs` abstention. Categories that will lag in v0:
   `multi-session` aggregation, `single-session-preference` (until a
   `Preference` schema lands in the seed pack).
3. **MemoryAgentBench FactConsolidation** (HUST-AI, ICLR 2026) —
   single-hop and multi-hop counterfactual updates. Structurally
   identical to Legend's supersession semantics. Multiple-choice
   format means string-match scoring suffices.

### 20.4 Smoke-Test Benchmark (Embedding / Routing)

**RULER MK-NIAH and MV-NIAH at 8K and 32K** (NVIDIA, COLM 2024).
Synthetic, deterministic, license-clean. Use as embedding/region-
routing smoke test in CI. Failures here mean §10 routing is broken
before you even touch memory.

### 20.5 Custom Conformance Fixtures (Companions to §19)

Three more, each ~15 minutes to author in §19 format:

1. **"Two Sarahs" — instance separation.** Two entities with identical
   name, divergent attributes (e.g. Sarah the teacher vs Sarah Chen
   the nurse). Asserts pattern separation fires on at least one role
   mismatch.
2. **"Forgotten correction" — supersession blindness.** Three reschedule
   events on the same appointment over 20 ticks of unrelated content.
   Asserts the focused current-state relation reflects the *third*
   time, not the most recent surface text.
3. **"Frame drift" — frame disambiguation.** User asks "do I have
   anything Tuesday?" then 30 ticks later asks "was Tuesday on the
   schedule?" referring to a *past* week. Asserts `active_frame`
   switches from `user_schedule:current` to `user_schedule:historical`.

§19 + these three are the v0 conformance gate. LongMemEval +
MemoryAgentBench are the v0 generalization gate.

### 20.6 Benchmarks We're Not Adopting

- **LoCoMo** (Snap, 2024) — documented scoring controversy. Skip.
- **MTEB / BEIR** — only when the embedding model swap is on the table.
- **SOTOPIA** — interactive LLM-vs-LLM rollouts, not memory-shaped.
- **NIAH variants beyond RULER** — RULER subsumes them.

### 20.7 What "Compression Safety" Means

LongMemEval-style conservation test:

```text
Can the compressed memory still surface the focus-bearing fact and the
relation path that supports it in `focused_relations`?
```

Replay is benchmark-aware (§14.8): any candidate replay mutation that
would break recall on the §19 walkthrough is rejected before it lands.

---

## 21. Build Order

Solo coder with Claude as reviewer. Every step's done-criterion is the
inspection-harness diff: hypergraph + attention frame after each tick
must match the predicted state. Spec sections in parens are the source
of truth; this section gives sequence and gates only.

**Conformance-test discipline.** Two test tiers, with different
determinism contracts:

- **Substrate conformance (§19, §20.5).** Run with **mocked
  extractor outputs** — predicate proposals and confidence values
  are hand-supplied per the walkthrough. The test asserts the
  *substrate's* behavior given fixed extraction: bit-identical
  hypergraph delta after each tick. This is the discipline already
  established in Step 4 ("hard-code §19 via direct `add_element` /
  `add_relation`, no NLP") and continues through every later step.
  Substrate conformance does **not** call ONNX.
- **Full-stack smoke tests.** Run the actual extractor stack
  (BGE-small, GLiNER2 via gline-rs, chrono-english) on the same
  fixtures. Pin CI hardware to a fixed machine class
  (linux x86_64 AVX2, INT8 ONNX). Assert structural shape (which
  elements/relations exist, statuses, supersession links) but allow
  ε-tolerance on confidence values. Cross-machine determinism is
  out of substrate scope; ONNX FP rounding can shift confidences
  enough to flip threshold-driven decisions on different hardware.

The replay-determinism fixture (Step 11) is part of the substrate
tier — it tests confluence of replay's rule application
independently of any extractor output.

### Step 0 — Foundation Infrastructure (~1 wk)

**Build:** Add v0 crates (`ort`, `tokenizers`, `tantivy`, `gline-rs`,
`chrono-english`, `rayon`, `hashbrown`, `lz4`, `rmp-serde`, `serde`).
Round-trip BGE-small via `EmbeddingWrapper` against a
`sentence-transformers` parity oracle. Wire the inspection harness
(serialize → deserialize → pretty-print, including region-proliferation
over time per §10.6).
**Done:** bit-identical round-trip; embedding parity; harness prints
region creation rate.

### Step 1 — Substrate (~2 wk)

**Build:** §7 + §9 types + indices + supersession-chain walk. Element
+ Relation + payload tables (`embeddings`, `regions`, `values`) +
meta-relation indices (`relation_frame`, `relation_valid_from`,
`relation_valid_to`, `relation_source`, `relation_modality`,
`relation_supersedes`, `relation_superseded_by`,
`relation_derived_from`) + recognition indices
(`inbound_predicate_counts`, `outbound_predicate_counts`,
`meta_relation_presence`).
**Done:** 50-element round-trip; supersession chains walk both
directions via `relation_supersedes` index; debug-asserts fire on
cache-relations-without-`derived_from`-meta-relation (Inv 9) and on
snapshot/log without `ModelFingerprint` (§18.4).

### Step 2 — Snapshot + Bounded WAL (~1 wk)

**Build:** §18 — segmented WAL (1 MB segments, LZ4 hot, zstd-19
closed, 10 MB cap with oldest-segment eviction; `LEGEND_WAL_UNBOUNDED=1`
for dev builds), snapshot serializer stamped with `Tick` +
`ModelFingerprint`, hybrid checkpoint (N=1000 ∨ S=5 MB ∨ T=1 hr),
boot-time fingerprint check that refuses startup on mismatch.
**Done:** crash mid-corpus → restart → state matches; post-checkpoint
WAL truncation works; binary built against a different model refuses
to boot against an existing snapshot.

### Step 3 — Seed Pack (~1.5 wk)

**Build:** Hand-author the seed pack per §16 + `seed_pack.yaml`:
2 anchors (Genesis, Void) + 10 seeded predicates (`instance_of`,
`subclass_of`, plus the 8 meta-relation predicates) + 6 modal elements
+ 15 regions + 11 roles + 8 reference frames. Embed descriptor strings
at boot to compute initial region prototype vectors. Serialize as
`seed_v0.msgpack.lz4`.
**Done:** boot shows ~52 atoms in expected configuration; 2D
projection of region descriptor embeddings clusters sensibly;
meta-relation indices populate from seeded `instance_of` /
`subclass_of` pins.

### Step 4 — Manual Ten-Tick Test (~1 wk)

**Build:** Hard-code §19 via direct `add_element` / `add_relation` (no
NLP).
**Done:** §19 walkthrough passes; `ConsciousAttentionFrame` shape is
right.

### Step 5 — Embeddings + Region Routing (~1.5 wk)

**Build:** §10.2 — `route_regions` (read-only, parallel, top-k DAG) +
`apply_region_delta` (spherical k-means, §10.5). Diff-passing
discipline (§9.7).
**Done:** every span lands in the expected region; multi-prototype
bounded at 8; region-creation rate decays after first 20 ticks.

### Step 6 — Temporal Parser + NER + Relation Extraction (~2 wk)

**Build:** `chrono-english` for the 80% + thin uncertainty-grounding
layer; `gline-rs` zero-shot NER + relations.
**Done:** Tick 1 emits `Tuesday`, `Friday`, `DrRao`, and the
reschedule triple without hand-coding.

### Step 7 — Event Reification + Supersession Cache (~1.5 wk)

**Build:** §14.4 Event Calculus mapping; supersession chains via
`(R, supersedes, R')` meta-relations and the `relation_supersedes`
index; cache relations with paired `(R, derived_from, X)`
meta-relations.
**Done:** Ticks 1/2/5/7 build correct events; chain `Tuesday → Friday
→ Monday` walks both directions.

### Step 8 — Heuristic Coreference + Conservative Instances (~1 wk)

**Build:** §11.8 — recency-based pronoun resolution (Centering Theory
baseline) + pattern separation.
**Done:** Tick 5 "it" → `appointment_1`; `appointment_1` and
`appointment_2` stay separate; Tick 9 reinforces `DrRao` instead of
duplicating. §19 + the three §20.5 fixtures pass end-to-end.

### Step 9 — Lexical Index + Hybrid Retrieval (~1 wk)

**Build:** `tantivy` BM25 over element names + relation role fillers;
RRF fusion of dense + sparse.
**Done:** rare proper nouns / identifiers retrieve correctly even
when dense similarity is low.

### Step 9.5 — Domain-Neutrality Smoke Test (~1 wk)

**Lands before reinforcement and replay** so heuristics can't calcify
around appointments.

**Build:** hand-author one ≥10-tick fixture in a non-appointment
domain (codebase rename, chat preference shift, or novel character
revision). Run Steps 0–9 against it unchanged.
**Done:** fixture passes with the same code path that passes §19, no
domain-specific shortcuts.

### Step 10 — Hebbian + Salience + Decay + Path-Aware Reinforcement (~1.5 wk)

**Build:** §14.6 path-aware reinforcement; §14.7 utility decay; §13.4
salience; co-activation strengthening.
**Done:** across a 100-tick corpus, accessed paths strengthen, unused
links decay, no focus-bearing relation is destructively removed.

### Step 11 — Replay (~2 wk)

**Build:** §9.4 + §14.8 — replay thread gets a snapshot clone, returns
`Vec<ReplayMutation>` for `&mut` apply on main; region split/merge,
mid-path insertion (§10.3.5), coref resolution, prototype eviction,
cache pruning, predicate dedup, cycle resolution (lowest-confidence
retraction), and the background decay sweep (§14.7). Reject any
mutation that breaks §19 or §20.5. Add the **replay-determinism
fixture**: take a starting hypergraph (the §19 walkthrough's
end-state works), run replay twice with different rule-application
orders (e.g. shuffled by a fixed seed; permute the order in which
mid-path / predicate-dedup / cycle-resolution / region-split jobs
are applied), assert bit-identical final hypergraph state. This is
v0's confluence contract — formal proofs are out of scope (§14
preamble); the two-pass fixture is what surfaces violations.
**Done:** 100-tick corpus passes §19 + fixtures; region-creation
rate flattens; mid-path insertion fires when a new element scores
higher cosine to a specific element than to its current parent;
provenance cycles introduced in a fixture get retracted within one
replay tick; replay-determinism fixture passes (two passes →
bit-identical state).

### Step 12 — External Benchmarks (~2 wk)

**Build:** wire LongMemEval `oracle`, MemoryAgentBench
FactConsolidation, RULER MK/MV-NIAH at 8K/32K into the harness.
**Done:** end-to-end numbers logged. Beating SOTA is *not* the v0
goal; passing fixtures + credible external numbers is.

### Step 13 — Reference Frontend: Notes App (~1 wk)

**Build:** the §2.5 notes app — minimal CLI (input field in, rendered
output out), one-thought-per-tick, Qwen-0.5B-class render LLM
verbalizing the returned `ConsciousAttentionFrame`. Render-only to
start; the rest of the render-LLM job is left open and explored by
use.
**Done:** a multi-day personal-notes session and a coding-project
session both exercise Legend end-to-end through the frontend; render
output is readable; quality differences between the two sessions are
diagnosed against the substrate, not the frontend.

**v0 sign-off** = Steps 0–13 pass + §19 deterministic + §20.5
fixtures + Step 9.5 fixture + LongMemEval + MemoryAgentBench +
RULER all produce credible numbers + the §2.5 notes app is in
regular use.

**Total: ~19 wk part-time.** Patterns and pattern-emergence machinery
are deferred to v1 (§24); the v0 build is correspondingly tighter.

### Reviewer Workflow

User writes code → runs the inspection harness → pastes diff → Claude
reviews diff + code, flags spec drift → user iterates. Step is done
when the harness shows zero unexpected diffs across the walkthrough up
to that step.

---

## 22. Source Map

Two passes, read in priority order. **Mathematical Foundations** ground
the substrate (§6). **Substrate / Algorithm** is load-bearing for v0;
the rest are background and reference.

### Mathematical Foundations (§6)

- **Habel 1992** — *Hyperedge Replacement: Grammars and Languages.*
  LNCS 643. Formal definition of typed hypergraphs whose edges have
  labeled connection points (called "tentacles" in the literature;
  Legend calls them slots). Backbone for §6 (1).
- **Ehrig, Ehrig, Prange & Taentzer 2006** — *Fundamentals of
  Algebraic Graph Transformation.* DPO graph rewriting; the formalism
  behind replay-as-transformation (§14 preamble, §14.8).
- **Parsons 1990** — *Events in the Semantics of English.* Neo-
  Davidsonian event semantics; the formal account of role-tagged
  predications used in §7.2.
- **Fillmore 1976; Baker, Fillmore & Lowe 1998** — Frame semantics /
  FrameNet. Informs the `(R, frame, F)` meta-relation as Legend's
  contextual-scope mechanism.
- **Snodgrass 1995** — *Developing Time-Oriented Database Applications
  in SQL.* Bitemporal model reference; pairs with Datomic/XTDB for
  §6 (3).
- **Green, Karvounarakis & Tannen 2007** — *Provenance Semirings*,
  PODS. Closed-form aggregation rules for derived data; backbone for
  §6 (4) and §7.2 `derived_from` / supersession.
- **Richardson & Domingos 2006; Domingos & Lowd 2009** — *Markov
  Logic Networks.* Weighted FOL with online weight updates;
  structural match to Legend's relation weights and replay (§6 (5),
  §14 preamble).
- **Wolfram 2020 — *A Path to the Fundamental Theory of Physics*.**
  The one-primitive + relations design family that Legend's substrate
  belongs to. Reference for §6 (6) and §1.6.
- **Wolfram 2002 — *A New Kind of Science*.** Computational
  irreducibility, bounded observers, pockets of reducibility. Reference
  for §6 (7) and §1.7. Read the chapter on irreducibility ("The
  Phenomenon of Computational Irreducibility") for the full argument;
  the rest of NKS is supporting evidence at book length.

### Substrate / Algorithm

1. **DDVFA** — Brito da Silva, Elnabarawy & Wunsch, *Neural Networks*
   116 (2019), arXiv 1901.00794. Closest published kin to §10
   (two-level vigilance + multi-prototype + Merge-ART). **Read
   end-to-end before writing v0 region code.**
2. **ART Survey** — Brito da Silva et al. 2019, arXiv 1905.11437.
   Failure-mode catalogue used for §10.6.
3. **Adaptive Resonance Theory** — Carpenter & Grossberg. Vigilance /
   resonance / stable-plastic learning; conceptual backbone of §10.
4. **GNG + GWR** — Fritzke 1995; Marsland, Shapiro & Nehmzow 2002.
   GWR's activation-and-firing-counter add criterion is closer to
   Legend's `descend_threshold` than vanilla GNG.

### Bounded Hebbian Learning

5. **Oja 1982 — *J. Math. Biol.* 15:267-273.** Bounded Hebbian
   operators (§14.9) used by §14.6 path-aware reinforcement and Step
   11's co-activation update.

### Truth Maintenance / Temporal / Provenance

10. **Event Calculus** — Kowalski & Sergot, *New Generation Computing*
    4(1), 1986. 40-year foundation for §14.4. Shanahan's modern
    formulation: doc.ic.ac.uk/~mpsha/ECExplained.pdf.
11. **PROV-O (W3C 2013)** — vocabulary for `derived_from` and
    `supersedes` (Inv 9, §7.2).
12. **Wikidata data model** — statements/qualifiers/references/ranks.
    Legend's design rejects Wikidata's qualifier struct in favor of
    meta-relations (everything Wikidata calls a qualifier is, in
    Legend, a relation whose subject is the modified relation), but
    the role catalogue (frame, valid_from/to, source, modality) maps
    directly. See §7.2.
13. **JTMS / ATMS** — Doyle 1979; de Kleer 1986. Legend's relation-
    status discipline is JTMS-flavored.
14. **AGM + Hansson Base Revision** — Levi identity is the formal name
    for Legend's correction protocol.
15. **TimeML / TempEval-3** — temporal annotation standard; Legend
    adopts the 7-relation pragmatic subset.

### Knowledge Representation Background

16. **Wolfram Physics Project** — wolframphysics.org. The one-
    primitive + relations design family Legend belongs to. Read the
    "Time and Spacetime" page for the events-as-causal-graph framing.
17. **Wolfram Language `Entity[]`** —
    reference.wolfram.com/language/ref/Entity.html. Typed knowledge
    representation in the same code family that hosts the Physics
    Project.
18. **Wikidata** — instance-of (P31) / subclass-of (P279) split;
    informs §8.1, §8.2.
19. **Cyc** — Individual vs Collection; isa vs genls; relevant for
    §8.1, §8.2.
20. **BFO/DOLCE** — continuant vs occurrent; relevant for §8.3 (events
    as a top-level ontological category).

### Durability / Materialized Views

21. **Write-Ahead Logging** — every relational DB ever shipped.
    Legend's §18 follows the WAL pattern, not full event sourcing.
22. **Datomic / XTDB** — referenced only for the bitemporal data model
    (§7.2 + Inv 6), not for log-as-ground-truth.
23. **Differential Dataflow** — McSherry, Murray, Isaacs et al., CIDR
    2013. Diff-passing discipline (§9.7).
24. **Salsa** — Rust pure-spec / `&mut`-impl pattern (rust-analyzer).
    Closest existing Rust analog to Legend's brain-processes-as-
    functions discipline.
25. **IVM** — PostgreSQL IVM wiki + Cui & Widom (TODS 2000).
    Background for `derived_from`.

### Comparable Memory Systems

26. **Graphiti / Zep** — Rasmussen et al. 2025, arXiv 2501.13956.
    Bi-temporal KG for agent memory; closest production competitor to
    §7.2 + §7.3.
27. **HippoRAG 2** — Gutiérrez et al. 2025, arXiv 2502.14802. Dual-
    node KG + Personalized PageRank.
28. **A-MEM** — NeurIPS 2025, arXiv 2502.12110. LLM-driven memory
    evolution.
29. **Mem0** — arXiv 2504.19413. Hybrid vector + graph + KV memory
    layer.

### NLP / Embedding / Retrieval

30. **Sentence-BERT** — Reimers & Gurevych 2019, arXiv 1908.10084. Why
    raw BERT is not an embedding model.
31. **BGE technical report** — arXiv 2309.07597. The v0 embedding
    model.
32. **GLiNER paper** — arXiv 2311.08526. Zero-shot NER used by
    `gline-rs`.
33. **`tokenizers`** — HuggingFace, Apache-2.0, pure-Rust.
34. **`tantivy`** — Quickwit-OSS, Lucene-grade BM25 in pure Rust.
35. **`gline-rs`** — fbilhaut, GLiNER inference on `ort`.
36. **`ort`** — pyke.io, Rust ONNX Runtime wrapper.

### Cognitive Background

37. **FrameNet** — frames + frame elements; informs the
    `(R, frame, F)` meta-relation as Legend's contextual-scope
    mechanism.
38. **AMR paper** — sentence meaning as graph; design reference only,
    not v0.
39. **Centering Theory** — Grosz, Joshi, Weinstein 1995. Recency-based
    coreference baseline.

### Benchmarks

40. **LongMemEval** — ICLR 2025, arXiv 2410.10813. v0 evaluation gate.
41. **MemoryAgentBench** — ICLR 2026, arXiv 2507.05257. Fact
    Consolidation = supersession semantics test.
42. **RULER** — COLM 2024, arXiv 2404.06654. MK/MV-NIAH smoke tests.
43. **AbstentionBench** — FAIR 2025, arXiv 2506.09038. "Don't
    hallucinate when you don't know."

### Deferred / Not v0

- **BIRCH (1996)** — threshold-gated descent pattern only; CF-tree
  breaks under multi-prototype + cosine + DAG.
- **HNSW** — possibly a fast-lookup index *over* regions later.
- **RDF 1.1** — triple baseline; n-ary reification is the practical
  model.

Considered and dropped: Stanford OpenIE (JVM), AllenNLP SRL docs
(Python), BERT paper (not an embedding model — see Sentence-BERT),
MiniLM (subsumed by BGE-small lineage), LoCoMo (scoring controversy
— §20.6).

---

## 23. Deferred Questions

These remain open. None block v0.

- When (if ever) does AMR/UMR earn its way back into the pipeline?
  Likely trigger: §20 metrics show consistent failures on document-
  level temporal reasoning that SRL + temporal parser cannot recover.
- What is the right cold-storage policy after v1? v0 keeps the full
  hypergraph in memory.
- What is the right replay scheduling cadence? Per-tick? Every N
  ticks? Idle-only? Profile in v0 step 9.
- Should query success reinforce only the selected path, or also
  nearby alternatives at lower weight? v0 does selected-only; revisit
  once reinforcement metrics are visible.
- Should `HashMap` swap to `hashbrown` or a hand-rolled open-
  addressing table? Decide on first profile, not earlier.
- When does the wide `MemoryStats` struct split into parallel
  `Vec<f32>` arrays for cache locality? Decide on first profile.
- When does `HNSW` (or another approximate-NN index) get added on top
  of the region DAG for fast lookup? When the DAG search becomes a
  measurable bottleneck.
- When (if ever) should new payload tables be added at runtime instead
  of being pre-defined in the substrate? v0 keeps the payload-table
  set fixed; v2 may reconsider for emergent payload kinds.
- When does recognition switch from hardcoded to adaptive
  (percentile-based) thresholds? When v0 corpus data shows the
  distributional shape per recognition kind. v0 ships hardcoded
  defaults (concept ≥ 3, frame ≥ 5) in `Policy` (§9.3); revisit once
  inbound-count distributions are observable on real corpora.
- What is the model-swap recovery story when sources are unreachable?
  v0 accepts loss per the §15.1 recoverability matrix. v1 will design
  a recovery path informed by the actual source-class distribution
  observed in real Legend instances. Candidate approaches:
  secondary-embedder rotation (write under primary + secondary, swap
  primary while existing entries stay reachable via the secondary),
  opt-in source-text retention (`Policy.retain_source_text` for
  consumers who'd rather store transcripts than lose elements on
  swap), or a hybrid. Each trades the brain-analogy purity for
  recoverability; choosing requires data v0 doesn't have yet.
- Should `Defeasible → Asserted` promotion be automatic (replay
  decides on threshold) or human-confirmed (replay flags candidates,
  user approves)? v0 ships fully-automatic at `support_count >= 3`;
  revisit if drift is observed.
- Should write-time provenance-cycle detection be added in v1, or stay
  replay-only per Invariant 15? v0 stays replay-only; v1 may add a
  cheap one-hop check on `derived_from` writes if cycle resolution
  proves expensive in practice.

---

## 24. Beyond v0

This section consolidates the v1+ ideas referenced throughout the
doc. None of these are scoped for v0; they are noted here so the body
of the spec can stay focused on what the first cut ships with.

### 24.1 Pattern Templates as a First-Class Citizen

The most consequential v1 addition: re-introduce **patterns** as a
relation kind with `Term::Variable` slots and matchers
(`input_prototype`, `neighborhood_prototype`, `surface_triggers`).
v0 dropped patterns because emergence — minting new patterns from
clustered relation shapes — was unvalidated and added substantial
machinery. The v0 substrate handles concept/instance/event recognition
through derived indices (§8) and explicit extraction; this is enough
to ship.

Patterns are simultaneously a **quality** play and a **latency** play:

- **Quality.** A mature, recurring shape (e.g. the reschedule-event
  shape that fires across many domains) becomes one relation with
  Variable slots; instantiation is "bind variables to concrete
  fillers, write the resulting concrete relation, link via
  `derived_from`." Replay can mint these from clustered relation
  shapes per §1.7's inward-compression argument. Active templates
  also contribute slot priors that bias extractor attention before
  extraction runs.
- **Latency.** Surface-pattern templates can fire as a **fast-path
  extractor** for inputs that match a known shape — ~5–20 ms per
  tick on a hit, vs GLiNER2's 130–208 ms. GLiNER2 stays in slot 6
  as the fallback for novel/complex inputs. Average tick latency
  drops as pattern hit-rate climbs on mature corpora; the §11.0
  budget moves from ~250 ms p50 toward ~80–150 ms p50 once a
  reasonable pattern library accumulates. This is the cheapest path
  toward sub-100 ms p50 because it doesn't require swapping the
  extractor architecture.

The v1 work is: implement `Term::Variable` and a `pattern_matchers`
side table; add an `activate_patterns` step before extraction (or as
a fast-path that short-circuits Step 6 on match); add a replay job
that clusters concrete relation shapes and mints new pattern
relations with `Defeasible` status; add the `Defeasible → Asserted`
promotion path for patterns specifically.

### 24.2 Latency Optimization (Secondary Contributors)

v0 targets ~200–300 ms p50 per tick. The path to sub-100 ms is
dominated by Step 6 (zero-shot relation extraction) — see §15.1's
GLiNER2 callout, §24.1's pattern fast-path, and §24.7's unified
tiny-LLM extractor. Those are the *primary* paths; this section
lists the **secondary** contributors that help once the long pole
is shorter.

- **Read-path / background-work split.** v0 already pushes the
  full-graph decay sweep (§14.7) and predicate dedup (§14.8) onto the
  replay thread, so the tick path is mostly free of these. v1 can
  push the remaining incremental decay (Step 12) onto a per-tick
  background hand-off if profiling shows it on the critical path.
  Unlike pre-rewrite expectations, this split is small: the decay
  budget at §11.0 is 3–8 ms, not tens of ms.
- **Interning predicate names** — small fixed table of `u32` ids
  alongside the `ElementId` lookup, so hot extractor paths skip a
  hash probe. Saves ~1–3 ms per tick.
- **Splitting the wide `MemoryStats` struct** into parallel
  `Vec<f32>` columns for cache locality (already in §23 deferred
  questions). Saves a few ms on Steps 11–12.

These together knock 5–15 ms off a tick. They are not a substitute
for Step 6 changes; they are what closes the last gap once the
extractor slot is faster.

### 24.3 Hierarchical / Composed Frames

v0 frame scope is flat (§3.4): a relation either is or is not in a
given frame, and frames don't transitively contain each other. v1
could add a `(F1, contains_frame, F2)` meta-relation and have frame-
scoped queries follow the containment chain — useful for "every
project frame inherits the user's preferences" without writing the
inheritance explicitly on every relation. Defer until a real consumer
asks for it.

### 24.4 Storage-Tier Quantization (INT8)

v0 stores embeddings as FP32. INT8 stored embeddings (with the
embedder still doing inference at INT8) cut substrate memory roughly
4× with measured cosine error in the ±0.01 range, well below the
similarity thresholds Legend uses. The migration is one-time and
backward-compatible via the `ModelFingerprint` boot check; defer
until memory pressure or snapshot size becomes the pain point.

### 24.5 HNSW Over Regions

v0 region routing is a DAG descent — fine when there are hundreds to
low thousands of regions. Once region count grows past that, layering
an HNSW (or similar approximate-NN index) over region prototypes
becomes the right move. The DAG stays as the structural model; HNSW
becomes the lookup accelerator.

### 24.6 Forward-Chaining Inference for Rules

Conditional relations exist in v0 via the `(R, antecedent_of, R')`
meta-relation, but no machinery automatically applies them — they're
inert state. v1 could add a forward-chaining pass during replay that
fires conditions whose antecedents now hold and emits the consequent
as an `Entailed` relation with `derived_from` lineage back to the
rule.

### 24.7 Local LLM as Unified Extractor

The v0 extractor stack is multiple separate models (NER + temporal
parser + GLiNER2 + heuristic coref). A small local LLM (Qwen-0.5B /
SmolLM-360M / Phi-3-mini class) could replace the entire extractor
stack with structured-output prompting — one model, one inference
call per tick, more flexible label sets. The trade-offs:

- **Latency.** ~50–150 ms on CPU INT8 with greedy decoding (matches
  or beats GLiNER2 at the smaller model sizes). Faster on a small
  GPU.
- **Quality.** Small LLMs are surprisingly good at structured
  extraction with sharp prompts. Quality on novel-domain extraction
  often exceeds zero-shot relation extractors of comparable size.
- **Flexibility.** One inference call subsumes NER + relation
  extraction + temporal parsing + heuristic coref. Pipeline gets
  simpler.
- **Determinism.** Greedy decoding gives deterministic output for a
  fixed (model, prompt) pair. ONNX runtime supports this.
- **Bundle size.** A 1.5B INT8 model is ~1.5 GB; a 0.5B INT8 model
  is ~500 MB. Both heavier than gline-rs (~100–500 MB).
- **Structured-output failure modes.** JSON-schema validation
  required; small LLMs occasionally produce malformed structured
  output that has to be retried or fall back.

This is one of the two primary paths to sub-100 ms p50 (§24.1 is the
other). Together with §24.1 pattern fast-paths, the unified extractor
handles the novel-input slow path: pattern hit → ~5–20 ms; pattern
miss → unified-LLM ~50–150 ms instead of GLiNER2's 130–208 ms.
Revisit once v0 extractors hit a quality ceiling on real consumer
traffic, or earlier if latency proves binding for the §2.5 notes app
or coding-project use.

### 24.8 Lexicon-Paired-Noun Compound Acceleration

Mentioned in passing in §11.7: when both components of a compound
noun (`X Y`) are already known concepts, propose `Y → X Y` as an
intermediate-region candidate upfront rather than waiting for replay
to discover it via mid-path insertion (§10.3.5). This accelerates the
cases where the components are already known, without changing the
general replay-driven discovery path.
