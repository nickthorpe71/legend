mod common;

use common::{
    write_events_fixture, write_msgpack_memory_fixture, write_msgpack_memory_fixture_extended,
    FixtureGraphMemory, FixtureGraphNode, FixtureSessionEntry, FixtureShortTermEntry, Harness,
};

#[test]
fn git_merge_driver_merges_append_only_event_logs() {
    let harness = Harness::new();
    let ancestor = harness.path().join("ancestor.jsonl");
    let ours = harness.path().join("ours.jsonl");
    let theirs = harness.path().join("theirs.jsonl");

    write_events_fixture(
        &ancestor,
        &[r#"{"ts":1,"cmd":"start","detail":"session cold-start"}"#],
    );
    write_events_fixture(
        &ours,
        &[
            r#"{"ts":1,"cmd":"start","detail":"session cold-start"}"#,
            r#"{"ts":3,"cmd":"tick","detail":"ours"}"#,
        ],
    );
    write_events_fixture(
        &theirs,
        &[
            r#"{"ts":1,"cmd":"start","detail":"session cold-start"}"#,
            r#"{"ts":2,"cmd":"tick","detail":"theirs"}"#,
        ],
    );

    let output = harness.cmd_in_ok(
        harness.path(),
        &[
            "git-merge-driver",
            ancestor.to_str().unwrap(),
            ours.to_str().unwrap(),
            theirs.to_str().unwrap(),
            "events.jsonl",
        ],
    );

    assert!(output
        .stderr
        .contains("[LEGEND] Auto-merging conflicted state file: events.jsonl"));
    assert!(output
        .stderr
        .contains("[LEGEND] Merged events.jsonl: 1 base + 2 new lines"));

    let merged = std::fs::read_to_string(&ours).expect("read merged events");
    let lines: Vec<&str> = merged.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains(r#""ts":1"#));
    assert!(lines[1].contains(r#""ts":2"#));
    assert!(lines[2].contains(r#""ts":3"#));
}

#[test]
fn git_merge_driver_merges_memory_state_from_both_sides() {
    let harness = Harness::new();
    let ancestor = harness.path().join("ancestor-memory.lz4");
    let ours = harness.legend_dir().join("memory.lz4");
    let theirs = harness.path().join("theirs-memory.lz4");

    std::fs::create_dir_all(harness.legend_dir()).expect("create legend dir");
    write_msgpack_memory_fixture(
        &ancestor,
        vec![FixtureShortTermEntry {
            id: 1,
            text: "shared base memory".into(),
            summary: "shared base memory".into(),
            embedding: vec![0.1, 0.2, 0.3],
            last_access: 1,
            usage: 1,
            salience: 0.5,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
        }],
        vec![FixtureSessionEntry {
            timestamp: 1,
            text: "shared base memory".into(),
        }],
    );
    std::fs::copy(&ancestor, &ours).expect("copy ancestor to ours");
    write_msgpack_memory_fixture(
        &theirs,
        vec![
            FixtureShortTermEntry {
                id: 1,
                text: "shared base memory".into(),
                summary: "shared base memory".into(),
                embedding: vec![0.1, 0.2, 0.3],
                last_access: 1,
                usage: 1,
                salience: 0.5,
                reconsolidation_count: 0,
                labile_until: 0,
                refs: vec![],
                gradient_sq_sum: 0.0,
                density: 0.0,
                consolidated: false,
                emotional_valence: 0.0,
                stability: 1.0,
                last_retrieval_interval: 0,
            },
            FixtureShortTermEntry {
                id: 2,
                text: "DECISION: theirs memory entry".into(),
                summary: "DECISION: theirs memory entry".into(),
                embedding: vec![0.3, 0.2, 0.1],
                last_access: 2,
                usage: 1,
                salience: 0.6,
                reconsolidation_count: 0,
                labile_until: 0,
                refs: vec![],
                gradient_sq_sum: 0.0,
                density: 0.0,
                consolidated: false,
                emotional_valence: 0.0,
                stability: 1.0,
                last_retrieval_interval: 0,
            },
        ],
        vec![
            FixtureSessionEntry {
                timestamp: 1,
                text: "shared base memory".into(),
            },
            FixtureSessionEntry {
                timestamp: 2,
                text: "DECISION: theirs memory entry".into(),
            },
        ],
    );

    let output = harness.cmd_in_ok(
        harness.path(),
        &[
            "git-merge-driver",
            ancestor.to_str().unwrap(),
            ours.to_str().unwrap(),
            theirs.to_str().unwrap(),
            ".legend/memory.lz4",
        ],
    );
    assert!(output
        .stderr
        .contains("[LEGEND] Auto-merging conflicted state file: .legend/memory.lz4"));
    // Should now report structural merge stats
    assert!(output
        .stderr
        .contains("[LEGEND] Structural merge complete:"));

    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert!(short_term
        .iter()
        .any(|entry| entry["text"] == "shared base memory"));
    assert!(short_term.iter().any(|entry| entry["text"]
        .as_str()
        .unwrap_or("")
        .contains("theirs memory entry")));
}

#[test]
fn merge_states_unions_l2_entries_and_l3_nodes() {
    let harness = Harness::new();
    std::fs::create_dir_all(harness.legend_dir()).expect("create legend dir");

    // "Ours" has entry id=1 and graph node id=10
    let mut our_graph = FixtureGraphMemory::default();
    our_graph.nodes.insert(
        10,
        FixtureGraphNode {
            id: 10,
            label: "NodeA".into(),
            kind: "Entity".into(),
            weight: 0.5,
            last_seen: 5,
            salience: 0.4,
            source_texts: vec!["source1".into()],
        },
    );
    our_graph.index.insert("NodeA".into(), 10);

    write_msgpack_memory_fixture_extended(
        &harness.legend_dir().join("memory.lz4"),
        vec![FixtureShortTermEntry {
            id: 1,
            text: "ours entry".into(),
            summary: "ours entry".into(),
            embedding: vec![0.1, 0.2, 0.3],
            last_access: 5,
            usage: 1,
            salience: 0.5,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
        }],
        vec![FixtureSessionEntry {
            timestamp: 5,
            text: "ours entry".into(),
        }],
        our_graph,
        10,
        100,
        None,
    );

    // "Theirs" has entry id=2 and graph node id=20
    let their_file = harness.path().join("theirs.lz4");
    let mut their_graph = FixtureGraphMemory::default();
    their_graph.nodes.insert(
        20,
        FixtureGraphNode {
            id: 20,
            label: "NodeB".into(),
            kind: "Entity".into(),
            weight: 0.6,
            last_seen: 8,
            salience: 0.5,
            source_texts: vec!["source2".into()],
        },
    );
    their_graph.index.insert("NodeB".into(), 20);

    write_msgpack_memory_fixture_extended(
        &their_file,
        vec![FixtureShortTermEntry {
            id: 2,
            text: "theirs entry".into(),
            summary: "theirs entry".into(),
            embedding: vec![0.3, 0.2, 0.1],
            last_access: 8,
            usage: 1,
            salience: 0.7,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
        }],
        vec![FixtureSessionEntry {
            timestamp: 8,
            text: "theirs entry".into(),
        }],
        their_graph,
        15,
        200,
        Some("their task".into()),
    );

    // Run merge-local
    let output = harness.cmd_in_ok(
        harness.path(),
        &["merge-local", their_file.to_str().unwrap()],
    );
    assert!(output
        .stderr
        .contains("[LEGEND] Post-merge recovery: merged"));

    // Verify merged state
    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term");

    // Both L2 entries should be present
    assert!(
        short_term.iter().any(|e| e["text"] == "ours entry"),
        "ours entry missing"
    );
    assert!(
        short_term.iter().any(|e| e["text"] == "theirs entry"),
        "theirs entry missing"
    );

    // Both L3 nodes should be present
    let nodes = dump["graph"]["nodes"].as_array().expect("nodes");
    assert!(nodes.iter().any(|n| n["label"] == "NodeA"), "NodeA missing");
    assert!(nodes.iter().any(|n| n["label"] == "NodeB"), "NodeB missing");

    // Clock should be max(10, 15) = 15
    assert_eq!(dump["clock"].as_u64().unwrap(), 15);
}

#[test]
fn merge_states_l2_collision_keeps_higher_salience() {
    let harness = Harness::new();
    std::fs::create_dir_all(harness.legend_dir()).expect("create legend dir");

    // Ours: entry id=1 with salience 0.3
    write_msgpack_memory_fixture_extended(
        &harness.legend_dir().join("memory.lz4"),
        vec![FixtureShortTermEntry {
            id: 1,
            text: "shared entry".into(),
            summary: "shared entry".into(),
            embedding: vec![0.1, 0.2, 0.3],
            last_access: 5,
            usage: 1,
            salience: 0.3,
            reconsolidation_count: 0,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 1.0,
            last_retrieval_interval: 0,
        }],
        vec![],
        FixtureGraphMemory::default(),
        10,
        100,
        None,
    );

    // Theirs: same entry id=1 but with higher salience 0.8
    let their_file = harness.path().join("theirs.lz4");
    write_msgpack_memory_fixture_extended(
        &their_file,
        vec![FixtureShortTermEntry {
            id: 1,
            text: "shared entry updated".into(),
            summary: "shared entry updated".into(),
            embedding: vec![0.1, 0.2, 0.3],
            last_access: 12,
            usage: 5,
            salience: 0.8,
            reconsolidation_count: 2,
            labile_until: 0,
            refs: vec![],
            gradient_sq_sum: 0.0,
            density: 0.0,
            consolidated: false,
            emotional_valence: 0.0,
            stability: 2.0,
            last_retrieval_interval: 0,
        }],
        vec![],
        FixtureGraphMemory::default(),
        15,
        100,
        None,
    );

    harness.cmd_in_ok(
        harness.path(),
        &["merge-local", their_file.to_str().unwrap()],
    );

    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term");

    // Should have exactly 1 entry (collision resolved)
    assert_eq!(short_term.len(), 1);

    // Should have the higher-salience version
    let entry = &short_term[0];
    assert_eq!(entry["text"], "shared entry updated");
    assert!(entry["salience"].as_f64().unwrap() > 0.7);
}

#[test]
fn merge_local_nonexistent_file_does_not_error() {
    let harness = Harness::new();
    std::fs::create_dir_all(harness.legend_dir()).expect("create legend dir");

    // Create a minimal state so legend can load something
    write_msgpack_memory_fixture(&harness.legend_dir().join("memory.lz4"), vec![], vec![]);

    // merge-local with a nonexistent file should succeed (non-fatal)
    let output = harness.cmd_in_ok(
        harness.path(),
        &["merge-local", "/tmp/nonexistent_legend_file.lz4"],
    );
    // Should have logged that it couldn't load, but not errored
    assert!(output.status.success());
}
