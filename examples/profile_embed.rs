//! Profile where time goes for a single `embed_text` invocation.
//! Run: `cargo run --release --example profile_embed`
//! Throwaway diagnostic.

use legend::embed::embed_text;
use std::time::Instant;

fn main() {
    // Warm-up — first call lazy-loads the model.
    let t0 = Instant::now();
    let _ = embed_text("warm up");
    println!("first call (model load + 1 embed):   {:>8.1?}", t0.elapsed());

    // Subsequent calls — model is already loaded.
    let phrases = [
        "sarah is my friend",
        "I prefer green tea",
        "the meeting is at 3pm",
        "Alice told me the deploy went live",
        "a quaternion extends complex numbers",
    ];
    let mut total = std::time::Duration::ZERO;
    for p in &phrases {
        let t = Instant::now();
        let _ = embed_text(p);
        let dt = t.elapsed();
        total += dt;
        println!("warm embed ({}b text):              {:>8.1?}", p.len(), dt);
    }
    println!(
        "warm avg over {} calls:              {:>8.1?}",
        phrases.len(),
        total / phrases.len() as u32,
    );

    // Batch of 20 — same shape as embedding one region's examples.
    let batch: Vec<String> = (0..20)
        .map(|i| format!("test sentence number {i} for batch timing"))
        .collect();
    let t = Instant::now();
    for s in &batch {
        let _ = embed_text(s);
    }
    println!("20 calls (one region's examples):   {:>8.1?}", t.elapsed());
}
