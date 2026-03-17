use crate::cli::{parse_args, CommandDef, FlagDef};
use crate::memory::MemoryState;

static SESSIONS_CMD: CommandDef = CommandDef {
    name: "sessions",
    about: "Show session log entries",
    usage: "legend memory sessions [--all] [count]",
    flags: &[
        FlagDef { long: "--all", short: Some('a'), about: "Include empty entries", takes_value: false },
    ],
    positionals: &[],
};

struct SessionsOptions {
    count: usize,
    show_all: bool,
}

fn parse_sessions_args(args: &[String]) -> SessionsOptions {
    let parsed = parse_args(args, &SESSIONS_CMD);

    let count = parsed
        .positional
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);

    SessionsOptions {
        count,
        show_all: parsed.has("all"),
    }
}

pub(super) fn handle_sessions(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_sessions_args(args);

    let memory = MemoryState::load_or_default()?;
    let recent = memory.recent_sessions(opts.count);

    if recent.is_empty() {
        println!("No session log entries yet.");
    } else {
        for entry in recent {
            if !opts.show_all && entry.text.trim().is_empty() {
                continue;
            }
            println!("[t={}] {}", entry.timestamp, entry.text);
        }
    }
    Ok(())
}
