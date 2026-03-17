use serde::Serialize;
use std::io::Write;

// ---------------------------------------------------------------------------
// Rich Event Data Types for Dashboard Observability
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum EventData {
    Tick(TickEventData),
    Query(QueryEventData),
    Reinforce(ReinforceEventData),
    Consolidate(ConsolidateEventData),
    Start(StartEventData),
}

#[derive(Debug, Clone, Serialize)]
pub struct TickEventData {
    pub entry_id: Option<u64>,
    pub matches: Vec<MatchedEntry>,
    pub graph_nodes: Vec<GraphHit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueryEventData {
    pub matches: Vec<MatchedEntry>,
    pub graph_nodes: Vec<GraphHit>,
    pub primed_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchedEntry {
    pub id: u64,
    pub similarity: f32,
    pub text_preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphHit {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReinforceEventData {
    pub signal: f32,
    pub entries: Vec<ReinforceEntry>,
    pub graph_nodes_affected: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReinforceEntry {
    pub id: u64,
    pub before: f32,
    pub after: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsolidateEventData {
    pub groups_merged: usize,
    pub summaries: Vec<ConsolidatedGroup>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConsolidatedGroup {
    pub node_id: u64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StartEventData {
    pub clock: u64,
    pub short_term_count: usize,
    pub long_term_nodes: usize,
    pub session_log_entries: usize,
}

// ---------------------------------------------------------------------------
// Event logging
// ---------------------------------------------------------------------------

/// Maximum lines in events.jsonl before rotation.
const EVENT_LOG_MAX_LINES: usize = 10_000;
pub const EVENT_LOG_PATH: &str = ".legend/events.jsonl";
const EVENT_LOG_ARCHIVE: &str = ".legend/events.jsonl.1";

/// Rotate event log if it exceeds the max line count.
fn maybe_rotate_event_log() {
    use std::io::BufRead;
    let Ok(file) = std::fs::File::open(EVENT_LOG_PATH) else {
        return;
    };
    let line_count = std::io::BufReader::new(file).lines().count();
    if line_count >= EVENT_LOG_MAX_LINES {
        let _ = std::fs::rename(EVENT_LOG_PATH, EVENT_LOG_ARCHIVE);
    }
}

/// Append a structured event to `.legend/events.jsonl` for dashboard streaming.
/// Includes optional rich data payload for detailed observability.
/// Automatically rotates the log when it exceeds EVENT_LOG_MAX_LINES.
pub fn log_event_rich(cmd: &str, detail: &str, data: Option<EventData>) {
    maybe_rotate_event_log();

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let entry = if let Some(data) = data {
        serde_json::json!({"ts": ts, "cmd": cmd, "detail": detail, "data": data})
    } else {
        serde_json::json!({"ts": ts, "cmd": cmd, "detail": detail})
    };
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(EVENT_LOG_PATH)
    else {
        return;
    };
    let _ = writeln!(f, "{}", entry);
}

/// Simple event logging without rich data (backwards compatible).
pub fn log_event(cmd: &str, detail: &str) {
    log_event_rich(cmd, detail, None);
}
