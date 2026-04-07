use super::memory::{
    append_to_architecture_md, format_start_summary_markdown, is_noise_tick, log_event,
    log_event_rich, truncate_text, EventData, GraphHit, MatchedEntry, QueryEventData,
    StartEventData, TickEventData,
};
use crate::cli::{parse_args, CommandDef};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

fn make_success_response(id: &Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn make_error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        },
    })
}

fn write_response(stdout: &mut io::StdoutLock, response: &Value) -> io::Result<()> {
    let line = serde_json::to_string(response).unwrap_or_else(|_| "{}".to_string());
    writeln!(stdout, "{}", line)?;
    stdout.flush()
}

// ---------------------------------------------------------------------------
// MCP Protocol Handlers
// ---------------------------------------------------------------------------

fn handle_initialize(id: &Value) -> Value {
    make_success_response(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {},
            },
            "serverInfo": {
                "name": "legend-memory",
                "version": env!("CARGO_PKG_VERSION"),
            },
        }),
    )
}

fn handle_tools_list(id: &Value) -> Value {
    let tools = json!([
        {
            "name": "legend_memory_start",
            "description": "Start session. Returns categorized memories and recent activity.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Optional category filter (bugs, todos, decisions, architecture, preferences)"
                    }
                },
                "required": []
            }
        },
        {
            "name": "legend_memory_tick",
            "description": "Record a decision/discovery/insight. Prefix: DECISION:, BUG:, ARCHITECTURE:, BLOCKER:",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "What happened — include rationale for decisions (e.g. 'DECISION: Chose X over Y because Z')"
                    }
                },
                "required": ["description"]
            }
        },
        {
            "name": "legend_memory_query",
            "description": "Search memory for topic context. Auto-reinforces top result.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "The topic to search for in memory"
                    }
                },
                "required": ["topic"]
            }
        },
        {
            "name": "legend_memory_task_get",
            "description": "Get current task.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        },
        {
            "name": "legend_memory_task_set",
            "description": "Set current task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The task description to set"
                    }
                },
                "required": ["task"]
            }
        },
        {
            "name": "legend_memory_stats",
            "description": "Memory statistics.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }
    ]);

    make_success_response(id, json!({ "tools": tools }))
}

// ---------------------------------------------------------------------------
// Tool Handlers
// ---------------------------------------------------------------------------

/// Execute a tool call by name, returning Ok(text) or Err(error message).
fn dispatch_tool(name: &str, arguments: &Value) -> Result<String, String> {
    match name {
        "legend_memory_start" => tool_memory_start(arguments),
        "legend_memory_tick" => tool_memory_tick(arguments),
        "legend_memory_query" => tool_memory_query(arguments),
        "legend_memory_task_get" => tool_memory_task_get(),
        "legend_memory_task_set" => tool_memory_task_set(arguments),
        "legend_memory_stats" => tool_memory_stats(),
        _ => Err(format!("Unknown tool: {}", name)),
    }
}

fn tool_memory_start(arguments: &Value) -> Result<String, String> {
    let category = arguments
        .get("category")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());

    let mut memory = crate::memory::load_or_default().map_err(|e| e.to_string())?;
    let mut summary = crate::memory::build_start_summary_with_options(
        &mut memory,
        false,
        category.as_deref(),
        None,
    );

    // Session log capacity warning
    const SESSION_LOG_WARNING_THRESHOLD: usize = 90;
    if memory.session_log.len() >= SESSION_LOG_WARNING_THRESHOLD {
        if let Some(obj) = summary.as_object_mut() {
            obj.insert(
                "warning".to_string(),
                json!(format!(
                    "Session log at {}% capacity ({}/100). Oldest entries will be dropped.",
                    memory.session_log.len(),
                    memory.session_log.len()
                )),
            );
        }
    }

    // Flush working memory: promote qualifying entries to L2, then clear L1
    crate::memory::prefrontal::flush_working_memory(&mut memory.brain);

    // Log event
    let event_data = EventData::Start(StartEventData {
        clock: memory.brain.clock,
        short_term_count: memory.brain.short_term.len(),
        long_term_nodes: memory.brain.long_term.nodes.len(),
        session_log_entries: memory.session_log.len(),
    });

    crate::memory::save(&memory).map_err(|e| e.to_string())?;
    log_event_rich("start", "session cold-start (MCP)", Some(event_data));

    let output = format_start_summary_markdown(&summary);
    Ok(output)
}

fn tool_memory_tick(arguments: &Value) -> Result<String, String> {
    let text = arguments
        .get("description")
        .and_then(|d| d.as_str())
        .ok_or_else(|| "Missing required argument: description".to_string())?;

    let text = text.trim();
    if text.is_empty() {
        return Err("Description cannot be empty".to_string());
    }

    if is_noise_tick(text) {
        return Err("Tick rejected: low-quality content detected".to_string());
    }

    let mut memory = crate::memory::load_or_default().map_err(|e| e.to_string())?;
    let tick_result = crate::memory::tick(&mut memory, text);
    crate::memory::save(&memory).map_err(|e| e.to_string())?;

    // Reset pending-tick counter
    let _ = std::fs::write(".legend/.pending_ticks", "0");

    // Log rich event data
    let event_data = EventData::Tick(TickEventData {
        entry_id: Some(tick_result.entry_id),
        matches: tick_result
            .context
            .short_term
            .iter()
            .take(5)
            .map(|m| MatchedEntry {
                id: m.id,
                similarity: m.similarity,
                text_preview: truncate_text(&m.text, 80),
            })
            .collect(),
        graph_nodes: tick_result
            .context
            .long_term
            .iter()
            .take(5)
            .map(|n| GraphHit {
                id: n.id,
                label: n.label.clone(),
                kind: n.kind.clone(),
                weight: n.weight,
            })
            .collect(),
    });
    log_event_rich("tick", text, Some(event_data));

    // Auto-append to ARCHITECTURE.md when tick is tagged with ARCHITECTURE:
    if text.to_uppercase().starts_with("ARCHITECTURE:") {
        append_to_architecture_md(text);
    }

    let output = json!({
        "action": tick_result.action,
        "entry_id": tick_result.entry_id,
    });
    Ok(serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string()))
}

fn tool_memory_query(arguments: &Value) -> Result<String, String> {
    let topic = arguments
        .get("topic")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "Missing required argument: topic".to_string())?;

    let topic = topic.trim();
    if topic.is_empty() {
        return Err("Topic cannot be empty".to_string());
    }

    let mut memory = crate::memory::load_or_default().map_err(|e| e.to_string())?;
    let context = crate::memory::retrieve_context(&mut memory.brain, topic);
    crate::memory::save(&memory).map_err(|e| e.to_string())?;

    // Count primed nodes
    let primed_count = context
        .long_term
        .iter()
        .filter(|n| n.edge_type.is_some())
        .count();

    // Log rich event data
    let event_data = EventData::Query(QueryEventData {
        matches: context
            .short_term
            .iter()
            .take(5)
            .map(|m| MatchedEntry {
                id: m.id,
                similarity: m.similarity,
                text_preview: truncate_text(&m.text, 80),
            })
            .collect(),
        graph_nodes: context
            .long_term
            .iter()
            .take(8)
            .map(|n| GraphHit {
                id: n.id,
                label: n.label.clone(),
                kind: n.kind.clone(),
                weight: n.weight,
            })
            .collect(),
        primed_count,
    });
    log_event_rich("query", topic, Some(event_data));

    let working_memory: Vec<&str> = context
        .working_memory
        .iter()
        .map(|m| m.text.as_str())
        .collect();
    let memories: Vec<&str> = context.short_term.iter().map(|m| m.text.as_str()).collect();
    let related_topics: Vec<&str> = context.long_term.iter().map(|n| n.label.as_str()).collect();

    let result = json!({
        "working_memory": working_memory,
        "memories": memories,
        "related_topics": related_topics,
    });
    Ok(serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string()))
}

fn tool_memory_task_get() -> Result<String, String> {
    let memory = crate::memory::load_or_default().map_err(|e| e.to_string())?;
    match crate::memory::get_task(&memory) {
        Some(task) => Ok(format!("Current task: {}", task)),
        None => Ok("No current task set".to_string()),
    }
}

fn tool_memory_task_set(arguments: &Value) -> Result<String, String> {
    let task = arguments
        .get("task")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "Missing required argument: task".to_string())?;

    let task = task.trim();
    if task.is_empty() {
        return Err("Task cannot be empty".to_string());
    }

    let mut memory = crate::memory::load_or_default().map_err(|e| e.to_string())?;
    crate::memory::set_task(&mut memory, task);
    crate::memory::save(&memory).map_err(|e| e.to_string())?;
    log_event("task_set", task);

    Ok(format!("Current task set: {}", task))
}

fn tool_memory_stats() -> Result<String, String> {
    let memory = crate::memory::load_or_default().map_err(|e| e.to_string())?;

    let mut out = String::new();
    out.push_str(&format!(
        "Working memory (L1): {}\n",
        memory.brain.working_memory.len()
    ));
    out.push_str(&format!(
        "Short-term entries: {}\n",
        memory.brain.short_term.len()
    ));
    out.push_str(&format!(
        "Long-term nodes: {}\n",
        memory.brain.long_term.nodes.len()
    ));
    out.push_str(&format!(
        "Long-term edges: {}\n",
        memory.brain.long_term.edges.len()
    ));
    out.push_str(&format!(
        "Ticks since consolidation: {}\n",
        memory.brain.ticks_since_consolidation
    ));
    if let Some(task) = crate::memory::get_task(&memory) {
        out.push_str(&format!("Current task: {}", task));
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// Tool call response builder
// ---------------------------------------------------------------------------

fn handle_tools_call(id: &Value, params: &Value) -> Value {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    match dispatch_tool(name, &arguments) {
        Ok(text) => make_success_response(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": text,
                }],
            }),
        ),
        Err(error_msg) => make_success_response(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": error_msg,
                }],
                "isError": true,
            }),
        ),
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn mcp_main_loop() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();
    let mut writer = stdout.lock();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[mcp] Failed to parse JSON: {}", e);
                continue;
            }
        };

        let id = msg.get("id");
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");

        // Notifications (no id) — no response needed
        if id.is_none() {
            // Silently ignore all notifications
            continue;
        }

        let id = id.unwrap();

        let response = match method {
            "initialize" => handle_initialize(id),
            "tools/list" => handle_tools_list(id),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                handle_tools_call(id, &params)
            }
            "ping" => make_success_response(id, json!({})),
            "resources/list" => make_success_response(id, json!({ "resources": [] })),
            "prompts/list" => make_success_response(id, json!({ "prompts": [] })),
            _ => make_error_response(id, -32601, &format!("Method not found: {}", method)),
        };

        write_response(&mut writer, &response)?;
    }

    Ok(())
}

/// Entry point for `legend mcp-serve`
pub fn handle_mcp_serve(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let parsed = parse_args(args, def);

    if let Some(cwd) = parsed.get("cwd") {
        std::env::set_current_dir(cwd)?;
    }

    mcp_main_loop()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_success_response() {
        let resp = make_success_response(&json!(1), json!({"key": "value"}));
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["key"], "value");
        assert!(resp.get("error").is_none());
    }

    #[test]
    fn test_make_error_response() {
        let resp = make_error_response(&json!(2), -32601, "Method not found");
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 2);
        assert_eq!(resp["error"]["code"], -32601);
        assert_eq!(resp["error"]["message"], "Method not found");
        assert!(resp.get("result").is_none());
    }

    #[test]
    fn test_handle_initialize() {
        let resp = handle_initialize(&json!(1));
        let result = &resp["result"];
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert!(result.get("capabilities").is_some());
        assert!(result["capabilities"].get("tools").is_some());
        assert_eq!(result["serverInfo"]["name"], "legend-memory");
        assert_eq!(result["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_handle_tools_list_returns_6_tools() {
        let resp = handle_tools_list(&json!(1));
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 6);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"legend_memory_start"));
        assert!(names.contains(&"legend_memory_tick"));
        assert!(names.contains(&"legend_memory_query"));
        assert!(names.contains(&"legend_memory_task_get"));
        assert!(names.contains(&"legend_memory_task_set"));
        assert!(names.contains(&"legend_memory_stats"));

        // Verify each tool has inputSchema
        for tool in tools {
            assert!(
                tool.get("inputSchema").is_some(),
                "Tool {} missing inputSchema",
                tool["name"]
            );
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn test_unknown_method_returns_error() {
        let id = json!(99);
        let resp = make_error_response(&id, -32601, "Method not found: foo/bar");
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn test_tick_missing_description() {
        let result = dispatch_tool("legend_memory_tick", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("description"));
    }

    #[test]
    fn test_tick_empty_description() {
        let result = dispatch_tool("legend_memory_tick", &json!({"description": "  "}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn test_query_missing_topic() {
        let result = dispatch_tool("legend_memory_query", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("topic"));
    }

    #[test]
    fn test_task_set_missing_task() {
        let result = dispatch_tool("legend_memory_task_set", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("task"));
    }

    #[test]
    fn test_unknown_tool_returns_error() {
        let result = dispatch_tool("nonexistent_tool", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown tool"));
    }

    #[test]
    fn test_tools_call_error_uses_is_error() {
        let resp = handle_tools_call(
            &json!(1),
            &json!({
                "name": "nonexistent_tool",
                "arguments": {}
            }),
        );
        // Tool errors are JSON-RPC successes with isError: true
        assert!(resp.get("error").is_none());
        assert_eq!(resp["result"]["isError"], true);
        assert!(resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Unknown tool"));
    }
}
