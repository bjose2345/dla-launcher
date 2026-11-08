#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Chromium starts this executable again for its renderer, GPU and other
// helper processes; the attribute hands those to CEF before main runs.
#[tauri_runtime_cef::cef_entry_point]
fn main() {
    configure_linux_display_backend();
    dla_launcher_tauri::run();
}

#[cfg(target_os = "linux")]
fn configure_linux_display_backend() {
    // As in tauri2/src-tauri/src/main.rs: the native video surface must track
    // absolute bounds, which Wayland does not expose. CEF's runtime is X11 too.
    unsafe {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_display_backend() {}
