use crate::cli::{parse_args, CommandDef};
use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

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

pub(super) fn handle_sessions(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_sessions_args(args, def);

    if let Some(stdout) = try_over_ipc(Command::Sessions {
        count: opts.count,
        all: opts.show_all,
    })? {
        print!("{}", stdout);
        return Ok(());
    }

    let memory = crate::memory::load_or_default()?;
    let stdout = handlers::render_sessions(&memory, opts.count, opts.show_all)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    print!("{}", stdout);
    Ok(())
}
