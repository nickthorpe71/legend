//! Wall-clock benchmark of the fp32 GLiNER2 forward pass on the dentist
//! fixture (24 input tokens). The bound from v0_core §5 is 130–208 ms
//! p50 — that target assumes INT8; fp32 will be slower. Use this number
//! as the baseline for the Phase 6 INT8 work.
//!
//! Run: `cargo run --release --example bench_gliner2 --features gliner2_fp32`

use std::time::Instant;

use legend::inference::deberta::embedding::embed_and_layernorm;
use legend::inference::deberta::encoder::run_encoder_stack;
use legend::inference::deberta::head::{
    build_span_rep, decode, generate_span_indices, project_prompts, project_tokens, run_bilstm,
    score, split_tokens,
};
use legend::inference::deberta::weights::WeightsDebertaV3;

const IDS: &[u32] = &[
    1, 128002, 604, 128002, 720, 128002, 20467, 128002, 985, 128003, 573, 8301, 3198, 275, 1011,
    323, 25773, 1594, 292, 1586, 264, 1178, 323, 2,
];
const MASK: &[u32] = &[1; 24];
const WORDS_MASK: &[u32] = &[
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0,
];
const LABELS: &[&str] = &["person", "event", "weekday", "role"];
const MAX_WIDTH: usize = 12;
const THRESHOLD: f32 = 0.3;

fn run_once(w: &WeightsDebertaV3) -> Vec<f32> {
    let x = embed_and_layernorm(w, IDS, MASK);
    let enc = run_encoder_stack(w, x, MASK);
    let projected = project_tokens(w, &enc, IDS.len());
    let split = split_tokens(w, &projected, IDS, WORDS_MASK, IDS.len());
    let lstm = run_bilstm(w, &split.words, split.num_words);
    let (spans, valid) = generate_span_indices(split.num_words, MAX_WIDTH);
    let span_rep = build_span_rep(w, &lstm, split.num_words, &spans);
    let prompts = project_prompts(w, &split.prompts, split.num_prompts);
    let scores = score(
        &span_rep,
        &prompts,
        spans.len(),
        split.num_prompts,
        w.projection_out,
    );
    let _entities = decode(&scores, &spans, &valid, split.num_prompts, THRESHOLD);
    scores
}

fn main() {
    let w = WeightsDebertaV3::load_bundled();

    // Warm-up — first call pays for any one-time allocs.
    let _ = run_once(w);

    let n_iters = 30;
    let mut samples = Vec::with_capacity(n_iters);
    for _ in 0..n_iters {
        let start = Instant::now();
        let _ = run_once(w);
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = samples[n_iters / 2];
    let p10 = samples[n_iters / 10];
    let p90 = samples[(n_iters * 9) / 10];
    let mean = samples.iter().sum::<f64>() / n_iters as f64;

    println!("GLiNER2 fp32 — dentist fixture (24 tokens, 4 labels)");
    println!("  iters {n_iters}");
    println!("  p10   {:>7.2} ms", p10);
    println!("  p50   {:>7.2} ms", p50);
    println!("  p90   {:>7.2} ms", p90);
    println!("  mean  {:>7.2} ms", mean);
    println!("\n  v0 budget for Step 5 (run_extractors)   130-208 ms p50 (INT8)");
    println!("  unused label set: {LABELS:?}");
}
