use super::event_log::*;
use super::helpers::truncate_text;
use crate::memory::MemoryState;

pub(super) fn handle_consolidate() -> Result<(), Box<dyn std::error::Error>> {
    let mut memory = MemoryState::load_or_default()?;
    let summaries = memory.consolidate();
    memory.save()?;

    let event_data = EventData::Consolidate(ConsolidateEventData {
        groups_merged: summaries.len(),
        summaries: summaries
            .iter()
            .map(|s| ConsolidatedGroup {
                node_id: s.id,
                label: truncate_text(&s.label, 60),
            })
            .collect(),
    });
    log_event_rich(
        "consolidate",
        &format!("{} groups merged", summaries.len()),
        Some(event_data),
    );
    let json = serde_json::to_string(&summaries).unwrap_or_else(|_| "[]".to_string());
    println!("{}", json);
    Ok(())
}
