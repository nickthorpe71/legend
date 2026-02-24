mod commands;
mod memory;
mod storage;
mod tui;
mod types;

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    match args[1].as_str() {
        "help" | "--help" | "-h" => print_help(),
        "init" => commands::init::handle_init(&args[2..])?,
        "get_state" => commands::get_state::handle_get_state()?,
        "update" => commands::update::handle_update()?,
        "show" => commands::show::handle_show()?,
        "search" => commands::search::handle_search(&args[2..])?,
        "discover" => commands::discover::handle_discover(&args[2..])?,
        "memory" => commands::memory::handle_memory(&args[2..])?,
        "project" => commands::project::handle_project(&args[2..])?,
        "dev" => commands::dev::handle_dev(&args[2..])?,
        "dashboard" => {
            // Check for --3d flag to launch Bevy dashboard
            if args.iter().any(|a| a == "--3d") {
                commands::dashboard::handle_dashboard()?
            } else {
                tui::run_tui()?
            }
        }
        unknown => {
            eprintln!("Unknown command: {}", unknown);
            print_help();
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_help() {
    println!("Legend - Lightweight context memory for AI-assisted development");
    println!();
    println!("Usage: legend <command> [options]");
    println!();
    println!("Commands:");
    println!("  memory [start|tick|query|...]  Hierarchical memory (context, decisions, history)");
    println!("  project [ls|set|schema]         Project feature roadmap & status management");
    println!("  discover [path]                 Scan project to suggest features and context");
    println!("  dashboard [--3d]               Launch TUI dashboard (--3d for Bevy 3D view)");
    println!("  init                            Initialize Legend in new project");
    println!("  help                            Show this help message");
}
