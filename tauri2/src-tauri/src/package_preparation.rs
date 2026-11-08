use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
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
        PackagePreparationStage, PreparedPackageInstallation,
    },
};
use tauri::AppHandle;
use uuid::Uuid;

use crate::operation::{BackgroundOperation, OperationProgress, ProgressPublisher};

pub const PACKAGE_PREPARATION_PROGRESS_EVENT: &str = "package-preparation-progress";

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

impl OperationProgress for PackagePreparationProgress {
    type Stage = PackagePreparationStage;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn stage(&self) -> PackagePreparationStage {
        self.stage
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.stage,
            PackagePreparationStage::Completed
                | PackagePreparationStage::Cancelled
                | PackagePreparationStage::Failed
        )
    }
}

impl PackagePreparationProgressSink for ProgressPublisher<PackagePreparationProgress> {
    fn publish(
        &self,
        progress: &PackagePreparationProgress,
    ) -> Result<(), PackagePreparationError> {
        self.record(progress);
        Ok(())
    }
}

/// Publishes as usual and also tells an observer, such as Install from DLA.
struct ObservedProgress<'a> {
    sink: &'a dyn PackagePreparationProgressSink,
    observer: &'a (dyn Fn(&PackagePreparationProgress) + Sync),
}

impl PackagePreparationProgressSink for ObservedProgress<'_> {
    fn publish(
        &self,
        progress: &PackagePreparationProgress,
    ) -> Result<(), PackagePreparationError> {
        (self.observer)(progress);
        self.sink.publish(progress)
    }
}

pub struct PackagePreparationController {
    service: Arc<PackagePreparationService>,
    destinations: DestinationAccessRegistry,
    operation:
        BackgroundOperation<PackagePreparationProgress, InstallationId, PackagePreparationError>,
}

impl PackagePreparationController {
    pub fn new(service: Arc<PackagePreparationService>, app: AppHandle) -> Self {
        Self {
            service,
            destinations: DestinationAccessRegistry::new(),
            operation: BackgroundOperation::new(
                app,
                PACKAGE_PREPARATION_PROGRESS_EVENT,
                "dla::package",
                "package_preparation",
            ),
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

    pub fn has_active_operation(&self) -> bool {
        self.operation.slot.is_busy()
    }

    pub fn installation_is_active(&self, installation_id: &InstallationId) -> bool {
        self.operation
            .slot
            .holds(|active| active == installation_id)
    }

    pub fn latest(&self) -> Option<PackagePreparationProgress> {
        self.operation.progress.latest()
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
        if !self
            .operation
            .begin(installation_id.clone(), &cancellation, &initial)
        {
            return Err(PackagePreparationError::AlreadyRunning);
        }
        log::info!(
            target: "dla::package",
            "event=package_preparation_started operation_id={operation_id} installation_id={}",
            installation_id.0
        );

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
                &controller.operation.progress,
            );
            controller.operation.finish(
                &operation_id,
                result.map(|_| ()),
                |latest, installation_id, error| PackagePreparationProgress {
                    operation_id: operation_id.clone(),
                    installation_id: installation_id
                        .unwrap_or_else(|| InstallationId(String::new())),
                    stage: if matches!(error, PackagePreparationError::Cancelled) {
                        PackagePreparationStage::Cancelled
                    } else {
                        PackagePreparationStage::Failed
                    },
                    counters: latest
                        .map(|progress| progress.counters.clone())
                        .unwrap_or_default(),
                    current_path: latest.and_then(|progress| progress.current_path.clone()),
                    detail: error.to_string(),
                },
            );
        });
        Ok(initial)
    }

    /// Prepares an installation and waits for the result, taking the single
    /// preparation slot as soon as it is free. `cancelled` is polled while
    /// waiting and during the work. Install from DLA prepares this way.
    pub fn execute_now(
        &self,
        installation_id: InstallationId,
        destination_parent: String,
        destination_conflict_policy: PackageDestinationConflictPolicy,
        archive_retention: ArchiveRetentionPolicy,
        cancelled: &(dyn Fn() -> bool + Sync),
        observer: &(dyn Fn(&PackagePreparationProgress) + Sync),
    ) -> Result<PreparedPackageInstallation, PackagePreparationError> {
        let operation_id = Uuid::new_v4().to_string();
        let cancellation = PackagePreparationCancellationToken::default();
        loop {
            if cancelled() {
                return Err(PackagePreparationError::Cancelled);
            }
            if self
                .operation
                .slot
                .claim(&operation_id, &cancellation, installation_id.clone())
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let finished = std::sync::atomic::AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !finished.load(std::sync::atomic::Ordering::Acquire) {
                    if cancelled() {
                        cancellation.cancel();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            });
            let result = self.service.execute(
                ExecutePackagePreparationRequest {
                    operation_id: operation_id.clone(),
                    installation_id,
                    destination_parent,
                    destination_conflict_policy,
                    archive_retention,
                    prepared_at: dla_sqlite::current_timestamp(),
                },
                &cancellation,
                &ObservedProgress {
                    sink: &self.operation.progress,
                    observer,
                },
            );
            finished.store(true, std::sync::atomic::Ordering::Release);
            result
        });
        self.operation.slot.release(&operation_id);
        result
    }

    pub fn cancel(&self, operation_id: &str) -> bool {
        self.operation.cancel(operation_id)
    }
}
