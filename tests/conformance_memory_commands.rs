mod common;

use common::{seed_basic_repo, Harness};

#[test]
fn query_reasons_reinforce_task_and_reset_work_as_public_contracts() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let tick = harness.output_json(&[
        "memory",
        "tick",
        "DECISION: Chose graph index because it keeps lookups cheap.",
    ]);
    let entry_id = tick["entry_id"].as_u64().expect("entry id");

    let reasons = harness.output_json(&["memory", "query", "--reasons", "graph index"]);
    assert_eq!(reasons["note"], "Top result auto-reinforced (+3% salience boost)");
    assert!(
        reasons["short_term"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .any(|item| item["text"] == "DECISION: Chose graph index because it keeps lookups cheap.")
    );
    assert!(
        reasons["short_term"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .any(|item| item["reason"].as_str().unwrap_or("").contains("similarity"))
    );

    let reinforce = harness.output_json(&[
        "memory",
        "reinforce",
        "1.0",
        &entry_id.to_string(),
    ]);
    let reinforced = reinforce["reinforced"].as_array().expect("reinforced array");
    assert_eq!(reinforced.len(), 1);
    assert_eq!(reinforced[0]["id"], entry_id);
    assert!(
        reinforced[0]["salience_after"].as_f64().unwrap_or(0.0)
            >= reinforced[0]["salience_before"].as_f64().unwrap_or(0.0)
    );

    let task_set = harness.cmd_ok(&["memory", "task", "set", "Refactor retrieval pipeline"]);
    assert!(task_set.stdout.contains("✓ Current task set: Refactor retrieval pipeline"));

    let task_show = harness.cmd_ok(&["memory", "task"]);
    assert!(task_show.stdout.contains("Current task: Refactor retrieval pipeline"));

    let start_json = harness.output_json(&["memory", "start", "--json"]);
    assert_eq!(start_json["current_task"], "Refactor retrieval pipeline");

    let task_clear = harness.cmd_ok(&["memory", "task", "clear"]);
    assert!(task_clear.stdout.contains("✓ Current task cleared"));

    let reset = harness.cmd_ok(&["memory", "reset"]);
    assert!(reset.stdout.contains("✓ Memory reset"));

    let dump = harness.output_json(&["memory", "dump"]);
    assert_eq!(dump["short_term"].as_array().map(|v| v.len()), Some(0));
    assert_eq!(dump["graph"]["nodes"].as_array().map(|v| v.len()), Some(0));
}
