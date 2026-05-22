//! One-shot smoke test: load SmolLM2-360M-Instruct via the shared
//! `verbalizer` module, run a single hardcoded prompt, print the
//! generated answer. Verifies the candle + tokenizers integration
//! works end-to-end and shows generation latency on this machine.
//!
//! First run downloads ~720 MB of model weights into
//! `benchmarks/models/SmolLM2-360M-Instruct/`. Subsequent runs use
//! the cache.
//!
//! Run:
//!   cargo run --release --example verbalize_smoke

#[path = "verbalizer.rs"]
mod verbalizer;

use anyhow::Result;
use std::time::Instant;
use verbalizer::Verbalizer;

fn main() -> Result<()> {
    let t0 = Instant::now();
    let mut v = Verbalizer::load()?;
    eprintln!("loaded verbalizer in {:.1}s", t0.elapsed().as_secs_f64());

    let context = "The chief executive officer of Microsoft is Satya Nadella.\n\
                   The chief executive officer of Microsoft is Steve Jobs.";
    let question = "Who is the chief executive officer of Microsoft?";

    let t1 = Instant::now();
    let answer = v.verbalize(question, context)?;
    let secs = t1.elapsed().as_secs_f64();

    println!();
    println!("context: {context:?}");
    println!("question: {question:?}");
    println!("answer: {answer:?}");
    println!("(generated in {secs:.2}s)");

    Ok(())
}
