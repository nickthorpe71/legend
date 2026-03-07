use crate::types::LegendState;
use std::fs;
use std::path::Path;

const STATE_FILE: &str = ".legend/state.lz4";

/// Magic bytes prepended to MessagePack payloads (after LZ4 decompression).
const MSGPACK_MAGIC: &[u8; 4] = b"LGND";
/// Format version for the MessagePack serialization.
const MSGPACK_FORMAT_VERSION: u8 = 1;

/// Save LegendState to disk (msgpack → LZ4, atomic write via temp+rename).
pub fn save_state(state: &LegendState) -> Result<(), Box<dyn std::error::Error>> {
    save_state_to_path(state, STATE_FILE)
}

/// Load LegendState from disk (LZ4 → bincode).
pub fn load_state() -> Result<LegendState, Box<dyn std::error::Error>> {
    load_state_from_path(STATE_FILE)
}

/// Load LegendState from a specific path.
pub fn load_state_from_path<P: AsRef<Path>>(path: P) -> Result<LegendState, Box<dyn std::error::Error>> {
    if !path.as_ref().exists() {
        return Err(format!("State file not found at {:?}", path.as_ref()).into());
    }

    let compressed =
        fs::read(path).map_err(|e| format!("Failed to read state file: {}", e))?;

    let decompressed = lz4::block::decompress(&compressed, None)
        .map_err(|e| format!("Failed to decompress state: {}", e))?;

    // MessagePack format: starts with LGND magic + version byte
    if decompressed.len() >= 5 && &decompressed[..4] == MSGPACK_MAGIC {
        let state: LegendState = rmp_serde::from_slice(&decompressed[5..])
            .map_err(|e| format!("Failed to deserialize msgpack state: {}", e))?;
        return Ok(state);
    }

    // Legacy bincode fallback
    let state: LegendState = bincode::deserialize(&decompressed)
        .map_err(|e| format!("Failed to deserialize state: {}", e))?;

    Ok(state)
}

/// Save LegendState to a specific path.
pub fn save_state_to_path<P: AsRef<Path>>(state: &LegendState, path: P) -> Result<(), Box<dyn std::error::Error>> {
    let serialized =
        rmp_serde::to_vec_named(state).map_err(|e| format!("Failed to serialize state: {}", e))?;

    // Prepend magic header: LGND + version byte
    let mut payload = Vec::with_capacity(5 + serialized.len());
    payload.extend_from_slice(MSGPACK_MAGIC);
    payload.push(MSGPACK_FORMAT_VERSION);
    payload.extend_from_slice(&serialized);

    let compressed = lz4::block::compress(&payload, None, true)
        .map_err(|e| format!("Failed to compress state: {}", e))?;

    let path_ref = path.as_ref();
    let temp_file = format!("{}.tmp", path_ref.display());
    fs::write(&temp_file, &compressed).map_err(|e| format!("Failed to write state file: {}", e))?;
    fs::rename(&temp_file, path_ref).map_err(|e| format!("Failed to rename state file: {}", e))?;

    Ok(())
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
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn test_msgpack_state_roundtrip() {
        let dir = std::env::temp_dir().join("legend_test_state_mp");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("state.lz4");

        let mut state = LegendState::new("MsgPack Project".to_string());
        state.add_feature(Feature::new(
            "mp-feat".to_string(),
            "MP Feature".to_string(),
            "test".to_string(),
            "Testing msgpack state".to_string(),
        ));

        save_state_to_path(&state, &path).expect("save");

        // Verify magic header
        let compressed = fs::read(&path).unwrap();
        let decompressed = lz4::block::decompress(&compressed, None).unwrap();
        assert_eq!(&decompressed[..4], b"LGND");

        let loaded = load_state_from_path(&path).expect("load");
        assert_eq!(loaded.project_name, "MsgPack Project");
        assert_eq!(loaded.features.len(), 1);
        assert_eq!(loaded.features[0].id, "mp-feat");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_bincode_state_fallback() {
        let dir = std::env::temp_dir().join("legend_test_state_bc");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("state.lz4");

        // Write in old bincode format
        let state = LegendState::new("Bincode Project".to_string());
        let serialized = bincode::serialize(&state).unwrap();
        let compressed = lz4::block::compress(&serialized, None, true).unwrap();
        fs::write(&path, &compressed).unwrap();

        // Should load via bincode fallback
        let loaded = load_state_from_path(&path).expect("load bincode fallback");
        assert_eq!(loaded.project_name, "Bincode Project");

        let _ = fs::remove_dir_all(&dir);
    }
}
