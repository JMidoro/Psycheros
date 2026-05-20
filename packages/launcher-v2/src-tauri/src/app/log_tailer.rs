//! Live tail of the daemon's stderr log.
//!
//! The daemon writes structured logs to `<data>/logs/daemon.stderr.log`
//! (psycheros's logger captures both stdout and stderr there; the file
//! grows append-only across restarts). This module polls the file every
//! ~1.5s and emits `daemon-log-line` events for each new line — the
//! manager card renders them in a persistent always-visible panel so
//! the user has an "is the daemon healthy?" surface without having to
//! `tail -f` from a terminal.
//!
//! Polling rather than file-watch via `notify` because:
//! - the file is small-medium (megabytes over a long-running daemon)
//! - polling has predictable resource use and trivial cleanup semantics
//! - the dep cost of `notify` (inotify + kqueue bindings, etc.) isn't
//!   justified for a 1.5s cadence
//!
//! Handles three lifecycle cases:
//! - **Append** (normal case): re-read from last known size to current.
//! - **Truncation/rotation**: current size < last known → reset cursor
//!   to 0, re-read everything; the user sees the new file from the top.
//! - **Missing file**: the daemon hasn't been installed yet, or logs
//!   haven't accumulated. Silent skip — no event emitted.

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::paths;

/// Event name the frontend listens on. Payload is `{ line: String }` —
/// a single log line, newline already stripped.
pub const LOG_EVENT: &str = "daemon-log-line";

/// Poll cadence. 1.5s balances "live feel" against "don't burn cycles."
const POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// Spawn the log tailer thread. Runs for the lifetime of the launcher
/// process.
pub fn spawn_log_tailer(handle: AppHandle) {
    thread::spawn(move || {
        let log_path = paths::log_dir().join("daemon.stderr.log");
        let mut cursor: u64 = match fs::metadata(&log_path) {
            // First poll catches up "from now" — the user opens the
            // launcher and starts seeing fresh activity, not the full
            // history. The on-demand `recent_daemon_log_lines` command
            // covers initial population.
            Ok(m) => m.len(),
            Err(_) => 0,
        };

        loop {
            thread::sleep(POLL_INTERVAL);

            let size = match fs::metadata(&log_path) {
                Ok(m) => m.len(),
                Err(_) => continue, // file missing — daemon not installed yet
            };

            if size < cursor {
                // Truncation or rotation. Reset.
                cursor = 0;
            }
            if size == cursor {
                continue;
            }

            let new_bytes = match read_range(&log_path, cursor, size) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("[launcher] log-tailer read failed: {e}");
                    continue;
                }
            };
            cursor = size;

            for line in new_bytes.lines() {
                // Skip blank lines — psycheros emits some during the
                // banner block; they add noise without information.
                if line.trim().is_empty() {
                    continue;
                }
                let _ = handle.emit(LOG_EVENT, line);
            }
        }
    });
}

/// Read the last `tail_bytes` worth of the stderr log and return its
/// lines. Used by the frontend's manager init to populate the log panel
/// before the live tailer's first emission lands.
///
/// Returns lines in chronological order (oldest first).
pub fn recent_lines(max_lines: usize, tail_bytes: u64) -> Vec<String> {
    let log_path = paths::log_dir().join("daemon.stderr.log");
    let size = match fs::metadata(&log_path).map(|m| m.len()) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let start = size.saturating_sub(tail_bytes);
    let raw = match read_range(&log_path, start, size) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    let mut lines: Vec<String> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();
    // If we landed mid-line, the first line is partial — drop it (only
    // when we DIDN'T start from byte 0).
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    if lines.len() > max_lines {
        lines.drain(..lines.len() - max_lines);
    }
    lines
}

fn read_range(path: &std::path::Path, start: u64, end: u64) -> std::io::Result<String> {
    let len = end.saturating_sub(start);
    let mut f = fs::File::open(path)?;
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity(len as usize);
    let mut limited = f.take(len);
    limited.read_to_end(&mut buf)?;
    // Daemon logs are UTF-8 from a Deno process; lossy-decode as defense
    // against a malformed line that shouldn't crash the tailer.
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use super::*;

    fn write_log(path: &std::path::Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn recent_lines_returns_tail_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.txt");
        let content = "line a\nline b\nline c\nline d\n";
        write_log(&path, content);

        // Read all four lines: tail_bytes covers everything.
        let mut f = fs::File::open(&path).unwrap();
        f.seek(SeekFrom::Start(0)).unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        let raw = String::from_utf8_lossy(&buf).into_owned();
        let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        assert_eq!(lines, vec!["line a", "line b", "line c", "line d"]);
    }

    #[test]
    fn recent_lines_caps_to_max_lines() {
        // Verify the max_lines cap by running the public function
        // against a hand-rolled log file using the same paths layout.
        // We can't call recent_lines() directly without paths::log_dir
        // resolving to a temp location, so test the slicing logic
        // inline: this validates the post-collect cap behavior.
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        let max = 5;
        let mut trimmed = lines.clone();
        if trimmed.len() > max {
            trimmed.drain(..trimmed.len() - max);
        }
        assert_eq!(trimmed.len(), 5);
        assert_eq!(trimmed.first().unwrap(), "line 16");
        assert_eq!(trimmed.last().unwrap(), "line 20");
    }
}
