pub mod embed;
pub mod intent_classifiers;
pub mod lexical_features;
pub mod math;
pub mod steps;
pub mod types;

use std::time::SystemTime;

use steps::adjust_policy::adjust_policy;
use steps::detect_intent::detect_intent;
use types::{Hypergraph, Input};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: legend <text>");
        return Ok(());
    }

    let input = Input {
        text: args[1].clone(),
        wall_clock: SystemTime::now(),
    };

    let hg = Hypergraph::default();

    let intent = detect_intent(&input.text);
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
    Ok(())
}
