//! Native macOS menu bar + accelerators.
//!
//! macOS users expect the standard app menu (Psycheros / File / Edit /
//! View / Window / Help) at the top of the screen. The launcher's only
//! custom items are **Preferences…** (Cmd+,) — flips between chat and
//! manager — and a custom **Quit Psycheros** (Cmd+Q) that routes
//! through the same hide-then-surfaces-check path the window's close
//! button uses. Using `PredefinedMenuItem::quit` here would force
//! `app.exit(0)` and bypass that path entirely, killing the tray even
//! when the daemon is still running.
//!
//! On Linux and Windows, Tauri also exposes a menu bar within the window
//! frame; the same menu structure renders there.
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
    let preferences = MenuItemBuilder::new("Preferences…")
        .id(PREFERENCES_ID)
        .accelerator("Cmd+,")
        .build(app)?;

    let about = PredefinedMenuItem::about(app, None, None)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItemBuilder::new("Quit Psycheros")
        .id(QUIT_ID)
        .accelerator("Cmd+Q")
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
