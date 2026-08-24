use std::{
    backtrace::Backtrace,
    ffi::OsStr,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, Once, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use dla_application::support::{
    FrontendFaultReport, RedactionRoot, SUPPORT_BUNDLE_BYTES, SUPPORT_FAULT_FILE_COUNT,
    SUPPORT_FAULT_TEXT_BYTES, SUPPORT_LOG_FILE_BYTES, SUPPORT_LOG_FILE_COUNT,
    SUPPORT_SCHEMA_VERSION, SupportBundleEntry, SupportBundleManifest, SupportFaultKind,
    SupportFaultRecord, SupportFaultSummary, SupportSaveOutcome, SupportSaveResult, SupportStatus,
    bounded_text, redact_text, single_line,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

const ISSUE_URL: &str = env!("DLA_SUPPORT_ISSUES_URL");
const APP_VERSION: &str = env!("DLA_LAUNCHER_VERSION");
const BUILD_ID: &str = env!("DLA_BUILD_ID");
const RUN_MARKER_FILE: &str = "active-run.json";
const ACKNOWLEDGED_FILE: &str = "acknowledged-run";

#[derive(Clone)]
pub struct SupportRuntime {
    inner: Arc<SupportInner>,
}

struct SupportInner {
    fault_directory: PathBuf,
    log_directory: PathBuf,
    marker_path: PathBuf,
    acknowledged_path: PathBuf,
    run_id: String,
    track_session: bool,
    previous: Mutex<PreviousRun>,
    redaction_roots: Vec<RedactionRoot>,
}

#[derive(Clone, Debug, Default)]
struct PreviousRun {
    unclean: bool,
    run_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunMarker {
    schema_version: u32,
    run_id: String,
    started_at: String,
    startup_ready: bool,
}

#[derive(Clone)]
struct BundlePayload {
    name: String,
    contents: Vec<u8>,
}

impl SupportRuntime {
    pub fn initialize(app: &tauri::App, track_session: bool) -> Result<Self, String> {
        let support_directory = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?
            .join("support");
        let fault_directory = support_directory.join("faults");
        let log_directory = app
            .path()
            .app_log_dir()
            .map_err(|error| error.to_string())?;
        fs::create_dir_all(&fault_directory).map_err(|error| error.to_string())?;
        fs::create_dir_all(&log_directory).map_err(|error| error.to_string())?;
        secure_directory(&support_directory)?;
        secure_directory(&fault_directory)?;
        secure_directory(&log_directory)?;
        let marker_path = support_directory.join(RUN_MARKER_FILE);
        let acknowledged_path = support_directory.join(ACKNOWLEDGED_FILE);
        let previous_marker = read_json::<RunMarker>(&marker_path);
        let acknowledged = fs::read_to_string(&acknowledged_path).unwrap_or_default();
        let previous = previous_marker
            .map(|marker| PreviousRun {
                unclean: marker.run_id != acknowledged.trim(),
                run_id: marker.run_id,
            })
            .unwrap_or_default();
        let run_id = Uuid::new_v4().to_string();
        let redaction_roots = redaction_roots(app, &support_directory, &log_directory);
        let runtime = Self {
            inner: Arc::new(SupportInner {
                fault_directory,
                log_directory,
                marker_path,
                acknowledged_path,
                run_id,
                track_session,
                previous: Mutex::new(previous),
                redaction_roots,
            }),
        };
        runtime.prune_faults();
        if track_session {
            runtime.write_run_marker(false)?;
        }
        Ok(runtime)
    }

    pub fn run_id(&self) -> &str {
        &self.inner.run_id
    }

    pub fn log_directory(&self) -> &Path {
        &self.inner.log_directory
    }

    pub fn redaction_roots(&self) -> Vec<RedactionRoot> {
        self.inner.redaction_roots.clone()
    }

    pub fn mark_startup_ready(&self) -> Result<(), String> {
        if self.inner.track_session {
            self.write_run_marker(true)?;
        }
        log::info!(target: "dla::lifecycle", "event=startup_ready run_id={}", self.run_id());
        Ok(())
    }

    pub fn mark_clean_shutdown(&self) -> bool {
        if !self.inner.track_session {
            return false;
        }
        let Some(marker) = read_json::<RunMarker>(&self.inner.marker_path) else {
            return false;
        };
        if marker.run_id == self.inner.run_id {
            return fs::remove_file(&self.inner.marker_path).is_ok();
        }
        false
    }

    pub fn record_startup_failure(&self, error: &str) {
        self.record_fault(SupportFaultKind::StartupFailure, error, "", "");
    }

    pub fn record_frontend_fault(&self, report: FrontendFaultReport) {
        self.record_fault(
            report.kind,
            &report.message,
            &report.stack,
            &report.component_stack,
        );
    }

    pub fn record_fault(
        &self,
        kind: SupportFaultKind,
        message: &str,
        stack: &str,
        component_stack: &str,
    ) {
        let record = SupportFaultRecord {
            schema_version: SUPPORT_SCHEMA_VERSION,
            kind,
            occurred_at: now(),
            run_id: self.inner.run_id.clone(),
            message: single_line(&self.redact(message), 4 * 1024),
            stack: bounded_text(&self.redact(stack), SUPPORT_FAULT_TEXT_BYTES),
            component_stack: bounded_text(&self.redact(component_stack), SUPPORT_FAULT_TEXT_BYTES),
        };
        let stamp = OffsetDateTime::now_utc().unix_timestamp_nanos();
        let path = self
            .inner
            .fault_directory
            .join(format!("fault-{stamp}-{}.json", fault_slug(&record.kind)));
        if let Ok(bytes) = serde_json::to_vec_pretty(&record) {
            let _ = write_private(&path, &bytes);
        }
        self.prune_faults();
        log::error!(
            target: "dla::fault",
            "event=fault_recorded kind={} run_id={} message={}",
            fault_slug(&record.kind),
            self.run_id(),
            record.message
        );
    }

    pub fn status(&self) -> SupportStatus {
        let previous = self
            .inner
            .previous
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let logs = self.collect_regular_files(
            &self.inner.log_directory,
            SUPPORT_LOG_FILE_COUNT,
            SUPPORT_LOG_FILE_BYTES,
            Some("log"),
        );
        let faults = self.collect_regular_files(
            &self.inner.fault_directory,
            SUPPORT_FAULT_FILE_COUNT,
            SUPPORT_FAULT_TEXT_BYTES as u64 * 3,
            Some("json"),
        );
        let last_fault = faults
            .first()
            .and_then(|path| read_json::<SupportFaultRecord>(path))
            .map(|fault| SupportFaultSummary {
                kind: fault.kind,
                occurred_at: fault.occurred_at,
                message: fault.message,
            });
        let estimated_bundle_bytes = logs
            .iter()
            .filter_map(|path| fs::metadata(path).ok())
            .map(|metadata| metadata.len().min(SUPPORT_LOG_FILE_BYTES))
            .chain(
                faults
                    .iter()
                    .filter_map(|path| fs::metadata(path).ok())
                    .map(|metadata| metadata.len().min(SUPPORT_FAULT_TEXT_BYTES as u64 * 3)),
            )
            .sum::<u64>()
            .saturating_add(8 * 1024)
            .min(SUPPORT_BUNDLE_BYTES);
        let summary = self.summary(&previous, last_fault.as_ref());
        SupportStatus {
            schema_version: SUPPORT_SCHEMA_VERSION,
            previous_shutdown_unclean: previous.unclean,
            previous_run_id: previous.run_id,
            last_fault,
            retained_log_files: logs.len(),
            retained_fault_files: faults.len(),
            estimated_bundle_bytes,
            max_bundle_bytes: SUPPORT_BUNDLE_BYTES,
            summary,
        }
    }

    pub fn acknowledge_previous_run(&self) -> Result<(), String> {
        let mut previous = self
            .inner
            .previous
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !previous.run_id.is_empty() {
            write_private(&self.inner.acknowledged_path, previous.run_id.as_bytes())?;
        }
        previous.unclean = false;
        Ok(())
    }

    pub fn save_bundle(&self, destination: &Path) -> Result<SupportSaveResult, String> {
        let file_name = destination
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("dla-launcher-diagnostic.zip")
            .to_owned();
        let temporary = destination.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
        let result = self.write_bundle(&temporary);
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        replace_file(&temporary, destination)?;
        let bytes = fs::metadata(destination)
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        log::info!(target: "dla::support", "event=bundle_saved bytes={bytes}");
        Ok(SupportSaveResult {
            outcome: SupportSaveOutcome::Saved,
            file_name,
            bytes,
        })
    }

    pub fn issue_url(&self) -> String {
        use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

        let status = self.status();
        let title = format!("Bug report — DLA Launcher {APP_VERSION}");
        let body = format!(
            "## What happened\n\nPlease describe what you were doing.\n\n## Diagnostic summary\n\n```text\n{}\n```\n\nDiagnostic ZIP files may contain system and failure details. GitHub issue content and attachments are public.",
            bounded_text(&status.summary, 2 * 1024)
        );
        format!(
            "{}/new?title={}&body={}",
            ISSUE_URL.trim_end_matches('/'),
            utf8_percent_encode(&title, NON_ALPHANUMERIC),
            utf8_percent_encode(&body, NON_ALPHANUMERIC),
        )
    }

    pub fn project_url(&self) -> String {
        let issue_url = ISSUE_URL.trim_end_matches('/');
        issue_url
            .strip_suffix("/issues")
            .unwrap_or(issue_url)
            .to_owned()
    }

    pub fn suggested_file_name(&self) -> String {
        let stamp = OffsetDateTime::now_utc()
            .format(&time::macros::format_description!(
                "[year][month][day]T[hour][minute][second]Z"
            ))
            .unwrap_or_else(|_| "report".to_owned());
        format!("dla-launcher-diagnostic-{stamp}.zip")
    }

    fn write_run_marker(&self, startup_ready: bool) -> Result<(), String> {
        let marker = RunMarker {
            schema_version: SUPPORT_SCHEMA_VERSION,
            run_id: self.inner.run_id.clone(),
            started_at: now(),
            startup_ready,
        };
        let bytes = serde_json::to_vec_pretty(&marker).map_err(|error| error.to_string())?;
        write_private(&self.inner.marker_path, &bytes)
    }

    fn redact(&self, value: &str) -> String {
        redact_text(value, &self.inner.redaction_roots)
    }

    fn summary(&self, previous: &PreviousRun, fault: Option<&SupportFaultSummary>) -> String {
        let system = crate::system_report::read_system_report();
        let mut lines = vec![
            format!("DLA Launcher: {APP_VERSION}"),
            format!("Build: {BUILD_ID}"),
            format!(
                "Platform: {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            format!("System: {}", system.os_version),
            format!("Kernel: {}", system.kernel),
            format!("Renderer: {}", system.webview),
            format!(
                "Previous shutdown: {}",
                if previous.unclean {
                    "unclean"
                } else {
                    "clean or unavailable"
                }
            ),
        ];
        if let Some(fault) = fault {
            lines.push(format!(
                "Last fault: {} at {}",
                fault_slug(&fault.kind),
                fault.occurred_at
            ));
            lines.push(format!("Fault message: {}", fault.message));
        }
        self.redact(&lines.join("\n"))
    }

    fn write_bundle(&self, temporary: &Path) -> Result<(), String> {
        let status = self.status();
        let report_id = Uuid::new_v4().to_string();
        let report = format!(
            "DLA Launcher diagnostic report\nReport ID: {report_id}\nCreated: {}\n\n{}\n\nPrivacy\nThis archive contains only bounded launcher logs and fault records. It excludes databases, catalogs, packages, media, preferences, notes, and library files. Known and absolute filesystem paths are redacted.\n",
            now(),
            status.summary,
        );
        let mut payloads = vec![BundlePayload {
            name: "report.txt".to_owned(),
            contents: report.into_bytes(),
        }];
        let mut remaining =
            SUPPORT_BUNDLE_BYTES.saturating_sub(payloads[0].contents.len() as u64 + 16 * 1024);
        for (prefix, paths, per_file_limit) in [
            (
                "logs",
                self.collect_regular_files(
                    &self.inner.log_directory,
                    SUPPORT_LOG_FILE_COUNT,
                    SUPPORT_LOG_FILE_BYTES,
                    Some("log"),
                ),
                SUPPORT_LOG_FILE_BYTES,
            ),
            (
                "faults",
                self.collect_regular_files(
                    &self.inner.fault_directory,
                    SUPPORT_FAULT_FILE_COUNT,
                    SUPPORT_FAULT_TEXT_BYTES as u64 * 3,
                    Some("json"),
                ),
                SUPPORT_FAULT_TEXT_BYTES as u64 * 3,
            ),
        ] {
            for path in paths {
                if remaining == 0 {
                    break;
                }
                let limit = per_file_limit.min(remaining) as usize;
                let contents = self.read_redacted_file(&path, limit)?;
                if contents.is_empty() {
                    continue;
                }
                remaining = remaining.saturating_sub(contents.len() as u64);
                let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                    continue;
                };
                payloads.push(BundlePayload {
                    name: format!("{prefix}/{name}"),
                    contents,
                });
            }
        }
        let entries = payloads
            .iter()
            .map(|payload| SupportBundleEntry {
                name: payload.name.clone(),
                bytes: payload.contents.len() as u64,
            })
            .collect();
        let manifest = SupportBundleManifest {
            schema_version: SUPPORT_SCHEMA_VERSION,
            report_id,
            created_at: now(),
            app_version: APP_VERSION.to_owned(),
            build_id: BUILD_ID.to_owned(),
            platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            entries,
            excluded: vec![
                "databases".to_owned(),
                "catalog packages".to_owned(),
                "media and library files".to_owned(),
                "preferences and notes".to_owned(),
            ],
            redaction:
                "Known application roots and absolute filesystem paths are replaced before export."
                    .to_owned(),
        };
        payloads.push(BundlePayload {
            name: "manifest.json".to_owned(),
            contents: serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?,
        });
        let total = payloads
            .iter()
            .map(|payload| payload.contents.len() as u64)
            .sum::<u64>();
        if total > SUPPORT_BUNDLE_BYTES {
            return Err("support bundle exceeded its configured size limit".to_owned());
        }

        let file = create_private_file(temporary)?;
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o600);
        for payload in payloads {
            writer
                .start_file(payload.name, options)
                .map_err(|error| error.to_string())?;
            writer
                .write_all(&payload.contents)
                .map_err(|error| error.to_string())?;
        }
        let file = writer.finish().map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())
    }

    fn read_redacted_file(&self, path: &Path, limit: usize) -> Result<Vec<u8>, String> {
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Ok(Vec::new());
        }
        let file = File::open(path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
        file.take(limit as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        let redacted = self.redact(&text);
        Ok(bounded_text(&redacted, limit.saturating_sub(16)).into_bytes())
    }

    fn collect_regular_files(
        &self,
        directory: &Path,
        count: usize,
        _max_bytes: u64,
        extension: Option<&str>,
    ) -> Vec<PathBuf> {
        let mut files = fs::read_dir(directory)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).ok()?;
                if !metadata.file_type().is_file()
                    || metadata.file_type().is_symlink()
                    || extension.is_some_and(|value| path.extension() != Some(OsStr::new(value)))
                {
                    return None;
                }
                let modified = metadata.modified().ok();
                Some((modified, path))
            })
            .collect::<Vec<_>>();
        files.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        files
            .into_iter()
            .take(count)
            .map(|(_, path)| path)
            .collect()
    }

    fn prune_faults(&self) {
        let files = self.collect_regular_files(
            &self.inner.fault_directory,
            usize::MAX,
            u64::MAX,
            Some("json"),
        );
        for path in files.into_iter().skip(SUPPORT_FAULT_FILE_COUNT) {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn install_panic_capture(runtime: SupportRuntime) {
    static RUNTIME: OnceLock<SupportRuntime> = OnceLock::new();
    static INSTALL: Once = Once::new();
    static CAPTURING: AtomicBool = AtomicBool::new(false);
    let _ = RUNTIME.set(runtime);
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |information| {
            if let Some(runtime) = RUNTIME.get()
                && CAPTURING
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                let location = information
                    .location()
                    .map(|location| {
                        format!(
                            "{}:{}:{}",
                            location.file(),
                            location.line(),
                            location.column()
                        )
                    })
                    .unwrap_or_else(|| "unknown location".to_owned());
                let payload = information
                    .payload()
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| {
                        information
                            .payload()
                            .downcast_ref::<String>()
                            .map(String::as_str)
                    })
                    .unwrap_or("Rust panic");
                runtime.record_fault(
                    SupportFaultKind::RustPanic,
                    &format!("{payload} at {location}"),
                    &Backtrace::force_capture().to_string(),
                    "",
                );
                CAPTURING.store(false, Ordering::Release);
            }
            previous(information);
        }));
    });
}

pub fn install_logging(app: &tauri::App, runtime: &SupportRuntime) -> Result<(), String> {
    use tauri_plugin_log::{FileOpenStrategy, RotationStrategy, Target, TargetKind};

    let roots = runtime.redaction_roots();
    let run_id = runtime.run_id().to_owned();
    let mut targets = vec![Target::new(TargetKind::Folder {
        path: runtime.log_directory().to_path_buf(),
        file_name: Some("dla-launcher".to_owned()),
    })];
    if cfg!(debug_assertions) {
        targets.push(Target::new(TargetKind::Stdout));
    }
    app.handle()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .file_open_strategy(FileOpenStrategy::Rotate)
                .rotation_strategy(RotationStrategy::KeepSome(SUPPORT_LOG_FILE_COUNT - 1))
                .max_file_size(SUPPORT_LOG_FILE_BYTES as u128)
                .targets(targets)
                .format(move |out, message, record| {
                    let message = redact_text(&message.to_string(), &roots);
                    let line = serde_json::json!({
                        "timestamp": now(),
                        "level": record.level().to_string(),
                        "target": record.target(),
                        "runId": run_id,
                        "message": message,
                    });
                    out.finish(format_args!("{line}"));
                })
                .build(),
        )
        .map_err(|error| error.to_string())?;
    secure_directory(runtime.log_directory())?;
    for path in
        runtime.collect_regular_files(runtime.log_directory(), usize::MAX, u64::MAX, Some("log"))
    {
        secure_file(&path)?;
    }
    Ok(())
}

#[tauri::command]
pub fn read_support_status(runtime: tauri::State<'_, SupportRuntime>) -> SupportStatus {
    runtime.status()
}

#[tauri::command]
pub fn acknowledge_unclean_shutdown(
    runtime: tauri::State<'_, SupportRuntime>,
) -> Result<(), String> {
    runtime.acknowledge_previous_run()
}

#[tauri::command]
pub fn record_frontend_fault(
    runtime: tauri::State<'_, SupportRuntime>,
    report: FrontendFaultReport,
) {
    runtime.record_frontend_fault(report);
}

#[cfg(desktop)]
#[tauri::command]
pub async fn save_support_bundle(
    app: AppHandle,
    runtime: tauri::State<'_, SupportRuntime>,
) -> Result<SupportSaveResult, String> {
    save_with_dialog(app, runtime.inner().clone()).await
}

#[cfg(mobile)]
#[tauri::command]
pub async fn save_support_bundle(
    _app: AppHandle,
    _runtime: tauri::State<'_, SupportRuntime>,
) -> Result<SupportSaveResult, String> {
    Err("diagnostic export is not available on this platform yet".to_owned())
}

#[cfg(desktop)]
#[tauri::command]
pub fn open_support_issue(
    app: AppHandle,
    runtime: tauri::State<'_, SupportRuntime>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    app.opener()
        .open_url(runtime.issue_url(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[cfg(desktop)]
#[tauri::command]
pub fn open_support_project(
    app: AppHandle,
    runtime: tauri::State<'_, SupportRuntime>,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    app.opener()
        .open_url(runtime.project_url(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[cfg(mobile)]
#[tauri::command]
pub fn open_support_issue(
    _app: AppHandle,
    _runtime: tauri::State<'_, SupportRuntime>,
) -> Result<(), String> {
    Err("opening the issue tracker is not available on this platform yet".to_owned())
}

#[cfg(mobile)]
#[tauri::command]
pub fn open_support_project(
    _app: AppHandle,
    _runtime: tauri::State<'_, SupportRuntime>,
) -> Result<(), String> {
    Err("opening the project page is not available on this platform yet".to_owned())
}

#[cfg(desktop)]
pub async fn save_with_dialog(
    app: AppHandle,
    runtime: SupportRuntime,
) -> Result<SupportSaveResult, String> {
    use tauri_plugin_dialog::DialogExt;

    let suggested = runtime.suggested_file_name();
    tauri::async_runtime::spawn_blocking(move || {
        let selected = app
            .dialog()
            .file()
            .set_title("Save DLA Launcher diagnostic report")
            .set_file_name(suggested)
            .add_filter("ZIP archive", &["zip"])
            .blocking_save_file();
        let Some(selected) = selected else {
            return Ok(SupportSaveResult {
                outcome: SupportSaveOutcome::Cancelled,
                file_name: String::new(),
                bytes: 0,
            });
        };
        let path = selected.into_path().map_err(|error| error.to_string())?;
        runtime.save_bundle(&path)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn redaction_roots(
    app: &tauri::App,
    support_directory: &Path,
    log_directory: &Path,
) -> Vec<RedactionRoot> {
    let mut roots = vec![
        RedactionRoot {
            value: support_directory.to_string_lossy().into_owned(),
            replacement: "<support-data>",
        },
        RedactionRoot {
            value: log_directory.to_string_lossy().into_owned(),
            replacement: "<logs>",
        },
    ];
    for (path, replacement) in [
        (app.path().app_data_dir().ok(), "<app-data>"),
        (app.path().app_cache_dir().ok(), "<app-cache>"),
        (app.path().home_dir().ok(), "<home>"),
        (Some(std::env::temp_dir()), "<temp>"),
    ] {
        if let Some(path) = path {
            roots.push(RedactionRoot {
                value: path.to_string_lossy().into_owned(),
                replacement,
            });
        }
    }
    roots
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_private(path: &Path, contents: &[u8]) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = fs::OpenOptions::new();
        options.create(true).truncate(true).write(true).mode(0o600);
        let mut file = options.open(path).map_err(|error| error.to_string())?;
        file.write_all(contents)
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
    }
    #[cfg(not(unix))]
    {
        fs::write(path, contents).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn create_private_file(path: &Path) -> Result<File, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| error.to_string())
    }
    #[cfg(not(unix))]
    {
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|error| error.to_string())
    }
}

fn secure_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn secure_file(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn replace_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    match fs::rename(temporary, destination) {
        Ok(()) => Ok(()),
        Err(_error) if destination.is_file() => {
            fs::remove_file(destination).map_err(|remove_error| remove_error.to_string())?;
            fs::rename(temporary, destination).map_err(|rename_error| rename_error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

fn fault_slug(kind: &SupportFaultKind) -> &'static str {
    match kind {
        SupportFaultKind::RustPanic => "rust-panic",
        SupportFaultKind::FrontendRender => "frontend-render",
        SupportFaultKind::FrontendError => "frontend-error",
        SupportFaultKind::UnhandledRejection => "unhandled-rejection",
        SupportFaultKind::StartupFailure => "startup-failure",
    }
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(directory: &Path, previous_unclean: bool) -> SupportRuntime {
        runtime_with_tracking(directory, previous_unclean, false)
    }

    fn runtime_with_tracking(
        directory: &Path,
        previous_unclean: bool,
        track_session: bool,
    ) -> SupportRuntime {
        let support_directory = directory.join("support");
        let fault_directory = support_directory.join("faults");
        let log_directory = directory.join("logs");
        fs::create_dir_all(&fault_directory).expect("fault directory");
        fs::create_dir_all(&log_directory).expect("log directory");
        SupportRuntime {
            inner: Arc::new(SupportInner {
                fault_directory,
                log_directory,
                marker_path: support_directory.join(RUN_MARKER_FILE),
                acknowledged_path: support_directory.join(ACKNOWLEDGED_FILE),
                run_id: "test-run".to_owned(),
                track_session,
                previous: Mutex::new(PreviousRun {
                    unclean: previous_unclean,
                    run_id: "previous-run".to_owned(),
                }),
                redaction_roots: vec![RedactionRoot {
                    value: "/home/alice".to_owned(),
                    replacement: "<home>",
                }],
            }),
        }
    }

    #[test]
    fn clean_shutdown_removes_the_current_run_marker() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let runtime = runtime_with_tracking(directory.path(), false, true);
        runtime.write_run_marker(true).expect("write run marker");
        assert!(runtime.inner.marker_path.is_file());

        assert!(runtime.mark_clean_shutdown());

        assert!(!runtime.inner.marker_path.exists());
    }

    #[test]
    fn clean_shutdown_does_not_remove_another_run_marker() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let runtime = runtime_with_tracking(directory.path(), false, true);
        let marker = RunMarker {
            schema_version: SUPPORT_SCHEMA_VERSION,
            run_id: "another-run".to_owned(),
            started_at: now(),
            startup_ready: true,
        };
        write_private(
            &runtime.inner.marker_path,
            &serde_json::to_vec(&marker).expect("serialize marker"),
        )
        .expect("write run marker");

        assert!(!runtime.mark_clean_shutdown());
        assert!(runtime.inner.marker_path.is_file());
    }

    #[test]
    fn issue_destination_is_the_workspace_bug_tracker() {
        assert_eq!(
            ISSUE_URL,
            "https://github.com/DLA-Project/dla-launcher/issues"
        );
    }

    #[test]
    fn project_destination_is_derived_from_the_bug_tracker() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert_eq!(
            runtime(directory.path(), false).project_url(),
            "https://github.com/DLA-Project/dla-launcher"
        );
    }

    #[test]
    fn fault_names_do_not_expose_internal_type_names() {
        assert_eq!(
            fault_slug(&SupportFaultKind::FrontendRender),
            "frontend-render"
        );
        assert_eq!(fault_slug(&SupportFaultKind::RustPanic), "rust-panic");
    }

    #[test]
    fn private_writes_replace_the_previous_contents() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("record");
        write_private(&path, b"long contents").expect("first write");
        write_private(&path, b"short").expect("replacement write");
        assert_eq!(fs::read(path).expect("read record"), b"short");
    }

    #[test]
    fn bundle_is_allowlisted_bounded_and_redacted() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let runtime = runtime(directory.path(), true);
        fs::write(
            runtime.log_directory().join("dla-launcher.log"),
            "opened /home/alice/Games/private/game.exe\n",
        )
        .expect("write log");
        fs::write(
            runtime.log_directory().join("library.sqlite"),
            "must not be included",
        )
        .expect("write excluded database");
        runtime.record_fault(
            SupportFaultKind::FrontendError,
            "failed at /home/alice/project/main.tsx",
            "at /home/alice/project/main.tsx:1",
            "",
        );

        let destination = directory.path().join("diagnostic.zip");
        runtime.save_bundle(&destination).expect("save bundle");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&destination)
                    .expect("bundle metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let file = File::open(destination).expect("open bundle");
        let mut archive = zip::ZipArchive::new(file).expect("read bundle");
        let names = (0..archive.len())
            .map(|index| archive.by_index(index).expect("entry").name().to_owned())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "report.txt"));
        assert!(names.iter().any(|name| name == "manifest.json"));
        assert!(names.iter().any(|name| name.starts_with("logs/")));
        assert!(names.iter().any(|name| name.starts_with("faults/")));
        assert!(!names.iter().any(|name| name.ends_with("library.sqlite")));

        let mut log = String::new();
        archive
            .by_name("logs/dla-launcher.log")
            .expect("included log")
            .read_to_string(&mut log)
            .expect("read log");
        assert!(log.contains("<home>/Games/private/game.exe"));
        assert!(!log.contains("/home/alice"));

        let uncompressed = (0..archive.len())
            .map(|index| archive.by_index(index).expect("entry").size())
            .sum::<u64>();
        assert!(uncompressed <= SUPPORT_BUNDLE_BYTES);
    }

    #[test]
    fn acknowledgement_hides_only_the_previous_unclean_run() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let runtime = runtime(directory.path(), true);
        assert!(runtime.status().previous_shutdown_unclean);
        runtime
            .acknowledge_previous_run()
            .expect("acknowledge previous run");
        assert!(!runtime.status().previous_shutdown_unclean);
        assert_eq!(
            fs::read_to_string(&runtime.inner.acknowledged_path).expect("acknowledgement"),
            "previous-run"
        );
    }
}
