use std::{
    fs,
    path::{Path, PathBuf},
};

use dla_application::package_inspection::PackageManifestError;
use dla_domain::package::PackageSourceSet;

use crate::portable_path;

pub(crate) fn resolve_source_files(
    root_path: &str,
    source_set: &PackageSourceSet,
) -> Result<(PathBuf, Vec<PathBuf>), PackageManifestError> {
    let root = Path::new(root_path)
        .canonicalize()
        .map_err(|error| PackageManifestError::Unavailable(error.to_string()))?;
    let mut paths = Vec::with_capacity(source_set.volumes.len());
    for volume in &source_set.volumes {
        let unresolved = root.join(portable_path(&volume.relative_path));
        let metadata = fs::symlink_metadata(&unresolved)
            .map_err(|error| PackageManifestError::Unavailable(error.to_string()))?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(PackageManifestError::Unavailable(format!(
                "package volume is not a regular file: {}",
                volume.relative_path
            )));
        }
        if volume.size_bytes.is_some_and(|size| size != metadata.len()) {
            return Err(PackageManifestError::Unavailable(format!(
                "package volume size changed after scanning: {}",
                volume.relative_path
            )));
        }
        let path = unresolved
            .canonicalize()
            .map_err(|error| PackageManifestError::Unavailable(error.to_string()))?;
        if !path.starts_with(&root) {
            return Err(PackageManifestError::Unavailable(
                "package volume escapes the selected scan root".to_owned(),
            ));
        }
        paths.push(path);
    }
    if paths.is_empty() {
        return Err(PackageManifestError::Unavailable(
            "package source set is empty".to_owned(),
        ));
    }
    if paths.len() > 1 {
        let parent = paths[0].parent();
        if paths.iter().any(|path| path.parent() != parent) {
            return Err(PackageManifestError::Unavailable(
                "multipart package volumes must share one directory".to_owned(),
            ));
        }
    }
    Ok((root, paths))
}
