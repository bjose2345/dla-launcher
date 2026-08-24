use dla_domain::package::{PackageManifest, PackageSourceSet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PackageManifestError {
    #[error("package source is unavailable: {0}")]
    Unavailable(String),
    #[error("unsupported package format: {0}")]
    UnsupportedFormat(String),
    #[error("package manifest inspection failed: {0}")]
    Inspection(String),
}

pub trait PackageManifestReader: Send + Sync {
    fn read_manifest(
        &self,
        root_path: &str,
        source_set: &PackageSourceSet,
    ) -> Result<PackageManifest, PackageManifestError>;
}
