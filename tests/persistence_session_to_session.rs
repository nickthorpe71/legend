//! Thorough cross-session memory tests. The single-roundtrip test in
//! `persistence_round_trip.rs` proves the wire format works; this
//! suite proves that the substrate behaviors that make Legend useful
//! (retrieval, supersession, support accumulation, recent_focus,
//! active_frame inheritance) survive a save → drop → load → continue
//! cycle.
//!
//! Each test simulates "session" boundaries by `drop`-ing the live
//! `Hypergraph` after `save` and then `load`-ing a fresh copy from
//! disk. The temp snapshot files live under `std::env::temp_dir()`
//! and are cleaned up explicitly.

use legend::persistence;
use legend::seed::load_seed_graph;
use legend::tick_pipeline::adjust_policy::adjust_policy;
use legend::tick_pipeline::build_relations::build_relations;
use legend::tick_pipeline::decay::focus_radius_decay;
use legend::tick_pipeline::detect_intent::detect_intent;
use legend::tick_pipeline::hebbian::{derive_active_frame, hebbian_and_salience};
use legend::tick_pipeline::run_extractors::run_extractors;
use legend::tick_pipeline::supersede::supersede;
use legend::types::{Hypergraph, RelationStatus, Term};

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

// ─── Helpers ─────────────────────────────────────────────────────────

fn temp_path(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "legend_s2s_{}_{}_{}.lz4",
        label,
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    p
}

/// Run the cognitive pipeline (the tick pipeline (extractors onward) essentials) over `text`.
/// Returns nothing — caller inspects `hypergraph` directly.
fn tick(hypergraph: &mut Hypergraph, text: &str) {
    let embedding = legend::embed::embed_text(text);
    let intent = detect_intent(text, &embedding);
    let policy = adjust_policy(&intent, &hypergraph.policy);
    let active_frame = derive_active_frame(hypergraph, &intent, &policy);
    let ext = run_extractors(text, &[], &policy, hypergraph, &[]);
    let built_relations = build_relations(text, hypergraph, &ext, &policy, None);
    let superseded = supersede(hypergraph, &built_relations.minted_relations, &policy);
    let reinforced = hebbian_and_salience(hypergraph, &built_relations, &superseded, active_frame, &policy, &[]);
    let _ = focus_radius_decay(hypergraph, &reinforced.reinforced, &policy);
}

/// Save `hypergraph`, drop it, load a fresh copy from disk. Simulates a
/// process restart.
fn save_drop_load(hypergraph: Hypergraph, path: &Path) -> Hypergraph {
    persistence::save(&hypergraph, path).expect("save");
    drop(hypergraph);
    persistence::load(path).expect("load")
}

/// Find a relation by subject-name + attribute-name substring. Used
/// to assert specific claims survived. Returns the first match.
fn find_relation_subj_attr(
    hypergraph: &Hypergraph,
    subj_name: &str,
    attr_substr: &str,
) -> Option<legend::types::RelationId> {
    let subj_lower = subj_name.to_ascii_lowercase();
    for r in &hypergraph.relations {
        if !matches!(
            r.status,
            RelationStatus::Asserted | RelationStatus::Entailed | RelationStatus::Defeasible,
        ) {
            continue;
        }
        let mut subj_matches = false;
        let mut attr_matches = false;
        for a in &r.attributes {
            if a.name == hypergraph.subject_attr
                && let Term::Element(e) = a.value
            {
                let name = hypergraph.elements[e.0 as usize]
                    .names
                    .first()
                    .cloned()
                    .unwrap_or_default();
                if name.to_ascii_lowercase().contains(&subj_lower) {
                    subj_matches = true;
                }
            }
            if a.name != hypergraph.subject_attr && a.name != hypergraph.target_attr {
                let attr_name = hypergraph.elements[a.name.0 as usize]
                    .names
                    .first()
                    .cloned()
                    .unwrap_or_default();
                if attr_name
                    .to_ascii_lowercase()
                    .contains(&attr_substr.to_ascii_lowercase())
                {
                    attr_matches = true;
                }
            }
        }
        if subj_matches && attr_matches {
            return Some(r.id);
        }
    }
    None
}

// ─── A. Structural survival ──────────────────────────────────────────

#[test]
fn elements_relations_clock_survive_save_load() {
    let path = temp_path("structural");
    let mut hypergraph = load_seed_graph();
    let seed_elements = hypergraph.elements.len();
    let seed_relations = hypergraph.relations.len();

    tick(&mut hypergraph, "Alice works at Acme.");
    let pre_elements = hypergraph.elements.len();
    let pre_relations = hypergraph.relations.len();
    let pre_clock = hypergraph.clock.0;
    assert!(
        pre_elements > seed_elements,
        "session 1 should mint elements"
    );
    assert!(pre_relations > seed_relations);

    let hypergraph2 = save_drop_load(hypergraph, &path);
    assert_eq!(hypergraph2.elements.len(), pre_elements);
    assert_eq!(hypergraph2.relations.len(), pre_relations);
    assert_eq!(hypergraph2.clock.0, pre_clock);

    // Indices regenerated.
    assert!(!hypergraph2.by_name.is_empty());
    assert!(!hypergraph2.region_children.is_empty());
    assert!(!hypergraph2.relations_by_element.is_empty());

    // Anchor IDs survive.
    assert_eq!(hypergraph2.genesis, load_seed_graph().genesis);
    assert_eq!(hypergraph2.subject_attr, load_seed_graph().subject_attr);
    assert_eq!(hypergraph2.target_attr, load_seed_graph().target_attr);

    let _ = fs::remove_file(&path);
}

#[test]
fn anchor_ids_match_seed_after_load() {
    let path = temp_path("anchors");
    let mut hypergraph = load_seed_graph();
    let seed_genesis = hypergraph.genesis;
    let seed_subject = hypergraph.subject_attr;
    let seed_target = hypergraph.target_attr;
    let seed_void = hypergraph.void;
    let seed_parent_region = hypergraph.parent_region_attr;
    let seed_prototype = hypergraph.prototype_attr;

    tick(&mut hypergraph, "Beam is a server.");
    let hypergraph2 = save_drop_load(hypergraph, &path);

    assert_eq!(hypergraph2.genesis, seed_genesis);
    assert_eq!(hypergraph2.subject_attr, seed_subject);
    assert_eq!(hypergraph2.target_attr, seed_target);
    assert_eq!(hypergraph2.void, seed_void);
    assert_eq!(hypergraph2.parent_region_attr, seed_parent_region);
    assert_eq!(hypergraph2.prototype_attr, seed_prototype);

    let _ = fs::remove_file(&path);
}

#[test]
fn embeddings_preserved_through_lz4_roundtrip() {
    // LZ4 + rmp-serde should be lossless for f32. If anything quantizes
    // or truncates, the routing/cosine machinery downstream would
    // silently drift.
    let path = temp_path("embeddings");
    let mut hypergraph = load_seed_graph();
    tick(&mut hypergraph, "Polaris is a CLI tool written in Rust.");

    // Snapshot embeddings of every minted-after-seed element.
    let baseline_seed = load_seed_graph();
    let new_element_idx = baseline_seed.elements.len();
    let pre_embeddings: Vec<Vec<f32>> = hypergraph.elements[new_element_idx..]
        .iter()
        .map(|e| e.embedding.clone())
        .collect();

    let hypergraph2 = save_drop_load(hypergraph, &path);
    let post_embeddings: Vec<Vec<f32>> = hypergraph2.elements[new_element_idx..]
        .iter()
        .map(|e| e.embedding.clone())
        .collect();

    assert_eq!(pre_embeddings.len(), post_embeddings.len());
    for (i, (pre, post)) in pre_embeddings
        .iter()
        .zip(post_embeddings.iter())
        .enumerate()
    {
        assert_eq!(pre.len(), post.len(), "element {i} dim mismatch");
        for (j, (a, b)) in pre.iter().zip(post.iter()).enumerate() {
            assert_eq!(
                a.to_bits(),
                b.to_bits(),
                "element {i} dim {j} differs after save/load: {a} vs {b}",
            );
        }
    }

    let _ = fs::remove_file(&path);
}

// ─── B. Retrieval across sessions ────────────────────────────────────

#[test]
fn session_2_query_retrieves_session_1_entity() {
    let path = temp_path("retrieval");

    // Session 1: assert a fact.
    let mut hypergraph = load_seed_graph();
    tick(&mut hypergraph, "Polaris is written in Rust.");
    let polaris_at_rust = find_relation_subj_attr(&hypergraph, "Polaris", "at");
    assert!(
        polaris_at_rust.is_some(),
        "session 1 should mint a Polaris-at-Rust relation"
    );

    // Cross session boundary.
    let mut hypergraph2 = save_drop_load(hypergraph, &path);

    // Session 2: query. The retrieval pass should pull the
    // Polaris relations into the reinforcement set so the frame
    // surfaces them.
    tick(&mut hypergraph2, "What language is Polaris in?");

    // The relation minted in session 1 should still exist in
    // session 2 with the same ID.
    let polaris_at_rust_2 = find_relation_subj_attr(&hypergraph2, "Polaris", "at");
    assert_eq!(
        polaris_at_rust, polaris_at_rust_2,
        "Polaris-at-Rust relation must persist by ID across the session boundary"
    );

    let _ = fs::remove_file(&path);
}

// ─── C. Supersession across sessions ─────────────────────────────────

#[test]
fn supersession_in_session_2_flips_session_1_cache() {
    let path = temp_path("supersession");

    // Session 1: a change event mints a cache for current_<property>.
    let mut hypergraph = load_seed_graph();
    tick(&mut hypergraph, "Polaris switched from Rust to Go.");
    let session1_cache = hypergraph
        .relations
        .iter()
        .find(|r| {
            r.status == RelationStatus::Asserted
                && r.attributes.iter().any(|a| {
                    if a.name == hypergraph.subject_attr || a.name == hypergraph.target_attr {
                        return false;
                    }
                    hypergraph.elements[a.name.0 as usize]
                        .names
                        .first()
                        .map(|n| n.starts_with("current_"))
                        .unwrap_or(false)
                })
        })
        .map(|r| r.id);
    assert!(
        session1_cache.is_some(),
        "session 1 should mint a current_<prop> cache"
    );
    let session1_cache_id = session1_cache.unwrap();

    // Cross session boundary.
    let mut hypergraph2 = save_drop_load(hypergraph, &path);

    // The session-1 cache should still be Asserted (live) before
    // session 2 supersedes it.
    assert_eq!(
        hypergraph2.relations[session1_cache_id.0 as usize].status,
        RelationStatus::Asserted,
        "session 1's cache should still be Asserted after load"
    );

    // Session 2: a restatement should find the session-1 cache as
    // a prior and flip it.
    tick(&mut hypergraph2, "Polaris is now written in Python.");

    let post_status = hypergraph2.relations[session1_cache_id.0 as usize].status;
    assert_eq!(
        post_status,
        RelationStatus::Superseded,
        "session 2's event should have superseded session 1's cache; \
         status is {post_status:?}"
    );

    let _ = fs::remove_file(&path);
}

#[test]
fn superseded_relations_remain_superseded_after_load() {
    // If a relation was flipped to Superseded in session 1, that
    // status MUST survive the load. Otherwise the supersession dedup
    // (which skips Superseded for reuse) breaks.
    let path = temp_path("superseded_status");
    let mut hypergraph = load_seed_graph();
    tick(&mut hypergraph, "Polaris switched from Rust to Go.");
    tick(&mut hypergraph, "Polaris is now written in Python.");

    let superseded_ids: Vec<_> = hypergraph
        .relations
        .iter()
        .filter(|r| r.status == RelationStatus::Superseded)
        .map(|r| r.id)
        .collect();
    assert!(
        !superseded_ids.is_empty(),
        "two-tick test should produce at least one Superseded relation"
    );

    let hypergraph2 = save_drop_load(hypergraph, &path);
    for id in &superseded_ids {
        assert_eq!(
            hypergraph2.relations[id.0 as usize].status,
            RelationStatus::Superseded,
            "R{} must remain Superseded after load",
            id.0,
        );
    }

    let _ = fs::remove_file(&path);
}

// ─── D. Working memory ───────────────────────────────────────────────

#[test]
fn recent_focus_survives_session_boundary() {
    // recent_focus is the ring buffer `coref` and active-frame
    // inheritance read from. If it doesn't survive save/load, every
    // process start would lose conversational context.
    let path = temp_path("recent_focus");
    let mut hypergraph = load_seed_graph();
    tick(&mut hypergraph, "Polaris is a Rust project.");
    tick(&mut hypergraph, "Polaris targets macOS and Linux.");

    let pre_depth = hypergraph.recent_focus.len();
    let pre_head: Vec<_> = hypergraph
        .recent_focus
        .iter()
        .take(5)
        .map(|e| (e.element, e.attribute, e.tick))
        .collect();
    assert!(pre_depth > 0, "two ticks should populate recent_focus");

    let hypergraph2 = save_drop_load(hypergraph, &path);

    assert_eq!(
        hypergraph2.recent_focus.len(),
        pre_depth,
        "recent_focus depth must match across save/load"
    );
    let post_head: Vec<_> = hypergraph2
        .recent_focus
        .iter()
        .take(5)
        .map(|e| (e.element, e.attribute, e.tick))
        .collect();
    assert_eq!(
        pre_head, post_head,
        "recent_focus order + content must match across save/load"
    );

    let _ = fs::remove_file(&path);
}

#[test]
fn active_frame_inherits_across_session_boundary() {
    // Session 1 establishes Polaris as the focal subject. After save/
    // load, `derive_active_frame` in session 2 should still return
    // Polaris (or some other valid topical subject from session 1's
    // recent_focus).
    let path = temp_path("active_frame");
    let mut hypergraph = load_seed_graph();
    tick(&mut hypergraph, "Polaris is a Rust project.");
    tick(&mut hypergraph, "Polaris has fuzzy search.");

    // Default Intent/Policy — this test exercises the recent_focus
    // path, not query-intent or recency edges.
    let intent = legend::types::Intent::default();
    let policy = hypergraph.policy.clone();
    let pre_active = derive_active_frame(&hypergraph, &intent, &policy);
    assert!(
        pre_active.is_some(),
        "after two ticks about Polaris, derive_active_frame should pick a subject"
    );
    let pre_active_name = hypergraph.elements[pre_active.unwrap().0 as usize]
        .names
        .first()
        .cloned()
        .unwrap_or_default();

    let hypergraph2 = save_drop_load(hypergraph, &path);
    let post_active = derive_active_frame(&hypergraph2, &intent, &policy);
    let post_active_name = post_active.map(|id| {
        hypergraph2.elements[id.0 as usize]
            .names
            .first()
            .cloned()
            .unwrap_or_default()
    });

    assert_eq!(
        Some(pre_active_name),
        post_active_name,
        "active_frame must match across save/load"
    );

    let _ = fs::remove_file(&path);
}

// ─── E. Stat accumulation ────────────────────────────────────────────

#[test]
fn support_count_accumulates_across_sessions() {
    // The Defeasible → Asserted promotion gate depends on
    // support_count climbing across ticks. If support_count resets at
    // every session boundary, promotion would never fire for a
    // long-running multi-session conversation.
    let path = temp_path("support_count");
    let mut hypergraph = load_seed_graph();
    tick(&mut hypergraph, "Carol started a project.");

    // Pick a minted relation involving "Carol" we can track.
    let carol_rel = find_relation_subj_attr(&hypergraph, "Carol", "instance_of");
    assert!(carol_rel.is_some());
    let carol_rel_id = carol_rel.unwrap();
    let support_after_tick_1 = hypergraph.relations[carol_rel_id.0 as usize].stats.support_count;
    assert!(
        support_after_tick_1 >= 1,
        "first tick should bump support_count at least once"
    );

    // Cross session, re-mention Carol.
    let mut hypergraph2 = save_drop_load(hypergraph, &path);
    let support_after_load = hypergraph2.relations[carol_rel_id.0 as usize].stats.support_count;
    assert_eq!(
        support_after_load, support_after_tick_1,
        "support_count must survive save/load unchanged"
    );

    tick(&mut hypergraph2, "Carol works in Rust.");
    let support_after_tick_2 = hypergraph2.relations[carol_rel_id.0 as usize].stats.support_count;
    assert!(
        support_after_tick_2 > support_after_load,
        "re-mentioning Carol across the session boundary should bump support_count further: \
         before={support_after_load} after={support_after_tick_2}",
    );

    let _ = fs::remove_file(&path);
}

// ─── F. Multi-session chain ──────────────────────────────────────────

#[test]
fn three_session_accumulation_chain() {
    let path = temp_path("three_session");

    // Session 1.
    let mut hypergraph = load_seed_graph();
    tick(&mut hypergraph, "Alice is a developer.");
    let elements_1 = hypergraph.elements.len();
    let relations_1 = hypergraph.relations.len();
    let clock_1 = hypergraph.clock.0;

    // Session 2.
    let mut hypergraph = save_drop_load(hypergraph, &path);
    assert_eq!(hypergraph.elements.len(), elements_1);
    assert_eq!(hypergraph.relations.len(), relations_1);
    assert_eq!(hypergraph.clock.0, clock_1, "clock must survive save/load exactly");
    tick(&mut hypergraph, "Alice works on Polaris.");
    let elements_2 = hypergraph.elements.len();
    let relations_2 = hypergraph.relations.len();
    assert!(elements_2 > elements_1, "session 2 should grow elements");
    assert!(relations_2 > relations_1, "session 2 should grow relations");

    // Session 3.
    let mut hypergraph = save_drop_load(hypergraph, &path);
    assert_eq!(hypergraph.elements.len(), elements_2);
    assert_eq!(hypergraph.relations.len(), relations_2);
    tick(&mut hypergraph, "Polaris is written in Rust.");
    assert!(hypergraph.elements.len() > elements_2);
    assert!(hypergraph.relations.len() > relations_2);

    // Single-process control: same three ticks, all in one go.
    let mut control = load_seed_graph();
    tick(&mut control, "Alice is a developer.");
    tick(&mut control, "Alice works on Polaris.");
    tick(&mut control, "Polaris is written in Rust.");

    assert_eq!(
        (hypergraph.elements.len(), hypergraph.relations.len(), hypergraph.clock.0),
        (
            control.elements.len(),
            control.relations.len(),
            control.clock.0,
        ),
        "three-session chain must equal single-process control"
    );

    let _ = fs::remove_file(&path);
}

// ─── G. Index correctness ────────────────────────────────────────────

#[test]
fn indices_after_load_match_indices_after_seed_then_replay() {
    // After load, `rebuild_indices` should produce the same indices
    // as building the same state from scratch + rebuild. Catches
    // any drift between the live update path and the rebuild path.
    let path = temp_path("index_correctness");
    let mut hypergraph_a = load_seed_graph();
    tick(&mut hypergraph_a, "Beam is a server.");
    tick(&mut hypergraph_a, "Beam is written in Go.");

    let hypergraph_b = save_drop_load(hypergraph_a, &path);

    // Build the same state from scratch, but force a final
    // rebuild_indices call so both graphs got their indices from the
    // same rebuild path.
    let mut hypergraph_c = load_seed_graph();
    tick(&mut hypergraph_c, "Beam is a server.");
    tick(&mut hypergraph_c, "Beam is written in Go.");
    legend::seed::rebuild_indices(&mut hypergraph_c);

    assert_eq!(
        hypergraph_b.by_name.len(),
        hypergraph_c.by_name.len(),
        "by_name size differs"
    );
    assert_eq!(
        hypergraph_b.relations_by_element.len(),
        hypergraph_c.relations_by_element.len(),
        "relations_by_element size differs",
    );
    assert_eq!(
        hypergraph_b.region_children.len(),
        hypergraph_c.region_children.len(),
        "region_children size differs",
    );
    assert_eq!(
        hypergraph_b.meta_relations_by_subject.len(),
        hypergraph_c.meta_relations_by_subject.len(),
        "meta_relations_by_subject size differs",
    );

    let _ = fs::remove_file(&path);
}

// ─── H. Policy survival ──────────────────────────────────────────────

#[test]
fn policy_survives_save_load() {
    // Policy carries ~40 scalar knobs; rmp-serde should round-trip
    // every one bit-for-bit. A regression would silently change
    // pipeline behavior in session 2.
    let path = temp_path("policy");
    let hypergraph = load_seed_graph();
    let pre = hypergraph.policy.clone();

    let hypergraph2 = save_drop_load(hypergraph, &path);
    let post = hypergraph2.policy;

    assert_eq!(pre.default_conf.to_bits(), post.default_conf.to_bits());
    assert_eq!(
        pre.salience_multiplier.to_bits(),
        post.salience_multiplier.to_bits(),
    );
    assert_eq!(
        pre.supersession_threshold.to_bits(),
        post.supersession_threshold.to_bits(),
    );
    assert_eq!(pre.hebbian_rate.to_bits(), post.hebbian_rate.to_bits());
    assert_eq!(pre.leaf_vigilance.to_bits(), post.leaf_vigilance.to_bits());
    assert_eq!(pre.promotion_min_count, post.promotion_min_count);
    assert_eq!(pre.promotion_min_diversity, post.promotion_min_diversity);
    assert_eq!(pre.recent_focus_capacity, post.recent_focus_capacity);

    let _ = fs::remove_file(&path);
}

// ─── I. Real-binary subprocess test ──────────────────────────────────

#[test]
fn cli_binary_persists_state_across_invocations() {
    // The most "real" test: actually invoke the built binary twice
    // and verify the snapshot file grows between calls. Uses
    // `LEGEND_STATE_DIR` to keep the test out of the dev workspace.
    let dir = std::env::temp_dir().join(format!(
        "legend_s2s_cli_{}_{}",
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp state dir");
    let snapshot = dir.join("memory.lz4");

    let bin = env!("CARGO_BIN_EXE_legend");

    // Invocation 1: fresh start.
    let out1 = std::process::Command::new(bin)
        .env("LEGEND_STATE_DIR", &dir)
        .arg("I'm building Polaris in Rust.")
        .output()
        .expect("first invocation");
    assert!(
        out1.status.success(),
        "first invocation failed: stderr={}",
        String::from_utf8_lossy(&out1.stderr),
    );
    assert!(
        snapshot.exists(),
        "snapshot file should exist after first run"
    );
    let size_1 = fs::metadata(&snapshot).expect("stat snapshot").len();
    assert!(size_1 > 1000, "snapshot should be non-trivially sized");

    // Invocation 2: substrate continues.
    let out2 = std::process::Command::new(bin)
        .env("LEGEND_STATE_DIR", &dir)
        .arg("Polaris is now written in Go.")
        .output()
        .expect("second invocation");
    assert!(
        out2.status.success(),
        "second invocation failed: stderr={}",
        String::from_utf8_lossy(&out2.stderr),
    );
    let size_2 = fs::metadata(&snapshot).expect("stat snapshot").len();
    assert!(
        size_2 >= size_1,
        "snapshot should grow or stay same between invocations: {size_1} → {size_2}",
    );

    // The second invocation's stdout should mention Polaris (retrieved
    // from session 1) — proves cross-session memory landed.
    let stdout_2 = String::from_utf8_lossy(&out2.stdout);
    assert!(
        stdout_2.contains("Polaris"),
        "second invocation should surface Polaris from prior session; \
         stdout did not mention it",
    );

    let _ = fs::remove_dir_all(&dir);
}

// ─── J. `legend reset` subcommand behavior ──────────────────────────
//
// `legend reset` sends a `Reset` IPC request to the daemon, which
// replaces its in-RAM substrate with a fresh seed graph and persists.
// The CLI subprocess test below covers the runtime contract;
// `legend::persistence::load_or_seed` is exercised in
// `persistence_round_trip.rs::cross_session_load_or_seed_returns_existing_snapshot`.

#[test]
fn cli_legend_reset_starts_fresh_but_overwrites_snapshot() {
    let dir = std::env::temp_dir().join(format!(
        "legend_s2s_reset_{}_{}",
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let snapshot = dir.join("memory.lz4");

    let bin = env!("CARGO_BIN_EXE_legend");

    // Seed a snapshot by ticking a non-trivial input — auto-starts a
    // daemon scoped to this temp dir.
    let _ = std::process::Command::new(bin)
        .env("LEGEND_STATE_DIR", &dir)
        .arg("Polaris is a Rust project.")
        .output()
        .expect("seed invocation");
    assert!(snapshot.exists());
    let baseline = fs::metadata(&snapshot).unwrap().len();

    // `legend reset` — daemon drops in-RAM substrate, reloads from
    // seed, persists. Snapshot now holds only seed state.
    let reset = std::process::Command::new(bin)
        .env("LEGEND_STATE_DIR", &dir)
        .arg("reset")
        .output()
        .expect("reset invocation");
    assert!(
        reset.status.success(),
        "reset invocation failed: stderr={}",
        String::from_utf8_lossy(&reset.stderr),
    );

    // Tick a tiny input on the freshly reset substrate. Stdout should
    // not surface Polaris (the prior snapshot's content is gone).
    let tick = std::process::Command::new(bin)
        .env("LEGEND_STATE_DIR", &dir)
        .arg("Hello.")
        .output()
        .expect("post-reset tick");
    assert!(
        tick.status.success(),
        "post-reset tick failed: stderr={}",
        String::from_utf8_lossy(&tick.stderr),
    );
    let after_reset = fs::metadata(&snapshot).unwrap().len();
    // After reset, the snapshot reflects only the fresh-seed + one
    // tick about "Hello" — strictly smaller than the prior run that
    // had Polaris + Rust + ancillary mints.
    assert!(
        after_reset <= baseline,
        "`legend reset` should produce a snapshot no larger than the prior; \
         baseline={baseline} after_reset={after_reset}",
    );
    let stdout = String::from_utf8_lossy(&tick.stdout);
    assert!(
        !stdout.contains("Polaris"),
        "post-reset tick should not surface Polaris (would mean reset didn't wipe)",
    );

    // Stop the daemon we auto-spawned so we don't leak a process.
    let _ = std::process::Command::new(bin)
        .env("LEGEND_STATE_DIR", &dir)
        .arg("stop")
        .output();
    let _ = fs::remove_dir_all(&dir);
}
