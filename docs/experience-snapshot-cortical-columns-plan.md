# Experience Snapshots and Cortical Columns Plan

Status: planning document for a future experimental branch.

This document captures the proposed shift from Legend as a memory database with
consolidation/decay toward Legend as a brain-like continuous experience system.
The intent is to implement and test this on a separate branch before changing
the current command contract or storage format on `master`.

## Executive Summary

Legend currently behaves like a structured memory store:

- `tick` encodes text into working memory, episodic memory, and a global L3
  knowledge graph.
- `query` retrieves context from L1/L2/L3.
- consolidation, decay, salience, replay, and graph extraction make the store
  more brain-inspired, but the core product is still "write facts, retrieve
  facts."

The target shift is different:

- Legend should maintain an ongoing, attention-weighted model of experience.
- A tick should be a discrete sampled moment of that stream.
- The tick response should be an experience snapshot: what Legend currently
  perceives, what it is attending to, what columns/models are active, what they
  vote for, what conflicts or uncertainties exist, and what response/action is
  optimal.
- Durable memory writes become one side effect of experience processing, not the
  whole operation.

Recent neuroscience makes the target more specific:

- response selection should be distributed across active columns, not centralized
  in one query/decision module,
- attention should route through column subspaces/facets, not just choose whole
  graph nodes,
- uncertainty should be classified by source before deciding whether to answer,
  inspect, ask, revise, or explore,
- replay/consolidation should have separate maintenance modes for recent
  experiences, older memories, schema/gist extraction, and contradiction repair,
- local columns should have support stores and observability so high-capacity
  association does not become opaque cache behavior.

In this model, a query is not a separate cognitive primitive. Querying is
directed attention. A question becomes a tick whose inferred intent is
`Question`, usually with read-only durable-memory policy. `query` can remain as a
compatibility alias, but internally it should use the same experience pipeline.

## Why This Is A Big Shift

The current L3 knowledge graph is global. It stores nodes, edges, edge
semantics, support/conflict metadata, and embeddings. This is useful, but it
does not match the Thousand Brains intuition: many local models interpret
partial evidence and vote into one current perception.

The proposed shift is:

```text
Current:
  input text -> encode/retrieve -> global memory graph -> result

Target:
  repo/user/LLM event -> attention -> local cortical columns
  -> evidence/hypothesis votes -> consensus experience snapshot
  -> optional durable memory effects
```

The graph should no longer be "the brain." It should become one or more of:

- evidence inside column-local models,
- learned wiring between columns,
- an index for routing/activation,
- a compatibility projection for existing query/dump tooling.

## Research Backing

This plan is inspired by Hawkins/Numenta and the Thousand Brains Project, but it
does not attempt to simulate biology literally. The goal is to extract the
useful computational principles.

### Cortical Columns Learn Local Complete Models

Hawkins, Ahmad, and Cui (2017) argue that cortical columns can learn complete
predictive models of objects by integrating changing sensory input over time.
Each column only receives partial input at any instant, but movement lets it
observe multiple features at object-relative locations. Multiple columns can
then reach consensus faster through lateral connections.

Key principle for Legend:

- A single model should not have to own all knowledge.
- Local models can each learn "complete enough" models from their own slice of
  the input stream.
- Fast inference comes from many partial models voting, not from one global
  hierarchy finding the answer alone.

Source: Hawkins et al., "A Theory of How Columns in the Neocortex Enable
Learning the Structure of the World", Frontiers in Neural Circuits, 2017:
https://www.frontiersin.org/journals/neural-circuits/articles/10.3389/fncir.2017.00081/full

### Features Are Meaningful Inside Reference Frames

The same paper frames object knowledge as features at locations in an
object-centric reference frame. Later Numenta work generalizes this with grid
cell-inspired reference frames in the neocortex.

Key principle for Legend:

- Facts are not all globally true in one flat namespace.
- "SQLite is the datastore" is meaningful inside a project/time/branch/source
  frame.
- "The user prefers concise answers" is meaningful inside a user/agent
  interaction frame.
- "This function is risky" is meaningful inside a repo/file/task frame.

Source: Hawkins et al., "A Framework for Intelligence and Cortical Function
Based on Grid Cells in the Neocortex", Frontiers in Neural Circuits, 2018:
https://www.frontiersin.org/journals/neural-circuits/articles/10.3389/fncir.2018.00121/full

### Many Models Vote Into One Perception

Numenta's Thousand Brains explanations emphasize that the brain does not build
one model of an object or concept. It builds many models in parallel, and those
models vote together to produce the single perceived interpretation.

Key principle for Legend:

- The tick response should be a consensus perception, not a list of retrieved
  rows.
- Competing hypotheses should be allowed and visible.
- Confidence should come from evidence agreement across models, not only vector
  similarity or node weight.

Source: Numenta, "The Thousand Brains Theory of Intelligence":
https://www.numenta.com/blog/2019/01/16/the-thousand-brains-theory-of-intelligence/

### Learning Modules, Votes, and Goal States

The Thousand Brains Project's Monty architecture uses learning modules that
receive observations, maintain hypotheses, emit votes, and can output goal
states. Evidence-based learning modules share hypotheses/evidence with other
modules; multiple modules can reach decisions faster than one.

Key principle for Legend:

- Legend columns should produce three categories of output:
  - what they perceive,
  - what they vote for,
  - what goal/action they recommend.
- Vote messages should be compact and comparable across columns.
- A consensus layer should aggregate votes and decide what is experienced now.

Sources:

- Thousand Brains Project principles:
  https://thousandbrains.org/learn/thousand-brains-principles/
- Monty learning module outputs:
  https://thousandbrainsproject.readme.io/docs/learning-module-outputs
- Monty evidence-based learning module:
  https://thousandbrainsproject.readme.io/docs/evidence-based-learning-module
- Multiple learning modules:
  https://thousandbrainsproject.readme.io/docs/multiple-learning-modules

### Attention Is Not The Same As Voting

This plan treats attention and voting as related but separate.

- Attention is gating and gain control: which signals matter, which columns are
  active, how much each vote should count.
- Voting is consensus formation: active columns compare hypotheses and converge
  on the current perceived state.
- Experience is the product: the current attention-weighted consensus snapshot.

That distinction maps cleanly onto Legend's existing modules:

- thalamus: salience and attention gates,
- prefrontal/anterior PFC: current focus, plan, task, goal state,
- hippocampus: episodic traces and temporal continuity,
- neocortex: local world models and inter-column consensus.

### Recent Neuroscience Directions To Incorporate

This section captures newer research that should shape the experimental branch.
The point is not to copy anatomy. The point is to extract computational
constraints that make Legend less database-like and more experience-like.

#### Decision-Making Is Brain-Wide, Not Centralized

The International Brain Laboratory published a brain-wide decision-making map in
2025 using recordings from 621,733 neurons across 279 mouse brain areas. The
work challenges a simple sensory-to-executive-to-motor hierarchy: decision
variables, prior expectations, movement, and feedback are represented across
distributed regions.

Design implication for Legend:

- Do not create one "answer selector" that behaves like a central executive.
- Let repo, user, task, temporal, research, truth-maintenance, and self-model
  columns all vote on the current response.
- The prefrontal-like layer should arbitrate goals and policies, but the
  response should emerge from distributed column evidence.

Source: International Brain Laboratory, "A brain-wide map of neural activity
during complex behaviour", Nature, 2025:
https://www.nature.com/articles/s41586-025-09235-0

Related source: Findling et al., "Brain-wide representations of prior
information in mouse decision-making", Nature, 2025:
https://www.nature.com/articles/s41586-025-09226-1

#### Flexible Cognition Uses Multiplexed Routing Subspaces

MacDowell et al. (2025) found that different dimensions of population activity
within a region can connect to different overlapping brain-wide networks. The
active network can change moment to moment as neural activity aligns with a
particular subspace dimension.

Design implication for Legend:

- A column should not be one monolithic bucket.
- Each column should expose local subspaces/facets such as `structure`,
  `preference`, `source_authority`, `temporal_context`, `uncertainty`, and
  `goal_relevance`.
- Attention should align the current sensory frame to a column subspace, then
  weight votes from that subspace.
- This is stronger than tagging graph nodes. The selected subspace determines
  what the column is currently allowed to say and how it communicates.

Source: MacDowell et al., "Multiplexed subspaces route neural activity across
brain-wide networks", Nature Communications, 2025:
https://www.nature.com/articles/s41467-025-58698-2

#### Uncertainty Must Be Demixed Before Strategy Changes

Lam et al. (2025) found that mediodorsal thalamus can independently represent
different uncertainty sources and help prefrontal cortex reconfigure strategy
after rule changes. The useful abstraction is "uncertainty demixing": the system
should not treat all uncertainty as the same.

Design implication for Legend:

- Add explicit uncertainty-source classification.
- Different uncertainty sources should trigger different policies:
  - missing evidence -> retrieve/inspect,
  - contradiction -> preserve both claims and open truth maintenance,
  - stale model -> schedule replay/revalidation,
  - ambiguous intent -> ask or defer,
  - source conflict -> weight by authority and recency,
  - task shift -> re-route attention and update focus.
- This belongs near the thalamus/prefrontal boundary, before final voting.

Source: Lam et al., "Prefrontal transthalamic uncertainty processing drives
flexible switching", Nature, 2025:
https://www.nature.com/articles/s41586-024-08180-8

#### Sleep Replay Separates Recent And Older Memory Processing

Chang et al. (2025) found that NREM sleep microstructure can temporally
segregate replay of recent versus older memories. This supports the idea that
offline maintenance is not one generic consolidation operation.

Design implication for Legend:

- Split replay/consolidation into named maintenance modes.
- Recent tick snapshots should be stabilized differently from old memories.
- Older memories should be replayed for schema extraction, contradiction
  repair, and stale-model detection.
- Maintenance mode should be recorded in the snapshot/replay trace so future
  debugging can explain why a memory changed.

Source: Chang et al., "Sleep microstructure organizes memory replay", Nature,
2025:
https://www.nature.com/articles/s41586-024-08340-w

#### Hippocampal Replay Can Compose Reusable Building Blocks

Bakermans et al. (2025) argue that hippocampal replay can build compositional
state spaces from reusable cortical building blocks, supporting rapid
generalization in new situations.

Design implication for Legend:

- Treat the current experience as a composition of reusable primitives:
  repository object, user intent, source authority, task state, temporal frame,
  and candidate response.
- A tick should bind those primitives into a snapshot, not merely attach a new
  fact to a graph.
- Replay should build better future responses by composing known primitives into
  new task states.

Source: Bakermans et al., "Constructing future behavior in the hippocampal
formation through composition and replay", Nature Neuroscience, 2025:
https://www.nature.com/articles/s41593-025-01908-3

#### Engrams Reorganize From Precision Toward Gist

Ko et al. (2025) found that systems consolidation can reorganize hippocampal
engram circuitry so memories become less precise but more useful as generalized
gist. This is not just decay; it is transformation.

Design implication for Legend:

- Consolidation should explicitly distinguish exact episodic traces from derived
  gist.
- Gist should never erase source-backed details.
- Each generalized belief should record what evidence it compresses, what it
  omits, and what frame it applies to.
- The system should be able to answer both "what happened?" and "what pattern
  did we learn from it?"

Source: Ko et al., "Systems consolidation reorganizes hippocampal engram
circuitry", Nature, 2025:
https://www.nature.com/articles/s41586-025-08993-1

#### Astrocyte-Like Support Stores May Be Useful

Recent astrocyte research and computational modeling suggest that non-neuronal
support structures can contribute to learning and high-capacity associative
memory. Kozachkov et al. (2025) model neuron-astrocyte networks as
high-capacity associative memory systems.

Design implication for Legend:

- Add column support stores that are not themselves the column's explicit
  beliefs.
- Use them for associative lookup, evidence indexing, normalization,
  confidence calibration, and cache-like recall.
- Keep them auditable. A support store may propose evidence, but a column vote
  must cite durable evidence or mark itself as unsupported.

Source: Kozachkov, Slotine, and Krotov, "Neuron-astrocyte associative memory",
PNAS, 2025:
https://pmc.ncbi.nlm.nih.gov/articles/PMC12130835/

Related source: Williamson et al., "Learning-associated astrocyte ensembles
regulate memory recall", Nature, 2025:
https://www.nature.com/articles/s41586-024-08170-w

#### Connectomics Supports Sparse Local Modules With Dense Communication

Recent connectomics efforts provide useful engineering pressure. FlyWire mapped
the full adult fruit fly brain connectome; MICrONS released functional
connectomics spanning mouse visual cortex; BRAIN Initiative cell-atlas work
continues to emphasize heterogeneous cell types and circuits rather than one
uniform processing unit.

Design implication for Legend:

- Use sparse local columns, but make inter-column communication explicit.
- Track learned wiring strength between columns when they co-activate or
  co-vote.
- Add "connectome observability": for any snapshot, show active columns,
  subspaces, vote paths, conflicts, and memory effects.
- Allow heterogeneous column policies. A repo-structure column, user-preference
  column, and external-research column should not learn the same way.

Sources:

- MICrONS Consortium, "Functional connectomics spanning multiple areas of mouse
  visual cortex", Nature, 2025:
  https://www.nature.com/articles/s41586-025-08790-w
- Dorkenwald et al., "Neuronal wiring diagram of an adult brain", Nature, 2024:
  https://www.nature.com/articles/s41586-024-07558-y
- BRAIN Initiative Cell Atlas Network:
  https://www.braininitiative.nih.gov/research/tools-and-technologies-brain-cells-and-circuits/brain-initiative-cell-atlas-network

#### Predictive Coding Adds Confidence And Error Calibration

Predictive coding work remains useful because it gives a clear loop: predict,
observe, compute error, update confidence. Newer work extends this to temporal
hierarchies and confidence errors.

Design implication for Legend:

- Each active column should predict what evidence or task state it expects next.
- A tick should record prediction error, not just salience.
- Confidence should be learned from performance. A column that repeatedly votes
  confidently and gets corrected should lose reliability in that frame.
- The snapshot should distinguish "low confidence because evidence is missing"
  from "low confidence because the column is historically unreliable here."

Sources:

- Jiang and Rao, "Dynamic predictive coding: A model of hierarchical sequence
  learning and prediction in the neocortex", PLOS Computational Biology, 2024:
  https://journals.plos.org/ploscompbiol/article?id=10.1371%2Fjournal.pcbi.1011801
- Granier et al., "Confidence and second-order errors in cortical circuits",
  PNAS Nexus, 2024:
  https://academic.oup.com/pnasnexus/article/3/9/pgae404/7756710

## Current Legend Baseline

The relevant current code paths are:

- `src/commands/memory/tick.rs`: CLI tick wrapper. It loads memory, handles
  keyword directives, calls `crate::memory::tick`, saves, logs, and prints
  `{action, entry_id}`.
- `src/tool/mod.rs`: tool-layer `tick` wrapper. It calls `tick_impl` and appends
  to the session log.
- `src/memory/mod.rs::tick_impl`: the core write path. It increments the clock,
  applies decay, chunks/embeds input, computes salience/emotional valence,
  routes through L1/L2, updates the global L3 graph, runs encoding activation,
  prunes, replays, and maybe consolidates.
- `src/memory/mod.rs::encoding_activation`: tick-time retrieval-like activation.
  It is already close to an experience step because it activates related L2/L3
  context without full query side effects.
- `src/commands/memory/query.rs`: CLI query wrapper. It calls
  `retrieve_context_with_mode(..., RetrievalMode::ReadOnly)`.
- `src/memory/mod.rs::retrieve_context_with_mode`: retrieval path with
  `ReadOnly` and `RecallStudy` modes.
- `src/memory/neocortex.rs`: global `GraphMemory`, graph update, spreading
  activation, edge semantics, reference frames, replay/consolidation helpers.

Important current strengths:

- There is already a monotonic cognitive clock.
- There is already salience, neurochemistry, decay, replay, and consolidation.
- `query` is already read-only by default.
- `encoding_activation` already separates tick-time activation from query-time
  retrieval.
- `GraphEdgeSemantics` already carries evidence, support, contradiction,
  correction, and reference-frame metadata.
- Plans are already separated into anterior PFC instead of polluting L1/L2/L3.

Important current gaps:

- L3 is one dense global graph, not many local models.
- Reference frames annotate edges but do not route computation.
- The tick response is operational (`entry_id`, action), not experiential.
- Query is still a separate user-facing primitive and output shape.
- Column-like consensus/voting does not exist.
- Attention gates memory promotion, but not yet column selection and vote
  weighting.
- The graph can be dominated by hub terms and high-degree generic concepts.
- The MCP query tool description is stale: it says query auto-reinforces, while
  the implementation uses `RetrievalMode::ReadOnly`.

## Biological Scale And Engineering Scale

The human neocortex has many columns and many neurons per column, but exact
counts vary by terminology and source. Hawkins/Numenta often discuss roughly
150k cortical columns; other literature distinguishes minicolumns from cortical
columns and gives different counts. This project should not copy a biological
count.

Legend has fewer sensor streams than a human:

- repo state: code, docs, images, configs, git history, diagnostics,
- user input: prompt text, preferences, decisions, corrections,
- LLM output: assistant reasoning/results/ticks, tool activity summaries,
- internal state: plans, current focus, prior experience snapshot, active
  hypotheses.

Because sensors are narrower, Legend can start with far fewer columns, then let
columns specialize and split over time.

Initial experimental scale:

- 16-64 columns for one repo.
- 4-8 core column families.
- Hard cap per column for local evidence/hypotheses.
- Explicit metrics before scaling.

## Core Concept: Tick As Experience Snapshot

A tick should represent a sampled moment of experience.

The tick input is whatever the LLM provides, but the tick event should also be
able to include current repo/task/session context when available.

Proposed tick output:

```json
{
  "action": "experienced",
  "entry_id": 12345,
  "snapshot": {
    "clock": 311,
    "intent": "Question",
    "focus": "rethinking Legend architecture",
    "attended_inputs": [
      "user message",
      "recent session context",
      "active work queue"
    ],
    "active_columns": [
      {
        "id": "architecture.legend-memory",
        "confidence": 0.92,
        "role": "primary"
      },
      {
        "id": "research.thousand-brains",
        "confidence": 0.87,
        "role": "supporting"
      }
    ],
    "consensus": "Legend should shift from database-like memory retrieval to an attention-weighted continuous experience system.",
    "hypotheses": [
      {
        "statement": "Graph nodes should become local evidence inside columns.",
        "confidence": 0.82
      },
      {
        "statement": "Graph nodes should be grouped by columns but remain globally addressable.",
        "confidence": 0.61
      }
    ],
    "conflicts": [
      {
        "statement": "MCP query description conflicts with read-only implementation.",
        "severity": "low"
      }
    ],
    "uncertainty_sources": [
      {
        "kind": "SourceConflict",
        "confidence": 0.71,
        "policy": "preserve_both_and_weight_by_authority"
      }
    ],
    "routing_trace": [
      {
        "column": "architecture.legend-memory",
        "subspace": "truth_maintenance",
        "reason": "correction and stale MCP description"
      }
    ],
    "recommended_response": "Create a branch plan before changing storage.",
    "memory_effects": [
      "stored_architecture_decision",
      "updated_column_evidence"
    ]
  }
}
```

This snapshot becomes the user/LLM-facing return value. The existing
`{action, entry_id}` can remain as a compatibility wrapper or a compact view.

## Proposed Data Model

### SensoryFrame

Represents the current input event.

```rust
pub struct SensoryFrame {
    pub id: u64,
    pub clock: u64,
    pub source: SensorSource,
    pub raw_text: String,
    pub chunks: Vec<String>,
    pub embedding: Vec<f32>,
    pub entities: Vec<ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
    pub refs: Vec<MemoryRef>,
    pub repo_context: Option<RepoContext>,
    pub llm_context: Option<LlmContext>,
    pub user_context: Option<UserContext>,
}
```

`SensorSource` should include at least:

- `Repo`
- `User`
- `Llm`
- `Internal`
- `Tool`

### ExperienceIntent

Classifies what the event is doing.

```rust
pub enum ExperienceIntent {
    Assertion,
    Question,
    Correction,
    Feedback,
    PlanUpdate,
    ToolObservation,
    RepoObservation,
    Reflection,
}
```

This replaces the command-level distinction between tick and query as the
primary cognitive branch. Commands can still exist as wrappers.

### UncertaintySource

Classifies why the system is unsure. This is the thalamic demixing piece.

```rust
pub enum UncertaintySource {
    MissingEvidence,
    Contradiction,
    StaleModel,
    AmbiguousIntent,
    SourceConflict,
    PredictionError,
    TaskShift,
    LowColumnReliability,
}
```

Uncertainty classification should happen before final consensus. The response
policy depends on the class:

- `MissingEvidence`: inspect, retrieve, or ask for repo context.
- `Contradiction`: preserve both claims and open truth maintenance.
- `StaleModel`: schedule replay/revalidation.
- `AmbiguousIntent`: ask or make the ambiguity visible in the snapshot.
- `SourceConflict`: weight by authority, recency, and correction history.
- `PredictionError`: lower confidence and update the relevant column model.
- `TaskShift`: re-route attention and update active focus.
- `LowColumnReliability`: downweight that column in this frame.

### AttentionState

Tracks current focus and gain.

```rust
pub struct AttentionState {
    pub focus_text: String,
    pub active_task_id: Option<u64>,
    pub active_plan_id: Option<u64>,
    pub salience: f32,
    pub novelty: f32,
    pub uncertainty: f32,
    pub uncertainty_sources: Vec<UncertaintyEstimate>,
    pub conflict_pressure: f32,
    pub routing_subspaces: Vec<RoutingSubspace>,
    pub selected_columns: Vec<ColumnActivation>,
}
```

Attention should consider:

- current user request,
- current plan/task,
- repo files touched,
- recent experience snapshots,
- prediction error/correction cues,
- conflict with existing column beliefs,
- high-value long-term goals.

### RoutingSubspace

Represents the selected facet/dimension of a column for the current tick. This
is inspired by multiplexed neural subspaces: the same column can communicate
different information depending on which subspace is active.

```rust
pub struct RoutingSubspace {
    pub column_id: ColumnId,
    pub subspace_id: SubspaceId,
    pub label: String,
    pub weight: f32,
    pub reason: String,
}
```

Initial subspace labels can be pragmatic:

- `structure`
- `source_authority`
- `truth_maintenance`
- `temporal_context`
- `goal_relevance`
- `user_preference`
- `uncertainty`
- `prediction`

### CorticalColumn

The new top-level neocortical unit.

```rust
pub struct CorticalColumn {
    pub id: ColumnId,
    pub kind: ColumnKind,
    pub receptive_field: ColumnSelector,
    pub reference_frame: ReferenceFrameKey,
    pub subspaces: Vec<ColumnSubspace>,
    pub local_model: ColumnModel,
    pub local_graph: GraphMemory,
    pub support_store: ColumnSupportStore,
    pub evidence_buffer: Vec<EvidenceTrace>,
    pub hypotheses: Vec<ColumnHypothesis>,
    pub reliability: f32,
    pub maintenance_policy: MaintenancePolicy,
    pub outgoing_wiring: Vec<ColumnConnection>,
    pub last_active_clock: u64,
    pub stats: ColumnStats,
}
```

Column kinds:

- `RepoStructure`
- `FileOrModule`
- `ProjectConcept`
- `UserPreference`
- `TaskPlan`
- `DecisionHistory`
- `BugIncident`
- `ExternalResearch`
- `AgentSelfModel`
- `TemporalContext`
- `TruthMaintenance`
- `UncertaintyRouter`
- `MaintenanceReplay`

The same raw fact can appear in multiple columns, but with different reference
frames and weights. That is expected. The system should prefer evidence-backed
redundancy over one brittle global representation.

### ColumnSubspace

Defines a communication facet inside a column.

```rust
pub struct ColumnSubspace {
    pub id: SubspaceId,
    pub label: String,
    pub selector: ColumnSelector,
    pub reliability: f32,
    pub last_active_clock: u64,
}
```

Subspaces prevent columns from becoming generic tags. A user-preference column
may have separate subspaces for style, tool usage, risk tolerance, and explicit
instructions. A repo column may have subspaces for architecture, build/test
workflow, dependencies, and current branch state.

### ColumnModel

Stores local learned structure. Start pragmatic.

```rust
pub struct ColumnModel {
    pub objects: HashMap<ObjectId, ObjectModel>,
    pub features: HashMap<FeatureId, Feature>,
    pub relations: Vec<LocalRelation>,
    pub embeddings: Vec<Centroid>,
    pub predictions: Vec<Prediction>,
}
```

In the first version, `local_graph: GraphMemory` can be reused as the local
feature/relation store. Later versions can replace it with a more specific
object-model representation.

### ColumnSupportStore

Astrocyte-inspired support memory for high-capacity association and
normalization. This is not the source of truth.

```rust
pub struct ColumnSupportStore {
    pub associative_index: Vec<AssociativeTrace>,
    pub evidence_index: Vec<EvidenceIndexEntry>,
    pub confidence_history: Vec<ConfidenceCalibration>,
    pub normalization_stats: SupportStats,
}
```

Rules:

- support stores can retrieve candidates,
- support stores can calibrate confidence,
- support stores can suggest maintenance,
- support stores cannot produce uncited truth claims,
- every support-derived vote must either cite durable evidence or mark itself as
  exploratory.

### MaintenanceMode

Separates online experience processing from sleep-like replay.

```rust
pub enum MaintenanceMode {
    RecentSnapshotStabilization,
    RemoteMemoryReplay,
    SchemaExtraction,
    GistCompression,
    ContradictionRepair,
    StaleModelRevalidation,
    ColumnSplitMerge,
    WiringStrengthening,
}
```

Maintenance should be schedulable by tick output but usually run outside the
hot path. The mode should be recorded for observability.

### ColumnVote

The compact communication primitive.

```rust
pub struct ColumnVote {
    pub column_id: ColumnId,
    pub subspace_id: Option<SubspaceId>,
    pub target: VoteTarget,
    pub hypothesis: String,
    pub confidence: f32,
    pub prediction_error: f32,
    pub confidence_error: Option<f32>,
    pub uncertainty_sources: Vec<UncertaintyEstimate>,
    pub evidence: Vec<EvidenceRef>,
    pub conflicts: Vec<ConflictRef>,
    pub proposed_memory_effects: Vec<MemoryEffect>,
    pub proposed_maintenance: Vec<MaintenanceMode>,
    pub proposed_goal: Option<GoalState>,
}
```

Votes should be normalized before aggregation so that columns with more stored
evidence do not automatically dominate. Monty's evidence voting normalizes
evidence for cross-module comparison; Legend should use the same principle even
if the exact math differs.

### ExperienceSnapshot

The output of every cognitive tick.

```rust
pub struct ExperienceSnapshot {
    pub clock: u64,
    pub intent: ExperienceIntent,
    pub focus: String,
    pub attended_inputs: Vec<String>,
    pub active_columns: Vec<ColumnActivation>,
    pub consensus: Vec<ConsensusItem>,
    pub hypotheses: Vec<ColumnVote>,
    pub conflicts: Vec<ConflictSummary>,
    pub uncertainty: f32,
    pub uncertainty_sources: Vec<UncertaintyEstimate>,
    pub routing_trace: Vec<RoutingTraceEntry>,
    pub maintenance_queue: Vec<MaintenanceRequest>,
    pub recommended_response: Option<String>,
    pub memory_effects: Vec<AppliedMemoryEffect>,
}
```

Snapshots should be storable as compressed episodic traces. Future starts should
surface recent snapshots because they are closer to "what was happening" than a
raw log of commands.

## Relationship To Existing L1/L2/L3

Do not delete the current memory stack immediately. Reinterpret it.

### L1 Working Memory

Current role: recent working-memory entries.

New role:

- active sensory frame,
- current experience snapshot,
- active focus,
- short-lived column activations,
- current uncertainty-source estimates,
- current routing subspaces.

### L2 Hippocampus

Current role: episodic vector store with salience, decay, usage, stability.

New role:

- event/snapshot trace store,
- temporal continuity between snapshots,
- replay source for columns,
- reconstruction when columns disagree or lack evidence,
- source of recent-vs-remote maintenance scheduling,
- storage for exact episodes before gist compression.

### L3 Neocortex

Current role: one global graph.

New role:

- many columns with local object models,
- graph projection for compatibility,
- inter-column wiring/synapses,
- stable reference-frame-local knowledge,
- heterogeneous column policies,
- column-local support stores and confidence calibration.

## Columnizing Or Replacing Graph Nodes

There are two viable paths. The experimental branch should test both lightly,
then choose.

### Option A: Columns Group Graph Nodes

Keep `GraphMemory` but add column ownership.

```rust
pub struct GraphNode {
    ...
    pub column_ids: Vec<ColumnId>,
}

pub struct GraphEdgeSemantics {
    ...
    pub column_ids: Vec<ColumnId>,
}
```

Pros:

- smaller migration,
- existing graph tests can still pass,
- easy to expose old `related_topics`.

Cons:

- global graph can keep dominating the design,
- column boundaries may become tags rather than real local models,
- hub-node problem may persist.

### Option B: Columns Replace Global Graph As Primary Storage

Move graph structures inside columns and keep a derived global index.

```rust
pub struct BrainState {
    ...
    pub columns: ColumnStore,
    pub long_term_projection: GraphMemory,
}
```

Pros:

- matches the target architecture,
- forces local reference-frame modeling,
- allows different columns to hold conflicting truths safely.

Cons:

- larger migration,
- more compatibility code,
- more test fallout.

Recommendation:

Start with Option B on the experimental branch, but reuse `GraphMemory` inside
`ColumnModel` for the first iteration. This keeps implementation cost bounded
while preventing the global graph from remaining the cognitive center.

## Proposed Tick Lifecycle

### 1. Capture Sensory Frame

Build a structured `SensoryFrame` from:

- tick text,
- source type,
- active task/plan,
- recent snapshot,
- optional repo deltas,
- extracted entities/relations/dates/refs,
- embeddings.

For the current CLI/MCP, source will usually be `Llm` or `User` depending on who
called the tick. Later hooks can pass richer source metadata.

### 2. Infer Intent

Infer `ExperienceIntent` from:

- explicit prefixes: `DECISION:`, `BUG:`, `ARCHITECTURE:`, `PLAN:`,
  `BLOCKER:`, `CORRECTION:`,
- question syntax,
- tool/repo observation markers,
- reinforcement/feedback markers,
- task/plan language.

Important policy:

- `Question` should normally not store the question as durable knowledge.
- `Question` should still update transient experience and maybe snapshot
  history.
- `Correction` should update truth/conflict state.
- `Feedback` should update reliability/reinforcement.

### 3. Compute Attention And Uncertainty

Replace "salience only gates L2 promotion" with "attention selects columns and
weights votes." At the same time, classify uncertainty by source before the
system decides how to respond.

Inputs:

- salience score,
- active plan/task,
- current focus,
- prediction error,
- conflict pressure,
- source authority,
- recent columns,
- repo files/entities touched,
- prior column reliability,
- stale-model indicators,
- ambiguity in user intent.

Output:

- selected columns,
- per-column attention weight,
- selected routing subspaces,
- global uncertainty/conflict pressure,
- one or more `UncertaintySource` values,
- a response policy hint: answer, inspect, ask, revise, explore, or schedule
  maintenance.

### 4. Route To Columns And Subspaces

Column routing should consider:

- reference frame match,
- entity/object match,
- source match,
- embedding similarity,
- plan/task match,
- recency of column activation,
- expected usefulness,
- subspace alignment,
- uncertainty-source match.

A tick should not touch every column. Start with top K columns plus mandatory
system columns:

- active task column,
- user preference column,
- agent self-model column,
- temporal context column.

Routing should produce a trace:

- why each column was selected,
- which subspace was selected,
- what input feature activated it,
- what uncertainty source, if any, caused mandatory routing.

### 5. Local Column Inference/Update

Each active column:

- compares the sensory frame against its local model,
- updates current hypotheses,
- predicts what should come next,
- detects mismatch/conflict,
- computes prediction error and confidence error,
- consults its support store for candidate evidence,
- optionally updates local evidence if policy allows,
- emits a `ColumnVote`.

For `Question`, columns should primarily infer and vote.

For `Assertion`, columns can update evidence.

For `Correction`, columns must preserve superseded facts and update conflict
state rather than deleting.

Support-store policy:

- support stores can propose related traces or candidate hypotheses,
- support-derived candidates must be marked exploratory unless backed by durable
  evidence,
- confidence calibration should update from later corrections or benchmark
  outcomes.

### 6. Lateral Voting

Columns exchange compact votes, not full local graphs.

Initial implementation:

- collect all votes centrally,
- normalize by column reliability, subspace reliability, and attention weight,
- group similar hypotheses by embedding/text/entity target,
- penalize known confidence errors,
- compute consensus confidence,
- keep dissenting hypotheses above a threshold.

Later implementation:

- add explicit column-to-column edges,
- allow neighboring columns to update each other before final consensus.
- strengthen wiring when columns co-vote accurately.
- weaken wiring when co-votes repeatedly cause corrected errors.

### 7. Build Experience Snapshot

The consensus layer creates the tick response:

- focus,
- active columns,
- consensus,
- relevant evidence,
- conflicts,
- uncertainty,
- uncertainty sources,
- routing trace,
- prediction/confidence errors,
- proposed maintenance modes,
- recommended response/action,
- memory effects.

This is the thing the LLM should read after a tick. It should feel like
"what Legend currently experiences" rather than "database write result."

### 8. Apply Durable Memory Effects

Possible effects:

- append snapshot trace to L2,
- update column-local evidence,
- update column reliability,
- update inter-column wiring,
- update plan/task state,
- update compatibility graph projection,
- schedule replay/consolidation by `MaintenanceMode`.

Maintenance should not be one generic pass. The scheduler should distinguish:

- recent snapshot stabilization,
- remote memory replay,
- schema extraction,
- gist compression,
- contradiction repair,
- stale-model revalidation,
- column split/merge,
- wiring strengthening.

### 9. Return Snapshot

CLI compact output can still print:

```json
{
  "action": "experienced",
  "entry_id": 123,
  "focus": "...",
  "consensus": "...",
  "uncertainty": 0.21
}
```

MCP should return a richer text or JSON snapshot.

## Query As Tick

Current `query` should become:

```text
legend memory tick --intent question --read-only "What do we know about X?"
```

Compatibility command:

```text
legend memory query "What do we know about X?"
```

Internally:

- parse as `ExperienceIntent::Question`,
- set durable policy to read-only except snapshot trace if enabled,
- route through attention/columns/voting,
- return an experience snapshot.

This matches the brain analogy: querying memory is directed attention, not a
separate database operation.

## Start And Init

`start` and `init` remain separate infrequent commands.

### Init

`init` is a sensory bootstrap pass over repo state. It should create initial
repo/file/project columns, not just seed graph nodes.

Potential columns from init:

- repo overview column,
- package/dependency column,
- test/build column,
- docs/product column,
- module/file clusters,
- known command/workflow column,
- source-authority column for repo-derived evidence,
- uncertainty/truth-maintenance columns.

### Start

`start` should become a summary of the current global experience state:

- current task/plan,
- recent snapshots,
- active columns,
- active subspaces,
- unresolved conflicts,
- unresolved uncertainty sources,
- pending maintenance modes,
- high-confidence user preferences,
- relevant repo state,
- warnings.

It should not require a query. It should surface "where the mind is" at session
start.

## Command Contract

Do not remove commands immediately.

Experimental branch contract:

- Keep existing `memory tick`.
- Add `--json` or richer default output behind a feature flag/config.
- Keep `memory query` as compatibility wrapper.
- Keep `memory start`.
- Keep dev/admin commands: `stats`, `dump`, `reset`, `consolidate`,
  `reinforce`, `sessions`.

Long-term LLM-facing contract:

- `start`: resume continuous experience.
- `tick`: advance experience.
- `init`: bootstrap repo sensors.

Everything else should be dev/admin or compatibility.

## Implementation Plan

### Phase 0: Branch And Guardrails

Goal: make experimentation safe.

Tasks:

- Create a branch such as `experiment/experience-snapshots`.
- Add a config flag: `experimental_experience_snapshots`.
- Preserve current storage loading.
- Do not delete current graph fields.
- Add new structs with serde defaults so old memory files load.
- Add golden tests proving current commands still work when the flag is off.

Acceptance:

- Existing normal test subset passes with flag off.
- `memory start`, `memory tick`, and `memory query` outputs are unchanged with
  flag off.

### Phase 1: Define Snapshot Types

Goal: introduce the vocabulary without changing behavior.

Tasks:

- Add `ExperienceIntent`.
- Add `UncertaintySource`.
- Add `SensoryFrame`.
- Add `AttentionState`.
- Add `RoutingSubspace`.
- Add `MaintenanceMode`.
- Add `ColumnVote`.
- Add `ExperienceSnapshot`.
- Add `RoutingTraceEntry` and `MaintenanceRequest`.
- Extend `TickResult` with optional `snapshot`.
- Add output formatting helpers.

Acceptance:

- Normal tick returns old JSON by default.
- Test helper can call an internal API and receive a snapshot skeleton.

### Phase 2: Intent Inference And Sensory Frames

Goal: turn incoming text into structured experience input.

Tasks:

- Extract current chunk/entity/relation/date/ref logic into a sensory-frame
  builder.
- Infer intent from prefixes/question syntax.
- Add initial uncertainty-source classifier.
- Track source type.
- Add tests for assertion/question/correction/plan/tool observation.
- Add tests for missing evidence, contradiction, stale model, ambiguous intent,
  source conflict, prediction error, and task shift.

Acceptance:

- Questions are classified as `Question`.
- `DECISION:`, `BUG:`, `ARCHITECTURE:`, `PLAN:` keep expected behavior.
- `Question` does not create durable L2/L3 facts unless policy explicitly says
  to.
- Correction-style ticks produce `Contradiction` or `SourceConflict` instead of
  flattening old/new claims.

### Phase 3: Column Store Skeleton

Goal: create columns while reusing existing graph internals.

Tasks:

- Add `ColumnStore` to `BrainState`.
- Add `CorticalColumn`, `ColumnKind`, `ColumnSelector`, `ReferenceFrameKey`,
  and `ColumnSubspace`.
- Add `ColumnSupportStore` with empty/auditable defaults.
- Create default system columns:
  - current task/plan,
  - user preferences,
  - agent self-model,
  - repo overview,
  - temporal context,
  - truth maintenance,
  - uncertainty routing.
- Add a compatibility projection from columns to `long_term` if needed.

Acceptance:

- Memory loads with no columns and initializes defaults lazily.
- Dump/stats can show column counts.
- Dump/stats can show subspace counts and support-store sizes.
- No existing graph tests break with feature flag off.

### Phase 4: Attention-Based Column Routing

Goal: select relevant columns per tick.

Tasks:

- Implement `compute_attention_state`.
- Route sensory frames to top K columns.
- Use salience, active plan, entities, refs, source, recency, and embedding
  similarity.
- Use uncertainty-source classification to force mandatory routing when needed.
- Select column subspaces, not only whole columns.
- Add instrumentation for selected columns, selected subspaces, and routing
  reasons.

Acceptance:

- Architecture discussion activates architecture/research/task columns.
- Repo file observation activates repo/file columns.
- User preference activates user preference/self-model columns.
- Correction activates truth-maintenance and uncertainty-routing subspaces.
- Snapshot includes a routing trace explaining selected columns/subspaces.

### Phase 5: Column-Local Inference And Votes

Goal: columns emit useful votes.

Tasks:

- Implement first `ColumnModel` using local `GraphMemory`.
- Add local update policy by intent.
- Add local hypothesis generation:
  - direct entity/relation match,
  - embedding match,
  - contradiction/correction match,
  - plan/task match,
  - support-store candidate match,
  - prediction-error match.
- Emit `ColumnVote`.

Acceptance:

- A tick produces multiple votes from different columns.
- Votes include confidence and evidence refs.
- Votes include selected subspace, uncertainty source, and prediction error.
- Question ticks produce votes without durable fact insertion.
- Support-derived votes are marked exploratory unless backed by evidence refs.

### Phase 6: Consensus Snapshot

Goal: make the tick response experience-shaped.

Tasks:

- Aggregate votes by target/hypothesis.
- Normalize votes by attention weight, column reliability, subspace reliability,
  evidence support, confidence calibration, and conflict penalties.
- Preserve dissenting hypotheses above threshold.
- Generate consensus text, uncertainty, uncertainty-source summary, and routing
  trace.
- Include recommended response/action.

Acceptance:

- Tick output can answer "what is happening now?"
- Competing interpretations are visible.
- Conflicts are visible without deleting either side.
- The snapshot can explain why uncertainty exists and what policy it triggered.

### Phase 7: Query Compatibility Wrapper

Goal: route query through the same pipeline.

Tasks:

- Make `memory query` call the experience pipeline with intent `Question`.
- Preserve old JSON shape optionally behind `--legacy` or compatibility mode.
- Fix MCP query description to remove stale auto-reinforcement claim.
- Update lifecycle docs.

Acceptance:

- Existing query tests either pass in compatibility mode or are intentionally
  migrated.
- Query does not mutate durable memory except allowed snapshot trace.
- MCP docs match implementation.

### Phase 8: Init Creates Columns

Goal: repo initialization becomes sensory-model bootstrap.

Tasks:

- Create repo/file/doc/test columns during init/discover.
- Use existing dependency scanning to seed repo columns.
- Seed columns with source authority, subspaces, and frame metadata.
- Keep compatibility graph projection.

Acceptance:

- Fresh init creates meaningful columns.
- Fresh init creates meaningful subspaces for architecture, dependencies,
  docs/product, tests, and command workflow.
- Start surfaces active/relevant columns.
- Repo queries/ticks route to repo columns.

### Phase 9: Replay And Consolidation Become Column Maintenance

Goal: make sleep-like maintenance operate over columns.

Tasks:

- Replay snapshots into active/stale columns by `MaintenanceMode`.
- Stabilize recent snapshots separately from remote memories.
- Extract gist/schema while preserving exact source traces.
- Run stale-model revalidation for columns with high prediction error.
- Run contradiction repair when truth-maintenance pressure is high.
- Merge/split columns based on evidence density and conflict.
- Strengthen inter-column wiring when columns co-vote.
- Decay unreliable or unused hypotheses.
- Preserve high-authority corrections.

Acceptance:

- Repeated cross-column agreement strengthens consensus.
- Redundant local evidence compacts without losing source support.
- Conflicting frames stay separated.
- Recent and remote replay modes are visible in maintenance logs.
- Gist entries cite compressed source traces and omitted-frame caveats.

### Phase 10: Evaluate Against Current And New Benchmarks

Goal: prove this is better, not just more complex.

Tasks:

- Keep existing observability harness.
- Add experience snapshot benchmark fixtures.
- Add query-as-tick compatibility tests.
- Add uncertainty demixing fixtures.
- Add routing-subspace fixtures.
- Add maintenance-mode fixtures.
- Add branch-level comparison against current graph-only behavior.

Acceptance:

- Existing memory behavior does not regress unacceptably.
- Snapshot output improves LLM usability.
- Column routing reduces hub-term domination.
- Corrections and frame-relative contradictions improve.
- Uncertainty classification produces useful policy decisions.
- Routing traces make snapshot decisions debuggable.
- Latency remains within budget or is clearly attributable.

## Benchmark Plan

### Existing Benchmarks To Preserve

- conformance memory commands,
- MCP conformance,
- start/query/context/dump workflows,
- observability pre-phase fixtures,
- Project Alpha 25-tick fixture,
- multi-domain signal/noise fixture.

### New Snapshot Benchmarks

Add fixtures where each tick expects:

- active focus,
- active columns,
- active subspaces,
- consensus statement,
- retained uncertainty,
- uncertainty-source classification,
- routing trace,
- conflicts,
- recommended next action,
- proposed maintenance mode when appropriate.

Example fixture:

```json
{
  "tick": "The user says Project Alpha no longer uses SQLite; it now uses Postgres for production only.",
  "expected": {
    "intent": "Correction",
    "active_columns": ["project_alpha", "datastore", "truth_maintenance"],
    "consensus_contains": ["Postgres", "production"],
    "conflict_contains": ["SQLite"],
    "must_not_delete_prior_fact": true
  }
}
```

Additional fixture families:

- uncertainty demixing: same low-confidence query should produce different
  policies for missing evidence, contradiction, stale model, and ambiguous
  intent,
- routing subspaces: same repo column should route through different subspaces
  for architecture, source authority, test workflow, and current task,
- compositional snapshot: user intent, repo state, active plan, and source
  authority should bind into one coherent current experience,
- recent-vs-remote replay: recent corrections should stabilize before remote
  gist compression changes older beliefs,
- support-store auditability: associative candidates can help retrieval but
  cannot become truth claims without evidence refs,
- connectome observability: snapshots should expose active columns, subspaces,
  vote paths, and wiring changes.

### Metrics

- Snapshot relevance: does consensus match the user's actual context?
- Column routing precision: are the right columns active?
- Column routing recall: did required columns activate?
- Subspace routing precision: did the right column facet activate?
- Uncertainty classification accuracy: did uncertainty source map to the right
  policy?
- Vote diversity: are multiple useful hypotheses represented?
- Conflict preservation: are contradictions visible and scoped?
- Confidence calibration: do corrected columns lose reliability in the right
  frame?
- Maintenance suitability: did the system schedule the right replay mode?
- Source coverage: does gist cite the exact traces it compresses?
- Observability: can a developer explain a snapshot from routing/vote traces?
- Query compatibility: can questions still retrieve needed context?
- LLM usability: does snapshot reduce need for follow-up queries?
- Latency: cold/warm tick wall time.
- Memory growth: columns, local evidence, snapshots, projection graph.

## Risks

### Risk: Overengineering

Mitigation:

- Build behind a feature flag.
- Reuse `GraphMemory` inside columns initially.
- Keep old command behavior until snapshot path proves itself.
- Treat neuroscience as design pressure, not requirements to simulate biology.

### Risk: Snapshot Text Becomes Vague

Mitigation:

- Snapshot must carry evidence refs and votes, not just generated prose.
- Tests should assert concrete entities/conflicts/actions.
- Routing traces and uncertainty sources must be machine-readable.

### Risk: Columns Become Tags

Mitigation:

- Give each column local state, local hypotheses, local reliability, and local
  update policy.
- Do not rely only on `column_ids` attached to global graph nodes.
- Require selected subspaces for column participation.

### Risk: Too Much Duplication

Mitigation:

- Allow deliberate redundancy but compact evidence locally.
- Keep shared evidence refs so duplicated claims point to the same source trace.

### Risk: Support Stores Become Opaque Truth Sources

Mitigation:

- Support stores can retrieve and calibrate, but cannot assert truth directly.
- Support-derived votes must be marked exploratory unless backed by durable
  evidence.
- Add tests where unsupported associative guesses must not become consensus.

### Risk: Misclassified Uncertainty Causes Bad Actions

Mitigation:

- Keep uncertainty source and chosen policy visible in snapshots.
- Start with conservative policies: preserve contradictions, ask on ambiguous
  intent, and avoid destructive updates.
- Add benchmark fixtures for every uncertainty source.

### Risk: Overconfident Consensus

Mitigation:

- Preserve dissenting hypotheses above threshold.
- Penalize columns with repeated confidence errors in the same reference frame.
- Distinguish evidence agreement from duplicated evidence copied across columns.

### Risk: Slow Ticks

Mitigation:

- Route to top K columns.
- Use cached embeddings and column centroids.
- Defer full maintenance to replay/consolidation.
- Keep a compact snapshot output mode.

### Risk: Confusing User-Facing Contract

Mitigation:

- Keep `start`, `tick`, `query` initially.
- Document that `query` is a question tick.
- Only simplify commands after compatibility tests pass.

## Open Design Questions

- Should snapshots be stored every tick, or only high-salience/meaningful ticks?
- Should questions leave episodic snapshot traces by default?
- How many default columns should exist for an empty repo?
- When should a column split into more specific columns?
- When should multiple columns merge?
- Should column reliability be learned from explicit reinforcement, benchmark
  outcomes, or downstream user corrections?
- Should recommended response/action be generated deterministically or left to
  the LLM after reading the snapshot?
- How should images be represented if repo images become a real sensor later?
- Should the compatibility graph projection be persisted or rebuilt from
  columns on load?
- Should uncertainty routing be part of thalamus, prefrontal, or a separate
  column family in code?
- What is the minimal useful representation for a subspace: labels, centroids,
  learned dimensions, or explicit selectors?
- When should support-store associations be promoted into explicit column
  evidence?
- What triggers each maintenance mode?
- How much routing/vote observability can be stored without making snapshots too
  large?
- How should column reliability account for LLM error versus memory-system
  error?

## Minimal First Prototype

The smallest useful branch prototype:

1. Add `ExperienceSnapshot` and return it behind `memory tick --snapshot`.
2. Add 7 default columns:
   - current task/plan,
   - repo architecture,
   - user preference,
   - agent self-model,
   - temporal/session context,
   - truth maintenance,
   - uncertainty router.
3. Add minimal subspaces: `structure`, `truth_maintenance`, `temporal_context`,
   `goal_relevance`, and `user_preference`.
4. Route ticks to columns/subspaces with simple entity/keyword/embedding
   matching.
5. Add minimal uncertainty-source classification for `MissingEvidence`,
   `Contradiction`, `AmbiguousIntent`, and `StaleModel`.
6. Let each column emit one vote using existing retrieval/graph functions.
7. Aggregate votes into consensus/uncertainty/conflicts/routing trace.
8. Make `memory query --snapshot` call the same path as a read-only question.
9. Add four benchmark fixtures:
   - architecture discussion snapshot,
   - correction/truth-maintenance snapshot,
   - uncertainty-demixing snapshot,
   - routing-subspace snapshot.

If that does not produce a noticeably better LLM-facing context than the current
query result, stop and reassess before deeper storage migration.

## Success Criteria

This experiment is worth merging only if it demonstrates:

- Ticks return a useful current-experience snapshot, not just a write receipt.
- Questions can be handled as directed-attention ticks.
- The active focus survives across turns better than current query/start output.
- Contradictions are clearer and better scoped.
- Uncertainty sources are visible and drive sensible response policies.
- Column subspaces produce better routing than flat graph/entity matching alone.
- Maintenance modes prevent recent corrections from being blurred into stale
  gist.
- Support-store candidates remain auditable and do not become unsupported truth.
- Routing/vote traces make snapshots explainable during debugging.
- Hub graph nodes dominate less.
- Existing memory recall does not regress materially.
- The LLM can use the snapshot directly to produce better responses with fewer
  manual queries.

## Final Target

Long-term, Legend should feel less like:

```text
store memory -> query memory -> retrieve rows
```

and more like:

```text
experience stream -> attention -> cortical consensus -> response/action
```

Durable memory still matters, but it becomes the substrate that shapes future
experience rather than the primary interface.
