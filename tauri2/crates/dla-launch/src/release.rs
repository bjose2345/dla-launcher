//! A tested release of a helper program, fetched from its GitHub release
//! the first time a work needs it and kept in the launcher's data folder.

use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
    time::Duration,
};

use dla_application::launch::LaunchError;
use sha2::{Digest, Sha256};

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) fn download(name: &str, url: &str, most_bytes: u64) -> Result<Vec<u8>, LaunchError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .into();
    let failed = |error: &dyn std::fmt::Display| {
        LaunchError::adapter(format!("{name} could not be downloaded: {error}"))
    };
    let mut response = agent.get(url).call().map_err(|error| failed(&error))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(most_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| failed(&error))?;
    if bytes.len() as u64 > most_bytes {
        return Err(failed(&"the download is larger than expected"));
    }
    Ok(bytes)
}

/// Refuses any file that is not the release the launcher was tested with.
pub(crate) fn verify(
    name: &str,
    version: &str,
    archive: &[u8],
    sha256: &str,
) -> Result<(), LaunchError> {
    let digest = hex::encode(Sha256::digest(archive));
    if digest == sha256 {
        Ok(())
    } else {
        Err(LaunchError::adapter(format!(
            "{name} {version} did not match the tested download (SHA-256 {digest})"
        )))
    }
}

/// The archive's files under `top_folder` (all of them when it is empty)
/// into `target`.
pub(crate) fn unpack(archive: &[u8], target: &Path, top_folder: &str) -> std::io::Result<()> {
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    fs::create_dir_all(target)?;
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(std::io::Error::other)?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(std::io::Error::other)?;
        let Some(name) = entry.enclosed_name() else {
            return Err(std::io::Error::other("an unsafe path in the archive"));
        };
        let Ok(name) = name.strip_prefix(top_folder) else {
            continue;
        };
        if entry.is_dir() || name.as_os_str().is_empty() {
            continue;
        }
        let path = target.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        fs::write(path, bytes)?;
    }
    Ok(())
}
