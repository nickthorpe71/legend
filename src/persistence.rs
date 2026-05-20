//! On-disk snapshot of the live `Hypergraph` so memory survives
//! process restarts.
//!
//! The wire format is a small fixed header followed by an
//! LZ4-compressed rmp-serde payload. The derived index fields on
//! `Hypergraph` are `#[serde(skip)]` — they regenerate on load via
//! `seed::rebuild_indices`.
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │ 0..8   "LEGEND01"     8-byte ASCII magic                     │
//! │ 8..12  u32 LE         format version (currently 1)           │
//! │ 12..20 [u8; 8]        FNV-1a64 of the embedded seed bytes —  │
//! │                       refused on mismatch to avoid loading   │
//! │                       a snapshot whose anchor IDs would now  │
//! │                       point at the wrong elements.           │
//! │ 20..   LZ4 payload    size-prepended LZ4 of rmp-serde(hg)    │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! Writes are atomic: encode → temp file in the destination dir →
//! fsync → rename. A crashed process can leave `memory.lz4.tmp.<pid>`
//! files; they are safe to delete.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process;

use crate::seed::{load_seed_graph, rebuild_indices};
use crate::types::Hypergraph;

/// Embedded seed binary. Same bytes `src/seed.rs` deserializes; we
/// re-include here so the fingerprint computation doesn't need a
/// public accessor on the `seed` module.
const SEED_GRAPH: &[u8] = include_bytes!("seed/graph.bin");

const MAGIC: &[u8; 8] = b"LEGEND01";
const FORMAT_VERSION: u32 = 1;
const HEADER_LEN: usize = 20; // 8 magic + 4 version + 8 fingerprint

// ─── Errors ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum PersistError {
    /// Filesystem / IO failure on read or write.
    Io { path: PathBuf, source: io::Error },
    /// File is shorter than the fixed header.
    Truncated {
        path: PathBuf,
        expected_at_least: usize,
        got: usize,
    },
    /// Header magic doesn't match — file isn't a Legend snapshot.
    BadMagic { path: PathBuf, found: [u8; 8] },
    /// Header carries a format version this binary doesn't recognize.
    /// v0 has no migrations; the user is expected to delete the file
    /// or downgrade.
    VersionMismatch {
        path: PathBuf,
        found: u32,
        expected: u32,
    },
    /// The persisted snapshot was written against a different seed
    /// binary. Anchor IDs may have shifted; loading would point at
    /// the wrong elements. Caller should regenerate the seed or
    /// delete the snapshot.
    SeedDrift { path: PathBuf },
    /// LZ4 decompression failed (truncated payload, wrong format, etc.).
    Decompress {
        path: PathBuf,
        source: lz4_flex::block::DecompressError,
    },
    /// rmp-serde failed to decode the payload — typically because the
    /// snapshot was written by an older or newer schema.
    Decode {
        path: PathBuf,
        source: rmp_serde::decode::Error,
    },
    /// rmp-serde failed to encode the live Hypergraph. Shouldn't
    /// happen for well-formed graphs; included for completeness.
    Encode(rmp_serde::encode::Error),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PersistError::Io { path, source } => write!(
                f,
                "persistence I/O at {}: {source}. Check directory permissions or set LEGEND_STATE_DIR.",
                path.display()
            ),
            PersistError::Truncated {
                path,
                expected_at_least,
                got,
            } => write!(
                f,
                "{} is too short to be a Legend snapshot ({got} bytes, expected ≥{expected_at_least}). Delete it to start fresh.",
                path.display()
            ),
            PersistError::BadMagic { path, found } => write!(
                f,
                "{} is not a Legend snapshot (magic {found:?} ≠ {MAGIC:?}). Delete it to start fresh.",
                path.display()
            ),
            PersistError::VersionMismatch {
                path,
                found,
                expected,
            } => write!(
                f,
                "{} is format v{found}, this binary expects v{expected}. v0 has no migration — delete the file to start fresh.",
                path.display()
            ),
            PersistError::SeedDrift { path } => write!(
                f,
                "{} was written against a different seed binary. Anchor IDs may have shifted; delete the snapshot or regenerate the seed.",
                path.display()
            ),
            PersistError::Decompress { path, source } => write!(
                f,
                "decompressing {} failed: {source}. The file may be corrupted; delete it to start fresh.",
                path.display()
            ),
            PersistError::Decode { path, source } => write!(
                f,
                "decoding {} failed: {source}. The schema may have changed; delete the snapshot to start fresh.",
                path.display()
            ),
            PersistError::Encode(source) => write!(f, "encoding snapshot failed: {source}"),
        }
    }
}

impl std::error::Error for PersistError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PersistError::Io { source, .. } => Some(source),
            PersistError::Decompress { source, .. } => Some(source),
            PersistError::Decode { source, .. } => Some(source),
            PersistError::Encode(source) => Some(source),
            _ => None,
        }
    }
}

// ─── Path / fingerprint helpers ──────────────────────────────────────

/// Resolve the snapshot path. `LEGEND_STATE_DIR` env var if set, else
/// `./.legend/`. Filename is always `memory.lz4`.
pub fn default_path() -> PathBuf {
    let dir = std::env::var_os("LEGEND_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".legend"));
    dir.join("memory.lz4")
}

/// 8-byte fingerprint of the embedded seed binary. Computed with
/// FNV-1a (64-bit) so we don't drag in a hash crate. Deterministic;
/// only changes when `src/seed/graph.bin` itself changes.
pub fn seed_fingerprint() -> [u8; 8] {
    fnv1a_64(SEED_GRAPH).to_le_bytes()
}

fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ─── Save ────────────────────────────────────────────────────────────

/// Encode `hg` and atomically write the result to `path`. The 12
/// `#[serde(skip)]` index fields on `Hypergraph` are omitted by serde
/// itself — they regenerate on load.
pub fn save(hg: &Hypergraph, path: &Path) -> Result<(), PersistError> {
    let encoded = rmp_serde::to_vec(hg).map_err(PersistError::Encode)?;
    let compressed = lz4_flex::block::compress_prepend_size(&encoded);

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| PersistError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let tmp_path = atomic_tmp_path(path);
    // Scope the file handle so its drop runs before the rename.
    {
        let mut f = fs::File::create(&tmp_path).map_err(|source| PersistError::Io {
            path: tmp_path.clone(),
            source,
        })?;
        f.write_all(MAGIC).map_err(|source| PersistError::Io {
            path: tmp_path.clone(),
            source,
        })?;
        f.write_all(&FORMAT_VERSION.to_le_bytes())
            .map_err(|source| PersistError::Io {
                path: tmp_path.clone(),
                source,
            })?;
        f.write_all(&seed_fingerprint())
            .map_err(|source| PersistError::Io {
                path: tmp_path.clone(),
                source,
            })?;
        f.write_all(&compressed)
            .map_err(|source| PersistError::Io {
                path: tmp_path.clone(),
                source,
            })?;
        f.sync_all().map_err(|source| PersistError::Io {
            path: tmp_path.clone(),
            source,
        })?;
    }
    fs::rename(&tmp_path, path).map_err(|source| PersistError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

fn atomic_tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp.{}", process::id()));
    path.with_file_name(name)
}

// ─── Load ────────────────────────────────────────────────────────────

/// Read `path`, verify the header, decompress and decode, then
/// rebuild the derived indices. Returns a fully-usable `Hypergraph`.
pub fn load(path: &Path) -> Result<Hypergraph, PersistError> {
    let mut f = fs::File::open(path).map_err(|source| PersistError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes)
        .map_err(|source| PersistError::Io {
            path: path.to_path_buf(),
            source,
        })?;

    if bytes.len() < HEADER_LEN {
        return Err(PersistError::Truncated {
            path: path.to_path_buf(),
            expected_at_least: HEADER_LEN,
            got: bytes.len(),
        });
    }

    // Magic.
    let mut found_magic = [0u8; 8];
    found_magic.copy_from_slice(&bytes[0..8]);
    if &found_magic != MAGIC {
        return Err(PersistError::BadMagic {
            path: path.to_path_buf(),
            found: found_magic,
        });
    }

    // Version.
    let mut version_bytes = [0u8; 4];
    version_bytes.copy_from_slice(&bytes[8..12]);
    let found_version = u32::from_le_bytes(version_bytes);
    if found_version != FORMAT_VERSION {
        return Err(PersistError::VersionMismatch {
            path: path.to_path_buf(),
            found: found_version,
            expected: FORMAT_VERSION,
        });
    }

    // Seed fingerprint.
    let mut fp = [0u8; 8];
    fp.copy_from_slice(&bytes[12..20]);
    if fp != seed_fingerprint() {
        return Err(PersistError::SeedDrift {
            path: path.to_path_buf(),
        });
    }

    // Decompress + decode.
    let payload = &bytes[HEADER_LEN..];
    let decompressed = lz4_flex::block::decompress_size_prepended(payload).map_err(|source| {
        PersistError::Decompress {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let mut hg: Hypergraph =
        rmp_serde::from_slice(&decompressed).map_err(|source| PersistError::Decode {
            path: path.to_path_buf(),
            source,
        })?;

    // Skipped index fields land as `Default::default()`. Repopulate.
    rebuild_indices(&mut hg);
    Ok(hg)
}

/// If `path` exists, `load` from it. Otherwise build a fresh
/// Hypergraph via `load_seed_graph`, persist it to `path`, and
/// return — so the snapshot file always exists after the first call.
pub fn load_or_seed(path: &Path) -> Result<Hypergraph, PersistError> {
    if path.exists() {
        load(path)
    } else {
        let hg = load_seed_graph();
        save(&hg, path)?;
        Ok(hg)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-test temp path under the OS temp dir. Tests clean up
    /// after themselves; the pid suffix keeps parallel test runs
    /// from colliding.
    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("legend_persistence_{name}_{}.lz4", process::id()));
        p
    }

    #[test]
    fn round_trip_seed_graph() {
        let hg = load_seed_graph();
        let elements_before = hg.elements.len();
        let relations_before = hg.relations.len();
        let clock_before = hg.clock;
        let by_name_before = hg.by_name.len();

        let path = temp_path("round_trip");
        save(&hg, &path).expect("save");
        let hg2 = load(&path).expect("load");

        assert_eq!(hg2.elements.len(), elements_before);
        assert_eq!(hg2.relations.len(), relations_before);
        assert_eq!(hg2.clock, clock_before);
        // Indices were rebuilt — by_name should be populated, not empty.
        assert_eq!(hg2.by_name.len(), by_name_before);
        assert!(!hg2.region_children.is_empty(), "indices regenerated");
        // Anchor IDs survived.
        assert_eq!(hg2.genesis, hg.genesis);
        assert_eq!(hg2.subject_attr, hg.subject_attr);
        assert_eq!(hg2.target_attr, hg.target_attr);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn magic_mismatch_returns_bad_magic() {
        let path = temp_path("bad_magic");
        let mut garbage = vec![0u8; 64];
        garbage[0..8].copy_from_slice(b"NOTLEGND");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &garbage).unwrap();

        match load(&path) {
            Err(PersistError::BadMagic { .. }) => {}
            other => panic!("expected BadMagic, got {other:?}"),
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn version_mismatch_returns_version_error() {
        let path = temp_path("bad_version");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&99u32.to_le_bytes());
        bytes.extend_from_slice(&seed_fingerprint());
        bytes.extend_from_slice(&[0u8; 32]); // filler payload
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();

        match load(&path) {
            Err(PersistError::VersionMismatch {
                found: 99,
                expected: FORMAT_VERSION,
                ..
            }) => {}
            other => panic!("expected VersionMismatch, got {other:?}"),
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn seed_drift_returns_seed_drift() {
        let path = temp_path("seed_drift");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&[0xFFu8; 8]); // tampered fingerprint
        bytes.extend_from_slice(&[0u8; 32]);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();

        match load(&path) {
            Err(PersistError::SeedDrift { .. }) => {}
            other => panic!("expected SeedDrift, got {other:?}"),
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn truncated_returns_truncated() {
        let path = temp_path("truncated");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"LEGEND").unwrap(); // 6 bytes < HEADER_LEN

        match load(&path) {
            Err(PersistError::Truncated { got: 6, .. }) => {}
            other => panic!("expected Truncated, got {other:?}"),
        }
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn load_or_seed_first_run_creates_file() {
        let path = temp_path("first_run");
        let _ = fs::remove_file(&path);
        assert!(!path.exists());

        let hg = load_or_seed(&path).expect("load_or_seed");
        assert!(path.exists(), "snapshot file should exist after first run");
        assert!(!hg.elements.is_empty(), "seed graph should be non-empty");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn seed_fingerprint_is_stable() {
        // Same input → same hash on every call.
        assert_eq!(seed_fingerprint(), seed_fingerprint());
    }
}
