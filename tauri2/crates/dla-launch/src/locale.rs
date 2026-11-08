//! Starting a Windows game as if Windows were Japanese.
//!
//! Older Japanese games read and write text in the Japanese code page, so on
//! Windows set to another language they show garbled text or do not start.
//! Locale Remulator (<https://github.com/InWILL/Locale_Remulator>, LGPL-3.0)
//! starts a program with the Japanese code page, locale and time zone, for
//! 32-bit and 64-bit programs alike. The launcher downloads the release it
//! was tested with from GitHub the first time a work needs it, checks it
//! against that release's SHA-256, and keeps it in its own data folder.
//! Wine needs none of this: the launcher already runs it in Japanese.

use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use dla_application::launch::LaunchError;
use sha2::{Digest, Sha256};

pub const VERSION: &str = "1.6.0";
const URL: &str = "https://github.com/InWILL/Locale_Remulator/releases/download/v1.6.0/Locale_Remulator.1.6.0.zip";
const SHA256: &str = "f7f7b4ed8ef5bf1b25d46eb6cbf10d0a01dddfe5030ae09f8a1e73c5ad3c1b02";
const ARCHIVE_FOLDER: &str = "Locale_Remulator.1.6.0/";
const MOST_BYTES: u64 = 4 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// The launcher's profile in Locale Remulator's settings: Japanese, as its
/// own "Run in Japanese" profile. LRProc takes it by this ID.
pub const PROFILE: &str = "d1a0c0de-5a4e-4c1a-9e0a-4a5000000932";

// LRProc fails before starting anything without the Type of `Profiles`
// (the file type its Explorer menu is for).
const CONFIG: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<LRConfig>
  <Profiles Type="exe">
    <Profile Name="DLA Launcher (Japanese)" Guid="d1a0c0de-5a4e-4c1a-9e0a-4a5000000932">
      <Location>ja-JP</Location>
      <CodePage>932</CodePage>
      <LCID>1041</LCID>
      <TimeZone>Tokyo Standard Time</TimeZone>
      <Bias>540</Bias>
      <RunAsAdmin>false</RunAsAdmin>
      <HookIME>false</HookIME>
      <HookLCID>true</HookLCID>
    </Profile>
  </Profiles>
</LRConfig>
"#;

/// Microsoft's Visual C++ runtime, which LRProc and its hooks are built
/// against: the 64-bit one for LRProc and 64-bit games, the 32-bit one for
/// 32-bit games. Windows 10 and 11 bring the rest (the Universal C runtime
/// and .NET Framework 4), but not this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualCppRuntime {
    X64,
    X86,
}

impl VisualCppRuntime {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::X64 => "x64",
            Self::X86 => "x86",
        }
    }
}

/// Locale Remulator, kept under `directory/VERSION`.
#[derive(Clone, Debug)]
pub struct LocaleRemulator {
    directory: PathBuf,
}

impl LocaleRemulator {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// The Visual C++ runtimes this Windows lacks. Without them LRProc, or
    /// the game it starts, stops at Windows' "VCRUNTIME140.dll was not found".
    pub fn missing_runtimes() -> Vec<VisualCppRuntime> {
        #[cfg(target_os = "windows")]
        {
            let windows = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
            missing_runtimes_in(&windows)
        }
        #[cfg(not(target_os = "windows"))]
        Vec::new()
    }

    pub fn installed(&self) -> bool {
        self.program().is_file()
            && fs::read_to_string(self.config()).is_ok_and(|config| config == CONFIG)
    }

    /// Downloads and unpacks it, unless it is there already. Settings that
    /// are not the launcher's, from an earlier build or changed by hand, are
    /// put back.
    pub fn install(&self) -> Result<(), LaunchError> {
        if self.installed() {
            return Ok(());
        }
        if self.program().is_file() {
            return fs::write(self.config(), CONFIG).map_err(|error| {
                LaunchError::adapter(format!("Locale Remulator could not be set up: {error}"))
            });
        }
        self.install_from(&download()?)
    }

    /// LRProc with the launcher's profile, starting `target` in `folder`.
    pub fn command(&self, target: &Path, folder: &Path) -> Command {
        let mut command = Command::new(self.program());
        command
            .arg(PROFILE)
            .arg(ordinary_path(target))
            .current_dir(folder);
        command
    }

    fn folder(&self) -> PathBuf {
        self.directory.join(VERSION)
    }

    fn program(&self) -> PathBuf {
        self.folder().join("LRProc.exe")
    }

    fn config(&self) -> PathBuf {
        self.folder().join("LRConfig.xml")
    }

    fn install_from(&self, archive: &[u8]) -> Result<(), LaunchError> {
        let digest = hex::encode(Sha256::digest(archive));
        if digest != SHA256 {
            return Err(LaunchError::adapter(format!(
                "Locale Remulator {VERSION} did not match the tested download (SHA-256 {digest})"
            )));
        }
        let staging = self
            .directory
            .join(format!(".{VERSION}-{}", std::process::id()));
        let unpacked = unpack(archive, &staging).and_then(|()| {
            fs::write(staging.join("LRConfig.xml"), CONFIG)?;
            if self.folder().exists() {
                fs::remove_dir_all(self.folder())?;
            }
            fs::rename(&staging, self.folder())
        });
        if let Err(error) = unpacked {
            let _ = fs::remove_dir_all(&staging);
            return Err(LaunchError::adapter(format!(
                "Locale Remulator could not be set up: {error}"
            )));
        }
        Ok(())
    }
}

fn download() -> Result<Vec<u8>, LaunchError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(DOWNLOAD_TIMEOUT))
        .build()
        .into();
    let failed = |error: &dyn std::fmt::Display| {
        LaunchError::adapter(format!("Locale Remulator could not be downloaded: {error}"))
    };
    let mut response = agent.get(URL).call().map_err(|error| failed(&error))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MOST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| failed(&error))?;
    if bytes.len() as u64 > MOST_BYTES {
        return Err(failed(&"the download is larger than expected"));
    }
    Ok(bytes)
}

/// The release's files, without its top folder, into `target`.
fn unpack(archive: &[u8], target: &Path) -> std::io::Result<()> {
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
        let Ok(name) = name.strip_prefix(ARCHIVE_FOLDER) else {
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

/// The launcher runs as a 64-bit program, so System32 is the 64-bit folder
/// and SysWOW64 the 32-bit one.
#[cfg(any(target_os = "windows", test))]
fn missing_runtimes_in(windows: &Path) -> Vec<VisualCppRuntime> {
    [
        (VisualCppRuntime::X64, "System32"),
        (VisualCppRuntime::X86, "SysWOW64"),
    ]
    .into_iter()
    .filter(|(_, folder)| !windows.join(folder).join("vcruntime140.dll").is_file())
    .map(|(runtime, _)| runtime)
    .collect()
}

/// Windows canonical paths carry the `\\?\` prefix, which a command line
/// does not take.
fn ordinary_path(path: &Path) -> PathBuf {
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn refuses_a_download_that_is_not_the_tested_release() {
        let directory = tempdir().expect("temporary directory");
        let remulator = LocaleRemulator::new(directory.path());
        let error = remulator.install_from(b"not it").expect_err("refused");
        assert!(error.to_string().contains("did not match"));
        assert!(!remulator.installed());
        assert!(!directory.path().join(VERSION).exists());
    }

    #[test]
    fn unpacks_the_release_without_its_top_folder() {
        let directory = tempdir().expect("temporary directory");
        let mut archive = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut archive));
            for (name, body) in [
                ("Locale_Remulator.1.6.0/LRProc.exe", "program"),
                ("Locale_Remulator.1.6.0/LRHookx64.dll", "hook"),
                ("README.md", "outside"),
            ] {
                zip.start_file(name, SimpleFileOptions::default())
                    .expect("file");
                zip.write_all(body.as_bytes()).expect("write");
            }
            zip.finish().expect("archive");
        }
        let target = directory.path().join("unpacked");
        unpack(&archive, &target).expect("unpacked");
        assert_eq!(
            fs::read_to_string(target.join("LRProc.exe")).expect("program"),
            "program"
        );
        assert!(target.join("LRHookx64.dll").is_file());
        assert!(!target.join("README.md").exists());
    }

    #[test]
    fn finds_the_visual_cpp_runtimes_windows_lacks() {
        let windows = tempdir().expect("temporary directory");
        assert_eq!(
            missing_runtimes_in(windows.path()),
            [VisualCppRuntime::X64, VisualCppRuntime::X86]
        );

        let system = windows.path().join("System32");
        fs::create_dir_all(&system).expect("System32");
        fs::write(system.join("vcruntime140.dll"), "runtime").expect("runtime");
        assert_eq!(missing_runtimes_in(windows.path()), [VisualCppRuntime::X86]);

        let wow = windows.path().join("SysWOW64");
        fs::create_dir_all(&wow).expect("SysWOW64");
        fs::write(wow.join("vcruntime140.dll"), "runtime").expect("runtime");
        assert!(missing_runtimes_in(windows.path()).is_empty());
    }

    #[test]
    fn puts_back_settings_that_are_not_the_launchers() {
        let directory = tempdir().expect("temporary directory");
        let folder = directory.path().join(VERSION);
        fs::create_dir_all(&folder).expect("folder");
        fs::write(folder.join("LRProc.exe"), "program").expect("program");
        // What the first Windows test build wrote, which LRProc fails on.
        fs::write(
            folder.join("LRConfig.xml"),
            CONFIG.replace(r#" Type="exe""#, ""),
        )
        .expect("settings");
        let remulator = LocaleRemulator::new(directory.path());
        assert!(!remulator.installed());

        remulator.install().expect("set up without a download");

        assert!(remulator.installed());
        assert_eq!(
            fs::read_to_string(folder.join("LRConfig.xml")).expect("settings"),
            CONFIG
        );
    }

    #[test]
    fn starts_the_game_through_lrproc_with_the_launchers_profile() {
        let remulator = LocaleRemulator::new("/data/tools/locale-remulator");
        let command = remulator.command(
            Path::new(r"\\?\C:\Library\RJ1\Game.exe"),
            Path::new(r"C:\Library\RJ1"),
        );
        assert!(
            command
                .get_program()
                .to_string_lossy()
                .ends_with("LRProc.exe")
        );
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(arguments, [PROFILE, r"C:\Library\RJ1\Game.exe"]);
        // LRProc skips 37 characters, the ID and a space, to find the game.
        assert_eq!(PROFILE.len(), 36);
        assert!(CONFIG.contains(&format!("Guid=\"{PROFILE}\"")));
        assert!(CONFIG.contains(r#"<Profiles Type="exe">"#));
    }
}
