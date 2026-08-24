#![cfg(target_os = "linux")]

use std::{env, path::PathBuf};

use dla_application::launch::{LaunchExecutionPlan, LaunchExecutor};
use dla_domain::{
    installation::{InstallationId, InstallationPlatform, LaunchActionKind, RelativePath},
    launch::LaunchAdapter,
};
use dla_launch::DesktopLaunchExecutor;

#[test]
#[ignore = "run through scripts/verify-wine32.sh in the Tauri development image"]
fn launches_a_pe32_fixture_through_the_desktop_wine_adapter() {
    let fixture = PathBuf::from(
        env::var("DLA_WINE32_FIXTURE").expect("DLA_WINE32_FIXTURE must identify the PE32 fixture"),
    );
    let root = fixture.parent().expect("fixture parent directory");
    let target = fixture
        .file_name()
        .and_then(|name| name.to_str())
        .expect("fixture filename must be UTF-8");
    let plan = LaunchExecutionPlan {
        installation_id: InstallationId("wine32-runtime-gate".to_owned()),
        root_path: root.to_string_lossy().into_owned(),
        action: LaunchActionKind::LaunchExecutable,
        relative_target: RelativePath::parse(target).expect("relative fixture target"),
        supported_platforms: vec![InstallationPlatform::Windows],
        expected_sha256: None,
    };

    let mut result = DesktopLaunchExecutor::new()
        .execute(&plan)
        .expect("PE32 fixture should start through Wine");
    assert_eq!(result.adapter, LaunchAdapter::LinuxWine);
    assert_eq!(
        result
            .process
            .wait()
            .expect("fixture should be reaped")
            .exit_code,
        Some(0)
    );
}
