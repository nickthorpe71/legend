use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

pub(super) fn handle_consolidate() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::Consolidate)? {
        print!("{}", stdout);
        return Ok(());
    }

    let mut memory = crate::memory::load_or_default()?;
    let stdout = handlers::render_consolidate(&mut memory)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    crate::memory::save(&memory)?;
    print!("{}", stdout);
    Ok(())
}
