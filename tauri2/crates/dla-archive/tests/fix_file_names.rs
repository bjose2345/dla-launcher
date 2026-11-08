//! Fixing an older install's wrongly read Japanese file names end to end:
//! the files on disk, the launcher's stored paths and the file list.

use std::{fs, sync::Arc};

use dla_application::{installation::InstallationStore, maintenance::LibraryMaintenanceService};
use dla_archive::{
    DesktopInstalledFileNames, DesktopLibraryMaintenance, DesktopPackageInstaller,
    DesktopPackageManifestReader,
};
use dla_domain::{
    installation::{
        CatalogIdentity, ContentItem, InferenceConfidence, Installation, InstallationDetection,
        InstallationId, InstallationOverrides, InstallationPlatform, InstallationStatus,
        LaunchActionKind, LaunchCandidate, LaunchCandidateId, LaunchTarget, ManualLaunchSelection,
        MediaType, RelativePath,
    },
    scanner::ScanMatchConfidence,
};
use dla_sqlite::SqliteLibraryStore;
use tempfile::tempdir;

// 作品/Game.exe, 作品/img/タイトル画面.png, 作品/表示板.png and a save, as the
// old install wrote them: Shift-JIS read as CP437, and 表 split.
const FILES: [(&str, &str, &str); 4] = [
    ("ì∞òi/Game.exe", "作品/Game.exe", "game"),
    (
        "ì∞òi/img/â^âCâgâïëµû╩.png",
        "作品/img/タイトル画面.png",
        "title",
    ),
    ("ì∞òi/ò/Äªö┬.png", "作品/表示板.png", "board"),
    (
        "ì∞òi/save/file1.rmmzsave",
        "作品/save/file1.rmmzsave",
        "save",
    ),
];

fn path(value: &str) -> RelativePath {
    RelativePath::parse(value).expect("path")
}

/// The file list and start choices as an install found them, keyed the
/// Windows way.
fn detection(paths: impl Fn(usize) -> &'static str) -> InstallationDetection {
    let item = |index: usize, media_type| ContentItem {
        relative_path: path(paths(index)),
        path_key: paths(index).to_lowercase(),
        media_type,
        size_bytes: None,
        modified_at: None,
        confidence: InferenceConfidence::High,
        reason_codes: vec!["installed_file".to_owned()],
    };
    // In path order, as the library lists them.
    let mut content_items = vec![
        item(0, MediaType::Executable),
        item(1, MediaType::Image),
        item(2, MediaType::Image),
        item(3, MediaType::Unknown),
    ];
    content_items.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    InstallationDetection {
        source_scan_session_id: None,
        catalog_identity: Some(CatalogIdentity {
            work_code: "RJ000001".to_owned(),
            confidence: ScanMatchConfidence::Exact,
            reason_codes: vec!["code_in_filename".to_owned()],
        }),
        suggested_status: InstallationStatus::Ready,
        content_items,
        launch_candidates: vec![
            LaunchCandidate {
                id: LaunchCandidateId("launch-game".to_owned()),
                action: LaunchActionKind::LaunchExecutable,
                target: LaunchTarget::RelativePath(path(paths(0))),
                supported_platforms: vec![InstallationPlatform::Windows],
                confidence: InferenceConfidence::High,
                reason_codes: vec!["preferred_executable_name".to_owned()],
            },
            LaunchCandidate {
                id: LaunchCandidateId("read-images".to_owned()),
                action: LaunchActionKind::ReadImages,
                target: LaunchTarget::InstallationRoot,
                supported_platforms: vec![InstallationPlatform::Windows],
                confidence: InferenceConfidence::Medium,
                reason_codes: vec!["dominant_image_set".to_owned()],
            },
        ],
        package_inspection: None,
    }
}

/// Fixes a ready work's names, with or without a start file chosen by hand,
/// and returns the work as stored afterwards.
fn fix(start_file_chosen: bool) -> Installation {
    let directory = tempdir().expect("temporary directory");
    let work = directory.path().join("RJ000001");
    for (old, _, body) in FILES {
        let file = work.join(old);
        fs::create_dir_all(file.parent().expect("parent")).expect("folders");
        fs::write(file, body).expect("file");
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
            detection: detection(|index| FILES[index].0),
            overrides: InstallationOverrides {
                preferred_action: start_file_chosen.then(|| ManualLaunchSelection {
                    action: LaunchActionKind::LaunchExecutable,
                    target: LaunchTarget::RelativePath(path(FILES[0].0)),
                }),
                ..InstallationOverrides::default()
            },
            discovered_at: "2026-10-30T00:00:00Z".to_owned(),
            updated_at: "2026-10-30T00:00:00Z".to_owned(),
        })
        .expect("installation");
    let service = LibraryMaintenanceService::new(
        store.clone(),
        store.clone(),
        store.clone(),
        Arc::new(DesktopLibraryMaintenance::new()),
        Arc::new(DesktopPackageManifestReader::new()),
        Arc::new(DesktopPackageInstaller::new()),
    )
    .with_file_names(Arc::new(DesktopInstalledFileNames::new()), store.clone());

    assert_eq!(service.check_file_names(&id).expect("check").wrong, 4);
    let report = service
        .fix_file_names(&id, "2026-10-30T06:00:00Z".to_owned())
        .expect("fix");

    assert_eq!(report.fixed, 4);
    for (_, new, body) in FILES {
        assert_eq!(fs::read_to_string(work.join(new)).expect(new), body);
    }
    assert!(!work.join("ì∞òi").exists());
    assert_eq!(service.check_file_names(&id).expect("again").wrong, 0);
    store.read(&id).expect("read").expect("installation")
}

#[test]
fn gives_an_older_install_its_japanese_names_back_and_keeps_its_settings() {
    let installation = fix(true);

    assert_eq!(installation.status, InstallationStatus::Ready);
    assert_eq!(
        installation
            .overrides
            .preferred_action
            .map(|selection| selection.target),
        Some(LaunchTarget::RelativePath(path("作品/Game.exe")))
    );
    assert_eq!(installation.detection, detection(|index| FILES[index].1));
}

#[test]
fn a_work_its_install_set_up_stays_ready() {
    // Its start file came with the install, not from a review, so a new scan
    // of its game, pictures and save would have asked for one.
    let installation = fix(false);

    assert_eq!(installation.status, InstallationStatus::Ready);
    assert_eq!(installation.detection, detection(|index| FILES[index].1));
}
