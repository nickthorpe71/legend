//! `build_relations` — build Relations + Events.
//!
//! Pure HashMap inserts + index updates over what `run_extractors` already
//! proposed. For each surviving extractor proposal:
//!
//! 1. Resolve every span to an existing Element, or mint a fresh one.
//!    Reuse folds the new contextualized vector into the element's
//!    running centroid.
//! 2. Resolve `attr_label` to an attribute-name Element (exact-name
//!    today; cosine-dedup deferred — see `resolve_attribute_name`).
//! 3. Build one Relation per proposal — attribute list assembled
//!    from the extractor's frame.
//! 4. Stamp status from `policy.ner_assertion_threshold`; stamp
//!    `stats.confidence` from `policy.default_conf × extractor_conf`.
//! 5. Write a `source` meta-relation if the tick's source is `Some`.
//! 6. Incrementally update all seven `build_relations` indices.
//!
//! See `new_foundation.md`.

use std::collections::HashMap;

use crate::embed::{SequenceContext, embed_sequence_with_offsets, embed_text, fold_streaming_centroid};
use crate::tick_pipeline::coref::CorefDecision;
use crate::tick_pipeline::orthographic::OrthographicChunk;
use crate::tick_pipeline::relation_patterns::{ObjectRef, RelationCandidate};
use crate::tick_pipeline::run_extractors::ExtractionOutput;
use crate::types::{
    Attribute, Element, ElementId, Hypergraph, MemoryStats, Polarity, Relation, RelationId,
    RelationStatus, Term,
};

/// Surface of `build_relations`'s work in a single tick — what got minted and
/// how many new attribute names appeared. Frame's `durable_writes`
/// reads `minted_elements`; observability reads `attr_names_minted`.
#[derive(Debug, Default, Clone)]
pub struct MintedRelations {
    pub minted_elements: Vec<ElementId>,
    pub minted_relations: Vec<RelationId>,
    /// Every element touched by `build_relations` this tick — both newly minted
    /// and resolved-by-reuse via `resolve_span` /
    /// `resolve_label_element`. Deduped, order = first reference.
    /// `hebbian_and_salience` walks this list to retrieve live relations involving
    /// each element so the frame surfaces prior knowledge for query
    /// ticks (otherwise `focused_relations` would only contain new
    /// writes). Used as the retrieval seed in
    /// `build_reinforcement_set`.
    pub referenced_elements: Vec<ElementId>,
    /// New attribute-name elements minted this tick. Threshold
    /// signalling lives at the caller — `build_relations` just counts.
    pub attr_names_minted: u32,
    /// Per-tick uncertainty signals raised by `build_relations`. `LowConfidence`
    /// fires when any minted base relation lands `Defeasible` —
    /// extraction was below `ner_assertion_threshold`. The frame
    /// merges these into its `uncertainty` field.
    pub uncertainty: Vec<crate::types::UncertaintySignal>,
}

/// Read-only shared inputs threaded through the phase helpers. Bundled
/// so each helper takes one `Copy` context instead of three loose
/// borrows: the raw `input_text` (span offsets index into it), the
/// tick's precomputed BERT `seq_ctx`, and the adjusted `policy`.
#[derive(Clone, Copy)]
struct RelationBuildContext<'a> {
    input_text: &'a str,
    seq_ctx: SequenceContext<'a>,
    policy: &'a crate::types::Policy,
}

/// Build relations + events for one tick.
///
/// - `input_text` — the raw input. Spans are character offsets into this.
/// - `hypergraph` — mutated in place; new Elements/Relations appended,
///   all seven `build_relations` indices updated incrementally.
/// - `out` — the `ExtractionOutput` from `run_extractors`.
/// - `policy` — adjusted Policy from `adjust_policy` (Steps 4–12 read this).
/// - `source` — the tick's source (§11.1). If `Some`, every minted
///   base relation gets a companion `(target: R, source: source)`.
///
/// Handles binary `instance_of` and pattern-RE proposals plus
/// n-ary event merging and the novelty branch. Coref decisions
/// (coref decisions output) short-circuit element resolution via §4a's
/// `apply_coref_decisions`.
pub fn build_relations(
    input_text: &str,
    hypergraph: &mut Hypergraph,
    out: &ExtractionOutput,
    policy: &crate::types::Policy,
    source: Option<ElementId>,
) -> MintedRelations {
    let mut result = MintedRelations::default();

    // Once-per-tick BERT forward pass over the whole input. Every span
    // resolution below mean-pools out of this precomputed sequence
    // instead of re-running the forward pass — span resolution fires
    // 5–10× per tick over the same text, so this is the difference
    // between one forward pass and one per span.
    let (sequence, offsets) = embed_sequence_with_offsets(input_text);
    let seq_ctx = SequenceContext {
        sequence: &sequence,
        offsets: &offsets,
    };
    let context = RelationBuildContext {
        input_text,
        seq_ctx,
        policy,
    };

    // Char-span → ElementId cache for this tick. Two proposals over
    // the same span resolve to the same element without re-running
    // mint/dedup and without bumping access_count twice for one mention.
    let mut span_cache: HashMap<(usize, usize), ElementId> = HashMap::new();

    // Base relations minted this tick, in mint order. Each phase below
    // appends to this; §7 attaches a `source` companion to every entry
    // and the uncertainty pass scans their statuses.
    let mut base_rel_ids: Vec<RelationId> = Vec::new();

    // §4a — Coref override. Pre-populate the span cache from coref
    // decisions BEFORE any proposal-driven resolution runs. Each decision
    // folds the pronoun's contextualized vector into the antecedent and
    // pins the pronoun's char range to the antecedent's id. Downstream
    // `resolve_span` calls for that range short-circuit on the cache.
    // No-op behaviorally today (coref decisions is a stub), but the wiring
    // lands so once `recent_focus` lights up the override is live.
    apply_coref_decisions(hypergraph, seq_ctx, &out.known.coref, &mut span_cache);
    build_pattern_relations(hypergraph, context, out, &mut span_cache, &mut base_rel_ids, &mut result);
    build_novelty_relations(hypergraph, context, out, &mut span_cache, &mut base_rel_ids, &mut result);
    if !emit_source_meta_relations(hypergraph, source, &base_rel_ids, &mut result) {
        // Pack-shape error: `source` was requested but the seed pack has
        // no `source` attribute name. Nothing more to do — bail before
        // the uncertainty pass, matching the original early return.
        return result;
    }

    // ── Uncertainty: LowConfidence ──────────────────────────────────
    // Any minted base relation that landed `Defeasible` came from a
    // sub-threshold extraction — surface that so `assemble_frame`'s frame can
    // forward `LowConfidence` to the consumer. Single dedup'd signal
    // per tick; the consumer doesn't need the count, just the flag.
    if base_rel_ids
        .iter()
        .any(|&rid| hypergraph.relations[rid.0 as usize].status == RelationStatus::Defeasible)
    {
        result
            .uncertainty
            .push(crate::types::UncertaintySignal::LowConfidence);
    }

    result
}

// ─── §-phase helpers (the body of `build_relations`, named) ───────────

/// §6.1–§6.3 — the known branch. Mints, in order:
///
/// - §6.1 binary `instance_of` proposals (NER + Temporal),
/// - §6.2 binary pattern RE proposals, skipping any subsumed by an
///   n-ary event merge,
/// - §6.3 n-ary event relations, one per merge group.
///
/// Merge groups are computed up front so the §6.2 loop can skip the
/// from/to proposals they subsume. Every minted base relation is
/// appended to both `base_rel_ids` (for the §7 source companion + the
/// uncertainty scan) and `result.minted_relations`.
fn build_pattern_relations(
    hypergraph: &mut Hypergraph,
    context: RelationBuildContext,
    out: &ExtractionOutput,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    base_rel_ids: &mut Vec<RelationId>,
    result: &mut MintedRelations,
) {
    let RelationBuildContext { input_text, seq_ctx, policy } = context;
    // ── §6.1 — Binary instance_of proposals (NER + Temporal) ────────
    for p in &out.known.instance_of {
        let subj_text = &input_text[p.subject_char_start..p.subject_char_end];
        let subj_id = resolve_span(
            hypergraph,
            seq_ctx,
            p.subject_char_start,
            p.subject_char_end,
            subj_text,
            span_cache,
            result,
        );
        let label = match &p.object {
            ObjectRef::Label(l) => l.as_str(),
            ObjectRef::Span { .. } => {
                // Span-typing always emits Label objects. Defense-in-depth:
                // skip anything else so a future mis-emit doesn't crash.
                continue;
            }
        };
        let label_id = resolve_label_element(hypergraph, label, result);
        let attr_ctx = pattern_attr_context(input_text, p);
        let attr_id =
            resolve_attribute_name(hypergraph, seq_ctx, &p.attribute_name, attr_ctx, policy, result);
        let Some(rel_id) = mint_or_reuse_base_relation(
            hypergraph,
            vec![
                Attribute {
                    name: hypergraph.subject_attr,
                    value: Term::Element(subj_id),
                },
                Attribute {
                    name: attr_id,
                    value: Term::Element(label_id),
                },
            ],
            p.status,
            confidence_for(p.confidence, policy),
        ) else {
            // Self-referential triple (e.g. `language → instance_of →
            // language`). Skip silently — the extractor's tag and the
            // surface span just happened to collide.
            continue;
        };
        base_rel_ids.push(rel_id);
        result.minted_relations.push(rel_id);
    }

    // ── §6.3 — N-ary event merge (computed up front) ────────────────
    // Identify groups of verb-anchored from/to proposals that will
    // collapse into one event relation. Subsumed indices are skipped
    // in the binary loop below.
    let merge_groups = compute_event_merge_groups(&out.known.relations);
    let mut subsumed: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for g in &merge_groups {
        subsumed.insert(g.from_idx);
        subsumed.insert(g.to_idx);
    }

    // ── §6.2 — Binary pattern RE proposals (non-subsumed only) ──────
    for (idx, p) in out.known.relations.iter().enumerate() {
        if subsumed.contains(&idx) {
            continue;
        }
        let Some(rel_id) =
            mint_candidate_relation(hypergraph, input_text, seq_ctx, p, policy, span_cache, result)
        else {
            // Self-referential extraction; drop.
            continue;
        };
        base_rel_ids.push(rel_id);
        result.minted_relations.push(rel_id);
    }

    // ── §6.3 — Emit n-ary event relations ───────────────────────────
    for (seq, g) in merge_groups.iter().enumerate() {
        if let Some(rel_id) = mint_event_relations(
            hypergraph,
            input_text,
            seq_ctx,
            &out.known.relations,
            g,
            seq,
            policy,
            span_cache,
            result,
        ) {
            base_rel_ids.push(rel_id);
        }
    }
}

/// §6.4 — Novelty branch. Mints an Element per novelty chunk (Phrase +
/// Token) so spans NER missed still get substrate identities; the span
/// cache dedups overlapping chunks against the spans the known branch
/// already resolved. Then mints a `Defeasible` Relation per
/// `NoveltyRelation` triple — candidates for replay confirmation. Each
/// minted triple is appended to `base_rel_ids` and
/// `result.minted_relations`.
fn build_novelty_relations(
    hypergraph: &mut Hypergraph,
    context: RelationBuildContext,
    out: &ExtractionOutput,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    base_rel_ids: &mut Vec<RelationId>,
    result: &mut MintedRelations,
) {
    let RelationBuildContext { input_text, seq_ctx, policy } = context;
    mint_novelty_chunks(hypergraph, seq_ctx, &out.novelty.chunks, span_cache, result);
    for nr in &out.novelty.relations {
        let Some(rel_id) =
            mint_candidate_relation(hypergraph, input_text, seq_ctx, nr, policy, span_cache, result)
        else {
            // Self-referential novelty extraction; drop.
            continue;
        };
        base_rel_ids.push(rel_id);
        result.minted_relations.push(rel_id);
    }
}

/// §7 — Source meta-relations. When the tick's source is `Some`, attach
/// a companion `(target: base, source: source)` meta-relation to every
/// base relation minted this tick. Returns `false` iff `source` was
/// requested but the seed pack has no `source` attribute name — a
/// pack-shape error the caller treats as a hard stop. Returns `true`
/// when `source` is `None` (nothing to emit) or emission succeeded.
fn emit_source_meta_relations(
    hypergraph: &mut Hypergraph,
    source: Option<ElementId>,
    base_rel_ids: &[RelationId],
    result: &mut MintedRelations,
) -> bool {
    let Some(source_id) = source else {
        return true;
    };
    let Some(source_attr) = hypergraph.by_name.get("source").and_then(|v| v.first().copied()) else {
        return false; // pack-shape error; nothing to do
    };
    for &base_id in base_rel_ids {
        let meta_id = mint_relation(
            hypergraph,
            vec![
                Attribute {
                    name: hypergraph.target_attr,
                    value: Term::Relation(base_id),
                },
                Attribute {
                    name: source_attr,
                    value: Term::Element(source_id),
                },
            ],
            RelationStatus::Entailed,
            1.0,
        );
        result.minted_relations.push(meta_id);
    }
    true
}

/// Resolve a character span to an `ElementId`. Cached per tick.
/// Exact-name lookup; on miss, mint with the span's contextualized
/// embedding. Reuse folds the contextualized vector into the
/// element's running centroid and bumps `access_count`.
fn resolve_span(
    hypergraph: &mut Hypergraph,
    seq_ctx: SequenceContext,
    char_start: usize,
    char_end: usize,
    span_text: &str,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut MintedRelations,
) -> ElementId {
    if let Some(&id) = span_cache.get(&(char_start, char_end)) {
        push_referenced_unique(result, id);
        return id;
    }

    let observed = seq_ctx.embed_span(char_start, char_end);

    // 1. Exact-name lookup. If multiple IDs share the name, pick the
    //    one with highest cosine to the contextualized vector (or the
    //    first, if we have no contextualized vector to compare against).
    let existing = hypergraph.by_name.get(span_text).cloned().unwrap_or_default();
    if let Some(id) = pick_best_by_cosine(&existing, observed.as_deref(), hypergraph) {
        // Reuse path: fold the new mention's vector + bump access count.
        if let Some(obs) = observed.as_ref() {
            let prev_n = hypergraph.elements[id.0 as usize].stats.access_count;
            fold_streaming_centroid(&mut hypergraph.elements[id.0 as usize].embedding, obs, prev_n);
        }
        let el = &mut hypergraph.elements[id.0 as usize];
        el.stats.access_count = el.stats.access_count.saturating_add(1);
        el.stats.last_seen = hypergraph.clock;
        span_cache.insert((char_start, char_end), id);
        push_referenced_unique(result, id);
        return id;
    }

    // 2. Mint. Use contextualized vector if available; otherwise fall
    //    back to surface-form text embedding (matches the seed-pack
    //    void-member fallback path).
    let embedding = observed.unwrap_or_else(|| embed_text(span_text));
    // Mint-time confidence. The Policy isn't threaded into every mint
    // site; reading the rest-state default off the Hypergraph is good
    // enough — `assemble_frame`'s frame walker reads `stats.confidence`
    // for ranking, not for truth-bearing.
    let default_conf = hypergraph.policy.default_conf;
    let id = mint_element(
        hypergraph,
        vec![span_text.to_string()],
        embedding,
        Polarity::Signal,
        default_conf,
    );
    span_cache.insert((char_start, char_end), id);
    result.minted_elements.push(id);
    push_referenced_unique(result, id);
    id
}

/// Push `id` onto `result.referenced_elements` unless it's already
/// present. O(N) lookup, but N is bounded by spans-per-tick (~10s)
/// so a HashSet would cost more than it saves.
pub(crate) fn push_referenced_unique(result: &mut MintedRelations, id: ElementId) {
    if !result.referenced_elements.contains(&id) {
        result.referenced_elements.push(id);
    }
}

/// True iff `rid` is a cache relation — minted by `supersede` with a
/// `current_<property>` attribute name. These represent the
/// substrate's current belief for a `(target, property)` bucket and
/// should outrank stale binary assertions in the frame. Walks the
/// relation's attributes looking for any attribute whose surface
/// name starts with `"current_"`. O(attrs_per_relation).
pub(crate) fn is_cache_relation(hypergraph: &Hypergraph, rid: RelationId) -> bool {
    // invariant: attr.name is an attribute-name element minted before the
    // relation that references it, so indexing `elements[attr.name]` is safe.
    let r = &hypergraph.relations[rid.0 as usize];
    for attr in &r.attributes {
        if attr.name == hypergraph.subject_attr || attr.name == hypergraph.target_attr {
            continue;
        }
        if let Some(name) = hypergraph.elements[attr.name.0 as usize].names.first()
            && name.starts_with("current_")
        {
            return true;
        }
    }
    false
}

/// True iff `rid` is a seed-graph structural relation — an
/// `instance_of` pin from a region/frame anchor element to its
/// class element (e.g. `tasks → instance_of → region_class`,
/// `meta → instance_of → reference_frame_class`). These exist to
/// make the routing tree work; they're load-bearing in the
/// substrate but uniformly high-confidence noise in the frame.
/// Filtered out of `focused_relations` and `current_state` in
/// `assemble_frame`.
pub(crate) fn is_structural_relation(hypergraph: &Hypergraph, rid: RelationId) -> bool {
    let r = &hypergraph.relations[rid.0 as usize];
    // Pin to class anchors: `instance_of` → region_class /
    // reference_frame_class. Seed scaffolding for routing topology.
    let instance_of_attr = hypergraph.by_name.get("instance_of").and_then(|v| v.first().copied());
    if let Some(instance_of_attr) = instance_of_attr {
        for attr in &r.attributes {
            if attr.name != instance_of_attr {
                continue;
            }
            if let crate::types::Term::Element(eid) = attr.value
                && (eid == hypergraph.region_class || eid == hypergraph.reference_frame_class)
            {
                return true;
            }
        }
    }
    // Region-topology relations: `(R, prototype, P)`,
    // `(R_a, parent_region, R_b)`, `(R_a, lateral_region, R_b)`,
    // `(E, member_of, R)`. Same kind of scaffolding — they exist to
    // wire the routing tree, not as content the consumer should see.
    for attr in &r.attributes {
        if attr.name == hypergraph.prototype_attr || attr.name == hypergraph.parent_region_attr {
            return true;
        }
        if let Some(name) = hypergraph.elements[attr.name.0 as usize].names.first()
            && matches!(name.as_str(), "lateral_region" | "member_of")
        {
            return true;
        }
    }
    false
}

/// Resolve a type-class label (e.g. `"person"`, `"weekday"`, `"event"`)
/// to an `ElementId`. Mints a fresh element if no match exists.
/// Distinct from `resolve_span` because labels don't have char offsets
/// or a contextualized vector — they're symbolic.
fn resolve_label_element(hypergraph: &mut Hypergraph, label: &str, result: &mut MintedRelations) -> ElementId {
    let existing = hypergraph.by_name.get(label).cloned().unwrap_or_default();
    if let Some(&id) = existing.first() {
        // Reuse, bump access. No streaming centroid (no observed vector).
        let el = &mut hypergraph.elements[id.0 as usize];
        el.stats.access_count = el.stats.access_count.saturating_add(1);
        el.stats.last_seen = hypergraph.clock;
        push_referenced_unique(result, id);
        return id;
    }
    let default_conf = hypergraph.policy.default_conf;
    let id = mint_element(
        hypergraph,
        vec![label.to_string()],
        embed_text(label),
        Polarity::Signal,
        default_conf,
    );
    result.minted_elements.push(id);
    push_referenced_unique(result, id);
    id
}

/// Layered predicate consolidation: resolve a surface predicate form
/// (e.g. `"instance_of"`, `"from"`, `"using"`) to an attribute-name
/// Element via four cascading layers:
///
/// 1. **Exact-name lookup** in `hypergraph.by_name`. Prefers Signal-polarity
///    candidates over Void (so `"with"` and `"at"` bind to their
///    attribute-name siblings, not their adposition members).
/// 2. **Typed-relation lexicon** — surface verbs and prepositional
///    phrases mapped to canonical seed predicates. `"uses"` →
///    `"with"`, `"lives in"` → `"at"`, etc. Closed-class linguistic
///    primitive (same status as the SVO irregular-verb lexicon).
/// 3. **Embedding-knn over predicate centroids** (only when `context`
///    is provided). The novel predicate's contextualized span vector
///    is compared cosine-wise against existing attribute-name
///    Signal-polarity elements; ≥ `policy.attribute_name_dedup_threshold`
///    → reuse the top hit. Per `new_foundation_v0_core.md:820-836`.
/// 4. **Mint** a fresh attribute-name element. Embedding: the
///    contextualized span vector from this observation when
///    `context` is provided, else the bare-token embedding
///    (legacy fallback for canonical-name minters that don't carry
///    a char range).
///
/// `context` is `(input_text, attr_char_start, attr_char_end)`. SVO
/// populates it; canonical-name emitters pass `None`.
fn resolve_attribute_name(
    hypergraph: &mut Hypergraph,
    seq_ctx: SequenceContext,
    label: &str,
    context: Option<(usize, usize)>,
    policy: &crate::types::Policy,
    result: &mut MintedRelations,
) -> ElementId {
    // ── Layer 1: exact-name lookup ─────────────────────────────────
    if let Some(ids) = hypergraph.by_name.get(label)
        && let Some(&id) = ids.first()
    {
        for candidate in ids {
            if hypergraph.elements[candidate.0 as usize].polarity == Polarity::Signal {
                return *candidate;
            }
        }
        return id;
    }

    // ── Layer 2: typed-relation lexicon ────────────────────────────
    if let Some(canonical) = typed_relation_lookup(label)
        && let Some(ids) = hypergraph.by_name.get(canonical)
    {
        for candidate in ids {
            if hypergraph.elements[candidate.0 as usize].polarity == Polarity::Signal {
                return *candidate;
            }
        }
    }

    // ── Layer 3: embedding-knn over predicate centroids ────────────
    let contextualized: Option<Vec<f32>> =
        context.and_then(|(s, e)| seq_ctx.embed_span(s, e));
    if let Some(ref query_vec) = contextualized
        && let Some(neighbor) = knn_attribute_name(hypergraph, query_vec, policy.attribute_name_dedup_threshold)
    {
        return neighbor;
    }

    // ── Layer 4: mint ──────────────────────────────────────────────
    // Prefer the contextualized embedding when we have it; fall back
    // to the bare-token embedding for canonical-name minters.
    let embedding = contextualized.unwrap_or_else(|| embed_text(label));
    let default_conf = hypergraph.policy.default_conf;
    let id = mint_element(
        hypergraph,
        vec![label.to_string()],
        embedding,
        Polarity::Signal,
        default_conf,
    );
    result.minted_elements.push(id);
    result.attr_names_minted = result.attr_names_minted.saturating_add(1);
    id
}

/// Build the `(attr_start, attr_end)` char range `resolve_attribute_name`
/// uses to mean-pool a predicate's contextualized embedding out of the
/// tick's precomputed sequence. Returns `None` when the candidate's
/// emitter didn't populate a char range (canonical-name patterns like
/// NER-anchored templates and span-typing). `input_text` is read only
/// to bounds-check the range against the input length.
fn pattern_attr_context(input_text: &str, p: &RelationCandidate) -> Option<(usize, usize)> {
    match (p.attribute_char_start, p.attribute_char_end) {
        (Some(s), Some(e)) if e <= input_text.len() && s < e => Some((s, e)),
        _ => None,
    }
}

/// Closed-class mapping from common verb/preposition surface forms to
/// canonical seed predicate names. Layer (2) of
/// [`resolve_attribute_name`]. Entries are case-insensitive on lookup.
///
/// Curated for the predicates SVO most commonly produces (binding-
/// shaped verbs like "use/has/with" and locative-shaped verbs like
/// "lives in"/"works at"). Add entries as benches surface predicates
/// that should consolidate but currently mint fresh elements.
fn typed_relation_lookup(label: &str) -> Option<&'static str> {
    let lower = label.to_ascii_lowercase();
    match lower.as_str() {
        // Instrumental / co-participant.
        "uses" | "using" | "use" | "used" => Some("with"),
        "has" | "have" | "had" | "owns" | "with" => Some("with"),
        // Locative.
        "lives in" | "located at" | "located in" | "in" => Some("at"),
        "works at" | "works in" | "at" => Some("at"),
        // Origin / destination.
        "came from" | "originally from" => Some("from"),
        "went to" | "moved to" | "transferred to" | "switched to" => Some("to"),
        // Taxonomic.
        "is a" | "is an" | "is" | "are" | "instance of" => Some("instance_of"),
        _ => None,
    }
}

/// Find the highest-cosine Signal-polarity attribute-name element
/// whose embedding cosines ≥ `threshold` against `query`. Returns
/// `None` if no element clears the threshold. The "attribute-name
/// element" universe is bounded by `hypergraph.relations_by_attribute_name`
/// — the index already keys on every attribute-name element that's
/// been used in a relation, so this is the natural enumeration.
fn knn_attribute_name(
    hypergraph: &Hypergraph,
    query: &[f32],
    threshold: f32,
) -> Option<ElementId> {
    let mut best: Option<(ElementId, f32)> = None;
    for &eid in hypergraph.relations_by_attribute_name.keys() {
        let el = &hypergraph.elements[eid.0 as usize];
        if el.polarity != Polarity::Signal {
            continue;
        }
        // Embeddings are L2-normalized (the substrate contract), so cosine
        // reduces to a dot product — same path `pick_best_by_cosine` takes.
        let cos = crate::math::dot(query, &el.embedding);
        if cos >= threshold && best.is_none_or(|(_, b)| cos > b) {
            best = Some((eid, cos));
        }
    }
    best.map(|(eid, _)| eid)
}

/// Mint (or reuse) a binary base relation from a single
/// `RelationCandidate`. The shared path for both §6.2 binary pattern
/// RE proposals and §6.4 novelty triples — they're structurally
/// identical: resolve subject span, resolve object, resolve the
/// predicate's attribute-name element (contextualized when the
/// candidate carries an attribute char range), then mint-or-reuse the
/// `(subject, predicate, object)` triple stamped with the candidate's
/// own status and `default_conf × candidate.confidence`. The only
/// difference between the two callsites is the candidate's `status`
/// (novelty triples arrive `Defeasible`), which the candidate already
/// carries — so one helper covers both.
fn mint_candidate_relation(
    hypergraph: &mut Hypergraph,
    input_text: &str,
    seq_ctx: SequenceContext,
    candidate: &RelationCandidate,
    policy: &crate::types::Policy,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut MintedRelations,
) -> Option<RelationId> {
    let subj_text = &input_text[candidate.subject_char_start..candidate.subject_char_end];
    let subj_id = resolve_span(
        hypergraph,
        seq_ctx,
        candidate.subject_char_start,
        candidate.subject_char_end,
        subj_text,
        span_cache,
        result,
    );
    let obj_id = resolve_object(hypergraph, input_text, seq_ctx, &candidate.object, span_cache, result);
    let attr_ctx = pattern_attr_context(input_text, candidate);
    let attr_id =
        resolve_attribute_name(hypergraph, seq_ctx, &candidate.attribute_name, attr_ctx, policy, result);
    mint_or_reuse_base_relation(
        hypergraph,
        vec![
            Attribute {
                name: hypergraph.subject_attr,
                value: Term::Element(subj_id),
            },
            Attribute {
                name: attr_id,
                value: Term::Element(obj_id),
            },
        ],
        candidate.status,
        confidence_for(candidate.confidence, policy),
    )
}

/// Resolve an [`ObjectRef`] to an `ElementId`. Span objects route
/// through [`resolve_span`] (with the span-cache); Label objects
/// route through [`resolve_label_element`] (seed-pack lookup +
/// mint-if-missing).
fn resolve_object(
    hypergraph: &mut Hypergraph,
    input_text: &str,
    seq_ctx: SequenceContext,
    object: &ObjectRef,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut MintedRelations,
) -> ElementId {
    match object {
        ObjectRef::Span {
            char_start,
            char_end,
        } => {
            let obj_text = &input_text[*char_start..*char_end];
            resolve_span(
                hypergraph,
                seq_ctx,
                *char_start,
                *char_end,
                obj_text,
                span_cache,
                result,
            )
        }
        ObjectRef::Label(label) => resolve_label_element(hypergraph, label, result),
    }
}

/// Mint a fresh Element + update `by_name`. Returns the new ID.
pub(crate) fn mint_element(
    hypergraph: &mut Hypergraph,
    names: Vec<String>,
    embedding: Vec<f32>,
    polarity: Polarity,
    default_conf: f32,
) -> ElementId {
    let id = ElementId(hypergraph.elements.len() as u32);
    let stats = MemoryStats {
        confidence: default_conf,
        plasticity: 1.0,
        last_seen: hypergraph.clock,
        ..MemoryStats::default()
    };
    let el = Element {
        id,
        names: names.clone(),
        stats,
        created_at: hypergraph.clock,
        embedding,
        polarity,
    };
    hypergraph.elements.push(el);
    for name in names {
        hypergraph.by_name.entry(name).or_default().push(id);
    }
    id
}

/// Append a Relation, run the §8 index updates, return its ID.
pub(crate) fn mint_relation(
    hypergraph: &mut Hypergraph,
    attributes: Vec<Attribute>,
    status: RelationStatus,
    confidence: f32,
) -> RelationId {
    let id = RelationId(hypergraph.relations.len() as u32);
    let stats = MemoryStats {
        confidence,
        plasticity: 1.0,
        last_seen: hypergraph.clock,
        ..MemoryStats::default()
    };
    let r = Relation {
        id,
        attributes,
        status,
        stats,
        priority: 0,
        created_at: hypergraph.clock,
    };
    index_relation(hypergraph, &r);
    hypergraph.relations.push(r);
    id
}

/// Mint a base relation OR reuse an existing one with the same
/// attribute-set shape. Cross-tick dedup: if the user says
/// "Sarah called me" twice, the second tick reuses the existing
/// `(Sarah, instance_of, person)` relation so `support_count`
/// accumulates and the promotion gate (§11.11) can fire.
///
/// Equality check: same attribute count AND every (name, value)
/// pair in `attributes` also appears in the candidate's attribute
/// list. Order doesn't matter — same set = same claim.
///
/// On reuse: bumps `last_seen`, leaves `status` and `confidence`
/// alone (`hebbian_and_salience` / `supersede` handle those). Status promotion via
/// `hebbian_and_salience`'s gate is the right path to upgrade Defeasible →
/// Asserted on re-encounter.
///
/// Use this for base relations (instance_of, n-ary events, binary
/// pattern RE, novelty triples). DON'T use it for meta-relations
/// (`source`, `derived_from`, `supersedes`, `intervened`) — each
/// of those is a distinct provenance link that should mint fresh.
pub(crate) fn mint_or_reuse_base_relation(
    hypergraph: &mut Hypergraph,
    attributes: Vec<Attribute>,
    status: RelationStatus,
    confidence: f32,
) -> Option<RelationId> {
    if is_self_referential(hypergraph, &attributes) {
        return None;
    }
    if let Some(existing) = find_matching_relation(hypergraph, &attributes) {
        // Reuse path: bump last_seen so decay walks can age this
        // relation correctly. Don't touch status or confidence —
        // those belong to `supersede`/10/promotion. support_count tracks
        // independent re-extractions (this branch is the canonical
        // re-extraction event); `hebbian_and_salience`'s spreading activation
        // bumps `focus_success_count` instead.
        let r = &mut hypergraph.relations[existing.0 as usize];
        r.stats.last_seen = hypergraph.clock;
        r.stats.support_count = r.stats.support_count.saturating_add(1);
        return Some(existing);
    }
    Some(mint_relation(hypergraph, attributes, status, confidence))
}

/// True iff `attributes` describes a self-referential relation: the
/// subject element appears as the value of one of the non-subject
/// attribute slots. Catches degenerate extractions like
/// `language → instance_of → language` that the NER+span_typing
/// pipeline emits when a noun's surface form happens to match its
/// type label. Meta-relations (subject role bound on `target_attr`
/// with a `Term::Relation` value) are never self-referential under
/// this rule and pass through.
fn is_self_referential(hypergraph: &Hypergraph, attributes: &[Attribute]) -> bool {
    let subj = attributes
        .iter()
        .find(|a| a.name == hypergraph.subject_attr)
        .and_then(|a| match a.value {
            Term::Element(eid) => Some(eid),
            _ => None,
        });
    let Some(subj_eid) = subj else {
        return false;
    };
    attributes.iter().any(|a| {
        if a.name == hypergraph.subject_attr {
            return false;
        }
        matches!(a.value, Term::Element(eid) if eid == subj_eid)
    })
}

/// Find a pre-existing **live** relation whose attribute set matches
/// `proposed` exactly. Lookup is anchored on the first Element-
/// valued attribute target (typically the subject) via
/// `relations_by_element` — O(relations on subject) per check,
/// small in practice.
///
/// "Live" = `Asserted | Entailed | Defeasible`. We deliberately
/// skip `Superseded` and `Retracted`:
/// - Superseded means `supersede` retired this state; reusing it would
///   confuse "we asserted X now" with "X was true previously."
///   Example: tick 1 sets `current_date=Tuesday`; tick 2 supersedes
///   to Friday; tick 3 sets `current_date=Tuesday` again. Tick 3
///   should mint a fresh Asserted relation (and `supersede` should
///   supersede tick 2's current Friday cache) — not resurrect
///   tick 1's already-superseded one.
/// - Retracted means the relation was explicitly withdrawn; reusing
///   would silently revive retracted state.
fn find_matching_relation(hypergraph: &Hypergraph, proposed: &[Attribute]) -> Option<RelationId> {
    let anchor = proposed.iter().find_map(|a| match a.value {
        Term::Element(e) => Some(e),
        _ => None,
    })?;
    let candidates = hypergraph.relations_by_element.get(&anchor)?;
    for &rid in candidates {
        let r = &hypergraph.relations[rid.0 as usize];
        if matches!(
            r.status,
            RelationStatus::Superseded | RelationStatus::Retracted
        ) {
            continue;
        }
        if r.attributes.len() != proposed.len() {
            continue;
        }
        if attributes_match(&r.attributes, proposed) {
            return Some(rid);
        }
    }
    None
}

/// Attribute-set equality: every (name, value) pair in `a` appears
/// in `b`, and vice versa (lengths are pre-checked equal at the
/// callsite). O(n²) but n is typically 2-5 so it's negligible.
fn attributes_match(a: &[Attribute], b: &[Attribute]) -> bool {
    for ax in a {
        let matched = b
            .iter()
            .any(|bx| bx.name == ax.name && attr_value_eq(bx.value, ax.value));
        if !matched {
            return false;
        }
    }
    true
}

fn attr_value_eq(a: Term, b: Term) -> bool {
    match (a, b) {
        (Term::Element(x), Term::Element(y)) => x == y,
        (Term::Relation(x), Term::Relation(y)) => x == y,
        _ => false,
    }
}

/// Apply §8 incremental index updates for one Relation. Mirrors the
/// build-from-scratch loop in `seed::rebuild_indices` so the two
/// paths converge.
fn index_relation(hypergraph: &mut Hypergraph, r: &Relation) {
    let r_id = r.id;
    // First pass: per-attribute indices + collect any parent relations
    // this relation points at via `target` (Term::Relation). That set
    // drives the second pass for `meta_relation_presence`.
    let mut parent_relations: Vec<RelationId> = Vec::new();
    for attr in &r.attributes {
        hypergraph.relations_by_attribute_name
            .entry(attr.name)
            .or_default()
            .push(r_id);
        match attr.value {
            Term::Element(e) => {
                hypergraph.relations_by_element.entry(e).or_default().push(r_id);
                *hypergraph.attribute_value_counts.entry((attr.name, e)).or_insert(0) += 1;
            }
            Term::Relation(parent) => {
                if attr.name == hypergraph.target_attr {
                    hypergraph.meta_relations_by_subject
                        .entry(parent)
                        .or_default()
                        .push(r_id);
                    parent_relations.push(parent);
                } else {
                    hypergraph.meta_relations_by_object
                        .entry(parent)
                        .or_default()
                        .push(r_id);
                }
            }
        }
    }
    // Second pass: for each `parent` we point at via `target`, mark
    // every NON-target sibling attribute as present on the parent.
    // This is the semantics the field comment promises: "does relation
    // R carry a meta-attribute with this name?" Used by `supersede`'s
    // `intervened` gate as an O(1) lookup.
    for &parent in &parent_relations {
        for attr in &r.attributes {
            if attr.name != hypergraph.target_attr {
                hypergraph.meta_relation_presence.insert((parent, attr.name), true);
            }
        }
    }
    for i in 0..r.attributes.len() {
        for j in 0..r.attributes.len() {
            if i == j {
                continue;
            }
            *hypergraph.attribute_co_counts
                .entry((r.attributes[i].name, r.attributes[j].name))
                .or_insert(0) += 1;
        }
    }
}

fn pick_best_by_cosine(
    candidates: &[ElementId],
    observed: Option<&[f32]>,
    hypergraph: &Hypergraph,
) -> Option<ElementId> {
    if candidates.is_empty() {
        return None;
    }
    let Some(obs) = observed else {
        return Some(candidates[0]);
    };
    let mut best: Option<(ElementId, f32)> = None;
    for &id in candidates {
        let cand_emb = &hypergraph.elements[id.0 as usize].embedding;
        let score = crate::math::dot(obs, cand_emb);
        match best {
            Some((_, b)) if score <= b => {}
            _ => best = Some((id, score)),
        }
    }
    best.map(|(id, _)| id)
}

/// Confidence stamped onto the Relation. Spec: `default_conf ×
/// extractor_confidence`. `default_conf` is intent-modulated by
/// `adjust_policy`.
fn confidence_for(extractor_conf: f32, policy: &crate::types::Policy) -> f32 {
    (policy.default_conf * extractor_conf).clamp(0.0, 1.0)
}

// ─── Property kind inference (used by n-ary mint + `supersede`) ────────────

/// Walk `value_id`'s `instance_of` relations and return the kind
/// label's surface form. Returns `None` if `value_id` has no
/// `instance_of` relation in the subject slot.
///
/// "Subject slot" means `[subject: value_id, instance_of: kind]`.
/// Relations where `value_id` appears in other slots (e.g. as
/// `from` or `to`) are skipped — we want the typing of `value_id`,
/// not relations that mention it.
pub(crate) fn kind_of(hypergraph: &Hypergraph, value_id: ElementId) -> Option<String> {
    let instance_of_attr = hypergraph.by_name.get("instance_of")?.first().copied()?;
    let candidates = hypergraph.relations_by_element.get(&value_id)?;
    for &rid in candidates {
        let r = &hypergraph.relations[rid.0 as usize];
        let mut is_subject = false;
        let mut kind_value: Option<ElementId> = None;
        for attr in &r.attributes {
            if attr.name == hypergraph.subject_attr {
                if let Term::Element(e) = attr.value
                    && e == value_id
                {
                    is_subject = true;
                }
            } else if attr.name == instance_of_attr
                && let Term::Element(e) = attr.value
            {
                kind_value = Some(e);
            }
        }
        if is_subject && let Some(kid) = kind_value {
            // invariant: kid is the Element value of an `instance_of` attribute
            // on a live relation — minted before the relation, so it exists.
            return hypergraph.elements[kid.0 as usize].names.first().cloned();
        }
    }
    None
}

/// Derive a coarse property-kind label from the `from` and `to`
/// value types of a state-change event. `build_relations` reads this at n-ary
/// mint time and adds the result as a `property` attribute slot on
/// the event relation. `supersede` reads it back off the event without
/// re-inferring.
///
/// Match table:
///
/// - both weekday or month (any combination) → `"date"`
/// - both time → `"time"`
/// - both quantity → `"amount"`
/// - both place → `"location"`
/// - both role → `"role"`
/// - if `to`'s kind alone is one of the above → that label
///   (handles change-verb singletons where `from` is the synthetic
///   `unknown_prior` placeholder — Templates 5 and 6 of
///   `relation_patterns.rs`)
/// - else → `"value"` (generic fallback)
pub fn infer_property_kind(hypergraph: &Hypergraph, from_id: ElementId, to_id: ElementId) -> &'static str {
    let f = kind_of(hypergraph, from_id);
    let t = kind_of(hypergraph, to_id);
    let (f, t) = (f.as_deref(), t.as_deref());
    if let Some(to_bucket) = bucket_of(t) {
        // `to` carries a typed kind — that determines the property
        // bucket regardless of `from`. Common case for both T1
        // ("Bob moved from Boston to Austin") and T5/T6 ("Bob moved
        // to Austin"; "Bob is now a staff engineer") where the
        // `from` may be a synthetic placeholder.
        //
        // Cross-bucket pair (e.g., weekday → quantity) is
        // suspicious — fall back to generic so it doesn't conflict
        // with a clean single-type cache. Same-bucket pairs
        // (month → weekday both → "date") still collapse cleanly.
        return match (bucket_of(f), Some(to_bucket)) {
            (Some(fb), Some(tb)) if fb != tb => "value",
            _ => to_bucket,
        };
    }
    "value"
}

/// Map a raw NER kind (or coarse type label) to its property-bucket
/// name. `None` for unrecognized kinds — caller falls back to
/// `"value"`.
fn bucket_of(kind: Option<&str>) -> Option<&'static str> {
    match kind? {
        "weekday" | "month" | "year" => Some("date"),
        "time" => Some("time"),
        "quantity" => Some("amount"),
        "place" => Some("location"),
        "role" => Some("role"),
        "language" | "software" => Some("tech"),
        _ => None,
    }
}

// ─── §6.3 — N-ary event merging ───────────────────────────────────────

/// One group of pattern proposals that collapse into a single n-ary
/// event relation. Holds the indices (into `known.relations`) of the
/// `from` and `to` proposals plus the surface verb that anchored them.
#[derive(Debug, Clone)]
struct EventMergeGroup {
    /// Surface verb (`"changed"`, `"rescheduled"`, …). Used to mint
    /// the event element's name and to gate the `intervened` meta.
    anchor: String,
    /// Index of the `from` proposal in `known.relations`.
    from_idx: usize,
    /// Index of the `to` proposal in `known.relations`.
    to_idx: usize,
}

/// Group verb-anchored pattern proposals by `(subject_span, anchor)`.
/// A group is emitted iff both a `from` and a `to` proposal exist for
/// the pair (one-sided proposals stay binary — they're not a
/// state-change frame). The first matching pair wins; pattern RE
/// emits at most one `from` and one `to` per `(subject, verb)` triple
/// over a sliding window so collisions in practice are vanishingly
/// rare.
fn compute_event_merge_groups(relations: &[RelationCandidate]) -> Vec<EventMergeGroup> {
    use std::collections::HashMap;
    type Key = ((usize, usize), String);
    let mut from_idx: HashMap<Key, usize> = HashMap::new();
    let mut to_idx: HashMap<Key, usize> = HashMap::new();
    for (i, p) in relations.iter().enumerate() {
        let Some(anchor) = p.event_anchor.as_ref() else {
            continue;
        };
        let key = ((p.subject_char_start, p.subject_char_end), anchor.clone());
        match p.attribute_name.as_str() {
            "from" => {
                from_idx.entry(key).or_insert(i);
            }
            "to" => {
                to_idx.entry(key).or_insert(i);
            }
            _ => {}
        }
    }
    let mut groups: Vec<EventMergeGroup> = Vec::new();
    for (key, fi) in from_idx {
        if let Some(&ti) = to_idx.get(&key) {
            groups.push(EventMergeGroup {
                anchor: key.1,
                from_idx: fi,
                to_idx: ti,
            });
        }
    }
    // Stable order so test assertions don't flake on HashMap iteration.
    groups.sort_by_key(|g| g.from_idx);
    groups
}

/// Mint the event element + the n-ary relation + the typing relation
/// and the `intervened` meta-relation when applicable. Returns the
/// n-ary relation's `RelationId` so the caller can attach a source
/// meta-relation to it.
#[allow(clippy::too_many_arguments)]
fn mint_event_relations(
    hypergraph: &mut Hypergraph,
    input_text: &str,
    seq_ctx: SequenceContext,
    relations: &[RelationCandidate],
    g: &EventMergeGroup,
    seq: usize,
    policy: &crate::types::Policy,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut MintedRelations,
) -> Option<RelationId> {
    let from_p = &relations[g.from_idx];
    let to_p = &relations[g.to_idx];

    // Resolve participant elements via the same span cache used by
    // the binary path — keeps duplicate-mention dedup consistent.
    let subj_text = &input_text[from_p.subject_char_start..from_p.subject_char_end];
    let subj_id = resolve_span(
        hypergraph,
        seq_ctx,
        from_p.subject_char_start,
        from_p.subject_char_end,
        subj_text,
        span_cache,
        result,
    );
    let from_val_id = resolve_object(hypergraph, input_text, seq_ctx, &from_p.object, span_cache, result);
    let to_val_id = resolve_object(hypergraph, input_text, seq_ctx, &to_p.object, span_cache, result);

    // Event element — a fresh identity per merge. Name carries the
    // verb + tick + seq so it stays human-readable in dumps.
    let event_name = format!("{}_event_{}_{}", g.anchor, hypergraph.clock.0, seq);
    let event_emb = embed_text(&event_name);
    let default_conf = hypergraph.policy.default_conf;
    let event_id = mint_element(
        hypergraph,
        vec![event_name],
        event_emb,
        Polarity::Signal,
        default_conf,
    );
    result.minted_elements.push(event_id);

    // Typing relation — `(event, instance_of, <verb-kind>)`. Verb kind
    // resolves via lookup table; falls back to the verb itself if no
    // canonical kind is seeded.
    let kind_label = event_kind_for(&g.anchor);
    let kind_id = resolve_label_element(hypergraph, kind_label, result);
    let instance_of_attr = resolve_attribute_name(hypergraph, seq_ctx, "instance_of", None, policy, result);
    let typing_id = mint_relation(
        hypergraph,
        vec![
            Attribute {
                name: hypergraph.subject_attr,
                value: Term::Element(event_id),
            },
            Attribute {
                name: instance_of_attr,
                value: Term::Element(kind_id),
            },
        ],
        RelationStatus::Entailed,
        1.0,
    );
    result.minted_relations.push(typing_id);

    // N-ary relation —
    // `[subject: event, target: subj, property: kind, from: …, to: …]`.
    // The `property` slot carries the coarse value-type inference so
    // `supersede` (supersession) can identify the cache bucket without
    // re-walking value Elements' `instance_of` relations. Falls back
    // to the generic "value" kind when the values aren't typed.
    let target_attr = hypergraph.target_attr;
    let from_attr = resolve_attribute_name(hypergraph, seq_ctx, "from", None, policy, result);
    let to_attr = resolve_attribute_name(hypergraph, seq_ctx, "to", None, policy, result);
    let property_attr = resolve_attribute_name(hypergraph, seq_ctx, "property", None, policy, result);
    let property_kind_label = infer_property_kind(hypergraph, from_val_id, to_val_id);
    let property_kind_id = resolve_label_element(hypergraph, property_kind_label, result);
    let nary_conf = confidence_for(from_p.confidence.min(to_p.confidence), policy);
    let nary_status = if from_p.confidence.min(to_p.confidence) >= policy.ner_assertion_threshold {
        RelationStatus::Asserted
    } else {
        RelationStatus::Defeasible
    };
    let nary_id = mint_relation(
        hypergraph,
        vec![
            Attribute {
                name: hypergraph.subject_attr,
                value: Term::Element(event_id),
            },
            Attribute {
                name: target_attr,
                value: Term::Element(subj_id),
            },
            Attribute {
                name: property_attr,
                value: Term::Element(property_kind_id),
            },
            Attribute {
                name: from_attr,
                value: Term::Element(from_val_id),
            },
            Attribute {
                name: to_attr,
                value: Term::Element(to_val_id),
            },
        ],
        nary_status,
        nary_conf,
    );
    result.minted_relations.push(nary_id);

    // `intervened` meta-relation — fires when the surface verb is in
    // the agent-action lexicon (§11.7's causal-shape conventions). The
    // value points at an Element capturing the verb itself, so future
    // ticks recognizing the same verb reuse the element.
    if is_intervention_verb(&g.anchor) {
        let intervened_attr = resolve_attribute_name(hypergraph, seq_ctx, "intervened", None, policy, result);
        let verb_id = resolve_label_element(hypergraph, &g.anchor, result);
        let meta_id = mint_relation(
            hypergraph,
            vec![
                Attribute {
                    name: target_attr,
                    value: Term::Relation(nary_id),
                },
                Attribute {
                    name: intervened_attr,
                    value: Term::Element(verb_id),
                },
            ],
            RelationStatus::Entailed,
            1.0,
        );
        result.minted_relations.push(meta_id);
    }

    Some(nary_id)
}

/// Canonical event-kind label for a surface verb. Small lookup table
/// covering the seed-pack frames; on miss, returns the verb itself
/// (replay can collapse synonyms once it has enough mentions).
///
/// Pattern RE's anchor extractor sometimes captures more than the
/// verb proper ("rescheduled the meeting"), so look up keyed on the
/// LAST whitespace-delimited token, which carries the verb head in
/// English. Multi-token verb phrases (e.g. "moved to") are rare
/// enough at the pattern level that the last-token heuristic is fine.
fn event_kind_for(verb: &str) -> &str {
    let head = verb_head(verb);
    match head {
        "changed" | "change" | "changes" | "changing" => "change_event",
        "rescheduled" | "reschedule" | "reschedules" | "rescheduling" => "reschedule_event",
        "moved" | "move" | "moves" | "moving" => "move_event",
        "shifted" | "shift" | "shifts" | "shifting" => "shift_event",
        // Fallback: use the head verb as the kind label. Caller's
        // resolve_label_element will mint it on miss.
        _ => head,
    }
}

/// Extract the verb head from a pattern-RE anchor — the first
/// whitespace-delimited token, with trailing ASCII punctuation
/// stripped. Pattern RE's anchor is everything between the subject
/// span and the " from " connective, so for "Sarah rescheduled the
/// meeting" the captured anchor is "rescheduled the meeting" and
/// the verb sits at the start. Multi-token verb phrases like "has
/// rescheduled" are rare in this position because the 32-char
/// connective cap excludes most auxiliaries.
fn verb_head(anchor: &str) -> &str {
    anchor
        .split_whitespace()
        .next()
        .unwrap_or(anchor)
        .trim_end_matches(|c: char| c.is_ascii_punctuation())
}

/// Returns `true` iff the verb's lowercase head form is in the
/// agent-action lexicon (§11.7). Reads via `verb_head` so anchor
/// noise like "rescheduled the meeting" still resolves to the head.
fn is_intervention_verb(verb: &str) -> bool {
    const LEXICON: &[&str] = &[
        "reschedule",
        "rescheduled",
        "reschedules",
        "rescheduling",
        "move",
        "moved",
        "moves",
        "moving",
        "set",
        "sets",
        "setting",
        "configure",
        "configured",
        "configures",
        "configuring",
        "decide",
        "decided",
        "decides",
        "deciding",
        "cancel",
        "cancels",
        "cancelling",
        "canceling",
        "cancelled",
        "canceled",
        "ship",
        "ships",
        "shipping",
        "shipped",
        "revert",
        "reverts",
        "reverting",
        "reverted",
        "merge",
        "merges",
        "merging",
        "merged",
        "deploy",
        "deploys",
        "deploying",
        "deployed",
        "delete",
        "deletes",
        "deleting",
        "deleted",
    ];
    let lower = verb_head(verb).to_ascii_lowercase();
    LEXICON.iter().any(|&v| v == lower)
}

// ─── §4a — Coref override ─────────────────────────────────────────────

/// Apply coref decisions by binding each pronoun's character range
/// to its antecedent element via the span cache. Folds the pronoun's
/// contextualized vector into the antecedent (coref ≠ no-op for the
/// embedding — the new mention's context legitimately updates the
/// element's centroid). Bumps `access_count` and `last_seen` once
/// per decision.
///
/// Antecedents that don't resolve in `by_name` are silently skipped —
/// the resolver falls back to the normal mint path, which is
/// defensive against incomplete coref output.
fn apply_coref_decisions(
    hypergraph: &mut Hypergraph,
    seq_ctx: SequenceContext,
    decisions: &[CorefDecision],
    span_cache: &mut HashMap<(usize, usize), ElementId>,
) {
    for d in decisions {
        let antecedent_id = match hypergraph.by_name.get(&d.antecedent_text) {
            Some(ids) => match ids.first() {
                Some(&id) => id,
                None => continue,
            },
            None => continue,
        };
        if let Some(obs) = seq_ctx.embed_span(d.pronoun_char_start, d.pronoun_char_end) {
            let prev_n = hypergraph.elements[antecedent_id.0 as usize].stats.access_count;
            fold_streaming_centroid(
                &mut hypergraph.elements[antecedent_id.0 as usize].embedding,
                &obs,
                prev_n,
            );
        }
        let el = &mut hypergraph.elements[antecedent_id.0 as usize];
        el.stats.access_count = el.stats.access_count.saturating_add(1);
        el.stats.last_seen = hypergraph.clock;
        span_cache.insert((d.pronoun_char_start, d.pronoun_char_end), antecedent_id);
    }
}

// ─── §6.4 — Novelty branch ────────────────────────────────────────────

/// Mint Elements for every novelty chunk that doesn't already have
/// one. Span cache dedups against the known branch's mints so we
/// don't get two elements for the same character range.
fn mint_novelty_chunks(
    hypergraph: &mut Hypergraph,
    seq_ctx: SequenceContext,
    chunks: &[OrthographicChunk],
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut MintedRelations,
) {
    for c in chunks {
        let _id = resolve_span(
            hypergraph,
            seq_ctx,
            c.char_start,
            c.char_end,
            &c.text,
            span_cache,
            result,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;
    use crate::tick_pipeline::run_extractors::run_extractors;
    use crate::types::Policy;

    fn run_build(text: &str) -> (Hypergraph, MintedRelations) {
        run_build_labeled(text, &[])
    }

    fn run_build_labeled(text: &str, labels: &[&str]) -> (Hypergraph, MintedRelations) {
        let mut hypergraph = load_seed_graph();
        let policy = Policy::default();
        let out = run_extractors(text, labels, &policy, &hypergraph, &[]);
        let built_relations = build_relations(text, &mut hypergraph, &out, &policy, None);
        (hypergraph, built_relations)
    }

    #[test]
    fn typed_relation_lookup_maps_common_verbs() {
        assert_eq!(typed_relation_lookup("uses"), Some("with"));
        assert_eq!(typed_relation_lookup("Using"), Some("with"));
        assert_eq!(typed_relation_lookup("lives in"), Some("at"));
        assert_eq!(typed_relation_lookup("works at"), Some("at"));
        assert_eq!(typed_relation_lookup("is a"), Some("instance_of"));
        assert_eq!(typed_relation_lookup("nonsense verb"), None);
    }

    #[test]
    fn resolve_attribute_name_dedups_via_typed_lexicon() {
        // "uses" should resolve to the seeded `with` attribute element,
        // not mint a new "uses" element.
        let mut hypergraph = load_seed_graph();
        let policy = Policy::default();
        let mut result = MintedRelations::default();
        let with_id = hypergraph.by_name["with"].iter()
            .copied()
            .find(|&id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
            .expect("seed should have a Signal-polarity `with`");
        // `None` context never reads the sequence, so an empty one suffices.
        let seq_ctx = SequenceContext { sequence: &[], offsets: &[] };
        let resolved =
            resolve_attribute_name(&mut hypergraph, seq_ctx, "uses", None, &policy, &mut result);
        assert_eq!(resolved, with_id, "`uses` should typed-lexicon-dedup to seed `with`");
        assert_eq!(result.attr_names_minted, 0, "lexicon hit should not mint");
    }

    #[test]
    fn mints_element_for_unseen_span() {
        let (hypergraph, out) = run_build("Nick lived in Brantford for 3 years.");
        // Nick + Brantford + 3 years (NER) — plus the type-label
        // elements (person, place, time) if not already seeded as
        // attribute names. `person`/`place`/`time` are NOT seeded as
        // Element names today, so they get minted too.
        assert!(
            out.minted_elements.len() >= 3,
            "expected ≥3 mints, got {}: {:?}",
            out.minted_elements.len(),
            out.minted_elements
                .iter()
                .map(|id| &hypergraph.elements[id.0 as usize].names[0])
                .collect::<Vec<_>>(),
        );
        let names: Vec<&str> = out
            .minted_elements
            .iter()
            .map(|id| hypergraph.elements[id.0 as usize].names[0].as_str())
            .collect();
        assert!(names.contains(&"Nick"));
        assert!(names.contains(&"Brantford"));
    }

    #[test]
    fn instance_of_relation_uses_seeded_attribute_name() {
        let (hypergraph, out) = run_build("Sarah called me yesterday.");
        // At least one instance_of relation should have been minted.
        let instance_of_attr = hypergraph.by_name["instance_of"][0];
        let has_io = out.minted_relations.iter().any(|&rid| {
            let r = &hypergraph.relations[rid.0 as usize];
            r.attributes.iter().any(|a| a.name == instance_of_attr)
        });
        assert!(has_io, "expected at least one instance_of relation");
    }

    #[test]
    fn indices_updated_after_mint() {
        let (hypergraph, out) = run_build("Nick lived in Brantford for 3 years.");
        // Every minted relation should appear in
        // relations_by_attribute_name for every attribute it has.
        for &rid in &out.minted_relations {
            let r = &hypergraph.relations[rid.0 as usize];
            for attr in &r.attributes {
                let bucket = hypergraph
                    .relations_by_attribute_name
                    .get(&attr.name)
                    .expect("attribute name must be indexed");
                assert!(bucket.contains(&rid));
            }
        }
    }

    #[test]
    fn span_cache_prevents_double_mint() {
        // "Nick" appears once but both NER (person) and pattern RE
        // (subject of `at`) reference the same char range. `build_relations`
        // should hit the cache the second time.
        let (hypergraph, out) = run_build("Nick lived in Brantford for 3 years.");
        let nick_count = out
            .minted_elements
            .iter()
            .filter(|id| hypergraph.elements[id.0 as usize].names.iter().any(|n| n == "Nick"))
            .count();
        assert_eq!(nick_count, 1, "expected exactly one Nick mint");
    }

    #[test]
    fn signal_attr_name_wins_over_void_for_with() {
        // ATTR_WITH is Signal, VOID_ADP_WITH is Void. Resolver should
        // pick the Signal one for the relation's attribute slot.
        // Pattern RE needs two NER-tagged spans flanking "with" to
        // fire — this sentence supplies both ("appointment" → event,
        // "Dr. Rao" → person). The explicit label set matches what
        // active-region warming would do in a fully wired tick.
        let (hypergraph, out) = run_build_labeled(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
        );
        let with_attr_id = hypergraph.by_name["with"]
            .iter()
            .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .expect("Signal `with` attribute must be seeded");

        let any_uses_signal = out.minted_relations.iter().any(|&rid| {
            let r = &hypergraph.relations[rid.0 as usize];
            r.attributes.iter().any(|a| a.name == with_attr_id)
        });
        assert!(
            any_uses_signal,
            "`build_relations` should bind `with` to the Signal attribute name",
        );

        // Make sure no relation accidentally bound to the Void `with`.
        let void_with_id = hypergraph.by_name["with"]
            .iter()
            .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Void)
            .copied()
            .expect("Void `with` adposition must be seeded");
        let any_uses_void = out.minted_relations.iter().any(|&rid| {
            let r = &hypergraph.relations[rid.0 as usize];
            r.attributes.iter().any(|a| a.name == void_with_id)
        });
        assert!(
            !any_uses_void,
            "no relation should bind to the Void `with` adposition",
        );
    }

    #[test]
    fn from_to_pair_collapses_into_one_nary_event() {
        // "appointment changed from Tuesday to Friday" — pattern RE
        // emits (subj, from, Tuesday) and (subj, to, Friday) with
        // event_anchor=Some("changed"); `build_relations` merges them into one
        // n-ary relation plus a typing relation. The original two
        // binary from/to relations should NOT exist independently.
        let (hypergraph, out) = run_build_labeled(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
        );
        let from_attr = hypergraph.by_name["from"]
            .iter()
            .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .expect("Signal `from` attribute must be seeded");
        let to_attr = hypergraph.by_name["to"]
            .iter()
            .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .expect("Signal `to` attribute must be seeded");

        // Find n-ary relations: contain both `from` and `to`.
        // Pattern RE can emit multiple overlapping from/to triples
        // when several NER spans flank the verb; each triggers a
        // merge. We assert the *shape* of every n-ary, not the count
        // (pattern recall/precision is a separate concern).
        let nary: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                let has_from = r.attributes.iter().any(|a| a.name == from_attr);
                let has_to = r.attributes.iter().any(|a| a.name == to_attr);
                has_from && has_to
            })
            .collect();
        assert!(!nary.is_empty(), "expected at least one n-ary event");
        for &rid in &nary {
            let r = &hypergraph.relations[rid.0 as usize];
            assert_eq!(
                r.attributes.len(),
                5,
                "n-ary should have [subject, target, property, from, to]; got {:?}",
                r.attributes,
            );
        }

        // No standalone binary `from` relation should remain — every
        // verb-anchored from/to pair was consumed by a merge.
        let standalone_from: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                r.attributes.len() == 2 && r.attributes.iter().any(|a| a.name == from_attr)
            })
            .collect();
        assert!(
            standalone_from.is_empty(),
            "binary from relation should be subsumed by the merge",
        );
    }

    #[test]
    fn nary_event_carries_property_slot() {
        // `build_relations`'s n-ary merge should now stamp a `property` slot on
        // every event so `supersede` doesn't have to re-infer. Both values
        // are weekdays → property kind = "date".
        let (hypergraph, out) = run_build_labeled(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
        );
        let property_attr = hypergraph.by_name["property"][0];
        let date_id = hypergraph
            .by_name
            .get("date")
            .and_then(|v| v.first().copied())
            .expect("`build_relations` should mint the `date` kind element");

        // Find an n-ary event and verify the property slot binds to `date`.
        let nary_with_date = out.minted_relations.iter().any(|&rid| {
            let r = &hypergraph.relations[rid.0 as usize];
            r.attributes.iter().any(|a| {
                a.name == property_attr && matches!(a.value, Term::Element(e) if e == date_id)
            })
        });
        assert!(
            nary_with_date,
            "n-ary event should carry a property slot bound to `date`",
        );
    }

    #[test]
    fn flight_from_to_stays_binary_no_verb_anchor() {
        // "Flight from JFK to LAX" — no verb between subject and
        // "from", so event_anchor is None and the merge pass doesn't
        // fire. Should produce two standalone binary relations.
        let (hypergraph, out) = run_build_labeled("Flight from JFK to LAX.", &["place", "event"]);
        let from_attr = hypergraph.by_name["from"]
            .iter()
            .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .expect("Signal `from` attribute must be seeded");

        let nary: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| hypergraph.relations[rid.0 as usize].attributes.len() >= 4)
            .collect();
        assert!(
            nary.is_empty(),
            "no n-ary event should fire without a verb anchor",
        );

        let binary_from: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                r.attributes.len() == 2 && r.attributes.iter().any(|a| a.name == from_attr)
            })
            .collect();
        assert_eq!(binary_from.len(), 1, "expected one standalone binary from");
    }

    #[test]
    fn intervention_verb_emits_intervened_meta() {
        // `rescheduled` is in the intervention lexicon. Test should
        // fire even though our pack doesn't seed change_event etc.;
        // the merge mints those on the fly.
        let (hypergraph, out) = run_build_labeled(
            "The meeting rescheduled from Tuesday to Friday.",
            &["event", "weekday"],
        );
        let intervened_attr = hypergraph
            .by_name
            .get("intervened")
            .and_then(|v| v.first().copied())
            .expect("`intervened` attribute must be seeded");
        let any_intervened = out.minted_relations.iter().any(|&rid| {
            hypergraph.relations[rid.0 as usize]
                .attributes
                .iter()
                .any(|a| a.name == intervened_attr)
        });
        assert!(
            any_intervened,
            "intervened meta-relation should fire for `rescheduled`",
        );
    }

    #[test]
    fn non_intervention_verb_skips_intervened_meta() {
        // `changed` is NOT in the lexicon — observation, not action.
        let (hypergraph, out) = run_build_labeled(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
        );
        let intervened_attr = hypergraph
            .by_name
            .get("intervened")
            .and_then(|v| v.first().copied())
            .expect("`intervened` attribute must be seeded");
        let any_intervened = out.minted_relations.iter().any(|&rid| {
            hypergraph.relations[rid.0 as usize]
                .attributes
                .iter()
                .any(|a| a.name == intervened_attr)
        });
        assert!(
            !any_intervened,
            "intervened should not fire for observed `changed`",
        );
    }

    #[test]
    fn novelty_chunks_mint_elements_for_unlabeled_tokens() {
        // "Nick lived in Brantford" — `lived` is a content token NER
        // doesn't tag (no verb labels in SEED_KINDS), but the novelty
        // chunker emits it. `build_relations` should mint an Element for it.
        let (hypergraph, out) = run_build("Nick lived in Brantford for 3 years.");
        let names: Vec<&str> = out
            .minted_elements
            .iter()
            .map(|id| hypergraph.elements[id.0 as usize].names[0].as_str())
            .collect();
        assert!(
            names.contains(&"lived"),
            "novelty chunker should mint `lived`; got {names:?}",
        );
    }

    #[test]
    fn novelty_relation_lands_as_defeasible() {
        let (hypergraph, out) = run_build("Nick lived in Brantford for 3 years.");
        // Find a Defeasible relation whose subject is "Nick".
        let nick_id = hypergraph
            .by_name
            .get("Nick")
            .and_then(|v| v.first().copied())
            .expect("Nick should have been minted");
        let nick_defeasible: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                r.status == RelationStatus::Defeasible
                    && r.attributes.iter().any(|a| {
                        matches!(
                            a.value,
                            Term::Element(e) if e == nick_id
                        )
                    })
            })
            .collect();
        assert!(
            !nick_defeasible.is_empty(),
            "expected at least one Defeasible Nick relation from novelty",
        );
    }

    #[test]
    fn coref_override_binds_pronoun_to_antecedent() {
        // Build a graph that already contains a Sarah element, then
        // feed a coref decision for "she" → Sarah and verify
        // apply_coref_decisions pins "she"'s char range to Sarah's id.
        let mut hypergraph = load_seed_graph();
        // Mint Sarah manually so we have a target to bind to.
        let sarah_id = mint_element(
            &mut hypergraph,
            vec!["Sarah".to_string()],
            embed_text("Sarah"),
            Polarity::Signal,
            1.0,
        );
        let text = "She arrived.";
        let decisions = vec![CorefDecision {
            pronoun_text: "She".to_string(),
            pronoun_char_start: 0,
            pronoun_char_end: 3,
            antecedent_text: "Sarah".to_string(),
            confidence: 0.9,
        }];
        let mut cache: HashMap<(usize, usize), ElementId> = HashMap::new();
        let prior_access = hypergraph.elements[sarah_id.0 as usize].stats.access_count;
        let (sequence, offsets) = embed_sequence_with_offsets(text);
        let seq_ctx = SequenceContext { sequence: &sequence, offsets: &offsets };
        apply_coref_decisions(&mut hypergraph, seq_ctx, &decisions, &mut cache);

        assert_eq!(
            cache.get(&(0, 3)).copied(),
            Some(sarah_id),
            "coref should pin the pronoun range to Sarah's id",
        );
        assert_eq!(
            hypergraph.elements[sarah_id.0 as usize].stats.access_count,
            prior_access + 1,
            "antecedent's access_count must bump on coref bind",
        );
    }

    #[test]
    fn coref_override_skips_unknown_antecedent() {
        // If the antecedent isn't in by_name, the override is a no-op.
        let mut hypergraph = load_seed_graph();
        let text = "She arrived.";
        let decisions = vec![CorefDecision {
            pronoun_text: "She".to_string(),
            pronoun_char_start: 0,
            pronoun_char_end: 3,
            antecedent_text: "SomeoneUnseeded".to_string(),
            confidence: 0.9,
        }];
        let mut cache: HashMap<(usize, usize), ElementId> = HashMap::new();
        let (sequence, offsets) = embed_sequence_with_offsets(text);
        let seq_ctx = SequenceContext { sequence: &sequence, offsets: &offsets };
        apply_coref_decisions(&mut hypergraph, seq_ctx, &decisions, &mut cache);
        assert!(
            cache.is_empty(),
            "no binding should be created for unknown antecedent",
        );
    }

    /// End-to-end integration test for the §11.9 worked example.
    /// Validates that `run_extractors` → `build_relations` produces the expected substrate
    /// state: minted entities, event element, instance_of relations,
    /// n-ary event relation, binary `with` relation, intervened
    /// presence/absence, and index population.
    #[test]
    fn dentist_sentence_integration() {
        let text = "My dentist appointment with Dr. Rao changed from Tuesday to Friday.";
        let (hypergraph, out) = run_build_labeled(text, &["person", "event", "weekday", "role"]);

        // ── Entities — NER tags spans (events/persons/weekdays).
        // "My dentist appointment" lands as one event-typed span per
        // Q1 (whole-span minting); the test asserts presence by name.
        let minted_names: Vec<&str> = out
            .minted_elements
            .iter()
            .map(|id| hypergraph.elements[id.0 as usize].names[0].as_str())
            .collect();
        for must_be_minted in &["Dr. Rao", "Tuesday", "Friday"] {
            assert!(
                minted_names.contains(must_be_minted),
                "expected `{must_be_minted}` to land; got {minted_names:?}",
            );
        }
        // The compound event span — NER's exact text varies slightly
        // ("My dentist appointment" vs "dentist appointment") so we
        // accept any minted name containing "appointment".
        assert!(
            minted_names.iter().any(|n| n.contains("appointment")),
            "expected an appointment-style span minted; got {minted_names:?}",
        );

        // ── Event element — at least one "<verb>_event_<tick>_<seq>".
        let event_names: Vec<&str> = minted_names
            .iter()
            .copied()
            .filter(|n| n.contains("_event_"))
            .collect();
        assert!(
            !event_names.is_empty(),
            "n-ary merge should mint at least one event element; got {minted_names:?}",
        );

        // ── N-ary event relation — has [subject(event), target, from, to].
        let from_attr = hypergraph.by_name["from"]
            .iter()
            .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .unwrap();
        let to_attr = hypergraph.by_name["to"]
            .iter()
            .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .unwrap();
        let nary: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                r.attributes.iter().any(|a| a.name == from_attr)
                    && r.attributes.iter().any(|a| a.name == to_attr)
                    && r.attributes.len() == 5
            })
            .collect();
        assert!(
            !nary.is_empty(),
            "expected at least one n-ary event relation",
        );

        // ── Binary `with` relation — appointment with Dr. Rao.
        let with_attr = hypergraph.by_name["with"]
            .iter()
            .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .unwrap();
        let with_rels: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                r.attributes.len() == 2 && r.attributes.iter().any(|a| a.name == with_attr)
            })
            .collect();
        assert!(
            !with_rels.is_empty(),
            "expected a binary `with` relation tying appointment to Dr. Rao",
        );

        // ── No `intervened` meta-relation — `changed` is observation,
        //    not agent action (lexicon excludes it).
        let intervened_attr = hypergraph.by_name["intervened"][0];
        let intervened_metas: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                hypergraph.relations[rid.0 as usize]
                    .attributes
                    .iter()
                    .any(|a| a.name == intervened_attr)
            })
            .collect();
        assert!(
            intervened_metas.is_empty(),
            "`changed` ∉ intervention lexicon → no intervened meta expected",
        );

        // ── Indices populated — every minted base relation is
        //    indexed under every Element-valued attribute it has.
        for &rid in &out.minted_relations {
            let r = &hypergraph.relations[rid.0 as usize];
            for attr in &r.attributes {
                if let Term::Element(e) = attr.value {
                    let bucket = hypergraph
                        .relations_by_element
                        .get(&e)
                        .unwrap_or_else(|| panic!("element {e:?} missing from index"));
                    assert!(
                        bucket.contains(&rid),
                        "relation {rid:?} missing from relations_by_element[{e:?}]",
                    );
                }
            }
        }
    }

    #[test]
    fn source_meta_relation_emitted_when_source_some() {
        let mut hypergraph = load_seed_graph();
        let policy = Policy::default();
        let text = "Sarah called me yesterday.";
        let out = run_extractors(text, &[], &policy, &hypergraph, &[]);

        // Pick an arbitrary source element — any seeded one is fine.
        let user_id = hypergraph.by_name["user"][0];
        let prior_rel_count = hypergraph.relations.len();
        let result = build_relations(text, &mut hypergraph, &out, &policy, Some(user_id));

        let source_attr = hypergraph.by_name["source"][0];
        let source_metas: Vec<RelationId> = (prior_rel_count..hypergraph.relations.len())
            .map(|i| RelationId(i as u32))
            .filter(|rid| {
                hypergraph.relations[rid.0 as usize]
                    .attributes
                    .iter()
                    .any(|a| a.name == source_attr)
            })
            .collect();
        assert!(
            !source_metas.is_empty(),
            "expected source meta-relations when source = Some",
        );
        // Every base relation should have one meta companion.
        let base_count = result.minted_relations.len() - source_metas.len();
        assert_eq!(
            source_metas.len(),
            base_count,
            "one source meta per base relation",
        );
    }
}
