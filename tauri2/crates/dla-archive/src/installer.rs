use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    thread,
    time::Duration,
};

use dla_application::{
    package_inspection::PackageManifestReader,
    package_preparation::{
        PackageDestinationConflictPolicy, PackageDestinationInspection, PackageDestinationPreview,
        PackageDestinationState, PackageExtractionResult, PackageInstallExecution,
        PackageInstaller, PackagePreparationCancellationToken, PackagePreparationError,
        PackagePreparationProgressSink,
    },
};
use dla_domain::package::{
    ArchiveFormat, PackageManifest, PackagePreparationCounters, PackagePreparationProgress,
    PackagePreparationStage, PackageSafety, PackageSourceSet,
};
use fs2::available_space;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::ZipArchive;

use crate::{
    DesktopPackageManifestReader, portable_path, rar_tool::rar_tool_candidates,
    source::resolve_source_files,
};

const COPY_BUFFER_BYTES: usize = 128 * 1024;
const MINIMUM_FREE_SPACE_MARGIN: u64 = 64 * 1024 * 1024;
pub(crate) const INSTALLATION_MARKER: &str = ".dla-installation.json";
pub(crate) const STAGING_MARKER: &str = ".dla-stage.json";
pub(crate) const SOURCE_CLEANUP_MARKER: &str = ".dla-source-cleanup.json";

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InstallationMarker {
    pub operation_id: String,
    pub installation_id: dla_domain::installation::InstallationId,
    pub installed_file_count: u64,
    pub installed_bytes: u64,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagingMarker {
    pub operation_id: String,
    pub installation_id: dla_domain::installation::InstallationId,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceCleanupMarker {
    pub file_names: Vec<String>,
}

#[derive(Default)]
pub struct DesktopPackageInstaller;

impl DesktopPackageInstaller {
    pub fn new() -> Self {
        Self
    }
}

impl PackageInstaller for DesktopPackageInstaller {
    fn inspect_destination(
        &self,
        request: &PackageDestinationInspection,
    ) -> Result<PackageDestinationPreview, PackagePreparationError> {
        let destination_parent = validate_destination_parent(&request.destination_parent)?;
        inspect_destination(
            &destination_parent,
            &request.destination_name,
            &request.installation_id,
        )
    }

    fn extract(
        &self,
        request: &PackageInstallExecution,
        cancellation: &PackagePreparationCancellationToken,
        progress: &dyn PackagePreparationProgressSink,
    ) -> Result<PackageExtractionResult, PackagePreparationError> {
        cancellation.check()?;
        let manifest = DesktopPackageManifestReader::new()
            .read_manifest(&request.source_root, &request.source_set)
            .map_err(PackagePreparationError::adapter)?;
        if manifest.safety != PackageSafety::Safe {
            return Err(PackagePreparationError::UnsafePackage);
        }
        validate_internal_paths(&manifest)?;
        let (_, sources) = resolve_source_files(&request.source_root, &request.source_set)
            .map_err(PackagePreparationError::adapter)?;
        validate_source_hashes(request, &sources, cancellation)?;
        let destination_parent = validate_destination_parent(&request.destination_parent)?;
        let mut destination = select_destination(
            &destination_parent,
            &request.destination_name,
            &request.installation_id,
            request.destination_conflict_policy,
        )?;
        validate_available_space(&destination_parent, manifest.total_uncompressed_bytes)?;
        let staging = destination_parent.join(format!(
            ".dla-stage-{}-{}",
            request.operation_id,
            Uuid::new_v4()
        ));
        fs::create_dir(&staging).map_err(PackagePreparationError::adapter)?;
        write_staging_marker(&staging, request)?;

        let result = (|| {
            match manifest.format {
                ArchiveFormat::Zip => extract_zip(
                    request,
                    &sources[0],
                    &staging,
                    &manifest,
                    cancellation,
                    progress,
                )?,
                ArchiveFormat::Rar => extract_rar(
                    request,
                    &sources[0],
                    &staging,
                    &manifest,
                    cancellation,
                    progress,
                )?,
            }
            let verified = verify_staging(request, &staging, &manifest, cancellation, progress)?;
            fs::remove_file(staging.join(STAGING_MARKER))
                .map_err(PackagePreparationError::adapter)?;
            write_marker(&staging, request, &verified)?;
            publish(
                request,
                progress,
                PackagePreparationStage::Activating,
                completed_counters(&manifest),
                None,
                "Activating the verified installation atomically",
            )?;
            cancellation.check()?;
            loop {
                match rename_directory_noreplace(&staging, &destination) {
                    Ok(()) => break,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::AlreadyExists
                            && request.destination_conflict_policy
                                == PackageDestinationConflictPolicy::KeepBoth =>
                    {
                        destination = destination_parent.join(next_available_destination_name(
                            &destination_parent,
                            &request.destination_name,
                        )?);
                    }
                    Err(error) => return Err(PackagePreparationError::adapter(error)),
                }
            }
            Ok(PackageExtractionResult {
                destination_root: destination.to_string_lossy().into_owned(),
                installed_file_count: verified.installed_file_count,
                installed_bytes: verified.installed_bytes,
            })
        })();
        if result.is_err() && staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    fn rollback(&self, destination_root: &str) -> Result<(), PackagePreparationError> {
        let destination = Path::new(destination_root);
        let metadata =
            fs::symlink_metadata(destination).map_err(PackagePreparationError::adapter)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(PackagePreparationError::adapter(
                "rollback target is not a regular installation directory",
            ));
        }
        if !destination.join(INSTALLATION_MARKER).is_file() {
            return Err(PackagePreparationError::adapter(
                "rollback target has no DLA installation marker",
            ));
        }
        fs::remove_dir_all(destination).map_err(PackagePreparationError::adapter)
    }

    fn delete_sources(
        &self,
        source_root: &str,
        source_set: &PackageSourceSet,
    ) -> Result<(), PackagePreparationError> {
        let (_, sources) = resolve_source_files(source_root, source_set)
            .map_err(PackagePreparationError::adapter)?;
        quarantine_and_delete_sources(&sources)
    }

    fn repair(
        &self,
        request: &PackageInstallExecution,
        destination_root: &str,
        cancellation: &PackagePreparationCancellationToken,
        progress: &dyn PackagePreparationProgressSink,
    ) -> Result<PackageExtractionResult, PackagePreparationError> {
        let destination = Path::new(destination_root);
        validate_owned_installation(destination, &request.installation_id)?;
        let parent = destination
            .parent()
            .ok_or_else(|| PackagePreparationError::adapter("repair target has no parent"))?
            .canonicalize()
            .map_err(PackagePreparationError::adapter)?;
        let destination_name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| {
                PackagePreparationError::adapter("repair target has no portable name")
            })?;
        let repair_name = format!(".dla-repair-{}-{}", request.operation_id, Uuid::new_v4());
        let repair_request = PackageInstallExecution {
            operation_id: request.operation_id.clone(),
            installation_id: request.installation_id.clone(),
            source_root: request.source_root.clone(),
            destination_parent: parent.to_string_lossy().into_owned(),
            destination_name: repair_name,
            destination_conflict_policy: PackageDestinationConflictPolicy::Refuse,
            inspection: request.inspection.clone(),
            source_set: request.source_set.clone(),
        };
        let repaired = self.extract(&repair_request, cancellation, progress)?;
        let repaired_path = PathBuf::from(&repaired.destination_root);
        let backup = parent.join(format!(
            ".dla-backup-{}-{}",
            request.operation_id,
            Uuid::new_v4()
        ));
        cancellation.check()?;
        if let Err(error) = fs::rename(destination, &backup) {
            let _ = fs::remove_dir_all(&repaired_path);
            return Err(PackagePreparationError::adapter(error));
        }
        if let Err(error) = fs::rename(&repaired_path, parent.join(destination_name)) {
            let restore = fs::rename(&backup, destination);
            let _ = fs::remove_dir_all(&repaired_path);
            return match restore {
                Ok(()) => Err(PackagePreparationError::adapter(error)),
                Err(restore_error) => Err(PackagePreparationError::adapter(format!(
                    "repair activation failed ({error}) and the previous installation could not be restored ({restore_error})"
                ))),
            };
        }
        if validate_owned_installation(&backup, &request.installation_id).is_ok() {
            let _ = fs::remove_dir_all(&backup);
        }
        Ok(PackageExtractionResult {
            destination_root: destination.to_string_lossy().into_owned(),
            installed_file_count: repaired.installed_file_count,
            installed_bytes: repaired.installed_bytes,
        })
    }
}

fn write_staging_marker(
    staging: &Path,
    request: &PackageInstallExecution,
) -> Result<(), PackagePreparationError> {
    let marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging.join(STAGING_MARKER))
        .map_err(PackagePreparationError::adapter)?;
    serde_json::to_writer(
        marker,
        &StagingMarker {
            operation_id: request.operation_id.clone(),
            installation_id: request.installation_id.clone(),
        },
    )
    .map_err(PackagePreparationError::adapter)
}

pub(crate) fn read_staging_marker(
    staging: &Path,
) -> Result<StagingMarker, PackagePreparationError> {
    let marker =
        File::open(staging.join(STAGING_MARKER)).map_err(PackagePreparationError::adapter)?;
    serde_json::from_reader(BufReader::new(marker)).map_err(PackagePreparationError::adapter)
}

fn quarantine_and_delete_sources(sources: &[PathBuf]) -> Result<(), PackagePreparationError> {
    let parent = sources
        .first()
        .and_then(|source| source.parent())
        .ok_or_else(|| PackagePreparationError::adapter("package source set is empty"))?;
    if sources.iter().any(|source| source.parent() != Some(parent)) {
        return Err(PackagePreparationError::adapter(
            "package source volumes do not share one directory",
        ));
    }
    let file_names = sources
        .iter()
        .map(|source| {
            source
                .file_name()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    PackagePreparationError::adapter("package source has no portable file name")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if file_names
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != file_names.len()
    {
        return Err(PackagePreparationError::adapter(
            "package source set contains duplicate file names",
        ));
    }
    let quarantine = parent.join(format!(".dla-source-cleanup-{}", Uuid::new_v4()));
    fs::create_dir(&quarantine).map_err(PackagePreparationError::adapter)?;
    let marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(quarantine.join(SOURCE_CLEANUP_MARKER))
        .map_err(PackagePreparationError::adapter)?;
    serde_json::to_writer(marker, &SourceCleanupMarker { file_names })
        .map_err(PackagePreparationError::adapter)?;
    let mut moved = Vec::with_capacity(sources.len());
    for source in sources {
        let Some(file_name) = source.file_name() else {
            restore_quarantined_sources(&moved, &quarantine)?;
            return Err(PackagePreparationError::adapter(
                "package source has no file name",
            ));
        };
        let quarantined = quarantine.join(file_name);
        if let Err(error) = fs::rename(source, &quarantined) {
            restore_quarantined_sources(&moved, &quarantine)?;
            return Err(PackagePreparationError::adapter(error));
        }
        moved.push((source.clone(), quarantined));
    }
    if let Err(error) = fs::remove_dir_all(&quarantine) {
        restore_quarantined_sources(&moved, &quarantine)?;
        return Err(PackagePreparationError::adapter(error));
    }
    Ok(())
}

fn restore_quarantined_sources(
    moved: &[(PathBuf, PathBuf)],
    quarantine: &Path,
) -> Result<(), PackagePreparationError> {
    let mut failures = Vec::new();
    for (source, quarantined) in moved.iter().rev() {
        if let Err(error) = fs::rename(quarantined, source) {
            failures.push(format!("{}: {error}", source.display()));
        }
    }
    if failures.is_empty() {
        if quarantine.exists() {
            let marker = quarantine.join(SOURCE_CLEANUP_MARKER);
            if marker.exists() {
                fs::remove_file(marker).map_err(PackagePreparationError::adapter)?;
            }
            fs::remove_dir(quarantine).map_err(PackagePreparationError::adapter)?;
        }
        Ok(())
    } else {
        Err(PackagePreparationError::adapter(format!(
            "could not restore the complete source set: {}",
            failures.join("; ")
        )))
    }
}

fn validate_internal_paths(manifest: &PackageManifest) -> Result<(), PackagePreparationError> {
    if manifest.entries.iter().any(|entry| {
        entry.relative_path.as_ref().is_some_and(|path| {
            [INSTALLATION_MARKER, STAGING_MARKER]
                .iter()
                .any(|reserved| path.as_str().eq_ignore_ascii_case(reserved))
        })
    }) {
        return Err(PackagePreparationError::adapter(
            "archive contains a path reserved for DLA installation metadata",
        ));
    }
    Ok(())
}

fn validate_destination_parent(path: &str) -> Result<PathBuf, PackagePreparationError> {
    let unresolved = Path::new(path);
    let metadata = fs::symlink_metadata(unresolved).map_err(PackagePreparationError::adapter)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PackagePreparationError::adapter(
            "installation destination is not a regular directory",
        ));
    }
    unresolved
        .canonicalize()
        .map_err(PackagePreparationError::adapter)
}

fn inspect_destination(
    parent: &Path,
    destination_name: &str,
    installation_id: &dla_domain::installation::InstallationId,
) -> Result<PackageDestinationPreview, PackagePreparationError> {
    let destination = parent.join(destination_name);
    let metadata = match fs::symlink_metadata(&destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PackageDestinationPreview {
                state: PackageDestinationState::Available,
                destination_name: destination_name.to_owned(),
                keep_both_destination_name: None,
            });
        }
        Err(error) => return Err(PackagePreparationError::adapter(error)),
    };
    let state = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        match read_installation_marker(&destination) {
            Ok(marker) if marker.installation_id == *installation_id => {
                PackageDestinationState::ManagedSameInstallation
            }
            Ok(_) => PackageDestinationState::ManagedOtherInstallation,
            Err(_) => PackageDestinationState::OccupiedUnknown,
        }
    } else {
        PackageDestinationState::OccupiedUnknown
    };
    let keep_both_destination_name = (state != PackageDestinationState::ManagedSameInstallation)
        .then(|| next_available_destination_name(parent, destination_name))
        .transpose()?;
    Ok(PackageDestinationPreview {
        state,
        destination_name: destination_name.to_owned(),
        keep_both_destination_name,
    })
}

fn select_destination(
    parent: &Path,
    destination_name: &str,
    installation_id: &dla_domain::installation::InstallationId,
    policy: PackageDestinationConflictPolicy,
) -> Result<PathBuf, PackagePreparationError> {
    let preview = inspect_destination(parent, destination_name, installation_id)?;
    match (preview.state, policy) {
        (PackageDestinationState::Available, _) => Ok(parent.join(destination_name)),
        (PackageDestinationState::ManagedSameInstallation, _) => {
            Err(PackagePreparationError::adapter(
                "the destination is already managed for this installation",
            ))
        }
        (_, PackageDestinationConflictPolicy::Refuse) => Err(PackagePreparationError::adapter(
            format!("installation destination already contains {destination_name}"),
        )),
        (_, PackageDestinationConflictPolicy::KeepBoth) => {
            Ok(parent.join(next_available_destination_name(parent, destination_name)?))
        }
    }
}

fn next_available_destination_name(
    parent: &Path,
    destination_name: &str,
) -> Result<String, PackagePreparationError> {
    for ordinal in 2..=9_999 {
        let candidate = format!("{destination_name} ({ordinal})");
        if destination_is_available(&parent.join(&candidate))? {
            return Ok(candidate);
        }
    }
    Err(PackagePreparationError::adapter(
        "could not find an available keep-both destination name",
    ))
}

fn destination_is_available(path: &Path) -> Result<bool, PackagePreparationError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(PackagePreparationError::adapter(error)),
    }
}

#[cfg(target_os = "linux")]
fn rename_directory_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "source contains NUL")
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination contains NUL")
    })?;
    // renameat2 with RENAME_NOREPLACE is the activation boundary: a concurrent
    // creator must turn into a conflict instead of being replaced.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "macos")]
fn rename_directory_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    let source = CString::new(source.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "source contains NUL")
    })?;
    let destination = CString::new(destination.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "destination contains NUL")
    })?;
    // renamex_np with RENAME_EXCL provides the same no-replace activation
    // invariant as Linux renameat2.
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
fn rename_directory_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn rename_directory_noreplace(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace package activation is not available on this platform",
    ))
}

fn validate_available_space(
    parent: &Path,
    expanded_bytes: u64,
) -> Result<(), PackagePreparationError> {
    let margin = (expanded_bytes / 10).max(MINIMUM_FREE_SPACE_MARGIN);
    let required = expanded_bytes.saturating_add(margin);
    let available = available_space(parent).map_err(PackagePreparationError::adapter)?;
    if available < required {
        return Err(PackagePreparationError::adapter(format!(
            "insufficient disk space: {required} bytes required, {available} bytes available"
        )));
    }
    Ok(())
}

fn validate_source_hashes(
    request: &PackageInstallExecution,
    sources: &[PathBuf],
    cancellation: &PackagePreparationCancellationToken,
) -> Result<(), PackagePreparationError> {
    for (volume, source) in request.source_set.volumes.iter().zip(sources) {
        let Some(expected) = volume.sha256.as_deref() else {
            continue;
        };
        let actual = sha256_file(source, cancellation)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(PackagePreparationError::adapter(format!(
                "package volume hash changed after scanning: {}",
                volume.relative_path
            )));
        }
    }
    Ok(())
}

fn extract_zip(
    request: &PackageInstallExecution,
    source: &Path,
    staging: &Path,
    manifest: &PackageManifest,
    cancellation: &PackagePreparationCancellationToken,
    progress: &dyn PackagePreparationProgressSink,
) -> Result<(), PackagePreparationError> {
    let file = File::open(source).map_err(PackagePreparationError::adapter)?;
    let mut archive =
        ZipArchive::new(BufReader::new(file)).map_err(PackagePreparationError::adapter)?;
    let mut counters = PackagePreparationCounters {
        total_bytes: manifest.total_uncompressed_bytes,
        total_files: manifest.file_count,
        ..PackagePreparationCounters::default()
    };
    for expected in &manifest.entries {
        cancellation.check()?;
        let relative = expected
            .relative_path
            .as_ref()
            .ok_or(PackagePreparationError::UnsafePackage)?;
        let output_path = staging.join(portable_path(relative));
        let mut entry = archive
            .by_index(expected.entry_index as usize)
            .map_err(PackagePreparationError::adapter)?;
        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(PackagePreparationError::adapter)?;
            continue;
        }
        if entry.encrypted()
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            || entry.size() != expected.uncompressed_size
        {
            return Err(PackagePreparationError::UnsafePackage);
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(PackagePreparationError::adapter)?;
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)
            .map_err(PackagePreparationError::adapter)?;
        let mut written = 0_u64;
        let mut buffer = [0_u8; COPY_BUFFER_BYTES];
        loop {
            cancellation.check()?;
            let count = entry
                .read(&mut buffer)
                .map_err(PackagePreparationError::adapter)?;
            if count == 0 {
                break;
            }
            written = written
                .checked_add(count as u64)
                .ok_or_else(|| PackagePreparationError::adapter("extracted size overflow"))?;
            if written > expected.uncompressed_size {
                return Err(PackagePreparationError::adapter(
                    "archive entry exceeded its declared size",
                ));
            }
            output
                .write_all(&buffer[..count])
                .map_err(PackagePreparationError::adapter)?;
            counters.processed_bytes = counters.processed_bytes.saturating_add(count as u64);
            publish(
                request,
                progress,
                PackagePreparationStage::Extracting,
                counters.clone(),
                Some(relative.to_string()),
                "Extracting ZIP content",
            )?;
        }
        if written != expected.uncompressed_size {
            return Err(PackagePreparationError::adapter(
                "archive entry size did not match its manifest",
            ));
        }
        counters.processed_files += 1;
    }
    Ok(())
}

fn extract_rar(
    request: &PackageInstallExecution,
    primary: &Path,
    staging: &Path,
    manifest: &PackageManifest,
    cancellation: &PackagePreparationCancellationToken,
    progress: &dyn PackagePreparationProgressSink,
) -> Result<(), PackagePreparationError> {
    publish(
        request,
        progress,
        PackagePreparationStage::Extracting,
        PackagePreparationCounters {
            total_bytes: manifest.total_uncompressed_bytes,
            total_files: manifest.file_count,
            ..PackagePreparationCounters::default()
        },
        Some(primary.to_string_lossy().into_owned()),
        "Extracting RAR volumes as archive data; executable volumes are never run",
    )?;
    let mut unavailable = Vec::new();
    for tool in rar_tool_candidates() {
        let mut command = tool.extraction_command(primary, staging);
        let mut child = match command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                unavailable.push(tool.program().to_owned());
                continue;
            }
            Err(error) => return Err(PackagePreparationError::adapter(error)),
        };
        loop {
            if cancellation.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PackagePreparationError::Cancelled);
            }
            match child.try_wait().map_err(PackagePreparationError::adapter)? {
                Some(status) if status.success() => return Ok(()),
                Some(status) => {
                    return Err(PackagePreparationError::adapter(format!(
                        "{} extraction failed with status {status}",
                        tool.label()
                    )));
                }
                None => thread::sleep(Duration::from_millis(50)),
            }
        }
    }
    Err(PackagePreparationError::adapter(format!(
        "RAR support requires 7-Zip or UnRAR ({})",
        unavailable.join(", ")
    )))
}

fn verify_staging(
    request: &PackageInstallExecution,
    staging: &Path,
    manifest: &PackageManifest,
    cancellation: &PackagePreparationCancellationToken,
    progress: &dyn PackagePreparationProgressSink,
) -> Result<PackageExtractionResult, PackagePreparationError> {
    let expected = manifest
        .entries
        .iter()
        .filter(|entry| !entry.is_directory)
        .map(|entry| {
            entry
                .relative_path
                .as_ref()
                .map(|path| (path.as_str().to_owned(), entry.uncompressed_size))
                .ok_or(PackagePreparationError::UnsafePackage)
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut actual = BTreeMap::new();
    let mut counters = PackagePreparationCounters {
        total_bytes: manifest.total_uncompressed_bytes,
        total_files: manifest.file_count,
        ..PackagePreparationCounters::default()
    };
    for entry in WalkDir::new(staging).follow_links(false).into_iter() {
        cancellation.check()?;
        let entry = entry.map_err(PackagePreparationError::adapter)?;
        if entry.path() == staging {
            continue;
        }
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(PackagePreparationError::adapter)?;
        if metadata.file_type().is_symlink() {
            return Err(PackagePreparationError::adapter(
                "extracted content contains a symbolic link",
            ));
        }
        if metadata.is_dir() {
            continue;
        }
        if !metadata.is_file() {
            return Err(PackagePreparationError::adapter(
                "extracted content contains a special filesystem entry",
            ));
        }
        let relative = entry
            .path()
            .strip_prefix(staging)
            .map_err(PackagePreparationError::adapter)?;
        let key = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if key == STAGING_MARKER {
            continue;
        }
        if actual.insert(key.clone(), metadata.len()).is_some() {
            return Err(PackagePreparationError::adapter(
                "extracted content contains duplicate paths",
            ));
        }
        counters.processed_files += 1;
        counters.processed_bytes = counters.processed_bytes.saturating_add(metadata.len());
        publish(
            request,
            progress,
            PackagePreparationStage::Verifying,
            counters.clone(),
            Some(key),
            "Verifying extracted files against the archive manifest",
        )?;
    }
    if actual != expected {
        return Err(PackagePreparationError::adapter(
            "extracted files do not match the inspected archive manifest",
        ));
    }
    if let Some(content_root) = request.inspection.install_plan.content_root.as_ref() {
        let root = staging.join(portable_path(content_root));
        if !root.is_dir() {
            return Err(PackagePreparationError::adapter(
                "detected content root is missing after extraction",
            ));
        }
    }
    if let Some(action) = request.inspection.install_plan.preferred_action.as_ref() {
        let target = staging.join(portable_path(&action.relative_path));
        if !target.is_file() {
            return Err(PackagePreparationError::adapter(
                "preferred action target is missing after extraction",
            ));
        }
        if let Some(expected_hash) = action.expected_sha256.as_deref() {
            let actual_hash = sha256_file(&target, cancellation)?;
            if !actual_hash.eq_ignore_ascii_case(expected_hash) {
                return Err(PackagePreparationError::adapter(
                    "preferred action hash does not match the catalog",
                ));
            }
        }
    }
    Ok(PackageExtractionResult {
        destination_root: staging.to_string_lossy().into_owned(),
        installed_file_count: counters.processed_files,
        installed_bytes: counters.processed_bytes,
    })
}

fn write_marker(
    staging: &Path,
    request: &PackageInstallExecution,
    verified: &PackageExtractionResult,
) -> Result<(), PackagePreparationError> {
    let body = InstallationMarker {
        operation_id: request.operation_id.clone(),
        installation_id: request.installation_id.clone(),
        installed_file_count: verified.installed_file_count,
        installed_bytes: verified.installed_bytes,
    };
    let marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(staging.join(INSTALLATION_MARKER))
        .map_err(PackagePreparationError::adapter)?;
    serde_json::to_writer(marker, &body).map_err(PackagePreparationError::adapter)
}

pub(crate) fn read_installation_marker(
    root: &Path,
) -> Result<InstallationMarker, PackagePreparationError> {
    let marker =
        File::open(root.join(INSTALLATION_MARKER)).map_err(PackagePreparationError::adapter)?;
    serde_json::from_reader(BufReader::new(marker)).map_err(PackagePreparationError::adapter)
}

pub(crate) fn validate_owned_installation(
    root: &Path,
    installation_id: &dla_domain::installation::InstallationId,
) -> Result<InstallationMarker, PackagePreparationError> {
    let metadata = fs::symlink_metadata(root).map_err(PackagePreparationError::adapter)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(PackagePreparationError::adapter(
            "target is not a regular installation directory",
        ));
    }
    let marker = read_installation_marker(root)?;
    if marker.installation_id != *installation_id {
        return Err(PackagePreparationError::adapter(
            "DLA installation marker belongs to a different installation",
        ));
    }
    Ok(marker)
}

fn sha256_file(
    path: &Path,
    cancellation: &PackagePreparationCancellationToken,
) -> Result<String, PackagePreparationError> {
    let mut file = File::open(path).map_err(PackagePreparationError::adapter)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    loop {
        cancellation.check()?;
        let count = file
            .read(&mut buffer)
            .map_err(PackagePreparationError::adapter)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn completed_counters(manifest: &PackageManifest) -> PackagePreparationCounters {
    PackagePreparationCounters {
        total_bytes: manifest.total_uncompressed_bytes,
        processed_bytes: manifest.total_uncompressed_bytes,
        total_files: manifest.file_count,
        processed_files: manifest.file_count,
    }
}

fn publish(
    request: &PackageInstallExecution,
    sink: &dyn PackagePreparationProgressSink,
    stage: PackagePreparationStage,
    counters: PackagePreparationCounters,
    current_path: Option<String>,
    detail: &str,
) -> Result<(), PackagePreparationError> {
    sink.publish(&PackagePreparationProgress {
        operation_id: request.operation_id.clone(),
        installation_id: request.installation_id.clone(),
        stage,
        counters,
        current_path,
        detail: detail.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::{io::Write, sync::Mutex};

    use dla_domain::{
        installation::{
            InferenceConfidence, InstallationId, InstallationPlatform, LaunchActionKind,
            RelativePath,
        },
        package::{
            ArchiveRetentionPolicy, InstallPlan, PackageClassification, PackageContentKind,
            PackageInspection, PackageLaunchCandidate, PackageSourceSetKind, SourceArtifact,
            SourceArtifactKind,
        },
        scanner::ScanEntryId,
    };
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    struct Progress(Mutex<Vec<PackagePreparationStage>>);

    impl PackagePreparationProgressSink for Progress {
        fn publish(
            &self,
            value: &PackagePreparationProgress,
        ) -> Result<(), PackagePreparationError> {
            self.0.lock().expect("progress").push(value.stage);
            Ok(())
        }
    }

    #[test]
    fn extracts_verifies_and_activates_a_zip_without_overwriting() {
        let directory = tempdir().expect("temporary directory");
        let source = directory.path().join("RJ000001.zip");
        let file = File::create(&source).expect("archive");
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("Work/Game.exe", SimpleFileOptions::default())
            .expect("entry");
        writer.write_all(b"fixture").expect("body");
        writer.finish().expect("finish");
        let source_set = PackageSourceSet {
            kind: PackageSourceSetKind::SingleArchive,
            volumes: vec![SourceArtifact {
                scan_entry_id: ScanEntryId("entry".to_owned()),
                kind: SourceArtifactKind::Archive,
                relative_path: RelativePath::parse("RJ000001.zip").expect("path"),
                size_bytes: Some(fs::metadata(&source).expect("metadata").len()),
                sha256: None,
            }],
        };
        let manifest = DesktopPackageManifestReader::new()
            .read_manifest(directory.path().to_str().expect("root"), &source_set)
            .expect("manifest");
        let action = PackageLaunchCandidate {
            action: LaunchActionKind::LaunchExecutable,
            relative_path: RelativePath::parse("Work/Game.exe").expect("target"),
            supported_platforms: vec![InstallationPlatform::Windows],
            confidence: InferenceConfidence::High,
            reason_codes: vec!["fixture".to_owned()],
            expected_sha256: None,
        };
        let request = PackageInstallExecution {
            operation_id: "operation".to_owned(),
            installation_id: InstallationId("installation".to_owned()),
            source_root: directory.path().to_string_lossy().into_owned(),
            destination_parent: directory.path().to_string_lossy().into_owned(),
            destination_name: "Installed".to_owned(),
            destination_conflict_policy: PackageDestinationConflictPolicy::Refuse,
            inspection: PackageInspection {
                source: source_set.volumes[0].clone(),
                source_set: Some(source_set.clone()),
                format: ArchiveFormat::Zip,
                safety: PackageSafety::Safe,
                entry_count: manifest.entries.len() as u64,
                file_count: manifest.file_count,
                directory_count: manifest.directory_count,
                total_compressed_bytes: manifest.total_compressed_bytes,
                total_uncompressed_bytes: manifest.total_uncompressed_bytes,
                common_root: manifest.common_root.clone(),
                issues: vec![],
                classification: PackageClassification {
                    content_kind: PackageContentKind::WindowsGame,
                    engine: None,
                    platform: InstallationPlatform::Windows,
                    confidence: InferenceConfidence::High,
                    reason_codes: vec!["fixture".to_owned()],
                    content_root: manifest.common_root.clone(),
                    launch_candidates: vec![action.clone()],
                },
                install_plan: InstallPlan {
                    requires_extraction: true,
                    content_root: manifest.common_root,
                    preferred_action: Some(action),
                    archive_retention: ArchiveRetentionPolicy::Keep,
                },
                inspected_at: "2026-08-08T00:00:00Z".to_owned(),
            },
            source_set,
        };
        let progress = Progress(Mutex::new(Vec::new()));
        let result = DesktopPackageInstaller::new()
            .extract(
                &request,
                &PackagePreparationCancellationToken::default(),
                &progress,
            )
            .expect("extract");
        assert_eq!(result.installed_file_count, 1);
        assert!(
            Path::new(&result.destination_root)
                .join("Work/Game.exe")
                .is_file()
        );
        assert!(
            Path::new(&result.destination_root)
                .join(INSTALLATION_MARKER)
                .is_file()
        );
        assert!(
            progress
                .0
                .lock()
                .expect("progress")
                .contains(&PackagePreparationStage::Verifying)
        );

        let occupied = directory.path().join("Occupied");
        fs::create_dir(&occupied).expect("occupied destination");
        fs::write(occupied.join("keep.txt"), b"existing").expect("existing file");
        let keep_both_request = PackageInstallExecution {
            operation_id: "keep-both-operation".to_owned(),
            destination_name: "Occupied".to_owned(),
            destination_conflict_policy: PackageDestinationConflictPolicy::KeepBoth,
            ..request.clone()
        };
        let kept_both = DesktopPackageInstaller::new()
            .extract(
                &keep_both_request,
                &PackagePreparationCancellationToken::default(),
                &progress,
            )
            .expect("keep both extraction");
        assert_eq!(
            fs::read(occupied.join("keep.txt")).expect("existing file"),
            b"existing"
        );
        assert_eq!(
            Path::new(&kept_both.destination_root)
                .file_name()
                .and_then(|name| name.to_str()),
            Some("Occupied (2)")
        );
    }

    #[test]
    fn destination_inspection_distinguishes_unknown_and_managed_owners() {
        let directory = tempdir().expect("temporary directory");
        let unknown = directory.path().join("Unknown");
        fs::create_dir(&unknown).expect("unknown destination");
        let same = directory.path().join("Same");
        fs::create_dir(&same).expect("same destination");
        fs::write(
            same.join(INSTALLATION_MARKER),
            serde_json::to_vec(&InstallationMarker {
                operation_id: "operation".to_owned(),
                installation_id: InstallationId("installation".to_owned()),
                installed_file_count: 1,
                installed_bytes: 1,
            })
            .expect("marker"),
        )
        .expect("same marker");
        let other = directory.path().join("Other");
        fs::create_dir(&other).expect("other destination");
        fs::write(
            other.join(INSTALLATION_MARKER),
            serde_json::to_vec(&InstallationMarker {
                operation_id: "operation".to_owned(),
                installation_id: InstallationId("other-installation".to_owned()),
                installed_file_count: 1,
                installed_bytes: 1,
            })
            .expect("marker"),
        )
        .expect("other marker");
        let installer = DesktopPackageInstaller::new();
        let inspect = |destination_name: &str| {
            installer
                .inspect_destination(&PackageDestinationInspection {
                    installation_id: InstallationId("installation".to_owned()),
                    destination_parent: directory.path().to_string_lossy().into_owned(),
                    destination_name: destination_name.to_owned(),
                })
                .expect("destination preview")
        };

        assert_eq!(
            inspect("Available").state,
            PackageDestinationState::Available
        );
        assert_eq!(
            inspect("Unknown").state,
            PackageDestinationState::OccupiedUnknown
        );
        assert_eq!(
            inspect("Same").state,
            PackageDestinationState::ManagedSameInstallation
        );
        assert_eq!(
            inspect("Other").state,
            PackageDestinationState::ManagedOtherInstallation
        );
        assert_eq!(
            inspect("Other").keep_both_destination_name.as_deref(),
            Some("Other (2)")
        );
    }

    #[test]
    fn atomic_activation_never_replaces_an_existing_directory() {
        let directory = tempdir().expect("temporary directory");
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        fs::create_dir(&source).expect("source");
        fs::create_dir(&destination).expect("destination");

        let error = rename_directory_noreplace(&source, &destination).expect_err("conflict");

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(source.is_dir());
        assert!(destination.is_dir());
    }

    #[test]
    fn removes_a_complete_source_set_through_one_quarantine() {
        let directory = tempdir().expect("temporary directory");
        let first = directory.path().join("Work.part01.rar");
        let second = directory.path().join("Work.part02.rar");
        fs::write(&first, b"first").expect("first volume");
        fs::write(&second, b"second").expect("second volume");

        quarantine_and_delete_sources(&[first.clone(), second.clone()]).expect("delete source set");

        assert!(!first.exists());
        assert!(!second.exists());
        assert_eq!(
            fs::read_dir(directory.path()).expect("directory").count(),
            0
        );
    }

    #[test]
    fn restores_moved_volumes_when_quarantine_cannot_collect_the_whole_set() {
        let directory = tempdir().expect("temporary directory");
        let first = directory.path().join("Work.part01.rar");
        let missing = directory.path().join("Work.part02.rar");
        fs::write(&first, b"first").expect("first volume");

        quarantine_and_delete_sources(&[first.clone(), missing]).expect_err("missing volume");

        assert_eq!(fs::read(&first).expect("restored volume"), b"first");
        assert_eq!(
            fs::read_dir(directory.path()).expect("directory").count(),
            1
        );
    }

    #[test]
    fn rejects_archive_entries_reserved_for_launcher_metadata() {
        let manifest = PackageManifest {
            format: ArchiveFormat::Zip,
            entries: vec![dla_domain::package::PackageManifestEntry {
                entry_index: 0,
                relative_path: Some(
                    RelativePath::parse(".DLA-INSTALLATION.JSON").expect("reserved path"),
                ),
                raw_name: ".DLA-INSTALLATION.JSON".to_owned(),
                is_directory: false,
                is_symlink: false,
                encrypted: false,
                compressed_size: 1,
                uncompressed_size: 1,
                crc32: 0,
            }],
            file_count: 1,
            directory_count: 0,
            total_compressed_bytes: 1,
            total_uncompressed_bytes: 1,
            common_root: None,
            safety: PackageSafety::Safe,
            issues: Vec::new(),
        };

        let error = validate_internal_paths(&manifest).expect_err("reserved path");

        assert!(error.to_string().contains("reserved"));
    }
}
