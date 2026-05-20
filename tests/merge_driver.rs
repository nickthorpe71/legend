//! End-to-end test for the git-merge-driver subcommand. Simulates
//! exactly what git does when it hits a `.legend/memory.lz4`
//! conflict: invokes the binary with `%O %A %B %P` paths, expects
//! the merged result written back to `%A`, exit code 0.

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

fn temp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "legend_merge_driver_{}_{}_{}",
        label,
        process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn tick(hg: &mut Hypergraph, text: &str) {
    let policy = Policy::default();
    let ext = run_extractors(text, &[], &policy, hg, &[]);
    let step8 = build_relations(text, hg, &ext, &policy, None);
    let step9 = supersede(hg, &step8.minted_relations, &policy);
    let step10 = hebbian_and_salience(hg, &step8, &step9, None, &policy);
    let _ = focus_radius_decay(hg, &step10.reinforced, &policy);
}

#[test]
fn git_merge_driver_unifies_two_branches_via_subprocess() {
    let dir = temp_dir("subprocess_unify");

    // Build three snapshots: ancestor (just seed), ours (seed +
    // tick about Polaris/Rust), theirs (seed + tick about Beam/Go).
    let ancestor = dir.join("ancestor.lz4");
    let ours = dir.join("ours.lz4");
    let theirs = dir.join("theirs.lz4");

    let hg_ancestor = load_seed_graph();
    persistence::save(&hg_ancestor, &ancestor).expect("save ancestor");

    let mut hg_ours = load_seed_graph();
    tick(&mut hg_ours, "Polaris is written in Rust.");
    let ours_elements = hg_ours.elements.len();
    let ours_relations = hg_ours.relations.len();
    persistence::save(&hg_ours, &ours).expect("save ours");

    let mut hg_theirs = load_seed_graph();
    tick(&mut hg_theirs, "Beam is a Go server.");
    persistence::save(&hg_theirs, &theirs).expect("save theirs");

    // Invoke the merge driver exactly as git would.
    let bin = env!("CARGO_BIN_EXE_legend");
    let output = std::process::Command::new(bin)
        .arg("git-merge-driver")
        .arg(&ancestor)
        .arg(&ours)
        .arg(&theirs)
        .arg("memory.lz4")
        .output()
        .expect("invoke merge driver");
    assert!(
        output.status.success(),
        "merge driver exited non-zero. stderr:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[legend] merged:"),
        "merge driver should print summary; stderr was:\n{stderr}"
    );

    // Reload the merged result and verify both Polaris (from ours)
    // and Beam (from theirs) are present.
    let merged = persistence::load(&ours).expect("load merged");
    assert!(
        merged.by_name.contains_key("Polaris"),
        "merged graph missing Polaris from ours"
    );
    assert!(
        merged.by_name.contains_key("Beam"),
        "merged graph missing Beam from theirs"
    );
    // Substrate grew: theirs's elements got added to ours.
    assert!(
        merged.elements.len() >= ours_elements,
        "merged element count must be >= ours: {} vs {}",
        merged.elements.len(),
        ours_elements,
    );
    assert!(merged.relations.len() >= ours_relations);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn git_merge_driver_dedupes_same_entity_minted_on_both_branches() {
    // Both branches independently mint "Polaris" with the same name
    // and polarity. After merge, ours should have ONE Polaris (not
    // two). The merge driver's name-based remap collapses them.
    let dir = temp_dir("subprocess_dedup");
    let ancestor = dir.join("ancestor.lz4");
    let ours = dir.join("ours.lz4");
    let theirs = dir.join("theirs.lz4");

    persistence::save(&load_seed_graph(), &ancestor).expect("save ancestor");

    let mut hg_ours = load_seed_graph();
    tick(&mut hg_ours, "Polaris is a project.");
    persistence::save(&hg_ours, &ours).expect("save ours");

    let mut hg_theirs = load_seed_graph();
    tick(&mut hg_theirs, "Polaris is a project.");
    persistence::save(&hg_theirs, &theirs).expect("save theirs");

    let bin = env!("CARGO_BIN_EXE_legend");
    let output = std::process::Command::new(bin)
        .arg("git-merge-driver")
        .arg(&ancestor)
        .arg(&ours)
        .arg(&theirs)
        .arg("memory.lz4")
        .output()
        .expect("invoke merge driver");
    assert!(output.status.success());

    let merged = persistence::load(&ours).expect("load merged");
    let polaris_count = merged
        .elements
        .iter()
        .filter(|e| e.names.first().map(|n| n == "Polaris").unwrap_or(false))
        .count();
    assert_eq!(
        polaris_count, 1,
        "merge driver should dedup the duplicate Polaris elements; got {polaris_count}",
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn git_merge_driver_handles_meta_relations_correctly() {
    // Both branches drive a supersession event (which mints metas
    // referencing the cache relation). After merge, the meta links
    // must point at the unified cache, not at orphaned IDs.
    let dir = temp_dir("subprocess_metas");
    let ancestor = dir.join("ancestor.lz4");
    let ours = dir.join("ours.lz4");
    let theirs = dir.join("theirs.lz4");

    persistence::save(&load_seed_graph(), &ancestor).expect("save ancestor");

    let mut hg_ours = load_seed_graph();
    tick(&mut hg_ours, "Polaris switched from Rust to Go.");
    persistence::save(&hg_ours, &ours).expect("save ours");

    let mut hg_theirs = load_seed_graph();
    tick(&mut hg_theirs, "Beam switched from Python to Rust.");
    persistence::save(&hg_theirs, &theirs).expect("save theirs");

    let bin = env!("CARGO_BIN_EXE_legend");
    let output = std::process::Command::new(bin)
        .arg("git-merge-driver")
        .arg(&ancestor)
        .arg(&ours)
        .arg(&theirs)
        .arg("memory.lz4")
        .output()
        .expect("invoke merge driver");
    assert!(
        output.status.success(),
        "merge driver failed: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    let merged = persistence::load(&ours).expect("load merged");
    // Every Term::Relation reference must point at a valid index.
    for r in &merged.relations {
        for a in &r.attributes {
            if let legend::types::Term::Relation(rid) = a.value {
                assert!(
                    (rid.0 as usize) < merged.relations.len(),
                    "dangling Term::Relation({}) — must be < {}",
                    rid.0,
                    merged.relations.len(),
                );
            }
        }
    }
    // Both Polaris and Beam should be present.
    assert!(merged.by_name.contains_key("Polaris"));
    assert!(merged.by_name.contains_key("Beam"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn git_merge_driver_rejects_seed_drift_loudly() {
    // If theirs's snapshot was written against a different seed
    // binary, the persistence loader will refuse it with SeedDrift
    // BEFORE merge even starts. Verify that error path.
    let dir = temp_dir("subprocess_drift");
    let ancestor = dir.join("ancestor.lz4");
    let ours = dir.join("ours.lz4");
    let theirs = dir.join("theirs.lz4");

    persistence::save(&load_seed_graph(), &ancestor).expect("save ancestor");
    persistence::save(&load_seed_graph(), &ours).expect("save ours");

    // Hand-craft a snapshot with a tampered seed fingerprint.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"LEGEND01");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0xAB; 8]); // tampered fingerprint
    bytes.extend_from_slice(&[0u8; 32]);
    fs::write(&theirs, &bytes).unwrap();

    let bin = env!("CARGO_BIN_EXE_legend");
    let output = std::process::Command::new(bin)
        .arg("git-merge-driver")
        .arg(&ancestor)
        .arg(&ours)
        .arg(&theirs)
        .arg("memory.lz4")
        .output()
        .expect("invoke merge driver");
    assert!(
        !output.status.success(),
        "merge driver should exit non-zero on seed drift"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("seed") || stderr.to_lowercase().contains("drift"),
        "stderr should mention seed drift; got:\n{stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}
