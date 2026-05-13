//! Wall-clock sweep for Step 5 (`run_extractors`) across realistic
//! input shapes. Reports p50/p90/mean for each (input_token_count,
//! label_count) combination so we can see how latency scales.
//!
//! Run: `cargo run --release --features gliner2_fp32 --example bench_step5_sweep`

use std::time::Instant;

use legend::inference::deberta::predict::predict_entities;
use legend::inference::deberta::weights_int8::WeightsDebertaInt8;

const FIXTURES: &[(&str, &str)] = &[
    (
        "short_24t",
        "My dentist appointment with Dr. Rao changed from Tuesday to Friday.",
    ),
    (
        "medium_60t",
        "Alice and Bob met at the conference in Berlin on Tuesday. They discussed the GraphQL migration with the team from Acme. The talk was rescheduled from Wednesday to Friday morning because the speaker was sick.",
    ),
    (
        "long_120t",
        "Project Atlas kicks off next Monday in San Francisco with engineers from Anthropic, Google, and Meta. The first sprint covers a redesign of the retrieval backend, switching from elasticsearch to a hybrid vector + bm25 setup served by Qdrant. Carol will lead the migration team. Dave will own the API surface. The plan was originally to start in October but the leadership team — Ellen, Frank, Greg — pushed it to early November so the security review could finish on time. The on-call rotation moves from Tuesday to Friday handoffs.",
    ),
    (
        "verbose_300t",
        "The first paragraph describes the dentist appointment with Dr. Rao moving from Tuesday to Friday. The second covers Alice and Bob meeting at the Berlin conference, and their conversation about the GraphQL migration. The third introduces Project Atlas, scheduled to launch in San Francisco next Monday with participation from Anthropic, Google, and Meta. Carol leads the retrieval-backend rewrite, swapping elasticsearch for a hybrid Qdrant cluster, while Dave owns the API. Originally October, the launch was pushed to November after Ellen, Frank, and Greg insisted on a security review. The on-call schedule shifts handoffs from Tuesday to Friday, and the standup moves to 09:30 every weekday. By the end of the quarter the team expects to ship the new ranker, the updated GraphQL gateway, and a working migration tool from elasticsearch into Qdrant — provided that the security review concludes by mid-November. Everyone hopes it works.",
    ),
];

const SHORT_LABELS: &[&str] = &["person", "event", "weekday", "place"];
const FULL_LABELS: &[&str] = &[
    "person", "org", "place", "weekday", "quantity", "event", "role", "state", "time", "project",
    "technology",
];

fn percentile(xs: &[f64], p: usize) -> f64 {
    let mut ys = xs.to_vec();
    ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = (ys.len() * p / 100).min(ys.len() - 1);
    ys[idx]
}

fn run_sweep() {
    // Warm the weights and INT8 kernels.
    let _ = WeightsDebertaInt8::load_bundled();
    let _ = predict_entities("warmup", SHORT_LABELS, 0.3);

    println!(
        "{:<12} {:<6} {:>7} {:>7} {:>7} {:>5}",
        "fixture", "labels", "p50ms", "p90ms", "mean", "n"
    );
    println!(
        "{:-<12} {:-<6} {:->7} {:->7} {:->7} {:->5}",
        "", "", "", "", "", ""
    );

    let n = 20;
    for &(name, text) in FIXTURES {
        for &(label_tag, labels) in
            &[("short", SHORT_LABELS), ("full", FULL_LABELS)]
        {
            let mut samples = Vec::with_capacity(n);
            for _ in 0..n {
                let start = Instant::now();
                let _ = predict_entities(text, labels, 0.3);
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            let p50 = percentile(&samples, 50);
            let p90 = percentile(&samples, 90);
            let mean = samples.iter().sum::<f64>() / n as f64;
            println!(
                "{:<12} {:<6} {:>7.1} {:>7.1} {:>7.1} {:>5}",
                name, label_tag, p50, p90, mean, n
            );
        }
    }
    println!("\nv0 budget for run_extractors: 130-208 ms p50.");
}

fn main() {
    run_sweep();
}
