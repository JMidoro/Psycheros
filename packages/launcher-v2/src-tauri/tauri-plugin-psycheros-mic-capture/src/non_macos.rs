//! Non-macOS stub. Windows WebView2 and Linux webkit2gtk handle
//! getUserMedia via the normal browser permission flow, so the launcher
//! never invokes this plugin there. We still need a compile-clean
//! stub so the host crate builds on those platforms.

use tauri::{ipc::Channel, AppHandle, Runtime, State};

/// Placeholder state — empty on non-macOS since there's no capture state to track.
#[derive(Default)]
pub struct CaptureState {}

pub async fn platform_start_capture<R: Runtime>(
    _app: AppHandle<R>,
    _state: State<'_, CaptureState>,
    _on_frame: Channel<Vec<u8>>,
) -> Result<(), String> {
    Err("mic-capture plugin is macOS-only — voice chat on this platform uses getUserMedia directly".to_string())
}

pub fn platform_stop_capture(_state: State<'_, CaptureState>) -> Result<(), String> {
    Ok(())
}
