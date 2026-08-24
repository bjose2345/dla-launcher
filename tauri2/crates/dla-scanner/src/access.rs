use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use dla_application::scanner::ScannerError;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedScanRoot {
    pub access_handle: String,
    pub platform: String,
    pub path_key: String,
    pub display_path: String,
}

pub struct ScanAccessRegistry {
    roots: Mutex<HashMap<String, PathBuf>>,
}

impl Default for ScanAccessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanAccessRegistry {
    pub fn new() -> Self {
        Self {
            roots: Mutex::new(HashMap::new()),
        }
    }

    pub fn approve(&self, path: &Path) -> Result<ApprovedScanRoot, ScannerError> {
        let canonical = path
            .canonicalize()
            .map_err(|error| ScannerError::RootUnavailable(error.to_string()))?;
        if !canonical.is_dir() {
            return Err(ScannerError::RootUnavailable(
                "the selected path is not a directory".to_owned(),
            ));
        }
        let platform = std::env::consts::OS.to_owned();
        let display_path = canonical.to_string_lossy().into_owned();
        let path_key = normalize_root_key(&display_path, &platform);
        let access_handle = Uuid::new_v4().to_string();
        self.roots
            .lock()
            .map_err(|error| ScannerError::Persistence(error.to_string()))?
            .insert(access_handle.clone(), canonical);
        Ok(ApprovedScanRoot {
            access_handle,
            platform,
            path_key,
            display_path,
        })
    }

    pub fn resolve(&self, access_handle: &str) -> Result<PathBuf, ScannerError> {
        self.roots
            .lock()
            .map_err(|error| ScannerError::Persistence(error.to_string()))?
            .get(access_handle)
            .cloned()
            .ok_or_else(|| {
                ScannerError::RootUnavailable(
                    "the folder approval is missing or expired; choose the folder again".to_owned(),
                )
            })
    }

    pub fn describe(&self, access_handle: &str) -> Result<ApprovedScanRoot, ScannerError> {
        let path = self.resolve(access_handle)?;
        let platform = std::env::consts::OS.to_owned();
        let display_path = path.to_string_lossy().into_owned();
        Ok(ApprovedScanRoot {
            access_handle: access_handle.to_owned(),
            path_key: normalize_root_key(&display_path, &platform),
            display_path,
            platform,
        })
    }
}

fn normalize_root_key(path: &str, platform: &str) -> String {
    let normalized = path.replace('\\', "/");
    if platform == "windows" {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn approves_only_existing_directories() {
        let directory = tempdir().expect("temporary directory");
        let registry = ScanAccessRegistry::new();
        let approved = registry.approve(directory.path()).expect("approved root");

        assert_eq!(
            registry
                .resolve(&approved.access_handle)
                .expect("resolved root"),
            directory.path().canonicalize().expect("canonical root")
        );
        assert!(registry.approve(&directory.path().join("missing")).is_err());
    }

    #[test]
    fn normalizes_windows_keys_without_changing_unix_case() {
        assert_eq!(
            normalize_root_key("C:\\Library\\Works", "windows"),
            "c:/library/works"
        );
        assert_eq!(
            normalize_root_key("/Library/Works", "linux"),
            "/Library/Works"
        );
    }
}
