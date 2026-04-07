mod common;

use common::{
    write_msgpack_memory_fixture, write_msgpack_missing_field_fixture, FixtureMemoryRef,
    FixtureSessionEntry, FixtureShortTermEntry, Harness,
};

#[test]
fn corrupt_store_is_backed_up_and_falls_back_to_empty_state() {
    let harness = Harness::new();
    std::fs::create_dir_all(harness.legend_dir()).expect("create legend dir");
    std::fs::write(harness.legend_dir().join("memory.lz4"), b"not-valid")
        .expect("write corrupt store");

    let output = harness.cmd_ok(&["memory", "stats"]);

    assert!(output.stdout.contains("Working memory (L1): 0"));
    assert!(output.stdout.contains("Short-term entries: 0"));
    assert!(output
        .stderr
        .contains("Warning: failed to load memory store"));
    assert!(output
        .stderr
        .contains("Backup saved to .legend/memory.lz4.corrupt"));
    assert!(harness.exists(".legend/memory.lz4.corrupt"));
}

#[test]
fn msgpack_fixture_missing_new_fields_loads_without_data_loss() {
    let harness = Harness::new();
    write_msgpack_missing_field_fixture(&harness);

    let dump = harness.output_json(&["memory", "dump"]);
    assert_eq!(dump["clock"], 100);
    // Old `immediate` field is discarded during migration; working_memory starts empty
    assert!(dump["working_memory"].as_array().unwrap().is_empty());

    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert_eq!(short_term.len(), 1);
    assert_eq!(short_term[0]["text"], "important migrated memory");
    let salience = short_term[0]["salience"].as_f64().expect("salience number");
    assert!(
        (salience - 0.9).abs() < 0.001,
        "unexpected salience: {salience}"
    );
    assert_eq!(short_term[0]["refs"][0]["path"], "src/main.rs");
    assert_eq!(dump["session_log"][0]["text"], "test session");

    let context = harness.output_json(&["memory", "context"]);
    assert_eq!(context["current_task"], "testing msgpack");
    assert_eq!(context["stats"]["short_term_entries"], 1);
}

#[test]
fn fixture_structs_produce_fully_loadable_state() {
    let harness = Harness::new();

    // Write a fixture with distinctive non-default values for every exposed field
    let entry = FixtureShortTermEntry {
        id: 42,
        text: "DECISION: Distinctive drift-test entry".into(),
        summary: "Drift test summary".into(),
        embedding: vec![0.1, 0.9, 0.5],
        last_access: 777,
        usage: 13,
        salience: 0.85,
        reconsolidation_count: 3,
        labile_until: 999,
        refs: vec![FixtureMemoryRef {
            path: "src/drift.rs".into(),
            start_line: 10,
            end_line: 20,
            snippet: "fn drift_check()".into(),
        }],
        gradient_sq_sum: 1.5,
        density: 2.3,
        consolidated: true,
        emotional_valence: 0.0,
        stability: 1.0,
        last_retrieval_interval: 0,
    };

    let session = FixtureSessionEntry {
        timestamp: 777,
        text: "drift validation session".into(),
    };

    write_msgpack_memory_fixture(
        &harness.legend_dir().join("memory.lz4"),
        vec![entry],
        vec![session],
    );

    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert_eq!(short_term.len(), 1);

    let e = &short_term[0];
    assert_eq!(e["id"], 42);
    assert_eq!(e["text"], "DECISION: Distinctive drift-test entry");
    assert_eq!(e["summary"], "Drift test summary");
    assert_eq!(e["last_access"], 777);
    assert_eq!(e["usage"], 13);
    assert!((e["salience"].as_f64().unwrap() - 0.85).abs() < 0.01);
    assert_eq!(e["reconsolidation_count"], 3);
    // labile_until=999 > clock=10 → labile should be true
    assert_eq!(e["labile"], true);
    assert_eq!(e["refs"][0]["path"], "src/drift.rs");
    assert_eq!(e["refs"][0]["start_line"], 10);
    assert_eq!(e["refs"][0]["end_line"], 20);
    assert_eq!(e["refs"][0]["snippet"], "fn drift_check()");

    // Session log round-trips
    assert_eq!(dump["session_log"][0]["text"], "drift validation session");
    assert_eq!(dump["session_log"][0]["timestamp"], 777);
}
