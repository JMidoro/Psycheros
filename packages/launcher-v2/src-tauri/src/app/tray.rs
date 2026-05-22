//! System tray icon — macOS menu bar, Windows notification area.
//!
//! Strict identity: **the tray is visible if and only if the daemon is
//! actually running.** For non-technical users this is a clear,
//! at-a-glance binary signal — Psycheros is alive (icon present) or it
//! isn't (icon gone). Re-entry when the daemon is down is via the .app
//! icon in `/Applications` (macOS) or the Start Menu shortcut /
//! `Program Files\Psycheros\Psycheros.exe` (Windows).
//!
//! ## Per-OS icon
//!
//! macOS menu bars auto-tint template images (white-on-transparent
//! PNGs) to match the menu-bar theme. Windows notification areas
//! render bytes as-is, so a template-style image looks like an
//! illegible black blob there. [`install`] branches the icon asset
//! choice — `tray-icon-template.png` on macOS, the full-color
//! `32x32.png` on Windows/Linux — and sets `icon_as_template(true)`
//! only on macOS.
//!
//! ## Lifecycle
//!
//! [`install`] builds the tray + menu unconditionally (so the launcher
//! can flip visibility cheaply later), but starts it hidden. The
//! daemon-status watcher calls [`update_for_state`] on every state
//! transition; that function shows the tray only when
//! [`DaemonState::Running`], hides it otherwise, and exits the launcher
//! process when both surfaces (tray + window) are hidden — the
//! launcher has no reason to keep consuming resources without a
//! surface to interact through.
//!
//! ## Boot grace
//!
//! At login the OS supervisor (launchd on macOS, Task Scheduler on
//! Windows) starts the launcher with `--no-window`. The daemon is also
//! booting in parallel and may take a few seconds to bind its port.
//! During that window the daemon's state is `Installed`, not
//! `Running`, so by the strict rule the tray is hidden — but the
//! launcher MUST stay alive to detect the eventual transition to
//! `Running` and show the tray then. The [`HAS_BEEN_RUNNING`] flag
//! guards this: we only exit on a hide if we've previously seen the
//! daemon `Running` in this session. A login boot that never reaches
//! `Running` will keep the launcher alive in the background, but it's
//! cheap and the user can investigate via the installed .exe / .app.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{
    image::Image,
    menu::{
        CheckMenuItem, CheckMenuItemBuilder, IsMenuItem, Menu, MenuItem, MenuItemBuilder,
        PredefinedMenuItem,
    },
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Wry,
};

use crate::commands;
use crate::config::{self, DaemonMode};
use crate::daemon::{self, DaemonState};

use super::state::AppState;

/// Tracks whether the daemon has reached `Running` at least once in
/// this launcher session. Set to true the first time
/// [`update_for_state`] sees a `Running` transition; used to gate the
/// no-surfaces auto-exit so the launcher waits for the daemon to come
/// up at login instead of exiting prematurely.
static HAS_BEEN_RUNNING: AtomicBool = AtomicBool::new(false);

/// Mirrors the tray icon's last-set visibility so the window-close
/// handler can answer "is the tray hidden right now?" without a probe.
/// Tauri's `TrayIcon` only exposes `set_visible`, no getter.
static TRAY_VISIBLE: AtomicBool = AtomicBool::new(false);

/// UNIX seconds at which the watcher last saw `DaemonState::Running`.
/// Used as a debounce on the no-surfaces auto-exit so brief daemon
/// transients — mode switches (unload + reload), crashloops where the
/// daemon comes back quickly — don't kill the launcher mid-flight.
static LAST_RUNNING_AT: AtomicU64 = AtomicU64::new(0);

/// Grace window (seconds) between losing the Running state and acting
/// on the no-surfaces auto-exit. 5s comfortably covers a launchd
/// transient (the daemon crashing and being respawned by KeepAlive)
/// without keeping the launcher hanging around after a real Stop.
const EXIT_GRACE_SECS: u64 = 5;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Handles to the mutable menu items. Stored in Tauri's managed state so
/// the status watcher can update them from another thread.
pub struct TrayHandles {
    status: MenuItem<Wry>,
    autostart_check: CheckMenuItem<Wry>,
}

/// Pure mapping from daemon state to the status menu label. Extracted so
/// the test suite can cover every variant without spinning up a Tauri app.
pub fn status_label(state: DaemonState) -> &'static str {
    match state {
        DaemonState::Running => "Psycheros: Running",
        DaemonState::Installed => "Psycheros: Starting…",
        DaemonState::Stopped => "Psycheros: Stopped",
        DaemonState::NotInstalled => "Psycheros: Not installed",
    }
}

/// True when the daemon is currently configured to start at login.
/// Drives the tray's autostart checkbox state, mirroring the
/// `Switch to … mode` button on the Settings card.
pub fn is_autostart(mode: DaemonMode) -> bool {
    matches!(mode, DaemonMode::Autostart)
}

/// Build the tray icon + menu. Always starts hidden — the status
/// watcher's first tick reveals it iff the daemon is `Running`.
pub fn install(app: &AppHandle) -> tauri::Result<()> {
    // Tray icon — embedded at compile time so a corrupted icons/ dir at
    // runtime can't take the tray down. The macOS menu bar auto-tints
    // template images (white on dark mode, black on light); the
    // notification area on Windows just renders the bytes as-is, so a
    // template-style monochrome PNG appears as an illegible black blob
    // there. Branch the asset choice on OS rather than ship one icon
    // for both.
    #[cfg(target_os = "macos")]
    let icon_bytes: &[u8] = include_bytes!("../../icons/tray-icon-template.png");
    #[cfg(not(target_os = "macos"))]
    let icon_bytes: &[u8] = include_bytes!("../../icons/32x32.png");
    let icon = Image::from_bytes(icon_bytes)?;

    // The strict rule means the tray is only ever visible when the
    // daemon is Running, so most menu items can be unconditionally
    // enabled — Start daemon and Quit Launcher don't exist anymore
    // because they'd contradict the model (you can't Start something
    // that's already Running, and Stop daemon IS the quit).
    let status = MenuItemBuilder::with_id("tray_status", status_label(DaemonState::Running))
        .enabled(false)
        .build(app)?;
    let open_chat = MenuItemBuilder::with_id("tray_open_chat", "Open chat").build(app)?;
    let open_manager = MenuItemBuilder::with_id("tray_open_manager", "Open manager").build(app)?;
    let stop = MenuItemBuilder::with_id("tray_stop", "Stop daemon").build(app)?;
    let view_logs = MenuItemBuilder::with_id("tray_view_logs", "View logs…").build(app)?;
    let initial_mode = config::load()
        .map(|c| c.effective_mode())
        .unwrap_or_default();
    // Checkbox-style menu item — macOS renders a leading ✓ when checked,
    // which non-technical users grok instantly compared to a mode-name
    // toggle. Checked == daemon autostarts at login.
    let autostart_check = CheckMenuItemBuilder::new("Start Psycheros at login")
        .id("tray_autostart_check")
        .checked(is_autostart(initial_mode))
        .build(app)?;

    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    let items: Vec<&dyn IsMenuItem<Wry>> = vec![
        &status,
        &sep1,
        &open_chat,
        &open_manager,
        &sep2,
        &stop,
        &view_logs,
        &sep3,
        &autostart_check,
    ];
    let menu = Menu::with_items(app, &items)?;

    // icon_as_template is macOS-only behavior — it tells the menu bar
    // to recolor a monochrome image to match the current theme.
    // Setting it true on Windows/Linux doesn't help and arguably hurts
    // (notification areas there just want a normal RGBA icon).
    #[cfg(target_os = "macos")]
    let use_template = true;
    #[cfg(not(target_os = "macos"))]
    let use_template = false;

    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(use_template)
        .menu(&menu)
        // false = left-click runs our custom handler instead of opening
        // the menu. Right-click always opens the menu, matching the
        // Tailscale / 1Password / Backblaze convention.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // Tray only exists when daemon is Running, so the
                // default action is always chat — that's where the
                // value is. Power users who want the manager card can
                // right-click → Open manager.
                switch_to_chat(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray_open_chat" => switch_to_chat(app),
            "tray_open_manager" => summon_manager(app),
            "tray_stop" => spawn_command(|| {
                if let Err(e) = commands::stop_daemon() {
                    eprintln!("[launcher] tray stop: {e}");
                }
            }),
            "tray_view_logs" => open_daemon_logs(),
            "tray_autostart_check" => {
                // Mode toggle is now a plain plist file rewrite — no
                // `launchctl unload/load`, no daemon restart, no
                // cascade through entity-core and the chat surface.
                // The change takes effect at next session load
                // (next login) which is exactly when "autostart at
                // login" matters. set_daemon_mode is fast (~50ms
                // total), so we just run it inline on the main thread
                // and let the watcher's normal sync bring the
                // checkbox into agreement with the persisted config.
                let current = config::load()
                    .map(|c| c.effective_mode())
                    .unwrap_or_default();
                let target = match current {
                    DaemonMode::Autostart => "manual",
                    DaemonMode::Manual => "autostart",
                };
                eprintln!(
                    "[launcher] autostart preference: {:?} -> {}",
                    current, target
                );
                if let Err(e) = commands::set_daemon_mode(target.to_string()) {
                    eprintln!("[launcher] autostart preference failed: {e}");
                    // Persist failed — revert the checkbox visual so
                    // it doesn't claim a mode that isn't on disk.
                    if let Some(handles) = app.try_state::<TrayHandles>() {
                        let _ = handles.autostart_check.set_checked(is_autostart(current));
                    }
                }
            }
            _ => {}
        })
        .build(app)?;

    // Hide on install. The watcher's first tick will flip this to
    // visible if the daemon is Running.
    let _ = tray.set_visible(false);

    app.manage(TrayHandles {
        status,
        autostart_check,
    });
    Ok(())
}

/// Reactively update the tray on every daemon state transition. Sole
/// caller is [`super::spawn_status_watcher`]. Three responsibilities:
///
/// 1. Update the status menu label (cosmetic, helps right-click users
///    confirm what they're looking at).
/// 2. Flip tray visibility to match the strict rule.
/// 3. Trip the no-surfaces auto-exit when the launcher has nothing
///    left to do (no tray + no window + has previously been useful).
pub fn update_for_state(app: &AppHandle, state: DaemonState) {
    let is_running = matches!(state, DaemonState::Running);
    if is_running {
        HAS_BEEN_RUNNING.store(true, Ordering::SeqCst);
        LAST_RUNNING_AT.store(now_secs(), Ordering::SeqCst);
    }

    if let Some(handles) = app.try_state::<TrayHandles>() {
        let _ = handles.status.set_text(status_label(state));
        // Sync the checkbox to whatever is currently persisted in
        // config. Mode toggles no longer disrupt daemon state, so
        // there's no mid-switch transient to guard against — config
        // and the checkbox just stay in agreement at all times.
        let current_mode = config::load()
            .map(|c| c.effective_mode())
            .unwrap_or_default();
        let _ = handles
            .autostart_check
            .set_checked(is_autostart(current_mode));
    }

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_visible(is_running);
        TRAY_VISIBLE.store(is_running, Ordering::SeqCst);
    }

    // No-surfaces auto-exit. Gated behind a grace window — see
    // `should_exit_no_surfaces` for why.
    if !is_running && should_exit_no_surfaces(app) {
        eprintln!("[launcher] no surfaces (tray + window both hidden) — exiting");
        app.exit(0);
    }
}

/// Same surfaces-check as [`update_for_state`], called from the window
/// close handler. If the window goes away and the tray was already
/// hidden, the launcher has no reason to stay alive.
pub fn maybe_exit_on_window_hidden(app: &AppHandle) {
    if !TRAY_VISIBLE.load(Ordering::SeqCst) && should_exit_no_surfaces(app) {
        eprintln!("[launcher] window closed and tray already hidden — exiting");
        app.exit(0);
    }
}

/// Common gate for the no-surfaces auto-exit. Three conditions must
/// hold simultaneously:
///
/// 1. **We've seen the daemon Running at least once this session.**
///    Otherwise the launcher just booted and is still waiting for the
///    daemon to come up — exiting now would be premature.
/// 2. **The window is hidden.** A user actively looking at the manager
///    counts as a reason to stay alive even when the tray is down.
/// 3. **The daemon has been not-Running for longer than the grace
///    window.** Mode switches and crash-recoveries take a fraction of
///    a second and result in a momentary `Running → Stopped → Running`
///    blip. Exiting on the first not-Running tick would kill the
///    launcher mid-mode-switch and leave the user with no tray after
///    the daemon comes back. The 5-second grace covers those transients
///    while still feeling instant for a genuine Stop.
fn should_exit_no_surfaces(app: &AppHandle) -> bool {
    if !HAS_BEEN_RUNNING.load(Ordering::SeqCst) {
        return false;
    }
    let window_hidden = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .map(|v| !v)
        .unwrap_or(true);
    if !window_hidden {
        return false;
    }
    let last_running = LAST_RUNNING_AT.load(Ordering::SeqCst);
    let elapsed = now_secs().saturating_sub(last_running);
    elapsed >= EXIT_GRACE_SECS
}

fn summon_manager(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.user_summoned.store(true, Ordering::SeqCst);
    daemon::navigation::drive(app, daemon::probe());
    // `set_manager_visible` shows the window AND flips activation policy
    // to Regular so the dock icon reappears — the launcher behaves like
    // a normal app while the user is interacting with it.
    super::set_manager_visible(app, true);
}

fn switch_to_chat(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.user_summoned.store(false, Ordering::SeqCst);
    daemon::navigation::drive(app, daemon::probe());
    super::set_manager_visible(app, true);
}

fn open_daemon_logs() {
    use crate::supervisor::{default_supervisor, ServiceSupervisor};
    // The launchd / systemd / task_scheduler impls all return stdout first,
    // stderr second. The stderr log is the one users want — the daemon
    // logs structured lines to stderr.
    let paths = default_supervisor().log_paths();
    let target = paths.get(1).or_else(|| paths.first());
    if let Some(p) = target {
        if let Some(s) = p.to_str() {
            if let Err(e) = commands::open_path(s.to_string()) {
                eprintln!("[launcher] tray view-logs: {e}");
            }
        }
    }
}

fn spawn_command<F: FnOnce() + Send + 'static>(f: F) {
    // Start / Stop run launchctl synchronously — keep them off the
    // main thread so the menu doesn't appear to hang when the user
    // clicks.
    std::thread::spawn(f);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_label_covers_all_states() {
        assert_eq!(status_label(DaemonState::Running), "Psycheros: Running");
        assert_eq!(status_label(DaemonState::Installed), "Psycheros: Starting…");
        assert_eq!(status_label(DaemonState::Stopped), "Psycheros: Stopped");
        assert_eq!(
            status_label(DaemonState::NotInstalled),
            "Psycheros: Not installed"
        );
    }

    #[test]
    fn is_autostart_maps_each_mode() {
        assert!(is_autostart(DaemonMode::Autostart));
        assert!(!is_autostart(DaemonMode::Manual));
    }
}
