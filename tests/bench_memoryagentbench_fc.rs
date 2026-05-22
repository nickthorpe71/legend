//! Long-form integration tests for MemoryAgentBench
//! FactConsolidation. One test per measured cell of the 3×2 grid
//! (context size × hop count) — six in total, all `#[ignore]` so
//! they stay out of the default `cargo test` set.
//!
//! Each test is expensive: ~2–30 minutes depending on row size.
//! Run individually by name when validating a specific cell, or
//! all together as a release-gate when you want full coverage.
//!
//! ## Run all six (slow — ~2.5 h wall time sequential, less in parallel)
//!
//! ```bash
//! cargo build --release --example bench_memoryagentbench_fc
//! cargo test --release --test bench_memoryagentbench_fc -- --ignored --nocapture
//! ```
//!
//! ## Run one specific cell
//!
//! ```bash
//! cargo build --release --example bench_memoryagentbench_fc
//! cargo test --release --test bench_memoryagentbench_fc \
//!     factconsolidation_sh_6k -- --ignored --nocapture
//! ```
//!
//! ## What each test asserts
//!
//! Three floors per cell, each tied to a different failure mode so
//! a regression points at the responsible layer:
//!
//! 1. **MIN_FRAME_HITS** — questions whose gold answer appears in
//!    the focused frame. The headline score. Pinned ~3 pts below
//!    the latest measured value so small drift doesn't trip the
//!    test; raise it when an improvement lands.
//! 2. **MAX_SUBSTRATE_ONLY** — questions where the gold is in the
//!    substrate but the frame didn't surface it. Catches *retrieval*
//!    regressions even if headline holds.
//! 3. **MAX_ABSENT** — questions where the gold is not in the
//!    substrate at all. Catches *extractor* regressions.
//!
//! Measured values as of 2026-05-21 are recorded in the per-test
//! constants below.

use std::path::PathBuf;
use std::process::Command;

const QUESTIONS: usize = 100;

// ─── Per-variant test wrappers ──────────────────────────────────────
//
// Floors are set ~3 pts below the latest measured headline score
// and a few above the measured retrieval/extractor counts so that
// run-to-run variance doesn't trip the gate.

#[test]
#[ignore]
fn factconsolidation_sh_6k_meets_baseline() {
    // Measured 2026-05-21: 91 / 8 / 1
    run_variant("sh_6k", 88, 12, 2);
}

#[test]
#[ignore]
fn factconsolidation_sh_32k_meets_baseline() {
    // Measured 2026-05-21: 90 / 8 / 2
    run_variant("sh_32k", 87, 12, 4);
}

#[test]
#[ignore]
fn factconsolidation_sh_64k_meets_baseline() {
    // Measured 2026-05-21: 89 / 10 / 1
    run_variant("sh_64k", 86, 14, 3);
}

#[test]
#[ignore]
fn factconsolidation_mh_6k_meets_baseline() {
    // Measured 2026-05-21: 81 / 16 / 3.
    // Validity caveat: mh scores are "gold entity in focused frame,"
    // not "model composed the right answer." See bench doc.
    run_variant("mh_6k", 78, 20, 5);
}

#[test]
#[ignore]
fn factconsolidation_mh_32k_meets_baseline() {
    // Measured 2026-05-21: 78 / 22 / 0
    run_variant("mh_32k", 75, 26, 2);
}

#[test]
#[ignore]
fn factconsolidation_mh_64k_meets_baseline() {
    // Measured 2026-05-21: 74 / 25 / 1
    run_variant("mh_64k", 71, 29, 3);
}

// ─── Shared runner ──────────────────────────────────────────────────

fn run_variant(variant: &str, min_hits: u32, max_substrate_only: u32, max_absent: u32) {
    let bin = PathBuf::from("target/release/examples/bench_memoryagentbench_fc");
    assert!(
        bin.exists(),
        "harness binary not found at {}.\n\
         Run `cargo build --release --example bench_memoryagentbench_fc` first.",
        bin.display(),
    );

    let output = Command::new(&bin)
        .args(["--variant", variant, "--questions", &QUESTIONS.to_string()])
        .output()
        .expect("failed to spawn bench harness");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "harness exited non-zero ({:?})\n--- stderr ---\n{}\n--- stdout (tail) ---\n{}",
        output.status.code(),
        stderr,
        tail(&stdout, 40),
    );

    let summary = stdout
        .lines()
        .find(|l| l.starts_with("summary:"))
        .unwrap_or_else(|| {
            panic!(
                "no `summary:` line in harness output:\n{}",
                tail(&stdout, 40)
            )
        });

    let parsed = parse_summary(summary)
        .unwrap_or_else(|| panic!("could not parse summary line: {summary:?}"));

    eprintln!(
        "FactConsolidation-{} ({} questions): hits={} substrate_only={} absent={}",
        variant, QUESTIONS, parsed.hits, parsed.substrate_only, parsed.absent,
    );

    assert!(
        parsed.hits >= min_hits,
        "{variant}: frame hits {} below baseline {} — likely a retrieval or extraction regression",
        parsed.hits,
        min_hits,
    );
    assert!(
        parsed.substrate_only <= max_substrate_only,
        "{variant}: substrate-only misses {} above ceiling {} — retrieval surfaced fewer facts than it should",
        parsed.substrate_only,
        max_substrate_only,
    );
    assert!(
        parsed.absent <= max_absent,
        "{variant}: absent-from-substrate count {} above ceiling {} — extractor regressed",
        parsed.absent,
        max_absent,
    );
}

// ─── Helpers ────────────────────────────────────────────────────────

#[derive(Debug)]
struct Summary {
    hits: u32,
    substrate_only: u32,
    absent: u32,
}

/// Parse a line like:
///   summary: 91/100 frame hits (91.0%) | 8 substrate-only (frame missed it) | 1 absent from substrate
///
/// Resilient to whitespace and to small phrasing tweaks — we anchor
/// on the four integers, not on the prose between them. Order is:
/// hits, total, [percent], substrate_only, absent.
fn parse_summary(line: &str) -> Option<Summary> {
    let nums: Vec<u32> = line
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();
    if nums.len() < 4 {
        return None;
    }
    Some(Summary {
        hits: nums[0],
        substrate_only: nums[nums.len() - 2],
        absent: *nums.last()?,
    })
}

fn tail(s: &str, lines: usize) -> String {
    let v: Vec<&str> = s.lines().collect();
    let start = v.len().saturating_sub(lines);
    v[start..].join("\n")
}
