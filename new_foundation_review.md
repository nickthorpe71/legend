# new_foundation.md — Review Checklist

Working doc for going through review items one by one. For each item: **leave**, **correct**, or **something else** (clarify, defer, expand, etc.).

Items are tagged with originating reviewer: **[C]** = Claude main, **[A2]** = second agent, **[A3]** = third agent. Multi-tag = surfaced by multiple reviewers.

---

## A. Architecture / system risks

- [x] **A1. Unbounded meta-relation recursion vs flat indices.** [C] §3.2 promises arbitrary-depth reflective reasoning; structural-consequences callout admits indices are flat. v0 hot path only reads depth-1. Risk: depth is supported but unused, will bug the first time it's actually needed. Worth explicit "v0 stores arbitrary depth; v0 only reads depth-1 via flat indices." **Resolved:** added "v0 reads depth-1 only" callout in §3.2 after structural-consequences list. Storage unbounded, hot path depth-1, depth-N traversal a private replay helper (cycle resolution), depth-2+ reasoning deferred to v1.

- [x] **A2. Recognition-index thresholds unspecified.** [C] §3.4 / §8.2 refer to "high inbound `instance_of` counts" but never name a cutoff. Coreference's merge bias reads `inbound_predicate_counts[E][instance_of]` — uncalibrated without a concrete number. v0 should pick one even if "calibrate after Step 8." **Resolved:** added `concept_recognition_threshold: u32` (default 3) and `frame_recognition_threshold: u32` (default 5) to §9.3 Policy. Wired thresholds into §3.4 recognitions and §8.2 behavior reads. Added §23 deferred question for switching to adaptive thresholds once corpus data shows distributional shape.

- [x] **A3. Question-shaped ticks and extractor emission gating.** [C, A2, A3] §4 says extractors run in read-mostly phase before mutation; §11 applies region_delta before run_extractors and runs extraction/coref after mutation starts. **Resolved:** rejected the `MutationMode` framing — every tick runs the full pipeline. Fixed §4.2 and §11.1 phase boundary: Steps 1–7 are read-mostly under `&Hypergraph` (route_regions returns a held `RegionDelta`, no commit); Steps 8–13 are mutation under `&mut Hypergraph` (apply_region_delta is now Step 8, the first mutation). Reframed §12: questions don't have empty durable_writes by mandate — they tend to write little, but the frame reflects what was discovered, not an intent gate. Tightened §11.2: intent modulates exactly four things — vigilance, plasticity, salience, default confidence — and does not affect pipeline shape, extraction, mutation, or frame structure. Expanded §10.6 table from vigilance-only to canonical per-intent modulator (vigilance/plasticity/salience/default_conf columns); referenced from §11.2, §11.3, §11.11. Updated §11.11 to read `salience_multiplier` and intent-scaled `hebbian_rate` from Policy. Updated §12 latency framing to v0 200–300 ms. Rewrote §4.3 as "Pre-Mutation Diagnosis: What We Learn Before Changing State" — explicit per-step table of what each Step 0–7 extracts, why, and where it pays off in the mutation phase; parallelism story now falls out of the diagnosis/commit split rather than standing alone.

- [x] **A4. Predicate minting + dedup race / explosion.** [C, A3] §11.7 mints Defeasible predicates; §14.8 replay merges duplicates when embeddings converge. **Resolved:** synchronous universal cosine dedup at mint time. §11.7 step 2 now searches *all* predicate elements (not just warm), with hits at ≥ `policy.predicate_dedup_threshold` reused (Defeasible — surface label didn't match canonical, but predicate is right). §11.7 step 3 mints only when cosine misses across the full predicate set. Added rationale paragraph explaining why warm-only dedup was wrong. Added `policy.predicate_dedup_threshold` (default 0.85) and `policy.predicate_mint_warning_count` (default 5) to §9.3 — warning is observability-only, not a hard cap; it priority-flags the tick for replay's predicate-dedup job. §14.8 reframed: replay's predicate-merge is now cleanup-only (catches embedding drift / dissimilar surface labels), priority-bumped for warning ticks.

- [x] **A5. Mid-path insertion (§10.3.5) recovery + noise sensitivity.** [C, A3] If active-region routing is wrong, a new node lands in the wrong subtree. Diffuse-routing fallback marks parent meta-relation Defeasible but the span vector remains in the wrong place. [A3] also flags noise sensitivity: BGE-small at 384 dims gives cosine differences of 0.02–0.05 routinely. **Resolved:** Option 3 (provisional insert + replay-confirmed). Resolved internal contradiction in §10.3.5 (closing line said no tick-time insertion, body described it). Now: all tick-time mid-path insertions write parent meta-relation as `Defeasible`. Region recognition reads `RegionPayload` structurally so the DAG benefits immediately for routing. Replay (§14.8) resolves to confirm / re-parent / retract via `policy.midpath_confirm_gap` (0.05), `policy.midpath_confirm_evidence` (3), `policy.midpath_reparent_gap` (0.10) — added to §9.3. Cross-subtree re-parenting is explicit and emits `(node, supersedes_parent_region, old_parent)` for lineage. Stability gate at confirm-time prevents BGE-small noise churn; reparent_gap > confirm_gap prevents flapping.

- [x] **A6. Pin-for-life embedder cost.** [C, A3] §15.1 + §18.4 commit to model swap = re-ingest from `(R, source, S)` pointers, accepting element loss. **Resolved:** visibility-only (no v0 mechanism change). Added recoverability-by-source-class table to §15.1 (user-as-source = recoverable; git history = recoverable but expensive; file events = partial; Slack/chat = partial-to-ephemeral; agent-internal = unrecoverable). Reframed §15.1 closing from "treat as one-way door" to "the pin is what the design costs, not a conservative default; treat as load-bearing infrastructure that does not get swapped without an explicit recovery plan." §18.4 cross-references the matrix. Added §23 deferred question naming candidate v1 recovery approaches: secondary-embedder rotation, opt-in source-text retention, or hybrid.

- [x] **A7. No per-input record + extraction failure = silent loss + audit conflict.** [C, A2, A3] §1.5 / §7.4 / §17.2 commit hard: failed extraction drops the input. **Resolved:** dev-only debugging, production unchanged. §1.5 / §7.4 / §17.2 stay intact for production (distilled relations only, no per-input record, no exceptions). Two dev-only debugging surfaces: (1) §18.2 dev WAL via `LEGEND_WAL_UNBOUNDED=1` (already existed; clarified that production rejects the flag and that dev WAL doubles as the full input-history record for replay-through-the-extractor debugging); (2) new §18.2a "Extraction-Failure Quarantine (Dev Only)" — bounded 100-entry in-memory ring capturing `(Tick, Input, Reason)` for inputs that emitted no relations, gated behind `LEGEND_DEV_QUARANTINE=1`, exposed via `legend memory show-failures`, production builds compile it out. Tightened §2.6 #6 to make explicit the audit promise is structural lineage, not transcript recovery — recovering original text depends on the source's own retention, not Legend.

- [x] **A8. Latency math doesn't close.** [C, A3] GLiNER2 at 130–208 ms/call (§15.1) is the long pole; v1 sub-100 ms is implausible without replacing or augmenting it. **Resolved:** added §11.0 per-step latency budget table (p50 numbers per step, GLiNER2 explicitly the dominant entry at ~60–80% of tick budget). Added §15.1 GLiNER2 callout marking it as v0's binding latency constraint and pointing at §24.1 / §24.7 / smaller GLiNER variant as the realistic paths. Reframed §24.1 as both quality + latency play (pattern fast-path = ~5–20 ms on hit vs GLiNER2's 130–208 ms; cheapest path to sub-100 ms p50). Rewrote §24.2 as "Secondary Contributors" — read-path/background split helps but is not the primary path; the primary paths are §24.1 + §24.7. Expanded §24.7 with concrete latency / quality / determinism / bundle-size trade-offs and explicit framing as one of two primary paths to sub-100 ms. Swept doc for stale "sub-100 ms" v0 framing: §2.2 (item 5), §2.6 (item 9), §12 latency bullet — all updated.

- [x] **A9. Replay determinism asserted but not testable.** [C, A3] §14 preamble claims order-independent rule application; no test exists. ONNX FP rounding shifts confidences across hardware. **Resolved:** softened §14 preamble — removed the "rules do not depend on order when correctly written" claim; replaced with "designed to be order-independent and tested as such (§21 Step 11 determinism fixture)." Added the determinism fixture spec to §21 Step 11: two replay passes over same starting hypergraph with shuffled rule-application order, assert bit-identical final state. Added "Conformance-test discipline" preamble to §21 establishing two test tiers: (1) substrate conformance with mocked extractor outputs (no ONNX, bit-identical delta required); (2) full-stack smoke tests on pinned CI hardware with ε-tolerance on confidences. Cross-machine determinism explicitly out of substrate scope.

- [ ] **A10. Modality as pre-declared closed set.** [C] §16.3 ships 6 modal elements. Mild contradiction with §1.6's "no pre-declared categories" rhetoric. One sentence acknowledging some categories are pre-committed because load-bearing for specific behaviors.

- [ ] **A11. No system-level failure-mode story.** [C, A3] §10.6 covers region-routing failures; nothing equivalent for "extraction misfires," "coref over-merges," "recognition undercounts," "replay falls behind." When Legend fails, what does the calling LLM see? Empty `focused_relations`? `uncertainty: high`? Confabulation? [A3] proposes a cross-cutting "Failure Modes & Mitigations" section.

- [ ] **A12. Scaling story implicit.** [C, A3] Hypergraph in memory fine for ~10K–100K elements; at 1M, indices alone get heavy. [A3] adds: replay snapshot clone of 10K elements + 50K relations is tens of MB, expensive every cycle. What if the snapshot is N ticks stale and a proposed mutation references a since-retracted relation? Conflict detection isn't specified. Cost/cadence missing. Worth one sentence in §18.5 / §23 bounding v0's element budget; spec replay snapshot semantics in §9.4 / §14.8.

- [ ] **A13. Render LLM is load-bearing but underspecified.** [C, A3] §2.5 honestly defers role-definition. But the consumer-side promise is that a 0.5B model verbalizes the frame — requires concrete role bindings, sufficient `supporting_claims`, enough `history` for context. No spec for "what makes a frame answerable by Qwen-0.5B." [A3]: what's the render LLM prompt? How do you diagnose hallucination from a sparse frame? When is it the frame's fault vs the renderer's? Riskiest unspecified piece for v0 sign-off.

- [ ] **A14. One-thought-per-tick is a frontend rule, not substrate.** [C, A3] §2.5 risks being read as gospel. [A3] adds: a Slack channel watcher batches; what's the policy? Flag explicitly.

- [ ] **A15. Tick pipeline phase boundary contradicts itself.** [A2] §4 (line 781) places extractors in the read-mostly phase before mutation; §11 (line 1849) applies region_delta before run_extractors and runs extraction/coref after mutation starts. Decide the exact phase boundary. (Overlaps with A3 but worth tracking separately as a concrete spec contradiction to fix.)

- [ ] **A16. "Semantic strings do not drive control flow" conflicts with seeded predicates.** [A2] Inv 4 (line 887) says strings don't drive control flow, but seeded predicates (line 715) and extractor label resolution by name (line 1961) both use names. Fixable: say strings are allowed only at bootstrap/extraction boundaries; after resolution, hot-path logic uses ElementIds and recognition indices.

- [ ] **A17. seed_pack.yaml is not synced with the spec.** [A2] Many modal elements, regions, roles, and frames in `seed_pack.yaml` lack names (lines 129, 310, 375), even though the doc says extractors and seed predicates need name-resolved elements. Add names for every seed atom or state that `element_id` deterministically derives canonical names.

- [ ] **A18. Pseudocode references fields not in the structs.** [A2] `policy.region_activation_threshold` (line 1770), `policy.ner_assertion_threshold` (line 1935), `stats.support_count` (line 2067) are read but not declared in §9.3 Policy or §7.1 MemoryStats. Do a compile-contract pass over all pseudocode.

- [ ] **A19. "There is no separate retrieval index" is not literally true.** [A2, A3] (line 2151) The design has tantivy plus derived indices (line 2483). Better wording: "no separate query API or memory store." [A3]: the genuine claim is "retrieval is differential — path traversal with reinforcement, not a separate index." The "no query" slogan trips readers because Tick 3/4/6/8/10 in §19 are queries; you've made them share the same code path with different effects, not eliminated retrieval. Rephrase §12.

- [ ] **A20. Runtime "benchmark-aware replay" is too strong.** [A2] (lines 2445, 3253) Rejecting replay mutations by testing against §19 at runtime overfits and doesn't scale. Make benchmarks CI gates; runtime replay should use local safety predicates.

- [ ] **A21. Defeasible → Asserted at support_count >= 3 is a thin gate.** [A3] In real conversation, repetition ≠ truth — three rephrasings of the same wrong claim auto-promote. No diversity check (different sources? intents? frames?). Once Asserted, only path to correction is supersession, which depends on Legend recognizing the supersede shape. Bar should be evidence diversity, not count.

- [ ] **A22. Frame scope flat in v0 — handwaves what to do without inheritance.** [A3] §3.4 says "cross-frame visibility must be expressed by explicit meta-relations" — what meta-relations? When `appointment_1` is scoped to FRAME_USER and a query under FRAME_PROJECT needs it, what fires? §24.3 punts to v1. v0 needs either a concrete workaround or an explicit "v0 does not handle cross-frame access; consumers operate in one frame at a time" claim.

- [ ] **A23. Coreference is recency-only and thinner than walkthrough relies on.** [A3] §11.8 / §13.3 / §15.1 — pronoun resolution to "most-recently-focused element whose role matches." `recent_focus` is a flat `VecDeque<ElementId>` (capacity ~64). Tick 5 says coref: "it" → `appointment_1` (most-recently-focused with dentist context) — "with dentist context" implies filtering the spec doesn't describe. Either enrich the buffer with role/frame metadata or describe the scoring formula past "recency."

- [ ] **A24. RelationStatus semantics partially specified.** [A3] §11.10 covers Asserted ↔ Superseded. §11.7 / §11.11 cover Defeasible ↔ Asserted. §14.8 covers Retracted via cycle resolution. Where does Entailed come from operationally beyond NER auto-emit? Does `focused_relations` include Defeasible relations or filter them? Inv 7 says all five remain "distinct in the substrate" but read-time treatment is inconsistent across the doc.

- [ ] **A25. Lexical (tantivy) integration missing from §11.** [A3] §15.1 lists tantivy. §21 Step 9 mentions hybrid retrieval with RRF fusion. But §11's pipeline never names lexical lookup. Where does it fire — Step 5 (region routing modulation)? Step 7 (coref support)? Step 13 (frame assembly)? Real gap.

- [ ] **A26. Recent_focus capacity 64 is a magic number.** [A3] Not adjustable in Policy. Either move to Policy or document why fixed.

- [ ] **A27. InputEcho referenced but not defined.** [A3] §11.13 references `InputEcho`; not defined in §9.6 (auxiliary types).

- [ ] **A28. Migration plan from current Legend missing.** [A3] §17 covers concept-level carry-forward but not existing `.legend/memory.lz4` content. Discarded? Re-ingested?

- [ ] **A29. How Hard Invariants are tested.** [A3] §5 lists 15 invariants; §21 tests via "harness diff" but per-invariant assertions aren't specified.

- [ ] **A30. Privacy/access control deferred without saying so.** [A3] §2.6 implies multi-consumer Legend; substrate has zero access boundary. Defer explicitly.

- [ ] **A31. Salience formula not specified.** [A3] §11.11 is too compressed. Hebbian update goes to §14.9; salience formula is hand-waved. The actual `score_salience(R, p)` formula doesn't appear anywhere — only its inputs.

- [ ] **A32. Failure handling for extraction unspecified.** [A3] GLiNER2 panics, chrono-english returns garbage — pipeline behavior unspecified. WAL has the input; substrate has no relations. (Subset of A11.)

- [ ] **A33. Dependency versions stale.** [A2] gline-rs is GLiNER-on-ort, while current gliner2 docs show a separate crate with Candle/TCH-style backends. Tantivy current is 0.26.1, doc says 0.25. Verify and update §15.1.

---

## B. Document structure / readability

- [ ] **B1. Length / abstraction-level mixing.** [C, A2, A3] 3,736 lines / ~60K tokens. Three layers, some redundancy. [A3] suggests extracting §15 (model stack), §22 (source map), §24 (beyond v0) into separate files referenced from main doc — gets ~30% shorter without losing anything load-bearing.

- [ ] **B2. Hard Invariants positioning.** [C] §5 in Layer 1, good. A one-page front-matter version (just 15 numbered items, no rationale) would let readers hold them in cache while reading conceptual chapters.

- [ ] **B3. §6 ↔ §22 overlap.** [C] Each formalism named in §6 with a short hook, named again in §22 with full citation. Either tighten §6 or trim §22.

- [ ] **B4. §11 numbering misaligned with step numbers.** [C, A3] §11.10 covers Steps 9–10, §11.12 covers Step 12, §11.13 covers Step 13. Mild reader friction. [A3] also flags step-count inconsistency: "14 steps" (line 783) vs "12 sequential steps" (line 1843) vs "12 pure sub-functions" (line 858).

- [ ] **B5. No diagrams.** [C] Not even ASCII. Four-piece architecture (Element ↔ Relation + side tables + recognition indices) and tick pipeline would absorb faster visually.

- [ ] **B6. No glossary.** [C, A2, A3] "Memory citizen" / "substrate citizen" / "hypergraph citizen" used interchangeably. "Active frame" vs "frame scope" vs "reference frame." "Discovery frame" (§4.5) vs "attention frame" (§11.13). "Cone neighbors," "warm predicates," "focused path." [A2] proposes glossary for: Element, Relation, claim, predicate, role, frame, region, source, evidence, attention frame. [A3] adds: discovery, tick, predicate element, modal element, anchor, atom.

- [ ] **B7. No competitor-positioning narrative.** [C] §22 cites Graphiti/Zep, HippoRAG 2, A-MEM, Mem0. Reader has to assemble "how is Legend different?" 5-bullet table would help.

- [ ] **B8. §17 reads "trust me" to a v2-fresh reader.** [C, A3] [A3] proposes a "current Legend → v2" diff table side-by-side, not just §17 prose.

- [ ] **B9. Terminology drift: "citizen" / "primitive" / "thing".** [C] Pick one.

- [ ] **B10. §3.2 "claims about claims" raises an unanswered question.** [C] Where in v0 is reflective reasoning actually used? Forward-pointer: "v0 hot path uses depth-1 only; deeper recursion supported but unused until v1." (Closely related to A1.)

- [ ] **B11. §16's "Code / Seed / Input / Replay owns" framing.** [C] Great — should appear earlier (§1.5 or §2) as a one-sentence guide.

- [ ] **B12. "Discovery" rebranding used inconsistently.** [C, A3] §4.5 "discovery frame" vs §11.13 "attention frame." §11/§13/§19 say "tick." Either commit or drop "discovery" as an alias.

- [ ] **B13. §20.5 companion fixtures land too late in build order.** [C, A2, A3] Currently in Step 8. [A2, A3] both propose adding a non-appointment walkthrough before build starts (in the doc, even at 2–3 ticks), so domain neutrality is visible early — not only Step 9.5.

- [ ] **B14. §16.4 totals seed atoms (~52) but doesn't enumerate roles/frames inline.** [C] Naming the 11 roles or 8 reference frames in the doc would help one-sitting comprehension.

- [ ] **B15. Add a "v0 contract" table.** [A2] Authoritative structs, phases, mutation permissions, persistence guarantees, benchmark gates. Up front, before philosophy.

- [ ] **B16. Move philosophy/source-map material after the build contract.** [A2] Useful but currently interrupts implementation clarity.

- [ ] **B17. Forward references that don't bottom out.** [A3] §3 and §4 use `ConsciousAttentionFrame`, `AttentionIntent`, `Term`, `RelationStatus`, `MemoryStats` before they're defined. Add `(spec in §X)` pointers.

- [ ] **B18. Mixed conceptual/typed levels.** [A3] §3 promises conceptual but §3.2 dives into role bindings and `Term::Relation(RelationId)`. §4 is "conceptual" but has Rust pseudocode. Sometimes redundant.

- [ ] **B19. §7.2 is dense.** [A3] Most important type but mixes prose, table, code, rationale. Lead with the type, then meta-relation table, then explanations as bullets. [A3] also wants an inline meta-relation example: a one-block "here's a relation R, here's its frame meta-relation written out fully as a Relation."

- [ ] **B20. §8 underplays the most important design move.** [A3] §1.6 framing of emergent kinds is stronger than §8 framing. §8 reads as reference; should also be the argument that recognition-via-indices is the right thing — with index types embedded.

- [ ] **B21. §11.11 too compressed.** [A3] (Subset of A31.) Hebbian update goes to §14.9; salience formula hand-waved.

- [ ] **B22. §19 notation inconsistency.** [A3] Concrete relations are `R1: subject predicate object [Status]`; meta-relations are `(R, predicate, X) [Status]`. Pick one notation for both.

- [ ] **B23. §24 mixes two intents.** [A3] Some items (24.1 Patterns) are "removed from v0 because unvalidated" (defensive). Others (24.5 HNSW) are "add when scale demands" (forward roadmap). Separate or label.

- [ ] **B24. §22 citation form inconsistent.** [A3] Some entries cite paper + venue + year, others just author + year, others URLs. Standardize.

- [ ] **B25. Repetition between layers — concrete instances.** [A3] Durability in §1.4, §5 Inv 1, and §18 — three times. Wolfram-irreducibility argument in §1.7 and §6.7 nearly verbatim. §12 restates §11.13.

- [ ] **B26. Add a per-step latency budget table.** [A2, A3] Required to defend the 200–300 ms claim. (Tied to A8.)

- [ ] **B27. Add cross-cutting "Failure Modes & Mitigations" section.** [A3] §10.6 has it for regions; nothing similar for extractors, recognition, supersession, replay divergence. (Tied to A11.)

- [ ] **B28. Reframe "side tables" as "payload tables" / "storage shapes."** [C] §3.5 and §7.3 call them "side tables," which reads like an apology. They're not adjacent to the hypergraph — they're owned by `Hypergraph` and are part of it. Proposed fixes: (1) consistent naming across §3.5, §7.3, §9.2 ("payload tables" or "storage shapes"); (2) lead §3.5 / §7.3 with "the hypergraph has three storage shapes — elements (identity), relations (claims), and payload tables (dense and typed leaf data) — all owned by the same struct"; (3) clarify in §1.6 that "one primitive" means one identity primitive, not uniform storage layout. Rationale: the hypergraph primitive is for discrete identity + relational structure; dense numeric data (embeddings, prototypes) and typed leaves (values) are categorically different storage shapes that every property/graph DB resolves the same way. Dropping payload tables in favor of pure relations either tanks latency (~100× storage blow-up + hashmap probes for cosine) or violates Inv 4 (string-encoded values driving control flow).
