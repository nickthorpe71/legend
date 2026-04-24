//! Daemon server — accept-loop, per-connection handler, Phase 1 stub commands.
//!
//! Phase 1 only wires Ping / Status / Shutdown. Phase 2 extends the dispatch
//! match to cover Tick, Query, Start, Task, etc., and holds the `MemoryState`
//! behind an `RwLock`.

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
    /// ~1 s `load_or_default()` cost on daemon startup.
    pub(super) state: RwLock<Option<MemoryState>>,
}

impl Daemon {
    fn new(socket_path: String) -> Self {
        Self {
            started_at: Instant::now(),
            requests_handled: AtomicU64::new(0),
            shutdown: AtomicBool::new(false),
            socket_path,
            state: RwLock::new(None),
        }
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

/// Run `f` with exclusive access to `MemoryState`, lazy-loading from disk on
/// first use. Errors during load propagate to the client as `ErrorKind::Internal`.
pub(super) fn with_state_mut<R>(
    daemon: &Daemon,
    f: impl FnOnce(&mut MemoryState) -> R,
) -> Result<R, String> {
    let mut guard = daemon.state.write().map_err(|e| format!("state lock poisoned: {}", e))?;
    if guard.is_none() {
        *guard = Some(crate::memory::load_or_default().map_err(|e| e.to_string())?);
    }
    Ok(f(guard.as_mut().expect("just initialized")))
}

/// Same as [`with_state_mut`] but takes a shared lock — callers that only read
/// don't need to serialize with other readers. Lazy-init still happens under
/// an exclusive upgrade if required.
///
/// Unused in Phase 2 Commit A (only Tick is wired). Commit B uses this for
/// the read-only Query/Context/Dump/Stats/Sessions/TaskGet commands.
#[allow(dead_code)]
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
        *wguard = Some(crate::memory::load_or_default().map_err(|e| e.to_string())?);
    }
    Ok(f(wguard.as_ref().expect("just initialized")))
}

/// Serialize the daemon's in-RAM state to disk. Phase 2 calls this after every
/// mutation; Phase 3b replaces with WAL append + periodic checkpoint.
pub(super) fn persist(daemon: &Daemon) -> Result<(), String> {
    let guard = daemon.state.read().map_err(|e| format!("state lock poisoned: {}", e))?;
    if let Some(state) = guard.as_ref() {
        crate::memory::save(state).map_err(|e| e.to_string())?;
    }
    Ok(())
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
    let daemon = Arc::new(Daemon::new(path.clone()));

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
        Command::Tick { text, blocker } => command_output(
            id,
            with_state_mut(daemon, |state| handlers::render_tick(state, &text, blocker))
                .and_then(|r| r)
                .and_then(|out| persist(daemon).map(|()| out)),
        ),

        // Phase 2 Commit B will wire these — stubbed NotImplemented for now so
        // clients get a structured error and can fall back to in-process.
        Command::TaskSet { .. }
        | Command::TaskClear
        | Command::Reinforce { .. }
        | Command::Consolidate
        | Command::Reset
        | Command::Discover { .. }
        | Command::Init { .. }
        | Command::DevPruneNoise => not_implemented(id),

        // --- Read-only commands ---------------------------------------------
        Command::Start { .. }
        | Command::Query { .. }
        | Command::Context
        | Command::Dump
        | Command::Stats
        | Command::Sessions { .. }
        | Command::TaskGet => not_implemented(id),
    }
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

    /// Dispatch path is pure aside from the atomic counter; test it in isolation
    /// without binding a real socket.
    #[test]
    fn dispatch_ping_returns_pong() {
        let d = Arc::new(Daemon::new("/tmp/test.sock".into()));
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
        let d = Arc::new(Daemon::new("/tmp/test.sock".into()));
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
        let d = Arc::new(Daemon::new("/tmp/test.sock".into()));
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
        let d = Arc::new(Daemon::new("/tmp/test.sock".into()));
        let before = d.requests_handled.load(Ordering::Relaxed);
        dispatch(&d, Envelope::request(1, Command::Ping));
        dispatch(&d, Envelope::request(2, Command::Ping));
        let after = d.requests_handled.load(Ordering::Relaxed);
        assert_eq!(after - before, 2);
    }

    #[test]
    fn dispatch_shutdown_flips_flag() {
        let d = Arc::new(Daemon::new("/tmp/test.sock".into()));
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
