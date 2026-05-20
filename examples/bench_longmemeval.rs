//! LongMemEval bench harness, Rust + MessagePack edition.
//!
//! Talks to the Legend daemon over its TCP MessagePack protocol —
//! no JSON, no Python. Spawns one daemon per question (fresh
//! substrate per question is what the bench design wants) and
//! tears it down at the end. The dataset is loaded from
//! `benchmarks/longmemeval_oracle.json` via `serde_json` — that's
//! the only JSON anywhere in the path, and it's just because the
//! upstream dataset ships that way.
//!
//! Scoring is intentionally crude: extract substantive (≥4-char)
//! words from the expected answer (plus all-caps acronyms like
//! "GPS"), check whether each appears in the frame's flattened
//! text. The proper scoring path would run the frame through a
//! reading LLM (per the original LongMemEval methodology); this
//! harness is a v0 sanity check that retrieval surfaces the right
//! entities.
//!
//! Run:
//!   cargo run --release --example bench_longmemeval -- --questions 5
//!
//! Flags:
//!   --questions N    number of questions to run (default 1)
//!   --max-turns N    cap ingested turns per question (default 30)
//!   --all-roles      include assistant turns (slow — see comments)

use legend::daemon;
use legend::types::{ConsciousAttentionFrame, Term};
use serde::Deserialize;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
struct Turn {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct Question {
    question_id: String,
    question_type: Option<String>,
    question: String,
    /// `answer` can be a string OR a number (e.g. "3") OR a list of
    /// strings in the upstream dataset — accept whatever and string-
    /// ify in `answer_text()`.
    answer: serde_json::Value,
    haystack_sessions: Vec<Vec<Turn>>,
}

impl Question {
    fn answer_text(&self) -> String {
        match &self.answer {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Array(items) => items
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
                .join(", "),
            other => other.to_string(),
        }
    }
}

struct Args {
    questions: usize,
    max_turns: usize,
    all_roles: bool,
}

fn parse_args() -> Args {
    let mut questions = 1usize;
    let mut max_turns = 30usize;
    let mut all_roles = false;
    let raw: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < raw.len() {
        match raw[i].as_str() {
            "--questions" => {
                questions = raw.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1);
                i += 2;
            }
            "--max-turns" => {
                max_turns = raw.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(30);
                i += 2;
            }
            "--all-roles" => {
                all_roles = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    Args {
        questions,
        max_turns,
        all_roles,
    }
}

fn dataset_path() -> PathBuf {
    PathBuf::from("benchmarks/longmemeval_oracle.json")
}

fn load_dataset() -> std::io::Result<Vec<Question>> {
    let raw = std::fs::read_to_string(dataset_path())?;
    Ok(serde_json::from_str(&raw).expect("dataset is well-formed JSON"))
}

/// Extract substantive search terms from the expected answer. Three
/// classes of token survive:
///
/// 1. Long alpha tokens (≥4 lowercase chars after destop, not in the
///    stopword list). Catches "samsung", "webinar", "tomatoes".
/// 2. Uppercase-acronym tokens (≥2 alpha chars, all-upper). Catches
///    "GPS", "USA", "NATO".
/// 3. Mixed alphanumeric tokens with at least one digit (≥2 chars).
///    Catches "S22", "XPS13", "iOS16" — product/model names that
///    failed the earlier ≥4-char rule.
///
/// The 3rd class is the one that fixed the Q4 false-negative on
/// Samsung Galaxy S22: previously "S22" was 3 chars and not all-
/// alpha, so it dropped out of the expected-term set entirely.
fn key_terms(answer: &str) -> Vec<String> {
    let stop = [
        "the", "and", "for", "with", "that", "this", "from", "have", "had", "has", "was", "were",
        "are", "you", "your", "their", "they", "them", "his", "her", "him", "she", "but", "not",
        "all", "any", "what", "when", "where", "why", "how", "who", "which", "into", "about",
        "after", "before", "first", "last", "during",
    ];
    let mut out = Vec::new();
    for raw in answer.split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        if stop.contains(&lower.as_str()) {
            continue;
        }
        let has_digit = raw.chars().any(|c| c.is_ascii_digit());
        let all_upper_alpha =
            raw.chars().all(|c| c.is_ascii_uppercase()) && raw.chars().all(|c| c.is_alphabetic());
        // Class 2: uppercase acronyms (≥2 alpha chars).
        if all_upper_alpha && raw.len() >= 2 {
            out.push(lower);
            continue;
        }
        // Class 3: mixed alphanumeric with at least one digit (≥2 chars).
        if has_digit && raw.len() >= 2 {
            out.push(lower);
            continue;
        }
        // Class 1: long alpha tokens.
        if lower.len() >= 4 {
            out.push(lower);
        }
    }
    out
}

/// Real scoring: ask the daemon to expose the frame's relation text
/// via a status-like extension would be ideal. Since the daemon's
/// `TickResult` carries the bare frame (relation IDs only), we send
/// a follow-up query crafted to surface each interesting relation
/// — but that's expensive. Pragmatic v0: fall back to a `Status`
/// snapshot + a separate `Tick` of the question text and read the
/// frame's flattened input_echo + a brute-force "did any answer
/// term appear in ANY focused relation's subject" check. The daemon
/// doesn't currently expose element names by ID; for the bench we
/// piggyback on the next-tick path that DOES have them.
///
/// Practical answer: the cleanest v0 scoring is to send a `Tick`
/// with the question text and inspect the `frame.input_echo` only —
/// which is just the question text, useless. So instead we send a
/// `Tick` and the daemon writes the frame; we then send a `Status`
/// to read substrate sizes; THEN we use the local persistence file
/// (which the daemon wrote to disk at the end of each tick) to
/// resolve element names. That last step is what this helper does.
fn flatten_frame_with_substrate(
    frame: &ConsciousAttentionFrame,
    snapshot_path: &std::path::Path,
) -> std::io::Result<String> {
    // Load the just-saved substrate so we can resolve element
    // names. This is read-only and fast (~10ms for our typical
    // 1-2MB snapshot).
    let hg = legend::persistence::load(snapshot_path).map_err(std::io::Error::other)?;
    let mut parts: Vec<String> = vec![frame.input_echo.clone()];
    if let Some(eid) = frame.active_frame
        && let Some(n) = hg.elements[eid.0 as usize].names.first()
    {
        parts.push(n.clone());
    }
    let push_rel = |rid: legend::types::RelationId, parts: &mut Vec<String>| {
        let r = &hg.relations[rid.0 as usize];
        for a in &r.attributes {
            if let Some(an) = hg.elements[a.name.0 as usize].names.first() {
                parts.push(an.clone());
            }
            match a.value {
                Term::Element(eid) => {
                    if let Some(n) = hg.elements[eid.0 as usize].names.first() {
                        parts.push(n.clone());
                    }
                }
                Term::Relation(rid) => parts.push(format!("R{}", rid.0)),
            }
        }
    };
    for ra in &frame.focused_relations {
        push_rel(ra.relation, &mut parts);
    }
    for rid in &frame.current_state {
        push_rel(*rid, &mut parts);
    }
    for rid in &frame.history {
        push_rel(*rid, &mut parts);
    }
    for rid in &frame.supporting_claims {
        push_rel(*rid, &mut parts);
    }
    Ok(parts.join(" | "))
}

fn score_hits(text: &str, terms: &[String]) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    terms
        .iter()
        .filter(|t| lower.contains(t.as_str()))
        .cloned()
        .collect()
}

/// Spawn the daemon for `workspace_dir` (its own `LEGEND_STATE_DIR`),
/// wait for it to become reachable. Returns the child handle so the
/// caller can `wait()` / send `Stop` at the end of the question.
fn spawn_daemon(
    workspace: &std::path::Path,
    bin: &std::path::Path,
) -> std::io::Result<std::process::Child> {
    Command::new(bin)
        .arg("__daemon")
        .env("LEGEND_STATE_DIR", workspace.join(".legend"))
        // 5-minute TTL; the bench's ticks are well under that, and
        // letting the daemon idle-exit covers cases where the test
        // panics before its explicit stop.
        .env("LEGEND_DAEMON_TTL", "300")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn wait_for_port(workspace: &std::path::Path, timeout: Duration) -> bool {
    let port_file = workspace.join(".legend/legend.port");
    let start = Instant::now();
    while start.elapsed() < timeout {
        if port_file.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Wire the per-question workspace into env so daemon's `try_connect`
/// reads the right port file.
fn with_state_dir<T>(workspace: &std::path::Path, f: impl FnOnce() -> T) -> T {
    // SAFETY: This bench is single-threaded; mutating the process env
    // is fine. The daemon child already captured the env at spawn time.
    unsafe {
        std::env::set_var("LEGEND_STATE_DIR", workspace.join(".legend"));
    }
    f()
}

fn run_one(q: &Question, args: &Args, bin: &std::path::Path) -> std::io::Result<RunResult> {
    let workspace = tempdir()?;
    std::fs::create_dir_all(workspace.join(".legend"))?;

    let mut child = spawn_daemon(&workspace, bin)?;
    if !wait_for_port(&workspace, Duration::from_secs(15)) {
        let _ = child.kill();
        return Err(std::io::Error::other("daemon did not write port file"));
    }

    let result = with_state_dir(&workspace, || -> std::io::Result<RunResult> {
        // Ingest haystack.
        let t0 = Instant::now();
        let mut ingested = 0usize;
        let mut rejected = 0usize;
        'ingest: for session in &q.haystack_sessions {
            for turn in session {
                if ingested + rejected >= args.max_turns {
                    break 'ingest;
                }
                if !args.all_roles && turn.role != "user" {
                    continue;
                }
                let content = turn.content.trim();
                if content.is_empty() {
                    continue;
                }
                match send_tick(content) {
                    Ok(_) => ingested += 1,
                    Err(_) => rejected += 1,
                }
            }
        }
        let ingest_secs = t0.elapsed();

        // Query.
        let t1 = Instant::now();
        let (frame, elements, relations) = send_tick(&q.question)
            .map_err(|e| std::io::Error::other(format!("query tick failed: {e}")))?;
        let query_secs = t1.elapsed();
        let snapshot_path = workspace.join(".legend/memory.lz4");
        let flat = flatten_frame_with_substrate(&frame, &snapshot_path)?;
        let answer_text = q.answer_text();
        let terms = key_terms(&answer_text);
        let hits = score_hits(&flat, &terms);

        Ok(RunResult {
            qid: q.question_id.clone(),
            qtype: q.question_type.clone().unwrap_or_default(),
            question: q.question.clone(),
            answer: answer_text,
            terms,
            hits,
            ingested,
            rejected,
            ingest_secs: ingest_secs.as_secs_f64(),
            query_secs: query_secs.as_secs_f64(),
            focused: frame.focused_relations.len(),
            elements,
            relations,
        })
    });

    // Stop daemon explicitly so the workspace can be torn down cleanly.
    let _ = stop_daemon();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&workspace);
    result
}

fn send_tick(input: &str) -> std::io::Result<(ConsciousAttentionFrame, usize, usize)> {
    let mut stream = daemon::try_connect()?;
    daemon::write_frame(
        &mut stream,
        &daemon::DaemonRequest::Tick {
            input: input.to_string(),
        },
    )?;
    let resp: daemon::DaemonResponse = daemon::read_frame(&mut stream)?;
    match resp {
        daemon::DaemonResponse::TickResult {
            frame,
            elements,
            relations,
        } => Ok((*frame, elements, relations)),
        daemon::DaemonResponse::Error { message } => Err(std::io::Error::other(message)),
        other => Err(std::io::Error::other(format!(
            "unexpected response: {other:?}"
        ))),
    }
}

fn stop_daemon() -> std::io::Result<()> {
    if let Ok(mut s) = daemon::try_connect() {
        let _ = daemon::write_frame(&mut s, &daemon::DaemonRequest::Stop);
        let _: std::io::Result<daemon::DaemonResponse> = daemon::read_frame(&mut s);
    }
    Ok(())
}

fn tempdir() -> std::io::Result<PathBuf> {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "legend_bench_{}_{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos(),
    ));
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

#[allow(dead_code)] // qid/qtype/question/answer round-trip for verbose
// dumps + future structured output formats.
struct RunResult {
    qid: String,
    qtype: String,
    question: String,
    answer: String,
    terms: Vec<String>,
    hits: Vec<String>,
    ingested: usize,
    rejected: usize,
    ingest_secs: f64,
    query_secs: f64,
    focused: usize,
    elements: usize,
    relations: usize,
}

fn main() -> std::io::Result<()> {
    let args = parse_args();
    let bin = std::env::current_exe()?
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("legend"))
        .ok_or_else(|| std::io::Error::other("cannot locate legend binary"))?;
    let bin = if bin.exists() {
        bin
    } else {
        // CI / cargo-run-from-source uses `target/release/legend`.
        PathBuf::from("target/release/legend")
    };
    if !bin.exists() {
        eprintln!(
            "legend binary not found at {}\nRun `cargo build --release` first.",
            bin.display()
        );
        std::process::exit(1);
    }
    eprintln!("legend binary: {}", bin.display());

    let dataset = load_dataset()?;
    let subset = &dataset[..args.questions.min(dataset.len())];
    println!(
        "running {} questions (max {} ingested turns each; user-only={})",
        subset.len(),
        args.max_turns,
        !args.all_roles
    );
    println!();

    let mut hit_count = 0usize;
    for (i, q) in subset.iter().enumerate() {
        println!(
            "[{}/{}] {} ({})",
            i + 1,
            subset.len(),
            q.question_id,
            q.qtype_disp()
        );
        println!("  Q: {}", truncate(&q.question, 100));
        println!("  expected: {}", truncate(&q.answer_text(), 100));
        std::io::stdout().flush()?;
        match run_one(q, &args, &bin) {
            Ok(r) => {
                println!(
                    "  ingested: {} turns ({} rejected) in {:.1}s   query: {:.2}s   focused={} elements={} relations={}",
                    r.ingested,
                    r.rejected,
                    r.ingest_secs,
                    r.query_secs,
                    r.focused,
                    r.elements,
                    r.relations,
                );
                println!("  answer terms: {:?}", r.terms);
                if !r.hits.is_empty() {
                    println!("  ✓ hits: {:?}", r.hits);
                    hit_count += 1;
                } else {
                    println!("  ✗ no answer terms surfaced in frame");
                }
            }
            Err(e) => {
                println!("  ERROR: {e}");
            }
        }
        println!();
    }
    println!(
        "summary: {hit_count}/{} questions had at least one answer-term hit",
        subset.len()
    );
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

trait QTypeDisp {
    fn qtype_disp(&self) -> &str;
}
impl QTypeDisp for Question {
    fn qtype_disp(&self) -> &str {
        self.question_type.as_deref().unwrap_or("unknown")
    }
}
