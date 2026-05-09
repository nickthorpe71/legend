pub mod embed;
pub mod intent_classifiers;
pub mod lexical_features;
pub mod math;
pub mod seed;
pub mod steps;
pub mod types;

use std::time::SystemTime;

use seed::load_seed_graph;
use steps::adjust_policy::adjust_policy;
use steps::detect_intent::detect_intent;

/// Maximum tokens accepted in a single tick. Matches GLiNER2's 512-token
/// max-input minus a safety margin for special tokens, positional buffer,
/// and coref-context bytes (§11.4). Inputs above this are rejected at the
/// tick boundary — the caller chunks long inputs into multiple ticks.
pub const MAX_INPUT_TOKENS: usize = 480;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: legend <text>");
        return Ok(());
    }

    let input_text = args[1].clone();
    let _wall_clock = SystemTime::now();

    let token_count = embed::token_count(&input_text);
    if token_count > MAX_INPUT_TOKENS {
        eprintln!(
            "input too long: got {token_count} tokens, max is {MAX_INPUT_TOKENS}.\n\
             legend processes one tick at a time. chunk your input into smaller pieces \
             (each ≤{MAX_INPUT_TOKENS} tokens) and submit them as separate ticks."
        );
        std::process::exit(1);
    }

    let hg = load_seed_graph();

    let embedding = embed::embed_text(&input_text);
    let intent = detect_intent(&input_text, &embedding);
    let policy = adjust_policy(&intent, &hg.policy);

    println!("intent");
    println!("  conviction       {:.3}", intent.conviction);
    println!("  prediction_error {:.3}", intent.prediction_error);
    println!("  arousal          {:.3}", intent.arousal);
    println!("  curiosity        {:.3}", intent.curiosity);

    println!("policy (adjusted)");
    println!("  default_conf           {:.3}", policy.default_conf);
    println!("  salience_multiplier    {:.3}", policy.salience_multiplier);
    println!("  leaf_vigilance         {:.3}", policy.leaf_vigilance);
    println!("  hebbian_rate           {:.3}", policy.hebbian_rate);
    println!(
        "  supersession_threshold {:.3}",
        policy.supersession_threshold
    );

    println!("embedding ({}-dim, shared with Step 1)", embedding.len());
    print!(" ");
    for v in embedding.iter().take(8) {
        print!(" {v:+.4}");
    }
    println!(" …");

    println!("seed graph");
    println!("  elements         {}", hg.elements.len());
    println!("  relations        {}", hg.relations.len());
    println!(
        "  region children of GENESIS  {}",
        hg.region_children.get(&hg.genesis).map_or(0, |v| v.len()),
    );
    Ok(())
}
