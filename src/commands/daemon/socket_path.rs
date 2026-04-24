//! Platform-specific resolver for the daemon IPC socket location.
//!
//! Unix: `$XDG_RUNTIME_DIR/legend/daemon.sock` if `$XDG_RUNTIME_DIR` is set
//! (respects the modern Linux convention for runtime sockets), otherwise
//! `~/.cache/legend/daemon.sock` as a portable fallback that works on macOS.
//!
//! Windows: `\\.\pipe\legend-<user>` — named pipes aren't filesystem paths, but
//! the same resolver returns a string the `interprocess` crate can parse.
//!
//! The `LEGEND_SOCKET` env var overrides the default everywhere, which is how
//! conformance tests isolate concurrent daemons.

use std::path::PathBuf;

/// Env-var override used by tests and for debugging.
pub const SOCKET_ENV_VAR: &str = "LEGEND_SOCKET";

/// Resolve the socket path for this user. Returns an OS-native string.
///
/// On Unix this is a filesystem path; callers that need to create directories
/// should use [`socket_parent_dir`] before binding.
pub fn socket_path() -> String {
    if let Ok(override_path) = std::env::var(SOCKET_ENV_VAR) {
        return override_path;
    }
    default_socket_path()
}

/// The default (non-overridden) socket path.
fn default_socket_path() -> String {
    #[cfg(unix)]
    {
        let dir = unix_socket_dir();
        dir.join("daemon.sock").to_string_lossy().into_owned()
    }
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
        // Sanitize — named-pipe names cannot contain backslashes or colons.
        let safe_user: String = user
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        format!(r"\\.\pipe\legend-{}", safe_user)
    }
}

/// Directory that should exist before binding the socket. `None` on Windows —
/// named pipes don't have a parent directory that needs to be created.
pub fn socket_parent_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        if std::env::var(SOCKET_ENV_VAR).is_ok() {
            // Override may point anywhere; callers are responsible for the dir.
            let path = PathBuf::from(socket_path());
            return path.parent().map(|p| p.to_path_buf());
        }
        Some(unix_socket_dir())
    }
    #[cfg(windows)]
    {
        None
    }
}

#[cfg(unix)]
fn unix_socket_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("legend");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".cache").join("legend")
}

/// PID file path — co-located with the socket so cleanup logic can find both together.
pub fn pid_path() -> PathBuf {
    #[cfg(unix)]
    {
        let parent = socket_parent_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
        parent.join("daemon.pid")
    }
    #[cfg(windows)]
    {
        // Named pipes have no parent dir; store PID in LocalAppData or tmp.
        let local_app = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
            std::env::var("TEMP").unwrap_or_else(|_| ".".to_string())
        });
        PathBuf::from(local_app).join("legend").join("daemon.pid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_env_wins() {
        // Serialize against other env-manipulating tests via a unique path.
        let unique = format!("/tmp/legend_test_override_{}.sock", std::process::id());
        std::env::set_var(SOCKET_ENV_VAR, &unique);
        assert_eq!(socket_path(), unique);
        std::env::remove_var(SOCKET_ENV_VAR);
    }

    #[test]
    fn default_path_is_nonempty() {
        // Guard against accidentally returning an empty string on odd envs.
        std::env::remove_var(SOCKET_ENV_VAR);
        assert!(!default_socket_path().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn unix_path_uses_xdg_when_set() {
        std::env::remove_var(SOCKET_ENV_VAR);
        std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000");
        let path = default_socket_path();
        assert!(path.starts_with("/run/user/1000/legend"), "{}", path);
        std::env::remove_var("XDG_RUNTIME_DIR");
    }

    #[cfg(windows)]
    #[test]
    fn windows_path_is_named_pipe() {
        std::env::remove_var(SOCKET_ENV_VAR);
        let path = default_socket_path();
        assert!(path.starts_with(r"\\.\pipe\legend-"), "{}", path);
    }
}
