mod common;

use common::{
    seed_basic_repo, write_bincode_memory_fixture, write_msgpack_missing_field_fixture, Harness,
};

#[test]
fn corrupt_store_is_backed_up_and_falls_back_to_empty_state() {
    let harness = Harness::new();
    std::fs::create_dir_all(harness.legend_dir()).expect("create legend dir");
    std::fs::write(harness.legend_dir().join("memory.lz4"), b"not-valid").expect("write corrupt store");

    let output = harness.cmd_ok(&["memory", "stats"]);

    assert!(output.stdout.contains("Immediate buffer: 0"));
    assert!(output.stdout.contains("Short-term entries: 0"));
    assert!(output.stderr.contains("Warning: failed to load memory store"));
    assert!(output.stderr.contains("Backup saved to .legend/memory.lz4.corrupt"));
    assert!(harness.exists(".legend/memory.lz4.corrupt"));
}

#[test]
fn legacy_bincode_store_is_migrated_on_init_without_losing_data() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    write_bincode_memory_fixture(&harness);

    let output = harness.cmd_ok(&["init"]);
    assert!(output.stdout.contains("Legend already initialized in this directory"));
    assert!(output.stdout.contains("Memory store OK (1 entries"));

    let compressed = std::fs::read(harness.legend_dir().join("memory.lz4")).expect("read migrated memory");
    let decompressed = lz4::block::decompress(&compressed, None).expect("decompress migrated memory");
    assert_eq!(&decompressed[..4], b"LGND");

    let dump = harness.output_json(&["memory", "dump"]);
    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert!(short_term.iter().any(|entry| entry["text"] == "bincode entry"));
}

#[test]
fn msgpack_fixture_missing_new_fields_loads_without_data_loss() {
    let harness = Harness::new();
    write_msgpack_missing_field_fixture(&harness);

    let dump = harness.output_json(&["memory", "dump"]);
    assert_eq!(dump["clock"], 100);
    assert_eq!(dump["immediate"][0], "hello");

    let short_term = dump["short_term"].as_array().expect("short_term array");
    assert_eq!(short_term.len(), 1);
    assert_eq!(short_term[0]["text"], "important migrated memory");
    let salience = short_term[0]["salience"]
        .as_f64()
        .expect("salience number");
    assert!((salience - 0.9).abs() < 0.001, "unexpected salience: {salience}");
    assert_eq!(short_term[0]["refs"][0]["path"], "src/main.rs");
    assert_eq!(dump["session_log"][0]["text"], "test session");

    let context = harness.output_json(&["memory", "context"]);
    assert_eq!(context["current_task"], "testing msgpack");
    assert_eq!(context["stats"]["short_term_entries"], 1);
}
