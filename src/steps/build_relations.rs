//! Step 8 — build Relations + Events.
//!
//! Pure HashMap inserts + index updates over what Step 5 already
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
//! 6. Incrementally update all seven Step 8 indices.
//!
//! See `step_8_design.md` for the full spec; this is Phase 2 (binary
//! relations only). Phase 3 adds n-ary event merging.

use std::collections::HashMap;

use crate::embed::{embed_span_in_context, embed_text, fold_streaming_centroid};
use crate::steps::coref::CorefDecision;
use crate::steps::novelty_relations::NoveltyRelation;
use crate::steps::orthographic::OrthographicChunk;
use crate::steps::relation_patterns::RelationProposal;
use crate::steps::run_extractors::ExtractionOutput;
use crate::types::{
    Attribute, Element, ElementId, Hypergraph, MemoryStats, Polarity, Relation, RelationId,
    RelationStatus, Term,
};

/// Surface of Step 8's work in a single tick — what got minted and
/// how many new attribute names appeared. Frame's `durable_writes`
/// reads `minted_elements`; observability reads `attr_names_minted`.
#[derive(Debug, Default, Clone)]
pub struct Step8Output {
    pub minted_elements: Vec<ElementId>,
    pub minted_relations: Vec<RelationId>,
    /// New attribute-name elements minted this tick. Threshold
    /// signalling lives at the caller — Step 8 just counts.
    pub attr_names_minted: u32,
}

/// Build relations + events for one tick.
///
/// - `input_text` — the raw input. Spans are character offsets into this.
/// - `hg` — mutated in place; new Elements/Relations appended,
///   all seven Step 8 indices updated incrementally.
/// - `out` — the `ExtractionOutput` from Step 5.
/// - `policy` — adjusted Policy from Step 2 (Steps 4–12 read this).
/// - `source` — the tick's source (§11.1). If `Some`, every minted
///   base relation gets a companion `(target: R, source: source)`.
///
/// The current implementation handles binary `instance_of` and
/// pattern-RE proposals. N-ary event merging lands in Phase 3;
/// novelty branch mints land in Phase 4.
pub fn build_relations(
    input_text: &str,
    hg: &mut Hypergraph,
    out: &ExtractionOutput,
    policy: &crate::types::Policy,
    source: Option<ElementId>,
) -> Step8Output {
    let mut result = Step8Output::default();

    // Char-span → ElementId cache for this tick. Two proposals over
    // the same span resolve to the same element without re-running
    // mint/dedup and without bumping access_count twice for one mention.
    let mut span_cache: HashMap<(usize, usize), ElementId> = HashMap::new();

    // ── §4a — Coref override ───────────────────────────────────────
    // Pre-populate the span cache from coref decisions BEFORE any
    // proposal-driven resolution runs. Each decision folds the
    // pronoun's contextualized vector into the antecedent and pins
    // the pronoun's char range to the antecedent's id. Downstream
    // `resolve_span` calls for that range short-circuit on the cache.
    // No-op behaviorally today (Step 5e is a stub), but the wiring
    // lands so once `recent_focus` lights up the override is live.
    apply_coref_decisions(hg, input_text, &out.known.coref, &mut span_cache);

    // ── §6.1 — Binary instance_of proposals (NER + Temporal) ────────
    let mut base_rel_ids: Vec<RelationId> = Vec::new();
    for p in &out.known.instance_of {
        let subj_id = resolve_span(
            hg,
            input_text,
            p.subject_char_start,
            p.subject_char_end,
            &p.subject_text,
            &mut span_cache,
            &mut result,
        );
        let label_id = resolve_label_element(hg, &p.object_label, &mut result);
        let attr_id = resolve_attribute_name(hg, p.attribute_name, &mut result);
        let rel_id = mint_relation(
            hg,
            vec![
                Attribute {
                    name: hg.subject_attr,
                    value: Term::Element(subj_id),
                },
                Attribute {
                    name: attr_id,
                    value: Term::Element(label_id),
                },
            ],
            p.status,
            confidence_for(p.confidence, policy),
        );
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
        let rel_id = mint_pattern_relation(hg, input_text, p, policy, &mut span_cache, &mut result);
        base_rel_ids.push(rel_id);
        result.minted_relations.push(rel_id);
    }

    // ── §6.3 — Emit n-ary event relations ───────────────────────────
    for (seq, g) in merge_groups.iter().enumerate() {
        if let Some(rel_id) = mint_event_relations(
            hg,
            input_text,
            &out.known.relations,
            g,
            seq,
            policy,
            &mut span_cache,
            &mut result,
        ) {
            base_rel_ids.push(rel_id);
        }
    }

    // ── §6.4 — Novelty branch ───────────────────────────────────────
    // Mint an Element per novelty chunk (Phrase + Token) so spans
    // NER missed still get substrate identities. Span cache dedups
    // overlapping chunks against the spans the known branch already
    // resolved. Then mint a Defeasible Relation per NoveltyRelation
    // triple — these are candidates for replay confirmation.
    mint_novelty_chunks(
        hg,
        input_text,
        &out.novelty.chunks,
        &mut span_cache,
        &mut result,
    );
    for nr in &out.novelty.relations {
        let rel_id =
            mint_novelty_relation(hg, input_text, nr, policy, &mut span_cache, &mut result);
        base_rel_ids.push(rel_id);
        result.minted_relations.push(rel_id);
    }

    // ── §7 — Source meta-relations ──────────────────────────────────
    if let Some(source_id) = source {
        let source_attr = match hg.by_name.get("source").and_then(|v| v.first().copied()) {
            Some(id) => id,
            None => return result, // pack-shape error; nothing to do
        };
        for &base_id in &base_rel_ids {
            let meta_id = mint_relation(
                hg,
                vec![
                    Attribute {
                        name: hg.target_attr,
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
    }

    result
}

/// Resolve a character span to an `ElementId`. Cached per tick.
/// Exact-name lookup; on miss, mint with the span's contextualized
/// embedding. Reuse folds the contextualized vector into the
/// element's running centroid and bumps `access_count`.
fn resolve_span(
    hg: &mut Hypergraph,
    input_text: &str,
    char_start: usize,
    char_end: usize,
    span_text: &str,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut Step8Output,
) -> ElementId {
    if let Some(&id) = span_cache.get(&(char_start, char_end)) {
        return id;
    }

    let observed = embed_span_in_context(input_text, char_start, char_end);

    // 1. Exact-name lookup. If multiple IDs share the name, pick the
    //    one with highest cosine to the contextualized vector (or the
    //    first, if we have no contextualized vector to compare against).
    let existing = hg.by_name.get(span_text).cloned().unwrap_or_default();
    if let Some(id) = pick_best_by_cosine(&existing, observed.as_deref(), hg) {
        // Reuse path: fold the new mention's vector + bump access count.
        if let Some(obs) = observed.as_ref() {
            let prev_n = hg.elements[id.0 as usize].stats.access_count;
            fold_streaming_centroid(&mut hg.elements[id.0 as usize].embedding, obs, prev_n);
        }
        let el = &mut hg.elements[id.0 as usize];
        el.stats.access_count = el.stats.access_count.saturating_add(1);
        el.stats.last_seen = hg.clock;
        span_cache.insert((char_start, char_end), id);
        return id;
    }

    // 2. Mint. Use contextualized vector if available; otherwise fall
    //    back to surface-form text embedding (matches the seed-pack
    //    void-member fallback path).
    let embedding = observed.unwrap_or_else(|| embed_text(span_text));
    let id = mint_element(
        hg,
        vec![span_text.to_string()],
        embedding,
        Polarity::Signal,
        policy_default_conf_or_one(hg),
    );
    span_cache.insert((char_start, char_end), id);
    result.minted_elements.push(id);
    id
}

/// Resolve a type-class label (e.g. `"person"`, `"weekday"`, `"event"`)
/// to an `ElementId`. Mints a fresh element if no match exists.
/// Distinct from `resolve_span` because labels don't have char offsets
/// or a contextualized vector — they're symbolic.
fn resolve_label_element(hg: &mut Hypergraph, label: &str, result: &mut Step8Output) -> ElementId {
    let existing = hg.by_name.get(label).cloned().unwrap_or_default();
    if let Some(&id) = existing.first() {
        // Reuse, bump access. No streaming centroid (no observed vector).
        let el = &mut hg.elements[id.0 as usize];
        el.stats.access_count = el.stats.access_count.saturating_add(1);
        el.stats.last_seen = hg.clock;
        return id;
    }
    let id = mint_element(
        hg,
        vec![label.to_string()],
        embed_text(label),
        Polarity::Signal,
        policy_default_conf_or_one(hg),
    );
    result.minted_elements.push(id);
    id
}

/// Resolve an attribute-name surface form (e.g. `"instance_of"`,
/// `"from"`) to its attribute-name Element. Exact-name today; cosine
/// dedup (§5 step 2) is deferred — synonym normalization isn't urgent
/// because pattern RE emits canonical names already seeded, and NER's
/// auto-emitted `instance_of` matches the seeded `ATTR_INSTANCE_OF`.
///
/// On miss, mints a new attribute-name element and bumps
/// `attr_names_minted`. Caller is responsible for deciding whether
/// the relation should land `Defeasible` because of the mint.
fn resolve_attribute_name(hg: &mut Hypergraph, label: &str, result: &mut Step8Output) -> ElementId {
    if let Some(ids) = hg.by_name.get(label)
        && let Some(&id) = ids.first()
    {
        // Prefer a Signal hit over a Void hit — `with` and `at` exist
        // as both Signal attribute-name elements and Void adposition
        // members; the Signal sibling is the right binding here.
        for candidate in ids {
            if hg.elements[candidate.0 as usize].polarity == Polarity::Signal {
                return *candidate;
            }
        }
        return id;
    }
    let id = mint_element(
        hg,
        vec![label.to_string()],
        embed_text(label),
        Polarity::Signal,
        policy_default_conf_or_one(hg),
    );
    result.minted_elements.push(id);
    result.attr_names_minted = result.attr_names_minted.saturating_add(1);
    id
}

fn mint_pattern_relation(
    hg: &mut Hypergraph,
    input_text: &str,
    p: &RelationProposal,
    policy: &crate::types::Policy,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut Step8Output,
) -> RelationId {
    let subj_text = &input_text[p.subject_char_start..p.subject_char_end];
    let obj_text = &input_text[p.object_char_start..p.object_char_end];
    let subj_id = resolve_span(
        hg,
        input_text,
        p.subject_char_start,
        p.subject_char_end,
        subj_text,
        span_cache,
        result,
    );
    let obj_id = resolve_span(
        hg,
        input_text,
        p.object_char_start,
        p.object_char_end,
        obj_text,
        span_cache,
        result,
    );
    let attr_id = resolve_attribute_name(hg, p.attribute_name, result);
    mint_relation(
        hg,
        vec![
            Attribute {
                name: hg.subject_attr,
                value: Term::Element(subj_id),
            },
            Attribute {
                name: attr_id,
                value: Term::Element(obj_id),
            },
        ],
        p.status,
        confidence_for(p.confidence, policy),
    )
}

/// Mint a fresh Element + update `by_name`. Returns the new ID.
pub(crate) fn mint_element(
    hg: &mut Hypergraph,
    names: Vec<String>,
    embedding: Vec<f32>,
    polarity: Polarity,
    default_conf: f32,
) -> ElementId {
    let id = ElementId(hg.elements.len() as u32);
    let stats = MemoryStats {
        confidence: default_conf,
        plasticity: 1.0,
        last_seen: hg.clock,
        ..MemoryStats::default()
    };
    let el = Element {
        id,
        names: names.clone(),
        stats,
        created_at: hg.clock,
        embedding,
        polarity,
    };
    hg.elements.push(el);
    for name in names {
        hg.by_name.entry(name).or_default().push(id);
    }
    id
}

/// Append a Relation, run the §8 index updates, return its ID.
pub(crate) fn mint_relation(
    hg: &mut Hypergraph,
    attributes: Vec<Attribute>,
    status: RelationStatus,
    confidence: f32,
) -> RelationId {
    let id = RelationId(hg.relations.len() as u32);
    let stats = MemoryStats {
        confidence,
        plasticity: 1.0,
        last_seen: hg.clock,
        ..MemoryStats::default()
    };
    let r = Relation {
        id,
        attributes,
        status,
        stats,
        priority: 0,
        created_at: hg.clock,
    };
    index_relation(hg, &r);
    hg.relations.push(r);
    id
}

/// Apply §8 incremental index updates for one Relation. Mirrors the
/// build-from-scratch loop in `seed::rebuild_indices` so the two
/// paths converge.
fn index_relation(hg: &mut Hypergraph, r: &Relation) {
    let r_id = r.id;
    // First pass: per-attribute indices + collect any parent relations
    // this relation points at via `target` (Term::Relation). That set
    // drives the second pass for `meta_relation_presence`.
    let mut parent_relations: Vec<RelationId> = Vec::new();
    for attr in &r.attributes {
        hg.relations_by_attribute_name
            .entry(attr.name)
            .or_default()
            .push(r_id);
        match attr.value {
            Term::Element(e) => {
                hg.relations_by_element.entry(e).or_default().push(r_id);
                *hg.attribute_value_counts.entry((attr.name, e)).or_insert(0) += 1;
            }
            Term::Relation(parent) => {
                if attr.name == hg.target_attr {
                    hg.meta_relations_by_subject
                        .entry(parent)
                        .or_default()
                        .push(r_id);
                    parent_relations.push(parent);
                } else {
                    hg.meta_relations_by_object
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
    // R carry a meta-attribute with this name?" Used by Step 9's
    // `intervened` gate as an O(1) lookup.
    for &parent in &parent_relations {
        for attr in &r.attributes {
            if attr.name != hg.target_attr {
                hg.meta_relation_presence.insert((parent, attr.name), true);
            }
        }
    }
    for i in 0..r.attributes.len() {
        for j in 0..r.attributes.len() {
            if i == j {
                continue;
            }
            *hg.attribute_co_counts
                .entry((r.attributes[i].name, r.attributes[j].name))
                .or_insert(0) += 1;
        }
    }
}

fn pick_best_by_cosine(
    candidates: &[ElementId],
    observed: Option<&[f32]>,
    hg: &Hypergraph,
) -> Option<ElementId> {
    if candidates.is_empty() {
        return None;
    }
    let Some(obs) = observed else {
        return Some(candidates[0]);
    };
    let mut best: Option<(ElementId, f32)> = None;
    for &id in candidates {
        let cand_emb = &hg.elements[id.0 as usize].embedding;
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
/// Step 2.
fn confidence_for(extractor_conf: f32, policy: &crate::types::Policy) -> f32 {
    (policy.default_conf * extractor_conf).clamp(0.0, 1.0)
}

// ─── Property kind inference (used by n-ary mint + Step 9) ────────────

/// Walk `value_id`'s `instance_of` relations and return the kind
/// label's surface form. Returns `None` if `value_id` has no
/// `instance_of` relation in the subject slot.
///
/// "Subject slot" means `[subject: value_id, instance_of: kind]`.
/// Relations where `value_id` appears in other slots (e.g. as
/// `from` or `to`) are skipped — we want the typing of `value_id`,
/// not relations that mention it.
pub(crate) fn kind_of(hg: &Hypergraph, value_id: ElementId) -> Option<String> {
    let instance_of_attr = hg.by_name.get("instance_of")?.first().copied()?;
    let candidates = hg.relations_by_element.get(&value_id)?;
    for &rid in candidates {
        let r = &hg.relations[rid.0 as usize];
        let mut is_subject = false;
        let mut kind_value: Option<ElementId> = None;
        for attr in &r.attributes {
            if attr.name == hg.subject_attr {
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
            return hg.elements[kid.0 as usize].names.first().cloned();
        }
    }
    None
}

/// Derive a coarse property-kind label from the `from` and `to`
/// value types of a state-change event. Step 8 reads this at n-ary
/// mint time and adds the result as a `property` attribute slot on
/// the event relation. Step 9 reads it back off the event without
/// re-inferring.
///
/// Match table:
///
/// - both weekday or month (any combination) → `"date"`
/// - both time → `"time"`
/// - both quantity → `"amount"`
/// - both place → `"location"`
/// - else → `"value"` (generic fallback)
pub fn infer_property_kind(hg: &Hypergraph, from_id: ElementId, to_id: ElementId) -> &'static str {
    let f = kind_of(hg, from_id);
    let t = kind_of(hg, to_id);
    let (f, t) = (f.as_deref(), t.as_deref());
    match (f, t) {
        (Some("weekday" | "month"), Some("weekday" | "month")) => "date",
        (Some("time"), Some("time")) => "time",
        (Some("quantity"), Some("quantity")) => "amount",
        (Some("place"), Some("place")) => "location",
        _ => "value",
    }
}

/// Mint-time confidence for new Elements. The Policy isn't threaded
/// into every mint site; reading the rest-state default off the
/// Hypergraph is good enough — Step 12's frame walker reads
/// `stats.confidence` for ranking, not for truth-bearing.
fn policy_default_conf_or_one(hg: &Hypergraph) -> f32 {
    hg.policy.default_conf
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
fn compute_event_merge_groups(relations: &[RelationProposal]) -> Vec<EventMergeGroup> {
    use std::collections::HashMap;
    type Key = ((usize, usize), String);
    let mut from_idx: HashMap<Key, usize> = HashMap::new();
    let mut to_idx: HashMap<Key, usize> = HashMap::new();
    for (i, p) in relations.iter().enumerate() {
        let Some(anchor) = p.event_anchor.as_ref() else {
            continue;
        };
        let key = ((p.subject_char_start, p.subject_char_end), anchor.clone());
        match p.attribute_name {
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
    hg: &mut Hypergraph,
    input_text: &str,
    relations: &[RelationProposal],
    g: &EventMergeGroup,
    seq: usize,
    policy: &crate::types::Policy,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut Step8Output,
) -> Option<RelationId> {
    let from_p = &relations[g.from_idx];
    let to_p = &relations[g.to_idx];

    // Resolve participant elements via the same span cache used by
    // the binary path — keeps duplicate-mention dedup consistent.
    let subj_text = &input_text[from_p.subject_char_start..from_p.subject_char_end];
    let subj_id = resolve_span(
        hg,
        input_text,
        from_p.subject_char_start,
        from_p.subject_char_end,
        subj_text,
        span_cache,
        result,
    );
    let from_text = &input_text[from_p.object_char_start..from_p.object_char_end];
    let from_val_id = resolve_span(
        hg,
        input_text,
        from_p.object_char_start,
        from_p.object_char_end,
        from_text,
        span_cache,
        result,
    );
    let to_text = &input_text[to_p.object_char_start..to_p.object_char_end];
    let to_val_id = resolve_span(
        hg,
        input_text,
        to_p.object_char_start,
        to_p.object_char_end,
        to_text,
        span_cache,
        result,
    );

    // Event element — a fresh identity per merge. Name carries the
    // verb + tick + seq so it stays human-readable in dumps.
    let event_name = format!("{}_event_{}_{}", g.anchor, hg.clock.0, seq);
    let event_emb = embed_text(&event_name);
    let event_id = mint_element(
        hg,
        vec![event_name],
        event_emb,
        Polarity::Signal,
        policy_default_conf_or_one(hg),
    );
    result.minted_elements.push(event_id);

    // Typing relation — `(event, instance_of, <verb-kind>)`. Verb kind
    // resolves via lookup table; falls back to the verb itself if no
    // canonical kind is seeded.
    let kind_label = event_kind_for(&g.anchor);
    let kind_id = resolve_label_element(hg, kind_label, result);
    let instance_of_attr = resolve_attribute_name(hg, "instance_of", result);
    let typing_id = mint_relation(
        hg,
        vec![
            Attribute {
                name: hg.subject_attr,
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
    // Step 9 (supersession) can identify the cache bucket without
    // re-walking value Elements' `instance_of` relations. Falls back
    // to the generic "value" kind when the values aren't typed.
    let target_attr = hg.target_attr;
    let from_attr = resolve_attribute_name(hg, "from", result);
    let to_attr = resolve_attribute_name(hg, "to", result);
    let property_attr = resolve_attribute_name(hg, "property", result);
    let property_kind_label = infer_property_kind(hg, from_val_id, to_val_id);
    let property_kind_id = resolve_label_element(hg, property_kind_label, result);
    let nary_conf = confidence_for(from_p.confidence.min(to_p.confidence), policy);
    let nary_status = if from_p.confidence.min(to_p.confidence) >= policy.ner_assertion_threshold {
        RelationStatus::Asserted
    } else {
        RelationStatus::Defeasible
    };
    let nary_id = mint_relation(
        hg,
        vec![
            Attribute {
                name: hg.subject_attr,
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
        let intervened_attr = resolve_attribute_name(hg, "intervened", result);
        let verb_id = resolve_label_element(hg, &g.anchor, result);
        let meta_id = mint_relation(
            hg,
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
    hg: &mut Hypergraph,
    input_text: &str,
    decisions: &[CorefDecision],
    span_cache: &mut HashMap<(usize, usize), ElementId>,
) {
    for d in decisions {
        let antecedent_id = match hg.by_name.get(&d.antecedent_text) {
            Some(ids) => match ids.first() {
                Some(&id) => id,
                None => continue,
            },
            None => continue,
        };
        if let Some(obs) =
            embed_span_in_context(input_text, d.pronoun_char_start, d.pronoun_char_end)
        {
            let prev_n = hg.elements[antecedent_id.0 as usize].stats.access_count;
            fold_streaming_centroid(
                &mut hg.elements[antecedent_id.0 as usize].embedding,
                &obs,
                prev_n,
            );
        }
        let el = &mut hg.elements[antecedent_id.0 as usize];
        el.stats.access_count = el.stats.access_count.saturating_add(1);
        el.stats.last_seen = hg.clock;
        span_cache.insert((d.pronoun_char_start, d.pronoun_char_end), antecedent_id);
    }
}

// ─── §6.4 — Novelty branch ────────────────────────────────────────────

/// Mint Elements for every novelty chunk that doesn't already have
/// one. Span cache dedups against the known branch's mints so we
/// don't get two elements for the same character range.
fn mint_novelty_chunks(
    hg: &mut Hypergraph,
    input_text: &str,
    chunks: &[OrthographicChunk],
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut Step8Output,
) {
    for c in chunks {
        let _id = resolve_span(
            hg,
            input_text,
            c.char_start,
            c.char_end,
            &c.text,
            span_cache,
            result,
        );
    }
}

/// Mint a binary `Defeasible` Relation from a novelty triple. The
/// attribute text (the verbatim connective from the input — verb
/// surface form plus any void words around it) is resolved against
/// `by_name`; on miss, a new attribute-name element is minted and
/// `attr_names_minted` bumps.
fn mint_novelty_relation(
    hg: &mut Hypergraph,
    input_text: &str,
    nr: &NoveltyRelation,
    policy: &crate::types::Policy,
    span_cache: &mut HashMap<(usize, usize), ElementId>,
    result: &mut Step8Output,
) -> RelationId {
    let subj_text = &input_text[nr.subject_char_start..nr.subject_char_end];
    let obj_text = &input_text[nr.object_char_start..nr.object_char_end];
    let subj_id = resolve_span(
        hg,
        input_text,
        nr.subject_char_start,
        nr.subject_char_end,
        subj_text,
        span_cache,
        result,
    );
    let obj_id = resolve_span(
        hg,
        input_text,
        nr.object_char_start,
        nr.object_char_end,
        obj_text,
        span_cache,
        result,
    );
    let attr_id = resolve_attribute_name(hg, &nr.attribute_text, result);
    mint_relation(
        hg,
        vec![
            Attribute {
                name: hg.subject_attr,
                value: Term::Element(subj_id),
            },
            Attribute {
                name: attr_id,
                value: Term::Element(obj_id),
            },
        ],
        RelationStatus::Defeasible,
        confidence_for(nr.confidence, policy),
    )
}

/// Hand-rolled debug print so the dev-time tick output shows what
/// Step 8 actually wrote. Mirrors the style of `print_extraction`
/// in `lib.rs`.
pub fn print_step8(
    out: &Step8Output,
    hg: &Hypergraph,
    prior_element_count: usize,
    prior_relation_count: usize,
) {
    println!();
    println!("build_relations (Step 8)");
    println!(
        "  minted elements    {} ({} → {})",
        out.minted_elements.len(),
        prior_element_count,
        hg.elements.len(),
    );
    println!(
        "  minted relations   {} ({} → {})",
        out.minted_relations.len(),
        prior_relation_count,
        hg.relations.len(),
    );
    if out.attr_names_minted > 0 {
        println!(
            "  new attribute names {}  (>{} would warn per policy)",
            out.attr_names_minted, hg.policy.attribute_name_mint_warning_count,
        );
    }
    if out.minted_elements.is_empty() {
        return;
    }
    println!();
    println!("  {:<6} {:<28} {:<8}", "id", "name", "polarity");
    println!("  {:-<6} {:-<28} {:-<8}", "", "", "");
    for &id in &out.minted_elements {
        let el = &hg.elements[id.0 as usize];
        let name = el.names.first().map(|s| s.as_str()).unwrap_or("?");
        println!("  {:<6} {:<28} {:?}", id.0, truncate(name, 28), el.polarity);
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;
    use crate::steps::run_extractors::run_extractors;
    use crate::types::Policy;

    fn run_step8(text: &str) -> (Hypergraph, Step8Output) {
        run_step8_labeled(text, &[])
    }

    fn run_step8_labeled(text: &str, labels: &[&str]) -> (Hypergraph, Step8Output) {
        let mut hg = load_seed_graph();
        let policy = Policy::default();
        let out = run_extractors(text, labels, &policy, &hg, &[]);
        let step8 = build_relations(text, &mut hg, &out, &policy, None);
        (hg, step8)
    }

    #[test]
    fn mints_element_for_unseen_span() {
        let (hg, out) = run_step8("Nick lived in Brantford for 3 years.");
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
                .map(|id| &hg.elements[id.0 as usize].names[0])
                .collect::<Vec<_>>(),
        );
        let names: Vec<&str> = out
            .minted_elements
            .iter()
            .map(|id| hg.elements[id.0 as usize].names[0].as_str())
            .collect();
        assert!(names.contains(&"Nick"));
        assert!(names.contains(&"Brantford"));
    }

    #[test]
    fn instance_of_relation_uses_seeded_attribute_name() {
        let (hg, out) = run_step8("Sarah called me yesterday.");
        // At least one instance_of relation should have been minted.
        let instance_of_attr = hg.by_name["instance_of"][0];
        let has_io = out.minted_relations.iter().any(|&rid| {
            let r = &hg.relations[rid.0 as usize];
            r.attributes.iter().any(|a| a.name == instance_of_attr)
        });
        assert!(has_io, "expected at least one instance_of relation");
    }

    #[test]
    fn indices_updated_after_mint() {
        let (hg, out) = run_step8("Nick lived in Brantford for 3 years.");
        // Every minted relation should appear in
        // relations_by_attribute_name for every attribute it has.
        for &rid in &out.minted_relations {
            let r = &hg.relations[rid.0 as usize];
            for attr in &r.attributes {
                let bucket = hg
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
        // (subject of `at`) reference the same char range. Step 8
        // should hit the cache the second time.
        let (hg, out) = run_step8("Nick lived in Brantford for 3 years.");
        let nick_count = out
            .minted_elements
            .iter()
            .filter(|id| hg.elements[id.0 as usize].names.iter().any(|n| n == "Nick"))
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
        let (hg, out) = run_step8_labeled(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
        );
        let with_attr_id = hg.by_name["with"]
            .iter()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .expect("Signal `with` attribute must be seeded");

        let any_uses_signal = out.minted_relations.iter().any(|&rid| {
            let r = &hg.relations[rid.0 as usize];
            r.attributes.iter().any(|a| a.name == with_attr_id)
        });
        assert!(
            any_uses_signal,
            "Step 8 should bind `with` to the Signal attribute name",
        );

        // Make sure no relation accidentally bound to the Void `with`.
        let void_with_id = hg.by_name["with"]
            .iter()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Void)
            .copied()
            .expect("Void `with` adposition must be seeded");
        let any_uses_void = out.minted_relations.iter().any(|&rid| {
            let r = &hg.relations[rid.0 as usize];
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
        // event_anchor=Some("changed"); Step 8 merges them into one
        // n-ary relation plus a typing relation. The original two
        // binary from/to relations should NOT exist independently.
        let (hg, out) = run_step8_labeled(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
        );
        let from_attr = hg.by_name["from"]
            .iter()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .expect("Signal `from` attribute must be seeded");
        let to_attr = hg.by_name["to"]
            .iter()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
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
                let r = &hg.relations[rid.0 as usize];
                let has_from = r.attributes.iter().any(|a| a.name == from_attr);
                let has_to = r.attributes.iter().any(|a| a.name == to_attr);
                has_from && has_to
            })
            .collect();
        assert!(!nary.is_empty(), "expected at least one n-ary event");
        for &rid in &nary {
            let r = &hg.relations[rid.0 as usize];
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
                let r = &hg.relations[rid.0 as usize];
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
        // Step 8's n-ary merge should now stamp a `property` slot on
        // every event so Step 9 doesn't have to re-infer. Both values
        // are weekdays → property kind = "date".
        let (hg, out) = run_step8_labeled(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
        );
        let property_attr = hg.by_name["property"][0];
        let date_id = hg
            .by_name
            .get("date")
            .and_then(|v| v.first().copied())
            .expect("Step 8 should mint the `date` kind element");

        // Find an n-ary event and verify the property slot binds to `date`.
        let nary_with_date = out.minted_relations.iter().any(|&rid| {
            let r = &hg.relations[rid.0 as usize];
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
        let (hg, out) = run_step8_labeled("Flight from JFK to LAX.", &["place", "event"]);
        let from_attr = hg.by_name["from"]
            .iter()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .expect("Signal `from` attribute must be seeded");

        let nary: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| hg.relations[rid.0 as usize].attributes.len() >= 4)
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
                let r = &hg.relations[rid.0 as usize];
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
        let (hg, out) = run_step8_labeled(
            "The meeting rescheduled from Tuesday to Friday.",
            &["event", "weekday"],
        );
        let intervened_attr = hg
            .by_name
            .get("intervened")
            .and_then(|v| v.first().copied())
            .expect("`intervened` attribute must be seeded");
        let any_intervened = out.minted_relations.iter().any(|&rid| {
            hg.relations[rid.0 as usize]
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
        let (hg, out) = run_step8_labeled(
            "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
            &["person", "event", "weekday", "role"],
        );
        let intervened_attr = hg
            .by_name
            .get("intervened")
            .and_then(|v| v.first().copied())
            .expect("`intervened` attribute must be seeded");
        let any_intervened = out.minted_relations.iter().any(|&rid| {
            hg.relations[rid.0 as usize]
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
        // chunker emits it. Step 8 should mint an Element for it.
        let (hg, out) = run_step8("Nick lived in Brantford for 3 years.");
        let names: Vec<&str> = out
            .minted_elements
            .iter()
            .map(|id| hg.elements[id.0 as usize].names[0].as_str())
            .collect();
        assert!(
            names.contains(&"lived"),
            "novelty chunker should mint `lived`; got {names:?}",
        );
    }

    #[test]
    fn novelty_relation_lands_as_defeasible() {
        let (hg, out) = run_step8("Nick lived in Brantford for 3 years.");
        // Find a Defeasible relation whose subject is "Nick".
        let nick_id = hg
            .by_name
            .get("Nick")
            .and_then(|v| v.first().copied())
            .expect("Nick should have been minted");
        let nick_defeasible: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hg.relations[rid.0 as usize];
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
        let mut hg = load_seed_graph();
        // Mint Sarah manually so we have a target to bind to.
        let sarah_id = mint_element(
            &mut hg,
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
        let prior_access = hg.elements[sarah_id.0 as usize].stats.access_count;
        apply_coref_decisions(&mut hg, text, &decisions, &mut cache);

        assert_eq!(
            cache.get(&(0, 3)).copied(),
            Some(sarah_id),
            "coref should pin the pronoun range to Sarah's id",
        );
        assert_eq!(
            hg.elements[sarah_id.0 as usize].stats.access_count,
            prior_access + 1,
            "antecedent's access_count must bump on coref bind",
        );
    }

    #[test]
    fn coref_override_skips_unknown_antecedent() {
        // If the antecedent isn't in by_name, the override is a no-op.
        let mut hg = load_seed_graph();
        let text = "She arrived.";
        let decisions = vec![CorefDecision {
            pronoun_text: "She".to_string(),
            pronoun_char_start: 0,
            pronoun_char_end: 3,
            antecedent_text: "SomeoneUnseeded".to_string(),
            confidence: 0.9,
        }];
        let mut cache: HashMap<(usize, usize), ElementId> = HashMap::new();
        apply_coref_decisions(&mut hg, text, &decisions, &mut cache);
        assert!(
            cache.is_empty(),
            "no binding should be created for unknown antecedent",
        );
    }

    /// End-to-end integration test for the §11.9 worked example.
    /// Validates that Step 5 → Step 8 produces the expected substrate
    /// state: minted entities, event element, instance_of relations,
    /// n-ary event relation, binary `with` relation, intervened
    /// presence/absence, and index population.
    #[test]
    fn dentist_sentence_integration() {
        let text = "My dentist appointment with Dr. Rao changed from Tuesday to Friday.";
        let (hg, out) = run_step8_labeled(text, &["person", "event", "weekday", "role"]);

        // ── Entities — NER tags spans (events/persons/weekdays).
        // "My dentist appointment" lands as one event-typed span per
        // Q1 (whole-span minting); the test asserts presence by name.
        let minted_names: Vec<&str> = out
            .minted_elements
            .iter()
            .map(|id| hg.elements[id.0 as usize].names[0].as_str())
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
        let from_attr = hg.by_name["from"]
            .iter()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .unwrap();
        let to_attr = hg.by_name["to"]
            .iter()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .unwrap();
        let nary: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hg.relations[rid.0 as usize];
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
        let with_attr = hg.by_name["with"]
            .iter()
            .find(|id| hg.elements[id.0 as usize].polarity == Polarity::Signal)
            .copied()
            .unwrap();
        let with_rels: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                let r = &hg.relations[rid.0 as usize];
                r.attributes.len() == 2 && r.attributes.iter().any(|a| a.name == with_attr)
            })
            .collect();
        assert!(
            !with_rels.is_empty(),
            "expected a binary `with` relation tying appointment to Dr. Rao",
        );

        // ── No `intervened` meta-relation — `changed` is observation,
        //    not agent action (lexicon excludes it).
        let intervened_attr = hg.by_name["intervened"][0];
        let intervened_metas: Vec<RelationId> = out
            .minted_relations
            .iter()
            .copied()
            .filter(|rid| {
                hg.relations[rid.0 as usize]
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
            let r = &hg.relations[rid.0 as usize];
            for attr in &r.attributes {
                if let Term::Element(e) = attr.value {
                    let bucket = hg
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
        let mut hg = load_seed_graph();
        let policy = Policy::default();
        let text = "Sarah called me yesterday.";
        let out = run_extractors(text, &[], &policy, &hg, &[]);

        // Pick an arbitrary source element — any seeded one is fine.
        let user_id = hg.by_name["user"][0];
        let prior_rel_count = hg.relations.len();
        let result = build_relations(text, &mut hg, &out, &policy, Some(user_id));

        let source_attr = hg.by_name["source"][0];
        let source_metas: Vec<RelationId> = (prior_rel_count..hg.relations.len())
            .map(|i| RelationId(i as u32))
            .filter(|rid| {
                hg.relations[rid.0 as usize]
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
