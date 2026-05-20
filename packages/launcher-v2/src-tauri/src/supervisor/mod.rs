//! OS service-supervisor abstraction.
//!
//! The launcher delegates daemon lifecycle (install, start, restart-on-crash,
//! uninstall) to the OS-native service supervisor — launchd on macOS,
//! systemd-user on Linux, Task Scheduler on Windows. The launcher itself
//! never owns the daemon process; that decoupling is the whole point of
//! the v2 architecture (see docs/architecture.md).
//!
//! ## Trait surface
//!
//! All three supervisor implementations share the [`ServiceSupervisor`]
//! trait. The trait deliberately exposes the smallest possible surface —
//! enough for the launcher's UI to drive install/uninstall and observe
//! state, but not enough to leak per-OS quirks into the manager surface.
//!
//! ## Default supervisor
//!
//! [`DefaultSupervisor`] is the type alias for the current OS's impl,
//! selected at compile time. The frontend never knows which it is.

use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

use crate::config::DaemonMode;

#[cfg(target_os = "macos")]
mod launchd;

#[cfg(target_os = "macos")]
pub mod launcher_agent;

#[cfg(target_os = "linux")]
mod systemd;

#[cfg(target_os = "windows")]
mod task_scheduler;

#[cfg(target_os = "macos")]
pub use launchd::LaunchdSupervisor as DefaultSupervisor;

#[cfg(target_os = "linux")]
pub use systemd::SystemdUserSupervisor as DefaultSupervisor;

#[cfg(target_os = "windows")]
pub use task_scheduler::TaskSchedulerSupervisor as DefaultSupervisor;

// ============================================================================
// Public surface
// ============================================================================

/// Inputs needed to register the daemon with the OS supervisor.
///
/// Constructed by `daemon::lifecycle` from the user's persisted config plus
/// the launcher's bundled paths. The supervisor turns this into the OS-native
/// service definition (plist / unit file / scheduled task).
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Reverse-DNS label used by the OS supervisor to identify the service.
    pub label: String,
    /// Absolute path to the bundled Deno binary (or system Deno in dev).
    pub deno_path: PathBuf,
    /// Path to the psycheros source bundle (where `src/main.ts` lives).
    /// Set as the service's working directory + the value of psycheros's
    /// `projectRoot`.
    pub source_dir: PathBuf,
    /// Path to user-mutable runtime state. Passed to the daemon as
    /// `PSYCHEROS_DATA_DIR` — see psycheros's PSYCHEROS_DATA_DIR refactor.
    pub data_dir: PathBuf,
    /// Where stdout/stderr land. The manager's log viewer tails these.
    pub log_dir: PathBuf,
    /// HTTP port the daemon binds to. Default 3000.
    pub port: u16,
    /// Optional path to entity-core source (for `PSYCHEROS_ENTITY_CORE_PATH`).
    /// When None, psycheros falls back to its sibling-package convention.
    pub entity_core_dir: Option<PathBuf>,
    /// Optional override for entity-core's data directory.
    /// When None, defaults to `<data_dir>/entity-core/data`.
    pub entity_core_data_dir: Option<PathBuf>,
}

/// Best-effort runtime info about the OS-supervised daemon, parsed out of
/// the supervisor's native list/status output. Used by the manager's
/// diagnostics card.
///
/// All fields are `Option` because supervisors expose different things:
/// launchd reports `PID` when running and `LastExitStatus` when not;
/// systemd's `systemctl --user show` is similar; Task Scheduler's
/// equivalent is sparser. When something isn't available, we render
/// "—" in the UI rather than guessing.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeInfo {
    /// PID of the currently-running daemon process, if any.
    pub pid: Option<u32>,
    /// Exit status of the last terminated invocation (zero on clean
    /// shutdown, non-zero on crash). On launchd this comes from the
    /// `LastExitStatus` field of `launchctl list <label>`.
    pub last_exit_status: Option<i32>,
}

/// Errors a supervisor can return. All variants are user-presentable.
#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("supervisor command failed: {0}")]
    Command(String),
    #[error("service definition is malformed: {0}")]
    Malformed(String),
    #[error("not yet implemented on this platform")]
    NotImplemented,
}

/// What the manager surface can do with the daemon's OS-supervisor record.
///
/// Implementations must be idempotent: `install_autostart`/`install_manual`
/// when already installed succeed (overwrite), `uninstall` when not
/// installed succeeds. This makes the manager UI tolerant of inconsistent
/// on-disk state (e.g. plist exists but isn't loaded).
pub trait ServiceSupervisor: Send + Sync {
    /// Register the service in autostart mode — runs at every login and
    /// auto-restarts on crash. Starts the daemon immediately.
    fn install_autostart(&self, cfg: &DaemonConfig) -> Result<(), SupervisorError>;

    /// Register the service in manual mode — loaded into the supervisor
    /// but does not run at login and does not auto-restart on crash.
    /// User drives via `start_daemon` / `stop_daemon`. Starts the daemon
    /// immediately on install (the user just clicked Install — they
    /// probably want it on right now).
    fn install_manual(&self, cfg: &DaemonConfig) -> Result<(), SupervisorError>;

    /// Unregister the service. Stops the daemon as a side effect. Works
    /// for both autostart and manual modes.
    fn uninstall(&self) -> Result<(), SupervisorError>;

    /// Whether the service definition (e.g. plist file) is present on
    /// disk — installed but possibly stopped. Distinct from `is_loaded`:
    /// an autostart user who clicked "Stop" has `is_installed=true` but
    /// `is_loaded=false`.
    fn is_installed(&self) -> bool;

    /// Whether the service is currently loaded into the OS supervisor
    /// (i.e., its records are active in launchd/systemd/Task Scheduler).
    /// Independent of whether the daemon process is actually running —
    /// see `daemon::status` for the combined view.
    fn is_loaded(&self) -> bool;

    /// Start the daemon. Loads the service if it isn't loaded, then
    /// kicks it off. Idempotent — no-op if the daemon is already
    /// running. Works for both modes (autostart-mode Start after a
    /// user-initiated Stop is a normal flow).
    fn start_daemon(&self) -> Result<(), SupervisorError>;

    /// Stop the daemon. In autostart mode, uses a session-scoped unload
    /// so the daemon comes back at next login without flipping the
    /// persistent enable state. In manual mode, the same operation
    /// works because manual mode also doesn't persistently disable.
    /// Idempotent — no-op if already stopped.
    fn stop_daemon(&self) -> Result<(), SupervisorError>;

    /// Restart the daemon process while keeping the service registration
    /// intact. Used after a source update so the daemon picks up new
    /// code without uninstalling/reinstalling autostart.
    ///
    /// No-op (success) when the service isn't registered or isn't
    /// loaded — callers don't have to gate on `is_loaded()`.
    fn restart(&self) -> Result<(), SupervisorError>;

    /// Paths to stdout/stderr log files. The manager surface tails these.
    fn log_paths(&self) -> Vec<PathBuf>;

    /// Service identifier (label / unit name / task name) the supervisor
    /// uses. Surfaced in diagnostics + the manager's "Service info" view.
    fn label(&self) -> &str;

    /// Best-effort PID + last-exit-status, parsed from the OS supervisor's
    /// native status output. Never returns an error — when the supervisor
    /// command fails or the fields are absent, returns the defaulted
    /// `RuntimeInfo { pid: None, last_exit_status: None }`. This keeps
    /// the diagnostics card render path simple (no error branch).
    fn query_runtime_info(&self) -> RuntimeInfo {
        RuntimeInfo::default()
    }

    /// Update the on-disk service definition's mode (autostart vs manual)
    /// **without restarting the daemon**. The OS picks up the new content
    /// at the next session load (next login on macOS), which is
    /// precisely when "autostart at login" matters. The currently-running
    /// daemon is left untouched — no `launchctl unload/load` cycle, no
    /// dropped HTTP connections, no entity-core MCP teardown, no cascade
    /// of reconnect logic in the chat surface. The trade-off is that
    /// `KeepAlive` (autostart's crash-restart behavior) takes effect
    /// only at the next daemon start; for the common "set and forget"
    /// use case that's fine.
    ///
    /// Default implementation returns `NotImplemented` so non-macOS
    /// supervisor stubs don't need to add a body.
    fn set_mode_only(&self, _cfg: &DaemonConfig, _mode: DaemonMode) -> Result<(), SupervisorError> {
        Err(SupervisorError::NotImplemented)
    }
}

/// Construct the default supervisor for this OS.
///
/// All supervisors are stateless w.r.t. their own data — they re-read the
/// OS supervisor's state on every call rather than caching. This keeps
/// the manager UI's state in lockstep with what the OS thinks.
pub fn default_supervisor() -> DefaultSupervisor {
    DefaultSupervisor::new()
}
