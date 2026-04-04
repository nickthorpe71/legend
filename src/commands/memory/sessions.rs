use crate::cli::{parse_args, CommandDef};

struct SessionsOptions {
    count: usize,
    show_all: bool,
}

fn parse_sessions_args(args: &[String], def: &CommandDef) -> SessionsOptions {
    let parsed = parse_args(args, def);

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

pub(super) fn handle_sessions(args: &[String], def: &CommandDef) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_sessions_args(args, def);

    let memory = crate::memory::load_or_default()?;
    let recent = crate::memory::recent_sessions(&memory, opts.count);

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
