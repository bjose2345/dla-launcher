use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use dla_application::{
    scan_execution::{PreparedScan, ScanExecutionService},
    scanner::{
        PrepareScanRequest, ScanCancellation, ScanIssuePage, ScanIssueRequest, ScanProgressSink,
        ScanResultPage, ScanResultRequest, ScanRootPreference, ScanRootPreferenceService,
        ScanSessionView, ScannerError,
    },
};
use dla_domain::scanner::{ScanOptions, ScanProgress, ScanSessionId};
use dla_scanner::ScanAccessRegistry;
use tauri::{AppHandle, Emitter};

#[cfg(desktop)]
use dla_application::scanner::ScanRootLocation;
#[cfg(desktop)]
use dla_scanner::ApprovedScanRoot;
#[cfg(desktop)]
use std::path::Path;

pub const SCAN_PROGRESS_EVENT: &str = "scanner-progress";

pub struct TauriScanProgressSink {
    app: AppHandle,
}

impl TauriScanProgressSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ScanProgressSink for TauriScanProgressSink {
    fn publish(&self, progress: &ScanProgress) -> Result<(), ScannerError> {
        self.app
            .emit(SCAN_PROGRESS_EVENT, progress)
            .map_err(ScannerError::filesystem)
    }
}

pub struct ScanController {
    service: Arc<ScanExecutionService>,
    access: Arc<ScanAccessRegistry>,
    preferences: Arc<ScanRootPreferenceService>,
    active: Mutex<HashMap<ScanSessionId, Arc<AtomicScanCancellation>>>,
}

impl ScanController {
    pub fn new(
        service: Arc<ScanExecutionService>,
        access: Arc<ScanAccessRegistry>,
        preferences: Arc<ScanRootPreferenceService>,
    ) -> Self {
        Self {
            service,
            access,
            preferences,
            active: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(desktop)]
    pub fn approve_root(&self, path: &Path) -> Result<ApprovedScanRoot, ScannerError> {
        self.access.approve(path)
    }

    pub fn read_root_preference(&self) -> Result<ScanRootPreference, ScannerError> {
        self.preferences.read()
    }

    #[cfg(desktop)]
    pub fn configure_root(&self, path: &Path) -> Result<ScanRootPreference, ScannerError> {
        let approved = self.access.approve(path)?;
        self.preferences.configure(ScanRootLocation {
            platform: approved.platform,
            display_path: approved.display_path,
        })
    }

    pub fn reset_root_preference(&self) -> Result<ScanRootPreference, ScannerError> {
        self.preferences.reset()
    }

    #[cfg(desktop)]
    pub fn approve_preferred_root(&self) -> Result<ApprovedScanRoot, ScannerError> {
        let location = self.preferences.prepare()?;
        self.access.approve(Path::new(&location.display_path))
    }

    pub fn start(self: &Arc<Self>, access_handle: String) -> Result<ScanSessionView, ScannerError> {
        let mut active = self
            .active
            .lock()
            .map_err(|error| ScannerError::Persistence(error.to_string()))?;
        if !active.is_empty() {
            return Err(ScannerError::InvalidRequest(
                "another library scan is already running".to_owned(),
            ));
        }
        let approved = self.access.describe(&access_handle)?;
        let prepared = self.service.prepare(PrepareScanRequest {
            platform: approved.platform,
            path_key: approved.path_key,
            display_path: approved.display_path,
            access_handle,
            options: ScanOptions::default(),
        })?;
        let view = prepared.view();
        let session_id = prepared.session.id.clone();
        let cancellation = Arc::new(AtomicScanCancellation::default());
        active.insert(session_id.clone(), Arc::clone(&cancellation));
        drop(active);
        log::info!(target: "dla::scanner", "event=scan_started session_id={}", session_id.0);

        let controller = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            controller.run_prepared(session_id, prepared, cancellation)
        });
        Ok(view)
    }

    pub fn cancel(&self, session_id: &ScanSessionId) -> Result<bool, ScannerError> {
        let active = self
            .active
            .lock()
            .map_err(|error| ScannerError::Persistence(error.to_string()))?;
        let Some(cancellation) = active.get(session_id) else {
            return Ok(false);
        };
        cancellation.cancel();
        log::info!(target: "dla::scanner", "event=scan_cancel_requested session_id={}", session_id.0);
        Ok(true)
    }

    pub fn read_latest(&self) -> Result<Option<ScanSessionView>, ScannerError> {
        self.service.read_latest()
    }

    pub fn browse_results(
        &self,
        request: ScanResultRequest,
    ) -> Result<ScanResultPage, ScannerError> {
        self.service.browse_results(request)
    }

    pub fn browse_issues(&self, request: ScanIssueRequest) -> Result<ScanIssuePage, ScannerError> {
        self.service.browse_issues(request)
    }

    fn run_prepared(
        &self,
        session_id: ScanSessionId,
        prepared: PreparedScan,
        cancellation: Arc<AtomicScanCancellation>,
    ) {
        match self.service.execute(prepared, cancellation.as_ref()) {
            Ok(_) => {
                log::info!(target: "dla::scanner", "event=scan_completed session_id={}", session_id.0)
            }
            Err(error) => {
                log::warn!(target: "dla::scanner", "event=scan_stopped session_id={} error={error}", session_id.0)
            }
        }
        match self.active.lock() {
            Ok(mut active) => {
                active.remove(&session_id);
            }
            Err(error) => log::error!("could not release library scan {session_id:?}: {error}"),
        }
    }
}

#[derive(Default)]
struct AtomicScanCancellation {
    cancelled: AtomicBool,
}

impl AtomicScanCancellation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

impl ScanCancellation for AtomicScanCancellation {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
