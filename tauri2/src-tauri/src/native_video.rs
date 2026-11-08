#[cfg(all(desktop, native_video_backend = "surface"))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
#[cfg(all(desktop, native_video_backend = "surface"))]
use tauri::{AppHandle, Emitter, Manager};

#[cfg(all(desktop, native_video_backend = "surface"))]
use dla_domain::media::MediaSessionId;
use dla_media::SidecarSubtitle;

#[cfg(all(desktop, native_video_backend = "surface"))]
use crate::media_protocol;

#[cfg(all(desktop, not(native_video_backend = "none")))]
pub(crate) const VIDEO_STATE_EVENT: &str = "native-video-state";
#[cfg(all(desktop, native_video_backend = "surface"))]
const VIDEO_TITLE_PREFIX: &str = "dla-video-state:";

// The native picture is a build-time choice (`native_video_backend`, set by
// build.rs): a webview window drawing the video, or none when the page's own
// <video> element plays, as the Chromium build does with the CEF the project
// builds itself.

#[cfg(all(desktop, native_video_backend = "surface"))]
#[derive(Default)]
pub struct NativeVideoController {
    active: Mutex<Option<ActiveNativeVideo>>,
}

/// No native picture: the page plays video itself. These answer a page that
/// asked anyway, which `read_video_backend` tells it not to.
#[cfg(any(mobile, all(desktop, native_video_backend = "none")))]
#[derive(Default)]
pub struct NativeVideoController {}

#[cfg(all(desktop, native_video_backend = "none"))]
const NO_NATIVE_PICTURE: &str = "this build plays video in the page; it has no native picture";

#[cfg(all(desktop, native_video_backend = "none"))]
impl NativeVideoController {
    pub fn open(
        &self,
        _app: &tauri::AppHandle,
        _request: OpenNativeVideoRequest,
    ) -> Result<OpenNativeVideoResponse, String> {
        Err(NO_NATIVE_PICTURE.to_owned())
    }

    pub fn update_viewport(
        &self,
        _app: &tauri::AppHandle,
        _session_id: &str,
        _surface_id: &str,
        _viewport: NativeVideoViewport,
        _obstructions: Vec<NativeVideoRectangle>,
    ) -> Result<(), String> {
        Err(NO_NATIVE_PICTURE.to_owned())
    }

    pub fn reposition_active(&self, _app: &tauri::AppHandle) -> Result<(), String> {
        Ok(())
    }

    pub fn close_all(&self, _app: &tauri::AppHandle) -> Result<(), String> {
        Ok(())
    }

    pub fn control(
        &self,
        _app: &tauri::AppHandle,
        _request: NativeVideoControlRequest,
    ) -> Result<(), String> {
        Err(NO_NATIVE_PICTURE.to_owned())
    }

    pub fn close(&self, _app: &tauri::AppHandle, _session_id: &str) -> Result<(), String> {
        Ok(())
    }

    pub fn close_surface(
        &self,
        _app: &tauri::AppHandle,
        _session_id: &str,
        _surface_id: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(all(desktop, native_video_backend = "surface"))]
struct ActiveNativeVideo {
    label: String,
    session_id: String,
    viewport: NativeVideoViewport,
    obstructions: Vec<NativeVideoRectangle>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeVideoFit {
    Contain,
    Cover,
    Original,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeVideoSubtitleColor {
    White,
    Yellow,
    Cyan,
}

/// How subtitles are drawn, chosen by the user for every video.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoSubtitleStyle {
    /// Text size as a factor of the normal size, 1 being normal.
    pub size: f64,
    pub color: NativeVideoSubtitleColor,
    /// Text opacity, 0 to 1.
    pub opacity: f64,
    /// Opacity of the black box behind the text, 0 to 1; 0 is no box.
    pub background: f64,
}

impl Default for NativeVideoSubtitleStyle {
    fn default() -> Self {
        Self {
            size: 1.0,
            color: NativeVideoSubtitleColor::White,
            opacity: 1.0,
            background: 0.75,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoViewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A part of the page over the video that must stay visible, in CSS pixels.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoRectangle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Corner radius, for a round control.
    #[serde(default)]
    pub radius: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
// The Chromium build's GStreamer surface draws no poster, title or playlist.
#[cfg_attr(not(native_video_backend = "surface"), allow(dead_code))]
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
    #[serde(default)]
    pub subtitle_style: NativeVideoSubtitleStyle,
    pub labels: NativeVideoLabels,
    /// Whether the letterbox bands carry a blurred copy of the picture.
    #[serde(default = "ambient_default")]
    pub ambient: bool,
}

fn ambient_default() -> bool {
    true
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
    pub playlist: String,
    pub open_playlist: String,
    pub close_playlist: String,
    pub now_playing: String,
    pub playback_failed: String,
    pub codec_unsupported: String,
    pub exit_fullscreen: String,
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
    Rate,
    SubtitleStyle,
    /// Whether the letterbox bands carry a blurred copy of the picture.
    Ambient,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeVideoControlRequest {
    pub session_id: String,
    pub command: NativeVideoCommand,
    pub value: Option<f64>,
    /// A seek while scrubbing: quick, landing on the nearest keyframe.
    #[serde(default)]
    pub preview: Option<bool>,
    pub enabled: Option<bool>,
    pub fit: Option<NativeVideoFit>,
    pub subtitle_track: Option<u32>,
    #[serde(default)]
    pub subtitle_style: Option<NativeVideoSubtitleStyle>,
    /// The command came from a click on the picture: the player shows the
    /// play or pause badge over it, as the web pictures do themselves.
    #[serde(default)]
    pub badge: Option<bool>,
}

#[cfg(all(desktop, native_video_backend = "surface"))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeVideoConfiguration {
    ordinal: u32,
    fullscreen: bool,
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
    subtitle_style: NativeVideoSubtitleStyle,
    labels: NativeVideoLabels,
    ambient: bool,
}

#[cfg(all(desktop, native_video_backend = "surface"))]
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
    #[serde(default)]
    volume: Option<f64>,
}

#[cfg(all(desktop, not(native_video_backend = "none")))]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeVideoState {
    pub(crate) surface_id: String,
    pub(crate) session_id: String,
    pub(crate) ordinal: u32,
    pub(crate) kind: String,
    pub(crate) position_seconds: f64,
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) paused: bool,
    pub(crate) ended: bool,
    pub(crate) ready_state: u16,
    pub(crate) error_code: Option<u16>,
    pub(crate) subtitle_track: Option<u32>,
    pub(crate) action_ordinal: Option<u32>,
    pub(crate) fullscreen: bool,
    /// The surface's own volume, with a "volume" event.
    pub(crate) volume: Option<f64>,
}

#[cfg(mobile)]
impl NativeVideoController {
    pub fn close(&self, _app: &tauri::AppHandle, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(all(desktop, native_video_backend = "surface"))]
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
            fullscreen: main_is_fullscreen(app),
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
            subtitle_style: request.subtitle_style,
            labels: request.labels,
            ambient: request.ambient,
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
            .on_document_title_changed(move |webview, title| {
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
                    let _ = webview.eval(fullscreen_script(!current));
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
                    volume: raw.volume.filter(|value| value.is_finite()),
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
            obstructions: Vec::new(),
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
        obstructions: Vec<NativeVideoRectangle>,
    ) -> Result<(), String> {
        validate_viewport(viewport)?;
        validate_obstructions(&obstructions)?;
        let label = self.active_label(session_id, Some(surface_id))?;
        let window = app
            .get_webview_window(&label)
            .ok_or_else(|| "native video surface is unavailable".to_owned())?;
        position_video_window(app, &window, viewport)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| "native video state is unavailable")?;
        if let Some(active) = active.as_mut()
            && active.session_id == session_id
            && active.label == surface_id
        {
            let reshape = active.obstructions != obstructions
                || (!obstructions.is_empty() && active.viewport != viewport);
            active.viewport = viewport;
            if reshape {
                shape_video_window(&window, viewport, &obstructions)?;
                active.obstructions = obstructions;
            }
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
        if matches!(request.command, NativeVideoCommand::Fullscreen) {
            let enabled = request
                .enabled
                .ok_or_else(|| "fullscreen state is required".to_owned())?;
            app.get_webview_window("main")
                .ok_or_else(|| "main window is unavailable".to_owned())?
                .set_fullscreen(enabled)
                .map_err(|error| error.to_string())?;
            // The player leaving fullscreen on its way out may find the
            // surface already closed; the window change still counts.
            if let Ok(label) = self.active_label(&request.session_id, None)
                && let Some(window) = app.get_webview_window(&label)
            {
                window
                    .eval(fullscreen_script(enabled))
                    .map_err(|error| error.to_string())?;
            }
            return Ok(());
        }
        let label = self.active_label(&request.session_id, None)?;
        let window = app
            .get_webview_window(&label)
            .ok_or_else(|| "native video surface is unavailable".to_owned())?;
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

#[cfg(all(desktop, native_video_backend = "surface"))]
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

#[cfg(all(desktop, native_video_backend = "surface"))]
fn main_is_fullscreen(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|main| main.is_fullscreen().ok())
        .unwrap_or(false)
}

#[cfg(all(desktop, native_video_backend = "surface"))]
fn fullscreen_script(enabled: bool) -> String {
    format!("window.__dlaVideo && window.__dlaVideo.setFullscreen({enabled});")
}

/// Cuts the page's controls that lie over the picture in fullscreen (the dock,
/// its menus, the exit button) out of the surface window, so they show through
/// it and take the pointer.
#[cfg(windows)]
fn shape_video_window(
    video: &tauri::WebviewWindow,
    viewport: NativeVideoViewport,
    obstructions: &[NativeVideoRectangle],
) -> Result<(), String> {
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, RGN_DIFF, SetWindowRgn,
    };

    let hwnd = video.hwnd().map_err(|error| error.to_string())?.0;
    let scale = video.scale_factor().map_err(|error| error.to_string())?;
    let size = video.inner_size().map_err(|error| error.to_string())?;
    let physical = |value: f64| (value * scale).round() as i32;
    // SAFETY: GDI calls on regions created here and on the surface's own
    // window; SetWindowRgn owns the region it is given, the holes are deleted.
    unsafe {
        if obstructions.is_empty() {
            SetWindowRgn(hwnd as _, 0 as _, 1);
            return Ok(());
        }
        let region = CreateRectRgn(0, 0, size.width as i32, size.height as i32);
        for obstruction in obstructions {
            let left = physical(obstruction.x - viewport.x);
            let top = physical(obstruction.y - viewport.y);
            let right = physical(obstruction.x - viewport.x + obstruction.width);
            let bottom = physical(obstruction.y - viewport.y + obstruction.height);
            let corner = physical(obstruction.radius * 2.0);
            let hole = CreateRoundRectRgn(left, top, right, bottom, corner, corner);
            CombineRgn(region, region, hole, RGN_DIFF);
            DeleteObject(hole as _);
        }
        SetWindowRgn(hwnd as _, region, 1);
    }
    Ok(())
}

/// GTK shapes the window in logical pixels; Wayland ignores bounding shapes,
/// so there the dock stays behind the surface.
#[cfg(all(target_os = "linux", native_video_backend = "surface"))]
fn shape_video_window(
    video: &tauri::WebviewWindow,
    viewport: NativeVideoViewport,
    obstructions: &[NativeVideoRectangle],
) -> Result<(), String> {
    use gtk::cairo::{RectangleInt, Region};
    use gtk::prelude::WidgetExt;

    let window = video.gtk_window().map_err(|error| error.to_string())?;
    if obstructions.is_empty() {
        window.shape_combine_region(None);
        return Ok(());
    }
    let logical = |value: f64| value.round() as i32;
    let region = Region::create_rectangle(&RectangleInt::new(
        0,
        0,
        logical(viewport.width),
        logical(viewport.height),
    ));
    for obstruction in obstructions {
        region
            .subtract_rectangle(&RectangleInt::new(
                logical(obstruction.x - viewport.x),
                logical(obstruction.y - viewport.y),
                logical(obstruction.width),
                logical(obstruction.height),
            ))
            .map_err(|error| error.to_string())?;
    }
    window.shape_combine_region(Some(&region));
    Ok(())
}

#[cfg(all(desktop, not(windows), not(target_os = "linux")))]
fn shape_video_window(
    _video: &tauri::WebviewWindow,
    _viewport: NativeVideoViewport,
    _obstructions: &[NativeVideoRectangle],
) -> Result<(), String> {
    Ok(())
}

#[cfg(all(desktop, native_video_backend = "surface"))]
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

#[cfg(all(desktop, not(native_video_backend = "none")))]
pub(crate) fn validate_viewport(viewport: NativeVideoViewport) -> Result<(), String> {
    validate_number(viewport.x, "viewport x")?;
    validate_number(viewport.y, "viewport y")?;
    validate_number(viewport.width, "viewport width")?;
    validate_number(viewport.height, "viewport height")?;
    if viewport.width < 1.0 || viewport.height < 1.0 {
        return Err("native video viewport must have a positive size".to_owned());
    }
    Ok(())
}

#[cfg(all(desktop, not(native_video_backend = "none")))]
pub(crate) fn validate_obstructions(obstructions: &[NativeVideoRectangle]) -> Result<(), String> {
    for obstruction in obstructions {
        validate_number(obstruction.x, "obstruction x")?;
        validate_number(obstruction.y, "obstruction y")?;
        validate_number(obstruction.width, "obstruction width")?;
        validate_number(obstruction.height, "obstruction height")?;
        validate_number(obstruction.radius, "obstruction radius")?;
        if obstruction.width < 0.0 || obstruction.height < 0.0 || obstruction.radius < 0.0 {
            return Err("native video obstructions cannot have a negative size".to_owned());
        }
    }
    Ok(())
}

#[cfg(all(desktop, not(native_video_backend = "none")))]
pub(crate) fn validate_number(value: f64, name: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("native video {name} must be finite"))
    }
}

#[cfg(all(desktop, not(native_video_backend = "none")))]
pub(crate) fn validate_control(request: &NativeVideoControlRequest) -> Result<(), String> {
    match request.command {
        NativeVideoCommand::Seek | NativeVideoCommand::Volume | NativeVideoCommand::Rate => {
            validate_number(
                request
                    .value
                    .ok_or_else(|| "native video control value is required".to_owned())?,
                "control value",
            )
        }
        NativeVideoCommand::Muted | NativeVideoCommand::Ambient => request
            .enabled
            .map(|_| ())
            .ok_or_else(|| "native video boolean state is required".to_owned()),
        NativeVideoCommand::Fit => request
            .fit
            .map(|_| ())
            .ok_or_else(|| "native video fit mode is required".to_owned()),
        NativeVideoCommand::Subtitle => Ok(()),
        NativeVideoCommand::SubtitleStyle => {
            let style = request
                .subtitle_style
                .ok_or_else(|| "native video subtitle style is required".to_owned())?;
            validate_number(style.opacity, "subtitle opacity")?;
            validate_number(style.background, "subtitle background")?;
            validate_number(style.size, "subtitle size")?;
            if !(0.0..=1.0).contains(&style.opacity) || !(0.0..=1.0).contains(&style.background) {
                return Err("native video subtitle opacities must be between 0 and 1".to_owned());
            }
            if !(0.25..=4.0).contains(&style.size) {
                return Err("native video subtitle size must be between 0.25 and 4".to_owned());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(all(desktop, native_video_backend = "surface"))]
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
            | "volume"
            | "back"
            | "choose"
    )
}

#[cfg(all(desktop, not(native_video_backend = "none")))]
pub(crate) fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

// The validation and surface tests have no subject in a build without a
// native picture; the Tauri 2 workspace runs them.
#[cfg(all(test, not(native_video_backend = "none")))]
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

    #[cfg(native_video_backend = "surface")]
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

    #[cfg(native_video_backend = "surface")]
    #[test]
    fn only_known_child_state_events_are_forwarded() {
        assert!(video_state_kind_is_valid("playing"));
        assert!(video_state_kind_is_valid("fullscreen"));
        assert!(video_state_kind_is_valid("back"));
        assert!(!video_state_kind_is_valid("finish"));
        assert!(video_state_kind_is_valid("choose"));
        assert!(!video_state_kind_is_valid("navigate"));
    }
}
