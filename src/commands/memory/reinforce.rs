use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

pub(super) fn handle_reinforce(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 2 {
        return Err("Usage: legend memory reinforce <signal> <id1> [id2 ...]\n  signal: float from -1.0 (irrelevant) to 1.0 (very useful)".into());
    }

    let signal: f32 = args[0].parse().map_err(|_| {
        format!(
            "Invalid signal '{}': expected a float like 1.0 or -0.5",
            args[0]
        )
    })?;

    let ids: Result<Vec<u64>, _> = args[1..].iter().map(|s| s.parse()).collect();
    let ids = ids.map_err(|_| "Invalid entry ID: expected integer(s)")?;

    if let Some(stdout) = try_over_ipc(Command::Reinforce {
        signal,
        ids: ids.clone(),
    })? {
        print!("{}", stdout);
        return Ok(());
    }

    let mut memory = crate::memory::load_or_default()?;
    let stdout = handlers::render_reinforce(&mut memory, signal, &ids)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&memory)?;
    print!("{}", stdout);
    Ok(())
}
