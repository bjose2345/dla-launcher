#[cfg(desktop)]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
#[cfg(desktop)]
use tauri::{AppHandle, Emitter, Manager};

#[cfg(desktop)]
use dla_domain::media::MediaSessionId;
use dla_media::SidecarSubtitle;

#[cfg(desktop)]
use crate::media_protocol;

#[cfg(desktop)]
const VIDEO_STATE_EVENT: &str = "native-video-state";
#[cfg(desktop)]
const VIDEO_TITLE_PREFIX: &str = "dla-video-state:";

#[derive(Default)]
pub struct NativeVideoController {
    #[cfg(desktop)]
    active: Mutex<Option<ActiveNativeVideo>>,
}

#[cfg(desktop)]
struct ActiveNativeVideo {
    label: String,
    session_id: String,
    viewport: NativeVideoViewport,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeVideoFit {
    Contain,
    Cover,
    Original,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoViewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenNativeVideoRequest {
    pub session_id: String,
    pub ordinal: u32,
    pub viewport: NativeVideoViewport,
    pub position_seconds: f64,
    pub volume: f64,
    pub muted: bool,
    pub fit: NativeVideoFit,
    pub auto_play: bool,
    pub poster_url: Option<String>,
    pub title: String,
    pub item_name: String,
    pub completed: bool,
    pub playlist: Vec<NativeVideoPlaylistItem>,
    pub subtitle_label: Option<String>,
    pub labels: NativeVideoLabels,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoPlaylistItem {
    pub ordinal: u32,
    pub item_name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenNativeVideoResponse {
    pub surface_id: String,
    pub subtitles: Vec<SidecarSubtitle>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoLabels {
    pub back: String,
    pub player: String,
    pub play: String,
    pub mark_finished: String,
    pub completed: String,
    pub playlist: String,
    pub open_playlist: String,
    pub close_playlist: String,
    pub now_playing: String,
    pub playback_failed: String,
    pub codec_unsupported: String,
    pub retry: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeVideoCommand {
    Play,
    Pause,
    Seek,
    Volume,
    Muted,
    Fit,
    Retry,
    Fullscreen,
    Subtitle,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoControlRequest {
    pub session_id: String,
    pub command: NativeVideoCommand,
    pub value: Option<f64>,
    pub enabled: Option<bool>,
    pub fit: Option<NativeVideoFit>,
    pub subtitle_track: Option<u32>,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeVideoConfiguration {
    ordinal: u32,
    position_seconds: f64,
    volume: f64,
    muted: bool,
    fit: NativeVideoFit,
    auto_play: bool,
    poster_url: Option<String>,
    title: String,
    item_name: String,
    completed: bool,
    playlist: Vec<NativeVideoPlaylistItem>,
    subtitle_label: Option<String>,
    labels: NativeVideoLabels,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawNativeVideoState {
    kind: String,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    paused: bool,
    ended: bool,
    ready_state: u16,
    error_code: Option<u16>,
    subtitle_track: Option<u32>,
    action_ordinal: Option<u32>,
}

#[cfg(desktop)]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeVideoState {
    surface_id: String,
    session_id: String,
    ordinal: u32,
    kind: String,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    paused: bool,
    ended: bool,
    ready_state: u16,
    error_code: Option<u16>,
    subtitle_track: Option<u32>,
    action_ordinal: Option<u32>,
    fullscreen: bool,
}

#[cfg(mobile)]
impl NativeVideoController {
    pub fn close(&self, _app: &tauri::AppHandle, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(desktop)]
impl NativeVideoController {
    pub fn open(
        &self,
        app: &AppHandle,
        request: OpenNativeVideoRequest,
    ) -> Result<OpenNativeVideoResponse, String> {
        use tauri::{WebviewUrl, webview::WebviewWindowBuilder};

        validate_viewport(request.viewport)?;
        validate_number(request.position_seconds, "position")?;
        validate_number(request.volume, "volume")?;
        media_protocol::validate_video_asset(
            app,
            &MediaSessionId(request.session_id.clone()),
            request.ordinal,
        )?;
        let subtitles = media_protocol::video_subtitles(
            app,
            &MediaSessionId(request.session_id.clone()),
            request.ordinal,
        )?;

        self.close_active(app)?;

        let label = format!("video-{}", uuid::Uuid::new_v4());
        let url = format!(
            "dla-video://localhost/{}/{}",
            request.session_id, request.ordinal
        )
        .parse()
        .map_err(|error| format!("invalid native video URL: {error}"))?;
        let configuration = NativeVideoConfiguration {
            ordinal: request.ordinal,
            position_seconds: request.position_seconds.max(0.0),
            volume: request.volume.clamp(0.0, 1.0),
            muted: request.muted,
            fit: request.fit,
            auto_play: request.auto_play,
            poster_url: request.poster_url,
            title: request.title,
            item_name: request.item_name,
            completed: request.completed,
            playlist: request.playlist,
            subtitle_label: request.subtitle_label,
            labels: request.labels,
        };
        let configuration =
            serde_json::to_string(&configuration).map_err(|error| error.to_string())?;
        let configure_script =
            format!("window.__dlaVideo && window.__dlaVideo.configure({configuration});");
        let state_app = app.clone();
        let state_surface_id = label.clone();
        let state_session_id = request.session_id.clone();
        let state_ordinal = request.ordinal;
        let main = app
            .get_webview_window("main")
            .ok_or_else(|| "main window is unavailable".to_owned())?;
        let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::CustomProtocol(url))
            .title("DLA Video")
            .background_color(tauri::window::Color(5, 5, 8, 255))
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .skip_taskbar(true)
            .focused(false)
            .visible(false)
            .parent(&main)
            .map_err(|error| error.to_string())?
            .on_page_load(move |webview, payload| {
                if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                    let _ = webview.eval(configure_script.clone());
                }
            })
            .on_document_title_changed(move |_webview, title| {
                let Some(payload) = title.strip_prefix(VIDEO_TITLE_PREFIX) else {
                    return;
                };
                let Ok(raw) = serde_json::from_str::<RawNativeVideoState>(payload) else {
                    return;
                };
                if !video_state_kind_is_valid(&raw.kind) {
                    return;
                }
                if raw.kind == "fullscreen"
                    && let Some(main) = state_app.get_webview_window("main")
                    && let Ok(current) = main.is_fullscreen()
                {
                    let _ = main.set_fullscreen(!current);
                }
                let fullscreen = state_app
                    .get_webview_window("main")
                    .and_then(|main| main.is_fullscreen().ok())
                    .unwrap_or(false);
                let event = NativeVideoState {
                    surface_id: state_surface_id.clone(),
                    session_id: state_session_id.clone(),
                    ordinal: state_ordinal,
                    kind: raw.kind,
                    position_seconds: finite_or_zero(raw.position_seconds),
                    duration_seconds: raw.duration_seconds.filter(|value| value.is_finite()),
                    paused: raw.paused,
                    ended: raw.ended,
                    ready_state: raw.ready_state,
                    error_code: raw.error_code,
                    subtitle_track: raw.subtitle_track,
                    action_ordinal: raw.action_ordinal,
                    fullscreen,
                };
                let _ = state_app.emit_to("main", VIDEO_STATE_EVENT, event);
            });
        let video = builder.build().map_err(|error| error.to_string())?;
        position_video_window(app, &video, request.viewport)?;
        video.show().map_err(|error| error.to_string())?;
        *self
            .active
            .lock()
            .map_err(|_| "native video state is unavailable")? = Some(ActiveNativeVideo {
            label: label.clone(),
            session_id: request.session_id,
            viewport: request.viewport,
        });
        Ok(OpenNativeVideoResponse {
            surface_id: label,
            subtitles,
        })
    }

    pub fn update_viewport(
        &self,
        app: &AppHandle,
        session_id: &str,
        surface_id: &str,
        viewport: NativeVideoViewport,
    ) -> Result<(), String> {
        validate_viewport(viewport)?;
        let label = self.active_label(session_id, Some(surface_id))?;
        let window = app
            .get_webview_window(&label)
            .ok_or_else(|| "native video surface is unavailable".to_owned())?;
        position_video_window(app, &window, viewport)?;
        if let Some(active) = self
            .active
            .lock()
            .map_err(|_| "native video state is unavailable")?
            .as_mut()
            && active.session_id == session_id
            && active.label == surface_id
        {
            active.viewport = viewport;
        }
        Ok(())
    }

    pub fn reposition_active(&self, app: &AppHandle) -> Result<(), String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "native video state is unavailable")?
            .as_ref()
            .map(|active| (active.label.clone(), active.viewport));
        let Some((label, viewport)) = active else {
            return Ok(());
        };
        let Some(window) = app.get_webview_window(&label) else {
            let mut active = self
                .active
                .lock()
                .map_err(|_| "native video state is unavailable")?;
            if active.as_ref().is_some_and(|active| active.label == label) {
                *active = None;
            }
            return Ok(());
        };
        position_video_window(app, &window, viewport)
    }

    pub fn close_all(&self, app: &AppHandle) -> Result<(), String> {
        self.close_active(app)
    }

    pub fn control(
        &self,
        app: &AppHandle,
        request: NativeVideoControlRequest,
    ) -> Result<(), String> {
        let label = self.active_label(&request.session_id, None)?;
        let window = app
            .get_webview_window(&label)
            .ok_or_else(|| "native video surface is unavailable".to_owned())?;
        if matches!(request.command, NativeVideoCommand::Fullscreen) {
            let enabled = request
                .enabled
                .ok_or_else(|| "fullscreen state is required".to_owned())?;
            return app
                .get_webview_window("main")
                .ok_or_else(|| "main window is unavailable".to_owned())?
                .set_fullscreen(enabled)
                .map_err(|error| error.to_string());
        }
        validate_control(&request)?;
        let payload = serde_json::to_string(&request).map_err(|error| error.to_string())?;
        window
            .eval(format!(
                "window.__dlaVideo && window.__dlaVideo.command({payload});"
            ))
            .map_err(|error| error.to_string())
    }

    pub fn close(&self, app: &AppHandle, session_id: &str) -> Result<(), String> {
        let should_close = self
            .active
            .lock()
            .map_err(|_| "native video state is unavailable")?
            .as_ref()
            .is_some_and(|active| active.session_id == session_id);
        if should_close {
            self.close_active(app)?;
        }
        Ok(())
    }

    pub fn close_surface(
        &self,
        app: &AppHandle,
        session_id: &str,
        surface_id: &str,
    ) -> Result<(), String> {
        let should_close = self
            .active
            .lock()
            .map_err(|_| "native video state is unavailable")?
            .as_ref()
            .is_some_and(|active| active.session_id == session_id && active.label == surface_id);
        if should_close {
            self.close_active(app)?;
        }
        Ok(())
    }

    fn active_label(&self, session_id: &str, surface_id: Option<&str>) -> Result<String, String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "native video state is unavailable")?;
        match active.as_ref() {
            Some(active)
                if active.session_id == session_id
                    && surface_id.is_none_or(|surface_id| surface_id == active.label) =>
            {
                Ok(active.label.clone())
            }
            _ => Err("native video session is no longer active".to_owned()),
        }
    }

    fn close_active(&self, app: &AppHandle) -> Result<(), String> {
        let active = self
            .active
            .lock()
            .map_err(|_| "native video state is unavailable")?
            .take();
        if let Some(active) = active
            && let Some(window) = app.get_webview_window(&active.label)
        {
            window.destroy().map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[cfg(desktop)]
fn position_video_window(
    app: &AppHandle,
    video: &tauri::WebviewWindow,
    viewport: NativeVideoViewport,
) -> Result<(), String> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "main window is unavailable".to_owned())?;
    let scale = main.scale_factor().map_err(|error| error.to_string())?;
    let origin = main.inner_position().map_err(|error| error.to_string())?;
    let (target_position, target_size) = video_window_bounds(origin, scale, viewport);
    if video.outer_position().map_err(|error| error.to_string())? != target_position {
        video
            .set_position(target_position)
            .map_err(|error| error.to_string())?;
    }
    if video.inner_size().map_err(|error| error.to_string())? != target_size {
        video
            .set_size(target_size)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(desktop)]
fn video_window_bounds(
    origin: tauri::PhysicalPosition<i32>,
    scale: f64,
    viewport: NativeVideoViewport,
) -> (tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>) {
    let x = f64::from(origin.x) + viewport.x * scale;
    let y = f64::from(origin.y) + viewport.y * scale;
    let width = (viewport.width * scale).round().max(1.0) as u32;
    let height = (viewport.height * scale).round().max(1.0) as u32;
    (
        tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32),
        tauri::PhysicalSize::new(width, height),
    )
}

#[cfg(desktop)]
fn validate_viewport(viewport: NativeVideoViewport) -> Result<(), String> {
    validate_number(viewport.x, "viewport x")?;
    validate_number(viewport.y, "viewport y")?;
    validate_number(viewport.width, "viewport width")?;
    validate_number(viewport.height, "viewport height")?;
    if viewport.width < 1.0 || viewport.height < 1.0 {
        return Err("native video viewport must have a positive size".to_owned());
    }
    Ok(())
}

#[cfg(desktop)]
fn validate_number(value: f64, name: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("native video {name} must be finite"))
    }
}

#[cfg(desktop)]
fn validate_control(request: &NativeVideoControlRequest) -> Result<(), String> {
    match request.command {
        NativeVideoCommand::Seek | NativeVideoCommand::Volume => validate_number(
            request
                .value
                .ok_or_else(|| "native video control value is required".to_owned())?,
            "control value",
        ),
        NativeVideoCommand::Muted => request
            .enabled
            .map(|_| ())
            .ok_or_else(|| "native video boolean state is required".to_owned()),
        NativeVideoCommand::Fit => request
            .fit
            .map(|_| ())
            .ok_or_else(|| "native video fit mode is required".to_owned()),
        NativeVideoCommand::Subtitle => Ok(()),
        _ => Ok(()),
    }
}

#[cfg(desktop)]
fn video_state_kind_is_valid(kind: &str) -> bool {
    matches!(
        kind,
        "loading"
            | "metadata"
            | "ready"
            | "playing"
            | "paused"
            | "time"
            | "waiting"
            | "ended"
            | "error"
            | "interaction"
            | "fullscreen"
            | "subtitle"
            | "subtitle_error"
            | "back"
            | "finish"
            | "choose"
    )
}

#[cfg(desktop)]
fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_rejects_non_finite_and_empty_bounds() {
        assert!(
            validate_viewport(NativeVideoViewport {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 450.0,
            })
            .is_ok()
        );
        assert!(
            validate_viewport(NativeVideoViewport {
                x: 0.0,
                y: 0.0,
                width: f64::NAN,
                height: 450.0,
            })
            .is_err()
        );
        assert!(
            validate_viewport(NativeVideoViewport {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 450.0,
            })
            .is_err()
        );
    }

    #[test]
    fn video_bounds_follow_the_main_window_origin() {
        let viewport = NativeVideoViewport {
            x: 120.0,
            y: 80.0,
            width: 800.0,
            height: 450.0,
        };
        let (initial_position, size) =
            video_window_bounds(tauri::PhysicalPosition::new(140, 90), 2.0, viewport);
        let (moved_position, moved_size) =
            video_window_bounds(tauri::PhysicalPosition::new(400, 300), 2.0, viewport);

        assert_eq!(initial_position, tauri::PhysicalPosition::new(380, 250));
        assert_eq!(moved_position, tauri::PhysicalPosition::new(640, 460));
        assert_eq!(size, tauri::PhysicalSize::new(1600, 900));
        assert_eq!(moved_size, size);
    }

    #[test]
    fn only_known_child_state_events_are_forwarded() {
        assert!(video_state_kind_is_valid("playing"));
        assert!(video_state_kind_is_valid("fullscreen"));
        assert!(video_state_kind_is_valid("back"));
        assert!(video_state_kind_is_valid("finish"));
        assert!(video_state_kind_is_valid("choose"));
        assert!(!video_state_kind_is_valid("navigate"));
    }
}
