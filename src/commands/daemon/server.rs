//! Daemon server — accept-loop, per-connection handler, Phase 2 command dispatch,
//! Phase 3b WAL-backed durability.
//!
//! State is lazy-loaded on first command that needs it. On lazy-init, any
//! WAL file is replayed onto the snapshot before returning — this is the
//! crash-recovery path. After replay we take a fresh checkpoint (save +
//! truncate WAL) so the reloaded state is the new durability anchor.
//!
//! Mutating commands append a [`WalEntry`] to the WAL instead of doing a
//! full save; the background fsync thread in `WalWriter` flushes those on
//! the schedule set by `docs/daemon-durability.md`. Consolidate and Reset
//! bypass the WAL entirely and take an immediate checkpoint (they're big
//! rewrites and already expensive; no reason to also carry them in the WAL).

use std::io::{BufReader, BufWriter};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Stream};
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;

use crate::memory::MemoryState;
use crate::tool::wal::{self, WalEntry, WalWriter, WAL_FILE};

use super::handlers;
use super::ipc::{
    read_frame, write_frame, Command, Envelope, Error, ErrorKind, Message, Payload, StatusInfo,
    PROTOCOL_VERSION,
};
use super::socket_path::{pid_path, socket_parent_dir, socket_path};

/// Shared daemon state across the accept loop and per-connection workers.
pub(super) struct Daemon {
    pub(super) started_at: Instant,
    pub(super) requests_handled: AtomicU64,
    pub(super) shutdown: AtomicBool,
    pub(super) socket_path: String,
    /// `None` until the first command that needs state. Lazy-loaded via
    /// `with_state_mut` / `with_state` so `Ping` and `Status` don't pay the
    /// ~1 s `load_or_default()` cost on daemon startup. On lazy init, the
    /// WAL is replayed on top of the loaded snapshot and a fresh checkpoint
    /// is written.
    pub(super) state: RwLock<Option<MemoryState>>,
    /// WAL append handle. Eagerly created when the daemon starts (cheap —
    /// opens the file + spawns the bg fsync thread). Drop on daemon
    /// shutdown flushes and joins.
    pub(super) wal: WalWriter,
}

impl Daemon {
    fn new(socket_path: String) -> std::io::Result<Self> {
        Self::new_with_wal(socket_path, std::path::Path::new(WAL_FILE))
    }

    /// Test-friendly constructor: lets the caller point the WAL writer at a
    /// tmpdir so parallel tests don't race on the default `.legend/memory.wal`.
    fn new_with_wal(socket_path: String, wal_path: &std::path::Path) -> std::io::Result<Self> {
        let wal = WalWriter::open(wal_path)?;
        Ok(Self {
            started_at: Instant::now(),
            requests_handled: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            socket_path,
            state: RwLock::new(None),
            wal,
        })
    }

    fn status(&self) -> StatusInfo {
        StatusInfo {
            pid: std::process::id(),
            uptime_secs: self.started_at.elapsed().as_secs(),
            protocol_version: PROTOCOL_VERSION,
            requests_handled: self.requests_handled.load(Ordering::Relaxed),
            socket_path: self.socket_path.clone(),
        }
    }
}

/// Replay WAL entries into `state`. Internal to the lazy-init path.
fn apply_wal_entry(state: &mut MemoryState, entry: WalEntry) {
    // Errors during replay are swallowed because the render functions
    // report them to clients via stdout strings; during replay nothing is
    // listening. We still want the state mutation side-effect to happen,
    // which it does even when render_* returns Err for the text portion.
    let _ = match entry {
        WalEntry::Tick { text, blocker } => handlers::render_tick(state, &text, blocker),
        WalEntry::TaskSet { text } => handlers::render_task_set(state, &text),
        WalEntry::TaskClear => handlers::render_task_clear(state),
        WalEntry::Reinforce { signal, ids } => handlers::render_reinforce(state, signal, &ids),
        WalEntry::Consolidate => handlers::render_consolidate(state),
        WalEntry::Reset => handlers::render_reset(state),
    };
}

/// Lazy state init: load snapshot, replay WAL, take a fresh checkpoint.
/// Caller must hold the write lock.
fn lazy_init_state(daemon: &Daemon) -> Result<MemoryState, String> {
    let mut state = crate::memory::load_or_default().map_err(|e| e.to_string())?;
    let replayed = wal::replay(WAL_FILE, |entry| apply_wal_entry(&mut state, entry))
        .map_err(|e| format!("wal replay: {}", e))?;
    if replayed > 0 {
        eprintln!(
            "legend daemon: replayed {} WAL {} onto snapshot",
            replayed,
            if replayed == 1 { "entry" } else { "entries" }
        );
        // Post-replay checkpoint: the state now reflects snapshot + WAL. Save
        // it as the new snapshot and truncate the WAL so we don't re-replay
        // on the next start.
        crate::memory::save(&state).map_err(|e| format!("post-replay save: {}", e))?;
        daemon
            .wal
            .truncate()
            .map_err(|e| format!("post-replay wal truncate: {}", e))?;
    }
    Ok(state)
}

/// Run `f` with exclusive access to `MemoryState`, lazy-loading from disk on
/// first use. Errors during load propagate to the client as `ErrorKind::Internal`.
pub(super) fn with_state_mut<R>(
    daemon: &Daemon,
    f: impl FnOnce(&mut MemoryState) -> R,
) -> Result<R, String> {
    let mut guard = daemon.state.write().map_err(|e| format!("state lock poisoned: {}", e))?;
    if guard.is_none() {
        *guard = Some(lazy_init_state(daemon)?);
    }
    Ok(f(guard.as_mut().expect("just initialized")))
}

/// Same as [`with_state_mut`] but takes a shared lock — callers that only read
/// don't need to serialize with other readers. Lazy-init still happens under
/// an exclusive upgrade if required.
pub(super) fn with_state<R>(
    daemon: &Daemon,
    f: impl FnOnce(&MemoryState) -> R,
) -> Result<R, String> {
    // Fast path: shared read if already initialized.
    {
        let guard = daemon.state.read().map_err(|e| format!("state lock poisoned: {}", e))?;
        if let Some(state) = guard.as_ref() {
            return Ok(f(state));
        }
    }
    // Slow path: upgrade to write lock for lazy init.
    let mut wguard = daemon.state.write().map_err(|e| format!("state lock poisoned: {}", e))?;
    if wguard.is_none() {
        *wguard = Some(lazy_init_state(daemon)?);
    }
    Ok(f(wguard.as_ref().expect("just initialized")))
}

/// Take an immediate checkpoint: full `save()` of the current in-RAM state
/// plus WAL truncate. Called after Consolidate and Reset (natural big
/// rewrites) and on daemon shutdown.
pub(super) fn checkpoint(daemon: &Daemon) -> Result<(), String> {
    let guard = daemon.state.read().map_err(|e| format!("state lock poisoned: {}", e))?;
    if let Some(state) = guard.as_ref() {
        crate::memory::save(state).map_err(|e| e.to_string())?;
    }
    daemon
        .wal
        .truncate()
        .map_err(|e| format!("wal truncate: {}", e))?;
    Ok(())
}

/// Append a mutation to the WAL. Failure here is surfaced as a log line
/// but does not fail the command — state is already updated in RAM, and
/// the next clean checkpoint will persist it. A WAL write failing
/// typically indicates disk full / permissions, conditions under which
/// the snapshot save during checkpoint would also fail; we'd rather the
/// user see that at checkpoint time with a clearer error.
fn wal_append(daemon: &Daemon, entry: &WalEntry) {
    if let Err(e) = daemon.wal.append(entry) {
        eprintln!("legend daemon: wal append failed: {}", e);
    }
}

/// Run the daemon in the foreground. Returns when `Shutdown` is requested or a
/// fatal error occurs. Used both by `legend daemon start` (interactive) and by
/// the post-detach child in `spawn_detached_daemon`.
pub fn run_foreground() -> std::io::Result<()> {
    // Ensure the socket's parent dir exists before binding.
    if let Some(parent) = socket_parent_dir() {
        std::fs::create_dir_all(&parent)?;
    }

    let path = socket_path();
    let listener = bind_listener(&path)?;
    let daemon = Arc::new(Daemon::new(path.clone())?);

    // Write PID file for CLI liveness checks.
    write_pid_file()?;

    // Best-effort log line for users watching the foreground run.
    eprintln!(
        "legend daemon: listening on {} (pid {})",
        path,
        std::process::id()
    );

    // The accept loop runs until shutdown flag flips. Each connection is handled
    // on a dedicated thread so a slow client can't block the rest.
    for incoming in listener.incoming() {
        if daemon.shutdown.load(Ordering::Acquire) {
            break;
        }
        match incoming {
            Ok(stream) => {
                let daemon = Arc::clone(&daemon);
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, daemon) {
                        eprintln!("legend daemon: connection error: {}", e);
                    }
                });
            }
            Err(e) => {
                // An individual accept failure shouldn't kill the daemon; log and continue.
                eprintln!("legend daemon: accept error: {}", e);
            }
        }
    }

    // Graceful shutdown: take a final checkpoint so on-disk state is current
    // and the WAL is empty for the next startup. Best-effort; any error is
    // logged but doesn't block exit.
    if let Err(e) = checkpoint(&daemon) {
        eprintln!("legend daemon: final checkpoint failed: {}", e);
    }

    cleanup(&path);
    Ok(())
}

/// Bind the local-socket listener on the given path. The `interprocess` crate
/// picks the right backend (Unix socket vs. named pipe) based on the name type.
fn bind_listener(path: &str) -> std::io::Result<interprocess::local_socket::Listener> {
    // On Unix, stale socket files from a crashed daemon block the bind. Remove
    // them first; the PID-file check in `handle_daemon_start` guards against
    // stepping on a live daemon.
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    let name = path.to_fs_name::<GenericFilePath>()?;
    #[cfg(windows)]
    let name = path.to_fs_name::<GenericNamespaced>()?;
    ListenerOptions::new().name(name).create_sync()
}

/// Per-connection handler — read frames in a loop, dispatch, write response.
fn handle_connection(stream: Stream, daemon: Arc<Daemon>) -> std::io::Result<()> {
    // Split the stream for buffered I/O; local-socket Streams are Read+Write.
    let (reader, writer) = (&stream, &stream);
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(writer);

    while let Some(envelope) = read_frame(&mut reader)? {
        let response = dispatch(&daemon, envelope);
        write_frame(&mut writer, &response)?;

        if daemon.shutdown.load(Ordering::Acquire) {
            break;
        }
    }
    Ok(())
}

/// Dispatch a single request to the correct Phase 1 handler. Unknown commands
/// are not rejected at this layer — they return `NotImplemented`, which matches
/// the Phase 2 surface still being stubbed.
fn dispatch(daemon: &Arc<Daemon>, envelope: Envelope) -> Envelope {
    daemon.requests_handled.fetch_add(1, Ordering::Relaxed);

    // Version mismatch is handled per-envelope so the client always gets a
    // structured error rather than a raw parse failure.
    if envelope.version != PROTOCOL_VERSION {
        return Envelope::err(
            envelope.id,
            Error {
                kind: ErrorKind::VersionMismatch {
                    client: envelope.version,
                    daemon: PROTOCOL_VERSION,
                },
                message: format!(
                    "client protocol v{}, daemon v{} — upgrade mismatch",
                    envelope.version, PROTOCOL_VERSION
                ),
            },
        );
    }

    let id = envelope.id;
    let cmd = match envelope.body {
        Message::Request(cmd) => cmd,
        Message::Response(_) => {
            return Envelope::err(
                id,
                Error {
                    kind: ErrorKind::Internal,
                    message: "client sent a Response envelope".into(),
                },
            );
        }
    };

    match cmd {
        // --- Daemon control -------------------------------------------------
        Command::Ping => Envelope::ok(id, Payload::Pong),
        Command::Status => Envelope::ok(id, Payload::Status(daemon.status())),
        Command::Shutdown { reason } => {
            // Flip the shutdown flag; the accept loop will exit on next iteration.
            daemon.shutdown.store(true, Ordering::Release);
            // Nudge the listener so incoming.next() wakes. Concretely, we connect
            // to ourselves — this unblocks the blocking accept.
            #[cfg(unix)]
            let name_result = daemon.socket_path.clone().to_fs_name::<GenericFilePath>();
            #[cfg(windows)]
            let name_result = daemon.socket_path.clone().to_fs_name::<GenericNamespaced>();
            if let Ok(name) = name_result {
                let _ = Stream::connect(name);
            }
            eprintln!("legend daemon: shutdown requested ({:?})", reason);
            Envelope::ok(id, Payload::Ack)
        }

        // --- Mutating commands ----------------------------------------------
        // WAL-backed: apply + append WAL (no full save on the hot path).
        Command::Tick { text, blocker } => {
            let entry = WalEntry::Tick {
                text: text.clone(),
                blocker,
            };
            mutating_wal(daemon, id, entry, |s| handlers::render_tick(s, &text, blocker))
        }
        Command::TaskSet { text } => {
            let entry = WalEntry::TaskSet { text: text.clone() };
            mutating_wal(daemon, id, entry, |s| handlers::render_task_set(s, &text))
        }
        Command::TaskClear => {
            mutating_wal(daemon, id, WalEntry::TaskClear, handlers::render_task_clear)
        }
        Command::Reinforce { signal, ids } => {
            let entry = WalEntry::Reinforce {
                signal,
                ids: ids.clone(),
            };
            mutating_wal(daemon, id, entry, |s| {
                handlers::render_reinforce(s, signal, &ids)
            })
        }

        // Checkpoint-backed: apply + full save + truncate WAL (natural big
        // rewrites where a fresh snapshot is cheaper than carrying the
        // mutation in the WAL).
        Command::Consolidate => {
            mutating_checkpoint(daemon, id, handlers::render_consolidate)
        }
        Command::Reset => mutating_checkpoint(daemon, id, handlers::render_reset),

        // Phase 2 Commit B+ will wire these (they need inner-helper extraction
        // from init.rs / discover.rs / dev.rs before they can be rendered from
        // a `&mut MemoryState`). Return NotImplemented → client falls back.
        Command::Discover { .. } | Command::Init { .. } | Command::DevPruneNoise => {
            not_implemented(id)
        }

        // --- Read-only commands ---------------------------------------------
        Command::Context => read_only(daemon, id, handlers::render_context),
        Command::Dump => read_only(daemon, id, handlers::render_dump),
        Command::Stats => read_only(daemon, id, handlers::render_stats),
        Command::Sessions { count, all } => read_only(daemon, id, move |s| {
            handlers::render_sessions(s, count, all)
        }),
        Command::TaskGet => read_only(daemon, id, handlers::render_task_get),

        // `Start` and `Query` stay in-process for now — deferred from Phase 2.
        Command::Start { .. } | Command::Query { .. } => not_implemented(id),
    }
}

/// Run a mutating handler + append `wal_entry` to the WAL (no full save).
/// The background fsync thread flushes on the schedule in `wal.rs`.
fn mutating_wal<F>(
    daemon: &Arc<Daemon>,
    id: u64,
    wal_entry: WalEntry,
    f: F,
) -> Envelope
where
    F: FnOnce(&mut MemoryState) -> Result<String, String>,
{
    let result = with_state_mut(daemon, f).and_then(|inner| inner);
    if result.is_ok() {
        wal_append(daemon, &wal_entry);
    }
    command_output(id, result)
}

/// Run a mutating handler + take a full checkpoint (save + truncate WAL).
/// Used for Consolidate and Reset — commands that rewrite most of state so
/// a fresh snapshot is cheaper than replaying the mutation from the WAL.
fn mutating_checkpoint<F>(daemon: &Arc<Daemon>, id: u64, f: F) -> Envelope
where
    F: FnOnce(&mut MemoryState) -> Result<String, String>,
{
    let result = with_state_mut(daemon, f)
        .and_then(|inner| inner)
        .and_then(|stdout| checkpoint(daemon).map(|()| stdout));
    command_output(id, result)
}

/// Run a read-only handler under a shared lock, no persist.
fn read_only<F>(daemon: &Arc<Daemon>, id: u64, f: F) -> Envelope
where
    F: FnOnce(&MemoryState) -> Result<String, String>,
{
    command_output(id, with_state(daemon, f).and_then(|inner| inner))
}

/// Turn a `Result<String, String>` into an `Envelope` — success becomes
/// `CommandOutput`, failure becomes `ErrorKind::Internal`.
fn command_output(id: u64, result: Result<String, String>) -> Envelope {
    match result {
        Ok(stdout) => Envelope::ok(id, Payload::CommandOutput { stdout }),
        Err(msg) => Envelope::err(
            id,
            Error {
                kind: ErrorKind::Internal,
                message: msg,
            },
        ),
    }
}

fn not_implemented(id: u64) -> Envelope {
    Envelope::err(
        id,
        Error {
            kind: ErrorKind::NotImplemented,
            message: "command not yet handled by daemon (Phase 2 in progress)".into(),
        },
    )
}

/// Write the daemon's PID to `pid_path()`. Best-effort — if the write fails,
/// log and continue; clients fall back to bare socket-connect when PID file
/// is missing.
fn write_pid_file() -> std::io::Result<()> {
    let path = pid_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, std::process::id().to_string())
}

fn cleanup(socket_path: &str) {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(socket_path);
    }
    #[cfg(windows)]
    {
        let _ = socket_path; // Named pipes auto-cleanup on process exit.
    }
    let _ = std::fs::remove_file(pid_path());
}

#[cfg(test)]
mod tests {
    use super::super::ipc::{
        Command, Envelope, Message, Payload, Response, ShutdownReason, PROTOCOL_VERSION,
    };
    use super::*;

    /// Build a Daemon under a tmpdir's WAL path so parallel tests don't race.
    /// Returns `(Arc<Daemon>, TempDir)` — keep the tempdir alive for the
    /// duration of the test.
    fn test_daemon() -> (Arc<Daemon>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tmpdir");
        let wal_path = dir.path().join("test.wal");
        let d = Daemon::new_with_wal("/tmp/test.sock".into(), &wal_path).expect("daemon");
        (Arc::new(d), dir)
    }

    /// Dispatch path is pure aside from the atomic counter; test it in isolation
    /// without binding a real socket.
    #[test]
    fn dispatch_ping_returns_pong() {
        let (d, _tmp) = test_daemon();
        let resp = dispatch(&d, Envelope::request(42, Command::Ping));
        assert_eq!(resp.id, 42);
        assert_eq!(resp.version, PROTOCOL_VERSION);
        assert!(matches!(
            resp.body,
            Message::Response(Response::Ok(Payload::Pong))
        ));
    }

    #[test]
    fn dispatch_status_returns_status() {
        let (d, _tmp) = test_daemon();
        let resp = dispatch(&d, Envelope::request(1, Command::Status));
        match resp.body {
            Message::Response(Response::Ok(Payload::Status(info))) => {
                assert_eq!(info.pid, std::process::id());
                assert_eq!(info.protocol_version, PROTOCOL_VERSION);
                assert_eq!(info.socket_path, "/tmp/test.sock");
            }
            other => panic!("expected Status payload, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_version_mismatch_returns_structured_error() {
        let (d, _tmp) = test_daemon();
        // Craft an envelope with a bogus version.
        let bad = Envelope {
            version: 9999,
            id: 7,
            body: Message::Request(Command::Ping),
        };
        let resp = dispatch(&d, bad);
        match resp.body {
            Message::Response(Response::Err(err)) => {
                assert!(matches!(
                    err.kind,
                    ErrorKind::VersionMismatch { client: 9999, daemon: _ }
                ));
            }
            other => panic!("expected VersionMismatch error, got {:?}", other),
        }
    }

    #[test]
    fn dispatch_increments_counter() {
        let (d, _tmp) = test_daemon();
        let before = d.requests_handled.load(Ordering::Relaxed);
        dispatch(&d, Envelope::request(1, Command::Ping));
        dispatch(&d, Envelope::request(2, Command::Ping));
        let after = d.requests_handled.load(Ordering::Relaxed);
        assert_eq!(after - before, 2);
    }

    #[test]
    fn dispatch_shutdown_flips_flag() {
        let (d, _tmp) = test_daemon();
        assert!(!d.shutdown.load(Ordering::Acquire));
        // Use a socket path that can't connect so the self-connect side-effect is
        // a no-op; the flag flip is what matters.
        dispatch(
            &d,
            Envelope::request(
                3,
                Command::Shutdown {
                    reason: ShutdownReason::Explicit,
                },
            ),
        );
        assert!(d.shutdown.load(Ordering::Acquire));
    }
}
