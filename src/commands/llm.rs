use crate::memory::{LlmEntity, MemoryState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, Read};
use std::path::Path;

const LLM_TASKS_PATH: &str = ".legend/llm_tasks.json";
const LLM_TASKS_ARCHIVE_PATH: &str = ".legend/llm_tasks_archive.lz4";
const MIN_ENTITY_TASK_CONFIDENCE: f32 = 0.65;
const MAX_ENTITY_APPLY: usize = 50;
const AUTO_MAX_PENDING_PER_KIND: usize = 3;
const AUTO_MIN_TICK_GAP_PER_KIND: u64 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LlmTaskKind {
    EntityExtract,
    ClusterSummary,
    QueryRerank,
}

impl LlmTaskKind {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "entity_extract" | "extract" => Some(Self::EntityExtract),
            "cluster_summary" | "summary" => Some(Self::ClusterSummary),
            "query_rerank" | "rerank" => Some(Self::QueryRerank),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Pending,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskRecommendation {
    kind: String,
    reason: String,
    priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignalMetrics {
    chars: usize,
    words: usize,
    entities: usize,
    high_signal_entities: usize,
    entity_density: f32,
    high_signal_density: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignalReport {
    needs_llm: bool,
    recommended_tasks: Vec<TaskRecommendation>,
    metrics: SignalMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LlmTaskRecord {
    id: String,
    kind: LlmTaskKind,
    status: TaskStatus,
    created_ts: u64,
    updated_ts: u64,
    source: String,
    trigger: SignalReport,
    input: Value,
    prompt: String,
    json_schema: Value,
    acceptance_rules: Value,
    result: Option<Value>,
    apply_summary: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_tick: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAutoTriggerSummary {
    pub needs_llm: bool,
    pub created_task_ids: Vec<String>,
    pub recommended_kinds: Vec<String>,
}

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

    let mut input_json: Option<Value> = None;
    let mut text_arg: Option<String> = None;
    let mut source = "manual".to_string();
    let mut i = 1usize;
    let mut trailing: Vec<&str> = Vec::new();

    while i < args.len() {
        match args[i].as_str() {
            "--input-json" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --input-json".into());
                }
                input_json = Some(serde_json::from_str(&args[i])?);
            }
            "--text" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --text".into());
                }
                text_arg = Some(args[i].clone());
            }
            "--source" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --source".into());
                }
                source = args[i].clone();
            }
            other => trailing.push(other),
        }
        i += 1;
    }

    let input = if let Some(v) = input_json {
        validate_task_input(&kind, &v)?;
        v
    } else {
        let text = if let Some(t) = text_arg {
            t
        } else if !trailing.is_empty() {
            trailing.join(" ")
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

fn create_task_record(
    tasks: &mut Vec<LlmTaskRecord>,
    kind: LlmTaskKind,
    input: Value,
    source: String,
    trigger: SignalReport,
    source_tick: Option<u64>,
) -> String {
    let schema = task_schema(&kind);
    let acceptance_rules = acceptance_rules(&kind);
    let prompt = task_prompt(&kind, &input, &schema, &acceptance_rules);
    let now = now_ts();
    let id = format!("llm_{}_{}", now, tasks.len() + 1);
    let input_fingerprint = extract_text_from_input(&input).map(text_fingerprint);

    tasks.push(LlmTaskRecord {
        id: id.clone(),
        kind,
        status: TaskStatus::Pending,
        created_ts: now,
        updated_ts: now,
        source,
        trigger,
        input,
        prompt,
        json_schema: schema,
        acceptance_rules,
        result: None,
        apply_summary: None,
        input_fingerprint,
        source_tick,
    });

    id
}

fn should_enqueue_auto_task(
    tasks: &[LlmTaskRecord],
    kind: &LlmTaskKind,
    fingerprint: &str,
    source_tick: Option<u64>,
) -> bool {
    // 1) Near-duplicate dedupe: do not enqueue if same kind+fingerprint is already pending.
    if tasks.iter().any(|t| {
        t.status == TaskStatus::Pending
            && t.kind == *kind
            && t.input_fingerprint.as_deref() == Some(fingerprint)
    }) {
        return false;
    }

    // 2) Pending cap per kind for auto-trigger source.
    let pending_same_kind = tasks
        .iter()
        .filter(|t| {
            t.status == TaskStatus::Pending
                && t.kind == *kind
                && t.source.starts_with("memory_tick")
        })
        .count();
    if pending_same_kind >= AUTO_MAX_PENDING_PER_KIND {
        return false;
    }

    // 3) Tick-gap rate limit: max one auto-enqueue per kind every N ticks.
    if let Some(now_tick) = source_tick {
        let latest_same_kind_tick = tasks
            .iter()
            .filter(|t| t.kind == *kind && t.source.starts_with("memory_tick"))
            .filter_map(|t| t.source_tick)
            .max();
        if let Some(prev_tick) = latest_same_kind_tick {
            if now_tick.saturating_sub(prev_tick) < AUTO_MIN_TICK_GAP_PER_KIND {
                return false;
            }
        }
    }

    true
}

fn extract_text_from_input(input: &Value) -> Option<&str> {
    input.get("text").and_then(|v| v.as_str())
}

fn text_fingerprint(text: &str) -> String {
    let normalized = text
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn handle_apply(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        return Err("Usage: legend llm apply <task_id> [--result <json>]".into());
    }

    let task_id = &args[0];
    let mut result_json: Option<Value> = None;

    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--result" => {
                i += 1;
                if i >= args.len() {
                    return Err("Missing value for --result".into());
                }
                result_json = Some(serde_json::from_str(&args[i])?);
            }
            _ => {}
        }
        i += 1;
    }

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
    let mut limit = 20usize;
    if let Some(n) = args.first().and_then(|s| s.parse::<usize>().ok()) {
        limit = n;
    }

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
    if args.is_empty() {
        return Err("Usage: legend llm show <task_id>".into());
    }
    let task_id = &args[0];
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

fn parse_text_or_stdin(args: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    if !args.is_empty() {
        Ok(args.join(" "))
    } else {
        read_stdin()
    }
}

fn read_stdin() -> Result<String, Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

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
    let entities = crate::memory::extract::extract_entities(text);
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

fn validate_task_input(
    kind: &LlmTaskKind,
    input: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    match kind {
        LlmTaskKind::EntityExtract | LlmTaskKind::ClusterSummary => {
            if input
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err("Input JSON must include non-empty `text`".into());
            }
        }
        LlmTaskKind::QueryRerank => {
            if input
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                return Err("Input JSON must include non-empty `query`".into());
            }
            let candidates = input
                .get("candidates")
                .and_then(|v| v.as_array())
                .ok_or("Input JSON must include `candidates` array")?;
            if candidates.is_empty() {
                return Err("`candidates` must not be empty".into());
            }
        }
    }
    Ok(())
}

fn task_schema(kind: &LlmTaskKind) -> Value {
    match kind {
        LlmTaskKind::EntityExtract => json!({
            "type": "object",
            "required": ["confidence", "entities"],
            "properties": {
                "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                "entities": {
                    "type": "array",
                    "maxItems": MAX_ENTITY_APPLY,
                    "items": {
                        "type": "object",
                        "required": ["label", "kind", "context"],
                        "properties": {
                            "label": {"type": "string", "minLength": 2, "maxLength": 120},
                            "kind": {"type": "string"},
                            "context": {"type": "string", "enum": ["defines", "uses", "implements", "mentions", "performs"]},
                            "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0}
                        }
                    }
                }
            }
        }),
        LlmTaskKind::ClusterSummary => json!({
            "type": "object",
            "required": ["confidence", "summary"],
            "properties": {
                "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                "summary": {"type": "string", "minLength": 10, "maxLength": 320},
                "decision_rationale": {"type": "string"},
                "risks": {"type": "array", "items": {"type": "string"}}
            }
        }),
        LlmTaskKind::QueryRerank => json!({
            "type": "object",
            "required": ["confidence", "ranked_ids"],
            "properties": {
                "confidence": {"type": "number", "minimum": 0.0, "maximum": 1.0},
                "ranked_ids": {"type": "array", "items": {"type": "integer"}},
                "reasoning": {"type": "string"}
            }
        }),
    }
}

fn acceptance_rules(kind: &LlmTaskKind) -> Value {
    match kind {
        LlmTaskKind::EntityExtract => json!({
            "min_confidence": MIN_ENTITY_TASK_CONFIDENCE,
            "max_entities": MAX_ENTITY_APPLY,
            "drop_stopwords": true,
            "drop_short_labels": true,
            "drop_low_entity_confidence_below": 0.5,
            "on_reject": "fallback_to_deterministic_extractor"
        }),
        LlmTaskKind::ClusterSummary => json!({
            "min_confidence": 0.60,
            "max_summary_chars": 320,
            "on_reject": "fallback_to_extract_summarizer"
        }),
        LlmTaskKind::QueryRerank => json!({
            "min_confidence": 0.60,
            "must_include_ranked_ids": true,
            "on_reject": "keep_original_ranking"
        }),
    }
}

fn task_prompt(kind: &LlmTaskKind, input: &Value, schema: &Value, rules: &Value) -> String {
    match kind {
        LlmTaskKind::EntityExtract => format!(
            "You are augmenting a deterministic memory extractor. Return JSON only. Extract high-signal technical entities and relations from input text. Keep precision high.\\nInput: {}\\nSchema: {}\\nAcceptance rules: {}",
            input,
            schema,
            rules
        ),
        LlmTaskKind::ClusterSummary => format!(
            "You are generating a compact milestone summary for a memory cluster. Return JSON only. Prefer decision rationale and key outcomes.\\nInput: {}\\nSchema: {}\\nAcceptance rules: {}",
            input,
            schema,
            rules
        ),
        LlmTaskKind::QueryRerank => format!(
            "You are reranking retrieval candidates. Return JSON only. Rank by direct relevance and actionable specificity.\\nInput: {}\\nSchema: {}\\nAcceptance rules: {}",
            input,
            schema,
            rules
        ),
    }
}

struct ValidationOutcome {
    accepted: bool,
    reason: String,
    normalized_result: Value,
    entities: Vec<LlmEntity>,
    task_confidence: f32,
}

fn validate_result(
    kind: &LlmTaskKind,
    result: &Value,
) -> Result<ValidationOutcome, Box<dyn std::error::Error>> {
    let confidence = result
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0) as f32;

    match kind {
        LlmTaskKind::EntityExtract => {
            let Some(items) = result.get("entities").and_then(|v| v.as_array()) else {
                return Ok(ValidationOutcome {
                    accepted: false,
                    reason: "Missing `entities` array".to_string(),
                    normalized_result: result.clone(),
                    entities: Vec::new(),
                    task_confidence: confidence,
                });
            };

            if confidence < MIN_ENTITY_TASK_CONFIDENCE {
                return Ok(ValidationOutcome {
                    accepted: false,
                    reason: format!(
                        "Task confidence {:.2} below threshold {:.2}",
                        confidence, MIN_ENTITY_TASK_CONFIDENCE
                    ),
                    normalized_result: result.clone(),
                    entities: Vec::new(),
                    task_confidence: confidence,
                });
            }

            let mut entities = Vec::new();
            let mut seen = std::collections::HashSet::new();

            for item in items.iter().take(MAX_ENTITY_APPLY) {
                let label = item
                    .get("label")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if label.len() < 2 || label.len() > 120 {
                    continue;
                }
                if crate::memory::extract::is_stopword(&label) {
                    continue;
                }

                let kind = item
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Term")
                    .to_string();
                let context = item
                    .get("context")
                    .and_then(|v| v.as_str())
                    .unwrap_or("mentions")
                    .to_string();
                let ent_conf = item
                    .get("confidence")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(confidence as f64)
                    .clamp(0.0, 1.0) as f32;
                if ent_conf < 0.5 {
                    continue;
                }

                let dedupe_key = label.to_ascii_lowercase();
                if seen.insert(dedupe_key) {
                    entities.push(LlmEntity {
                        label,
                        kind,
                        context,
                        confidence: ent_conf,
                    });
                }
            }

            let accepted = !entities.is_empty();
            let reason = if accepted {
                "Validated entity extraction".to_string()
            } else {
                "No valid entities after rule filtering".to_string()
            };
            let normalized_result = json!({
                "confidence": confidence,
                "entities": entities,
            });

            Ok(ValidationOutcome {
                accepted,
                reason,
                normalized_result,
                entities,
                task_confidence: confidence,
            })
        }
        LlmTaskKind::ClusterSummary => {
            let summary = result
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let accepted = confidence >= 0.60 && summary.len() >= 10 && summary.len() <= 320;
            let reason = if accepted {
                "Validated cluster summary".to_string()
            } else {
                "Cluster summary failed confidence/length checks".to_string()
            };
            Ok(ValidationOutcome {
                accepted,
                reason,
                normalized_result: result.clone(),
                entities: Vec::new(),
                task_confidence: confidence,
            })
        }
        LlmTaskKind::QueryRerank => {
            let ranked_ids = result.get("ranked_ids").and_then(|v| v.as_array());
            let accepted = confidence >= 0.60
                && ranked_ids
                    .map(|ids| !ids.is_empty() && ids.iter().all(|v| v.as_u64().is_some()))
                    .unwrap_or(false);
            let reason = if accepted {
                "Validated query rerank output".to_string()
            } else {
                "Rerank failed confidence or ranked_ids validation".to_string()
            };
            Ok(ValidationOutcome {
                accepted,
                reason,
                normalized_result: result.clone(),
                entities: Vec::new(),
                task_confidence: confidence,
            })
        }
    }
}

fn load_tasks() -> Result<Vec<LlmTaskRecord>, Box<dyn std::error::Error>> {
    if !Path::new(LLM_TASKS_PATH).exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(LLM_TASKS_PATH)?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let tasks = serde_json::from_str::<Vec<LlmTaskRecord>>(&content)
        .map_err(|e| format!("Failed to parse {}: {}", LLM_TASKS_PATH, e))?;
    Ok(tasks
        .into_iter()
        .filter(|t| t.status == TaskStatus::Pending)
        .collect())
}

fn save_tasks(tasks: &[LlmTaskRecord]) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(".legend")?;
    let pending: Vec<&LlmTaskRecord> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Pending)
        .collect();
    let content = serde_json::to_string_pretty(&pending)?;
    std::fs::write(LLM_TASKS_PATH, content)?;
    Ok(())
}

fn load_archived_tasks() -> Result<Vec<LlmTaskRecord>, Box<dyn std::error::Error>> {
    if !Path::new(LLM_TASKS_ARCHIVE_PATH).exists() {
        return Ok(Vec::new());
    }
    let compressed = std::fs::read(LLM_TASKS_ARCHIVE_PATH)?;
    if compressed.is_empty() {
        return Ok(Vec::new());
    }

    let decompressed = lz4::block::decompress(&compressed, None)
        .map_err(|e| format!("Failed to decompress {}: {}", LLM_TASKS_ARCHIVE_PATH, e))?;
    let text = String::from_utf8(decompressed)
        .map_err(|e| format!("Invalid UTF-8 in {}: {}", LLM_TASKS_ARCHIVE_PATH, e))?;

    let mut out = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let task: LlmTaskRecord = serde_json::from_str(line)
            .map_err(|e| format!("Failed to parse archived task line: {}", e))?;
        out.push(task);
    }
    Ok(out)
}

fn archive_tasks(tasks: &[LlmTaskRecord]) -> Result<(), Box<dyn std::error::Error>> {
    if tasks.is_empty() {
        return Ok(());
    }

    let mut archived = load_archived_tasks()?;
    archived.extend(tasks.iter().cloned());

    let mut text = String::new();
    for task in archived {
        text.push_str(&serde_json::to_string(&task)?);
        text.push('\n');
    }

    std::fs::create_dir_all(".legend")?;
    let compressed = lz4::block::compress(text.as_bytes(), None, true)
        .map_err(|e| format!("Failed to compress archive: {}", e))?;
    std::fs::write(LLM_TASKS_ARCHIVE_PATH, compressed)?;
    Ok(())
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

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
            &[],
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
