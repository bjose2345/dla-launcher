use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use dla_application::package_preparation::{
    ExecutePackagePreparationRequest, PackageDestinationConflictPolicy, PackageDestinationPreview,
    PackagePreparationCancellationToken, PackagePreparationError, PackagePreparationProgressSink,
    PackagePreparationService,
};
use dla_domain::{
    installation::InstallationId,
    package::{
        ArchiveRetentionPolicy, PackagePreparationCounters, PackagePreparationProgress,
        PackagePreparationStage,
    },
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

pub const PACKAGE_PREPARATION_PROGRESS_EVENT: &str = "package-preparation-progress";
const PACKAGE_PREPARATION_PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
pub struct ApprovedInstallationDestination {
    pub access_handle: String,
    pub display_path: String,
}

struct DestinationAccessRegistry {
    paths: Mutex<HashMap<String, PathBuf>>,
}

impl DestinationAccessRegistry {
    fn new() -> Self {
        Self {
            paths: Mutex::new(HashMap::new()),
        }
    }

    fn approve(
        &self,
        path: &Path,
    ) -> Result<ApprovedInstallationDestination, PackagePreparationError> {
        let metadata = fs::symlink_metadata(path).map_err(PackagePreparationError::adapter)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(PackagePreparationError::adapter(
                "installation destination is not a regular directory",
            ));
        }
        let canonical = path
            .canonicalize()
            .map_err(PackagePreparationError::adapter)?;
        let access_handle = Uuid::new_v4().to_string();
        self.paths
            .lock()
            .map_err(|error| PackagePreparationError::adapter(error.to_string()))?
            .insert(access_handle.clone(), canonical.clone());
        Ok(ApprovedInstallationDestination {
            access_handle,
            display_path: canonical.to_string_lossy().into_owned(),
        })
    }

    fn resolve(&self, access_handle: &str) -> Result<String, PackagePreparationError> {
        self.paths
            .lock()
            .map_err(|error| PackagePreparationError::adapter(error.to_string()))?
            .get(access_handle)
            .map(|path| path.to_string_lossy().into_owned())
            .ok_or_else(|| {
                PackagePreparationError::adapter(
                    "installation destination access has expired; choose the folder again",
                )
            })
    }
}

pub struct TauriPackagePreparationProgressSink {
    app: AppHandle,
    latest: Arc<Mutex<Option<PackagePreparationProgress>>>,
    event_gate: Mutex<PackagePreparationProgressEventGate>,
}

#[derive(Default)]
struct PackagePreparationProgressEventGate {
    operation_id: Option<String>,
    stage: Option<PackagePreparationStage>,
    emitted_at: Option<Instant>,
}

impl PackagePreparationProgressEventGate {
    fn should_emit(&mut self, progress: &PackagePreparationProgress, now: Instant) -> bool {
        let operation_changed = self.operation_id.as_deref() != Some(&progress.operation_id);
        let stage_changed = self.stage != Some(progress.stage);
        let interval_elapsed = self.emitted_at.is_none_or(|emitted_at| {
            now.saturating_duration_since(emitted_at) >= PACKAGE_PREPARATION_PROGRESS_EMIT_INTERVAL
        });
        let terminal = matches!(
            progress.stage,
            PackagePreparationStage::Completed
                | PackagePreparationStage::Cancelled
                | PackagePreparationStage::Failed
        );
        if !operation_changed && !stage_changed && !interval_elapsed && !terminal {
            return false;
        }
        self.operation_id = Some(progress.operation_id.clone());
        self.stage = Some(progress.stage);
        self.emitted_at = Some(now);
        true
    }
}

impl TauriPackagePreparationProgressSink {
    fn new(app: AppHandle, latest: Arc<Mutex<Option<PackagePreparationProgress>>>) -> Self {
        Self {
            app,
            latest,
            event_gate: Mutex::new(PackagePreparationProgressEventGate::default()),
        }
    }
}

impl PackagePreparationProgressSink for TauriPackagePreparationProgressSink {
    fn publish(
        &self,
        progress: &PackagePreparationProgress,
    ) -> Result<(), PackagePreparationError> {
        *self
            .latest
            .lock()
            .map_err(|error| PackagePreparationError::persistence(error.to_string()))? =
            Some(progress.clone());
        let should_emit = self
            .event_gate
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .should_emit(progress, Instant::now());
        if should_emit {
            if let Err(error) = self.app.emit(PACKAGE_PREPARATION_PROGRESS_EVENT, progress) {
                log::warn!(
                    "could not emit package preparation progress; polling remains available: {error}"
                );
            }
        }
        Ok(())
    }
}

struct ActiveOperation {
    installation_id: InstallationId,
    cancellation: PackagePreparationCancellationToken,
}

pub struct PackagePreparationController {
    service: Arc<PackagePreparationService>,
    destinations: DestinationAccessRegistry,
    progress: Arc<TauriPackagePreparationProgressSink>,
    latest: Arc<Mutex<Option<PackagePreparationProgress>>>,
    active: Mutex<HashMap<String, ActiveOperation>>,
}

impl PackagePreparationController {
    pub fn new(service: Arc<PackagePreparationService>, app: AppHandle) -> Self {
        let latest = Arc::new(Mutex::new(None));
        Self {
            service,
            destinations: DestinationAccessRegistry::new(),
            progress: Arc::new(TauriPackagePreparationProgressSink::new(
                app,
                Arc::clone(&latest),
            )),
            latest,
            active: Mutex::new(HashMap::new()),
        }
    }

    pub fn approve_destination(
        &self,
        path: &Path,
    ) -> Result<ApprovedInstallationDestination, PackagePreparationError> {
        self.destinations.approve(path)
    }

    pub fn resolve_destination(
        &self,
        access_handle: &str,
    ) -> Result<String, PackagePreparationError> {
        self.destinations.resolve(access_handle)
    }

    pub fn has_active_operation(&self) -> Result<bool, PackagePreparationError> {
        self.active
            .lock()
            .map(|active| !active.is_empty())
            .map_err(|error| PackagePreparationError::persistence(error.to_string()))
    }

    pub fn installation_is_active(
        &self,
        installation_id: &InstallationId,
    ) -> Result<bool, PackagePreparationError> {
        self.active
            .lock()
            .map(|active| {
                active
                    .values()
                    .any(|operation| &operation.installation_id == installation_id)
            })
            .map_err(|error| PackagePreparationError::persistence(error.to_string()))
    }

    pub fn latest(&self) -> Result<Option<PackagePreparationProgress>, PackagePreparationError> {
        self.latest
            .lock()
            .map(|latest| latest.clone())
            .map_err(|error| PackagePreparationError::persistence(error.to_string()))
    }

    pub fn inspect_destination(
        &self,
        installation_id: &InstallationId,
        destination_access_handle: &str,
    ) -> Result<PackageDestinationPreview, PackagePreparationError> {
        let destination_parent = self.destinations.resolve(destination_access_handle)?;
        self.service
            .inspect_destination(installation_id, destination_parent)
    }

    pub fn start(
        self: &Arc<Self>,
        installation_id: InstallationId,
        destination_access_handle: String,
        destination_conflict_policy: PackageDestinationConflictPolicy,
        archive_retention: ArchiveRetentionPolicy,
    ) -> Result<PackagePreparationProgress, PackagePreparationError> {
        let destination_parent = self.destinations.resolve(&destination_access_handle)?;
        let operation_id = Uuid::new_v4().to_string();
        let initial = PackagePreparationProgress {
            operation_id: operation_id.clone(),
            installation_id: installation_id.clone(),
            stage: PackagePreparationStage::Queued,
            counters: PackagePreparationCounters::default(),
            current_path: None,
            detail: "Package preparation queued".to_owned(),
        };
        let cancellation = PackagePreparationCancellationToken::default();
        self.claim(
            operation_id.clone(),
            installation_id.clone(),
            cancellation.clone(),
        )?;
        log::info!(
            target: "dla::package",
            "event=package_preparation_started operation_id={} installation_id={}",
            operation_id,
            installation_id.0
        );
        if let Err(error) = self.progress.publish(&initial) {
            self.release(&operation_id);
            return Err(error);
        }

        let controller = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            let result = controller.service.execute(
                ExecutePackagePreparationRequest {
                    operation_id: operation_id.clone(),
                    installation_id,
                    destination_parent,
                    destination_conflict_policy,
                    archive_retention,
                    prepared_at: dla_sqlite::current_timestamp(),
                },
                &cancellation,
                controller.progress.as_ref(),
            );
            controller.finish(&operation_id, result.map(|_| ()))
        });
        Ok(initial)
    }

    pub fn cancel(&self, operation_id: &str) -> Result<bool, PackagePreparationError> {
        let active = self
            .active
            .lock()
            .map_err(|error| PackagePreparationError::persistence(error.to_string()))?;
        let Some(operation) = active.get(operation_id) else {
            return Ok(false);
        };
        operation.cancellation.cancel();
        log::info!(target: "dla::package", "event=package_preparation_cancel_requested operation_id={operation_id}");
        Ok(true)
    }

    fn claim(
        &self,
        operation_id: String,
        installation_id: InstallationId,
        cancellation: PackagePreparationCancellationToken,
    ) -> Result<(), PackagePreparationError> {
        let mut active = self
            .active
            .lock()
            .map_err(|error| PackagePreparationError::persistence(error.to_string()))?;
        if !active.is_empty() {
            return Err(PackagePreparationError::AlreadyRunning);
        }
        active.insert(
            operation_id,
            ActiveOperation {
                installation_id,
                cancellation,
            },
        );
        Ok(())
    }

    fn finish(&self, operation_id: &str, result: Result<(), PackagePreparationError>) {
        match &result {
            Ok(()) => log::info!(
                target: "dla::package",
                "event=package_preparation_completed operation_id={operation_id}"
            ),
            Err(error) => log::warn!(
                target: "dla::package",
                "event=package_preparation_stopped operation_id={operation_id} error={error}"
            ),
        }
        if let Err(error) = result {
            let latest = self.latest().ok().flatten();
            let installation_id = self
                .active
                .lock()
                .ok()
                .and_then(|active| {
                    active
                        .get(operation_id)
                        .map(|operation| operation.installation_id.clone())
                })
                .unwrap_or_else(|| InstallationId(String::new()));
            let terminal = PackagePreparationProgress {
                operation_id: operation_id.to_owned(),
                installation_id,
                stage: if matches!(error, PackagePreparationError::Cancelled) {
                    PackagePreparationStage::Cancelled
                } else {
                    PackagePreparationStage::Failed
                },
                counters: latest
                    .as_ref()
                    .filter(|progress| progress.operation_id == operation_id)
                    .map(|progress| progress.counters.clone())
                    .unwrap_or_default(),
                current_path: latest
                    .as_ref()
                    .filter(|progress| progress.operation_id == operation_id)
                    .and_then(|progress| progress.current_path.clone()),
                detail: error.to_string(),
            };
            if let Err(publish_error) = self.progress.publish(&terminal) {
                log::error!("could not publish package preparation failure: {publish_error}");
            }
        }
        self.release(operation_id);
    }

    fn release(&self, operation_id: &str) {
        match self.active.lock() {
            Ok(mut active) => {
                active.remove(operation_id);
            }
            Err(error) => {
                log::error!("could not release package preparation operation: {error}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(operation_id: &str, stage: PackagePreparationStage) -> PackagePreparationProgress {
        PackagePreparationProgress {
            operation_id: operation_id.to_owned(),
            installation_id: InstallationId("installation".to_owned()),
            stage,
            counters: PackagePreparationCounters::default(),
            current_path: None,
            detail: String::new(),
        }
    }

    #[test]
    fn progress_event_gate_limits_repeated_copy_updates_but_never_stage_transitions() {
        let started_at = Instant::now();
        let mut gate = PackagePreparationProgressEventGate::default();

        assert!(gate.should_emit(
            &progress("operation", PackagePreparationStage::Extracting),
            started_at,
        ));
        assert!(!gate.should_emit(
            &progress("operation", PackagePreparationStage::Extracting),
            started_at + Duration::from_millis(99),
        ));
        assert!(gate.should_emit(
            &progress("operation", PackagePreparationStage::Extracting),
            started_at + PACKAGE_PREPARATION_PROGRESS_EMIT_INTERVAL,
        ));
        assert!(gate.should_emit(
            &progress("operation", PackagePreparationStage::Verifying),
            started_at + Duration::from_millis(101),
        ));
        assert!(gate.should_emit(
            &progress("operation", PackagePreparationStage::Completed),
            started_at + Duration::from_millis(102),
        ));
        assert!(gate.should_emit(
            &progress("next-operation", PackagePreparationStage::Queued),
            started_at + Duration::from_millis(103),
        ));
    }
}
