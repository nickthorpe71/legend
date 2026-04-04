use super::event_log::log_event;
use crate::memory::reset_memory;

pub(super) fn handle_reset() -> Result<(), Box<dyn std::error::Error>> {
    reset_memory()?;
    log_event("reset", "memory store cleared");
    println!("✓ Memory reset");
    Ok(())
}

pub(super) fn handle_context() -> Result<(), Box<dyn std::error::Error>> {
    let memory = crate::memory::load_or_default()?;
    let summary = crate::memory::build_context_summary(&memory);
    let json = serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
    Ok(())
}

pub(super) fn handle_dump() -> Result<(), Box<dyn std::error::Error>> {
    let memory = crate::memory::load_or_default()?;
    let dump = crate::memory::build_dump(&memory);
    let json = serde_json::to_string(&dump).unwrap_or_else(|_| "{}".to_string());
    println!("{}", json);
    Ok(())
}
