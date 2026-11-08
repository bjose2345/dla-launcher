//! Refresh the file list on a work the launcher unpacked: its install's
//! start file keeps it ready, however its other files look to a new scan.

use std::{fs, sync::Arc};

use dla_application::{
    installation::InstallationStore, maintenance::LibraryMaintenanceService,
    package_preparation::PackagePreparationStore,
};
use dla_archive::{
    DesktopLibraryMaintenance, DesktopPackageInstaller, DesktopPackageManifestReader,
};
use dla_domain::{
    installation::{
        CatalogIdentity, InferenceConfidence, Installation, InstallationDetection, InstallationId,
        InstallationOverrides, InstallationPlatform, InstallationStatus, LaunchActionKind,
        RelativePath,
    },
    package::{
        ArchiveRetentionPolicy, PackageLaunchCandidate, PackageSourceSet, PackageSourceSetKind,
        PreparedPackageInstallation, SourceArtifact, SourceArtifactKind,
    },
    scanner::{ScanEntryId, ScanMatchConfidence},
};
use dla_sqlite::SqliteLibraryStore;
use tempfile::tempdir;

fn path(value: &str) -> RelativePath {
    RelativePath::parse(value).expect("path")
}

#[test]
fn a_game_the_launcher_unpacked_stays_ready_while_its_start_file_is_there() {
    let directory = tempdir().expect("temporary directory");
    let work = directory.path().join("RJ000001");
    // A game with enough pictures and music that a scan alone would ask
    // whether Play starts the game, shows the pictures or plays the album.
    for file in [
        "作品/Game.exe",
        "作品/img/1.png",
        "作品/img/2.png",
        "作品/img/3.png",
        "作品/audio/1.ogg",
        "作品/audio/2.ogg",
        "作品/audio/3.ogg",
    ] {
        let file = work.join(file);
        fs::create_dir_all(file.parent().expect("parent")).expect("folders");
        fs::write(file, "data").expect("file");
    }
    let store = Arc::new(
        SqliteLibraryStore::open(&directory.path().join("library.sqlite")).expect("library"),
    );
    let id = InstallationId("installation-1".to_owned());
    store
        .create(&Installation {
            id: id.clone(),
            scan_root_id: None,
            root_path: work.to_str().expect("root").to_owned(),
            platform: InstallationPlatform::Windows,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: Some(CatalogIdentity {
                    work_code: "RJ000001".to_owned(),
                    confidence: ScanMatchConfidence::Exact,
                    reason_codes: vec!["code_in_filename".to_owned()],
                }),
                suggested_status: InstallationStatus::Ready,
                content_items: vec![],
                launch_candidates: vec![],
                package_inspection: None,
            },
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-10-30T00:00:00Z".to_owned(),
            updated_at: "2026-10-30T00:00:00Z".to_owned(),
        })
        .expect("installation");
    store
        .save_prepared_package(&PreparedPackageInstallation {
            installation_id: id.clone(),
            destination_root: work.to_str().expect("root").to_owned(),
            content_root: Some(path("作品")),
            preferred_action: Some(PackageLaunchCandidate {
                action: LaunchActionKind::LaunchExecutable,
                relative_path: path("作品/Game.exe"),
                supported_platforms: vec![InstallationPlatform::Windows],
                confidence: InferenceConfidence::High,
                reason_codes: vec!["conventional_game_executable".to_owned()],
                expected_sha256: None,
            }),
            source_set: PackageSourceSet {
                kind: PackageSourceSetKind::SingleArchive,
                volumes: vec![SourceArtifact {
                    scan_entry_id: ScanEntryId("entry".to_owned()),
                    kind: SourceArtifactKind::Archive,
                    relative_path: path("RJ000001.zip"),
                    size_bytes: None,
                    sha256: None,
                }],
            },
            archive_retention: ArchiveRetentionPolicy::Keep,
            sources_deleted: false,
            source_cleanup_error: None,
            installed_file_count: 7,
            installed_bytes: 28,
            prepared_at: "2026-10-30T00:00:00Z".to_owned(),
        })
        .expect("prepared package");
    let service = LibraryMaintenanceService::new(
        store.clone(),
        store.clone(),
        store.clone(),
        Arc::new(DesktopLibraryMaintenance::new()),
        Arc::new(DesktopPackageManifestReader::new()),
        Arc::new(DesktopPackageInstaller::new()),
    );
    let refresh = || {
        service
            .rescan(&id, "2026-10-30T06:00:00Z".to_owned())
            .expect("refresh");
        store.read(&id).expect("read").expect("installation")
    };

    let installation = refresh();
    assert!(installation.detection.launch_candidates.len() > 1);
    assert_eq!(installation.status, InstallationStatus::Ready);

    // Without its start file, the work needs a look again.
    fs::remove_file(work.join("作品/Game.exe")).expect("remove");
    assert_eq!(refresh().status, InstallationStatus::NeedsReview);
}
