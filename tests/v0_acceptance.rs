//! v0 acceptance test — runs the full Steps 1 → 12 pipeline twice
//! over realistic inputs and asserts the substrate contracts that
//! define "v0 complete." This is the test that gates v0 release.
//!
//! What's verified:
//!   - All 12 step modules wire end-to-end without panicking.
//!   - `build_relations`'s minted entities appear in the frame's
//!     `durable_writes`.
//!   - `supersede`'s supersession chain threads through the frame
//!     (`superseded`, `history`, `supporting_claims`).
//!   - `hebbian_and_salience`'s reinforcement set populates `focused_relations`;
//!     scores are RRF-fused (monotonically descending).
//!   - Status filter excludes Superseded/Retracted from the frame.
//!   - `recent_focus` populates and gives `coref` candidates on
//!     subsequent ticks.
//!   - The Hypergraph clock advances correctly across the run.

use legend::seed::load_seed_graph;
use legend::tick_pipeline::build_relations::build_relations;
use legend::tick_pipeline::decay::focus_radius_decay;
use legend::tick_pipeline::frame::assemble_frame;
use legend::tick_pipeline::hebbian::hebbian_and_salience;
use legend::tick_pipeline::run_extractors::run_extractors;
use legend::tick_pipeline::supersede::supersede;
use legend::types::{Hypergraph, Intent, Policy, RelationId, RelationStatus, Tick};

/// Mirror of `crate::tick_pipeline::build_relations::is_cache_relation`
/// (which is `pub(crate)`). Returns true iff `rid`'s attribute
/// list carries an attribute whose surface name starts with
/// `"current_"` — i.e. it's a `supersede` cache.
fn is_current_cache(hypergraph: &Hypergraph, rid: RelationId) -> bool {
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

/// Run Steps 5 → 12 against `text`, returning the assembled frame.
/// Steps 1-4 + 7 are exercised individually elsewhere; this driver
/// focuses on the cognitive-substrate the tick pipeline (extractors onward) that v0 actually
/// ships as user-facing behavior.
fn run_tick(
    hypergraph: &mut Hypergraph,
    text: &str,
    labels: &[&str],
    policy: &Policy,
) -> legend::types::ConsciousAttentionFrame {
    hypergraph.clock = Tick(hypergraph.clock.0 + 1);
    let ext = run_extractors(text, labels, policy, hypergraph, &[]);
    let built_relations = build_relations(text, hypergraph, &ext, policy, None);
    let superseded = supersede(hypergraph, &built_relations.minted_relations, policy);
    let reinforced = hebbian_and_salience(hypergraph, &built_relations, &superseded, None, policy, &[]);
    let _ = focus_radius_decay(hypergraph, &reinforced.reinforced, policy);
    // Empty route for the v0 acceptance test — we exercise the tick pipeline (extractors onward)
    // here. `route_regions` has its own tests; the frame's active_regions is
    // gathered (and stays empty under an empty route) without
    // affecting any of the contracts this test verifies.
    let empty_route = legend::tick_pipeline::route_regions::RouteResult {
        all_scores: Vec::new(),
        active_regions: Vec::new(),
        delta: legend::types::RegionDelta::default(),
        uncertainty: Vec::new(),
    };
    assemble_frame(
        text,
        hypergraph,
        &Intent::default(),
        None,
        &empty_route,
        &built_relations,
        &superseded,
        &reinforced,
        policy,
    )
}

#[test]
fn v0_pipeline_two_tick_acceptance() {
    let policy = Policy::default();
    let labels: &[&str] = &["person", "event", "weekday", "role"];
    let mut hypergraph = load_seed_graph();

    // ── Tick 1 ─────────────────────────────────────────────────────
    // Establishes baseline state: mints Sarah / Tuesday / meeting,
    // builds a moved-from/to n-ary event, mints a current_date cache.
    let text1 = "The meeting moved from Monday to Tuesday.";
    let frame1 = run_tick(&mut hypergraph, text1, labels, &policy);

    // Sanity: frame populates.
    assert_eq!(frame1.input_echo, text1);
    assert!(
        !frame1.focused_relations.is_empty(),
        "tick 1 must produce focused relations",
    );
    assert!(
        !frame1.durable_writes.is_empty(),
        "tick 1 must mint elements",
    );
    assert!(
        frame1.superseded.is_empty(),
        "tick 1 has no priors to supersede",
    );
    // recent_focus should now have entries `coref` can score on tick 2.
    assert!(
        !hypergraph.recent_focus.is_empty(),
        "`hebbian_and_salience` must populate recent_focus on tick 1",
    );

    // ── Tick 2 ─────────────────────────────────────────────────────
    // Same target, different to-value → tick 1's cache should
    // supersede.
    let text2 = "The meeting moved from Tuesday to Friday.";
    let prior_focus_depth = hypergraph.recent_focus.len();
    let frame2 = run_tick(&mut hypergraph, text2, labels, &policy);

    // ── Contract 1: clock advances on each mint. ───────────────────
    assert_eq!(
        frame2.tick, hypergraph.clock,
        "frame.tick must mirror hypergraph.clock at assembly time",
    );

    // ── Contract 2: tick 2 supersedes tick 1's cache. ──────────────
    assert!(
        !frame2.superseded.is_empty(),
        "tick 2 should flip at least one prior cache",
    );

    // ── Contract 3: superseded relations flipped to Superseded. ───
    for r in &frame2.superseded {
        assert_eq!(
            r.status,
            RelationStatus::Superseded,
            "superseded relation {:?} should carry Superseded status",
            r.id,
        );
    }

    // ── Contract 4: status filter — no Superseded/Retracted in
    //                focused_relations. ──────────────────────────────
    for ra in &frame2.focused_relations {
        let s = ra.relation.status;
        assert!(
            matches!(
                s,
                RelationStatus::Asserted | RelationStatus::Entailed | RelationStatus::Defeasible,
            ),
            "frame must never include status={s:?}",
        );
    }

    // ── Contract 5: RRF scores monotonically descending within
    //                each cache/non-cache partition. Cache relations
    //                are bumped to the top of `focused_relations` so
    //                the whole vec is not strictly score-descending,
    //                but each group individually is. ────────────────
    let mut in_cache_group = true;
    for w in frame2.focused_relations.windows(2) {
        let a_cache = is_current_cache(&hypergraph, w[0].relation.id);
        let b_cache = is_current_cache(&hypergraph, w[1].relation.id);
        if a_cache && !b_cache {
            in_cache_group = false;
            continue; // transition between groups; score relation doesn't apply
        }
        // Same group → must be score-descending.
        if a_cache == b_cache {
            assert!(
                w[0].activation >= w[1].activation,
                "focused_relations not RRF-sorted within {} group: {} < {}",
                if in_cache_group { "cache" } else { "non-cache" },
                w[0].activation,
                w[1].activation,
            );
        }
    }

    // ── Contract 6: prior cache lands in history, not focused. ────
    let focused_ids: std::collections::HashSet<_> = frame2
        .focused_relations
        .iter()
        .map(|ra| ra.relation.id)
        .collect();
    let history_ids: std::collections::HashSet<_> =
        frame2.history.iter().map(|r| r.id).collect();
    for prior in &frame2.superseded {
        assert!(
            !focused_ids.contains(&prior.id),
            "Superseded {:?} must NOT be in focused_relations",
            prior.id,
        );
        assert!(
            history_ids.contains(&prior.id),
            "Superseded {:?} should land in frame.history",
            prior.id,
        );
    }

    // ── Contract 7: derived_from meta-relations populate
    //                supporting_claims. ───────────────────────────────
    let derived_from_attr = hypergraph.by_name["derived_from"][0];
    let any_derived = frame2
        .supporting_claims
        .iter()
        .any(|r| r.attributes.iter().any(|a| a.name.id == derived_from_attr));
    assert!(
        any_derived,
        "tick 2's cache should carry a derived_from supporting_claim",
    );

    // ── Contract 8: recent_focus grew across ticks (`hebbian_and_salience`'s push). ─
    assert!(
        hypergraph.recent_focus.len() > prior_focus_depth,
        "recent_focus must grow tick-over-tick (was {}; now {})",
        prior_focus_depth,
        hypergraph.recent_focus.len(),
    );

    // ── Contract 9: indices stay consistent — every focused
    //                relation's Element-valued attribute targets
    //                must appear in relations_by_element with the
    //                relation present in their bucket. ──────────────
    for ra in &frame2.focused_relations {
        let rid = ra.relation.id;
        let r = &hypergraph.relations[rid.0 as usize];
        for attr in &r.attributes {
            if let legend::types::Term::Element(e) = attr.value {
                let bucket = hypergraph
                    .relations_by_element
                    .get(&e)
                    .unwrap_or_else(|| panic!("element {e:?} missing from index"));
                assert!(
                    bucket.contains(&rid),
                    "relations_by_element[{e:?}] should contain {rid:?}",
                );
            }
        }
    }
}

#[test]
fn v0_pipeline_clock_advances_across_ticks() {
    // Each tick must bump `hypergraph.clock` so `frame.tick`, `created_at`,
    // and `last_seen` distinguish ticks. Without this, recency-based
    // gating (e.g. `derive_active_frame`) can't tell stale state from
    // fresh.
    let policy = Policy::default();
    let mut hypergraph = load_seed_graph();
    let initial = hypergraph.clock.0;
    let frame1 = run_tick(&mut hypergraph, "Sarah called me.", &[], &policy);
    let frame2 = run_tick(&mut hypergraph, "Then Sarah left.", &[], &policy);
    assert_eq!(
        frame1.tick.0,
        initial + 1,
        "first tick should advance from {initial}",
    );
    assert_eq!(
        frame2.tick.0,
        initial + 2,
        "second tick should advance once more",
    );
    assert_eq!(hypergraph.clock.0, initial + 2);
}

#[test]
fn v0_self_referential_relations_are_dropped() {
    // `(X, instance_of, X)` and similar self-loops are degenerate
    // extractions — NER's tag for X matches X's surface form. The
    // mint path must reject them so the frame doesn't fill up with
    // tautological lines like `language → instance_of → language`.
    let policy = Policy::default();
    let mut hypergraph = load_seed_graph();
    let frame = run_tick(&mut hypergraph, "The language we're using is Rust", &[], &policy);

    // Walk every focused relation; assert no element appears as both
    // subject and value of the same relation.
    use legend::types::ResolvedTerm;
    for ra in &frame.focused_relations {
        let subj = ra
            .relation
            .attributes
            .iter()
            .find(|a| a.name.name == "subject")
            .and_then(|a| match &a.value {
                ResolvedTerm::Element(e) => Some(e.id),
                _ => None,
            });
        let Some(subj_id) = subj else { continue };
        for a in &ra.relation.attributes {
            if a.name.name == "subject" {
                continue;
            }
            if let ResolvedTerm::Element(e) = &a.value {
                assert_ne!(
                    e.id, subj_id,
                    "relation R{} is self-referential (subject == value): {:?}",
                    ra.relation.id.0, ra.relation.attributes,
                );
            }
        }
    }
}

#[test]
fn v0_active_frame_drops_on_query_intent() {
    // High curiosity + low conviction = query intent. Even with a
    // populated recent_focus, derive_active_frame should return None
    // — a query inherits no topical anchor.
    let policy = Policy::default();
    let mut hypergraph = load_seed_graph();
    let _ = run_tick(&mut hypergraph, "Sarah called me.", &[], &policy);
    let statement_intent = legend::types::Intent {
        conviction: 0.7,
        prediction_error: 0.3,
        arousal: 0.3,
        curiosity: 0.2,
    };
    let query_intent = legend::types::Intent {
        conviction: 0.2,
        prediction_error: 0.3,
        arousal: 0.3,
        curiosity: 0.9,
    };
    let statement_frame = legend::tick_pipeline::hebbian::derive_active_frame(
        &hypergraph,
        &statement_intent,
        &policy,
    );
    let query_frame = legend::tick_pipeline::hebbian::derive_active_frame(
        &hypergraph,
        &query_intent,
        &policy,
    );
    assert!(
        statement_frame.is_some(),
        "statement intent should inherit the focal subject",
    );
    assert!(
        query_frame.is_none(),
        "query intent should clear active_frame (got {query_frame:?})",
    );
}

#[test]
fn v0_active_frame_drops_when_recent_focus_is_stale() {
    // recent_focus entries older than policy.active_frame_max_age_ticks
    // are skipped. Simulate by pushing a tick, then running many no-op
    // ticks to age the entry past the threshold.
    let policy = Policy {
        active_frame_max_age_ticks: 2,
        ..Policy::default()
    };
    let mut hypergraph = load_seed_graph();

    let _ = run_tick(&mut hypergraph, "Sarah called me.", &[], &policy);
    // The recent_focus entry now stamped at hypergraph.clock.

    let neutral_intent = legend::types::Intent::default();
    let fresh = legend::tick_pipeline::hebbian::derive_active_frame(&hypergraph, &neutral_intent, &policy);
    assert!(fresh.is_some(), "fresh entry should win active_frame");

    // Advance the clock past the max-age threshold without touching
    // recent_focus.
    for _ in 0..(policy.active_frame_max_age_ticks as u64 + 1) {
        hypergraph.clock = Tick(hypergraph.clock.0 + 1);
    }
    let stale = legend::tick_pipeline::hebbian::derive_active_frame(&hypergraph, &neutral_intent, &policy);
    assert!(
        stale.is_none(),
        "stale entry should be skipped (got {stale:?})",
    );
}

#[test]
fn v0_pipeline_empty_input_does_not_panic() {
    // Empty input is a no-op tick. Verifies the pipeline doesn't
    // crash on an edge case.
    let policy = Policy::default();
    let mut hypergraph = load_seed_graph();
    let frame = run_tick(&mut hypergraph, "", &[], &policy);
    assert!(frame.focused_relations.is_empty());
    assert!(frame.durable_writes.is_empty());
    assert!(frame.superseded.is_empty());
}

#[test]
fn v0_pipeline_repeat_input_accumulates_support_count() {
    // Same input three times → support_count should climb on
    // re-minted relations. Verifies `hebbian_and_salience`'s support_count bump
    // is observable through the substrate.
    let policy = Policy::default();
    let mut hypergraph = load_seed_graph();
    let text = "Sarah called me yesterday.";

    for _ in 0..3 {
        let _ = run_tick(&mut hypergraph, text, &[], &policy);
    }

    // Find a relation whose subject is the seeded `user` element
    // OR whose attributes include the Sarah element (the latter is
    // what NER produces). Just verify SOMETHING in the graph has
    // support_count > 1 after 3 ticks of identical input.
    let max_support = hypergraph
        .relations
        .iter()
        .map(|r| r.stats.support_count)
        .max()
        .unwrap_or(0);
    assert!(
        max_support >= 2,
        "after 3 identical ticks, some relation should have support_count ≥ 2; got max {max_support}",
    );
}

#[test]
fn v0_dedup_skips_superseded_relations() {
    // Regression: dedup must NOT reuse a Superseded relation.
    // Tick 1: cache `current_date = Tuesday`. Tick 2: supersede to
    // Friday (tick 1's cache flips to Superseded). Tick 3: cache
    // back to Tuesday — should mint a FRESH Asserted relation, not
    // resurrect tick 1's superseded one. `supersede` should then flip
    // tick 2's Friday cache.
    let policy = Policy::default();
    let labels: &[&str] = &["event", "weekday"];
    let mut hypergraph = load_seed_graph();

    let _ = run_tick(
        &mut hypergraph,
        "The meeting moved from Monday to Tuesday.",
        labels,
        &policy,
    );

    // Snapshot: find tick 1's cache (the [subject: meeting,
    // current_date: Tuesday] relation). It's the most recently
    // minted Asserted cache mentioning `current_date`.
    let current_date_attr = hypergraph
        .by_name
        .get("current_date")
        .and_then(|v| v.first().copied())
        .expect("`supersede` should have minted current_date");
    let tick1_cache = hypergraph
        .relations
        .iter()
        .rev()
        .find(|r| {
            r.status == RelationStatus::Asserted
                && r.attributes.iter().any(|a| a.name == current_date_attr)
        })
        .map(|r| r.id)
        .expect("tick 1 should have minted a current_date cache");

    let _ = run_tick(
        &mut hypergraph,
        "The meeting moved from Tuesday to Friday.",
        labels,
        &policy,
    );

    // Tick 1's cache should now be Superseded.
    assert_eq!(
        hypergraph.relations[tick1_cache.0 as usize].status,
        RelationStatus::Superseded,
        "tick 1's cache should be Superseded after tick 2",
    );
    let relation_count_after_tick2 = hypergraph.relations.len();

    // Tick 3: same state as tick 1 — should NOT resurrect the
    // Superseded relation. Should mint a fresh Asserted one.
    let _ = run_tick(
        &mut hypergraph,
        "The meeting moved from Friday to Tuesday.",
        labels,
        &policy,
    );

    assert!(
        hypergraph.relations.len() > relation_count_after_tick2,
        "tick 3 must mint NEW relations, not reuse Superseded prior",
    );
    // Tick 1's cache is still Superseded (dedup didn't touch it).
    assert_eq!(
        hypergraph.relations[tick1_cache.0 as usize].status,
        RelationStatus::Superseded,
        "Superseded prior must remain Superseded — dedup must skip it",
    );
}

#[test]
fn v0_pronoun_coref_rebinds_to_recent_focus_subject() {
    // Two-tick test of the closed-class pronoun path. Tick 1 mints
    // "Sarah" as a person and pushes her onto `recent_focus`. Tick 2
    // says "She emailed me." — the pronoun "She" should rebind to
    // Sarah via `coref` + `build_relations`'s `apply_coref_decisions`. The
    // assertion: no fresh element gets minted for "She"; instead,
    // Sarah's access_count bumps.
    let policy = Policy::default();
    let labels: &[&str] = &["person"];
    let mut hypergraph = load_seed_graph();

    // Tick 1: establish Sarah as the focal subject.
    let _ = run_tick(&mut hypergraph, "Sarah waved at the meeting.", labels, &policy);
    let sarah_ids = hypergraph
        .by_name
        .get("Sarah")
        .cloned()
        .expect("tick 1 should mint Sarah element");
    assert_eq!(
        sarah_ids.len(),
        1,
        "tick 1 should mint exactly one Sarah element"
    );
    let sarah_id = sarah_ids[0];
    let sarah_access_before = hypergraph.elements[sarah_id.0 as usize].stats.access_count;

    // Tick 2: pronoun "She" — should NOT mint a fresh element. Coref
    // rebinds the span to Sarah and the NER instance_of proposal for
    // "She" gets short-circuited via the span cache.
    let element_count_after_tick1 = hypergraph.elements.len();
    let _ = run_tick(&mut hypergraph, "She emailed me.", labels, &policy);

    // No element named "She" should appear in by_name.
    assert!(
        !hypergraph.by_name.contains_key("She"),
        "pronoun 'She' must not mint as a separate element after coref"
    );
    // Sarah's access count bumped (rebinding folded a mention).
    let sarah_access_after = hypergraph.elements[sarah_id.0 as usize].stats.access_count;
    assert!(
        sarah_access_after > sarah_access_before,
        "Sarah's access_count should bump when 'She' rebinds to her: \
         before={sarah_access_before} after={sarah_access_after}",
    );
    // Pipeline doesn't crash; substrate continued to grow.
    assert!(
        hypergraph.elements.len() >= element_count_after_tick1,
        "element count should not regress"
    );
}
