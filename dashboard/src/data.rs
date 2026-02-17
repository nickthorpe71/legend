use bevy::prelude::*;
use bevy::time::Timer;
use serde::Deserialize;
use std::io::BufRead;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Project directory resource — set via --project-dir CLI arg
// ---------------------------------------------------------------------------

#[derive(Resource, Clone)]
pub struct ProjectDir(pub PathBuf);

impl Default for ProjectDir {
    fn default() -> Self {
        Self(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

// ---------------------------------------------------------------------------
// Types mirroring Legend's `memory dump` JSON output
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DumpPayload {
    pub clock: u64,
    #[serde(default)]
    pub immediate: Vec<String>,
    #[serde(default)]
    pub short_term: Vec<DumpShortTerm>,
    #[serde(default)]
    pub graph: DumpGraph,
    #[serde(default)]
    pub session_log: Vec<DumpSession>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DumpGraph {
    #[serde(default)]
    pub nodes: Vec<DumpNode>,
    #[serde(default)]
    pub edges: Vec<DumpEdge>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct DumpNode {
    pub id: u64,
    pub label: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub weight: f32,
    #[serde(default)]
    pub salience: f32,
    #[serde(default)]
    pub last_seen: u64,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct DumpEdge {
    pub from: u64,
    pub to: u64,
    #[serde(default)]
    pub weight: f32,
    #[serde(default)]
    pub kind: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct DumpShortTerm {
    pub id: u64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub salience: f32,
    #[serde(default)]
    pub usage: u32,
    #[serde(default)]
    pub last_access: u64,
    #[serde(default)]
    pub reconsolidation_count: u32,
    #[serde(default)]
    pub labile: bool,
    #[serde(default)]
    pub refs: Vec<DumpMemoryRef>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct DumpMemoryRef {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub start_line: usize,
    #[serde(default)]
    pub end_line: usize,
    #[serde(default)]
    pub snippet: String,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
pub struct DumpSession {
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub text: String,
}

// ---------------------------------------------------------------------------
// Live event from `.legend/events.jsonl`
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct EventEntry {
    pub timestamp: u64,
    pub command: String,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Bevy resource — holds all Legend data for the dashboard
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct LegendData {
    // Snapshot from `memory dump`
    pub graph_nodes: Vec<DumpNode>,
    pub graph_edges: Vec<DumpEdge>,
    pub short_term: Vec<DumpShortTerm>,
    pub immediate: Vec<String>,
    pub session_log: Vec<DumpSession>,
    pub clock: u64,

    // Live event stream
    pub events: Vec<EventEntry>,

    // Dirty flag — set when snapshot refreshed, cleared by graph sync
    pub dirty: bool,

    // Internal polling state
    event_timer: Timer,
    state_timer: Timer,
    events_cursor: usize,
}

impl Default for LegendData {
    fn default() -> Self {
        Self {
            graph_nodes: Vec::new(),
            graph_edges: Vec::new(),
            short_term: Vec::new(),
            immediate: Vec::new(),
            session_log: Vec::new(),
            clock: 0,
            events: Vec::new(),
            dirty: false,
            event_timer: Timer::from_seconds(1.0, TimerMode::Repeating),
            state_timer: Timer::from_seconds(3.0, TimerMode::Repeating),
            events_cursor: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Polling system — runs every frame, uses timers to pace work
// ---------------------------------------------------------------------------

pub fn poll_legend_data(
    mut data: ResMut<LegendData>,
    time: Res<Time>,
    project_dir: Res<ProjectDir>,
) {
    data.event_timer.tick(time.delta());
    data.state_timer.tick(time.delta());

    if data.event_timer.just_finished() {
        poll_events(&mut data, &project_dir.0);
    }

    if data.state_timer.just_finished() {
        fetch_full_state(&mut data, &project_dir.0);
    }
}

// ---------------------------------------------------------------------------
// Full state refresh via CLI
// ---------------------------------------------------------------------------

fn fetch_full_state(data: &mut LegendData, project_dir: &PathBuf) {
    let Some(stdout) = legend_command(project_dir, &["memory", "dump"]) else {
        return;
    };
    let Ok(payload) = serde_json::from_slice::<DumpPayload>(&stdout) else {
        return;
    };

    data.clock = payload.clock;
    data.immediate = payload.immediate;
    data.short_term = payload.short_term;
    data.graph_nodes = payload.graph.nodes;
    data.graph_edges = payload.graph.edges;
    data.session_log = payload.session_log;
    data.dirty = true;
}

/// Run a legend CLI command.
/// When running as a Windows exe launched from WSL, we shell out via wsl.exe
/// so the legend Linux binary runs inside WSL with the correct working directory.
fn legend_command(project_dir: &PathBuf, args: &[&str]) -> Option<Vec<u8>> {
    use std::process::Command;

    // Build the full legend command string for WSL
    let legend_args: Vec<&str> = args.to_vec();

    // If the project dir looks like a WSL UNC path (\\wsl$ or \\wsl.localhost),
    // we're a Windows exe — use wsl.exe to run the Linux legend binary.
    let dir_str = project_dir.to_string_lossy();
    if dir_str.starts_with(r"\\wsl") {
        // Extract the Linux path from the UNC path:
        //   \\wsl.localhost\Ubuntu\home\user\project → /home/user/project
        //   \\wsl$\Ubuntu\home\user\project → /home/user/project
        let linux_path = extract_linux_path(&dir_str);
        let cmd_str = format!(
            "cd {} && ./target/release/legend {} 2>/dev/null || ./target/debug/legend {} 2>/dev/null || cargo run --quiet -- {}",
            shell_escape(&linux_path),
            legend_args.join(" "),
            legend_args.join(" "),
            legend_args.join(" "),
        );
        if let Ok(out) = Command::new("wsl.exe")
            .args(["-e", "bash", "-c", &cmd_str])
            .output()
        {
            if out.status.success() {
                return Some(out.stdout);
            }
        }
        return None;
    }

    // Native Linux path — run legend directly
    let candidates = [
        project_dir.join("target/release/legend"),
        project_dir.join("target/debug/legend"),
    ];

    for bin in &candidates {
        if let Ok(out) = Command::new(bin)
            .args(&legend_args)
            .current_dir(project_dir)
            .output()
        {
            if out.status.success() {
                return Some(out.stdout);
            }
        }
    }

    // Fallback: cargo run
    let mut cargo_args: Vec<&str> = vec!["run", "--quiet", "--"];
    cargo_args.extend_from_slice(args);
    if let Ok(out) = Command::new("cargo")
        .args(&cargo_args)
        .current_dir(project_dir)
        .output()
    {
        if out.status.success() {
            return Some(out.stdout);
        }
    }

    None
}

/// Convert a WSL UNC path to a Linux path.
/// `\\wsl.localhost\Ubuntu\home\user\proj` → `/home/user/proj`
/// `\\wsl$\Ubuntu\home\user\proj` → `/home/user/proj`
fn extract_linux_path(unc: &str) -> String {
    // Strip \\wsl.localhost\Distro or \\wsl$\Distro prefix
    let stripped = unc
        .trim_start_matches(r"\\wsl.localhost\")
        .trim_start_matches(r"\\wsl$\");
    // First segment is the distro name, rest is the path
    if let Some(idx) = stripped.find('\\') {
        let path = &stripped[idx..];
        path.replace('\\', "/")
    } else {
        "/".to_string()
    }
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ---------------------------------------------------------------------------
// Event log polling — reads new lines from `.legend/events.jsonl`
// ---------------------------------------------------------------------------

fn poll_events(data: &mut LegendData, project_dir: &PathBuf) {
    let events_path = project_dir.join(".legend").join("events.jsonl");
    let Ok(file) = std::fs::File::open(&events_path) else {
        return;
    };
    let reader = std::io::BufReader::new(file);

    let new_events: Vec<EventEntry> = reader
        .lines()
        .skip(data.events_cursor)
        .filter_map(|line| {
            let line = line.ok()?;
            let json: serde_json::Value = serde_json::from_str(&line).ok()?;
            Some(EventEntry {
                timestamp: json["ts"].as_u64().unwrap_or(0),
                command: json["cmd"].as_str().unwrap_or("").to_string(),
                detail: json["detail"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();

    data.events_cursor += new_events.len();
    data.events.extend(new_events);

    // Keep last 500 events
    const MAX_EVENTS: usize = 500;
    if data.events.len() > MAX_EVENTS {
        data.events.drain(0..data.events.len() - MAX_EVENTS);
    }
}
