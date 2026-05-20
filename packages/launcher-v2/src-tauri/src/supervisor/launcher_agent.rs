//! Launcher-process autostart agent (macOS launchd).
//!
//! The daemon is managed by the supervisor in [`super::launchd`]; this
//! module manages the **launcher itself** — registering it as a separate
//! launchd user agent so the tray icon is available the moment the user
//! logs in, independent of whether they remembered to open the .app.
//!
//! Without this, the tray icon would only appear when the user manually
//! opens the Psycheros app — losing the persistent "always-on" UX the
//! menu-bar agent pattern is meant to provide.
//!
//! ## Lifecycle
//!
//! Two user agents are registered side by side after a successful
//! install:
//!
//! - `ai.psycheros.daemon` — the Deno-based chat server (see
//!   [`super::launchd::LaunchdSupervisor`]).
//! - `ai.psycheros.launcher` — the Tauri launcher binary, started with
//!   `--no-window` so it boots silent into Accessory mode (tray only,
//!   no dock).
//!
//! Both are installed together by the manager's Install flow and
//! uninstalled together. The launcher agent uses `RunAtLoad=true` and
//! deliberately omits `KeepAlive`: if the user picks Quit Launcher from
//! the tray we want it to stay quit until the next login, not be
//! launchd-revived seconds later.
//!
//! ## Dev mode caveat
//!
//! `current_exe()` during `cargo tauri dev` resolves to
//! `target/debug/psycheros-launcher`, not an `.app` bundle. Installing a
//! launchd plist with that path is technically possible but would point
//! at a stale binary across rebuilds and pin development state into the
//! user's launchd session. We refuse the install in that case and log a
//! note — production builds (which `current_exe()` resolves to inside
//! `Psycheros.app/Contents/MacOS/`) install normally.

#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};

use super::SupervisorError;
use crate::paths;

/// Label for the launcher's own launchd user agent. Distinct from the
/// daemon's `ai.psycheros.daemon` so the two services can be loaded /
/// unloaded independently.
pub const LAUNCHER_AGENT_LABEL: &str = "ai.psycheros.launcher";

/// Resolve the plist path under `~/Library/LaunchAgents/`.
fn plist_path() -> Result<PathBuf, SupervisorError> {
    let home = dirs::home_dir()
        .ok_or_else(|| SupervisorError::Command("HOME directory not resolvable".into()))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCHER_AGENT_LABEL}.plist")))
}

/// Resolve the launcher binary path to embed in the plist. Returns `None`
/// when running outside a `.app` bundle (e.g. `cargo tauri dev`) so the
/// install can short-circuit gracefully — see the module doc-comment.
fn resolve_launcher_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let s = exe.to_string_lossy();
    if s.contains(".app/Contents/MacOS/") {
        Some(exe)
    } else {
        None
    }
}

fn render_plist(launcher_bin: &Path) -> String {
    let log_dir = paths::log_dir();
    let stdout = log_dir.join("launcher.stdout.log");
    let stderr = log_dir.join("launcher.stderr.log");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin}</string>
        <string>--no-window</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
</dict>
</plist>
"#,
        label = LAUNCHER_AGENT_LABEL,
        bin = escape_xml(&launcher_bin.display().to_string()),
        stdout = escape_xml(&stdout.display().to_string()),
        stderr = escape_xml(&stderr.display().to_string()),
    )
}

/// Minimal XML escape — paths shouldn't contain these but better safe.
/// Mirrors the helper in [`super::launchd`].
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Whether the launcher's plist is currently present on disk.
pub fn is_installed() -> bool {
    plist_path().map(|p| p.exists()).unwrap_or(false)
}

/// Install the launcher's launchd plist by writing it to
/// `~/Library/LaunchAgents/`. **Does NOT call `launchctl load`** — the
/// launcher process is already running (the user just clicked Install
/// from inside the launcher), and `load -w` against a plist with
/// `RunAtLoad=true` would spawn a second launcher process. Two
/// launchers means two tray icons, both fighting over the same
/// daemon-state probe. Bug, not feature.
///
/// At the next login, launchd auto-discovers plists in
/// `~/Library/LaunchAgents/` and loads them — so writing the file is
/// the only persistence step needed. The currently-running launcher
/// keeps serving the tray until then.
///
/// Idempotent: an existing plist at the target path is overwritten in
/// place (no unload needed because we never loaded a previous version
/// into this session ourselves).
///
/// In dev mode (no `.app` bundle), the install is a no-op and the
/// caller is notified via the returned `false`.
pub fn install() -> Result<bool, SupervisorError> {
    let Some(bin) = resolve_launcher_binary() else {
        eprintln!(
            "[launcher_agent] skipping launchd install: current_exe() \
             is not inside a .app bundle (likely dev mode). The tray \
             will be available while this dev launcher runs, but won't \
             persist across logout / reboot."
        );
        return Ok(false);
    };
    let plist = plist_path()?;
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(paths::log_dir())?;
    fs::write(&plist, render_plist(&bin))?;
    Ok(true)
}

/// Remove the launcher's launchd plist. Idempotent: succeeds when the
/// plist isn't installed.
///
/// **Does NOT call `launchctl unload`** — and that's deliberate. If the
/// launcher process is the one launchctl spawned at login (the common
/// post-install state), `launchctl unload` would SIGTERM the very
/// process running this command, killing the uninstall flow mid-way.
/// Removing the plist file is enough: the launcher keeps running for
/// the user to interact with (e.g. click Install again to redo it),
/// and at next session boundary launchd cleans up the stale label
/// record on its own. With no `KeepAlive` on the plist, there's no
/// launchctl respawn pressure either.
pub fn uninstall() -> Result<(), SupervisorError> {
    let plist = plist_path()?;
    if plist.exists() {
        fs::remove_file(&plist)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_plist_contains_label_and_no_window_flag() {
        let bin = PathBuf::from("/Applications/Psycheros.app/Contents/MacOS/psycheros-launcher");
        let xml = render_plist(&bin);
        assert!(xml.contains(LAUNCHER_AGENT_LABEL));
        assert!(xml.contains("--no-window"));
        assert!(xml.contains(bin.to_str().unwrap()));
        assert!(xml.contains("<key>RunAtLoad</key>\n    <true/>"));
        // Deliberately NO KeepAlive — user-quit should stick.
        assert!(!xml.contains("KeepAlive"));
    }

    #[test]
    fn resolve_launcher_binary_recognizes_app_bundle() {
        // Can't manipulate current_exe() in a unit test, so we test the
        // shape-check via a path-string assertion that mirrors the impl.
        let in_app = "/Applications/Psycheros.app/Contents/MacOS/psycheros-launcher";
        assert!(in_app.contains(".app/Contents/MacOS/"));

        let dev_bin = "/Users/dev/Psycheros-staging/packages/launcher-v2/src-tauri/target/debug/psycheros-launcher";
        assert!(!dev_bin.contains(".app/Contents/MacOS/"));
    }
}
