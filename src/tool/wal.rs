//! Write-ahead log for daemon-mode mutations.
//!
//! **Durability policy:** latency-first. Appends land in the kernel page cache
//! synchronously; a background thread fsyncs every `FSYNC_INTERVAL_MS` (or
//! sooner if `FSYNC_BUFFER_BYTES` of unflushed writes accumulate). Clean
//! shutdowns force a final fsync before the writer is dropped. A hard crash
//! (kernel panic, SIGKILL, power loss) can lose up to one fsync interval of
//! mutations. Full rationale in `docs/daemon-durability.md`.
//!
//! **Frame format** (on disk, repeating):
//! ```text
//! ┌──────────────────────┬────────────────────────┬──────────────────────┐
//! │ 4 bytes BE u32 len   │ `len` bytes MessagePack │ 8 bytes BE u64 hash  │
//! └──────────────────────┴────────────────────────┴──────────────────────┘
//! ```
//! `hash` is XXH3_64 of the payload bytes only (not the length prefix). On
//! mismatch or truncation the replayer cuts the file to the last good
//! boundary and continues.
//!
//! The WAL is a transient overlay on top of `memory.lz4`. Every checkpoint
//! (`persist_checkpoint`) writes a fresh snapshot and truncates the WAL to
//! zero, so WAL entries always represent "mutations since last snapshot."
//! When the daemon is not running and an in-process fallback takes a full
//! `save()`, that path also truncates the WAL so disk state stays coherent.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use xxhash_rust::xxh3::xxh3_64;

/// How long the background thread waits between fsync passes.
const FSYNC_INTERVAL_MS: u64 = 100;

/// Bytes of unflushed WAL after which we wake the background thread eagerly.
const FSYNC_BUFFER_BYTES: usize = 64 * 1024;

/// Hard cap on a single WAL entry. Defends against a corrupt length prefix
/// claiming a multi-GB frame. No legitimate entry exceeds this.
const MAX_ENTRY_BYTES: u32 = 16 * 1024 * 1024;

/// Default WAL location relative to `.legend/`. Matches `MEMORY_FILE` in
/// `src/tool/persistence.rs` so the two files stay co-located.
pub const WAL_FILE: &str = ".legend/memory.wal";

// ---------------------------------------------------------------------------
// WalEntry — one variant per mutating command
// ---------------------------------------------------------------------------

/// A single mutation captured in the WAL. Replay applies these against a
/// snapshot via the same handlers the live daemon path uses.
///
/// Each variant carries exactly the arguments needed to reproduce the
/// mutation deterministically. `Reset` is included so the WAL can record a
/// wipe (after which subsequent entries apply to an empty state).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WalEntry {
    Tick { text: String, blocker: bool },
    TaskSet { text: String },
    TaskClear,
    Reinforce { signal: f32, ids: Vec<u64> },
    Consolidate,
    Reset,
    /// Targeted plan-item status flip (item #14). Replay applies the same
    /// status change during crash recovery.
    PlanSetStatus {
        plan_name: String,
        item_number: u64,
        status: String,
    },
}

// ---------------------------------------------------------------------------
// Framing helpers
// ---------------------------------------------------------------------------

/// Encode a WalEntry into a complete frame (length prefix + payload + hash).
pub fn encode_frame(entry: &WalEntry) -> std::io::Result<Vec<u8>> {
    let payload = rmp_serde::to_vec_named(entry)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if payload.len() as u64 > MAX_ENTRY_BYTES as u64 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("wal entry too large: {} bytes", payload.len()),
        ));
    }
    let hash = xxh3_64(&payload);
    let mut frame = Vec::with_capacity(4 + payload.len() + 8);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    frame.extend_from_slice(&hash.to_be_bytes());
    Ok(frame)
}

/// Decode one frame starting at the current reader position. Returns:
/// - `Ok(Some(entry))` on a valid frame
/// - `Ok(None)` on clean EOF before any byte of the length prefix
/// - `Err(WalReadError::Truncated { good_until })` if the frame is corrupt
///   or truncated; `good_until` is the byte offset where the replayer should
///   call `set_len` to clip the bad tail.
pub fn decode_frame<R: Read + Seek>(reader: &mut R) -> Result<Option<WalEntry>, WalReadError> {
    let frame_start = reader.stream_position().map_err(WalReadError::Io)?;

    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(WalReadError::Io(e)),
    }
    let payload_len = u32::from_be_bytes(len_buf);
    if payload_len > MAX_ENTRY_BYTES {
        return Err(WalReadError::Truncated { good_until: frame_start });
    }

    let mut payload = vec![0u8; payload_len as usize];
    if reader.read_exact(&mut payload).is_err() {
        return Err(WalReadError::Truncated { good_until: frame_start });
    }

    let mut hash_buf = [0u8; 8];
    if reader.read_exact(&mut hash_buf).is_err() {
        return Err(WalReadError::Truncated { good_until: frame_start });
    }
    let expected = u64::from_be_bytes(hash_buf);
    let actual = xxh3_64(&payload);
    if expected != actual {
        return Err(WalReadError::Truncated { good_until: frame_start });
    }

    match rmp_serde::from_slice::<WalEntry>(&payload) {
        Ok(entry) => Ok(Some(entry)),
        Err(_) => Err(WalReadError::Truncated { good_until: frame_start }),
    }
}

#[derive(Debug)]
pub enum WalReadError {
    Io(std::io::Error),
    Truncated { good_until: u64 },
}

impl std::fmt::Display for WalReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalReadError::Io(e) => write!(f, "wal io: {}", e),
            WalReadError::Truncated { good_until } => {
                write!(f, "wal truncated at offset {}", good_until)
            }
        }
    }
}

impl std::error::Error for WalReadError {}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

/// Iterate every valid frame in the file at `path`, applying `apply` to each
/// decoded `WalEntry`. On corruption, truncates the file at the last good
/// boundary and logs via `stderr_log`. Returns the number of entries
/// successfully applied.
///
/// Safe to call when no WAL file exists — treats a missing file as zero
/// entries.
pub fn replay<P: AsRef<Path>>(
    path: P,
    mut apply: impl FnMut(WalEntry),
) -> std::io::Result<usize> {
    let path_ref = path.as_ref();
    if !path_ref.exists() {
        return Ok(0);
    }

    let mut file = OpenOptions::new().read(true).write(true).open(path_ref)?;
    let mut applied = 0usize;
    loop {
        match decode_frame(&mut file) {
            Ok(Some(entry)) => {
                apply(entry);
                applied += 1;
            }
            Ok(None) => break,
            Err(WalReadError::Io(e)) => return Err(e),
            Err(WalReadError::Truncated { good_until }) => {
                eprintln!(
                    "legend wal: corruption at offset {}; truncating",
                    good_until
                );
                file.set_len(good_until)?;
                break;
            }
        }
    }
    Ok(applied)
}

// ---------------------------------------------------------------------------
// WalWriter — the live append handle
// ---------------------------------------------------------------------------

/// Append-only WAL writer with a background fsync thread.
///
/// Construction: [`WalWriter::open`] opens/creates the file in append mode,
/// spawns the fsync thread, and returns. The thread is joined in `Drop`
/// after a final fsync.
pub struct WalWriter {
    inner: Arc<WalInner>,
    bg: Option<JoinHandle<()>>,
}

struct WalInner {
    path: PathBuf,
    file: Mutex<BufWriter<File>>,
    pending_bytes: AtomicUsize,
    state: Mutex<BgState>,
    cv: Condvar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BgState {
    Running,
    Shutdown,
}

impl WalWriter {
    /// Open or create the WAL file at `path` in append mode, and spawn the
    /// background fsync thread.
    pub fn open<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(false)
            .open(&path)?;
        let inner = Arc::new(WalInner {
            path,
            file: Mutex::new(BufWriter::new(file)),
            pending_bytes: AtomicUsize::new(0),
            state: Mutex::new(BgState::Running),
            cv: Condvar::new(),
        });

        let bg_inner = Arc::clone(&inner);
        let bg = std::thread::Builder::new()
            .name("legend-wal-fsync".into())
            .spawn(move || bg_loop(bg_inner))?;

        Ok(Self {
            inner,
            bg: Some(bg),
        })
    }

    /// Append a `WalEntry`. Writes land in the OS page cache synchronously;
    /// fsync happens on a timer (or when the unflushed buffer crosses
    /// `FSYNC_BUFFER_BYTES`).
    pub fn append(&self, entry: &WalEntry) -> std::io::Result<()> {
        let frame = encode_frame(entry)?;
        {
            let mut guard = self
                .inner
                .file
                .lock()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
            guard.write_all(&frame)?;
            guard.flush()?; // BufWriter → kernel page cache; no fsync.
        }
        let old = self
            .inner
            .pending_bytes
            .fetch_add(frame.len(), Ordering::AcqRel);
        if old + frame.len() >= FSYNC_BUFFER_BYTES {
            // Wake the background thread to sync now rather than wait for the timer.
            let _state = self.inner.state.lock().ok();
            self.inner.cv.notify_one();
        }
        Ok(())
    }

    /// Force an immediate fsync. Reserved for future SIGTERM / SIGINT
    /// handlers that want to guarantee durability before a forced exit;
    /// the normal shutdown path relies on `Drop` + the final `checkpoint`.
    #[allow(dead_code)]
    pub fn sync_now(&self) -> std::io::Result<()> {
        let guard = self
            .inner
            .file
            .lock()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        guard.get_ref().sync_data()?;
        self.inner.pending_bytes.store(0, Ordering::Release);
        Ok(())
    }

    /// Rewind the WAL to zero bytes. Called after a checkpoint writes a fresh
    /// snapshot — any entries in the WAL are already incorporated into the
    /// snapshot so the tail is redundant.
    pub fn truncate(&self) -> std::io::Result<()> {
        let mut guard = self
            .inner
            .file
            .lock()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        guard.flush()?;
        let file = guard.get_mut();
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        file.sync_data()?;
        self.inner.pending_bytes.store(0, Ordering::Release);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.inner.path
    }
}

fn bg_loop(inner: Arc<WalInner>) {
    loop {
        let action = {
            let mut state = match inner.state.lock() {
                Ok(g) => g,
                Err(_) => return, // poisoned — bail, main thread will crash or recover
            };
            // Sleep until timer OR threshold notification OR shutdown.
            let (new_state, _timed_out) = match inner
                .cv
                .wait_timeout(state, Duration::from_millis(FSYNC_INTERVAL_MS))
            {
                Ok(res) => res,
                Err(_) => return,
            };
            state = new_state;
            *state
        };

        // Do the sync outside the state lock so append() isn't contended on it.
        if inner.pending_bytes.load(Ordering::Acquire) > 0 {
            if let Ok(guard) = inner.file.lock() {
                let _ = guard.get_ref().sync_data();
                inner.pending_bytes.store(0, Ordering::Release);
            }
        }

        if matches!(action, BgState::Shutdown) {
            break;
        }
    }
}

impl Drop for WalWriter {
    fn drop(&mut self) {
        // Flag shutdown, wake bg thread, join.
        if let Ok(mut state) = self.inner.state.lock() {
            *state = BgState::Shutdown;
        }
        self.inner.cv.notify_all();
        if let Some(h) = self.bg.take() {
            let _ = h.join();
        }
        // Belt-and-suspenders final fsync in case the bg thread didn't get one in.
        if let Ok(guard) = self.inner.file.lock() {
            let _ = guard.get_ref().sync_data();
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip_tick() {
        let entry = WalEntry::Tick {
            text: "DECISION: test".into(),
            blocker: false,
        };
        let frame = encode_frame(&entry).unwrap();
        let mut cursor = Cursor::new(frame);
        let decoded = decode_frame(&mut cursor).unwrap().unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn frame_roundtrip_reinforce() {
        let entry = WalEntry::Reinforce {
            signal: 0.5,
            ids: vec![1, 2, 3],
        };
        let frame = encode_frame(&entry).unwrap();
        let decoded = decode_frame(&mut Cursor::new(frame)).unwrap().unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn multiple_frames_read_sequentially() {
        let a = WalEntry::TaskSet { text: "one".into() };
        let b = WalEntry::TaskClear;
        let c = WalEntry::Tick {
            text: "x".into(),
            blocker: true,
        };
        let mut buf = Vec::new();
        buf.extend(encode_frame(&a).unwrap());
        buf.extend(encode_frame(&b).unwrap());
        buf.extend(encode_frame(&c).unwrap());
        let mut cursor = Cursor::new(buf);
        assert_eq!(decode_frame(&mut cursor).unwrap().unwrap(), a);
        assert_eq!(decode_frame(&mut cursor).unwrap().unwrap(), b);
        assert_eq!(decode_frame(&mut cursor).unwrap().unwrap(), c);
        assert!(decode_frame(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn truncated_tail_detected_as_good_until() {
        let a = WalEntry::TaskSet { text: "ok".into() };
        let b = WalEntry::Tick {
            text: "partial".into(),
            blocker: false,
        };
        let mut buf = encode_frame(&a).unwrap();
        let good_until = buf.len() as u64;
        let mut b_bytes = encode_frame(&b).unwrap();
        // Simulate torn write by keeping only half of the second frame.
        b_bytes.truncate(b_bytes.len() / 2);
        buf.extend(b_bytes);

        let mut cursor = Cursor::new(buf);
        assert_eq!(decode_frame(&mut cursor).unwrap().unwrap(), a);
        match decode_frame(&mut cursor) {
            Err(WalReadError::Truncated { good_until: observed }) => {
                assert_eq!(observed, good_until);
            }
            other => panic!("expected Truncated, got {:?}", other),
        }
    }

    #[test]
    fn hash_mismatch_detected_as_truncated() {
        let entry = WalEntry::Consolidate;
        let mut frame = encode_frame(&entry).unwrap();
        // Corrupt one byte inside the payload (length stays valid, hash breaks).
        let payload_start = 4usize;
        frame[payload_start] ^= 0xFF;
        let mut cursor = Cursor::new(frame);
        match decode_frame(&mut cursor) {
            Err(WalReadError::Truncated { good_until: 0 }) => {}
            other => panic!("expected Truncated at 0, got {:?}", other),
        }
    }

    #[test]
    fn absurd_length_detected() {
        // Length prefix claims > MAX_ENTRY_BYTES, trailing bytes irrelevant.
        let mut frame = (MAX_ENTRY_BYTES + 1).to_be_bytes().to_vec();
        frame.extend(vec![0u8; 100]);
        let mut cursor = Cursor::new(frame);
        match decode_frame(&mut cursor) {
            Err(WalReadError::Truncated { good_until: 0 }) => {}
            other => panic!("expected Truncated at 0, got {:?}", other),
        }
    }

    #[test]
    fn writer_append_and_replay_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.wal");

        let entries = vec![
            WalEntry::TaskSet { text: "alpha".into() },
            WalEntry::Tick {
                text: "beta".into(),
                blocker: false,
            },
            WalEntry::Reinforce {
                signal: 1.0,
                ids: vec![42],
            },
            WalEntry::TaskClear,
        ];

        {
            let writer = WalWriter::open(&path).unwrap();
            for entry in &entries {
                writer.append(entry).unwrap();
            }
            writer.sync_now().unwrap();
            // Drop triggers final fsync + bg join.
        }

        let mut replayed = Vec::new();
        let count = replay(&path, |e| replayed.push(e)).unwrap();
        assert_eq!(count, entries.len());
        assert_eq!(replayed, entries);
    }

    #[test]
    fn replay_missing_file_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.wal");
        let count = replay(&path, |_| panic!("should not be called")).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn replay_truncates_corrupt_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corrupt.wal");

        // Build a file with two good frames + garbage tail.
        {
            let writer = WalWriter::open(&path).unwrap();
            writer
                .append(&WalEntry::TaskSet { text: "one".into() })
                .unwrap();
            writer.append(&WalEntry::TaskClear).unwrap();
            writer.sync_now().unwrap();
        }

        // Append garbage bytes (simulate a torn write).
        {
            let mut f = OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(&[0xFF; 32]).unwrap();
            f.sync_data().unwrap();
        }

        let len_before = std::fs::metadata(&path).unwrap().len();
        let mut replayed = Vec::new();
        let count = replay(&path, |e| replayed.push(e)).unwrap();
        assert_eq!(count, 2);
        assert_eq!(replayed.len(), 2);

        let len_after = std::fs::metadata(&path).unwrap().len();
        assert!(len_after < len_before, "wal should be truncated after corruption");
    }

    #[test]
    fn truncate_resets_file_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trunc.wal");

        let writer = WalWriter::open(&path).unwrap();
        writer
            .append(&WalEntry::TaskSet { text: "set".into() })
            .unwrap();
        writer.sync_now().unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 0);

        writer.truncate().unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);

        // After truncate, new appends work fine and start from zero.
        writer.append(&WalEntry::TaskClear).unwrap();
        writer.sync_now().unwrap();
        let mut replayed = Vec::new();
        replay(&path, |e| replayed.push(e)).unwrap();
        assert_eq!(replayed, vec![WalEntry::TaskClear]);
    }
}
