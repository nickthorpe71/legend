//! `legend memory personality` — distill recurring style/preferences from
//! the L2/L3 store. Read-only; surfaces top preference / decision /
//! architecture / bug entries plus the heaviest L3 entities so a future
//! session can quickly load the user's persistent voice. See queue item #31.

use crate::commands::daemon::{client::try_over_ipc, handlers, ipc::Command};

pub(super) fn handle_personality(_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(stdout) = try_over_ipc(Command::Personality)? {
        print!("{}", stdout);
        return Ok(());
    }
    let state = crate::memory::load_or_default()?;
    let stdout = handlers::render_personality(&state)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    print!("{}", stdout);
    Ok(())
}
