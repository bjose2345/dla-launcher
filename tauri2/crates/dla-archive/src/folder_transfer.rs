use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use fs2::available_space;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::installer::rename_directory_noreplace;

const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const MINIMUM_FREE_SPACE_MARGIN: u64 = 64 * 1024 * 1024;

type VolumeCheck = dyn Fn(&Path, &Path) -> io::Result<bool> + Send + Sync;

/// How a work reached the install folder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferMode {
    Rename,
    Copy,
}

#[derive(Debug)]
pub struct TransferOutcome {
    pub destination: PathBuf,
    pub mode: TransferMode,
    /// Set when a copy is in place but the original could not be removed.
    pub source_cleanup_error: Option<String>,
}

#[derive(Debug)]
pub enum FolderTransferError {
    Exists(String),
    SymbolicLink(PathBuf),
    InsufficientSpace { required: u64, available: u64 },
    Verification(String),
    Cancelled,
    Io(io::Error),
}

impl std::fmt::Display for FolderTransferError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exists(name) => write!(
                formatter,
                "the install folder already has a folder named {name}"
            ),
            Self::SymbolicLink(path) => write!(
                formatter,
                "the work holds a link ({}), so it is not moved",
                path.display()
            ),
            Self::InsufficientSpace {
                required,
                available,
            } => write!(
                formatter,
                "not enough free space: {required} bytes needed, {available} bytes free"
            ),
            Self::Verification(detail) => write!(formatter, "the copy did not match: {detail}"),
            Self::Cancelled => formatter.write_str("cancelled"),
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for FolderTransferError {}

impl From<io::Error> for FolderTransferError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Brings a work's folder, or a single file, into the install folder: a
/// no-replace rename on the same drive, else a copy into a hidden staging
/// folder that is checked, renamed into place, and only then removed from
/// where it was. A failure or cancel leaves the original untouched.
#[derive(Clone)]
pub struct FolderTransfer {
    same_volume: Arc<VolumeCheck>,
}

impl Default for FolderTransfer {
    fn default() -> Self {
        Self::new()
    }
}

impl FolderTransfer {
    pub fn new() -> Self {
        Self {
            same_volume: Arc::new(same_volume),
        }
    }

    /// Decides same or different drive with `check`, as tests need.
    pub fn with_volume_check(
        check: impl Fn(&Path, &Path) -> io::Result<bool> + Send + Sync + 'static,
    ) -> Self {
        Self {
            same_volume: Arc::new(check),
        }
    }

    pub fn mode(
        &self,
        source: &Path,
        destination_parent: &Path,
    ) -> Result<TransferMode, FolderTransferError> {
        Ok(if (self.same_volume)(source, destination_parent)? {
            TransferMode::Rename
        } else {
            TransferMode::Copy
        })
    }

    pub fn transfer(
        &self,
        source: &Path,
        destination_parent: &Path,
        destination_name: &str,
        cancelled: &dyn Fn() -> bool,
        progress: &dyn Fn(u64, u64),
    ) -> Result<TransferOutcome, FolderTransferError> {
        let destination = destination_parent.join(destination_name);
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(FolderTransferError::Exists(destination_name.to_owned()));
        }
        let metadata = fs::symlink_metadata(source)?;
        if metadata.file_type().is_symlink() {
            return Err(FolderTransferError::SymbolicLink(source.to_path_buf()));
        }
        let files = list_files(source)?;
        let total = files.iter().map(|file| file.bytes).sum::<u64>();
        if cancelled() {
            return Err(FolderTransferError::Cancelled);
        }
        if self.mode(source, destination_parent)? == TransferMode::Rename {
            match rename_into_place(source, &destination, metadata.is_dir()) {
                Ok(()) => {
                    progress(total, total);
                    return Ok(TransferOutcome {
                        destination,
                        mode: TransferMode::Rename,
                        source_cleanup_error: None,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    return Err(FolderTransferError::Exists(destination_name.to_owned()));
                }
                Err(error) if !crosses_devices(&error) => return Err(error.into()),
                Err(_) => {}
            }
        }
        validate_available_space(destination_parent, total)?;
        let staging = destination_parent.join(format!(".dla-stage-add-{}", Uuid::new_v4()));
        fs::create_dir(&staging)?;
        let copied = (|| {
            copy_files(
                source,
                &staging,
                metadata.is_dir(),
                &files,
                total,
                cancelled,
                progress,
            )?;
            verify_copy(&staging, &files)?;
            if cancelled() {
                return Err(FolderTransferError::Cancelled);
            }
            rename_directory_noreplace(&staging, &destination).map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists
                    || fs::symlink_metadata(&destination).is_ok()
                {
                    FolderTransferError::Exists(destination_name.to_owned())
                } else {
                    error.into()
                }
            })
        })();
        if let Err(error) = copied {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let removed = if metadata.is_dir() {
            fs::remove_dir_all(source)
        } else {
            fs::remove_file(source)
        };
        Ok(TransferOutcome {
            destination,
            mode: TransferMode::Copy,
            source_cleanup_error: removed.err().map(|error| error.to_string()),
        })
    }
}

#[derive(Debug)]
struct SourceFile {
    /// Below the source folder, or the file's own name for a single file.
    relative: PathBuf,
    bytes: u64,
}

/// Every file of the work; a link anywhere in it refuses the whole work, so
/// nothing outside the folder is ever followed or copied.
fn list_files(source: &Path) -> Result<Vec<SourceFile>, FolderTransferError> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.is_dir() {
        let name = source
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the file has no name"))?;
        return Ok(vec![SourceFile {
            relative: PathBuf::from(name),
            bytes: metadata.len(),
        }]);
    }
    let mut files = Vec::new();
    for entry in WalkDir::new(source).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(|error| {
            error
                .into_io_error()
                .unwrap_or_else(|| io::Error::other("the folder changed while it was read"))
        })?;
        if entry.file_type().is_symlink() {
            return Err(FolderTransferError::SymbolicLink(
                entry.path().to_path_buf(),
            ));
        }
        if entry.file_type().is_file() {
            let relative = entry
                .path()
                .strip_prefix(source)
                .map_err(io::Error::other)?
                .to_path_buf();
            files.push(SourceFile {
                relative,
                bytes: entry.metadata().map_err(io::Error::other)?.len(),
            });
        }
    }
    Ok(files)
}

fn rename_into_place(source: &Path, destination: &Path, is_directory: bool) -> io::Result<()> {
    if is_directory {
        return rename_directory_noreplace(source, destination);
    }
    fs::create_dir(destination)?;
    let name = source
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the file has no name"))?;
    fs::rename(source, destination.join(name)).inspect_err(|_| {
        let _ = fs::remove_dir(destination);
    })
}

fn copy_files(
    source: &Path,
    staging: &Path,
    is_directory: bool,
    files: &[SourceFile],
    total: u64,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(u64, u64),
) -> Result<(), FolderTransferError> {
    if is_directory {
        for entry in WalkDir::new(source).follow_links(false).min_depth(1) {
            let entry = entry.map_err(io::Error::other)?;
            if entry.file_type().is_dir() {
                let relative = entry
                    .path()
                    .strip_prefix(source)
                    .map_err(io::Error::other)?;
                fs::create_dir_all(staging.join(relative))?;
            }
        }
    }
    let mut done = 0u64;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    for file in files {
        let from = if is_directory {
            source.join(&file.relative)
        } else {
            source.to_path_buf()
        };
        let to = staging.join(&file.relative);
        let mut reader = File::open(&from)?;
        let mut writer = OpenOptions::new().write(true).create_new(true).open(&to)?;
        let mut written = 0u64;
        loop {
            if cancelled() {
                return Err(FolderTransferError::Cancelled);
            }
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            writer.write_all(&buffer[..read])?;
            written += read as u64;
            done += read as u64;
            progress(done, total);
        }
        writer.flush()?;
        writer.sync_all()?;
        if let Ok(modified) = fs::metadata(&from).and_then(|metadata| metadata.modified()) {
            writer.set_modified(modified)?;
        }
        if written != file.bytes {
            return Err(FolderTransferError::Verification(format!(
                "{} changed while it was copied",
                file.relative.display()
            )));
        }
    }
    Ok(())
}

fn verify_copy(staging: &Path, files: &[SourceFile]) -> Result<(), FolderTransferError> {
    let copied = list_files(staging)?;
    if copied.len() != files.len() {
        return Err(FolderTransferError::Verification(format!(
            "{} files expected, {} copied",
            files.len(),
            copied.len()
        )));
    }
    for (expected, copied) in files.iter().zip(&copied) {
        if expected.relative != copied.relative || expected.bytes != copied.bytes {
            return Err(FolderTransferError::Verification(format!(
                "{} differs after copying",
                expected.relative.display()
            )));
        }
    }
    Ok(())
}

fn validate_available_space(parent: &Path, bytes: u64) -> Result<(), FolderTransferError> {
    let margin = (bytes / 10).max(MINIMUM_FREE_SPACE_MARGIN);
    let required = bytes.saturating_add(margin);
    let available = available_space(parent)?;
    if available < required {
        return Err(FolderTransferError::InsufficientSpace {
            required,
            available,
        });
    }
    Ok(())
}

fn crosses_devices(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::CrossesDevices
}

#[cfg(unix)]
fn same_volume(source: &Path, destination_parent: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    Ok(fs::symlink_metadata(source)?.dev() == fs::metadata(destination_parent)?.dev())
}

#[cfg(windows)]
fn same_volume(source: &Path, destination_parent: &Path) -> io::Result<bool> {
    use std::path::Component;

    let volume = |path: &Path| -> io::Result<String> {
        let canonical = fs::canonicalize(path)?;
        Ok(match canonical.components().next() {
            Some(Component::Prefix(prefix)) => prefix.as_os_str().to_string_lossy().to_uppercase(),
            _ => String::new(),
        })
    };
    Ok(volume(source)? == volume(destination_parent)?)
}

#[cfg(not(any(unix, windows)))]
fn same_volume(_source: &Path, _destination_parent: &Path) -> io::Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, time::SystemTime};

    use tempfile::tempdir;

    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().expect("parent")).expect("parent folder");
        fs::write(path, bytes).expect("write file");
    }

    fn work(root: &Path) -> PathBuf {
        let work = root.join("source/My Work");
        write(&work.join("game.exe"), b"exe");
        write(&work.join("data/system.json"), b"{}");
        write(&work.join("data/img/01.png"), &[7u8; 4096]);
        work
    }

    fn no() -> bool {
        false
    }

    #[test]
    fn a_work_on_the_same_drive_is_renamed_into_place() {
        let root = tempdir().expect("temporary folder");
        let source = work(root.path());
        let library = root.path().join("library");
        fs::create_dir(&library).expect("library");

        let outcome = FolderTransfer::with_volume_check(|_, _| Ok(true))
            .transfer(&source, &library, "RJ01000001", &no, &|_, _| {})
            .expect("move");

        assert_eq!(outcome.mode, TransferMode::Rename);
        assert!(!source.exists());
        assert_eq!(
            fs::read(library.join("RJ01000001/data/system.json")).expect("moved file"),
            b"{}"
        );
    }

    #[test]
    fn a_work_on_another_drive_is_copied_checked_and_then_removed_from_there() {
        let root = tempdir().expect("temporary folder");
        let source = work(root.path());
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);
        File::options()
            .write(true)
            .open(source.join("game.exe"))
            .and_then(|file| file.set_modified(old))
            .expect("old time");
        let library = root.path().join("library");
        fs::create_dir(&library).expect("library");
        let reported = Cell::new((0, 0));

        let outcome = FolderTransfer::with_volume_check(|_, _| Ok(false))
            .transfer(&source, &library, "RJ01000001", &no, &|done, total| {
                reported.set((done, total))
            })
            .expect("copy");

        assert_eq!(outcome.mode, TransferMode::Copy);
        assert!(outcome.source_cleanup_error.is_none());
        assert!(!source.exists());
        let copied = library.join("RJ01000001");
        assert_eq!(
            fs::read(copied.join("data/img/01.png"))
                .expect("copy")
                .len(),
            4096
        );
        assert_eq!(
            fs::metadata(copied.join("game.exe"))
                .and_then(|metadata| metadata.modified())
                .expect("time"),
            old
        );
        assert_eq!(reported.get(), (4101, 4101));
        assert!(
            fs::read_dir(&library)
                .expect("library listing")
                .all(|entry| !entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".dla-stage"))
        );
    }

    #[test]
    fn a_single_file_arrives_in_a_folder_of_its_own() {
        let root = tempdir().expect("temporary folder");
        let file = root.path().join("source/movie.mp4");
        write(&file, b"video");
        let library = root.path().join("library");
        fs::create_dir(&library).expect("library");

        for (index, same) in [true, false].into_iter().enumerate() {
            let source = root.path().join(format!("source/{index}.mp4"));
            fs::copy(&file, &source).expect("copy fixture");
            let name = format!("movie {index}");
            FolderTransfer::with_volume_check(move |_, _| Ok(same))
                .transfer(&source, &library, &name, &no, &|_, _| {})
                .expect("file transfer");
            assert_eq!(
                fs::read(library.join(&name).join(format!("{index}.mp4"))).expect("moved"),
                b"video"
            );
            assert!(!source.exists());
        }
    }

    #[test]
    fn a_taken_name_is_refused_without_touching_either_folder() {
        let root = tempdir().expect("temporary folder");
        let source = work(root.path());
        let library = root.path().join("library");
        write(&library.join("RJ01000001/keep.txt"), b"keep");

        for same in [true, false] {
            let error = FolderTransfer::with_volume_check(move |_, _| Ok(same))
                .transfer(&source, &library, "RJ01000001", &no, &|_, _| {})
                .expect_err("taken");
            assert!(matches!(error, FolderTransferError::Exists(_)));
            assert_eq!(
                error.to_string(),
                "the install folder already has a folder named RJ01000001"
            );
            assert!(source.join("game.exe").exists());
            assert!(library.join("RJ01000001/keep.txt").exists());
        }
    }

    #[test]
    fn a_cancelled_copy_removes_its_staging_and_keeps_the_original() {
        let root = tempdir().expect("temporary folder");
        let source = work(root.path());
        let library = root.path().join("library");
        fs::create_dir(&library).expect("library");
        let calls = Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() > 3
        };

        let error = FolderTransfer::with_volume_check(|_, _| Ok(false))
            .transfer(&source, &library, "RJ01000001", &cancelled, &|_, _| {})
            .expect_err("cancelled");

        assert!(matches!(error, FolderTransferError::Cancelled));
        assert!(source.join("data/img/01.png").exists());
        assert_eq!(fs::read_dir(&library).expect("library").count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_work_holding_a_link_is_refused_before_anything_moves() {
        let root = tempdir().expect("temporary folder");
        let source = work(root.path());
        std::os::unix::fs::symlink("/etc", source.join("data/outside")).expect("link");
        let library = root.path().join("library");
        fs::create_dir(&library).expect("library");

        for same in [true, false] {
            let error = FolderTransfer::with_volume_check(move |_, _| Ok(same))
                .transfer(&source, &library, "RJ01000001", &no, &|_, _| {})
                .expect_err("link");
            assert!(matches!(error, FolderTransferError::SymbolicLink(_)));
            assert!(source.join("game.exe").exists());
            assert_eq!(fs::read_dir(&library).expect("library").count(), 0);
        }
    }

    #[test]
    fn the_real_check_sees_one_drive_for_folders_side_by_side() {
        let root = tempdir().expect("temporary folder");
        let source = work(root.path());
        let library = root.path().join("library");
        fs::create_dir(&library).expect("library");
        assert_eq!(
            FolderTransfer::new().mode(&source, &library).expect("mode"),
            TransferMode::Rename
        );
    }
}
