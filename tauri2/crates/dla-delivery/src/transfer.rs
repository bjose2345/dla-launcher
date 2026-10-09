use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use dla_application::delivery::{DeliveryControl, DeliveryError, DeliveryTransfer, STAGING_FOLDER};
use dla_domain::delivery::{DeliveryArchive, DeliveryPhase, DeliveryProblem};
use fs2::available_space;
use sha2::{Digest, Sha256};

use crate::{DeliveryClient, local_name};

/// Appended to the archive's SHA-256 while it is incomplete.
pub const PART_SUFFIX: &str = ".dla-part";
const RANGE_BYTES: u64 = 32 << 20;
const BUFFER_BYTES: usize = 1 << 20;
const REPORT_EVERY: u64 = 8 << 20;
// Room left on the volume for the extraction that follows.
const SPACE_MARGIN: u64 = 64 << 20;

/// Downloads archives in ranges into the staging folder, hashing as it
/// writes, so an interrupted file continues from its last byte and a
/// finished file is kept only if its SHA-256 matches.
pub struct FileTransfer {
    client: Arc<DeliveryClient>,
    range_bytes: u64,
}

impl FileTransfer {
    pub fn new(client: Arc<DeliveryClient>) -> Self {
        Self {
            client,
            range_bytes: RANGE_BYTES,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_range_bytes(client: Arc<DeliveryClient>, range_bytes: u64) -> Self {
        Self {
            client,
            range_bytes,
        }
    }
}

fn disk(error: io::Error) -> DeliveryError {
    if error.kind() == io::ErrorKind::StorageFull {
        DeliveryError::problem(DeliveryProblem::DiskSpace, error.to_string())
    } else {
        DeliveryError::problem(DeliveryProblem::InstallFolder, error.to_string())
    }
}

/// Feeds an existing file to the hash, reporting the bytes read.
fn hash_file(
    path: &Path,
    hasher: &mut Sha256,
    control: &dyn DeliveryControl,
    progress: &mut dyn FnMut(DeliveryPhase, u64),
) -> Result<u64, DeliveryError> {
    let mut file = File::open(path).map_err(disk)?;
    let mut buffer = vec![0; BUFFER_BYTES];
    let mut read: u64 = 0;
    let mut reported: u64 = 0;
    loop {
        let count = file.read(&mut buffer).map_err(disk)?;
        if count == 0 {
            return Ok(read);
        }
        hasher.update(&buffer[..count]);
        read += count as u64;
        if read - reported >= REPORT_EVERY {
            control.check()?;
            progress(DeliveryPhase::Checking, read);
            reported = read;
        }
    }
}

fn matches(hasher: Sha256, expected: &str) -> bool {
    hex::encode(hasher.finalize()) == expected
}

impl DeliveryTransfer for FileTransfer {
    fn fetch(
        &self,
        archive: &DeliveryArchive,
        folder: &str,
        control: &dyn DeliveryControl,
        progress: &mut dyn FnMut(DeliveryPhase, u64),
    ) -> Result<(), DeliveryError> {
        let remote = archive.remote_path.as_deref().ok_or_else(|| {
            DeliveryError::problem(
                DeliveryProblem::NotInArchive,
                "the archive has not been located",
            )
        })?;
        let name = local_name(&archive.file_name);
        if name != archive.file_name {
            return Err(DeliveryError::problem(
                DeliveryProblem::NotInArchive,
                format!(
                    "{} is not a safe file name on this computer",
                    archive.file_name
                ),
            ));
        }
        fs::create_dir_all(folder).map_err(disk)?;
        let folder = Path::new(folder);
        let target = folder.join(&name);
        let part = folder.join(format!("{}{PART_SUFFIX}", archive.sha256));

        // Finished before the job could record it, for example in a crash.
        if fs::metadata(&target).is_ok_and(|meta| meta.len() == archive.size) {
            let mut hasher = Sha256::new();
            hash_file(&target, &mut hasher, control, progress)?;
            if matches(hasher, &archive.sha256) {
                progress(DeliveryPhase::Verifying, archive.size);
                return Ok(());
            }
            fs::remove_file(&target).map_err(disk)?;
        }

        let mut hasher = Sha256::new();
        let mut held = match fs::metadata(&part) {
            Ok(meta) if meta.len() <= archive.size => meta.len(),
            Ok(_) => {
                fs::remove_file(&part).map_err(disk)?;
                0
            }
            Err(_) => 0,
        };
        if held > 0 {
            held = hash_file(&part, &mut hasher, control, progress)?;
        }
        let needed = archive.size - held + SPACE_MARGIN;
        if available_space(folder).is_ok_and(|free| free < needed) {
            return Err(DeliveryError::problem(
                DeliveryProblem::DiskSpace,
                format!("{} more bytes are needed in the install folder", needed),
            ));
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part)
            .map_err(disk)?;
        let mut last_modified: Option<String> = None;
        let mut buffer = vec![0; BUFFER_BYTES];
        progress(DeliveryPhase::Receiving, held);
        while held < archive.size {
            control.check()?;
            let length = self.range_bytes.min(archive.size - held);
            let reply = self
                .client
                .range(remote, held, length, last_modified.as_deref())?;
            if reply.total != archive.size {
                fs::remove_file(&part).ok();
                return Err(DeliveryError::problem(
                    DeliveryProblem::Corrupt,
                    format!("the server's copy of {name} has a different size"),
                ));
            }
            last_modified = last_modified.or(reply.last_modified);
            let mut reader = reply.body.into_reader();
            let mut received: u64 = 0;
            while received < length {
                let count = match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(error) => {
                        // Keep what arrived; the next attempt continues here.
                        file.flush().map_err(disk)?;
                        return Err(DeliveryError::problem(
                            DeliveryProblem::Unreachable,
                            error.to_string(),
                        ));
                    }
                };
                let count = count.min((length - received) as usize);
                file.write_all(&buffer[..count]).map_err(disk)?;
                hasher.update(&buffer[..count]);
                received += count as u64;
                held += count as u64;
                progress(DeliveryPhase::Receiving, held);
            }
            if received < length {
                file.flush().map_err(disk)?;
                return Err(DeliveryError::problem(
                    DeliveryProblem::Unreachable,
                    "the connection closed during the download",
                ));
            }
        }
        file.sync_all().map_err(disk)?;
        drop(file);
        progress(DeliveryPhase::Verifying, held);
        if !matches(hasher, &archive.sha256) {
            fs::remove_file(&part).map_err(disk)?;
            return Err(DeliveryError::problem(
                DeliveryProblem::Corrupt,
                format!("{name} did not match its SHA-256 and was deleted"),
            ));
        }
        fs::rename(&part, &target).map_err(disk)?;
        Ok(())
    }

    fn discard(&self, folder: &str) -> Result<(), DeliveryError> {
        let folder = PathBuf::from(folder);
        // Only a work's folder directly inside the staging folder.
        let inside_staging = folder
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == STAGING_FOLDER);
        if !inside_staging {
            return Err(DeliveryError::problem(
                DeliveryProblem::InstallFolder,
                "refusing to delete a folder outside the downloads staging folder",
            ));
        }
        match fs::remove_dir_all(&folder) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(disk(error)),
        }
        if let Some(staging) = folder.parent() {
            // Leave no empty staging folder behind.
            fs::remove_dir(staging).ok();
        }
        Ok(())
    }
}
