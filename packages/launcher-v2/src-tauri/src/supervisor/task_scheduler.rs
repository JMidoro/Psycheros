//! Windows Task Scheduler supervisor (stub — see docs/supervisors.md).
//!
//! This impl is a stub. The next implementer should:
//!
//! 1. Use `schtasks.exe` or PowerShell's `Register-ScheduledTask` cmdlet
//!    to create a task that fires on user logon. Set the action to
//!    `<deno_path> run -A src\main.ts` with working directory `<source_dir>`
//!    and environment variables passed via the task's properties.
//! 2. Restart-on-failure: configure the task's settings with
//!    `RestartCount=3`, `RestartInterval=PT5S` (the equivalent of launchd
//!    KeepAlive, weaker but present).
//! 3. `is_loaded()`: `schtasks /query /tn Psycheros /fo csv /nh` exit 0 =
//!    registered.
//! 4. `uninstall()`: `schtasks /delete /tn Psycheros /f`.
//!
//! ## The weaker supervision tradeoff
//!
//! Task Scheduler's "restart on failure" is genuinely less robust than
//! launchd's `KeepAlive` or systemd's `Restart=on-failure`. There's no
//! equivalent to launchd's throttling-after-crashloop. The manager surface
//! should compensate by polling daemon status more aggressively on Windows
//! and surfacing "daemon stopped" states with a manual restart button
//! more prominently than on macOS/Linux.
//!
//! ## Logs
//!
//! Windows has no `journalctl`. The daemon's stdout/stderr must be
//! redirected at the Task Scheduler level — set the action's program to
//! `cmd.exe /c <deno> ... 1> stdout.log 2> stderr.log` or wrap in a
//! PowerShell launcher script. See docs/supervisors.md for the chosen
//! approach.
//!
//! ## SmartScreen
//!
//! Unsigned `.exe`/`.msi` triggers SmartScreen warnings. Documented
//! workaround in docs/release.md: right-click → Properties → Unblock,
//! then "More info → Run anyway." Same posture as macOS Gatekeeper
//! workaround.

use std::path::PathBuf;

use super::{DaemonConfig, ServiceSupervisor, SupervisorError};

/// Windows supervisor stub backed by Task Scheduler. Every mutating
/// method returns [`SupervisorError::NotImplemented`] until the
/// PowerShell `Register-ScheduledTask` integration lands — see
/// [`docs/supervisors.md`](../../docs/supervisors.md) for the planned
/// shape.
pub struct TaskSchedulerSupervisor {
    label: String,
}

impl TaskSchedulerSupervisor {
    /// Construct a supervisor bound to the canonical `Psycheros` task
    /// name. Stub today; the label is the only piece of state the final
    /// impl will need.
    pub fn new() -> Self {
        Self {
            label: "Psycheros".to_string(),
        }
    }
}

impl Default for TaskSchedulerSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceSupervisor for TaskSchedulerSupervisor {
    fn install_autostart(&self, _cfg: &DaemonConfig) -> Result<(), SupervisorError> {
        Err(SupervisorError::NotImplemented)
    }

    fn install_manual(&self, _cfg: &DaemonConfig) -> Result<(), SupervisorError> {
        Err(SupervisorError::NotImplemented)
    }

    fn uninstall(&self) -> Result<(), SupervisorError> {
        Err(SupervisorError::NotImplemented)
    }

    fn is_installed(&self) -> bool {
        false
    }

    fn is_loaded(&self) -> bool {
        false
    }

    fn start_daemon(&self) -> Result<(), SupervisorError> {
        Err(SupervisorError::NotImplemented)
    }

    fn stop_daemon(&self) -> Result<(), SupervisorError> {
        Err(SupervisorError::NotImplemented)
    }

    fn restart(&self) -> Result<(), SupervisorError> {
        Err(SupervisorError::NotImplemented)
    }

    fn log_paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn label(&self) -> &str {
        &self.label
    }
}
