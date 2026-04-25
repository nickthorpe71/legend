//! Per-subsystem tick latency profile.
//!
//! Builds with `--features instrument`. Initializes Legend's trace channel,
//! runs a set of ticks against the master-clone state at
//! `/tmp/legend_bench/.legend/`, and prints a breakdown of which pipeline
//! step each millisecond of tick latency is landing in.
//!
//! Usage:
//!   cargo run --release --features instrument --example tick_profile
//!
//! Methodology mirrors `examples/embed_bench.rs` — load the real state the
//! #04/#04a baselines use so numbers are comparable. Runs each sample
//! against the same state (state grows across samples); report the median
//! and the slowest sample's per-step hotspots.

use std::collections::HashMap;
use std::time::Instant;

use legend::memory::trace;
use legend::memory::TickOptions;

/// How many ticks to run. First is cold (ORT init + state already loaded),
/// rest are warm.
const SAMPLES: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the trace channel BEFORE anything touches the tick path.
    let rx = trace::init_trace_channel().expect("init trace channel");

    // Operate in the same state location as #04/#04a so we see realistic
    // decay/graph-scan costs on an 11k-entry graph.
    let bench_dir = std::path::Path::new("/tmp/legend_bench");
    if !bench_dir.join(".legend").exists() {
        eprintln!(
            "expected {} to exist (clone of master state); restore via:\n  rm -rf /tmp/legend_bench/.legend && cp -r /tmp/legend_bench_master/.legend /tmp/legend_bench/",
            bench_dir.display()
        );
        return Err("no bench state".into());
    }
    std::env::set_current_dir(bench_dir)?;

    let mut state = legend::memory::load_or_default()?;

    // A small bank of unique text so the embedding cache doesn't mask real
    // inference cost. Each tick embeds a fresh string.
    let samples: Vec<String> = (0..SAMPLES)
        .map(|i| {
            format!(
                "DECISION: profiling tick latency sample {} with some descriptive body text",
                i
            )
        })
        .collect();

    let mut per_sample_events: Vec<(std::time::Duration, Vec<trace::TraceEvent>)> =
        Vec::with_capacity(SAMPLES);

    // Match the daemon's CLI tick path: compute_context = false. The MCP
    // tick path keeps context on; we measure the CLI/daemon contract here
    // since that is what §#06 budgets target (100 ms warm).
    let options = TickOptions {
        compute_context: false,
        defer_consolidation: true,
        defer_offline_replay: true,
    };
    for text in &samples {
        let t0 = Instant::now();
        let _ = legend::memory::tick_with_options(&mut state, text, options);
        let total = t0.elapsed();

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        per_sample_events.push((total, events));
    }

    // ---- Summary: per-step deltas, averaged over non-first samples ----
    //
    // Sample 0 is the cold one (ORT session init fires on first embed). We
    // include it in the raw output but exclude it from the average so the
    // reported numbers reflect steady-state latency.

    println!("=== raw per-sample wall time ===");
    for (i, (total, _events)) in per_sample_events.iter().enumerate() {
        println!("  sample {i}: {:.3?}", total);
    }
    let warm: Vec<_> = per_sample_events.iter().skip(1).collect();
    if !warm.is_empty() {
        let total_sum: std::time::Duration = warm.iter().map(|(t, _)| *t).sum();
        let avg = total_sum / warm.len() as u32;
        let mut sorted: Vec<_> = warm.iter().map(|(t, _)| *t).collect();
        sorted.sort();
        let median = sorted[sorted.len() / 2];
        println!("\n  warm mean:   {:.3?}", avg);
        println!("  warm median: {:.3?}", median);
    }

    println!("\n=== per-step delta by pipeline step (warm samples only) ===");
    println!("{:<30} {:>12} {:>12} {:>12}", "step", "mean (µs)", "median (µs)", "max (µs)");

    let mut per_step: HashMap<String, Vec<u64>> = HashMap::new();
    for (_, events) in warm.iter() {
        // Deltas: each event's contribution is the time between itself and
        // the previous event. The first event's delta is just its own
        // elapsed_us (time from TickStart which is emitted first).
        let mut prev_us: u64 = 0;
        for event in events.iter() {
            let delta = event.elapsed_us.saturating_sub(prev_us);
            per_step
                .entry(format!("{:?}", event.step))
                .or_default()
                .push(delta);
            prev_us = event.elapsed_us;
        }
    }

    let mut steps: Vec<(String, Vec<u64>)> = per_step.into_iter().collect();
    steps.sort_by_key(|(_, v)| {
        let sum: u64 = v.iter().sum();
        std::cmp::Reverse(sum / v.len().max(1) as u64)
    });

    for (name, deltas) in &steps {
        if deltas.is_empty() {
            continue;
        }
        let mut sorted = deltas.clone();
        sorted.sort();
        let mean = deltas.iter().sum::<u64>() / deltas.len() as u64;
        let median = sorted[sorted.len() / 2];
        let max = *sorted.last().unwrap();
        println!(
            "{:<30} {:>12} {:>12} {:>12}",
            name, mean, median, max
        );
    }

    // ---- Slowest-sample breakdown ----
    if let Some((slow_idx, (slow_total, slow_events))) = per_sample_events
        .iter()
        .enumerate()
        .skip(1)
        .max_by_key(|(_, (t, _))| *t)
    {
        println!(
            "\n=== slowest warm sample: #{} ({:.3?}) — full step timeline ===",
            slow_idx, slow_total
        );
        let mut prev_us: u64 = 0;
        for event in slow_events {
            let delta = event.elapsed_us.saturating_sub(prev_us);
            println!(
                "  +{:>7} µs  @{:>8} µs  {:?}",
                delta, event.elapsed_us, event.step
            );
            prev_us = event.elapsed_us;
        }
    }

    Ok(())
}
