# New Foundation

> **Status:** Living architecture spec for **Legend v2** — long-term memory
> for LLMs (Large Language Models), built as a hypergraph that accumulates
> discoveries about its world. One primitive (Elements), typed connections
> (Relations), and a single-verb API (Application Programming Interface) —
> `tick`. 
> **Audience:** A solo developer should be able to read this top to bottom
> and start coding against it without consulting prior versions of Legend.

---

## 0. Reading Guide

This document is structured in three layers. Read them in order; jump back
when something later refers to something earlier. §0.1 below is the
authoritative one-page contract — implementers should keep it in
view while reading.

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
- §7 — The Substrate (Element, Relation).
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

### 0.1 v0 Contract (At-a-Glance)

Authoritative one-page contract; details in the cited sections.

```text
SUBSTRATE TYPES        Element (with inline Vec<f32> embedding),
                       Relation, Term::{Element, Relation},
                       RelationStatus::{Asserted, Entailed, Defeasible,
                       Superseded, Retracted}, MemoryStats          (§7, §9)

NO PAYLOAD TABLES      Two primitives. Region structure (member_of,
                       parent_region, lateral_region, prototype) is in
                       the relation graph; embeddings are inline on
                       Element; typed values are Elements whose names
                       parse on comparison. v0 carries no side tables. (§7, §10)

PUBLIC SURFACE         fn tick(&mut Hypergraph, Input,
                                  source: Option<ElementId>)
                                  -> ConsciousAttentionFrame         (§2.1, §11.1)
                       — every input is one tick; no separate query path
                       — source is a sibling parameter, not on Input
                         (it's meta-relation-shaped, §7.2)

PROCESS MODEL          single binary; daemon mode (`legend start`) or
                       in-process (`LEGEND_INPROC=1 legend "..."`);
                       same tick code path either way. Lock-enforced
                       single-writer invariant via flock on
                       .legend/legend.lock; daemon listens on TCP
                       loopback, port discovered via .legend/legend.port.
                                                                       (§18.7)

PHASES                 Step 0     WAL (Write-Ahead Log) append
                                  (durability I/O)
                       Steps 1–6  read-mostly (&Hypergraph, parallel)
                       Steps 7–12 mutation    (&mut Hypergraph, seq)  (§4.2, §4.3)

INTENT VECTOR          Intent { conviction, prediction_error,
                                     arousal, curiosity } — 4-dim,
                       per-dim logistic-regression classifier over
                       MiniLM embedding ++ lexical features (418 dims),
                       trained build-time from seed pack; modulates
                       default_conf, salience, vigilance, hebbian_rate,
                       supersession_threshold. Does NOT gate which steps
                       run. Maps to DA (dopamine) / NE (norepinephrine) /
                       cognitive analogs.                              (§10.6, §11.2)

DURABILITY             snapshot (LZ4+MessagePack) + bounded WAL
                       (10 MB cap, LZ4 hot, zstd-19 closed),
                       checkpoint at N=1000 ticks ∨ S=5MB ∨ T=1hr,
                       boot fingerprint check refuses on mismatch     (§18)

EMBEDDER               all-MiniLM-L6-v2 (INT8-quantized) via the
                       in-house pure-Rust BERT engine
                       (`src/inference/`; no tract, ort, or C deps),
                       pinned for life — model swap = re-ingest
                       per recoverability matrix                      (§15.1, §18.4)

LATENCY BUDGET (v0)    ~80–230 ms p50; Step 5 GLiNER NER dominates    (§11.0, §15.1)

CONFORMANCE GATES      §19 ten-tick walkthrough (substrate, mocked extractors)
                       §20.5 three companion fixtures (instance separation,
                       supersession blindness, frame drift)
                       §21 Step 11 replay-determinism fixture
                       LongMemEval + MemoryAgentBench + RULER (full-stack) (§20)

KEY INVARIANTS         15 numbered items                              (§5)
```

---

## 1. Executive Summary

This section gives you the entire design, compressed. The rest of the doc
is detail.

### 1.1 What Legend Is

Legend is **long-term memory for LLMs** (Large Language Models) —
including future sessions of the model reading this document. LLM
sessions are fleeting by default; Legend is the persistent substrate
that lets continuity carry across them. It is not a chatbot, not a
RAG (Retrieval-Augmented Generation) store, not a knowledge graph
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
is an evidence-weighted hyperedge expressed as a flat list of named
attributes. Each attribute binds an attribute-name (itself an Element
— `instance_of`, `subject`, `target`, `frame`, `valid_from`, …) to a
value (an Element or another Relation). The relation also carries a
status (asserted, defeasible, superseded, retracted, entailed),
memory stats (confidence and decay), a defeasible-priority, and a
creation tick. There is no separate predicate slot and no separate
role-binding struct — both collapse into the uniform attribute list.
Anything that *modifies* a relation — its frame scope, valid-time,
source pointer, modality, supersession links, lineage, conditional
antecedents — is itself a relation whose attribute value is the
modified relation. There is no annotation layer; there are only
relations. Relations are how Legend says anything about anything,
*and* how Legend says anything about its own claims.

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

(Mental model: **Legend runs like a game loop.** Input → update the
whole hypergraph → render only the attention-relevant subgraph as a
frame. The vocabulary is deliberate; full analogy in §4.0.)

User says: *"My dentist appointment with Dr. Rao changed from Tuesday to
Friday."*

Legend processes this as a single discovery:

1. **Embed and route.** The input is segmented into clauses, each
   embedded by a pinned MiniLM-L6-v2 encoder, then routed through the
   semantic-region DAG (Directed Acyclic Graph). Active regions:
   `appointments`, `dental_appointments`, `change_history`.
2. **Run extractors.** NER (Named Entity Recognition), temporal
   parsing, zero-shot relation extraction produce element/relation
   candidates: elements for `user`,
   `Dr. Rao`, `dentist`, `appointment_1`, `Tuesday`, `Friday`,
   `reschedule_event_1`; relations binding them. Active regions bias
   the attribute-name label set toward warm attribute names from this
   part of the graph.
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
focused relation; the calling LLM reads `Friday` off that frame.
There is no separate query API and no pre-assembled answer field —
retrieval shares the same code path as writes (§12).

### 1.4 How The Substrate Is Stored

The hypergraph is one in-memory struct, durably mirrored by snapshot +
WAL.

```rust
struct Hypergraph {
    // Two storage primitives — that is the entire substrate.
    elements: Vec<Element>,        // each carries an inline Vec<f32> embedding
    relations: Vec<Relation>,      // includes region structural relations
                                   // (member_of, parent_region, lateral_region, prototype)

    // No payload tables. Region topology lives in the relation graph;
    // typed values (dates, quantities, locations) are Elements whose
    // names parse at comparison time.

    clock: Tick,
    policy: Policy,
    recent_focus: VecDeque<RecentFocusEntry>,

    // Derived indices — rebuild on load, never serialize.
    // Includes region indices (region_members, region_parents,
    // region_children, region_lateral, region_prototypes — derived
    // from the region structural relations), meta-relation lookups
    // (meta_relations_by_subject / by_object), and recognition indices
    // (inbound / outbound attribute counts, meta-relation presence).
    // Full list in §9.2.
    by_name:                        HashMap<String, Vec<ElementId>>,
    region_members:                 HashMap<ElementId, Vec<ElementId>>,
    region_parents:                 HashMap<ElementId, Vec<(ElementId, f32)>>,
    region_children:                HashMap<ElementId, Vec<ElementId>>,
    relations_by_element:           HashMap<ElementId, Vec<RelationId>>,
    relations_by_attribute_name:    HashMap<ElementId, Vec<RelationId>>,
    meta_relations_by_subject:      HashMap<RelationId, Vec<RelationId>>,
    meta_relations_by_object:       HashMap<RelationId, Vec<RelationId>>,
    // ... recognition indices ...
}
```

**Durability.** A bounded write-ahead log (10 MB, segmented, queue-style
oldest-eviction; LZ4 hot, zstd-19 closed) sits alongside the hypergraph
for crash recovery between snapshots. Snapshots are stamped with a
`ModelFingerprint` (embedding-model hash, tokenizer vocab hash, code
version) that is checked at boot — refuse to start on mismatch. Boot =
load latest snapshot, replay WAL suffix on top.

**Embedder pin.** The embedder (all-MiniLM-L6-v2 quantized, via
tract-onnx) is pinned for Legend's lifetime. Model swap costs are
stratified by source class
(§15.1 recoverability matrix); for coding-project use the unrecoverable
share dominates over time. The pin is what the design costs, not a
conservative default — treat as load-bearing infrastructure that does
not get swapped without an explicit recovery plan.

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
per-input record. Inputs that produce no extractable elements or
relations are discarded (the dev-only WAL keeps raw text for
debugging; production has only the distilled relations).

**Who owns what** (full version §16.1):

```text
Code owns mechanics — substrate types, the tick pipeline, decay/
  reinforcement/replay machinery, the embedding interface.
Seeds own priors — anchors, the meta-relation attribute names,
  the behavioral modal attribute names, broad regions, generic
  participant attribute names, reference frames.
Inputs own truth — Legend keeps the distilled relations, not the
  inputs themselves.
Replay owns consolidation — region splits/merges, mid-path inserts,
  cycle resolution, attribute-name dedup, the background decay sweep.
```

### 1.6 Why One Primitive

Legend's job is to model the entire **written** world that a project
sits in. That means new ontological categories will keep arriving:
new domains, new subject matters, new conceptual frameworks. A typed
substrate (`ElementKind::Concept`, `ElementKind::Event`, …) locks in
a predetermined ontology and forces every new structure to be slotted
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

**Scope of the no-pre-declared-categories bet.** This applies to
*world-content* categories — what kinds of things exist in the
modeled domain (concepts, instances, events, frames, attribute names).
The seed pack does pre-declare a small set of *substrate-mechanism
anchors*: the meta-relation attribute names (§7.2), the five
behavioral modal attribute names (§16.3 — `negated`, `uncertain`,
`non_actual`, `general`, `intervened`), and the four causal-relation
attribute names (§16.3 — `caused`, `correlated_with`, `enables`,
`prevents`). Recognition machinery, meta-relation routing, and
"why"-shaped retrieval read these by name, so they have to be
present at boot. Pre-committing the substrate's plumbing is not
the same as pre-committing the world's ontology — the bet is about
the latter.

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
compression of recurring structures via pattern templates) is on the
v1 horizon (§24.1).

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
- `x` — the input (text + wall-clock) plus an optional source pointer
  passed as a sibling parameter.
- `G'` — the hypergraph after this tick.
- `A` — the **attention frame** (a `ConsciousAttentionFrame`, full
  type spec in §11.13): a structured snapshot of what fired, what's
  in focus, what changed, what's uncertain, what to replay next.

In Rust:
`fn tick(&mut Hypergraph, Input, source: Option<ElementId>)
    -> ConsciousAttentionFrame`. The `&mut` is the operational form
of `G → G'`.

This is the *internal* contract. The *external* contract is a CLI
(Command-Line Interface): `legend "..."` runs as either a thin client
to a long-lived daemon (`legend start`) or as a one-shot per-invocation
process. Both modes call the same `tick()`; the daemon amortizes
substantial cold-start cost (~700 ms – 1.5 s) when one is running.
Lock-enforced single-writer invariant; full process model in §18.7.

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
- Anything Python or JVM (Java Virtual Machine; no sidecars, no exceptions).

### 2.5 First Consumer: A Notes App

The first thing Legend ships against is a minimal notes app — input
field in, rendered output out, CLI to start. It exists to prove Legend
works end-to-end against a real consumer, before it's wired into
larger agentic systems.

The shape:

1. The user types a thought.
2. The frontend hands the raw text to Legend as one tick. One thought
   per tick — no batching, no segmentation in the frontend. **This is
   a frontend convention specific to the notes app, not a substrate
   constraint.** Other consumers (a Slack-channel watcher, a
   coding-project file-event listener) batch differently; the
   substrate accepts inputs of any size and uses §11.4 segmentation
   internally.
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
    an MCP (Model Context Protocol) client) read and write the same
    Legend. Continuity isn't just one agent across time — it's across
    agents at once.
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

```text
                          THE HYPERGRAPH
                            (two primitives)

  ┌─────────────────────────────────────────────────────────┐
  │  ELEMENTS                       RELATIONS               │
  │  (identity + inline             (claims, hyperedges)    │
  │   Vec<f32>)                     ┌──────────────────┐    │
  │  ┌──────┐                       │ R1: attributes   │    │
  │  │  E1  │◄──── attribute  ──────┤   name₁ → E1     │    │
  │  │  E2  │      values           │   name₂ → E2     │    │
  │  │  E3  │           +           │   status, stats  │    │
  │  │  ...  │      confidence     └─────────┬────────┘    │
  │  └──────┘           +                     │             │
  │                  status                   │             │
  │                                           │             │
  │     region structure              meta-relations        │
  │     ─────────────────             ───────────────►      │
  │     (E, member_of, R)             (R, frame, F)         │
  │     (R_a, parent_region, R_b)     (R, supersedes, R')   │
  │     (R_a, lateral_region, R_b)    (R, valid_from, T)    │
  │     (R, prototype, P)             (R, derived_from, X)  │
  └─────────────────────────────────────────────────────────┘

           DERIVED INDICES (rebuilt on load, never serialized)
           ───────────────────────────────────────────────────
           attribute_value_counts[E][N]   → "concept" / "frame"
           attribute_co_counts[E][N]      → "instance"
           meta_relation_presence[R]      → "event-shaped"
           region_members[R], region_parents[R], region_children[R],
                                          → region topology cache
           meta_relations_by_subject[R]   → meta-rels targeting R
           meta_relations_by_object[R]    → meta-rels mentioning R
                                            in non-target attributes
```

The four pieces:

1. **Elements** (§3.1) — bare identities with optional inline embedding.
2. **Relations** (§3.2) — typed hyperedges, uniform with meta-relations
   *and* with region structural relations.
3. **Discoveries** (§3.3) — what each tick is, semantically.
4. **Emergence** (§3.4) — kinds read from the recognition indices.

Region topology (membership, parenthood, prototypes) lives in the
relation graph; typed leaf values (dates, quantities, locations) are
Elements whose names parse at comparison time (§3.5 / §7.3). v0
carries no payload tables.

### 3.1 Elements: The One Primitive

An **Element** is a bare identity Legend can refer to. It has:

- An id (`ElementId`, `u32`).
- Zero or more names (strings — canonical, variant, alias all in one
  list; lifecycle is uniform).
- Memory stats (activation, confidence, plasticity, salience, access
  count, last seen, etc.).
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

- One or more **attributes** — each binding an attribute-name (an
  Element) to a value (a `Term`: Element or Relation). The attribute
  names tell you what kind of relation this is and what each
  participant's role is — `instance_of`, `subject`, `provider`,
  `target`, `from`, `to`, or any meta-attribute like `frame`,
  `valid_from`, `source`, `negated`, `uncertain`, `non_actual`,
  `supersedes`, `derived_from`.
- A **status** — asserted, entailed, defeasible, superseded, retracted.
- A **priority** for defeasible tie-breaking.
- Memory stats — relations decay and reinforce just like elements
  (confidence is one of those stats).

That's it. There is no separate "predicate" slot and no separate
"role" struct — both collapse into a uniform attribute list, because
the distinction was never structural in the first place (a predicate
is just the attribute name that names the relation kind; a role is
just an attribute name that names a participant slot). Relations have
no qualifier struct, no `supersedes` / `derived_from` fields, no
status sub-fields. Anything that *modifies* a relation — frame scope,
valid-time, source, modality, supersession chain, lineage, conditional
antecedent — is itself a relation whose subject (one of its
attribute values) is the modified relation, via
`Term::Relation(RelationId)`.

Relations are first-class substrate citizens. They are not passive labels
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
  marked `Defeasible`, qualified via `[uncertain: <surface-form>,
  target: meta_R]`, or superseded. "I'm not sure that scope assertion
  was right" is just an `uncertain` meta-relation on a frame
  meta-relation.
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

1. **Indices are flat, not recursive.** A frame lookup reads
   `meta_relations_by_subject[R]` filtered by attribute name=`frame`
   and returns the value Term (an ElementId), not the
   meta-relation itself. To reason about the meta-relation —
   its status, source, modality — recurse: read
   `meta_relations_by_subject[meta_id]` for that meta-relation's
   own meta-relations. Hot path stops at depth-1; reflective
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

- **Concept** — element with `attribute_value_counts[E][instance_of]
  >= policy.concept_recognition_threshold` (default 3). Many other
  relations bind E as the value of an `instance_of` attribute.
- **Instance** — element with non-zero
  `attribute_co_counts[E][instance_of]`. It participates in at least
  one relation that asserts an `instance_of` claim — itself an
  instance of one or more concepts.
- **Event** — element mentioned by relations whose attribute lists
  combine participant slots (`target`/`from`/`to`/`actor`/`time`) with
  valid-time meta-relations (`(target: R, valid_from: T)` /
  `(target: R, valid_to: T)`).
- **Reference frame** — element with `attribute_value_counts[E][frame]
  >= policy.frame_recognition_threshold` (default 5).

A given element can be functioning as multiple kinds at once.
`healthcare_provider` is both an instance (of `concept`) and a concept
(`dentist` is an instance of it). Both index entries are non-zero;
both behaviors apply.

**How recognition affects behavior.** Each recognition reads an index
threshold and routes a specific behavior:

- **Coreference** (§14.3) reuses concept-like elements broadly and
  treats instance-like elements with pattern separation. The merge
  bias is computed from `attribute_value_counts` directly.
- **Supersession** (§11.10) fires when a relation's
  meta-relation-presence indicates state-change shape (`from`/`to`
  attributes on a `property` relation, plus valid-time).
- **Frame-relative scoping** reads `meta_relations_by_subject[R]`
  filtered by attribute name=`frame` to find each relation's frame.
  Elements with many inbound `(?r, frame, ?)` references are
  recognized as frames at query time. **Frame scope is flat in
  v0.** A relation either is or is not in a given frame; there is no
  transitive inheritance — a relation scoped to `FRAME_PROJECT`
  doesn't automatically inherit `FRAME_USER` even if the project is
  the user's. **v0 retrieval operates within a single active frame at
  a time** (`ConsciousAttentionFrame.active_frame`); cross-frame
  access requires the consumer to issue a separate tick under the
  other frame, or to author explicit `(R, also_in_frame, F')`
  meta-relations on the relations that should appear in multiple
  frames. Hierarchical/composed frames are a v1+ idea (§24.3).
- **Decay** treats event-like elements (valid-time-bounded) and
  persistent state differently.

These are observed properties of the graph, not stored tags.
Recognition is a function of the indices; the indices are derived
from the relations.

**The seed pack is bootstrapping, not a closed vocabulary.** Attribute
names, concepts, and participant slots all emerge by default — Step 5
(§11.7) mints new attribute-name elements from extractor proposals.
The handful of attribute names that *are* seeded (`instance_of`,
`subclass_of`, the participant attribute names, and the meta-relation
attribute names — see §16) are seeded because they're **load-bearing
for emergence recognition itself**: the recognition rules in this
section refer to `instance_of` by name, so without it present at boot,
replay would have to rediscover one of its own logical primitives
before any other emergence could be observed. Seeding fixes those
anchors so the rest of the lattice can be discovered cheaply. Seeded
attribute names are not privileged in code — there's no
`if name == INSTANCE_OF` branch — only privileged in being *present at
boot*.

### 3.5 No Payload Tables

Two storage primitives. That is the whole substrate.

- **`Element.embedding: Vec<f32>`** — semantic anchor for the element.
  Populated at mint time by embedding `names` (or, for anonymous NER
  spans, the originating span text held as a variant name). Seed-pack
  elements are embedded at boot from descriptors or canonical names.
  Hot-path access is one struct read — no HashMap indirection.
- **Region structure** — membership, parent/child, lateral edges, and
  prototype attachment are all expressed as ordinary relations between
  elements (§10). Region topology is recovered through derived indices
  over those relations — same pattern as the meta-relation indices for
  frame, source, and supersession.
- **Typed values are Elements.** "Tuesday", "6 pounds",
  "2026-04-30", "Berlin" are bare Elements whose `names` carry the
  surface forms ("Tuesday" / "Tues" / "tue" coreference into one
  Element). When code needs typed semantics — date ordering, unit
  conversion, interval overlap — it parses the name on comparison
  (chrono for dates, a quantity parser for numbers, a geo parser for
  locations). v0 stores no parsed form: parsing is cheap relative to
  GLiNER2's 130–208 ms per tick, and skipping the cache lets the
  substrate stay at exactly two primitives.

The §1.6 "one primitive" claim is about *identity* — there is one
identity primitive (Element). With values folded into Elements and
region structure folded into Relations, the substrate carries no
side tables at all in v0. Future versions may revisit (e.g. a
parallel `Vec<f32>`-per-element embedding array for SIMD (Single Instruction Multiple Data) scans, or a
typed-comparison cache if profiling shows parse cost dominating);
v0's job is to prove the two-primitive design works without them.

---

## 4. How A Tick Works (Conceptual)

This section walks an input through Legend without dropping into types
yet. §11 specifies the same pipeline at the type level.

A tick is one call into Legend: one input, one updated hypergraph, one
attention frame returned. Legend has no separate query path — every
interaction (user message, file event, agent observation) becomes an
`Input`, gets handed to a single function, and produces a structured
snapshot of what's now in focus.

### 4.0 Legend Runs Like A Game Loop

The vocabulary is not accidental. **Legend's tick → frame cycle is the
same shape as a game's input → update → render cycle.**

```text
                GAME ENGINE                          LEGEND
                ───────────                          ──────
                player input                         tick input
                     │                                    │
                     ▼                                    ▼
                process input                      process input
                     │                                    │
                     ▼                                    ▼
                update entire                      update entire
                game state                         hypergraph
                     │                                    │
                     ▼                                    ▼
                render only what's                 render only what's
                in the camera frustum              in the user's focus
                (a frame buffer)                   (a ConsciousAttentionFrame)
                     │                                    │
                     ▼                                    ▼
                next tick                          next tick
```

Each correspondence is load-bearing, not decorative:

- **Discrete time-stepped state evolution.** A game advances state in
  fixed-size ticks (16.7 ms at 60 fps). Legend advances state in
  fixed-shape ticks (one `tick()` call per discovery, ~200–300 ms at
  the §11.0 budget). Both treat time as a sequence of state
  transitions, not as a continuous flow.
- **The whole world updates; only the visible slice is rendered.**
  A game-engine tick re-runs physics on every entity, even the ones
  off-camera. The renderer then walks the camera frustum and
  produces a frame containing only what the player sees. Legend's
  tick re-runs decay, reinforcement, and supersession against the
  whole hypergraph, but only the attention-relevant subgraph is
  surfaced in the returned `ConsciousAttentionFrame`. The full
  state stays in the hypergraph the way the full game world stays
  in the engine; the frame is a *view*, not a snapshot of the world.
- **The camera is the user's attention.** In a game, what the player
  is looking at determines which pixels exist next frame. In Legend,
  what the input is *about* — its active regions, its frame scope,
  its focused relations — determines which subgraph the
  attention frame contains. Reinforcement is the analog of "draw
  these pixels brightly"; decay is "let off-screen state fade until
  it's needed again."
- **No query API; every interaction is a tick.** A game doesn't have
  a separate "ask the engine where the goblin is" RPC (Remote Procedure Call); you tick,
  and the goblin's position is in this frame's render. Legend
  doesn't have a separate query path; you tick, and the answer is
  in this frame's `focused_relations`. §12 spells this out at
  length.
- **Frame as caller-readable output.** A game frame is what the
  display actually paints; a Legend frame is what the calling LLM
  reads to know what's now in focus. Neither is the engine's
  authoritative state — both are derived views the next iteration
  can choose to re-derive differently.

This is why the public surface is named `tick`, the output struct is
named `ConsciousAttentionFrame`, and the §11 walkthrough is structured
as a fixed sequence of steps within one tick. Game-engine authors and
graphics programmers should recognize the architecture immediately;
the rest of this document is filling in what each engine subsystem
does during its slice of the tick.

```text
              Input (text + optional source + wall_clock)
                              │
                              ▼
        ┌────────────────────────────────────────────────┐
        │  STEP 0   WAL append                           │
        ├────────────────────────────────────────────────┤
        │            ─── READ-MOSTLY PHASE ───           │
        │            (&Hypergraph; parallelizable)       │
        │  STEP 1   detect_intent  ─► Intent        │
        │           (4-dim: conviction, prediction_error,│
        │            arousal, curiosity)               │
        │  STEP 2   adjust_policy  ─► Policy             │
        │  STEP 3   REMOVED in v0 — caller chunks long   │
        │           inputs; tick accepts ≤480 tokens     │
        │           (input embedding computed at tick    │
        │           entry; consumed by Steps 1 & 4)      │
        │  STEP 4   route_regions  ─► active_regions +   │
        │                              held RegionDelta  │
        │  STEP 5   run_extractors ─► proposals          │
        │           ★ GLiNER NER = the long pole         │
        │  STEP 6   coreference    ─► reuse decisions    │
        ├────────────────────────────────────────────────┤
        │            ─── MUTATION PHASE ───              │
        │            (&mut Hypergraph; sequential)       │
        │  STEP 7   apply_region_delta                   │
        │  STEP 8   build_relations + events             │
        │  STEP 9   supersession + cache                 │
        │  STEP 10  Hebbian + salience                   │
        │  STEP 11  focus-radius decay                   │
        │  STEP 12  aggregate_focus  ─►                  │
        └──────────────────┬─────────────────────────────┘
                              │
                              ▼
              ConsciousAttentionFrame
                              │
                              ▼
                  enqueue_replay (post-tick)
```

Steps 1–6 produce proposals against `&Hypergraph` (no commits).
Steps 7–12 apply all proposals in one mutation pass under `&mut`.
§4.3 details what each step extracts and where it pays off.

### 4.1 Input

An `Input` carries:

- `text: String` — the new information.
- `wall_clock: SystemTime` — for log entries only; never drives
  substrate logic.

`tick` takes an optional `source: Option<ElementId>` as a sibling
parameter (a pointer to a source element — Slack message id, file
path, URL, modeled as ordinary elements). When present, Step 8
attaches `(R, source, source)` meta-relations to relations born this
tick. Source is meta-relation-shaped — provenance about a claim,
not a property of the text — so it stays out of the `Input` struct
(§7.2 / §11.1).

Inputs are one stream. Discoveries arrive in tick order; tick order is
transaction time.

### 4.2 The Thirteen Steps

Each tick threads through 13 steps (0–12). Steps 1–6 are read-mostly
and parallelize where possible under `&Hypergraph`. Steps 7–12 are
sequential under `&mut Hypergraph`. Every tick — statement, question,
correction — runs the full pipeline; the `Intent` vector
modulates *policy*, never which steps run.

```
0.  log entry                  -> append (Tick, Input, ModelFingerprint) to WAL
                                  -- READ-MOSTLY PHASE BEGINS (&Hypergraph) --
1.  detect intent              -> Intent (conviction, prediction_error,
                                              arousal, curiosity)
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
inputs that produce no extractable elements or relations — there is no
input citizen to preserve).

### 4.3 Pre-Mutation Diagnosis: What We Learn Before Changing State

Every tick has a clean phase split. Steps 0–7 *diagnose* the input
without touching hypergraph state. Steps 7–12 *commit* what falls out
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
1     Intent (4-dim vector:               score how much this tick should change the substrate         Step 2 (sole consumer)
      conviction, prediction_error,            along DA / NE / cognitive axes; policy is computed
      arousal, curiosity)                    from this vector, not from a categorical label
2     adjusted Policy                          turn intent into the four knobs that govern this tick:      Steps 4, 8, 10, 11
                                               vigilance, plasticity, salience, default confidence         (every weighted op)
                                               (§10.6 table)
3     REMOVED in v0                            (was: spans / windowing). Caller chunks long inputs;        —
                                               one tick = one ≤480-token unit.
4     input embedding (tick-level)             dense semantic anchor for routing and salience;             Step 1 (intent feature
      (computed at tick entry)                 computed once at tick entry; threaded into Step 1 and       vector), Step 4 (route_
                                               Step 4. Inlined on Element.embedding for any element        regions query vector),
                                               written in Step 8.                                          Element.embedding
                                                                                                            (§7.3) on mint
5     active_regions + RegionDelta (held)      identify the conceptual locale (which DAG branches the      Step 5 (warm-attribute-
                                               input lives in); compute proposed DAG structural changes    name bias on label set),
                                               but DO NOT commit them yet                                  Step 7 (commit the
                                                                                                            held delta), Step 12
                                                                                                            (active_regions in frame)
6     element + relation proposals             turn diagnosed text into structural candidates              Step 8 (build base
      (with confidence per proposal)                                                                       relations and events),
                                                                                                            Step 9 (recognize
                                                                                                            from/to/property
                                                                                                            shape for supersession)
7     coreference decisions (reuse vs. mint)   reconcile each candidate with prior identity; decide        Step 8 (which
                                               whether "Dr. Rao" = existing DrRao element or a new one     ElementIds bind to
                                                                                                            each role), Step 10
                                                                                                            (reinforcement target —
                                                                                                            reused elements get
                                                                                                            their existing path
                                                                                                            strengthened)
```

Two things to notice:

1. **Steps 1–6 cannot be skipped.** Even on a question that mints
   nothing, we still classify intent (so reinforcement weight is
   right), run extractors (a question can introduce new entities),
   and score coreference (so "it" resolves correctly for the focus
   set). The full diagnosis runs every tick.
2. **Steps 1–6 do not commit.** `route_regions` returns a
   `RegionDelta` rather than applying it; extractor output is a
   `Vec<Proposal>`, not a write; coreference produces a decision
   table, not a merge. All of these are values held in tick-local
   state until Step 7 opens the mutation phase.

The parallelism story falls out of (2): because Steps 4–6 hold
`&Hypergraph` only, embedding + region routing + extractor calls run
under `rayon::par_iter` per span at no risk of conflict — roughly
5–7× tick speedup on a modern multicore CPU (Central Processing Unit) vs. fully sequential.
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

### 4.5 The Tick As Discovery

Each tick is a discovery — Legend's model of its world evolving as new
information arrives. The mechanics above (segment, embed, extract,
build, reinforce, decay) are how the discovery is processed. The
*meaning* is: Legend just learned something, possibly contradicting or
confirming what it knew, and its updated model is reflected in the
returned attention frame. Throughout this doc, "tick" names the
mechanical operation and "discovery" names what the operation is
doing semantically; the output type is `ConsciousAttentionFrame` in
both views.

### 4.6 Recap

- One hypergraph. Elements (richly-typed only through emergent
  structure read from indices) bound by Relations (typed hyperedges).
  Both decay and reinforce; both first-class substrate citizens.
- Vector hierarchy is a region DAG inside the same hypergraph.
- Recognition (concept, instance, event, frame) is read from derived
  indices over the relation graph (§8) — no kind tag on Element.
- Events are first-class; corrections supersede via PROV-O-style
  chains rather than overwriting.
- Every input is one call to `tick`, which threads through 14 steps
  (Steps 0–12: read-mostly diagnosis 1–6, mutation 7–12) and returns
  an attention frame; replay enqueue is the post-tick handoff.
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
14. **One input operation: `tick`. No separate query API and no
    separate memory store.** Retrieval is differential — path
    traversal with reinforcement, sharing the same code path as
    writes — not a parallel index alongside the substrate.
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
nodes; relations are the edges. Elements carry their own attributes
(names, stats); relations carry a flat list of typed attributes — each
binding a named slot to either an Element or another Relation (§7.2).
Replay's bulk rewrites (§14) are a known kind of structured graph
rewriting under this formalism, with results that do not depend on
rule application order when the rules are written correctly —
important for replay determinism. Note: unlike Wikidata or Cyc,
Legend does **not** type elements; only relations carry types, and
the type itself is just an attribute-name element like any other.
This is the Wolfram-Physics end of the design spectrum (untyped
primitive + typed connections).

**2. Predicates with named role-fillers** (Parsons 1990; Fillmore 1976;
Baker et al. 1998). The classical formalism `P(role₁ → t₁, role₂ → t₂,
…)` is a predicate applied to **named** arguments, not positional ones
— verbs have role slots like `agent`, `patient`, `instrument`, each
filled by a specific element. FrameNet (Berkeley, 1998) is the largest
catalogue of this shape: ~1,200 frames, each a predicate with its
expected role inventory. Legend collapses the predicate/role
distinction at the substrate level: both are attribute-name elements
in a relation's attribute list, and recognition treats them
uniformly. Fillers may be concrete (`Term::Element`) or nested
(`Term::Relation`); the latter is what makes meta-relations work —
any relation can take another relation as a filler, and that
recursion is unbounded by the substrate.

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
2002, *A New Kind of Science*; Wolfram Physics Project). Legend
treats the accumulated understanding of a project as computationally
irreducible and the consuming LLM as a bounded observer; the
substrate's forward-compression role (hypergraph as queryable running
state) and the v1 inward-compression mechanism (template relations
shortcutting recurring structure) follow from this framing. **Full
argument in §1.7** — not repeated here.

**8. Pearl's causal hierarchy** (Pearl 2009, *Causality*, 2nd ed.;
Pearl & Mackenzie 2018, *The Book of Why*). The three-rung distinction
between **association** (`P(Y|X)` — what we see together),
**intervention** (`P(Y|do(X))` — what happens if we act), and
**counterfactual** (`P(Y_x|x', y')` — what would have happened),
with structural causal models (SCMs) and do-calculus as the
machinery for moving between rungs. Legend applies the framework at
several points and is shaped to support the rest cheaply later:

- §11.2 trains intent classifiers with the lexical-feature vector
  acting as a **front-door mediator** that strips topic confounding
  from the embedding (linguistic surface form is causally upstream of
  the embedding); cross-class negatives perform confounder adjustment
  for the shared "first-person assertion shape"; Bradley-Terry pairs
  are same-topic / flipped-axis controlled experiments.
- §16.3's `intervened` behavioral modal distinguishes **rung-2
  evidence** (an agent acted) from default **rung-1 observation**
  (the world was seen to be that way). §11.10 supersession and
  §11.11 reinforcement read it because do() severs prior causes —
  intervention updates only the intervened claim; observation
  updates the claim *and* propagates back to its latent causes.
- §16.3's seeded **causal-relation attribute names** (`caused`,
  `correlated_with`, `enables`, `prevents`) let the relation graph
  carry the level of causal commitment a source actually claims —
  the precondition for not collapsing rung-1 co-occurrence into
  rung-2 causal structure during retrieval.
- §7.2's `derived_from` chain *is* the substrate's structural causal
  model — every relation that has one points back to the cause from
  which it was derived. Replay's cycle resolution and §11.11's
  topological-independence promotion gate (§14.8) walk this DAG
  directly. `antecedent_of` plays the same role for conditional
  rules.
- Full **counterfactual queries** (Pearl's abduction / action /
  prediction three-step) are deferred to v1+ (§24.9). The v0
  substrate carries everything needed (`non_actual` modal,
  `derived_from` DAG, the §11.13 frame-assembly pipeline) so the
  query path is a non-mutating projection over an existing
  hypergraph rather than a parallel mechanism.

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
    embedding: Vec<f32>,          // semantic anchor, populated at mint time
    polarity: Polarity,           // Signal (default) or Void — closed-class
                                  // stop-word seeds under VOID carry Void
                                  // so Step 5a's token pass can drop them.
                                  // Distinct from "routing-VOID" (a Step 4
                                  // unrouted_count signal, not an edge).
}

enum Polarity { Signal, Void }
```

That is the entire Element struct. No kind enum. No type tag.
Typed leaf values (named dates, weekdays, quantities, locations) are
themselves Elements; their typed semantics are parsed from `names` at
comparison time (§7.3).

**`names`** is the unified list of strings that refer to this element.
The seed pack gives some elements canonical names ("Change",
"change_event"); extractors add variant forms as inputs use them
("Doc Rao" appears alongside "Dr. Rao"). All names share the same
lifecycle; noisy or unused names decay.

**`embedding`** is the semantic anchor. Populated at mint time by
embedding `names` with the MiniLM embedder (§15.1). Seed-pack elements get
embeddings at boot from descriptors or canonical names; extractors
embed each newly-minted element from its surface form (or the
originating span text for anonymous NER spans, held as a variant
name). Region routing (§10.2), similarity search, attribute-name
dedup (§11.7), coreference scoring (§11.8), and salience computation
all read it. FP32.

**`stats: MemoryStats`** governs decay, reinforcement, salience.
Elements and Relations share the same stats struct — memory dynamics
are uniform.

```rust
struct MemoryStats {
    activation: f32,             // current tick's activation level
    confidence: f32,             // belief strength
    plasticity: f32,             // long-term durability scalar — high =
                                 // formative phase (easy to update,
                                 // fast to decay); low = settled (hard
                                 // to overwrite, slow to decay)
    salience: f32,               // accumulated amygdala-style protection
    access_count: u32,
    focus_success_count: u32,
    support_count: u32,          // independent ticks supporting this; drives Defeasible→Asserted (§11.11)
    support_diversity: u32,      // distinct evidence-source dimensions seen; pairs with support_count
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
    attributes: Vec<Attribute>,        // typically 2–5 entries
    status: RelationStatus,
    stats: MemoryStats,
    priority: i8,
    created_at: Tick,
}

struct Attribute {
    name: ElementId,                   // attribute-name element (e.g. instance_of,
                                       //   target, from, to, frame, supersedes, ...)
    value: Term,
}

enum Term {
    Element(ElementId),                // concrete filler — including value-Elements
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

Six fields on Relation. **A Relation is a flat list of named
attributes.** No privileged predicate slot — every named property of
the relation is just an `Attribute { name, value }`. The
attribute-name element identifies what kind of slot this is; the
value is either a concrete Element or another Relation (which is
how meta-relations and nested claims work).

**Why Vec, not HashMap.** Typical relations have 2–5 attributes. At
that size a linear scan beats a HashMap lookup (constant factors —
hash + bucket probe + indirection > 5 u32 comparisons that fit in
one cache line). Vec also preserves insertion order, which the
§21 Step 11 replay-determinism fixture needs for bit-identical
state across passes. If profiling later shows the scan dominates,
the upgrade is `smallvec` / `tinyvec` (stack-inlined for small N),
not HashMap.

**Meta-relations.** Anything that scopes, provenances, modalizes,
supersedes, or otherwise contextualizes a Relation is itself a
Relation whose attributes include a `target` (or otherwise-named
slot) pointing at the modified relation via `Term::Relation`:

| What it carries | Form |
|---|---|
| Frame (contextual scope) | attributes: `[frame: F, target: R]` |
| Valid-time start | `[valid_from: T1, target: R]` |
| Valid-time end | `[valid_to: T2, target: R]` |
| External source pointer | `[source: S, target: R]` |
| Negation | `[negated: <surface-form>, target: R]` — polarity-flipped claim |
| Uncertainty | `[uncertain: <degree-or-surface-form>, target: R]` — confidence-reducing modality |
| Non-actual | `[non_actual: <kind>, target: R]` — claim is not about actual world state (counterfactual, desired, obligatory) |
| General | `[general: <kind>, target: R]` — habitual / generic / universal claim that resists supersession by specific instances |
| Intervention | `[intervened: <agent-or-surface>, target: R]` — agent-action evidence (Pearl rung 2, §6 (8)); do() severs prior causes for supersession (§11.10) and reinforcement updates only the intervened claim, not its latent causes (§11.11). Absence = default observation. |
| Supersession backward link | `[supersedes: R_old, target: R_new]` |
| Lineage | `[derived_from: X, target: R]` where X is an Element or Relation — Legend's structural causal model is the DAG these meta-relations form (§6 (8)) |
| Conditional antecedent | `[antecedent_of: R', target: R]` — captures rule shape "R holds if R' holds"; substrate hook for v1 forward-chaining (§24.6) and counterfactual queries (§24.9) |

Each meta-relation is just a Relation. It has its own status, stats,
priority, decay, and can itself carry meta-relations (a frame-scoping
fact can have its own valid-time, source, etc.) — the same recursion
that `Term::Relation(RelationId)` always supported. There is no
substrate-level annotation layer; there are only relations.

**Worked example.** A claim `R = (DrRao, has_role, dentist)` scoped
to the user's frame, sourced from a Slack message:

```rust
// The world claim itself. Binary triples bind the head Element under
// a participant attribute. `subject` is the conventional generic head
// — a seeded attribute name (§16.3) extractors reach for when no
// frame-specific participant slot fits. It carries no structural
// privilege: no `relations_by_subject` index, no recognition rule
// reads it.
R = Relation {
    id: 42,
    attributes: [
        Attribute { name: SUBJECT,  value: Term::Element(DrRao) },
        Attribute { name: HAS_ROLE, value: Term::Element(dentist) },
    ],
    status: Asserted,
    stats: MemoryStats { ... },
    priority: 0,
    created_at: 100,
};

// Frame meta-relation: R is in FRAME_USER.
M_frame = Relation {
    id: 43,
    attributes: [
        Attribute { name: FRAME,  value: Term::Element(FRAME_USER) },
        Attribute { name: TARGET, value: Term::Relation(42) },  // ← R
    ],
    status: Entailed,
    ...
};

// Source meta-relation: R came from slack_msg_4127.
M_source = Relation {
    id: 44,
    attributes: [
        Attribute { name: SOURCE, value: Term::Element(slack_msg_4127) },
        Attribute { name: TARGET, value: Term::Relation(42) },  // ← R
    ],
    status: Entailed,
    ...
};
```

`M_frame` and `M_source` are ordinary Relations whose `target`
attribute holds `Term::Relation(42)`. They land in
`meta_relations_by_subject[42]` on Step 8 commit, so retrieval
finds them with one HashMap lookup followed by a tiny filter on
attribute name (typical relations have 0–3 meta-relations). Reflective
reasoning ("what is the modality of M_frame itself?") walks the same
index keyed at the meta-relation's own id (§3.2's "v0 reads depth-1
only" callout).

**Hot-path access is via two derived indices** (§9.2):

```rust
// All meta-relations on R (R appears as the value of some
// target-shaped attribute).
meta_relations_by_subject: HashMap<RelationId, Vec<RelationId>>

// All meta-relations whose attribute value is R (R appears as the
// value of an attribute named `supersedes` / `derived_from` / etc.).
// Used for inverse walks: "what supersedes R?" / "what is derived
// from R?"
meta_relations_by_object:  HashMap<RelationId, Vec<RelationId>>
```

Specific lookups become small filters over the index entry:

```rust
fn frame_of(hg: &Hypergraph, r: RelationId) -> Option<ElementId> {
    hg.meta_relations_by_subject.get(&r)?.iter()
        .map(|&m| &hg.relations[m as usize])
        .find_map(|m| {
            // Only returns Some if this meta-relation has a `frame` attribute
            m.attributes.iter()
                .find(|a| a.name == FRAME)
                .and_then(|a| match a.value {
                    Term::Element(e) => Some(e),
                    _ => None,
                })
        })
}
```

These indices are **derived state**, rebuilt on load and updated
incrementally during Steps 8–9. The relation graph is the source of
truth. Reading "what's the frame of R?" is one HashMap lookup plus
a 0–3-element scan — effectively O(1).

**What makes an Element an attribute name.** Nothing structural — there
is no `is_attribute_name` flag. An Element is *functioning as* an
attribute name exactly when it appears in the `name` field of some
relation's attribute list (§3.4). Identity is established by names and
by incoming `subclass_of` / `instance_of` relations that place it in a
broader category. New attribute names enter the system either via the
seed pack (§16) or via Step 5 extractor proposals; minting policy and
label-set resolution are specified in §11.7.

**`instance_of` vs. `has_role` convention.** `instance_of` is reserved
for **ontological kind** — what something fundamentally *is* (e.g. an
attribute `instance_of: person` means the head is a person). `has_role`
carries **situational role** — a function the element plays in a frame
(`has_role: dentist` in the dental-appointment frame). Both can hold
simultaneously on the same element with no contradiction; the same
person can have multiple roles across frames without any of them
changing what the person *is*. Extractors and seed schemas must
respect this split: NER emits `instance_of`; situational attribute
names (`has_role`, `provider`) come from frame- and event-shaped
extraction. The emergence rules in §3.4 read `instance_of` for
concept/instance recognition; they do not read `has_role`.

**Status** — `RelationStatus` is mechanical and is allowed to drive
control flow. `Asserted` outranks `Defeasible` regardless of priority;
priority breaks ties between same-status relations.

**Supersession.** Forward walk ("what does R supersede?") reads
`meta_relations_by_subject[R]` filtered for an attribute named
`supersedes`; inverse walk ("what supersedes R?") reads
`meta_relations_by_object[R]` with the same filter.

**Lineage** is the `derived_from` attribute on a meta-relation —
`prov:wasDerivedFrom`. Present for cache relations (current-state
derivations) and for relations derived from another relation (the
value is `Term::Relation(parent_id)`); absent for asserted base
relations. Required by Invariant 9.

**Defeasible priority** — `priority: i8` follows Antoniou's defeasible
logic with dynamic priorities (2002). Stored as a field because tie-
breaking happens on every comparison and must be branchless.

**Belief revision via supersession** is the **Levi identity** —
contraction-of-negation followed by expansion (Alchourrón-Gärdenfors-
Makinson 1985). Legend's correction protocol is base belief revision
(Hansson 1999) made operational.

A Relation can represent binary triples, n-ary events, nested
relations, conditional relations, time-scoping, modality, and
uncertainty — all in one structure, all expressed as attribute lists.

### 7.3 Typed Values Are Elements

The substrate has no `values` payload table. A "typed value" — a
date, a quantity, a location, a weekday — is an ordinary Element.
The surface forms ("Tuesday", "Tues", "tue"; "6 pounds", "6lbs",
"six pounds"; "2026-04-30", "April 30, 2026") are *names* on that
Element; coreference (§14.3) collapses the surface variants onto a
single referent the same way it does for "Dr. Rao" and "Doc Rao".
Relations target value-Elements via `Term::Element(ElementId)` like
any other element.

**Typed semantics are computed at comparison time, not stored.**
When a behavior needs ordered or arithmetic semantics — sorting two
dates, deciding whether a `valid_from`/`valid_to` interval overlaps
the active temporal frame, comparing "6 pounds" against "5 pounds" —
it parses the relevant names on demand:

- Dates and weekdays parse via `chrono` + `chrono-english` (§15.1).
- Quantities parse via the v0 number/unit parser (§15.1 temporal
  parser slot, generalized).
- Locations parse via the v0 geo parser.

Parsing a handful of names per comparison is cheap — chrono is
sub-microsecond per call, and the comparison sites in v0
(supersession's `from`/`to` lookup, frame-relative valid-time
filtering, decay's "exact value" check) each touch a small fixed
number of values per tick. v0 carries no parsed-value cache; the
substrate stays at exactly two primitives.

**Worked example.** *"My dentist appointment moved from Tuesday to
Friday."* mints two value-Elements (`Tuesday`, `Friday`) — both
ordinary Elements with the surface form as a name and an
`(E, instance_of, weekday)` relation pinning the kind. The supersession
chain in §11.10 walks `(R, from, Tuesday)` → `(R, to, Friday)`;
when downstream code asks "is Friday after Tuesday?" it reads the
two Elements' names, parses them as `Weekday::Tue` / `Weekday::Fri`,
and compares. No third storage shape is consulted.

**Booleans, probabilities, and opaque vectors are not values.** They
look like value-shaped data but the substrate already carries each
through other machinery:

- *Boolean / negation* — `[negated: <surface-form>, target: R]` (§7.2).
- *Probability* — `MemoryStats.confidence` on relations (§7.1).
- *Opaque vector* — `Element.embedding` (§7.1).

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
- **No kind discriminator.** §1.6 covers the design bet; §8 covers
  how kinds are observed.

---

## 8. Recognition Indices

Recognition is the load-bearing answer to §1.6's bet that ontology
emerges. Without a kind tag on Element, every behavior that wants to
treat "concepts" or "events" or "frames" specially has to *recognize*
that role from the relation graph — and recognition has to be cheap
enough to run on the hot path. This section is how that works.

The mechanism: read derived indices that summarize each element's
relation neighborhood, then condition behavior on count thresholds
from `Policy`. Recognition is index lookup + comparison, not a
separate "is this a concept?" function. The indices are the
**operational realization of emergent ontology**: kinds *are* the
patterns the indices report, and behavior is conditioned on those
patterns directly.

**Why this is short.** Earlier drafts enumerated six "emergent kinds"
(concept, instance, event, frame, schema, rule), each with its own
structural rule and recognition path. That collapsed: there is no
stored kind, no recognition function. There is a small set of indices
and a few thresholds the pipeline reads. New behaviors are added by
maintaining a new index, not by adding a new kind. This is what makes
§1.6's no-pre-declared-categories bet operationally cheap rather than
expensive.

### 8.1 The Indices

Per-element derived state, rebuilt on load and updated incrementally
during the mutation phase (Steps 8–9: `build_relations` writes the
relation, `apply_supersession_and_cache` updates the indices):

- `attribute_value_counts: HashMap<ElementId, HashMap<ElementId, u32>>`
  — for element `E`, counts of relations where E is the value of an
  attribute, grouped by attribute name. Concept-recognition reads
  `[E][instance_of]` (many relations claim something is_a E); reference-
  frame recognition reads `[E][frame]` (many relations are scoped to E
  as their frame); any "how many relations bind E to attribute name N"
  query reads here.

- `attribute_co_counts: HashMap<ElementId, HashMap<ElementId, u32>>` —
  co-occurrence counts: relations that mention E (as the value of any
  attribute) *and* also carry an attribute named N. Instance-recognition
  reads `[E][instance_of]` — non-zero means E participates in at least
  one is-a claim, so E is itself an instance of something. There is no
  privileged subject/object distinction; co-occurrence is the
  signal.

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
  `attribute_value_counts[E][instance_of]` and treats counts at or
  above `policy.concept_recognition_threshold` (default 3) as
  concept-like (broad reuse); counts below that with non-zero
  `attribute_co_counts[E][instance_of]` are instance-like (pattern
  separation).
- **Supersession trigger** (§11.10) reads
  `meta_relation_presence[E]` for valid-time-bounded participant-
  attribute shape.
- **Frame-relative scoping** reads
  `meta_relations_by_subject[R]` (filter attribute name=`frame`) and
  `attribute_value_counts[E][frame]`; elements at or above
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
   (e.g. "plan-shaped elements" defined by
   `attribute_co_counts[E][step_n]`) plug into the same index types;
   no new tables, no new code paths.

---

## 9. Core Data Model

Concrete substrate types and the Hypergraph struct. This is the spec
the coder works against first, before any pipeline code, before any
NLP (Natural Language Processing). The substrate must serialize round-
trip and the inspection harness (§21) must dump it before anything
else is written.

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
- Hot scalar fields (`activation`, `plasticity`, `salience`) are
  candidates for split-out into parallel `Vec<f32>` arrays if
  profiling shows the wide `MemoryStats` struct hurts cache. Decide
  on first profile, not
  earlier.

### 9.2 The Hypergraph Struct

```rust
struct Hypergraph {
    // Two storage primitives — that is the entire substrate. Each
    // Element carries an inline Vec<f32> embedding (§7.1); region
    // topology lives as ordinary relations between elements (§10);
    // typed values are Elements whose names parse on comparison
    // (§7.3 — there is no values table).
    elements: Vec<Element>,
    relations: Vec<Relation>,

    // Tick clock — monotonic, incremented once per tick.
    clock: Tick,

    // Current policy (vigilance, plasticity, decay, thresholds).
    policy: Policy,

    // Working memory — recent focused entries. Used by coreference
    // (pronoun + attribute-tagged context lookup) and Hebbian
    // co-activation. Bounded by `policy.recent_focus_capacity`
    // (default 64).
    recent_focus: VecDeque<RecentFocusEntry>,

    // Derived indices — rebuild on load, never serialize.
    //
    // v0 implementation status: only the indices that Steps 0–5 need
    // are live today (`by_name`, `region_children`, `region_parents`,
    // `region_prototypes`, plus a new `region_stats` carrying
    // per-region mean+var of prototype embeddings for Mahalanobis
    // routing). The meta-relation / recognition / lateral indices
    // below land alongside the steps that consume them (Steps 6–12);
    // until then the relation graph itself is consulted directly.
    by_name:                     HashMap<String, Vec<ElementId>>,
    // All relations that mention E as the value of any attribute. The
    // primary "what touches E?" lookup. No privileged head slot — the
    // `subject` attribute name (§16.3) exists as a generic head
    // convention but is not indexed separately; callers that want
    // "relations where E is subject" filter on the returned list.
    relations_by_element:        HashMap<ElementId, Vec<RelationId>>,
    // Relations that have at least one attribute named N.
    relations_by_attribute_name: HashMap<ElementId, Vec<RelationId>>,

    // Region indices — derived from the region structural relations
    // (member_of, parent_region, lateral_region, prototype). Hot-path
    // routing (§10.2, §11.6) reads these instead of walking the
    // relation graph.
    //   region_members[R]    : elements with (E, member_of, R)
    //   region_parents[R]    : (parent, weight) for (R, parent_region, parent)
    //                          where weight = parent_region relation's stats.confidence
    //   region_children[R]   : children C with (C, parent_region, R)
    //   region_lateral[R]    : siblings reachable via (R, lateral_region, _)
    //                          and (_, lateral_region, R)
    //   region_prototypes[R] : prototype elements P with (R, prototype, P)
    region_members:         HashMap<ElementId, Vec<ElementId>>,
    region_parents:         HashMap<ElementId, Vec<(ElementId, f32)>>,
    region_children:        HashMap<ElementId, Vec<ElementId>>,
    region_lateral:         HashMap<ElementId, Vec<ElementId>>,
    region_prototypes:      HashMap<ElementId, Vec<ElementId>>,

    // Meta-relation indices — two inverses that together answer any
    // depth-1 meta-relation question via a small filter on the
    // returned list (typical relation has 0–3 meta-relations).
    //   meta_relations_by_subject[R] : meta-relations where R is the
    //                                  subject (R, _, _) — drives
    //                                  forward walks like "what's
    //                                  R's frame?", "what does R
    //                                  supersede?", "valid_from of R?"
    //   meta_relations_by_object[R]  : meta-relations where R is the
    //                                  object (_, _, R) — drives
    //                                  inverse walks like "what
    //                                  supersedes R?" or "what is
    //                                  derived from R?"
    meta_relations_by_subject: HashMap<RelationId, Vec<RelationId>>,
    meta_relations_by_object:  HashMap<RelationId, Vec<RelationId>>,

    // Recognition indices (§8) — derived attribute-name counts.
    // Reading these is how we tell concept from instance from event
    // without a kind enum, and without privileging any attribute name
    // structurally.
    //   attribute_value_counts[E][N] : count of relations where E is
    //                                  the value of an attribute named
    //                                  N. Drives concept and frame
    //                                  recognition (high count for
    //                                  N=instance_of → concept; high
    //                                  count for N=frame → reference
    //                                  frame).
    //   attribute_co_counts[E][N]    : count of relations that mention
    //                                  E (in any attribute) *and* also
    //                                  carry an attribute named N.
    //                                  Drives instance recognition
    //                                  (non-zero for N=instance_of → E
    //                                  participates in some
    //                                  is-a claim, i.e. is an instance
    //                                  of something).
    attribute_value_counts: HashMap<ElementId, HashMap<ElementId, u32>>,
    attribute_co_counts:    HashMap<ElementId, HashMap<ElementId, u32>>,
    meta_relation_presence: HashMap<RelationId, HashSet<ElementId>>,
}
```

**Why no payload tables at all.** `Element.embedding` is inlined so
hot-path access is one struct read instead of a HashMap lookup.
Region structure is relational — membership, parenthood,
prototype-of, lateral are claims about the graph, not dense
per-node data — so folding it into the relation primitive removes a
whole storage shape without giving up hot-path performance (the
region indices above are derived the same way the meta-relation
indices are). And typed values ("Tuesday", "6 pounds", "Berlin")
are Elements whose surface forms live in `names`; parsed semantics
(date ordering, unit conversion, geo) are computed at comparison
sites on demand. The result is exactly two storage shapes: elements
and relations. Future versions may revisit (parallel `Vec<f32>`
arrays for SIMD scans, or a typed-comparison cache) once profiling
justifies the cost; v0's job is to prove the two-primitive design
works without them.

### 9.3 Policy

Per-tick modulators set by PFC (Prefrontal Cortex — see §13.5's
`adjust_policy`):

```rust
struct Policy {
    // Region routing
    descend_threshold: f32,
    leaf_vigilance: f32,
    merge_threshold: f32,
    split_variance: f32,
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

    // Defeasible → Asserted promotion gate (§11.11). All three must
    // hold; no auto-promotion on repetition alone.
    // - promotion_min_count: minimum support_count. Default: 3.
    // - promotion_min_diversity: minimum distinct evidence sources
    //   (source elements, intents, or frames). Default: 2.
    // - promotion_window_ticks: window in which support is counted.
    //   Default: 1000.
    promotion_min_count: u32,
    promotion_min_diversity: u32,
    promotion_window_ticks: u32,

    // Working-memory ring capacity (§9.2 recent_focus). Default: 64.
    recent_focus_capacity: u32,

    // Region routing thresholds (§10.2, §10.3.5).
    // - region_activation_threshold: minimum cosine for a region to be
    //   considered "active" on this tick. Used by the diffuse-routing
    //   fallback in §10.3.5 to decide whether candidate filtering
    //   constrains. Default: 0.55.
    region_activation_threshold: f32,

    // NER auto-emit threshold (§11.7). Confidence at or above is
    // emitted as Entailed; below as Defeasible. Default: 0.7.
    ner_assertion_threshold: f32,

    // Replay safety floor (§14.8). Relations whose
    // focus_success_count exceeds this floor cannot be retracted by
    // replay compression. Default: 3.
    replay_focus_floor: u32,

    // Attribute-name dedup (entity collapse threshold for attribute-
    // name elements — what was previously called "predicate dedup").
    // Universal cosine search at mint time (§11.7); hits at or above
    // this threshold reuse instead of mint. Default: 0.85.
    attribute_name_dedup_threshold: f32,

    // Mint-rate observability (§11.7). If a single tick mints more than
    // this many new attribute-name elements, the inspection harness
    // logs the tick and replay priority-bumps dedup for it. Not a hard
    // cap. Default: 5.
    attribute_name_mint_warning_count: u32,

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

- **Read-mostly parallel phase** (Steps 4–6): `&Hypergraph` shared
  across `rayon::par_iter` workers. No interior mutability. No
  locking.
- **Mutation phase** (Steps 7–12): single `&mut Hypergraph` owned by
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

struct Input {
    text: String,
    wall_clock: SystemTime,
}

/// Echo of the input that produced a frame — read-only, not durable
/// (§11.13). Carries the question/statement back to the caller
/// alongside the focused subgraph; not a substrate citizen.
struct InputEcho {
    text: String,
}

/// Working-memory entry. Beyond the bare ElementId, carries the
/// attribute name under which the element was bound and the frame
/// it was focused in on its most recent tick — so coreference
/// (§11.8) can filter "most-recently-focused element with dentist
/// context" rather than just "most recent element."
struct RecentFocusEntry {
    element: ElementId,
    attribute: Option<ElementId>, // attribute name binding this
                                  // element in the focused relation
                                  // (e.g. SUBJECT, ACTOR, TARGET)
    frame: Option<ElementId>,     // active_frame at time of focus
    tick: Tick,
}

/// Frame-assembly types used in §11.13 ConsciousAttentionFrame.
struct RegionActivation {
    region: ElementId,
    similarity: f32,
}

struct RelationActivation {
    relation: RelationId,
    score: f32,                   // RRF (Reciprocal Rank Fusion)-fused score (§11.13)
    is_defeasible: bool,          // status filter at frame-assembly time
}

enum UncertaintySignal {
    LowConfidence(RelationId),
    Contradiction { a: RelationId, b: RelationId },
    AmbiguousCoref { span: String, candidates: Vec<ElementId> },
    UngroundedTime(RelationId),
    DiffuseRouting,               // §10.3.5 diffuse-routing fallback
}

enum AttentionAction {
    EnqueueReplay { kind: ReplayJob },
    FollowUpQuery(String),
}

const MAX_ATTRIBUTES_PER_RELATION: usize = 8;

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

The vector subgraph that lives inside the hypergraph. **A region is an
ordinary Element** whose role is established by the relation
`(R, instance_of, REGION_CLASS)` — the same recognition pattern §3.4
uses for concepts and frames. Region topology, membership, and
prototype attachment are all expressed as relations between elements.

### 10.1 Topology

Regions form a **weighted directed acyclic graph** rooted at `Genesis`
with a `Void` sink for sub-threshold inputs. The topology is
expressed via four seeded attribute names:

```text
(E,    member_of,       R)        // E lives inside region R
(R_a,  parent_region,   R_b)      // R_a's parent is R_b; weight in
                                  //   the relation's stats.confidence
(R_a,  lateral_region,  R_b)      // sibling-shortcut between regions
(R,    prototype,       P)        // P is one of R's prototype elements
```

- Every region has 1–8 prototype elements (§10.4); each prototype is
  itself an Element with an inline embedding.
- Multi-parent attachment is allowed (this is what makes the topology
  a DAG rather than a tree). Parent edges are weighted via the
  `parent_region` relation's `stats.confidence`; the same child can
  attach to multiple parents at different strengths.
- Lateral edges may connect sibling regions for fast-pivot retrieval.

Hot-path routing (§10.2, §11.6) reads the derived
`region_parents` / `region_children` / `region_lateral` /
`region_prototypes` / `region_members` indices populated from these
relations during Step 7 commit; reflective traversal can walk the
relations directly.

The DAG topology is what makes Legend handle polysemy — `Tuesday` in
the `user_schedule:current` region is reachable from multiple parent
regions (`weekday`, `appointment_slot`).

**No region-level scalars.** A region's "metadata" — radius, density,
variance, utility, noise — is either derivable from its prototype
embeddings + members or already lives on the region Element's
`MemoryStats`. Vigilance is a per-tick `Policy.leaf_vigilance` set
by intent (§10.6), not a per-region constant. The substrate carries
exactly what the relation graph and `MemoryStats` carry.

### 10.2 Region Routing (Read-Only) + Application (Mutation)

Region routing happens in the **read-mostly parallel phase** of the
tick (Step 4). The algorithm walks the DAG from Genesis via the
`region_children` and `region_prototypes` indices, considering the
top-k children at each node by cosine similarity to the candidate
node's prototype Elements.

```rust
fn route_regions(
    embeddings: &[Vec<f32>],
    hg: &Hypergraph,
    p: &Policy,
) -> (Vec<ActiveRegion>, RegionDelta);
```

Outputs:

- a list of active regions per embedding (with similarity scores);
- a `RegionDelta` describing the proposed structural changes (parent
  attachments, prototype embedding updates, newly minted regions).

```rust
struct RegionDelta {
    // (child, parent, weight) — committed as new
    // (child, parent_region, parent) relations with stats.confidence = weight,
    // or as confidence updates on existing parent_region relations.
    parent_attachments: Vec<(ElementId, ElementId, f32)>,

    // (prototype_element, new_embedding) — committed by overwriting
    // the prototype Element's inline embedding via the spherical
    // k-means update rule (§10.5).
    prototype_updates: Vec<(ElementId, Vec<f32>)>,

    // Minted: a new region Element plus its initial prototype Element,
    // plus the seed structural relations
    //   (new_region, instance_of, REGION_CLASS)
    //   (new_region, parent_region, parent)
    //   (new_region, prototype, new_prototype).
    new_regions: Vec<NewRegion>,

    // Member attachments: (member, region) — committed as
    // (member, member_of, region) relations.
    new_members: Vec<(ElementId, ElementId)>,

    void_count: u32,
}

struct NewRegion {
    parent: ElementId,
    initial_prototype: Vec<f32>,    // becomes the inline embedding of
                                    // the minted prototype Element
}
```

`RegionDelta` is held until the mutation phase, where
`apply_region_delta` (Step 7) commits the relations and applies
spherical k-means prototype embedding updates.

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
`(node, parent_region, R)` relation is written `Defeasible`
regardless of whether sentence-level routing was crisp or diffuse.
The DAG benefits from the refined topology immediately for routing —
the `region_children` / `region_parents` indices are populated from
`Asserted`, `Entailed`, *and* `Defeasible` `parent_region` relations,
so Defeasible-parent regions are routable on the next tick. The
*claim* that the new node belongs there is what stays provisional
until replay confirms.

Replay (§14.8) accumulates evidence across ticks and resolves each
provisional insertion to one of three outcomes:

- **Confirm.** Cosine gap between node-to-current-child and
  node-to-current-parent ≥ `policy.midpath_confirm_gap`, and the
  node has been routed-against in ≥ `policy.midpath_confirm_evidence`
  ticks without contradiction. The `(node, parent_region, R)`
  relation flips from `Defeasible` to `Asserted`.
- **Re-parent (cross-subtree allowed).** When a node's cosine to a
  parent in a different subtree exceeds its current parent by
  ≥ `policy.midpath_reparent_gap`, replay moves it. Wider gap than
  `confirm_gap` to avoid flapping. The old parent_region relation
  flips to `Superseded`; a new `(node, parent_region, new_parent)`
  relation is written, and `(R_new, supersedes, R_old)` records the
  lineage. This is the recovery path for wrong-subtree placements
  that came out of weak or wrong sentence-level routing on the
  introducing tick.
- **Retract.** A Defeasible parent_region relation that fails to
  accumulate evidence within the window flips to `Retracted`; the
  node's children re-parent to the original (pre-insertion) parent
  via new `parent_region` relations.

The stability gate is essential at MiniLM's 384 dimensions, where
adjacent cosine differences of 0.02–0.05 are routinely within
embedding noise. `confirm_gap` (default 0.05) and the multi-tick
evidence requirement keep replay from churning provisional nodes in
and out across passes on noise-driven signals.

**Anaphoric spans are not DAG-insertion candidates.** Spans like "it",
"this approach", "the pattern" must resolve to an existing element via
the coref cascade (§11.8), not become new DAG nodes. Enforcing this is
the extractor stack's job at §11.7 (GLiNER (Generalist Lightweight
Named Entity Recognizer) + lexicon should not propose anaphoric/deictic
spans as entity candidates).

### 10.4 Multi-Prototype

Each region carries one or more **prototype Elements**, attached by
`(R, prototype, P)` relations. The set is kept small by
`policy.merge_threshold` (collapses near-duplicates) and
`policy.split_variance` (splits high-scatter regions); no hard cap.
Each prototype Element holds its own inline embedding;
this is the storage shape that replaces the old per-prototype
`(vector, weight, support_count)` payload — `weight` and
`support_count` live in the prototype Element's `MemoryStats`, the
vector lives inline. Reasons:

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
  merging. Mitigation: per-intent vigilance from the policy table
  below.
- **Prototype dimension collapse** (e.g. e5-base-v2 under
  quantization). Mitigation: smoke-test against a held-out set of seed
  prototypes; require ≤ 2% recall@10 drop after any quantization
  change.

**Intent policy modulators (the canonical "what does intent
change" mapping — referenced from §11.2 and §11.3).** Intent is a
4-dimensional weight vector (`Intent` in §11.2), not a
categorical label. Each dimension projects onto specific `Policy`
knobs; the full mapping (all coefficients are v0 starting points;
calibrate against §19 + §20.5 after Step 8):

```text
dimension          neuro analog       knobs it modulates
─────────────────────────────────────────────────────────────────
conviction         (cognitive)        default_conf, leaf_vigilance
prediction_error   dopamine (DA)      salience_multiplier,
                                      leaf_vigilance,
                                      supersession_threshold
arousal            norepinephrine     salience_multiplier,
                                      hebbian_rate
curiosity        (Legend-specific)  default_conf (reduces),
                                      hebbian_rate (reduces)
```

The full policy formulas:

```text
default_conf       = base_conf
                   * conviction
                   * (1.0 - 0.7 * curiosity)

salience_multiplier = base_salience
                    + 1.0 * arousal
                    + 1.0 * prediction_error

leaf_vigilance     = base_vigilance
                   + 0.20 * prediction_error
                   + 0.20 * conviction

hebbian_rate       = base_rate
                   * (1.0 - 0.5 * curiosity)
                   * (1.0 + 0.3 * arousal)

supersession_threshold = base_threshold * (1.0 - prediction_error)
```

**Why this shape.** Each coefficient corresponds to a specific
neuroscience finding:

- **`prediction_error → salience_multiplier`** — DA-driven
  encoding boost on novelty / contradiction (Lisman & Grace 2005;
  Wittmann et al. 2011). Surprising inputs land harder.
- **`arousal → salience_multiplier`** — NE-driven encoding boost
  on emotionally important content (LaBar & Cabeza 2006). Both
  extremes (positive and negative valence) protect the memory.
- **`prediction_error → supersession_threshold`** — high
  prediction-error inputs are precisely the ones where prior
  beliefs need to be revisited; low prediction-error inputs leave
  the cache alone (no DA spike, no supersession lookup).
- **`conviction × (1 - curiosity) → default_conf`** —
  separates "speaker certainty" from "speaker is asking." A
  high-conviction question still writes new content low-confidence
  because the speaker isn't asserting it; a low-conviction
  statement also writes low-confidence because the speaker is
  hedging.
- **`prediction_error + conviction → leaf_vigilance`** — both
  contradiction and confident assertion warrant tighter routing
  (don't blur entities during corrections; don't blur entities
  during identity claims). Brainstorming (low both) loosens
  routing so neighboring concepts cross-pollinate.
- **`(1 - curiosity) × (1 + arousal) → hebbian_rate`** —
  questions traverse paths but reinforce them more lightly than
  statements (arousal still amplifies the effect when present).

**Recovering the named-intent labels (optional summary view).**
Cluster regions of the 4-vector space carry useful names — the
old categorical enum collapses to derived labels:

```text
"Statement"        ≈ moderate conviction, low prediction_error,
                     low arousal, low curiosity
"Question"         ≈ curiosity > 0.6
"Correction"       ≈ prediction_error > 0.7 + conviction > 0.7
"Identity"         ≈ conviction > 0.8 (+ entity-density signal)
"TemporalUpdate"   ≈ prediction_error > 0.5 + temporal extraction
                     activity in Step 5
"Brainstorming"    ≈ conviction < 0.3 + curiosity < 0.5
```

These aren't computed by the pipeline — they're how a debugger or
the inspection harness summarizes a tick's `Intent` for a
human reader. Policy is computed from the vector directly.

---

## 11. The Tick Pipeline

This section specifies the 14 steps (0–13) `tick` runs through. §4
covered the conceptual shape; this section is the typed spec. Every
tick runs every step regardless of intent; the `Intent`
vector modulates `Policy` (§11.2 / §10.6), not which steps execute.

### 11.0 Per-Step Latency Budget

v0 budget table on commodity CPU (4-core; both the MiniLM embedder
and the Step 5 GLiNER NER encoder — DeBERTa-v3-small — run through
the in-house INT8 BERT engine in `src/inference/`, AVX-VNNI when
available, AVX2/scalar fallback otherwise). Numbers are p50
(50th-percentile) targets; p95 (95th-percentile) typically runs
1.5–2× p50 driven by NER variance.

```text
step  name                              p50 budget    notes
0     log entry (WAL append)            <1 ms         LZ4 hot segment append
1     detect_intent                     <1 ms         4 logistic classifiers over the precomputed
                                                      embedding + lexical features; sub-ms once the
                                                      shared embedding is available
2     adjust_policy                     <1 ms         scalar copy + multiplier apply
3     REMOVED in v0                     —             caller chunks long inputs (§11.4)
                                                      (input embedding computed at tick entry,
                                                      ~1.7–2 ms on AVX-VNNI; consumed by Steps 1 & 4)
4     route_regions                     5–15 ms       DAG descent over per-region prototypes
                                                      (mean-of-top-K cosine + diagonal Mahalanobis;
                                                      no model call)
5     run_extractors                    50–150 ms     ★ Step 5b GLiNER NER (DeBERTa-v3-small INT8
                                                      on the in-house BERT engine) — the long pole.
                                                      5a chunker, 5c temporal regex, 5d pattern RE,
                                                      and 5e coref stub are sub-ms each.
6     score_coreference                 2–5 ms        small candidate sets, recency-based
7     apply_region_delta                2–5 ms        k-means prototype updates
8     build_relations                   3–8 ms        hashmap inserts + index updates
9     supersession + cache              2–5 ms        chain walks via meta_relations_by_subject filter on supersedes
10    reinforce_hebbian + salience      2–5 ms        Oja-rule bumps along focused path
11    decay_focus_radius                3–8 ms        bounded by policy.focus_decay_radius
12    aggregate_focus + enqueue_replay  2–5 ms        RRF merge + handoff to replay thread
                                        ─────────
                                        ~80–230 ms p50  (single tick, ≤480 tokens)

★ Step 5b GLiNER NER is v0's binding latency constraint. With Step 3
removed each tick processes a single ≤480-token unit (one inference
call through the NER encoder), so the p50 floor moves linearly with
extractor choice rather than with window count. Long inputs are the
caller's responsibility (chunk into multiple ticks); the binding
constraint is per-tick NER cost, not aggregate input length.
```

The path to sub-50 ms p50 is replacing or augmenting Step 5:

- **Pattern fast-path (§24.1).** Already live as Step 5d for the
  seed-pack frames (`from`/`to`/`with`/`at`/`property`). Average
  tick latency drops as pattern hit-rate climbs on mature corpora
  and we can skip Step 5b on confident pattern hits. Cheapest win.
- **Unified tiny-LLM extractor (§24.7).** A single Qwen-0.5B / Phi-3-
  mini class call replaces NER + RE + temporal + heuristic coref.
  ~50–150 ms on CPU INT8. Disruptive but flexible.
- **Smaller NER encoder.** A calibration-only change during
  bootstrap Step 6 (§21). Floor lowers further if a smaller GLiNER
  variant passes §19 + §20.5 quality gates.

Read-path / background-work splitting (§24.2) addresses non-extractor
contributions to tick latency, not the NER long pole. It is
secondary, not primary, on the path to sub-50 ms.

### 11.1 The Function

```rust
fn tick(
    hg: &mut Hypergraph,
    input: Input,
    source: Option<ElementId>,         // tick-level provenance pointer; written
                                       // as (R, source, source) meta-relations
                                       // on relations born this tick (Step 8–9)
) -> ConsciousAttentionFrame {
    wal_append(hg, &input);                                   // Step 0 (durability — §18.2)

    // --- Read-mostly phase (Steps 1–6, &Hypergraph) ---
    // Tick entry: validate input size + compute the input embedding
    // once. The embedding feeds Step 1 (intent classifier features)
    // and Step 4 (region routing). v0 collapses what was Step 4
    // ("embed") into this single up-front computation. See §11.5.
    let embedding = embed(&input);
    let intent  = detect_intent(&input, &embedding, hg);      // Step 1 → Intent (4-dim)
    let policy  = adjust_policy(&intent, &hg.policy);         // Step 2
    // Step 3 (segment/window) — REMOVED in v0. Caller chunks long
    // inputs into multiple ticks; one tick = one ≤480-token unit.
    // See §11.4 for rationale.
    let (active_regions, region_delta)
                = route_regions(&embedding, hg, &policy);     // Step 4 (delta held, not applied)
    let extractions
                = run_extractors(&input, &active_regions,
                                 &policy, hg);                // Step 5
    let coref   = score_coreference(&extractions, hg);        // Step 6

    // --- Mutation phase (Steps 7–12, &mut Hypergraph) ---
    apply_region_delta(hg, region_delta);                     // Step 7
    let (relations, events)
                = build_relations(&extractions, &coref, hg);  // Step 8
    apply_supersession_and_cache(hg, &relations, &events);    // Step 9
    reinforce_hebbian(hg, &focused_path, &policy);            // Step 10
    decay_focus_radius(hg, &focused_path, &policy);           // Step 11
    let attn = aggregate_focus(&relations, &policy);          // Step 12
    enqueue_replay(hg, &attn);     // also schedules background decay sweep
    attn
}
```

The phase boundary is strict: Steps 1–6 take `&Hypergraph` and produce
proposals (`region_delta`, `extractions`, `coref`); no hypergraph state
changes during this window. Step 7 onward takes `&mut Hypergraph` and
commits all proposals together. `apply_region_delta` is the first
mutation, not part of the read-mostly phase.

**Step 0 (WAL append)** runs before the read-mostly phase. It writes
`(Tick, Input, ModelFingerprint)` to the hot WAL segment as
LZ4-compressed MessagePack — a single sequential append to a
memory-mapped file, no fsync on the hot path (group commit at segment
close, §18.2). Cost is ~1 µs typical; the §11.0 `<1 ms` budget
covers worst-case page-fault. The fingerprint stamps each entry so
boot-time replay can refuse a WAL written under a different model
(§18.4).

### 11.2 Step 1 — Detect Intent

Intent is **not a categorical label**. It is a **4-dimensional
weight vector** scoring how much this tick should change the
substrate, along axes mapped to the neuromodulators that gate
brain memory consolidation:

```rust
struct Intent {
    /// Speaker certainty. High = "absolutely / definitely / I know";
    /// low = "maybe / I think / not sure". Drives default confidence
    /// for new relations and the Asserted/Defeasible threshold.
    /// Cognitive listener-evaluation analog (no direct neuromodulator;
    /// listeners do this evaluation explicitly).
    conviction: f32,           // [0.0, 1.0]

    /// Informational surprise. High when the input contradicts a
    /// prior belief OR introduces a concept far from existing
    /// regions. Drives salience boost + supersession-lookup trigger.
    /// Maps to the dopamine novelty/prediction-error signal
    /// (Lisman & Grace 2005; Wittmann et al. 2011).
    prediction_error: f32,     // [0.0, 1.0]

    /// Magnitude of importance signal. Caps, exclamation, intensifying
    /// vocabulary, emotional language. Drives salience independent
    /// of conviction or prediction-error. Maps to norepinephrine /
    /// amygdala-hippocampus arousal (LaBar & Cabeza 2006;
    /// Tully & Bolshakov 2010).
    arousal: f32,              // [0.0, 1.0]

    /// Retrieval-shape vs assertion-shape. High = "what is X / find
    /// when / show me"; low = "X is Y / X happened". Drives
    /// default-confidence reduction (the question's content shouldn't
    /// elevate as much as a statement's would) while still firing
    /// path reinforcement. No direct neuromodulator analog —
    /// Legend-specific because we have a single tick verb that
    /// covers both encoding and retrieval.
    curiosity: f32,          // [0.0, 1.0]
}
```

**How each dimension is scored.** Per-dimension binary
logistic-regression classifiers, one per intent dim, each trained
build-time from the seed pack and baked into the binary as a
`.bin` blob of `[f32; 418]` weights + `f32` bias. At runtime each
classifier outputs `sigmoid(w·x + b) ∈ [0, 1]`.

The 418-dim feature vector is the all-MiniLM-L6-v2 sentence
embedding (384) concatenated with 34 hand-crafted lexical features:
modal counts (epistemic strong/weak, deontic, certainty/uncertainty
idioms), pronoun person, question shape (`?`, wh-words, auxiliary-
verb-fronted, imperative-query stems), negation, correction /
revision / discovery markers, continuity markers, intensifiers,
emergency vocabulary, positive/negative emotion, importance vs.
dismissal, punctuation density, caps ratio, tense, and length.
The full list lives in `src/lexical_features.rs`.

The seed pack ships per-dim **phrase pools** (`high_pole`,
`low_pole`) plus **counterfactual pairs** (`pairs[].high`,
`pairs[].low`) — sentence pairs that share a topic but flip the
intent axis. Training combines two losses:

1. **Standard logistic regression** with class-weighted gradient +
   L2. Positives = own dim's high pool ∪ pair-highs. Negatives =
   own dim's low pool ∪ pair-lows ∪ **every phrase from the other
   three dims**. The cross-class negatives push the learned
   direction toward what's *unique* to this dim's high pole — Pearl
   Level-2 confounder adjustment ("first-person assertion shape" is
   common to all four dims; subtracting it leaves the dim-specific
   signal).
2. **Bradley-Terry contrastive** over own-dim pairs:
   `−log sigmoid(w·(h − l))` per pair. Pearl Level-3 controlled
   experiment — same topic, flipped intent — strips topical content
   out of the learned weights and pins the axis to intent shape.

**Lexical features as a front-door mediator.** The earlier draft of
this section said *"no marker phrases, no punctuation rules, no
hard-coded keywords."* That stance has been retired. Linguistic
surface form (modals, person, mood, punctuation) is causally
upstream of the embedding — the speaker's intent → word choice →
embedding — so hand-crafted features capture intent signal cleanly,
while the embedding alone carries intent confounded with topic.
Front-door routing around the topic confound (Pearl, *Book of
Why*, ch. 7) lifted held-out test accuracy from ~75% to 97.5% and
collapsed paraphrase-invariance score deltas on the curiosity
classifier from 0.33 to 0.06. The embedding still does most of the
semantic work; the lexical features are 34 of 418 dims and act as a
correction term.

**Graph-state component for `prediction_error` (deferred).** The
earlier spec called for `prediction_error` to be bumped toward 1.0
when Step 5's candidate extraction would supersede an existing
Asserted relation — the "actual contradiction" signal beyond the
linguistic-surprise component. Not yet implemented. Current
`prediction_error` is purely linguistic. The graph-state probe is
expected to land alongside Step 5 / Step 9. **Causal framing
(§6 (8)).** Binary contradiction is the v0 shape; the natural
extension scores the *information shift* between the substrate's
current causal-model prediction over the affected fluents (walking
`derived_from` and `antecedent_of`) and the new claim — surprise is
high when the new evidence contradicts what the existing structural
model would have predicted, modest when it just adds detail. Once
`intervened` (§16.3) lands, observation-vs-intervention status
weights the probe: contradicting an observation is more informative
about substrate error than contradicting an interventional claim
(the latter is just the world being changed).

**Cost.** The input embedding is computed once at tick entry
(~5–15 ms for short inputs; see §11.5) and threaded through to
Step 1 here and Step 4 (region routing). Step 1's marginal cost is
sub-ms — lexical-feature extraction + four 418-dim dot products —
since the heavy embedding work has already happened.

**No reliance on punctuation alone.** Even with the lexical
features, the embedding is still load-bearing. The question "Find
when I last saw Dr. Rao" lands as high `curiosity` driven by the
embedding plus the imperative-query-stem feature; "I don't know
where he is" lands as low conviction driven by the weak-epistemic
modal plus the negation feature plus the embedding. Neither cue
alone determines the score.

**What `Intent` does and does not change.** The vector feeds
Step 2 (`adjust_policy`, §11.3) and through it modulates exactly
five substrate knobs (`default_conf`, `vigilance`, `plasticity`,
`salience_multiplier`, supersession-trigger threshold).

`Intent` does **not** affect: which pipeline steps run, what
gets extracted, whether `apply_region_delta` commits, whether
`build_relations` writes, whether `reinforce_hebbian` or
`decay_focus_radius` fire, the shape of the returned frame, or any
structural decision about elements/relations. Every tick runs Steps
0–12 regardless. The only knobs are the weights inside those steps.

### 11.3 Step 2 — Adjust Policy

Pure scalar arithmetic — no model. PFC reads the `Intent`
vector from Step 1 and computes the adjusted `Policy` by combining
each dimension into the substrate knobs it drives. The base
mappings (§10.6 specifies the formulas in full):

```text
default_conf       = base_conf * conviction * (1.0 - 0.7 * curiosity)
                     // high conviction non-questions write at high
                     // confidence; questions and hedges write low

salience_multiplier = base_salience
                    + 1.0 * arousal                  // NE-analog
                    + 1.0 * prediction_error         // DA-analog
                    // both extremes (emotional + surprising) bump

leaf_vigilance     = base_vigilance
                   + 0.20 * prediction_error
                   + 0.20 * conviction
                   // contradictions and confident claims tighten
                   // routing so we don't blur entities; brainstorming
                   // (low conviction, low prediction_error) loosens

hebbian_rate       = base_rate * (1.0 - 0.5 * curiosity)
                                * (1.0 + 0.3 * arousal)
                   // questions reinforce paths but at lower magnitude
                   // than statements; arousal slightly amplifies

supersession_threshold = base_threshold * (1.0 - prediction_error)
                       // high prediction-error ⇒ search prior cache
                       // relations to supersede; low ⇒ skip the lookup
```

Every tick-internal subroutine reads `&Policy`; only PFC writes it.
The base `Policy` on `Hypergraph` is the inter-tick rest state; the
per-tick adjusted copy is what Steps 4–12 see.

**Worked examples:**

1. *"That's absolutely wrong! All the trees in my yard are under 4
   feet tall and will NEVER get taller"* —
   `conviction ≈ 0.95, prediction_error ≈ 0.90, arousal ≈ 0.85,
   curiosity ≈ 0.05`. Yields high `default_conf` (≈ 0.90) →
   relations land Asserted; high salience multiplier (≈ 2.75) →
   the new state and its supersession history get strong decay
   protection; low supersession threshold → Step 9 actively
   searches for prior cache relations to mark Superseded.

2. *"I'm not sure if the grass is green"* —
   `conviction ≈ 0.10, prediction_error ≈ 0.15, arousal ≈ 0.05,
   curiosity ≈ 0.20`. Yields very low `default_conf` (≈ 0.09) →
   any relation born this tick is Defeasible; near-zero salience
   bump → decays fast; high supersession threshold → Step 9 leaves
   prior beliefs alone.

Cost is a handful of multiplies — well under 1 ms.

### 11.4 Step 3 — REMOVED in v0

The original spec called for a `window_input` step that chunked long
inputs into one or more **windows** via SaT (Segment Any Text). v0
drops this step entirely: **one tick = one window, max 480 tokens.**
Long-input chunking is the caller's responsibility (LLM client,
ingestion script, human user) — they have better context about
natural boundaries (paragraph structure, semantic units, document
hierarchy) than this layer can recover from raw text alone.

**Why removed.** Three independent reasons that compound:

1. **SaT is too large to bundle.** The smallest variant
   (`segment-any-text/sat-3l-sm`) is ~409 MB FP16 / ~817 MB FP32.
   XLM-RoBERTa's 250K-vocab embedding table dominates regardless of
   layer count — `sat-1l-sm` is barely smaller. Bundling adds
   hundreds of MB to the binary; runtime download adds first-run
   network friction across every dev machine.

2. **SaT requires `ort`, which is disqualified.** SaT and GLiNER2
   were both spec'd on `ort` (the pyke.io binding to Microsoft's
   C++ ONNX Runtime). That runtime fails to link cleanly on dev
   machines we care about, and re-introducing native-link friction
   per machine is non-negotiable. Tract is the only acceptable
   runtime — and SaT loads in tract only after a manual FP16→FP32
   conversion that nearly doubles the size.

3. **UAX#29 sentence-aware packing isn't good enough.** A pure-Rust
   alternative via `unicode-segmentation` was prototyped end-to-end.
   It mis-splits on common abbreviations (`Mr. Henderson` → split
   between `Mr.` and `Henderson`), `Dr.`, `Inc.`, `p.m.`, `U.S.`,
   etc. The ~10% mis-split rate hits exactly the cases that matter
   for downstream relation extraction — separating named-entity
   tokens across window boundaries breaks the very relations Step 5
   would otherwise extract.

**The contract.** Callers submit ≤480 tokens per `tick()` call.
Inputs above the budget are rejected at the tick boundary with an
explicit error carrying the actual count and the limit; the caller
chunks and re-submits as multiple ticks. The 480-token threshold
matches GLiNER2's 512-token max minus a safety margin for special
tokens, positional buffer, and coref-context bytes.

**What didn't change.** References to "windows" elsewhere in this
doc remain accurate — there is still one window per tick, and Steps
4–6 still operate on it as a unit. Plural-window orchestration code
(rayon fan-out across windows, multi-window coreference linking,
multi-window relation merging) is simply not exercised in v0.

**Latency.** Step 3 contributes 0 ms. The validation check (single
tokenizer pass to count) lives wherever the tick entry point gates
input — counted under Step 0 / WAL append, not its own line.

**Future re-introduction.** Long-form ingestion can grow back into
the pipeline post-v0 if a real use case demands it — most likely as
a separate layer above `tick()` that hands off pre-chunked
sub-inputs, rather than as a Step 3 inside the pipeline. By then
either tract's op coverage may have grown to admit SaT, or a smaller
multilingual segmenter may have appeared.

### 11.5 Tick Entry — Input Embedding

(Was Step 4 in the original spec; folded into tick entry in v0 so
Step 1 and Step 4 share one inference. The MiniLM call is no longer
its own pipeline step — it runs once before Step 1 and the result
threads through.)

**all-MiniLM-L6-v2** (384-dim, 6 transformer layers, INT8-quantized,
~22 MB) running through the **in-house pure-Rust BERT engine**
(`src/inference/`, §15.1; no `tract-onnx`, no `ort`, no C deps).
Model weights are baked into the binary via `include_bytes!`.
Tokenization is `tokenizers` (HuggingFace pure-Rust crate, §15.1).
Inference is ~1.7–2.0 ms per call at 13 tokens on AVX-VNNI
(i7-1365U); AVX2 widen-pmaddwd and scalar fallbacks cover older
silicon and ARM. Implementation rationale + kernel notes live in
`docs/inference-engine.md`. With Step 3 removed there are no
multi-window fan-outs — one tick, one input, one inference.

**Storage format.** The in-house engine loads INT8 weights directly
from the bundled bin, so we don't carry a separate FP32 master in
production (an FP32 reference path is available under the
`fp32_reference` feature flag for `examples/validate_int8.rs`).
Snapshots keep the embedding output (the L2-normalized 384-dim FP32
vector) inline on each Element's `embedding` field — that's an
output of the quantized model, not the model weights themselves.

**Consumers within the tick.**
1. **Step 1 (`detect_intent`)** — feeds the embedding into the
   feature vector (embedding ++ lexical features → 4 logistic
   classifiers).
2. **Step 4 (`route_regions`)** — uses the same embedding as the
   query vector for DAG descent over region prototypes.

Both consumers receive `&[f32]` borrows of the same allocation; no
clone, no second inference call.

The substrate is dimension-agnostic at the type level, but the seed
pack's prototypes are dim-specific; swapping dimensions requires
re-embedding the seed and re-running the boot fingerprint check
(§18.4).

### 11.6 Step 4 — Route Through Regions

**The job.** Identify which regions of the DAG are *active for this
tick*, so Step 5's extractor can warm-bias its attribute-name label
set toward attribute names that already live near this input
semantically. This is a **fast predictive prefilter**, not the
substrate's authoritative answer about where new elements belong.

**Two-phase region routing in the pipeline.** The DAG is consulted
twice per tick, with different inputs and different jobs:

| Phase | Step | Input | Job | Persistence |
|---|---|---|---|---|
| Predictive | **Step 4** (here) | the **window's** ephemeral embedding | "what KIND of input is this?" → bias Step 5's label set | the window embedding is discarded after the tick |
| Authoritative | **Step 7** (§11.8a) | each minted **element's** own inline embedding | "where does this element belong?" → write `member_of` relations + spherical k-means prototype updates | element embeddings persist; region membership is durable |

Step 4's window-embedding routing exists specifically to break a
chicken-and-egg between extraction and biasing: extraction wants the
warm label set; the warm label set requires knowing which regions
are active; knowing active regions normally requires already-minted
elements; minting requires extraction. The window embedding gives a
~5 ms semantic prefilter that captures the input's *gestalt* (the
verb shape, the from/to construction, the change-vs-state cue —
things that don't reduce to any single extracted element) and lets
extraction proceed with a correctly tuned label set on its first
pass. Step 7 then does the authoritative placement once the actual
elements exist and have their own persistent embeddings.

This is why the substrate's "where does X live?" answer comes from
Step 7, not Step 4 — and why the window embedding is correctly
ephemeral while element embeddings persist.

**Mechanics.** Read-only DAG descent — no model, just cosine
similarity over already-computed vectors. Each window's embedding
runs `route_regions(...)` (§10.2) against the DAG. Parallelizes
across windows via `rayon::par_iter`. The active-regions set
surfaced to Step 5 is the **union** of each window's routing
results — multi-window inputs touch multiple regions, which is the
desired behavior.

**Algorithm.** Starting from `GENESIS`, for the window embedding:

1. Look up candidate children via `region_children[current]`.
2. For each candidate region, fetch its prototype Elements via
   `region_prototypes[region]` and per-region mean/var via
   `region_stats[region]` (built at load time from the same set).
3. Score each candidate by **two metrics** (Option D fusion):
   - `cosine` = **mean of the top-K** prototype cosines
     (K = `policy.cosine_top_k`, default 3; K=1 reproduces the
     previous max-pool). Mean-of-top-K requires prototype
     agreement so a single surface-overlap outlier can't dominate.
     Multi-prototype regions still preserve modes (§10.4).
   - `mahalanobis` = diagonal-Mahalanobis similarity against the
     per-region mean/var, with `policy.variance_prior` mixed into
     each per-dim variance for n=1 stability and small-n bootstrap
     smoothing. Distribution-aware: "does this input fit the
     region's spread?"
4. **Descent gate (cosine).** Descend into every child whose
   `cosine ≥ policy.descend_threshold` (no K cap on breadth).
5. **Leaf-vigilance gate (cosine).** Stop on a branch when the
   best `cosine` across children falls below `policy.leaf_vigilance`;
   the branch is **unrouted** and `delta.unrouted_count` increments.
   (Distinct from elements whose `polarity == Polarity::Void` —
   that's a semantic-content classification; this is a routing
   failure that surfaces as a quality signal in the frame, not an
   actual edge into VOID.)
6. **Activation gate (mahalanobis).** Among descended children,
   activate those whose `mahalanobis ≥
   policy.region_activation_threshold` (interpreted on the
   Mahalanobis-similarity scale, not cosine).
7. If the final active set is empty across the whole descent,
   raise `UncertaintySignal::DiffuseRouting`.

Each comparison is O(prototypes-in-region); the prototype set is
kept small by `policy.merge_threshold` (collapses near-duplicates)
and `policy.split_variance` (splits high-scatter regions). The
`RegionDelta` returned alongside the `ActiveRegion` list captures
proposed parent attachments, prototype-vector updates (k-means
targets — `(best_cosine_prototype, target)` pairs so Step 7's
spherical-k-means drift has per-prototype targets), and any
newly-minted regions (§10.3.5 mid-path insertions — owned by
Step 8, not Step 4); it is **held** through the read-mostly phase
and committed by Step 7 (§11.8a).

The `active_regions` set seeds extractor attention in Step 5 —
when a region is active, attribute names authored within relations
whose participants are members of that region (per the
`region_members` index over `member_of` relations) get a small
label-set priority, so GLiNER2 prefers the lexicon's "warm"
attribute names over cold ones.

### 11.7 Step 5 — Run Extractors

One call per tick over the whole input (Step 3 was removed; one
tick = one ≤480-token unit, see §11.4). The extractor sees the
entire input at once; sentence boundaries inside the input are not
consulted, which is what makes cross-sentence relations
recoverable without pre-segmentation hazards. The output is the
(entity, relation) candidate stream that feeds Step 6.

The v0 extractor stack runs five passes sequentially within the
step (the step itself is the latency long pole because of GLiNER
NER):

- **Step 5a — Orthographic chunker** (`src/steps/orthographic.rs`,
  `src/steps/void_filter.rs`). Pure Rust, no model. Always runs
  first; always emits at least one chunk for non-empty input.
  Emits `OrthographicChunk` records at three scales: `Phrase`
  (punctuation/whitespace-delimited spans), `Repeated` (any
  2..=5-token n-gram appearing ≥ 2× in the input), `Token`
  (content tokens — every whitespace-and-slash atom whose
  lowercased form does **not** resolve to a `Polarity::Void`
  element via `by_name`). The 118 closed-class void members
  seeded under the 8 void regions cover the high-frequency
  English function-word tail. Output lands in
  `ExtractionOutput.unconditional_chunks`; Step 8 mints an
  Element per entry regardless of whether NER labels it.
- **Step 5b — NER** — hand-rolled GLiNER1 span tagger on the
  in-house INT8 BERT engine (encoder = DeBERTa-v3-small, INT8).
  Spec originally called for `gline-rs` on `ort`, but `ort` is
  disqualified for portability and GLiNER2's disentangled-attention
  `Clip` op blocks naive tract loading; the span head is bespoke.
  Labels passed in are the seed kinds (`person`, `org`, `place`,
  `weekday`, `quantity`, `event`, `role`, `state`, `time`) plus
  warm-bias names from Step 4's active regions. Returns
  `(span, kind, confidence)` triples. Each tagged span auto-emits
  `(span_element, instance_of, K)`. Auto-emitted `instance_of`
  relations dedup against existing equivalents; reinforcement
  flows through the normal Step 8 path. Confidence
  ≥ `policy.ner_assertion_threshold` → `Entailed`; below →
  `Defeasible`. Anonymous spans are minted with
  `name = "<kind>_<counter>"`.
- **Step 5c — Temporal parser** (`src/steps/temporal.rs`). Pure
  `regex` over weekdays / months / `today` / `tomorrow` /
  `yesterday` / `tonight`. Each match becomes a value-Element with
  the surface form as a name; spans overlapping a 5b NER hit are
  dropped to avoid duplicate typing. Confidence is fixed at 0.95
  (regex precision). **`chrono` / `chrono-english` relative-phrase
  parsing is deferred** until we need to ground `"next Tuesday"`
  to a concrete datetime; typed comparison still re-parses the
  surface name on demand (§7.3).
- **Step 5d — Pattern-based relation extraction**
  (`src/steps/relation_patterns.rs`). Pure-Rust surface templates
  (§15.1 / §24.1 fast-path) over Step 5b spans: `X from A to B`,
  `X at Y`, `X with Y`, `X's Y`. Emits canonical
  `(subj_span, attr_name, obj_span, confidence)` quads with
  hardcoded attribute names (`from`, `to`, `with`, `at`,
  `property`). Covers the seed-pack frames. **The GLiNER2
  multi-task zero-shot RE model is deferred**; broader coverage
  grows through replay and warm-region labels.
- **Step 5e — Heuristic coref** (`src/steps/coref.rs`). Pure Rust,
  recency-based, no model. Pronouns (he / she / it / they / this /
  that) and definite descriptions ("the dentist") resolve to the
  most-recently-focused `RecentFocusEntry` whose `attribute`
  matches the span's grammatical slot (Centering Theory + Hobbs'
  baselines, §15.1). **Currently a stub returning no decisions** —
  wired into Step 5's return shape so the downstream API is stable,
  but `Hypergraph.recent_focus` isn't populated until §11.11
  lands, so there are no candidates to score against yet.

All extractor output carries confidence. The tick's `source`
parameter (§9.6, §11.1) flows into `(R, source, source)`
meta-relations on the resulting relations during Step 8.

**Causal-shape extraction conventions (§6 (8)).** Three conventions
sit on top of the four extractors above; none requires a new model
in v0, only that the relation-extraction step bias its label set
and emit the right meta-relations:

- **Tag agent-action verbs with `intervened`.** When the
  relation-extractor's verb belongs to the surface lexicon for
  agent action (`reschedule`, `move`, `set`, `configure`, `decide`,
  `cancel`, `ship`, `revert`, `merge`, `deploy`, `delete`), emit a
  `[target: R, intervened: <verb-element>]` meta-relation alongside
  the base relation. Verb elements link to `intervened` via
  `subclass_of` (§16.3); recognition walks the cone, so the
  surface lexicon stays open. Default — no tag, no `intervened`
  meta-relation — means observation. Steps 9 and 10 read the tag
  for do()-shaped supersession and reinforcement (§11.10, §11.11).
- **Prefer causal vs. correlational attribute names from
  linguistic markers.** "X caused Y" / "Y because X" / "X led to Y" /
  "due to X" → `caused`. "X enabled Y" / "made it possible for Y"
  → `enables`. "X prevented Y" / "blocked Y" → `prevents`.
  "X tends to happen with Y" / "X and Y both" / "comes with X" →
  `correlated_with`. The four canonical names are seeded (§16.3);
  surface variants emerge as ordinary attribute-name elements
  pinned via `subclass_of`. Without these conventions, GLiNER2
  collapses correlational and causal claims onto whatever surface
  attr_label the input used and Legend loses the rung distinction
  the seed pack created the names for.
- **Capture conditionals as `antecedent_of`, not as flattened
  base relations.** "If X happens, Y" / "Y holds when X" / "Y
  unless X" → emit a base relation for Y plus a meta-relation
  `[target: R_y, antecedent_of: R_x]`. Forward-chaining is §24.6's
  job to add later; v0's job is to *record the shape* so v1 has
  substrate to read from on day one (§16.3).

**Attribute-name label set.** GLiNER2's relation-extraction labels come
from, in order:

1. Seed-pack canonical attribute names (`instance_of`, `subclass_of`,
   participant attribute names) — always included.
2. The "warm" attribute names: attribute-name elements whose
   `MemoryStats` activation is above a floor. This is what active
   regions modulate — when Step 4 returned active regions, attribute
   names whose participants live in those regions get included even if
   their activation has decayed somewhat. Bounds open-vocabulary drift
   without freezing extraction to seed coverage.

**Resolving a proposed attribute-name label to an ElementId.** Each
extractor proposal arrives as
`(subj_span, attr_label, obj_span, confidence)`. Resolve `attr_label`
to an `ElementId` by:

1. Exact-match lookup against element names in the lexical index
   (tantivy, §15.1). On hit, reuse the attribute-name element.
2. On miss, embed `attr_label` and run a cosine search across **all**
   attribute-name elements (not just warm ones — the warm set is
   used for GLiNER2's label-set bias above, not for dedup). On any hit
   with cosine ≥ `policy.attribute_name_dedup_threshold`, reuse the top
   hit. The relation is marked `Defeasible` (the surface label didn't
   match the canonical name, so the binding carries some uncertainty
   even though the attribute name is right); replay can reinforce the
   alias later.
3. On miss, mint a new attribute-name element with the label as its
   name. Every relation that uses it this tick is `Defeasible` until
   replay either reinforces it (≥ N independent ticks within a window)
   or prunes it.

Note: there is no privileged "predicate" position in the resulting
Relation. The `attr_label` becomes the *attribute name* of one slot in
the relation's attribute list; the subject and object Elements are
bound via separately seeded attribute names (typically `subject` for
the head, `attr_label` itself for the object — see §7.2 worked example).
Step 8 assembles the full attribute list.

**Why the cosine search is universal, not warm-only.** Attribute-name
synonyms ("rescheduled to" / "moved to" / "changed to") embed close
together regardless of which is currently warm. Restricting dedup to
the warm set lets cold-but-equivalent attribute names pile up and re-
mint on every tick, defeating recognition that counts by attribute-
name id. The universal cosine search is `O(P)` per proposed label —
tractable at v0 attribute-name counts (low thousands).

**Mint-rate observability.** When a single tick mints more than
`policy.attribute_name_mint_warning_count` new attribute-name elements
(default 5), the inspection harness logs the tick id and replay
receives a priority flag for attribute-name dedup on this tick's
outputs (§14.8). Not a hard cap — a tick that legitimately introduces
several new attribute names is allowed — but the warning surfaces the
cases where synchronous dedup didn't catch a synonym cluster.

**Optional accelerator (post-v0):** a *lexicon-paired-noun* rule that
proposes intermediate DAG nodes upfront when both components of a
compound noun are already in the lexicon. See §24.8 for the v1+ form.

### 11.8 Step 6 — Coreference Scoring

Pure Rust scorer — no model. Operates on **entity-mention spans
returned by Step 5's NER and relation extractor** — not on Step 3's
windows (windows are containers; mentions are what coref resolves).
A "span" here is a contiguous slice of source text that an extractor
identified as referring to something — a pronoun ("it", "they"), a
definite description ("the dentist"), a partial name ("Doc Rao"),
or a freshly-tagged entity. Identity is conservative. For each
ambiguous span, build the candidate set from `recent_focus` (working
memory) plus elements within the active regions' neighborhood (via
`region_members[R]`), then score each candidate:

```text
score(span, candidate) =
    name_overlap(span, candidate.names)         // 0..1, edit distance / lemma match
  + embedding_similarity(span_emb, cand_emb)    // cosine
  + frame_overlap(active_frame, candidate)      // 1.0 if same frame, 0.5 if adjacent
  + attribute_overlap(span_slot, candidate)     // 1.0 if recent_focus entry's `attribute` matches the span's grammatical slot
  + temporal_compatibility                      // 1.0 if no valid-time conflict
  + relation_support                            // 0..1, fraction of candidate's relations consistent with span's neighborhood
  - contradiction_penalty                       // 1.5 if candidate has a Superseded relation that would re-fire
  - distinct_instance_penalty                   // §14.3 pattern_separation output
```

The attribute-overlap term is what `recent_focus` carrying
`RecentFocusEntry { element, attribute, frame, tick }` (§9.6) buys
us: "it" + most-recently-focused-as-`target` = the right element;
"it" + most-recently-focused-as-`actor` = a different one. Without
attribute-tagged focus, a flat `VecDeque<ElementId>` would resolve
all "it" pronouns to the most-recent-anything, which fails on
multi-attribute ticks like §19's Tick 5 ("the dentist moved it again
to Monday" — "it" should resolve to the appointment, not to the
dentist or the previous date).

Filtering the candidate set by `attribute` (the grammatical slot
the candidate was bound under in its prior focus) is **back-door-
style adjustment** (Pearl, §6 (8)): "most recent thing" confounds
"what the speaker just referred to" through "what was just
mentioned in any role"; conditioning on the slot blocks that
confounding path so the remaining signals (name, embedding, frame,
relation support) carry causal information about identity rather
than slot-mixed correlation.

Rules:

- Reuse concepts broadly (concept-recognition fires per §3.4 / §8.1).
- Reuse instances only with coreference support (multi-term score
  above a threshold).
- Create provisional instances when uncertain.
- Replay merges provisional instances later if support accumulates.

Pattern separation (`separate_pattern`, ported from current Legend's
dentate gyrus) is the dampening function on the merge side: when two
candidates are close-but-distinct on a discriminating role, force them
apart.

### 11.8a Step 7 — Apply Region Delta

The first mutation step — no model. This is the **authoritative**
phase of region routing (the predictive prefilter ran in Step 4,
§11.6). Step 4 used the window's ephemeral embedding to identify
which regions are active for biasing extraction; Step 7 now uses
the actual *element* embeddings (each minted Element's persistent
inline `embedding` field) to update the substrate's belief about
where elements belong. After this step, region membership is the
DAG's source of truth.

The `RegionDelta` returned by Step 4 (held through the read-mostly
phase) is now committed:

- **Parent attachments.** Each `(child, parent, weight)` becomes (or
  reinforces) a `(child, parent_region, parent)` relation with
  `stats.confidence = weight`.
- **Prototype updates.** Each `(prototype_element, new_embedding)`
  overwrites the prototype Element's inline embedding via the
  spherical k-means update rule (§10.5). The
  `(R, prototype, prototype_element)` relation already exists.
- **New regions.** Each `NewRegion` mints a region Element plus a
  prototype Element with the supplied initial vector as its inline
  embedding, then writes the seed structural relations
  (`instance_of REGION_CLASS`, `parent_region`, `prototype`).
  Mid-path insertions (§10.3.5) write the `parent_region` relation
  as `Defeasible`.
- **New members.** Each `(member, region)` becomes a
  `(member, member_of, region)` relation.

The region indices (`region_parents`, `region_children`,
`region_lateral`, `region_prototypes`, `region_members`) update
incrementally as these relations land. After this step the DAG
reflects whatever §10.2 routing decided, and Steps 8–12 see the
updated topology.

### 11.9 Step 8 — Build Relations and Events

No model — pure HashMap inserts + index updates. Each surviving
extractor proposal becomes a Relation whose **attribute list** is
assembled from the extractor's emitted slots. For a binary triple
`(subj_span, attr_label, obj_span)` the resulting relation has two
attributes — one binding the head Element under a participant
attribute-name appropriate to the extractor's frame (default
`subject`, or a frame-specific slot like `actor` for animate event
participants), and one binding the object Element under the attribute
name resolved from `attr_label`. `subject` is a seeded participant
attribute (§16.3) with no structural privilege — it is not indexed
separately, and recognition does not read it. For n-ary events the
attribute list grows: a reschedule event becomes one Relation with
`[target: appointment_1, property: date, from: Tuesday, to: Friday]`.

Per relation:

- **Status** set from extractor confidence vs
  `policy.ner_assertion_threshold` (Entailed / Defeasible).
- **`stats.confidence`** initialized as `policy.default_conf`
  (intent-modulated) × extractor confidence.
- **Source meta-relation** — a separate Relation
  `[target: R, source: source_id]` written iff the tick's `source`
  parameter is `Some` (§11.1).

`relations_by_element` / `relations_by_attribute_name` /
`meta_relations_by_subject` / `meta_relations_by_object` /
`attribute_value_counts` / `attribute_co_counts` /
`meta_relation_presence` all update incrementally — one HashMap
insert per (relation × attribute) pair per index. Build compact base
relations only; entailment closure is computed on demand (§14.5).

For: *"My dentist appointment with Dr. Rao changed from Tuesday to
Friday."*

Base elements created or reused:

```text
user, Dr. Rao, dentist, appointment, appointment_1,
Tuesday, Friday, reschedule_event_1
```

Base relations (shorthand `S attr O` ≡ a Relation with attribute list
`[subject: S, attr: O]` — `subject` is the seeded generic head
participant (§16.3), used here unless a frame-specific slot fits
better. The n-ary `reschedule_event_1` row is one Relation with four
attributes — `[target: appointment_1, property: date, from: Tuesday,
to: Friday]` — not four separate triples):

```text
DrRao instance_of person                         [Defeasible]
DrRao has_role dentist                           [Asserted]
appointment_1 instance_of appointment            [Entailed]
appointment_1 participant user                   [Entailed]
appointment_1 provider DrRao                     [Asserted]
appointment_1 domain dental                      [Entailed]
reschedule_event_1 instance_of reschedule_event  [Entailed]
reschedule_event_1 [target: appointment_1,
                    property: date,
                    from: Tuesday,
                    to: Friday]                  [Asserted]
```

### 11.10 Step 9 — Supersession and Cache

No model — index lookups + status flips. For each new event-shaped
relation (Event Calculus fluent update, §14.4) whose attribute list
includes `target`, `property`, `from`, and `to`:

1. **Look up prior cache relations** for the same target+property
   pair via `relations_by_element[target]` filtered to entries whose
   attribute list contains both `target` (with this value) and
   `property` (matching the event's `property` value).
2. **Mark each prior cache `Superseded`** — status flip in place,
   no delete.
3. **Write the new cache relation** `R_new` with
   `MemoryStats.confidence` carried from the event.
4. **Write the linking meta-relations** (themselves Relations whose
   attributes target `R_new`): one with `(target: R_new, derived_from:
   event)` and one per superseded cache with
   `(target: R_new, supersedes: R_old)`.

Worked example for `appointment_1 current_time`:

```text
R_new: appointment_1 current_time Friday   [Asserted]
R_old: appointment_1 current_time Tuesday  [Superseded]

(R_new, derived_from, reschedule_event_1)  [Entailed]
(R_new, supersedes,   R_old)               [Entailed]
```

`meta_relations_by_subject` and `meta_relations_by_object` update
incrementally as the new `supersedes` and `derived_from` meta-
relations land, so chain walks (forward via
`meta_relations_by_subject[R]` filtered to entries with a `supersedes`
attribute, inverse via `meta_relations_by_object[R]` filtered the same
way) remain O(chain-length) — one HashMap lookup plus a 0–3-element
filter per hop.

**`intervened` modulates the trigger threshold (§6 (8), §16.3).**
When the event carries an `intervened` meta-relation (the agent
acted on the world — `rescheduled`, `set`, `cancelled`, …), prior
cache supersession fires unconditionally regardless of confidence
gaps: do() severs prior causes by definition, so the previous
state's confidence is irrelevant. When the event lacks
`intervened` (default observation), the existing
`policy.supersession_threshold` (§10.6, intent-modulated) applies —
an observed contradiction may indicate the substrate's prior model
was wrong somewhere upstream rather than a clean state transition,
so high prior confidence raises the bar for flipping. Both paths
produce the same `Superseded` flip + `supersedes` meta-relation;
only the gate condition differs.

### 11.11 Step 10 — Hebbian + Salience

Pure arithmetic over `MemoryStats` — no model. Two updates fire:

**Hebbian co-activation.** For every pair (A, B) of elements that
co-occurred in the focus set this tick, walk to their connecting
relation R via `relations_by_element[A]` (filter to relations whose
attribute list also mentions B) and bump `R.stats.activation` via the
bounded Oja rule (§14.9):

```text
new = old + rate * (1 - old)
where rate = policy.hebbian_rate * intent.plasticity_multiplier
```

Asymptotes to 1.0; never overshoots. The intent multiplier from
Step 2 (§10.6 table) is already baked into `policy.hebbian_rate`,
so a question's reinforcement lands at lower weight than a
statement's on the same path.

**Salience formula.** For each relation R produced or reinforced this
tick, compute:

```text
score_salience(R, p) =
    p.salience_floor
  + 1.0  if R has an exact-value attribute (date-named, number-named, named entity)
  + 1.0  if R was just produced by supersession (Step 9) — preserve correction history
  + 0.5  if R is a user-stated preference (frame-scoped FRAME_USER + an attribute name is preference-shaped)
  + 0.5  if R is in this tick's focus set (focus-bearing on this tick)
  + 0.0  otherwise

bump = score * p.salience_multiplier
       // p.salience_multiplier already carries the +1.0*arousal +
       // 1.0*prediction_error contributions from §10.6, so emotionally
       // intense or surprising ticks land bigger bumps automatically.
R.stats.salience = bounded_hebbian_bump(R.stats.salience, bump * p.hebbian_rate)
```

The bump uses `bounded_hebbian_bump` (§14.9) so salience asymptotes
to 1.0 rather than running away. Salience floors decay's effect:
high-salience relations decay much more slowly than low-salience
ones (§14.7 utility formula).

**Promotion check (Defeasible → Asserted).** A `Defeasible` relation
is promoted to `Asserted` in this step when *all three* hold:

1. `stats.support_count >= policy.promotion_min_count` (default 3) —
   the relation has been observed across at least N independent ticks
   within `policy.promotion_window_ticks`.
2. `stats.support_diversity >= policy.promotion_min_diversity`
   (default 2) — the supporting ticks come from at least D
   *topologically distinct* evidence sources. Distinctness is
   measured across: different `(R, source, S)` source elements,
   different `Intent` regions (e.g. high-conviction-statement vs.
   curiosity vs. high-prediction-error mention), and different
   `active_frame` scopes — *and* the source elements themselves
   must be topologically independent in the source DAG. Step 10
   reads a replay-maintained annotation that maps each source
   element to its independent-root id (replay walks `derived_from`
   chains; §14.8), so two sources that trace back to the same
   root event count as one rather than two. Two Slack messages
   from the same user reposting the same git commit are downstream
   of one root event and don't clear the bar.
3. No contradicting relation has been written within the window
   (one `meta_relations_by_object[R]` lookup + `supersedes` filter).

The diversity check is what distinguishes "repeated assertion" from
"converging evidence" — and the topological-independence extension
is what distinguishes "converging evidence" from "echo chamber."
Without it, an extractor that rephrases the same input three times
auto-promotes wrong content; with it, promotion requires actually
independent evidence in the Pearl sense (§6 (8) — independent
causes, not just nominally distinct ids).

### 11.12 Step 11 — Focus-Radius Decay

No model — bounded BFS (Breadth-First Search) + scalar multiplies. Decay during the tick
is **bounded to the focus radius** so the read-mostly-then-mutate
phase stays under the latency budget. Walk outward from the focus
set up to `policy.focus_decay_radius` hops via
`relations_by_element`; for each element/relation reached, decay
`activation` via `bounded_hebbian_decay` (§14.9):

```text
new = old * (1 - rate * (1 - normalize(utility)))
```

where utility is the §14.7 score (focus_success + support_count +
salience − noise_score − redundancy − age_without_access).
High-utility relations decay slowly; sub-radius low-utility ones
decay quickly.

Everything outside the radius is decayed by the **background sweep**
(§14.7), scheduled by `enqueue_replay`. The sweep runs in the replay
thread, computes a delta against a snapshot, and the next tick applies
it under `&mut`. Decay weakens **access paths**, never destroys
focus-bearing relations (Invariants 2, 8).

### 11.13 Step 12 — Assemble Attention Frame

No model. The frame is a **post-tick snapshot of the focused
subgraph** — not a pre-assembled answer. Most fields are not
*computed* in Step 12; they are *gathered* from per-tick buffers
that earlier steps populated as a side effect of doing their own
work. The two things Step 12 itself produces are (a) the
`focused_relations` RRF (Reciprocal Rank Fusion; Cormack et al.
2009) over three already-computed signals plus a single tantivy
BM25 (Best Match 25) query, and (b) the `next_actions` suggestions.
Output shape:

```rust
struct ConsciousAttentionFrame {
    tick: Tick,
    input: InputEcho,
    intent: Intent,
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

#### Field-by-field reference

The "When" column names the step whose work *produces* the field's
contents; Step 12 only finalizes assembly.

| Field | What it carries | When | How |
|---|---|---|---|
| `tick` | The monotonic clock value at this tick's commit point. Lets the caller correlate with WAL entries, snapshot timestamps, and recent-focus tick stamps. | Step 12 | Read `hg.clock`. |
| `input` | A read-only echo of the input text. Not a substrate citizen; discarded after the caller consumes the frame. Exists so the caller has the question/statement in hand alongside Legend's response without threading it separately. | Step 0 (captured at tick entry); Step 12 (returned) | Construct `InputEcho { text }` from the original `Input.text`. |
| `intent` | The 4-dim `Intent { conviction, prediction_error, arousal, curiosity }` (§11.2). Exposes how this tick's intent vector landed, so the calling LLM can see "this tick was high-conviction correction-shaped" without reverse-engineering it. | Step 1 | Per-dim logistic-regression classifier over `embedding ++ lexical_features` (418 dims), trained build-time from `seed_pack.yaml`'s `intent_prototypes` (high/low pools + counterfactual pairs). Held through the tick; Step 12 just attaches it. |
| `active_frame` | The reference-frame element this tick operated under (e.g. `FRAME_USER`, `FRAME_PROJECT`). `None` if no frame was identified. Drives frame-relative supersession and frame-scoped retrieval. | Step 4 (carried forward from the previous tick's working memory unless this tick's input shifted frame) | Either inherited from `recent_focus`'s most recent entry's `frame`, or set by a frame-shifting cue extracted in Step 5 (e.g. a domain marker routing through `REGION_DOMAINS`). |
| `active_regions` | The regions activated for *this tick*, with per-region similarity scores. The union across windows for multi-window inputs. Lets the caller see "this input touched events + change_history + time." | Step 4 | `route_regions(...)` results per window, unioned. Each entry is a `RegionActivation { region, similarity }`. |
| `focused_relations` | The relations the caller reads its answer off of. Status-filtered: `Asserted` + `Entailed` by default; `Defeasible` flagged with `is_defeasible = true`; `Superseded` excluded (it lands in `history`); `Retracted` excluded entirely. | Step 12 | RRF merge over three signals — see §11.13 RRF prose below. |
| `supporting_claims` | Provenance pointers for `focused_relations`: the events and sources that produced the focused state. Lets the caller answer "where did this come from?" without an extra round-trip. | Step 12 | For each focused relation `R`, walk `meta_relations_by_subject[R]` filtered to entries with `derived_from` or `source` attributes; collect the resulting `RelationId`s. |
| `history` | Superseded ancestors of focused relations — what was true *before*. Lets the caller distinguish "is true now" from "was true." Bounded by the supersession chain depth. | Step 12 | For each focused relation `R`, walk `meta_relations_by_subject[R]` filtered to entries with a `supersedes` attribute, collecting the chain of `Superseded` predecessors. |
| `uncertainty` | Per-tick signals the caller may want to verify, follow up on, or surface to the user. Each entry is an `UncertaintySignal` variant: `LowConfidence(R)`, `Contradiction { a, b }`, `AmbiguousCoref { span, candidates }`, `UngroundedTime(R)`, `DiffuseRouting`. | Steps 4, 5, 6, 8, 9 (each step pushes signals into a per-tick buffer as it detects them) | Step 4 emits `DiffuseRouting` when no region clears `policy.region_activation_threshold`. Step 5 emits `UngroundedTime(R)` when chrono-english fails to ground a temporal expression. Step 6 emits `AmbiguousCoref` when no coref candidate exceeds the merge threshold. Step 8 emits `LowConfidence(R)` for relations under `policy.ner_assertion_threshold`. Step 9 emits `Contradiction` when a write would create a contradiction with an `Asserted` peer. Step 12 collects the buffer into the frame. |
| `durable_writes` | The `ElementId`s newly minted by this tick — what was *added* to the substrate. Lets the caller see "Legend learned about appointment_1 and reschedule_event_1 just now." | Steps 7, 8 (each mint records the new id into the per-tick write buffer) | Step 7 `apply_region_delta` records new region / prototype / member-of element mints. Step 8 `build_relations` records new entity / attribute-name mints. The buffer is finalized in Step 12. |
| `superseded` | Relation IDs flipped to `Superseded` status in this tick — what was *revised*. Pairs with `history` (which carries the same ids in their relation form, walked through the chain) but at the surface as a flat list for quick "what did this tick invalidate?" inspection. | Step 9 | Each `Superseded`-status flip in the supersession routine records the affected `RelationId` into the per-tick supersession buffer. |
| `next_actions` | Advisory suggestions for the caller / orchestrator: `EnqueueReplay { kind }` when this tick triggered a replay job (e.g. mid-path insertion confirmation, attribute-name dedup warning), `FollowUpQuery(text)` when an `UncertaintySignal::AmbiguousCoref` left a question unresolved that asking the user could answer cheaply. Not commands; the caller decides whether to act. | Step 12 | Step 12 inspects the assembled frame (especially `uncertainty` and any replay flags raised in Steps 4–10) and emits `AttentionAction` variants. The replay-enqueue itself happens in §11.14, after Step 12 returns. |

#### How `focused_relations` is computed

`focused_relations` aggregates relations from three signals via
**Reciprocal Rank Fusion** (RRF; Cormack, Clarke & Buettcher, SIGIR
2009). RRF is a parameter-light method for merging ranked lists:
for each item `d` appearing in lists `L₁, …, Lₙ`, the fused score
is `Σᵢ 1 / (k + rankᵢ(d))` with `k = 60` (the paper's recommended
default). The trick is that RRF discards the *scores* and keeps only
the *ranks*, which sidesteps the problem that the three signals
produce incompatibly-scaled values (BM25 is unbounded positive,
path-reinforcement is `[0, 1]`-ish, vote-weights are on yet another
distribution). A relation that ranks #3 in the dense list, #1 in
the BM25 list, and #5 in the path-vote list gets
`1/(60+3) + 1/(60+1) + 1/(60+5)` regardless of what magnitudes each
signal reported — robust, well-studied, and on retrieval benchmarks
consistently beats more complex score-normalization schemes.

The three input signals:

1. **Dense.** Path-reinforced relations from the focus set —
   relations on the path that this tick traversed in Steps 5–10
   (region routing → extractor proposals → coreference resolution
   → supersession). Each step adds the relations it touched into a
   per-tick focus buffer.
2. **Sparse.** Tantivy BM25 lookup against element names and
   relation participant fillers. This is what the §15.1 lexical
   index buys — proper-noun and identifier matches that dense
   embeddings systematically underweight (the `DrRao` element, the
   file path `src/foo.rs`, the issue id `#42`). The query terms are
   derived from the input text and from the focus set's element
   names. The lookup fires *in Step 12* and feeds RRF alongside the
   dense signal.
3. **Path-reinforced.** Relations whose `MemoryStats.focus_success_count`
   was bumped this tick (Step 10) get a vote weight from cone
   neighbors that reinforced together.

RRF merges all three rankings into a single
`Vec<RelationActivation>`. Each relation's `score` carries the
fused RRF value; `is_defeasible` is set independently from
`RelationStatus`.

#### Why some fields look redundant

`durable_writes` and `superseded` overlap with `focused_relations` /
`history` in ID space — anything in `superseded` will also appear
inside `history`, and the new relations corresponding to
`durable_writes` typically appear in `focused_relations`. The two
flat lists exist as a convenience for callers that want a quick
"what did this tick *change*?" view without walking the
fusion-ranked structure or the supersession chains. Same data, two
shapes.

#### What the frame is *not*

Not a pre-assembled answer. Not a knowledge-base query result. Not
durable. The calling LLM reads any natural-language response off
`focused_relations` (with `supporting_claims` for provenance and
`history` for superseded context), in light of `input`. Legend does
not assemble or rank answer candidates; that is the caller's job.

Provenance lives on the focused relations themselves — consumers
that want to know "where did this claim come from?" walk
`meta_relations_by_subject[R]` filtered by attribute name=`source`.
`supporting_claims` is just a pre-computed shortcut for the most
common provenance walk.

**Status filtering at frame-assembly time** (restated for emphasis,
since it crosses several fields). `focused_relations` includes
`Asserted` and `Entailed` by default. `Defeasible` appears with the
`is_defeasible` flag set and lower base weight; the calling LLM
can filter or present them as low-confidence. `Superseded` lands in
`history`, never in `focused_relations`. `Retracted` is excluded
from both. Consumers that need different semantics (e.g. debug
tooling that wants to see retracted state) read from the hypergraph
directly.

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
fn detect_intent(input: &str) -> Intent;
// v0: no Hypergraph dependency yet (graph-state probe for
// `prediction_error` is deferred — see §11.2). The eventual signature
// will take `embeddings: &[Vec<f32>]` (cached from tick entry) and
// `hg: &Hypergraph` for the supersession-candidate probe.
fn adjust_policy(intent: &Intent, base: &Policy) -> Policy;
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

Fires inside the co-activation step (Step 10). Bounded by
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

- value-Elements with exact, durable content (named times, ids,
  numeric quantities — §7.3)
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
| `Initiates(e, f, t)` | new current-state cache relation `R_new` plus paired meta-relation `[target: R_new, derived_from: e]` |
| `Terminates(e, f, t)` | prior current-state relation `R_old` marked `Superseded`; meta-relation `[target: R_new, supersedes: R_old]` written |
| `HoldsAt(f, t)` | walk `meta_relations_by_subject[R]` filtered to entries with a `supersedes` attribute to reach the non-Superseded leaf |

This is a 40-year-old logical foundation; adopt the vocabulary, don't
reinvent under different names. The participant attributes (`target`,
`property`, `from`, `to`) follow the standard treatment of events as
objects with named role-fillers (Parsons 1990; Davidson 1967) — under
attribute collapse, those role-fillers are just attribute-name
elements like any other.

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

1. **Focus-radius decay (in tick, Step 11).** Walk outward from the
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
protocol (§9.4). Region structural changes — splits, merges,
re-parents, retracts — surface as `ReplayMutation`s that write or
flip `parent_region` / `member_of` / `prototype` / `lateral_region`
relations; the relation graph stays the source of truth. Replay jobs:

- **background decay sweep (§14.7)** — periodic full-graph utility-based
  decay for everything outside the per-tick focus radius.
- split high-variance regions — emits a new region Element, a fresh
  `parent_region` relation pointing at the old parent, prototype
  relations partitioning the originals, and `member_of` relations
  re-parenting members; the old region either keeps a reduced
  membership or is retracted.
- merge duplicate regions — flips one region's `parent_region` /
  `prototype` / `member_of` relations to `Superseded` and writes
  redirected ones onto the surviving region; attribute-name-merge
  safety checks apply (Inv 8).
- **resolve provisional mid-path insertions (§10.3.5)** — every
  tick-time mid-path insertion writes a `Defeasible` `parent_region`
  relation. Replay walks `Defeasible` `parent_region` relations and
  resolves each to one of: (a) **confirm** — gap between
  node-to-child and node-to-parent cosine ≥
  `policy.midpath_confirm_gap` and the node was routed-against in ≥
  `policy.midpath_confirm_evidence` ticks without contradiction;
  flip the `parent_region` relation to `Asserted`; (b) **re-parent
  across subtrees** — node's cosine to a parent in a different
  subtree exceeds its current parent by ≥
  `policy.midpath_reparent_gap`; flip the old `parent_region`
  relation to `Superseded`, write a new `parent_region` relation
  to the new parent, and link them via `(R_new, supersedes,
  R_old)`. Available regardless of `Asserted` / `Defeasible`
  status — this is the recovery path for wrong-subtree placements
  driven by weak sentence-level routing on the introducing tick;
  (c) **retract** — `Defeasible` `parent_region` relation that
  failed to accumulate evidence within the window flips to
  `Retracted`; the node's children re-parent to the pre-insertion
  parent via fresh `parent_region` relations. This is how the DAG resolves
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
- merge duplicate attribute-name elements — Step 5 mint-time dedup
  (§11.7) is the primary defense; this replay job is **cleanup-only**,
  catching attribute-name elements whose embeddings drifted into
  convergence after their initial mint or whose surface labels were too
  dissimilar to trigger synchronous dedup. Merge when two attribute-
  name elements' embeddings converge within
  `policy.attribute_name_dedup_threshold`. Priority-bumped for ticks
  that fired the §11.7 mint-rate warning.
- **maintain source-DAG independence annotations (§11.11 promotion
  gate, §6 (8)).** Walk `derived_from` (and other lineage) chains
  over cited `(R, source, S)` source elements; for each source,
  cache the id of its independent root (the ancestor reached when
  no further `derived_from` link exists). Step 10's
  `support_diversity` check reads this annotation so two sources
  with the same root count as one. Cheap incremental update on
  any tick that wrote a new source element or new `derived_from`
  meta-relation; full sweep periodic. Catches echo-chamber
  repromotion — the same input rephrased through multiple
  downstream sources — where nominal id-distinctness would
  otherwise pass the gate.
- resolve provisional coreference.
- compact redundant relations.
- materialize useful derived relations.
- demote unused derived relations.
- evict prototypes when a region exceeds 8 — retract the
  lowest-weight prototype Element's `(R, prototype, P)` relation;
  the prototype Element itself is retracted unless still cited.

**Replay safety checks.** Every replay mutation is checked against a
small set of *local* safety conditions before it lands — not against
§19 at runtime (running the conformance fixture inside each replay
tick doesn't scale and overfits to one corpus). The checks are
structural and cheap:

- **Inv 8: facts don't merge.** Region merges are rejected if they
  would collapse two elements connected by distinct
  `instance_of`-targeted concepts.
- **Inv 9: cache relations carry `derived_from`.** Attribute-name-
  merge and cache-prune mutations preserve `derived_from` lineage;
  bare cache relations cannot be created or left as outputs.
- **Focus-bearing relations are not retracted.** A relation whose
  `stats.focus_success_count > policy.replay_focus_floor` (default
  3) is protected from compression-driven retraction; replay can
  still re-parent or re-scope it but not delete the claim.
- **Cycle resolution preserves at least one path.** Cycle retraction
  cannot leave a connected subgraph disconnected from its
  `derived_from` ancestor.

§19 + §20.5 conformance gates run in CI (Continuous Integration), not
in the replay loop. If a replay rule violates a benchmark, the CI
failure feeds back into
the rule's safety conditions — not into a runtime check that grows
unboundedly with corpus size.

### 14.9 Bounded Hebbian Operators

The Oja-rule-derived bounded Hebbian update used by §14.6 and Step 10
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

Pure Rust, no Python, no JVM, no sidecars, **no ONNX runtime at
runtime**. `tract-onnx` is retained as a *build-time* dev-dependency
for one-time weight extraction; runtime inference is hand-rolled.

### 15.1 v0 Components

1. **`tokenizers` (HuggingFace)** — tokenization. Apache-2.0, pure
   Rust. Golden-vector tests against reference outputs.
2. **In-house INT8 BERT engine** (`src/inference/`) — hand-rolled
   forward pass with three matmul kernels (AVX-VNNI / AVX2
   widen-pmaddwd / scalar), per-channel symmetric INT8 weights, and
   per-row dynamic activation quant. Carries both the embedder and
   the Step 5b NER encoder. ~17× faster than the `tract-onnx`
   baseline on the same MiniLM model. Pipeline + kernel notes:
   `docs/inference-engine.md`. `tract-onnx` is a dev-dependency
   only — used by `examples/extract_weights.rs` to dump fp32
   weights from the upstream ONNX one time, after which the runtime
   carries no tract code.
3. **~~`ort` (pyke.io)~~ — REMOVED.** Originally spec'd as a second
   ONNX runtime for GLiNER2 and SaT alongside tract. Disqualified:
   `ort` (Microsoft's C++ ONNX Runtime via pyke.io binding) fails to
   link cleanly on dev machines we care about. Tract was also
   examined and rejected for runtime use (slow on our model,
   doesn't load GLiNER2's disentangled-attention `Clip` op); the
   in-house engine in item 2 above replaces both. Consequence: SaT
   is dropped (§11.4); GLiNER1 NER ships on the in-house engine
   (item 4 below); GLiNER2 multi-task RE is deferred in favor of
   the pattern fast-path (item 5).
4. **all-MiniLM-L6-v2 (INT8, ~22 MB)** as the embedder.
   384-dim, 6 transformer layers. Pinned for Legend's lifetime
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
4. **`tantivy`** (0.26.x or current stable) — BM25 lexical index over
   element names + relation role fillers. Mandatory for proper-noun /
   identifier / file-path retrieval that dense embeddings systematically
   underweight. Pin a specific minor version at first integration; bump
   deliberately. The §11.13 frame-assembly RRF fusion uses tantivy as
   the sparse signal alongside the dense focus-set / path-reinforced
   signals.
5. **Temporal parser** — pure `regex` over weekdays / months /
   `today` / `tomorrow` / `yesterday` / `tonight` (Step 5c). Each
   match becomes a value-Element with the surface form as a name
   (e.g. `"Tuesday"`); confidence is 0.95 (regex precision).
   `chrono` / `chrono-english` relative-phrase grounding is
   **deferred** — required only when we want to resolve
   `"next Tuesday"` to a concrete datetime. Used at two sites:
   (a) **Extraction (Step 5)** — see above.
   (b) **Comparison sites** — supersession's `from`/`to` ordering
   (§11.10), frame-relative valid-time filtering (§11.13), and
   decay's exact-value spare (§13.8) re-parse the relevant
   value-Elements' names on demand. v0 stores no parsed form
   (§7.3); parse cost is negligible.
6. **NER + relation extraction — RESOLVED, hybrid.** The original
   `gline-rs` / `gliner2` path is unusable (`ort` disqualified;
   tract refuses GLiNER2's disentangled-attention `Clip` op over
   symbolic shape values). Current v0 stack:
   (a) **NER (Step 5b)** — GLiNER1 span head, hand-rolled, running
   a DeBERTa-v3-small encoder through the in-house INT8 BERT engine.
   ~50–150 ms per call. Output is `(span, kind, confidence)` triples
   over seed kinds plus warm-bias active-region names.
   (b) **Relation extraction (Step 5d)** — pure-Rust surface-pattern
   templates (`X from A to B`, `X at Y`, `X with Y`, `X's Y`) in
   `src/steps/relation_patterns.rs`. Covers the seed-pack frames.
   Sub-ms.
   (c) **Zero-shot RE (GLiNER2 multi-task) — deferred.** Pattern
   coverage grows through replay + warm-region labels; revisit if
   recall stalls.

   **★ Binding latency constraint in v0.** Step 5b NER is the long
   pole. The path to sub-50 ms p50 ticks runs through skipping
   Step 5b on confident pattern hits (5d → label propagation), a
   smaller NER encoder, or the unified tiny-LLM extractor (§24.7).
   See §11.0 for the per-step budget table.
7. **Heuristic coreference** — Step 5e (`src/steps/coref.rs`),
   pure-Rust recency-based stub. Centering Theory + Hobbs'
   baselines. Currently returns no decisions because
   `Hypergraph.recent_focus` isn't populated until §11.11; wired
   into Step 5's return shape for API stability.
8. **~~SaT (Segment Any Text)~~ — REMOVED.** Original spec invoked
   SaT only on inputs > 480 tokens via the `ort` runtime. Three
   blockers compound: SaT's smallest variant is ~409 MB FP16
   (XLM-RoBERTa's 250K vocab dominates), `ort` is disqualified, and
   the pure-Rust UAX#29 fallback mis-splits on common abbreviations
   (`Mr.`, `Dr.`, `Inc.`, `p.m.`). v0 rejects oversized inputs at the
   tick boundary instead — chunking is the caller's responsibility
   (§11.4).

### 15.2 What We Drop In v0

- **OpenIE (Open Information Extraction).** Stanford CoreNLP (Core
  Natural Language Processing) is JVM-only.
- **AMR (Abstract Meaning Representation) / UMR (Uniform Meaning
  Representation).** No portable implementation.
- **Cross-encoder reranker.** Path-aware reinforcement IS the reranker.
- **Dependency parser.** Not on the §19 walkthrough's critical path.
- **From-scratch tokenizer / BM25 / NER+BIO (Begin-Inside-Outside
  tagging) decoder.** Pure-Rust mature crates exist; writing our own
  buys nothing in 2026.

### 15.3 Beyond v0

Substantive v1+ ideas (patterns, latency optimization, hierarchical
frames, INT8 stored embeddings, HNSW (Hierarchical Navigable Small
World) over regions, forward-chaining inference, local-LLM unified
extractor, lexicon-paired-noun acceleration) live in §24.

### 15.4 Honest Estimates

Solo developer, evenings/weekends:

| Component | Estimate |
|---|---|
| `tokenizers` integration + golden-vector tests | ~0.25 wk |
| In-house INT8 BERT engine + MiniLM round-trip + validation | ~3 wk |
| `tantivy` integration + Legend's index schema | ~0.5 wk |
| Temporal parser (regex pass; chrono-english deferred) | ~0.25 wk |
| NER (hand-rolled GLiNER1 head on in-house BERT) | ~2 wk |
| Pattern-based RE (§24.1 fast-path; GLiNER2 deferred) | ~0.5 wk |
| Heuristic coref (stub; activates when recent_focus lands) | ~0.5 wk |
| Orthographic chunker + void filter (Step 5a) | ~0.5 wk |
| ~~SaT integration + windowing logic + multi-window fan-out~~ — REMOVED with Step 3 (§11.4) | — |
| **v0 model-stack total** | **~7.5 wk** |

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
seeded element with a one-sentence reason for being there.

**Seeding Criterion.** Seed an element only if it is **load-bearing
for recognition or for the v0 extraction machinery** — i.e. one of
the §3.4 recognition rules, the Step 5 extractor stack (§11.7), or
replay (§14.8) must be able to read its name or its prototype to
function. Everything else emerges via extraction and replay (§3.4:
"the seed pack is bootstrapping, not a closed vocabulary").

The seed pack has three categories:

- **Anchors** — `Genesis` and `Void`. Roots of the region DAG.
- **Attribute names** — the names §3.4's recognition rules and §11.7's
  extractor stack read by name (`instance_of`, `subclass_of`, the
  meta-relation attribute names, the region structural attribute
  names, plus a small set of generic participant attribute names like
  `target` / `from` / `to` / `actor`). Without these present at boot,
  recognition has nothing to count and extractors have no anchor to
  bind participants under.
- **Regions** — broad shape priors that bias routing (§10.2).
  Without seed regions, every input lands in an unparented cluster
  and routing has nothing to descend through.

Reference frames round out the pack so `[frame: F, target: R]`
meta-relations can be written without minting fresh elements per
tick. Modality is handled via the five behavioral attribute names
above (`negated` / `uncertain` / `non_actual` / `general` /
`intervened`) — there is no separate "modal elements" category,
since modality is just an attribute name in the uniform attribute-
list model. Causal commitment (rung 1 vs rung 2 — §6 (8)) is
handled the same way via the four causal-relation attribute names
(`caused` / `correlated_with` / `enables` / `prevents`).

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
  negated/uncertain/non_actual/general, supersedes, derived_from)
decay/reinforcement/replay mechanics
the tick pipeline
the embedding interface
```

Seeded hypergraph data owns priors:

```text
Genesis, Void
seeded attribute-name elements (§16.3 — including the 3 behavioral
                                 modal attribute names)
broad seed regions (§16.4)
seed reference frames
```

Inputs own truth — Legend keeps the distilled relations
(elements + relations), not raw inputs. Source pointers live on
`[target: R, source: S]` meta-relations for relations that need them.

Replay owns consolidation — region splits/merges, mid-path inserts,
cycle resolution, attribute-name dedup, the background decay sweep.

### 16.2 Seed Regions

Shape priors that bias routing (§10.2). Hand-author 15 broad regions
rooted at `Genesis` with a `Void` sink. Example:

```yaml
- element_id: REGION_CHANGE_HISTORY
  names: ["change/history"]
  instance_of: REGION_CLASS
  parent_regions:
    - [GENESIS, 1.0]              # written as (R, parent_region, GENESIS) at boot
  descriptor: >
    Something that was one way and is now different. A value moved from
    an old state to a new state. A revision, an edit, a correction, a
    rescheduling, an update.
```

A region is an Element pinned by relations
`[subject: R, instance_of: REGION_CLASS]` plus one or more
`[subject: R, parent_region: parent]`. Each is seeded with a descriptor
string; the boot loader computes the descriptor's embedding, mints a
prototype Element with that vector as its inline embedding, and writes
`[subject: R, prototype: P]`. Refined by online clustering (§14.1) as
inputs flow through.

There is **no** question region. Question-shape lives in the
`Intent.curiosity` dimension (Step 1, §11.2), not in
content routing. A question routes through the same regions as a
statement on the same topic — "what time is my appointment?" goes
through REGION_EVENTS / REGION_TIME.

### 16.3 Seeded Attribute Names

Attribute-name elements §3.4's recognition rules, §11.7's Step 5
extractor, and §10's region structural relations read by name:

- **`instance_of`, `subclass_of`** — concept-hierarchy attribute names.
- **Meta-relation attribute names** (8): `target`, `frame`,
  `valid_from`, `valid_to`, `source`, `supersedes`, `derived_from`,
  `antecedent_of`. These are the attribute names of meta-relations
  maintained by hot-path indices (§9.2). `target` is the head of
  every meta-relation (it identifies the relation being modified);
  the other seven name the modification. Modality used to live here
  as a single `modality` attribute pointing at one of six fixed modal
  elements; under the v0 design it has been replaced by the three
  behavioral modal attribute names below, with surface modals
  emerging via `subclass_of` rather than enumerated.
- **Region structural attribute names** (4): `member_of`,
  `parent_region`, `lateral_region`, `prototype` — the four attribute
  names that express region topology (§10.1) as ordinary relations.
  Maintained by the region indices (`region_members`, `region_parents`,
  `region_children`, `region_lateral`, `region_prototypes`) that
  hot-path routing reads in §11.6.
- **Generic participant attribute names** (7): `subject`, `actor`,
  `from`, `to`, `instrument`, `property`, `reason`. Extractors choose
  from these to anchor n-ary event participants (§11.9); `subject` is
  the catch-all head used when no frame-specific slot fits. Cold
  extractor proposals outside this set are minted via §11.7's path.
  None of these names is structurally privileged — recognition reads
  `attribute_value_counts` / `attribute_co_counts` keyed by attribute
  name, not by any specific slot.
- **Behavioral modal attribute names** (5): `negated`, `uncertain`,
  `non_actual`, `general`, `intervened`. These are the *behavioral
  kinds* of modality — substrate behaviors condition on them by name
  (negated content must not be stored as actual; uncertain content
  propagates lower confidence; non_actual content must be isolated
  from actual-state caches; general content resists supersession by
  specific instances and carries open-ended valid-time; intervened
  content is the result of an agent acting on the world rather than
  a passive observation, so supersession severs prior causes and
  reinforcement updates only the intervened claim — Pearl rung-2
  evidence vs. default rung-1 observation; §6 (8), §11.10, §11.11).
  Form: `[target: R, negated: <surface-form>]` /
  `[target: R, uncertain: <degree-or-surface>]` /
  `[target: R, non_actual: <kind>]` /
  `[target: R, general: <kind>]` /
  `[target: R, intervened: <agent-or-surface>]`. New surface modals
  (`might`, `must`, `would have`, `usually`, `typically`,
  `rescheduled`, `set`) emerge as ordinary attribute-name elements
  via §11.7 and link to one of these five via `subclass_of` (e.g.
  `(might, subclass_of, uncertain)`,
  `(usually, subclass_of, general)`,
  `(rescheduled, subclass_of, intervened)`); recognition walks the
  `subclass_of` cone. Absence of any modal meta-relation = actual
  observed specific claim — there is no separate `actual` or
  `observed` anchor; observation is the default.

- **Causal-relation attribute names** (4): `caused`,
  `correlated_with`, `enables`, `prevents`. Carry the *level of
  causal commitment* the source actually claims, so retrieval can
  walk only causally-grounded relations when answering "why" without
  collapsing rung-1 co-occurrence into rung-2 causal structure.
  Active form: `(subject: refactor_X, caused: deadline_slip)` reads
  as "refactor_X caused deadline_slip"; "deadline_slip was caused
  by refactor_X" extracts to the same active form. Surface aliases
  (`led to`, `produced`, `because of`, `due to`, `goes with`,
  `tends to follow`, `blocks`, `unblocks`) emerge via §11.7 and
  pin to the appropriate causal anchor via `subclass_of`. §11.7
  spells out the extractor convention; §6 (8) gives the rationale.

All other attribute names emerge per §11.7's mint-new-attribute-name
path.

**`antecedent_of` is load-bearing in v0 even though §24.6 forward-
chaining is deferred.** Conditionals expressed in input ("the deploy
fails if the migration ran first", "I cancel if it rains") are
captured as `[target: R, antecedent_of: R']` meta-relations on the
introducing tick — recording the conditional structure now so v1's
forward-chainer has substrate to read from on day one. Walking the
antecedent DAG is also the substrate-side hook for §24.9's
counterfactual queries.

**Why five behavioral kinds, extensibly.** Substrate behaviors
genuinely need *some* anchor to recognize "don't store this as
actual" (negation), "lower the propagated confidence" (uncertainty),
"isolate from actual-state caches" (non-actuality), "this is a
typical-case rule, not a specific instance — don't supersede it
from specific events" (generality), and "this claim is the result of
an agent acting, not a passive observation, so do() severs prior
causes" (intervention — Pearl's rung-2 lift, §6 (8)). Five attribute-
name anchors give those behaviors a stable target without fixing the
surface vocabulary: extractors mint `might` / `must` / `would have` /
`usually` / `typically` / `rescheduled` / `set` as ordinary
attribute-name elements and pin them to the appropriate behavioral
anchor via `subclass_of`. Recognition walks the cone. Same shape as
concept emergence (`instance_of: <kind>`), applied to modality.
Speech-act-shaped modals (desired, obligatory) all subclass
`non_actual` in v0; habitual / generic / universal modals (usually,
typically, always, in general) all subclass `general`; agent-action
verbs (rescheduled, moved, set, configured, decided) all subclass
`intervened`. v1 may introduce finer behavioral kinds if real usage
demands them.

### 16.4 Seed Pack Manifest

Total seed elements: ~55 (2 anchors + 30 seeded attribute-name
elements + 15 regions + 8 reference frames). Inline names below; full
rationales in `seed_pack.yaml`.

```text
anchors (2):                 GENESIS, VOID

seeded attribute names (30):
  ontology (2):              instance_of, subclass_of
  meta-relation (8):         target, frame, valid_from, valid_to,
                             source, supersedes, derived_from,
                             antecedent_of
  region structural (4):     member_of, parent_region,
                             lateral_region, prototype
  generic participant (7):   subject, actor, from, to, instrument,
                             property, reason
  behavioral modal (5):      negated, uncertain, non_actual, general,
                             intervened
                             (recognition walks subclass_of cone;
                              `might` / `must` / `usually` /
                              `typically` / `rescheduled` are minted
                              at runtime as subclasses)
  causal-relation (4):       caused, correlated_with, enables,
                             prevents
                             (Pearl-rung commitment of a relation;
                              §6 (8))

regions (15):             entities, events, states, change_history,
                          relationships, quantities, time, locations,
                          tasks, decisions, preferences, definitions,
                          provenance, domains, modal_negated

reference frames (8):     user, project, domain, session,
                          temporal_now, temporal_past,
                          temporal_future, meta
```

The old 11-entry "roles" category collapses into the participant
attribute names above (predicates and roles now share one uniform
set). `time` and `location` are not separately seeded as participant
slots — temporal and locational scope live on `valid_from` /
`valid_to` meta-relations and on value-Elements (§7.3); extractors
that want a within-relation location slot mint one via §11.7.
`agent` / `participant` collapse into `subject` (catch-all head) and
`actor` (for animate participants of an event).

Seeded relations:

- `instance_of` relations pinning seed elements into their structural
  roles (e.g. `[subject: REGION_CHANGE_HISTORY, instance_of:
  REGION_CLASS]`).
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

- **Decay + reinforcement scalars on every substrate citizen.** In
  `MemoryStats`, shared by elements and relations. Constants worth
  cribbing from current Legend's basal-ganglia AdaGrad code.
- **Salience scoring at write time.** Becomes the function that
  decides amygdala protection and the initial element's plasticity
  (high salience → lower plasticity → settled faster). Not a
  module — a function.
- **Pattern separation.** The "do not collapse close-but-distinct"
  rule used inside coreference scoring (§14.3). Current
  `dentate_gyrus.rs` is the reference implementation; we re-derive
  from scratch.
- **Working-memory ring buffer.** A `VecDeque<ElementId>` of the last
  ~64 focused elements. Used by coreference ("it" resolves against
  recent focus) and by Hebbian co-activation (§13.6).
- **Neurochemistry-style policy modulators.** Not the names
  (NE / DA / ACh (acetylcholine) / etc. are noise to a new reader), but the *idea* — global
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
- **Typed element kinds.** No `ElementKind` enum. Kinds are emergent
  structures of relations and recognition-index thresholds (§8).
- **Concept/Instance distinction at the type level.** Both are just
  Elements; recognition is via the
  `attribute_value_counts[E][instance_of]` index (§8.1).

### 17.3 What "Brain Regions" Means In v2

Each brain region from current Legend maps to a **function**, not a
module — none own state, the hypergraph is the only owned thing. The
full mapping with signatures lives in §13. Names are retained as
descriptive shorthand, not architectural boundaries.

### 17.4 Migration From Existing Legend Data

**There is no migration.** v2 is a fresh substrate; existing
`.legend/memory.lz4` files from v1 are not loaded, not converted, and
not referenced. Starting v2 against an existing v1 directory creates
a fresh hypergraph; the v1 files remain on disk untouched until the
user removes them. The boot-time fingerprint check (§18.4) refuses
mixed-version state, which makes accidental migration impossible.

Concepts carry forward (§17.1); data does not. This keeps v2 honest
to its substrate commitments — re-ingesting from sources where they
exist, accepting loss where they don't, and avoiding the engineering
cost of a one-time converter for a substrate that hasn't yet shipped.

---

## 18. Durability

### 18.1 The Snapshot

The on-disk hypergraph image is the canonical state.

- Format: LZ4 + MessagePack.
- Serialized fields: `elements` (each carries its inline embedding),
  `relations` (including the region structural relations),
  `clock`, `policy`, plus a `stamped_at: Tick` marker and the
  `ModelFingerprint` in force when written.
- Derived indices — including the region indices (`region_members`,
  `region_parents`, `region_children`, `region_lateral`,
  `region_prototypes`), the meta-relation indices, and the
  recognition indices — are rebuilt on load.
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
inputs that emitted no relations after Step 8, for diagnosing
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

### 18.5 Storage Cost and v0 Scale Bound

The hypergraph is dominated by **embeddings** (one ~1.5 KB f32 vector
per concept/region/pattern element; INT8 quantization in v1 cuts
this 4×). Relations and concept elements are ~100–500 B each. With raw
text and input records dropped, typical hypergraph sizes are orders of
magnitude smaller than naive transcript-with-index designs. The WAL is
bounded at 10 MB. Latency, not disk, is the primary scarcity (§2.2).

**v0 scale bound.** The substrate is in-memory and v0 is comfortable
up to ~100K elements + ~500K relations (roughly 200 MB resident with
FP32 embeddings). At ~1M elements / ~5M relations the recognition
indices alone reach hundreds of MB and tick latency drifts; that's
the v1 horizon for cold-storage tiering, INT8 stored embeddings
(§24.4), or HNSW over regions (§24.5). v0 does not implement
spill-to-disk; running past the soft bound degrades latency before
it fails.

**Replay snapshot cost.** §9.4 describes replay receiving a snapshot
clone. v0 clones the full hypergraph at replay-job start (~tens of
MB at v0 scale), computes a `Vec<ReplayMutation>`, and ships it back
via channel. Conflicts (a proposed mutation references a
since-retracted relation) are detected at apply-time on the next
tick boundary: the main thread skips affected mutations and logs
them; replay re-runs on the next cycle with the updated snapshot.
This is structurally similar to optimistic concurrency control —
cheap when conflicts are rare (the common case at v0 scale).

### 18.6 Privacy and Access Control

v0 has no access boundary. All consumers of a Legend instance
(notes-app frontend, agent harness, multiple Claude Code sessions)
operate as a single trust domain — there is no per-frame ACL, no
per-element redaction, no auth on the tick API. Multi-tenant
authentication and access-controlled retrieval are explicitly out of
scope for v0 (§2.4). Consumers that need separation should run
separate Legend instances per trust boundary; cross-instance sharing
is not supported in v0.

### 18.7 Process Model

Legend ships as a single binary that supports two execution modes
sharing the same `tick()` code path and the same on-disk state
(snapshot + WAL). The user-facing CLI looks like `grep` either way;
under the hood, a long-lived daemon amortizes the substantial
cold-start cost when one is running.

**Two modes, one binary:**

```text
legend "..."                  run one tick (auto-starts the daemon
                              on first call; subsequent calls are
                              thin TCP clients).
legend start                  launch daemon in the background.
legend stop                   graceful shutdown of the daemon.
legend status                 pid, uptime, tick count, substrate sizes.
legend init                   register the git merge driver for this repo.
LEGEND_INPROC=1 legend "..."  run Step 1–12 in-process, verbose dump
                              of every intermediate (no daemon).
LEGEND_RESET=1 legend "..."   skip on-disk snapshot, load fresh from
                              seed (in-process only).
```

**Mode resolution for `legend "..."`:**

```text
1. Read .legend/legend.port (ASCII "<pid>:<port>\n") and connect
   to 127.0.0.1:<port>.
2a. Connection succeeds → TCP-client mode.
       Send the Tick request over the socket; render the returned
       frame; exit. Cold-start cost: zero (the daemon already paid it).
2b. Connect fails OR port file missing → auto-spawn `legend start`
       in the background, poll for the port file (up to a short
       timeout), then retry the connect. Hard-error only if the
       daemon fails to come up.

For verbose debugging or test setup, set LEGEND_INPROC=1 (or
LEGEND_RESET=1) to bypass the daemon and run Step 1–12 directly in
the calling process. This path acquires .legend/legend.lock,
loads the snapshot, runs tick(), writes the snapshot, releases the
lock.
```

**Concurrency invariant: exactly one writer.** The lock file is the
single point of truth. The daemon holds a `fs2` exclusive flock on
`.legend/legend.lock` for its lifetime; the kernel releases it on
holder death. Two daemons cannot race: the second loses the flock
and exits quietly, leaving the first to keep serving. In-process
(`LEGEND_INPROC=1`) invocations acquire the same lock briefly
during snapshot write. TCP clients hold no lock — they only talk
to the daemon, which holds the single writer lock end-to-end.

**WAL is the bridge between modes.** Whether a tick is written from
one-shot or from the daemon, the WAL append is the same operation
against the same file. The daemon, on next start, loads the
snapshot and replays the WAL — picking up everything written by any
intervening one-shot invocations. This is what makes the modes
freely interchangeable.

**Checkpoint authority.** Snapshot compaction (writing a fresh
`seed_v0.msgpack.lz4` and truncating the WAL) is **daemon-only**,
on the §18.3 hybrid triggers (N=1000 ∨ S=5 MB ∨ T=1 hr), OR by
explicit `legend checkpoint` (which itself acquires the lock and
runs a one-shot checkpoint). One-shot tick mode (`legend "..."`)
never compacts; it only appends. The 10 MB WAL cap (§18.2) still
applies regardless of which mode is writing — segmented
oldest-eviction kicks in either way. A workflow that's exclusively
one-shot accumulates WAL up to the cap, then loses the oldest
segments earlier than a checkpointing workflow would. **Practical
recommendation: run `legend start` for any sustained workload;
reserve `legend "..."` for occasional ad-hoc ticks.**

**Cost picture, both modes:**

| Operation | Daemon (CLI-client) | One-shot |
|---|---|---|
| Snapshot deserialization | 0 (already in memory) | ~50–200 ms |
| Index rebuild | 0 | ~10–30 ms |
| tract + MiniLM load | 0 (already loaded) | ~300–500 ms |
| Embedder warm-up | 0 (already warm) | ~100–200 ms |
| Tick (§11.0 budget) | ~200–300 ms p50 | ~200–300 ms p50 |
| IPC (Inter-Process Communication) / lock overhead | ~1 ms | ~5–10 ms |
| **Wall-clock total** | **~200–300 ms** | **~700 ms – 1.5 s** |

One-shot is ~3–5× slower than daemon mode but is a real tick — same
code path, same correctness guarantees, same WAL durability. The
gap is purely the cold-start overhead the daemon amortizes across
its lifetime.

**Stale-socket handling.** Socket file present but daemon crashed
(ECONNREFUSED on connect): the CLI deletes the stale socket and
falls into one-shot mode. The daemon's startup acquires the lock
before opening the socket, so a stale socket without a live daemon
can never coexist with a live daemon.

**Daemon vs library.** The daemon binary is, mechanically, "a Rust
program that loops": load state, open a Unix socket, accept
requests, run `tick()`, send the reply, repeat until told to stop.
The function-signature contract in §0.1 / §11.1 (`fn tick(&mut
Hypergraph, ...)`) is the *internal* contract; the *external*
contract is the CLI surface above. Embedding Legend as a library
into another Rust process is a v1+ direction (§24.x); v0 ships the
CLI + daemon split.

**Read-only ticks under the daemon.** A daemon-routed query (no
relations minted) currently serializes through the daemon's tick
loop. At human speeds (200–300 ms ticks) this is fine. v1 may add a
parallel read-only endpoint that takes a snapshot reference of the
hypergraph and runs the read-path in parallel against it; v0 keeps
the single-loop model simple.

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

The active regions bias the attribute-name label set in Step 5
toward warm attribute names from these parts of the graph.

(The triple shorthand below — `S attr O` — denotes a Relation with
attributes `[subject: S, attr: O]`; meta-relation rows like
`(R9, derived_from, reschedule_event_1)` denote a Relation with
attributes `[target: R9, derived_from: reschedule_event_1]`. See
§7.2 / §11.9.)

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

stats updates (Step 10):
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
only the load-bearing attribute names and broad regions; the
appointment domain was never presumed.

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
warm `change` / `from` / `to` attribute names already present in the
graph.

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

Step 1 lands `Intent { conviction ≈ 0.20, prediction_error ≈
0.05, arousal ≈ 0.0, curiosity ≈ 0.85 }`; aggregate focus walks
all `appointment instance_of` elements with non-superseded
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

Replay enforces local safety checks at mutation time (§14.8) —
not a runtime benchmark check. §19 + §20.5 are CI gates; if a replay
rule violates them, the failure feeds back into the safety
conditions rather than into a runtime check that doesn't scale.

---

## 21. Build Order

Solo coder with Claude as reviewer. Every step's done-criterion is the
inspection-harness diff: hypergraph + attention frame after each tick
must match the predicted state. Spec sections in parens are the source
of truth; this section gives sequence and gates only.

**Conformance-test discipline.** Two test tiers, with different
determinism contracts:

- **Substrate conformance (§19, §20.5).** Run with **mocked
  extractor outputs** — attribute-name proposals and confidence values
  are hand-supplied per the walkthrough. The test asserts the
  *substrate's* behavior given fixed extraction: bit-identical
  hypergraph delta after each tick. This is the discipline already
  established in Step 4 ("hard-code §19 via direct `add_element` /
  `add_relation`, no NLP") and continues through every later step.
  Substrate conformance does **not** call the inference engine.
- **Full-stack smoke tests.** Run the actual extractor stack
  (MiniLM + GLiNER NER through the in-house INT8 BERT engine,
  pattern-based RE, regex temporal) on the same fixtures. Pin CI
  hardware to a fixed machine class (linux x86_64 AVX2 (Advanced
  Vector Extensions 2), INT8). Assert structural shape (which
  elements/relations exist, statuses, supersession links) but allow
  ε-tolerance on confidence values. Cross-machine determinism is
  out of substrate scope; INT8 / FP rounding can shift confidences
  enough to flip threshold-driven decisions on different hardware.

The replay-determinism fixture (Step 11) is part of the substrate
tier — it tests confluence of replay's rule application
independently of any extractor output.

### Step 0 — Foundation Infrastructure (~1 wk)

**Build:** Add v0 crates (`tract-onnx`, `ort`, `tokenizers`,
`tantivy`, `gline-rs`, `chrono-english`, `rayon`, `hashbrown`, `lz4`,
`rmp-serde`, `serde`). Round-trip the MiniLM-L6-v2 quantized model
through tract-onnx against a `sentence-transformers` parity oracle.
Wire the inspection harness (serialize → deserialize → pretty-print,
including region-proliferation over time per §10.6).
**Done:** bit-identical round-trip; embedding parity; harness prints
region creation rate.

### Step 1 — Substrate (~2 wk)

**Build:** §7 + §9 types + indices + supersession-chain walk. Element
(with inline `Vec<f32>` embedding) + Relation. No payload tables.
Region indices (`region_members`, `region_parents`, `region_children`,
`region_lateral`, `region_prototypes`) derived from the four region
structural attribute names + meta-relation indices
(`meta_relations_by_subject`, `meta_relations_by_object`) + recognition
indices (`attribute_value_counts`, `attribute_co_counts`,
`meta_relation_presence`).
**Done:** 50-element round-trip; supersession chains walk both
directions via `meta_relations_by_subject` / `_by_object` filtered
on a `supersedes` attribute; debug-asserts fire on
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

### Step 2.5 — CLI Front-End + IPC + Lock (~1 wk)

**Build:** §18.7 — single-binary subcommand dispatcher
(`legend "..."` / `legend start` / `legend stop` / `legend status` /
`legend init`, plus `LEGEND_INPROC=1` / `LEGEND_RESET=1` env-toggled
in-process paths). Mode resolution: read `.legend/legend.port`
(ASCII `"<pid>:<port>\n"`) and connect to 127.0.0.1 on that port;
on missing port file or connect failure, auto-spawn `legend start`
in the background and retry. Daemon side: acquire `fs2` exclusive
flock on `.legend/legend.lock`, bind TCP loopback, write the port
file, load substrate; accept loop runs `tick()` per request and
replies with the rendered frame as a length-prefixed MessagePack
payload. Stale-port handling: connect-fail → re-spawn. Stale-lock
handling: kernel-managed (flock auto-releases on holder death; no
PID file).
**Done:** `legend "..."` auto-starts the daemon on first call and
runs subsequent ticks as a TCP client (latency matches §11.0
budget); `LEGEND_INPROC=1 legend "..."` runs the full Step 1–12
pipeline in-process and prints every intermediate; two concurrent
daemons cannot race (the second loses the flock and exits);
`legend stop` exits cleanly and deletes the port file;
`legend status` reports pid / uptime / tick count / substrate sizes.

### Step 3 — Seed Pack (~1.5 wk)

**Build:** Hand-author the seed pack per §16 + `seed_pack.yaml`:
2 anchors (Genesis, Void) + 30 seeded attribute-name elements
(2 ontology + 8 meta-relation + 4 region structural + 7 generic
participant + 5 behavioral modal + 4 causal-relation — see §16.4)
+ 15 regions + 8 reference frames. Embed descriptor strings at boot
to mint each region's initial prototype Element with that
descriptor's vector as its inline embedding, then write the seed
relations (`[subject: R, instance_of: REGION_CLASS]`,
`[subject: R, parent_region: parent]`,
`[subject: R, prototype: P]`). Serialize as `seed_v0.msgpack.lz4`.
**Done:** boot shows ~55 elements in expected configuration; 2D
projection of region descriptor embeddings clusters sensibly;
meta-relation and region indices populate from the seeded
relations.

### Step 4 — Manual Conformance Set (~1 wk)

**Build:** Hard-code the §19 ten-tick walkthrough plus the three
§20.5 companion fixtures ("Two Sarahs," "Forgotten correction,"
"Frame drift") via direct `add_element` / `add_relation` (no NLP).
The non-appointment §21 Step 9.5 fixture (codebase rename or chat
preference shift) also lands here; landing it now exercises domain
neutrality before reinforcement and replay accumulate
appointment-shaped bias.
**Done:** §19 walkthrough passes; all three §20.5 fixtures pass; the
non-appointment fixture passes against the same code path;
`ConsciousAttentionFrame` shape is right.

### Step 5 — Embeddings + Region Routing (~1.5 wk)

**Build:** §10.2 — `route_regions` (read-only, parallel, top-k DAG) +
`apply_region_delta` (spherical k-means, §10.5). Diff-passing
discipline (§9.7).
**Done:** every span lands in the expected region; multi-prototype
bounded at 8; region-creation rate decays after first 20 ticks.

### Step 6 — Temporal Parser + NER + Relation Extraction (~2.5 wk)

**Build:** Input-size validation at the tick boundary (reject inputs
> 480 tokens with an explicit error carrying the actual count and
the limit; caller chunks). `chrono-english` for the 80% temporal
coverage + thin uncertainty-grounding layer. NER + RE: see §15.1.6
— `gline-rs` is ruled out by its `ort` dep, decision deferred to
this step.
**Done:** Tick 1 emits `Tuesday`, `Friday`, `DrRao`, and the
reschedule triple without hand-coding; oversized inputs return a
rejection with a clear "got N tokens, max 480, please chunk"
message and do not partially process. (Original gate also tested
SaT-driven multi-window equivalence — REMOVED with Step 3, §11.4.)

### Step 7 — Event Reification + Supersession Cache (~1.5 wk)

**Build:** §14.4 Event Calculus mapping; supersession chains via
`[target: R_new, supersedes: R_old]` meta-relations and the
`meta_relations_by_subject` / `_by_object` indices (filtered by
attribute name=`supersedes`); cache relations with paired
`[target: R_new, derived_from: X]` meta-relations.
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
cache pruning, attribute-name dedup, cycle resolution
(lowest-confidence retraction), and the background decay sweep
(§14.7). Reject any mutation that breaks §19 or §20.5. Add the
**replay-determinism fixture**: take a starting hypergraph (the §19
walkthrough's end-state works), run replay twice with different
rule-application orders (e.g. shuffled by a fixed seed; permute the
order in which mid-path / attribute-name-dedup / cycle-resolution /
region-split jobs are applied), assert bit-identical final hypergraph
state. This is
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
    the role catalogue (frame, valid_from/to, source, modality —
    where Legend splits modality into five behavioral attribute
    names, §16.3) maps directly. See §7.2.
13. **JTMS (Justification-based Truth Maintenance System) / ATMS
    (Assumption-based Truth Maintenance System)** — Doyle 1979;
    de Kleer 1986. Legend's relation-status discipline is
    JTMS-flavored.
14. **AGM (Alchourrón-Gärdenfors-Makinson) + Hansson Base Revision**
    — Levi identity is the formal name for Legend's correction
    protocol.
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

**How Legend differs (capsule):**

```text
system        primitive             retrieval shape          legend differs by
─────────────────────────────────────────────────────────────────────────────
Graphiti/Zep  typed nodes + edges,  query API + KG walk      no typed nodes; no separate
              bitemporal                                     query API; one tick verb
HippoRAG 2    dual-node KG (entity  Personalized PageRank    no dense/sparse split node-
              vs concept) + dense   over a separate index    side; recognition via indices
              embeddings                                     not declared kinds
A-MEM         LLM-driven schema     LLM rewrites memory      no LLM in the substrate;
              evolution             on demand                emergence via replay rewrites
Mem0          hybrid vec/graph/KV   3-store federation       single hypergraph; no
              memory                                         federation overhead
```

The shared design space is bitemporal + dense + structural retrieval
for agent memory. Legend's distinguishing bets: one identity primitive
(no typed nodes), one verb (no query API), recognition through
derived indices (no declared kinds), inline embeddings on elements
with no payload tables (typed values are Elements parsed on
comparison), replay-driven structural emergence.

### NLP / Embedding / Retrieval

30. **Sentence-BERT** — Reimers & Gurevych 2019, arXiv 1908.10084. Why
    raw BERT is not an embedding model.
31. **MiniLM** — Wang et al. 2020, arXiv 2002.10957.
    "Deep Self-Attention Distillation for Task-Agnostic Compression
    of Pre-Trained Transformers." The v0 embedding model
    (`all-MiniLM-L6-v2`, sentence-transformers fine-tune,
    ONNX-quantized).
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
BGE-small (initial pick; replaced by MiniLM-L6-v2 quantized once we
moved the embedder to pure-Rust tract-onnx — both are
sentence-transformers 384-dim models, MiniLM gives faster inference
on 6 layers vs. 12 with comparable retrieval quality on our
workload), LoCoMo (scoring controversy — §20.6).

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
- When (if ever) should a payload table be introduced? v0 has none —
  embeddings are inline on Element, region structure is in the
  relation graph, typed values are Elements that parse on
  comparison. v2 may reconsider if (a) profiling shows date/quantity
  parsing on the hot path dominates and a typed-comparison cache pays
  for itself, (b) a parallel `Vec<f32>` embedding array beats inline
  `Vec<f32>` for SIMD scans, or (c) a future emergent payload
  kind (e.g. `pattern_matchers` for v1 patterns, §24.1) warrants its
  own table.
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
doc. None are scoped for v0. Two intents in this section, separated
below: **Deferred capabilities** (24.1, 24.6, 24.7 — features
removed from v0 because unvalidated, planned to land in v1) and
**Forward roadmap** (24.2, 24.3, 24.4, 24.5, 24.8 — optimizations
and extensions to add when scale or quality demands them).

### Deferred Capabilities

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
payload table; add an `activate_patterns` step before extraction (or as
a fast-path that short-circuits Step 5 on match); add a replay job
that clusters concrete relation shapes and mints new pattern
relations with `Defeasible` status; add the `Defeasible → Asserted`
promotion path for patterns specifically.

### Forward Roadmap

### 24.2 Latency Optimization (Secondary Contributors)

v0 targets ~200–300 ms p50 per tick. The path to sub-100 ms is
dominated by Step 5 (zero-shot relation extraction) — see §15.1's
GLiNER2 callout, §24.1's pattern fast-path, and §24.7's unified
tiny-LLM extractor. Those are the *primary* paths; this section
lists the **secondary** contributors that help once the long pole
is shorter.

- **Read-path / background-work split.** v0 already pushes the
  full-graph decay sweep (§14.7) and attribute-name dedup (§14.8) onto
  the replay thread, so the tick path is mostly free of these. v1 can
  push the remaining incremental decay (Step 11) onto a per-tick
  background hand-off if profiling shows it on the critical path.
  Unlike pre-rewrite expectations, this split is small: the decay
  budget at §11.0 is 3–8 ms, not tens of ms.
- **Interning attribute names** — small fixed table of `u32` ids
  alongside the `ElementId` lookup, so hot extractor paths skip a
  hash probe. Saves ~1–3 ms per tick.
- **Splitting the wide `MemoryStats` struct** into parallel
  `Vec<f32>` columns for cache locality (already in §23 deferred
  questions). Saves a few ms on Steps 10–11.

These together knock 5–15 ms off a tick. They are not a substitute
for Step 5 changes; they are what closes the last gap once the
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

### 24.9 Counterfactual Queries As Non-Actual-Modal Ticks

Pearl rung 3 (§6 (8)). v0 commits to one verb (`tick`, §12) and
"no separate query API"; counterfactual queries should preserve
both. The shape: a counterfactual *is* a tick whose input carries
`non_actual` modal content ("what if I hadn't rescheduled?", "if
the migration had run first, would the deploy have failed?"); Step
12 returns a counterfactual subgraph instead of an actual one.

Pearl's three-step procedure maps onto existing Legend machinery:

| Pearl step | Legend operation |
|---|---|
| **Abduction** — update `P(U)` given evidence | The supporting-claims walk Step 12 already runs for the actual frame: collect supporting relations and `derived_from` ancestors. No new mechanism. |
| **Action** — surgically replace `X` with `X = x` | Tick-local, **non-mutating** projection over the hypergraph: hide the relation(s) the counterfactual severs and mask their `derived_from` descendants in the read view. |
| **Prediction** — compute `Y` under the modified model | Run Step 12's same path traversal against the projected hypergraph; emit the resulting `focused_relations` back to the caller, every relation in the counterfactual subgraph tagged via `non_actual` so the caller can distinguish counterfactual from actual content. |

The substrate carries everything needed: `non_actual` modal for
tagging the input claim and the projected output, `derived_from`
DAG as the structural causal model to walk, `antecedent_of` for
conditional rules to re-fire under the projection, the §11.13
frame-assembly pipeline for the read-only output. No new substrate
shape — only a query-time projection layer over existing reads,
plus an extractor-side convention for recognizing counterfactual
sentence shapes ("what if", "would have", "had X been"). Counter-
factual ticks are non-mutating by definition; Step 0 still appends
to the WAL but Steps 7–11 are skipped, and the returned
`ConsciousAttentionFrame.durable_writes` is empty.

What v0 does *not* do but should not preclude: actually compute
Pearl-correct counterfactual probabilities. v0 will return a
*structural* answer ("which relations would change, and to what?")
rather than a calibrated probability; calibration needs richer SCM
annotation than the substrate currently carries (functional
equations on the `derived_from` DAG, exogenous-variable priors).
**Design now, build later** — the v0 substrate is shaped so this
projection-over-existing-state fits without parallel infrastructure.
