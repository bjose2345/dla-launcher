//! Showing a Windows game enlarged through Magpie.
//!
//! Older games open a fixed small window and have no size setting. Magpie
//! (<https://github.com/Blinue/Magpie>, GPL-3.0) captures a window and draws
//! it enlarged, full screen or in a window of its own. It has no command
//! line for that: it reads profiles from its settings file when it starts,
//! and scales a profile's window by itself once that window is in front. So
//! the launcher waits for the game's window, writes a settings file with a
//! profile for exactly that window, and starts Magpie in the tray.
//!
//! The launcher downloads the release it was tested with from GitHub the
//! first time a work needs it, checks it against that release's SHA-256, and
//! keeps it in its own data folder, where Magpie runs in its portable mode
//! and never touches the settings of a Magpie the player installed.

use std::{fs, path::PathBuf};

use dla_application::launch::LaunchError;

use crate::release;

#[cfg(any(target_os = "windows", test))]
pub(crate) mod settings;
#[cfg(target_os = "windows")]
pub(crate) mod windows;

pub const VERSION: &str = "0.13.0";
const NAME: &str = "Magpie";
const URL: &str =
    "https://github.com/Blinue/Magpie/releases/download/v0.13.0/Magpie-v0.13.0-x64.zip";
const SHA256: &str = "a3f774dce9d2e050fb0934c362d51d8db459a2e79e6f937bdb86852312899a9e";
const MOST_BYTES: u64 = 32 * 1024 * 1024;

/// Magpie, kept under `directory/VERSION`.
#[derive(Clone, Debug)]
pub struct Magpie {
    directory: PathBuf,
}

impl Magpie {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn installed(&self) -> bool {
        self.program().is_file()
    }

    /// Downloads and unpacks it, unless it is there already.
    pub fn install(&self) -> Result<(), LaunchError> {
        if self.installed() {
            return Ok(());
        }
        self.install_from(&release::download(NAME, URL, MOST_BYTES)?)
    }

    pub(crate) fn folder(&self) -> PathBuf {
        self.directory.join(VERSION)
    }

    pub(crate) fn program(&self) -> PathBuf {
        self.folder().join("Magpie.exe")
    }

    /// A settings file beside the program is what puts Magpie in its
    /// portable mode.
    #[cfg(any(target_os = "windows", test))]
    pub(crate) fn settings_file(&self) -> PathBuf {
        self.folder().join("config").join("config.json")
    }

    fn install_from(&self, archive: &[u8]) -> Result<(), LaunchError> {
        release::verify(NAME, VERSION, archive, SHA256)?;
        self.unpack(archive)
    }

    fn unpack(&self, archive: &[u8]) -> Result<(), LaunchError> {
        let staging = self
            .directory
            .join(format!(".{VERSION}-{}", std::process::id()));
        let unpacked = release::unpack(archive, &staging, "").and_then(|()| {
            if self.folder().exists() {
                fs::remove_dir_all(self.folder())?;
            }
            fs::rename(&staging, self.folder())
        });
        if let Err(error) = unpacked {
            let _ = fs::remove_dir_all(&staging);
            return Err(LaunchError::adapter(format!(
                "Magpie could not be set up: {error}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn refuses_a_download_that_is_not_the_tested_release() {
        let directory = tempdir().expect("temporary directory");
        let magpie = Magpie::new(directory.path());
        let error = magpie.install_from(b"not it").expect_err("refused");
        assert!(error.to_string().contains("did not match"));
        assert!(!magpie.installed());
        assert!(!directory.path().join(VERSION).exists());
    }

    #[test]
    fn unpacks_the_release_into_its_version_folder() {
        let directory = tempdir().expect("temporary directory");
        let mut archive = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut archive));
            for (name, body) in [
                ("Magpie.exe", "program"),
                ("effects/Nearest.hlsl", "effect"),
            ] {
                zip.start_file(name, SimpleFileOptions::default())
                    .expect("file");
                zip.write_all(body.as_bytes()).expect("write");
            }
            zip.finish().expect("archive");
        }
        let magpie = Magpie::new(directory.path());

        magpie.unpack(&archive).expect("unpacked");

        assert!(magpie.installed());
        assert!(
            magpie
                .folder()
                .join("effects")
                .join("Nearest.hlsl")
                .is_file()
        );
        assert_eq!(
            magpie.settings_file(),
            directory
                .path()
                .join(VERSION)
                .join("config")
                .join("config.json")
        );
    }
}
