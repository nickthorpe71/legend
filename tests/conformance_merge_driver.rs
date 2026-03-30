mod common;

use common::{
    write_events_fixture, write_msgpack_memory_fixture, FixtureSessionEntry, FixtureShortTermEntry,
    Harness,
};

#[test]
fn git_merge_driver_merges_append_only_event_logs() {
    let harness = Harness::new();
    let ancestor = harness.path().join("ancestor.jsonl");
    let ours = harness.path().join("ours.jsonl");
    let theirs = harness.path().join("theirs.jsonl");

    write_events_fixture(&ancestor, &[r#"{"ts":1,"cmd":"start","detail":"session cold-start"}"#]);
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

    assert!(output.stderr.contains("[LEGEND] Auto-merging conflicted state file: events.jsonl"));
    assert!(output.stderr.contains("[LEGEND] Merged events.jsonl: 1 base + 2 new lines"));

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
    assert!(output.stderr.contains("[LEGEND] Auto-merging conflicted state file: .legend/memory.lz4"));

    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert!(
        short_term
            .iter()
            .any(|entry| entry["text"] == "shared base memory")
    );
    assert!(
        short_term
            .iter()
            .any(|entry| entry["text"].as_str().unwrap_or("").contains("theirs memory entry"))
    );
}
