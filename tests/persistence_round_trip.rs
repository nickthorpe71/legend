//! Cross-session memory: state survives a save → load → continue cycle
//! and produces the same final substrate as running both ticks in a
//! single process.

use legend::persistence;
use legend::seed::load_seed_graph;
use legend::steps::build_relations::build_relations;
use legend::steps::decay::focus_radius_decay;
use legend::steps::hebbian::hebbian_and_salience;
use legend::steps::run_extractors::run_extractors;
use legend::steps::supersede::supersede;
use legend::types::{Hypergraph, Policy};

use std::fs;
use std::path::PathBuf;
use std::process;

const TURN_1: &str = "I'm starting a new side project called Polaris.";
const TURN_2: &str = "Polaris is a CLI tool for managing notes.";

fn run_tick(hg: &mut Hypergraph, text: &str, policy: &Policy) {
    let ext = run_extractors(text, &[], policy, hg, &[]);
    let step8 = build_relations(text, hg, &ext, policy, None);
    let step9 = supersede(hg, &step8.minted_relations, policy);
    let step10 = hebbian_and_salience(hg, &step8, &step9, None, policy);
    let _ = focus_radius_decay(hg, &step10.reinforced, policy);
}

/// `(elements.len, relations.len, clock.0, by_name.len, sum-of-element-access-counts)`
/// — a coarse fingerprint that catches any structural drift across the
/// save/load boundary without depending on `Hypergraph: PartialEq`.
fn shape(hg: &Hypergraph) -> (usize, usize, u64, usize, u64) {
    let access_sum: u64 = hg
        .elements
        .iter()
        .map(|e| e.stats.access_count as u64)
        .sum();
    (
        hg.elements.len(),
        hg.relations.len(),
        hg.clock.0,
        hg.by_name.len(),
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
    let mut hg_control = load_seed_graph();
    run_tick(&mut hg_control, TURN_1, &policy);
    run_tick(&mut hg_control, TURN_2, &policy);
    let state_control = shape(&hg_control);

    // ── Two ticks split across "sessions" via save/load. ───────────
    let mut hg_a = load_seed_graph();
    run_tick(&mut hg_a, TURN_1, &policy);
    let state_after_tick1 = shape(&hg_a);

    let path = temp_snapshot_path("two_session");
    persistence::save(&hg_a, &path).expect("save");
    // Drop the in-memory graph so we exercise the load path cleanly.
    drop(hg_a);

    let mut hg_b = persistence::load(&path).expect("load");
    // Loaded snapshot's shape should match the post-tick-1 shape.
    assert_eq!(
        shape(&hg_b),
        state_after_tick1,
        "loaded snapshot doesn't match the in-memory tick-1 state",
    );
    // Indices regenerated — by_name and region_children should be
    // populated, not empty.
    assert!(
        !hg_b.by_name.is_empty(),
        "by_name should be repopulated after load",
    );
    assert!(
        !hg_b.region_children.is_empty(),
        "region_children should be repopulated after load",
    );

    run_tick(&mut hg_b, TURN_2, &policy);
    let state_b = shape(&hg_b);

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
    let mut hg = persistence::load_or_seed(&path).expect("seed");
    assert!(path.exists(), "snapshot file should exist after first call");
    run_tick(&mut hg, TURN_1, &policy);
    persistence::save(&hg, &path).expect("save");
    let state_after = shape(&hg);
    drop(hg);

    // Second call should load the persisted snapshot, NOT re-seed.
    let hg_reload = persistence::load_or_seed(&path).expect("reload");
    assert_eq!(
        shape(&hg_reload),
        state_after,
        "load_or_seed should reuse the persisted snapshot on second call",
    );

    let _ = fs::remove_file(&path);
}
