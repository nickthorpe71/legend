use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

pub(super) fn handle_reset() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::Reset)? {
        print!("{}", stdout);
        return Ok(());
    }
    // In-process fallback: no in-memory state to wipe since we're the only
    // process; just delete the file. render_reset also resets daemon state
    // in the IPC path, but here `MemoryState::default()` would be dropped
    // immediately anyway.
    crate::memory::reset_memory()?;
    crate::commands::memory::log_event_rich("reset", "memory store cleared", None);
    print!("✓ Memory reset\n");
    Ok(())
}

pub(super) fn handle_context() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::Context)? {
        print!("{}", stdout);
        return Ok(());
    }
    let state = crate::memory::load_or_default()?;
    let stdout = handlers::render_context(&state)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    print!("{}", stdout);
    Ok(())
}

pub(super) fn handle_dump() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::Dump)? {
        print!("{}", stdout);
        return Ok(());
    }
    let state = crate::memory::load_or_default()?;
    let stdout =
        handlers::render_dump(&state).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    print!("{}", stdout);
    Ok(())
}
