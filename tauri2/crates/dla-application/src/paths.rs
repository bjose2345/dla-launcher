//! The one filesystem check the adapters share: a path from the database or
//! a URL must stay inside the folder the user approved, also once symbolic
//! links are followed.

use std::{
    io,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathEscape {
    #[error("the root folder is unavailable: {0}")]
    Root(#[source] io::Error),
    #[error("{}: {error}", .path.display())]
    Target {
        path: PathBuf,
        #[source]
        error: io::Error,
    },
    #[error("the path resolves outside its root")]
    Outside,
}

/// Resolves `relative` under `root` and returns the canonical path, refusing
/// absolute or parent segments and anything that resolves outside the
/// canonical root.
pub fn resolve_within(root: &Path, relative: &Path) -> Result<PathBuf, PathEscape> {
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(PathEscape::Outside);
    }
    let root = root.canonicalize().map_err(PathEscape::Root)?;
    let attempted = root.join(relative);
    let target = attempted
        .canonicalize()
        .map_err(|error| PathEscape::Target {
            path: attempted.clone(),
            error,
        })?;
    if target.starts_with(&root) {
        Ok(target)
    } else {
        Err(PathEscape::Outside)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolves_a_file_under_the_root() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.txt"), b"a").unwrap();
        assert_eq!(
            resolve_within(root.path(), Path::new("./a.txt")).unwrap(),
            root.path().canonicalize().unwrap().join("a.txt")
        );
    }

    #[test]
    fn refuses_parent_segments_and_absolute_paths() {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            resolve_within(root.path(), Path::new("../a.txt")),
            Err(PathEscape::Outside)
        ));
        assert!(matches!(
            resolve_within(root.path(), Path::new("/a.txt")),
            Err(PathEscape::Outside)
        ));
    }

    #[test]
    fn reports_a_missing_target_with_its_path() {
        let root = tempfile::tempdir().unwrap();
        match resolve_within(root.path(), Path::new("gone.txt")) {
            Err(PathEscape::Target { path, error }) => {
                assert_eq!(error.kind(), io::ErrorKind::NotFound);
                assert!(path.ends_with("gone.txt"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_link_that_leaves_the_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("private.txt"), b"p").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        assert!(matches!(
            resolve_within(root.path(), Path::new("escape/private.txt")),
            Err(PathEscape::Outside)
        ));
    }
}
