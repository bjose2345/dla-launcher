use std::{fs, path::Path};

use dla_detection::{MediaClassificationError, MediaClassificationRequest, classify_media};
use dla_domain::{
    installation::{
        CatalogIdentity, InstallationDetection, InstallationStatus, MediaType, RelativePathError,
    },
    scanner::{
        ScanEntry, ScanEntryId, ScanEntryKind, ScanEntryPresence, ScanRootId, ScanSessionId,
    },
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioManifest {
    schema_version: u32,
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scenario {
    id: String,
    fixture_root: String,
    expected: InstallationDetection,
}

#[test]
fn shared_fixture_trees_produce_the_approved_detection_contracts() {
    let fixture_root = fixture_root();
    let manifest: ScenarioManifest = serde_json::from_str(
        &fs::read_to_string(fixture_root.join("manifest-v1.json")).expect("fixture manifest"),
    )
    .expect("valid fixture manifest");
    assert_eq!(manifest.schema_version, 1);

    for scenario in manifest.scenarios {
        let mut entries = fixture_entries(&fixture_root.join(&scenario.fixture_root));
        entries.reverse();
        let actual = classify_media(MediaClassificationRequest {
            source_scan_session_id: None,
            catalog_identity: None,
            entries: &entries,
        })
        .unwrap_or_else(|error| panic!("{} classification failed: {error}", scenario.id));

        assert_eq!(actual, scenario.expected, "{} classification", scenario.id);
    }
}

#[test]
fn classification_preserves_source_identity_and_file_observations() {
    let mut entry = file_entry("Voice/track.FLAC");
    entry.size = Some("128".to_owned());
    entry.modified_at = Some("2026-08-07T10:00:00Z".to_owned());
    let session_id = ScanSessionId("scan-session-1".to_owned());
    let identity = CatalogIdentity {
        work_code: "RJ01326398".to_owned(),
        confidence: dla_domain::scanner::ScanMatchConfidence::Exact,
        reason_codes: vec!["archive_sha256_match".to_owned()],
    };

    let detection = classify_media(MediaClassificationRequest {
        source_scan_session_id: Some(session_id.clone()),
        catalog_identity: Some(identity.clone()),
        entries: &[entry],
    })
    .expect("classification");

    assert_eq!(detection.source_scan_session_id, Some(session_id));
    assert_eq!(detection.catalog_identity, Some(identity));
    assert_eq!(detection.content_items[0].size_bytes, Some(128));
    assert_eq!(
        detection.content_items[0].modified_at.as_deref(),
        Some("2026-08-07T10:00:00Z")
    );
    assert_eq!(detection.content_items[0].media_type, MediaType::Audio);
}

#[test]
fn missing_files_and_structural_directories_do_not_become_content() {
    let directory = directory_entry("images");
    let mut missing = file_entry("images/001.webp");
    missing.presence = ScanEntryPresence::Missing;

    let detection = classify_media(MediaClassificationRequest {
        source_scan_session_id: None,
        catalog_identity: None,
        entries: &[directory, missing],
    })
    .expect("classification");

    assert!(detection.content_items.is_empty());
    assert!(detection.launch_candidates.is_empty());
    assert_eq!(detection.suggested_status, InstallationStatus::NeedsReview);
}

#[test]
fn one_standalone_archive_is_reviewable_without_becoming_automatic_launch() {
    let detection = classify_media(MediaClassificationRequest {
        source_scan_session_id: None,
        catalog_identity: None,
        entries: &[file_entry("RJ01326398.zip")],
    })
    .expect("classification");

    assert_eq!(detection.launch_candidates.len(), 1);
    assert_eq!(
        detection.launch_candidates[0].action,
        dla_domain::installation::LaunchActionKind::OpenArchive
    );
    assert_eq!(
        detection.launch_candidates[0].confidence,
        dla_domain::installation::InferenceConfidence::Medium
    );
}

#[test]
fn malformed_scanner_paths_and_sizes_fail_closed() {
    let invalid_path = file_entry("../Game.exe");
    assert_eq!(
        classify_media(MediaClassificationRequest {
            source_scan_session_id: None,
            catalog_identity: None,
            entries: &[invalid_path],
        }),
        Err(MediaClassificationError::InvalidRelativePath {
            path: "../Game.exe".to_owned(),
            source: RelativePathError::UnsafeSegment,
        })
    );

    let mut invalid_size = file_entry("Game.exe");
    invalid_size.size = Some("not-a-number".to_owned());
    assert_eq!(
        classify_media(MediaClassificationRequest {
            source_scan_session_id: None,
            catalog_identity: None,
            entries: &[invalid_size],
        }),
        Err(MediaClassificationError::InvalidSize {
            path: "Game.exe".to_owned(),
            value: "not-a-number".to_owned(),
        })
    );
}

#[test]
fn repeated_candidate_names_receive_stable_unique_ids() {
    let entries = [
        file_entry("disc-1/movie.mp4"),
        file_entry("disc-2/movie.mp4"),
    ];
    let detection = classify_media(MediaClassificationRequest {
        source_scan_session_id: None,
        catalog_identity: None,
        entries: &entries,
    })
    .expect("classification");

    assert_eq!(detection.suggested_status, InstallationStatus::NeedsReview);
    assert_eq!(
        detection
            .launch_candidates
            .iter()
            .map(|candidate| candidate.id.0.as_str())
            .collect::<Vec<_>>(),
        vec!["play-movie", "play-movie-2"]
    );
}

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fixtures/library-content")
}

fn fixture_entries(root: &Path) -> Vec<ScanEntry> {
    let mut entries = Vec::new();
    collect_fixture_entries(root, root, &mut entries);
    entries
}

fn collect_fixture_entries(root: &Path, current: &Path, entries: &mut Vec<ScanEntry>) {
    let mut children = fs::read_dir(current)
        .expect("fixture directory")
        .map(|entry| entry.expect("fixture entry"))
        .collect::<Vec<_>>();
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let relative_path = path
            .strip_prefix(root)
            .expect("fixture path below root")
            .to_string_lossy()
            .replace('\\', "/");
        if child.file_type().expect("fixture file type").is_dir() {
            entries.push(directory_entry(&relative_path));
            collect_fixture_entries(root, &path, entries);
        } else {
            entries.push(file_entry(&relative_path));
        }
    }
}

fn file_entry(relative_path: &str) -> ScanEntry {
    entry(relative_path, ScanEntryKind::File)
}

fn directory_entry(relative_path: &str) -> ScanEntry {
    entry(relative_path, ScanEntryKind::Directory)
}

fn entry(relative_path: &str, kind: ScanEntryKind) -> ScanEntry {
    ScanEntry {
        id: ScanEntryId(format!("entry:{relative_path}")),
        root_id: ScanRootId("fixture-root".to_owned()),
        relative_path: relative_path.to_owned(),
        path_key: relative_path.to_lowercase(),
        kind,
        extension: Path::new(relative_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase(),
        size: None,
        modified_at: None,
        presence: ScanEntryPresence::Present,
        first_seen_session_id: None,
        last_seen_session_id: None,
        created_at: "2026-08-07T00:00:00Z".to_owned(),
        updated_at: "2026-08-07T00:00:00Z".to_owned(),
    }
}
