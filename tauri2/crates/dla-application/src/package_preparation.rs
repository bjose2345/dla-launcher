use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use dla_domain::{
    installation::{Installation, InstallationId, ManualCatalogIdentity},
    package::{
        ArchiveRetentionPolicy, PackageInspection, PackagePreparationCounters,
        PackagePreparationProgress, PackagePreparationStage, PackageSafety, PackageSourceSet,
        PackageSourceSetKind, PreparedPackageInstallation,
    },
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::installation::{InstallationLibrary, InstallationLibraryError, InstallationStore};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutePackagePreparationRequest {
    pub operation_id: String,
    pub installation_id: InstallationId,
    pub destination_parent: String,
    pub destination_conflict_policy: PackageDestinationConflictPolicy,
    pub archive_retention: ArchiveRetentionPolicy,
    pub prepared_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageInstallExecution {
    pub operation_id: String,
    pub installation_id: InstallationId,
    pub source_root: String,
    pub destination_parent: String,
    pub destination_name: String,
    pub destination_conflict_policy: PackageDestinationConflictPolicy,
    pub inspection: PackageInspection,
    pub source_set: PackageSourceSet,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PackageDestinationConflictPolicy {
    Refuse,
    KeepBoth,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageDestinationState {
    Available,
    OccupiedUnknown,
    ManagedSameInstallation,
    ManagedOtherInstallation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDestinationPreview {
    pub state: PackageDestinationState,
    pub destination_name: String,
    pub keep_both_destination_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDestinationInspection {
    pub installation_id: InstallationId,
    pub destination_parent: String,
    pub destination_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageExtractionResult {
    pub destination_root: String,
    pub installed_file_count: u64,
    pub installed_bytes: u64,
}

#[derive(Clone, Default)]
pub struct PackagePreparationCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl PackagePreparationCancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn check(&self) -> Result<(), PackagePreparationError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(PackagePreparationError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Error)]
pub enum PackagePreparationError {
    #[error("another package preparation is already running")]
    AlreadyRunning,
    #[error("package preparation was cancelled")]
    Cancelled,
    #[error("installation has no inspected package")]
    MissingInspection,
    #[error("package inspection is unsafe")]
    UnsafePackage,
    #[error("package source set is empty")]
    EmptySourceSet,
    #[error("package has already been prepared at {0}")]
    AlreadyPrepared(String),
    #[error("package installation adapter failed: {0}")]
    Adapter(String),
    #[error("package installation persistence failed: {0}")]
    Persistence(String),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
}

impl PackagePreparationError {
    pub fn adapter(error: impl std::fmt::Display) -> Self {
        Self::Adapter(error.to_string())
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait PackagePreparationProgressSink: Send + Sync {
    fn publish(&self, progress: &PackagePreparationProgress)
    -> Result<(), PackagePreparationError>;
}

pub trait PackageInstaller: Send + Sync {
    fn inspect_destination(
        &self,
        request: &PackageDestinationInspection,
    ) -> Result<PackageDestinationPreview, PackagePreparationError>;

    fn extract(
        &self,
        request: &PackageInstallExecution,
        cancellation: &PackagePreparationCancellationToken,
        progress: &dyn PackagePreparationProgressSink,
    ) -> Result<PackageExtractionResult, PackagePreparationError>;

    fn rollback(&self, destination_root: &str) -> Result<(), PackagePreparationError>;

    fn delete_sources(
        &self,
        source_root: &str,
        source_set: &PackageSourceSet,
    ) -> Result<(), PackagePreparationError>;

    fn repair(
        &self,
        _request: &PackageInstallExecution,
        _destination_root: &str,
        _cancellation: &PackagePreparationCancellationToken,
        _progress: &dyn PackagePreparationProgressSink,
    ) -> Result<PackageExtractionResult, PackagePreparationError> {
        Err(PackagePreparationError::adapter(
            "package repair is not supported by this installer",
        ))
    }
}

pub trait PackagePreparationStore: Send + Sync {
    fn read_prepared_package(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<PreparedPackageInstallation>, PackagePreparationError>;

    fn read_prepared_packages(
        &self,
        installation_ids: &[InstallationId],
    ) -> Result<Vec<PreparedPackageInstallation>, PackagePreparationError> {
        let mut prepared = Vec::with_capacity(installation_ids.len());
        for installation_id in installation_ids {
            if let Some(package) = self.read_prepared_package(installation_id)? {
                prepared.push(package);
            }
        }
        Ok(prepared)
    }

    fn save_prepared_package(
        &self,
        prepared: &PreparedPackageInstallation,
    ) -> Result<(), PackagePreparationError>;
}

pub struct PackagePreparationService {
    installations: InstallationLibrary,
    preparations: Arc<dyn PackagePreparationStore>,
    installer: Arc<dyn PackageInstaller>,
}

impl PackagePreparationService {
    pub fn new(
        installations: Arc<dyn InstallationStore>,
        preparations: Arc<dyn PackagePreparationStore>,
        installer: Arc<dyn PackageInstaller>,
    ) -> Self {
        Self {
            installations: InstallationLibrary::new(installations),
            preparations,
            installer,
        }
    }

    pub fn execute(
        &self,
        request: ExecutePackagePreparationRequest,
        cancellation: &PackagePreparationCancellationToken,
        progress: &dyn PackagePreparationProgressSink,
    ) -> Result<PreparedPackageInstallation, PackagePreparationError> {
        let installation = self
            .installations
            .read(&request.installation_id)?
            .ok_or_else(|| InstallationLibraryError::NotFound(request.installation_id.0.clone()))?;
        let inspection = installation
            .detection
            .package_inspection
            .clone()
            .ok_or(PackagePreparationError::MissingInspection)?;
        if let Some(prepared) = self
            .preparations
            .read_prepared_package(&request.installation_id)?
        {
            return Err(PackagePreparationError::AlreadyPrepared(
                prepared.destination_root,
            ));
        }
        if inspection.safety != PackageSafety::Safe {
            return Err(PackagePreparationError::UnsafePackage);
        }
        let source_set = inspection.source_set.clone().unwrap_or(PackageSourceSet {
            kind: PackageSourceSetKind::SingleArchive,
            volumes: vec![inspection.source.clone()],
        });
        if source_set.volumes.is_empty() {
            return Err(PackagePreparationError::EmptySourceSet);
        }
        cancellation.check()?;
        progress.publish(&progress_value(
            &request,
            PackagePreparationStage::Validating,
            &inspection,
            None,
            "Validating destination, package volumes, and available disk space",
        ))?;

        let execution = PackageInstallExecution {
            operation_id: request.operation_id.clone(),
            installation_id: request.installation_id.clone(),
            source_root: installation.root_path.clone(),
            destination_parent: request.destination_parent.clone(),
            destination_name: destination_name(&installation, &inspection),
            destination_conflict_policy: request.destination_conflict_policy,
            inspection: inspection.clone(),
            source_set: source_set.clone(),
        };
        let extracted = self.installer.extract(&execution, cancellation, progress)?;
        if let Err(error) = cancellation.check() {
            let _ = self.installer.rollback(&extracted.destination_root);
            return Err(error);
        }
        let mut prepared = PreparedPackageInstallation {
            installation_id: request.installation_id.clone(),
            destination_root: extracted.destination_root,
            content_root: inspection.install_plan.content_root.clone(),
            preferred_action: inspection.install_plan.preferred_action.clone(),
            source_set,
            archive_retention: request.archive_retention,
            sources_deleted: false,
            source_cleanup_error: None,
            installed_file_count: extracted.installed_file_count,
            installed_bytes: extracted.installed_bytes,
            prepared_at: request.prepared_at,
        };
        if let Err(error) = self.preparations.save_prepared_package(&prepared) {
            let _ = self.installer.rollback(&prepared.destination_root);
            return Err(error);
        }

        if request.archive_retention == ArchiveRetentionPolicy::DeleteAfterVerifiedInstall {
            progress.publish(&PackagePreparationProgress {
                operation_id: request.operation_id.clone(),
                installation_id: request.installation_id.clone(),
                stage: PackagePreparationStage::CleaningSources,
                counters: completed_counters(&inspection),
                current_path: None,
                detail: "Removing the complete verified source-volume set".to_owned(),
            })?;
            match self
                .installer
                .delete_sources(&installation.root_path, &prepared.source_set)
            {
                Ok(()) => prepared.sources_deleted = true,
                Err(error) => prepared.source_cleanup_error = Some(error.to_string()),
            }
            self.preparations.save_prepared_package(&prepared)?;
        }
        progress.publish(&PackagePreparationProgress {
            operation_id: request.operation_id,
            installation_id: request.installation_id,
            stage: PackagePreparationStage::Completed,
            counters: completed_counters(&inspection),
            current_path: None,
            detail: prepared.source_cleanup_error.clone().map_or_else(
                || "Package prepared and verified".to_owned(),
                |error| format!("Package prepared; source cleanup needs attention: {error}"),
            ),
        })?;
        Ok(prepared)
    }

    pub fn inspect_destination(
        &self,
        installation_id: &InstallationId,
        destination_parent: String,
    ) -> Result<PackageDestinationPreview, PackagePreparationError> {
        let installation = self
            .installations
            .read(installation_id)?
            .ok_or_else(|| InstallationLibraryError::NotFound(installation_id.0.clone()))?;
        let inspection = installation
            .detection
            .package_inspection
            .as_ref()
            .ok_or(PackagePreparationError::MissingInspection)?;
        if let Some(prepared) = self.preparations.read_prepared_package(installation_id)? {
            return Err(PackagePreparationError::AlreadyPrepared(
                prepared.destination_root,
            ));
        }
        self.installer
            .inspect_destination(&PackageDestinationInspection {
                installation_id: installation_id.clone(),
                destination_parent,
                destination_name: destination_name(&installation, inspection),
            })
    }
}

fn destination_name(installation: &Installation, inspection: &PackageInspection) -> String {
    let identity = match installation.overrides.catalog_identity.as_ref() {
        Some(ManualCatalogIdentity::CatalogWork { work_code }) => Some(work_code.as_str()),
        Some(ManualCatalogIdentity::Unidentified) => None,
        None => installation
            .detection
            .catalog_identity
            .as_ref()
            .map(|identity| identity.work_code.as_str()),
    };
    let fallback = inspection
        .source
        .relative_path
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or("DLA Work")
        .split('.')
        .next()
        .unwrap_or("DLA Work");
    sanitize_directory_name(identity.unwrap_or(fallback))
}

fn sanitize_directory_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => '_',
            _ => character,
        })
        .collect::<String>();
    let sanitized = sanitized.trim().trim_end_matches(['.', ' ']);
    if sanitized.is_empty() {
        "DLA Work".to_owned()
    } else {
        sanitized.to_owned()
    }
}

fn progress_value(
    request: &ExecutePackagePreparationRequest,
    stage: PackagePreparationStage,
    inspection: &PackageInspection,
    current_path: Option<String>,
    detail: &str,
) -> PackagePreparationProgress {
    PackagePreparationProgress {
        operation_id: request.operation_id.clone(),
        installation_id: request.installation_id.clone(),
        stage,
        counters: PackagePreparationCounters {
            total_bytes: inspection.total_uncompressed_bytes,
            total_files: inspection.file_count,
            ..PackagePreparationCounters::default()
        },
        current_path,
        detail: detail.to_owned(),
    }
}

fn completed_counters(inspection: &PackageInspection) -> PackagePreparationCounters {
    PackagePreparationCounters {
        total_bytes: inspection.total_uncompressed_bytes,
        processed_bytes: inspection.total_uncompressed_bytes,
        total_files: inspection.file_count,
        processed_files: inspection.file_count,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use dla_domain::{
        installation::{
            InstallationDetection, InstallationOverrides, InstallationPlatform, InstallationStatus,
            RelativePath,
        },
        package::{
            ArchiveFormat, InstallPlan, PackageClassification, PackageContentKind,
            PackageSourceSetKind, SourceArtifact, SourceArtifactKind,
        },
        scanner::ScanEntryId,
    };

    use super::*;

    struct MemoryInstallations {
        installation: Installation,
    }

    impl InstallationStore for MemoryInstallations {
        fn create(&self, _installation: &Installation) -> Result<(), InstallationLibraryError> {
            Ok(())
        }

        fn create_or_refresh(
            &self,
            installation: &Installation,
        ) -> Result<Installation, InstallationLibraryError> {
            Ok(installation.clone())
        }

        fn read(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<Installation>, InstallationLibraryError> {
            Ok((self.installation.id == *installation_id).then(|| self.installation.clone()))
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(vec![self.installation.clone()])
        }

        fn replace_detection(
            &self,
            _installation_id: &InstallationId,
            _detection: &InstallationDetection,
            _status: InstallationStatus,
            _updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            Ok(())
        }

        fn replace_overrides(
            &self,
            _installation_id: &InstallationId,
            _overrides: &InstallationOverrides,
            _status: InstallationStatus,
            _updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            Ok(())
        }
    }

    struct MemoryPreparations {
        saved: Mutex<Vec<PreparedPackageInstallation>>,
        fail_next_save: std::sync::atomic::AtomicBool,
    }

    impl PackagePreparationStore for MemoryPreparations {
        fn read_prepared_package(
            &self,
            _installation_id: &InstallationId,
        ) -> Result<Option<PreparedPackageInstallation>, PackagePreparationError> {
            Ok(None)
        }

        fn save_prepared_package(
            &self,
            prepared: &PreparedPackageInstallation,
        ) -> Result<(), PackagePreparationError> {
            if self.fail_next_save.swap(false, Ordering::AcqRel) {
                return Err(PackagePreparationError::persistence("fixture failure"));
            }
            self.saved
                .lock()
                .expect("preparations")
                .push(prepared.clone());
            Ok(())
        }
    }

    struct RecordingInstaller {
        cancel_after_extract: bool,
        delete_error: bool,
        rollbacks: Mutex<Vec<String>>,
        deletes: AtomicUsize,
    }

    impl PackageInstaller for RecordingInstaller {
        fn inspect_destination(
            &self,
            request: &PackageDestinationInspection,
        ) -> Result<PackageDestinationPreview, PackagePreparationError> {
            Ok(PackageDestinationPreview {
                state: PackageDestinationState::Available,
                destination_name: request.destination_name.clone(),
                keep_both_destination_name: None,
            })
        }

        fn extract(
            &self,
            _request: &PackageInstallExecution,
            cancellation: &PackagePreparationCancellationToken,
            _progress: &dyn PackagePreparationProgressSink,
        ) -> Result<PackageExtractionResult, PackagePreparationError> {
            if self.cancel_after_extract {
                cancellation.cancel();
            }
            Ok(PackageExtractionResult {
                destination_root: "/library/RJ000001".to_owned(),
                installed_file_count: 2,
                installed_bytes: 7,
            })
        }

        fn rollback(&self, destination_root: &str) -> Result<(), PackagePreparationError> {
            self.rollbacks
                .lock()
                .expect("rollbacks")
                .push(destination_root.to_owned());
            Ok(())
        }

        fn delete_sources(
            &self,
            _source_root: &str,
            _source_set: &PackageSourceSet,
        ) -> Result<(), PackagePreparationError> {
            self.deletes.fetch_add(1, Ordering::AcqRel);
            if self.delete_error {
                Err(PackagePreparationError::adapter("cleanup fixture failure"))
            } else {
                Ok(())
            }
        }
    }

    struct NoopProgress;

    impl PackagePreparationProgressSink for NoopProgress {
        fn publish(
            &self,
            _progress: &PackagePreparationProgress,
        ) -> Result<(), PackagePreparationError> {
            Ok(())
        }
    }

    #[test]
    fn destination_names_never_preserve_native_path_metacharacters() {
        assert_eq!(sanitize_directory_name("RJ:01/Bad*Name"), "RJ_01_Bad_Name");
        assert_eq!(sanitize_directory_name("..."), "DLA Work");
    }

    #[test]
    fn rolls_back_activation_when_persistence_fails() {
        let preparations = Arc::new(MemoryPreparations {
            saved: Mutex::new(Vec::new()),
            fail_next_save: std::sync::atomic::AtomicBool::new(true),
        });
        let installer = Arc::new(recording_installer(false, false));
        let service = service(Arc::clone(&preparations), Arc::clone(&installer));

        let error = service
            .execute(
                request(ArchiveRetentionPolicy::Keep),
                &PackagePreparationCancellationToken::default(),
                &NoopProgress,
            )
            .expect_err("persistence failure");

        assert!(matches!(error, PackagePreparationError::Persistence(_)));
        assert_eq!(
            installer.rollbacks.lock().expect("rollbacks").as_slice(),
            ["/library/RJ000001"]
        );
    }

    #[test]
    fn rolls_back_when_cancellation_arrives_before_persistence() {
        let preparations = Arc::new(MemoryPreparations {
            saved: Mutex::new(Vec::new()),
            fail_next_save: std::sync::atomic::AtomicBool::new(false),
        });
        let installer = Arc::new(recording_installer(true, false));
        let service = service(Arc::clone(&preparations), Arc::clone(&installer));

        let error = service
            .execute(
                request(ArchiveRetentionPolicy::Keep),
                &PackagePreparationCancellationToken::default(),
                &NoopProgress,
            )
            .expect_err("cancelled preparation");

        assert!(matches!(error, PackagePreparationError::Cancelled));
        assert!(preparations.saved.lock().expect("preparations").is_empty());
        assert_eq!(installer.rollbacks.lock().expect("rollbacks").len(), 1);
    }

    #[test]
    fn preserves_a_verified_installation_when_optional_source_cleanup_fails() {
        let preparations = Arc::new(MemoryPreparations {
            saved: Mutex::new(Vec::new()),
            fail_next_save: std::sync::atomic::AtomicBool::new(false),
        });
        let installer = Arc::new(recording_installer(false, true));
        let service = service(Arc::clone(&preparations), Arc::clone(&installer));

        let prepared = service
            .execute(
                request(ArchiveRetentionPolicy::DeleteAfterVerifiedInstall),
                &PackagePreparationCancellationToken::default(),
                &NoopProgress,
            )
            .expect("verified installation");

        assert!(!prepared.sources_deleted);
        assert!(prepared.source_cleanup_error.is_some());
        assert_eq!(installer.deletes.load(Ordering::Acquire), 1);
        assert!(installer.rollbacks.lock().expect("rollbacks").is_empty());
        assert_eq!(preparations.saved.lock().expect("preparations").len(), 2);
    }

    fn service(
        preparations: Arc<MemoryPreparations>,
        installer: Arc<RecordingInstaller>,
    ) -> PackagePreparationService {
        PackagePreparationService::new(
            Arc::new(MemoryInstallations {
                installation: installation(),
            }),
            preparations,
            installer,
        )
    }

    fn recording_installer(cancel_after_extract: bool, delete_error: bool) -> RecordingInstaller {
        RecordingInstaller {
            cancel_after_extract,
            delete_error,
            rollbacks: Mutex::new(Vec::new()),
            deletes: AtomicUsize::new(0),
        }
    }

    fn request(archive_retention: ArchiveRetentionPolicy) -> ExecutePackagePreparationRequest {
        ExecutePackagePreparationRequest {
            operation_id: "operation".to_owned(),
            installation_id: InstallationId("installation".to_owned()),
            destination_parent: "/library".to_owned(),
            destination_conflict_policy: PackageDestinationConflictPolicy::Refuse,
            archive_retention,
            prepared_at: "2026-08-08T00:01:00Z".to_owned(),
        }
    }

    fn installation() -> Installation {
        let source = SourceArtifact {
            scan_entry_id: ScanEntryId("archive".to_owned()),
            kind: SourceArtifactKind::Archive,
            relative_path: RelativePath::parse("RJ000001.zip").expect("source path"),
            size_bytes: Some(5),
            sha256: None,
        };
        let source_set = PackageSourceSet {
            kind: PackageSourceSetKind::SingleArchive,
            volumes: vec![source.clone()],
        };
        Installation {
            id: InstallationId("installation".to_owned()),
            scan_root_id: None,
            root_path: "/incoming".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::NeedsReview,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: InstallationStatus::NeedsReview,
                content_items: Vec::new(),
                launch_candidates: Vec::new(),
                package_inspection: Some(PackageInspection {
                    source,
                    source_set: Some(source_set),
                    format: ArchiveFormat::Zip,
                    safety: PackageSafety::Safe,
                    entry_count: 2,
                    file_count: 2,
                    directory_count: 0,
                    total_compressed_bytes: 5,
                    total_uncompressed_bytes: 7,
                    common_root: None,
                    issues: Vec::new(),
                    classification: PackageClassification {
                        content_kind: PackageContentKind::Unknown,
                        engine: None,
                        platform: InstallationPlatform::Linux,
                        confidence: dla_domain::installation::InferenceConfidence::Low,
                        reason_codes: vec!["fixture".to_owned()],
                        content_root: None,
                        launch_candidates: Vec::new(),
                    },
                    install_plan: InstallPlan {
                        requires_extraction: true,
                        content_root: None,
                        preferred_action: None,
                        archive_retention: ArchiveRetentionPolicy::Keep,
                    },
                    inspected_at: "2026-08-08T00:00:00Z".to_owned(),
                }),
            },
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-08-08T00:00:00Z".to_owned(),
            updated_at: "2026-08-08T00:00:00Z".to_owned(),
        }
    }
}
