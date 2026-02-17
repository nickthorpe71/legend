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
        "init" => commands::init::handle_init()?,
        "get_state" => commands::get_state::handle_get_state()?,
        "update" => commands::update::handle_update()?,
        "show" => commands::show::handle_show()?,
        "search" => commands::search::handle_search(&args[2..])?,
        "discover" => commands::discover::handle_discover(&args[2..])?,
        "memory" => commands::memory::handle_memory(&args[2..])?,
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
    println!("  init                Initialize .legend directory");
    println!("  get_state           Print current state as JSON");
    println!("  update              Update feature state from stdin");
    println!("  show                Display human-readable state");
    println!("  search <query>      Search features (--domain, --tag, --status)");
    println!("  discover [path]     Scan project and suggest features");
    println!("  memory <sub>        Hierarchical memory (tick, query, stats, context, sessions, reset, consolidate)");
    println!("  dashboard [--3d]    Launch the TUI dashboard (--3d for Bevy 3D view)");
    println!("  help                Show this help message");
}
