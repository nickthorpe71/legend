use crate::memory::extract::is_stopword;
use crate::memory::MemoryState;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Dev-only commands — not exposed in user/LLM docs or `legend help`
// ---------------------------------------------------------------------------

pub fn handle_dev(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    match args.first().map(|s| s.as_str()) {
        Some("prune-noise") => handle_prune_noise(),
        _ => {
            eprintln!("legend dev: unknown subcommand");
            eprintln!("Available: prune-noise");
            Ok(())
        }
    }
}

/// Surgically remove hook-telemetry noise from the memory store.
///
/// Targets entries that were created by the old SubagentStop / PostToolUse
/// passive ticks ("EXPERIENCE: Experience: Executed tool…"). These ticks
/// dominated the session log, L2, and L3 during early Legend development
/// before the hooks were fixed.
///
/// Removes:
///   - L2 entries whose text starts with "EXPERIENCE:"
///   - L3 nodes with labels from hook vocabulary OR the full stopword list
///   - L3 edges connected to removed nodes
///   - Session log entries starting with "EXPERIENCE:"
///
/// Then rebalances weights so real nodes rise back to their natural position.
fn handle_prune_noise() -> Result<(), Box<dyn std::error::Error>> {
    let mut memory = MemoryState::load_or_default()?;

    // --- L2: remove EXPERIENCE entries ---
    // Match both fresh entries ("EXPERIENCE: Experience: ...") and heavily-
    // reconsolidated ones ("| EXPERIENCE: Experience: ...") where the text was
    // summarized and may no longer start cleanly with "EXPERIENCE:".
    let l2_before = memory.short_term.len();
    memory
        .short_term
        .retain(|e| !e.text.contains("EXPERIENCE: Experience:"));
    let l2_removed = l2_before - memory.short_term.len();

    // --- L3 nodes: remove labels that are hook-telemetry artifacts OR generic
    // stopwords (both the old hardcoded list and the full is_stopword list).
    // "EXPERIENCE", "Executed", etc. are hook-structural; "tool", "success",
    // "status" etc. are now stopwords too.
    const HOOK_LABELS: &[&str] = &["EXPERIENCE", "Experience", "Executed", "Completed"];
    let noise_ids: HashSet<u64> = memory
        .long_term
        .nodes
        .iter()
        .filter(|(_, n)| {
            HOOK_LABELS.contains(&n.label.as_str()) || is_stopword(&n.label)
        })
        .map(|(id, _)| *id)
        .collect();

    let l3_before = memory.long_term.nodes.len();
    memory.long_term.nodes.retain(|id, _| !noise_ids.contains(id));
    memory.long_term.index.retain(|_, id| !noise_ids.contains(id));
    let l3_removed = l3_before - memory.long_term.nodes.len();

    // --- L3 edges: drop any edge touching a removed node ---
    let edges_before = memory.long_term.edges.len();
    memory
        .long_term
        .edges
        .retain(|e| !noise_ids.contains(&e.from) && !noise_ids.contains(&e.to));
    let edges_removed = edges_before - memory.long_term.edges.len();

    // --- Session log: drop EXPERIENCE entries so they stop polluting
    // Recent Activity even on stores that predate the display filter ---
    let log_before = memory.session_log.len();
    memory
        .session_log
        .retain(|e| !e.text.contains("EXPERIENCE: Experience:"));
    let log_removed = log_before - memory.session_log.len();

    // --- Rebalance so real nodes float back to their natural weights ---
    memory.rebalance_weights();

    memory.save()?;

    println!("✓ Noise pruned:");
    println!("  L2 entries removed  : {}", l2_removed);
    println!("  L3 nodes removed    : {}", l3_removed);
    println!("  L3 edges removed    : {}", edges_removed);
    println!("  Session log removed : {}", log_removed);

    Ok(())
}
