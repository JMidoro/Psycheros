//! Cross-platform path resolution for everything the launcher reads/writes.
//!
//! Conventions (via the `dirs` crate):
//!
//! | OS      | App data dir                                    |
//! | ------- | ----------------------------------------------- |
//! | macOS   | `~/Library/Application Support/Psycheros/`      |
//! | Linux   | `~/.local/share/Psycheros/`                     |
//! | Windows | `%APPDATA%\Psycheros\`                          |
//!
//! Inside `<launcher_data_dir>/`:
//! - `config.json` — user preferences (install path, port, autostart toggle)
//! - `source/` — extracted release bundle (psycheros + entity-core)
//! - `data/` — `PSYCHEROS_DATA_DIR` target — runtime entity state
//! - `bin/deno` — bundled Deno copied here on first run (stable path the
//!   service definition can reference)
//! - `logs/` — daemon stdout/stderr (launchd / Task Scheduler write here;
//!   systemd uses the journal)
//! - `cache/` — vec0 extension + any other regenerable caches (deferred
//!   to a future psycheros refactor; see docs/source-provisioning.md)

use std::path::PathBuf;

const APP_DIR_NAME: &str = "Psycheros";

/// Env var that overrides the launcher data dir. Tests set this to a
/// tempdir so they don't touch the user's real `~/Library/Application
/// Support/Psycheros/`. Production code leaves it unset and everything
/// resolves via the OS-conventional path below.
///
/// macOS's `dirs::data_dir()` calls `NSSearchPathForDirectoriesInDomains`
/// rather than honoring `$HOME`, so a `HOME=` redirect doesn't work for
/// hermetic tests — the env override is what makes the whole `paths`
/// module testable without per-test mocking.
const LAUNCHER_DATA_DIR_ENV: &str = "PSYCHEROS_LAUNCHER_DATA_DIR";

/// Env var that overrides the user's Downloads directory. Used by
/// `backup_data` so tests can land the backup zip in a tempdir rather
/// than the real `~/Downloads/`.
pub const DOWNLOAD_DIR_ENV: &str = "PSYCHEROS_DOWNLOAD_DIR";

/// The launcher's app-data root. Lazily created on first call.
pub fn launcher_data_dir() -> PathBuf {
    if let Ok(override_dir) = std::env::var(LAUNCHER_DATA_DIR_ENV) {
        return PathBuf::from(override_dir);
    }
    let base = dirs::data_dir().unwrap_or_else(|| dirs::home_dir().expect("HOME unresolvable"));
    base.join(APP_DIR_NAME)
}

/// Resolve the user's Downloads directory, honoring [`DOWNLOAD_DIR_ENV`]
/// in test contexts. Returns `None` when neither the override nor the
/// OS-conventional lookup yields a path — same fallback semantics as
/// `dirs::download_dir()` alone.
pub fn download_dir() -> Option<PathBuf> {
    if let Ok(override_dir) = std::env::var(DOWNLOAD_DIR_ENV) {
        return Some(PathBuf::from(override_dir));
    }
    dirs::download_dir()
}

/// Where `config.json` lives.
pub fn config_path() -> PathBuf {
    launcher_data_dir().join("config.json")
}

/// Where the extracted psycheros source bundle lives. The daemon's
/// `projectRoot` resolves to this directory.
pub fn source_dir() -> PathBuf {
    launcher_data_dir()
        .join("source")
        .join("packages")
        .join("psycheros")
}

/// Where the entity-core source lives within the extracted bundle.
pub fn entity_core_source_dir() -> PathBuf {
    launcher_data_dir()
        .join("source")
        .join("packages")
        .join("entity-core")
}

/// Where user-mutable runtime state lives — the value of `PSYCHEROS_DATA_DIR`.
pub fn data_dir() -> PathBuf {
    launcher_data_dir().join("data")
}

/// Where entity-core's data lives — the value of `PSYCHEROS_ENTITY_CORE_DATA_DIR`.
pub fn entity_core_data_dir() -> PathBuf {
    launcher_data_dir().join("data").join("entity-core")
}

/// Where the bundled Deno binary is copied to on first run. The OS supervisor's
/// service definition references this stable path, NOT the binary inside the
/// .app/.exe bundle (which moves on auto-update).
pub fn bundled_deno_path() -> PathBuf {
    let mut p = launcher_data_dir().join("bin").join("deno");
    if cfg!(target_os = "windows") {
        p.set_extension("exe");
    }
    p
}

/// Windows-only: stable path for the `psycheros-daemon-runner` sidecar.
/// Staged from the launcher's resources at install time so the Task
/// Scheduler action references a path that survives launcher
/// auto-update (the runner inside the .exe bundle moves on each
/// upgrade). See `bundle::stage_bundled_binary`.
#[cfg(target_os = "windows")]
pub fn bundled_runner_path() -> PathBuf {
    launcher_data_dir()
        .join("bin")
        .join("psycheros-daemon-runner.exe")
}

/// Where daemon logs go. Created by the OS supervisor on first start.
pub fn log_dir() -> PathBuf {
    launcher_data_dir().join("logs")
}
