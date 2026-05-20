#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--smoke` flag runs a headless CI gate (no Tauri webview, no
    // display server needed) and exits with the result code. See
    // lib.rs::smoke() for what it covers.
    if std::env::args().any(|a| a == "--smoke") {
        std::process::exit(psycheros_launcher_lib::smoke());
    }
    psycheros_launcher_lib::run()
}
