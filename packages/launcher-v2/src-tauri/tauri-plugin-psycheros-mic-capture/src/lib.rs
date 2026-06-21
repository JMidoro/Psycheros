//! Tauri 2 plugin exposing native mic capture for the Psycheros launcher.
//!
//! Lives in its own crate so Tauri's build process auto-generates proper
//! plugin-namespaced ACL permissions (`mic-capture:allow-start-capture`,
//! etc.) that work from the remote origin (`http://localhost:3000`) the
//! launcher's webview navigates to when showing the daemon's voice UI.
//! In-app `tauri::plugin::Builder` plugins get lumped into `__app-acl__`
//! and silently rejected from remote origins — this crate structure is
//! load-bearing, not stylistic.
//!
//! ## Why native capture at all
//!
//! On macOS Tahoe (26), WKWebView does not expose `navigator.mediaDevices`
//! at all. Wry 0.55.1's WKUIDelegate auto-grants getUserMedia but the API
//! surface itself isn't there to be granted. Capturing natively via
//! AVAudioEngine sidesteps the broken WebRTC pipeline entirely — we pull
//! PCM frames straight off the input node and stream them to JS via a
//! Tauri IPC channel, where voice.js forwards them to the daemon over
//! the existing voice WebSocket.
//!
//! ## Platform scope
//!
//! macOS only. Windows WebView2 and Linux webkit2gtk handle getUserMedia
//! via the normal browser permission flow — the non_macos.rs stub returns
//! an error so voice.js can fall back to getUserMedia there.

mod commands;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod non_macos;

#[cfg(target_os = "macos")]
pub use macos::CaptureState;

#[cfg(not(target_os = "macos"))]
pub use non_macos::CaptureState;

use tauri::{plugin::TauriPlugin, Manager, Runtime};

/// Plugin entry point. Wire from the host app's `.tauri::Builder` chain
/// via `.plugin(tauri_plugin_psycheros_mic_capture::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    // Plugin name MUST match the ACL namespace Tauri derives from the
    // package name (`tauri-plugin-psycheros-mic-capture` → namespace
    // `psycheros-mic-capture`). A mismatch here causes "not allowed by
    // ACL" even when the capability file grants the permission — the
    // runtime resolves invoke('plugin:<name>|...') against <name>, not
    // against the package-derived namespace.
    tauri::plugin::Builder::<R>::new("psycheros-mic-capture")
        .invoke_handler(tauri::generate_handler![
            commands::start_capture,
            commands::stop_capture,
        ])
        .setup(|app, _api| {
            app.manage(CaptureState::default());
            Ok(())
        })
        .build()
}
