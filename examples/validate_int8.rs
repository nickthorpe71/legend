//! Side-by-side validation: same input, fp32 vs INT8 forward pass.
//! Reports cosine similarity (1.0 = identical, ≥0.99 is acceptable
//! for INT8 quantization).
//!
//! Run: `cargo run --release --example validate_int8`

use std::time::Instant;

use legend::inference::{bert, bert_int8, Weights, WeightsInt8};
use legend::math::dot;
use tokenizers::Tokenizer;

fn embed_with(forward: impl Fn(&[u32], &[u32]) -> Vec<f32>, text: &str, tok: &Tokenizer) -> Vec<f32> {
    if text.trim().is_empty() {
        return vec![0.0f32; 384];
    }
    let enc = tok.encode(text, true).expect("tokenize");
    forward(enc.get_ids(), enc.get_attention_mask())
}

fn main() {
    let tokenizer_bytes: &[u8] = include_bytes!("../models/all-MiniLM-L6-v2-q/tokenizer.json");
    let mut tok = Tokenizer::from_bytes(tokenizer_bytes).expect("load tokenizer");
    let _ = tok.with_truncation(Some(tokenizers::TruncationParams {
        max_length: 512,
        ..Default::default()
    }));

    let w_fp32: &Weights = Weights::load_bundled();
    let w_int8: &WeightsInt8 = WeightsInt8::load_bundled();

    // Warm both code paths so the first-call cost doesn't pollute timing.
    let _ = bert::forward(w_fp32, &[101, 102], &[1, 1]);
    let _ = bert_int8::forward(w_int8, &[101, 102], &[1, 1]);

    let inputs: &[&str] = &[
        "hello world",
        "sarah is my friend",
        "I prefer green tea to black coffee",
        "the meeting is at 3pm on Tuesday",
        "a quaternion extends complex numbers to four dimensions",
        "the conference is at the Hilton downtown",
        "I need to finish the quarterly report by Friday",
        "Sarah is a software engineer at Anthropic",
        "the price went from ten dollars to twenty dollars",
        "Alice told me that Bob saw the package arrive",
    ];

    println!(
        "{:<50} {:>9} {:>9} {:>10} {:>8}",
        "input", "fp32 ms", "int8 ms", "cosine", "speedup"
    );
    println!("{:-<50} {:->9} {:->9} {:->10} {:->8}", "", "", "", "", "");

    let mut all_cosines: Vec<f32> = Vec::with_capacity(inputs.len());
    let mut total_fp32_us = 0u128;
    let mut total_int8_us = 0u128;

    for input in inputs {
        // Time fp32
        let t = Instant::now();
        let v_fp32 = embed_with(|ids, mask| bert::forward(w_fp32, ids, mask), input, &tok);
        let dt_fp32 = t.elapsed();
        total_fp32_us += dt_fp32.as_micros();

        // Time int8
        let t = Instant::now();
        let v_int8 = embed_with(
            |ids, mask| bert_int8::forward(w_int8, ids, mask),
            input,
            &tok,
        );
        let dt_int8 = t.elapsed();
        total_int8_us += dt_int8.as_micros();

        let cos = dot(&v_fp32, &v_int8);
        all_cosines.push(cos);
        let speedup = dt_fp32.as_secs_f64() / dt_int8.as_secs_f64();
        println!(
            "{:<50.50} {:>9.2} {:>9.2} {:>10.6} {:>7.2}×",
            input,
            dt_fp32.as_secs_f64() * 1000.0,
            dt_int8.as_secs_f64() * 1000.0,
            cos,
            speedup,
        );
    }

    let avg_cos: f32 = all_cosines.iter().sum::<f32>() / all_cosines.len() as f32;
    let min_cos: f32 = all_cosines.iter().copied().fold(f32::INFINITY, f32::min);
    let total_speedup = total_fp32_us as f64 / total_int8_us as f64;
    println!();
    println!("Summary:");
    println!("  Average cosine fp32 vs int8: {avg_cos:.6}");
    println!("  Minimum cosine:              {min_cos:.6}");
    println!(
        "  Total time fp32:             {:.2} ms",
        total_fp32_us as f64 / 1000.0
    );
    println!(
        "  Total time int8:             {:.2} ms",
        total_int8_us as f64 / 1000.0
    );
    println!("  Overall speedup:             {total_speedup:.2}×");

    if min_cos < 0.99 {
        eprintln!();
        eprintln!("WARNING: minimum cosine below 0.99 — quantization may be lossy");
        std::process::exit(1);
    }
}
