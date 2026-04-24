//! Minimal synchronous IPC client used by `daemon stop` / `daemon status` /
//! Phase 2's `try_over_ipc`.
//!
//! Opens a single connection, writes one request, reads one response, closes.
//! No pooling — the cost of a local-socket connect is ~10 µs.

use std::io::{BufReader, BufWriter};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::Stream;
#[cfg(unix)]
use interprocess::local_socket::GenericFilePath;
#[cfg(windows)]
use interprocess::local_socket::GenericNamespaced;

use super::ipc::{read_frame, write_frame, Command, Envelope, Message, Payload, Response};
use super::socket_path::socket_path;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Errors the client surfaces to callers. Kept small so Phase 2 can match on
/// `NoDaemon` to decide whether to auto-spawn.
#[derive(Debug)]
pub enum ClientError {
    /// Socket file / named pipe not found, or peer refused the connection.
    /// Callers typically treat this as "daemon not running".
    NoDaemon,
    /// Wire error, version mismatch, or the daemon returned a structured error.
    Protocol(String),
    /// Any other I/O failure.
    Io(std::io::Error),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NoDaemon => f.write_str("daemon not running"),
            ClientError::Protocol(msg) => write!(f, "daemon protocol error: {}", msg),
            ClientError::Io(e) => write!(f, "daemon ipc io: {}", e),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<std::io::Error> for ClientError {
    fn from(e: std::io::Error) -> Self {
        // Map connection-refused / not-found to `NoDaemon` so callers don't
        // have to re-match these every time.
        match e.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                ClientError::NoDaemon
            }
            _ => ClientError::Io(e),
        }
    }
}

/// Send one `Command` and read one `Envelope` back. The `_timeout` parameter is
/// reserved for Phase 2; today the call blocks until the daemon responds or the
/// peer closes the connection.
pub fn send(cmd: Command, _timeout: Duration) -> Result<Envelope, ClientError> {
    let path = socket_path();
    #[cfg(unix)]
    let name = path
        .to_fs_name::<GenericFilePath>()
        .map_err(|e| ClientError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;
    #[cfg(windows)]
    let name = path
        .to_fs_name::<GenericNamespaced>()
        .map_err(|e| ClientError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))?;

    let stream = Stream::connect(name)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let envelope = Envelope::request(id, cmd);

    let (reader, writer) = (&stream, &stream);
    let mut reader = BufReader::new(reader);
    let mut writer = BufWriter::new(writer);
    write_frame(&mut writer, &envelope)?;

    let response = read_frame(&mut reader)?
        .ok_or_else(|| ClientError::Protocol("daemon closed connection before responding".into()))?;

    if response.id != id {
        return Err(ClientError::Protocol(format!(
            "response id {} did not match request id {}",
            response.id, id
        )));
    }

    Ok(response)
}

/// Extract a typed `Payload` from an `Envelope`, failing with the daemon's
/// structured `Error` if the response was a failure.
pub fn into_payload(envelope: Envelope) -> Result<Payload, ClientError> {
    match envelope.body {
        Message::Response(Response::Ok(payload)) => Ok(payload),
        Message::Response(Response::Err(err)) => Err(ClientError::Protocol(format!(
            "{:?}: {}",
            err.kind, err.message
        ))),
        Message::Request(_) => Err(ClientError::Protocol(
            "daemon replied with a Request, not a Response".into(),
        )),
    }
}

/// Convenience: check whether the daemon is reachable. Does NOT auto-spawn.
pub fn is_running() -> bool {
    match send(Command::Ping, Duration::from_millis(500)) {
        Ok(env) => matches!(
            env.body,
            Message::Response(Response::Ok(Payload::Pong))
        ),
        Err(_) => false,
    }
}
