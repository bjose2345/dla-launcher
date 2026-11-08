use dla_application::launch::LaunchActivityObserver;
use dla_domain::launch::LaunchActivity;
use tauri::{AppHandle, Emitter};

pub const LAUNCH_ACTIVITY_EVENT: &str = "launch-activity";

pub struct TauriLaunchActivityEvents {
    app: AppHandle,
}

impl TauriLaunchActivityEvents {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl LaunchActivityObserver for TauriLaunchActivityEvents {
    fn launch_activity_changed(&self, activity: &LaunchActivity) {
        if let Err(error) = self.app.emit(LAUNCH_ACTIVITY_EVENT, activity) {
            log::warn!(target: "dla::launch", "event=launch_activity_emit_failed error={error}");
        }
    }
}
