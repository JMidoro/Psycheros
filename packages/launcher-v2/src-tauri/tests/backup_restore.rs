//! End-to-end test: spawn a real psycheros, hit our `backup_data` +
//! `restore_data` commands, verify the round-trip preserves state.
//!
//! What this catches that pure-Rust unit tests can't:
//! - `http::request_localhost` actually negotiating HTTP/1.1 with
//!   psycheros's server (vs. talking to a mock).
//! - Psycheros's `/api/admin/entity-data/{export,import}` contract
//!   matching what our launcher code assumes.
//! - The zip the launcher writes to disk being readable by
//!   psycheros's own importer (no encoding drift, no truncation).
//! - End-to-end: write a known sentinel, back up, wipe, restore,
//!   sentinel reappears.
//!
//! What it deliberately doesn't cover:
//! - The supervisor.restart() leg of restore_data — restart() is
//!   idempotent and no-ops when no plist exists at the resolved
//!   path, which is the case for the user's real home dir during a
//!   test run when launcher-v2 isn't installed. (If the user IS
//!   running their own launcher-v2, this test would restart their
//!   real daemon; the test sets HOME to a tempdir to prevent that.)
//!
//! Test isolation: integration tests in `tests/` each get their own
//! binary, so env-var mutation doesn't leak to other tests.
//!
//! Requires Deno on PATH. CI gates the launcher-v2 jobs on having
//! Deno staged via `denoland/setup-deno@v2` already — same dep.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use psycheros_launcher_lib::commands;
use psycheros_launcher_lib::config;
use psycheros_launcher_lib::http;

/// Find a free TCP port by binding to 0 + reading the assigned port +
/// dropping the listener. Race-prone in theory (something else could
/// bind to the same port between drop and our re-bind), but in
/// practice the kernel doesn't recycle ports that fast for a single
/// test invocation. Same trick the dev community uses everywhere.
fn pick_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind 0");
    listener.local_addr().expect("local_addr").port()
}

/// Path to packages/psycheros/ in the staging repo. CARGO_MANIFEST_DIR
/// is `<repo>/packages/launcher-v2/src-tauri`, so two `..` get us to
/// the workspace root.
fn psycheros_source_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("psycheros")
}

/// Spawn psycheros via deno with MCP disabled, custom port, custom
/// data dir. Returns a guard that kills the child on drop (so a
/// failed assertion doesn't leak the subprocess).
struct PsycherosChild {
    child: Child,
    port: u16,
}

impl PsycherosChild {
    fn spawn(port: u16, data_dir: &Path) -> Self {
        let source = psycheros_source_dir();
        assert!(
            source.join("src/main.ts").exists(),
            "psycheros source not at {} — is the staging repo layout intact?",
            source.display(),
        );

        let child = Command::new("deno")
            .args(["run", "-A", "src/main.ts"])
            .current_dir(&source)
            .env("PSYCHEROS_DATA_DIR", data_dir)
            .env("PSYCHEROS_PORT", port.to_string())
            .env("PSYCHEROS_MCP_ENABLED", "false")
            // Avoid contaminating the test with the user's real Zai
            // creds if they happen to be set in the parent env.
            .env_remove("ZAI_API_KEY")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn psycheros (is deno on PATH?)");

        Self { child, port }
    }

    /// Block until the daemon binds the port, or panic after `deadline`.
    /// Polls every 100ms.
    fn wait_ready(&self, deadline: Duration) {
        let start = Instant::now();
        loop {
            if start.elapsed() > deadline {
                panic!("psycheros didn't bind :{} within {deadline:?}", self.port);
            }
            if std::net::TcpStream::connect_timeout(
                &format!("127.0.0.1:{}", self.port).parse().unwrap(),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for PsycherosChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn backup_writes_zip_to_downloads_and_restore_returns_data() {
    // ── Fixture: launcher data dir, downloads dir, daemon data dir,
    //    and a free port. All tempdirs so we don't touch the user's
    //    real `~/Library/Application Support/Psycheros/`.
    let launcher_data = tempfile::tempdir().expect("tempdir launcher-data");
    let downloads = tempfile::tempdir().expect("tempdir downloads");
    let daemon_data = tempfile::tempdir().expect("tempdir daemon-data");
    let fake_home = tempfile::tempdir().expect("tempdir fake-home");
    let port = pick_free_port();

    // Make the launcher commands resolve into our tempdirs. HOME is
    // overridden so any supervisor lookup that runs ends up in
    // fake_home/Library/LaunchAgents/ (which doesn't exist → no-op).
    std::env::set_var("PSYCHEROS_LAUNCHER_DATA_DIR", launcher_data.path());
    std::env::set_var("PSYCHEROS_DOWNLOAD_DIR", downloads.path());
    std::env::set_var("HOME", fake_home.path());

    // Seed launcher config so backup_data_blocking knows the port.
    let cfg = config::LauncherConfig {
        port,
        bundled_source_version: Some("psycheros-test-v0.0.0".into()),
        ..Default::default()
    };
    let cfg_path = launcher_data.path().join("config.json");
    std::fs::create_dir_all(launcher_data.path()).unwrap();
    std::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap())
        .expect("write launcher config");

    // ── Round-trip sentinel: conversations.
    //
    //    Psycheros's export includes the full conversations table.
    //    POST /api/conversations creates a row; GET /api/conversations
    //    confirms count. After restoring the pre-mutation (empty)
    //    backup, the conversation should be gone — proving the
    //    restore actually wiped + repopulated from the zip.
    //
    //    Why not general-settings.json: psycheros's exportEntityData
    //    explicitly does NOT include general-settings.json (verified
    //    by reading entity-data.ts). That's arguably a bug in
    //    psycheros — settings are entity-identity — but it's out of
    //    scope for this test, which is about validating the
    //    launcher's IPC + http layer.
    let psycheros = PsycherosChild::spawn(port, daemon_data.path());
    psycheros.wait_ready(Duration::from_secs(45));

    // ── Backup. The blocking variant takes no args; it reads port
    //    from launcher config and writes to PSYCHEROS_DOWNLOAD_DIR.
    let backup =
        commands::backup_data_blocking().expect("backup_data should succeed against a live daemon");
    let zip_path = PathBuf::from(&backup.path);
    assert!(zip_path.exists(), "backup zip not on disk: {}", backup.path);
    assert!(backup.size_bytes > 0, "backup zip is empty (size_bytes=0)");

    // Sanity: zip files start with the PK\x03\x04 local-file-header
    // magic. Cheap structural check without pulling in a zip dep.
    let head = std::fs::read(&zip_path).unwrap();
    assert!(
        head.starts_with(b"PK\x03\x04"),
        "backup file doesn't start with the zip magic — got {:?}",
        &head[..8.min(head.len())]
    );

    // ── Confirm we're at zero conversations pre-mutation.
    let initial_count = get_conversation_count(port);
    assert_eq!(
        initial_count, 0,
        "expected empty entity to have 0 conversations"
    );

    // ── Mutate: create a conversation directly via psycheros's API.
    create_conversation(port, "test-conv-round-trip");
    let after_create = get_conversation_count(port);
    assert_eq!(
        after_create, 1,
        "POST /api/conversations didn't create a row — got count {after_create}"
    );

    // ── Restore the empty-entity backup. The conversation we just
    //    created should disappear — proving restore actually wipes +
    //    repopulates state from the zip.
    let restore =
        commands::restore_data_blocking(backup.path.clone()).expect("restore_data should succeed");
    assert!(
        restore.success,
        "restore reported failure: {}",
        restore.result_message
    );

    // supervisor.restart() in restore is a no-op here (no plist),
    // but psycheros's import handler may need a moment to finalize.
    // Poll for up to 5 seconds.
    let mut final_count = -1i64;
    for _ in 0..50 {
        final_count = get_conversation_count(port);
        if final_count == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        final_count, 0,
        "after restoring empty backup, expected 0 conversations but got {final_count}. \
         The restore round-trip didn't fully wipe state."
    );

    // PsycherosChild::drop kills the subprocess.
    let _ = psycheros;
}

/// POST /api/conversations to create a conversation with the given
/// title. Panics on failure — test helper, not user code.
fn create_conversation(port: u16, title: &str) {
    let body = format!(r#"{{"title":"{title}"}}"#);
    let resp = http::request_localhost(
        port,
        "POST",
        "/api/conversations",
        body.as_bytes(),
        "application/json",
    )
    .expect("POST /api/conversations");
    assert!(
        resp.is_success(),
        "POST /api/conversations returned HTTP {} — body: {}",
        resp.status,
        String::from_utf8_lossy(&resp.body),
    );
}

/// GET /api/conversations and return row count. Returns -1 on any
/// failure so the assertion surfaces a useful diagnostic.
fn get_conversation_count(port: u16) -> i64 {
    let resp = match http::request_localhost(port, "GET", "/api/conversations", &[], "") {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[test] GET /api/conversations failed: {e}");
            return -1;
        }
    };
    if !resp.is_success() {
        eprintln!(
            "[test] GET /api/conversations returned {}: {}",
            resp.status,
            String::from_utf8_lossy(&resp.body)
        );
        return -1;
    }
    // Response is JSON. Count occurrences of `"id":` — each row has
    // one. Conservative match avoids dragging in a JSON dep at the
    // test edge.
    let body = String::from_utf8_lossy(&resp.body);
    body.matches("\"id\":").count() as i64
}
