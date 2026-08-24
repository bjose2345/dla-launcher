use std::collections::BTreeMap;

use dla_domain::{
    installation::RelativePath,
    package::{PackageSourceSet, PackageSourceSetKind, SourceArtifact, SourceArtifactKind},
    scanner::{ScanEntry, ScanEntryId, ScanEntryKind, ScanEntryPresence},
};
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PackageSourceSetError {
    #[error("package source path is invalid: {0}")]
    InvalidPath(String),
    #[error("package source size is invalid: {0}")]
    InvalidSize(String),
    #[error("multipart package has duplicate part {0}")]
    DuplicatePart(u32),
    #[error("multipart package is missing part {0}")]
    MissingPart(u32),
    #[error("multipart package must begin with part 1")]
    MissingFirstPart,
    #[error("multipart SFX packages require one part 1 .exe and .rar continuation volumes")]
    InvalidSfxSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MultipartName {
    parent: String,
    base: String,
    part: u32,
    extension: String,
    legacy: bool,
}

pub fn discover_package_source_set(
    selected_entry_id: &ScanEntryId,
    entries: &[ScanEntry],
) -> Result<Option<PackageSourceSet>, PackageSourceSetError> {
    let Some(selected) = entries.iter().find(|entry| {
        &entry.id == selected_entry_id
            && entry.kind == ScanEntryKind::File
            && entry.presence == ScanEntryPresence::Present
    }) else {
        return Ok(None);
    };
    let extension = file_extension(&selected.relative_path);
    if extension == "zip" {
        return Ok(Some(PackageSourceSet {
            kind: PackageSourceSetKind::SingleArchive,
            volumes: vec![artifact(selected)?],
        }));
    }

    let descriptor = multipart_name(&selected.relative_path);
    if descriptor.is_none() && extension != "rar" {
        return Ok(None);
    }
    if descriptor.is_none() {
        let legacy_volumes = legacy_source_set(selected, entries)?;
        return Ok(Some(legacy_volumes.unwrap_or(PackageSourceSet {
            kind: PackageSourceSetKind::SingleArchive,
            volumes: vec![artifact(selected)?],
        })));
    }
    let selected_name = descriptor.expect("descriptor checked");
    let mut parts = BTreeMap::<u32, (&ScanEntry, String)>::new();
    for entry in entries.iter().filter(|entry| {
        entry.kind == ScanEntryKind::File && entry.presence == ScanEntryPresence::Present
    }) {
        let Some(name) = multipart_name(&entry.relative_path) else {
            continue;
        };
        if name.legacy != selected_name.legacy
            || name.parent != selected_name.parent
            || !name.base.eq_ignore_ascii_case(&selected_name.base)
        {
            continue;
        }
        if parts.insert(name.part, (entry, name.extension)).is_some() {
            return Err(PackageSourceSetError::DuplicatePart(name.part));
        }
    }
    let Some(last_part) = parts.keys().next_back().copied() else {
        return Ok(None);
    };
    if !parts.contains_key(&1) {
        return Err(PackageSourceSetError::MissingFirstPart);
    }
    for part in 1..=last_part {
        if !parts.contains_key(&part) {
            return Err(PackageSourceSetError::MissingPart(part));
        }
    }
    let first_extension = &parts.get(&1).expect("first part checked").1;
    let kind = if first_extension == "exe" {
        if parts.iter().any(|(part, (_, extension))| {
            (*part == 1 && extension != "exe") || (*part > 1 && extension != "rar")
        }) {
            return Err(PackageSourceSetError::InvalidSfxSet);
        }
        PackageSourceSetKind::MultipartRarSfx
    } else {
        if parts.values().any(|(_, extension)| extension != "rar") {
            return Err(PackageSourceSetError::InvalidSfxSet);
        }
        PackageSourceSetKind::MultipartRar
    };
    Ok(Some(PackageSourceSet {
        kind,
        volumes: parts
            .into_values()
            .map(|(entry, _)| artifact(entry))
            .collect::<Result<Vec<_>, _>>()?,
    }))
}

fn legacy_source_set(
    selected: &ScanEntry,
    entries: &[ScanEntry],
) -> Result<Option<PackageSourceSet>, PackageSourceSetError> {
    let (parent, file_name) = parent_and_name(&selected.relative_path);
    let base = file_name
        .strip_suffix(".rar")
        .or_else(|| file_name.strip_suffix(".RAR"))
        .unwrap_or(file_name);
    let mut parts = BTreeMap::<u32, &ScanEntry>::new();
    parts.insert(1, selected);
    for entry in entries.iter().filter(|entry| {
        entry.kind == ScanEntryKind::File && entry.presence == ScanEntryPresence::Present
    }) {
        let (entry_parent, entry_name) = parent_and_name(&entry.relative_path);
        if entry_parent != parent {
            continue;
        }
        let lower = entry_name.to_ascii_lowercase();
        let Some((candidate_base, suffix)) = lower.rsplit_once(".r") else {
            continue;
        };
        if !candidate_base.eq_ignore_ascii_case(base)
            || suffix.len() != 2
            || !suffix.bytes().all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let index = suffix.parse::<u32>().expect("two digits") + 2;
        if parts.insert(index, entry).is_some() {
            return Err(PackageSourceSetError::DuplicatePart(index));
        }
    }
    if parts.len() == 1 {
        return Ok(None);
    }
    let last_part = *parts.keys().next_back().expect("selected part");
    for part in 1..=last_part {
        if !parts.contains_key(&part) {
            return Err(PackageSourceSetError::MissingPart(part));
        }
    }
    Ok(Some(PackageSourceSet {
        kind: PackageSourceSetKind::MultipartRar,
        volumes: parts
            .into_values()
            .map(artifact)
            .collect::<Result<Vec<_>, _>>()?,
    }))
}

fn multipart_name(path: &str) -> Option<MultipartName> {
    let (parent, file_name) = parent_and_name(path);
    let lower = file_name.to_ascii_lowercase();
    let (stem, extension) = lower.rsplit_once('.')?;
    if !matches!(extension, "rar" | "exe") {
        return None;
    }
    let marker = stem.rfind(".part")?;
    let part_text = &stem[marker + 5..];
    if part_text.is_empty() || !part_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let part = part_text.parse().ok()?;
    Some(MultipartName {
        parent: parent.to_owned(),
        base: stem[..marker].to_owned(),
        part,
        extension: extension.to_owned(),
        legacy: false,
    })
}

fn artifact(entry: &ScanEntry) -> Result<SourceArtifact, PackageSourceSetError> {
    Ok(SourceArtifact {
        scan_entry_id: entry.id.clone(),
        kind: SourceArtifactKind::Archive,
        relative_path: RelativePath::parse(entry.relative_path.clone())
            .map_err(|_| PackageSourceSetError::InvalidPath(entry.relative_path.clone()))?,
        size_bytes: entry
            .size
            .as_deref()
            .map(str::parse)
            .transpose()
            .map_err(|_| PackageSourceSetError::InvalidSize(entry.relative_path.clone()))?,
        sha256: None,
    })
}

fn parent_and_name(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn file_extension(path: &str) -> String {
    parent_and_name(path)
        .1
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use dla_domain::scanner::{ScanEntryPresence, ScanRootId, ScanSessionId};

    use super::*;

    #[test]
    fn discovers_zero_padded_rar_parts_as_one_ordered_set() {
        let entries = vec![
            entry("third", "RJ000001.part003.rar"),
            entry("first", "RJ000001.part001.rar"),
            entry("second", "RJ000001.part002.rar"),
        ];
        let set = discover_package_source_set(&ScanEntryId("second".to_owned()), &entries)
            .expect("source set")
            .expect("supported set");

        assert_eq!(set.kind, PackageSourceSetKind::MultipartRar);
        assert_eq!(
            set.volumes
                .iter()
                .map(|volume| volume.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "RJ000001.part001.rar",
                "RJ000001.part002.rar",
                "RJ000001.part003.rar"
            ]
        );
    }

    #[test]
    fn accepts_supported_part_number_widths_from_a_continuation_volume() {
        for (first, second) in [
            ("Work.part1.rar", "Work.part2.rar"),
            ("Work.part01.rar", "Work.part02.rar"),
            ("Work.part001.rar", "Work.part002.rar"),
        ] {
            let entries = vec![entry("first", first), entry("second", second)];
            let set = discover_package_source_set(&ScanEntryId("second".to_owned()), &entries)
                .expect("source set")
                .expect("supported set");

            assert_eq!(set.kind, PackageSourceSetKind::MultipartRar);
            assert_eq!(set.volumes[0].relative_path.as_str(), first);
            assert_eq!(set.volumes[1].relative_path.as_str(), second);
        }
    }

    #[test]
    fn treats_the_executable_first_volume_as_archive_data() {
        let entries = vec![
            entry("first", "RJ000001.part01.exe"),
            entry("second", "RJ000001.part02.rar"),
        ];
        let set = discover_package_source_set(&ScanEntryId("first".to_owned()), &entries)
            .expect("source set")
            .expect("supported set");

        assert_eq!(set.kind, PackageSourceSetKind::MultipartRarSfx);
        assert_eq!(set.volumes[0].relative_path.as_str(), "RJ000001.part01.exe");
    }

    #[test]
    fn rejects_a_gap_in_a_multipart_set() {
        let entries = vec![
            entry("first", "RJ000001.part1.rar"),
            entry("third", "RJ000001.part3.rar"),
        ];
        assert_eq!(
            discover_package_source_set(&ScanEntryId("first".to_owned()), &entries),
            Err(PackageSourceSetError::MissingPart(2))
        );
    }

    #[test]
    fn rejects_executable_continuation_volumes() {
        let entries = vec![
            entry("first", "RJ000001.part01.exe"),
            entry("second", "RJ000001.part02.exe"),
        ];

        assert_eq!(
            discover_package_source_set(&ScanEntryId("first".to_owned()), &entries),
            Err(PackageSourceSetError::InvalidSfxSet)
        );
    }

    #[test]
    fn discovers_legacy_rar_continuation_volumes() {
        let entries = vec![
            entry("first", "RJ000001.rar"),
            entry("second", "RJ000001.r00"),
            entry("third", "RJ000001.r01"),
        ];
        let set = discover_package_source_set(&ScanEntryId("first".to_owned()), &entries)
            .expect("source set")
            .expect("supported set");

        assert_eq!(set.kind, PackageSourceSetKind::MultipartRar);
        assert_eq!(set.volumes.len(), 3);
    }

    #[test]
    fn never_treats_a_generic_executable_as_an_archive() {
        let entries = vec![entry("game", "Game.exe")];

        assert_eq!(
            discover_package_source_set(&ScanEntryId("game".to_owned()), &entries),
            Ok(None)
        );
    }

    fn entry(id: &str, path: &str) -> ScanEntry {
        ScanEntry {
            id: ScanEntryId(id.to_owned()),
            root_id: ScanRootId("root".to_owned()),
            relative_path: path.to_owned(),
            path_key: path.to_ascii_lowercase(),
            kind: ScanEntryKind::File,
            extension: file_extension(path),
            size: Some("10".to_owned()),
            modified_at: None,
            presence: ScanEntryPresence::Present,
            first_seen_session_id: Some(ScanSessionId("session".to_owned())),
            last_seen_session_id: Some(ScanSessionId("session".to_owned())),
            created_at: "2026-08-08T00:00:00Z".to_owned(),
            updated_at: "2026-08-08T00:00:00Z".to_owned(),
        }
    }
}
