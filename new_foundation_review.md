# new_foundation.md — Review Checklist

Working doc for going through review items one by one. All items now resolved or skipped — see per-item notes for what changed (or why not).

Items are tagged with originating reviewer: **[C]** = Claude main, **[A2]** = second agent, **[A3]** = third agent. Multi-tag = surfaced by multiple reviewers.

---

## A. Architecture / system risks

- [x] **A1. Unbounded meta-relation recursion vs flat indices.** [C] **Resolved:** added "v0 reads depth-1 only" callout in §3.2 after structural-consequences list. Storage unbounded, hot path depth-1, depth-N traversal a private replay helper (cycle resolution), depth-2+ reasoning deferred to v1.

- [x] **A2. Recognition-index thresholds unspecified.** [C] **Resolved:** added `concept_recognition_threshold: u32` (default 3) and `frame_recognition_threshold: u32` (default 5) to §9.3 Policy. Wired thresholds into §3.4 recognitions and §8.2 behavior reads. Added §23 deferred question for switching to adaptive thresholds once corpus data shows distributional shape.

- [x] **A3. Question-shaped ticks and extractor emission gating.** [C, A2, A3] **Resolved:** rejected `MutationMode` framing — every tick runs full pipeline. Fixed §4.2 / §11.1 phase boundary; expanded §10.6 to canonical per-intent modulator table (vigilance/plasticity/salience/default_conf); rewrote §4.3 as "Pre-Mutation Diagnosis" with per-step extract-and-payoff table; updated §12 latency framing to v0 200–300 ms. Intent modulates exactly four Policy knobs; does not gate which steps run.

- [x] **A4. Predicate minting + dedup race / explosion.** [C, A3] **Resolved:** §11.7 step 2 now does universal cosine search across *all* predicates (not warm-only) with hits at ≥ `policy.predicate_dedup_threshold` reused as Defeasible. Added `predicate_dedup_threshold` (0.85) and `predicate_mint_warning_count` (5) to §9.3. §14.8 reframed: predicate-merge is cleanup-only, priority-bumped for warning ticks.

- [x] **A5. Mid-path insertion (§10.3.5) recovery + noise sensitivity.** [C, A3] **Resolved:** Option 3 (provisional insert + replay-confirmed). Resolved internal contradiction in §10.3.5. All tick-time mid-path insertions now Defeasible. Replay resolves to confirm / re-parent (cross-subtree) / retract via `midpath_confirm_gap` (0.05), `midpath_confirm_evidence` (3), `midpath_reparent_gap` (0.10). Cross-subtree re-parent emits `(node, supersedes_parent_region, old_parent)` for lineage.

- [x] **A6. Pin-for-life embedder cost.** [C, A3] **Resolved:** visibility-only. Added recoverability-by-source-class table to §15.1; reframed closing as "load-bearing infrastructure that does not get swapped without an explicit recovery plan." §18.4 cross-references the matrix. Added §23 deferred question listing v1 candidate approaches.

- [x] **A7. No per-input record + extraction failure = silent loss + audit conflict.** [C, A2, A3] **Resolved:** dev-only debugging, production unchanged. §18.2 dev WAL via `LEGEND_WAL_UNBOUNDED=1`; new §18.2a "Extraction-Failure Quarantine (Dev Only)" 100-entry ring gated by `LEGEND_DEV_QUARANTINE=1`, exposed via `legend memory show-failures`. Tightened §2.6 #6: provenance is structural lineage, not transcript recovery.

- [x] **A8. Latency math doesn't close.** [C, A3] **Resolved:** added §11.0 per-step latency budget table; added §15.1 GLiNER2 callout marking v0 binding constraint; reframed §24.1 as quality + latency play; rewrote §24.2 as "Secondary Contributors"; expanded §24.7 with concrete trade-offs. Swept doc for stale "sub-100 ms" v0 framing.

- [x] **A9. Replay determinism asserted but not testable.** [C, A3] **Resolved:** softened §14 preamble — "designed to be order-independent and tested as such (§21 Step 11 determinism fixture)." Added fixture spec to Step 11. Added "Conformance-test discipline" preamble to §21: substrate tier (mocked extractors, bit-identical) + full-stack tier (pinned hardware, ε-tolerance).

- [x] **A10. Modality as pre-declared closed set.** [C] **Resolved:** scoped the §1.6 claim — added "scope of the no-pre-declared-categories bet" paragraph clarifying it applies to world-content, not substrate-mechanism anchors. Added §16.3 closing paragraph explaining why modalities are pre-declared (fixed ElementId for behaviors that compare against modal stance).

- [x] **A11. No system-level failure-mode story.** [C, A3] **Skipped (per Q2):** no cross-cutting Failure Modes section. In-place coverage already exists in §10.6 (regions), §11.7 mint-warning observability, §18.2a quarantine for extraction failures, §14.8 replay safety predicates, §18.5 replay-snapshot conflict handling. The discrete failures each have a treatment; the gap was a unified section, which the user opted out of.

- [x] **A12. Scaling story implicit.** [C, A3] **Resolved:** added §18.5 "v0 scale bound" (~100K elements / 500K relations comfortable; ~1M is v1 horizon for HNSW/INT8 stored). Added §18.5 "Replay snapshot cost" — full-clone at job start, conflict detection at apply-time on next tick boundary, optimistic-concurrency-style.

- [x] **A13. Render LLM is load-bearing but underspecified.** [C, A3] **Skipped:** §2.5 already honestly defers role-definition; the substrate-side spec (frame structural completeness) is implicit in §11.13's `focused_relations` / `supporting_claims` / `history` / `uncertainty` shape. Closing the "what makes a frame answerable by Qwen-0.5B" gap is real work that belongs to notes-app implementation, not to the substrate spec. Will surface during Step 13 of build order.

- [x] **A14. One-thought-per-tick is a frontend rule, not substrate.** [C, A3] **Resolved:** §2.5 step 2 now explicitly flags it as a frontend convention specific to the notes app; substrate accepts inputs of any size and uses §11.4 segmentation internally.

- [x] **A15. Tick pipeline phase boundary contradicts itself.** [A2] **Resolved (subsumed by A3):** §4.2 + §11.1 made consistent — Steps 1–7 read-mostly, Step 8 `apply_region_delta` is first mutation.

- [x] **A16. "Semantic strings do not drive control flow" conflicts with seeded predicates.** [A2] **Resolved (in-place):** Inv 4 already says "branching uses recognition indices, payload-table membership, RelationStatus, and meta-relation indices — never element name strings." The §11.7 update clarifies that string-name resolution happens at the *boundary* (lexical lookup, embedding match, mint), after which all hot-path branching uses ElementIds. Existing wording covers the scope; no further edit needed.

- [x] **A17. seed_pack.yaml is not synced with the spec.** [A2] **Resolved:** added `names` fields to all 6 modal_elements, all 11 roles, all 8 reference_frames, all 15 regions in `seed_pack.yaml`. Updated §16.4 manifest to enumerate names inline.

- [x] **A18. Pseudocode references fields not in the structs.** [A2] **Resolved (compile-contract pass):** added `support_count: u32` and `support_diversity: u32` to MemoryStats (§7.1). Added `region_activation_threshold`, `ner_assertion_threshold`, `replay_focus_floor`, `recent_focus_capacity`, `promotion_min_count`, `promotion_min_diversity`, `promotion_window_ticks` to Policy (§9.3). Defined `RegionActivation`, `RelationActivation`, `UncertaintySignal`, `AttentionAction`, `RecentFocusEntry`, `InputEcho`, `SourceKind` in §9.6.

- [x] **A19. "There is no separate retrieval index" is not literally true.** [A2, A3] **Resolved:** §12 reframed — "no separate query API and no separate memory store. Retrieval is differential — path traversal with reinforcement — not a parallel index alongside the substrate." §11.13 §13 frame assembly now explicitly names the dense + sparse + path-reinforced RRF fusion using tantivy as the sparse signal.

- [x] **A20. Runtime "benchmark-aware replay" is too strong.** [A2] **Resolved:** §14.8 rewrote "replay must be benchmark-aware" to "replay safety predicates" (local, structural, cheap): Inv 8/9 enforcement, focus-bearing protection, cycle resolution preserves connectivity. §20.7 updated to match. §19 + §20.5 gates run in CI, not in the replay loop.

- [x] **A21. Defeasible → Asserted at support_count >= 3 is a thin gate.** [A3] **Resolved:** §11.11 promotion check now requires all three: count ≥ `policy.promotion_min_count` (3), diversity ≥ `policy.promotion_min_diversity` (2 distinct evidence sources across source elements / intents / frames), and no contradicting relation written within the window. Diversity check distinguishes "repeated assertion" from "converging evidence."

- [x] **A22. Frame scope flat in v0 — handwaves what to do without inheritance.** [A3] **Resolved:** §3.4 updated — "v0 retrieval operates within a single active frame at a time (`ConsciousAttentionFrame.active_frame`); cross-frame access requires the consumer to issue a separate tick under the other frame, or to author explicit `(R, also_in_frame, F')` meta-relations on relations that should appear in multiple frames."

- [x] **A23. Coreference is recency-only and thinner than walkthrough relies on.** [A3] **Resolved:** changed `recent_focus: VecDeque<ElementId>` → `VecDeque<RecentFocusEntry>` carrying `{ element, role, frame, tick }`. §11.8 coref scoring now reads role-overlap from RecentFocusEntry — "with dentist context" filtering is now spec'd.

- [x] **A24. RelationStatus semantics partially specified.** [A3] **Resolved:** §11.13 frame-assembly now spec'd: `Asserted` and `Entailed` in `focused_relations` by default; `Defeasible` flagged with `is_defeasible: true` and lower base weight; `Superseded` lands in `history`; `Retracted` excluded from both. Canonical read-time treatment.

- [x] **A25. Lexical (tantivy) integration missing from §11.** [A3] **Resolved:** §11.13 frame-assembly now explicitly names tantivy BM25 as the sparse retrieval signal in the RRF fusion alongside the dense focus-set and path-reinforced rankings. Query terms derived from input text + focus-set element names.

- [x] **A26. Recent_focus capacity 64 is a magic number.** [A3] **Resolved:** added `recent_focus_capacity: u32` (default 64) to §9.3 Policy. §9.2 comment updated to read from Policy.

- [x] **A27. InputEcho referenced but not defined.** [A3] **Resolved:** added `InputEcho` and `SourceKind` definitions to §9.6 auxiliary types.

- [x] **A28. Migration plan from current Legend missing.** [A3] **Resolved:** added §17.4 "Migration From Existing Legend Data" — explicit "there is no migration; v2 is a fresh substrate." Boot fingerprint check refuses mixed-version state.

- [x] **A29. How Hard Invariants are tested.** [A3] **Skipped:** per-invariant test fixtures are real work that belongs in §21 build steps as they're implemented. The conformance test discipline (§21 preamble) covers the substrate-tier mechanism. Adding 15 hand-spec'd fixtures up front overweights the doc relative to the value; will land per-invariant as build steps add the relevant code.

- [x] **A30. Privacy/access control deferred without saying so.** [A3] **Resolved:** added §18.6 "Privacy and Access Control" — explicit "v0 has no access boundary; consumers operate as a single trust domain. Multi-tenant auth and access-controlled retrieval are out of scope (§2.4). Consumers needing separation should run separate Legend instances per trust boundary."

- [x] **A31. Salience formula not specified.** [A3] **Resolved:** §11.11 now contains the explicit `score_salience(R, p)` formula with bumps for exact-value role-fillers, correction/temporal-update intent, supersession output, user preferences, and focus-bearing relations; bump magnitude scaled by `policy.salience_multiplier` and applied via `bounded_hebbian_bump`.

- [x] **A32. Failure handling for extraction unspecified.** [A3] **Resolved (subsumed by A7):** dev-only quarantine in §18.2a captures `(Tick, Input, Reason)` for extraction failures. Production: input is dropped per §1.5 commitment.

- [x] **A33. Dependency versions stale.** [A2] **Resolved:** §15.1 tantivy entry now reads "0.26.x or current stable; pin a specific minor version at first integration; bump deliberately." gline-rs entry clarified to mention `gline-rs` (NER) and `gliner2` (relation extraction) crate boundary; substrate is indifferent to which crate exposes the relation-extraction surface.

---

## B. Document structure / readability

- [x] **B1. Length / abstraction-level mixing.** [C, A2, A3] **Skipped (defer file extraction):** decided to keep §15, §22, §24 inline. The doc is long but each section has a defined audience and the §0 reading guide steers. Extracting to separate files adds navigation cost (mental + git). Revisit if the doc grows further.

- [x] **B2. Hard Invariants positioning.** [C] **Skipped per Q2.**

- [x] **B3. §6 ↔ §22 overlap.** [C] **Resolved:** §6 (7) trimmed to a one-paragraph hook deferring to §1.7 for full argument; eliminates verbatim Wolfram-irreducibility duplication.

- [x] **B4. §11 numbering misaligned with step numbers.** [C, A3] **Resolved (in A3):** §4.2 step list and §11.1 pseudocode now align — `apply_region_delta` is Step 8, etc. Step counts consistent at "14 steps (0–13)" throughout.

- [x] **B5. No diagrams.** [C] **Resolved:** added two ASCII diagrams — four-piece architecture in §3 preamble, tick pipeline in §4 preamble.

- [x] **B6. No glossary.** [C, A2, A3] **Skipped per Q2.**

- [x] **B7. No competitor-positioning narrative.** [C] **Resolved:** added "How Legend differs" capsule to §22 Comparable Memory Systems — five-row table comparing Graphiti/Zep, HippoRAG 2, A-MEM, Mem0 against Legend on primitive, retrieval shape, and the differentiating bet.

- [x] **B8. §17 reads "trust me" to a v2-fresh reader.** [C, A3] **Skipped:** audience is solo dev who is the same person who built v1. Diff table would be churn; §17 prose is sufficient for the audience.

- [x] **B9. Terminology drift: "citizen" / "primitive" / "thing".** [C] **Resolved:** swept "memory citizen" / "hypergraph citizen" → "substrate citizen" via sed.

- [x] **B10. §3.2 "claims about claims" raises an unanswered question.** [C] **Resolved (in A1):** "v0 reads depth-1 only" callout in §3.2 closes the question.

- [x] **B11. §16's "Code / Seed / Input / Replay owns" framing.** [C] **Resolved:** added the four-line "who owns what" framing to §1.5 with a forward-pointer to §16.1 for the full version.

- [x] **B12. "Discovery" rebranding used inconsistently.** [C, A3] **Resolved:** renamed §4.5 from "The Discovery Frame" to "The Tick As Discovery"; added explicit "tick" = mechanical, "discovery" = semantic, output type is `ConsciousAttentionFrame` in both views.

- [x] **B13. §20.5 companion fixtures land too late in build order.** [C, A2, A3] **Resolved:** Step 4 renamed to "Manual Conformance Set" and now includes §20.5 fixtures + the non-appointment fixture (was Step 9.5). Domain neutrality lands before reinforcement and replay accumulate appointment bias.

- [x] **B14. §16.4 totals seed atoms (~52) but doesn't enumerate roles/frames inline.** [C] **Resolved:** §16.4 manifest now contains an inline name table for all five categories (anchors, predicates, modals, regions, roles, frames).

- [x] **B15. Add a "v0 contract" table.** [A2] **Resolved:** added §0.1 "v0 Contract (At-a-Glance)" — substrate types, payload tables, public surface, phases, intent scope, durability, embedder, latency budget, conformance gates, invariants — with section pointers.

- [x] **B16. Move philosophy/source-map material after the build contract.** [A2] **Skipped:** the §0.1 contract table at the top satisfies the "implementer wants the contract first" use case without restructuring the whole doc. §0 reading guide steers readers to the right section by intent.

- [x] **B17. Forward references that don't bottom out.** [A3] **Light pass:** key forward-references (`ConsciousAttentionFrame`) now have `(spec in §11.13)` markers in §2.3. Remaining forward-references (`AttentionIntent`, `Term`, `RelationStatus`, `MemoryStats`) are conventional types defined within a few sections of first reference; over-pointering would clutter.

- [x] **B18. Mixed conceptual/typed levels.** [A3] **Skipped:** the alternation is intentional — Layer 1 (§3, §4) needs both conceptual framing and concrete types because the conceptual claims are tightly tied to the type structure. The §3 four-piece diagram + §4 pipeline diagram help bridge.

- [x] **B19. §7.2 is dense.** [A3] **Resolved:** added inline meta-relation worked example to §7.2 — full Rust struct showing `R = (DrRao, has_role, dentist)` plus its `frame` and `source` meta-relations as ordinary Relations with `Term::Relation(42)` subjects, plus how the hot-path indices populate from them.

- [x] **B20. §8 underplays the most important design move.** [A3] **Resolved:** §8 preamble now leads with "recognition is the load-bearing answer to §1.6's bet that ontology emerges" — frames §8 as the operational realization of emergent ontology, not just reference. Closing line ties back to the no-pre-declared-categories bet being operationally cheap because of the index design.

- [x] **B21. §11.11 too compressed.** [A3] **Resolved (in A31):** salience formula now explicit; promotion gate fully spec'd.

- [x] **B22. §19 notation inconsistency.** [A3] **Skipped:** notation is already consistent — concrete relations as `R1: subj pred obj [Status]`, meta-relations as `(R, predicate, X) [Status]`. Audited the §19 walkthrough and §11 examples; no inconsistencies remain.

- [x] **B23. §24 mixes two intents.** [A3] **Resolved:** §24 preamble now distinguishes "Deferred Capabilities" (24.1, 24.6, 24.7) from "Forward Roadmap" (24.2, 24.3, 24.4, 24.5, 24.8). Subheadings inserted.

- [x] **B24. §22 citation form inconsistent.** [A3] **Skipped:** spot-checked §22; citation form is reasonably consistent (Author Year — Title, with arXiv id where applicable). The variation is between paper-with-venue and paper-with-just-arxiv, which reflects what's actually canonical for each source. Standardizing further is busy work.

- [x] **B25. Repetition between layers — concrete instances.** [A3] **Partially resolved:** §6 (7) Wolfram-irreducibility now defers to §1.7 instead of duplicating. §12 latency framing aligned with §11.0 and §15.1. Other repetition (durability across §1.4 / §5 Inv 1 / §18) is intentional layered exposition (Layer 1 preview, invariant, Layer 2 spec) — left as-is.

- [x] **B26. Add a per-step latency budget table.** [A2, A3] **Resolved (in A8):** §11.0 added.

- [x] **B27. Add cross-cutting "Failure Modes & Mitigations" section.** [A3] **Skipped per Q2** (tied to A11 decision).

- [x] **B28. Reframe "side tables" as "payload tables" / "storage shapes."** [C] **Resolved:** swept "side table" → "payload table" via sed. §3.5 reframed: "the hypergraph has three storage shapes — elements (identity), relations (claims), and payload tables (dense and typed leaf data) — all owned by the same struct." Closing tie-in clarifying §1.6's "one primitive" claim is about identity, not uniform storage layout.
