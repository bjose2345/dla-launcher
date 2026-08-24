use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use dla_application::maintenance::{LibraryMaintenanceError, LibraryMaintenanceFilesystem};
use dla_domain::{
    installation::{InstallationId, RelativePath},
    maintenance::{
        ExpectedInstallationFile, FilesystemHealthSnapshot, InstallationHealthIssue,
        InstallationHealthIssueKind, InstallationInventoryEntry, MaintenanceCleanupReport,
    },
};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::installer::{
    INSTALLATION_MARKER, SOURCE_CLEANUP_MARKER, STAGING_MARKER, SourceCleanupMarker,
    read_installation_marker, read_staging_marker, validate_owned_installation,
};

#[derive(Default)]
pub struct DesktopLibraryMaintenance;

impl DesktopLibraryMaintenance {
    pub fn new() -> Self {
        Self
    }
}

impl LibraryMaintenanceFilesystem for DesktopLibraryMaintenance {
    fn verify(
        &self,
        root_path: &str,
        installation_id: &InstallationId,
        managed: bool,
        expected_files: &[ExpectedInstallationFile],
        expected_file_count: u64,
        expected_bytes: u64,
    ) -> Result<FilesystemHealthSnapshot, LibraryMaintenanceError> {
        let root = Path::new(root_path);
        let metadata = match fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(missing_root(root_path, expected_file_count));
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                return Ok(inaccessible_root(root_path, expected_file_count, error));
            }
            Err(error) => return Err(LibraryMaintenanceError::adapter(error)),
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Ok(inaccessible_root(
                root_path,
                expected_file_count,
                "installation root is not a regular directory",
            ));
        }

        let mut issues = Vec::new();
        let ownership_marker_valid = !managed
            || match validate_owned_installation(root, installation_id) {
                Ok(_) => true,
                Err(error) => {
                    issues.push(InstallationHealthIssue {
                        kind: InstallationHealthIssueKind::InvalidOwnershipMarker,
                        relative_path: None,
                        detail: error.to_string(),
                    });
                    false
                }
            };
        let (actual, inaccessible_files) = collect_files(root, &mut issues)?;
        let present_files = actual.len() as u64;
        let present_bytes = actual.values().copied().sum();
        let mut missing_files = 0;
        let mut modified_files = 0;
        let mut unexpected_files = 0;

        if expected_files.is_empty() {
            if present_files < expected_file_count {
                missing_files = expected_file_count - present_files;
                issues.push(InstallationHealthIssue {
                    kind: InstallationHealthIssueKind::Missing,
                    relative_path: None,
                    detail: format!(
                        "managed inventory contains {present_files} of {expected_file_count} indexed files"
                    ),
                });
            } else if present_files > expected_file_count {
                unexpected_files = present_files - expected_file_count;
            }
            if present_files == expected_file_count && present_bytes != expected_bytes {
                modified_files = 1;
                issues.push(InstallationHealthIssue {
                    kind: InstallationHealthIssueKind::Modified,
                    relative_path: None,
                    detail: format!(
                        "managed inventory size changed from {expected_bytes} to {present_bytes} bytes"
                    ),
                });
            }
        } else {
            let expected = expected_files
                .iter()
                .map(|file| (file.relative_path.as_str().to_owned(), file))
                .collect::<BTreeMap<_, _>>();
            for (path, expected_file) in &expected {
                match actual.get(path) {
                    None => {
                        missing_files += 1;
                        issues.push(InstallationHealthIssue {
                            kind: InstallationHealthIssueKind::Missing,
                            relative_path: Some(expected_file.relative_path.clone()),
                            detail: "indexed file is missing on disk".to_owned(),
                        });
                    }
                    Some(actual_size)
                        if expected_file
                            .size_bytes
                            .is_some_and(|expected_size| expected_size != *actual_size) =>
                    {
                        modified_files += 1;
                        issues.push(InstallationHealthIssue {
                            kind: InstallationHealthIssueKind::Modified,
                            relative_path: Some(expected_file.relative_path.clone()),
                            detail: format!(
                                "file size changed from {} to {actual_size} bytes",
                                expected_file.size_bytes.unwrap_or_default()
                            ),
                        });
                    }
                    _ => {}
                }
            }
            for path in actual.keys().filter(|path| !expected.contains_key(*path)) {
                unexpected_files += 1;
                if issues.len() < 100 {
                    issues.push(InstallationHealthIssue {
                        kind: InstallationHealthIssueKind::Unexpected,
                        relative_path: RelativePath::parse(path.clone()).ok(),
                        detail: "file is not part of the indexed installation".to_owned(),
                    });
                }
            }
        }

        Ok(FilesystemHealthSnapshot {
            root_exists: true,
            root_accessible: true,
            ownership_marker_valid,
            present_files,
            present_bytes,
            missing_files,
            modified_files,
            inaccessible_files,
            unexpected_files,
            issues,
        })
    }

    fn inventory(
        &self,
        root_path: &str,
    ) -> Result<Vec<InstallationInventoryEntry>, LibraryMaintenanceError> {
        let root = Path::new(root_path);
        let metadata = fs::symlink_metadata(root).map_err(LibraryMaintenanceError::adapter)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(LibraryMaintenanceError::adapter(
                "installation root is not a regular directory",
            ));
        }
        let mut inventory = Vec::new();
        for entry in WalkDir::new(root).follow_links(false) {
            let entry = entry.map_err(LibraryMaintenanceError::adapter)?;
            if entry.path() == root || entry.file_type().is_dir() {
                continue;
            }
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(LibraryMaintenanceError::adapter)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                continue;
            }
            let relative = portable_relative(root, entry.path())?;
            if internal_file(relative.as_str()) {
                continue;
            }
            inventory.push(InstallationInventoryEntry {
                relative_path: relative,
                size_bytes: metadata.len(),
                modified_at: None,
            });
        }
        inventory.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        Ok(inventory)
    }

    fn uninstall_managed(
        &self,
        root_path: &str,
        installation_id: &InstallationId,
    ) -> Result<(), LibraryMaintenanceError> {
        let root = Path::new(root_path);
        validate_owned_installation(root, installation_id)
            .map_err(LibraryMaintenanceError::adapter)?;
        fs::remove_dir_all(root).map_err(LibraryMaintenanceError::adapter)
    }

    fn cleanup_abandoned(
        &self,
        source_roots: &[String],
        managed_destinations: &[String],
        known_installations: &[InstallationId],
    ) -> Result<MaintenanceCleanupReport, LibraryMaintenanceError> {
        let mut report = MaintenanceCleanupReport {
            removed_staging_directories: 0,
            removed_repair_directories: 0,
            restored_source_files: 0,
            retained_paths: Vec::new(),
        };
        let parents = managed_destinations
            .iter()
            .filter_map(|destination| Path::new(destination).parent().map(Path::to_path_buf))
            .collect::<BTreeSet<_>>();
        let known_installations = known_installations.iter().collect::<BTreeSet<_>>();
        for parent in parents {
            cleanup_destination_parent(
                &parent,
                managed_destinations,
                &known_installations,
                &mut report,
            )?;
        }
        let source_roots = source_roots
            .iter()
            .map(PathBuf::from)
            .collect::<BTreeSet<_>>();
        for root in source_roots {
            restore_source_quarantines(&root, &mut report)?;
        }
        Ok(report)
    }
}

fn collect_files(
    root: &Path,
    issues: &mut Vec<InstallationHealthIssue>,
) -> Result<(BTreeMap<String, u64>, u64), LibraryMaintenanceError> {
    let mut files = BTreeMap::new();
    let mut inaccessible = 0;
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                inaccessible += 1;
                issues.push(InstallationHealthIssue {
                    kind: InstallationHealthIssueKind::Inaccessible,
                    relative_path: None,
                    detail: error.to_string(),
                });
                continue;
            }
        };
        if entry.path() == root || entry.file_type().is_dir() {
            continue;
        }
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
            Ok(_) => continue,
            Err(error) => {
                inaccessible += 1;
                issues.push(InstallationHealthIssue {
                    kind: InstallationHealthIssueKind::Inaccessible,
                    relative_path: None,
                    detail: format!("{}: {error}", entry.path().display()),
                });
                continue;
            }
        };
        let relative = portable_relative(root, entry.path())?;
        if internal_file(relative.as_str()) {
            continue;
        }
        files.insert(relative.as_str().to_owned(), metadata.len());
    }
    Ok((files, inaccessible))
}

fn missing_root(root_path: &str, expected_files: u64) -> FilesystemHealthSnapshot {
    FilesystemHealthSnapshot {
        root_exists: false,
        root_accessible: false,
        ownership_marker_valid: false,
        present_files: 0,
        present_bytes: 0,
        missing_files: expected_files,
        modified_files: 0,
        inaccessible_files: 0,
        unexpected_files: 0,
        issues: vec![InstallationHealthIssue {
            kind: InstallationHealthIssueKind::Missing,
            relative_path: None,
            detail: format!("installation root does not exist: {root_path}"),
        }],
    }
}

fn inaccessible_root(
    root_path: &str,
    expected_files: u64,
    error: impl std::fmt::Display,
) -> FilesystemHealthSnapshot {
    FilesystemHealthSnapshot {
        root_exists: true,
        root_accessible: false,
        ownership_marker_valid: false,
        present_files: 0,
        present_bytes: 0,
        missing_files: 0,
        modified_files: 0,
        inaccessible_files: expected_files.max(1),
        unexpected_files: 0,
        issues: vec![InstallationHealthIssue {
            kind: InstallationHealthIssueKind::Inaccessible,
            relative_path: None,
            detail: format!("installation root is inaccessible ({root_path}): {error}"),
        }],
    }
}

fn portable_relative(root: &Path, path: &Path) -> Result<RelativePath, LibraryMaintenanceError> {
    let relative = path
        .strip_prefix(root)
        .map_err(LibraryMaintenanceError::adapter)?;
    let portable = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    RelativePath::parse(portable).map_err(LibraryMaintenanceError::adapter)
}

fn internal_file(path: &str) -> bool {
    path == INSTALLATION_MARKER || path == STAGING_MARKER
}

fn cleanup_destination_parent(
    parent: &Path,
    managed_destinations: &[String],
    known_installations: &BTreeSet<&InstallationId>,
    report: &mut MaintenanceCleanupReport,
) -> Result<(), LibraryMaintenanceError> {
    let known_names = managed_destinations
        .iter()
        .filter_map(|destination| {
            let path = Path::new(destination);
            if path.parent() == Some(parent) {
                path.file_name().map(|name| name.to_owned())
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(LibraryMaintenanceError::adapter(error)),
    };
    for entry in entries {
        let entry = entry.map_err(LibraryMaintenanceError::adapter)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(LibraryMaintenanceError::adapter)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        if name.starts_with(".dla-stage-") {
            let operation_id = abandoned_operation_id(&name, ".dla-stage-");
            let marker = read_staging_marker(&path).ok();
            let owned = operation_id
                .zip(marker.as_ref())
                .is_some_and(|(operation_id, marker)| {
                    marker.operation_id == operation_id
                        && known_installations.contains(&marker.installation_id)
                });
            if owned {
                fs::remove_dir_all(&path).map_err(LibraryMaintenanceError::adapter)?;
                report.removed_staging_directories += 1;
            } else {
                report
                    .retained_paths
                    .push(path.to_string_lossy().into_owned());
            }
        } else if (name.starts_with(".dla-repair-") || name.starts_with(".dla-backup-"))
            && valid_abandoned_name(
                &name,
                if name.starts_with(".dla-repair-") {
                    ".dla-repair-"
                } else {
                    ".dla-backup-"
                },
            )
        {
            let marker = read_installation_marker(&path).ok();
            let corresponding_install_exists = marker.is_some_and(|marker| {
                known_names.iter().any(|name| {
                    let destination = parent.join(name);
                    read_installation_marker(&destination)
                        .is_ok_and(|current| current.installation_id == marker.installation_id)
                })
            });
            if corresponding_install_exists {
                fs::remove_dir_all(&path).map_err(LibraryMaintenanceError::adapter)?;
                report.removed_repair_directories += 1;
            } else {
                report
                    .retained_paths
                    .push(path.to_string_lossy().into_owned());
            }
        }
    }
    Ok(())
}

fn abandoned_operation_id<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    valid_abandoned_name(name, prefix).then(|| &name[prefix.len()..prefix.len() + 36])
}

fn valid_abandoned_name(name: &str, prefix: &str) -> bool {
    let Some(ids) = name.strip_prefix(prefix) else {
        return false;
    };
    if ids.len() != 73 {
        return false;
    }
    Uuid::parse_str(&ids[..36]).is_ok()
        && ids.as_bytes().get(36) == Some(&b'-')
        && Uuid::parse_str(&ids[37..]).is_ok()
}

fn restore_source_quarantines(
    root: &Path,
    report: &mut MaintenanceCleanupReport,
) -> Result<(), LibraryMaintenanceError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(LibraryMaintenanceError::adapter(error)),
    };
    for entry in entries {
        let entry = entry.map_err(LibraryMaintenanceError::adapter)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let quarantine = entry.path();
        if !name.starts_with(".dla-source-cleanup-")
            || Uuid::parse_str(name.trim_start_matches(".dla-source-cleanup-")).is_err()
        {
            continue;
        }
        let metadata =
            fs::symlink_metadata(&quarantine).map_err(LibraryMaintenanceError::adapter)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            report
                .retained_paths
                .push(quarantine.to_string_lossy().into_owned());
            continue;
        }
        if !restore_source_quarantine(root, &quarantine, report)? {
            report
                .retained_paths
                .push(quarantine.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

fn restore_source_quarantine(
    source_root: &Path,
    quarantine: &Path,
    report: &mut MaintenanceCleanupReport,
) -> Result<bool, LibraryMaintenanceError> {
    let marker = match fs::File::open(quarantine.join(SOURCE_CLEANUP_MARKER))
        .map_err(LibraryMaintenanceError::adapter)
        .and_then(|file| {
            serde_json::from_reader::<_, SourceCleanupMarker>(file)
                .map_err(LibraryMaintenanceError::adapter)
        }) {
        Ok(marker) => marker,
        Err(_) => return Ok(false),
    };
    let expected = marker.file_names.into_iter().collect::<BTreeSet<_>>();
    if expected.is_empty()
        || expected.iter().any(|name| {
            name.is_empty()
                || Path::new(name).components().count() != 1
                || name == SOURCE_CLEANUP_MARKER
        })
    {
        return Ok(false);
    }
    for entry in fs::read_dir(quarantine).map_err(LibraryMaintenanceError::adapter)? {
        let entry = entry.map_err(LibraryMaintenanceError::adapter)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == SOURCE_CLEANUP_MARKER {
            continue;
        }
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(LibraryMaintenanceError::adapter)?;
        if !expected.contains(&name) || !metadata.is_file() || metadata.file_type().is_symlink() {
            return Ok(false);
        }
    }
    let mut recover = Vec::new();
    for name in expected {
        let quarantined = quarantine.join(&name);
        let destination = source_root.join(&name);
        match (quarantined.is_file(), destination.exists()) {
            (true, false) => recover.push((quarantined, destination)),
            (false, true) => {}
            _ => return Ok(false),
        }
    }
    for (quarantined, destination) in recover {
        fs::rename(quarantined, destination).map_err(LibraryMaintenanceError::adapter)?;
        report.restored_source_files += 1;
    }
    fs::remove_file(quarantine.join(SOURCE_CLEANUP_MARKER))
        .map_err(LibraryMaintenanceError::adapter)?;
    fs::remove_dir(quarantine).map_err(LibraryMaintenanceError::adapter)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use dla_application::maintenance::LibraryMaintenanceFilesystem;
    use dla_domain::maintenance::ExpectedInstallationFile;
    use tempfile::tempdir;

    use super::*;
    use crate::installer::{InstallationMarker, SourceCleanupMarker, StagingMarker};

    #[test]
    fn verification_reports_missing_and_modified_files_without_counting_metadata() {
        let directory = tempdir().expect("temporary directory");
        let installation_id = InstallationId("installation-1".to_owned());
        write_installation_marker(directory.path(), &installation_id);
        fs::write(directory.path().join("present.bin"), b"changed").expect("content");
        let expected = vec![
            ExpectedInstallationFile {
                relative_path: RelativePath::parse("present.bin").expect("present path"),
                size_bytes: Some(3),
            },
            ExpectedInstallationFile {
                relative_path: RelativePath::parse("missing.bin").expect("missing path"),
                size_bytes: Some(4),
            },
        ];

        let snapshot = DesktopLibraryMaintenance::new()
            .verify(
                directory.path().to_str().expect("root"),
                &installation_id,
                true,
                &expected,
                2,
                7,
            )
            .expect("verification");

        assert!(snapshot.ownership_marker_valid);
        assert_eq!(snapshot.present_files, 1);
        assert_eq!(snapshot.missing_files, 1);
        assert_eq!(snapshot.modified_files, 1);
        assert_eq!(snapshot.unexpected_files, 0);
    }

    #[test]
    fn uninstall_refuses_a_marker_owned_by_another_installation() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path().join("Installed");
        fs::create_dir(&root).expect("installation root");
        write_installation_marker(&root, &InstallationId("other".to_owned()));
        fs::write(root.join("content.bin"), b"user bytes").expect("content");

        DesktopLibraryMaintenance::new()
            .uninstall_managed(
                root.to_str().expect("root"),
                &InstallationId("expected".to_owned()),
            )
            .expect_err("foreign marker must be rejected");

        assert!(root.is_dir());
        assert_eq!(
            fs::read(root.join("content.bin")).expect("content"),
            b"user bytes"
        );
    }

    #[test]
    fn cleanup_removes_only_staging_owned_by_a_known_installation() {
        let directory = tempdir().expect("temporary directory");
        let installation_id = InstallationId("installation-1".to_owned());
        let destination = directory.path().join("Installed");
        fs::create_dir(&destination).expect("destination");
        write_installation_marker(&destination, &installation_id);
        let operation_id = Uuid::new_v4().to_string();
        let owned = directory
            .path()
            .join(format!(".dla-stage-{operation_id}-{}", Uuid::new_v4()));
        fs::create_dir(&owned).expect("owned staging");
        serde_json::to_writer(
            File::create(owned.join(STAGING_MARKER)).expect("staging marker"),
            &StagingMarker {
                operation_id: operation_id.clone(),
                installation_id: installation_id.clone(),
            },
        )
        .expect("write staging marker");
        fs::write(owned.join("partial.bin"), b"partial").expect("partial content");
        let unowned =
            directory
                .path()
                .join(format!(".dla-stage-{}-{}", Uuid::new_v4(), Uuid::new_v4()));
        fs::create_dir(&unowned).expect("unowned staging");
        fs::write(unowned.join("keep.bin"), b"keep").expect("unowned content");

        let report = DesktopLibraryMaintenance::new()
            .cleanup_abandoned(
                &[],
                &[destination.to_string_lossy().into_owned()],
                std::slice::from_ref(&installation_id),
            )
            .expect("cleanup");

        assert!(!owned.exists());
        assert!(unowned.is_dir());
        assert_eq!(report.removed_staging_directories, 1);
        assert_eq!(
            report.retained_paths,
            vec![unowned.to_string_lossy().into_owned()]
        );
    }

    #[test]
    fn cleanup_restores_only_marker_declared_source_files() {
        let directory = tempdir().expect("temporary directory");
        let source_root = directory.path().join("Sources");
        fs::create_dir(&source_root).expect("source root");
        fs::write(source_root.join("second.zip"), b"second").expect("remaining source");
        let quarantine = source_root.join(format!(".dla-source-cleanup-{}", Uuid::new_v4()));
        fs::create_dir(&quarantine).expect("quarantine");
        fs::write(quarantine.join("first.zip"), b"first").expect("quarantined source");
        serde_json::to_writer(
            File::create(quarantine.join(SOURCE_CLEANUP_MARKER)).expect("cleanup marker"),
            &SourceCleanupMarker {
                file_names: vec!["first.zip".to_owned(), "second.zip".to_owned()],
            },
        )
        .expect("write cleanup marker");

        let report = DesktopLibraryMaintenance::new()
            .cleanup_abandoned(&[source_root.to_string_lossy().into_owned()], &[], &[])
            .expect("cleanup");

        assert_eq!(
            fs::read(source_root.join("first.zip")).expect("restored"),
            b"first"
        );
        assert_eq!(
            fs::read(source_root.join("second.zip")).expect("existing"),
            b"second"
        );
        assert!(!quarantine.exists());
        assert_eq!(report.restored_source_files, 1);
        assert!(report.retained_paths.is_empty());
    }

    fn write_installation_marker(root: &Path, installation_id: &InstallationId) {
        serde_json::to_writer(
            File::create(root.join(INSTALLATION_MARKER)).expect("installation marker"),
            &InstallationMarker {
                operation_id: "operation".to_owned(),
                installation_id: installation_id.clone(),
                installed_file_count: 1,
                installed_bytes: 1,
            },
        )
        .expect("write installation marker");
    }
}
