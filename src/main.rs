mod cli;
mod commands;
mod memory;
mod tool;
mod tui;

use cli::{ArgDef, CommandDef, FlagDef};

// ---------------------------------------------------------------------------
// CLI Command Definitions — children defined before parents
// ---------------------------------------------------------------------------

// -- legend memory subcommands --
static MEMORY_TICK: CommandDef = CommandDef {
    name: "tick",
    about: "Record a memory (decision, progress, discovery)",
    usage: "legend memory tick <text>",
    flags: &[],
    positionals: &[],
    children: &[],
};
static MEMORY_QUERY: CommandDef = CommandDef {
    name: "query",
    about: "Query memory (read-only retrieval)",
    usage: "legend memory query [--reasons] <text>",
    flags: &[FlagDef {
        long: "--reasons",
        short: Some('r'),
        about: "Include retrieval reasoning",
        takes_value: false,
    }],
    positionals: &[],
    children: &[],
};
static MEMORY_START: CommandDef = CommandDef {
    name: "start",
    about: "Session start: context + categorized in one call",
    usage: "legend memory start [options] [query]",
    flags: &[
        FlagDef {
            long: "--compact",
            short: Some('c'),
            about: "Compact output",
            takes_value: false,
        },
        FlagDef {
            long: "--json",
            short: Some('j'),
            about: "JSON output",
            takes_value: false,
        },
        FlagDef {
            long: "--tokens",
            short: Some('t'),
            about: "Show token overhead",
            takes_value: false,
        },
        FlagDef {
            long: "--category",
            short: None,
            about: "Filter by category",
            takes_value: true,
        },
        FlagDef {
            long: "--query",
            short: Some('q'),
            about: "Query string",
            takes_value: true,
        },
    ],
    positionals: &[],
    children: &[],
};
static MEMORY_SESSIONS: CommandDef = CommandDef {
    name: "sessions",
    about: "Show session log entries",
    usage: "legend memory sessions [--all] [count]",
    flags: &[FlagDef {
        long: "--all",
        short: Some('a'),
        about: "Include empty entries",
        takes_value: false,
    }],
    positionals: &[],
    children: &[],
};

static MEMORY_PLAN: CommandDef = CommandDef {
    name: "plan",
    about: "Surgical edits to the executive plan queue (set-status, …)",
    usage: "legend memory plan <subcommand> [args]",
    flags: &[],
    positionals: &[],
    children: &[],
};

// -- legend memory --
static MEMORY: CommandDef = CommandDef {
    name: "memory",
    about: "Hierarchical memory (context, decisions, history)",
    usage: "legend memory <subcommand> [options]",
    flags: &[],
    positionals: &[],
    children: &[
        &MEMORY_START,
        &MEMORY_TICK,
        &MEMORY_QUERY,
        &MEMORY_SESSIONS,
        &MEMORY_PLAN,
    ],
};

// -- leaf commands --
static DISCOVER: CommandDef = CommandDef {
    name: "discover",
    about: "Scan project to suggest features and context",
    usage: "legend discover [path] [--apply]",
    flags: &[FlagDef {
        long: "--apply",
        short: None,
        about: "Ingest context into Legend memory",
        takes_value: false,
    }],
    positionals: &[ArgDef {
        name: "PATH",
        about: "Project path to scan",
        required: false,
    }],
    children: &[],
};

static INIT: CommandDef = CommandDef {
    name: "init",
    about: "Initialize Legend in a new project",
    usage: "legend init [--discover] [--help]",
    flags: &[
        FlagDef {
            long: "--discover",
            short: None,
            about: "Scan existing project to pre-fill memory context",
            takes_value: false,
        },
        FlagDef {
            long: "--help",
            short: Some('h'),
            about: "Show this help message",
            takes_value: false,
        },
    ],
    positionals: &[],
    children: &[],
};

static MCP_SERVE: CommandDef = CommandDef {
    name: "mcp-serve",
    about: "Run MCP stdio server for AI tool integration",
    usage: "legend mcp-serve [--cwd <path>]",
    flags: &[FlagDef {
        long: "--cwd",
        short: None,
        about: "Working directory",
        takes_value: true,
    }],
    positionals: &[],
    children: &[],
};

static MERGE: CommandDef = CommandDef {
    name: "git-merge-driver",
    about: "Auto-merge Legend state files during git conflicts",
    usage: "legend git-merge-driver %O %A %B %P",
    flags: &[],
    positionals: &[],
    children: &[],
};

static MERGE_LOCAL: CommandDef = CommandDef {
    name: "merge-local",
    about: "Merge a pre-merge memory.lz4 into current state",
    usage: "legend merge-local <path>",
    flags: &[],
    positionals: &[ArgDef {
        name: "PATH",
        about: "Path to pre-merge memory.lz4 file",
        required: true,
    }],
    children: &[],
};

static DASHBOARD: CommandDef = CommandDef {
    name: "dashboard",
    about: "Launch TUI dashboard (--3d for Bevy 3D view)",
    usage: "legend dashboard [--3d]",
    flags: &[FlagDef {
        long: "--3d",
        short: None,
        about: "Launch Bevy 3D view instead of TUI",
        takes_value: false,
    }],
    positionals: &[],
    children: &[],
};

static DEV: CommandDef = CommandDef {
    name: "dev",
    about: "Developer-only debug commands",
    usage: "legend dev <subcommand>",
    flags: &[],
    positionals: &[],
    children: &[],
};

// -- daemon subcommands (children for help only; dispatch is string-matched) --
static DAEMON_START: CommandDef = CommandDef {
    name: "start",
    about: "Run the daemon in foreground (Ctrl-C to exit)",
    usage: "legend daemon start [--detach]",
    flags: &[FlagDef {
        long: "--detach",
        short: None,
        about: "Internal — post-fork entry point used by auto-spawn",
        takes_value: false,
    }],
    positionals: &[],
    children: &[],
};
static DAEMON_STOP: CommandDef = CommandDef {
    name: "stop",
    about: "Request a clean shutdown of the running daemon",
    usage: "legend daemon stop",
    flags: &[],
    positionals: &[],
    children: &[],
};
static DAEMON_STATUS: CommandDef = CommandDef {
    name: "status",
    about: "Show daemon pid / uptime / protocol / request count",
    usage: "legend daemon status",
    flags: &[],
    positionals: &[],
    children: &[],
};
static DAEMON_CHECKPOINT: CommandDef = CommandDef {
    name: "checkpoint",
    about: "Force save + WAL truncate without shutting down",
    usage: "legend daemon checkpoint",
    flags: &[],
    positionals: &[],
    children: &[],
};
static DAEMON: CommandDef = CommandDef {
    name: "daemon",
    about: "Long-running daemon that caches ONNX model + state in RAM",
    usage: "legend daemon <subcommand>",
    flags: &[],
    positionals: &[],
    children: &[&DAEMON_START, &DAEMON_STOP, &DAEMON_STATUS, &DAEMON_CHECKPOINT],
};

// ---------------------------------------------------------------------------
// Top-level command registry
// ---------------------------------------------------------------------------

struct TopCommand {
    def: &'static CommandDef,
    usage_hint: &'static str,
    handler: fn(&[String], &CommandDef) -> Result<(), Box<dyn std::error::Error>>,
}

static COMMANDS: &[TopCommand] = &[
    TopCommand {
        def: &MEMORY,
        usage_hint: "memory [start|tick|query|...]",
        handler: commands::memory::handle_memory,
    },
    TopCommand {
        def: &DISCOVER,
        usage_hint: "discover [path]",
        handler: commands::discover::handle_discover,
    },
    TopCommand {
        def: &INIT,
        usage_hint: "init",
        handler: commands::init::handle_init,
    },
    TopCommand {
        def: &MCP_SERVE,
        usage_hint: "mcp-serve",
        handler: commands::mcp::handle_mcp_serve,
    },
    TopCommand {
        def: &MERGE,
        usage_hint: "git-merge-driver %O %A %B %P",
        handler: commands::merge::handle_git_merge_driver,
    },
    TopCommand {
        def: &MERGE_LOCAL,
        usage_hint: "merge-local <path>",
        handler: commands::merge::handle_merge_local,
    },
    TopCommand {
        def: &DASHBOARD,
        usage_hint: "dashboard [--3d]",
        handler: commands::dashboard::handle_dashboard_dispatch,
    },
    TopCommand {
        def: &DEV,
        usage_hint: "dev <subcommand>",
        handler: commands::dev::handle_dev,
    },
    TopCommand {
        def: &DAEMON,
        usage_hint: "daemon <subcommand>",
        handler: commands::daemon::handle_daemon,
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
            return (cmd.handler)(&args[2..], cmd.def);
        }
    }

    eprintln!("Unknown command: {}", args[1]);
    print_help();
    std::process::exit(1);
}

fn print_help() {
    let help_entries: Vec<(&CommandDef, &str)> = COMMANDS
        .iter()
        .filter(|c| {
            c.def.name != "dev" && c.def.name != "git-merge-driver" && c.def.name != "merge-local"
        })
        .map(|c| (c.def, c.usage_hint))
        .collect();

    cli::print_top_level_help(&help_entries);
    println!("  {:<30}{}", "help", "Show this help message");
}
