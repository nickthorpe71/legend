// Get State command - loads and outputs current state as JSON
//
// This is the critical READ PATH that must be <5ms
// Called by Claude at the start of every session
//
// Performance target: <5ms end-to-end
// - Load from disk: ~1ms
// - Decompress LZ4: ~1ms
// - Deserialize bincode: ~1ms
// - Serialize to JSON: ~1ms
// - Total: ~4ms ✅

use crate::storage;
use std::time::Instant;

/// Get current Legend state and output as JSON
///
/// This is the command Claude calls to load project context.
/// Must be extremely fast (<5ms) as it's called frequently.
///
/// Output: JSON to stdout (Claude parses this)
/// Timing info: Logged to stderr (won't interfere with JSON output)
pub fn handle_get_state() -> Result<(), Box<dyn std::error::Error>> {
    // Measure performance (critical path!)
    let start = Instant::now();

    // Load state from disk
    // This does: read file → decompress LZ4 → deserialize bincode
    let state = storage::load_state()?;

    let load_time = start.elapsed();

    // Convert to JSON
    // Use to_string_pretty for human-readable output
    // (Claude can parse either compact or pretty JSON)
    let json = serde_json::to_string_pretty(&state)
        .map_err(|e| format!("Failed to serialize state to JSON: {}", e))?;

    let total_time = start.elapsed();

    // Output JSON to stdout (this is what Claude reads)
    println!("{}", json);

    // Log performance to stderr (doesn't interfere with stdout)
    // This helps us verify we're meeting <5ms target
    eprintln!("⚡ Loaded state in {}ms (load: {}ms)",
        total_time.as_millis(),
        load_time.as_millis()
    );

    // Warn if we're getting close to the 5ms limit
    if total_time.as_millis() > 5 {
        eprintln!("⚠️  Warning: get_state took {}ms (target: <5ms)", total_time.as_millis());
    }

    Ok(())
}
