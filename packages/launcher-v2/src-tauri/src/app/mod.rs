//! Tauri app wiring — builder setup, menu wiring, watcher thread.
//!
//! Pulls everything together: registers Tauri commands, builds the native
//! menu, kicks off the daemon-status watcher, and registers the menu event
//! handler that toggles the user-summon flag.
//!
//! The watcher is a plain `std::thread` (not a tokio task). It polls
//! [`daemon::probe`] every 2s, emits a `daemon-status-changed` event when
//! the state transitions, and drives the webview navigation via
//! [`daemon::navigation::drive`]. The frontend listens for the event to
//! update its own UI; navigation is driven from Rust so cross-origin
//! restrictions don't bite.

pub mod log_tailer;
pub mod menu;
pub mod state;
pub mod tray;
pub mod update_watcher;

use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::daemon::{self, DaemonState};
use state::AppState;

/// Show or hide the manager window and flip the macOS activation policy
/// to match. Visible = `Regular` (dock + Cmd+Tab, like a normal app);
/// hidden = `Accessory` (menu-bar-only, no dock icon, no Cmd+Tab entry).
///
/// The tray icon stays alive in either mode because it's owned by the
/// AppHandle (which outlives any individual window). This is what makes
/// the launcher behave as a true menu-bar agent: closing the window
/// doesn't kill the process, the tray persists, and the user can
/// re-summon the window via the tray or by re-launching the .app.
pub fn set_manager_visible(app: &AppHandle, visible: bool) {
    if let Some(window) = app.get_webview_window("main") {
        if visible {
            let _ = window.show();
            let _ = window.set_focus();
        } else {
            let _ = window.hide();
        }
    }
    #[cfg(target_os = "macos")]
    {
        let policy = if visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        let _ = app.set_activation_policy(policy);
    }
}

/// Event name emitted to the frontend on every daemon-state transition.
pub const STATUS_EVENT: &str = "daemon-status-changed";

/// Polling interval for the watcher. 2s is the sweet spot — fast enough
/// that "click install → see chat" feels instant once the daemon binds
/// the port, slow enough that we're not pegging launchctl on every tick.
const WATCHER_INTERVAL: Duration = Duration::from_secs(2);

/// Spawn the daemon-status watcher. Loop body splits into two layers:
///
/// 1. **Self-correcting visibility** — [`tray::update_for_state`] is
///    called on every tick, not just on transitions. Tray visibility
///    is derived from the current state on every poll, so if anything
///    upstream (a mode-switch worker, an external `launchctl` command,
///    a probe that briefly returned the wrong state) leaves the tray
///    out of sync, the next tick brings it back into agreement. The
///    cost is negligible (one `tray.set_visible(bool)` per 2s).
///
/// 2. **Edge-triggered side effects** — event emit + navigation drive
///    only on actual state transitions. These are surfaced to the
///    frontend and the webview navigation, where firing on every tick
///    would be wasteful and visible (e.g. log-spam, repeat redraws).
pub fn spawn_status_watcher(handle: AppHandle) {
    thread::spawn(move || {
        let mut last: Option<DaemonState> = None;
        loop {
            let status = daemon::probe();

            // Always pass through the tray so visibility is a function
            // of *current* state, not "what's changed since last poll."
            tray::update_for_state(&handle, status.state);

            if Some(status.state) != last {
                eprintln!("[launcher] daemon state -> {:?}", status.state);
                if let Err(e) = handle.emit(STATUS_EVENT, status) {
                    eprintln!("[launcher] emit failed: {e}");
                }
                daemon::navigation::drive(&handle, status);
                last = Some(status.state);
            }

            thread::sleep(WATCHER_INTERVAL);
        }
    });
}

/// Handle a menu event. Routed from the global `on_menu_event` handler.
pub fn handle_menu_event(handle: &AppHandle, menu_id: &str) {
    match menu_id {
        id if id == menu::PREFERENCES_ID => {
            let state = handle.state::<AppState>();
            // Toggle user_summoned and re-navigate to honor the new view choice.
            let now = !state.user_summoned.load(Ordering::SeqCst);
            state.user_summoned.store(now, Ordering::SeqCst);
            eprintln!("[launcher] Cmd+, -> user_summoned={now}");
            daemon::navigation::drive(handle, daemon::probe());
        }
        id if id == menu::QUIT_ID => {
            // Cmd+Q is treated as a window-close gesture, not a
            // process-kill. The launcher stays alive as long as the
            // tray is up (i.e. daemon is Running); if the tray was
            // already hidden, the no-surfaces check exits the process
            // cleanly. Either way, the daemon is never killed by a
            // user pressing Cmd+Q — Stop daemon is the only path to
            // that, by design.
            eprintln!("[launcher] Cmd+Q -> hide window + maybe exit");
            set_manager_visible(handle, false);
            tray::maybe_exit_on_window_hidden(handle);
        }
        _ => {}
    }
}
