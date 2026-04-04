// Persistence layer — save/load, LZ4+MessagePack, format migrations.
//
// All filesystem IO for the memory store lives here. The brain (memory/)
// never touches the filesystem; this module handles serialization,
// compression, and backward-compatible migration from older formats.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;

use crate::memory::{
    wernicke::KeywordCache, BrainState, GraphMemory, MemoryConfig, MemoryRef, MemoryState,
    SessionEntry, ShortTermEntry,
};

pub const MEMORY_FILE: &str = ".legend/memory.lz4";

/// Magic bytes prepended to MessagePack payloads (after LZ4 decompression).
pub(crate) const MSGPACK_MAGIC: &[u8; 4] = b"LGND";
/// Format version for the MessagePack serialization.
pub(crate) const MSGPACK_FORMAT_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn load_or_default() -> Result<MemoryState, Box<dyn std::error::Error>> {
    // Try to migrate corrupt backup first (old format without new fields)
    if let Ok(Some(mut migrated)) = migrate_corrupt_backup() {
        migrated.brain.keyword_cache = KeywordCache::from_graph(&migrated.brain.long_term);
        return Ok(migrated);
    }

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

    // Legacy bincode fallback: try current format first
    if let Ok(state) = bincode::deserialize::<MemoryState>(&decompressed) {
        eprintln!("Migrating memory from bincode to msgpack format...");
        return Ok(state);
    }

    // Fall back to V5 (pre-working-memory, has immediate: VecDeque<String>)
    if let Ok(v5) = bincode::deserialize::<MemoryStateV5>(&decompressed) {
        eprintln!("Migrating memory from pre-working-memory bincode format...");
        return Ok(migrate_v5(v5));
    }

    // Fall back to V4 (pre-consolidated field, has gradient_sq_sum + density)
    if let Ok(v4) = bincode::deserialize::<MemoryStateV4>(&decompressed) {
        eprintln!("Migrating memory from v0.3.4 bincode format...");
        return Ok(migrate_v4(v4));
    }

    Err("Failed to deserialize memory: no known format matched".into())
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

// ---------------------------------------------------------------------------
// Migration types
// ---------------------------------------------------------------------------

/// ShortTermEntry before emotional_valence was added (pre-v0.3.10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ShortTermEntryV5 {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub summary: String,
    pub embedding: Vec<f32>,
    pub last_access: u64,
    pub usage: u32,
    #[serde(default)]
    pub salience: f32,
    #[serde(default)]
    pub reconsolidation_count: u32,
    #[serde(default)]
    pub labile_until: u64,
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
    #[serde(default)]
    pub gradient_sq_sum: f32,
    #[serde(default)]
    pub density: f32,
    #[serde(default)]
    pub consolidated: bool,
}

/// MemoryState before working_memory rework (had `immediate: VecDeque<String>`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct MemoryStateV5 {
    pub config: MemoryConfig,
    pub immediate: VecDeque<String>,
    pub short_term: Vec<ShortTermEntryV5>,
    pub long_term: GraphMemory,
    pub clock: u64,
    pub next_id: u64,
    #[serde(default)]
    pub session_log: Vec<SessionEntry>,
    #[serde(default)]
    pub current_task: Option<String>,
    #[serde(default)]
    pub ticks_since_consolidation: u32,
    #[serde(default)]
    pub last_retrieved_ids: Vec<u64>,
    #[serde(default)]
    pub last_synced_sha: Option<String>,
}

pub(crate) fn migrate_v5(v5: MemoryStateV5) -> MemoryState {
    MemoryState {
        brain: BrainState {
            config: v5.config,
            working_memory: Vec::new(),
            short_term: v5
                .short_term
                .into_iter()
                .map(|e| ShortTermEntry {
                    id: e.id,
                    text: e.text,
                    summary: e.summary,
                    embedding: e.embedding,
                    last_access: e.last_access,
                    usage: e.usage,
                    salience: e.salience,
                    reconsolidation_count: e.reconsolidation_count,
                    labile_until: e.labile_until,
                    refs: e.refs,
                    gradient_sq_sum: e.gradient_sq_sum,
                    density: e.density,
                    consolidated: e.consolidated,
                    emotional_valence: 0.0,
                    stability: 1.0,
                    last_retrieval_interval: 0,
                })
                .collect(),
            long_term: v5.long_term,
            clock: v5.clock,
            next_id: v5.next_id,
            ticks_since_consolidation: v5.ticks_since_consolidation,
            last_retrieved_ids: v5.last_retrieved_ids,
            recent_valence_sum: 0.0,
            last_tick_embedding: Vec::new(),
            term_frequency: HashMap::new(),
            keyword_cache: KeywordCache::default(),
        },
        session_log: v5.session_log,
        current_task: v5.current_task,
        last_synced_sha: v5.last_synced_sha,
    }
}

/// ShortTermEntry before `consolidated` was added (v0.3.4 format).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ShortTermEntryV4 {
    pub id: u64,
    pub text: String,
    #[serde(default)]
    pub summary: String,
    pub embedding: Vec<f32>,
    pub last_access: u64,
    pub usage: u32,
    #[serde(default)]
    pub salience: f32,
    #[serde(default)]
    pub reconsolidation_count: u32,
    #[serde(default)]
    pub labile_until: u64,
    #[serde(default)]
    pub refs: Vec<MemoryRef>,
    #[serde(default)]
    pub gradient_sq_sum: f32,
    #[serde(default)]
    pub density: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub(crate) struct MemoryStateV4 {
    pub config: MemoryConfig,
    pub immediate: VecDeque<String>,
    pub short_term: Vec<ShortTermEntryV4>,
    pub long_term: GraphMemory,
    pub clock: u64,
    pub next_id: u64,
    #[serde(default)]
    pub session_log: Vec<SessionEntry>,
    #[serde(default)]
    pub current_task: Option<String>,
    #[serde(default)]
    pub ticks_since_consolidation: u32,
    #[serde(default)]
    pub last_retrieved_ids: Vec<u64>,
    #[serde(default)]
    pub last_synced_sha: Option<String>,
}

pub(crate) fn migrate_v4(v4: MemoryStateV4) -> MemoryState {
    MemoryState {
        brain: BrainState {
            config: v4.config,
            working_memory: Vec::new(),
            short_term: v4
                .short_term
                .into_iter()
                .map(|e| ShortTermEntry {
                    id: e.id,
                    text: e.text,
                    summary: e.summary,
                    embedding: e.embedding,
                    last_access: e.last_access,
                    usage: e.usage,
                    salience: e.salience,
                    reconsolidation_count: e.reconsolidation_count,
                    labile_until: e.labile_until,
                    refs: e.refs,
                    gradient_sq_sum: e.gradient_sq_sum,
                    density: e.density,
                    consolidated: false,
                    emotional_valence: 0.0,
                    stability: 1.0,
                    last_retrieval_interval: 0,
                })
                .collect(),
            long_term: v4.long_term,
            clock: v4.clock,
            next_id: v4.next_id,
            ticks_since_consolidation: v4.ticks_since_consolidation,
            last_retrieved_ids: v4.last_retrieved_ids,
            recent_valence_sum: 0.0,
            last_tick_embedding: Vec::new(),
            term_frequency: HashMap::new(),
            keyword_cache: KeywordCache::default(),
        },
        session_log: v4.session_log,
        current_task: v4.current_task,
        last_synced_sha: v4.last_synced_sha,
    }
}

/// Attempt to migrate old memory format from .corrupt backup.
fn migrate_corrupt_backup() -> Result<Option<MemoryState>, Box<dyn std::error::Error>> {
    const CORRUPT_FILE: &str = ".legend/memory.lz4.corrupt";

    if !Path::new(CORRUPT_FILE).exists() {
        return Ok(None);
    }

    eprintln!("Detected old memory format backup, attempting migration...");

    #[derive(Debug, Clone, Deserialize)]
    struct MemoryRefV1 {
        pub path: String,
        pub start_line: usize,
        pub end_line: usize,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct ShortTermEntryV1 {
        pub id: u64,
        pub text: String,
        #[serde(default)]
        pub summary: String,
        pub embedding: Vec<f32>,
        pub last_access: u64,
        pub usage: u32,
        #[serde(default)]
        pub salience: f32,
        #[serde(default)]
        pub reconsolidation_count: u32,
        #[serde(default)]
        pub labile_until: u64,
        #[serde(default)]
        pub refs: Vec<MemoryRefV1>,
    }

    #[derive(Debug, Clone, Deserialize)]
    struct ShortTermEntryV2 {
        pub id: u64,
        pub text: String,
        #[serde(default)]
        pub summary: String,
        pub embedding: Vec<f32>,
        pub last_access: u64,
        pub usage: u32,
        #[serde(default)]
        pub salience: f32,
        #[serde(default)]
        pub reconsolidation_count: u32,
        #[serde(default)]
        pub labile_until: u64,
        #[serde(default)]
        pub refs: Vec<MemoryRef>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[allow(dead_code)]
    struct MemoryStateV3 {
        pub config: MemoryConfig,
        pub immediate: VecDeque<String>,
        pub short_term: Vec<ShortTermEntryV2>,
        pub long_term: GraphMemory,
        pub clock: u64,
        pub next_id: u64,
        #[serde(default)]
        pub session_log: Vec<SessionEntry>,
        #[serde(default)]
        pub current_task: Option<String>,
        #[serde(default)]
        pub ticks_since_consolidation: u32,
        #[serde(default)]
        pub last_retrieved_ids: Vec<u64>,
        #[serde(default)]
        pub last_synced_sha: Option<String>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[allow(dead_code)]
    struct MemoryStateV2 {
        pub config: MemoryConfig,
        pub immediate: VecDeque<String>,
        pub short_term: Vec<ShortTermEntryV1>,
        pub long_term: GraphMemory,
        pub clock: u64,
        pub next_id: u64,
        #[serde(default)]
        pub session_log: Vec<SessionEntry>,
        #[serde(default)]
        pub current_task: Option<String>,
        #[serde(default)]
        pub ticks_since_consolidation: u32,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[allow(dead_code)]
    struct MemoryStateV1 {
        pub config: MemoryConfig,
        pub immediate: VecDeque<String>,
        pub short_term: Vec<ShortTermEntryV1>,
        pub long_term: GraphMemory,
        pub clock: u64,
        pub next_id: u64,
        #[serde(default)]
        pub session_log: Vec<SessionEntry>,
    }

    let compressed = fs::read(CORRUPT_FILE)?;
    let serialized = match lz4::block::decompress(&compressed, None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "  Backup file is unrecoverable (decompress failed: {}). Archiving.",
                e
            );
            let archive = format!("{}.unrecoverable", CORRUPT_FILE);
            let _ = fs::rename(CORRUPT_FILE, &archive);
            return Ok(None);
        }
    };

    // Try msgpack first, then bincode V5, V4, V3, V2, V1.
    let new_state = if serialized.len() >= 5 && &serialized[..4] == MSGPACK_MAGIC {
        rmp_serde::from_slice::<MemoryState>(&serialized[5..])?
    } else if let Ok(v5) = bincode::deserialize::<MemoryStateV5>(&serialized) {
        migrate_v5(v5)
    } else if let Ok(v4) = bincode::deserialize::<MemoryStateV4>(&serialized) {
        migrate_v4(v4)
    } else if let Ok(v3) = bincode::deserialize::<MemoryStateV3>(&serialized) {
        MemoryState {
            brain: BrainState {
                config: v3.config,
                working_memory: Vec::new(),
                short_term: v3
                    .short_term
                    .into_iter()
                    .map(|e| ShortTermEntry {
                        id: e.id,
                        text: e.text,
                        summary: e.summary,
                        embedding: e.embedding,
                        last_access: e.last_access,
                        usage: e.usage,
                        salience: e.salience,
                        reconsolidation_count: e.reconsolidation_count,
                        labile_until: e.labile_until,
                        refs: e.refs,
                        gradient_sq_sum: 0.0,
                        density: 0.0,
                        consolidated: false,
                        emotional_valence: 0.0,
                        stability: 1.0,
                        last_retrieval_interval: 0,
                    })
                    .collect(),
                long_term: v3.long_term,
                clock: v3.clock,
                next_id: v3.next_id,
                ticks_since_consolidation: v3.ticks_since_consolidation,
                last_retrieved_ids: v3.last_retrieved_ids,
                recent_valence_sum: 0.0,
                last_tick_embedding: Vec::new(),
                term_frequency: HashMap::new(),
                keyword_cache: KeywordCache::default(),
            },
            session_log: v3.session_log,
            current_task: v3.current_task,
            last_synced_sha: v3.last_synced_sha,
        }
    } else if let Ok(v2) = bincode::deserialize::<MemoryStateV2>(&serialized) {
        MemoryState {
            brain: BrainState {
                config: v2.config,
                working_memory: Vec::new(),
                short_term: v2
                    .short_term
                    .into_iter()
                    .map(|e| ShortTermEntry {
                        id: e.id,
                        text: e.text,
                        summary: e.summary,
                        embedding: e.embedding,
                        last_access: e.last_access,
                        usage: e.usage,
                        salience: e.salience,
                        reconsolidation_count: e.reconsolidation_count,
                        labile_until: e.labile_until,
                        refs: e
                            .refs
                            .into_iter()
                            .map(|r| MemoryRef {
                                path: r.path,
                                start_line: r.start_line,
                                end_line: r.end_line,
                                snippet: String::new(),
                            })
                            .collect(),
                        gradient_sq_sum: 0.0,
                        density: 0.0,
                        consolidated: false,
                        emotional_valence: 0.0,
                        stability: 1.0,
                        last_retrieval_interval: 0,
                    })
                    .collect(),
                long_term: v2.long_term,
                clock: v2.clock,
                next_id: v2.next_id,
                ticks_since_consolidation: v2.ticks_since_consolidation,
                last_retrieved_ids: Vec::new(),
                recent_valence_sum: 0.0,
                last_tick_embedding: Vec::new(),
                term_frequency: HashMap::new(),
                keyword_cache: KeywordCache::default(),
            },
            session_log: v2.session_log,
            current_task: v2.current_task,
            last_synced_sha: None,
        }
    } else {
        match bincode::deserialize::<MemoryStateV1>(&serialized) {
            Ok(old) => MemoryState {
                brain: BrainState {
                    config: old.config,
                    working_memory: Vec::new(),
                    short_term: old
                        .short_term
                        .into_iter()
                        .map(|e| ShortTermEntry {
                            id: e.id,
                            text: e.text,
                            summary: e.summary,
                            embedding: e.embedding,
                            last_access: e.last_access,
                            usage: e.usage,
                            salience: e.salience,
                            reconsolidation_count: e.reconsolidation_count,
                            labile_until: e.labile_until,
                            refs: old_refs_to_current(e.refs),
                            gradient_sq_sum: 0.0,
                            density: 0.0,
                            consolidated: false,
                            emotional_valence: 0.0,
                            stability: 1.0,
                            last_retrieval_interval: 0,
                        })
                        .collect(),
                    long_term: old.long_term,
                    clock: old.clock,
                    next_id: old.next_id,
                    ticks_since_consolidation: 0,
                    last_retrieved_ids: Vec::new(),
                    recent_valence_sum: 0.0,
                    last_tick_embedding: Vec::new(),
                    term_frequency: HashMap::new(),
                    keyword_cache: KeywordCache::default(),
                },
                session_log: old.session_log,
                current_task: None,
                last_synced_sha: None,
            },
            Err(_) => {
                eprintln!("  Backup file is unrecoverable (no format matched). Archiving.");
                let archive = format!("{}.unrecoverable", CORRUPT_FILE);
                let _ = fs::rename(CORRUPT_FILE, &archive);
                return Ok(None);
            }
        }
    };

    fn old_refs_to_current(old: Vec<MemoryRefV1>) -> Vec<MemoryRef> {
        old.into_iter()
            .map(|r| MemoryRef {
                path: r.path,
                start_line: r.start_line,
                end_line: r.end_line,
                snippet: String::new(),
            })
            .collect()
    }

    // Save migrated state
    save(&new_state)?;

    // Remove corrupt backup after successful migration
    if let Err(e) = fs::remove_file(CORRUPT_FILE) {
        eprintln!(
            "  Warning: could not remove {} after migration: {}",
            CORRUPT_FILE, e
        );
    } else {
        eprintln!("  ✓ Cleaned up old format backup.");
    }

    eprintln!(
        "✓ Migration complete: {} short-term entries, {} graph nodes recovered",
        new_state.brain.short_term.len(),
        new_state.brain.long_term.nodes.len()
    );

    Ok(Some(new_state))
}
