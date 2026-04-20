# Legend: A Neuroscience-Informed Hierarchical Memory System for Persistent LLM Agents

**Author:** Nick Thorpe
**Affiliation:** Independent Researcher
**Date:** April 6, 2026

## Abstract

Large language model agents exhibit strong single-session reasoning but lack durable memory across session boundaries — a critical limitation for any task that unfolds over days or weeks, from software engineering to research synthesis to creative projects. Useful state in such work includes not only artifacts but rationale, failed approaches, constraints, and evolving task structure. This paper presents Legend, a local-first hierarchical persistent memory system for LLM agents. Legend organizes memory into three bounded layers — working memory (prefrontal cortex analog, capacity 10), episodic memory (hippocampus analog, capacity 1,024), and a long-term knowledge graph (neocortex analog, up to 2,048 nodes and 8,192 edges) — connected by a deterministic pipeline of attention gating, salience-weighted retention, retrieval-induced reconsolidation, pattern separation, emotional valence, multi-hop spreading activation, sharp-wave ripple replay, and controlled forgetting. The architecture draws functional motifs from cognitive neuroscience — particularly Kandel's work on synaptic plasticity, complementary learning systems theory, and hippocampal reconsolidation — while remaining fully deterministic and computationally bounded. Legend is implemented as a ~16,700-line Rust CLI with 659 passing tests, zero external API dependencies, and sub-5ms operation latency. The paper makes three contributions: (1) a concrete, reproducible memory architecture that treats memory as a governed dynamical system rather than an append-only store; (2) an argument for evaluating agent memory by longitudinal behavioral outcomes rather than storage volume; and (3) an observational case study from 218 instrumented development sessions demonstrating that bounded selective memory can preserve project continuity across repeated session boundaries. Controlled comparative evaluation remains future work.

## 1. Introduction

The modern LLM agent presents a sharp asymmetry. Within a session it can reason, produce high-quality output, and recover from nontrivial failures. Across sessions it often behaves as if prior work never occurred. This discontinuity affects any domain where work is cumulative and path-dependent: software engineering, research, writing, operations, design. Previous trade-offs are revisited, rejected approaches are retried, and subtle constraints become easy to overlook. The limiting factor is often not per-turn reasoning quality but the absence of durable external memory aligned with how knowledge-intensive work unfolds over time.

The argument rests on a simple claim: stateful agent behavior requires stateful memory infrastructure external to the model context window. Storage alone is insufficient — real working signals are noisy. Ephemeral experiments, incomplete hypotheses, and routine chatter all compete with high-value artifacts such as decisions, incident postmortems, and structural constraints. A memory system that indiscriminately accumulates text eventually degrades retrieval precision. In practice, such systems fail not because they forget too much, but because they remember without discrimination. Even very large context windows exhibit attention dilution and weak structural traversal for long-horizon work; Liu et al. [1] demonstrate that models frequently underuse information placed in the middle of long prompts, suggesting that raw transcript accumulation is structurally inadequate for persistent memory.

Legend addresses this gap. Its core design principle is that memory should behave as a governed dynamical system with differentiated retention, reinforcement, and decay, rather than as an append-only notebook. The architecture separates immediate experience from medium-term semantic traces and long-term relational abstractions, organized into brain-region modules that make the cognitive analogy explicit in code while enforcing separation of concerns.

Legend is not primarily a human note-taking tool. It is an operational substrate for agent behavior. The intended loop is machine-to-machine: the agent opens each session with `memory start`, records decisions with `memory tick`, and retrieves context through `memory query` before uncertain work. The human role is governance — setting priorities, reviewing outcomes, and intervening when policy concerns require it. While the primary case study in this paper is software engineering, the mechanisms are domain-general: Legend's keyword system supports arbitrary domain vocabularies via workspace bootstrap and statistical learning, and its memory lifecycle makes no assumptions specific to code.

### 1.1 Contributions

- A bounded, local-first architecture for persistent LLM agent memory, implemented as a Rust CLI with deterministic retrieval and update rules, applicable across knowledge-intensive domains.
- A memory lifecycle that couples salience-based retention, reconsolidation, pattern separation, emotional valence, sharp-wave ripple replay, multi-hop spreading activation, and Hebbian reinforcement in a single operational pipeline.
- A Git-aware cold-start mechanism that addresses the observer gap between sessions when the working environment changes while the agent is offline.
- An observational case study showing that the system remains compact and operationally useful across 218 instrumented sessions.
- A falsifiable evaluation framing that distinguishes demonstrated properties from hypotheses requiring benchmark comparison.

## 2. Background and Related Work

### 2.1 The Neuroscience Foundation

Legend's architecture draws on three bodies of neuroscience research, adopted as functional design constraints rather than claims of biological fidelity.

**Synaptic plasticity and structural learning (Kandel).** Kandel's Nobel Prize-winning work on *Aplysia* demonstrated that learning operates at the synapse through both functional changes (short-term facilitation, lasting minutes to hours) and structural changes (growth of new synaptic terminals for long-term memory) [2]. The key insight for Legend is that memory strength is not a single scalar but a multi-timescale process: short-term sensitization modulates existing connections, while long-term learning physically restructures the network. Legend implements this through dual-timescale edge encoding (short-term plasticity and long-term potentiation EMAs on graph edges), stability-modulated decay (the Ebbinghaus forgetting curve), and Hebbian reinforcement where co-retrieved nodes strengthen their connections — "neurons that fire together wire together" translated into graph operations.

**Complementary learning systems (McClelland, McNaughton & O'Reilly).** CLS theory [3] explains why the brain maintains separate fast-learning (hippocampal) and slow-integrating (neocortical) memory systems. The hippocampus rapidly encodes specific episodes with minimal interference, while the neocortex gradually extracts statistical regularities across episodes. This complementary structure prevents catastrophic interference — new learning destroying old knowledge. Legend's L2 (episodic, hippocampus analog) and L3 (knowledge graph, neocortex analog) directly implement this separation: L2 stores specific episodes that can be rapidly written and revised, while L3 accumulates entity relationships that strengthen gradually through Hebbian learning and consolidation.

**Thousand Brains Theory and reference frames (Hawkins, Ahmad & Cui).** Hawkins et al. [6] propose that cortical columns learn predictive object models by binding sensed features to object-relative location signals, with multiple columns rapidly converging through lateral connections. Hawkins' later popular account [7] frames intelligence as many parallel cortical models using reference frames rather than a single centralized world model; Numenta's companion paper [8] summarizes the same idea for non-specialists. Legend adopts this as a design constraint for L3 extraction: semantic memory should not be a flat bag of co-occurring keywords. Facts should be represented as typed relations scoped by reference frames such as project, time, source, domain, goal, physical location, and epistemic status. This motivates Phase 5's transition from entity co-occurrence toward subject-relation-object facts with evidence, confidence, and frame context.

**Reconsolidation (Nader, Schafe & Le Doux).** Nader et al. [4] demonstrated that retrieving a consolidated memory returns it to a labile state requiring protein synthesis to restabilize — retrieval is not a read-only operation but a modification opportunity. This overturned the long-held assumption that consolidated memories are fixed. Legend implements reconsolidation directly: retrieved L2 entries enter a labile window (5 ticks) during which new, related information updates them in-place rather than creating duplicates. This is a critical mechanism for iterative knowledge work where early understanding of a problem is routinely revised by later evidence.

### 2.2 LLM Memory Systems

Research on LLM long-term memory is expanding rapidly. Recent surveys argue that future AI systems will require explicit memory mechanisms beyond the transient context window and increasingly draw on analogies to human memory systems [5]. That framing is useful for Legend, but it also raises a risk: neuroscience metaphors can easily become rhetorical decoration unless they correspond to concrete algorithmic choices. The relevant question is not whether a system sounds biologically inspired, but whether the analogy motivates useful engineering structure.

Existing memory systems for language agents span several design families — from structured dialogue-history stores with explicit retrieval and updating, to learned memory modules embedded directly into sequence architectures, to benchmark efforts revealing that sustained interactive memory remains difficult even for strong commercial assistants. These approaches target different layers of the problem from Legend: some seek neural architectures for long-range sequence handling, while Legend is an external memory substrate for real workflows operating around existing LLMs.

What distinguishes Legend from most prior work is not any single mechanism in isolation but their combination: bounded storage, deterministic behavior, multi-layer retention dynamics, retrieval-triggered plasticity, emotional modulation, graph-based relational expansion, and environment-aware startup reconciliation — all in a single local system with no external dependencies.

## 3. Design Goals

Legend targets a specific operational setting: an LLM agent repeatedly working on the same project across many sessions, with a human supervising but not manually curating memory on every turn. While the primary validation is in software engineering, the goals are domain-general:

- Memory must survive process restarts and session boundaries.
- Retrieval must remain cheap and bounded on local hardware.
- Stored information must remain inspectable and reproducible.
- The system must preserve high-signal rationale without retaining all raw interaction history in active memory.
- Environmental drift between sessions must be surfaced explicitly.
- The vocabulary and salience model must adapt to the working domain rather than being hardcoded for any single field.

These goals imply several design choices. The active store must be size-bounded and selectively updated. The implementation should avoid dependence on external APIs for correctness. The memory representation must support both semantic similarity and relational traversal, because many real queries are underspecified and depend on structural neighbors rather than exact wording. The system should expose enough internal state for later evaluation.

## 4. System Architecture

Legend is implemented as a local Rust CLI (~16,700 lines, 659 tests). Persistent state is stored in `.legend/memory.lz4` as a MessagePack-serialized, LZ4-compressed `MemoryState`, and an append-only event log in `.legend/events.jsonl`. All memory operations are performed offline and deterministically.

### 4.1 Brain-Region Module Organization

The codebase is organized so that each cognitive mechanism lives in a module named after its functional neuroscience analog. This is not cosmetic — it enforces separation of concerns and makes design intent readable from file names alone:

| Module | Brain Region | Function |
|--------|-------------|----------|
| `prefrontal.rs` | Prefrontal cortex | Working memory (L1): attention gating, context-switch flushing |
| `hippocampus.rs` | Hippocampus | Episodic memory (L2): reconsolidation, pattern completion, SWR replay |
| `neocortex.rs` | Neocortex | Knowledge graph (L3): spreading activation, Hebbian learning, consolidation |
| `thalamus.rs` | Thalamus | Sensory encoding: salience scoring, attentional gating |
| `amygdala.rs` | Amygdala | Emotional valence: bipolar threat/reward scoring |
| `dentate_gyrus.rs` | Dentate gyrus | Pattern separation: sparse orthogonalization, diversity gating |
| `basal_ganglia.rs` | Basal ganglia | Reinforcement: AdaGrad optimization, contrastive descent |
| `entorhinal.rs` | Entorhinal cortex | Encoding + compression: semantic embeddings (MiniLM), text chunking, summarization |
| `wernicke/` | Wernicke's area | Language comprehension: entity extraction, keyword vocabulary |

All brain modules (`src/memory/`) are pure computation with no filesystem or network access. IO and persistence live in a separate tool layer (`src/tool/`), maintaining a clean boundary between cognitive logic and system integration.

### 4.2 Three-Layer Memory Hierarchy

**Layer 1 — Working Memory (Prefrontal Cortex).** A small buffer (`Vec<WorkingMemoryEntry>`, capacity 10) informed by Miller's Law (~7±2 items). Only entries with salience exceeding an attention gate threshold (0.25) are promoted to L2. On context switch — detected when cosine similarity between consecutive ticks drops below 0.15 — L1 is flushed and unpromoted entries receive a final promotion opportunity.

**Layer 2 — Episodic Memory (Hippocampus).** A bounded store (`Vec<ShortTermEntry>`, capacity 1,024). Each entry contains text, extractive summary, 384-dimensional embedding (from an embedded sentence transformer), salience score, usage count, reconsolidation metadata, emotional valence, Ebbinghaus stability, density, and optional source references. Salience decays exponentially: `salience *= exp(-age × 0.001 / stability)`, yielding a base half-life of ~693 ticks that increases with spaced retrieval (stability ranges 1.0–10.0). Emotional valence decays at half the hippocampal rate, modeling how emotionally significant memories resist forgetting.

**Layer 3 — Knowledge Graph (Neocortex).** Up to 2,048 nodes and 8,192 edges. Nodes represent extracted entities with typed labels. Edges carry one of seven relationship kinds (`contains`, `depends-on`, `implements`, `co-defined`, `drives`, `represents`, `related`) with weight, activation count, stability, and dual-timescale interval averages — short-term plasticity (α=0.5) and long-term potentiation (α=0.1), directly reflecting Kandel's distinction between short-term facilitation and long-term structural change [2]. Phase 5 extends this layer toward Thousand Brains-style reference-frame semantics [6–8]: durable L3 facts are modeled as subject-relation-object propositions scoped by project/time/source/domain/goal/location/epistemic frames rather than as globally true keyword co-occurrences. Decay rate is 0.0005 per tick (half-life ~1,386 ticks), twice as durable as L2.

This decomposition implements the complementary learning systems principle [3]: L2 is the fast-learning hippocampal system that rapidly encodes specific episodes, while L3 is the slow-integrating neocortical system that gradually extracts stable relational structure through repeated consolidation.

## 5. Methods

### 5.1 End-to-End Update Pipeline

The central write path is `tick_impl(text, passive)`. Each call increments a monotonic clock, applies decay to existing entries, expires old labile states, and runs periodic normalization (salience every 10 ticks, graph weights every 5 ticks, rolling emotional intensity decay ×0.8). The ordering is deliberate: Legend first updates the temporal geometry of existing memory, then integrates new evidence into that updated landscape.

Input text is chunked into ~200-character segments (entorhinal cortex compression). Each chunk proceeds through:

1. **Working memory insertion** (prefrontal cortex)
2. **Sensory encoding** (entorhinal cortex) — 384-dimensional semantic embedding via embedded MiniLM-L6-v2
3. **Salience scoring** (thalamus) — content heuristics from dynamic keyword cache
4. **Emotional valence** (amygdala) — bipolar threat/reward signal
5. **Attention gate** — salience ≥ 0.25 promotes to L2; otherwise remains in L1 only
6. **Pattern separation** (dentate gyrus) — sparse orthogonalization against similar L2 entries
7. **Reconsolidation check** — if a labile entry matches (similarity ≥ 0.40), update in-place
8. **Dual-threshold matching** — ≥ 0.85 + word overlap ≥ 0.40: reinforce; ≥ 0.75: merge; below: insert
9. **Graph update** (neocortex) — entity extraction (Wernicke's area), node and edge creation
10. **Retrieval priming** — local retrieval marks entries labile
11. **Pruning** — garbage collect L2 and L3
12. **Context-switch detection** — cosine similarity to previous tick < 0.15 triggers L1 flush

This pipeline treats every observation simultaneously as raw recency (L1), semantic evidence (L2), and relational signal (L3), avoiding a common failure mode where subsystems update out of sync.

### 5.2 Embedding and Similarity

Legend uses all-MiniLM-L6-v2 quantized, a 6-layer sentence transformer producing 384-dimensional embeddings. The ~23MB quantized ONNX model is compiled directly into the binary via `include_bytes!()`, eliminating network dependency while providing semantic similarity that captures meaning beyond lexical overlap. This design privileges zero-dependency operation and semantic quality over minimal binary size.

Legend adds a lexical overlap gate (Jaccard word overlap ≥ 0.40) before merge or reinforce actions — a precision control that ensures entries share sufficient vocabulary to be considered the same memory, even when embeddings are similar.

### 5.3 Salience and Adaptive Retention

Salience is initialized by content heuristics from a three-layer keyword system: ~288 static domain-independent keywords (decision, action, architecture, bug, blocker, preference), workspace-bootstrapped domain keywords (extracted from project manifests during initialization), and incrementally discovered terms (auto-promoted after appearing in ≥ 5 distinct ticks with keyword co-occurrence). The keyword system makes no assumptions about the working domain — it learns project vocabulary through observation.

Temporal decay is modulated by two factors: a `density` term (weighted count of high-signal entities, slowing decay up to 2×) and a `stability` term from the Ebbinghaus forgetting model (1.0–10.0, growing with spaced retrieval). This implements the spacing effect from memory research: memories retrieved at increasing intervals develop stronger resistance to forgetting than those accessed in massed repetition.

Explicit reinforcement uses AdaGrad-style adaptive learning: `lr = 0.15 / sqrt(sq_sum + ε)`. This prevents frequently-reinforced memories from saturating while allowing rarely-reinforced ones to respond strongly to a single positive signal.

### 5.4 Pattern Separation (Dentate Gyrus)

Before matching, new embeddings undergo sparse orthogonalization against similar-but-distinct L2 embeddings, pushing representations apart in vector space. This reduces retrieval interference between memories sharing vocabulary but representing different concepts. The mechanism directly implements the pattern separation function of the dentate gyrus, which creates sparse, orthogonal representations to minimize interference between similar episodes during hippocampal encoding [3].

### 5.5 Reconsolidation

Retrieved memories enter a labile window for 5 ticks, implementing Nader et al.'s finding that retrieval destabilizes consolidated memories [4]. If a new tick matches a labile entry (similarity ≥ 0.40, lexical overlap ≥ 0.10), the existing entry is updated in-place: text merged, embedding recomputed, salience incremented. This prevents duplicate proliferation during iterative investigation — a ubiquitous pattern in knowledge work where early understanding is routinely revised.

If reconsolidation does not trigger, dual-threshold matching applies: θ_high (0.85) reinforces; between θ_low (0.75) and θ_high merges; below θ_low inserts new.

### 5.6 Emotional Valence (Amygdala)

Each memory receives a bipolar emotional valence in [-1.0, 1.0]. Negative valence flags threats (bugs, crashes, failures, regressions); positive flags rewards (shipped, fixed, resolved, success). Urgency amplifiers (blocker, critical, P0) push magnitude toward extremes.

Valence influences retention (slower decay), retrieval (small similarity boost), and eviction resistance. Rolling emotional intensity triggers early consolidation when it exceeds 1.5, modeling amygdala-driven memory consolidation after emotionally significant events — consistent with evidence that emotional arousal enhances memory consolidation [2].

### 5.7 Graph Construction and Hebbian Learning

Entity extraction (Wernicke's area) recognizes file paths, action verbs, environment markers, language constructs across multiple programming and natural languages, and residual identifiers after stopword filtering. Edges carry typed relationships inferred from co-occurrence context.

Enriched synaptic encoding tracks each edge's activation count, stability (capped at 10.0), and dual-timescale interval averages. This implements Kandel's insight that synaptic strength encodes both recent facilitation and long-term structural change [2]. Frequently co-activated edges — representing genuinely related concepts — develop higher effective weight than one-time coincidences.

Hebbian reinforcement fires automatically during retrieval: co-retrieved node pairs receive edge weight boosts (+0.05, ceiling 10.0) and node weight boosts (+0.02, ceiling 5.0).

### 5.8 Retrieval and Associative Priming

`retrieve_context(query)` is a learning event, not a read-only call. It applies decay, embeds the query, gathers all L2 candidates above a similarity floor (0.25) with keyword bonus, and adds L3 Summary hits to the candidate pool.

**Diversity selection (dentate gyrus at retrieval time).** When the candidate pool exceeds 5, Maximal Marginal Relevance (MMR) with λ=0.7 iteratively selects results that balance relevance to the query with diversity among selected items. This prevents near-duplicate memories from dominating retrieval, complementing the encoding-time sparse orthogonalization. Selected entries are marked labile and the top result is auto-reinforced.

**Pattern completion (CA3).** When initial results are weak (top similarity < 0.5 or fewer than 3 results), entities are extracted from partial matches, spreading activation traverses the graph, and L2 is searched for entries containing activated entities — modeling how the hippocampal CA3 network reconstructs full memories from partial cues.

**Multi-hop spreading activation.** BFS-style outward spread from seed entities, up to 3 hops with 0.5× decay per hop. Edge weight is modulated by activation count and temporal pattern, so frequently co-activated paths propagate more strongly.

**Associative priming.** A second pass builds seeds from entities in the retrieved L2 snippets and follows additional graph hops, expanding recall beyond the literal query text.

### 5.9 Consolidation

Consolidation transforms repeated local observations into compressed long-term abstractions. Three triggers:

1. **Cadence** — after 15 non-passive ticks.
2. **Emotional intensity** — rolling `recent_valence_sum` exceeds 1.5 (amygdala-driven).
3. **Context switch** — cosine similarity between consecutive ticks drops below 0.15 (novelty detection).

The pipeline:

1. **Sharp-wave ripple replay** — temporally co-active L2 pairs (within 5 ticks) have shared graph edges reinforced (+0.08) and receive salience boosts (+0.02). New temporal edges are created between co-active entities. This implements the hippocampal replay mechanism believed to support memory consolidation during rest [3].
2. **Clustering** — L2 entries grouped by similarity ≥ θ_low (0.75).
3. **Summarization** — multi-entry groups produce extractive summaries as L3 Summary nodes.
4. **Systems consolidation** — high-salience groups (average ≥ 0.4) receive centroid embeddings and rich text on Summary nodes, enabling L3 to serve queries independently after L2 entries decay.
5. **Pruning** — L2 and L3 garbage collected.

### 5.10 Boundedness and Stability

Legend enforces hard capacity limits: 10 L1 entries, 1,024 L2 entries, 2,048 L3 nodes, 8,192 L3 edges. Pruning, salience renormalization (gentle EMA blend every 10 ticks), and graph weight normalization (ceiling 2.0 every 5 ticks) prevent reinforcement blow-up.

Contrastive descent penalizes retrieved-but-unreinforced entries (-0.02 salience), distinguishing confirmed-useful memories from those that merely surface without proving relevant. These controls ensure stable learning dynamics over arbitrarily long usage.

## 6. Cold-Start Continuity and the Observer Gap

A major failure mode appears when humans modify the working environment while the agent is offline. Legend addresses this through Git-aware cold-start synchronization. On `memory start`, the system compares current HEAD to the last synchronized SHA, reports intervening commits, summarizes uncommitted changes, and presents this alongside the current task and categorized high-signal memories (decisions, architecture, bugs, TODOs, preferences). This is operational reconciliation that lets the agent re-enter a moving project with reduced epistemic drift.

## 7. Implementation

### 7.1 Persistence

Memory is serialized with MessagePack (`rmp-serde`), prefixed with a `LGND` magic header and format version byte, compressed with LZ4, and written atomically via temporary file and rename. Corrupt stores are quarantined and the system returns a fresh default. All data lives in two files: `.legend/memory.lz4` (structured memory) and `.legend/events.jsonl` (append-only event log, rotating at 10,000 lines).

### 7.2 Performance

| Operation | Latency |
|-----------|---------|
| `memory tick` | < 5 ms |
| `memory query` | < 5 ms |
| `memory start` | < 10 ms |
| `memory consolidate` | < 50 ms |
| Cosine similarity scan (1,024 entries) | < 1 ms |

### 7.3 Integration

Legend integrates with LLM agents via shell hooks (session start, prompt submit, tool use, session end) and an MCP server (JSON-RPC 2.0 stdio, 6 tools). Currently supports Claude Code, Codex, Gemini CLI, VS Code Copilot, Cursor, and Zed.

### 7.4 Testing

659 passing tests covering unit, integration, and conformance scenarios. Tests verify MessagePack backward compatibility, individual brain-region modules, the full tick pipeline, and end-to-end command behavior.

## 8. Case Study: Tactical RPG Development

Current evidence comes from longitudinal operational use in software engineering — not because Legend is limited to that domain, but because it is the context where the most complete instrumentation exists.

### 8.1 Methodology

The subject is a Fire Emblem-inspired tactical RPG in Rust, featuring procedural map generation, an MML-based music system, unit bonding mechanics, and biome-driven theming. A single developer worked with an LLM agent across 218 sessions over 16.4 days, with Legend integrated from inception. The project underwent a major architectural pivot on day one (tower defense → tactics RPG) and evolved through multiple feature phases, making it a useful stress test for continuity and decision retention.

### 8.2 Aggregate Metrics

| Metric | Value |
|--------|-------|
| Sessions initialized | 218 |
| Ticks recorded | 1,055 |
| Explicit queries | 76 |
| Auto-consolidations | 57 |
| Memory groups merged | 239 |
| Avg. ticks/session | 4.8 |
| Event log size | 1.38 MB |
| Compressed memory state | 0.15 MB |
| Compression ratio | 9.2:1 |

The memory state remained at 0.15 MB despite 218 sessions, confirming that pruning and consolidation prevent unbounded growth.

### 8.3 Tick Frequency and Memory Self-Sufficiency

| Session Range | Avg. Ticks/Session | Phase |
|---------------|-------------------|-------|
| 1–50 | 11.8 | Foundation and exploration |
| 51–100 | 3.3 | Refinement |
| 101–150 | 1.0 | Steady-state, memory-sufficient |
| 151–218 | 3.0 | New feature introduction |

The decline from 11.8 to 1.0 reflects a transition from knowledge-building to knowledge-retrieval: as the memory graph matured, less recording was needed because prior context was available. Recovery to 3.0 in later sessions corresponds to genuinely novel information. A well-functioning memory system should exhibit decreasing marginal recording cost as its coverage increases.

### 8.4 Behavioral Evidence

**Decision stability.** The agent recalled domain-specific design choices across session boundaries (e.g., "Mixed damage takes max(phys, mag) instead of average"), evidence that selective persistence and reconsolidation preserve high-salience traces.

**Failure path avoidance.** Root-cause analyses were surfaced before failed approaches were retried, evidence of graph-primed retrieval on bug and architecture ticks.

**Design continuity across pivots.** The day-one pivot was captured with rationale for what was retained and replaced. Subsequent sessions referenced this context for analogous decisions.

**Task hand-off.** 218-session continuity with zero manual re-onboarding prompts.

### 8.5 Limitations

Single developer, single agent, single project. Post-hoc tick categorization. Token savings estimates (~150,000+ over session history) not validated against a controlled stateless baseline. Cross-project and cross-domain generalization not established.

## 9. Hypotheses and Evaluation Protocol

**H1 — Continuity.** Sessions with Legend should require less manual re-onboarding than sessions without it.

**H2 — Decision retention.** Prior decisions should be recoverable at higher rates than a stateless baseline, with reduced reintroduction of rejected approaches.

**H3 — Stability.** Memory growth and retrieval quality should remain bounded under sustained use. This is the best-supported hypothesis: 218 sessions with bounded 0.15 MB state provides direct evidence.

**H4 — Selective forgetting.** The system should preferentially retain high-signal memories, measurable by salience distribution and survival rates by content category.

A controlled comparison — identical tasks with and without Legend, across multiple projects and operators — remains the most important future evaluation work.

## 10. Discussion

### 10.1 Memory as Governance

Legend suggests that persistent agent performance is better modeled as memory governance than memory accumulation. Thresholds, decay, pruning, normalization, and contrastive descent all introduce friction against indiscriminate retention. In Kandel's terms, the system implements both habituation (low-value traces decay) and sensitization (high-value traces are reinforced and structurally integrated) [2].

### 10.2 Mechanistic Coupling

Legend's effectiveness emerges from mechanism coupling:

1. **Selective persistence** — salience scoring, stability-modulated decay, and contrastive descent preserve high-information traces.
2. **Plasticity through reconsolidation** — iterative understanding mutates existing memories rather than duplicating, following Nader et al.'s insight that retrieval is a modification opportunity [4].
3. **Relational expansion** — multi-hop spreading activation and associative priming recover structural neighbors beyond textual similarity.
4. **Emotional modulation** — threat/reward signals influence retention, consolidation timing, and retrieval ranking.
5. **Cold-start synchronization** — environment-aware reconciliation reduces epistemic discontinuity.
6. **Complementary timescales** — fast episodic encoding (L2) and slow relational integration (L3), following CLS theory [3], prevent new learning from catastrophically interfering with established knowledge.

### 10.3 Generality Beyond Software Engineering

While the case study is from software development, Legend's core mechanisms are domain-independent. The three-layer architecture, reconsolidation, pattern separation, emotional valence, and spreading activation operate on arbitrary text. The keyword system adapts to any domain through workspace bootstrap and incremental discovery. The same architecture could serve research synthesis (preserving literature connections across sessions), creative writing (maintaining narrative consistency), operations management (retaining incident context), or any domain where an agent's work spans multiple sessions and accumulated context matters.

### 10.4 Evaluation Methodology

Evaluating memory systems by retrieval hit rate alone is insufficient. More relevant outcomes include reduction in repeated failed approaches, improved adherence to prior decisions, lower ramp-up latency after session boundaries, and stability under environmental change. Legend is designed around these behavioral outcomes.

## 11. Threats to Validity

The case study involves a single developer on a single project with no stateless control. Tick categorization was manual. Token savings are projected, not metered. Cross-domain generalization is not established. The neuroscience analogies are explicitly functional — each module implements a concrete algorithm motivated by the corresponding cognitive principle, but no claim is made of biological fidelity. 659 automated tests provide mechanism-level confidence but do not substitute for controlled behavioral evaluation.

## 12. Limitations

Legend is a heuristic, bounded, single-process system. Retrieval is linear scan, not approximate nearest-neighbor indexing. Entity extraction is pattern-driven, not AST-based. Salience uses rule-based priors, not learned objectives. The embedded MiniLM model adds ~23MB to binary size; a larger model (e.g., BGE-small, 12 layers) could improve retrieval quality at the cost of size and latency. Consolidation is extractive and can over-compress nuance. The system supports single-agent local workflows, not distributed multi-agent consensus.

These are acknowledged constraints that define a clear research agenda.

## 13. Future Work

The most direct extension is controlled evaluation: identical tasks with and without Legend, across multiple projects and domains. On the encoding side, optional AST-assisted extraction and hybrid learned/deterministic salience scoring. Architecturally, multi-edge connections between node pairs (encoding relationship facets independently, extending Kandel's structural plasticity model) and context-aware spreading activation. For broader adoption, multi-agent provenance and conflict-aware graph updates as agent teams become common.

## 14. Conclusion

Legend advances a specific claim: durable agent competence requires memory systems that are selective, relational, and updatable over time. The practical achievement is not infinite recall but controlled persistence under resource bounds. By coupling hierarchical timescales, pattern separation, reconsolidation, emotional valence, multi-hop spreading activation, sharp-wave ripple replay, and environment-aware cold starts — each grounded in a specific neuroscience principle — Legend provides a workable path from session-local intelligence to project-level continuity.

The system is operational, tested, and instrumented. The observational evidence from 218 sessions supports the design's core premise: that bounded selective memory outperforms unbounded accumulation for longitudinal work. The architecture, the code, and the hypotheses are ready for rigorous comparative evaluation.

## References

[1] N. F. Liu, K. Lin, J. Hewitt, A. Paranjape, M. Bevilacqua, F. Petroni, and P. Liang, "Lost in the Middle: How Language Models Use Long Contexts," *Transactions of the Association for Computational Linguistics*, vol. 12, pp. 157–173, 2024.

[2] E. R. Kandel, *In Search of Memory: The Emergence of a New Science of Mind*. W. W. Norton & Company, 2006. See also: E. R. Kandel, "The Molecular Biology of Memory Storage: A Dialogue Between Genes and Synapses," *Science*, vol. 294, pp. 1030–1038, 2001 (Nobel Lecture).

[3] J. L. McClelland, B. L. McNaughton, and R. C. O'Reilly, "Why There Are Complementary Learning Systems in the Hippocampus and Neocortex: Insights from the Successes and Failures of Connectionist Models of Learning and Memory," *Psychological Review*, vol. 102, no. 3, pp. 419–457, 1995.

[4] K. Nader, G. E. Schafe, and J. E. Le Doux, "Fear memories require protein synthesis in the amygdala for reconsolidation after retrieval," *Nature*, vol. 406, pp. 722–726, 2000.

[5] Z. Zhang, B. Chen, and others, "A Survey on Human-Inspired Long-Term Memory for AI Agents," *arXiv preprint arXiv:2501.13105*, 2025.

[6] J. Hawkins, S. Ahmad, and Y. Cui, "A Theory of How Columns in the Neocortex Enable Learning the Structure of the World," *Frontiers in Neural Circuits*, vol. 11, article 81, 2017. https://doi.org/10.3389/fncir.2017.00081

[7] J. Hawkins, *A Thousand Brains: A New Theory of Intelligence*. Basic Books, 2021.

[8] Numenta, "Thousand Brains Theory of Intelligence Companion Paper," 2018. https://www.numenta.com/resources/research-publications/papers/thousand-brains-theory-of-intelligence-companion-paper/
