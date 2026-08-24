use std::{
    fmt::Write as _,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use dla_application::{
    identity::{ArchiveHash, ArchiveHashAlgorithm},
    scanner::{
        ArchiveHashError, ArchiveHashRequest, ArchiveHasher, FilesystemScanObserver,
        FilesystemScanRequest, FilesystemScanner, ScanCancellation, ScanSourceIssue, ScannerError,
    },
};
use dla_domain::scanner::{DiscoveredEntry, ScanCounters, ScanEntryKind, ScanIssueCode};
use md5::Md5;
use rayon::iter::{ParallelBridge, ParallelIterator};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use walkdir::{DirEntry, Error as WalkError, WalkDir};

use crate::ScanAccessRegistry;

pub struct DesktopFilesystem {
    access: Arc<ScanAccessRegistry>,
}

impl DesktopFilesystem {
    pub fn new(access: Arc<ScanAccessRegistry>) -> Self {
        Self { access }
    }
}

impl FilesystemScanner for DesktopFilesystem {
    fn scan(
        &self,
        request: &FilesystemScanRequest,
        observer: &dyn FilesystemScanObserver,
        cancellation: &dyn ScanCancellation,
    ) -> Result<ScanCounters, ScannerError> {
        let root = self.access.resolve(&request.access_handle)?;
        if !root.is_dir() {
            return Err(ScannerError::RootUnavailable(
                "the approved scan root is no longer available".to_owned(),
            ));
        }
        let counters = SourceCounters::default();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(usize::from(request.options.worker_limit))
            .thread_name(|index| format!("dla-scan-{index}"))
            .build()
            .map_err(ScannerError::filesystem)?;
        pool.install(|| {
            WalkDir::new(&root)
                .follow_links(request.options.follow_symlinks)
                .into_iter()
                .par_bridge()
                .try_for_each(|walked| {
                    if cancellation.is_cancelled() {
                        return Err(ScannerError::Cancelled);
                    }
                    match walked {
                        Ok(entry) if entry.depth() == 0 => Ok(()),
                        Ok(entry) => observe_entry(&root, entry, observer, &counters, cancellation),
                        Err(error) => observe_walk_error(&root, error, observer, &counters),
                    }
                })
        })?;
        Ok(counters.snapshot())
    }
}

impl ArchiveHasher for DesktopFilesystem {
    fn hash(
        &self,
        request: &ArchiveHashRequest,
        cancellation: &dyn ScanCancellation,
    ) -> Result<ArchiveHash, ArchiveHashError> {
        let root = self
            .access
            .resolve(&request.access_handle)
            .map_err(|error| {
                ArchiveHashError::Source(ScanSourceIssue {
                    relative_path: Some(request.relative_path.clone()),
                    code: ScanIssueCode::RootUnavailable,
                    message: error.to_string(),
                    recoverable: false,
                })
            })?;
        let path = resolve_child_path(&root, &request.relative_path).map_err(|message| {
            ArchiveHashError::Source(ScanSourceIssue {
                relative_path: Some(request.relative_path.clone()),
                code: ScanIssueCode::UnsupportedEntry,
                message,
                recoverable: true,
            })
        })?;
        let file = File::open(&path).map_err(|error| {
            ArchiveHashError::Source(io_issue(&request.relative_path, &error, true))
        })?;
        let mut reader = BufReader::with_capacity(1024 * 1024, file);
        let digest = match request.algorithm {
            ArchiveHashAlgorithm::Md5 => digest_reader::<Md5>(&mut reader, cancellation),
            ArchiveHashAlgorithm::Sha1 => digest_reader::<Sha1>(&mut reader, cancellation),
            ArchiveHashAlgorithm::Sha256 => digest_reader::<Sha256>(&mut reader, cancellation),
        }
        .map_err(|error| match error {
            ArchiveHashError::Source(mut source) => {
                source.relative_path = Some(request.relative_path.clone());
                ArchiveHashError::Source(source)
            }
            ArchiveHashError::Cancelled => ArchiveHashError::Cancelled,
        })?;
        Ok(ArchiveHash {
            algorithm: request.algorithm,
            digest,
        })
    }
}

#[derive(Default)]
struct SourceCounters {
    files: AtomicU64,
    directories: AtomicU64,
    issues: AtomicU64,
}

impl SourceCounters {
    fn snapshot(&self) -> ScanCounters {
        ScanCounters {
            discovered_files: self.files.load(Ordering::Relaxed),
            discovered_directories: self.directories.load(Ordering::Relaxed),
            recoverable_errors: self.issues.load(Ordering::Relaxed),
            ..ScanCounters::default()
        }
    }
}

fn observe_entry(
    root: &Path,
    entry: DirEntry,
    observer: &dyn FilesystemScanObserver,
    counters: &SourceCounters,
    cancellation: &dyn ScanCancellation,
) -> Result<(), ScannerError> {
    if cancellation.is_cancelled() {
        return Err(ScannerError::Cancelled);
    }
    let relative_path = relative_display_path(root, entry.path());
    let file_type = entry.file_type();
    if file_type.is_symlink() {
        counters.issues.fetch_add(1, Ordering::Relaxed);
        return observer.issue(ScanSourceIssue {
            relative_path: Some(relative_path),
            code: ScanIssueCode::UnsupportedEntry,
            message: "symbolic links are excluded by the active scan policy".to_owned(),
            recoverable: true,
        });
    }
    let kind = if file_type.is_file() {
        ScanEntryKind::File
    } else if file_type.is_dir() {
        ScanEntryKind::Directory
    } else {
        counters.issues.fetch_add(1, Ordering::Relaxed);
        return observer.issue(ScanSourceIssue {
            relative_path: Some(relative_path),
            code: ScanIssueCode::UnsupportedEntry,
            message: "the filesystem entry is neither a regular file nor a directory".to_owned(),
            recoverable: true,
        });
    };
    let metadata = match entry.metadata() {
        Ok(metadata) => metadata,
        Err(error) => {
            counters.issues.fetch_add(1, Ordering::Relaxed);
            return observer.issue(walk_issue(root, &error, true));
        }
    };
    let extension = if kind == ScanEntryKind::File {
        entry
            .path()
            .extension()
            .map(|value| value.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let size = (kind == ScanEntryKind::File).then(|| metadata.len().to_string());
    let modified_at = metadata
        .modified()
        .ok()
        .and_then(|value| OffsetDateTime::from(value).format(&Rfc3339).ok());
    observer.discovered(DiscoveredEntry {
        path_key: normalize_relative_key(&relative_path),
        relative_path,
        kind,
        extension,
        size,
        modified_at,
    })?;
    match kind {
        ScanEntryKind::File => counters.files.fetch_add(1, Ordering::Relaxed),
        ScanEntryKind::Directory => counters.directories.fetch_add(1, Ordering::Relaxed),
    };
    Ok(())
}

fn observe_walk_error(
    root: &Path,
    error: WalkError,
    observer: &dyn FilesystemScanObserver,
    counters: &SourceCounters,
) -> Result<(), ScannerError> {
    if error.depth() == 0 {
        return Err(ScannerError::RootUnavailable(error.to_string()));
    }
    counters.issues.fetch_add(1, Ordering::Relaxed);
    observer.issue(walk_issue(root, &error, true))
}

fn walk_issue(root: &Path, error: &WalkError, recoverable: bool) -> ScanSourceIssue {
    let code = error
        .io_error()
        .map(io_issue_code)
        .unwrap_or(ScanIssueCode::Io);
    ScanSourceIssue {
        relative_path: error.path().map(|path| relative_display_path(root, path)),
        code,
        message: error.to_string(),
        recoverable,
    }
}

fn io_issue(relative_path: &str, error: &std::io::Error, recoverable: bool) -> ScanSourceIssue {
    ScanSourceIssue {
        relative_path: Some(relative_path.to_owned()),
        code: io_issue_code(error),
        message: error.to_string(),
        recoverable,
    }
}

fn io_issue_code(error: &std::io::Error) -> ScanIssueCode {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => ScanIssueCode::PermissionDenied,
        std::io::ErrorKind::NotFound => ScanIssueCode::EntryVanished,
        _ => ScanIssueCode::Io,
    }
}

fn relative_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn normalize_relative_key(relative_path: &str) -> String {
    if cfg!(windows) {
        relative_path.to_lowercase()
    } else {
        relative_path.to_owned()
    }
}

fn resolve_child_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative_path);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("the scan entry path is outside the approved root".to_owned());
    }
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("symbolic-link files cannot be hashed".to_owned());
    }
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.starts_with(root) {
        return Err("the scan entry resolved outside the approved root".to_owned());
    }
    Ok(canonical)
}

fn digest_reader<D: Digest + Default>(
    reader: &mut BufReader<File>,
    cancellation: &dyn ScanCancellation,
) -> Result<String, ArchiveHashError> {
    let mut digest = D::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        if cancellation.is_cancelled() {
            return Err(ArchiveHashError::Cancelled);
        }
        let read = reader.read(&mut buffer).map_err(|error| {
            ArchiveHashError::Source(ScanSourceIssue {
                relative_path: None,
                code: io_issue_code(&error),
                message: error.to_string(),
                recoverable: true,
            })
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let bytes = digest.finalize();
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("hex formatting");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_application::scanner::{FilesystemScanObserver, ScanCancellation};
    use dla_domain::scanner::{ScanHashPolicy, ScanOptions, ScanRootId, ScanSessionId};
    use tempfile::tempdir;

    use super::*;

    struct NeverCancelled;

    impl ScanCancellation for NeverCancelled {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    struct AlwaysCancelled;

    impl ScanCancellation for AlwaysCancelled {
        fn is_cancelled(&self) -> bool {
            true
        }
    }

    #[derive(Default)]
    struct RecordingObserver {
        entries: Mutex<Vec<DiscoveredEntry>>,
        issues: Mutex<Vec<ScanSourceIssue>>,
    }

    impl FilesystemScanObserver for RecordingObserver {
        fn discovered(&self, entry: DiscoveredEntry) -> Result<(), ScannerError> {
            self.entries.lock().expect("entries lock").push(entry);
            Ok(())
        }

        fn issue(&self, issue: ScanSourceIssue) -> Result<(), ScannerError> {
            self.issues.lock().expect("issues lock").push(issue);
            Ok(())
        }
    }

    #[test]
    fn walks_nested_entries_with_a_bounded_pool() {
        let directory = tempdir().expect("temporary directory");
        fs::create_dir(directory.path().join("RJ01326398")).expect("work directory");
        fs::write(directory.path().join("RJ01326398/game.exe"), b"game").expect("game file");
        let access = Arc::new(ScanAccessRegistry::new());
        let approved = access.approve(directory.path()).expect("approved root");
        let scanner = DesktopFilesystem::new(access);
        let observer = RecordingObserver::default();
        let counters = scanner
            .scan(
                &request(&approved.access_handle),
                &observer,
                &NeverCancelled,
            )
            .expect("filesystem scan");

        let mut paths = observer
            .entries
            .lock()
            .expect("entries lock")
            .iter()
            .map(|entry| entry.relative_path.clone())
            .collect::<Vec<_>>();
        paths.sort();
        assert_eq!(paths, vec!["RJ01326398", "RJ01326398/game.exe"]);
        assert_eq!(counters.discovered_directories, 1);
        assert_eq!(counters.discovered_files, 1);
    }

    #[test]
    fn hashes_an_approved_file() {
        let directory = tempdir().expect("temporary directory");
        fs::write(directory.path().join("archive.zip"), b"dla-launcher").expect("archive file");
        let access = Arc::new(ScanAccessRegistry::new());
        let approved = access.approve(directory.path()).expect("approved root");
        let scanner = DesktopFilesystem::new(access);
        let hash = scanner
            .hash(
                &ArchiveHashRequest {
                    access_handle: approved.access_handle,
                    relative_path: "archive.zip".to_owned(),
                    algorithm: ArchiveHashAlgorithm::Sha256,
                },
                &NeverCancelled,
            )
            .expect("archive hash");

        assert_eq!(
            hash.digest,
            "af752b95d170411f60fd279016c06877879c6dd5d7f9f9152fe584ee8ea5f557"
        );
    }

    #[test]
    fn stops_before_observing_entries_when_cancelled() {
        let directory = tempdir().expect("temporary directory");
        fs::write(directory.path().join("work.zip"), b"archive").expect("work file");
        let access = Arc::new(ScanAccessRegistry::new());
        let approved = access.approve(directory.path()).expect("approved root");
        let scanner = DesktopFilesystem::new(access);
        let observer = RecordingObserver::default();

        let error = scanner
            .scan(
                &request(&approved.access_handle),
                &observer,
                &AlwaysCancelled,
            )
            .expect_err("cancelled scan");

        assert!(matches!(error, ScannerError::Cancelled));
        assert!(observer.entries.lock().expect("entries lock").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn reports_and_does_not_follow_symbolic_links() {
        use std::os::unix::fs::symlink;

        let directory = tempdir().expect("temporary directory");
        let outside = tempdir().expect("outside directory");
        fs::write(outside.path().join("outside.txt"), b"outside").expect("outside file");
        symlink(outside.path(), directory.path().join("linked-library")).expect("symbolic link");
        let access = Arc::new(ScanAccessRegistry::new());
        let approved = access.approve(directory.path()).expect("approved root");
        let scanner = DesktopFilesystem::new(access);
        let observer = RecordingObserver::default();

        let counters = scanner
            .scan(
                &request(&approved.access_handle),
                &observer,
                &NeverCancelled,
            )
            .expect("filesystem scan");

        assert!(observer.entries.lock().expect("entries lock").is_empty());
        let issues = observer.issues.lock().expect("issues lock");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].relative_path.as_deref(), Some("linked-library"));
        assert_eq!(issues[0].code, ScanIssueCode::UnsupportedEntry);
        assert!(issues[0].recoverable);
        assert_eq!(counters.recoverable_errors, 1);
    }

    #[test]
    fn refuses_to_hash_paths_outside_the_approved_root() {
        let directory = tempdir().expect("temporary directory");
        let access = Arc::new(ScanAccessRegistry::new());
        let approved = access.approve(directory.path()).expect("approved root");
        let scanner = DesktopFilesystem::new(access);

        let error = scanner
            .hash(
                &ArchiveHashRequest {
                    access_handle: approved.access_handle,
                    relative_path: "../outside.zip".to_owned(),
                    algorithm: ArchiveHashAlgorithm::Sha256,
                },
                &NeverCancelled,
            )
            .expect_err("outside path rejected");

        assert!(matches!(
            error,
            ArchiveHashError::Source(ScanSourceIssue {
                code: ScanIssueCode::UnsupportedEntry,
                recoverable: true,
                ..
            })
        ));
    }

    fn request(access_handle: &str) -> FilesystemScanRequest {
        FilesystemScanRequest {
            session_id: ScanSessionId("session-test".to_owned()),
            root_id: ScanRootId("root-test".to_owned()),
            access_handle: access_handle.to_owned(),
            options: ScanOptions {
                follow_symlinks: false,
                hash_policy: ScanHashPolicy::CandidateArchives,
                worker_limit: 2,
            },
        }
    }
}
