use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use dla_application::media::MediaError;
use dla_domain::{installation::MediaType, media::MediaSessionId};
use dla_media::{SidecarSubtitle, find_sidecar_subtitles, subtitle_to_web_vtt};
use percent_encoding::percent_decode_str;
use serde::Serialize;
use tauri::{
    AppHandle, Manager,
    http::{
        Method, Request, Response, StatusCode,
        header::{
            ACCEPT_RANGES, ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_EXPOSE_HEADERS,
            CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
        },
    },
};

use crate::commands::AppState;

const MAX_RANGE_BYTES: u64 = 1024 * 1024;
const MAX_SUBTITLE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_MATERIALIZED_MEDIA_BYTES: u64 = 256 * 1024 * 1024;

pub fn respond(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match response(app, &request) {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}

pub fn respond_video_document(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match video_document_response(app, &request) {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}

pub fn respond_subtitle(app: &AppHandle, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    match subtitle_response(app, &request) {
        Ok(response) => response,
        Err(error) => error_response(error),
    }
}

pub(crate) fn validate_video_asset(
    app: &AppHandle,
    session_id: &MediaSessionId,
    ordinal: u32,
) -> Result<(), String> {
    let asset = resolve_video_asset(app, session_id, ordinal).map_err(protocol_error_message)?;
    File::open(&asset.path)
        .map(|_| ())
        .map_err(|error| protocol_error_message(map_asset_io_error(error, &asset.path)))
}

pub(crate) fn video_subtitles(
    app: &AppHandle,
    session_id: &MediaSessionId,
    ordinal: u32,
) -> Result<Vec<SidecarSubtitle>, String> {
    let asset = resolve_video_asset(app, session_id, ordinal).map_err(protocol_error_message)?;
    Ok(find_sidecar_subtitles(&asset.path))
}

fn response(
    app: &AppHandle,
    request: &Request<Vec<u8>>,
) -> Result<Response<Vec<u8>>, ProtocolError> {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return Err(ProtocolError::MethodNotAllowed);
    }
    let (session_id, ordinal) = parse_locator(request.uri().path())?;
    let asset = resolve_asset(app, &session_id, ordinal)?;
    let mut file =
        File::open(&asset.path).map_err(|error| map_asset_io_error(error, &asset.path))?;
    let length = file
        .metadata()
        .map_err(|error| map_asset_io_error(error, &asset.path))?
        .len();
    let content_type = media_content_type(asset.media_type, &asset.path);
    let mut builder = Response::builder()
        .header(CONTENT_TYPE, content_type)
        .header(ACCEPT_RANGES, "bytes")
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(CACHE_CONTROL, "private, no-store")
        .header("X-Content-Type-Options", "nosniff");

    if let Some(value) = request.headers().get("range") {
        let value = value
            .to_str()
            .map_err(|_| ProtocolError::RangeNotSatisfiable(length))?;
        let range = parse_range(value, length)?;
        builder = builder
            .status(StatusCode::PARTIAL_CONTENT)
            .header(ACCESS_CONTROL_EXPOSE_HEADERS, "content-range")
            .header(
                CONTENT_RANGE,
                format!("bytes {}-{}/{length}", range.start, range.end),
            )
            .header(CONTENT_LENGTH, range.length());
        if request.method() == Method::HEAD {
            return builder.body(Vec::new()).map_err(ProtocolError::response);
        }
        file.seek(SeekFrom::Start(range.start))
            .map_err(|error| map_asset_io_error(error, &asset.path))?;
        let mut bytes = Vec::with_capacity(range.length() as usize);
        file.take(range.length())
            .read_to_end(&mut bytes)
            .map_err(|error| map_asset_io_error(error, &asset.path))?;
        return builder.body(bytes).map_err(ProtocolError::response);
    }

    builder = builder.header(CONTENT_LENGTH, length);
    if request.method() == Method::HEAD {
        return builder.body(Vec::new()).map_err(ProtocolError::response);
    }
    let bytes =
        read_materialized_media(file, length, MAX_MATERIALIZED_MEDIA_BYTES).map_err(|error| {
            match error {
                MaterializedMediaError::TooLarge => ProtocolError::FileTooLarge,
                MaterializedMediaError::Read(error) => map_asset_io_error(error, &asset.path),
            }
        })?;
    builder.body(bytes).map_err(ProtocolError::response)
}

#[derive(Debug)]
enum MaterializedMediaError {
    TooLarge,
    Read(io::Error),
}

fn read_materialized_media(
    reader: impl Read,
    declared_length: u64,
    limit: u64,
) -> Result<Vec<u8>, MaterializedMediaError> {
    if declared_length > limit {
        return Err(MaterializedMediaError::TooLarge);
    }
    let capacity =
        usize::try_from(declared_length).map_err(|_| MaterializedMediaError::TooLarge)?;
    let mut bytes = Vec::with_capacity(capacity);
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(MaterializedMediaError::Read)?;
    if bytes.len() as u64 > limit {
        return Err(MaterializedMediaError::TooLarge);
    }
    Ok(bytes)
}

struct ResolvedAsset {
    path: PathBuf,
    media_type: MediaType,
}

fn resolve_asset(
    app: &AppHandle,
    session_id: &MediaSessionId,
    ordinal: u32,
) -> Result<ResolvedAsset, ProtocolError> {
    let state = app.state::<AppState>();
    let descriptor = state
        .media
        .resolve_asset(session_id, ordinal)
        .map_err(|error| map_media_error(session_id, ordinal, error))?;
    let path = resolve_path(
        &descriptor.root_path,
        descriptor.item.relative_path.as_str(),
    )?;
    Ok(ResolvedAsset {
        path,
        media_type: descriptor.item.media_type,
    })
}

fn resolve_video_asset(
    app: &AppHandle,
    session_id: &MediaSessionId,
    ordinal: u32,
) -> Result<ResolvedAsset, ProtocolError> {
    let asset = resolve_asset(app, session_id, ordinal)?;
    if asset.media_type != MediaType::Video {
        return Err(ProtocolError::Forbidden);
    }
    Ok(asset)
}

fn video_document_response(
    app: &AppHandle,
    request: &Request<Vec<u8>>,
) -> Result<Response<Vec<u8>>, ProtocolError> {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return Err(ProtocolError::MethodNotAllowed);
    }
    let (session_id, ordinal) = parse_locator(request.uri().path())?;
    let asset = resolve_video_asset(app, &session_id, ordinal)?;
    File::open(&asset.path).map_err(|error| map_asset_io_error(error, &asset.path))?;
    let source = video_source_url(&asset.path, &session_id, ordinal)?;
    let subtitles = find_sidecar_subtitles(&asset.path)
        .into_iter()
        .map(|subtitle| {
            Ok(VideoSubtitleSource {
                source: subtitle_source_url(&session_id, ordinal, subtitle.index),
                track: subtitle,
            })
        })
        .collect::<Result<Vec<_>, ProtocolError>>()?;
    let body = video_document(&source, &subtitles).into_bytes();
    let builder = Response::builder()
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(CACHE_CONTROL, "private, no-store")
        .header("Content-Security-Policy", "default-src 'none'; media-src file: dla-media: http://dla-media.localhost dla-subtitle: http://dla-subtitle.localhost; img-src https: http: data:; style-src 'unsafe-inline'; script-src 'unsafe-inline';")
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header(CONTENT_LENGTH, body.len());
    if request.method() == Method::HEAD {
        return builder.body(Vec::new()).map_err(ProtocolError::response);
    }
    builder.body(body).map_err(ProtocolError::response)
}

fn subtitle_response(
    app: &AppHandle,
    request: &Request<Vec<u8>>,
) -> Result<Response<Vec<u8>>, ProtocolError> {
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        return Err(ProtocolError::MethodNotAllowed);
    }
    let (session_id, ordinal, track_index) = parse_subtitle_locator(request.uri().path())?;
    let asset = resolve_video_asset(app, &session_id, ordinal)?;
    let track = find_sidecar_subtitles(&asset.path)
        .into_iter()
        .find(|track| track.index == track_index)
        .ok_or(ProtocolError::NotFound)?;
    let directory = asset.path.parent().ok_or(ProtocolError::Forbidden)?;
    let path = resolve_path(
        directory.to_str().ok_or(ProtocolError::Forbidden)?,
        &track.relative_path,
    )?;
    let length = path
        .metadata()
        .map_err(|error| map_asset_io_error(error, &path))?
        .len();
    if length > MAX_SUBTITLE_BYTES {
        return Err(ProtocolError::FileTooLarge);
    }
    let body = subtitle_to_web_vtt(&path, track.format).map_err(|error| {
        log::warn!(
            "subtitle conversion failed for {}: {}",
            path.display(),
            error
        );
        ProtocolError::SubtitleInvalid
    })?;
    let body = body.into_bytes();
    let builder = Response::builder()
        .header(CONTENT_TYPE, "text/vtt; charset=utf-8")
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(CACHE_CONTROL, "private, no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header(CONTENT_LENGTH, body.len());
    if request.method() == Method::HEAD {
        return builder.body(Vec::new()).map_err(ProtocolError::response);
    }
    builder.body(body).map_err(ProtocolError::response)
}

#[cfg(target_os = "linux")]
fn video_source_url(
    path: &Path,
    _session_id: &MediaSessionId,
    _ordinal: u32,
) -> Result<String, ProtocolError> {
    tauri::Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| ProtocolError::Internal)
}

#[cfg(target_os = "windows")]
fn video_source_url(
    _path: &Path,
    session_id: &MediaSessionId,
    ordinal: u32,
) -> Result<String, ProtocolError> {
    Ok(format!(
        "http://dla-media.localhost/{}/{ordinal}",
        session_id.0
    ))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn video_source_url(
    _path: &Path,
    session_id: &MediaSessionId,
    ordinal: u32,
) -> Result<String, ProtocolError> {
    Ok(format!("dla-media://localhost/{}/{ordinal}", session_id.0))
}

#[cfg(target_os = "windows")]
fn subtitle_source_url(session_id: &MediaSessionId, ordinal: u32, track_index: u32) -> String {
    format!(
        "http://dla-subtitle.localhost/{}/{ordinal}/{track_index}",
        session_id.0
    )
}

#[cfg(not(target_os = "windows"))]
fn subtitle_source_url(session_id: &MediaSessionId, ordinal: u32, track_index: u32) -> String {
    format!(
        "dla-subtitle://localhost/{}/{ordinal}/{track_index}",
        session_id.0
    )
}

#[derive(Serialize)]
struct VideoSubtitleSource {
    #[serde(flatten)]
    track: SidecarSubtitle,
    source: String,
}

fn video_document(source: &str, subtitles: &[VideoSubtitleSource]) -> String {
    let source = safe_script_json(source);
    let subtitles = safe_script_json(subtitles);
    VIDEO_DOCUMENT
        .replace("__DLA_VIDEO_SOURCE__", &source)
        .replace("__DLA_VIDEO_SUBTITLES__", &subtitles)
}

fn safe_script_json<T: Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_owned())
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
}

const VIDEO_DOCUMENT: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
:root{color-scheme:dark;font-family:Inter,"Noto Sans JP",system-ui,sans-serif;background:#050508;color:#f5f0ed}
*{box-sizing:border-box}html,body{width:100%;height:100%;margin:0;overflow:hidden;background:#050508}
body{display:grid;place-items:center;cursor:default}video{display:block;width:100%;height:100%;background:#000;object-fit:fill}
video::cue{color:#fff;background:rgba(5,5,8,.82);font-family:Inter,"Noto Sans JP",system-ui,sans-serif;font-size:clamp(16px,2.4vw,34px);line-height:1.35;text-shadow:0 2px 5px #000}
#chromeShade{position:fixed;z-index:18;top:0;right:0;left:0;height:140px;background:linear-gradient(to bottom,rgba(0,0,0,.72),rgba(0,0,0,.34) 52%,transparent);opacity:1;pointer-events:none;transition:opacity .24s ease-out}
#chrome{position:fixed;z-index:20;top:16px;right:16px;left:16px;min-height:66px;overflow:hidden;border:1px solid rgba(255,255,255,.16);border-radius:15px;background:rgba(20,18,27,.76);box-shadow:0 22px 58px rgba(0,0,0,.5);backdrop-filter:blur(22px) saturate(1.2);transition:opacity .2s ease-out,transform .28s cubic-bezier(.2,.8,.2,1),box-shadow .24s ease-out}
#chromeMain{display:grid;min-height:66px;grid-template-columns:auto minmax(0,1fr) auto auto auto;align-items:center;gap:12px;padding:9px 10px;transition:opacity .16s ease-out,transform .22s ease-out}
#chrome button{display:inline-flex;min-height:44px;align-items:center;justify-content:center;gap:8px;padding:8px 13px;border:1px solid rgba(255,255,255,.16);border-radius:10px;background:rgba(62,57,70,.58);color:#f5f0ed;cursor:pointer;font:800 12px/1 Inter,"Noto Sans JP",system-ui,sans-serif;white-space:nowrap;transition:border-color .15s ease-out,background .15s ease-out,color .15s ease-out,transform .15s ease-out}
#chrome button:hover:not(:disabled){border-color:rgba(241,80,121,.72);background:rgba(241,80,121,.18);color:#ff648b}
#chrome button:active:not(:disabled){transform:scale(.97)}#chrome button:disabled{cursor:default;opacity:.5}
.glyph{display:grid;width:18px;height:18px;place-items:center;flex:none;font-size:18px;font-weight:500;line-height:1}
#videoIdentity{min-width:0}#playerLabel{display:flex;align-items:center;gap:6px;color:#ff5c84;font-size:10px;font-weight:900;letter-spacing:.15em;text-transform:uppercase}
#videoTitle,#videoItem{display:block;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}#videoTitle{margin-top:4px;font-size:16px}#videoItem{margin-top:3px;color:#aaa4ad;font-size:11px}
#position{display:flex;min-width:70px;align-items:baseline;justify-content:center;gap:4px;padding:8px 12px;border-right:1px solid rgba(255,255,255,.14);border-left:1px solid rgba(255,255,255,.14);color:#aaa4ad;font-size:12px;font-variant-numeric:tabular-nums}#position strong{color:#f5f0ed;font-size:20px}
#finish{border-color:rgba(241,80,121,.5)!important;color:#ff648b!important}#playlistToggle{width:44px;padding:0!important}
#playlistPanel{position:absolute;inset:0;display:grid;grid-template-columns:auto minmax(0,1fr) auto;align-items:center;background:rgba(20,18,27,.97);opacity:0;pointer-events:none;transform:translateX(32px);transition:opacity .18s ease-out,transform .24s cubic-bezier(.2,.8,.2,1)}
#playlistPanel header{display:flex;height:100%;align-items:center;gap:10px;padding:8px 10px;border-right:1px solid rgba(255,255,255,.14)}#playlistPanel header strong{font-size:14px}#playlistCount{display:grid;min-width:24px;height:22px;place-items:center;padding:0 6px;border-radius:999px;background:rgba(241,80,121,.14);color:#ff648b;font-size:10px}#playlistClose{width:44px;min-height:44px!important;margin-right:10px;padding:0!important;border-color:rgba(241,80,121,.42)!important;color:#ff648b!important}
#playlist{display:flex;min-width:0;align-items:center;gap:6px;margin:0;padding:7px 10px;overflow-x:auto;overflow-y:hidden;list-style:none;scrollbar-width:thin}#playlist li{min-width:min(300px,32vw)}#playlist button{display:grid;width:100%;min-height:48px;grid-template-columns:26px minmax(0,1fr) auto;gap:9px;padding:8px 10px;text-align:left}#playlist button.is-active{border-color:rgba(241,80,121,.5);background:rgba(241,80,121,.12)}#playlist button span{color:#88818c;font-size:10px;font-variant-numeric:tabular-nums}#playlist button strong{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:12px}#playlist button i{color:#ff648b;font-size:10px;font-style:normal;text-transform:uppercase}
#chrome[data-drawer="open"] #chromeMain{opacity:0;pointer-events:none;transform:translateX(-32px)}#chrome[data-drawer="open"] #playlistPanel{opacity:1;pointer-events:auto;transform:none}
body[data-chrome="hidden"]{cursor:none}body[data-chrome="hidden"] #chrome{opacity:0;pointer-events:none;transform:translateY(calc(-100% - 24px));box-shadow:none}body[data-chrome="hidden"] #chromeShade{opacity:0}
#poster,#error{position:fixed;inset:0}
#poster{display:grid;place-items:center;overflow:hidden;background:radial-gradient(circle at 50% 35%,rgba(238,72,126,.22),transparent 46%),linear-gradient(145deg,#15131e,#07070b 70%);pointer-events:none}
#poster img{width:100%;height:100%;object-fit:cover;filter:brightness(.66) saturate(.9)}
#poster .copy{position:absolute;z-index:2;display:grid;max-width:min(560px,80%);justify-items:center;gap:8px;text-align:center;text-shadow:0 3px 18px #000}
#poster strong{font-size:clamp(18px,3vw,34px)}#poster small{color:#b6afb9;font-size:13px}
#error{display:none;place-items:center;padding:28px;background:#050508}
#error>div{display:grid;width:min(470px,90%);justify-items:center;gap:12px;padding:28px;border:1px solid rgba(255,255,255,.15);border-radius:18px;background:#15131e;text-align:center}
#error strong{font-size:16px}#error small{color:#aaa2ad}#error button{padding:10px 16px;border:0;border-radius:10px;background:#f15079;color:#251018;font-weight:800;cursor:pointer}
[hidden]{display:none!important}
@media(max-width:980px){#chromeMain{grid-template-columns:auto minmax(0,1fr) auto auto}#position{display:none}#back span:last-child,#finish span:last-child{display:none}#back,#finish{width:44px;padding:0!important}}
@media(max-width:680px){#chrome{top:8px;right:8px;left:8px;min-height:56px;border-radius:12px}#chromeMain{min-height:56px;gap:7px;padding:6px}#playerLabel,#videoItem{display:none}#videoTitle{margin:0;font-size:13px}#back,#finish,#playlistToggle{width:40px;min-height:40px}#playlistPanel header strong{display:none}#playlist li{min-width:min(260px,58vw)}}
@media(prefers-reduced-motion:reduce){#chrome,#chromeShade,#chromeMain,#playlistPanel{transition:none}}
</style>
</head>
<body data-fit="contain" data-chrome="visible">
<video id="video" preload="metadata" playsinline></video>
<div id="chromeShade"></div>
<section id="chrome" data-drawer="closed">
  <div id="chromeMain">
    <button id="back" type="button"><span class="glyph">←</span><span id="backLabel"></span></button>
    <div id="videoIdentity"><span id="playerLabel"></span><strong id="videoTitle"></strong><small id="videoItem"></small></div>
    <div id="position"><strong id="positionCurrent">0</strong><span>/ <span id="positionTotal">0</span></span></div>
    <button id="finish" type="button"><span class="glyph">✓</span><span id="finishLabel"></span></button>
    <button id="playlistToggle" type="button"><span class="glyph">▤</span></button>
  </div>
  <div id="playlistPanel">
    <header><span class="glyph">▣</span><strong id="playlistLabel"></strong><span id="playlistCount">0</span></header>
    <ol id="playlist"></ol>
    <button id="playlistClose" type="button"><span class="glyph">▤</span></button>
  </div>
</section>
<div id="poster"><img id="posterImage" alt="" hidden><div class="copy"><strong id="title"></strong><small id="itemName"></small></div></div>
<div id="error"><div><strong id="errorTitle"></strong><small id="errorDetail"></small><button id="retry" type="button"></button></div></div>
<script>
(() => {
  const source = __DLA_VIDEO_SOURCE__;
  const subtitles = __DLA_VIDEO_SUBTITLES__;
  const video = document.querySelector("#video");
  const poster = document.querySelector("#poster");
  const posterImage = document.querySelector("#posterImage");
  const error = document.querySelector("#error");
  const chrome = document.querySelector("#chrome");
  const playlist = document.querySelector("#playlist");
  const finish = document.querySelector("#finish");
  let configured = false;
  let pendingPosition = 0;
  let autoPlay = false;
  let lastInteraction = 0;
  let chromeTimer = 0;
  let drawerOpen = false;
  let finished = false;
  let labels = {};
  const trackElements = subtitles.map(track => {
    const element = document.createElement("track");
    element.kind = "subtitles";
    element.label = track.label;
    if (track.language) element.srclang = track.language;
    element.src = track.source;
    element.addEventListener("error", () => snapshot("subtitle_error"));
    video.append(element);
    return { ...track, element };
  });
  const activeSubtitle = () => {
    const index = Array.from(video.textTracks).findIndex(track => track.mode === "showing");
    return index < 0 ? null : index;
  };
  const finite = value => Number.isFinite(value) ? value : null;
  const snapshot = (kind, errorCode = null, actionOrdinal = null) => {
    document.title = "dla-video-state:" + JSON.stringify({
      kind,
      positionSeconds: finite(video.currentTime) ?? 0,
      durationSeconds: finite(video.duration),
      paused: video.paused,
      ended: video.ended,
      readyState: video.readyState,
      errorCode,
      subtitleTrack: activeSubtitle(),
      actionOrdinal,
      nonce: Date.now(),
    });
  };
  const scheduleChromeHide = () => {
    clearTimeout(chromeTimer);
    if (video.paused || drawerOpen || error.style.display === "grid") return;
    chromeTimer = setTimeout(() => { document.body.dataset.chrome = "hidden"; }, 2600);
  };
  const revealChrome = () => {
    document.body.dataset.chrome = "visible";
    scheduleChromeHide();
  };
  const setDrawerOpen = open => {
    drawerOpen = Boolean(open);
    chrome.dataset.drawer = drawerOpen ? "open" : "closed";
    revealChrome();
  };
  const renderPlaylist = (items, ordinal, nowPlayingLabel) => {
    playlist.replaceChildren();
    items.forEach((item, index) => {
      const row = document.createElement("li");
      const button = document.createElement("button");
      const position = document.createElement("span");
      const name = document.createElement("strong");
      position.textContent = String(index + 1).padStart(2, "0");
      name.textContent = item.itemName || "";
      button.type = "button";
      button.append(position, name);
      if (item.ordinal === ordinal) {
        button.className = "is-active";
        button.ariaCurrent = "true";
        const active = document.createElement("i");
        active.textContent = nowPlayingLabel || "";
        button.append(active);
      }
      button.addEventListener("click", () => snapshot("choose", null, item.ordinal));
      row.append(button);
      playlist.append(row);
    });
  };
  const selectSubtitle = index => {
    const selected = Number.isInteger(index) ? index : -1;
    Array.from(video.textTracks).forEach((track, trackIndex) => {
      track.mode = trackIndex === selected ? "showing" : "disabled";
    });
    snapshot("subtitle");
  };
  const applyVideoFit = () => {
    if (!video.videoWidth || !video.videoHeight) return;
    const viewportWidth = document.documentElement.clientWidth;
    const viewportHeight = document.documentElement.clientHeight;
    const fit = document.body.dataset.fit || "contain";
    const pixelRatio = Math.max(1, window.devicePixelRatio || 1);
    const sourceWidth = fit === "original" ? video.videoWidth / pixelRatio : video.videoWidth;
    const sourceHeight = fit === "original" ? video.videoHeight / pixelRatio : video.videoHeight;
    const scale = fit === "cover"
      ? Math.max(viewportWidth / sourceWidth, viewportHeight / sourceHeight)
      : Math.min(fit === "original" ? 1 : Number.POSITIVE_INFINITY, viewportWidth / sourceWidth, viewportHeight / sourceHeight);
    video.style.width = Math.max(1, Math.round(sourceWidth * scale)) + "px";
    video.style.height = Math.max(1, Math.round(sourceHeight * scale)) + "px";
  };
  const setVideoFit = fit => {
    document.body.dataset.fit = fit || "contain";
    applyVideoFit();
  };
  const applyPosition = () => {
    if (!Number.isFinite(pendingPosition) || pendingPosition <= 0 || !Number.isFinite(video.duration)) return;
    video.currentTime = Math.min(pendingPosition, Math.max(0, video.duration - .25));
    pendingPosition = 0;
  };
  const play = async () => {
    error.style.display = "none";
    try { await video.play(); }
    catch { snapshot("error", video.error?.code ?? 4); }
  };
  const pause = () => video.pause();
  window.__dlaVideo = {
    configure(next) {
      configured = true;
      labels = next.labels || {};
      pendingPosition = Number(next.positionSeconds) || 0;
      autoPlay = Boolean(next.autoPlay);
      video.volume = Math.max(0, Math.min(1, Number(next.volume) || 0));
      video.muted = Boolean(next.muted);
      setVideoFit(next.fit);
      document.querySelector("#title").textContent = next.title || "";
      document.querySelector("#itemName").textContent = next.itemName || "";
      document.querySelector("#backLabel").textContent = next.labels?.back || "";
      document.querySelector("#playerLabel").textContent = next.labels?.player || "";
      document.querySelector("#videoTitle").textContent = next.title || "";
      document.querySelector("#videoItem").textContent = next.itemName || "";
      const currentIndex = Array.isArray(next.playlist)
        ? next.playlist.findIndex(item => item.ordinal === next.ordinal)
        : -1;
      document.querySelector("#positionCurrent").textContent = String(currentIndex < 0 ? 0 : currentIndex + 1);
      document.querySelector("#positionTotal").textContent = String(Array.isArray(next.playlist) ? next.playlist.length : 0);
      finished = Boolean(next.completed);
      finish.disabled = finished;
      document.querySelector("#finishLabel").textContent = finished
        ? next.labels?.completed || ""
        : next.labels?.markFinished || "";
      document.querySelector("#playlistLabel").textContent = next.labels?.playlist || "";
      document.querySelector("#playlistCount").textContent = String(Array.isArray(next.playlist) ? next.playlist.length : 0);
      document.querySelector("#playlistToggle").ariaLabel = next.labels?.openPlaylist || "";
      document.querySelector("#playlistClose").ariaLabel = next.labels?.closePlaylist || "";
      renderPlaylist(Array.isArray(next.playlist) ? next.playlist : [], next.ordinal, next.labels?.nowPlaying || "");
      document.querySelector("#errorTitle").textContent = next.labels?.playbackFailed || "";
      document.querySelector("#errorDetail").textContent = next.labels?.codecUnsupported || "";
      document.querySelector("#retry").textContent = next.labels?.retry || "";
      const preferredSubtitle = typeof next.subtitleLabel === "string"
        ? trackElements.findIndex(track => track.label.toLocaleLowerCase() === next.subtitleLabel.toLocaleLowerCase())
        : -1;
      selectSubtitle(preferredSubtitle);
      if (next.posterUrl) { posterImage.src = next.posterUrl; posterImage.hidden = false; }
      revealChrome();
      if (video.readyState >= 1) applyPosition();
      if (autoPlay && video.readyState >= 3) void play();
    },
    command(request) {
      switch (request.command) {
        case "play": void play(); break;
        case "pause": pause(); break;
        case "seek": video.currentTime = Math.max(0, Math.min(Number(request.value) || 0, Number.isFinite(video.duration) ? video.duration : Number.MAX_SAFE_INTEGER)); break;
        case "volume": video.volume = Math.max(0, Math.min(1, Number(request.value) || 0)); break;
        case "muted": video.muted = Boolean(request.enabled); break;
        case "fit": setVideoFit(request.fit); break;
        case "subtitle": selectSubtitle(request.subtitleTrack); break;
        case "retry": error.style.display = "none"; video.load(); break;
      }
    }
  };
  video.addEventListener("loadstart", () => snapshot("loading"));
  video.addEventListener("loadedmetadata", () => { applyVideoFit(); applyPosition(); snapshot("metadata"); });
  video.addEventListener("canplay", () => { snapshot("ready"); if (configured && autoPlay && video.paused) void play(); });
  video.addEventListener("playing", () => { poster.hidden = true; snapshot("playing"); scheduleChromeHide(); });
  video.addEventListener("pause", () => { revealChrome(); snapshot("paused"); });
  video.addEventListener("timeupdate", () => snapshot("time"));
  video.addEventListener("waiting", () => snapshot("waiting"));
  video.addEventListener("ended", () => { revealChrome(); snapshot("ended"); });
  video.addEventListener("error", () => { poster.hidden = true; error.style.display = "grid"; revealChrome(); snapshot("error", video.error?.code ?? 4); });
  const toggle = () => video.paused ? void play() : pause();
  video.addEventListener("click", () => { revealChrome(); toggle(); });
  document.querySelector("#retry").addEventListener("click", () => window.__dlaVideo.command({ command: "retry" }));
  document.querySelector("#back").addEventListener("click", () => snapshot("back"));
  finish.addEventListener("click", () => {
    if (finished) return;
    finished = true;
    finish.disabled = true;
    finish.querySelector("#finishLabel").textContent = labels.completed || finish.querySelector("#finishLabel").textContent;
    snapshot("finish");
  });
  document.querySelector("#playlistToggle").addEventListener("click", () => setDrawerOpen(true));
  document.querySelector("#playlistClose").addEventListener("click", () => setDrawerOpen(false));
  chrome.addEventListener("pointermove", revealChrome);
  chrome.addEventListener("focusin", revealChrome);
  addEventListener("pointermove", () => { revealChrome(); const now = Date.now(); if (now - lastInteraction > 250) { lastInteraction = now; snapshot("interaction"); } });
  addEventListener("pointerleave", scheduleChromeHide);
  addEventListener("resize", applyVideoFit);
  addEventListener("keydown", event => {
    revealChrome();
    const key = event.key.toLowerCase();
    if (event.key === "Escape" && drawerOpen) { event.preventDefault(); setDrawerOpen(false); }
    else if (event.key === " " || key === "k") { event.preventDefault(); toggle(); }
    else if (event.key === "ArrowLeft") { event.preventDefault(); video.currentTime = Math.max(0, video.currentTime - 15); }
    else if (event.key === "ArrowRight") { event.preventDefault(); video.currentTime = Math.min(video.duration || Number.MAX_SAFE_INTEGER, video.currentTime + 15); }
    else if (event.key === "ArrowUp") { event.preventDefault(); video.volume = Math.min(1, video.volume + .05); }
    else if (event.key === "ArrowDown") { event.preventDefault(); video.volume = Math.max(0, video.volume - .05); }
    else if (key === "m") { event.preventDefault(); video.muted = !video.muted; }
    else if (key === "c" && trackElements.length) { event.preventDefault(); const current = activeSubtitle(); selectSubtitle(current === null ? 0 : current + 1 < trackElements.length ? current + 1 : null); }
    else if (key === "f") { event.preventDefault(); snapshot("fullscreen"); }
  });
  video.src = source;
  video.load();
})();
</script>
</body>
</html>"##;

fn parse_locator(path: &str) -> Result<(MediaSessionId, u32), ProtocolError> {
    let decoded = percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| ProtocolError::BadRequest)?;
    let mut segments = decoded.trim_matches('/').split('/');
    let session_id = segments.next().filter(|value| !value.is_empty());
    let ordinal = segments.next().and_then(|value| value.parse::<u32>().ok());
    if segments.next().is_some() {
        return Err(ProtocolError::BadRequest);
    }
    match (session_id, ordinal) {
        (Some(session_id), Some(ordinal)) if session_id.starts_with("media-") => {
            Ok((MediaSessionId(session_id.to_owned()), ordinal))
        }
        _ => Err(ProtocolError::BadRequest),
    }
}

fn parse_subtitle_locator(path: &str) -> Result<(MediaSessionId, u32, u32), ProtocolError> {
    let decoded = percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| ProtocolError::BadRequest)?;
    let mut segments = decoded.trim_matches('/').split('/');
    let session_id = segments.next().filter(|value| !value.is_empty());
    let ordinal = segments.next().and_then(|value| value.parse::<u32>().ok());
    let track_index = segments.next().and_then(|value| value.parse::<u32>().ok());
    if segments.next().is_some() {
        return Err(ProtocolError::BadRequest);
    }
    match (session_id, ordinal, track_index) {
        (Some(session_id), Some(ordinal), Some(track_index))
            if session_id.starts_with("media-") =>
        {
            Ok((MediaSessionId(session_id.to_owned()), ordinal, track_index))
        }
        _ => Err(ProtocolError::BadRequest),
    }
}

fn resolve_path(root: &str, relative: &str) -> Result<PathBuf, ProtocolError> {
    let root_path = Path::new(root);
    let attempted = root_path.join(relative);
    let canonical_root = root_path
        .canonicalize()
        .map_err(|error| map_asset_io_error(error, &attempted))?;
    let canonical_target = canonical_root
        .join(relative)
        .canonicalize()
        .map_err(|error| map_asset_io_error(error, &canonical_root.join(relative)))?;
    if !canonical_target.starts_with(&canonical_root) || !canonical_target.is_file() {
        return Err(ProtocolError::Forbidden);
    }
    Ok(canonical_target)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ByteRange {
    start: u64,
    end: u64,
}

impl ByteRange {
    fn length(self) -> u64 {
        self.end - self.start + 1
    }
}

fn parse_range(value: &str, length: u64) -> Result<ByteRange, ProtocolError> {
    let raw = value
        .strip_prefix("bytes=")
        .filter(|value| !value.contains(','))
        .ok_or(ProtocolError::RangeNotSatisfiable(length))?;
    let (start, end) = raw
        .split_once('-')
        .ok_or(ProtocolError::RangeNotSatisfiable(length))?;
    if length == 0 {
        return Err(ProtocolError::RangeNotSatisfiable(length));
    }
    let (start, requested_end) = if start.is_empty() {
        let suffix = end
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or(ProtocolError::RangeNotSatisfiable(length))?;
        (length.saturating_sub(suffix.min(length)), length - 1)
    } else {
        let start = start
            .parse::<u64>()
            .map_err(|_| ProtocolError::RangeNotSatisfiable(length))?;
        if start >= length {
            return Err(ProtocolError::RangeNotSatisfiable(length));
        }
        let end = if end.is_empty() {
            length - 1
        } else {
            end.parse::<u64>()
                .map_err(|_| ProtocolError::RangeNotSatisfiable(length))?
                .min(length - 1)
        };
        if end < start {
            return Err(ProtocolError::RangeNotSatisfiable(length));
        }
        (start, end)
    };
    let capped_end = requested_end.min(start.saturating_add(MAX_RANGE_BYTES - 1));
    Ok(ByteRange {
        start,
        end: capped_end,
    })
}

fn media_content_type(media_type: MediaType, path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match (media_type, extension.as_str()) {
        (MediaType::Audio, "aac") => "audio/aac",
        (MediaType::Audio, "flac") => "audio/flac",
        (MediaType::Audio, "m4a") => "audio/mp4",
        (MediaType::Audio, "mp3") => "audio/mpeg",
        (MediaType::Audio, "ogg" | "opus") => "audio/ogg",
        (MediaType::Audio, "wav") => "audio/wav",
        (MediaType::Audio, "wma") => "audio/x-ms-wma",
        (MediaType::Image, "avif") => "image/avif",
        (MediaType::Image, "bmp") => "image/bmp",
        (MediaType::Image, "gif") => "image/gif",
        (MediaType::Image, "jpeg" | "jpg") => "image/jpeg",
        (MediaType::Image, "png") => "image/png",
        (MediaType::Image, "webp") => "image/webp",
        (MediaType::Pdf, _) => "application/pdf",
        (MediaType::Video, "avi") => "video/x-msvideo",
        (MediaType::Video, "m4v" | "mp4") => "video/mp4",
        (MediaType::Video, "mkv") => "video/x-matroska",
        (MediaType::Video, "mov") => "video/quicktime",
        (MediaType::Video, "webm") => "video/webm",
        (MediaType::Video, "wmv") => "video/x-ms-wmv",
        (MediaType::Audio, _) => "application/octet-stream",
        (MediaType::Image, _) => "application/octet-stream",
        (MediaType::Video, _) => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

fn map_media_error(session_id: &MediaSessionId, ordinal: u32, error: MediaError) -> ProtocolError {
    match error {
        MediaError::SessionNotFound(_)
        | MediaError::ItemNotFound(_)
        | MediaError::SessionClosed => ProtocolError::NotFound,
        MediaError::InvalidRequest(_) => ProtocolError::BadRequest,
        MediaError::InstallationNotFound(_)
        | MediaError::NeedsReview
        | MediaError::NotReviewed
        | MediaError::MissingAction
        | MediaError::MissingContentTarget
        | MediaError::UnsupportedAction
        | MediaError::IgnoredTarget
        | MediaError::EmptyInventory
        | MediaError::InsufficientVoiceActivity(_)
        | MediaError::InvalidProgressState
        | MediaError::Inventory(_)
        | MediaError::Waveform(_)
        | MediaError::Persistence(_)
        | MediaError::Preference(_)
        | MediaError::Activity(_)
        | MediaError::Library(_)
        | MediaError::Package(_) => {
            log::error!(
                "media asset lookup failed for session {} track {}: {}",
                session_id.0,
                ordinal,
                error
            );
            ProtocolError::Internal
        }
    }
}

fn map_asset_io_error(error: std::io::Error, path: &Path) -> ProtocolError {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            log::warn!(
                "media asset is indexed but missing on disk: {}",
                path.display()
            );
            ProtocolError::AssetMissing
        }
        std::io::ErrorKind::PermissionDenied => ProtocolError::Forbidden,
        _ => ProtocolError::Internal,
    }
}

#[derive(Debug)]
enum ProtocolError {
    BadRequest,
    Forbidden,
    NotFound,
    AssetMissing,
    MethodNotAllowed,
    RangeNotSatisfiable(u64),
    FileTooLarge,
    SubtitleInvalid,
    Internal,
}

impl ProtocolError {
    fn response(error: tauri::http::Error) -> Self {
        let _ = error;
        Self::Internal
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::AssetMissing => StatusCode::GONE,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::RangeNotSatisfiable(_) => StatusCode::RANGE_NOT_SATISFIABLE,
            Self::FileTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::SubtitleInvalid => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

fn protocol_error_message(error: ProtocolError) -> String {
    match error {
        ProtocolError::BadRequest => "the video locator is invalid",
        ProtocolError::Forbidden => "the video is outside the approved installation",
        ProtocolError::NotFound => "the video session or item no longer exists",
        ProtocolError::AssetMissing => "the indexed video is missing on disk",
        ProtocolError::MethodNotAllowed => "the video request method is not supported",
        ProtocolError::RangeNotSatisfiable(_) => "the requested video range is invalid",
        ProtocolError::FileTooLarge => "the media item is too large to materialize",
        ProtocolError::SubtitleInvalid => "the subtitle could not be converted",
        ProtocolError::Internal => "the video could not be resolved",
    }
    .to_owned()
}

fn error_response(error: ProtocolError) -> Response<Vec<u8>> {
    let mut builder = Response::builder()
        .status(error.status())
        .header(ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(CACHE_CONTROL, "no-store")
        .header(CONTENT_TYPE, "text/plain; charset=utf-8");
    if let ProtocolError::RangeNotSatisfiable(length) = error {
        builder = builder.header(CONTENT_RANGE, format!("bytes */{length}"));
    }
    builder
        .body(Vec::new())
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_accepts_only_opaque_session_and_ordinal() {
        assert_eq!(
            parse_locator("/media-123/4").expect("locator"),
            (MediaSessionId("media-123".to_owned()), 4)
        );
        assert!(parse_locator("/tmp/private.mp3").is_err());
        assert!(parse_locator("/media-123/4/more").is_err());
    }

    #[test]
    fn locator_decodes_the_path_encoded_by_tauri_convert_file_src() {
        assert_eq!(
            parse_locator("/media-123%2F4").expect("encoded locator"),
            (MediaSessionId("media-123".to_owned()), 4)
        );
        assert!(parse_locator("/media-123%252F4").is_err());
        assert!(parse_locator("/media-123%2F4%2Fmore").is_err());
        assert!(parse_locator("/media-123%2F%FF").is_err());
    }

    #[test]
    fn subtitle_locator_requires_an_opaque_track_index() {
        assert_eq!(
            parse_subtitle_locator("/media-123/4/2").expect("subtitle locator"),
            (MediaSessionId("media-123".to_owned()), 4, 2)
        );
        assert!(parse_subtitle_locator("/media-123/4").is_err());
        assert!(parse_subtitle_locator("/media-123/4/2/more").is_err());
    }

    #[test]
    fn a_missing_file_is_distinguishable_from_an_unknown_item() {
        assert_eq!(
            map_asset_io_error(
                std::io::Error::from(std::io::ErrorKind::NotFound),
                Path::new("/indexed/missing.mp3"),
            )
            .status(),
            StatusCode::GONE
        );
        assert_eq!(ProtocolError::NotFound.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            map_media_error(
                &MediaSessionId("media-unknown".to_owned()),
                0,
                MediaError::SessionNotFound("media-unknown".to_owned()),
            )
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            map_media_error(
                &MediaSessionId("media-broken".to_owned()),
                0,
                MediaError::Persistence("database unavailable".to_owned()),
            )
            .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            map_media_error(
                &MediaSessionId("media-inconsistent".to_owned()),
                0,
                MediaError::InstallationNotFound("installation-missing".to_owned()),
            )
            .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            map_asset_io_error(
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                Path::new("/indexed/forbidden.mp3"),
            )
            .status(),
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn ranges_are_validated_and_capped() {
        assert_eq!(
            parse_range("bytes=10-19", 100).expect("range"),
            ByteRange { start: 10, end: 19 }
        );
        assert_eq!(
            parse_range("bytes=-10", 100).expect("suffix"),
            ByteRange { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range("bytes=0-", MAX_RANGE_BYTES * 2).expect("capped"),
            ByteRange {
                start: 0,
                end: MAX_RANGE_BYTES - 1,
            }
        );
        assert!(parse_range("bytes=100-", 100).is_err());
        assert!(parse_range("items=0-10", 100).is_err());
    }

    #[test]
    fn materialized_media_is_bounded_by_declared_and_observed_size() {
        assert_eq!(
            read_materialized_media(&b"small"[..], 5, 8).expect("bounded media"),
            b"small"
        );
        assert!(matches!(
            read_materialized_media(&b"small"[..], 9, 8),
            Err(MaterializedMediaError::TooLarge)
        ));
        assert!(matches!(
            read_materialized_media(&b"grew-past-limit"[..], 4, 8),
            Err(MaterializedMediaError::TooLarge)
        ));
    }

    #[test]
    fn video_document_keeps_the_source_inside_its_script_literal() {
        let document = video_document("file:///tmp/<script>&video.mp4", &[]);

        assert!(document.contains("file:///tmp/\\u003cscript\\u003e\\u0026video.mp4"));
        assert!(!document.contains("file:///tmp/<script>"));
        assert!(!document.contains("__DLA_VIDEO_SOURCE__"));
        assert!(!document.contains("__DLA_VIDEO_SUBTITLES__"));
    }

    #[test]
    fn video_document_reports_loading_without_a_center_overlay() {
        let document = video_document("dla-media://localhost/session/0", &[]);

        assert!(!document.contains("id=\"posterPlay\""));
        assert!(!document.contains("id=\"loading\""));
        assert!(document.contains("snapshot(\"loading\")"));
        assert!(document.contains("snapshot(\"waiting\")"));
    }
}
