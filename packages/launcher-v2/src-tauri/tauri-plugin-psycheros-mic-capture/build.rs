//! Tauri plugin build script — auto-generates ACL permission files
//! (`permissions/allow-start-capture.toml`, `allow-stop-capture.toml`)
//! at build time. Real plugins get proper plugin-namespaced permissions
//! that work from remote origins (http://localhost:3000), which is the
//! whole reason this is a separate crate and not an in-app plugin.

const COMMANDS: &[&str] = &["start_capture", "stop_capture"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
