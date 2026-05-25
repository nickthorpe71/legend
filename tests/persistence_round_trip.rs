//! Cross-session memory: state survives a save → load → continue cycle
//! and produces the same final substrate as running both ticks in a
//! single process.

use legend::persistence;
use legend::seed::load_seed_graph;
use legend::tick_pipeline::build_relations::build_relations;
use legend::tick_pipeline::decay::focus_radius_decay;
use legend::tick_pipeline::hebbian::hebbian_and_salience;
use legend::tick_pipeline::run_extractors::run_extractors;
use legend::tick_pipeline::supersede::supersede;
use legend::types::{Hypergraph, Policy};

use std::fs;
use std::path::PathBuf;
use std::process;

const TURN_1: &str = "I'm starting a new side project called Polaris.";
const TURN_2: &str = "Polaris is a CLI tool for managing notes.";

fn run_tick(hypergraph: &mut Hypergraph, text: &str, policy: &Policy) {
    let ext = run_extractors(text, &[], policy, hypergraph, &[]);
    let built_relations = build_relations(text, hypergraph, &ext, policy, None);
    let superseded = supersede(hypergraph, &built_relations.minted_relations, policy);
    let reinforced = hebbian_and_salience(hypergraph, &built_relations, &superseded, None, policy, &[]);
    let _ = focus_radius_decay(hypergraph, &reinforced.reinforced, policy);
}

/// `(elements.len, relations.len, clock.0, by_name.len, sum-of-element-access-counts)`
/// — a coarse fingerprint that catches any structural drift across the
/// save/load boundary without depending on `Hypergraph: PartialEq`.
fn shape(hypergraph: &Hypergraph) -> (usize, usize, u64, usize, u64) {
    let access_sum: u64 = hypergraph
        .elements
        .iter()
        .map(|e| e.stats.access_count as u64)
        .sum();
    (
        hypergraph.elements.len(),
        hypergraph.relations.len(),
        hypergraph.clock.0,
        hypergraph.by_name.len(),
        access_sum,
    )
}

fn temp_snapshot_path(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "legend_persistence_cross_session_{}_{}.lz4",
        label,
        process::id(),
    ));
    p
}

#[test]
fn cross_session_save_load_continues_correctly() {
    let policy = Policy::default();

    // ── Two ticks in one process — the control. ────────────────────
    let mut hypergraph_control = load_seed_graph();
    run_tick(&mut hypergraph_control, TURN_1, &policy);
    run_tick(&mut hypergraph_control, TURN_2, &policy);
    let state_control = shape(&hypergraph_control);

    // ── Two ticks split across "sessions" via save/load. ───────────
    let mut hypergraph_a = load_seed_graph();
    run_tick(&mut hypergraph_a, TURN_1, &policy);
    let state_after_tick1 = shape(&hypergraph_a);

    let path = temp_snapshot_path("two_session");
    persistence::save(&hypergraph_a, &path).expect("save");
    // Drop the in-memory graph so we exercise the load path cleanly.
    drop(hypergraph_a);

    let mut hypergraph_b = persistence::load(&path).expect("load");
    // Loaded snapshot's shape should match the post-tick-1 shape.
    assert_eq!(
        shape(&hypergraph_b),
        state_after_tick1,
        "loaded snapshot doesn't match the in-memory tick-1 state",
    );
    // Indices regenerated — by_name and region_children should be
    // populated, not empty.
    assert!(
        !hypergraph_b.by_name.is_empty(),
        "by_name should be repopulated after load",
    );
    assert!(
        !hypergraph_b.region_children.is_empty(),
        "region_children should be repopulated after load",
    );

    run_tick(&mut hypergraph_b, TURN_2, &policy);
    let state_b = shape(&hypergraph_b);

    assert_eq!(
        state_b, state_control,
        "cross-session state diverged from single-process control",
    );

    let _ = fs::remove_file(&path);
}

#[test]
fn cross_session_load_or_seed_returns_existing_snapshot() {
    let policy = Policy::default();
    let path = temp_snapshot_path("load_or_seed");
    let _ = fs::remove_file(&path);

    // First call seeds + persists.
    let mut hypergraph = persistence::load_or_seed(&path).expect("seed");
    assert!(path.exists(), "snapshot file should exist after first call");
    run_tick(&mut hypergraph, TURN_1, &policy);
    persistence::save(&hypergraph, &path).expect("save");
    let state_after = shape(&hypergraph);
    drop(hypergraph);

    // Second call should load the persisted snapshot, NOT re-seed.
    let hypergraph_reload = persistence::load_or_seed(&path).expect("reload");
    assert_eq!(
        shape(&hypergraph_reload),
        state_after,
        "load_or_seed should reuse the persisted snapshot on second call",
    );

    let _ = fs::remove_file(&path);
}
