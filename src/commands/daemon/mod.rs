//! `legend daemon {start, stop, status}` — Phase 1 scaffolding.
//!
//! The daemon holds the ONNX model and (Phase 2+) `MemoryState` in RAM between
//! CLI invocations. Phase 1 lands only the plumbing: IPC protocol, socket/pipe
//! server, client helpers, process detachment. No existing command paths
//! change; there is no behavior delta for users of Phase 1 alone.

pub mod client;
pub mod handlers;
pub mod ipc;
pub mod server;
pub mod socket_path;
pub mod spawn;

use std::time::Duration;

use crate::cli::CommandDef;

pub fn handle_daemon(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    if args.is_empty() {
        print_daemon_help();
        return Ok(());
    }

    let sub = args[0].as_str();
    let _child_def = def.children.iter().find(|c| c.name == sub);

    match sub {
        "start" => handle_start(&args[1..]),
        "stop" => handle_stop(),
        "status" => handle_status(),
        "checkpoint" => handle_checkpoint(),
        _ => {
            print_daemon_help();
            Ok(())
        }
    }
}

fn handle_checkpoint() -> Result<(), Box<dyn std::error::Error>> {
    match client::send(ipc::Command::Checkpoint, Duration::from_secs(5)) {
        Ok(env) => match client::into_payload(env) {
            Ok(ipc::Payload::Ack) => {
                println!("✓ legend daemon: checkpoint complete (state saved, WAL truncated)");
                Ok(())
            }
            Ok(other) => Err(format!(
                "daemon returned unexpected payload for Checkpoint: {:?}",
                other
            )
            .into()),
            Err(e) => Err(Box::new(e)),
        },
        Err(client::ClientError::NoDaemon) => {
            println!(
                "legend daemon: not running (nothing to checkpoint; in-process commands save synchronously)"
            );
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

fn handle_start(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let detach = args.iter().any(|a| a == "--detach");

    if detach {
        // Internal path used by `spawn_detached` — we *are* the post-fork child.
        // Just run the server; setsid/DETACHED_PROCESS was already applied by
        // the parent during spawn.
        server::run_foreground()?;
        return Ok(());
    }

    // User-facing path: block in the foreground. Ctrl-C exits.
    if client::is_running() {
        println!("legend daemon: already running (socket {})", socket_path::socket_path());
        return Ok(());
    }

    println!(
        "legend daemon: starting in foreground on {} — Ctrl-C to exit",
        socket_path::socket_path()
    );
    server::run_foreground()?;
    Ok(())
}

fn handle_stop() -> Result<(), Box<dyn std::error::Error>> {
    match client::send(
        ipc::Command::Shutdown {
            reason: ipc::ShutdownReason::Explicit,
        },
        Duration::from_secs(3),
    ) {
        Ok(env) => match client::into_payload(env) {
            Ok(ipc::Payload::Ack) => {
                println!("✓ legend daemon: shutdown acknowledged");
                Ok(())
            }
            Ok(other) => Err(format!(
                "daemon returned unexpected payload for Shutdown: {:?}",
                other
            )
            .into()),
            Err(e) => Err(Box::new(e)),
        },
        Err(client::ClientError::NoDaemon) => {
            println!("legend daemon: not running");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

fn handle_status() -> Result<(), Box<dyn std::error::Error>> {
    match client::send(ipc::Command::Status, Duration::from_secs(1)) {
        Ok(env) => match client::into_payload(env) {
            Ok(ipc::Payload::Status(info)) => {
                println!(
                    "legend daemon: running\n  pid: {}\n  uptime: {}s\n  protocol: v{}\n  requests: {}\n  socket: {}",
                    info.pid,
                    info.uptime_secs,
                    info.protocol_version,
                    info.requests_handled,
                    info.socket_path,
                );
                Ok(())
            }
            Ok(other) => Err(format!(
                "daemon returned unexpected payload for Status: {:?}",
                other
            )
            .into()),
            Err(e) => Err(Box::new(e)),
        },
        Err(client::ClientError::NoDaemon) => {
            println!("legend daemon: not running");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}

fn print_daemon_help() {
    println!("Legend Daemon - long-running helper that holds ONNX model + state in RAM");
    println!();
    println!("Usage:");
    println!("  legend daemon start           Run the daemon in foreground (debugging)");
    println!("  legend daemon start --detach  Internal: post-fork entry point (not for humans)");
    println!("  legend daemon stop            Request a clean shutdown");
    println!("  legend daemon status          Report pid, uptime, protocol version, request count");
    println!("  legend daemon checkpoint      Force save + WAL truncate without shutting down");
}
