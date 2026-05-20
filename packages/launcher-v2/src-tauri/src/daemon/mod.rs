//! Daemon detection + control.
//!
//! The launcher never owns the daemon process. It only **observes** the
//! daemon's state (via TCP probe + supervisor query) and **directs** the OS
//! supervisor to start/stop it. This separation is the architectural
//! foundation of the v2 launcher — see docs/architecture.md.
//!
//! Modules:
//! - [`status`] — point-in-time daemon state probe
//! - [`navigation`] — webview navigation helper that respects user-summon state

pub mod navigation;
pub mod status;

pub use status::{probe, DaemonState, DaemonStatus};

/// Fallback port when `config.json` is missing or malformed. The
/// effective port is `LauncherConfig.port` — see [`crate::config`] — and
/// flows through [`status::probe`] and `build_daemon_config` to keep the
/// probe, the supervisor install, and the backup/restore HTTP calls in
/// agreement. 3000 is psycheros's longstanding default for fresh
/// installs.
pub const DAEMON_PORT: u16 = 3000;
