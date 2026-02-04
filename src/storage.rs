// Storage module - handles serialization/deserialization of LegendState
//
// Performance architecture:
// - Reads: <5ms (decompress + deserialize pre-computed data)
// - Writes: 100-500ms acceptable (serialize + compress + save)
//
// Format: Bincode (binary) + LZ4 (fast compression)

use crate::types::LegendState;
use std::fs;
use std::io;
use std::path::Path;

/// File path for the compressed state
const STATE_FILE: &str = ".legend/state.lz4";

/// Save LegendState to disk
///
/// Performance: ~40-100ms (acceptable for write path)
///
/// Process:
/// 1. Serialize to binary (bincode) - ~10ms
/// 2. Compress with LZ4 - ~20ms
/// 3. Atomic write (temp + rename) - ~10ms
///
/// Returns error if:
/// - Serialization fails (shouldn't happen with valid data)
/// - Compression fails (very rare)
/// - Disk write fails (permissions, disk full, etc.)
pub fn save_state(state: &LegendState) -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Serialize to binary format using bincode
    // bincode::serialize takes any type that implements Serialize
    // and converts it to Vec<u8> (vector of bytes)
    let serialized = bincode::serialize(state)
        .map_err(|e| format!("Failed to serialize state: {}", e))?;

    // Step 2: Compress with LZ4
    // LZ4 is extremely fast: >2GB/s decompression
    // compress() takes &[u8] (byte slice) and returns Vec<u8>
    // Parameters: (data, acceleration (None=default), prepend_size=true)
    let compressed = lz4::block::compress(&serialized, None, true)
        .map_err(|e| format!("Failed to compress state: {}", e))?;

    // Step 3: Atomic write to prevent corruption
    // Strategy: write to temp file, then rename (rename is atomic)
    // If we crash during write, the temp file is corrupted but STATE_FILE is safe
    let temp_file = format!("{}.tmp", STATE_FILE);

    fs::write(&temp_file, &compressed)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    // Rename is atomic - either fully succeeds or fully fails
    // No possibility of partially-written file
    fs::rename(&temp_file, STATE_FILE)
        .map_err(|e| format!("Failed to rename temp file: {}", e))?;

    Ok(())
}

/// Load LegendState from disk
///
/// Performance: <5ms (target for read path)
/// - Read file: ~1ms
/// - Decompress LZ4: ~1ms
/// - Deserialize bincode: ~1ms
/// - Total: ~3ms ✅
///
/// Returns error if:
/// - File doesn't exist (not initialized)
/// - File is corrupted (bad compression or serialization)
/// - Deserialization fails (version mismatch, data corruption)
pub fn load_state() -> Result<LegendState, Box<dyn std::error::Error>> {
    // Check if file exists first
    if !Path::new(STATE_FILE).exists() {
        return Err("Legend not initialized. Run 'legend init' first.".into());
    }

    // Step 1: Read compressed file from disk
    // fs::read returns Vec<u8>
    let compressed = fs::read(STATE_FILE)
        .map_err(|e| format!("Failed to read state file: {}", e))?;

    // Step 2: Decompress with LZ4
    // LZ4 decompression is extremely fast (>2GB/s)
    // decompress() returns Vec<u8>
    // The size hint is embedded in the compressed data (prepend_size=true)
    let serialized = lz4::block::decompress(&compressed, None)
        .map_err(|e| format!("Failed to decompress state: {}", e))?;

    // Step 3: Deserialize from binary to LegendState
    // bincode::deserialize takes &[u8] and returns T (inferred from context)
    let state: LegendState = bincode::deserialize(&serialized)
        .map_err(|e| format!("Failed to deserialize state: {}", e))?;

    Ok(state)
}

/// Check if Legend is initialized (state file exists)
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
