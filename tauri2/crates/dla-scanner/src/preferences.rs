use std::{
    env,
    path::{Path, PathBuf},
};

use dla_application::scanner::{ScanRootLocation, ScanRootLocationProvider, ScannerError};

const DEFAULT_FOLDER_NAME: &str = "My Works";
const ROOT_OVERRIDE_ENVIRONMENT_VARIABLE: &str = "DLA_DEFAULT_SCAN_ROOT";

pub struct DesktopScanRootLocations {
    platform: String,
    default_path: Option<PathBuf>,
}

impl Default for DesktopScanRootLocations {
    fn default() -> Self {
        Self::new()
    }
}

impl DesktopScanRootLocations {
    pub fn new() -> Self {
        let platform = env::consts::OS.to_owned();
        let default_path = if matches!(platform.as_str(), "android" | "ios") {
            None
        } else {
            resolve_default_path(
                env::var_os(ROOT_OVERRIDE_ENVIRONMENT_VARIABLE).map(PathBuf::from),
                home_directory(&platform),
            )
        };
        Self {
            platform,
            default_path,
        }
    }

    #[cfg(test)]
    fn from_parts(platform: &str, override_path: Option<PathBuf>, home: Option<PathBuf>) -> Self {
        Self {
            platform: platform.to_owned(),
            default_path: resolve_default_path(override_path, home),
        }
    }
}

impl ScanRootLocationProvider for DesktopScanRootLocations {
    fn platform(&self) -> String {
        self.platform.clone()
    }

    fn default_root(&self) -> Option<ScanRootLocation> {
        self.default_path.as_ref().map(|path| ScanRootLocation {
            platform: self.platform.clone(),
            display_path: path.to_string_lossy().into_owned(),
        })
    }

    fn is_directory(&self, location: &ScanRootLocation) -> bool {
        location.platform == self.platform && Path::new(&location.display_path).is_dir()
    }

    fn create_default_root(&self, location: &ScanRootLocation) -> Result<(), ScannerError> {
        let expected = self.default_path.as_ref().ok_or_else(|| {
            ScannerError::RootUnavailable(
                "this platform does not provide a default scan root".to_owned(),
            )
        })?;
        if location.platform != self.platform || Path::new(&location.display_path) != expected {
            return Err(ScannerError::InvalidRequest(
                "only the platform default scan root can be created automatically".to_owned(),
            ));
        }
        std::fs::create_dir_all(expected).map_err(ScannerError::filesystem)
    }
}

fn resolve_default_path(override_path: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    override_path
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(|| home.map(|path| path.join(DEFAULT_FOLDER_NAME)))
}

fn home_directory(platform: &str) -> Option<PathBuf> {
    if platform == "windows" {
        env::var_os("USERPROFILE").map(PathBuf::from).or_else(|| {
            match (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
                (Some(drive), Some(path)) => {
                    let mut home = PathBuf::from(drive);
                    home.push(path);
                    Some(home)
                }
                _ => None,
            }
        })
    } else {
        env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_my_works_from_the_platform_home() {
        let locations = DesktopScanRootLocations::from_parts(
            "linux",
            None,
            Some(PathBuf::from("/home/example")),
        );

        assert_eq!(
            locations.default_root().expect("default root").display_path,
            "/home/example/My Works"
        );
    }

    #[test]
    fn explicit_environment_override_wins_over_the_home_default() {
        let locations = DesktopScanRootLocations::from_parts(
            "linux",
            Some(PathBuf::from("/workspace/libraries")),
            Some(PathBuf::from("/home/example")),
        );

        assert_eq!(
            locations.default_root().expect("default root").display_path,
            "/workspace/libraries"
        );
    }
}
