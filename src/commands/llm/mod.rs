mod helpers;

use crate::cli::{parse_args, CommandDef, FlagDef};
use crate::memory::MemoryState;
use helpers::*;
use serde_json::{json, Value};
use std::cmp::Ordering;

pub use helpers::LlmAutoTriggerSummary;

pub static COMMAND: CommandDef = CommandDef {
    name: "llm",
    about: "Policy-driven LLM task orchestration and validation",
    usage: "legend llm <subcommand> [options]",
    flags: &[],
    positionals: &[],
};

pub fn handle_llm(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        print_llm_help();
        return Ok(());
    }

    match args[0].as_str() {
        "signals" => handle_signals(&args[1..]),
        "task" => handle_task(&args[1..]),
        "apply" => handle_apply(&args[1..]),
        "list" => handle_list(&args[1..]),
        "show" => handle_show(&args[1..]),
        _ => {
            print_llm_help();
            Ok(())
        }
    }
}

/// Analyze text and automatically enqueue pending LLM tasks for recommended kinds.
/// This is used by `memory tick` to trigger LLM workflow without manual `llm signals`.
pub fn auto_trigger_for_text(
    text: &str,
    source: &str,
    source_tick: Option<u64>,
) -> Result<LlmAutoTriggerSummary, Box<dyn std::error::Error>> {
    let trigger = analyze_text_for_llm(text);
    if !trigger.needs_llm {
        return Ok(LlmAutoTriggerSummary {
            needs_llm: false,
            created_task_ids: Vec::new(),
            recommended_kinds: Vec::new(),
        });
    }

    let mut tasks = load_tasks()?;
    let fingerprint = text_fingerprint(text);
    let mut created_task_ids = Vec::new();
    let mut recommended_kinds = Vec::new();

    for rec in &trigger.recommended_tasks {
        recommended_kinds.push(rec.kind.clone());

        let Some(kind) = LlmTaskKind::parse(&rec.kind) else {
            continue;
        };
        if kind == LlmTaskKind::QueryRerank {
            continue; // query rerank needs candidates and is caller-driven
        }
        if !should_enqueue_auto_task(&tasks, &kind, &fingerprint, source_tick) {
            continue;
        }

        let input = json!({ "text": text });
        let id = create_task_record(
            &mut tasks,
            kind,
            input,
            source.to_string(),
            trigger.clone(),
            source_tick,
        );
        created_task_ids.push(id);
    }

    if !created_task_ids.is_empty() {
        save_tasks(&tasks)?;
    }

    Ok(LlmAutoTriggerSummary {
        needs_llm: true,
        created_task_ids,
        recommended_kinds,
    })
}

fn handle_signals(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let text = parse_text_or_stdin(args)?;
    if text.trim().is_empty() {
        return Err("Usage: legend llm signals <text> (or pipe text via stdin)".into());
    }

    let report = analyze_text_for_llm(&text);
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn handle_task(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err("Usage: legend llm task <entity_extract|cluster_summary|query_rerank> [--text <text> | --input-json <json>] [--source <label>]".into());
    }

    let kind = LlmTaskKind::parse(&args[0])
        .ok_or("Invalid task kind. Use one of: entity_extract, cluster_summary, query_rerank")?;

    static TASK_CMD: CommandDef = CommandDef {
        name: "task",
        about: "Create a typed LLM task payload",
        usage: "legend llm task <kind> [--text <text>] [--input-json <json>] [--source <label>]",
        flags: &[
            FlagDef { long: "--input-json", short: None, about: "JSON input payload", takes_value: true },
            FlagDef { long: "--text", short: None, about: "Text input", takes_value: true },
            FlagDef { long: "--source", short: None, about: "Source label", takes_value: true },
        ],
        positionals: &[],
    };

    let parsed = parse_args(&args[1..], &TASK_CMD);
    let input_json: Option<Value> = parsed
        .get("input-json")
        .map(|s| serde_json::from_str(s))
        .transpose()?;
    let text_arg = parsed.get("text").map(|s| s.to_string());
    let source = parsed.get("source").unwrap_or("manual").to_string();

    let input = if let Some(v) = input_json {
        validate_task_input(&kind, &v)?;
        v
    } else {
        let text = if let Some(t) = text_arg {
            t
        } else if !parsed.positional.is_empty() {
            parsed.positional.join(" ")
        } else {
            read_stdin()?
        };

        if text.trim().is_empty() {
            return Err(
                "No input provided. Pass --text, --input-json, trailing text, or stdin".into(),
            );
        }

        match kind {
            LlmTaskKind::EntityExtract | LlmTaskKind::ClusterSummary => json!({ "text": text }),
            LlmTaskKind::QueryRerank => {
                return Err(
                    "query_rerank requires --input-json with {\"query\":...,\"candidates\":[...]}."
                        .into(),
                );
            }
        }
    };

    let mut tasks = load_tasks()?;
    let trigger = analyze_input_for_llm(&kind, &input);
    let id = create_task_record(&mut tasks, kind, input, source, trigger, None);
    let record = tasks
        .iter()
        .find(|t| t.id == id)
        .cloned()
        .ok_or("Failed to load created task record")?;
    save_tasks(&tasks)?;

    let output = json!({
        "task_id": record.id,
        "kind": record.kind,
        "trigger": record.trigger,
        "input": record.input,
        "prompt": record.prompt,
        "json_schema": record.json_schema,
        "acceptance_rules": record.acceptance_rules,
        "instructions": {
            "step_1": "Call your LLM with prompt + input and enforce JSON-only output.",
            "step_2": "Submit model output via: legend llm apply <task_id> --result '<json>'",
            "step_3": "If apply returns rejected, keep deterministic Legend output."
        }
    });

    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn handle_apply(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err("Usage: legend llm apply <task_id> [--result <json>]".into());
    }

    static APPLY_CMD: CommandDef = CommandDef {
        name: "apply",
        about: "Validate and apply a model result",
        usage: "legend llm apply <task_id> [--result <json>]",
        flags: &[
            FlagDef { long: "--result", short: None, about: "JSON result payload", takes_value: true },
        ],
        positionals: &[],
    };

    let parsed = parse_args(args, &APPLY_CMD);
    let task_id = parsed
        .positional
        .first()
        .ok_or("Usage: legend llm apply <task_id> [--result <json>]")?;
    let result_json: Option<Value> = parsed
        .get("result")
        .map(|s| serde_json::from_str(s))
        .transpose()?;

    let result = if let Some(v) = result_json {
        v
    } else {
        let stdin = read_stdin()?;
        if stdin.trim().is_empty() {
            return Err("No result provided. Pass --result <json> or pipe JSON via stdin.".into());
        }
        serde_json::from_str(&stdin)?
    };

    let mut tasks = load_tasks()?;
    let Some(task_idx) = tasks.iter().position(|t| t.id == *task_id) else {
        return Err(format!("Task '{}' not found", task_id).into());
    };

    if tasks[task_idx].status != TaskStatus::Pending {
        return Err(format!(
            "Task '{}' is not pending (current status: {:?})",
            task_id, tasks[task_idx].status
        )
        .into());
    }

    let validation = validate_result(&tasks[task_idx].kind, &result)?;
    let now = now_ts();

    tasks[task_idx].result = Some(validation.normalized_result.clone());
    tasks[task_idx].updated_ts = now;

    if !validation.accepted {
        tasks[task_idx].status = TaskStatus::Rejected;
        tasks[task_idx].apply_summary = Some(json!({
            "status": "rejected",
            "reason": validation.reason,
        }));
        let completed = tasks.remove(task_idx);
        archive_tasks(&[completed])?;
        save_tasks(&tasks)?;

        println!(
            "{}",
            serde_json::to_string(&json!({
                "task_id": task_id,
                "status": "rejected",
                "reason": validation.reason,
            }))?
        );
        return Ok(());
    }

    let apply_summary = match tasks[task_idx].kind {
        LlmTaskKind::EntityExtract => {
            let entities = validation.entities;
            let task_conf = validation.task_confidence;
            let source_text = tasks[task_idx]
                .input
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let mut memory = MemoryState::load_or_default()?;
            let applied = memory.apply_llm_entities(source_text, &entities, task_conf);
            memory.save()?;

            json!({
                "status": "applied",
                "memory_updated": true,
                "accepted_entities": applied.accepted_entities,
                "created_nodes": applied.created_nodes,
                "updated_nodes": applied.updated_nodes,
                "edges_reinforced": applied.edges_reinforced,
            })
        }
        LlmTaskKind::ClusterSummary => json!({
            "status": "recorded",
            "memory_updated": false,
            "note": "Cluster summary output stored for caller-controlled usage.",
        }),
        LlmTaskKind::QueryRerank => json!({
            "status": "recorded",
            "memory_updated": false,
            "note": "Query rerank output stored for caller-side retrieval ordering.",
        }),
    };

    tasks[task_idx].status = TaskStatus::Applied;
    tasks[task_idx].apply_summary = Some(apply_summary.clone());
    let completed = tasks.remove(task_idx);
    archive_tasks(&[completed])?;
    save_tasks(&tasks)?;

    println!(
        "{}",
        serde_json::to_string(&json!({
            "task_id": task_id,
            "status": "applied",
            "summary": apply_summary,
        }))?
    );

    Ok(())
}

fn handle_list(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    static LIST_CMD: CommandDef = CommandDef {
        name: "list",
        about: "Show recent LLM tasks",
        usage: "legend llm list [limit]",
        flags: &[],
        positionals: &[],
    };

    let parsed = parse_args(args, &LIST_CMD);
    let limit = parsed
        .positional
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(20);

    let tasks = load_tasks()?;
    let mut recent: Vec<&LlmTaskRecord> = tasks.iter().collect();
    recent.sort_by(|a, b| b.created_ts.cmp(&a.created_ts));
    recent.truncate(limit);

    let output: Vec<Value> = recent
        .into_iter()
        .map(|t| {
            json!({
                "task_id": t.id,
                "kind": t.kind,
                "status": t.status,
                "created_ts": t.created_ts,
                "updated_ts": t.updated_ts,
                "source": t.source,
            })
        })
        .collect();

    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn handle_show(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    static SHOW_CMD: CommandDef = CommandDef {
        name: "show",
        about: "Show full task record",
        usage: "legend llm show <task_id>",
        flags: &[],
        positionals: &[],
    };

    let parsed = parse_args(args, &SHOW_CMD);
    let task_id = parsed
        .positional
        .first()
        .ok_or("Usage: legend llm show <task_id>")?;
    let tasks = load_tasks()?;
    if let Some(task) = tasks.into_iter().find(|t| t.id == *task_id) {
        println!("{}", serde_json::to_string(&task)?);
        return Ok(());
    }

    let archived = load_archived_tasks()?;
    let Some(task) = archived.into_iter().find(|t| t.id == *task_id) else {
        return Err(format!("Task '{}' not found", task_id).into());
    };
    println!("{}", serde_json::to_string(&task)?);
    Ok(())
}

// ---------------------------------------------------------------------------
// Signal analysis
// ---------------------------------------------------------------------------

fn analyze_input_for_llm(kind: &LlmTaskKind, input: &Value) -> SignalReport {
    match kind {
        LlmTaskKind::EntityExtract | LlmTaskKind::ClusterSummary => {
            let text = input.get("text").and_then(|v| v.as_str()).unwrap_or("");
            analyze_text_for_llm(text)
        }
        LlmTaskKind::QueryRerank => {
            let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
            let candidates = input
                .get("candidates")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            SignalReport {
                needs_llm: candidates >= 3 || query.contains(" or "),
                recommended_tasks: vec![TaskRecommendation {
                    kind: "query_rerank".to_string(),
                    reason: format!("{} retrieval candidates available", candidates),
                    priority: 1,
                }],
                metrics: SignalMetrics {
                    chars: query.len(),
                    words: query.split_whitespace().count(),
                    entities: 0,
                    high_signal_entities: 0,
                    entity_density: 0.0,
                    high_signal_density: 0.0,
                },
            }
        }
    }
}

fn analyze_text_for_llm(text: &str) -> SignalReport {
    let kw = crate::memory::keyword_cache::KeywordCache::default_from_static();
    let entities = crate::memory::extract::extract_entities(text, &kw);
    let chars = text.chars().count();
    let words = text.split_whitespace().count();
    let entity_count = entities.len();
    let high_signal_count = entities.iter().filter(|e| e.kind != "Term").count();
    let density = if words == 0 {
        0.0
    } else {
        entity_count as f32 / words as f32
    };
    let high_signal_density = if words == 0 {
        0.0
    } else {
        high_signal_count as f32 / words as f32
    };

    let mut tasks = Vec::new();

    if words >= 35 && high_signal_density < 0.05 {
        tasks.push(TaskRecommendation {
            kind: "entity_extract".to_string(),
            reason: "Low high-signal entity density for long text; deterministic extractor may miss domain-specific anchors".to_string(),
            priority: 1,
        });
    }

    let separator_count =
        text.matches('|').count() + text.matches('\n').count() + text.matches(';').count();
    if chars >= 320 || separator_count >= 3 {
        tasks.push(TaskRecommendation {
            kind: "cluster_summary".to_string(),
            reason:
                "Input appears multi-part/long; abstractive summary can improve milestone labeling"
                    .to_string(),
            priority: 2,
        });
    }

    let lower = text.to_ascii_lowercase();
    if text.contains('?') && (lower.contains(" or ") || lower.contains("which ") || words > 18) {
        tasks.push(TaskRecommendation {
            kind: "query_rerank".to_string(),
            reason: "Query appears ambiguous or broad; reranking can improve retrieval precision"
                .to_string(),
            priority: 3,
        });
    }

    tasks.sort_by(|a, b| {
        let p = a.priority.cmp(&b.priority);
        if p == Ordering::Equal {
            a.kind.cmp(&b.kind)
        } else {
            p
        }
    });

    SignalReport {
        needs_llm: !tasks.is_empty(),
        recommended_tasks: tasks,
        metrics: SignalMetrics {
            chars,
            words,
            entities: entity_count,
            high_signal_entities: high_signal_count,
            entity_density: density,
            high_signal_density,
        },
    }
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------

fn print_llm_help() {
    println!("Legend LLM - policy-driven LLM augmentation workflow");
    println!();
    println!("Usage:");
    println!("  legend llm signals <text>          Analyze whether LLM augmentation is needed");
    println!("  legend llm task <kind> [options]   Create a typed LLM task payload");
    println!("    kinds: entity_extract | cluster_summary | query_rerank");
    println!("    options: --text <text> | --input-json <json> | --source <label>");
    println!("  legend llm apply <task_id> [--result <json>]  Validate/apply a model result");
    println!("  legend llm list [n]                Show recent tasks (default 20)");
    println!("  legend llm show <task_id>          Show full task record (pending or archived)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_signal_report_low_density_recommends_entity_extract() {
        let text = "The team discussed how the service behaved during the release, and we described the incident in broad language without naming concrete modules, files, function names, infrastructure providers, or tooling details, so the report remained high level and hard to anchor to specific entities for retrieval.";
        let report = analyze_text_for_llm(text);
        assert!(report
            .recommended_tasks
            .iter()
            .any(|t| t.kind == "entity_extract"));
    }

    #[test]
    fn test_validate_entity_result_rejects_low_confidence() {
        let result = json!({
            "confidence": 0.40,
            "entities": [
                {"label": "postgres", "kind": "Tool", "context": "mentions", "confidence": 0.9}
            ]
        });
        let out = validate_result(&LlmTaskKind::EntityExtract, &result).unwrap();
        assert!(!out.accepted);
    }

    #[test]
    fn test_validate_entity_result_accepts_valid_payload() {
        let result = json!({
            "confidence": 0.80,
            "entities": [
                {"label": "postgres", "kind": "Tool", "context": "mentions", "confidence": 0.9},
                {"label": "QueryPlanner", "kind": "Class", "context": "defines", "confidence": 0.8}
            ]
        });
        let out = validate_result(&LlmTaskKind::EntityExtract, &result).unwrap();
        assert!(out.accepted);
        assert_eq!(out.entities.len(), 2);
    }

    #[test]
    fn test_auto_guardrail_dedupes_same_fingerprint_pending() {
        let fp = text_fingerprint("Same low anchor text");
        let tasks = vec![LlmTaskRecord {
            id: "t1".into(),
            kind: LlmTaskKind::EntityExtract,
            status: TaskStatus::Pending,
            created_ts: 1,
            updated_ts: 1,
            source: "memory_tick".into(),
            trigger: analyze_text_for_llm("dummy"),
            input: json!({"text": "Same low anchor text"}),
            prompt: "".into(),
            json_schema: json!({}),
            acceptance_rules: json!({}),
            result: None,
            apply_summary: None,
            input_fingerprint: Some(fp.clone()),
            source_tick: Some(10),
        }];
        assert!(!should_enqueue_auto_task(
            &tasks,
            &LlmTaskKind::EntityExtract,
            &fp,
            Some(11)
        ));
    }

    #[test]
    fn test_auto_guardrail_tick_gap_blocks_nearby_tasks() {
        let tasks = vec![LlmTaskRecord {
            id: "t1".into(),
            kind: LlmTaskKind::ClusterSummary,
            status: TaskStatus::Pending,
            created_ts: 1,
            updated_ts: 1,
            source: "memory_tick".into(),
            trigger: analyze_text_for_llm("dummy"),
            input: json!({"text": "x"}),
            prompt: "".into(),
            json_schema: json!({}),
            acceptance_rules: json!({}),
            result: None,
            apply_summary: None,
            input_fingerprint: Some(text_fingerprint("x")),
            source_tick: Some(20),
        }];

        assert!(!should_enqueue_auto_task(
            &tasks,
            &LlmTaskKind::ClusterSummary,
            &text_fingerprint("different text"),
            Some(23)
        ));
        assert!(should_enqueue_auto_task(
            &tasks,
            &LlmTaskKind::ClusterSummary,
            &text_fingerprint("different text"),
            Some(25)
        ));
    }

    // -----------------------------------------------------------------------
    // analyze_text_for_llm
    // -----------------------------------------------------------------------

    #[test]
    fn test_analyze_short_text_no_llm() {
        let report = analyze_text_for_llm("Fixed a bug");
        assert!(!report.needs_llm);
        assert!(report.recommended_tasks.is_empty());
    }

    #[test]
    fn test_analyze_long_multipart_recommends_cluster_summary() {
        let text = "First we fixed the parser. | Then we updated the serializer. | Finally we ran all tests and verified the output matched expectations. | Additional cleanup was performed on unused imports and dead code paths.";
        let report = analyze_text_for_llm(text);
        assert!(
            report.recommended_tasks.iter().any(|t| t.kind == "cluster_summary"),
            "Long multi-part text should recommend cluster_summary, got: {:?}",
            report.recommended_tasks.iter().map(|t| &t.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_analyze_ambiguous_query_recommends_rerank() {
        let text = "Which approach should we use for caching, or is there a better alternative for handling distributed state across multiple services in production?";
        let report = analyze_text_for_llm(text);
        assert!(
            report.recommended_tasks.iter().any(|t| t.kind == "query_rerank"),
            "Ambiguous query should recommend rerank, got: {:?}",
            report.recommended_tasks.iter().map(|t| &t.kind).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_analyze_metrics_populated() {
        let text = "Fixed a bug in src/main.rs";
        let report = analyze_text_for_llm(text);
        assert!(report.metrics.chars > 0);
        assert!(report.metrics.words > 0);
    }

    #[test]
    fn test_analyze_tasks_sorted_by_priority() {
        // Long text with separators and a question — should trigger multiple
        let text = "The team discussed how the service behaved during the release and we described the incident in broad language without naming concrete modules or files or function names or infrastructure providers or tooling details so the report remained high level? | Is it good or bad that we also added a new cache layer and refactored the query pipeline and updated all documentation?";
        let report = analyze_text_for_llm(text);
        if report.recommended_tasks.len() >= 2 {
            assert!(
                report.recommended_tasks[0].priority <= report.recommended_tasks[1].priority,
                "Tasks should be sorted by priority"
            );
        }
    }

    // -----------------------------------------------------------------------
    // text_fingerprint
    // -----------------------------------------------------------------------

    #[test]
    fn test_text_fingerprint_deterministic() {
        let a = text_fingerprint("Hello, world!");
        let b = text_fingerprint("Hello, world!");
        assert_eq!(a, b);
    }

    #[test]
    fn test_text_fingerprint_case_insensitive() {
        let a = text_fingerprint("Hello World");
        let b = text_fingerprint("hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn test_text_fingerprint_normalizes_punctuation() {
        let a = text_fingerprint("hello, world!");
        let b = text_fingerprint("hello world");
        assert_eq!(a, b);
    }

    #[test]
    fn test_text_fingerprint_different_text_different_hash() {
        assert_ne!(text_fingerprint("hello"), text_fingerprint("world"));
    }

    // -----------------------------------------------------------------------
    // should_enqueue_auto_task — additional coverage
    // -----------------------------------------------------------------------

    #[test]
    fn test_should_enqueue_empty_tasks() {
        assert!(should_enqueue_auto_task(
            &[] as &[LlmTaskRecord],
            &LlmTaskKind::EntityExtract,
            &text_fingerprint("test"),
            Some(100)
        ));
    }

    #[test]
    fn test_should_enqueue_respects_pending_cap() {
        let tasks: Vec<LlmTaskRecord> = (0..3)
            .map(|i| LlmTaskRecord {
                id: format!("t{}", i),
                kind: LlmTaskKind::EntityExtract,
                status: TaskStatus::Pending,
                created_ts: 1,
                updated_ts: 1,
                source: "memory_tick".into(),
                trigger: analyze_text_for_llm("dummy"),
                input: json!({"text": format!("unique text {}", i)}),
                prompt: "".into(),
                json_schema: json!({}),
                acceptance_rules: json!({}),
                result: None,
                apply_summary: None,
                input_fingerprint: Some(text_fingerprint(&format!("unique text {}", i))),
                source_tick: Some(i as u64),
            })
            .collect();

        // Should reject — already at cap (3)
        assert!(!should_enqueue_auto_task(
            &tasks,
            &LlmTaskKind::EntityExtract,
            &text_fingerprint("brand new text"),
            Some(100)
        ));
    }

    #[test]
    fn test_should_enqueue_different_kind_not_blocked() {
        let tasks = vec![LlmTaskRecord {
            id: "t1".into(),
            kind: LlmTaskKind::EntityExtract,
            status: TaskStatus::Pending,
            created_ts: 1,
            updated_ts: 1,
            source: "memory_tick".into(),
            trigger: analyze_text_for_llm("dummy"),
            input: json!({"text": "x"}),
            prompt: "".into(),
            json_schema: json!({}),
            acceptance_rules: json!({}),
            result: None,
            apply_summary: None,
            input_fingerprint: Some(text_fingerprint("x")),
            source_tick: Some(10),
        }];

        // Different kind should not be blocked by EntityExtract
        assert!(should_enqueue_auto_task(
            &tasks,
            &LlmTaskKind::ClusterSummary,
            &text_fingerprint("different"),
            Some(100)
        ));
    }

    // -----------------------------------------------------------------------
    // LlmTaskKind::parse
    // -----------------------------------------------------------------------

    #[test]
    fn test_llm_task_kind_parse() {
        assert_eq!(LlmTaskKind::parse("entity_extract"), Some(LlmTaskKind::EntityExtract));
        assert_eq!(LlmTaskKind::parse("extract"), Some(LlmTaskKind::EntityExtract));
        assert_eq!(LlmTaskKind::parse("cluster_summary"), Some(LlmTaskKind::ClusterSummary));
        assert_eq!(LlmTaskKind::parse("summary"), Some(LlmTaskKind::ClusterSummary));
        assert_eq!(LlmTaskKind::parse("query_rerank"), Some(LlmTaskKind::QueryRerank));
        assert_eq!(LlmTaskKind::parse("rerank"), Some(LlmTaskKind::QueryRerank));
        assert_eq!(LlmTaskKind::parse("unknown"), None);
    }
}
