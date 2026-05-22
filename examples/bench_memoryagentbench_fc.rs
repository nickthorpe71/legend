//! MemoryAgentBench FactConsolidation bench harness.
//!
//! Mirrors the (removed) LongMemEval harness pattern: spawns one
//! Legend daemon per row, ingests the row's `context` fact-by-fact,
//! then ticks each question and SubEM-scores the resulting frame
//! against the gold answer list.
//!
//! Dataset: `benchmarks/memoryagentbench/conflict_resolution.parquet`
//! (downloaded once from `ai-hyz/MemoryAgentBench` on Hugging Face).
//! Each row is one (variant, context-size) bucket — sh_6k, sh_32k,
//! mh_6k, etc. — with 100 questions per row. We target a single
//! variant per run and cap questions for smoke testing.
//!
//! SubEM = does any gold answer appear as a case-insensitive
//! substring in the flattened post-tick frame? This matches the
//! paper's `substring_exact_match` scoring rule. Frame flattening
//! reuses the LongMemEval trick of loading the just-saved substrate
//! to resolve element-name IDs.
//!
//! Run:
//!   cargo run --release --example bench_memoryagentbench_fc -- \
//!     --variant sh_6k --questions 5
//!
//! Flags:
//!   --dataset D        `cr` (Conflict Resolution / FactConsolidation,
//!                      default) or `ar` (Accurate Retrieval). `cr`
//!                      reads `conflict_resolution.parquet` and
//!                      prefixes `--variant` with `factconsolidation_`;
//!                      `ar` reads `accurate_retrieval.parquet` and
//!                      uses `--variant` as the source name directly
//!                      (e.g. `eventqa_65536`, `ruler_qa1_197K`).
//!   --variant V        For cr: sh_6k / sh_32k / sh_64k / sh_262k /
//!                      mh_6k / mh_32k / mh_64k / mh_262k (default
//!                      sh_6k). For ar: eventqa_65536 / eventqa_131072
//!                      / eventqa_full / longmemeval_s* /
//!                      ruler_qa1_197K / ruler_qa2_421K.
//!   --questions N      questions to run from the row (default 5)
//!   --max-facts N      cap facts ingested (default 0 = all)
//!   --verbose-misses   dump the flat frame for every miss
//!   --verbose-all      dump the flat frame for every question
//!                      (hits + misses). Use to see what the bench
//!                      is actually scoring against.

use legend::daemon;
use legend::types::ConsciousAttentionFrame;
use parquet::file::reader::{FileReader, SerializedFileReader};
use parquet::record::Field;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CR_PARQUET: &str = "benchmarks/memoryagentbench/conflict_resolution.parquet";
const AR_PARQUET: &str = "benchmarks/memoryagentbench/accurate_retrieval.parquet";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dataset {
    /// Conflict Resolution (FactConsolidation). Numbered-fact context;
    /// source names look like `factconsolidation_<variant>`.
    Cr,
    /// Accurate Retrieval. Prose-passage context (SQuAD-style); source
    /// names are bare (`eventqa_65536`, `ruler_qa1_197K`, etc.).
    Ar,
}

impl Dataset {
    fn parquet(self) -> &'static str {
        match self {
            Dataset::Cr => CR_PARQUET,
            Dataset::Ar => AR_PARQUET,
        }
    }
    fn full_source(self, variant: &str) -> String {
        match self {
            Dataset::Cr => format!("factconsolidation_{variant}"),
            Dataset::Ar => variant.to_string(),
        }
    }
}

struct Args {
    dataset: Dataset,
    variant: String,
    questions: usize,
    max_facts: usize,
    /// Dump the flat frame for every miss (the bench-scored string).
    /// Use to investigate why specific gold answers weren't surfaced.
    verbose_misses: bool,
    /// Dump the flat frame for every question, hit or miss. Use when
    /// you want to see what's actually being scored — including the
    /// good cases, not just the failures.
    verbose_all: bool,
}

fn parse_args() -> Args {
    let mut dataset = Dataset::Cr;
    let mut variant = "sh_6k".to_string();
    let mut questions = 5usize;
    let mut max_facts = 0usize;
    let mut verbose_misses = false;
    let mut verbose_all = false;
    let raw: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < raw.len() {
        match raw[i].as_str() {
            "--dataset" => {
                match raw.get(i + 1).map(|s| s.as_str()) {
                    Some("ar") => dataset = Dataset::Ar,
                    Some("cr") => dataset = Dataset::Cr,
                    Some(other) => {
                        eprintln!("unknown --dataset {other:?} (expected cr|ar)");
                        std::process::exit(2);
                    }
                    None => {
                        eprintln!("--dataset requires a value (cr|ar)");
                        std::process::exit(2);
                    }
                }
                i += 2;
            }
            "--variant" => {
                if let Some(v) = raw.get(i + 1) {
                    variant = v.clone();
                }
                i += 2;
            }
            "--questions" => {
                questions = raw.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(5);
                i += 2;
            }
            "--max-facts" => {
                max_facts = raw.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
                i += 2;
            }
            "--verbose-misses" => {
                verbose_misses = true;
                i += 1;
            }
            "--verbose-all" => {
                verbose_all = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    Args {
        dataset,
        variant,
        questions,
        max_facts,
        verbose_misses,
        verbose_all,
    }
}

// ─── Dataset loading ────────────────────────────────────────────────

struct Row {
    source: String,
    context: String,
    questions: Vec<String>,
    /// Outer index aligns with `questions`. Each inner Vec is the
    /// list of acceptable gold answers (any-of) for that question.
    answers: Vec<Vec<String>>,
}

fn load_row(path: &Path, target_source: &str) -> std::io::Result<Row> {
    let file = File::open(path)?;
    let reader = SerializedFileReader::new(file).map_err(std::io::Error::other)?;
    let iter = reader
        .get_row_iter(None)
        .map_err(std::io::Error::other)?;

    for record in iter {
        let record = record.map_err(std::io::Error::other)?;
        let mut context: Option<String> = None;
        let mut questions: Option<Vec<String>> = None;
        let mut answers: Option<Vec<Vec<String>>> = None;
        let mut source: Option<String> = None;

        for (name, field) in record.get_column_iter() {
            match name.as_str() {
                "context" => {
                    if let Field::Str(s) = field {
                        context = Some(s.clone());
                    }
                }
                "questions" => {
                    if let Field::ListInternal(list) = field {
                        let v = list
                            .elements()
                            .iter()
                            .filter_map(|f| match f {
                                Field::Str(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect();
                        questions = Some(v);
                    }
                }
                "answers" => {
                    if let Field::ListInternal(outer) = field {
                        let v: Vec<Vec<String>> = outer
                            .elements()
                            .iter()
                            .map(|inner| match inner {
                                Field::ListInternal(inner_list) => inner_list
                                    .elements()
                                    .iter()
                                    .filter_map(|f| match f {
                                        Field::Str(s) => Some(s.clone()),
                                        _ => None,
                                    })
                                    .collect(),
                                _ => Vec::new(),
                            })
                            .collect();
                        answers = Some(v);
                    }
                }
                "metadata" => {
                    if let Field::Group(g) = field {
                        for (mname, mfield) in g.get_column_iter() {
                            if mname == "source"
                                && let Field::Str(s) = mfield
                            {
                                source = Some(s.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let src = source.unwrap_or_default();
        if src == target_source {
            return Ok(Row {
                source: src,
                context: context.unwrap_or_default(),
                questions: questions.unwrap_or_default(),
                answers: answers.unwrap_or_default(),
            });
        }
    }

    Err(std::io::Error::other(format!(
        "no row with source={target_source} in {}",
        path.display()
    )))
}

/// Split the context block into individual fact lines. The block
/// starts with a header ("Here is a list of facts:") and then
/// numbered lines like "0. Thomas Kyd was born in...". We keep only
/// the numbered lines and strip the leading "N. " so what gets
/// ticked is the bare sentence.
fn fact_lines(context: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in context.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Lines look like "12. Bengaluru is located in...". Require
        // a leading run of digits, then ". ", then non-empty body.
        let (head, rest) = match trimmed.split_once(". ") {
            Some(pair) => pair,
            None => continue,
        };
        if head.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
            out.push(rest.to_string());
        }
    }
    out
}

/// Sentence-level chunker for prose context (AR datasets — SQuAD-style
/// passages). Splits on `.`, `?`, `!` followed by whitespace, keeping
/// each sentence as one tick. Filters empties and short fragments
/// (≤3 chars) that result from list bullets / abbreviations. Each
/// resulting sentence has its terminator restored so Legend's
/// extractors see complete punctuation.
fn sentence_chunks(context: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut prev_was_terminator = false;
    for ch in context.chars() {
        buf.push(ch);
        match ch {
            '.' | '?' | '!' => prev_was_terminator = true,
            c if c.is_whitespace() && prev_was_terminator => {
                let trimmed = buf.trim().to_string();
                if trimmed.len() > 3 {
                    out.push(trimmed);
                }
                buf.clear();
                prev_was_terminator = false;
            }
            _ => prev_was_terminator = false,
        }
    }
    let trimmed = buf.trim().to_string();
    if trimmed.len() > 3 {
        out.push(trimmed);
    }
    out
}

/// Dispatch chunking by dataset. CR uses the numbered-list extractor;
/// AR uses sentence boundaries.
fn chunk_context(dataset: Dataset, context: &str) -> Vec<String> {
    match dataset {
        Dataset::Cr => fact_lines(context),
        Dataset::Ar => sentence_chunks(context),
    }
}

// ─── Frame flattening ───────────────────────────────────────────────
//
// Both the raw flat string (what SubEM scores against) and the
// annotated multi-line view (what gets printed on `--verbose-misses`
// / `--verbose-all`) come from the same library functions the
// `legend --verbose` CLI uses, so what the bench scores against and
// what a user sees stay aligned.

struct FlatBundle {
    /// Raw pipe-joined string — what SubEM substring-matches against.
    raw: String,
    /// Annotated multi-line render — same content, human-readable
    /// with section headers and per-relation grouping.
    annotated: String,
}

fn flatten_frame_with_substrate(
    frame: &ConsciousAttentionFrame,
    snapshot_path: &Path,
) -> std::io::Result<FlatBundle> {
    let hg = legend::persistence::load(snapshot_path).map_err(std::io::Error::other)?;
    Ok(FlatBundle {
        raw: legend::render::flatten_attention_frame(frame, &hg),
        annotated: legend::render::render_flat_frame_annotated(frame, &hg),
    })
}

/// SubEM: case-insensitive substring of any gold answer in the
/// flattened frame.
fn subem_hit(flat: &str, golds: &[String]) -> Option<String> {
    let hay = flat.to_ascii_lowercase();
    for g in golds {
        let needle = g.to_ascii_lowercase();
        if !needle.is_empty() && hay.contains(&needle) {
            return Some(g.clone());
        }
    }
    None
}

/// Diagnostic: walk every element name in the whole persisted
/// hypergraph (not just the frame) and check whether any gold answer
/// appears as a substring. Used on frame-misses to split (a)
/// "substrate has it, frame didn't surface" from (b) "substrate
/// never extracted it." Not a scoring path — purely instrumentation.
fn gold_in_substrate(snapshot_path: &Path, golds: &[String]) -> std::io::Result<Option<String>> {
    let hg = legend::persistence::load(snapshot_path).map_err(std::io::Error::other)?;
    let all_text: String = hg
        .elements
        .iter()
        .flat_map(|e| e.names.iter().cloned())
        .collect::<Vec<_>>()
        .join(" | ")
        .to_ascii_lowercase();
    for g in golds {
        let needle = g.to_ascii_lowercase();
        if !needle.is_empty() && all_text.contains(&needle) {
            return Ok(Some(g.clone()));
        }
    }
    Ok(None)
}

// ─── Daemon plumbing (mirrors the deleted LongMemEval harness) ─────

fn spawn_daemon(
    workspace: &Path,
    bin: &Path,
) -> std::io::Result<std::process::Child> {
    Command::new(bin)
        .arg("__daemon")
        .env("LEGEND_STATE_DIR", workspace.join(".legend"))
        .env("LEGEND_DAEMON_TTL", "600")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

fn wait_for_port(workspace: &Path, timeout: Duration) -> bool {
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

fn with_state_dir<T>(workspace: &Path, f: impl FnOnce() -> T) -> T {
    // SAFETY: bench is single-threaded; mutating env here is fine.
    unsafe {
        std::env::set_var("LEGEND_STATE_DIR", workspace.join(".legend"));
    }
    f()
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
        "legend_macb_{}_{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos(),
    ));
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

// ─── Main ──────────────────────────────────────────────────────────

struct QResult {
    question: String,
    golds: Vec<String>,
    hit: Option<String>,
    /// Set only on frame-misses: which gold (if any) appeared
    /// somewhere in the full substrate but failed to surface in the
    /// focused frame.
    substrate_only: Option<String>,
    /// Flat-frame text, captured only on misses when --verbose-misses
    /// is set.
    flat: Option<String>,
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

    let path = PathBuf::from(args.dataset.parquet());
    let row = load_row(&path, &args.dataset.full_source(&args.variant))?;
    let all_facts = chunk_context(args.dataset, &row.context);
    let facts: Vec<&String> = if args.max_facts == 0 {
        all_facts.iter().collect()
    } else {
        all_facts.iter().take(args.max_facts).collect()
    };
    let n_questions = args.questions.min(row.questions.len());

    println!("variant: {}", row.source);
    println!(
        "ingesting {} of {} chunks, then running {} of {} questions",
        facts.len(),
        all_facts.len(),
        n_questions,
        row.questions.len(),
    );
    println!();

    let workspace = tempdir()?;
    std::fs::create_dir_all(workspace.join(".legend"))?;
    let mut child = spawn_daemon(&workspace, &bin)?;
    if !wait_for_port(&workspace, Duration::from_secs(15)) {
        let _ = child.kill();
        return Err(std::io::Error::other("daemon did not write port file"));
    }
    let snapshot_path = workspace.join(".legend/memory.lz4");

    let results = with_state_dir(&workspace, || -> std::io::Result<Vec<QResult>> {
        let t0 = Instant::now();
        let mut ingest_errs = 0usize;
        for (i, line) in facts.iter().enumerate() {
            if let Err(e) = send_tick(line) {
                ingest_errs += 1;
                if ingest_errs <= 3 {
                    eprintln!("  ingest err on fact #{i}: {e}");
                }
            }
        }
        eprintln!(
            "ingested {} facts ({} errors) in {:.1}s",
            facts.len(),
            ingest_errs,
            t0.elapsed().as_secs_f64(),
        );

        let mut out = Vec::new();
        for qi in 0..n_questions {
            let q = &row.questions[qi];
            let golds = row.answers.get(qi).cloned().unwrap_or_default();
            let t1 = Instant::now();
            let (frame, elements, relations) = send_tick(q)
                .map_err(|e| std::io::Error::other(format!("query tick failed: {e}")))?;
            let query_secs = t1.elapsed().as_secs_f64();
            let bundle = flatten_frame_with_substrate(&frame, &snapshot_path)?;
            let hit = subem_hit(&bundle.raw, &golds);
            let substrate_only = if hit.is_none() {
                gold_in_substrate(&snapshot_path, &golds)?
            } else {
                None
            };
            let flat_for_dump = if args.verbose_all || (hit.is_none() && args.verbose_misses) {
                Some(bundle.annotated.clone())
            } else {
                None
            };

            out.push(QResult {
                question: q.clone(),
                golds,
                hit,
                substrate_only,
                flat: flat_for_dump,
                query_secs,
                focused: frame.focused_relations.len(),
                elements,
                relations,
            });
        }
        Ok(out)
    });

    let _ = stop_daemon();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&workspace);

    let results = results?;
    let mut hits = 0usize;
    for (i, r) in results.iter().enumerate() {
        println!("[{}/{}]", i + 1, results.len());
        println!("  Q: {}", truncate(&r.question, 120));
        println!("  gold: {:?}", r.golds);
        println!(
            "  query: {:.2}s   focused={} elements={} relations={}",
            r.query_secs, r.focused, r.elements, r.relations,
        );
        match &r.hit {
            Some(g) => {
                println!("  ✓ SubEM hit on: {g:?}");
                hits += 1;
            }
            None => match &r.substrate_only {
                Some(g) => println!("  ~ substrate has {g:?} but frame did not surface it"),
                None => println!("  ✗ gold not in frame OR substrate"),
            },
        }
        if let Some(annotated) = &r.flat {
            // Already a multi-line annotated string from
            // `render::render_flat_frame_annotated`; print verbatim.
            // Indent two spaces to nest under the question's bullet
            // so the section structure stays clear in dumps with
            // many questions.
            for line in annotated.lines() {
                println!("  {line}");
            }
        }
        println!();
    }
    let total = results.len();
    let frame_only = results
        .iter()
        .filter(|r| r.hit.is_none() && r.substrate_only.is_some())
        .count();
    let absent = results
        .iter()
        .filter(|r| r.hit.is_none() && r.substrate_only.is_none())
        .count();
    println!(
        "summary: {hits}/{total} frame hits ({:.1}%) | {frame_only} substrate-only (frame missed it) | {absent} absent from substrate",
        100.0 * hits as f64 / total.max(1) as f64,
    );
    std::io::stdout().flush()?;
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
