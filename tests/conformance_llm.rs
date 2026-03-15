mod common;

use common::{seed_basic_repo, Harness};

#[test]
fn llm_no_args_prints_help() {
    let harness = Harness::new();

    let output = harness.cmd(&["llm"]);
    let combined = format!("{}{}", output.stdout, output.stderr);
    assert!(
        combined.contains("signals") && combined.contains("task"),
        "llm with no args should print help with subcommands: {}",
        combined
    );
}

#[test]
fn llm_signals_short_text_needs_no_llm() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.output_json(&["llm", "signals", "Short text here"]);
    assert_eq!(output["needs_llm"], false, "short text should not need LLM");
    assert!(
        output["metrics"]["words"].as_u64().unwrap_or(0) > 0,
        "should report word count"
    );
    assert!(
        output["metrics"]["chars"].as_u64().unwrap_or(0) > 0,
        "should report char count"
    );
}

#[test]
fn llm_signals_long_text_recommends_tasks() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Long text with low entity density should trigger entity_extract recommendation
    let long_text = "The application framework processes incoming requests through a series of middleware layers. Each middleware can modify the request or response objects before passing control to the next handler in the chain. The routing system maps URL patterns to controller functions using a tree-based data structure for efficient lookup. Database connections are managed through a connection pool that maintains a configurable number of idle connections ready for use.";

    let output = harness.output_json(&["llm", "signals", long_text]);
    assert!(
        output["metrics"]["words"].as_u64().unwrap_or(0) >= 35,
        "long text should have many words"
    );
    assert!(
        output["recommended_tasks"].is_array(),
        "should include recommended_tasks array"
    );
}

#[test]
fn llm_task_entity_extract_creates_pending_task() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.output_json(&[
        "llm",
        "task",
        "entity_extract",
        "--text",
        "ARCHITECTURE: The MemoryState struct stores short-term entries with embeddings computed by the embed module using trigram hashing.",
    ]);

    assert!(
        output["task_id"].as_str().is_some(),
        "should return task_id: {}",
        output
    );
    assert_eq!(
        output["kind"], "entity_extract",
        "kind should be entity_extract"
    );
    assert!(
        output["prompt"].as_str().is_some(),
        "should include prompt"
    );
    assert!(
        output["json_schema"].is_object(),
        "should include json_schema"
    );
}

#[test]
fn llm_task_cluster_summary_creates_pending_task() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.output_json(&[
        "llm",
        "task",
        "cluster_summary",
        "--text",
        "The query pipeline routes through graph lookup. The query pipeline coordinates graph lookup. The query system uses associative priming.",
    ]);

    assert!(output["task_id"].as_str().is_some());
    assert_eq!(output["kind"], "cluster_summary");
}

#[test]
fn llm_list_shows_pending_tasks() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Create a task first
    harness.cmd_ok(&[
        "llm",
        "task",
        "entity_extract",
        "--text",
        "ARCHITECTURE: The MemoryState struct uses trigram hashing for embeddings.",
    ]);

    let list = harness.output_json(&["llm", "list"]);
    let tasks = list.as_array().expect("list should be array");
    assert!(
        !tasks.is_empty(),
        "list should include the created task"
    );
    assert_eq!(tasks[0]["kind"], "entity_extract");
    assert_eq!(tasks[0]["status"], "pending");
}

#[test]
fn llm_show_displays_task_details() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let created = harness.output_json(&[
        "llm",
        "task",
        "entity_extract",
        "--text",
        "ARCHITECTURE: The query pipeline fans into graph lookup and priming.",
    ]);
    let task_id = created["task_id"].as_str().expect("task_id");

    let shown = harness.output_json(&["llm", "show", task_id]);
    assert_eq!(
        shown["id"].as_str().unwrap_or(""),
        task_id,
        "show should return the correct task"
    );
    assert_eq!(shown["kind"], "entity_extract");
    assert_eq!(shown["status"], "pending");
}

#[test]
fn llm_apply_with_valid_result_applies() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let created = harness.output_json(&[
        "llm",
        "task",
        "entity_extract",
        "--text",
        "ARCHITECTURE: The MemoryState struct stores entries with trigram-hashed embeddings.",
    ]);
    let task_id = created["task_id"].as_str().expect("task_id");

    let result_json = r#"{"confidence": 0.85, "entities": [{"label": "MemoryState", "kind": "Struct", "context": "defines", "confidence": 0.9}, {"label": "trigram hashing", "kind": "Algorithm", "context": "uses", "confidence": 0.8}]}"#;

    let applied = harness.output_json(&["llm", "apply", task_id, "--result", result_json]);
    assert!(
        applied["status"] == "applied" || applied["summary"]["status"] == "applied",
        "should apply successfully: {}",
        applied
    );
}

#[test]
fn llm_apply_low_confidence_rejects() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let created = harness.output_json(&[
        "llm",
        "task",
        "entity_extract",
        "--text",
        "ARCHITECTURE: The MemoryState struct stores entries with embeddings.",
    ]);
    let task_id = created["task_id"].as_str().expect("task_id");

    let result_json = r#"{"confidence": 0.3, "entities": [{"label": "MemoryState", "kind": "Struct", "context": "defines", "confidence": 0.3}]}"#;

    let applied = harness.output_json(&["llm", "apply", task_id, "--result", result_json]);
    assert_eq!(
        applied["status"], "rejected",
        "low confidence should be rejected: {}",
        applied
    );
}

#[test]
fn llm_task_invalid_kind_returns_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["llm", "task", "invalid_kind", "--text", "some text"]);
    assert!(
        !output.status.success(),
        "invalid kind should fail"
    );
    assert!(
        output.stderr.contains("Invalid") || output.stderr.contains("invalid"),
        "should mention invalid kind: {}",
        output.stderr
    );
}

#[test]
fn llm_task_no_input_returns_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["llm", "task", "entity_extract"]);
    assert!(
        !output.status.success(),
        "task with no input should fail"
    );
}

#[test]
fn llm_show_nonexistent_task_returns_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["llm", "show", "llm_nonexistent_999"]);
    assert!(
        !output.status.success(),
        "showing nonexistent task should fail"
    );
    assert!(
        output.stderr.contains("not found"),
        "should mention task not found: {}",
        output.stderr
    );
}

#[test]
fn llm_apply_nonexistent_task_returns_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&[
        "llm",
        "apply",
        "llm_nonexistent_999",
        "--result",
        r#"{"confidence": 0.9}"#,
    ]);
    assert!(
        !output.status.success(),
        "applying to nonexistent task should fail"
    );
}

#[test]
fn llm_signals_no_input_returns_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["llm", "signals"]);
    assert!(
        !output.status.success(),
        "signals with no input should fail"
    );
}

#[test]
fn llm_show_no_task_id_returns_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&["llm", "show"]);
    assert!(
        !output.status.success(),
        "show with no task_id should fail"
    );
}

#[test]
fn llm_list_empty_returns_empty_array() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let list = harness.output_json(&["llm", "list"]);
    let tasks = list.as_array().expect("list should be array");
    assert!(tasks.is_empty(), "list with no tasks should be empty");
}

#[test]
fn llm_task_query_rerank_requires_input_json() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let output = harness.cmd(&[
        "llm",
        "task",
        "query_rerank",
        "--text",
        "some text without candidates",
    ]);
    assert!(
        !output.status.success(),
        "query_rerank with --text should fail (requires --input-json)"
    );
}

#[test]
fn llm_list_with_count_limits_results() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Create two tasks of different kinds
    harness.cmd_ok(&[
        "llm",
        "task",
        "entity_extract",
        "--text",
        "ARCHITECTURE: The MemoryState struct stores entries with trigram-hashed embeddings.",
    ]);
    harness.cmd_ok(&[
        "llm",
        "task",
        "cluster_summary",
        "--text",
        "The query pipeline routes through graph lookup. The query pipeline coordinates graph lookup.",
    ]);

    let list = harness.output_json(&["llm", "list", "1"]);
    let tasks = list.as_array().expect("list array");
    assert_eq!(
        tasks.len(),
        1,
        "list with count=1 should return only 1 task"
    );
}
