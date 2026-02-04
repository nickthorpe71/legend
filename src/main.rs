use std::env;

// Declare our modules
// This tells Rust to look for types.rs, storage.rs, and commands/ in the same directory
mod types;
mod storage;
mod commands;

// Import types we'll use (later layers will use these)
use types::{Feature, FeatureStatus, LegendState};

fn main() {
    // R* principle: Keep main thin, call into run() for error handling
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Collect command-line arguments into a Vec<String>
    // Why Vec and not array? We don't know the arg count at compile time
    // env::args() returns an iterator, .collect() gathers into Vec
    let args: Vec<String> = env::args().collect();

    // args[0] is always the program name ("legend")
    // We need at least 2 args: program name + command
    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    // Get the command as a string slice (&str)
    // Why &str not String? We're borrowing, not taking ownership
    // args[1] is a String, &args[1] gives us &String, which coerces to &str
    let command = &args[1];

    // Match on the command string
    // R* principle: Match is for scannable control flow
    // Each arm is simple - just call a handler function
    match command.as_str() {
        "help" | "--help" | "-h" => {
            print_help();
        }
        "init" => {
            handle_init()?;
        }
        "get_state" => {
            handle_get_state()?;
        }
        "update" => {
            handle_update()?;
        }
        "show" => {
            handle_show()?;
        }
        // Unknown command - this is the catch-all
        unknown => {
            eprintln!("Unknown command: {}", unknown);
            eprintln!();
            print_help();
            std::process::exit(1);
        }
    }

    Ok(())
}

// Print help message
// R* principle: Boring, descriptive names
fn print_help() {
    println!("Legend - Lightweight context memory for AI-assisted development");
    println!();
    println!("Usage:");
    println!("  legend <command> [options]");
    println!();
    println!("Commands:");
    println!("  help                Show this help message");
    println!("  init                Initialize .legend directory");
    println!("  get_state           Print current state as JSON");
    println!("  update              Update feature state from stdin");
    println!("  show                Display human-readable state");
}

// Command handlers
// R* principle: Working code first, implement functionality layer by layer

fn handle_init() -> Result<(), Box<dyn std::error::Error>> {
    // Delegate to the real implementation in commands/init.rs
    commands::init::handle_init()
}

fn handle_get_state() -> Result<(), Box<dyn std::error::Error>> {
    // Delegate to the real implementation in commands/get_state.rs
    commands::get_state::handle_get_state()
}

fn handle_update() -> Result<(), Box<dyn std::error::Error>> {
    println!("update command - not implemented yet");
    Ok(())
}

fn handle_show() -> Result<(), Box<dyn std::error::Error>> {
    println!("show command - not implemented yet");
    Ok(())
}
