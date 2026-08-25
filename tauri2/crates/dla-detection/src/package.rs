use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use dla_domain::{
    installation::{
        InferenceConfidence, InstallationPlatform, LaunchActionKind, MediaType, RelativePath,
    },
    package::{
        ArchiveRetentionPolicy, CatalogPackageContext, CatalogPackageRelease, InstallPlan,
        PackageClassification, PackageContentKind, PackageInspection, PackageLaunchCandidate,
        PackageManifest, PackageSafety, PackageSourceSet, PackageSourceSetKind, SourceArtifact,
    },
};

use crate::media::classify_media_type;

const PORTABLE_PLATFORMS: [InstallationPlatform; 5] = [
    InstallationPlatform::Windows,
    InstallationPlatform::Linux,
    InstallationPlatform::Macos,
    InstallationPlatform::Android,
    InstallationPlatform::Ios,
];

pub fn classify_package(
    source: SourceArtifact,
    manifest: PackageManifest,
    catalog: Option<&CatalogPackageContext>,
    inspected_at: String,
) -> PackageInspection {
    let classification = if manifest.safety == PackageSafety::Safe {
        classify_safe_manifest(&source, &manifest, catalog)
    } else {
        PackageClassification {
            content_kind: PackageContentKind::Unknown,
            engine: None,
            platform: InstallationPlatform::Unknown,
            confidence: InferenceConfidence::Low,
            reason_codes: vec!["unsafe_archive_manifest".to_owned()],
            content_root: None,
            launch_candidates: Vec::new(),
        }
    };
    let install_plan = InstallPlan {
        requires_extraction: true,
        content_root: classification.content_root.clone(),
        preferred_action: classification.launch_candidates.first().cloned(),
        archive_retention: ArchiveRetentionPolicy::Keep,
    };

    PackageInspection {
        source: source.clone(),
        source_set: Some(PackageSourceSet {
            kind: PackageSourceSetKind::SingleArchive,
            volumes: vec![source],
        }),
        catalog_release: catalog.map(|context| CatalogPackageRelease {
            rom_position: context.rom_position,
            rom_count: context.rom_count,
            name: context.rom.name.clone(),
            version: context.rom.version.clone(),
            update_date: context.rom.update_date.clone(),
        }),
        format: manifest.format,
        safety: manifest.safety,
        entry_count: manifest.entries.len() as u64,
        file_count: manifest.file_count,
        directory_count: manifest.directory_count,
        total_compressed_bytes: manifest.total_compressed_bytes,
        total_uncompressed_bytes: manifest.total_uncompressed_bytes,
        common_root: manifest.common_root,
        issues: manifest.issues,
        classification,
        install_plan,
        inspected_at,
    }
}

fn classify_safe_manifest(
    source: &SourceArtifact,
    manifest: &PackageManifest,
    catalog: Option<&CatalogPackageContext>,
) -> PackageClassification {
    let paths = manifest
        .entries
        .iter()
        .filter(|entry| !entry.is_directory)
        .filter_map(|entry| entry.relative_path.as_ref())
        .map(|path| (path.as_str().to_lowercase(), path))
        .collect::<BTreeMap<_, _>>();
    let exact_catalog_archive = catalog.is_some_and(|context| {
        source.sha256.as_ref().is_some_and(|sha256| {
            !context.rom.sha256.is_empty() && context.rom.sha256.eq_ignore_ascii_case(sha256)
        })
    });
    let catalog_paths = catalog
        .and_then(|context| context.contents.as_ref())
        .map(|contents| {
            contents
                .entries
                .iter()
                .filter(|entry| !entry.is_directory)
                .map(|entry| entry.path.replace('\\', "/").to_lowercase())
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    let mut scored = Vec::new();
    for (key, path) in &paths {
        if !key.ends_with(".exe") || is_helper_executable(key) {
            continue;
        }
        let parent = parent_path(path.as_str());
        let file_name = key.rsplit('/').next().unwrap_or(key);
        let mut score = 25;
        let mut reasons = vec!["windows_executable".to_owned()];
        if file_name == "game.exe" {
            score += 100;
            reasons.push("conventional_game_executable".to_owned());
        }
        let system_json = child_key(parent.as_deref(), "data/system.json");
        if paths.contains_key(&system_json) {
            score += 70;
            reasons.push("rpg_maker_system_manifest".to_owned());
        }
        let package_json = child_key(parent.as_deref(), "package.json");
        if paths.contains_key(&package_json) {
            score += 30;
            reasons.push("nwjs_package_manifest".to_owned());
        }
        if exact_catalog_archive && catalog_paths.contains(key) {
            score += 35;
            reasons.push("catalog_internal_manifest_match".to_owned());
        }
        scored.push((score, (*path).clone(), reasons));
    }
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));

    let launch_candidates = scored
        .iter()
        .take(5)
        .map(|(score, path, reasons)| PackageLaunchCandidate {
            action: LaunchActionKind::LaunchExecutable,
            relative_path: path.clone(),
            supported_platforms: vec![InstallationPlatform::Windows],
            confidence: confidence_for_score(*score),
            reason_codes: reasons.clone(),
            expected_sha256: catalog_sha256(catalog, path),
        })
        .collect::<Vec<_>>();
    let content_root = launch_candidates
        .first()
        .and_then(|candidate| parent_path(candidate.relative_path.as_str()))
        .and_then(|path| RelativePath::parse(path).ok());
    let engine = content_root
        .as_ref()
        .map(|path| child_key(Some(path.as_str()), "data/system.json"))
        .or_else(|| Some("data/system.json".to_owned()))
        .filter(|path| paths.contains_key(path))
        .map(|_| "RPG Maker / NW.js".to_owned());
    let catalog_application = catalog.is_some_and(|context| {
        context
            .file_format_names
            .iter()
            .any(|value| value.to_lowercase().contains("application"))
    });
    let catalog_game = catalog.is_some_and(|context| {
        context.category_names.iter().any(|value| {
            let value = value.to_lowercase();
            value.contains("role-playing") || value.contains("rpg") || value.contains("game")
        })
    });

    if !launch_candidates.is_empty() {
        let content_kind = if engine.is_some() || catalog_game {
            PackageContentKind::WindowsGame
        } else {
            PackageContentKind::WindowsApplication
        };
        let mut reasons = vec!["executable_launch_candidate".to_owned()];
        if catalog_application {
            reasons.push("catalog_application_format".to_owned());
        }
        if catalog_game {
            reasons.push("catalog_game_category".to_owned());
        }
        if engine.is_some() {
            reasons.push("rpg_maker_layout".to_owned());
        }
        return PackageClassification {
            content_kind,
            engine,
            platform: InstallationPlatform::Windows,
            confidence: launch_candidates[0].confidence,
            reason_codes: reasons,
            content_root,
            launch_candidates,
        };
    }

    if let Some(classification) = classify_package_media_paths(
        &paths
            .values()
            .map(|path| (*path).clone())
            .collect::<Vec<_>>(),
    ) {
        return classification;
    }

    PackageClassification {
        content_kind: PackageContentKind::Unknown,
        engine: None,
        platform: InstallationPlatform::Unknown,
        confidence: InferenceConfidence::Low,
        reason_codes: vec!["no_safe_launch_candidate".to_owned()],
        content_root: manifest.common_root.clone(),
        launch_candidates,
    }
}

pub fn classify_package_media_paths(paths: &[RelativePath]) -> Option<PackageClassification> {
    let mut audio = Vec::new();
    let mut images = Vec::new();
    let mut videos = Vec::new();
    let mut documents = Vec::new();
    for path in paths {
        match classify_media_type(path) {
            MediaType::Audio => audio.push(path.clone()),
            MediaType::Image => images.push(path.clone()),
            MediaType::Video => videos.push(path.clone()),
            MediaType::Pdf => documents.push(path.clone()),
            _ => {}
        }
    }
    for family in [&mut audio, &mut images, &mut videos, &mut documents] {
        family.sort();
    }
    if audio.is_empty() && images.is_empty() && videos.is_empty() && documents.is_empty() {
        return None;
    }

    let mut primary_families = [
        (MediaType::Audio, audio.len()),
        (MediaType::Video, videos.len()),
        (MediaType::Pdf, documents.len()),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .collect::<Vec<_>>();
    primary_families.sort_by_key(|family| Reverse(family.1));
    if primary_families
        .get(1)
        .is_some_and(|second| second.1 == primary_families[0].1)
    {
        return Some(mixed_media_classification());
    }

    match primary_families.first().map(|family| family.0) {
        Some(MediaType::Audio) => Some(media_collection_classification(
            PackageContentKind::AudioCollection,
            LaunchActionKind::PlayAudio,
            preferred_collection_entry(&audio, true),
            InferenceConfidence::High,
            "dominant_audio_set",
        )),
        Some(MediaType::Video) => Some(media_collection_classification(
            PackageContentKind::VideoCollection,
            LaunchActionKind::PlayVideo,
            videos.first().cloned(),
            if videos.len() == 1 {
                InferenceConfidence::High
            } else {
                InferenceConfidence::Medium
            },
            if videos.len() == 1 {
                "single_video"
            } else {
                "video_file"
            },
        )),
        Some(MediaType::Pdf) => Some(media_collection_classification(
            PackageContentKind::MixedMedia,
            LaunchActionKind::OpenDocument,
            documents.first().cloned(),
            if documents.len() == 1 {
                InferenceConfidence::High
            } else {
                InferenceConfidence::Medium
            },
            if documents.len() == 1 {
                "single_pdf"
            } else {
                "pdf_document"
            },
        )),
        Some(_) => None,
        None if !images.is_empty() => Some(media_collection_classification(
            PackageContentKind::ImageCollection,
            LaunchActionKind::ReadImages,
            preferred_collection_entry(&images, false),
            if images.len() > 1 {
                InferenceConfidence::High
            } else {
                InferenceConfidence::Medium
            },
            "dominant_image_set",
        )),
        None => None,
    }
}

fn media_collection_classification(
    content_kind: PackageContentKind,
    action: LaunchActionKind,
    preferred_path: Option<RelativePath>,
    confidence: InferenceConfidence,
    reason: &str,
) -> PackageClassification {
    let reason_codes = vec![reason.to_owned()];
    let launch_candidates = preferred_path
        .as_ref()
        .map(|relative_path| PackageLaunchCandidate {
            action,
            relative_path: relative_path.clone(),
            supported_platforms: PORTABLE_PLATFORMS.to_vec(),
            confidence,
            reason_codes: reason_codes.clone(),
            expected_sha256: None,
        })
        .into_iter()
        .collect();
    PackageClassification {
        content_kind,
        engine: None,
        platform: InstallationPlatform::Unknown,
        confidence,
        reason_codes,
        content_root: preferred_path.as_ref().and_then(relative_parent),
        launch_candidates,
    }
}

fn mixed_media_classification() -> PackageClassification {
    PackageClassification {
        content_kind: PackageContentKind::MixedMedia,
        engine: None,
        platform: InstallationPlatform::Unknown,
        confidence: InferenceConfidence::Low,
        reason_codes: vec!["no_safe_launch_candidate".to_owned()],
        content_root: None,
        launch_candidates: Vec::new(),
    }
}

fn preferred_collection_entry(
    paths: &[RelativePath],
    prefer_compressed_audio: bool,
) -> Option<RelativePath> {
    let mut groups = BTreeMap::<String, Vec<&RelativePath>>::new();
    for path in paths {
        groups
            .entry(parent_path(path.as_str()).unwrap_or_default())
            .or_default()
            .push(path);
    }
    groups
        .into_iter()
        .min_by_key(|(parent, entries)| {
            let format_rank = if prefer_compressed_audio {
                entries
                    .iter()
                    .map(|path| audio_format_rank(path))
                    .min()
                    .unwrap_or(0)
            } else {
                0
            };
            (Reverse(entries.len()), format_rank, parent.clone())
        })
        .and_then(|(_, entries)| entries.first().map(|path| (*path).clone()))
}

fn audio_format_rank(path: &RelativePath) -> u8 {
    match Path::new(path.as_str())
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mp3") => 0,
        Some("m4a" | "aac") => 1,
        Some("ogg" | "opus") => 2,
        Some("flac") => 3,
        Some("wav" | "wave") => 4,
        _ => 5,
    }
}

fn relative_parent(path: &RelativePath) -> Option<RelativePath> {
    parent_path(path.as_str()).and_then(|parent| {
        (!parent.is_empty())
            .then(|| RelativePath::parse(parent).ok())
            .flatten()
    })
}

fn catalog_sha256(catalog: Option<&CatalogPackageContext>, path: &RelativePath) -> Option<String> {
    catalog
        .and_then(|context| context.contents.as_ref())
        .and_then(|contents| {
            contents.entries.iter().find(|entry| {
                !entry.is_directory
                    && entry
                        .path
                        .replace('\\', "/")
                        .eq_ignore_ascii_case(path.as_str())
            })
        })
        .map(|entry| entry.sha256.trim().to_ascii_lowercase())
        .filter(|sha256| !sha256.is_empty())
}

fn parent_path(path: &str) -> Option<String> {
    path.rsplit_once('/').map(|(parent, _)| parent.to_owned())
}

fn child_key(parent: Option<&str>, child: &str) -> String {
    parent
        .filter(|value| !value.is_empty())
        .map_or_else(|| child.to_owned(), |value| format!("{value}/{child}"))
        .to_lowercase()
}

fn is_helper_executable(path: &str) -> bool {
    let file_name = path.rsplit('/').next().unwrap_or(path);
    [
        "notification_helper",
        "unins",
        "uninstall",
        "setup",
        "update",
        "updater",
        "crashpad",
        "config",
        "configuration",
    ]
    .iter()
    .any(|marker| file_name.contains(marker))
}

fn confidence_for_score(score: i32) -> InferenceConfidence {
    if score >= 120 {
        InferenceConfidence::High
    } else if score >= 60 {
        InferenceConfidence::Medium
    } else {
        InferenceConfidence::Low
    }
}

#[cfg(test)]
mod tests {
    use dla_domain::{
        installation::RelativePath,
        package::{ArchiveFormat, PackageManifestEntry},
        scanner::ScanEntryId,
    };

    use super::*;

    fn entry(index: u64, path: &str, size: u64) -> PackageManifestEntry {
        PackageManifestEntry {
            entry_index: index,
            relative_path: Some(RelativePath::parse(path).expect("fixture path")),
            raw_name: path.to_owned(),
            is_directory: false,
            is_symlink: false,
            encrypted: false,
            compressed_size: size,
            uncompressed_size: size,
            crc32: 0,
        }
    }

    #[test]
    fn selects_the_game_executable_and_ignores_packaged_helpers() {
        let manifest = PackageManifest {
            format: ArchiveFormat::Zip,
            entries: vec![
                entry(0, "Work/Game.exe", 20),
                entry(1, "Work/notification_helper.exe", 10),
                entry(2, "Work/data/System.json", 5),
                entry(3, "Work/package.json", 5),
                entry(4, "Work/audio/bgm.ogg_", 100),
            ],
            file_count: 5,
            directory_count: 0,
            total_compressed_bytes: 140,
            total_uncompressed_bytes: 140,
            common_root: Some(RelativePath::parse("Work").expect("root")),
            safety: PackageSafety::Safe,
            issues: Vec::new(),
        };
        let source = SourceArtifact {
            scan_entry_id: ScanEntryId("entry-1".to_owned()),
            kind: dla_domain::package::SourceArtifactKind::Archive,
            relative_path: RelativePath::parse("RJ000001.zip").expect("source path"),
            size_bytes: Some(140),
            sha256: None,
        };

        let inspection =
            classify_package(source, manifest, None, "2026-08-08T00:00:00Z".to_owned());

        assert_eq!(
            inspection.classification.content_kind,
            PackageContentKind::WindowsGame
        );
        assert_eq!(
            inspection.classification.engine.as_deref(),
            Some("RPG Maker / NW.js")
        );
        assert_eq!(
            inspection
                .install_plan
                .preferred_action
                .as_ref()
                .map(|candidate| candidate.relative_path.as_str()),
            Some("Work/Game.exe")
        );
        assert_eq!(
            inspection
                .install_plan
                .content_root
                .as_ref()
                .map(RelativePath::as_str),
            Some("Work")
        );
        assert_eq!(inspection.classification.launch_candidates.len(), 1);
    }

    #[test]
    fn selects_the_compressed_album_inside_an_audio_package() {
        let manifest = PackageManifest {
            format: ArchiveFormat::Zip,
            entries: vec![
                entry(0, "mp3/sa02_01.mp3", 20),
                entry(1, "mp3/sa02_02.mp3", 20),
                entry(2, "wav/sa02_01.wav", 100),
                entry(3, "wav/sa02_02.wav", 100),
                entry(4, "omake/cover.jpg", 5),
            ],
            file_count: 5,
            directory_count: 0,
            total_compressed_bytes: 245,
            total_uncompressed_bytes: 245,
            common_root: None,
            safety: PackageSafety::Safe,
            issues: Vec::new(),
        };
        let source = SourceArtifact {
            scan_entry_id: ScanEntryId("entry-audio".to_owned()),
            kind: dla_domain::package::SourceArtifactKind::Archive,
            relative_path: RelativePath::parse("RJ01678999.zip").expect("source path"),
            size_bytes: Some(245),
            sha256: None,
        };

        let inspection =
            classify_package(source, manifest, None, "2026-08-13T00:00:00Z".to_owned());

        assert_eq!(
            inspection.classification.content_kind,
            PackageContentKind::AudioCollection
        );
        let preferred = inspection
            .install_plan
            .preferred_action
            .expect("audio action");
        assert_eq!(preferred.action, LaunchActionKind::PlayAudio);
        assert_eq!(preferred.relative_path.as_str(), "mp3/sa02_01.mp3");
        assert_eq!(
            inspection
                .install_plan
                .content_root
                .as_ref()
                .map(RelativePath::as_str),
            Some("mp3")
        );
    }

    #[test]
    fn leaves_equally_weighted_media_families_without_an_automatic_action() {
        let classification = classify_package_media_paths(&[
            RelativePath::parse("audio/track.mp3").expect("audio"),
            RelativePath::parse("video/scene.mp4").expect("video"),
        ])
        .expect("media classification");

        assert_eq!(classification.content_kind, PackageContentKind::MixedMedia);
        assert!(classification.launch_candidates.is_empty());
    }
}
