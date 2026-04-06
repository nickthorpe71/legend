// Persistence layer — save/load, LZ4+MessagePack, format migrations.
//
// All filesystem IO for the memory store lives here. The brain (memory/)
// never touches the filesystem; this module handles serialization,
// compression, and backward-compatible migration from older formats.

use std::fs;
use std::path::Path;

use crate::memory::{wernicke::KeywordCache, MemoryState};

pub const MEMORY_FILE: &str = ".legend/memory.lz4";

/// Magic bytes prepended to MessagePack payloads (after LZ4 decompression).
pub(crate) const MSGPACK_MAGIC: &[u8; 4] = b"LGND";
/// Format version for the MessagePack serialization.
pub(crate) const MSGPACK_FORMAT_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn load_or_default() -> Result<MemoryState, Box<dyn std::error::Error>> {
    if Path::new(MEMORY_FILE).exists() {
        match load_memory() {
            Ok(mut state) => {
                state.brain.keyword_cache = KeywordCache::from_graph(&state.brain.long_term);
                Ok(state)
            }
            Err(err) => {
                let backup = format!("{}.corrupt", MEMORY_FILE);
                // Only move to backup if one doesn't already exist, to avoid overwriting
                // potentially recoverable data from a previous crash.
                if !Path::new(&backup).exists() {
                    let _ = fs::rename(MEMORY_FILE, &backup);
                    eprintln!("Warning: failed to load memory store ({})", err);
                    eprintln!("Backup saved to {}", backup);
                } else {
                    eprintln!(
                        "Warning: failed to load memory store ({}), but a backup already exists.",
                        err
                    );
                    eprintln!(
                        "Starting with a fresh memory store to avoid corruption loop."
                    );
                    // Remove the unloadable memory file so next save writes a clean one
                    let _ = fs::remove_file(MEMORY_FILE);
                }
                Ok(MemoryState::default())
            }
        }
    } else {
        Ok(MemoryState::default())
    }
}

pub fn save(state: &MemoryState) -> Result<(), Box<dyn std::error::Error>> {
    save_memory_to_path(state, MEMORY_FILE)
}

pub fn reset_memory() -> Result<(), Box<dyn std::error::Error>> {
    if Path::new(MEMORY_FILE).exists() {
        fs::remove_file(MEMORY_FILE)
            .map_err(|e| format!("Failed to remove memory file: {}", e))?;
    }
    Ok(())
}

pub fn load_memory_from_path<P: AsRef<Path>>(
    path: P,
) -> Result<MemoryState, Box<dyn std::error::Error>> {
    let compressed =
        fs::read(path).map_err(|e| format!("Failed to read memory file: {}", e))?;
    let decompressed = lz4::block::decompress(&compressed, None)
        .map_err(|e| format!("Failed to decompress memory: {}", e))?;

    // MessagePack format: starts with LGND magic + version byte
    if decompressed.len() >= 5 && &decompressed[..4] == MSGPACK_MAGIC {
        let _version = decompressed[4];
        let state: MemoryState = rmp_serde::from_slice(&decompressed[5..])
            .map_err(|e| format!("Failed to deserialize msgpack memory: {}", e))?;
        return Ok(state);
    }

    Err("Unknown memory format (expected LGND header)".into())
}

pub fn save_memory_to_path<P: AsRef<Path>>(
    state: &MemoryState,
    path: P,
) -> Result<(), Box<dyn std::error::Error>> {
    let serialized =
        rmp_serde::to_vec_named(state).map_err(|e| format!("Failed to serialize memory: {}", e))?;

    // Prepend magic header: LGND + version byte
    let mut payload = Vec::with_capacity(5 + serialized.len());
    payload.extend_from_slice(MSGPACK_MAGIC);
    payload.push(MSGPACK_FORMAT_VERSION);
    payload.extend_from_slice(&serialized);

    let compressed = lz4::block::compress(&payload, None, true)
        .map_err(|e| format!("Failed to compress memory: {}", e))?;

    let path_ref = path.as_ref();
    let temp_file = format!("{}.tmp", path_ref.display());
    fs::write(&temp_file, &compressed)
        .map_err(|e| format!("Failed to write temp memory file: {}", e))?;
    fs::rename(&temp_file, path_ref)
        .map_err(|e| format!("Failed to write memory file: {}", e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn load_memory() -> Result<MemoryState, Box<dyn std::error::Error>> {
    load_memory_from_path(MEMORY_FILE)
}

