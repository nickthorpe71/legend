# Legend: A Hierarchical Memory Architecture for Persistent LLM Software Engineering

**Author:** Nick Thorpe  
**Date:** March 2, 2026

## Abstract
Large language model agents are highly capable within a single interaction window, yet weak at longitudinal reasoning in real software projects. Their internal context is transient, while engineering work is cumulative, path-dependent, and saturated with tacit rationale. This paper presents Legend, a local-first hierarchical persistent memory architecture built to close that gap. Legend is LLM-native infrastructure: agents are the primary operators and humans are supervisory users, with the intended loop running machine-to-machine across session boundaries. Legend is not a passive archive but an adaptive memory process organized around three coupled timescales: immediate recency, short-term semantic continuity, and long-term relational structure. The system combines deterministic n-gram embeddings, salience-weighted retention, retrieval-induced reconsolidation, graph-based associative priming, and cold-start synchronization. We argue that these mechanisms jointly approximate persistent agent cognition under bounded compute and storage constraints. The central claim is both technical and theoretical: memory quality in software agents depends less on maximum retained volume than on selective persistence, structured relation formation, and controlled forgetting.

## 1. Introduction
The modern LLM coding assistant is paradoxical. It can produce high-quality code, reason about architecture, and recover from nontrivial bugs within a session, yet return in the next session with little direct memory of the previous day’s hard-won decisions and discoveries. This discontinuity is not merely inconvenient; it alters the economics and reliability of AI-assisted engineering. Teams waste time re-explaining context, prior trade-offs are rediscovered, rejected approaches are retried, and subtle constraints become probabilistically present rather than operationally guaranteed. The central limitation is not intelligence per turn, but continuity and consistency across turns.

The argument rests on a simple claim: stateful engineering behavior requires stateful memory infrastructure external to the model context window. Storage alone is not enough—engineering signals are noisy. Build logs, ephemeral experiments, incomplete hypotheses, and abandoned branches all compete with high-value artifacts such as decisions, bug postmortems, and architectural constraints. A memory system that indiscriminately accumulates text eventually degrades retrieval precision. In practice, such systems fail not because they forget too much, but because they remember without discrimination. Even very large context windows exhibit attention dilution and weak structural traversal for long-horizon project memory.

Legend was designed to address this. Its core design principle is that memory should behave as a dynamical system with differentiated retention, reinforcement, and decay, rather than as an append-only notebook. The architecture therefore separates immediate experience from medium-term semantic traces and long-term relational abstractions. This separation allows Legend to preserve continuity while still discarding low-value noise.

This distinction matters because Legend is not primarily a human note-taking tool, nor a passive context dump for LLMs. It is an operational substrate for agent behavior. Humans can inspect and steer the system, but the intended loop is machine-to-machine: the agent opens each session with `memory start`, records decisions with `memory tick`, and retrieves context through `memory query` before uncertain work. Legend is persistent cognitive scaffolding that the model both writes and reads.

## 2. Conceptual Framework
Legend is informed by a computational analogy to multi-stage cognition, without claiming biological fidelity. The analogy is strictly functional: different classes of engineering information require different persistence mechanisms.

The first premise is layered persistence. There is immediate recency—the volatile record of what just happened—which supports situational awareness and should be cheap to overwrite. There is short-term semantic memory, where related observations merge, strengthen, or decay based on utility. And there is long-term relational memory, where entities and their interactions become the durable substrate for associative retrieval. In this framing, the objective is not maximal retention but maximal future utility per retained trace.

The second premise is that forgetting is a feature, not a defect. Useful recall depends on actively reducing the influence of stale or low-signal traces. Legend operationalizes this with exponential decay, usage-sensitive reinforcement, threshold-based pruning, and periodic normalization.

The third premise is reconsolidation: retrieval should temporarily reopen memories for update. Engineering understanding is iterative—early beliefs about a bug are often incomplete, and later evidence should revise the same memory object rather than spawn independent duplicates. Legend encodes this with labile windows and in-place updates.

The fourth premise is that retrieval should be both semantic and relational. Semantic similarity can find textually adjacent notes, but many engineering insights are graph-structured: a failing test implies a module; a module implies an interface; an interface implies dependency constraints. Legend therefore combines vector similarity with typed graph expansion to recover indirectly relevant context.

## 3. System Architecture
Legend persists memory state in `.legend/memory.lz4` as a serialized Rust `MemoryState`. All memory operations are exposed through the `legend memory` command family and are designed to operate locally without network calls. The implementation uses deterministic algorithms and bounded data structures to maintain predictable runtime behavior.

Legend also includes an LLM-orchestration surface (`legend llm`) for selective augmentation tasks such as entity extraction and cluster summarization. The memory engine remains local and deterministic: it creates typed tasks with explicit schemas and acceptance rules, while model execution is performed by the calling agent runtime. Returned outputs are applied only after validation (shape, confidence, and safety checks), preserving deterministic fallback when augmented output is low quality.

The architecture is hierarchical. Layer 1 is an immediate FIFO buffer (`VecDeque<String>`) with capacity 256. It stores raw recent text with minimal interpretation, preserving recency at low overhead. Layer 2 is a short-term semantic store (`Vec<ShortTermEntry>`) with capacity 1024. Each entry includes text, summary, 256-dimensional embedding, salience, usage count, access metadata, reconsolidation metadata, optional source references, semantic density, and adaptive reinforcement state. Layer 3 is a long-term associative graph with up to 2048 nodes and 8192 edges, where nodes represent extracted entities and edges represent typed relationships reinforced over time.

Every significant event is recorded as a tick, and each tick updates all three layers through a deterministic pipeline. The same state object tracks a monotonic clock, current task context, recent retrieval IDs, and a Git synchronization anchor (`last_synced_sha`) for cold-start reconciliation.

## 4. Methods
### 4.1 End-to-End Data and State Flow
The central operational unit in Legend is `tick_impl(text, passive)`, which performs a full state transition over `MemoryState`. At the beginning of each call, the global clock is incremented, temporal decay is applied, expired labile states are stabilized, and periodic renormalization routines are run. The ordering is deliberate: Legend first updates the temporal geometry of existing memory, then integrates new evidence into that updated landscape. If non-passive, the event is appended to the session log and contributes to consolidation cadence; passive events affect learning but are excluded from user-facing history pressure.

Input text is chunked into approximately 200-character segments. For each chunk, Legend executes a fixed sequence: immediate-buffer insertion, embedding, salience scoring, optional reference extraction, reconsolidation check, normal matching path, graph update, local retrieval priming, and finally global pruning. Conceptually, this means every observation is simultaneously treated as raw recency, semantic evidence, and relational signal. The pipeline therefore avoids a common failure mode in memory tools where one subsystem updates while others lag.

### 4.2 Embedding Model and Similarity Geometry
Legend uses deterministic hashed n-gram embeddings of dimension 256. Word unigrams, character trigrams, and word bigrams are accumulated into buckets via FNV-1a and L2-normalized. The result is a lightweight lexical-semantic geometry in which technical identifiers, file paths, and recurring phrase structure remain strongly represented. This design intentionally privileges reproducibility and locality over broad language-world semantics. In software settings, where retrieval often depends on exact symbols and near-variants of symbols, this bias is usually beneficial.

Similarity is computed by cosine distance. Because hashing can create occasional accidental proximity, Legend adds a lexical overlap gate before merge/reinforce actions. This hybrid criterion is one of the core precision controls in the system.

### 4.3 Salience as Utility Prior
At insertion time, salience is initialized by content heuristics that favor decision rationale, bugs, blockers, architecture notes, explicit preferences, and code/error-bearing text. Formally, this is a prior on expected future utility. Salience is bounded to `[0.05, 1.0]` to prevent dead entries and runaway dominance.

Temporal evolution then applies exponential decay. For short-term entry \(i\), the implementation uses base rate `0.001`, modulated by a `density` factor computed as the weighted count of high-signal entities (file paths, functions, structs) in the entry's text, so symbol-rich memories decay up to 2× more slowly. Long-term nodes and edges decay with rate `0.0005`. The practical effect is a layered half-life system: tactical traces fade unless reused, while structural abstractions persist longer. This solves the accumulation problem without hard deletion of everything old.

When explicit feedback is provided via `memory reinforce`, salience updates use an AdaGrad-style adaptive learning rate. Entries accumulate a squared-gradient sum across reinforcements; the effective learning rate is `0.15 / sqrt(sq_sum + epsilon)`. This prevents frequently-reinforced memories from saturating near 1.0 while allowing rarely-reinforced ones to respond strongly to a single positive signal.

### 4.4 Matching, Merge Control, and Reconsolidation
After embedding, Legend first attempts reconsolidation against currently labile entries. Labile entries are those recently retrieved and therefore marked editable for `LABILE_WINDOW = 5` ticks. If similarity is at least `0.35` and lexical overlap is at least `0.1`, the existing entry is updated in place: text is merged or summarized if too long, embedding is recomputed, salience is incremented, and reconsolidation count increases. This is a direct defense against duplicate memory proliferation during iterative investigation.

If reconsolidation does not trigger, Legend performs best-match lookup over short-term memory and applies dual-threshold control. For similarity above `theta_high = 0.92` with sufficient lexical overlap, the entry is reinforced; for similarity between `theta_low = 0.55` and `theta_high`, embeddings and summaries are merged; otherwise a new entry is inserted. The overlap requirement (`>= 0.3`) is decisive: it prevents collision-induced false merges and preserves representational diversity.

### 4.5 Graph Construction: From Mentions to Structure
Each chunk also updates long-term graph memory through multi-pass entity extraction. The extractor recognizes file paths, action verbs, environment markers, language constructs (`fn`, `struct`, `class`, `trait`, imports, modules), assignment/decorator cues, and residual identifiers after stopword filtering. Nodes are typed and weighted by specificity; for example, concrete code symbols and file paths receive stronger reinforcement than generic terms.

Edges are inferred from co-occurrence context and carry one of seven typed relationships: `contains`, `depends-on`, `implements`, `co-defined`, `drives`, `represents`, or `related`. The type is resolved from extraction context—`defines + mentions` yields `contains`, `uses` yields `depends-on`, `implements` yields `implements`, `defines + defines` yields `co-defined`, `performs` yields `drives`—and consolidation creates `represents` edges linking summary nodes to their dominant topic anchors. Subsequent co-occurrence strengthens existing edges. The graph therefore encodes not only which entities appear, but how they repeatedly co-appear in working context, which is a stronger predictor of future relevance than frequency alone.

### 4.6 Retrieval Flow and Associative Priming
`retrieve_context(query)` is itself a learning event, not a read-only call. It increments clock time, applies decay, embeds the query, ranks short-term entries by cosine similarity, and marks returned snippets as labile. The top result receives a passive salience increment proportional to similarity (`+ sim * 0.03`) when similarity exceeds 0.2. This ensures that repeatedly useful memories become easier to recover without explicit manual reinforcement.

Long-term retrieval proceeds in two linked stages. First, direct graph lookup extracts entities from the query and expands one hop over connected nodes, producing an initial result set. Second, associative priming builds a seed set from both the initial graph results and entities extracted from the retrieved short-term snippets, then follows one additional hop for neighbors above edge-weight threshold (`>= 0.15`), with a score discount (`0.7x`) to avoid overwhelming direct matches. The returned set is deduplicated, reranked, and capped. Finally, co-retrieved graph nodes receive Hebbian reinforcement (`+0.05` edges, `+0.02` nodes, with ceilings), closing the loop between usage and structure.

The causal reason this works is that many engineering queries are underspecified. A developer may ask about a bug symptom, while the relevant memory lies in a dependency interaction or environment constraint. Priming provides this bridge from lexical cues to structural neighborhoods.

### 4.7 Consolidation and Abstraction Formation
Consolidation runs by clustering short-term entries at similarity `>= theta_low` and generating summary nodes for multi-entry groups. For each group, Legend builds an extractive summary from the most salient/used members, stores source texts, assigns durable initial weight, and may attach topic anchors when entities recur across a majority of grouped entries. The process transforms repeated local events into compressed long-term abstractions that are easier to retrieve than dozens of near-duplicate tactical notes.

Consolidation is suggested after fifteen non-passive ticks, reflecting an empirical balance: too frequent consolidation over-compresses active work; too infrequent consolidation leaves short-term memory fragmented.

Separately, `memory tick` can emit LLM augmentation triggers when input text is long and weakly anchored by high-signal entities. This path is intentionally guarded by three controls: duplicate suppression via text fingerprint and task kind, per-kind pending limits, and tick-gap rate limiting. Together they prevent queue amplification under repetitive or low-signal input while still surfacing genuine extraction opportunities.

### 4.8 Boundedness and Stability Guarantees
Legend enforces bounded memory with explicit capacities and pruning functions. Short-term entries are removed when their composite survival score drops below threshold; graph nodes/edges are pruned by decayed weight and hard-cap limits. Periodic salience renormalization and graph weight normalization prevent reinforcement blow-up.

A complementary mechanism is contrastive descent: entries that are retrieved but not explicitly reinforced receive a small salience penalty (`-0.02`), distinguishing confirmed-useful memories from those that merely surface without proving relevant. Together, these controls ensure that learning dynamics remain stable over long sessions and that retrieval quality does not collapse under accumulation pressure.

### 4.9 Worked Trace: One Memory Through the System
To make the mechanics concrete, consider an agent that logs a decision tick: “DECISION: switched retry strategy to exponential backoff because fixed delay caused API 429 bursts.” At tick time, the text is chunked and inserted into Layer 1, embedded, and scored with high salience due to decision+rationale cues. Suppose no strong prior match exists; a new short-term entry is inserted with a fresh ID and initial usage of 1. Entity extraction yields terms such as `retry`, `exponential_backoff`, and `API`, which create or reinforce graph nodes and edges.

Later, possibly in a new session, the agent issues `memory query "rate limit spikes on retry"`. The prior entry ranks highly in short-term similarity, is returned in top-k, and enters labile state for five ticks. The top result also receives a small auto-reinforcement increment. Graph lookup finds direct and neighboring nodes linked to rate limiting and retry policy; associative priming adds adjacent nodes connected by strong edges, improving recall of related constraints.

Immediately afterward, the agent records a follow-up tick: “DECISION: kept exponential backoff and added jitter to prevent synchronized retries.” Because the first entry is labile and semantically related, reconsolidation updates that existing entry in place instead of creating a duplicate. Text is merged or summarized, embedding is recomputed, salience increases, and reconsolidation count increments. Over subsequent work, repeated access strengthens connected graph edges through Hebbian updates. During consolidation, this family of related short-term traces may be compressed into a summary node, preserving the design narrative while reducing tactical clutter.

This trace illustrates why the pipeline is effective in practice: it captures initial rationale, supports revision, preserves relational context, and eventually compresses repeated local events into durable abstractions.

## 5. Cold-Start Continuity and the Observer Gap
A major failure mode of agent workflows appears when humans modify the repository while the agent is offline. At the next session, the agent’s internal model is stale. Legend addresses this observer gap through Git-aware cold-start synchronization. On `memory start`, the system compares current HEAD to the last synchronized SHA, reports intervening commits, summarizes uncommitted changes, and presents this context alongside current task and categorized high-signal memories. The result is not merely historical reporting; it is operational reconciliation that lets the agent re-enter a moving codebase with reduced epistemic drift.

## 6. Implementation Integrity
Legend’s persistence path emphasizes robustness. Memory is serialized with bincode, compressed with LZ4, and written atomically via temporary file and rename. Corrupt stores are quarantined to a `.corrupt` backup path, and migration fallbacks are implemented to avoid repeated startup loops when historical schema variants are encountered. These guarantees matter because memory infrastructure that fails under normal interruption patterns cannot support longitudinal cognition in practice.

The system also exposes a structured event log (`.legend/events.jsonl`) capturing every `tick`, `query`, `reinforce`, `consolidate`, and `start` operation. This enables dashboard observability and session-quality introspection without external telemetry services. LLM orchestration state is persisted separately: pending tasks live in `.legend/llm_tasks.json`, and completed tasks are archived to compressed storage, keeping active orchestration overhead bounded regardless of history length.

## 7. Operational Roles: Agent and Human
Legend is agent-operated by design. The expected steady-state loop is autonomous at the memory layer: the LLM starts each session with `memory start`, writes high-signal events through `memory tick`, and retrieves context with `memory query` before entering unfamiliar parts of the codebase. In this role, the model is both producer and consumer of memory, and the quality of longitudinal behavior depends on the consistency of that loop.

The human role is governance rather than manual memory maintenance. Developers set priorities, review outcomes, and intervene when policy or reliability concerns require it. This includes validating critical claims, correcting drift, and deciding when structural changes warrant explicit architecture ticks. In failure handling, humans arbitrate exceptional cases such as migration incidents, ambiguous project context, or evidence quality concerns. This division of labor keeps the memory system agent-native while preserving human accountability.

## 8. Discussion
Legend suggests that persistent agent performance is better modeled as memory governance than memory accumulation. The architecture intentionally introduces friction against indiscriminate retention—thresholds, decay, pruning, and normalization all act as quality controls. In this sense, Legend resembles an information metabolism: useful traces are fed, reinforced, and integrated, while low-value traces are attenuated.

The interplay between reconsolidation and graph priming is particularly important. Reconsolidation keeps individual memories current as understanding changes, while priming surfaces structural neighbors beyond lexical overlap. Together they allow the system to preserve both evolving local detail and stable global topology.

### 8.1 Mechanistic Coupling
Legend's practical effectiveness emerges from the coupling of four mechanisms rather than any single innovation. The first is selective persistence: salience scoring, density-modulated decay, and contrastive descent together make it easier for high-information traces—especially decisions and incident narratives—to survive long enough to be useful in later sessions. The second is plasticity through reconsolidation: when agents refine understanding iteratively, Legend mutates existing memories instead of creating uncontrolled duplicates, improving signal density within bounded storage. The third is relational expansion: associative priming allows retrieval to recover architectural neighbors that may not be textually similar to the prompt, reducing the mismatch between lexical queries and structural code reality. The fourth is cold-start synchronization against Git state, which reduces epistemic discontinuity when repository evolution happens outside active agent runtime.

In combination, these mechanisms create a stable behavior loop: the agent records rationale-rich traces, retrieval preferentially surfaces those traces, reinforcement increases their future availability, and stale noise decays. The system does not need to remember everything; it needs to remember the right things often enough to shape subsequent decisions.

A broader implication is methodological. Evaluating memory systems only by retrieval hit rate is insufficient for engineering agents. More relevant outcomes include reduction in repeated failed approaches, improved adherence to prior architectural decisions, lower ramp-up latency after session boundaries, and stability under repository change between sessions. Legend is designed around these behavioral outcomes.

## 9. Evaluation Protocol
The paper’s claims can be evaluated against explicit hypotheses rather than narrative inspection alone.

**H1 — Continuity.** Sessions with Legend memory operations should require less manual re-onboarding than sessions without them, measured by prompt-token overhead and the frequency of repeated-context prompts.

**H2 — Decision retention.** Prior design decisions should be recoverable in later sessions at higher rates than a stateless baseline, measured by retrieval hit rate for previously logged decision ticks and reduced reintroduction of rejected approaches.

**H3 — Stability.** Memory growth and retrieval quality should remain bounded under sustained use, measured by capped store size behavior, pruning effectiveness, and query relevance over time.

Each hypothesis can be tracked with reproducible command outputs (`memory start --tokens`, `memory stats`, `memory sessions`, and test runs), fixed sampling windows, and explicit uncertainty declarations where estimates are used. This protocol is not yet a randomized controlled study, but it is falsifiable and suitable for iterative engineering validation.

## 10. Empirical Outcomes: A Case Study in Strategy Game Development
Current evidence comes from longitudinal operational use across multiple repositories and from the project’s built-in telemetry surfaces (`events.jsonl`, token overhead estimator, storage statistics, and tests). These results should be interpreted as engineering evidence rather than controlled academic trials, but they are concrete and reproducible within the project tooling.

### 10.1 Longitudinal Metrics
In a case study involving the development of a complex, terminal-based strategy game, a 14-day analysis of the `events.jsonl` telemetry revealed a sustained high-intensity development cycle:
- **Total Duration:** 13.9 days of active development.
- **Session Continuity:** 198 distinct sessions successfully initialized and synchronized via Legend hooks.
- **Memory Density:** 950 total ticks recorded, of which 949 (99.9%) were manual high-signal entries.
- **Knowledge Categorization:** 144 bug root-cause analyses, 19 architectural refactors, and 15 core design decisions were explicitly persisted and retrieved across session boundaries.

At the time of sampling, the memory store for this project remained extremely lean: `.legend/memory.lz4` occupied 217KB, despite representing nearly 200 sessions of complex path-dependent work.

### 10.2 Token Efficiency and Net Savings
Session-start token overhead is estimated by built-in heuristics. Across the strategy game case study, Legend maintained an average session-start injection of approximately 1,112 tokens.

Estimated net savings reached approximately **150,000+ tokens** over the 198-session history. This is calculated by comparing the automated injection against a conservative manual context reconstruction baseline (3,000–5,000 tokens) required for an agent to regain equivalent situational awareness in a stateless workflow. This represents a ~65% reduction in "re-onboarding" costs.

### 10.3 Behavioral Evidence
Beyond aggregate metrics, the telemetry reveals three recurring behavioral patterns that directly reflect the mechanisms described in Section 8.1. First, decision stability: the agent consistently recalled domain-specific constants and design choices (e.g., "Mixed damage takes max(phys, mag) instead of average") that would otherwise have been lost or hallucinated between sessions—evidence that selective persistence and reconsolidation are preserving high-salience traces across session boundaries. Second, failure path avoidance: recorded root-cause analyses (e.g., a dead-end approach in terrain generation) were persisted with sufficient relational context that subsequent sessions surfaced them before the failed approach was retried—evidence of effective graph-primed retrieval. Third, task hand-off efficiency: the 198-session continuity was maintained with zero manual re-onboarding prompts, with the agent autonomously resuming multi-day tasks from retrieved checklists and reproduction steps—evidence of cold-start synchronization functioning as intended.

Taken together, these outcomes support a moderate conclusion: Legend already delivers measurable continuity and efficiency gains in real workflows, while still requiring more formal evaluation and ongoing hardening for publication-grade claims.

## 11. Threats to Validity and Reliability Status
The current evidence base has limits. Some headline outcomes are estimator-derived rather than directly metered, and therefore carry uncertainty. Cross-project comparisons are observational and may confound workload shape, team behavior, and repository complexity. Sampling windows are also uneven across projects.

The reliability posture is well-established but not yet comprehensive. The codebase is covered by an automated test suite (`cargo test`), with one known stub (`storage::tests::test_load_nonexistent`) that exercises load of a non-existent file and is currently expected to fail. This does not invalidate the architecture, but it sets a clear boundary on current reliability claims.

## 12. Limitations
Legend remains a heuristic, bounded, single-process system. Short-term retrieval is linear scan over capped entries rather than approximate nearest-neighbor indexing. Entity extraction is pattern-driven rather than parser-complete AST analysis, and salience scoring uses rule-based priors rather than learned objective functions. Consolidation is extractive and can compress nuanced arguments too aggressively in some cases. LLM augmentation is likewise bounded by design: triggering and task generation are integrated, but external model execution and result submission remain orchestrator-dependent. Finally, Legend currently optimizes for local single-agent workflows rather than distributed multi-agent consensus memory.

These are acknowledged constraints, not hidden defects, and they define a clear research agenda.

## 13. Future Work
The most direct extension is a rigorous evaluation methodology linking memory interventions to downstream engineering outcomes—bug recurrence rates, architectural regression frequency, and session-ramp latency under controlled conditions. On the extraction side, optional AST-assisted entity recognition would yield higher-precision graph typing in strongly typed languages, replacing the current pattern-matching heuristics with structure-aware parsing. Adaptive salience could be upgraded from rule-based priors to hybrid learned controllers while preserving deterministic fallback for reliability. Storage-level encryption and key lifecycle policy are already an active direction. Looking further ahead, multi-agent provenance, arbitration, and conflict-aware graph updates represent the next major frontier: as agent teams become common, shared memory infrastructure will require principled merge semantics and trust boundaries.

## 14. Conclusion
Legend advances a specific claim about AI software agents: durable engineering competence requires memory systems that are selective, relational, and updatable over time. The practical achievement is not infinite recall but controlled persistence under resource bounds. By coupling hierarchical timescales, reconsolidation, associative priming, and repository-aware cold starts, Legend provides a workable path from session-local intelligence to project-level continuity.

If stateless reasoning is a snapshot, Legend is the mechanism that turns snapshots into history, and history into usable structure.
