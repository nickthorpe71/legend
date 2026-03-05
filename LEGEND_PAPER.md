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

## 10. Empirical Outcomes: A Case Study in Tactical RPG Development

Current evidence comes from longitudinal operational use and from Legend’s built-in telemetry surfaces (`events.jsonl`, storage statistics, and graph introspection). The primary case study tracks the full development lifecycle of a Fire Emblem-inspired tactical RPG built in Rust, from initial concept through multiple architectural pivots to a feature-complete prototype. These results should be interpreted as engineering evidence rather than controlled academic trials, but they are concrete, reproducible within the project tooling, and internally consistent across multiple observational dimensions.

### 10.1 Project Context and Methodology

The subject project is a pixel-art tactical RPG featuring procedural map generation, a music composition system (MML-based), unit bonding mechanics, and a biome-driven visual theming engine. Development was conducted by a single developer working with an LLM agent across 218 sessions over 16.4 days. Legend was integrated from project inception via `CLAUDE.md` mandate, meaning all sessions operated under the full memory protocol: `memory start` at session open, `memory tick` after significant actions, `memory query` before unfamiliar work, and summary ticks at session close.

The project underwent a major architectural pivot on day one—shifting from a tower defense game to a Fire Emblem-style tactics game—and subsequently evolved through multiple feature phases including procedural generation, sprite rendering, campaign progression, and music systems. This pivot-rich trajectory makes it a strong test case for Legend’s continuity and decision-retention claims.

All metrics below are derived from the project’s `events.jsonl` telemetry log (1,410 total events) and `.legend/memory.lz4` graph state.

### 10.2 Aggregate Metrics

| Metric | Value |
|--------|-------|
| Active development duration | 16.4 days (394 hours) |
| Sessions initialized | 218 |
| Ticks recorded | 1,055 |
| Explicit queries issued | 76 |
| Auto-consolidations triggered | 57 |
| Manual consolidations | 4 |
| Memory groups merged | 239 |
| Average ticks per session | 4.8 |
| Query-to-tick ratio | 1:13.8 |
| Total event log size | 1.38 MB |
| Compressed memory graph size | 0.15 MB |
| Storage compression ratio | 9.2:1 (graph vs. raw log) |

The memory graph remained bounded at 0.15 MB despite encoding 218 sessions of path-dependent work, confirming that Legend’s pruning and consolidation mechanisms prevent unbounded growth under sustained use (Section 4.8).

### 10.3 Tick Content Categorization

Manual classification of tick content reveals the distribution of recorded knowledge:

| Category | Count | Proportion | Description |
|----------|-------|------------|-------------|
| Feature implementation | 191 | 18.1% | New systems and capabilities built |
| Bug reports and root-cause analyses | 119 | 11.3% | Problems discovered, symptoms documented |
| Session summaries | 74 | 7.0% | End-of-session state and next-steps |
| Architecture decisions | 38 | 3.6% | Structural choices with rationale |
| Explicit decisions with rationale | 23 | 2.2% | Deliberate trade-off documentation |
| Discussion conclusions | 16 | 1.5% | User–agent agreements on direction |
| Bug fixes | 13 | 1.2% | Resolutions with root-cause linkage |
| Blockers | 12 | 1.1% | External dependencies, stuck points |
| Discoveries | 5 | 0.5% | Insights without corresponding file changes |
| Contextual and directional | 564 | 53.5% | User feedback, reference material, direction changes |

The 53.5% contextual category warrants specific attention. These entries capture user feedback, design direction changes, reference material, and in-progress reasoning that does not fit rigid categories. Inspection of samples reveals content such as "User provided reference sprite for knight class," "Moving from terminal rendering to bitmap," and "User needs dialog/notification system for story beats." This material is valuable precisely because it records *why* the project moved in particular directions—tacit rationale that is irretrievable from code diffs or commit messages alone.

### 10.4 Tick Frequency Decline and Memory Self-Sufficiency

A distinctive pattern emerges when tick frequency is analyzed by session cohort:

| Session range | Avg. ticks/session | Phase |
|---------------|-------------------|-------|
| 1–50 | 11.8 | Foundation and rapid exploration |
| 51–100 | 3.3 | Refinement and stabilization |
| 101–150 | 1.0 | Steady-state, memory-sufficient |
| 151–218 | 3.0 | New feature introduction |

The declining curve from 11.8 to 1.0 ticks per session is not evidence of disengagement. Rather, it reflects a transition from knowledge-building to knowledge-retrieval: as the memory graph matured, the agent needed to record less because prior context was available via retrieval and cold-start injection. The recovery to 3.0 ticks in sessions 151–218 corresponds to the introduction of new subsystems (biome theming, campaign progression) that generated genuinely novel information not yet captured in existing memory.

This pattern is consistent with the theoretical expectation from Section 2: a well-functioning memory system should exhibit decreasing marginal recording cost as its coverage of the project’s decision space increases. The system becomes self-sufficient rather than self-amplifying.

### 10.5 Query Patterns and Retrieval Behavior

The 76 explicit queries cluster around game design mechanics and systems integration:

| Query topic | Frequency | Example |
|-------------|-----------|---------|
| Campaign/progression | 10 | "after each battle next level campaign progression" |
| Story and narrative | 6 | "roguelike plan gameplay design features" |
| Battle mechanics | 5 | "new feature gameplay loop campaign" |
| Animation | 4 | "movement animation enemy sprites" |
| Class system | 3 | "class upgrades progression peasant soldier warrior" |
| Enemy behavior | 3 | "enemy teleportation bug movement position" |

Two observations are notable. First, queries focus on *design-level* concerns (campaign structure, progression philosophy) rather than implementation details (function signatures, file locations)—suggesting that Legend’s primary retrieval value in this project was preserving design intent across sessions, not serving as a code search tool. Second, the low query frequency (5.4% of all events) combined with the tick frequency decline implies that most context was delivered automatically via `memory start` cold-start injection rather than requiring explicit recall.

### 10.6 Consolidation Dynamics

Auto-consolidation first triggered after 109 ticks and subsequently occurred 57 times, merging 239 memory groups at an average of 4.2 groups per consolidation event. This cadence aligns with the 15-tick consolidation suggestion interval (Section 4.7) and confirms that the clustering threshold produces actionable group sizes—neither so aggressive as to over-compress active work, nor so conservative as to leave short-term memory fragmented.

The graph node composition evolved to reflect domain-specific concerns. The five most frequently referenced node types across tick events were `EXPERIENCE` (549 occurrences), `Executed` (528), `tool` (525), `status` (458), and `Fixed` (119). These reflect the game development domain’s characteristic cycle of implementation, testing, and iteration. Domain-specific symbols such as `pixel_renderer` (64 occurrences) and `units` (108) demonstrate that the entity extraction pipeline (Section 4.5) successfully captures project-specific vocabulary and promotes it to durable graph structure.

### 10.7 Storage Efficiency

Total Legend storage for the project remained at 1.54 MB:

| File | Size | Purpose |
|------|------|---------|
| `events.jsonl` | 1.38 MB | Append-only telemetry log |
| `memory.lz4` | 0.15 MB | Compressed graph + short-term memory |
| `llm_tasks.json` | 0.01 MB | Pending augmentation tasks |

The 9.2:1 compression ratio between the raw event log and the active memory graph demonstrates that consolidation and pruning are performing as designed: 218 sessions of complex, path-dependent work are represented in 0.15 MB of structured memory. This is consistent with the boundedness guarantees described in Section 4.8.

### 10.8 Behavioral Evidence

Beyond aggregate metrics, the telemetry reveals four recurring behavioral patterns that directly reflect the mechanisms described in Section 8.1.

**Decision stability.** The agent consistently recalled domain-specific constants and design choices (e.g., "Mixed damage takes max(phys, mag) instead of average," "Honor is rewarded, not just admired" as a core design tenet) that would otherwise have been lost or hallucinated between sessions. This is evidence that selective persistence and reconsolidation are preserving high-salience traces across session boundaries.

**Failure path avoidance.** Recorded root-cause analyses (e.g., dead-end approaches in terrain generation, pre-calculated pathfinding causing units to route around each other’s starting positions) were persisted with sufficient relational context that subsequent sessions surfaced them before the failed approach was retried. This is evidence of effective graph-primed retrieval operating on bug and architecture ticks.

**Design principle continuity across pivots.** The day-one pivot from tower defense to tactics RPG was captured with explicit rationale for what was retained (grid system, cursor mechanics) and what was replaced (towers with units, waves with turn-based combat). Subsequent sessions referenced this pivot context when making analogous design decisions, demonstrating that Legend preserved not just the outcome but the decision framework. Later, the "Spine Method" for procedural map generation was recorded as a design philosophy (structured tactical layouts over random noise scatter), and subsequent scenario implementations referenced this principle via query retrieval.

**Task hand-off efficiency.** The 218-session continuity was maintained with zero manual re-onboarding prompts. The agent autonomously resumed multi-day tasks from retrieved checklists and reproduction steps, evidence of cold-start synchronization functioning as intended.

### 10.9 Interpretation and Limitations

Taken together, these outcomes support a moderate conclusion: Legend delivers measurable continuity and efficiency gains in a real, pivot-rich development workflow. The tick frequency decline pattern provides indirect evidence for the system’s central theoretical claim—that memory quality depends on selective persistence rather than maximum retained volume. As the graph matured, less recording was needed because the right information was already available.

Several limitations apply. The case study involves a single developer working with a single agent, and the tick categorization was performed post hoc rather than by automated classification. The token savings estimate from prior sampling (approximately 150,000+ tokens over the session history, based on a conservative 3,000–5,000 token manual re-onboarding baseline versus Legend’s approximately 1,100-token automated injection) has not been validated against a controlled stateless baseline. Cross-project generalization remains to be established.

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
