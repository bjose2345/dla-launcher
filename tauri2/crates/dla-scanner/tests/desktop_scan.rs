use std::{
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use dla_application::{
    identity::ArchiveHash,
    identity::CatalogIdentityReader,
    scan_execution::ScanExecutionService,
    scanner::{
        ArchiveHashError, ArchiveHashRequest, ArchiveHasher, FilesystemScanner, PrepareScanRequest,
        ScanCancellation, ScanProgressSink, ScanRepository, ScanResultRequest, ScanSessionView,
        ScannerError,
    },
};
use dla_domain::{
    CatalogRom,
    scanner::{ScanMatchConfidence, ScanMatchOutcome, ScanOptions, ScanProgress, ScanStatus},
};
use dla_scanner::{
    ApprovedScanRoot, DesktopFilesystem, ScanAccessRegistry, SystemScanClock, SystemScanIdentifiers,
};
use dla_sqlite::{SqliteCatalogStore, SqliteLibraryStore};
use rusqlite::Connection;
use tempfile::tempdir;

struct NeverCancelled;

impl ScanCancellation for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Default)]
struct RecordingProgress {
    events: Mutex<Vec<ScanProgress>>,
}

impl ScanProgressSink for RecordingProgress {
    fn publish(&self, progress: &ScanProgress) -> Result<(), ScannerError> {
        self.events
            .lock()
            .map_err(|error| ScannerError::Persistence(error.to_string()))?
            .push(progress.clone());
        Ok(())
    }
}

/// Reads files as the desktop does and counts how many it was asked for.
struct CountingHasher {
    inner: Arc<DesktopFilesystem>,
    read: AtomicUsize,
}

impl ArchiveHasher for CountingHasher {
    fn hash(
        &self,
        request: &ArchiveHashRequest,
        cancellation: &dyn ScanCancellation,
        read: &(dyn Fn(u64) + Sync),
    ) -> Result<ArchiveHash, ArchiveHashError> {
        self.read.fetch_add(1, Ordering::Relaxed);
        self.inner.hash(request, cancellation, read)
    }
}

/// A scanner over `scan_root` whose catalog knows `RJ00000001` by an
/// archive of 12 bytes with the digest of `dla-launcher`.
struct Scanner {
    service: ScanExecutionService,
    hasher: Arc<CountingHasher>,
    progress: Arc<RecordingProgress>,
    approved: ApprovedScanRoot,
}

impl Scanner {
    fn new(directory: &Path, scan_root: &Path) -> Self {
        let mut fixture = dla_catalog::load_test_fixture().expect("catalog fixture");
        let archive = fixture
            .works
            .iter_mut()
            .find(|detail| detail.work.code == "DLA-SYNTH-0001")
            .expect("catalog fixture");
        archive.work.code = "RJ00000001".to_owned();
        archive.roms.push(CatalogRom {
            name: "synthetic-payload.zip".to_owned(),
            size: "12".to_owned(),
            crc: String::new(),
            md5: String::new(),
            sha1: String::new(),
            sha256: "af752b95d170411f60fd279016c06877879c6dd5d7f9f9152fe584ee8ea5f557".to_owned(),
            file_count: None,
            update_date: String::new(),
            version: String::new(),
        });
        let catalog = Arc::new(
            SqliteCatalogStore::open(&directory.join("catalog.sqlite"), &fixture)
                .expect("catalog store"),
        );
        let library = Arc::new(
            SqliteLibraryStore::open(&directory.join("library.sqlite")).expect("library store"),
        );
        let access = Arc::new(ScanAccessRegistry::new());
        let approved = access.approve(scan_root).expect("approved scan root");
        let desktop = Arc::new(DesktopFilesystem::new(access));
        let hasher = Arc::new(CountingHasher {
            inner: desktop.clone(),
            read: AtomicUsize::new(0),
        });
        let progress = Arc::new(RecordingProgress::default());
        let service = ScanExecutionService::new(
            desktop,
            hasher.clone(),
            catalog,
            library,
            progress.clone(),
            Arc::new(SystemScanClock),
            Arc::new(SystemScanIdentifiers),
        );
        Self {
            service,
            hasher,
            progress,
            approved,
        }
    }

    fn scan(&self) -> ScanSessionView {
        let prepared = self
            .service
            .prepare(PrepareScanRequest {
                platform: self.approved.platform.clone(),
                path_key: self.approved.path_key.clone(),
                display_path: self.approved.display_path.clone(),
                access_handle: self.approved.access_handle.clone(),
                options: ScanOptions::default(),
            })
            .expect("prepared scan");
        self.service
            .execute(prepared, &NeverCancelled)
            .expect("completed scan")
    }

    /// Whether the scan matched the archive by its digest.
    fn matched_by_digest(&self, scan: &ScanSessionView) -> bool {
        let results = self
            .service
            .browse_results(ScanResultRequest {
                session_id: scan.session.id.0.clone(),
                limit: 10,
                ..ScanResultRequest::default()
            })
            .expect("persisted results");
        results.items.iter().any(|item| {
            item.relative_path.as_deref() == Some("payload.zip")
                && item.result.confidence == Some(ScanMatchConfidence::Exact)
                && item
                    .result
                    .evidence
                    .iter()
                    .any(|evidence| evidence.reason_code == "archive_sha256_match")
        })
    }
}

#[test]
fn reads_a_file_again_only_when_it_has_changed_and_says_how_far_it_is() {
    let directory = tempdir().expect("temporary directory");
    let scan_root = directory.path().join("library");
    fs::create_dir_all(&scan_root).expect("scan root");
    let archive = scan_root.join("payload.zip");
    fs::write(&archive, b"dla-launcher").expect("archive file");
    fs::write(scan_root.join("note.txt"), b"a note").expect("loose file");
    let scanner = Scanner::new(directory.path(), &scan_root);

    let first = scanner.scan();
    assert!(scanner.matched_by_digest(&first));
    assert_eq!(scanner.hasher.read.load(Ordering::Relaxed), 1);
    // Counted before the first file: two files of 12 and 6 bytes, and all
    // of them checked at the end.
    let extents = scanner
        .progress
        .events
        .lock()
        .expect("progress lock")
        .iter()
        .filter(|event| event.session_id == first.session.id)
        .map(|event| event.extent)
        .collect::<Vec<_>>();
    assert_eq!(extents.first(), Some(&None));
    let counted = extents.iter().flatten().collect::<Vec<_>>();
    assert!(
        counted
            .iter()
            .all(|extent| extent.total_files == 2 && extent.total_bytes == 18)
    );
    assert_eq!(counted.first().map(|extent| extent.checked_bytes), Some(0));
    assert_eq!(counted.last().map(|extent| extent.checked_bytes), Some(18));
    assert!(
        counted
            .windows(2)
            .all(|pair| pair[0].checked_bytes <= pair[1].checked_bytes)
    );

    // Unchanged: matched by the same digest without the file being read.
    let second = scanner.scan();
    assert!(scanner.matched_by_digest(&second));
    assert_eq!(scanner.hasher.read.load(Ordering::Relaxed), 1);
    let third = scanner.scan();
    assert!(scanner.matched_by_digest(&third));
    assert_eq!(scanner.hasher.read.load(Ordering::Relaxed), 1);

    // Written again, at another time: read again.
    let file = fs::File::options()
        .write(true)
        .open(&archive)
        .expect("archive file");
    file.set_modified(SystemTime::now() + Duration::from_secs(90))
        .expect("another time of change");
    drop(file);
    let fourth = scanner.scan();
    assert!(scanner.matched_by_digest(&fourth));
    assert_eq!(scanner.hasher.read.load(Ordering::Relaxed), 2);

    // Another file of another size in its place is not known at all.
    fs::write(&archive, b"another one").expect("another archive");
    let fifth = scanner.scan();
    assert!(!scanner.matched_by_digest(&fifth));
    assert_eq!(scanner.hasher.read.load(Ordering::Relaxed), 2);
}

#[test]
fn traverses_matches_persists_and_restores_without_creating_an_installation() {
    let directory = tempdir().expect("temporary directory");
    let scan_root = directory.path().join("library");
    let known_work = scan_root.join("RJ00000001");
    fs::create_dir_all(&known_work).expect("known work directory");
    fs::write(known_work.join("game.exe"), b"game").expect("known work file");
    fs::write(scan_root.join("loose-note.txt"), b"unmatched evidence").expect("loose file");
    let incoming = scan_root.join("incoming");
    fs::create_dir_all(&incoming).expect("incoming directory");
    fs::write(incoming.join("payload.zip"), b"dla-launcher").expect("archive file");

    let mut fixture = dla_catalog::load_test_fixture().expect("catalog fixture");
    let archive = fixture
        .works
        .iter_mut()
        .find(|detail| detail.work.code == "DLA-SYNTH-0001")
        .expect("catalog fixture");
    archive.work.code = "RJ00000001".to_owned();
    archive.roms.push(CatalogRom {
        name: "synthetic-payload.zip".to_owned(),
        size: "12".to_owned(),
        crc: String::new(),
        md5: String::new(),
        sha1: String::new(),
        sha256: "af752b95d170411f60fd279016c06877879c6dd5d7f9f9152fe584ee8ea5f557".to_owned(),
        file_count: None,
        update_date: String::new(),
        version: String::new(),
    });
    let catalog = Arc::new(
        SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
            .expect("catalog store"),
    );
    let library_path = directory.path().join("library.sqlite");
    let library = Arc::new(SqliteLibraryStore::open(&library_path).expect("library store"));
    let access = Arc::new(ScanAccessRegistry::new());
    let approved = access.approve(&scan_root).expect("approved scan root");
    let desktop = Arc::new(DesktopFilesystem::new(access));
    let filesystem: Arc<dyn FilesystemScanner> = desktop.clone();
    let hasher: Arc<dyn ArchiveHasher> = desktop;
    let catalog_identity: Arc<dyn CatalogIdentityReader> = catalog;
    let repository: Arc<dyn ScanRepository> = library;
    let progress = Arc::new(RecordingProgress::default());
    let service = ScanExecutionService::new(
        filesystem,
        hasher,
        catalog_identity,
        repository,
        progress.clone(),
        Arc::new(SystemScanClock),
        Arc::new(SystemScanIdentifiers),
    );

    let prepared = service
        .prepare(PrepareScanRequest {
            platform: approved.platform,
            path_key: approved.path_key,
            display_path: approved.display_path,
            access_handle: approved.access_handle,
            options: ScanOptions::default(),
        })
        .expect("prepared scan");
    let session_id = prepared.session.id.0.clone();
    let completed = service
        .execute(prepared, &NeverCancelled)
        .expect("completed scan");

    assert_eq!(completed.session.status, ScanStatus::Completed);
    assert_eq!(completed.session.counters.discovered_directories, 2);
    assert_eq!(completed.session.counters.discovered_files, 3);
    assert_eq!(completed.session.counters.inspected_files, 3);
    assert_eq!(completed.session.counters.matched, 2);
    assert_eq!(completed.session.counters.unmatched, 1);

    let results = service
        .browse_results(ScanResultRequest {
            session_id,
            limit: 10,
            ..ScanResultRequest::default()
        })
        .expect("persisted results");
    assert_eq!(results.total, 3);
    let code_matched = results
        .items
        .iter()
        .find(|item| item.relative_path.as_deref() == Some("RJ00000001/game.exe"))
        .expect("code-matched result");
    assert_eq!(
        code_matched.result.selected_work_code.as_deref(),
        Some("RJ00000001")
    );
    assert!(
        code_matched
            .result
            .evidence
            .iter()
            .any(|item| item.reason_code == "code_in_directory_name")
    );
    let hash_matched = results
        .items
        .iter()
        .find(|item| item.relative_path.as_deref() == Some("incoming/payload.zip"))
        .expect("hash-matched result");
    assert_eq!(hash_matched.result.outcome, ScanMatchOutcome::Matched);
    assert_eq!(
        hash_matched.result.selected_work_code.as_deref(),
        Some("RJ00000001")
    );
    assert_eq!(
        hash_matched.result.confidence,
        Some(ScanMatchConfidence::Exact)
    );
    assert!(
        hash_matched
            .result
            .evidence
            .iter()
            .any(|item| item.reason_code == "archive_sha256_match")
    );

    let restored = service
        .read_latest()
        .expect("latest session")
        .expect("persisted session");
    assert_eq!(restored.session, completed.session);
    assert_eq!(restored.root, completed.root);
    let states = progress
        .events
        .lock()
        .expect("progress lock")
        .iter()
        .map(|event| event.status)
        .collect::<Vec<_>>();
    assert_eq!(states.first(), Some(&ScanStatus::Queued));
    assert!(states.contains(&ScanStatus::Running));
    assert_eq!(states.last(), Some(&ScanStatus::Completed));

    let connection = Connection::open(&library_path).expect("library database");
    let installations = connection
        .query_row("SELECT count(*) FROM library_installation", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("installation count");
    assert_eq!(installations, 0);
}
