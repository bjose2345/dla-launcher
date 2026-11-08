use dla_application::paths::resolve_within;
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
use encoding_rs::SHIFT_JIS;
use zip::ZipArchive;

mod folder_transfer;
mod installer;
mod maintenance;
mod names;
mod rar;
mod rar_tool;
mod source;

pub use folder_transfer::{FolderTransfer, FolderTransferError, TransferMode, TransferOutcome};
pub use installer::DesktopPackageInstaller;
pub use maintenance::DesktopLibraryMaintenance;
pub use names::DesktopInstalledFileNames;

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
        let archive_path =
            resolve_within(Path::new(root_path), &portable_path(archive_relative_path))
                .map_err(|error| PackageManifestError::Unavailable(error.to_string()))?;
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
            let japanese = shift_jis_name(entry.name_raw());
            let raw_name = japanese.clone().unwrap_or_else(|| entry.name().to_owned());
            let relative_path = match &japanese {
                Some(name) => decoded_relative_path(name),
                None => entry
                    .enclosed_name()
                    .and_then(|path| portable_relative_path(&path)),
            };
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

/// Japanese archives made on Windows store names in Shift-JIS without the
/// UTF-8 flag, which the zip crate reads as CP437: `タイトル画面.png` became
/// `â^âCâgâïëµû╩.png`. A name that is not UTF-8 but is valid Shift-JIS is
/// read as Shift-JIS; any other keeps the zip crate's reading.
fn shift_jis_name(raw: &[u8]) -> Option<String> {
    if std::str::from_utf8(raw).is_ok() {
        return None;
    }
    let (name, had_errors) = SHIFT_JIS.decode_without_bom_handling(raw);
    (!had_errors).then(|| name.into_owned())
}

/// The safe path of a name decoded here, split only after decoding: the
/// second byte of ソ or 表 in Shift-JIS is `\`, which the zip crate takes
/// for a folder break. Folders are split at `/` or `\` like the zip crate
/// does; a root, a drive or `..` makes the entry unsafe.
fn decoded_relative_path(name: &str) -> Option<RelativePath> {
    if name.contains('\0') || name.starts_with(['/', '\\']) || name.get(1..2) == Some(":") {
        return None;
    }
    let parts = name
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .map(|part| (part != "..").then(|| part.to_owned()))
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

    /// Writes a ZIP whose names are the given bytes without the UTF-8 flag,
    /// as Japanese Windows makes them: the zip crate writes ASCII stand-ins of
    /// the same length, whose bytes are then replaced.
    fn zip_with_raw_names(path: &Path, names: &[Vec<u8>], utf8_names: &[&str]) {
        let stand_in = |name: &[u8]| {
            name.iter()
                .map(|&byte| if byte.is_ascii() { byte } else { b'x' })
                .collect::<Vec<_>>()
        };
        let mut writer = ZipWriter::new(File::create(path).expect("archive file"));
        for name in names {
            let stand_in = String::from_utf8(stand_in(name)).expect("ascii stand-in");
            writer
                .start_file(stand_in, SimpleFileOptions::default())
                .expect("entry");
            writer.write_all(b"fixture").expect("body");
        }
        for name in utf8_names {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("utf-8 entry");
            writer.write_all(b"fixture").expect("body");
        }
        writer.finish().expect("complete archive");

        let mut bytes = std::fs::read(path).expect("archive bytes");
        for name in names {
            let from = stand_in(name);
            let mut replaced = 0;
            let mut at = 0;
            while let Some(found) = bytes[at..]
                .windows(from.len())
                .position(|window| window == from.as_slice())
            {
                let start = at + found;
                bytes[start..start + from.len()].copy_from_slice(name);
                at = start + from.len();
                replaced += 1;
            }
            // The local header and the central directory.
            assert_eq!(replaced, 2, "{}", String::from_utf8_lossy(&from));
        }
        std::fs::write(path, bytes).expect("patched archive");
    }

    fn read_manifest(root: &Path, archive: &str) -> PackageManifest {
        ZipPackageManifestReader::new()
            .read_manifest(
                root.to_str().expect("root path"),
                &PackageSourceSet {
                    kind: PackageSourceSetKind::SingleArchive,
                    volumes: vec![dla_domain::package::SourceArtifact {
                        scan_entry_id: dla_domain::scanner::ScanEntryId("entry".to_owned()),
                        kind: dla_domain::package::SourceArtifactKind::Archive,
                        relative_path: RelativePath::parse(archive).expect("archive path"),
                        size_bytes: None,
                        sha256: None,
                    }],
                },
            )
            .expect("manifest")
    }

    #[test]
    fn reads_japanese_names_stored_in_shift_jis() {
        let directory = tempdir().expect("temporary directory");
        let shift_jis = |name: &str| SHIFT_JIS.encode(name).0.into_owned();
        zip_with_raw_names(
            &directory.path().join("RJ000002.zip"),
            &[
                shift_jis("生意気 v2/img/titles1/タイトル画面.png"),
                // The second byte of ソ and of 表 is `\`.
                shift_jis("生意気 v2/ソフト.txt"),
                shift_jis("生意気 v2/表紙.png"),
            ],
            &["生意気 v2/説明.txt"],
        );

        let manifest = read_manifest(directory.path(), "RJ000002.zip");

        let paths = manifest
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_ref().map(RelativePath::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            [
                Some("生意気 v2/img/titles1/タイトル画面.png"),
                Some("生意気 v2/ソフト.txt"),
                Some("生意気 v2/表紙.png"),
                Some("生意気 v2/説明.txt"),
            ]
        );
        assert_eq!(manifest.entries[1].raw_name, "生意気 v2/ソフト.txt");
        assert_eq!(
            manifest.common_root.as_ref().map(RelativePath::as_str),
            Some("生意気 v2")
        );
        assert_eq!(manifest.safety, PackageSafety::Safe);
    }

    #[test]
    fn keeps_the_old_reading_of_names_that_are_not_shift_jis() {
        let directory = tempdir().expect("temporary directory");
        // é in CP437, followed by a byte Shift-JIS cannot pair it with.
        zip_with_raw_names(
            &directory.path().join("RJ000003.zip"),
            &[b"Work/caf\x82.txt".to_vec()],
            &[],
        );

        let manifest = read_manifest(directory.path(), "RJ000003.zip");

        assert_eq!(
            manifest.entries[0]
                .relative_path
                .as_ref()
                .map(RelativePath::as_str),
            Some("Work/café.txt")
        );
    }

    #[test]
    fn decoded_names_stay_inside_the_package() {
        let path = |name| decoded_relative_path(name).map(|path| path.as_str().to_owned());
        assert_eq!(path("作品\\表紙.png").as_deref(), Some("作品/表紙.png"));
        assert_eq!(path("作品/./表紙.png").as_deref(), Some("作品/表紙.png"));
        assert_eq!(path("作品/").as_deref(), Some("作品"));
        assert_eq!(path("../表紙.png"), None);
        assert_eq!(path("作品/../../表紙.png"), None);
        assert_eq!(path("/表紙.png"), None);
        assert_eq!(path("\\表紙.png"), None);
        assert_eq!(path("C:表紙.png"), None);
        assert_eq!(path("作品/表紙\0.png"), None);
    }
}
