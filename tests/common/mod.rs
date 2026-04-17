#![allow(dead_code)]

use assert_cmd::cargo::cargo_bin;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

const MSGPACK_MAGIC: &[u8; 4] = b"LGND";
const MSGPACK_FORMAT_VERSION: u8 = 1;

pub struct Harness {
    tempdir: TempDir,
    binary: PathBuf,
}

pub struct CmdOutput {
    pub status: std::process::ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Harness {
    pub fn new() -> Self {
        let tempdir = TempDir::new().expect("create tempdir");
        let binary = cargo_bin("legend");
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(tempdir.path())
            .status()
            .expect("run git init");
        assert!(status.success(), "git init failed: {status:?}");

        Self { tempdir, binary }
    }

    pub fn path(&self) -> &Path {
        self.tempdir.path()
    }

    pub fn legend_dir(&self) -> PathBuf {
        self.path().join(".legend")
    }

    pub fn cmd(&self, args: &[&str]) -> CmdOutput {
        self.cmd_in(self.path(), args)
    }

    pub fn cmd_ok(&self, args: &[&str]) -> CmdOutput {
        let output = self.cmd(args);
        assert!(
            output.status.success(),
            "legend {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            output.stdout,
            output.stderr
        );
        output
    }

    pub fn write_file(&self, relative: &str, contents: &str) {
        let path = self.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent directories");
        }
        fs::write(path, contents).expect("write fixture file");
    }

    pub fn cmd_in(&self, dir: &Path, args: &[&str]) -> CmdOutput {
        let output = Command::new(&self.binary)
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap_or_else(|err| panic!("failed to run legend {:?}: {err}", args));
        self.decode_output(output)
    }

    pub fn cmd_in_ok(&self, dir: &Path, args: &[&str]) -> CmdOutput {
        let output = self.cmd_in(dir, args);
        assert!(
            output.status.success(),
            "legend {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            output.stdout,
            output.stderr
        );
        output
    }

    pub fn read_file(&self, relative: &str) -> String {
        fs::read_to_string(self.path().join(relative)).expect("read file")
    }

    pub fn exists(&self, relative: &str) -> bool {
        self.path().join(relative).exists()
    }

    pub fn output_json(&self, args: &[&str]) -> Value {
        let output = self.cmd_ok(args);
        serde_json::from_str(&output.stdout).unwrap_or_else(|err| {
            panic!(
                "failed to parse JSON for {:?}: {err}\nstdout:\n{}",
                args, output.stdout
            )
        })
    }

    pub fn normalize(&self, text: &str) -> String {
        let mut normalized = text.replace(self.path().to_string_lossy().as_ref(), "<TMPDIR>");
        if let Some(name) = self.path().file_name().and_then(|s| s.to_str()) {
            normalized = normalized.replace(name, "<TMPNAME>");
        }
        normalized
    }

    pub fn mcp_session(&self, messages: &[Value]) -> Vec<Value> {
        self.mcp_session_in(self.path(), messages)
    }

    pub fn mcp_session_in(&self, cwd: &Path, messages: &[Value]) -> Vec<Value> {
        let mut child = Command::new(&self.binary)
            .args(["mcp-serve", "--cwd", cwd.to_str().unwrap()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn mcp-serve");

        let mut stdin = child.stdin.take().expect("open stdin");
        for msg in messages {
            let line = serde_json::to_string(msg).expect("serialize JSON-RPC message");
            writeln!(stdin, "{}", line).expect("write to stdin");
        }
        drop(stdin); // close stdin to signal EOF

        let output = child.wait_with_output().expect("wait for mcp-serve");
        let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
        stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).expect("parse JSON-RPC response"))
            .collect()
    }

    fn decode_output(&self, output: Output) -> CmdOutput {
        CmdOutput {
            status: output.status,
            stdout: String::from_utf8(output.stdout).expect("stdout utf8"),
            stderr: String::from_utf8(output.stderr).expect("stderr utf8"),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct FixtureMemoryConfig {
    immediate_capacity: usize,
    short_term_capacity: usize,
    embedding_dim: usize,
    theta_high: f32,
    theta_low: f32,
}

impl Default for FixtureMemoryConfig {
    fn default() -> Self {
        Self {
            immediate_capacity: 256,
            short_term_capacity: 1024,
            embedding_dim: 256,
            theta_high: 0.92,
            theta_low: 0.55,
        }
    }
}

#[derive(Clone, Default, Serialize)]
pub struct FixtureGraphMemory {
    pub nodes: HashMap<u64, FixtureGraphNode>,
    pub edges: Vec<FixtureGraphEdge>,
    pub index: HashMap<String, u64>,
}

#[derive(Clone, Default, Serialize)]
pub struct FixtureGraphNode {
    pub id: u64,
    pub label: String,
    pub kind: String,
    pub weight: f32,
    pub last_seen: u64,
    pub salience: f32,
    pub source_texts: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct FixtureGraphEdge {
    pub from: u64,
    pub to: u64,
    pub weight: f32,
    pub kind: String,
    pub last_seen: u64,
}

impl Default for FixtureGraphEdge {
    fn default() -> Self {
        Self {
            from: 0,
            to: 0,
            weight: 0.0,
            kind: "related".to_string(),
            last_seen: 0,
        }
    }
}

#[derive(Clone, Default, Serialize)]
pub struct FixtureMemoryRef {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub snippet: String,
}

#[derive(Clone, Default, Serialize)]
pub struct FixtureSessionEntry {
    pub timestamp: u64,
    pub text: String,
}

#[derive(Clone, Serialize)]
pub struct FixtureShortTermEntry {
    pub id: u64,
    pub text: String,
    pub summary: String,
    pub embedding: Vec<f32>,
    pub last_access: u64,
    pub usage: u32,
    pub salience: f32,
    pub reconsolidation_count: u32,
    pub labile_until: u64,
    pub refs: Vec<FixtureMemoryRef>,
    pub gradient_sq_sum: f32,
    pub density: f32,
    pub consolidated: bool,
    pub emotional_valence: f32,
    pub stability: f32,
    pub last_retrieval_interval: u64,
}

#[derive(Clone, Serialize)]
pub struct FixtureShortTermEntryMissingConsolidated {
    pub id: u64,
    pub text: String,
    pub summary: String,
    pub embedding: Vec<f32>,
    pub last_access: u64,
    pub usage: u32,
    pub salience: f32,
    pub reconsolidation_count: u32,
    pub labile_until: u64,
    pub refs: Vec<FixtureMemoryRef>,
    pub gradient_sq_sum: f32,
    pub density: f32,
    // Note: `consolidated` intentionally missing — tests deserialization with missing field
}

#[derive(Clone, Serialize)]
pub struct FixtureMemoryState<T> {
    config: FixtureMemoryConfig,
    immediate: VecDeque<String>,
    short_term: Vec<T>,
    long_term: FixtureGraphMemory,
    clock: u64,
    next_id: u64,
    session_log: Vec<FixtureSessionEntry>,
    current_task: Option<String>,
    ticks_since_consolidation: u32,
    last_retrieved_ids: Vec<u64>,
    last_synced_sha: Option<String>,
}

pub fn jsonrpc_request(id: u64, method: &str, params: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

pub fn seed_basic_repo(harness: &Harness) {
    harness.write_file(
        "src/main.rs",
        "fn main() {\n    println!(\"hello from fixture\");\n}\n",
    );
    harness.write_file(
        "Cargo.toml",
        "[package]\nname = \"fixture-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    harness.write_file("README.md", "# Fixture App\n");
}

pub fn write_msgpack_missing_field_fixture(harness: &Harness) {
    let state = FixtureMemoryState {
        config: FixtureMemoryConfig::default(),
        immediate: VecDeque::from(["hello".to_string()]),
        short_term: vec![FixtureShortTermEntryMissingConsolidated {
            id: 1,
            text: "important migrated memory".into(),
            summary: "important migrated memory".into(),
            embedding: vec![0.1, 0.2, 0.3],
            last_access: 50,
            usage: 5,
            salience: 0.9,
            reconsolidation_count: 2,
            labile_until: 0,
            refs: vec![FixtureMemoryRef {
                path: "src/main.rs".into(),
                start_line: 1,
                end_line: 3,
                snippet: "fn main()".into(),
            }],
            gradient_sq_sum: 0.5,
            density: 1.2,
        }],
        long_term: FixtureGraphMemory::default(),
        clock: 100,
        next_id: 2,
        session_log: vec![FixtureSessionEntry {
            timestamp: 99,
            text: "test session".into(),
        }],
        current_task: Some("testing msgpack".into()),
        ticks_since_consolidation: 5,
        last_retrieved_ids: vec![1],
        last_synced_sha: Some("deadbeef".into()),
    };

    write_msgpack_state(harness, &state);
}

pub fn write_consolidation_fixture(harness: &Harness) {
    let state = FixtureMemoryState {
        config: FixtureMemoryConfig::default(),
        immediate: VecDeque::from([
            "ARCHITECTURE: Query pipeline routes through graph lookup.".to_string(),
            "ARCHITECTURE: Query pipeline coordinates graph lookup.".to_string(),
        ]),
        short_term: vec![
            FixtureShortTermEntry {
                id: 1,
                text: "ARCHITECTURE: Query pipeline routes through graph lookup.".into(),
                summary: "Query pipeline routes through graph lookup.".into(),
                embedding: vec![0.95, 0.05, 0.0],
                last_access: 10,
                usage: 1,
                salience: 0.8,
                reconsolidation_count: 0,
                labile_until: 0,
                refs: vec![],
                gradient_sq_sum: 0.0,
                density: 0.0,
                consolidated: false,
                emotional_valence: 0.0,
                stability: 1.0,
                last_retrieval_interval: 0,
            },
            FixtureShortTermEntry {
                id: 2,
                text: "ARCHITECTURE: Query pipeline coordinates graph lookup.".into(),
                summary: "Query pipeline coordinates graph lookup.".into(),
                embedding: vec![0.93, 0.07, 0.0],
                last_access: 11,
                usage: 1,
                salience: 0.7,
                reconsolidation_count: 0,
                labile_until: 0,
                refs: vec![],
                gradient_sq_sum: 0.0,
                density: 0.0,
                consolidated: false,
                emotional_valence: 0.0,
                stability: 1.0,
                last_retrieval_interval: 0,
            },
        ],
        long_term: FixtureGraphMemory::default(),
        clock: 12,
        next_id: 3,
        session_log: vec![
            FixtureSessionEntry {
                timestamp: 10,
                text: "ARCHITECTURE: Query pipeline routes through graph lookup.".into(),
            },
            FixtureSessionEntry {
                timestamp: 11,
                text: "ARCHITECTURE: Query pipeline coordinates graph lookup.".into(),
            },
        ],
        current_task: None,
        ticks_since_consolidation: 2,
        last_retrieved_ids: vec![],
        last_synced_sha: None,
    };

    write_msgpack_state(harness, &state);
}

pub fn write_msgpack_memory_fixture(
    path: &Path,
    short_term: Vec<FixtureShortTermEntry>,
    session_log: Vec<FixtureSessionEntry>,
) {
    let state = FixtureMemoryState {
        config: FixtureMemoryConfig::default(),
        immediate: VecDeque::new(),
        short_term,
        long_term: FixtureGraphMemory::default(),
        clock: 10,
        next_id: 100,
        session_log,
        current_task: None,
        ticks_since_consolidation: 0,
        last_retrieved_ids: vec![],
        last_synced_sha: None,
    };

    write_msgpack_state_to_path(path, &state);
}

/// Extended fixture builder for merge tests — supports graph nodes and custom clock/next_id.
pub fn write_msgpack_memory_fixture_extended(
    path: &Path,
    short_term: Vec<FixtureShortTermEntry>,
    session_log: Vec<FixtureSessionEntry>,
    long_term: FixtureGraphMemory,
    clock: u64,
    next_id: u64,
    current_task: Option<String>,
) {
    let state = FixtureMemoryState {
        config: FixtureMemoryConfig::default(),
        immediate: VecDeque::new(),
        short_term,
        long_term,
        clock,
        next_id,
        session_log,
        current_task,
        ticks_since_consolidation: 0,
        last_retrieved_ids: vec![],
        last_synced_sha: None,
    };

    write_msgpack_state_to_path(path, &state);
}

pub fn write_events_fixture(path: &Path, lines: &[&str]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create events fixture parent");
    }
    let mut content = lines.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    fs::write(path, content).expect("write events fixture");
}

fn write_msgpack_state<T: Serialize>(harness: &Harness, state: &FixtureMemoryState<T>) {
    write_msgpack_state_to_path(&harness.legend_dir().join("memory.lz4"), state);
}

fn write_msgpack_state_to_path<T: Serialize>(path: &Path, state: &FixtureMemoryState<T>) {
    let serialized = rmp_serde::to_vec_named(state).expect("serialize msgpack fixture");
    let mut payload = Vec::with_capacity(5 + serialized.len());
    payload.extend_from_slice(MSGPACK_MAGIC);
    payload.push(MSGPACK_FORMAT_VERSION);
    payload.extend_from_slice(&serialized);
    let compressed = lz4::block::compress(&payload, None, true).expect("compress msgpack fixture");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create msgpack fixture parent");
    }
    fs::write(path, compressed).expect("write msgpack fixture");
}
