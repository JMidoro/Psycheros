//! Daemon state probe.
//!
//! Daemon state is the **intersection** of three signals:
//! - Is the port bound? (TCP probe to `127.0.0.1:PORT`)
//! - Is the service loaded in the OS supervisor? (`launchctl list`)
//! - Is the service installed at all? (plist file on disk)
//!
//! The three-signal model lets the manager surface render four
//! cases plus one transient:
//!
//! | installed | loaded | port up | state          | meaning                          |
//! | --------- | ------ | ------- | -------------- | -------------------------------- |
//! | no        | no     | no      | `NotInstalled` | offer install                    |
//! | yes       | no     | no      | `Stopped`      | user stopped it; offer start     |
//! | yes       | yes    | no      | `Installed`    | mid-boot or crashlooping         |
//! | yes/no    | _      | yes     | `Running`      | daemon is responding             |
//!
//! "Stopped" is distinct from "NotInstalled": the user picked Stop, but
//! the service definition is still on disk. At next login (autostart
//! mode) or next manual Start (either mode), the daemon comes back.
//!
//! Port-bound implies `Running` regardless of supervisor state, so a user
//! who likes `deno task start` from a terminal still gets a working chat
//! UI — the launcher just doesn't try to install over their existing run.
//!
//! The "is the port bound?" signal isn't a raw TCP connect — it's an HTTP
//! `GET /health` against the daemon's health endpoint. The launcher
//! treats the port as Psycheros-occupied only when the response body
//! carries the `"name":"psycheros"` signature. This disambiguates
//! against unrelated services that happen to bind the same port (a
//! different project's dev server, a misconfigured `python -m http.server`,
//! etc.) — those used to falsely flip the launcher to `Running` and break
//! the manager's UX.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::Serialize;

use super::DAEMON_PORT;
use crate::config;
use crate::supervisor::{default_supervisor, ServiceSupervisor};

/// Combined daemon lifecycle state — what the manager UI renders against.
/// Derived in [`probe`] from the intersection of three signals (service
/// definition on disk, service loaded in the OS supervisor, HTTP /health
/// answering on the configured port). Serialized over the IPC boundary
/// in kebab-case so the frontend matches `"not-installed"`, `"stopped"`,
/// etc. Keep the variant order stable — the frontend's manager.js maps
/// each variant to per-state copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonState {
    /// No service definition on disk, no daemon running. Offer install.
    NotInstalled,
    /// Service installed (plist file present) but not loaded — user
    /// chose Stop, or autostart user stopped for this session.
    Stopped,
    /// Service loaded but port not yet bound. Booting (~5-10s after
    /// install) or crashlooping (manager should surface logs).
    Installed,
    /// Port bound — daemon is responding. Whether the service is
    /// registered doesn't matter at this point; chat just works.
    Running,
}

impl DaemonState {
    /// Stable kebab-case string form. Matches the `serde(rename_all)`
    /// above so JSON-serialized and Rust-borrowed forms agree, and
    /// gives diagnostics callers a `&'static str` without going through
    /// serde.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotInstalled => "not-installed",
            Self::Stopped => "stopped",
            Self::Installed => "installed",
            Self::Running => "running",
        }
    }
}

/// Payload of `daemon-status-changed` events and the
/// [`daemon_status`](crate::commands::daemon_status) IPC return type.
/// All four fields are exposed to the frontend so the manager can
/// render diagnostic context alongside the headline state.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DaemonStatus {
    /// The combined lifecycle state — see [`DaemonState`].
    pub state: DaemonState,
    /// The port the probe targeted. Matches `LauncherConfig.port` on
    /// the current config (with [`DAEMON_PORT`] as the fallback).
    pub port: u16,
    /// `launchctl list <label>` (or equivalent) reported the service
    /// as loaded.
    pub supervisor_loaded: bool,
    /// The service definition file (plist / unit / task) is present
    /// on disk.
    pub supervisor_installed: bool,
}

/// Point-in-time probe. Cheap (<500ms worst case, typically <20ms), safe
/// to call on a watcher loop every few seconds.
///
/// Reads the user-configured port from `config.json` on every call so the
/// probe and the daemon install agree even after the user changes the
/// port. A missing or malformed config falls back to [`DAEMON_PORT`].
pub fn probe() -> DaemonStatus {
    let port = config::load().map(|c| c.port).unwrap_or(DAEMON_PORT);
    let port_up = psycheros_is_listening(port);
    let supervisor = default_supervisor();
    let supervisor_loaded = supervisor.is_loaded();
    let supervisor_installed = supervisor.is_installed();

    let state = match (supervisor_installed, supervisor_loaded, port_up) {
        (_, _, true) => DaemonState::Running,
        (true, true, false) => DaemonState::Installed,
        (true, false, false) => DaemonState::Stopped,
        (false, _, false) => DaemonState::NotInstalled,
    };

    DaemonStatus {
        state,
        port,
        supervisor_loaded,
        supervisor_installed,
    }
}

/// Probe timeout for the whole connect + write + read cycle. 500ms is
/// well above a healthy daemon's response time (single-digit ms locally)
/// while keeping the watcher loop responsive when the port is held by
/// something else.
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Response-body byte cap. `/health` returns ~300 bytes; capping at 2KB
/// covers comfortable future growth without inviting accidental DoS via
/// a misbehaving server on the same port.
const PROBE_READ_CAP: u64 = 2048;

/// Returns `true` only when the port is bound AND the listener identifies
/// itself as Psycheros via the `/health` endpoint. Anything else (raw TCP
/// listener, different HTTP server, slow response) returns `false` so the
/// state machine falls back to the supervisor-derived signals.
fn psycheros_is_listening(port: u16) -> bool {
    let Ok(addr) = format!("127.0.0.1:{port}").parse::<SocketAddr>() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT) else {
        return false;
    };
    if stream.set_read_timeout(Some(PROBE_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(PROBE_TIMEOUT)).is_err()
    {
        return false;
    }

    let request = format!(
        "GET /health HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         User-Agent: psycheros-launcher\r\n\
         Connection: close\r\n\
         \r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut buf = Vec::with_capacity(512);
    if stream.take(PROBE_READ_CAP).read_to_end(&mut buf).is_err() {
        return false;
    }
    is_psycheros_response(&buf)
}

/// Pure response-shape check — split out so unit tests can cover the
/// recognition logic without spinning up an HTTP listener. A real
/// Psycheros health response is JSON with `"name":"psycheros"` in the
/// body, served with a `200 OK` status line.
fn is_psycheros_response(raw: &[u8]) -> bool {
    let text = String::from_utf8_lossy(raw);
    text.starts_with("HTTP/1.1 200") && text.contains("\"name\":\"psycheros\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_real_psycheros_health_response() {
        let body = r#"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{"status":"ok","name":"psycheros","version":"0.3.3"}"#;
        assert!(is_psycheros_response(body.as_bytes()));
    }

    #[test]
    fn rejects_non_200_status() {
        let body = r#"HTTP/1.1 404 Not Found\r\n\r\n{"name":"psycheros"}"#;
        assert!(!is_psycheros_response(body.as_bytes()));
    }

    #[test]
    fn rejects_response_without_psycheros_signature() {
        // A different service answering on port 3000 — say, a Node dev
        // server returning a plain "ok".
        let body = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nok";
        assert!(!is_psycheros_response(body.as_bytes()));
    }

    #[test]
    fn rejects_psycheros_word_in_unrelated_response() {
        // A different service that mentions psycheros in passing (e.g.,
        // logs, error messages) — the signature requires the exact JSON
        // key shape, not just the word.
        let body = "HTTP/1.1 200 OK\r\n\r\nWelcome to my-psycheros-clone server";
        assert!(!is_psycheros_response(body.as_bytes()));
    }

    #[test]
    fn rejects_empty_response() {
        assert!(!is_psycheros_response(b""));
    }

    #[test]
    fn rejects_garbage_bytes() {
        assert!(!is_psycheros_response(&[
            0xff, 0x00, 0xde, 0xad, 0xbe, 0xef
        ]));
    }
}
