//! Command surface for the mic-capture plugin.
//!
//! `start_capture` takes a Tauri `ipc::Channel` argument — JS constructs
//! this with `new window.__TAURI__.ipc.Channel('audio-frame')` and sets
//! `channel.onmessage = (bytes) => voiceWs.send(...)`. Each captured PCM
//! frame is sent as a `Vec<u8>` (raw Int16 LE 16kHz mono).
//!
//! `stop_capture` removes the tap and stops the engine. Safe to call
//! multiple times — second call is a no-op.

use tauri::{ipc::Channel, AppHandle, Manager, Runtime, State};

use crate::CaptureState;

/// Begin capturing mic audio. PCM frames flow through `on_frame`.
///
/// Returns Err if mic permission was denied or capture is already active.
#[tauri::command]
pub async fn start_capture<R: Runtime>(
    app: AppHandle<R>,
    on_frame: Channel<Vec<u8>>,
) -> Result<(), String> {
    let state: State<'_, CaptureState> = app.state::<CaptureState>();
    platform_start_capture(app.clone(), state, on_frame).await
}

/// Stop capturing mic audio. Safe to call when not active.
#[tauri::command]
pub async fn stop_capture<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let state: State<'_, CaptureState> = app.state::<CaptureState>();
    platform_stop_capture(state)
}

// The platform-specific impls live in macos.rs / non_macos.rs but are
// re-exposed here so the command surface stays single-file.
#[cfg(target_os = "macos")]
use crate::macos::{platform_start_capture, platform_stop_capture};

#[cfg(not(target_os = "macos"))]
use crate::non_macos::{platform_start_capture, platform_stop_capture};
