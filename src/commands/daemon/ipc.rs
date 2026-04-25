//! Daemon IPC protocol — MessagePack frames over a local socket.
//!
//! Wire format: 4-byte big-endian length prefix, followed by a MessagePack-encoded
//! `Envelope`. Each `Envelope` carries a protocol version + request/response id +
//! typed body. The version field lets daemon and client detect skew after an
//! upgrade and exit cleanly rather than desync silently.
//!
//! Phase 1 scope: envelope + codec + the minimal command surface needed to boot
//! the daemon (Ping, Status, Shutdown). Mutating commands arrive in Phase 2.
//!
//! Keep this module free of socket I/O — the codec is pure (read_frame / write_frame
//! take any `Read`/`Write`). That way we can round-trip over `Vec<u8>` in unit tests
//! without spawning a daemon.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::memory::{MemoryContext, TickResult};

/// Wire protocol version. Bump on any `Command` / `Payload` / `Envelope` shape change.
pub const PROTOCOL_VERSION: u16 = 1;

/// Upper bound on a single framed message (defends against hostile or corrupt peers).
/// 64 MB is deliberately generous — `Dump` can be large; anything bigger is pathological.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Top-level IPC envelope. Carries version, correlation id, and typed body.
///
/// `PartialEq` is intentionally NOT derived — `Payload` contains
/// `TickResult`/`MemoryContext` whose full field trees include `f32`s and
/// other non-Eq types. Tests that need to assert envelope equality compare
/// their debug representations instead (see ipc::tests).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u16,
    pub id: u64,
    pub body: Message,
}

/// Request-or-response switch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Request(Command),
    Response(Response),
}

/// Commands the client can send. Each mutating variant produces `CommandOutput`
/// with the rendered stdout the CLI would print in-process; the in-process
/// render logic is shared so behavior matches byte-for-byte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    // --- Daemon control ------------------------------------------------------
    /// Health check. Returns `Payload::Pong`.
    Ping,
    /// Request `Payload::Status` with PID, uptime, counters.
    Status,
    /// Force the daemon to `save()` its in-RAM state to disk and truncate
    /// the WAL, without shutting down. Used after a batch of edits to
    /// guarantee they survive a hard crash without waiting for the 100 ms
    /// fsync timer or an idle checkpoint.
    Checkpoint,
    /// Request graceful shutdown. Daemon responds, closes socket, exits.
    Shutdown { reason: ShutdownReason },

    // --- Mutating ------------------------------------------------------------
    /// `legend memory tick` — record a memory. `blocker=true` prefixes `BLOCKER:`
    /// and boosts salience (same as `--blocker`).
    Tick { text: String, blocker: bool },
    TaskSet { text: String },
    TaskClear,
    Reinforce { signal: f32, ids: Vec<u64> },
    Consolidate,
    Reset,
    Discover { path: Option<String>, apply: bool },
    Init { discover: bool },
    DevPruneNoise,

    // --- Read-only -----------------------------------------------------------
    Start {
        category: Option<String>,
        compact: bool,
        json: bool,
        tokens: bool,
        query: Option<String>,
    },
    Query { text: String, with_reasons: bool },
    Context,
    Dump,
    Stats,
    Sessions { count: usize, all: bool },
    TaskGet,
    /// Personality summary: distilled preferences/decisions/architecture/
    /// concerns + top L3 entities. Read-only. See `render_personality`.
    Personality,

    // --- Structured payloads (for non-CLI consumers, e.g. `mcp-serve`) ------
    /// Same state mutation as [`Command::Tick`], but returns `TickResult`
    /// structured data instead of a pre-rendered stdout string. Used by
    /// `mcp-serve` which formats the response into MCP-shaped markdown.
    TickStructured { text: String, blocker: bool },
    /// Same state read as [`Command::Query`], but returns a full
    /// `MemoryContext` for MCP's own format path.
    QueryStructured { text: String },

    // --- Queue-management primitives (item #14) -----------------------------
    /// Flip a single plan item's status by its leading numeric prefix.
    /// E.g. `PlanSetStatus { plan_name: "Current Work Queue", item_number: 15,
    /// status: "done" }` finds the item whose text starts with "15. " in the
    /// "Current Work Queue" plan and sets its status.
    ///
    /// Surgical alternative to a full-state `PLAN:` tick; no heredoc, no
    /// renumber, ~20 ms wire cost.
    PlanSetStatus {
        plan_name: String,
        item_number: u64,
        status: String,
    },
    /// List all plans with their item-count breakdown (read-only).
    PlanList,
    /// Show one plan's items in order (read-only).
    PlanShow { plan_name: String },
    /// Move `from_pos` (1-indexed) to `to_pos`; renumber remaining items.
    PlanReorder {
        plan_name: String,
        from_pos: usize,
        to_pos: usize,
    },
    /// Append a new item to a plan. Auto-numbered at the tail.
    PlanAdd {
        plan_name: String,
        status: String,
        text: String,
    },
    /// Remove the item with leading number `item_number`; renumber the rest.
    PlanRemove {
        plan_name: String,
        item_number: u64,
    },
}

/// Why a shutdown was requested — useful for logs and the user-visible status line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShutdownReason {
    Explicit,
    VersionMismatch,
    IdleTimeout,
}

/// Response envelope. Either a typed `Payload` on success, or a structured `Error`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Ok(Payload),
    Err(Error),
}

/// Command-specific success payload.
///
/// `PartialEq` is derived on the enum for round-trip test assertions.
/// `TickResult` / `MemoryContext` both derive `PartialEq` transitively through
/// their serde-exposed fields, so the derive holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload {
    Pong,
    Status(StatusInfo),
    /// Acknowledgment with no data — e.g. successful Shutdown receipt.
    Ack,
    /// Rendered stdout text for a CLI command. The client prints this verbatim.
    /// Matches the byte-for-byte output of the in-process code path.
    CommandOutput { stdout: String },
    /// Structured `TickResult` returned by `Command::TickStructured`.
    /// Consumers render it themselves (e.g. mcp-serve emits MCP markdown).
    TickResultPayload(TickResult),
    /// Structured `MemoryContext` returned by `Command::QueryStructured`.
    QueryContext(MemoryContext),
}

/// Daemon health snapshot returned by `Command::Status`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusInfo {
    pub pid: u32,
    pub uptime_secs: u64,
    pub protocol_version: u16,
    pub requests_handled: u64,
    pub socket_path: String,
}

/// Structured error — version-skewed clients can still decode enough to show
/// a useful message even if they can't match the full variant set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ErrorKind {
    /// Client's `version` didn't match daemon's `PROTOCOL_VERSION`.
    VersionMismatch { client: u16, daemon: u16 },
    /// Command not yet implemented in the daemon (Phase 1 stubs).
    NotImplemented,
    /// Unexpected internal error; `message` has the detail.
    Internal,
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// Write a single framed envelope: 4-byte BE length + MessagePack body.
pub fn write_frame<W: Write>(writer: &mut W, envelope: &Envelope) -> std::io::Result<()> {
    let body = rmp_serde::to_vec_named(envelope)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame too large: {} bytes", body.len()),
        ));
    }
    let len = body.len() as u32;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(&body)?;
    writer.flush()
}

/// Read a single framed envelope. Returns `Ok(None)` on clean EOF before any byte
/// of the length prefix — callers treat that as the peer closing the connection.
pub fn read_frame<R: Read>(reader: &mut R) -> std::io::Result<Option<Envelope>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("frame too large: {} bytes", len),
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    let envelope: Envelope = rmp_serde::from_slice(&body)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(Some(envelope))
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

impl Envelope {
    pub fn request(id: u64, cmd: Command) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            body: Message::Request(cmd),
        }
    }

    pub fn ok(id: u64, payload: Payload) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            body: Message::Response(Response::Ok(payload)),
        }
    }

    pub fn err(id: u64, error: Error) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            id,
            body: Message::Response(Response::Err(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Envelope equality via debug-string comparison — Payload intentionally
    /// doesn't derive PartialEq (see the struct doc), so tests that need
    /// structural equality compare `format!("{:?}", …)` instead.
    fn debug_eq<T: std::fmt::Debug>(a: &T, b: &T) {
        assert_eq!(format!("{:?}", a), format!("{:?}", b));
    }

    #[test]
    fn envelope_roundtrip_ping() {
        let original = Envelope::request(42, Command::Ping);
        let mut buf = Vec::new();
        write_frame(&mut buf, &original).expect("write");
        let mut cursor = Cursor::new(&buf);
        let decoded = read_frame(&mut cursor).expect("read").expect("some");
        debug_eq(&original, &decoded);
    }

    #[test]
    fn envelope_roundtrip_status_response() {
        let original = Envelope::ok(
            7,
            Payload::Status(StatusInfo {
                pid: 1234,
                uptime_secs: 600,
                protocol_version: PROTOCOL_VERSION,
                requests_handled: 42,
                socket_path: "/tmp/legend.sock".into(),
            }),
        );
        let mut buf = Vec::new();
        write_frame(&mut buf, &original).expect("write");
        let decoded = read_frame(&mut Cursor::new(&buf)).expect("read").expect("some");
        debug_eq(&original, &decoded);
    }

    #[test]
    fn envelope_roundtrip_err() {
        let original = Envelope::err(
            1,
            Error {
                kind: ErrorKind::VersionMismatch {
                    client: 99,
                    daemon: PROTOCOL_VERSION,
                },
                message: "client too new".into(),
            },
        );
        let mut buf = Vec::new();
        write_frame(&mut buf, &original).unwrap();
        let decoded = read_frame(&mut Cursor::new(&buf)).unwrap().unwrap();
        debug_eq(&original, &decoded);
    }

    #[test]
    fn read_frame_clean_eof_returns_none() {
        let empty: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(&empty);
        assert!(read_frame(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn read_frame_rejects_oversized() {
        // length prefix claims a frame > MAX_FRAME_BYTES
        let bogus = ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes();
        let mut cursor = Cursor::new(&bogus);
        let err = read_frame(&mut cursor).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn multiple_frames_read_sequentially() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Envelope::request(1, Command::Ping)).unwrap();
        write_frame(&mut buf, &Envelope::request(2, Command::Status)).unwrap();
        let mut cursor = Cursor::new(&buf);
        let a = read_frame(&mut cursor).unwrap().unwrap();
        let b = read_frame(&mut cursor).unwrap().unwrap();
        assert_eq!(a.id, 1);
        assert_eq!(b.id, 2);
        assert!(read_frame(&mut cursor).unwrap().is_none());
    }
}
