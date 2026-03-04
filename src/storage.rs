use crate::types::LegendState;
use std::fs;
use std::path::Path;

const STATE_FILE: &str = ".legend/state.lz4";

/// Save LegendState to disk (bincode → LZ4, atomic write via temp+rename).
pub fn save_state(state: &LegendState) -> Result<(), Box<dyn std::error::Error>> {
    let serialized =
        bincode::serialize(state).map_err(|e| format!("Failed to serialize state: {}", e))?;

    let compressed = lz4::block::compress(&serialized, None, true)
        .map_err(|e| format!("Failed to compress state: {}", e))?;

    let temp_file = format!("{}.tmp", STATE_FILE);
    fs::write(&temp_file, &compressed).map_err(|e| format!("Failed to write temp file: {}", e))?;

    fs::rename(&temp_file, STATE_FILE).map_err(|e| format!("Failed to rename temp file: {}", e))?;

    Ok(())
}

/// Load LegendState from disk (LZ4 → bincode).
pub fn load_state() -> Result<LegendState, Box<dyn std::error::Error>> {
    if !Path::new(STATE_FILE).exists() {
        return Err("Legend not initialized. Run 'legend init' first.".into());
    }

    let compressed =
        fs::read(STATE_FILE).map_err(|e| format!("Failed to read state file: {}", e))?;

    let serialized = lz4::block::decompress(&compressed, None)
        .map_err(|e| format!("Failed to decompress state: {}", e))?;

    let state: LegendState = bincode::deserialize(&serialized)
        .map_err(|e| format!("Failed to deserialize state: {}", e))?;

    Ok(state)
}

/// Check if Legend is initialized (state file exists).
pub fn is_initialized() -> bool {
    Path::new(STATE_FILE).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Feature;

    #[test]
    fn test_save_load_roundtrip() {
        // Create a test state
        let mut state = LegendState::new("Test Project".to_string());

        let feature = Feature::new(
            "test-feature".to_string(),
            "Test Feature".to_string(),
            "testing".to_string(),
            "A test feature for serialization".to_string(),
        );

        state.add_feature(feature);

        // Save it
        save_state(&state).expect("Failed to save state");

        // Load it back
        let loaded = load_state().expect("Failed to load state");

        // Verify it matches
        assert_eq!(loaded.project_name, "Test Project");
        assert_eq!(loaded.features.len(), 1);
        assert_eq!(loaded.features[0].id, "test-feature");
        assert_eq!(loaded.features[0].domain, "testing");
    }

    #[test]
    fn test_load_nonexistent() {
        // Try to load when file doesn't exist
        // First, remove the file if it exists
        let _ = fs::remove_file(STATE_FILE);

        let result = load_state();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not initialized"));
    }
}
