//! The experimental Chromium build (tauri3/): the same launcher on Tauri 3 with
//! its CEF runtime instead of the system webview. Only that build compiles
//! this module.

use tauri_runtime_cef::{AutoplayPolicy, Cef, RemoteDebugging, SandboxPolicy};

/// Chromium's settings for the launcher. The development container cannot
/// start Chromium's sandbox, as it cannot start WebKit's (compose.yaml), so
/// `DLA_CHROMIUM_DISABLE_SANDBOX` turns it off there.
///
/// The page only ever loads the launcher's own files and protocols; every
/// remote request (covers, downloads, translation) goes through Rust. So the
/// Chromium services that call Google on their own stay off: Safe Browsing's
/// hash list, the component updater's certificate data and the spell checker's
/// dictionary download. Playback starts from the launcher's own code (a resumed
/// track, the next one in a queue), which Chromium's default autoplay policy
/// would block until a click.
///
/// A development build keeps its own Chromium profile (`cef-dev` beside the
/// runtime's default `cef` under the app's cache directory): a release build
/// writes `devtools.remote_debugging.allowed = false` into the profile it
/// opens, which closed the development build's DevTools port whenever a
/// packaged build had run, and a profile Chromium migrated forward is not
/// meant to be opened by an older Chromium again.
pub fn runtime(identifier: &str) -> Cef {
    let sandbox = if std::env::var_os("DLA_CHROMIUM_DISABLE_SANDBOX").is_some() {
        SandboxPolicy::Disabled
    } else {
        SandboxPolicy::Auto
    };
    let runtime = Cef::default()
        .sandbox(sandbox)
        .safe_browsing(false)
        .component_updates(false)
        .spell_checking(false)
        .autoplay(AutoplayPolicy::NoUserGestureRequired);
    let runtime = if cfg!(debug_assertions) {
        let cache_base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
        runtime.root_cache_path(cache_base.join(identifier).join("cef-dev"))
    } else {
        runtime
    };
    // For checking the page from inside the development container only: the
    // protocol has no authentication and any local process can reach the port.
    match std::env::var("DLA_CHROMIUM_DEVTOOLS_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
    {
        Some(port) => runtime.remote_debugging(RemoteDebugging::Port {
            port,
            allowed_origins: Vec::new(),
        }),
        None => runtime,
    }
}

/// Lets the main thread sleep between events on Linux. The runtime drives CEF
/// from the GLib loop that GTK runs (CEF's external message pump), but CEF
/// still builds Chromium's GLib pump for the main thread, and that pump's work
/// source joins the same loop. CEF never runs the pump, so the source asks GLib
/// for a poll timeout of zero on every pass (`MessagePumpGlib::HandlePrepare`
/// returns 0 while the pump is not running) and the loop spins one core at
/// 100%. Nothing else uses the source: its check is false while the pump is
/// not running, and CEF schedules Chromium's work through the runtime instead
/// of the source's wake-up pipe. Must run on the main thread after CEF starts.
#[cfg(target_os = "linux")]
pub fn remove_idle_glib_work_source() {
    use glib_sys::{
        G_PRIORITY_DEFAULT_IDLE, GFALSE, g_idle_source_new, g_main_context_default,
        g_main_context_find_source_by_id, g_source_attach, g_source_destroy, g_source_get_priority,
        g_source_unref,
    };

    let mut removed = 0;
    // SAFETY: the main thread owns the default main context, and every source
    // is used only while the context holds it. GSourceFuncs is GLib's public
    // vtable; calling a work source's prepare outside a loop pass only reads
    // the pump's state.
    unsafe {
        let context = g_main_context_default();
        // A context numbers its sources in order, so a new source's ID is one
        // past every source attached so far.
        let probe = g_idle_source_new();
        let next_id = g_source_attach(probe, context);
        g_source_destroy(probe);
        g_source_unref(probe);
        for id in 1..next_id {
            let source = g_main_context_find_source_by_id(context, id);
            if source.is_null() || g_source_get_priority(source) != G_PRIORITY_DEFAULT_IDLE {
                continue;
            }
            let funcs = (*source).source_funcs;
            let Some(prepare) = (*funcs).prepare else {
                continue;
            };
            if (*funcs).dispatch.is_none() || !defined_in_libcef(prepare as *const libc::c_void) {
                continue;
            }
            let mut timeout = -1;
            if prepare(source, &mut timeout) == GFALSE && timeout == 0 {
                g_source_destroy(source);
                removed += 1;
            }
        }
    }
    log::info!(target: "dla::lifecycle", "event=chromium_glib_work_source_removed count={removed}");
}

#[cfg(target_os = "linux")]
fn defined_in_libcef(address: *const libc::c_void) -> bool {
    // SAFETY: dladdr only reads the loader's tables and fills `info`, whose
    // file name stays valid while the library is loaded, as libcef always is.
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(address, &mut info) == 0 || info.dli_fname.is_null() {
            return false;
        }
        std::ffi::CStr::from_ptr(info.dli_fname)
            .to_bytes()
            .rsplit(|byte| *byte == b'/')
            .next()
            == Some(b"libcef.so")
    }
}
