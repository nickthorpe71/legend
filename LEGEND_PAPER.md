# Legend: A Hierarchical Memory Architecture for Persistent AI Software Engineering

**Author:** Nick Thorpe  
**Date:** March 2, 2026

## Abstract
Large language model agents are highly capable within a single interaction window, yet they remain fundamentally weak at longitudinal reasoning inside real software projects. Their internal context is transient, while engineering work is cumulative, path-dependent, and rich in tacit rationale. This paper presents Legend, a local-first hierarchical memory architecture designed to reduce this mismatch. Legend is not framed as a passive archival store but as an adaptive memory process with three coupled timescales: immediate recency, short-term semantic continuity, and long-term relational structure. The system combines deterministic n-gram embeddings, salience-weighted retention, retrieval-induced reconsolidation, graph-based associative priming, and Git-aware cold-start synchronization. We argue that these mechanisms jointly produce a practical approximation of persistent agent cognition under bounded compute and storage constraints. The contribution of Legend is therefore both technical and theoretical: memory quality in software agents is shown to depend less on maximum retained volume than on selective persistence, structured relation formation, and controlled forgetting.

## 1. Introduction
The modern coding assistant is paradoxical. It can produce high-quality code, reason about architecture, and recover from nontrivial bugs within a session, yet return in the next session with little direct memory of the previous day’s hard-won decisions. This discontinuity is not merely inconvenient; it alters the economics and reliability of AI-assisted engineering. Teams re-explain context, prior trade-offs are rediscovered, rejected approaches are retried, and subtle constraints become probabilistically present rather than operationally guaranteed. The central limitation is not intelligence per turn, but continuity across turns.

This paper begins from a simple claim: stateful engineering behavior requires stateful memory infrastructure external to the model context window. However, adding storage alone is insufficient. Engineering signals are noisy. Build logs, ephemeral experiments, incomplete hypotheses, and abandoned branches all compete with high-value artifacts such as decisions, bug postmortems, and architectural constraints. A memory system that indiscriminately accumulates text eventually degrades retrieval precision. In practice, this means that memory systems fail not because they forget too much, but because they remember without discrimination.

Legend was designed to address this failure mode. Its core design principle is that memory should behave as a dynamical system with differentiated retention, reinforcement, and decay, rather than as an append-only notebook. The architecture therefore separates immediate experience from medium-term semantic traces and long-term relational abstractions. This separation allows Legend to preserve continuity while still discarding low-value noise.

## 2. Conceptual Framework
Legend is informed by a computational analogy to multi-stage cognition, but it does not claim biological fidelity. The analogy is used functionally: different classes of engineering information require different persistence mechanisms.

First, there is immediate recency, the volatile record of what just happened. This layer supports situational awareness and should be cheap to overwrite. Second, there is short-term semantic memory, where related observations should merge, strengthen, or decay based on utility. Third, there is long-term relational memory, where entities and their interactions become the durable substrate for associative retrieval. In this framing, the objective is not maximal retention but maximal future utility per retained trace.

A second premise is that forgetting is a feature, not a defect. Under finite memory budgets, useful recall depends on actively reducing the influence of stale or low-signal traces. Legend operationalizes this with exponential decay, usage-sensitive reinforcement, threshold-based pruning, and periodic normalization.

A third premise is reconsolidation: retrieval should temporarily reopen memories for update. Engineering understanding is iterative. Early beliefs about a bug are often incomplete; later evidence should revise the same memory object rather than spawn independent duplicates. Legend encodes this with labile windows and in-place updates.

Finally, retrieval should be both semantic and relational. Semantic similarity can find textually adjacent notes, but many engineering insights are graph-structured: a failing test implies a module; a module implies an interface; an interface implies dependency constraints. Legend therefore combines vector similarity with typed graph expansion to recover indirectly relevant context.

## 3. System Architecture
Legend persists memory state in `.legend/memory.lz4` as a serialized Rust `MemoryState`. All memory operations are exposed through the `legend memory` command family and are designed to operate locally without network calls. The implementation uses deterministic algorithms and bounded data structures to maintain predictable runtime behavior.

The architecture is hierarchical. Layer 1 is an immediate FIFO buffer (`VecDeque<String>`) with capacity 256. It stores raw recent text with minimal interpretation, preserving recency at low overhead. Layer 2 is a short-term semantic store (`Vec<ShortTermEntry>`) with capacity 1024. Each entry includes text, summary, 256-dimensional embedding, salience, usage count, access metadata, reconsolidation metadata, optional source references, and adaptive reinforcement state. Layer 3 is a long-term associative graph with up to 2048 nodes and 8192 edges, where nodes represent extracted entities and edges represent typed relationships reinforced over time.

The architecture is unified by a single ingestion primitive: the tick. Every significant event can be recorded as a tick, which updates all three layers through a deterministic pipeline. The same state object tracks a monotonic clock, current task context, recent retrieval IDs, and Git synchronization anchor (`last_synced_sha`) for cold-start reconciliation.

## 4. Methods
### 4.1 Ingestion Pipeline
On each tick, Legend increments clock time, applies decay, stabilizes expired labile entries, and performs periodic normalization passes. Input text is chunked to approximately 200-character segments to preserve locality while preventing oversized traces. Each chunk is pushed into the immediate buffer, embedded, scored for salience, and processed through reconsolidation or insertion logic. The graph is updated with extracted entities and typed edges, and retrieval is executed to provide immediate context priming. Finally, short-term and long-term pruning enforce quality and bounded size.

Passive ticks are supported for background signals. They update semantic and graph memory while avoiding session-log pollution and avoiding inflation of consolidation cadence.

### 4.2 Embedding Model
Legend uses deterministic n-gram hashing embeddings. Word unigrams, character trigrams, and word bigrams are hashed with FNV-1a into a 256-dimensional vector and L2-normalized for cosine similarity. This approach intentionally prioritizes locality, deterministic behavior, and zero external dependencies over deep semantic generalization. In software projects, this tradeoff is often favorable because identifier-level overlap and structural token patterns carry high value.

### 4.3 Salience and Retention Dynamics
Salience initialization is heuristic. Decision language, rationale markers, bug terminology, blocker markers, architecture cues, preference statements, and code/error indicators increase salience; scores are then clamped to `[0.05, 1.0]`. This can be understood as a prior over expected future utility.

Retention evolves through exponential decay with density modulation in short-term memory and slower decay in long-term graph weights. Short-term decay uses base rate `0.001`; long-term node and edge decay use base rate `0.0005`. Entries are pruned when composite survival scores fall below threshold, and graph pruning removes weak or stale nodes while enforcing hard capacity constraints.

### 4.4 Matching, Merging, and Reconsolidation
For non-labile matching, Legend uses dual thresholds with lexical overlap gating. At high similarity (`theta_high = 0.92`) and adequate overlap, existing memories are reinforced; at moderate similarity (`theta_low = 0.55`) and overlap, embeddings and summaries merge; otherwise, new entries are inserted. The overlap gate reduces false merges when embedding collisions produce superficial similarity.

Reconsolidation operates on recently retrieved entries that remain labile for five ticks. If new information reaches similarity at least `0.35` with minimum overlap, the existing memory is updated in place, embedding is recomputed, salience is boosted, and reconsolidation count is incremented. This mechanism captures iterative refinement without proliferating near-duplicate traces.

### 4.5 Graph Construction and Typed Relations
Entity extraction is multi-pass and code-aware. It recognizes file paths, action verbs, environment markers, language-level constructs (`fn`, `struct`, `class`, `trait`, imports, modules), assignment patterns, decorators, and residual identifiers filtered by stopword rules. Extracted entities become graph nodes with kind-specific weighting. Co-occurring entities produce typed edges such as `depends-on`, `implements`, `contains`, `co-defined`, or `related`. Repeated co-occurrence reinforces edge weights.

### 4.6 Retrieval and Associative Priming
Retrieval combines vector and graph semantics. Given a query, Legend returns top short-term snippets by cosine similarity and marks them labile. The highest match may receive automatic salience reinforcement proportional to similarity. Graph lookup then expands from query entities and from entities found in retrieved snippets, adding one-hop neighbors above edge-strength threshold and discounting their score to preserve rank discipline. Co-retrieved graph nodes trigger Hebbian reinforcement (`+0.05` edge, `+0.02` node with ceilings), biasing future retrieval toward recurring architectural pathways.

### 4.7 Consolidation
Consolidation clusters similar short-term entries and generates summary nodes in long-term memory. Summary nodes retain source texts and may connect to emergent topic anchors detected from frequently recurring entities in the cluster. This process compresses repeated local activity into durable abstractions. A suggestion threshold of fifteen ticks indicates when consolidation is likely beneficial.

## 5. Cold-Start Continuity and the Observer Gap
A major failure mode of agent workflows occurs when humans modify the repository while the agent is offline. At the next session, the agent’s internal model is stale. Legend addresses this observer gap through Git-aware cold-start synchronization. On `memory start`, the system compares current HEAD to the last synchronized SHA, reports intervening commits, summarizes uncommitted changes, and presents this context alongside current task and categorized high-signal memories. The result is not merely historical reporting; it is operational reconciliation that allows the agent to re-enter a moving codebase with reduced epistemic drift.

## 6. Implementation Integrity
Legend’s persistence path emphasizes robustness. Memory is serialized with bincode, compressed with LZ4, and written atomically via temporary file and rename. Corrupt stores are quarantined to a `.corrupt` backup path, and migration fallbacks are implemented to avoid repeated startup loops when historical schema variants are encountered. These guarantees matter because memory infrastructure that fails under normal interruption patterns cannot support longitudinal cognition in practice.

The system also exposes structured events (`tick`, `query`, `reinforce`, `consolidate`, `start`) through `.legend/events.jsonl`, enabling dashboard observability and session-quality introspection without external telemetry services.

## 7. Discussion
Legend suggests that persistent agent performance is better modeled as memory governance than memory accumulation. The architecture intentionally introduces friction against indiscriminate retention: thresholds, decay, pruning, and normalization all serve as quality controls. In this sense, Legend resembles an information metabolism. Useful traces are fed, reinforced, and integrated; low-value traces are attenuated.

The interplay between reconsolidation and graph priming is particularly important. Reconsolidation keeps individual memories current as understanding changes, while priming surfaces structural neighbors beyond lexical overlap. Together they allow the system to preserve both evolving local detail and stable global topology.

A broader implication is methodological. Evaluating memory systems only by retrieval hit rate is insufficient for engineering agents. More relevant outcomes include reduction in repeated failed approaches, improved adherence to prior architectural decisions, lower ramp-up latency after session boundaries, and stability under repository change between sessions. Legend is designed around these behavioral outcomes.

## 8. Why the System Works in Practice
Legend’s practical effectiveness appears to come from a coupling of mechanisms rather than any single innovation. The first mechanism is selective persistence: salience scoring and decay make it easier for high-information traces, especially decisions and incident narratives, to survive long enough to be useful in later sessions. The second mechanism is plasticity through reconsolidation: when developers and agents refine understanding iteratively, Legend mutates existing memories instead of creating uncontrolled duplicates, which improves signal density in a bounded store. The third mechanism is relational expansion: associative priming allows retrieval to recover architectural neighbors that may not be textually similar to the prompt, reducing the mismatch between lexical queries and structural code reality. The fourth mechanism is cold-start synchronization against Git state, which reduces epistemic discontinuity when repository evolution happens outside active agent runtime.

In combination, these mechanisms create a stable behavior loop: the agent records rationale-rich traces, retrieval preferentially surfaces those traces, reinforcement increases their future availability, and stale noise decays. This loop is why Legend can remain small while preserving project continuity. The system does not need to remember everything; it needs to remember the right things often enough to shape subsequent decisions.

## 9. Empirical Outcomes So Far
Current evidence comes from operational use across multiple repositories and from the project’s built-in telemetry surfaces (`events.jsonl`, token overhead estimator, storage statistics, and tests). These results should be interpreted as engineering evidence rather than controlled academic trials, but they are concrete and reproducible within the project tooling.

First, token economics appear favorable, but with significant project-dependent variation. Previously observed session-start injection costs were approximately 1,099 tokens in the Legend repository itself, approximately 1,112 in `test-game`, and approximately 345 in `spritec` under sparse-memory conditions. In the additional `scrapingbee_mvp` project snapshot (March 2, 2026), `legend memory start --tokens` reported a session-start injection near 1,146 tokens and an estimated total per-session overhead near 2,196 tokens once hook reminders are included, across an observed lifetime of 52 sessions. This result strengthens, rather than weakens, the theoretical framing: overhead is not constant and scales with memory density and usage patterns. The relevant comparison is therefore not “small overhead in all cases,” but whether overhead remains lower than repeated manual context reconstruction at equivalent project complexity.

Second, measured benefit remains visible in cumulative terms where usage is sustained. Using the same estimator framework, prior reported net savings were approximately 53,400 tokens over 88 sessions in `test-game` and approximately 27,600 tokens over 20 sessions in `spritec`. These estimates carry the documented uncertainty band (roughly +/-30%), so they should be interpreted as directional evidence, not exact accounting. Even under that uncertainty, the observed order of magnitude supports the claim that structured persistent memory can reduce repetitive re-briefing cost.

Third, storage footprint and state growth remain bounded in practical operation. In `scrapingbee_mvp`, direct inspection showed `.legend/memory.lz4` around 288KB and `.legend/events.jsonl` around 64KB at the sampled time, with memory stats reporting 217 short-term entries and an 832-node / 7,573-edge long-term graph. This is consistent with the architecture’s bounded-capacity thesis: even active multi-day usage remains within small local artifacts rather than requiring external infrastructure.

Fourth, the observability layer is contributing useful quality control, even when no immediate work is happening. In the sampled `scrapingbee_mvp` session, the current-session quality score was 0/100 because no ticks or queries had yet occurred after the last `start`; this is expected behavior and demonstrates that the metric is sensitive to session process, not merely historical volume. In other words, Legend can detect under-engaged sessions just as it can reward high-signal workflows.

Fifth, implementation correctness is strong but not yet perfect. On March 2, 2026, a direct `cargo test -q` run in the Legend codebase executed 95 tests with 94 passing and 1 failing (`storage::tests::test_load_nonexistent`). This result matters for scientific honesty: the architecture and most mechanisms are test-backed, but at least one live regression remains and should be resolved before making stronger reliability claims.

Taken together, these outcomes support a moderate conclusion: Legend already delivers measurable continuity and efficiency benefits in real workflows, while still requiring more formal evaluation and ongoing hardening for publication-grade claims.

## 10. Limitations
Legend remains a heuristic, bounded, single-process system. Short-term retrieval is linear scan over capped entries rather than approximate nearest-neighbor indexing. Entity extraction is pattern-driven rather than parser-complete AST analysis, and salience scoring uses rule-based priors rather than learned objective functions. Consolidation is extractive and can compress nuanced arguments too aggressively in some cases. Finally, Legend currently optimizes for local single-agent workflows rather than distributed multi-agent consensus memory.

These are acknowledged constraints, not hidden defects, and they define a clear research agenda.

## 11. Future Work
A natural extension is richer evaluation methodology linking memory interventions to downstream engineering outcomes such as bug recurrence and architectural regression rates. Another is optional AST-assisted extraction for high-precision graph typing in strongly typed languages. Adaptive salience could be upgraded from heuristic priors to hybrid learned controllers while preserving deterministic fallback. Storage-level encryption and key lifecycle policy are already an active direction. Multi-agent provenance, arbitration, and conflict-aware graph updates represent the next major frontier for collaborative agent ecosystems.

## 12. Conclusion
Legend advances a specific claim about AI software agents: durable engineering competence requires memory systems that are selective, relational, and updateable over time. The practical achievement is not infinite recall but controlled persistence under resource bounds. By coupling hierarchical timescales, reconsolidation, associative priming, and repository-aware cold starts, Legend provides a workable path from session-local intelligence to project-level continuity.

If stateless reasoning is a snapshot, Legend is the mechanism that turns snapshots into history, and history into usable structure.

## Appendix: Canonical Parameters in Current Implementation
Legend currently uses the following defaults: immediate buffer capacity 256, short-term capacity 1024, embedding dimension 256, matching thresholds `0.92` and `0.55`, reconsolidation threshold `0.35`, labile window 5 ticks, graph caps 2048 nodes and 8192 edges, short-term decay rate `0.001`, long-term decay rate `0.0005`, and consolidation suggestion threshold 15 ticks.
