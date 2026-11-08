use std::{collections::HashSet, path::Path, sync::Arc};

use dla_detection::{MediaClassificationRequest, classify_media};
use dla_domain::{
    installation::{
        Installation, InstallationId, InstallationPlatform, InstallationStatus, RelativePath,
    },
    maintenance::{
        ExpectedInstallationFile, FileNameFix, FileNameReport, FilesystemHealthSnapshot,
        InstallationHealthIssueKind, InstallationHealthReport, InstallationHealthState,
        InstallationInventoryEntry, LibraryFileNameReport, MaintenanceCleanupReport,
    },
    package::{PackageManifest, PackagePreparationProgress, PreparedPackageInstallation},
    scanner::{
        ScanEntry, ScanEntryId, ScanEntryKind, ScanEntryPresence, ScanRootId, ScanSessionId,
    },
    translation::{GameFileChange, GameFileChangeKind},
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

/// Files the launcher itself added to or changed in a game to translate it.
/// The file check counts neither as damage.
pub trait LauncherFileChanges: Send + Sync {
    fn launcher_file_changes(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Vec<GameFileChange>, LibraryMaintenanceError>;
}

/// File names an older install read wrongly: the ZIP stored Japanese names
/// in Shift-JIS, and they were read as CP437.
pub trait InstalledFileNames: Send + Sync {
    /// The files under `root` with such a name, and the name they should have.
    fn find(&self, root: &str) -> Result<Vec<FileNameFix>, LibraryMaintenanceError>;

    /// Renames the files, all or none, and removes the folders left empty.
    fn fix(&self, root: &str, fixes: &[FileNameFix]) -> Result<(), LibraryMaintenanceError>;

    /// What a path the launcher stored becomes, when it has such a name.
    fn fixed_path(&self, path: &str) -> Option<String>;
}

/// Everything the launcher stored that names a path inside a work: the file
/// list and start-file choices found in it, the chosen start file, the
/// install's folders, reading and listening progress, launches and
/// translations.
pub trait InstalledPathStore: Send + Sync {
    fn rename_paths(
        &self,
        installation_id: &InstallationId,
        rename: &dyn Fn(&str) -> Option<String>,
    ) -> Result<(), LibraryMaintenanceError>;
}

pub trait LibraryMaintenanceFilesystem: Send + Sync {
    /// `ignored` files, and everything inside `ignored` folders, are left
    /// out of the check.
    #[allow(clippy::too_many_arguments)]
    fn verify(
        &self,
        root_path: &str,
        installation_id: &InstallationId,
        managed: bool,
        expected_files: &[ExpectedInstallationFile],
        expected_file_count: u64,
        expected_bytes: u64,
        ignored: &[RelativePath],
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
    launcher_changes: Option<Arc<dyn LauncherFileChanges>>,
    file_names: Option<(Arc<dyn InstalledFileNames>, Arc<dyn InstalledPathStore>)>,
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
            launcher_changes: None,
            file_names: None,
        }
    }

    pub fn with_launcher_changes(mut self, changes: Arc<dyn LauncherFileChanges>) -> Self {
        self.launcher_changes = Some(changes);
        self
    }

    pub fn with_file_names(
        mut self,
        names: Arc<dyn InstalledFileNames>,
        paths: Arc<dyn InstalledPathStore>,
    ) -> Self {
        self.file_names = Some((names, paths));
        self
    }

    /// How many of the work's file names an older install read wrongly.
    pub fn check_file_names(
        &self,
        installation_id: &InstallationId,
    ) -> Result<FileNameReport, LibraryMaintenanceError> {
        let (names, _) = self.file_name_ports()?;
        let installation = self.installation(installation_id)?;
        let prepared = self.preparations.read_prepared_package(installation_id)?;
        let wrong = names
            .find(effective_root(&installation, prepared.as_ref()))?
            .len() as u64;
        Ok(FileNameReport {
            installation_id: installation_id.clone(),
            wrong,
            fixed: 0,
        })
    }

    /// Gives the work's files back their Japanese names, updates what the
    /// launcher stored about them, and reads the file list again.
    pub fn fix_file_names(
        &self,
        installation_id: &InstallationId,
        updated_at: String,
    ) -> Result<FileNameReport, LibraryMaintenanceError> {
        let (names, paths) = self.file_name_ports()?;
        self.ensure_idle(installation_id)?;
        let installation = self.installation(installation_id)?;
        let prepared = self.preparations.read_prepared_package(installation_id)?;
        let root = effective_root(&installation, prepared.as_ref()).to_owned();
        let fixes = names.find(&root)?;
        if !fixes.is_empty() {
            names.fix(&root, &fixes)?;
            // Renamed, not scanned again: a scan would guess the start file
            // anew and could send a work that was set up back to review.
            paths.rename_paths(installation_id, &|path| names.fixed_path(path))?;
            self.verify(installation_id, updated_at)?;
        }
        Ok(FileNameReport {
            installation_id: installation_id.clone(),
            wrong: 0,
            fixed: fixes.len() as u64,
        })
    }

    /// Fixes the wrongly read file names of every work. A work in use, or
    /// one whose names cannot all be given back, is left as it was.
    pub fn fix_all_file_names(
        &self,
        updated_at: String,
    ) -> Result<LibraryFileNameReport, LibraryMaintenanceError> {
        let (names, _) = self.file_name_ports()?;
        let mut report = LibraryFileNameReport::default();
        for installation in self.installations.list()? {
            let prepared = self.preparations.read_prepared_package(&installation.id)?;
            let Ok(fixes) = names.find(effective_root(&installation, prepared.as_ref())) else {
                continue;
            };
            if fixes.is_empty() {
                continue;
            }
            match self.fix_file_names(&installation.id, updated_at.clone()) {
                Ok(fixed) => {
                    report.works += 1;
                    report.files += fixed.fixed;
                }
                Err(_) => report.skipped += 1,
            }
        }
        Ok(report)
    }

    fn file_name_ports(
        &self,
    ) -> Result<(&dyn InstalledFileNames, &dyn InstalledPathStore), LibraryMaintenanceError> {
        self.file_names
            .as_ref()
            .map(|(names, paths)| (names.as_ref(), paths.as_ref()))
            .ok_or_else(|| LibraryMaintenanceError::adapter("file names cannot be fixed here"))
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
        // A work the launcher unpacked starts with its install's start file,
        // which the Library offers no way to change. While that file is
        // there, the work stays ready, even when its pictures or music would
        // make a new scan ask what Play opens.
        let start_file = prepared
            .as_ref()
            .and_then(|prepared| prepared.preferred_action.as_ref())
            .map(|action| match installation.platform {
                InstallationPlatform::Windows => action.relative_path.as_str().to_lowercase(),
                _ => action.relative_path.as_str().to_owned(),
            });
        if detection.catalog_identity.is_some()
            && start_file.is_some_and(|key| {
                detection
                    .content_items
                    .iter()
                    .any(|item| item.path_key == key)
            })
        {
            detection.suggested_status = InstallationStatus::Ready;
        }
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
        let (mut expected, source_available) = self.expected_files(installation, prepared);
        let changes = match &self.launcher_changes {
            Some(reader) => reader.launcher_file_changes(&installation.id)?,
            None => Vec::new(),
        };
        // A rescan made while translation was on lists the plugin's files.
        expected.retain(|file| {
            !changes
                .iter()
                .any(|change| change.covers(file.relative_path.as_str()))
        });
        let mut size_change = 0_i64;
        for change in &changes {
            let sizes = match &change.kind {
                GameFileChangeKind::Added => None,
                GameFileChangeKind::Changed {
                    original,
                    written_bytes,
                    ..
                } => Some((original.len() as u64, *written_bytes)),
                GameFileChangeKind::Patched {
                    original_bytes,
                    written_bytes,
                    ..
                } => Some((*original_bytes, *written_bytes)),
            };
            if let Some((original_bytes, written_bytes)) = sizes {
                size_change += written_bytes as i64 - original_bytes as i64;
                for file in expected
                    .iter_mut()
                    .filter(|file| file.relative_path == change.path)
                {
                    file.size_bytes = Some(written_bytes);
                }
            }
        }
        let ignored = changes
            .iter()
            .filter(|change| change.kind == GameFileChangeKind::Added)
            .map(|change| change.path.clone())
            .collect::<Vec<_>>();
        let expected_count = prepared
            .map(|value| value.installed_file_count)
            .unwrap_or(expected.len() as u64);
        let expected_bytes = prepared
            .map(|value| value.installed_bytes.saturating_add_signed(size_change))
            .unwrap_or_else(|| expected.iter().filter_map(|file| file.size_bytes).sum());
        let snapshot = self.filesystem.verify(
            root,
            &installation.id,
            prepared.is_some(),
            &expected,
            expected_count,
            expected_bytes,
            &ignored,
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
