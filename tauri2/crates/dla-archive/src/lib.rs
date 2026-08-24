use std::{
    collections::BTreeSet,
    fs::File,
    io::BufReader,
    path::{Component, Path, PathBuf},
};

use dla_application::package_inspection::{PackageManifestError, PackageManifestReader};
use dla_domain::{
    installation::RelativePath,
    package::{
        ArchiveFormat, PackageIssue, PackageManifest, PackageManifestEntry, PackageSafety,
        PackageSourceSet, PackageSourceSetKind,
    },
};
use zip::ZipArchive;

mod installer;
mod maintenance;
mod rar;
mod rar_tool;
mod source;

pub use installer::DesktopPackageInstaller;
pub use maintenance::DesktopLibraryMaintenance;

const MAX_MANIFEST_ENTRIES: usize = 100_000;

#[derive(Default)]
pub struct DesktopPackageManifestReader {
    zip: ZipPackageManifestReader,
    rar: rar::RarPackageManifestReader,
}

impl DesktopPackageManifestReader {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PackageManifestReader for DesktopPackageManifestReader {
    fn read_manifest(
        &self,
        root_path: &str,
        source_set: &PackageSourceSet,
    ) -> Result<PackageManifest, PackageManifestError> {
        match source_set.kind {
            PackageSourceSetKind::SingleArchive
                if source_set.volumes.first().is_some_and(|source| {
                    source
                        .relative_path
                        .as_str()
                        .rsplit_once('.')
                        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("zip"))
                }) =>
            {
                self.zip.read_manifest(root_path, source_set)
            }
            PackageSourceSetKind::SingleArchive
            | PackageSourceSetKind::MultipartRar
            | PackageSourceSetKind::MultipartRarSfx => {
                self.rar.read_manifest(root_path, source_set)
            }
        }
    }
}

#[derive(Default)]
pub struct ZipPackageManifestReader;

impl ZipPackageManifestReader {
    pub fn new() -> Self {
        Self
    }
}

impl PackageManifestReader for ZipPackageManifestReader {
    fn read_manifest(
        &self,
        root_path: &str,
        source_set: &PackageSourceSet,
    ) -> Result<PackageManifest, PackageManifestError> {
        if source_set.kind != PackageSourceSetKind::SingleArchive || source_set.volumes.len() != 1 {
            return Err(PackageManifestError::UnsupportedFormat(
                "ZIP inspection requires exactly one archive volume".to_owned(),
            ));
        }
        let archive_relative_path = source_set
            .volumes
            .first()
            .map(|source| &source.relative_path)
            .ok_or_else(|| {
                PackageManifestError::Unavailable("package source set is empty".to_owned())
            })?;
        let root = Path::new(root_path)
            .canonicalize()
            .map_err(|error| PackageManifestError::Unavailable(error.to_string()))?;
        let archive_path = root.join(portable_path(archive_relative_path));
        let archive_path = archive_path
            .canonicalize()
            .map_err(|error| PackageManifestError::Unavailable(error.to_string()))?;
        if !archive_path.starts_with(&root) {
            return Err(PackageManifestError::Unavailable(
                "package source escapes the selected scan root".to_owned(),
            ));
        }
        if !archive_path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            return Err(PackageManifestError::UnsupportedFormat(
                archive_relative_path.to_string(),
            ));
        }
        let metadata = archive_path
            .symlink_metadata()
            .map_err(|error| PackageManifestError::Unavailable(error.to_string()))?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(PackageManifestError::Unavailable(
                "package source is not a regular file".to_owned(),
            ));
        }

        let file = File::open(&archive_path)
            .map_err(|error| PackageManifestError::Unavailable(error.to_string()))?;
        let mut archive = ZipArchive::new(BufReader::new(file))
            .map_err(|error| PackageManifestError::Inspection(error.to_string()))?;
        if archive.len() > MAX_MANIFEST_ENTRIES {
            return Err(PackageManifestError::Inspection(format!(
                "archive contains more than {MAX_MANIFEST_ENTRIES} entries"
            )));
        }

        let mut entries = Vec::with_capacity(archive.len());
        let mut issues = Vec::new();
        let mut path_keys = BTreeSet::new();
        let mut exact_paths = BTreeSet::new();
        let mut file_count = 0_u64;
        let mut directory_count = 0_u64;
        let mut total_compressed_bytes = 0_u64;
        let mut total_uncompressed_bytes = 0_u64;

        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|error| PackageManifestError::Inspection(error.to_string()))?;
            let raw_name = entry.name().to_owned();
            let relative_path = entry
                .enclosed_name()
                .and_then(|path| portable_relative_path(&path));
            let is_directory = entry.is_dir();
            let is_symlink = entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000);
            let encrypted = entry.encrypted();

            if relative_path.is_none() {
                issues.push(issue("unsafe_archive_path", index, Some(raw_name.clone())));
            }
            if is_symlink {
                issues.push(issue("archive_symlink", index, Some(raw_name.clone())));
            }
            if encrypted {
                issues.push(issue(
                    "encrypted_archive_entry",
                    index,
                    Some(raw_name.clone()),
                ));
            }
            if let Some(path) = &relative_path {
                let exact = path.as_str().to_owned();
                let path_key = exact.to_lowercase();
                if !exact_paths.insert(exact.clone()) {
                    issues.push(issue("duplicate_archive_path", index, Some(exact.clone())));
                }
                if !path_keys.insert(path_key) {
                    issues.push(issue("case_colliding_archive_path", index, Some(exact)));
                }
            }

            if is_directory {
                directory_count += 1;
            } else {
                file_count += 1;
            }
            total_compressed_bytes = total_compressed_bytes
                .checked_add(entry.compressed_size())
                .ok_or_else(|| {
                    PackageManifestError::Inspection("compressed size overflow".to_owned())
                })?;
            total_uncompressed_bytes = total_uncompressed_bytes
                .checked_add(entry.size())
                .ok_or_else(|| {
                    PackageManifestError::Inspection("uncompressed size overflow".to_owned())
                })?;
            entries.push(PackageManifestEntry {
                entry_index: index as u64,
                relative_path,
                raw_name,
                is_directory,
                is_symlink,
                encrypted,
                compressed_size: entry.compressed_size(),
                uncompressed_size: entry.size(),
                crc32: entry.crc32(),
            });
        }

        let common_root = common_root(&entries);
        let safety = if issues.is_empty() {
            PackageSafety::Safe
        } else {
            PackageSafety::Unsafe
        };
        Ok(PackageManifest {
            format: ArchiveFormat::Zip,
            entries,
            file_count,
            directory_count,
            total_compressed_bytes,
            total_uncompressed_bytes,
            common_root,
            safety,
            issues,
        })
    }
}

pub(crate) fn portable_path(path: &RelativePath) -> PathBuf {
    path.as_str().split('/').collect()
}

pub(crate) fn portable_relative_path(path: &Path) -> Option<RelativePath> {
    let parts = path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_owned),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if parts.is_empty() {
        return None;
    }
    RelativePath::parse(parts.join("/")).ok()
}

pub(crate) fn common_root(entries: &[PackageManifestEntry]) -> Option<RelativePath> {
    let roots = entries
        .iter()
        .filter_map(|entry| entry.relative_path.as_ref())
        .filter_map(|path| path.as_str().split('/').next())
        .collect::<BTreeSet<_>>();
    if roots.len() != 1 {
        return None;
    }
    RelativePath::parse((*roots.first()?).to_owned()).ok()
}

pub(crate) fn issue(code: &str, index: usize, path: Option<String>) -> PackageIssue {
    PackageIssue {
        code: code.to_owned(),
        entry_index: Some(index as u64),
        path,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    #[test]
    fn reads_a_manifest_without_extracting_archive_content() {
        let directory = tempdir().expect("temporary directory");
        let archive_path = directory.path().join("RJ000001.zip");
        let file = File::create(&archive_path).expect("archive file");
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("Work/Game.exe", SimpleFileOptions::default())
            .expect("game entry");
        writer.write_all(b"fixture").expect("game body");
        writer
            .start_file("Work/data/System.json", SimpleFileOptions::default())
            .expect("system entry");
        writer.write_all(b"{}").expect("system body");
        writer.finish().expect("complete archive");

        let manifest = ZipPackageManifestReader::new()
            .read_manifest(
                directory.path().to_str().expect("root path"),
                &PackageSourceSet {
                    kind: dla_domain::package::PackageSourceSetKind::SingleArchive,
                    volumes: vec![dla_domain::package::SourceArtifact {
                        scan_entry_id: dla_domain::scanner::ScanEntryId("entry".to_owned()),
                        kind: dla_domain::package::SourceArtifactKind::Archive,
                        relative_path: RelativePath::parse("RJ000001.zip").expect("archive path"),
                        size_bytes: None,
                        sha256: None,
                    }],
                },
            )
            .expect("manifest");

        assert_eq!(manifest.file_count, 2);
        assert_eq!(
            manifest.common_root.as_ref().map(RelativePath::as_str),
            Some("Work")
        );
        assert_eq!(manifest.safety, PackageSafety::Safe);
        assert!(!directory.path().join("Work").exists());
    }
}
