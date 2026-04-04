use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, Read};
use std::path::Path;

pub(super) const LLM_TASKS_PATH: &str = ".legend/llm_tasks.json";
pub(super) const LLM_TASKS_ARCHIVE_PATH: &str = ".legend/llm_tasks_archive.lz4";
pub(super) const MIN_ENTITY_TASK_CONFIDENCE: f32 = 0.65;
pub(super) const MAX_ENTITY_APPLY: usize = 50;
pub(super) const AUTO_MAX_PENDING_PER_KIND: usize = 3;
pub(super) const AUTO_MIN_TICK_GAP_PER_KIND: u64 = 5;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum LlmTaskKind {
    EntityExtract,
    ClusterSummary,
    QueryRerank,
}

impl LlmTaskKind {
    pub(super) fn parse(raw: &str) -> Option<Self> {
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
pub(super) enum TaskStatus {
    Pending,
    Applied,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TaskRecommendation {
    pub kind: String,
    pub reason: String,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SignalMetrics {
    pub chars: usize,
    pub words: usize,
    pub entities: usize,
    pub high_signal_entities: usize,
    pub entity_density: f32,
    pub high_signal_density: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SignalReport {
    pub needs_llm: bool,
    pub recommended_tasks: Vec<TaskRecommendation>,
    pub metrics: SignalMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LlmTaskRecord {
    pub id: String,
    pub kind: LlmTaskKind,
    pub status: TaskStatus,
    pub created_ts: u64,
    pub updated_ts: u64,
    pub source: String,
    pub trigger: SignalReport,
    pub input: Value,
    pub prompt: String,
    pub json_schema: Value,
    pub acceptance_rules: Value,
    pub result: Option<Value>,
    pub apply_summary: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_tick: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmAutoTriggerSummary {
    pub needs_llm: bool,
    pub created_task_ids: Vec<String>,
    pub recommended_kinds: Vec<String>,
}

// ---------------------------------------------------------------------------
// IO helpers
// ---------------------------------------------------------------------------

pub(super) fn read_stdin() -> Result<String, Box<dyn std::error::Error>> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

pub(super) fn parse_text_or_stdin(args: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    if !args.is_empty() {
        Ok(args.join(" "))
    } else {
        read_stdin()
    }
}

pub(super) fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Fingerprinting
// ---------------------------------------------------------------------------

pub(super) fn extract_text_from_input(input: &Value) -> Option<&str> {
    input.get("text").and_then(|v| v.as_str())
}

pub(super) fn text_fingerprint(text: &str) -> String {
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

// ---------------------------------------------------------------------------
// Task storage
// ---------------------------------------------------------------------------

pub(super) fn load_tasks() -> Result<Vec<LlmTaskRecord>, Box<dyn std::error::Error>> {
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

pub(super) fn save_tasks(tasks: &[LlmTaskRecord]) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(".legend")?;
    let pending: Vec<&LlmTaskRecord> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Pending)
        .collect();
    let content = serde_json::to_string_pretty(&pending)?;
    std::fs::write(LLM_TASKS_PATH, content)?;
    Ok(())
}

pub(super) fn load_archived_tasks() -> Result<Vec<LlmTaskRecord>, Box<dyn std::error::Error>> {
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

pub(super) fn archive_tasks(tasks: &[LlmTaskRecord]) -> Result<(), Box<dyn std::error::Error>> {
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

// ---------------------------------------------------------------------------
// Task creation & guardrails
// ---------------------------------------------------------------------------

pub(super) fn create_task_record(
    tasks: &mut Vec<LlmTaskRecord>,
    kind: LlmTaskKind,
    input: Value,
    source: String,
    trigger: SignalReport,
    source_tick: Option<u64>,
) -> String {
    let schema = task_schema(&kind);
    let rules = acceptance_rules(&kind);
    let prompt = task_prompt(&kind, &input, &schema, &rules);
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
        acceptance_rules: rules,
        result: None,
        apply_summary: None,
        input_fingerprint,
        source_tick,
    });

    id
}

pub(super) fn should_enqueue_auto_task(
    tasks: &[LlmTaskRecord],
    kind: &LlmTaskKind,
    fingerprint: &str,
    source_tick: Option<u64>,
) -> bool {
    if tasks.iter().any(|t| {
        t.status == TaskStatus::Pending
            && t.kind == *kind
            && t.input_fingerprint.as_deref() == Some(fingerprint)
    }) {
        return false;
    }

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

// ---------------------------------------------------------------------------
// Schema, rules, prompts
// ---------------------------------------------------------------------------

pub(super) fn task_schema(kind: &LlmTaskKind) -> Value {
    use serde_json::json;
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

pub(super) fn acceptance_rules(kind: &LlmTaskKind) -> Value {
    use serde_json::json;
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

pub(super) fn task_prompt(kind: &LlmTaskKind, input: &Value, schema: &Value, rules: &Value) -> String {
    match kind {
        LlmTaskKind::EntityExtract => format!(
            "You are augmenting a deterministic memory extractor. Return JSON only. Extract high-signal technical entities and relations from input text. Keep precision high.\\nInput: {}\\nSchema: {}\\nAcceptance rules: {}",
            input, schema, rules
        ),
        LlmTaskKind::ClusterSummary => format!(
            "You are generating a compact milestone summary for a memory cluster. Return JSON only. Prefer decision rationale and key outcomes.\\nInput: {}\\nSchema: {}\\nAcceptance rules: {}",
            input, schema, rules
        ),
        LlmTaskKind::QueryRerank => format!(
            "You are reranking retrieval candidates. Return JSON only. Rank by direct relevance and actionable specificity.\\nInput: {}\\nSchema: {}\\nAcceptance rules: {}",
            input, schema, rules
        ),
    }
}

pub(super) fn validate_task_input(
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

pub(super) struct ValidationOutcome {
    pub accepted: bool,
    pub reason: String,
    pub normalized_result: Value,
    pub entities: Vec<crate::memory::LlmEntity>,
    pub task_confidence: f32,
}

pub(super) fn validate_result(
    kind: &LlmTaskKind,
    result: &Value,
) -> Result<ValidationOutcome, Box<dyn std::error::Error>> {
    use serde_json::json;

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
                if crate::memory::wernicke::is_stopword(&label) {
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
                    entities.push(crate::memory::LlmEntity {
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
