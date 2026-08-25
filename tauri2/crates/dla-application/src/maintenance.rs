use std::{collections::HashSet, path::Path, sync::Arc};

use dla_detection::{MediaClassificationRequest, classify_media};
use dla_domain::{
    installation::{Installation, InstallationId, InstallationStatus},
    maintenance::{
        ExpectedInstallationFile, FilesystemHealthSnapshot, InstallationHealthIssueKind,
        InstallationHealthReport, InstallationHealthState, InstallationInventoryEntry,
        MaintenanceCleanupReport,
    },
    package::{PackageManifest, PackagePreparationProgress, PreparedPackageInstallation},
    scanner::{
        ScanEntry, ScanEntryId, ScanEntryKind, ScanEntryPresence, ScanRootId, ScanSessionId,
    },
};
use thiserror::Error;

use crate::{
    installation::{InstallationLibrary, InstallationLibraryError, InstallationStore},
    package_inspection::{PackageManifestError, PackageManifestReader},
    package_preparation::{
        PackageDestinationConflictPolicy, PackageInstallExecution, PackageInstaller,
        PackagePreparationCancellationToken, PackagePreparationError,
        PackagePreparationProgressSink, PackagePreparationStore,
    },
};

#[derive(Debug, Error)]
pub enum LibraryMaintenanceError {
    #[error("installation was not found: {0}")]
    NotFound(String),
    #[error("installation is currently in use")]
    InstallationInUse,
    #[error("installation is not managed by DLA Launcher")]
    NotManaged,
    #[error("the original package source is unavailable, so this installation cannot be repaired")]
    SourceUnavailable,
    #[error("the selected folder does not belong to this installation: {0}")]
    InvalidLocation(String),
    #[error("library maintenance adapter failed: {0}")]
    Adapter(String),
    #[error("library maintenance persistence failed: {0}")]
    Persistence(String),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
    #[error(transparent)]
    Preparation(#[from] PackagePreparationError),
    #[error(transparent)]
    Manifest(#[from] PackageManifestError),
}

impl LibraryMaintenanceError {
    pub fn adapter(error: impl std::fmt::Display) -> Self {
        Self::Adapter(error.to_string())
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait LibraryMaintenanceStore: Send + Sync {
    fn read_installation_health(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<InstallationHealthReport>, LibraryMaintenanceError>;

    fn read_installation_healths(
        &self,
        installation_ids: &[InstallationId],
    ) -> Result<Vec<InstallationHealthReport>, LibraryMaintenanceError> {
        installation_ids
            .iter()
            .map(|id| self.read_installation_health(id))
            .filter_map(Result::transpose)
            .collect()
    }

    fn save_installation_health(
        &self,
        report: &InstallationHealthReport,
    ) -> Result<(), LibraryMaintenanceError>;

    fn replace_installation_root(
        &self,
        installation_id: &InstallationId,
        root_path: &str,
        updated_at: &str,
    ) -> Result<(), LibraryMaintenanceError>;

    fn remove_installation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<(), LibraryMaintenanceError>;

    fn installation_is_active(
        &self,
        installation_id: &InstallationId,
    ) -> Result<bool, LibraryMaintenanceError>;
}

pub trait LibraryMaintenanceFilesystem: Send + Sync {
    fn verify(
        &self,
        root_path: &str,
        installation_id: &InstallationId,
        managed: bool,
        expected_files: &[ExpectedInstallationFile],
        expected_file_count: u64,
        expected_bytes: u64,
    ) -> Result<FilesystemHealthSnapshot, LibraryMaintenanceError>;

    fn inventory(
        &self,
        root_path: &str,
    ) -> Result<Vec<InstallationInventoryEntry>, LibraryMaintenanceError>;

    fn uninstall_managed(
        &self,
        root_path: &str,
        installation_id: &InstallationId,
    ) -> Result<(), LibraryMaintenanceError>;

    fn cleanup_abandoned(
        &self,
        source_roots: &[String],
        managed_destinations: &[String],
        known_installations: &[InstallationId],
    ) -> Result<MaintenanceCleanupReport, LibraryMaintenanceError>;
}

pub struct LibraryMaintenanceService {
    installations: InstallationLibrary,
    preparations: Arc<dyn PackagePreparationStore>,
    maintenance: Arc<dyn LibraryMaintenanceStore>,
    filesystem: Arc<dyn LibraryMaintenanceFilesystem>,
    manifests: Arc<dyn PackageManifestReader>,
    installer: Arc<dyn PackageInstaller>,
}

impl LibraryMaintenanceService {
    pub fn new(
        installation_store: Arc<dyn InstallationStore>,
        preparations: Arc<dyn PackagePreparationStore>,
        maintenance: Arc<dyn LibraryMaintenanceStore>,
        filesystem: Arc<dyn LibraryMaintenanceFilesystem>,
        manifests: Arc<dyn PackageManifestReader>,
        installer: Arc<dyn PackageInstaller>,
    ) -> Self {
        Self {
            installations: InstallationLibrary::new(Arc::clone(&installation_store)),
            preparations,
            maintenance,
            filesystem,
            manifests,
            installer,
        }
    }

    pub fn read_health(
        &self,
        installation_id: &InstallationId,
    ) -> Result<InstallationHealthReport, LibraryMaintenanceError> {
        if let Some(report) = self.maintenance.read_installation_health(installation_id)? {
            return Ok(report);
        }
        let installation = self.installation(installation_id)?;
        let prepared = self.preparations.read_prepared_package(installation_id)?;
        Ok(unknown_report(&installation, prepared.as_ref()))
    }

    pub fn read_healths(
        &self,
        installation_ids: &[InstallationId],
    ) -> Result<Vec<InstallationHealthReport>, LibraryMaintenanceError> {
        let stored = self
            .maintenance
            .read_installation_healths(installation_ids)?
            .into_iter()
            .map(|report| (report.installation_id.clone(), report))
            .collect::<std::collections::HashMap<_, _>>();
        let mut reports = Vec::with_capacity(installation_ids.len());
        for installation_id in installation_ids {
            if let Some(report) = stored.get(installation_id) {
                reports.push(report.clone());
            } else {
                let installation = self.installation(installation_id)?;
                let prepared = self.preparations.read_prepared_package(installation_id)?;
                reports.push(unknown_report(&installation, prepared.as_ref()));
            }
        }
        Ok(reports)
    }

    pub fn verify(
        &self,
        installation_id: &InstallationId,
        checked_at: String,
    ) -> Result<InstallationHealthReport, LibraryMaintenanceError> {
        let installation = self.installation(installation_id)?;
        let prepared = self.preparations.read_prepared_package(installation_id)?;
        let report = self.verify_at_root(
            &installation,
            prepared.as_ref(),
            effective_root(&installation, prepared.as_ref()),
            checked_at,
        )?;
        self.maintenance.save_installation_health(&report)?;
        Ok(report)
    }

    pub fn relocate(
        &self,
        installation_id: &InstallationId,
        selected_root: String,
        updated_at: String,
    ) -> Result<InstallationHealthReport, LibraryMaintenanceError> {
        self.ensure_idle(installation_id)?;
        let installation = self.installation(installation_id)?;
        let mut prepared = self.preparations.read_prepared_package(installation_id)?;
        let report = self.verify_at_root(
            &installation,
            prepared.as_ref(),
            &selected_root,
            updated_at.clone(),
        )?;
        let valid = report.state != InstallationHealthState::Moved
            && report.state != InstallationHealthState::Inaccessible
            && location_has_identity_evidence(&report, prepared.is_some());
        if !valid {
            return Err(LibraryMaintenanceError::InvalidLocation(
                report
                    .issues
                    .first()
                    .map(|issue| issue.detail.clone())
                    .unwrap_or_else(|| "folder does not contain this installation".to_owned()),
            ));
        }
        if let Some(prepared) = prepared.as_mut() {
            prepared.destination_root = selected_root;
            self.preparations.save_prepared_package(prepared)?;
        } else {
            self.maintenance.replace_installation_root(
                installation_id,
                &selected_root,
                &updated_at,
            )?;
        }
        self.maintenance.save_installation_health(&report)?;
        Ok(report)
    }

    pub fn rescan(
        &self,
        installation_id: &InstallationId,
        updated_at: String,
    ) -> Result<InstallationHealthReport, LibraryMaintenanceError> {
        self.ensure_idle(installation_id)?;
        let installation = self.installation(installation_id)?;
        let prepared = self.preparations.read_prepared_package(installation_id)?;
        let root = effective_root(&installation, prepared.as_ref()).to_owned();
        let inventory = self.filesystem.inventory(&root)?;
        let entries = scan_entries(&inventory, &installation, &updated_at);
        let mut detection = classify_media(MediaClassificationRequest {
            source_scan_session_id: None,
            catalog_identity: installation.detection.catalog_identity.clone(),
            entries: &entries,
        })
        .map_err(LibraryMaintenanceError::adapter)?;
        detection.package_inspection = installation.detection.package_inspection.clone();
        self.installations
            .replace_detection(installation_id, detection, updated_at.clone())?;
        self.verify(installation_id, updated_at)
    }

    pub fn repair(
        &self,
        installation_id: &InstallationId,
        operation_id: String,
        repaired_at: String,
    ) -> Result<InstallationHealthReport, LibraryMaintenanceError> {
        self.ensure_idle(installation_id)?;
        let installation = self.installation(installation_id)?;
        let mut prepared = self
            .preparations
            .read_prepared_package(installation_id)?
            .ok_or(LibraryMaintenanceError::NotManaged)?;
        if prepared.sources_deleted {
            return Err(LibraryMaintenanceError::SourceUnavailable);
        }
        let inspection = installation
            .detection
            .package_inspection
            .clone()
            .ok_or(LibraryMaintenanceError::SourceUnavailable)?;
        self.manifests
            .read_manifest(&installation.root_path, &prepared.source_set)
            .map_err(|_| LibraryMaintenanceError::SourceUnavailable)?;
        let destination = Path::new(&prepared.destination_root);
        let parent = destination
            .parent()
            .ok_or_else(|| LibraryMaintenanceError::adapter("managed destination has no parent"))?;
        let name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                LibraryMaintenanceError::adapter("managed destination has no portable name")
            })?;
        let execution = PackageInstallExecution {
            operation_id,
            installation_id: installation_id.clone(),
            source_root: installation.root_path.clone(),
            destination_parent: parent.to_string_lossy().into_owned(),
            destination_name: name.to_owned(),
            destination_conflict_policy: PackageDestinationConflictPolicy::Refuse,
            replace_managed_installation_id: None,
            inspection,
            source_set: prepared.source_set.clone(),
        };
        let extracted = self.installer.repair(
            &execution,
            &prepared.destination_root,
            &PackagePreparationCancellationToken::default(),
            &SilentProgress,
        )?;
        prepared.destination_root = extracted.destination_root;
        prepared.installed_file_count = extracted.installed_file_count;
        prepared.installed_bytes = extracted.installed_bytes;
        prepared.prepared_at = repaired_at.clone();
        self.preparations.save_prepared_package(&prepared)?;
        self.rescan(installation_id, repaired_at)
    }

    pub fn remove_from_library(
        &self,
        installation_id: &InstallationId,
    ) -> Result<(), LibraryMaintenanceError> {
        self.ensure_idle(installation_id)?;
        self.installation(installation_id)?;
        self.maintenance.remove_installation(installation_id)
    }

    pub fn uninstall(
        &self,
        installation_id: &InstallationId,
    ) -> Result<(), LibraryMaintenanceError> {
        self.ensure_idle(installation_id)?;
        self.installation(installation_id)?;
        let prepared = self
            .preparations
            .read_prepared_package(installation_id)?
            .ok_or(LibraryMaintenanceError::NotManaged)?;
        self.filesystem
            .uninstall_managed(&prepared.destination_root, installation_id)?;
        self.maintenance.remove_installation(installation_id)
    }

    pub fn cleanup_abandoned(&self) -> Result<MaintenanceCleanupReport, LibraryMaintenanceError> {
        let installations = self.installations.list()?;
        let ids = installations
            .iter()
            .map(|installation| installation.id.clone())
            .collect::<Vec<_>>();
        let prepared = self.preparations.read_prepared_packages(&ids)?;
        let source_roots = installations
            .into_iter()
            .map(|installation| installation.root_path)
            .collect::<Vec<_>>();
        let destinations = prepared
            .into_iter()
            .map(|prepared| prepared.destination_root)
            .collect::<Vec<_>>();
        self.filesystem
            .cleanup_abandoned(&source_roots, &destinations, &ids)
    }

    fn verify_at_root(
        &self,
        installation: &Installation,
        prepared: Option<&PreparedPackageInstallation>,
        root: &str,
        checked_at: String,
    ) -> Result<InstallationHealthReport, LibraryMaintenanceError> {
        let (expected, source_available) = self.expected_files(installation, prepared);
        let expected_count = prepared
            .map(|value| value.installed_file_count)
            .unwrap_or(expected.len() as u64);
        let expected_bytes = prepared
            .map(|value| value.installed_bytes)
            .unwrap_or_else(|| expected.iter().filter_map(|file| file.size_bytes).sum());
        let snapshot = self.filesystem.verify(
            root,
            &installation.id,
            prepared.is_some(),
            &expected,
            expected_count,
            expected_bytes,
        )?;
        let repairable = prepared.is_some()
            && source_available
            && !prepared.is_some_and(|value| value.sources_deleted)
            && snapshot.root_exists
            && snapshot.root_accessible
            && snapshot.ownership_marker_valid
            && (snapshot.missing_files > 0 || snapshot.modified_files > 0);
        let state = health_state(installation, &snapshot, repairable);
        Ok(InstallationHealthReport {
            installation_id: installation.id.clone(),
            state,
            managed: prepared.is_some(),
            repairable,
            checked_root: root.to_owned(),
            expected_files: expected_count,
            present_files: snapshot.present_files,
            missing_files: snapshot.missing_files,
            modified_files: snapshot.modified_files,
            inaccessible_files: snapshot.inaccessible_files,
            unexpected_files: snapshot.unexpected_files,
            issues: snapshot.issues,
            checked_at,
        })
    }

    fn expected_files(
        &self,
        installation: &Installation,
        prepared: Option<&PreparedPackageInstallation>,
    ) -> (Vec<ExpectedInstallationFile>, bool) {
        if let Some(prepared) = prepared {
            if !prepared.sources_deleted
                && let Ok(manifest) = self
                    .manifests
                    .read_manifest(&installation.root_path, &prepared.source_set)
            {
                return (manifest_expected_files(&manifest), true);
            }
            return (Vec::new(), false);
        }
        (
            installation
                .detection
                .content_items
                .iter()
                .filter(|item| item.media_type != dla_domain::installation::MediaType::Directory)
                .map(|item| ExpectedInstallationFile {
                    relative_path: item.relative_path.clone(),
                    size_bytes: item.size_bytes,
                })
                .collect(),
            false,
        )
    }

    fn installation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Installation, LibraryMaintenanceError> {
        self.installations
            .read(installation_id)?
            .ok_or_else(|| LibraryMaintenanceError::NotFound(installation_id.0.clone()))
    }

    fn ensure_idle(&self, installation_id: &InstallationId) -> Result<(), LibraryMaintenanceError> {
        if self.maintenance.installation_is_active(installation_id)? {
            Err(LibraryMaintenanceError::InstallationInUse)
        } else {
            Ok(())
        }
    }
}

struct SilentProgress;

impl PackagePreparationProgressSink for SilentProgress {
    fn publish(
        &self,
        _progress: &PackagePreparationProgress,
    ) -> Result<(), PackagePreparationError> {
        Ok(())
    }
}

fn manifest_expected_files(manifest: &PackageManifest) -> Vec<ExpectedInstallationFile> {
    manifest
        .entries
        .iter()
        .filter(|entry| !entry.is_directory)
        .filter_map(|entry| {
            entry
                .relative_path
                .clone()
                .map(|relative_path| ExpectedInstallationFile {
                    relative_path,
                    size_bytes: Some(entry.uncompressed_size),
                })
        })
        .collect()
}

fn effective_root<'a>(
    installation: &'a Installation,
    prepared: Option<&'a PreparedPackageInstallation>,
) -> &'a str {
    prepared
        .map(|prepared| prepared.destination_root.as_str())
        .unwrap_or(&installation.root_path)
}

fn health_state(
    installation: &Installation,
    snapshot: &FilesystemHealthSnapshot,
    repairable: bool,
) -> InstallationHealthState {
    if !snapshot.root_exists {
        return InstallationHealthState::Moved;
    }
    if !snapshot.root_accessible || snapshot.inaccessible_files > 0 {
        return InstallationHealthState::Inaccessible;
    }
    if repairable {
        return InstallationHealthState::Repairable;
    }
    if snapshot.missing_files > 0 || !snapshot.ownership_marker_valid {
        return InstallationHealthState::MissingFiles;
    }
    if snapshot.modified_files > 0 {
        return InstallationHealthState::ModifiedFiles;
    }
    if installation.status == InstallationStatus::NeedsReview {
        return InstallationHealthState::NeedsReview;
    }
    InstallationHealthState::Healthy
}

fn location_has_identity_evidence(report: &InstallationHealthReport, managed: bool) -> bool {
    if managed {
        return !report
            .issues
            .iter()
            .any(|issue| issue.kind == InstallationHealthIssueKind::InvalidOwnershipMarker);
    }
    report.expected_files > 0 && report.expected_files.saturating_sub(report.missing_files) > 0
}

fn unknown_report(
    installation: &Installation,
    prepared: Option<&PreparedPackageInstallation>,
) -> InstallationHealthReport {
    InstallationHealthReport {
        installation_id: installation.id.clone(),
        state: if installation.status == InstallationStatus::NeedsReview {
            InstallationHealthState::NeedsReview
        } else {
            InstallationHealthState::Unknown
        },
        managed: prepared.is_some(),
        repairable: false,
        checked_root: effective_root(installation, prepared).to_owned(),
        expected_files: prepared
            .map_or(installation.detection.content_items.len() as u64, |value| {
                value.installed_file_count
            }),
        present_files: 0,
        missing_files: 0,
        modified_files: 0,
        inaccessible_files: 0,
        unexpected_files: 0,
        issues: Vec::new(),
        checked_at: String::new(),
    }
}

fn scan_entries(
    inventory: &[InstallationInventoryEntry],
    installation: &Installation,
    timestamp: &str,
) -> Vec<ScanEntry> {
    let root_id = installation
        .scan_root_id
        .clone()
        .unwrap_or_else(|| ScanRootId(format!("maintenance-{}", installation.id.0)));
    let session_id = ScanSessionId(format!("maintenance-{}", timestamp));
    let mut seen = HashSet::new();
    inventory
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let path_key = match installation.platform {
                dla_domain::installation::InstallationPlatform::Windows => {
                    item.relative_path.as_str().to_lowercase()
                }
                _ => item.relative_path.as_str().to_owned(),
            };
            seen.insert(path_key.clone()).then(|| ScanEntry {
                id: ScanEntryId(format!("maintenance-entry-{index}")),
                root_id: root_id.clone(),
                relative_path: item.relative_path.as_str().to_owned(),
                path_key,
                kind: ScanEntryKind::File,
                extension: Path::new(item.relative_path.as_str())
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase(),
                size: Some(item.size_bytes.to_string()),
                modified_at: item.modified_at.clone(),
                presence: ScanEntryPresence::Present,
                first_seen_session_id: Some(session_id.clone()),
                last_seen_session_id: Some(session_id.clone()),
                created_at: timestamp.to_owned(),
                updated_at: timestamp.to_owned(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use dla_domain::installation::{
        InstallationDetection, InstallationOverrides, InstallationPlatform,
    };

    use super::*;

    #[test]
    fn health_state_prioritizes_recovery_conditions_deterministically() {
        let ready = installation(InstallationStatus::Ready);
        let needs_review = installation(InstallationStatus::NeedsReview);

        assert_eq!(
            health_state(&ready, &snapshot(false, false, false, 0, 0, 0), false,),
            InstallationHealthState::Moved,
        );
        assert_eq!(
            health_state(&ready, &snapshot(true, false, false, 0, 0, 1), false,),
            InstallationHealthState::Inaccessible,
        );
        assert_eq!(
            health_state(&ready, &snapshot(true, true, true, 1, 0, 0), true,),
            InstallationHealthState::Repairable,
        );
        assert_eq!(
            health_state(&ready, &snapshot(true, true, true, 1, 0, 0), false,),
            InstallationHealthState::MissingFiles,
        );
        assert_eq!(
            health_state(&ready, &snapshot(true, true, true, 0, 1, 0), false,),
            InstallationHealthState::ModifiedFiles,
        );
        assert_eq!(
            health_state(&needs_review, &snapshot(true, true, true, 0, 0, 0), false,),
            InstallationHealthState::NeedsReview,
        );
        assert_eq!(
            health_state(&ready, &snapshot(true, true, true, 0, 0, 0), false,),
            InstallationHealthState::Healthy,
        );
    }

    #[test]
    fn an_invalid_managed_marker_is_missing_not_repairable() {
        let state = health_state(
            &installation(InstallationStatus::Ready),
            &snapshot(true, true, false, 0, 0, 0),
            false,
        );

        assert_eq!(state, InstallationHealthState::MissingFiles);
    }

    #[test]
    fn relocation_requires_a_marker_or_matching_indexed_content() {
        let installation = installation(InstallationStatus::Ready);
        let mut report = unknown_report(&installation, None);
        report.expected_files = 2;
        report.missing_files = 2;
        assert!(!location_has_identity_evidence(&report, false));

        report.missing_files = 1;
        assert!(location_has_identity_evidence(&report, false));

        report
            .issues
            .push(dla_domain::maintenance::InstallationHealthIssue {
                kind: InstallationHealthIssueKind::InvalidOwnershipMarker,
                relative_path: None,
                detail: "wrong installation id".to_owned(),
            });
        assert!(!location_has_identity_evidence(&report, true));
    }

    fn installation(status: InstallationStatus) -> Installation {
        Installation {
            id: InstallationId("installation-1".to_owned()),
            scan_root_id: None,
            root_path: "/library/work".to_owned(),
            platform: InstallationPlatform::Linux,
            status,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: status,
                content_items: Vec::new(),
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-08-16T00:00:00Z".to_owned(),
            updated_at: "2026-08-16T00:00:00Z".to_owned(),
        }
    }

    fn snapshot(
        root_exists: bool,
        root_accessible: bool,
        ownership_marker_valid: bool,
        missing_files: u64,
        modified_files: u64,
        inaccessible_files: u64,
    ) -> FilesystemHealthSnapshot {
        FilesystemHealthSnapshot {
            root_exists,
            root_accessible,
            ownership_marker_valid,
            present_files: 1,
            present_bytes: 1,
            missing_files,
            modified_files,
            inaccessible_files,
            unexpected_files: 0,
            issues: Vec::new(),
        }
    }
}
