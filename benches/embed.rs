//! Benchmarks for the embedder and intent detection.
//!
//! Run with: `cargo bench`
//!
//! The first call to either `embed_text` or `detect_intent` lazily loads the
//! ONNX model (~300-500 ms one-time). We warm up before timing so the
//! reported numbers reflect steady-state per-call cost, not first-call cost.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use legend::embed::embed_text;
use legend::steps::detect_intent::detect_intent;

fn bench_embed_text(c: &mut Criterion) {
    // Warm up — first call loads the model.
    let _ = embed_text("warm-up");

    c.bench_function("embed_text short (statement)", |b| {
        b.iter(|| embed_text(black_box("I'm absolutely certain that grass is green")))
    });

    c.bench_function("embed_text short (question)", |b| {
        b.iter(|| embed_text(black_box("Find when I last saw Dr. Rao")))
    });

    c.bench_function("embed_text empty", |b| b.iter(|| embed_text(black_box(""))));

    let long_input: String = "This is a long input sentence. ".repeat(50);
    c.bench_function("embed_text long", |b| {
        b.iter(|| embed_text(black_box(&long_input)))
    });
}

fn bench_detect_intent(c: &mut Criterion) {
    // Warm up — loads model + prototypes.
    let _ = detect_intent("warm-up");

    c.bench_function("detect_intent (high-conviction statement)", |b| {
        b.iter(|| detect_intent(black_box("I'm absolutely certain that grass is green")))
    });

    c.bench_function("detect_intent (low-conviction statement)", |b| {
        b.iter(|| detect_intent(black_box("I'm not sure if the grass is green")))
    });

    c.bench_function("detect_intent (high-curiosity question)", |b| {
        b.iter(|| detect_intent(black_box("Find when I last saw Dr. Rao")))
    });
}

criterion_group!(benches, bench_embed_text, bench_detect_intent);
criterion_main!(benches);
