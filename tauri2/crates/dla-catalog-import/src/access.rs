use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
};

use dla_application::catalog_import::CatalogImportError;
use uuid::Uuid;

pub const CATALOG_PACKAGE_FILE_EXTENSIONS: &[&str] = &["dla"];
pub const DEFAULT_CATALOG_PACKAGE_FILENAME: &str = "catalog.dla";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovedCatalogPackage {
    pub access_handle: String,
    pub display_name: String,
}

pub struct CatalogPackageAccessRegistry {
    approved: Mutex<HashMap<String, PathBuf>>,
}

impl CatalogPackageAccessRegistry {
    pub fn new() -> Self {
        Self {
            approved: Mutex::new(HashMap::new()),
        }
    }

    pub fn approve(&self, path: &Path) -> Result<ApprovedCatalogPackage, CatalogImportError> {
        if !path.is_file() {
            return Err(CatalogImportError::access(format!(
                "selected catalog package is not a file: {}",
                path.display()
            )));
        }
        if !has_catalog_package_extension(path) {
            return Err(CatalogImportError::access(
                "catalog packages must use the .dla extension",
            ));
        }
        let canonical = path.canonicalize().map_err(CatalogImportError::access)?;
        let access_handle = Uuid::new_v4().to_string();
        let display_name = canonical
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(DEFAULT_CATALOG_PACKAGE_FILENAME)
            .to_owned();
        self.approved
            .lock()
            .map_err(|error| CatalogImportError::access(error.to_string()))?
            .insert(access_handle.clone(), canonical);
        Ok(ApprovedCatalogPackage {
            access_handle,
            display_name,
        })
    }

    pub fn resolve(&self, access_handle: &str) -> Result<PathBuf, CatalogImportError> {
        self.approved
            .lock()
            .map_err(|error| CatalogImportError::access(error.to_string()))?
            .get(access_handle)
            .cloned()
            .ok_or_else(|| CatalogImportError::access("catalog package access expired"))
    }
}

fn has_catalog_package_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            CATALOG_PACKAGE_FILE_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

impl Default for CatalogPackageAccessRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn approves_current_dla_extension_case_insensitively() {
        for filename in [
            "catalog-compact-20260808T162024Z.dla",
            "catalog-full-20260808T162024Z.dla",
            "catalog-custom-a1b2c3d4-20260808T162024Z.dla",
            "catalog.DLA",
        ] {
            let directory = tempdir().expect("temporary directory");
            let path = directory.path().join(filename);
            File::create(&path).expect("catalog package");

            let approved = CatalogPackageAccessRegistry::new()
                .approve(&path)
                .expect("approve current catalog package extension");

            assert_eq!(approved.display_name, filename);
        }
    }

    #[test]
    fn rejects_unrelated_file_extensions() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("catalog.zip");
        File::create(&path).expect("unrelated file");

        let error = CatalogPackageAccessRegistry::new()
            .approve(&path)
            .expect_err("reject unrelated extension");

        assert!(error.to_string().contains("must use the .dla extension"));
    }
}
