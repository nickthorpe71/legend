// Init command - creates .legend directory and initializes state
//
// R* principle: Working code first
// Layer 3: Create directory structure
// Layer 4: Add serialization (bincode + LZ4) ✓

use crate::storage;
use crate::types::LegendState;
use std::fs;
use std::path::Path;

/// Initialize a new Legend project
///
/// Creates `.legend/` directory and sets up initial state structure.
/// Safe to run multiple times - won't error if directory already exists.
pub fn handle_init() -> Result<(), Box<dyn std::error::Error>> {
    let legend_dir = Path::new(".legend");

    // Check if already initialized
    if storage::is_initialized() {
        println!("Legend already initialized in this directory");
        println!("  .legend/ directory exists");
        println!("  Use 'legend show' to view current state");
        return Ok(());
    }

    // Create .legend directory
    // R* principle: Add context to errors - tell user what failed
    fs::create_dir_all(legend_dir).map_err(|e| {
        format!("Failed to create .legend directory: {}", e)
    })?;

    // Create initial state
    // For now, we'll use a default project name
    // Later (Layer 6), we can accept --name flag or detect from git
    let project_name = "My Project".to_string();
    let state = LegendState::new(project_name);

    // Save the initial state to disk (bincode + LZ4)
    // This serializes and compresses the state
    storage::save_state(&state)?;

    println!("✓ Initialized Legend");
    println!("  Created .legend/ directory");
    println!("  Saved initial state to .legend/state.lz4");
    println!("  Ready to track features");

    Ok(())
}
