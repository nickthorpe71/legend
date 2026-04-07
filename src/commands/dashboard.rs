use crate::cli::{parse_args, CommandDef};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Dispatch entry point used by the COMMANDS registry.
pub fn handle_dashboard_dispatch(
    args: &[String],
    def: &CommandDef,
) -> Result<(), Box<dyn std::error::Error>> {
    let parsed = parse_args(args, def);
    if parsed.has("3d") {
        handle_dashboard()
    } else {
        crate::tui::run_tui()
    }
}

/// Launch the Legend dashboard.
///
/// Detects WSL and converts the project path to a Windows UNC path so the
/// dashboard .exe (compiled for Windows) can find the `.legend/` data.
/// Uses `cmd.exe /C start` to fully detach from WSL's file descriptors,
/// avoiding the "read failed 5: I/O error" spam.
pub fn handle_dashboard() -> Result<(), Box<dyn std::error::Error>> {
    let project_dir =
        std::env::current_dir().map_err(|e| format!("Cannot get current directory: {}", e))?;

    let exe_path = find_dashboard_exe(&project_dir)?;
    let win_project_dir = to_windows_path(&project_dir);

    println!("🚀 Launching Legend Dashboard...");

    if is_wsl() {
        // Convert the exe path to a Windows path too
        let win_exe = to_windows_path(&exe_path);
        println!("   exe: {}", win_exe);
        println!("   project: {}", win_project_dir);

        // Launch via cmd.exe /C start — fully detached from WSL fds
        Command::new("cmd.exe")
            .args([
                "/C",
                "start",
                "", // window title (empty)
                &win_exe,
                "--project-dir",
                &win_project_dir,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to launch dashboard: {}", e))?;
    } else {
        println!("   exe: {}", exe_path.display());
        Command::new(&exe_path)
            .args(["--project-dir", &win_project_dir])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to launch dashboard: {}", e))?;
    }

    println!("✓ Dashboard launched");
    Ok(())
}

/// Locate the dashboard executable.
/// Checks: dashboard/target/x86_64-pc-windows-gnu/release, then debug.
/// Falls back to native linux binary if not in WSL.
fn find_dashboard_exe(project_dir: &Path) -> Result<PathBuf, String> {
    let candidates = if is_wsl() {
        vec![
            project_dir.join("dashboard/target/x86_64-pc-windows-gnu/release/legend-dashboard.exe"),
            project_dir.join("dashboard/target/x86_64-pc-windows-gnu/debug/legend-dashboard.exe"),
        ]
    } else {
        vec![
            project_dir.join("dashboard/target/release/legend-dashboard"),
            project_dir.join("dashboard/target/debug/legend-dashboard"),
        ]
    };

    for path in &candidates {
        if path.exists() {
            return Ok(path.clone());
        }
    }

    Err("Dashboard executable not found. Build it first:\n  cd dashboard && cargo build --release --target x86_64-pc-windows-gnu".to_string())
}

/// Convert a Linux path to a Windows UNC path for WSL.
/// `/home/user/project` → `\\wsl.localhost\<distro>\home\user\project`
fn to_windows_path(linux_path: &Path) -> String {
    if !is_wsl() {
        return linux_path.to_string_lossy().to_string();
    }

    let distro = wsl_distro_name();
    let path_str = linux_path.to_string_lossy();
    format!(r"\\wsl.localhost\{}{}", distro, path_str.replace('/', r"\"))
}

/// Detect if running inside WSL.
fn is_wsl() -> bool {
    std::fs::read_to_string("/proc/version")
        .map(|v| v.contains("microsoft") || v.contains("Microsoft"))
        .unwrap_or(false)
}

/// Get the WSL distribution name.
fn wsl_distro_name() -> String {
    std::env::var("WSL_DISTRO_NAME").unwrap_or_else(|_| "Ubuntu".to_string())
}
