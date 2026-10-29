//! WebKitGTK can stop drawing a page after the window is minimized or
//! resized: the page keeps running, but its frame clock stays parked, so the
//! window shows its last picture. Hiding and showing the whole window again
//! restarts it; hiding only the web view does not. The page asks for this when
//! frames stop arriving while someone is using the window.

#[cfg(target_os = "linux")]
#[tauri::command]
pub fn recover_webview_rendering(window: tauri::WebviewWindow) -> Result<(), String> {
    use std::time::Duration;

    use gtk::{
        gdk::WindowState,
        prelude::{GtkWindowExt, WidgetExt},
    };

    log::warn!(target: "dla::render", "event=webview_rendering_recovery");
    let main_window = window.clone();
    window
        .run_on_main_thread(move || {
            let Ok(native_window) = main_window.gtk_window() else {
                return;
            };
            let maximized = native_window.is_maximized();
            let fullscreen = native_window
                .window()
                .is_some_and(|surface| surface.state().contains(WindowState::FULLSCREEN));
            native_window.hide();
            gtk::glib::timeout_add_local_once(Duration::from_millis(150), move || {
                native_window.show_all();
                if fullscreen {
                    native_window.fullscreen();
                } else if maximized {
                    native_window.maximize();
                }
                native_window.present();
            });
        })
        .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub fn recover_webview_rendering() -> Result<(), String> {
    Ok(())
}
