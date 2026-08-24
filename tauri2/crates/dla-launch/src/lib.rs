use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command},
};

use dla_application::launch::{
    LaunchClock, LaunchError, LaunchExecutionPlan, LaunchExecutionResult, LaunchExecutor,
    LaunchProcessExit, ManagedLaunchProcess,
};
#[cfg(any(target_os = "windows", target_os = "linux"))]
use dla_domain::installation::InstallationPlatform;
use dla_domain::{
    installation::{LaunchActionKind, RelativePath},
    launch::LaunchAdapter,
};
use sha2::{Digest, Sha256};

const HASH_BUFFER_BYTES: usize = 1024 * 1024;

pub struct SystemLaunchClock;

impl LaunchClock for SystemLaunchClock {
    fn now(&self) -> String {
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .expect("RFC 3339 timestamps are always representable")
    }
}

struct DesktopManagedProcess {
    child: Child,
}

impl ManagedLaunchProcess for DesktopManagedProcess {
    fn process_id(&self) -> u32 {
        self.child.id()
    }

    fn try_wait(&mut self) -> Result<Option<LaunchProcessExit>, LaunchError> {
        self.child
            .try_wait()
            .map(|status| {
                status.map(|status| LaunchProcessExit {
                    exit_code: status.code(),
                })
            })
            .map_err(|error| LaunchError::adapter(format!("could not monitor process: {error}")))
    }

    fn terminate(&mut self) -> Result<(), LaunchError> {
        if self
            .child
            .try_wait()
            .map_err(|error| LaunchError::adapter(format!("could not inspect process: {error}")))?
            .is_some()
        {
            return Ok(());
        }
        self.child
            .kill()
            .map_err(|error| LaunchError::adapter(format!("could not stop process: {error}")))
    }

    fn wait(&mut self) -> Result<LaunchProcessExit, LaunchError> {
        self.child
            .wait()
            .map(|status| LaunchProcessExit {
                exit_code: status.code(),
            })
            .map_err(|error| LaunchError::adapter(format!("could not reap process: {error}")))
    }
}

pub struct DesktopLaunchExecutor {
    wine_binary: String,
}

impl DesktopLaunchExecutor {
    pub fn new() -> Self {
        Self {
            wine_binary: std::env::var("DLA_WINE_BINARY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "wine".to_owned()),
        }
    }

    #[cfg(test)]
    fn with_wine_binary(wine_binary: impl Into<String>) -> Self {
        Self {
            wine_binary: wine_binary.into(),
        }
    }
}

impl Default for DesktopLaunchExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl LaunchExecutor for DesktopLaunchExecutor {
    fn execute(&self, plan: &LaunchExecutionPlan) -> Result<LaunchExecutionResult, LaunchError> {
        if plan.action != LaunchActionKind::LaunchExecutable {
            return Err(LaunchError::adapter(
                "the desktop executor accepts executable actions only",
            ));
        }
        let target = resolve_launch_target(
            &plan.root_path,
            &plan.relative_target,
            plan.expected_sha256.as_deref(),
        )?;
        let adapter = select_adapter(plan)?;
        let working_directory = target
            .parent()
            .ok_or_else(|| LaunchError::adapter("the executable has no valid working directory"))?;
        let mut command = match adapter {
            LaunchAdapter::WindowsNative | LaunchAdapter::LinuxNative => Command::new(&target),
            LaunchAdapter::LinuxWine => {
                let mut command = Command::new(&self.wine_binary);
                command.arg(&target);
                command
            }
        };
        let child = command
            .current_dir(working_directory)
            .spawn()
            .map_err(|error| {
                if adapter == LaunchAdapter::LinuxWine
                    && error.kind() == std::io::ErrorKind::NotFound
                {
                    LaunchError::adapter(format!(
                        "Wine is required to launch this Windows game, but '{}' was not found",
                        self.wine_binary
                    ))
                } else {
                    LaunchError::adapter(format!("could not start {}: {error}", target.display()))
                }
            })?;
        Ok(LaunchExecutionResult {
            adapter,
            process: Box::new(DesktopManagedProcess { child }),
        })
    }
}

fn resolve_launch_target(
    root_path: &str,
    relative_target: &RelativePath,
    expected_sha256: Option<&str>,
) -> Result<PathBuf, LaunchError> {
    let unresolved_root = Path::new(root_path);
    let root_metadata = fs::symlink_metadata(unresolved_root).map_err(|error| {
        LaunchError::adapter(format!("installation root is unavailable: {error}"))
    })?;
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return Err(LaunchError::adapter(
            "installation root must be a regular directory",
        ));
    }
    let canonical_root = unresolved_root
        .canonicalize()
        .map_err(|error| LaunchError::adapter(format!("installation root is invalid: {error}")))?;
    let unresolved_target = relative_target
        .as_str()
        .split('/')
        .fold(canonical_root.clone(), |path, segment| path.join(segment));
    reject_symlink_path(&canonical_root, relative_target)?;
    let target_metadata = fs::metadata(&unresolved_target)
        .map_err(|error| LaunchError::adapter(format!("launch target is unavailable: {error}")))?;
    if !target_metadata.is_file() {
        return Err(LaunchError::adapter("launch target is not a regular file"));
    }
    let canonical_target = unresolved_target
        .canonicalize()
        .map_err(|error| LaunchError::adapter(format!("launch target is invalid: {error}")))?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err(LaunchError::adapter(
            "launch target resolves outside the approved installation root",
        ));
    }
    if let Some(expected) = expected_sha256 {
        let actual = sha256_file(&canonical_target)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(LaunchError::adapter(
                "launch target hash no longer matches the verified package",
            ));
        }
    }
    Ok(canonical_target)
}

fn reject_symlink_path(root: &Path, relative_target: &RelativePath) -> Result<(), LaunchError> {
    let mut current = root.to_path_buf();
    for segment in relative_target.as_str().split('/') {
        current.push(segment);
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            LaunchError::adapter(format!("launch target path is unavailable: {error}"))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(LaunchError::adapter(
                "launch target path must not contain symbolic links",
            ));
        }
    }
    Ok(())
}

fn select_adapter(plan: &LaunchExecutionPlan) -> Result<LaunchAdapter, LaunchError> {
    #[cfg(target_os = "windows")]
    {
        if plan
            .supported_platforms
            .contains(&InstallationPlatform::Windows)
        {
            return Ok(LaunchAdapter::WindowsNative);
        }
        return Err(LaunchError::adapter(
            "the selected executable does not support Windows",
        ));
    }

    #[cfg(target_os = "linux")]
    {
        let windows_executable = plan
            .relative_target
            .as_str()
            .to_ascii_lowercase()
            .ends_with(".exe")
            && plan
                .supported_platforms
                .contains(&InstallationPlatform::Windows);
        if windows_executable {
            return Ok(LaunchAdapter::LinuxWine);
        }
        if plan
            .supported_platforms
            .contains(&InstallationPlatform::Linux)
        {
            return Ok(LaunchAdapter::LinuxNative);
        }
        Err(LaunchError::adapter(
            "the selected executable is not supported on Linux",
        ))
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = plan;
        Err(LaunchError::adapter(
            "desktop executable launch is not supported on this platform",
        ))
    }
}

fn sha256_file(path: &Path) -> Result<String, LaunchError> {
    let mut file = File::open(path).map_err(|error| {
        LaunchError::adapter(format!("could not verify launch target: {error}"))
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            LaunchError::adapter(format!("could not verify launch target: {error}"))
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(hex::encode(digest.finalize()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use dla_domain::installation::{InstallationId, InstallationPlatform, LaunchActionKind};
    use tempfile::tempdir;

    use super::*;

    fn plan(root: &Path, target: &str) -> LaunchExecutionPlan {
        LaunchExecutionPlan {
            installation_id: InstallationId("installation-1".to_owned()),
            root_path: root.to_string_lossy().into_owned(),
            action: LaunchActionKind::LaunchExecutable,
            relative_target: RelativePath::parse(target).expect("relative target"),
            supported_platforms: vec![InstallationPlatform::Windows],
            expected_sha256: None,
        }
    }

    #[test]
    fn resolves_a_regular_target_below_the_approved_root() {
        let directory = tempdir().expect("temporary directory");
        fs::create_dir(directory.path().join("game")).expect("game directory");
        fs::write(directory.path().join("game/Game.exe"), b"fixture").expect("fixture target");
        let target = RelativePath::parse("game/Game.exe").expect("target");

        let resolved = resolve_launch_target(
            directory.path().to_str().expect("root"),
            &target,
            Some("1e4ed5d2ea330d1a1ee2d2d2d2d6f11698b1629756d7a22cc2f2aa96c3a29c4c"),
        );

        assert!(matches!(resolved, Err(LaunchError::Adapter(_))));
        let resolved = resolve_launch_target(
            directory.path().to_str().expect("root"),
            &target,
            Some(&sha256_file(&directory.path().join("game/Game.exe")).expect("hash")),
        )
        .expect("resolved target");
        assert_eq!(resolved, directory.path().join("game/Game.exe"));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_even_when_it_points_back_inside_the_root() {
        let directory = tempdir().expect("temporary directory");
        fs::write(directory.path().join("Game.exe"), b"fixture").expect("fixture target");
        symlink("Game.exe", directory.path().join("Alias.exe")).expect("symlink");

        let error = resolve_launch_target(
            directory.path().to_str().expect("root"),
            &RelativePath::parse("Alias.exe").expect("target"),
            None,
        )
        .expect_err("symlink must fail");

        assert!(error.to_string().contains("symbolic links"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn routes_windows_executables_through_wine_on_linux() {
        let directory = tempdir().expect("temporary directory");
        let execution = plan(directory.path(), "Game.exe");
        assert_eq!(
            select_adapter(&execution).expect("adapter"),
            LaunchAdapter::LinuxWine
        );
        assert_eq!(
            DesktopLaunchExecutor::with_wine_binary("wine64").wine_binary,
            "wine64"
        );
    }
}
