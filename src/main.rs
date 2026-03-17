mod cli;
mod commands;
mod memory;
mod tui;

use cli::CommandDef;

// ---------------------------------------------------------------------------
// Top-level command registry
// ---------------------------------------------------------------------------

struct TopCommand {
    def: &'static CommandDef,
    usage_hint: &'static str,
    handler: fn(&[String]) -> Result<(), Box<dyn std::error::Error>>,
}

static COMMANDS: &[TopCommand] = &[
    TopCommand {
        def: &commands::memory::COMMAND,
        usage_hint: "memory [start|tick|query|...]",
        handler: commands::memory::handle_memory,
    },
    TopCommand {
        def: &commands::llm::COMMAND,
        usage_hint: "llm [signals|task|apply|...]",
        handler: commands::llm::handle_llm,
    },
    TopCommand {
        def: &commands::discover::COMMAND,
        usage_hint: "discover [path]",
        handler: commands::discover::handle_discover,
    },
    TopCommand {
        def: &commands::init::COMMAND,
        usage_hint: "init",
        handler: commands::init::handle_init,
    },
    TopCommand {
        def: &commands::mcp::COMMAND,
        usage_hint: "mcp-serve",
        handler: commands::mcp::handle_mcp_serve,
    },
    TopCommand {
        def: &commands::merge::COMMAND,
        usage_hint: "git-merge-driver %O %A %B %P",
        handler: commands::merge::handle_git_merge_driver,
    },
    TopCommand {
        def: &commands::dashboard::COMMAND,
        usage_hint: "dashboard [--3d]",
        handler: commands::dashboard::handle_dashboard_dispatch,
    },
    TopCommand {
        def: &commands::dev::COMMAND,
        usage_hint: "dev <subcommand>",
        handler: commands::dev::handle_dev,
    },
];

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

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
        "help" | "--help" | "-h" => {
            print_help();
            return Ok(());
        }
        _ => {}
    }

    // Registry-based dispatch
    for cmd in COMMANDS {
        if cmd.def.name == args[1] {
            return (cmd.handler)(&args[2..]);
        }
    }

    eprintln!("Unknown command: {}", args[1]);
    print_help();
    std::process::exit(1);
}

fn print_help() {
    let help_entries: Vec<(&CommandDef, &str)> = COMMANDS
        .iter()
        .filter(|c| c.def.name != "dev" && c.def.name != "git-merge-driver")
        .map(|c| (c.def, c.usage_hint))
        .collect();

    cli::print_top_level_help(&help_entries);
    println!("  {:<30}{}", "help", "Show this help message");
}
