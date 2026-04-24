//! Measure raw embedding inference cost — quick sanity check for where tick
//! latency actually goes. Run with: `cargo run --release --example embed_bench`

use legend::memory::entorhinal::embed_text;
use std::time::Instant;

fn main() {
    // Warm up the LazyLock (ONNX parse + optimize + runnable).
    let warm = Instant::now();
    let _ = embed_text("warm up call", 384);
    println!("cold first call (includes ONNX init): {:.3?}", warm.elapsed());

    // Measure repeated unique inputs — cache miss every time.
    for i in 0..5 {
        let text = format!("benchmark unique input number {}", i);
        let start = Instant::now();
        let _vec = embed_text(&text, 384);
        println!("miss call {}: {:.3?}", i, start.elapsed());
    }

    // Cached hit (same string as warmup).
    let start = Instant::now();
    let _ = embed_text("warm up call", 384);
    println!("cache hit: {:.3?}", start.elapsed());
}
