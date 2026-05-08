# Legend v0 — Core

> **Status:** Compressed read-path of `new_foundation.md`. Everything you
> need to understand the substrate and start building. Forward-points
> to the full spec for citations, deep rationale, beyond-v0 ideas, and
> the source map. Aim: ~1.5 hr read, complete enough to begin.
>
> **Companion docs:**
> - `new_foundation.md` — full spec (§ references in this doc resolve there).
> - `seed_pack.yaml` — the seed elements with rationales.

---

## 1. The Contract

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
                       — single-verb API (Application Programming
                       Interface); every input is one tick;
                       retrieval shares the same code path as writes
                       — source is a sibling parameter, not on Input

PROCESS MODEL          single binary; daemon mode (`legend start`) or
                       one-shot per-invocation (`legend "..."`); same
                       tick code path either way. Lock-enforced single-
                       writer invariant via fcntl flock on
                       ~/.legend/legend.lock; mode discovery via
                       ~/.legend/legend.sock.                        (§10.1)

PHASES                 Step 0     WAL (Write-Ahead Log) append
                                  (durability I/O)
                       Steps 1–7  read-mostly (&Hypergraph, parallel)
                       Steps 8–13 mutation    (&mut Hypergraph, seq)  (§4.2, §4.3)

INTENT VECTOR          Intent { conviction, prediction_error,
                                     arousal, curiosity } — 4-dim,
                       per-dim logistic-regression classifier over
                       MiniLM embedding ++ lexical features (418 dims),
                       trained build-time from seed pack; modulates
                       default_conf, salience, vigilance, hebbian_rate,
                       supersession_threshold. Maps to DA / NE /
                       cognitive analogs. Does NOT gate which steps
                       run.                                           (§10.6, §11.2)

DURABILITY             snapshot (LZ4+MessagePack) + bounded WAL
                       (10 MB cap, LZ4 hot, zstd-19 closed),
                       checkpoint at N=1000 ticks ∨ S=5MB ∨ T=1hr,
                       boot fingerprint check refuses on mismatch     (§18)

EMBEDDER               all-MiniLM-L6-v2 (ONNX-quantized, ~23 MB),
                       running through tract-onnx (pure-Rust runtime),
                       384-dim, pinned for life — model swap =
                       re-ingest per recoverability matrix            (§15.1, §18.4)

LATENCY BUDGET (v0)    ~200–300 ms p50; GLiNER2 dominates             (§11.0, §15.1)

CONFORMANCE GATES      §19 ten-tick walkthrough (substrate, mocked extractors)
                       §20.5 three companion fixtures
                       §21 Step 11 replay-determinism fixture
                       LongMemEval + MemoryAgentBench + RULER (full-stack) (§20)

KEY INVARIANTS         15 numbered items                              (§5)
```

---

## 2. What Legend Is

Legend is **long-term memory for LLMs** — a persistent substrate that
carries continuity across LLM (Large Language Model) sessions and
maintains a **living model of the slice of reality Legend has been
told about**. A single hypergraph holds everything Legend remembers;
vector content, relational content, and provenance all live in the
same structure, queried through different lenses.

**The four-piece story:**

1. **Elements.** Bare identities — `id`, names, stats, creation tick,
   plus an optional inline embedding. Meaning emerges from relational
   position; the inline embedding is the semantic anchor for elements
   that need a vector position.
2. **Relations.** Typed hyperedges expressed as a flat list of named
   attributes (each binding an attribute-name Element to a Term:
   either an Element or another Relation). Plus status (Asserted/
   Entailed/Defeasible/Superseded/Retracted), confidence, priority,
   creation tick. No separate predicate slot — predicates and roles
   collapse into the uniform attribute list.
   **Anything that modifies a relation — frame, valid-time, source,
   modality, supersession, lineage, antecedent — is a meta-relation,
   itself an ordinary Relation whose `target` attribute value is the
   modified relation.**
3. **Discoveries.** Each tick is one. New information arrives,
   distills into elements + relations + meta-relations, evolves
   Legend's model. One operation: `tick`.
4. **Emergence.** Kinds (concept, instance, event, frame) are read
   from derived **recognition indices** over the relation graph.

The single deepest commitment: ontology emerges from accumulated
relational structure. The seed pack supplies **substrate-mechanism
anchors** (meta-relation attribute names, five behavioral modal
attribute names — `negated` / `uncertain` / `non_actual` / `general`
/ `intervened`, four causal-relation attribute names — `caused` /
`correlated_with` / `enables` / `prevents` — Pearl rung 1/2/3
distinction, full doc §6 (8) + §16.3) that recognition rules read
by name; world-content categories emerge from extraction and replay.

---

## 3. The Architecture

**Mental model: Legend runs like a game loop.** Input → process →
update the entire hypergraph → render only the attention-relevant
subgraph as a `ConsciousAttentionFrame`. The vocabulary is deliberate
— `tick` and `frame` are the game-engine terms, used here for the same
reason: discrete time-stepped state evolution, with the rendered
output being a *view* (what's in the user's focus), not a snapshot of
the whole world. Full analogy in the main doc §4.0.

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

           RECOGNITION INDICES (derived, rebuilt on load)
           ──────────────────────────────────────────────
           attribute_value_counts[E][N]   → "concept" / "frame"
           attribute_co_counts[E][N]      → "instance"
           meta_relation_presence[R]      → "event-shaped"
           region_members[R], region_parents[R], region_children[R],
                                          → region topology cache
           meta_relations_by_subject[R]   → meta-rels targeting R
           meta_relations_by_object[R]    → meta-rels mentioning R
                                            in non-target attributes
```

**Two storage primitives, owned by `Hypergraph`:**

- Elements (identity, with inline `Vec<f32>` embedding; typed-value
  Elements like "Tuesday" / "6 pounds" carry the surface form as a
  name and parse on comparison)
- Relations (claims — including the four region structural attribute
  names `member_of`, `parent_region`, `lateral_region`, `prototype`)

Region topology is recovered through derived indices over the region
structural relations — same pattern as the meta-relation indices for
frame, source, and supersession; v0 carries no payload tables outside
the relation graph itself.

---

## 4. Substrate Types (Spec Slice)

### Element

```rust
struct Element {
    id: ElementId,
    names: Vec<String>,           // canonical + variant; both decay if unused
    stats: MemoryStats,
    created_at: Tick,
    embedding: Vec<f32>,          // semantic anchor; populated at mint
                                  // time from `names` (or originating
                                  // span text for anonymous NER (Named
                                  // Entity Recognition) spans)
}
```

That's the entire Element. Region topology lives in the relation
graph (§10) as `member_of` / `parent_region` / `lateral_region` /
`prototype` claims; typed leaf values ("Tuesday", "6 pounds",
"Berlin") are themselves Elements whose surface forms live in
`names` and whose typed semantics are parsed on comparison (§7.3).

### Relation

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
    name: ElementId,                   // attribute-name element
    value: Term,
}

enum Term {
    Element(ElementId),                // concrete filler
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

Six fields. A Relation is a flat list of named attributes — no
privileged predicate slot, no separate role-binding struct. The
attribute name identifies the slot; the value is either an Element
or a nested Relation. Anything that modifies a relation is a
meta-relation: an ordinary Relation whose attribute list includes a
`target` slot taking `Term::Relation(parent_id)`.

### MemoryStats

```rust
struct MemoryStats {
    activation: f32,              // current tick's activation level
    confidence: f32,              // belief strength
    plasticity: f32,              // long-term durability — high =
                                  // formative (easy update, fast decay);
                                  // low = settled (hard update, slow decay)
    salience: f32,                // accumulated amygdala-style protection
    access_count: u32,
    focus_success_count: u32,
    support_count: u32,           // independent ticks supporting this
    support_diversity: u32,       // distinct evidence-source dimensions
    prediction_error: f32,
    last_seen: Tick,
    last_accessed: Option<Tick>,
}
```

Elements and Relations share the same stats — memory dynamics are
uniform across both primitives.

### Hypergraph

```rust
struct Hypergraph {
    elements: Vec<Element>,        // each carries inline Vec<f32> embedding
    relations: Vec<Relation>,      // includes region structural relations
                                   // (no payload tables — typed values
                                   //  are Elements that parse on compare)

    clock: Tick,
    policy: Policy,
    recent_focus: VecDeque<RecentFocusEntry>,

    // Derived indices — rebuild on load, never serialize.
    by_name:                     HashMap<String, Vec<ElementId>>,
    // All relations that mention E as the value of any attribute.
    // No subject/object split — `subject` is just a seeded attribute
    // name (§16.3) with no structural privilege.
    relations_by_element:        HashMap<ElementId, Vec<RelationId>>,
    // Relations that have at least one attribute named N.
    relations_by_attribute_name: HashMap<ElementId, Vec<RelationId>>,

    // Region indices — derived from member_of / parent_region /
    // lateral_region / prototype relations. Hot-path routing reads
    // these instead of walking the relation graph.
    region_members:         HashMap<ElementId, Vec<ElementId>>,
    region_parents:         HashMap<ElementId, Vec<(ElementId, f32)>>,
    region_children:        HashMap<ElementId, Vec<ElementId>>,
    region_lateral:         HashMap<ElementId, Vec<ElementId>>,
    region_prototypes:      HashMap<ElementId, Vec<ElementId>>,

    // Meta-relation indices — two inverses. Specific lookups ("frame
    // of R", "what supersedes R") become a small filter (typical 0–3
    // entries) over the indexed list, keyed on attribute name.
    //   meta_relations_by_subject[R]: meta-rels whose `target` (or
    //                                 head-shaped) attribute value
    //                                 is R — forward walks.
    //   meta_relations_by_object[R]:  meta-rels mentioning R as the
    //                                 value of a non-target slot
    //                                 (`supersedes`, `derived_from`,
    //                                 …) — inverse walks.
    meta_relations_by_subject: HashMap<RelationId, Vec<RelationId>>,
    meta_relations_by_object:  HashMap<RelationId, Vec<RelationId>>,

    // Recognition indices — derived attribute-name counts, no
    // privileged head slot.
    //   attribute_value_counts[E][N]: relations binding (name=N,
    //                                 value=E). Drives concept /
    //                                 frame recognition.
    //   attribute_co_counts[E][N]:    relations mentioning E *and*
    //                                 carrying an attribute named N.
    //                                 Drives instance recognition.
    attribute_value_counts: HashMap<ElementId, HashMap<ElementId, u32>>,
    attribute_co_counts:    HashMap<ElementId, HashMap<ElementId, u32>>,
    meta_relation_presence: HashMap<RelationId, HashSet<ElementId>>,
}
```

### Meta-relation worked example

```rust
// World claim: Dr. Rao plays the role of dentist.
R = Relation {
    id: 42,
    attributes: [
        Attribute { name: SUBJECT,  value: Term::Element(DrRao) },
        Attribute { name: HAS_ROLE, value: Term::Element(dentist) },
    ],
    status: Asserted, ...
};

// Frame meta-relation: R is in FRAME_USER.
M_frame = Relation {
    id: 43,
    attributes: [
        Attribute { name: TARGET, value: Term::Relation(42) },  // ← R
        Attribute { name: FRAME,  value: Term::Element(FRAME_USER) },
    ],
    status: Entailed, ...
};
```

`M_frame` lands in `meta_relations_by_subject[42]` on commit;
retrieval reads "frame of 42" as one HashMap lookup followed by a
filter for an attribute named `frame` over a typically 0–3-element
list — effectively O(1). Reflective reasoning (depth-2 or deeper)
recurses on the meta-relation's own id; **v0 hot path reads depth-1
only**.

### Recognition indices

The four canonical recognitions are reads against derived indices
+ Policy thresholds:

- **Concept.** `attribute_value_counts[E][instance_of] >= policy.concept_recognition_threshold` (default 3).
- **Instance.** `attribute_co_counts[E][instance_of] > 0`.
- **Event.** `meta_relation_presence[E]` shows valid-time-bounded participant-attribute shape.
- **Reference frame.** `attribute_value_counts[E][frame] >= policy.frame_recognition_threshold` (default 5).

An element can function as multiple kinds simultaneously, but
**v0 retrieval operates within a single active frame at a time** —
cross-frame access requires an explicit per-frame tick or
`(R, also_in_frame, F')` meta-relations.

### Auxiliary types

```rust
type ClaimRef = RelationId;

struct Input {
    text: String,
    wall_clock: SystemTime,
}

struct InputEcho {                 // read-only echo in the frame
    text: String,
}

// Source is meta-relation-shaped — passed as a sibling parameter to
// tick(); the pipeline writes (R, source, source) meta-relations on
// relations born this tick. Not a property of the Input or the echo.

struct RecentFocusEntry {          // working-memory entry
    element: ElementId,
    attribute: Option<ElementId>,  // attribute name binding this
                                   // element on its most recent focus
                                   // (e.g. SUBJECT, ACTOR, TARGET, …)
    frame: Option<ElementId>,
    tick: Tick,
}

struct ModelFingerprint {
    embedder_hash: [u8; 32],
    tokenizer_vocab_hash: [u8; 32],
    extractor_versions: Vec<(String, String)>,
    code_version: String,
}
```

---

## 5. The Tick Pipeline

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
        │  STEP 3   window         ─► windows            │
        │           (SaT only if input > ~480 tokens)    │
        │  STEP 4   embed          ─► vectors per window │
        │  STEP 5   route_regions  ─► active_regions +   │
        │                              held RegionDelta  │
        │  STEP 6   run_extractors ─► proposals          │
        │           ★ GLiNER2 per window = the long pole │
        │  STEP 7   coreference    ─► reuse decisions    │
        ├────────────────────────────────────────────────┤
        │            ─── MUTATION PHASE ───              │
        │            (&mut Hypergraph; sequential)       │
        │  STEP 8   apply_region_delta                   │
        │  STEP 9   build_relations + events             │
        │  STEP 10  supersession + cache                 │
        │  STEP 11  Hebbian + salience                   │
        │  STEP 12  focus-radius decay                   │
        │  STEP 13  aggregate_focus  ─►                  │
        └──────────────────┬─────────────────────────────┘
                              │
                              ▼
              ConsciousAttentionFrame
                              │
                              ▼
                  enqueue_replay (post-tick)
```

### The function

```rust
fn tick(
    hg: &mut Hypergraph,
    input: Input,
    source: Option<ElementId>,            // tick-level provenance pointer
) -> ConsciousAttentionFrame {
    // --- Read-mostly phase (Steps 1–7, &Hypergraph) ---
    let intent  = detect_intent(&input, hg);                  // Step 1
    let policy  = adjust_policy(&intent, &hg.policy);         // Step 2
    let windows = window_input(&input, &policy);              // Step 3
                                                              // (SaT only if > ~480 tokens)
    let embeds  = embed(&windows);                            // Step 4 (per-window, parallel)
    let (active_regions, region_delta)
                = route_regions(&embeds, hg, &policy);        // Step 5  (delta held; union per-window)
    let extractions
                = run_extractors(&windows, &active_regions,
                                 &policy, hg);                // Step 6 (per-window, parallel)
    let coref   = score_coreference(&extractions, hg);        // Step 7

    // --- Mutation phase (Steps 8–13, &mut Hypergraph) ---
    apply_region_delta(hg, region_delta);                     // Step 8
    let (relations, events)
                = build_relations(&extractions, &coref, hg);  // Step 9
    apply_supersession_and_cache(hg, &relations, &events);    // Step 10
    reinforce_hebbian(hg, &focused_path, &policy);            // Step 11
    decay_focus_radius(hg, &focused_path, &policy);           // Step 12
    let attn = aggregate_focus(&relations, &policy);          // Step 13
    enqueue_replay(hg, &attn);
    attn
}
```

### Per-step latency budget (v0)

```text
step  name                              p50 budget    notes
0     log entry (WAL append)            <1 ms         LZ4 hot segment append
1     detect_intent                     5–15 ms       embedding + 4 logistic classifiers; today
                                                      embeds independently (will share with Step 4
                                                      once cached, then sub-ms marginal)
2     adjust_policy                     <1 ms         scalar copy + multiplier
3     window                            <1 ms (short) tokenize + length check
                                        +10–20 ms     SaT (~22M params) only when
                                        (long)         input > ~480 tokens
4     embed                             5–20 ms       MiniLM-L6-v2 (quantized) per window
5     route_regions                     5–15 ms       DAG (Directed Acyclic Graph) descent per window
6     run_extractors                    130–208 ms    ★ GLiNER2 per window
                                        × N windows
7     score_coreference                 2–5 ms        small candidate sets
8     apply_region_delta                2–5 ms        k-means prototype updates
9     build_relations                   3–8 ms        hashmap inserts + indices
10    supersession + cache              2–5 ms        chain walks via index
11    reinforce_hebbian + salience      2–5 ms        Oja-rule bumps
12    decay_focus_radius                3–8 ms        bounded radius
13    aggregate_focus + enqueue_replay  2–5 ms        RRF (Reciprocal Rank Fusion) merge + handoff
                                        ─────────
                                        ~160–290 ms p50  (single-window, the common case)
```

GLiNER2 is v0's binding latency constraint. Sub-100 ms p50 requires
replacing/augmenting Step 6 — see full spec §24.1 (pattern fast-path)
and §24.7 (unified tiny-LLM extractor).

### Step notes

**Step 0 — WAL Append.** Append `(Tick, Input, ModelFingerprint)` to
the hot WAL segment as LZ4-compressed MessagePack. Single
sequential write to a memory-mapped append-only file; no fsync on
the hot path (group commit at segment close). The fingerprint
stamps each entry so boot-time replay can refuse a WAL written
under a different model. ~1 µs typical; `<1 ms` budget covers
worst-case page-fault.

**Step 1 — Detect Intent.** Intent is **not a categorical label**. It
is a **4-dimensional weight vector** scoring how much this tick should
change the substrate, along axes mapped to the brain's
memory-consolidation neuromodulators:

```rust
struct Intent {
    conviction: f32,         // speaker certainty (cognitive analog)
    prediction_error: f32,   // novelty / contradiction (DA analog)
    arousal: f32,            // emotional intensity / importance (NE analog)
    curiosity: f32,          // retrieval-shape vs assertion-shape
}
```

Each dimension has its own binary logistic-regression classifier,
trained build-time from the seed pack and baked into the binary as
a `.bin` blob (`[f32; 418]` weights + `f32` bias). At inference each
classifier outputs `sigmoid(w·x + b)`.

The 418-dim feature vector concatenates the all-MiniLM-L6-v2
sentence embedding (384) with 34 hand-crafted lexical features
(modals, person, question/imperative shape, negation, correction
markers, intensity, punctuation, tense, length). Lexical features
sit upstream of the embedding in causal order — speaker's intent →
word choice → embedding — so they act as a Pearl front-door
mediator that strips topic confounding from the intent signal. See
full spec §11.2 for the rationale.

Training combines two losses per dim:

- **Logistic regression** with class-weighted gradient + L2.
  Cross-class negatives (every other dim's phrases used as
  negatives) force orthogonal directions across dims.
- **Bradley-Terry contrastive** over `pairs[]` in the seed pack —
  counterfactual sentence pairs that share a topic but flip the
  intent axis (e.g. `"I am certain the meeting is at 3pm"` vs.
  `"I think maybe the meeting is at 3pm"`). Forces
  `score(high) > score(low)` for matched pairs.

**Graph-state component for `prediction_error` (deferred).** Earlier
spec called for `prediction_error` to bump when Step 6's candidate
extraction would supersede an existing Asserted relation. Not yet
implemented; current score is purely linguistic. Lands with Step 6
/ Step 10.

Cost: per-call dominated by embedding inference (~5–15 ms short
input); lexical extraction + four 418-dim dot products are
microseconds. **The pipeline should cache the input embedding and
reuse it in Step 4 (Embed Windows)** — not yet wired, so for now
Step 1 embeds independently and Step 4 will re-embed. With caching,
Step 1's marginal cost drops to sub-ms.

**Step 2 — Adjust Policy.** Pure scalar arithmetic — no model. Map
the 4-vector to the substrate knobs via the §10.6 formulas:

```text
default_conf       = base_conf * conviction
                                * (1.0 - 0.7 * curiosity)

salience_multiplier = base_salience
                    + 1.0 * arousal              // NE analog
                    + 1.0 * prediction_error     // DA analog

leaf_vigilance     = base_vigilance
                   + 0.20 * prediction_error
                   + 0.20 * conviction

hebbian_rate       = base_rate * (1.0 - 0.5 * curiosity)
                                * (1.0 + 0.3 * arousal)

supersession_threshold = base_threshold * (1.0 - prediction_error)
```

Worked examples:

- *"That's absolutely wrong! All trees in my yard are under 4 feet
  tall and will NEVER get taller"* — `conviction ≈ 0.95,
  prediction_error ≈ 0.90, arousal ≈ 0.85, curiosity ≈ 0.05`.
  High `default_conf` → relations Asserted; high salience
  multiplier → strong decay protection; low supersession threshold
  → Step 10 actively searches prior cache for `(yard_trees,
  max_height, _)` to mark Superseded. Future query "max tree
  height in user's yard?" returns 4 ft.
- *"I'm not sure if the grass is green"* — `conviction ≈ 0.10,
  prediction_error ≈ 0.15, arousal ≈ 0.05, curiosity ≈ 0.20`.
  Very low `default_conf` → Defeasible; near-zero salience → fast
  decay; high supersession threshold → prior beliefs untouched.

The base `Policy` on the Hypergraph is the inter-tick rest state;
the adjusted copy is what Steps 3–13 see. Only PFC (Prefrontal Cortex)
writes Policy.

**Step 3 — Window The Input.** Chunk the input into one or more
**windows** sized to fit GLiNER2's max-input length (~512 tokens;
the threshold uses ~480 for safety margin). Each window is what
Steps 4–7 process as one piece — Step 6's extractor sees the entire
window at once and finds all relations across all sentences in it,
so the "logical pieces" of the input are the relations Step 6
produces, not anything Step 3 produces.

Two paths by length:
- **Short input (≤ ~480 tokens, the common case for chat-message-
  sized ticks).** No segmentation; the whole input is one window.
  Cost: one tokenizer pass, ~µs. Risk of mid-relation split: zero.
- **Long input (> ~480 tokens).** Invoke **SaT (Segment Any Text)**
  — a small ~22M-param ONNX (Open Neural Network Exchange) model
  running through the same `ort` runtime as GLiNER2 (the embedder
  uses tract-onnx; SaT/GLiNER2 are on `ort`) — to find sentence/paragraph
  boundaries that respect natural discourse breaks rather than
  blindly chopping at token counts. Greedy-group SaT segments into
  ≤480-token windows. If a single SaT segment exceeds the budget
  (rare — wall-of-text URL list, code block), fall back to
  token-budget windowing for that segment only. Cost: ~10–20 ms for
  SaT + ~µs for grouping.

Pre-splitting at sub-window granularity is deliberately avoided —
GLiNER2 finds cross-sentence relations within its window, and
forcing an internal split would risk separating an entity from its
relation partner. SaT operates *between* GLiNER2 windows, not
inside them.

**Step 4 — Embed Windows.** **all-MiniLM-L6-v2** (384-dim, 6
transformer layers, ONNX-quantized, ~23 MB) running through
**tract-onnx** (pure-Rust ONNX runtime — no C++ deps, portable to any
OS Rust supports). Model bytes are baked into the binary via
`include_bytes!`. Tokenization is `tokenizers` (HuggingFace pure-Rust
crate). Each window from Step 3 becomes one vector; for multi-window
inputs, embedding calls fan out across windows via `rayon::par_iter`.
Quantized inference is ~3–5 ms per call on a 4-core commodity CPU
(Central Processing Unit). Single-window inputs (the common case)
make one inference; multi-window inputs make N parallel inferences
and the 5–20 ms budget covers up to ~5 windows in parallel. The
runtime carries only the quantized model — no separate FP32 master,
since tract-onnx loads quantized weights directly.

**Sharing with Step 1.** Step 1 (`detect_intent`) currently embeds
its input independently through the same model; the pipeline should
cache that embedding once per window and let Step 4 reuse it
instead of re-embedding. Not yet wired — see Step 1 note above.

**Step 5 — Route Regions.** **Predictive prefilter, not authoritative
placement.** Step 5's job is to identify which regions are active for
*this tick* so Step 6's extractor can warm-bias its label set; the
substrate's authoritative answer about where new elements belong is
computed in Step 8 from each element's own persistent inline embedding
(see "Step 8" below). Two phases of the same DAG, different inputs,
different jobs:

```text
phase           step  input                           job                   persistence
predictive      5     window's ephemeral embedding    bias Step 6 labels    discarded after tick
authoritative   8     each minted element's inline    write member_of +     persistent in substrate
                      embedding (from `names`)        k-means prototype
                                                       updates
```

Step 5 exists to break a chicken-and-egg: extraction wants warm-bias,
warm-bias wants active regions, active regions normally want minted
elements, minting wants extraction. The window embedding is a ~5 ms
semantic prefilter that captures the input's gestalt (verb shape,
from/to construction, change-vs-state cue — things that don't reduce
to any single extracted element) and lets extraction proceed with a
correctly tuned label set on its first pass.

Mechanics: read-only DAG descent — no model, just cosine similarity
over already-computed vectors. Starting from GENESIS, walk
`region_children[current]` to enumerate candidates; for each
candidate region, look up its prototype Elements via
`region_prototypes[region]` and read each prototype Element's
`embedding` field directly (FP32 inline). Score = max cosine over
the region's prototypes; descend into the top-k children whose
score exceeds `policy.descend_threshold`. Stop when no child
exceeds the threshold (leaf reached) or when no child clears
`policy.leaf_vigilance` (sub-threshold input → routed to VOID).
Each comparison is O(prototypes-in-region); the prototype set is
kept small by `policy.merge_threshold` (collapses near-duplicates)
and `policy.split_variance` (splits high-scatter regions).
Parallelizes across windows via `par_iter`. Returns `(Vec<ActiveRegion>, RegionDelta)`;
the `RegionDelta` is **held** until Step 8.

**Step 6 — Run Extractors.** Run **per window** (Step 3). For
single-window inputs (the common case), the section runs once. For
multi-window inputs, it runs N times and extractors fan out across
windows via `rayon::par_iter`. Within each window the extractor sees
the entire window at once; sentence boundaries inside a window are
not consulted. Four extractors run sequentially within the step, but
the step itself is the long pole:

- **NER.** `gline-rs` running GLiNER1 NER on `ort` — INT8 zero-shot
  span tagging. Labels passed in are the seed kinds (`person`,
  `org`, `place`, `weekday`, `quantity`, `event`, ...). Returns
  `(span, kind, confidence)` triples. Each tagged span auto-emits
  `(span_element, instance_of, K)` Defeasible (or Entailed when
  confidence ≥ `policy.ner_assertion_threshold`).
- **Temporal parser.** `chrono` + `chrono-english` for date /
  weekday / duration spans. Each match becomes a value-Element
  with the surface form as a name (parses again on comparison;
  §7.3); the parser's confidence rides into the resulting
  `(R, valid_from, T)` / `(R, valid_to, T)` meta-relations.
- **Zero-shot relation extraction.** `gline-rs` / `gliner2` on
  `ort` — INT8 zero-shot RE. ~130–208 ms per call across 5–50
  candidate attribute-name labels. Label set comes from (1) seed-pack
  canonical attribute names always, plus (2) "warm" attribute names
  whose `MemoryStats.activation` is above a floor — biased toward
  attribute names whose participants live in the active regions
  returned by Step 5. Returns `(subj_span, attr_label, obj_span,
  confidence)` quads. **★ This call is the v0 latency floor and
  does not parallelize.**
- **Heuristic coref.** Pure Rust, recency-based — no model.
  Pronouns (he / she / it / they / this / that) and definite
  descriptions ("the dentist") resolve to the most-recently-focused
  `RecentFocusEntry` whose attribute name matches the span's
  grammatical slot (Centering Theory + Hobbs' algorithm baselines).

**Attribute-name label resolution.** Each extractor proposal arrives as
`(subj_span, attr_label, obj_span, confidence)`. The pipeline:

1. **Exact-match tantivy lookup** (BM25 (Best Match 25) index over element
   `names`). On hit, reuse the attribute-name Element.
2. On miss: **embed `attr_label` with the MiniLM embedder** and run a
   universal cosine search across **all** attribute-name elements
   (not just warm ones — synonyms might be cold). On any hit ≥
   `policy.attribute_name_dedup_threshold` (0.85), reuse the top hit
   and mark the resulting relation `Defeasible` (alias mismatch).
3. On miss: **mint** a new attribute-name Element with the label as
   its name and an inline embedding. Every relation using it this
   tick is `Defeasible` until replay confirms (≥ N independent ticks
   in a window) or prunes.

Tick mints exceeding `policy.attribute_name_mint_warning_count` (5)
flag the tick for priority replay-dedup.

**Step 7 — Coreference Scoring.** Pure Rust scorer — no model.
Operates on **entity-mention spans returned by Step 6's NER and
relation extractor** (pronouns, definite descriptions, partial names,
freshly-tagged entities) — not on Step 3's windows. For each ambiguous
span, build the candidate set from `recent_focus` (working memory) plus
elements in the active regions' `region_members[R]` neighborhood.
Score each candidate:

```text
score(span, candidate) =
    name_overlap            // edit distance / lemma match, 0..1
  + embedding_similarity    // cosine of span_emb vs candidate.embedding
  + frame_overlap           // 1.0 same frame, 0.5 adjacent
  + attribute_overlap       // 1.0 if RecentFocusEntry's `attribute`
                            //   matches the span's grammatical slot
  + temporal_compatibility  // 1.0 if no valid-time conflict
  + relation_support        // 0..1, fraction of candidate's relations
                            //   consistent with span's neighborhood
  - contradiction_penalty   // 1.5 if a Superseded relation would re-fire
  - distinct_instance_penalty   // pattern-separation dampener
```

Argmax wins if it clears the merge threshold; otherwise mint a
provisional instance and let replay decide.

**Step 8 — Apply Region Delta.** First mutation step — no model.
**Authoritative phase of region routing** (Step 5 was the
predictive prefilter; see its note above). Step 5 used the window's
ephemeral embedding to identify active regions for biasing
extraction; Step 8 uses each minted element's persistent inline
embedding to update the substrate's belief about where elements
belong. After this step, region membership is the DAG's source of
truth.

The held `RegionDelta` from Step 5 commits:

- **Parent attachments.** Each `(child, parent, weight)` writes (or
  reinforces) a `(child, parent_region, parent)` Relation with
  `stats.confidence = weight`.
- **Prototype updates.** Each `(prototype_element, new_embedding)`
  overwrites the prototype Element's inline `embedding` field via
  the spherical k-means rule:
  `new = normalize(old + lr · (target_embedding − old))`
  where `lr` is the prototype's plasticity, scaled by intent.
- **New regions.** Each `NewRegion` mints a region Element + a
  prototype Element holding the initial vector, plus the seed
  structural relations (`instance_of REGION_CLASS`,
  `parent_region`, `prototype`). Mid-path insertions (§10.3.5)
  write the `parent_region` relation as `Defeasible` until replay
  confirms.
- **New members.** Each `(member, region)` writes
  `(member, member_of, region)`.

The region indices (`region_parents`, `region_children`,
`region_lateral`, `region_prototypes`, `region_members`) update
incrementally as the relations land.

**Step 9 — Build Relations and Events.** No model — pure HashMap
inserts + index updates. Each surviving extractor proposal becomes
a Relation whose **attribute list** is assembled from the
extractor's emitted slots. For a binary triple
`(subj_span, attr_label, obj_span)` the resulting relation has two
attributes: one binding the head Element under a participant
attribute name (default `subject`, or a frame-specific slot like
`actor` for animate event participants — both seeded in §16.3 with
no structural privilege), and one binding the object Element under
the attribute name resolved from `attr_label`. For n-ary events the
attribute list grows: a reschedule event becomes one Relation with
`[target: appointment_1, property: date, from: Tuesday, to: Friday]`.

Per relation:

- `status` set from confidence vs `policy.ner_assertion_threshold`
  (Entailed / Defeasible).
- `stats.confidence` initialized from
  `policy.default_conf` (intent-modulated) × extractor confidence.
- A separate `[target: R, source: source_id]` meta-relation is
  written if `tick`'s `source` parameter is `Some`.

`relations_by_element` / `relations_by_attribute_name` /
`meta_relations_by_subject` / `meta_relations_by_object` /
`attribute_value_counts` / `attribute_co_counts` /
`meta_relation_presence` all update incrementally — one HashMap
insert per (relation × attribute) pair per index.

Build compact base relations only; entailment closure is computed
on demand (§14.5).

**Step 10 — Supersession and Cache.** No model — index lookups +
status flips. For each new event-shaped relation (Event Calculus
fluent update, §14.4) whose attribute list includes `target`,
`property`, `from`, and `to`:

1. Look up prior cache relations for the same target+property pair
   via `relations_by_element[target]` filtered to entries whose
   attribute list contains both `target` (with this value) and
   `property` (matching the event's `property` value).
2. Mark each prior cache `Superseded` (status flip in place; no
   delete).
3. Write the new cache relation `R_new` with
   `MemoryStats.confidence` from the event.
4. Write the linking meta-relations (themselves Relations whose
   attributes target `R_new`): one with `[target: R_new,
   derived_from: event]` and one per superseded cache with
   `[target: R_new, supersedes: R_old]`.

`meta_relations_by_subject` / `meta_relations_by_object` indices
update for each meta-relation. Forward chain walks
("what's the current state?") read
`meta_relations_by_subject[R]` filtered to entries with a
`supersedes` attribute; inverse walks ("what supersedes R?") read
`meta_relations_by_object[R]` with the same filter. Each hop is
one HashMap lookup + a 0–3-element scan.

**Step 11 — Hebbian + Salience.** Pure arithmetic over
`MemoryStats` — no model. Two updates:

*Hebbian co-activation.* For every pair (A, B) of elements that
co-occurred in the focus set this tick, walk to their connecting
relation R and bump `R.stats.activation` via the bounded Oja rule
(§14.9):
`new = old + rate · (1 − old)` where `rate = policy.hebbian_rate ·
intent.plasticity_multiplier`. Asymptotes to 1.0; never overshoots.

*Salience scoring.* For each relation R produced or reinforced
this tick:

```text
score_salience(R, p) =
    p.salience_floor
  + 1.0 if R has exact-value attribute (date-named, number-named, named entity)
  + 1.0 if intent ∈ {Correction, TemporalUpdate}
  + 1.0 if R produced by supersession this tick
  + 0.5 if R is user-stated preference (FRAME_USER + preference-shaped attribute name)
  + 0.5 if focus-bearing on this tick

bump = score * p.salience_multiplier
R.stats.salience = bounded_hebbian_bump(R.stats.salience, bump * p.hebbian_rate)
```

**Defeasible → Asserted promotion gate** (all three required):
1. `support_count >= policy.promotion_min_count` (3).
2. `support_diversity >= policy.promotion_min_diversity` (2) —
   measured across distinct `(R, source, S)` source elements,
   `Intent` regions (high-conviction-statement vs curiosity
   vs high-prediction-error), and `active_frame` scopes — *and*
   topologically distinct in the source DAG (replay-maintained
   `derived_from` annotation; full doc §11.11 + §14.8). Three
   rephrasings of the same wrong claim from one source / weight /
   frame don't clear the bar; nor do nominally-distinct sources
   that all trace back to the same root event.
3. No contradicting relation written within the window
   (one `meta_relations_by_object[R]` lookup + `supersedes`
   filter).

Diversity check distinguishes "repeated assertion" from "converging
evidence"; topological independence distinguishes "converging
evidence" from "echo chamber." Pearl independence (full doc §6 (8)).

**Step 12 — Focus-Radius Decay.** No model — bounded BFS (Breadth-First Search) + scalar
multiplies. Walk outward from the focus set up to
`policy.focus_decay_radius` hops via `relations_by_element`. For
each element/relation reached, decay `activation` via
`bounded_hebbian_decay`:
`new = old · (1 − rate · (1 − normalize(utility)))` where utility
is the §14.7 score (focus_success + support_count + salience −
noise_score − redundancy − age_without_access). High-utility
relations decay slowly; sub-radius low-utility ones decay quickly.

Everything outside the radius is decayed by the **background sweep**
(§14.7) scheduled by `enqueue_replay`. The sweep runs in the replay
thread, computes a delta against a snapshot, and the next tick
applies it under `&mut`.

**Step 13 — Assemble Attention Frame.** Most fields are not
*computed* in Step 13; they are *gathered* from per-tick buffers
that earlier steps populated as a side effect of doing their own
work. Step 13's own work is (a) the `focused_relations` RRF and
(b) `next_actions` suggestions.

```rust
struct ConsciousAttentionFrame {
    tick: Tick,
    input: InputEcho,
    intent: Intent,                  // 4-dim vector from Step 1
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

Field-by-field, with the step that produces each field's contents:

| Field | Step | How |
|---|---|---|
| `tick` | 13 | Read `hg.clock`. |
| `input` | 0 capture / 13 return | Echo of the input text; not durable. |
| `intent` | 1 | Per-dim logistic-regression classifier over `embedding ++ lexical_features` (418 dims), trained build-time from `seed_pack.yaml`'s `intent_prototypes`. |
| `active_frame` | 5 | Inherited from `recent_focus` or set by a frame-shifting cue. |
| `active_regions` | 5 | Union of per-window `route_regions(...)` results. |
| `focused_relations` | 13 | RRF over Dense (path-reinforced focus set) + Sparse (tantivy BM25) + Path-reinforced (focus_success bumps from Step 11). RRF (Cormack et al. 2009) merges ranked lists by `Σ 1 / (60 + rankᵢ)` — keeps ranks, discards incompatibly-scaled raw scores. |
| `supporting_claims` | 13 | For each focused R: `meta_relations_by_subject[R]` filtered to `derived_from` / `source` attributes. |
| `history` | 13 | For each focused R: walk `meta_relations_by_subject[R]` filtered to `supersedes`; collect the chain. |
| `uncertainty` | 5, 6, 7, 9, 10 | Each step pushes its detected signals (`DiffuseRouting`, `UngroundedTime`, `AmbiguousCoref`, `LowConfidence`, `Contradiction`) into a per-tick buffer; Step 13 collects. |
| `durable_writes` | 8, 9 | Each mint records the new `ElementId` into a per-tick write buffer. |
| `superseded` | 10 | Each `Superseded`-status flip records the affected `RelationId`. |
| `next_actions` | 13 | Inspect the assembled frame; emit `EnqueueReplay { kind }` and `FollowUpQuery(text)` advisories where appropriate. |

Status filtering at assembly: `Asserted` + `Entailed` in
`focused_relations` by default; `Defeasible` flagged with
`is_defeasible = true` and lower base weight; `Superseded` lands in
`history`, never in `focused_relations`; `Retracted` excluded from
both.

`durable_writes` and `superseded` overlap with `focused_relations` /
`history` in ID space — they exist as flat-list shortcuts for "what
did this tick change?" inspection without walking the fused
ranking or the supersession chains.

The frame is a post-tick snapshot of the focused subgraph, **not a
pre-assembled answer**. The calling LLM derives any natural-language
response from the structural content.

---

## 6. Hard Invariants

A v0 build that violates any of these is wrong.

1. **The hypergraph is Legend's model of its world.** WAL is for
   crash recovery between checkpoints; the hot path reads from the
   hypergraph alone.
2. **Inputs distill into elements + relations.** Memory is the
   distilled relations themselves; raw text and per-input audit
   records live in dev tooling only.
3. **Every learned abstraction points back to ancestry** via
   `derived_from`, `supersedes`, or extractor lineage.
4. **Hot-path branching uses recognition indices, payload-table
   membership, `RelationStatus`, and meta-relation indices.** Element
   name strings are touched only at the boundary (lexical lookup,
   embedding match, attribute-name mint).
5. **Type discrimination is relational and structural** — read from
   recognition indices, not from a stored field.
6. **Bitemporal split.** `Tick` is transaction time; valid time
   lives on `(R, valid_from, T)` / `(R, valid_to, T)` meta-relations.
7. **Status distinctions are mechanical and durable.** Asserted,
   Entailed, Defeasible, Superseded, and Retracted remain distinct
   in the substrate.
8. **Vector closeness may merge regions; facts, instances, and events
   never merge destructively.**
9. **Cache relations carry `derived_from`.** Every cache relation
   points at the event element that produced it.
10. **External source pointers live on the `(R, source, S)`
    meta-relation.** The pointer is the record; source text lives
    in the source's own system.
11. **Path-aware reinforcement.** Focus success bumps the exact path
    that produced the focused subgraph.
12. **Decay weakens; deletion is rare.** When usefulness is uncertain,
    let memory dynamics handle it.
13. **Compression is focus-preserving.** Replay may consolidate while
    keeping focus-bearing subgraphs intact.
14. **One input operation: `tick`.** Retrieval shares the same code
    path as writes; differential, via path traversal with
    reinforcement.
15. **Provenance cycles are resolved by replay** (lowest-confidence
    relation in the cycle flips to `Retracted`); the hot path does
    not enforce write-time acyclicity.

---

## 7. Brain Processes (Pure Functions)

Each brain region is a stateless function over the hypergraph; no
region holds memory of its own. The biological names are evocative
shorthand for what the function does — not module boundaries.

```text
brain region        function                          fires in
────────────────────────────────────────────────────────────────────
Thalamus            route_regions, apply_region_delta Step 5, 8
Hippocampus         (embedded), reinforce_path        Step 9-10, 11
Dentate Gyrus       separate_pattern                  Step 7
Amygdala            score_salience                    Step 11
Prefrontal Cortex   detect_intent, adjust_policy      Step 1, 2
Wernicke            run_extractors                    Step 6
Basal Ganglia       reinforce_path, decay_step        Step 11, 12
Entorhinal          window, embed                     Step 3, 4
```

§5 is the authoritative call sequence; this table is the inverse
view — which region wakes up when.

---

## 8. Replay (Background Thread)

Replay receives a `Hypergraph` snapshot, computes a
`Vec<ReplayMutation>`, and sends it back over a channel; the main
thread applies the mutations under `&mut` at the next tick boundary.

Jobs:
- Background full-graph decay sweep (utility-modulated).
- Region split (high variance) / merge (low variance, no Inv 8 violation).
- **Mid-path insertion resolution** — confirm / re-parent across
  subtrees / retract Defeasible `parent_region` relations from
  §10.3.5. Gates: `policy.midpath_confirm_gap` (0.05) + ≥
  `policy.midpath_confirm_evidence` (3) ticks of routing-against;
  reparent_gap (0.10) wider to prevent flapping.
- **Provenance cycle resolution** — retract lowest-confidence in cycle.
- **Attribute-name dedup (cleanup-only)** — Step 6 mint-time dedup
  is primary; replay catches embedding drift. Priority-bumped for
  warning-flagged ticks.
- Coref resolution, redundant-relation compaction, derived-relation
  materialization/demotion, prototype eviction (region > 8).

**Replay safety checks** (local, structural, cheap):
- Inv 8 — region merges preserve distinct facts/instances/events.
- Inv 9 — cache relations carry `derived_from`.
- Focus-bearing protection — relations whose `focus_success_count >
  replay_focus_floor` survive compression.
- Cycle resolution preserves connectivity.

§19 + §20.5 run as CI (Continuous Integration) gates, separate from the replay loop.

---

## 9. Policy

```rust
struct Policy {
    // Region routing
    descend_threshold: f32,
    leaf_vigilance: f32,                // absolute, intent-driven
    merge_threshold: f32,
    split_variance: f32,
    void_threshold: f32,
    region_activation_threshold: f32,   // 0.55

    // Attribute-name dedup (entity-collapse threshold for
    // attribute-name elements minted by §11.7).
    attribute_name_dedup_threshold: f32,    // 0.85
    attribute_name_mint_warning_count: u32, // 5

    // Mid-path DAG insertion
    midpath_confirm_gap: f32,           // 0.05
    midpath_confirm_evidence: u32,      // 3
    midpath_reparent_gap: f32,          // 0.10

    // Defeasible → Asserted gate
    promotion_min_count: u32,           // 3
    promotion_min_diversity: u32,       // 2
    promotion_window_ticks: u32,        // 1000

    // Recognition thresholds
    concept_recognition_threshold: u32, // 3
    frame_recognition_threshold: u32,   // 5

    // Memory dynamics
    decay_rate: f32,
    salience_floor: f32,
    hebbian_rate: f32,                  // intent-modulated
    focus_decay_radius: u32,
    recent_focus_capacity: u32,         // 64
    replay_focus_floor: u32,            // 3

    // Extractor confidence threshold
    ner_assertion_threshold: f32,       // 0.7

    // Replay
    replay_cadence: ReplayCadence,
}
```

Per-tick adjusted Policy is what Steps 3–13 see; only PFC writes it.

---

## 10. Durability

**Snapshot.** Each snapshot is an LZ4-compressed MessagePack stream of
`elements` (with their inline embeddings), `relations` (including the
region structural relations), `clock`, `policy`, `stamped_at`, and
`ModelFingerprint`. The derived indices — region, meta-relation,
recognition — rebuild from the relations on load instead of being
serialized.

**WAL.** Segmented at 1 MB with a 10 MB cap and queue-style oldest-
eviction; the hot segment is LZ4-compressed, closed segments roll
over to zstd-19. The hot path never reads from it — WAL is for crash
recovery only. Dev builds can retain full input via
`LEGEND_WAL_UNBOUNDED=1`; production builds reject the flag.

**Checkpoints.** Triggered by N=1000 ticks ∨ S=5 MB WAL growth ∨
T=1 hr. Compaction is **daemon-only**: it runs on these triggers when
`legend start` is up, or on explicit `legend checkpoint`. One-shot
ticks (`legend "..."`) append to the WAL but never compact.

**Boot fingerprint check.** On startup, the snapshot's
`ModelFingerprint` is compared against the running binary's; a
mismatch refuses startup. v2 starts fresh against any directory and
leaves any v1 data files on disk untouched until the user removes
them.

**Extraction-failure quarantine (dev only).** A 100-entry in-memory
ring of `(Tick, Input, Reason)` for inputs that emitted no relations,
gated by `LEGEND_DEV_QUARANTINE=1` and compiled out of production
builds. Inspect via `legend memory show-failures`.

**Privacy / access control.** v0 operates as a single trust domain;
all consumers of a Legend instance share access. Consumers needing
separation run separate Legend instances per trust boundary.

**Embedder pin.** all-MiniLM-L6-v2 (ONNX-quantized) running through
tract-onnx, pinned for life. A model swap means re-ingesting from
`(R, source, S)` per the §15.1 recoverability matrix:

```text
source class               recoverability on swap
─────────────────────────────────────────────────────────────────
User-as-source              Recoverable by re-prompting
Git history                 Recoverable but expensive
File events                 Partial (depends on file persistence)
Slack / chat messages       Partial-to-ephemeral
Agent-internal observations Unrecoverable
```

For coding-project use, the unrecoverable share dominates over time.
Treat as load-bearing infrastructure that does not get swapped
without an explicit recovery plan.

### 10.1 Process Model

Single binary, two execution modes sharing the same `tick()` code
path and the same WAL/snapshot files:

```text
legend "..."           one-shot OR thin client to the daemon
                       (mode auto-inferred from socket presence).
legend start           start a daemon; pays cold-start once.
legend stop            graceful daemon shutdown.
legend status          daemon up? lock held? WAL size?
legend checkpoint      force snapshot compaction.
```

**Mode resolution.** `legend "..."` first tries to connect to
`~/.legend/legend.sock` (or `$XDG_RUNTIME_DIR/legend.sock`). On
success → CLI (Command-Line Interface)-client mode (request goes
over the socket). On
failure → fall through to one-shot: acquire
`~/.legend/legend.lock` via fcntl flock (block-with-2 s timeout,
retry the socket connect once mid-fallback for daemon-mid-restart),
load snapshot + replay WAL, run `tick()`, append to WAL, release.

**Concurrency invariant.** Exactly one writer mutates the hypergraph
at any moment. The daemon holds the lock for its lifetime; one-shot
ticks acquire it briefly, and two simultaneous one-shot calls
serialize on it. A stale socket (ECONNREFUSED) gets deleted by the
CLI as it falls through to one-shot mode; a stale lock needs no
handling because fcntl flock is kernel-managed and released on
holder death.

**Cost picture.**

```text
                       daemon (CLI-client)    one-shot
snapshot deserialize   0 (in memory)          ~50–200 ms
index rebuild          0                      ~10–30 ms
tract + MiniLM load    0                      ~300–500 ms
embedder warm-up       0                      ~100–200 ms
tick (§11.0)           ~200–300 ms            ~200–300 ms
IPC (Inter-Process            ~1 ms                  ~5–10 ms
  Communication) / lock
total wall-clock       ~200–300 ms            ~700 ms – 1.5 s
```

One-shot is ~3–5× slower than the daemon path but is a real tick
with the same correctness and durability guarantees. Run
`legend start` for sustained workloads; reserve `legend "..."` for
ad-hoc ticks.

---

## 11. Model Stack (v0)

Pure Rust + deterministic ONNX.

1. **`tokenizers`** (HuggingFace) — Apache-2.0, pure Rust.
2. **`tract-onnx`** (Sonos) — pure-Rust ONNX runtime; carries the
   embedder. No C++ deps, portable to any OS Rust supports.
3. **`ort`** (pyke.io) — separate ONNX runtime for the larger
   transformer extractors (GLiNER2, SaT) where tract's coverage
   isn't yet sufficient.
4. **all-MiniLM-L6-v2 (quantized)** — 384-dim embedder, ONNX-
   quantized, ~23 MB, baked into the binary, pinned for life.
5. **`tantivy`** (current stable) — BM25 lexical index.
6. **Temporal parser** — `chrono` + `chrono-english`.
7. **`gline-rs` / `gliner2`** — pure-Rust GLiNER (Generalist
   Lightweight Named Entity Recognizer) on `ort`. 130–208 ms per
   window. ★ binding latency constraint.
8. **Heuristic coref** — recency-based, written from scratch.
9. **SaT (Segment Any Text)** — ~22M-param ONNX, loaded through
   `ort`. Invoked **only** when input > ~480 tokens (§10.x Step 3).
   ~10–20 ms per call when invoked; zero overhead otherwise.

---

## 12. The Seed Pack

`seed_pack.yaml` at the repo root. ~55 elements:

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
                             (surface modals like `might` / `must` /
                              `usually` / `rescheduled` emerge via
                              `subclass_of`; `intervened` carries
                              Pearl rung-2 do() semantics — full
                              doc §6 (8))
  causal-relation (4):       caused, correlated_with, enables,
                             prevents
                             (Pearl rung-1-vs-rung-2 commitment;
                              "why" queries walk causal links only;
                              full doc §6 (8))

regions (15):             entities, events, states, change_history,
                          relationships, quantities, time, locations,
                          tasks, decisions, preferences, definitions,
                          provenance, domains, modal_negated

reference frames (8):     user, project, domain, session,
                          temporal_now, temporal_past,
                          temporal_future, meta
```

The four region structural attribute names (`member_of`,
`parent_region`, `lateral_region`, `prototype`) are seeded because
§10 expresses region topology as ordinary relations using them, and
the region indices that hot-path routing reads are derived from
them. Seed-pack regions ship with their own
`[subject: R, instance_of: REGION_CLASS]`,
`[subject: R, parent_region: parent]`, and
`[subject: R, prototype: P]` relations baked in at boot.

The "generic participant" set replaces the old 11-entry "roles"
category; predicates and roles now share one uniform attribute-name
namespace. `subject` is the catch-all head used by extractors when
no frame-specific slot fits. None of these names is structurally
privileged — recognition reads `attribute_value_counts` /
`attribute_co_counts` keyed by attribute name, not by any specific
slot.

**Seeding criterion:** load-bearing for recognition (§3.4) or v0
extraction machinery (§11.7). Domain concepts (`appointment`,
`function_definition`, `plan`) are **not** seeded — they emerge.

```text
Code owns mechanics — substrate types, the tick pipeline,
  decay/reinforcement/replay machinery, the embedding interface.
Seeds own priors — anchors, attribute names (including the 5
  behavioral modal attribute names and the 4 causal-relation
  attribute names), regions, reference frames.
Inputs own truth — Legend keeps distilled relations, not inputs.
Replay owns consolidation — region splits/merges, mid-path inserts,
  cycle resolution, attribute-name dedup, the background decay sweep.
```

---

## 13. Ten-Tick Conformance Walkthrough

Executable conformance fixture. Each tick's expected output is both
the `ConsciousAttentionFrame` and the hypergraph delta. The
inspection harness diffs both. **Substrate conformance runs against
mocked extractor outputs** so the test asserts substrate behavior
under fixed extraction; bit-identical hypergraph delta required.

### Tick 1

Input: *"My dentist appointment with Dr. Rao changed from Tuesday to Friday."*

Active regions: `change_history` 0.92, `events` 0.85, `entities` 0.81, `time` 0.78.

Delta:
```text
added elements: user, DrRao, dentist, appointment, appointment_1,
                Tuesday, Friday, reschedule_event_1
added relations:
  R1:  DrRao has_role dentist                     [Asserted]
  R2:  appointment_1 instance_of appointment      [Entailed]
  R3:  appointment_1 provider DrRao               [Asserted]
  R4:  appointment_1 participant user             [Entailed]
  R5:  reschedule_event_1 target appointment_1    [Asserted]
  R6:  reschedule_event_1 property date           [Asserted]
  R7:  reschedule_event_1 from Tuesday            [Asserted]
  R8:  reschedule_event_1 to Friday               [Asserted]
  R9:  appointment_1 current_time Friday          [Asserted]
  R10: appointment_1 old_time Tuesday             [Superseded]
added meta-relations:
  (R9,  derived_from, reschedule_event_1)         [Entailed]
  (R10, derived_from, reschedule_event_1)         [Entailed]
  (R9,  supersedes,   R10)                        [Entailed]
```

Frame: intent = Statement; focused_relations include `appointment_1
current_time Friday`, `reschedule_event_1 from Tuesday`, `to Friday`.

### Tick 2

Input: *"I have an appointment at the body shop on Tuesday."*

Reuses: user, appointment, Tuesday. Adds: `appointment_2`,
`body_shop_1`, `appointment_2 instance_of appointment`,
`appointment_2 location_or_provider body_shop_1`,
`appointment_2 current_time Tuesday`.

**Critical:** `appointment_1` and `appointment_2` stay separate.
Pattern separation fires on `provider` vs `location_or_provider`.

### Tick 3

Input: *"When is my appointment at the dentist?"*

No new elements. Reinforced path: query → appointments →
dental_appointments → DrRao → appointment_1 → current_time → Friday.

Frame: intent = Question; focused_relations: `appointment_1
current_time Friday`.

### Tick 4

Input: *"What do I have on Tuesday?"*

No new elements. Reinforced path: query → Tuesday → [filter current]
→ appointment_2.

Frame: focused_relations: `appointment_2 current_time Tuesday`,
`appointment_2 location_or_provider body_shop_1`. History:
`appointment_1 old_time Tuesday [Superseded]`.

### Tick 5

Input: *"Actually, the dentist moved it again to Monday."*

Coref: "it" → `appointment_1` (most-recently-focused with dentist
context — `RecentFocusEntry.attribute` filtering).

Adds: Monday, reschedule_event_2, `R20: appointment_1 current_time
Monday [Asserted]`, `R21: appointment_1 previous_time Friday
[Superseded]`. Meta-relations: `(R20, derived_from, reschedule_event_2)`,
`(R20, supersedes, R9)`.

Active regions: `change_history` 0.94 (stronger than Tick 1 — path
reinforcement). Frame intent = Correction.

### Tick 6

Input: *"When is my appointment with Dr. Rao now?"*

No new elements. Frame: focused_relations: `appointment_1
current_time Monday`, `appointment_1 provider DrRao`. History:
`old_time Tuesday`, `previous_time Friday`.

### Tick 7

Input: *"The body shop appointment is for an oil leak."*

Coref: "the body shop appointment" → `appointment_2`. Adds:
`oil_leak`, `appointment_2 purpose oil_leak [Asserted]`.

### Tick 8

Input: *"Why am I going to the body shop?"*

Reinforced path: query → body_shop → appointment_2 → purpose →
oil_leak. Frame: focused_relations: `appointment_2 purpose oil_leak`,
`appointment_2 location_or_provider body_shop_1`.

### Tick 9

Input: *"Dr. Rao is my dentist."*

Matched existing DrRao. **Reinforced** `DrRao has_role dentist`
(focus_success_count + confidence). Adds entailed `user has_dentist
DrRao`.

### Tick 10

Input: *"What appointments do I have?"*

Aggregate focus walks `appointment instance_of` cone with
non-superseded `current_time` relations. Frame:

```text
focused_relations:
  appointment_1 current_time Monday
  appointment_1 provider DrRao
  appointment_2 current_time Tuesday
  appointment_2 purpose oil_leak
  appointment_2 location_or_provider body_shop_1
```

### Companion fixtures (§20.5)

Three more, ~15 min each in this format:
1. **Two Sarahs** — instance separation on identical names, divergent
   attributes.
2. **Forgotten correction** — three reschedules over 20 ticks of
   unrelated content; current state must reflect the third.
3. **Frame drift** — Tuesday-this-week vs Tuesday-past-week; active
   frame must switch.

Plus a non-appointment fixture (codebase rename or chat preference
shift) so domain neutrality is visible early.

---

## 14. Build Order

Solo coder with Claude as reviewer. Every step's done-criterion is
the inspection-harness diff.

**Conformance discipline:** Substrate conformance runs against
mocked extractor outputs and asserts bit-identical hypergraph deltas.
Full-stack smoke tests run on pinned CI hardware (linux x86_64
AVX2 (Advanced Vector Extensions 2), INT8) with ε-tolerance on confidence values; cross-machine
determinism is a separate concern and lives at the smoke-test tier.

| Step | Build | Done-criterion | Time |
|---|---|---|---|
| 0 | Crates + harness + BGE round-trip | Embedding parity; harness prints region creation rate | ~1 wk |
| 1 | §7 + §9 substrate types + indices | 50-element round-trip; supersession chain walks both directions; debug-asserts on Inv 9 | ~2 wk |
| 2 | Snapshot + bounded WAL | Crash mid-corpus → restart → state matches; fingerprint check refuses on mismatch | ~1 wk |
| 2.5 | CLI front-end + IPC + lock (§10.1) | `legend "..."` works in one-shot mode (cold-start ≤ 1.5 s); `legend start` brings up daemon; subsequent `legend "..."` lands in CLI-client mode at §11.0 latency; concurrent calls serialize on lock; stale socket from `kill -9 <daemon>` is cleaned up by next CLI call | ~1 wk |
| 3 | Seed pack | ~55 elements boot in expected configuration; seeded `parent_region` / `prototype` relations populate the region indices | ~1.5 wk |
| 4 | Manual conformance set: §19 + §20.5 + non-appointment fixture (mocked extractors) | All four fixtures pass via direct add_element/add_relation | ~1 wk |
| 5 | Embeddings + region routing | Spans land in expected regions; multi-prototype ≤ 8; creation rate decays | ~1.5 wk |
| 6 | Windowing + temporal parser + NER + RE | Tick 1 (single-window) emits `Tuesday`, `Friday`, `DrRao`, reschedule triple without hand-coding; multi-paragraph synthetic input above the token threshold routes through SaT, produces N windows, yields a relation set matching its single-window equivalent; chat-message-sized inputs skip SaT (no model invocation in the per-step trace) | ~2.5 wk |
| 7 | Event reification + supersession cache | Ticks 1/2/5/7 build correct events; Tuesday→Friday→Monday chain | ~1.5 wk |
| 8 | Heuristic coref + conservative instances | Tick 5 "it" → appointment_1; appointments stay separate; Tick 9 reinforces | ~1 wk |
| 9 | Lexical index + hybrid retrieval (RRF) | Rare proper nouns retrieve correctly | ~1 wk |
| 10 | Hebbian + salience + decay + path-aware reinforcement | 100-tick corpus: paths strengthen, unused links decay, focus-bearing relations survive | ~1.5 wk |
| 11 | Replay + determinism fixture | 100-tick passes §19 + §20.5; mid-path insertion fires; cycle retraction; **two replay passes with shuffled rule order produce bit-identical state** | ~2 wk |
| 12 | External benchmarks (LongMemEval, MemoryAgentBench, RULER) | Credible numbers logged | ~2 wk |
| 13 | Reference frontend: notes app | Multi-day notes session + coding-project session both exercise Legend end-to-end | ~1 wk |

**v0 sign-off** = Steps 0–13 + §19 deterministic + §20.5 fixtures +
non-appointment fixture + LongMemEval + MemoryAgentBench + RULER all
produce credible numbers + notes app in regular use.

**Total:** ~20 wk part-time. ~3–4 wk full-time.

---

## 15. Evaluation Gates

**Co-primary metrics** (recall + faithfulness + abstention):

1. **Relation recall@k.** Focus-bearing relation must appear in
   `focused_relations`.
2. **Update / supersession accuracy.** Focused subgraph reflects
   *current* state after correction.
3. **Abstention recall.** Empty / low-confidence frame when fact
   isn't in memory.

**v0 evaluation gates:**

- §19 ten-tick walkthrough — substrate conformance.
- LongMemEval (Wang et al., ICLR 2025) — `oracle.json` first, then
  `s_cleaned.json`. Categories Legend targets first:
  `single-session-*`, `knowledge-update`, `temporal-reasoning`,
  `*_abs`. `multi-session` aggregation lands later in v0.
- MemoryAgentBench `FactConsolidation` (HUST-AI, ICLR 2026).
- RULER MK-NIAH and MV-NIAH at 8K and 32K — embedding/routing smoke
  test.

---

## 16. Forward Pointers

This doc is the read-path. For deeper material:

| Topic | Full spec |
|---|---|
| Mathematical foundations + citations | §6, §22 |
| Concepts carried forward from Legend v1 | §17 |
| Beyond v0 (patterns, latency, INT8 stored, HNSW, etc.) | §24 |
| Deferred questions | §23 |
| Source map / reading list | §22 |
| Per-element seed pack rationales | `seed_pack.yaml` |
| Full evaluation discussion | §20 |
| Meta-relation table + index design | §7.2 |
| DDVFA / region routing details | §10, §14.1 |
| Event Calculus mapping | §14.4 |
| Bounded Hebbian operators | §14.9 |

When in doubt, the full `new_foundation.md` is the source of truth;
this doc is a compressed view of the parts an implementer needs first.
