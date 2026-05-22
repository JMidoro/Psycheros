//! Launcher-process autostart task (Windows Task Scheduler).
//!
//! The daemon is managed by the supervisor in [`super::task_scheduler`];
//! this module manages the **launcher itself** — registering it as a
//! separate user-context scheduled task so the tray icon is available
//! the moment the user logs in, independent of whether they remembered
//! to open the .exe.
//!
//! Without this, the tray icon would only appear when the user
//! manually opens Psycheros — losing the persistent "always-on" UX
//! the system-tray pattern is meant to provide.
//!
//! ## Lifecycle
//!
//! Two tasks are registered side by side after a successful daemon
//! install:
//!
//! - `Psycheros` — the Deno-based chat server (see
//!   [`super::task_scheduler::TaskSchedulerSupervisor`]).
//! - `Psycheros-Launcher` — the Tauri launcher binary, started with
//!   `--no-window` so it boots silent into tray-only mode.
//!
//! Both are installed together by the manager's Install flow and
//! uninstalled together. The launcher task uses a `<LogonTrigger>`
//! and deliberately omits `<RestartOnFailure>`: if the user picks
//! Quit Launcher from the tray we want it to stay quit until the
//! next login, not be Task-Scheduler-revived seconds later.
//!
//! ## Dev mode caveat
//!
//! `std::env::current_exe()` during `cargo tauri dev` resolves to
//! `target/debug/psycheros-launcher.exe`, not an installed `.exe`
//! under `Program Files`. Registering a task with that path is
//! technically possible but would point at a stale binary across
//! rebuilds and pin development state into the user's session. We
//! refuse the install in that case and log a note; production
//! builds install normally.

#![cfg(target_os = "windows")]

use std::fs;
use std::path::{Path, PathBuf};

use super::task_scheduler::{current_user, escape_xml, write_utf16_le_with_bom};
use super::SupervisorError;
use crate::paths;
use crate::proc::hidden_command;

/// Task name for the launcher's own scheduled task. Distinct from the
/// daemon's `Psycheros` so the two can be enabled / disabled
/// independently.
pub const LAUNCHER_TASK_LABEL: &str = "Psycheros-Launcher";

/// Resolve the launcher binary path to embed in the task XML. Returns
/// `None` when running outside an installed `.exe` (e.g. `cargo tauri
/// dev`'s `target/debug/...`) so the install can short-circuit
/// gracefully — see the module doc-comment.
fn resolve_launcher_binary() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let s = exe.to_string_lossy().to_lowercase();
    // Dev builds live under `target/debug` or `target/release` inside
    // the repo. Installed builds live under `Program Files`, `Program
    // Files (x86)`, or `AppData\Local\Psycheros\` (per-user MSI
    // install). Match the latter set positively rather than blocking
    // the former — a future packaging change shouldn't accidentally
    // disable the launcher agent.
    let installed = s.contains("\\program files\\")
        || s.contains("\\program files (x86)\\")
        || s.contains("\\appdata\\local\\psycheros\\");
    if installed {
        Some(exe)
    } else {
        None
    }
}

fn render_task_xml(launcher_bin: &Path, user: &str) -> String {
    // Invoke the launcher directly with `--no-window` — no `cmd /c`
    // wrapper. Wrapping in cmd.exe would flash a visible console
    // window at every user logon when the task fires; the launcher
    // itself is `windows_subsystem = "windows"` in release builds so
    // launching it directly is silent. The cost is that we lose
    // stdout/stderr file capture for the launcher (the daemon
    // supervisor still captures its child's logs via the runner
    // sidecar) — Tauri webview state typically doesn't need
    // post-mortem log inspection. If that changes, a future PR can
    // add a Tauri-side file logger.
    let arguments = "--no-window".to_string();

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n\
         <Task version=\"1.4\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\r\n\
         \x20 <RegistrationInfo>\r\n\
         \x20   <Description>Psycheros launcher (tray + manager surface)</Description>\r\n\
         \x20   <Author>Psycheros</Author>\r\n\
         \x20   <URI>\\{label}</URI>\r\n\
         \x20 </RegistrationInfo>\r\n\
         \x20 <Triggers>\r\n\
         \x20   <LogonTrigger>\r\n\
         \x20     <Enabled>true</Enabled>\r\n\
         \x20     <UserId>{user}</UserId>\r\n\
         \x20   </LogonTrigger>\r\n\
         \x20 </Triggers>\r\n\
         \x20 <Principals>\r\n\
         \x20   <Principal id=\"Author\">\r\n\
         \x20     <UserId>{user}</UserId>\r\n\
         \x20     <LogonType>InteractiveToken</LogonType>\r\n\
         \x20     <RunLevel>LeastPrivilege</RunLevel>\r\n\
         \x20   </Principal>\r\n\
         \x20 </Principals>\r\n\
         \x20 <Settings>\r\n\
         \x20   <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\r\n\
         \x20   <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\r\n\
         \x20   <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\r\n\
         \x20   <AllowHardTerminate>true</AllowHardTerminate>\r\n\
         \x20   <StartWhenAvailable>true</StartWhenAvailable>\r\n\
         \x20   <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>\r\n\
         \x20   <IdleSettings>\r\n\
         \x20     <StopOnIdleEnd>false</StopOnIdleEnd>\r\n\
         \x20     <RestartOnIdle>false</RestartOnIdle>\r\n\
         \x20   </IdleSettings>\r\n\
         \x20   <AllowStartOnDemand>true</AllowStartOnDemand>\r\n\
         \x20   <Enabled>true</Enabled>\r\n\
         \x20   <Hidden>false</Hidden>\r\n\
         \x20   <RunOnlyIfIdle>false</RunOnlyIfIdle>\r\n\
         \x20   <DisallowStartOnRemoteAppSession>false</DisallowStartOnRemoteAppSession>\r\n\
         \x20   <WakeToRun>false</WakeToRun>\r\n\
         \x20   <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\r\n\
         \x20   <Priority>7</Priority>\r\n\
         \x20 </Settings>\r\n\
         \x20 <Actions Context=\"Author\">\r\n\
         \x20   <Exec>\r\n\
         \x20     <Command>{bin}</Command>\r\n\
         \x20     <Arguments>{args}</Arguments>\r\n\
         \x20   </Exec>\r\n\
         \x20 </Actions>\r\n\
         </Task>\r\n",
        label = LAUNCHER_TASK_LABEL,
        user = escape_xml(user),
        bin = escape_xml(&launcher_bin.display().to_string()),
        args = escape_xml(&arguments),
    )
}

/// Whether the launcher task is currently registered.
pub fn is_installed() -> bool {
    hidden_command("schtasks.exe")
        .args(["/Query", "/TN", LAUNCHER_TASK_LABEL])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Register the launcher's scheduled task. Does NOT call `/Run` — the
/// launcher is already running (the user just clicked Install from
/// inside it), and a second invocation would race the tray icon and
/// the daemon-state probe.
///
/// At next user logon, Task Scheduler fires the LogonTrigger and the
/// launcher boots silently into tray-only mode. The currently-running
/// launcher continues serving the tray until then.
///
/// Idempotent: `/Create /F` overwrites an existing registration in
/// place.
///
/// In dev mode (binary outside an installed location), the install is
/// a no-op and the caller is notified via the returned `false`.
pub fn install() -> Result<bool, SupervisorError> {
    let Some(bin) = resolve_launcher_binary() else {
        eprintln!(
            "[launcher_agent] skipping schtasks install: current_exe() \
             isn't an installed binary (likely dev mode). The tray \
             will be available while this dev launcher runs, but won't \
             persist across logout / reboot."
        );
        return Ok(false);
    };

    fs::create_dir_all(paths::log_dir())?;

    let user = current_user()?;
    let xml = render_task_xml(&bin, &user);
    let xml_path = std::env::temp_dir().join(format!(
        "psycheros-launcher-task-{}.xml",
        std::process::id()
    ));
    write_utf16_le_with_bom(&xml_path, &xml)?;

    let out = hidden_command("schtasks.exe")
        .args(["/Create", "/XML"])
        .arg(&xml_path)
        .args(["/TN", LAUNCHER_TASK_LABEL, "/F"])
        .output()?;
    let _ = fs::remove_file(&xml_path);
    if !out.status.success() {
        return Err(SupervisorError::Command(format!(
            "schtasks /Create (launcher agent) failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(true)
}

/// Remove the launcher's scheduled task. Does NOT call `/End` — if
/// the launcher process is the one schtasks spawned at login (the
/// common post-install state), `/End` would kill the very process
/// running this command, taking the uninstall flow down with it.
///
/// Deleting the registration is enough: the launcher keeps running
/// for the user to interact with (e.g. click Install again to redo
/// it). With no LogonTrigger left and no RestartOnFailure, there's
/// no spawn pressure either.
pub fn uninstall() -> Result<(), SupervisorError> {
    if !is_installed() {
        return Ok(());
    }
    let out = hidden_command("schtasks.exe")
        .args(["/Delete", "/TN", LAUNCHER_TASK_LABEL, "/F"])
        .output()?;
    if !out.status.success() {
        return Err(SupervisorError::Command(format!(
            "schtasks /Delete (launcher agent) failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_task_xml_contains_label_and_no_window_flag() {
        let bin = PathBuf::from("C:\\Program Files\\Psycheros\\Psycheros.exe");
        let xml = render_task_xml(&bin, "PC\\me");
        assert!(xml.contains(LAUNCHER_TASK_LABEL));
        assert!(xml.contains("--no-window"));
        assert!(xml.contains("Psycheros.exe"));
        // LogonTrigger present (we want to start at login).
        assert!(xml.contains("<LogonTrigger>"));
        // Deliberately NO RestartOnFailure — user-quit should stick.
        assert!(!xml.contains("<RestartOnFailure>"));
    }

    #[test]
    fn render_task_xml_invokes_launcher_directly() {
        let bin = PathBuf::from("C:\\Program Files\\Psycheros\\Psycheros.exe");
        let xml = render_task_xml(&bin, "PC\\me");
        // The launcher .exe is the action's Command — no cmd.exe
        // wrapper, no shell. Wrapping in cmd would flash a console
        // window at user logon. The launcher itself is
        // `windows_subsystem = "windows"` in release so launching it
        // directly is silent.
        assert!(xml.contains(&format!(
            "<Command>{}</Command>",
            escape_xml(&bin.display().to_string())
        )));
        assert!(xml.contains("<Arguments>--no-window</Arguments>"));
        assert!(
            !xml.contains("cmd.exe"),
            "must not wrap in cmd.exe — would flash a console window at logon"
        );
    }

    #[test]
    fn resolve_launcher_binary_recognizes_installed_paths() {
        // Path-pattern coverage. We can't actually drive current_exe()
        // in a unit test, but we can document + assert the install
        // detection logic via the same string check.
        let cases = [
            ("C:\\Program Files\\Psycheros\\Psycheros.exe", true),
            ("C:\\Program Files (x86)\\Psycheros\\Psycheros.exe", true),
            (
                "C:\\Users\\me\\AppData\\Local\\Psycheros\\Psycheros.exe",
                true,
            ),
            (
                "C:\\Users\\me\\dev\\Psycheros-staging\\packages\\launcher-v2\\src-tauri\\target\\debug\\psycheros-launcher.exe",
                false,
            ),
            (
                "C:\\Users\\me\\repos\\launcher-v2\\src-tauri\\target\\release\\psycheros-launcher.exe",
                false,
            ),
        ];
        for (path, expected) in cases {
            let s = path.to_lowercase();
            let installed = s.contains("\\program files\\")
                || s.contains("\\program files (x86)\\")
                || s.contains("\\appdata\\local\\psycheros\\");
            assert_eq!(installed, expected, "mismatch for {path}");
        }
    }
}
