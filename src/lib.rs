pub mod daemon;
pub mod embed;
pub mod execute_tick;
pub mod hebbian;
pub mod inference;
pub mod intent_classifiers;
pub mod lexical_features;
pub mod math;
pub mod merge;
pub mod persistence;
pub mod render;
pub mod seed;
pub mod steps;
pub mod types;

use std::fs;

use embed::token_count;

/// Maximum tokens accepted in a single tick. Matches GLiNER2's 512-token
/// max-input minus a safety margin for special tokens, positional buffer,
/// and coref-context bytes (§11.4). Inputs above this are rejected at the
/// tick boundary — the caller chunks long inputs into multiple ticks.
pub const MAX_INPUT_TOKENS: usize = 480;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: legend <text>           # run one tick (auto-starts daemon)\n\
             legend start                  # launch daemon in the background\n\
             legend stop                   # ask the daemon to exit cleanly\n\
             legend status                 # daemon pid, uptime, substrate sizes\n\
             legend reset                  # wipe substrate; reload from seed\n\
             legend init                   # set up this repo for legend (merge driver, …)"
        );
        return Ok(());
    }

    // Subcommand dispatch. Daemon verbs first, then init, then fall
    // through to the tick path. `__daemon` is the private "run as
    // the daemon process" verb; `start` wraps it with detached spawn.
    // `git-merge-driver` is invoked by git itself (registered via
    // `legend init`); users never type it.
    match args[1].as_str() {
        "__daemon" => return daemon::serve(),
        "start" => return daemon_start(),
        "stop" => return daemon_stop(),
        "status" => return daemon_status(),
        "reset" => return daemon_reset(),
        "git-merge-driver" => return git_merge_driver(&args[2..]),
        "init" => return init(),
        _ => {}
    }

    // Tick path: first positional arg is the input text; trailing
    // positional args are ignored. `args.len() >= 2` is guaranteed
    // by the usage-banner short-circuit above.
    let input_text = args[1].clone();

    // Client-side token gate — reject too-long inputs before the IPC
    // round trip. The daemon performs its own check as the authoritative
    // boundary; this is a fast-fail so we don't pay for the round trip
    // on inputs we already know will be rejected.
    let n_tokens = token_count(&input_text);
    if n_tokens > MAX_INPUT_TOKENS {
        return Err(format!("input too long: {n_tokens} tokens, max {MAX_INPUT_TOKENS}").into());
    }

    let mut stream = daemon::connect_or_start()?;
    daemon::write_frame(
        &mut stream,
        &daemon::DaemonRequest::Tick { input: input_text },
    )?;
    let resp: daemon::DaemonResponse = daemon::read_frame(&mut stream)?;

    // Frame is the entire observable surface (§docs/frame-as-surface.md).
    // Rendering is a pure function over the frame; the CLI is a relay.
    match resp {
        daemon::DaemonResponse::Frame(frame) => {
            print!("{}", render::render_frame(&frame));
            Ok(())
        }
        daemon::DaemonResponse::Error { message } => Err(message.into()),
        other => Err(format!("unexpected tick response: {other:?}").into()),
    }
}

// ─── git-merge-driver entry point ─────────────────────────────────────────

/// Invoked by git when `.legend/memory.lz4` has merge conflicts.
/// Args (per git's merge-driver contract): `%O %A %B [%P]` —
/// ancestor, ours, theirs, and (optionally) the path it's resolving.
///
/// We don't use %O (the ancestor) in v0 — the merge is symmetric
/// union with conflict-resolution rules baked in, not a 3-way diff.
/// `%P` is purely informational; we accept and ignore it.
///
/// Writes the merged result to `%A` (ours's path) and exits 0 on
/// success — git treats non-zero as "conflict not resolved" and
/// leaves the user to resolve manually.
pub fn git_merge_driver(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.len() < 3 {
        return Err("usage: legend git-merge-driver %O %A %B [%P]\n\
             %O=ancestor, %A=ours, %B=theirs, %P=path (optional)"
            .into());
    }
    let _ancestor_path = &args[0];
    let ours_path = std::path::Path::new(&args[1]);
    let theirs_path = std::path::Path::new(&args[2]);
    let filename = args.get(3).map(|s| s.as_str()).unwrap_or("memory.lz4");

    eprintln!("[legend] merging {filename}");
    let mut ours = persistence::load(ours_path)?;
    let theirs = persistence::load(theirs_path)?;
    let stats = merge::merge_hypergraphs(&mut ours, &theirs)?;
    persistence::save(&ours, ours_path)?;
    eprintln!(
        "[legend] merged: +{} elements, ={} unified, +{} relations, ={} merged, +{} recent_focus",
        stats.elements_added,
        stats.elements_unified,
        stats.relations_added,
        stats.relations_merged,
        stats.recent_focus_added,
    );
    Ok(())
}

/// Set up the current git repo to use Legend. Today this only
/// registers the substrate-aware merge driver for `.legend/memory.lz4`
/// conflicts; future work will also drop the cmp/agent files (CLAUDE.md
/// integration, agent harness configs) into the repo.
///
/// Each setup step is idempotent — re-running is safe.
pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    init_merge_driver()
}

/// Register Legend's merge driver for `.legend/memory.lz4`. Writes:
///   - `git config --local merge.legend.driver "<this-binary> git-merge-driver %O %A %B %P"`
///   - `.gitattributes` line `.legend/memory.lz4 merge=legend`
fn init_merge_driver() -> Result<(), Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let driver_cmd = format!("{} git-merge-driver %O %A %B %P", exe.display());
    let status = std::process::Command::new("git")
        .args(["config", "--local", "merge.legend.driver", &driver_cmd])
        .status()?;
    if !status.success() {
        return Err("git config --local failed; are you inside a git repo?".into());
    }
    println!("✓ registered merge.legend.driver = {driver_cmd}");

    let attrs_path = std::path::Path::new(".gitattributes");
    let existing = fs::read_to_string(attrs_path).unwrap_or_default();
    let rule = ".legend/memory.lz4 merge=legend";
    if !existing.lines().any(|l| l.trim() == rule) {
        let mut out = existing;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(rule);
        out.push('\n');
        fs::write(attrs_path, out)?;
        println!("✓ added `{rule}` to .gitattributes");
    } else {
        println!("✓ .gitattributes already has `{rule}`");
    }
    Ok(())
}

// ─── Daemon subcommand wrappers + client tick path ───────────────────────

/// `legend start`: spawn the daemon detached, wait for it to become
/// reachable. Idempotent — if a daemon is already up, just reports
/// status.
fn daemon_start() -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(daemon::DaemonResponse::Status {
        pid, uptime_secs, ..
    }) = daemon::round_trip(&daemon::DaemonRequest::Status)
    {
        println!("daemon already running: pid {pid} ({uptime_secs}s uptime)");
        return Ok(());
    }
    daemon::spawn_detached(std::time::Duration::from_secs(15))?;
    let resp = daemon::round_trip(&daemon::DaemonRequest::Status)?;
    if let daemon::DaemonResponse::Status { pid, .. } = resp {
        println!("daemon started: pid {pid}");
    }
    Ok(())
}

/// `legend stop`: connect, send `Stop`, daemon exits and cleans up
/// its lock + port file. Reports a friendly message if no daemon
/// is running rather than an error.
fn daemon_stop() -> Result<(), Box<dyn std::error::Error>> {
    match daemon::round_trip(&daemon::DaemonRequest::Stop) {
        Ok(daemon::DaemonResponse::Stopping) => {
            println!("daemon stopping");
            Ok(())
        }
        Ok(other) => Err(format!("unexpected response to Stop: {other:?}").into()),
        Err(_) => {
            println!("no daemon running");
            Ok(())
        }
    }
}

/// `legend status`: print pid / uptime / tick count / substrate
/// sizes if a daemon is running.
fn daemon_status() -> Result<(), Box<dyn std::error::Error>> {
    match daemon::round_trip(&daemon::DaemonRequest::Status) {
        Ok(daemon::DaemonResponse::Status {
            pid,
            uptime_secs,
            tick_count,
            elements,
            relations,
        }) => {
            println!(
                "daemon  pid={pid}  uptime={uptime_secs}s  ticks={tick_count}  \
                 elements={elements}  relations={relations}",
            );
            Ok(())
        }
        Ok(other) => Err(format!("unexpected status response: {other:?}").into()),
        Err(_) => {
            println!("no daemon running");
            Ok(())
        }
    }
}

/// `legend reset`: ask the daemon to drop its in-RAM substrate and
/// reload from the bundled seed graph, persisting the fresh snapshot.
/// Auto-starts the daemon if not already running (a fresh daemon would
/// have loaded the prior snapshot before we reset it — slightly
/// wasteful but the only way to keep the daemon as sole writer).
fn daemon_reset() -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = daemon::connect_or_start()?;
    daemon::write_frame(&mut stream, &daemon::DaemonRequest::Reset)?;
    let resp: daemon::DaemonResponse = daemon::read_frame(&mut stream)?;
    match resp {
        daemon::DaemonResponse::Reset {
            elements,
            relations,
        } => {
            println!("reset to seed: elements={elements}  relations={relations}");
            Ok(())
        }
        daemon::DaemonResponse::Error { message } => Err(message.into()),
        other => Err(format!("unexpected reset response: {other:?}").into()),
    }
}

