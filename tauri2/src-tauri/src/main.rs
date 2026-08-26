#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    configure_linux_display_backend();
    dla_launcher_tauri::run();
}

#[cfg(target_os = "linux")]
fn configure_linux_display_backend() {
    // The native video surface must track absolute bounds, which Wayland does
    // not expose to application windows. XWayland preserves that contract.
    unsafe {
        std::env::set_var("GDK_BACKEND", "x11");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_display_backend() {}
