//! INT8 GLiNER2 wall-clock benchmark on the dentist fixture (24 tokens).
//! v0 budget per `new_foundation_v0_core.md` §5 is 130–208 ms p50 for
//! `run_extractors`; this measures the inference half of that.
//!
//! Run: `cargo run --release --example bench_gliner2_int8 --features gliner2_fp32`

use std::time::Instant;

use legend::inference::deberta::forward_int8::predict_entities_int8;
use legend::inference::deberta::weights_int8::WeightsDebertaInt8;

const IDS: &[u32] = &[
    1, 128002, 604, 128002, 720, 128002, 20467, 128002, 985, 128003, 573, 8301, 3198, 275, 1011,
    323, 25773, 1594, 292, 1586, 264, 1178, 323, 2,
];
const MASK: &[u32] = &[1; 24];
const WORDS_MASK: &[u32] = &[
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 0,
];
const MAX_WIDTH: usize = 12;
const THRESHOLD: f32 = 0.3;

fn main() {
    let w = WeightsDebertaInt8::load_bundled();

    // Warm-up.
    let _ = predict_entities_int8(w, IDS, MASK, WORDS_MASK, MAX_WIDTH, THRESHOLD);

    let n = 30;
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let start = Instant::now();
        let _ = predict_entities_int8(w, IDS, MASK, WORDS_MASK, MAX_WIDTH, THRESHOLD);
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = samples[n / 2];
    let p10 = samples[n / 10];
    let p90 = samples[(n * 9) / 10];
    let mean = samples.iter().sum::<f64>() / n as f64;
    println!("GLiNER2 INT8 — dentist fixture (24 tokens, 4 labels)");
    println!("  iters {n}");
    println!("  p10   {:>7.2} ms", p10);
    println!("  p50   {:>7.2} ms", p50);
    println!("  p90   {:>7.2} ms", p90);
    println!("  mean  {:>7.2} ms", mean);
    println!("\n  v0 budget for `run_extractors` (run_extractors)   130-208 ms p50");
}
