//! Cross-platform daemon process detachment.
//!
//! Phase 1 purpose: `legend daemon start --detach` (internal subcommand) can
//! fork/detach itself into a background process so a CLI invocation can call
//! `spawn_detached` and return immediately.
//!
//! Unix: `setsid()` via `pre_exec` so the child drops its controlling TTY and
//! survives parent exit.
//!
//! Windows: `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` via `creation_flags`
//! so the child doesn't inherit the console and forms its own process group
//! (prevents Ctrl-C in the console from killing the daemon).
//!
//! No external crate needed — this lives in ~30 LOC behind `#[cfg(...)]`.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::socket_path::socket_path;

/// Spawn this binary as a detached background daemon, then return. Returns
/// once the child socket is observable, up to `wait_for_socket`.
///
/// Unused in Phase 1 (the scaffolding phase). Phase 2 calls this from
/// `try_over_ipc` when `Stream::connect` returns `NotFound`/`ConnectionRefused`.
#[allow(dead_code)]
pub fn spawn_detached(binary: &std::path::Path) -> std::io::Result<()> {
    let mut cmd = Command::new(binary);
    cmd.arg("daemon")
        .arg("start")
        .arg("--detach")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is a syscall that only manipulates the child's
        // session/process-group membership. No memory is touched.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS = 0x00000008, CREATE_NEW_PROCESS_GROUP = 0x00000200.
        cmd.creation_flags(0x00000008 | 0x00000200);
    }

    // We don't wait() on the child — it's supposed to outlive us. Just spawn and
    // let the OS reparent it (to init on Unix, no parent on Windows).
    let _child = cmd.spawn()?;
    Ok(())
}

/// Poll until the socket at `socket_path()` is connectable, or `timeout` elapses.
/// Used by auto-spawn to avoid "no daemon yet" racing the spawn.
///
/// Unused in Phase 1. Phase 2 calls this immediately after `spawn_detached`
/// to block until the child daemon is accepting connections.
#[allow(dead_code)]
pub fn wait_for_socket(timeout: Duration) -> bool {
    use interprocess::local_socket::prelude::*;
    use interprocess::local_socket::Stream;
    #[cfg(unix)]
    use interprocess::local_socket::GenericFilePath;
    #[cfg(windows)]
    use interprocess::local_socket::GenericNamespaced;

    let path = socket_path();
    let deadline = Instant::now() + timeout;
    let mut sleep_ms = 10u64;
    while Instant::now() < deadline {
        #[cfg(unix)]
        let name_result = path.clone().to_fs_name::<GenericFilePath>();
        #[cfg(windows)]
        let name_result = path.clone().to_fs_name::<GenericNamespaced>();

        if let Ok(name) = name_result {
            if Stream::connect(name).is_ok() {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(sleep_ms));
        sleep_ms = (sleep_ms * 2).min(250);
    }
    false
}
