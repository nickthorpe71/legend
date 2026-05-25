//! `hebbian_and_salience` — Hebbian + Salience.
//!
//! Pure arithmetic over `MemoryStats` — no model. Three pieces of
//! substrate maintenance plus one side-effectful write to
//! `Hypergraph.recent_focus` that unblocks `coref` and
//! `route_regions`' frame inheritance:
//!
//! 1. Hebbian activation bump (bounded Oja rule) on every Relation
//!    produced or reinforced this tick.
//! 2. Salience computation + bump (intent-modulated).
//! 3. `support_count` increment + Defeasible → Asserted promotion
//!    when the gate clears.
//! 4. Push focal `RecentFocusEntry` records onto `recent_focus`.
//!
//! See `new_foundation.md`.
//!
//! With v0 default `policy.hebbian_rate = 0.0`, every bump is a
//! mathematical no-op — `support_count` still increments and
//! `recent_focus` still populates, but `stats.activation` /
//! `stats.salience` stay put. Same gating story as `apply_region_delta`'s drift.

use std::collections::HashSet;

use crate::hebbian::bounded_hebbian_bump;
use crate::tick_pipeline::build_relations::{MintedRelations, kind_of};
use crate::tick_pipeline::supersede::Supersession;
use crate::types::{
    ElementId, Hypergraph, Policy, RecentFocusEntry, RelationId, RelationStatus, Term,
};

/// Per-tick summary of what `hebbian_and_salience` actually did. Surfaces in the
/// debug print and (for `promoted`) in the frame's downstream
/// observability hooks.
#[derive(Debug, Default, Clone)]
pub struct Reinforcement {
    /// Relations whose `stats.activation` was bumped this tick via
    /// the bounded Oja rule. With default policy this is a no-op
    /// list — the IDs are recorded but the math is a noop.
    pub reinforced: Vec<RelationId>,
    /// Relations that promoted from `Defeasible` to `Asserted`
    /// this tick.
    pub promoted: Vec<RelationId>,
    /// `RecentFocusEntry` records pushed to `hypergraph.recent_focus`.
    pub focus_pushed: u32,
    /// Topically-similar elements the tick pulled in via
    /// `topical_neighbors` (e.g. "we" surfacing on a query that
    /// doesn't name it explicitly). Threaded through here so `assemble_frame`
    /// can walk their relations into `current_state` — without this,
    /// query inputs miss live caches keyed by entities they reference
    /// implicitly.
    pub topical_seeds: Vec<ElementId>,
}

/// Run `hebbian_and_salience` over the relations `build_relations` and `supersede` minted this tick.
///
/// Reinforcement set =
/// `built_relations.minted_relations` ∪ `superseded.cache_relations` ∪
/// `superseded.meta_relations` minus `superseded.superseded`. The superseded
/// set is excluded because reinforcing a relation we just flipped
/// to `Superseded` is paradoxical — supersession says "this is no
/// longer current state."
///
/// For each relation in the reinforcement set: bump
/// `stats.activation`, compute + bump `stats.salience` (intent-
/// modulated), bump `stats.support_count` + `stats.focus_success_count`.
/// Defeasible relations that clear `policy.promotion_min_count`
/// AND `policy.promotion_min_diversity` AND have no live
/// `supersedes` meta promote to `Asserted` (confidence lifted to
/// `policy.default_conf`). Finally, push focal entries onto
/// `recent_focus` for downstream coref / frame inheritance.
pub fn hebbian_and_salience(
    hypergraph: &mut Hypergraph,
    built_relations: &MintedRelations,
    superseded: &Supersession,
    active_frame: Option<ElementId>,
    policy: &Policy,
    extra_seeds: &[ElementId],
) -> Reinforcement {
    let mut out = Reinforcement::default();

    let reinforcement_set = build_reinforcement_set(built_relations, superseded, hypergraph, extra_seeds);
    out.topical_seeds = extra_seeds.to_vec();
    let supersession_derived: HashSet<RelationId> = superseded
        .cache_relations
        .iter()
        .chain(superseded.meta_relations.iter())
        .copied()
        .collect();

    for &rid in &reinforcement_set {
        // Activation bump first — only touches stats.activation.
        let r = &mut hypergraph.relations[rid.0 as usize];
        r.stats.activation = bounded_hebbian_bump(r.stats.activation, policy.hebbian_rate);

        // Salience score per §11.11. Each component contributes
        // additively; the sum is multiplied by salience_multiplier
        // (intent-modulated by `adjust_policy`) and folded into salience via
        // the same bounded Oja bump.
        let score = compute_salience_score(hypergraph, rid, &supersession_derived, policy);
        let bump = score * policy.salience_multiplier;
        let r = &mut hypergraph.relations[rid.0 as usize];
        r.stats.salience = bounded_hebbian_bump(
            r.stats.salience,
            (bump * policy.hebbian_rate).clamp(0.0, 1.0),
        );

        // support_count is bumped only on actual re-extraction
        // (see `mint_or_reuse_base_relation`'s reuse path). Spreading
        // activation is captured by `focus_success_count` below; using
        // it for support_count would let stale Defeasible relations
        // promote whenever a future tick mentions any of their
        // elements, which is the wrong semantic for "this fact has
        // been independently re-established."

        // focus_success_count tracks "this relation landed in a
        // successful focus path this tick." `assemble_frame`'s RRF uses it as
        // the path-reinforced ranking signal alongside the
        // dense (activation-based) signal. Per §11.13 the spec
        // distinguishes "in the focus path" from "got bumped" —
        // v0 collapses both: every reinforced relation is a
        // focus-path success. Replay can refine later.
        r.stats.focus_success_count = r.stats.focus_success_count.saturating_add(1);
    }
    out.reinforced.extend(reinforcement_set.iter().copied());

    // Defeasible → Asserted promotion. Walk only the reinforcement
    // set; full-graph sweeps belong to replay (§14.8).
    for &rid in &reinforcement_set {
        if hypergraph.relations[rid.0 as usize].status != RelationStatus::Defeasible {
            continue;
        }
        if check_promotion(hypergraph, rid, policy) {
            let r = &mut hypergraph.relations[rid.0 as usize];
            r.status = RelationStatus::Asserted;
            // Lift confidence so downstream queries see the higher
            // belief strength. Don't drop below the existing value.
            r.stats.confidence = r.stats.confidence.max(policy.default_conf).clamp(0.0, 1.0);
            out.promoted.push(rid);
        }
    }

    // Populate `recent_focus`. `coref` and `route_regions`' frame
    // inheritance read from this; the empty deque is why both run as
    // stubs / no-ops today. Push one entry per focal (element,
    // attribute) pair per tick; intra-tick dedup keeps a chatty
    // tick (many relations with the same subject) from spamming
    // the ring buffer. Source = `build_relations` minted + `supersede` caches —
    // meta-relations don't push (their `target` is a Relation,
    // not an Element).
    out.focus_pushed = push_recent_focus(hypergraph, built_relations, superseded, active_frame, policy);

    out
}

/// Push `RecentFocusEntry` records for this tick's focal elements.
/// Returns the number of entries pushed.
fn push_recent_focus(
    hypergraph: &mut Hypergraph,
    built_relations: &MintedRelations,
    superseded: &Supersession,
    active_frame: Option<ElementId>,
    policy: &Policy,
) -> u32 {
    let subject_attr = hypergraph.subject_attr;
    let target_attr = hypergraph.target_attr;
    let tick = hypergraph.clock;

    // Collect (element, attribute) pairs, deduped intra-tick. We
    // walk built_relations.minted_relations first then superseded.cache_relations
    // so `build_relations`'s mints take precedence when the same (element,
    // attribute) pair would land twice.
    let mut seen: HashSet<(ElementId, Option<ElementId>)> = HashSet::new();
    let mut entries: Vec<RecentFocusEntry> = Vec::new();

    let consider = |hypergraph: &Hypergraph, rid: RelationId, seen: &mut _, entries: &mut Vec<_>| {
        let r = &hypergraph.relations[rid.0 as usize];
        // Subject-bound entry: every relation with a subject slot.
        if let Some(subj) = element_at_attribute(r, subject_attr) {
            push_unique(seen, entries, subj, Some(subject_attr), active_frame, tick);
        }
        // N-ary events: also push the `target` as a focal element
        // bound under target_attr. Distinguishes "it (focused as
        // target)" from "it (focused as subject)" for coref later.
        if let Some(tgt) = element_at_attribute(r, target_attr) {
            push_unique(seen, entries, tgt, Some(target_attr), active_frame, tick);
        }
    };

    for &rid in &built_relations.minted_relations {
        consider(hypergraph, rid, &mut seen, &mut entries);
    }
    for &rid in &superseded.cache_relations {
        consider(hypergraph, rid, &mut seen, &mut entries);
    }

    // Push to the FRONT of the deque so the most recent tick's
    // entries are at the head. Coref walks front-to-back.
    let pushed = entries.len() as u32;
    for entry in entries.into_iter().rev() {
        hypergraph.recent_focus.push_front(entry);
    }
    // Truncate to capacity from the back (oldest entries drop).
    let cap = policy.recent_focus_capacity as usize;
    while hypergraph.recent_focus.len() > cap {
        hypergraph.recent_focus.pop_back();
    }
    pushed
}

fn push_unique(
    seen: &mut HashSet<(ElementId, Option<ElementId>)>,
    entries: &mut Vec<RecentFocusEntry>,
    element: ElementId,
    attribute: Option<ElementId>,
    frame: Option<ElementId>,
    tick: crate::types::Tick,
) {
    let key = (element, attribute);
    if seen.insert(key) {
        entries.push(RecentFocusEntry {
            element,
            attribute,
            frame,
            tick,
        });
    }
}

fn element_at_attribute(r: &crate::types::Relation, attr: ElementId) -> Option<ElementId> {
    for a in &r.attributes {
        if a.name == attr
            && let Term::Element(e) = a.value
        {
            return Some(e);
        }
    }
    None
}

/// Returns `true` iff Defeasible relation `R` clears all three
/// promotion gates per §11.11:
///
/// 1. `support_count >= policy.promotion_min_count`
/// 2. `support_diversity >= policy.promotion_min_diversity`
///    (v0 leaves this at 0 — replay's job to maintain)
/// 3. No live `supersedes` meta-relation points at R
///    (`meta_relations_by_object[R]` filtered to entries whose
///    attribute list contains `supersedes`)
fn check_promotion(hypergraph: &Hypergraph, rid: RelationId, policy: &Policy) -> bool {
    let r = &hypergraph.relations[rid.0 as usize];
    if r.stats.support_count < policy.promotion_min_count {
        return false;
    }
    if r.stats.support_diversity < policy.promotion_min_diversity {
        return false;
    }
    if has_live_supersedes_meta(hypergraph, rid) {
        return false;
    }
    // Attribute-name maturity gate: a relation can't promote until
    // its primary (non-subject) attribute-name element has been
    // around for at least `attribute_name_maturity_ticks`. Brand-new
    // predicate mints haven't had time to be validated; promoting them
    // freezes extraction noise. Seed predicates have created_at=0
    // so this gate is a no-op for them.
    let primary_attr = r
        .attributes
        .iter()
        .find(|a| a.name != hypergraph.subject_attr)
        .map(|a| a.name);
    if let Some(attr_eid) = primary_attr {
        let attr = &hypergraph.elements[attr_eid.0 as usize];
        let age = hypergraph.clock.0.saturating_sub(attr.created_at.0);
        if age < policy.attribute_name_maturity_ticks as u64 {
            return false;
        }
    }
    true
}

fn has_live_supersedes_meta(hypergraph: &Hypergraph, rid: RelationId) -> bool {
    let Some(supersedes_attr) = hypergraph
        .by_name
        .get("supersedes")
        .and_then(|v| v.first().copied())
    else {
        return false;
    };
    let Some(metas) = hypergraph.meta_relations_by_object.get(&rid) else {
        return false;
    };
    for &mid in metas {
        let m = &hypergraph.relations[mid.0 as usize];
        // Skip metas that have themselves been retracted.
        if matches!(m.status, RelationStatus::Retracted) {
            continue;
        }
        if m.attributes.iter().any(|a| a.name == supersedes_attr) {
            return true;
        }
    }
    false
}

/// Salience score per §11.11. Components stack additively; v0 uses
/// the §11.11 weights plus a `salience_floor` baseline from policy.
///
/// Returns a non-negative score; the caller multiplies by
/// `salience_multiplier` (intent-modulated) and uses
/// `bounded_hebbian_bump` to apply.
fn compute_salience_score(
    hypergraph: &Hypergraph,
    rid: RelationId,
    supersession_derived: &HashSet<RelationId>,
    policy: &Policy,
) -> f32 {
    let mut score = policy.salience_floor;

    // +1.0 if any Element-valued attribute references a typed leaf
    // value (date / number / named-entity kind via instance_of). The
    // detection reuses kind_of from build_relations.
    if relation_has_exact_value_attribute(hypergraph, rid) {
        score += 1.0;
    }

    // +1.0 if R was just produced by supersession.
    if supersession_derived.contains(&rid) {
        score += 1.0;
    }

    // +0.5 if R is in the reinforcement set (focus-bearing this tick).
    // True for every R reaching this function, so unconditional.
    score += 0.5;

    score
}

/// True iff `R` carries at least one Element-valued attribute whose
/// value Element has an `instance_of` relation pointing at a kind
/// label like `weekday`, `month`, `time`, `quantity`, `person`,
/// `place`, or `org`. Reuses `build_relations`'s `kind_of` walker.
fn relation_has_exact_value_attribute(hypergraph: &Hypergraph, rid: RelationId) -> bool {
    const EXACT_KINDS: &[&str] = &[
        "weekday", "month", "time", "quantity", "person", "place", "org",
    ];
    let r = &hypergraph.relations[rid.0 as usize];
    for attr in &r.attributes {
        if let Term::Element(e) = attr.value
            && let Some(kind) = kind_of(hypergraph, e)
            && EXACT_KINDS.contains(&kind.as_str())
        {
            return true;
        }
    }
    false
}

/// Construct the per-tick reinforcement set. Deduplicates via a
/// `HashSet` so meta-relations that appear in both built_relations and superseded
/// (unlikely but defensive) don't get double-counted.
/// Inherit the active frame for this tick from `recent_focus`.
/// Returns the most-recently-focused element whose attribute slot
/// was `subject` AND whose surface name isn't a closed-class
/// pronoun, copula, or interrogative — those are speaker / question
/// machinery, not the topical entity the conversation is about.
///
/// Two further gates on top of the prior subject-only / pronoun
/// filter:
///
/// 1. **Recency.** An entry pushed more than
///    `policy.active_frame_max_age_ticks` ago is stale by
///    construction (likely from a different conversational episode)
///    and skipped. Requires `hypergraph.clock` to advance per tick — the
///    clock bump lives at the top of `execute_tick::run`.
///
/// 2. **Query intent.** When `intent` reads as a query (curiosity
///    above `policy.query_curiosity_threshold` AND conviction below
///    `policy.query_conviction_threshold`), return `None`. A query
///    inherits no topical anchor; the speaker is asking *about*
///    something, not continuing a thread, and inheriting the prior
///    frame would lie about context.
///
/// Frame-shifting cues from the current input (which would override
/// this) are §11.6 territory and not wired in v0; the inheritance
/// path lights up first.
///
/// Returns `None` when `recent_focus` is empty (first tick), when
/// every eligible entry is a pronoun / question word, when every
/// entry is stale, or when intent is query-shaped.
///
/// Call this BEFORE `hebbian_and_salience`'s push so the result
/// reflects PRIOR ticks, not the current tick's about-to-be-pushed
/// entries.
pub fn derive_active_frame(
    hypergraph: &Hypergraph,
    intent: &crate::types::Intent,
    policy: &crate::types::Policy,
) -> Option<ElementId> {
    // Query intent → no inherited frame.
    if intent.curiosity >= policy.query_curiosity_threshold
        && intent.conviction <= policy.query_conviction_threshold
    {
        return None;
    }
    let now = hypergraph.clock.0;
    let max_age = policy.active_frame_max_age_ticks as u64;
    hypergraph.recent_focus.iter().find_map(|entry| {
        if entry.attribute != Some(hypergraph.subject_attr) {
            return None;
        }
        // Recency gate. `entry.tick == 0` means "pre-clock-bump
        // legacy data" — treat as fresh to avoid breaking the
        // first tick after upgrade; subsequent ticks carry a real
        // stamp from the per-tick clock bump.
        if entry.tick.0 > 0 && now.saturating_sub(entry.tick.0) > max_age {
            return None;
        }
        // Reject pronouns and question words — they're sentence
        // machinery (subject of "I started a new project" or "What
        // language is Polaris in?") rather than topical content.
        let names = &hypergraph.elements[entry.element.0 as usize].names;
        for name in names {
            if is_frame_unsuitable(&name.to_ascii_lowercase()) {
                return None;
            }
        }
        Some(entry.element)
    })
}

/// Surface names that should NEVER win the active-frame slot — the
/// conversation isn't "about" these even when they're the subject
/// of an utterance.
fn is_frame_unsuitable(lowered: &str) -> bool {
    matches!(
        lowered,
        // First/second/third-person pronouns (subject + reflexive).
        "i" | "i'm" | "me" | "my" | "myself"
        | "you" | "your" | "yours"
        | "we" | "us" | "our"
        | "they" | "them" | "their"
        | "he" | "him" | "his"
        | "she" | "her" | "hers"
        | "it" | "its"
        // Interrogative pronouns / wh-words.
        | "who" | "whom" | "whose"
        | "what" | "which"
        | "when" | "where" | "why" | "how"
        // Demonstratives.
        | "this" | "that" | "these" | "those"
    )
}

/// Per-element cap on retrieved relations so a single high-degree
/// element (e.g. a pronoun resolving to a frequently-mentioned
/// entity) doesn't dominate the focus set. Replay later refines via
/// salience-weighted ranking; v0 takes the first K live ones in
/// `relations_by_element` order.
const RETRIEVE_CAP_PER_ELEMENT: usize = 12;

fn build_reinforcement_set(
    built_relations: &MintedRelations,
    superseded: &Supersession,
    hypergraph: &crate::types::Hypergraph,
    extra_seeds: &[ElementId],
) -> Vec<RelationId> {
    let superseded_set: HashSet<RelationId> = superseded.superseded.iter().copied().collect();
    let mut seen: HashSet<RelationId> = HashSet::new();
    let mut out = Vec::new();
    // 1. New writes: `build_relations` mints + `supersede` caches + `supersede` metas.
    for &rid in built_relations
        .minted_relations
        .iter()
        .chain(superseded.cache_relations.iter())
        .chain(superseded.meta_relations.iter())
    {
        if superseded_set.contains(&rid) {
            continue;
        }
        if seen.insert(rid) {
            out.push(rid);
        }
    }
    // 2. Retrieval seeds = name-mentioned elements (built_relations.referenced)
    //    plus topical-similarity neighbors the caller passes in
    //    (typically `steps::topical::topical_neighbors`). The union
    //    lets queries surface haystack content that doesn't share
    //    surface tokens with the query — e.g. "what was the first
    //    issue with my car?" pulling in a GPS-system relation
    //    because its embedding sits close to the question's.
    //    Dedup is automatic via the `seen` set below.
    let referenced_then_topical = built_relations
        .referenced_elements
        .iter()
        .chain(extra_seeds.iter())
        .copied();
    let mut seen_seeds: HashSet<ElementId> = HashSet::new();
    for eid in referenced_then_topical {
        if !seen_seeds.insert(eid) {
            continue;
        }
        let Some(candidates) = hypergraph.relations_by_element.get(&eid) else {
            continue;
        };
        // Order candidates with cache relations first so the per-
        // element cap doesn't drop the current-state cache in favor
        // of older bystander relations. Stable so the secondary
        // order (insertion) is preserved within each group.
        let mut ordered: Vec<RelationId> = candidates.clone();
        ordered
            .sort_by_key(|&rid| !crate::tick_pipeline::build_relations::is_cache_relation(hypergraph, rid) as u8);
        let mut added_for_this_element = 0usize;
        for rid in ordered {
            if added_for_this_element >= RETRIEVE_CAP_PER_ELEMENT {
                break;
            }
            if superseded_set.contains(&rid) || seen.contains(&rid) {
                continue;
            }
            let r = &hypergraph.relations[rid.0 as usize];
            // Status filter: live relations only.
            if !matches!(
                r.status,
                crate::types::RelationStatus::Asserted
                    | crate::types::RelationStatus::Entailed
                    | crate::types::RelationStatus::Defeasible
            ) {
                continue;
            }
            // Skip meta-relations — those participate via the frame's
            // `supporting_claims` / `history` walker, not the focus
            // set. A meta-relation has `target` (not `subject`) as its
            // anchoring slot.
            let is_meta = !r.attributes.iter().any(|a| a.name == hypergraph.subject_attr);
            if is_meta {
                continue;
            }
            // Skip substrate-scaffolding relations: `parent_region`
            // and `prototype` are how the seed pack stitches the
            // region DAG together; they don't represent knowledge the
            // consumer LLM should see in `focused_relations`. Without
            // this filter, retrieving relations for common nouns
            // ("time", "language") pulls in 5-10 seed-shape rows that
            // crowd out the actual user content.
            if r.attributes
                .iter()
                .any(|a| a.name == hypergraph.parent_region_attr || a.name == hypergraph.prototype_attr)
            {
                continue;
            }
            seen.insert(rid);
            out.push(rid);
            added_for_this_element += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::load_seed_graph;
    use crate::tick_pipeline::build_relations::{build_relations, mint_element, mint_relation};
    use crate::tick_pipeline::run_extractors::run_extractors;
    use crate::tick_pipeline::supersede::supersede;
    use crate::types::{Attribute, Polarity};

    /// Run Steps 5/8/9 over `text` to produce realistic Step8 + Step9
    /// outputs; return the Hypergraph + both outputs so `hebbian_and_salience` tests
    /// can drive `hebbian_and_salience` over a real reinforcement set.
    fn run_through_supersede(
        text: &str,
        labels: &[&str],
        policy: &Policy,
    ) -> (Hypergraph, MintedRelations, Supersession) {
        let mut hypergraph = load_seed_graph();
        let ext = run_extractors(text, labels, policy, &hypergraph, &[]);
        let built_relations = build_relations(text, &mut hypergraph, &ext, policy, None);
        let superseded = supersede(&mut hypergraph, &built_relations.minted_relations, policy);
        (hypergraph, built_relations, superseded)
    }

    #[test]
    fn reinforcement_set_includes_built_relations_minted() {
        let policy = Policy::default();
        let (mut hypergraph, built_relations, superseded) = run_through_supersede("Sarah called me yesterday.", &[], &policy);
        let out = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        assert!(!out.reinforced.is_empty());
        for &rid in &built_relations.minted_relations {
            assert!(out.reinforced.contains(&rid));
        }
    }

    #[test]
    fn reinforcement_set_includes_superseded_caches_and_metas() {
        let policy = Policy::default();
        let (mut hypergraph, built_relations, superseded) = run_through_supersede(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        assert!(!superseded.cache_relations.is_empty(), "test prerequisite");
        let out = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        for &rid in &superseded.cache_relations {
            assert!(out.reinforced.contains(&rid));
        }
        for &rid in &superseded.meta_relations {
            assert!(out.reinforced.contains(&rid));
        }
    }

    #[test]
    fn reinforcement_excludes_superseded() {
        // Two ticks: the second's events superseded the first's caches.
        // `hebbian_and_salience` on tick 2 must NOT include the superseded priors.
        let policy = Policy::default();
        let labels: &[&str] = &["event", "weekday"];
        let (mut hypergraph, built_relations_1, superseded_1) =
            run_through_supersede_with("The meeting moved from Monday to Tuesday.", labels, &policy);
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations_1, &superseded_1, None, &policy, &[]);

        let text2 = "The meeting moved from Tuesday to Friday.";
        let ext2 = run_extractors(text2, labels, &policy, &hypergraph, &[]);
        let built_relations_2 = build_relations(text2, &mut hypergraph, &ext2, &policy, None);
        let superseded_2 = supersede(&mut hypergraph, &built_relations_2.minted_relations, &policy);
        assert!(
            !superseded_2.superseded.is_empty(),
            "test prerequisite: tick 2 should flip the prior",
        );
        let out2 = hebbian_and_salience(&mut hypergraph, &built_relations_2, &superseded_2, None, &policy, &[]);
        for &flipped in &superseded_2.superseded {
            assert!(
                !out2.reinforced.contains(&flipped),
                "superseded relation {flipped:?} should not be reinforced",
            );
        }
    }

    fn run_through_supersede_with(
        text: &str,
        labels: &[&str],
        policy: &Policy,
    ) -> (Hypergraph, MintedRelations, Supersession) {
        let mut hypergraph = load_seed_graph();
        let ext = run_extractors(text, labels, policy, &hypergraph, &[]);
        let built_relations = build_relations(text, &mut hypergraph, &ext, policy, None);
        let superseded = supersede(&mut hypergraph, &built_relations.minted_relations, policy);
        (hypergraph, built_relations, superseded)
    }

    #[test]
    fn default_policy_keeps_activation_at_zero() {
        // hebbian_rate = 0 → activation should stay at 0 (the
        // MemoryStats default) even though the relation IDs land in
        // the reinforced list.
        let policy = Policy::default();
        let (mut hypergraph, built_relations, superseded) = run_through_supersede("Sarah called me yesterday.", &[], &policy);
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        for &rid in &built_relations.minted_relations {
            assert_eq!(
                hypergraph.relations[rid.0 as usize].stats.activation, 0.0,
                "default policy should leave activation untouched",
            );
        }
    }

    #[test]
    fn nonzero_rate_bumps_activation_toward_one() {
        let policy = Policy {
            hebbian_rate: 0.5,
            ..Default::default()
        };
        let (mut hypergraph, built_relations, superseded) = run_through_supersede("Sarah called me yesterday.", &[], &policy);

        // Take a baseline.
        let pick = built_relations.minted_relations[0];
        let before = hypergraph.relations[pick.0 as usize].stats.activation;
        assert_eq!(before, 0.0, "freshly minted relations start at 0");

        // First bump.
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        let after_one = hypergraph.relations[pick.0 as usize].stats.activation;
        assert!(
            (after_one - 0.5).abs() < 1e-5,
            "bump(0, 0.5) = 0.5; got {after_one}"
        );

        // Second bump: bump(0.5, 0.5) = 0.75.
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        let after_two = hypergraph.relations[pick.0 as usize].stats.activation;
        assert!(
            (after_two - 0.75).abs() < 1e-5,
            "bump(0.5, 0.5) = 0.75; got {after_two}"
        );
    }

    #[test]
    fn salience_bumps_on_typed_value_relation() {
        // NER tags `Tuesday` with kind=weekday, so the
        // [subject: Tuesday, instance_of: weekday] relation should
        // count as an exact-value attribute hit and earn the +1.0
        // bonus on top of the focus-bearing +0.5 baseline.
        let policy = Policy {
            hebbian_rate: 0.5,
            ..Default::default()
        };
        let (mut hypergraph, built_relations, superseded) = run_through_supersede(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );

        // Find the instance_of relation for Tuesday — it should have
        // an exact-value attribute (its object is the `weekday` kind
        // element). Snapshot salience before/after.
        let weekday_id = hypergraph.by_name["weekday"][0];
        let instance_of_attr = hypergraph.by_name["instance_of"][0];
        let inst_rel = built_relations
            .minted_relations
            .iter()
            .copied()
            .find(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                r.attributes.iter().any(|a| {
                    a.name == instance_of_attr
                        && matches!(a.value, Term::Element(e) if e == weekday_id)
                })
            })
            .expect("expected at least one instance_of weekday relation");

        let before = hypergraph.relations[inst_rel.0 as usize].stats.salience;
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        let after = hypergraph.relations[inst_rel.0 as usize].stats.salience;
        assert!(
            after > before,
            "salience should bump for an exact-value relation; before={before} after={after}",
        );
    }

    #[test]
    fn salience_higher_for_supersession_derived() {
        // Cache + linking metas from `supersede` should get the +1.0
        // supersession-derived bonus on top of focus-bearing — total
        // ≥ 1.5 (before multiplier + floor). With salience_multiplier
        // = 1.0 and hebbian_rate = 0.5, salience after one bump =
        // bump(0, 0.5 * (0.0 + 1.5)) = bump(0, 0.75) which clamps to 0.75.
        let policy = Policy {
            hebbian_rate: 0.5,
            ..Default::default()
        };
        let (mut hypergraph, built_relations, superseded) = run_through_supersede(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        assert!(!superseded.cache_relations.is_empty(), "test prerequisite");
        let cache = superseded.cache_relations[0];
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        let cache_sal = hypergraph.relations[cache.0 as usize].stats.salience;
        assert!(
            cache_sal > 0.5,
            "cache (supersession-derived) salience should clear 0.5; got {cache_sal}",
        );
    }

    #[test]
    fn support_count_increments_per_reextraction() {
        // support_count is bumped when `build_relations`'s reuse path matches an
        // existing relation — i.e. on re-extraction. `hebbian_and_salience`'s
        // spreading activation does NOT bump it; that's
        // `focus_success_count`'s role.
        let policy = Policy::default();
        let text = "Sarah called me yesterday.";
        let (mut hypergraph, _built_relations, _superseded) = run_through_supersede(text, &[], &policy);

        // After tick 1, support_count for a freshly minted relation is 0
        // (the mint path starts at default).
        let pick = hypergraph
            .relations
            .iter()
            .find(|r| {
                r.attributes
                    .iter()
                    .any(|a| matches!(a.value, crate::types::Term::Element(eid)
                        if hypergraph.elements[eid.0 as usize].names.iter().any(|n| n == "Sarah")))
            })
            .map(|r| r.id)
            .expect("Sarah-bearing relation should mint");
        assert_eq!(hypergraph.relations[pick.0 as usize].stats.support_count, 0);

        // Re-extract via `run_extractors` + `build_relations` → reuse path fires → bump.
        let ext = run_extractors(text, &[], &policy, &hypergraph, &[]);
        let _ = build_relations(text, &mut hypergraph, &ext, &policy, None);
        assert_eq!(hypergraph.relations[pick.0 as usize].stats.support_count, 1);
        let ext = run_extractors(text, &[], &policy, &hypergraph, &[]);
        let _ = build_relations(text, &mut hypergraph, &ext, &policy, None);
        assert_eq!(hypergraph.relations[pick.0 as usize].stats.support_count, 2);
    }

    #[test]
    fn defeasible_promotes_when_gate_clears() {
        // Custom policy: min_count=3, min_diversity=0 (so v0's
        // 0-by-design diversity counter doesn't block). Disable the
        // maturity gate so this test focuses on the support_count
        // gate — a fresh seeded substrate has clock=0 and the seed
        // predicates also have created_at=0, but if the relation's
        // attribute is anything *not* seeded the maturity gate
        // would otherwise hold things back.
        let policy = Policy {
            promotion_min_count: 3,
            promotion_min_diversity: 0,
            attribute_name_maturity_ticks: 0,
            ..Default::default()
        };

        // Use a sentence whose NER score on `meeting` is below the
        // assertion threshold → instance_of relation lands Defeasible.
        let (mut hypergraph, built_relations, superseded) = run_through_supersede(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );

        // Find a Defeasible instance_of relation to track.
        let instance_of_attr = hypergraph.by_name["instance_of"][0];
        let target_rid = built_relations
            .minted_relations
            .iter()
            .copied()
            .find(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                r.status == RelationStatus::Defeasible
                    && r.attributes.iter().any(|a| a.name == instance_of_attr)
            })
            .expect("expected at least one Defeasible instance_of");

        // Simulate two prior re-extractions: support_count goes to 2.
        // (In real use this comes from `mint_or_reuse_base_relation`'s
        // reuse path firing twice on the same triple.)
        hypergraph.relations[target_rid.0 as usize].stats.support_count = 2;
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        assert_eq!(
            hypergraph.relations[target_rid.0 as usize].status,
            RelationStatus::Defeasible,
            "not promoted yet at support_count=2",
        );

        // One more re-extraction → support_count = 3 → gate clears.
        hypergraph.relations[target_rid.0 as usize].stats.support_count = 3;
        let out = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        assert_eq!(
            hypergraph.relations[target_rid.0 as usize].status,
            RelationStatus::Asserted,
            "expected promotion at support_count=3",
        );
        assert!(
            out.promoted.contains(&target_rid),
            "Reinforcement.promoted must list the promoted relation",
        );
    }

    #[test]
    fn promotion_blocked_by_attribute_name_maturity() {
        // A relation whose attribute-name element is brand-new
        // (created_at close to hypergraph.clock) should NOT promote even if
        // support_count clears. Maturity gate = 5 ticks; the relation's
        // attribute name was minted at clock=0 and the substrate's
        // clock is also still 0 here, so age=0 < 5 blocks promotion.
        let policy = Policy {
            promotion_min_count: 1,
            promotion_min_diversity: 0,
            attribute_name_maturity_ticks: 5,
            ..Default::default()
        };
        let mut hypergraph = load_seed_graph();
        // Build a relation using a brand-new attribute-name element
        // ("uses" — not a seeded predicate; lexicon-dedup would catch
        // it normally, but for this test bypass that by minting
        // directly).
        let mut result = MintedRelations::default();
        let new_attr_id = mint_element(
            &mut hypergraph,
            vec!["custom_attr".to_string()],
            crate::embed::embed_text("custom_attr"),
            crate::types::Polarity::Signal,
            1.0,
        );
        result.minted_elements.push(new_attr_id);
        // Some subject element.
        let subj_id = mint_element(
            &mut hypergraph,
            vec!["custom_subj".to_string()],
            crate::embed::embed_text("custom_subj"),
            crate::types::Polarity::Signal,
            1.0,
        );
        let obj_id = mint_element(
            &mut hypergraph,
            vec!["custom_obj".to_string()],
            crate::embed::embed_text("custom_obj"),
            crate::types::Polarity::Signal,
            1.0,
        );
        let subject_attr = hypergraph.subject_attr;
        let rel = mint_relation(
            &mut hypergraph,
            vec![
                Attribute {
                    name: subject_attr,
                    value: Term::Element(subj_id),
                },
                Attribute {
                    name: new_attr_id,
                    value: Term::Element(obj_id),
                },
            ],
            RelationStatus::Defeasible,
            0.3,
        );
        // Simulate enough support_count to clear that gate.
        hypergraph.relations[rel.0 as usize].stats.support_count = 5;

        // Hand-construct minimal `build_relations`/9 output that reinforces this
        // relation.
        let built_relations = MintedRelations {
            minted_relations: vec![rel],
            ..Default::default()
        };
        let superseded = crate::tick_pipeline::supersede::Supersession::default();
        let out = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        assert_eq!(
            hypergraph.relations[rel.0 as usize].status,
            RelationStatus::Defeasible,
            "maturity gate should block promotion of a relation whose attribute is brand-new",
        );
        assert!(out.promoted.is_empty());
    }

    #[test]
    fn promotion_blocked_by_low_support_count() {
        let policy = Policy {
            promotion_min_count: 5,
            promotion_min_diversity: 0,
            ..Default::default()
        };
        let (mut hypergraph, built_relations, superseded) = run_through_supersede(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        let out = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        // After one tick, support_count == 1; min is 5; nothing
        // should have promoted.
        assert!(
            out.promoted.is_empty(),
            "no promotion expected with support_count=1 and min=5",
        );
    }

    #[test]
    fn promotion_blocked_by_min_diversity_gate() {
        // Replay will eventually flip `promotion_min_diversity` above 0
        // and maintain `support_diversity`. Until then, asserting the
        // gate works means setting `min_diversity` explicitly and
        // confirming a 0-diversity relation can't clear it. (v0 default
        // is 0 — disabled — so this is a forward-compat test.)
        let policy = Policy {
            promotion_min_count: 1, // even trivially low won't help
            promotion_min_diversity: 2,
            ..Default::default()
        };
        let (mut hypergraph, built_relations, superseded) = run_through_supersede(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        let out = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        assert!(
            out.promoted.is_empty(),
            "min_diversity=2 with support_diversity=0 should block promotion",
        );
    }

    #[test]
    fn promotion_blocked_by_supersedes_meta() {
        // Two-tick fixture: tick 1 mints a Defeasible cache-attribute
        // relation; tick 2 supersedes it. `hebbian_and_salience` on tick 2 must NOT
        // promote the flipped-prior cache even if support_count clears.
        // (The flipped prior is excluded from reinforcement, so this
        // is really a test of the supersedes-meta gate via secondary
        // visibility — the cache that *was* superseded has a
        // meta_relations_by_object entry containing a supersedes meta.)
        let policy = Policy {
            promotion_min_count: 1,
            promotion_min_diversity: 0,
            ..Default::default()
        };
        let labels: &[&str] = &["event", "weekday"];

        let mut hypergraph = load_seed_graph();
        let ext1 = run_extractors(
            "The meeting moved from Monday to Tuesday.",
            labels,
            &policy,
            &hypergraph,
            &[],
        );
        let built_relations_1 = build_relations(
            "The meeting moved from Monday to Tuesday.",
            &mut hypergraph,
            &ext1,
            &policy,
            None,
        );
        let superseded_1 = supersede(&mut hypergraph, &built_relations_1.minted_relations, &policy);
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations_1, &superseded_1, None, &policy, &[]);

        // Tick 2 supersedes tick 1's cache.
        let ext2 = run_extractors(
            "The meeting moved from Tuesday to Friday.",
            labels,
            &policy,
            &hypergraph,
            &[],
        );
        let built_relations_2 = build_relations(
            "The meeting moved from Tuesday to Friday.",
            &mut hypergraph,
            &ext2,
            &policy,
            None,
        );
        let superseded_2 = supersede(&mut hypergraph, &built_relations_2.minted_relations, &policy);
        assert!(!superseded_2.superseded.is_empty());
        let prior = superseded_2.superseded[0];

        // Force the prior into Defeasible just for this test — we
        // want to verify the supersedes-meta gate, not the status
        // bookkeeping the prior already underwent.
        hypergraph.relations[prior.0 as usize].status = RelationStatus::Defeasible;

        // Manually craft a reinforcement set containing the prior so
        // promotion would be considered. Step10's normal path
        // excludes superseded; this synthetic call exercises the
        // gate logic only.
        let mut synthetic_superseded = superseded_2.clone();
        synthetic_superseded.superseded.clear();
        synthetic_superseded.cache_relations.push(prior); // include in reinforcement set
        let out = hebbian_and_salience(&mut hypergraph, &built_relations_2, &synthetic_superseded, None, &policy, &[]);

        assert!(
            !out.promoted.contains(&prior),
            "promotion must be blocked by an existing supersedes meta",
        );
    }

    #[test]
    fn promotion_bumps_confidence_to_default_conf() {
        let policy = Policy {
            promotion_min_count: 1,
            promotion_min_diversity: 0,
            attribute_name_maturity_ticks: 0,
            default_conf: 0.9,
            ..Default::default()
        };
        let (mut hypergraph, built_relations, superseded) = run_through_supersede(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );
        let instance_of_attr = hypergraph.by_name["instance_of"][0];
        let target_rid = built_relations
            .minted_relations
            .iter()
            .copied()
            .find(|rid| {
                let r = &hypergraph.relations[rid.0 as usize];
                r.status == RelationStatus::Defeasible
                    && r.attributes.iter().any(|a| a.name == instance_of_attr)
            })
            .expect("expected at least one Defeasible instance_of");
        let conf_before = hypergraph.relations[target_rid.0 as usize].stats.confidence;
        assert!(conf_before < 0.9);

        // promotion_min_count=1 so a single bump clears the gate.
        hypergraph.relations[target_rid.0 as usize].stats.support_count = 1;
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        assert_eq!(
            hypergraph.relations[target_rid.0 as usize].status,
            RelationStatus::Asserted,
        );
        assert!(
            (hypergraph.relations[target_rid.0 as usize].stats.confidence - 0.9).abs() < 1e-5,
            "promotion should lift confidence to default_conf",
        );
    }

    #[test]
    fn recent_focus_populated_with_subject_entries() {
        let policy = Policy::default();
        let (mut hypergraph, built_relations, superseded) = run_through_supersede("Sarah called me yesterday.", &[], &policy);
        assert!(hypergraph.recent_focus.is_empty(), "starts empty");

        let out = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        assert!(out.focus_pushed > 0, "expected focus entries pushed");
        assert!(
            !hypergraph.recent_focus.is_empty(),
            "recent_focus should have entries"
        );

        // Every minted relation's subject (and target for n-ary)
        // should appear once in recent_focus for this tick.
        let sarah_id = hypergraph.by_name["Sarah"][0];
        assert!(
            hypergraph.recent_focus
                .iter()
                .any(|e| e.element == sarah_id && e.attribute == Some(hypergraph.subject_attr)),
            "Sarah should be focused as subject",
        );
    }

    #[test]
    fn recent_focus_dedups_intra_tick() {
        // Multiple relations on the same subject within one tick
        // should produce ONE focus entry, not N.
        let policy = Policy::default();
        let (mut hypergraph, built_relations, superseded) =
            run_through_supersede("Nick lived in Brantford for 3 years.", &[], &policy);
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);

        let nick_id = hypergraph.by_name["Nick"][0];
        let nick_subject_entries = hypergraph
            .recent_focus
            .iter()
            .filter(|e| e.element == nick_id && e.attribute == Some(hypergraph.subject_attr))
            .count();
        assert_eq!(
            nick_subject_entries, 1,
            "expected exactly one (Nick, subject) entry; intra-tick dedup",
        );
    }

    #[test]
    fn recent_focus_pushes_target_for_nary_events() {
        let policy = Policy::default();
        let (mut hypergraph, built_relations, superseded) = run_through_supersede(
            "The meeting moved from Tuesday to Friday.",
            &["event", "weekday"],
            &policy,
        );

        // Find the n-ary event's target value before running `hebbian_and_salience`
        // — pattern RE may tag "The meeting" (with article) rather
        // than just "meeting", so we read the actual target Element
        // off the relation instead of guessing the name.
        let nary_target: ElementId = {
            let from_attr = hypergraph.by_name["from"]
                .iter()
                .copied()
                .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
                .unwrap();
            let to_attr = hypergraph.by_name["to"]
                .iter()
                .copied()
                .find(|id| hypergraph.elements[id.0 as usize].polarity == Polarity::Signal)
                .unwrap();
            built_relations
                .minted_relations
                .iter()
                .find_map(|rid| {
                    let r = &hypergraph.relations[rid.0 as usize];
                    let has_from = r.attributes.iter().any(|a| a.name == from_attr);
                    let has_to = r.attributes.iter().any(|a| a.name == to_attr);
                    if !(has_from && has_to) {
                        return None;
                    }
                    r.attributes.iter().find_map(|a| {
                        if a.name == hypergraph.target_attr {
                            match a.value {
                                Term::Element(e) => Some(e),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    })
                })
                .expect("n-ary event with target slot must exist")
        };

        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);

        let target_focus = hypergraph
            .recent_focus
            .iter()
            .filter(|e| e.element == nary_target && e.attribute == Some(hypergraph.target_attr))
            .count();
        assert_eq!(
            target_focus, 1,
            "n-ary event's target should push exactly one target-bound focus entry",
        );
    }

    #[test]
    fn recent_focus_respects_capacity() {
        let policy = Policy {
            recent_focus_capacity: 3,
            ..Default::default()
        };
        let mut hypergraph = load_seed_graph();
        let labels: &[&str] = &[];
        for tick in 0..5 {
            // Fresh sentence per tick keeps subjects distinct so
            // dedup doesn't hide capacity overflow.
            let text = format!("Person{tick} called me.");
            let ext = run_extractors(&text, labels, &policy, &hypergraph, &[]);
            let built_relations = build_relations(&text, &mut hypergraph, &ext, &policy, None);
            let superseded = supersede(&mut hypergraph, &built_relations.minted_relations, &policy);
            let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        }
        assert!(
            hypergraph.recent_focus.len() <= 3,
            "recent_focus must not exceed capacity; len={}",
            hypergraph.recent_focus.len(),
        );
    }

    #[test]
    fn recent_focus_front_is_most_recent() {
        let policy = Policy::default();
        let mut hypergraph = load_seed_graph();

        let text_a = "Alice called me.";
        let ext_a = run_extractors(text_a, &[], &policy, &hypergraph, &[]);
        let s8a = build_relations(text_a, &mut hypergraph, &ext_a, &policy, None);
        let s9a = supersede(&mut hypergraph, &s8a.minted_relations, &policy);
        let _ = hebbian_and_salience(&mut hypergraph, &s8a, &s9a, None, &policy, &[]);

        let text_b = "Bob called me.";
        let ext_b = run_extractors(text_b, &[], &policy, &hypergraph, &[]);
        let s8b = build_relations(text_b, &mut hypergraph, &ext_b, &policy, None);
        let s9b = supersede(&mut hypergraph, &s8b.minted_relations, &policy);
        let _ = hebbian_and_salience(&mut hypergraph, &s8b, &s9b, None, &policy, &[]);

        // Bob should appear ahead of Alice in the deque (most recent
        // first). We look for the first subject-bound entry for each.
        let bob_id = hypergraph.by_name.get("Bob").and_then(|v| v.first().copied());
        let alice_id = hypergraph.by_name.get("Alice").and_then(|v| v.first().copied());
        let (Some(bob), Some(alice)) = (bob_id, alice_id) else {
            panic!("Bob and Alice should both be minted");
        };
        let pos_b = hypergraph
            .recent_focus
            .iter()
            .position(|e| e.element == bob && e.attribute == Some(hypergraph.subject_attr));
        let pos_a = hypergraph
            .recent_focus
            .iter()
            .position(|e| e.element == alice && e.attribute == Some(hypergraph.subject_attr));
        let (Some(pb), Some(pa)) = (pos_b, pos_a) else {
            panic!("both should be in recent_focus");
        };
        assert!(pb < pa, "Bob (newer) should be ahead of Alice (older)");
    }

    /// End-to-end multi-tick integration test:
    /// - tick 1 mints + reinforces; activation should go from 0 to
    ///   ~rate.
    /// - tick 2 reinforces again on the SAME relations; activation
    ///   should climb monotonically toward 1.0.
    /// - support_count climbs by 1 per tick; eventually clears the
    ///   custom promotion gate.
    /// - recent_focus picks up both ticks' focal elements with the
    ///   second tick's entries at the front.
    #[test]
    fn multi_tick_integration() {
        let policy = Policy {
            hebbian_rate: 0.4,
            promotion_min_count: 2,
            promotion_min_diversity: 0,
            attribute_name_maturity_ticks: 0,
            ..Default::default()
        };
        let labels: &[&str] = &["person", "event", "weekday", "role"];
        let mut hypergraph = load_seed_graph();

        // ── Tick 1 ─────────────────────────────────────────────────
        let text1 = "Sarah called me on Tuesday.";
        let ext1 = run_extractors(text1, labels, &policy, &hypergraph, &[]);
        let built_relations_1 = build_relations(text1, &mut hypergraph, &ext1, &policy, None);
        let superseded_1 = supersede(&mut hypergraph, &built_relations_1.minted_relations, &policy);
        let out1 = hebbian_and_salience(&mut hypergraph, &built_relations_1, &superseded_1, None, &policy, &[]);

        assert!(!out1.reinforced.is_empty());
        assert!(out1.focus_pushed >= 1, "tick 1 should push focus entries");

        // Pick a relation that was reinforced and snapshot activation.
        let pick = out1.reinforced[0];
        let act_after_1 = hypergraph.relations[pick.0 as usize].stats.activation;
        let support_after_1 = hypergraph.relations[pick.0 as usize].stats.support_count;
        assert!(act_after_1 > 0.0 && act_after_1 < 1.0);
        // Fresh mint on this tick — no reuse bumps yet, so support_count
        // is still 0. (`focus_success_count` carries the reinforcement
        // signal; see Fix C in the frame-quality-fixes plan.)
        assert_eq!(support_after_1, 0);

        // Snapshot the head of recent_focus for ordering check.
        let head_after_1: Vec<ElementId> =
            hypergraph.recent_focus.iter().take(3).map(|e| e.element).collect();

        // ── Tick 2 ─────────────────────────────────────────────────
        // Re-feed a similar sentence so Sarah element gets reinforced.
        let text2 = "Sarah emailed me again.";
        let ext2 = run_extractors(text2, labels, &policy, &hypergraph, &[]);
        let built_relations_2 = build_relations(text2, &mut hypergraph, &ext2, &policy, None);
        let superseded_2 = supersede(&mut hypergraph, &built_relations_2.minted_relations, &policy);
        let out2 = hebbian_and_salience(&mut hypergraph, &built_relations_2, &superseded_2, None, &policy, &[]);

        // Find the same Sarah-instance-of relation in built_relations_2 — `build_relations`
        // dedups Sarah to the same Element, but mints a fresh
        // instance_of relation. Let's just check that tick 2's
        // reinforcement set produced its own activation bumps.
        for &rid in &out2.reinforced {
            let a = hypergraph.relations[rid.0 as usize].stats.activation;
            assert!(a > 0.0, "tick 2 activation should be > 0 for {rid:?}");
            assert!((0.0..=1.0).contains(&a));
        }

        // recent_focus ordering: tick 2's pushes should be at the
        // FRONT of the deque. Sarah (re-mentioned) won't appear twice
        // (intra-tick dedup applies per-tick, not across ticks — so
        // Sarah appears in BOTH tick 1's batch and tick 2's batch
        // with separate `tick` stamps).
        assert!(
            hypergraph.recent_focus.len() > head_after_1.len(),
            "recent_focus should grow across ticks",
        );

        // ── Promotion gate ─────────────────────────────────────────
        // The dentist NER below has `meeting` at sub-threshold conf
        // (~0.367), so the instance_of relation lands Defeasible.
        // With promotion_min_count=2, two ticks of reinforcement on
        // the same relation should fire promotion. We use a custom
        // multi-tick fixture to make this deterministic.
        let mut hypergraph2 = load_seed_graph();
        let dentist_text = "The meeting moved from Tuesday to Friday.";
        let labels2: &[&str] = &["event", "weekday"];
        let ext_a = run_extractors(dentist_text, labels2, &policy, &hypergraph2, &[]);
        let s8_a = build_relations(dentist_text, &mut hypergraph2, &ext_a, &policy, None);
        let s9_a = supersede(&mut hypergraph2, &s8_a.minted_relations, &policy);
        let _ = hebbian_and_salience(&mut hypergraph2, &s8_a, &s9_a, None, &policy, &[]);

        let instance_of_attr = hypergraph2.by_name["instance_of"][0];
        let defeasible_iotuple = s8_a
            .minted_relations
            .iter()
            .copied()
            .find(|rid| {
                let r = &hypergraph2.relations[rid.0 as usize];
                r.status == RelationStatus::Defeasible
                    && r.attributes.iter().any(|a| a.name == instance_of_attr)
            })
            .expect("expected a Defeasible instance_of relation");

        // After tick 1, support_count=0 (fresh mint, no reuse-bumps
        // yet), gate=2 → still Defeasible.
        assert_eq!(
            hypergraph2.relations[defeasible_iotuple.0 as usize].status,
            RelationStatus::Defeasible,
        );

        // Simulate two re-extractions of the same triple (in real use
        // these come from `build_relations`'s reuse path firing twice).
        hypergraph2.relations[defeasible_iotuple.0 as usize].stats.support_count = 2;
        let out_promo = hebbian_and_salience(&mut hypergraph2, &s8_a, &s9_a, None, &policy, &[]);
        assert_eq!(
            hypergraph2.relations[defeasible_iotuple.0 as usize].status,
            RelationStatus::Asserted,
            "Defeasible relation should promote at support_count=2",
        );
        assert!(out_promo.promoted.contains(&defeasible_iotuple));
    }

    #[test]
    fn salience_stays_at_zero_with_default_policy() {
        let policy = Policy::default();
        let (mut hypergraph, built_relations, superseded) = run_through_supersede("Sarah called me yesterday.", &[], &policy);
        let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        for &rid in &built_relations.minted_relations {
            assert_eq!(
                hypergraph.relations[rid.0 as usize].stats.salience, 0.0,
                "default rate=0 → salience untouched",
            );
        }
    }

    #[test]
    fn activation_caps_at_one() {
        let policy = Policy {
            hebbian_rate: 0.9,
            ..Default::default()
        };
        let (mut hypergraph, built_relations, superseded) = run_through_supersede("Sarah called me yesterday.", &[], &policy);
        // 50 bumps at rate 0.9 from 0.0: x_{n+1} = x + 0.9 * (1 - x)
        // converges very fast. After ~5 iterations we're > 0.999.
        for _ in 0..50 {
            let _ = hebbian_and_salience(&mut hypergraph, &built_relations, &superseded, None, &policy, &[]);
        }
        for &rid in &built_relations.minted_relations {
            let a = hypergraph.relations[rid.0 as usize].stats.activation;
            assert!((0.0..=1.0).contains(&a), "activation out of range: {a}");
            assert!(a > 0.99, "expected near 1.0 after 50 bumps; got {a}");
        }
    }
}
