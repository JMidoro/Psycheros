//! Native menu bar + accelerators.
//!
//! macOS users expect the standard app menu (Psycheros / File / Edit /
//! View / Window / Help) at the top of the screen. The launcher's only
//! custom items are **Preferences…** (Cmd+, on macOS, Ctrl+, on
//! Windows/Linux) — flips between chat and manager — and a custom
//! **Quit Psycheros** (Cmd/Ctrl+Q) that routes through the same hide-
//! then-surfaces-check path the window's close button uses. Using
//! `PredefinedMenuItem::quit` here would force `app.exit(0)` and
//! bypass that path entirely, killing the tray even when the daemon
//! is still running.
//!
//! On Linux and Windows, Tauri renders the menu in the window frame
//! at the top of the webview area; the same menu structure works
//! there. Tauri's `CmdOrCtrl` accelerator token maps to Cmd on macOS
//! and Ctrl on Windows/Linux — hardcoding `Cmd+` would silently
//! drop the binding on non-macOS platforms.
//!
//! ## Accelerator caveat on Windows
//!
//! The menu accelerator string `CmdOrCtrl+Comma` is also set on the
//! item, but Tauri 2's menu accelerator system **doesn't fire from a
//! webview-focused window on Windows** — WebView2 captures keyboard
//! events before they reach the host's menu chain. The chord still
//! works (because [`crate::register_preferences_shortcut_plugin`] in
//! `lib.rs` registers an OS-level hotkey via
//! `tauri-plugin-global-shortcut`), but if you're trying to add a
//! second accelerator and wondering why it doesn't fire on Windows,
//! this is why. Add the new chord to the global-shortcut plugin's
//! handler in `lib.rs`, not as a menu accelerator.
//!
//! Menu event handling lives in [`crate::app::handle_menu_event`].

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};

/// Menu item ID for the Preferences/Manager toggle. Matched in the
/// `on_menu_event` handler.
pub const PREFERENCES_ID: &str = "preferences";

/// Menu item ID for the custom Cmd+Q handler. Replaces the predefined
/// `Quit` item so the launcher's strict-tray lifecycle stays intact —
/// see the module doc-comment.
pub const QUIT_ID: &str = "psycheros_quit";

pub fn build_menu(app: &tauri::App) -> tauri::Result<Menu<tauri::Wry>> {
    // Note: the literal comma in an accelerator string parses fine on
    // macOS but silently fails on Windows (keyboard-types requires the
    // named form for punctuation). Use `Comma` so the chord works
    // cross-platform.
    let preferences = MenuItemBuilder::new("Preferences…")
        .id(PREFERENCES_ID)
        .accelerator("CmdOrCtrl+Comma")
        .build(app)?;

    let about = PredefinedMenuItem::about(app, None, None)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::new("Quit Psycheros")
        .id(QUIT_ID)
        .accelerator("CmdOrCtrl+Q")
        .build(app)?;

    let app_submenu = SubmenuBuilder::new(app, "Psycheros")
        .item(&about)
        .item(&separator)
        .item(&preferences)
        .item(&separator)
        .item(&quit)
        .build()?;

    MenuBuilder::new(app).item(&app_submenu).build()
}
