use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    process::Stdio,
};

use dla_application::package_inspection::{PackageManifestError, PackageManifestReader};
use dla_domain::{
    installation::RelativePath,
    package::{
        ArchiveFormat, PackageManifest, PackageManifestEntry, PackageSafety, PackageSourceSet,
        PackageSourceSetKind,
    },
};

use crate::{
    MAX_MANIFEST_ENTRIES, common_root, issue,
    rar_tool::{RarToolKind, rar_tool_candidates},
    source::resolve_source_files,
};

const MAX_LISTING_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct RarPackageManifestReader;

impl PackageManifestReader for RarPackageManifestReader {
    fn read_manifest(
        &self,
        root_path: &str,
        source_set: &PackageSourceSet,
    ) -> Result<PackageManifest, PackageManifestError> {
        validate_kind(source_set)?;
        let (_, paths) = resolve_source_files(root_path, source_set)?;
        let primary = paths.first().expect("source set validated");
        inspect_archive(primary)
    }
}

fn validate_kind(source_set: &PackageSourceSet) -> Result<(), PackageManifestError> {
    let primary = source_set.volumes.first().ok_or_else(|| {
        PackageManifestError::Unavailable("package source set is empty".to_owned())
    })?;
    let extension = primary
        .relative_path
        .as_str()
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    let valid = match source_set.kind {
        PackageSourceSetKind::SingleArchive | PackageSourceSetKind::MultipartRar => {
            extension == "rar"
        }
        PackageSourceSetKind::MultipartRarSfx => extension == "exe",
    };
    if valid {
        Ok(())
    } else {
        Err(PackageManifestError::UnsupportedFormat(
            primary.relative_path.to_string(),
        ))
    }
}

fn inspect_archive(primary: &std::path::Path) -> Result<PackageManifest, PackageManifestError> {
    let mut unavailable = Vec::new();
    for tool in rar_tool_candidates() {
        let mut command = tool.listing_command(primary);
        let mut child = match command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                unavailable.push(tool.program().to_owned());
                continue;
            }
            Err(error) => return Err(PackageManifestError::Inspection(error.to_string())),
        };
        let mut stdout = Vec::new();
        child
            .stdout
            .take()
            .expect("piped stdout")
            .take(MAX_LISTING_BYTES + 1)
            .read_to_end(&mut stdout)
            .map_err(|error| PackageManifestError::Inspection(error.to_string()))?;
        let output = child
            .wait_with_output()
            .map_err(|error| PackageManifestError::Inspection(error.to_string()))?;
        if stdout.len() as u64 > MAX_LISTING_BYTES {
            return Err(PackageManifestError::Inspection(
                "archive listing exceeds the safety limit".to_owned(),
            ));
        }
        if !output.status.success() {
            return Err(PackageManifestError::Inspection(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        let listing = String::from_utf8(stdout)
            .map_err(|error| PackageManifestError::Inspection(error.to_string()))?;
        return match tool.kind() {
            RarToolKind::SevenZip => parse_listing(&listing),
            RarToolKind::Unrar => parse_unrar_listing(&listing),
        };
    }
    Err(PackageManifestError::Unavailable(format!(
        "RAR support requires 7-Zip or UnRAR ({})",
        unavailable.join(", ")
    )))
}

fn parse_listing(output: &str) -> Result<PackageManifest, PackageManifestError> {
    let normalized = output.replace("\r\n", "\n");
    let records = normalized
        .split("\n\n")
        .map(parse_record)
        .filter(|record| !record.is_empty())
        .collect::<Vec<_>>();
    manifest_from_records(records)
}

fn parse_unrar_listing(output: &str) -> Result<PackageManifest, PackageManifestError> {
    let normalized = output.replace("\r\n", "\n");
    let records = normalized
        .split("\n\n")
        .map(parse_unrar_record)
        .filter(|record| record.contains_key("Path"))
        .collect::<Vec<_>>();
    manifest_from_records(records)
}

fn manifest_from_records(
    records: Vec<BTreeMap<String, String>>,
) -> Result<PackageManifest, PackageManifestError> {
    if records.len() > MAX_MANIFEST_ENTRIES {
        return Err(PackageManifestError::Inspection(format!(
            "archive contains more than {MAX_MANIFEST_ENTRIES} entries"
        )));
    }
    let mut entries = Vec::with_capacity(records.len());
    let mut issues = Vec::new();
    let mut exact_paths = BTreeSet::new();
    let mut path_keys = BTreeSet::new();
    let mut file_count = 0_u64;
    let mut directory_count = 0_u64;
    let mut total_compressed_bytes = 0_u64;
    let mut total_uncompressed_bytes = 0_u64;
    for (index, record) in records.into_iter().enumerate() {
        let Some(raw_name) = record.get("Path").cloned() else {
            continue;
        };
        let attributes = record
            .get("Attributes")
            .map(String::as_str)
            .unwrap_or_default();
        let is_directory = record.get("Type").is_some_and(|value| value == "Directory")
            || attributes
                .chars()
                .next()
                .is_some_and(|value| value.eq_ignore_ascii_case(&'d'))
            || raw_name.ends_with('/')
            || raw_name.ends_with('\\');
        let unix_mode = attributes.split_whitespace().last().unwrap_or_default();
        let is_symlink = unix_mode.starts_with('l');
        let encrypted = record.get("Encrypted").is_some_and(|value| value == "+");
        let normalized_name = raw_name.replace('\\', "/");
        let relative_path = RelativePath::parse(normalized_name.clone()).ok();
        if relative_path.is_none() || raw_name.chars().any(char::is_control) {
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
            if !exact_paths.insert(exact.clone()) {
                issues.push(issue("duplicate_archive_path", index, Some(exact.clone())));
            }
            if !path_keys.insert(exact.to_lowercase()) {
                issues.push(issue("case_colliding_archive_path", index, Some(exact)));
            }
        }
        let compressed_size = parse_size(record.get("Packed Size"), "packed")?;
        let uncompressed_size = parse_size(record.get("Size"), "uncompressed")?;
        total_compressed_bytes = total_compressed_bytes
            .checked_add(compressed_size)
            .ok_or_else(|| {
                PackageManifestError::Inspection("compressed size overflow".to_owned())
            })?;
        total_uncompressed_bytes = total_uncompressed_bytes
            .checked_add(uncompressed_size)
            .ok_or_else(|| {
                PackageManifestError::Inspection("uncompressed size overflow".to_owned())
            })?;
        if is_directory {
            directory_count += 1;
        } else {
            file_count += 1;
        }
        entries.push(PackageManifestEntry {
            entry_index: index as u64,
            relative_path,
            raw_name,
            is_directory,
            is_symlink,
            encrypted,
            compressed_size,
            uncompressed_size,
            crc32: record
                .get("CRC")
                .and_then(|value| u32::from_str_radix(value, 16).ok())
                .unwrap_or_default(),
        });
    }
    if entries.is_empty() {
        return Err(PackageManifestError::Inspection(
            "archive listing contained no entries".to_owned(),
        ));
    }
    let common_root = common_root(&entries);
    Ok(PackageManifest {
        format: ArchiveFormat::Rar,
        entries,
        file_count,
        directory_count,
        total_compressed_bytes,
        total_uncompressed_bytes,
        common_root,
        safety: if issues.is_empty() {
            PackageSafety::Safe
        } else {
            PackageSafety::Unsafe
        },
        issues,
    })
}

fn parse_record(block: &str) -> BTreeMap<String, String> {
    block
        .lines()
        .filter_map(|line| line.split_once(" = "))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn parse_unrar_record(block: &str) -> BTreeMap<String, String> {
    let mut record = BTreeMap::new();
    for line in block.lines() {
        let Some((key, value)) = line.trim_start().split_once(": ") else {
            continue;
        };
        match key {
            "Name" => {
                record.insert("Path".to_owned(), value.to_owned());
            }
            "Type" | "Size" | "Attributes" => {
                record.insert(key.to_owned(), value.to_owned());
            }
            "Packed size" => {
                record.insert("Packed Size".to_owned(), value.to_owned());
            }
            "CRC32" | "Pack-CRC32" => {
                record.insert("CRC".to_owned(), value.to_owned());
            }
            "Flags" if value.split_whitespace().any(|flag| flag == "encrypted") => {
                record.insert("Encrypted".to_owned(), "+".to_owned());
            }
            _ => {}
        }
    }
    record
}

fn parse_size(value: Option<&String>, label: &str) -> Result<u64, PackageManifestError> {
    value
        .map(String::as_str)
        .unwrap_or("0")
        .parse()
        .map_err(|_| {
            PackageManifestError::Inspection(format!("invalid {label} archive entry size"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_safe_seven_zip_technical_listing() {
        let manifest = parse_listing(
            "Path = Work/Game.exe\nSize = 7\nPacked Size = 5\nAttributes = A_ -rw-r--r--\nCRC = AABBCCDD\nEncrypted = -\n\nPath = Work/data/System.json\nSize = 2\nPacked Size = 2\nAttributes = A_ -rw-r--r--\nCRC = 00000000\nEncrypted = -\n",
        )
        .expect("manifest");
        assert_eq!(manifest.format, ArchiveFormat::Rar);
        assert_eq!(manifest.file_count, 2);
        assert_eq!(manifest.total_uncompressed_bytes, 9);
        assert_eq!(
            manifest.common_root.as_ref().map(RelativePath::as_str),
            Some("Work")
        );
        assert_eq!(manifest.safety, PackageSafety::Safe);
    }

    #[test]
    fn parses_windows_line_endings_from_seven_zip() {
        let manifest = parse_listing(
            "Path = Work\\Game.exe\r\nSize = 7\r\nPacked Size = 5\r\nAttributes = A_ -rw-r--r--\r\nCRC = AABBCCDD\r\nEncrypted = -\r\n\r\nPath = Work\\data\\System.json\r\nSize = 2\r\nPacked Size = 2\r\nAttributes = A_ -rw-r--r--\r\nCRC = 00000000\r\nEncrypted = -\r\n",
        )
        .expect("manifest");
        assert_eq!(manifest.file_count, 2);
        assert_eq!(manifest.safety, PackageSafety::Safe);
    }

    #[test]
    fn rejects_encrypted_symlink_and_traversal_entries() {
        let manifest = parse_listing(
            "Path = ../escape.exe\nSize = 7\nPacked Size = 5\nAttributes = A_ lrwxrwxrwx\nEncrypted = +\n",
        )
        .expect("manifest");
        assert_eq!(manifest.safety, PackageSafety::Unsafe);
        assert_eq!(manifest.issues.len(), 3);
    }

    #[test]
    fn parses_a_safe_unrar_technical_listing() {
        let manifest = parse_unrar_listing(
            "Archive: Work.rar\nDetails: RAR 5\n\n        Name: Work/Game.exe\n        Type: File\n        Size: 7\n Packed size: 5\n  Attributes: -rw-r--r--\n       CRC32: AABBCCDD\n\n        Name: Work/data\n        Type: Directory\n        Size: 0\n Packed size: 0\n  Attributes: drwxr-xr-x\n       CRC32: 00000000\n",
        )
        .expect("manifest");

        assert_eq!(manifest.file_count, 1);
        assert_eq!(manifest.directory_count, 1);
        assert_eq!(manifest.total_compressed_bytes, 5);
        assert_eq!(manifest.safety, PackageSafety::Safe);
    }

    #[test]
    fn rejects_unsafe_unrar_entries() {
        let manifest = parse_unrar_listing(
            "Archive: Work.rar\nDetails: RAR 5\n\n        Name: ../escape.exe\n        Type: File\n        Size: 7\n Packed size: 5\n  Attributes: lrwxrwxrwx\n       CRC32: AABBCCDD\n       Flags: encrypted\n",
        )
        .expect("manifest");

        assert_eq!(manifest.safety, PackageSafety::Unsafe);
        assert_eq!(manifest.issues.len(), 3);
    }
}
