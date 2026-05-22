//! Application-wide state shared across Tauri commands and the watcher.
//!
//! The state struct is intentionally small. The view-mode logic in
//! particular hinges on a single `AtomicBool` — `user_summoned` —
//! distinguishing two reasons the splash can be visible:
//!
//! - **Auto-fallback** (`user_summoned == false`): the daemon isn't running,
//!   so the splash is shown out of necessity. When daemon recovers, auto-
//!   navigate to chat.
//! - **Explicit summon** (`user_summoned == true`): the user pressed `Cmd+,`
//!   or clicked "Manager" while daemon was up. Stay on splash until they
//!   click "Back to chat" or press the accelerator again.
//!
//! See docs/frontend.md for the full state machine.

use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

pub struct AppState {
    /// When true, lock the webview to the splash regardless of daemon state.
    pub user_summoned: AtomicBool,
    /// The Tauri-asset URL the launcher splash was originally served from.
    /// In dev mode this is `http://127.0.0.1:<random>/`; in production it's
    /// `tauri://localhost/`. Captured at startup AND lazily refreshed on
    /// every watcher tick.
    ///
    /// Why lazy refresh: on Windows (WebView2) `Window::url()` called
    /// during Tauri's `setup()` hook returns the `about:blank` placeholder
    /// because the navigation to the actual frontend hasn't fired yet.
    /// macOS (WKWebView) resolves the URL synchronously so capture-at-
    /// setup works there, but using a single capture site for both
    /// platforms produced a "navigate-to-about:blank" bug on Windows
    /// when the user pressed Cmd+, to go to the manager.
    ///
    /// The watcher refreshes this whenever the current URL looks like a
    /// launcher splash (not the daemon's `http://localhost:<port>/` and
    /// not the about:blank placeholder). The watcher's first tick fires
    /// well after the initial navigation completes, so the bad initial
    /// capture is replaced before any user action could trigger a
    /// navigation that depends on it.
    pub splash_url: Mutex<String>,
    /// The URL most recently navigated to. Used to skip no-op
    /// `location.replace(sameURL)` calls that would otherwise wipe in-
    /// progress splash JS state.
    pub last_navigated: Mutex<String>,
}

impl AppState {
    pub fn new(splash_url: String) -> Self {
        Self {
            user_summoned: AtomicBool::new(false),
            splash_url: Mutex::new(splash_url.clone()),
            last_navigated: Mutex::new(splash_url),
        }
    }

    /// Update `splash_url` if `current_url` looks like a valid launcher
    /// splash URL — i.e. not blank, not the about:blank placeholder, and
    /// not the daemon's `http://localhost:<daemon_port>/` (which is the
    /// chat surface, not the splash).
    ///
    /// Called from the watcher tick. Idempotent: if the current URL
    /// matches what we already have, the lock is dropped without a write.
    pub fn maybe_capture_splash(&self, current_url: &str, daemon_port: u16) {
        if current_url.is_empty() || current_url == "about:blank" {
            return;
        }
        let daemon_origin = format!("http://localhost:{daemon_port}");
        if current_url.starts_with(&daemon_origin) {
            return;
        }
        let mut splash = self.splash_url.lock().expect("splash_url mutex poisoned");
        if *splash != current_url {
            *splash = current_url.to_string();
        }
    }

    /// Snapshot the splash URL for use by the navigator. Returns the
    /// stored string; callers should treat `"about:blank"` or empty as
    /// "splash hasn't been captured yet, do not navigate" (the navigator
    /// guards on this).
    pub fn splash_url_snapshot(&self) -> String {
        self.splash_url
            .lock()
            .expect("splash_url mutex poisoned")
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maybe_capture_skips_about_blank() {
        let state = AppState::new("captured".to_string());
        state.maybe_capture_splash("about:blank", 3000);
        assert_eq!(state.splash_url_snapshot(), "captured");
    }

    #[test]
    fn maybe_capture_skips_daemon_origin() {
        let state = AppState::new("captured".to_string());
        state.maybe_capture_splash("http://localhost:3000/", 3000);
        // Even with a non-trailing-slash variant we should not capture.
        state.maybe_capture_splash("http://localhost:3000/chat", 3000);
        assert_eq!(state.splash_url_snapshot(), "captured");
    }

    #[test]
    fn maybe_capture_updates_for_dev_random_port() {
        // Dev mode: Tauri serves the splash at a random localhost port
        // different from the daemon's port. Must not be confused for the
        // daemon URL.
        let state = AppState::new("about:blank".to_string());
        state.maybe_capture_splash("http://127.0.0.1:1430/", 3000);
        assert_eq!(state.splash_url_snapshot(), "http://127.0.0.1:1430/");
    }

    #[test]
    fn maybe_capture_updates_for_tauri_scheme() {
        let state = AppState::new("about:blank".to_string());
        state.maybe_capture_splash("tauri://localhost/", 3000);
        assert_eq!(state.splash_url_snapshot(), "tauri://localhost/");
    }
}
