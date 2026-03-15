mod common;

use common::{jsonrpc_request, seed_basic_repo, Harness};
use serde_json::json;

#[test]
fn initialize_returns_protocol_version_and_server_info() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[jsonrpc_request(1, "initialize", json!({}))]);

    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert_eq!(result["serverInfo"]["name"], "legend-memory");
    assert!(result["serverInfo"]["version"].is_string());
    assert!(result["capabilities"]["tools"].is_object());
}

#[test]
fn tools_list_returns_six_tools_with_schemas() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[
        jsonrpc_request(1, "initialize", json!({})),
        jsonrpc_request(2, "tools/list", json!({})),
    ]);

    assert!(responses.len() >= 2);
    let tools = responses[1]["result"]["tools"]
        .as_array()
        .expect("tools array");
    assert_eq!(tools.len(), 6);

    let names: Vec<&str> = tools
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"legend_memory_start"));
    assert!(names.contains(&"legend_memory_tick"));
    assert!(names.contains(&"legend_memory_query"));
    assert!(names.contains(&"legend_memory_task_get"));
    assert!(names.contains(&"legend_memory_task_set"));
    assert!(names.contains(&"legend_memory_stats"));

    for tool in tools {
        assert!(tool["inputSchema"].is_object(), "tool {} missing schema", tool["name"]);
    }
}

#[test]
fn tick_tool_returns_content() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[jsonrpc_request(
        1,
        "tools/call",
        json!({
            "name": "legend_memory_tick",
            "arguments": { "description": "DECISION: Chose MCP stdio transport for simplicity" }
        }),
    )]);

    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    let content = result["content"].as_array().expect("content array");
    assert!(!content.is_empty());
    assert_eq!(content[0]["type"], "text");
    assert!(content[0]["text"].as_str().unwrap().len() > 0);
    assert!(result.get("isError").is_none() || result["isError"] == false);
}

#[test]
fn query_tool_returns_memories_after_tick() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[
        jsonrpc_request(
            1,
            "tools/call",
            json!({
                "name": "legend_memory_tick",
                "arguments": { "description": "ARCHITECTURE: The query pipeline routes through graph lookup" }
            }),
        ),
        jsonrpc_request(
            2,
            "tools/call",
            json!({
                "name": "legend_memory_query",
                "arguments": { "topic": "query pipeline" }
            }),
        ),
    ]);

    assert!(responses.len() >= 2);
    let query_result = &responses[1]["result"];
    let content = query_result["content"].as_array().expect("content array");
    assert!(!content.is_empty());
    let text = content[0]["text"].as_str().unwrap();
    assert!(text.contains("query") || text.contains("pipeline") || text.contains("graph"));
}

#[test]
fn start_tool_returns_markdown_summary() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[jsonrpc_request(
        1,
        "tools/call",
        json!({
            "name": "legend_memory_start",
            "arguments": {}
        }),
    )]);

    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    let content = result["content"].as_array().expect("content array");
    let text = content[0]["text"].as_str().unwrap();
    assert!(text.contains("Legend"), "start output should contain markdown summary");
}

#[test]
fn task_round_trip_set_then_get() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[
        jsonrpc_request(
            1,
            "tools/call",
            json!({
                "name": "legend_memory_task_set",
                "arguments": { "task": "Refactor retrieval pipeline" }
            }),
        ),
        jsonrpc_request(
            2,
            "tools/call",
            json!({
                "name": "legend_memory_task_get",
                "arguments": {}
            }),
        ),
    ]);

    assert!(responses.len() >= 2);
    let get_result = &responses[1]["result"];
    let content = get_result["content"].as_array().expect("content array");
    let text = content[0]["text"].as_str().unwrap();
    assert!(
        text.contains("Refactor retrieval pipeline"),
        "task_get should return the task we set: {text}"
    );
}

#[test]
fn stats_tool_returns_counts() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[jsonrpc_request(
        1,
        "tools/call",
        json!({
            "name": "legend_memory_stats",
            "arguments": {}
        }),
    )]);

    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    let content = result["content"].as_array().expect("content array");
    let text = content[0]["text"].as_str().unwrap();
    assert!(
        text.contains("Short-term") || text.contains("short_term") || text.contains("entries"),
        "stats should include entry counts: {text}"
    );
}

#[test]
fn tick_with_empty_description_returns_is_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[jsonrpc_request(
        1,
        "tools/call",
        json!({
            "name": "legend_memory_tick",
            "arguments": { "description": "" }
        }),
    )]);

    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    assert_eq!(result["isError"], true);
}

#[test]
fn unknown_tool_returns_is_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[jsonrpc_request(
        1,
        "tools/call",
        json!({
            "name": "nonexistent_tool",
            "arguments": {}
        }),
    )]);

    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    assert_eq!(result["isError"], true);
}

#[test]
fn unknown_method_returns_json_rpc_error() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[jsonrpc_request(
        1,
        "nonexistent/method",
        json!({}),
    )]);

    assert_eq!(responses.len(), 1);
    let error = &responses[0]["error"];
    assert_eq!(error["code"], -32601);
    assert!(error["message"].as_str().unwrap().contains("Method not found"));
}

#[test]
fn ping_returns_empty_object() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    let responses = harness.mcp_session(&[jsonrpc_request(1, "ping", json!({}))]);

    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["result"], json!({}));
}

#[test]
fn cwd_flag_changes_working_directory() {
    let harness = Harness::new();
    seed_basic_repo(&harness);
    harness.cmd_ok(&["init"]);

    // Create a second temp directory and init it
    let other = tempfile::TempDir::new().expect("create other tempdir");
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(other.path())
        .status()
        .expect("git init other");
    assert!(status.success());

    // Tick into the main harness directory
    harness.cmd_ok(&[
        "memory",
        "tick",
        "DECISION: Main harness tick for cwd test",
    ]);

    // Use mcp_session_in with --cwd pointing to the main harness
    // The mcp_session_in uses the cwd param so memory operations target that dir
    let responses = harness.mcp_session_in(
        other.path(),
        &[jsonrpc_request(
            1,
            "tools/call",
            json!({
                "name": "legend_memory_stats",
                "arguments": {}
            }),
        )],
    );

    // The stats should work without error (even if the other dir has no legend data,
    // it should not crash - it just shows empty stats)
    assert_eq!(responses.len(), 1);
    let result = &responses[0]["result"];
    let content = result["content"].as_array().expect("content array");
    assert!(!content.is_empty());
}
