use std::time::SystemTime;
use steps::detect_intent::detect_intent;
use types::Input;

pub mod embed;
pub mod prototype_vectors;
pub mod steps;
pub mod types;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    let input = Input {
        text: args[1].clone(),
        wall_clock: SystemTime::now(),
    };
    println!("Input: {:?}", input);

    let intent = detect_intent(&input.text);
    println!("Intent: {:?}", intent);
    Ok(())
}

fn print_help() {
    println!("must input a string!")
}
