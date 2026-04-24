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

/// Wire protocol version. Bump on any `Command` / `Payload` / `Envelope` shape change.
pub const PROTOCOL_VERSION: u16 = 1;

/// Upper bound on a single framed message (defends against hostile or corrupt peers).
/// 64 MB is deliberately generous — `Dump` can be large; anything bigger is pathological.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Top-level IPC envelope. Carries version, correlation id, and typed body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u16,
    pub id: u64,
    pub body: Message,
}

/// Request-or-response switch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

/// Why a shutdown was requested — useful for logs and the user-visible status line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShutdownReason {
    Explicit,
    VersionMismatch,
    IdleTimeout,
}

/// Response envelope. Either a typed `Payload` on success, or a structured `Error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Ok(Payload),
    Err(Error),
}

/// Command-specific success payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Payload {
    Pong,
    Status(StatusInfo),
    /// Acknowledgment with no data — e.g. successful Shutdown receipt.
    Ack,
    /// Rendered stdout text for a CLI command. The client prints this verbatim.
    /// Matches the byte-for-byte output of the in-process code path.
    CommandOutput { stdout: String },
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

    #[test]
    fn envelope_roundtrip_ping() {
        let original = Envelope::request(42, Command::Ping);
        let mut buf = Vec::new();
        write_frame(&mut buf, &original).expect("write");
        let mut cursor = Cursor::new(&buf);
        let decoded = read_frame(&mut cursor).expect("read").expect("some");
        assert_eq!(original, decoded);
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
        assert_eq!(original, decoded);
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
        assert_eq!(original, decoded);
    }

    #[test]
    fn read_frame_clean_eof_returns_none() {
        let empty: Vec<u8> = Vec::new();
        let mut cursor = Cursor::new(&empty);
        assert_eq!(read_frame(&mut cursor).unwrap(), None);
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
        assert_eq!(read_frame(&mut cursor).unwrap(), None);
    }
}
