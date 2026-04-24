use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

pub(super) fn handle_task(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let subcommand = args.first().map(|s| s.as_str()).unwrap_or("show");

    match subcommand {
        "show" | "" => do_get(),
        "set" => {
            if args.len() < 2 {
                return Err("Usage: legend memory task set <task description>".into());
            }
            let task = args[1..].join(" ");
            do_set(&task)
        }
        "clear" => do_clear(),
        _ => {
            // Legacy affordance: `legend memory task <text>` (no `set` keyword)
            // treats the whole tail as the task description.
            let task = args.join(" ");
            do_set(&task)
        }
    }
}

fn do_get() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::TaskGet)? {
        print!("{}", stdout);
        return Ok(());
    }
    let state = crate::memory::load_or_default()?;
    let stdout = handlers::render_task_get(&state).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    print!("{}", stdout);
    Ok(())
}

fn do_set(task: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::TaskSet { text: task.to_string() })? {
        print!("{}", stdout);
        return Ok(());
    }
    let mut state = crate::memory::load_or_default()?;
    let stdout = handlers::render_task_set(&mut state, task)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&state)?;
    print!("{}", stdout);
    Ok(())
}

fn do_clear() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::TaskClear)? {
        print!("{}", stdout);
        return Ok(());
    }
    let mut state = crate::memory::load_or_default()?;
    let stdout = handlers::render_task_clear(&mut state)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&state)?;
    print!("{}", stdout);
    Ok(())
}
