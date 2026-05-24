//! Benchmarks for the embedder and intent detection.
//!
//! Run with: `cargo bench`
//!
//! The first call to either `embed_text` or `detect_intent` lazily loads the
//! ONNX model (~300-500 ms one-time). We warm up before timing so the
//! reported numbers reflect steady-state per-call cost, not first-call cost.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use legend::embed::embed_text;
use legend::tick_pipeline::detect_intent::detect_intent;

// Note: as of the Step 4 refactor, `detect_intent` no longer embeds — it
// takes a precomputed embedding (the pipeline computes it once and shares
// it with Step 4 region routing). The `bench_detect_intent` benchmark
// reflects that and only measures classifier inference; for the
// historical "embed + classify" cost, sum it with `bench_embed_text`.

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
    // Warm up — loads model + classifiers.
    let warmup_emb = embed_text("warm-up");
    let _ = detect_intent("warm-up", &warmup_emb);

    let high_conv = "I'm absolutely certain that grass is green";
    let low_conv = "I'm not sure if the grass is green";
    let high_cur = "Find when I last saw Dr. Rao";
    // Embeddings precomputed outside the timing loop — detect_intent no
    // longer embeds, and the benchmark should reflect that cost split.
    let emb_high = embed_text(high_conv);
    let emb_low = embed_text(low_conv);
    let emb_cur = embed_text(high_cur);

    c.bench_function("detect_intent (high-conviction statement)", |b| {
        b.iter(|| detect_intent(black_box(high_conv), black_box(&emb_high)))
    });

    c.bench_function("detect_intent (low-conviction statement)", |b| {
        b.iter(|| detect_intent(black_box(low_conv), black_box(&emb_low)))
    });

    c.bench_function("detect_intent (high-curiosity question)", |b| {
        b.iter(|| detect_intent(black_box(high_cur), black_box(&emb_cur)))
    });
}

criterion_group!(benches, bench_embed_text, bench_detect_intent);
criterion_main!(benches);
