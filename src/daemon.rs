//! Long-running daemon that keeps the substrate in RAM across ticks.
//!
//! The CLI's per-invocation tick path pays ~25s on long inputs because
//! every process restart reloads GLiNER's INT8 weights, the MiniLM
//! tokenizer, and re-decodes the LZ4 snapshot. The daemon amortizes
//! that — load once, then accept many tick requests over a TCP
//! loopback socket. Same-machine clients connect via a port file at
//! `.legend/legend.port`. A `fs2`-acquired exclusive flock on
//! `.legend/legend.lock` keeps two daemons from racing each other.
//!
//! Wire format: `[u32 LE length][rmp-serde MessagePack payload]`.
//! Same MessagePack the substrate already uses for the on-disk
//! snapshot — no JSON anywhere in Legend's serialization paths.
//!
//! Lifecycle:
//! - `legend start` — spawn detached daemon, wait for port file to
//!   appear, return.
//! - `legend stop` — connect, send `Stop`, daemon exits + cleans up.
//! - `legend status` — uptime, tick count, substrate sizes.
//! - `legend <text>` — connect (auto-start if not running), send
//!   `Tick`, render the returned frame.
//!
//! TTL: `LEGEND_DAEMON_TTL=300` (default 5 minutes). Idle timer
//! resets on every request. When idle exceeds TTL the daemon exits
//! cleanly. Per the design call, v0 is single-threaded — one tick
//! at a time; concurrent clients queue at the accept loop.

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::MAX_INPUT_TOKENS;
use crate::embed::{embed_text, token_count};
use crate::execute_tick;
use crate::inference::deberta::tokenizer::BUNDLED_TOKENIZER;
use crate::inference::deberta::weights_int8::WeightsDebertaInt8;
use crate::persistence::{default_path, load_or_seed, save};
use crate::seed::load_seed_graph;
use crate::types::{ConsciousAttentionFrame, Hypergraph};

// ─── Protocol ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonRequest {
    /// Run one Step 1-12 tick over `input`. The daemon mutates the
    /// in-RAM substrate and persists at the end of the tick (same
    /// semantics as the standalone CLI).
    Tick { input: String },
    /// Daemon exits after replying. Releases the lock, deletes the
    /// port file. Idempotent: stopping an already-stopped daemon is
    /// a no-op for the client (connect just fails).
    Stop,
    /// Snapshot of how the daemon is doing — uptime, how many ticks
    /// it's processed, current substrate sizes.
    Status,
    /// Wipe the in-RAM substrate and reload from the bundled seed
    /// graph, then persist the fresh snapshot to disk. The replacement
    /// of `legend.rs`'s old `LEGEND_RESET=1` env var, which only made
    /// sense when each CLI invocation owned its own substrate.
    Reset,
}

/// Boxed `ConsciousAttentionFrame` to keep the variant sizes balanced
/// for clippy's `large_enum_variant`. Frames carry `Vec`s but no
/// indices (transient type), so they're ~hundreds of bytes when
/// non-empty — small in absolute terms but multiples larger than
/// the other variants.
#[derive(Debug, Serialize, Deserialize)]
pub enum DaemonResponse {
    /// Successful tick. The frame is the entire observable surface
    /// (§docs/frame-as-surface.md) — it ships denormalized so the
    /// client renders without `Hypergraph` access. Substrate-size
    /// counts move to `Status` for consumers that want them.
    Frame(Box<ConsciousAttentionFrame>),
    /// Daemon acknowledged `Stop` and is exiting.
    Stopping,
    Status {
        pid: u32,
        uptime_secs: u64,
        tick_count: u64,
        elements: usize,
        relations: usize,
    },
    /// Daemon acknowledged `Reset`. Reports the post-reset substrate
    /// sizes (which equal the freshly-loaded seed sizes).
    Reset { elements: usize, relations: usize },
    /// Server-side failure (load error, panic-during-tick, persist
    /// failure). `message` is the `Display` of the underlying error.
    /// Clients should print it and exit non-zero.
    Error { message: String },
}

/// Send a framed MessagePack message to `w`. Length prefix is
/// `u32 LE`. Returns the total number of bytes written (header + body).
pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<usize> {
    let payload = rmp_serde::to_vec(msg).map_err(io::Error::other)?;
    let len = payload.len() as u32;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&payload)?;
    w.flush()?;
    Ok(payload.len() + 4)
}

/// Read a framed MessagePack message from `r`. Bounded to `MAX_FRAME`
/// to avoid a malformed/oversized header pulling unbounded memory.
pub fn read_frame<R: Read, T: for<'de> Deserialize<'de>>(r: &mut R) -> io::Result<T> {
    const MAX_FRAME: usize = 64 * 1024 * 1024; // 64 MiB
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame too large: {len} bytes (max {MAX_FRAME})"),
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    rmp_serde::from_slice(&payload).map_err(io::Error::other)
}

// ─── Paths ──────────────────────────────────────────────────────────

/// `.legend/legend.lock` — fs2 exclusive flock guards single-writer.
pub fn lock_path() -> PathBuf {
    state_dir().join("legend.lock")
}

/// `.legend/legend.port` — written once at daemon startup, deleted
/// at clean shutdown. Format: `"<pid>:<port>\n"`. ASCII only so
/// clients can read it without pulling another serde codec.
pub fn port_path() -> PathBuf {
    state_dir().join("legend.port")
}

fn state_dir() -> PathBuf {
    std::env::var_os("LEGEND_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".legend"))
}

/// Default TTL: 5 minutes idle. `LEGEND_DAEMON_TTL` overrides.
fn default_ttl() -> Duration {
    let secs = std::env::var("LEGEND_DAEMON_TTL")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(300);
    Duration::from_secs(secs)
}

// ─── Server ─────────────────────────────────────────────────────────

/// Daemon entry point. Acquires the lock, binds TCP loopback, writes
/// the port file, loads the substrate, then loops accepting requests
/// until TTL expires or a `Stop` arrives.
///
/// On any unrecoverable error (lock contention, bind failure,
/// persist failure) returns an `Err` and the parent / supervisor
/// can decide whether to retry. Clean exit on TTL or `Stop` returns
/// `Ok(())`.
pub fn serve() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Make sure the state dir exists so the lock file can land.
    let dir = state_dir();
    std::fs::create_dir_all(&dir)?;

    // 2. Exclusive flock. If another daemon already holds it, exit
    //    quietly — the client will find the existing one via the
    //    port file.
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path())?;
    if lock_file.try_lock_exclusive().is_err() {
        eprintln!("[daemon] another instance holds the lock; exiting");
        return Ok(());
    }

    // 3. Bind loopback with OS-assigned port, then publish that
    //    port + pid so clients can find us.
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
    let port = listener.local_addr()?.port();
    let pid = std::process::id();
    std::fs::write(port_path(), format!("{pid}:{port}\n"))?;
    eprintln!("[daemon] listening on 127.0.0.1:{port}, pid {pid}");

    // 4. Load substrate once. Subsequent ticks mutate this in place;
    //    persist runs at the end of each tick.
    let snapshot_path = default_path();
    let mut hypergraph = load_or_seed(&snapshot_path)?;
    eprintln!(
        "[daemon] loaded substrate: {} elements, {} relations",
        hypergraph.elements.len(),
        hypergraph.relations.len(),
    );

    // 5. Warm the heavy model singletons so the first tick doesn't
    //    pay their init cost. GLiNER tokenizer init alone is ~162 ms
    //    — shifting it off the user's first interactive tick onto
    //    daemon startup is the whole point of this step.
    let warm_start = Instant::now();
    warm_models();
    eprintln!("[daemon] warmed models in {:?}", warm_start.elapsed());

    // 5. Accept loop with TTL. set_nonblocking + spin loop with sleep
    //    is portable and good enough for a 100ms poll interval.
    listener.set_nonblocking(true)?;
    let ttl = default_ttl();
    let started = Instant::now();
    let mut last_request = Instant::now();
    let mut tick_count: u64 = 0;
    let mut stop_requested = false;

    while !stop_requested {
        // TTL check.
        if last_request.elapsed() >= ttl {
            eprintln!(
                "[daemon] idle for {:?} ≥ TTL {:?}; exiting",
                last_request.elapsed(),
                ttl,
            );
            break;
        }
        // Accept (non-blocking).
        match listener.accept() {
            Ok((stream, _)) => {
                last_request = Instant::now();
                handle_one(
                    stream,
                    &mut hypergraph,
                    &snapshot_path,
                    started,
                    tick_count,
                    &mut stop_requested,
                )?;
                tick_count += 1;
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.into()),
        }
    }

    // 6. Clean up.
    let _ = std::fs::remove_file(port_path());
    let _ = FileExt::unlock(&lock_file);
    eprintln!("[daemon] exiting cleanly");
    Ok(())
}

fn handle_one(
    mut stream: TcpStream,
    hypergraph: &mut Hypergraph,
    snapshot_path: &Path,
    started: Instant,
    tick_count_so_far: u64,
    stop_flag: &mut bool,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    stream.set_write_timeout(Some(Duration::from_secs(60)))?;
    let req: DaemonRequest = match read_frame(&mut stream) {
        Ok(r) => r,
        Err(e) => {
            let _ = write_frame(
                &mut stream,
                &DaemonResponse::Error {
                    message: format!("failed to parse request: {e}"),
                },
            );
            return Ok(());
        }
    };
    let resp = match req {
        DaemonRequest::Tick { input } => match tick(hypergraph, &input, snapshot_path) {
            Ok(frame) => DaemonResponse::Frame(Box::new(frame)),
            Err(e) => DaemonResponse::Error {
                message: format!("tick failed: {e}"),
            },
        },
        DaemonRequest::Status => DaemonResponse::Status {
            pid: std::process::id(),
            uptime_secs: started.elapsed().as_secs(),
            tick_count: tick_count_so_far,
            elements: hypergraph.elements.len(),
            relations: hypergraph.relations.len(),
        },
        DaemonRequest::Stop => {
            *stop_flag = true;
            DaemonResponse::Stopping
        }
        DaemonRequest::Reset => match reset(hypergraph, snapshot_path) {
            Ok(()) => DaemonResponse::Reset {
                elements: hypergraph.elements.len(),
                relations: hypergraph.relations.len(),
            },
            Err(e) => DaemonResponse::Error {
                message: format!("reset failed: {e}"),
            },
        },
    };
    let _ = write_frame(&mut stream, &resp);
    Ok(())
}

/// Run one tick over the daemon's in-RAM substrate. Same Step 1-12
/// pipeline as `lib::run()`'s tick block, just without process-level
/// scaffolding (printing, snapshot reload, init-warmup timing).
fn tick(
    hypergraph: &mut Hypergraph,
    input: &str,
    snapshot_path: &Path,
) -> Result<ConsciousAttentionFrame, Box<dyn std::error::Error>> {
    let n_tokens = token_count(input);
    if n_tokens > MAX_INPUT_TOKENS {
        return Err(format!("input too long: {n_tokens} tokens, max {MAX_INPUT_TOKENS}").into());
    }
    let frame = execute_tick::run(input, hypergraph);
    save(hypergraph, snapshot_path)?;
    Ok(frame)
}

/// Touch the heaviest model singletons so they're ready before the
/// first tick request lands. Only forces the one-time init cost
/// (LazyLock construction); doesn't run a forward pass — per-input
/// matmul work is paid every tick regardless and can't be amortized.
fn warm_models() {
    // GLiNER tokenizer — the dominant init cost (~162 ms). Parsing
    // a tokenizer.json with 250k merges is the bottleneck.
    let _ = BUNDLED_TOKENIZER.get_vocab_size(true);
    // GLiNER INT8 weights — ~150 MB memmap; cheap (~3 ms) but worth
    // doing here so the first NER call doesn't page-fault on read.
    let _ = WeightsDebertaInt8::load_bundled();
    // MiniLM tokenizer + INT8 weights + warm the matmul/cache state
    // via a tiny forward pass. `embed_text` exercises both singletons.
    let _ = embed_text("warmup");
}

/// Replace the in-RAM substrate with a fresh seed graph and persist
/// the new (seed) state to disk. The daemon keeps running; the next
/// tick will mutate the fresh substrate. Used by `legend reset`.
fn reset(
    hypergraph: &mut Hypergraph,
    snapshot_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    *hypergraph = load_seed_graph();
    save(hypergraph, snapshot_path)?;
    Ok(())
}

// ─── Client ─────────────────────────────────────────────────────────

/// Parse `.legend/legend.port` and try to connect. Returns
/// `Ok(stream)` on success, `Err(io::ErrorKind::NotFound)` when the
/// port file is missing, or other IO error variants for stale-port
/// cases (connection refused → daemon died with file present).
pub fn try_connect() -> io::Result<TcpStream> {
    let bytes = std::fs::read_to_string(port_path())?;
    let port = parse_port_file(&bytes).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "port file is not in <pid>:<port> form",
        )
    })?;
    let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(Duration::from_secs(120)))?;
    Ok(stream)
}

fn parse_port_file(s: &str) -> Option<u16> {
    let trimmed = s.trim();
    let (_pid, port) = trimmed.split_once(':')?;
    port.parse().ok()
}

/// Send a single request, return the response. Convenience wrapper
/// for the common "open, send, recv, drop" pattern.
pub fn round_trip(req: &DaemonRequest) -> io::Result<DaemonResponse> {
    let mut s = try_connect()?;
    write_frame(&mut s, req)?;
    read_frame(&mut s)
}

/// Spawn the daemon as a detached background process. Waits up to
/// `ready_timeout` for the port file to appear (a proxy for "the
/// daemon is listening"). Cross-platform: `std::process::Command`
/// with stdio nulled out on Unix and Windows alike.
pub fn spawn_detached(ready_timeout: Duration) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Propagate the state-dir env so the child binds to the same
    // workspace its parent meant to address.
    if let Some(dir) = std::env::var_os("LEGEND_STATE_DIR") {
        cmd.env("LEGEND_STATE_DIR", dir);
    }
    if let Some(ttl) = std::env::var_os("LEGEND_DAEMON_TTL") {
        cmd.env("LEGEND_DAEMON_TTL", ttl);
    }
    let _child = cmd.spawn()?;

    // Wait for the port file to appear. Poll every 50ms.
    let start = Instant::now();
    while start.elapsed() < ready_timeout {
        if port_path().exists() && try_connect().is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "daemon did not become reachable within {:?}; check stderr or run `legend __daemon` manually",
            ready_timeout
        ),
    ))
}

/// Try `try_connect`; if that fails because no daemon is running,
/// spawn one and try again. Returns the open stream.
pub fn connect_or_start() -> io::Result<TcpStream> {
    match try_connect() {
        Ok(s) => Ok(s),
        Err(_) => {
            spawn_detached(Duration::from_secs(15))?;
            try_connect()
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_tick_request() {
        let req = DaemonRequest::Tick {
            input: "Polaris is in Rust.".to_string(),
        };
        let mut buf: Vec<u8> = Vec::new();
        write_frame(&mut buf, &req).expect("write");
        let mut cursor = std::io::Cursor::new(&buf);
        let back: DaemonRequest = read_frame(&mut cursor).expect("read");
        match back {
            DaemonRequest::Tick { input } => assert_eq!(input, "Polaris is in Rust."),
            other => panic!("round-trip mismatch: {other:?}"),
        }
    }

    #[test]
    fn frame_round_trip_status_response() {
        let resp = DaemonResponse::Status {
            pid: 12345,
            uptime_secs: 60,
            tick_count: 7,
            elements: 700,
            relations: 800,
        };
        let mut buf: Vec<u8> = Vec::new();
        write_frame(&mut buf, &resp).expect("write");
        let back: DaemonResponse = read_frame(&mut std::io::Cursor::new(&buf)).expect("read");
        match back {
            DaemonResponse::Status {
                pid,
                uptime_secs,
                tick_count,
                elements,
                relations,
            } => {
                assert_eq!(pid, 12345);
                assert_eq!(uptime_secs, 60);
                assert_eq!(tick_count, 7);
                assert_eq!(elements, 700);
                assert_eq!(relations, 800);
            }
            other => panic!("expected Status, got {other:?}"),
        }
    }

    #[test]
    fn oversized_frame_rejected() {
        // Hand-craft a header claiming the body is bigger than MAX_FRAME.
        let mut buf: Vec<u8> = Vec::new();
        let huge: u32 = 200 * 1024 * 1024; // 200 MiB
        buf.extend_from_slice(&huge.to_le_bytes());
        let result: io::Result<DaemonRequest> = read_frame(&mut std::io::Cursor::new(&buf));
        assert!(result.is_err());
        let e = result.unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn port_file_parser_accepts_pid_colon_port() {
        assert_eq!(parse_port_file("12345:54321"), Some(54321));
        assert_eq!(parse_port_file("12345:54321\n"), Some(54321));
        assert_eq!(parse_port_file("not-a-port"), None);
        assert_eq!(parse_port_file("12345"), None);
    }
}
