use std::sync::Arc;

use dla_application::{
    android_app::{AndroidAppError, AndroidAppPlatform, AndroidAppPlatformObservation},
    android_package::{AndroidPackageError, AndroidPackagePlatform},
};
use dla_domain::android_package::AndroidPackageState;
use tauri::{Manager, Runtime, plugin::TauriPlugin};

#[cfg(not(target_os = "android"))]
use dla_application::android_app::AndroidAppPlatformState as ObservedAndroidAppPlatformState;

pub struct AndroidPackagePlatformState(pub Arc<dyn AndroidPackagePlatform>);
pub struct AndroidAppPlatformState(pub Arc<dyn AndroidAppPlatform>);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("android-package-native")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let platform = Arc::new(MobileAndroidPackagePlatform {
                handle: api.register_android_plugin(
                    "org.dlaproject.launcher.androidpackage",
                    "AndroidPackagePlugin",
                )?,
            });
            #[cfg(target_os = "android")]
            let package_platform: Arc<dyn AndroidPackagePlatform> = platform.clone();
            #[cfg(target_os = "android")]
            let app_platform: Arc<dyn AndroidAppPlatform> = platform;
            #[cfg(not(target_os = "android"))]
            let package_platform: Arc<dyn AndroidPackagePlatform> = {
                let _ = api;
                Arc::new(UnavailableAndroidPackagePlatform)
            };
            #[cfg(not(target_os = "android"))]
            let app_platform: Arc<dyn AndroidAppPlatform> =
                Arc::new(UnavailableAndroidPackagePlatform);
            app.manage(AndroidPackagePlatformState(package_platform));
            app.manage(AndroidAppPlatformState(app_platform));
            Ok(())
        })
        .build()
}

#[cfg(target_os = "android")]
struct MobileAndroidPackagePlatform<R: Runtime> {
    handle: tauri::plugin::PluginHandle<R>,
}

#[cfg(target_os = "android")]
impl<R: Runtime> AndroidPackagePlatform for MobileAndroidPackagePlatform<R> {
    fn read_state(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        self.handle
            .run_mobile_plugin("readState", ())
            .map_err(AndroidPackageError::adapter)
    }

    fn select_and_inspect(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        self.handle
            .run_mobile_plugin("selectPackage", ())
            .map_err(AndroidPackageError::adapter)
    }

    fn clear_selection(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        self.handle
            .run_mobile_plugin("clearSelection", ())
            .map_err(AndroidPackageError::adapter)
    }

    fn open_source_approval(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        self.handle
            .run_mobile_plugin("openSourceApproval", ())
            .map_err(AndroidPackageError::adapter)
    }

    fn request_install(
        &self,
        selection_id: &str,
    ) -> Result<AndroidPackageState, AndroidPackageError> {
        self.handle
            .run_mobile_plugin("requestInstall", RequestInstallPayload { selection_id })
            .map_err(AndroidPackageError::adapter)
    }
}

#[cfg(target_os = "android")]
impl<R: Runtime> AndroidAppPlatform for MobileAndroidPackagePlatform<R> {
    fn observe(
        &self,
        package_names: &[String],
    ) -> Result<Vec<AndroidAppPlatformObservation>, AndroidAppError> {
        let response: ObserveInstalledAppsResponse = self
            .handle
            .run_mobile_plugin(
                "inspectInstalledApps",
                ObserveInstalledAppsPayload { package_names },
            )
            .map_err(AndroidAppError::adapter)?;
        Ok(response.observations)
    }

    fn launch(
        &self,
        package_name: &str,
        expected_signing_certificate_sha256: &[String],
    ) -> Result<(), AndroidAppError> {
        let response: LaunchInstalledAppResponse = self
            .handle
            .run_mobile_plugin(
                "launchInstalledApp",
                LaunchInstalledAppPayload {
                    package_name,
                    expected_signing_certificate_sha256,
                },
            )
            .map_err(AndroidAppError::adapter)?;
        if response.package_name != package_name {
            return Err(AndroidAppError::InvalidPlatformState(
                "launched package identity does not match the request",
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestInstallPayload<'a> {
    selection_id: &'a str,
}

#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ObserveInstalledAppsPayload<'a> {
    package_names: &'a [String],
}

#[cfg(target_os = "android")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ObserveInstalledAppsResponse {
    observations: Vec<AndroidAppPlatformObservation>,
}

#[cfg(target_os = "android")]
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchInstalledAppPayload<'a> {
    package_name: &'a str,
    expected_signing_certificate_sha256: &'a [String],
}

#[cfg(target_os = "android")]
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchInstalledAppResponse {
    package_name: String,
}

#[cfg(not(target_os = "android"))]
struct UnavailableAndroidPackagePlatform;

#[cfg(not(target_os = "android"))]
impl AndroidPackagePlatform for UnavailableAndroidPackagePlatform {
    fn read_state(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        Ok(AndroidPackageState::unavailable())
    }

    fn select_and_inspect(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        Err(AndroidPackageError::UnsupportedPlatform)
    }

    fn clear_selection(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        Err(AndroidPackageError::UnsupportedPlatform)
    }

    fn open_source_approval(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        Err(AndroidPackageError::UnsupportedPlatform)
    }

    fn request_install(
        &self,
        _selection_id: &str,
    ) -> Result<AndroidPackageState, AndroidPackageError> {
        Err(AndroidPackageError::UnsupportedPlatform)
    }
}

#[cfg(not(target_os = "android"))]
impl AndroidAppPlatform for UnavailableAndroidPackagePlatform {
    fn observe(
        &self,
        package_names: &[String],
    ) -> Result<Vec<AndroidAppPlatformObservation>, AndroidAppError> {
        Ok(package_names
            .iter()
            .map(|package_name| AndroidAppPlatformObservation {
                package_name: package_name.clone(),
                state: ObservedAndroidAppPlatformState::Unavailable,
                application_label: None,
                version_name: None,
                version_code: None,
                signing_certificate_sha256: Vec::new(),
                launchable: false,
                technical_detail: None,
            })
            .collect())
    }

    fn launch(
        &self,
        _package_name: &str,
        _expected_signing_certificate_sha256: &[String],
    ) -> Result<(), AndroidAppError> {
        Err(AndroidAppError::UnsupportedPlatform)
    }
}
