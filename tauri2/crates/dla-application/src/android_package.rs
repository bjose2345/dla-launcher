use std::sync::{Arc, Mutex, MutexGuard};

use dla_domain::android_package::{
    AndroidPackageCapabilityStatus, AndroidPackageInspection, AndroidPackageState,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AndroidPackageError {
    #[error("Android package handling is unavailable on this platform")]
    UnsupportedPlatform,
    #[error("Android must approve DLA Launcher as an installation source first")]
    SourceApprovalRequired,
    #[error("select an inspected Android package first")]
    MissingSelection,
    #[error("the selected Android package cannot be installed: {0}")]
    BlockedSelection(&'static str),
    #[error("an Android package installation request is already in progress")]
    InstallInProgress,
    #[error("this selected Android package was already installed; choose it again to reinstall")]
    SelectionCompleted,
    #[error("the Android package adapter returned invalid state: {0}")]
    InvalidState(&'static str),
    #[error("Android package adapter failed: {0}")]
    Adapter(String),
    #[error("Android package operation state is unavailable")]
    OperationStateUnavailable,
}

impl AndroidPackageError {
    pub fn adapter(error: impl std::fmt::Display) -> Self {
        Self::Adapter(error.to_string())
    }
}

pub trait AndroidPackagePlatform: Send + Sync {
    fn read_state(&self) -> Result<AndroidPackageState, AndroidPackageError>;
    fn select_and_inspect(&self) -> Result<AndroidPackageState, AndroidPackageError>;
    fn clear_selection(&self) -> Result<AndroidPackageState, AndroidPackageError>;
    fn open_source_approval(&self) -> Result<AndroidPackageState, AndroidPackageError>;
    fn request_install(
        &self,
        selection_id: &str,
    ) -> Result<AndroidPackageState, AndroidPackageError>;
}

pub struct AndroidPackageService {
    platform: Arc<dyn AndroidPackagePlatform>,
    operation_gate: Mutex<()>,
}

impl AndroidPackageService {
    pub fn new(platform: Arc<dyn AndroidPackagePlatform>) -> Self {
        Self {
            platform,
            operation_gate: Mutex::new(()),
        }
    }

    pub fn read_state(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        let _guard = self.lock_operation()?;
        self.read_state_unlocked()
    }

    fn read_state_unlocked(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        validate_state(self.platform.read_state()?)
    }

    pub fn select_and_inspect(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        let _guard = self.lock_operation()?;
        let current = self.read_state_unlocked()?;
        ensure_available(&current)?;
        ensure_not_installing(&current)?;
        validate_state(self.platform.select_and_inspect()?)
    }

    pub fn clear_selection(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        let _guard = self.lock_operation()?;
        let current = self.read_state_unlocked()?;
        ensure_available(&current)?;
        ensure_not_installing(&current)?;
        validate_state(self.platform.clear_selection()?)
    }

    pub fn open_source_approval(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        let _guard = self.lock_operation()?;
        let current = self.read_state_unlocked()?;
        ensure_available(&current)?;
        ensure_not_installing(&current)?;
        validate_state(self.platform.open_source_approval()?)
    }

    pub fn request_install(&self) -> Result<AndroidPackageState, AndroidPackageError> {
        let _guard = self.lock_operation()?;
        let current = self.read_state_unlocked()?;
        ensure_available(&current)?;
        if current.capability.status != AndroidPackageCapabilityStatus::Ready {
            return Err(AndroidPackageError::SourceApprovalRequired);
        }
        ensure_not_installing(&current)?;
        if current.install_status.as_ref().is_some_and(|status| {
            status.state == dla_domain::android_package::AndroidPackageInstallState::Installed
        }) {
            return Err(AndroidPackageError::SelectionCompleted);
        }
        let inspection = current
            .inspection
            .as_ref()
            .ok_or(AndroidPackageError::MissingSelection)?;
        if !inspection.installable {
            return Err(AndroidPackageError::BlockedSelection(block_reason(
                inspection,
            )));
        }
        validate_state(self.platform.request_install(&inspection.selection_id)?)
    }

    fn lock_operation(&self) -> Result<MutexGuard<'_, ()>, AndroidPackageError> {
        self.operation_gate
            .lock()
            .map_err(|_| AndroidPackageError::OperationStateUnavailable)
    }
}

fn ensure_not_installing(state: &AndroidPackageState) -> Result<(), AndroidPackageError> {
    if state
        .install_status
        .as_ref()
        .is_some_and(|status| status.state.is_pending())
    {
        Err(AndroidPackageError::InstallInProgress)
    } else {
        Ok(())
    }
}

fn ensure_available(state: &AndroidPackageState) -> Result<(), AndroidPackageError> {
    if state.capability.status == AndroidPackageCapabilityStatus::Unavailable {
        Err(AndroidPackageError::UnsupportedPlatform)
    } else {
        Ok(())
    }
}

fn validate_state(state: AndroidPackageState) -> Result<AndroidPackageState, AndroidPackageError> {
    if state.capability.status == AndroidPackageCapabilityStatus::Unavailable {
        if state.capability.device_sdk.is_some()
            || state.inspection.is_some()
            || state.install_status.is_some()
        {
            return Err(AndroidPackageError::InvalidState(
                "an unavailable capability cannot expose Android state",
            ));
        }
        return Ok(state);
    }

    if state.capability.device_sdk.is_none() {
        return Err(AndroidPackageError::InvalidState(
            "Android capability is missing the device SDK",
        ));
    }

    if let Some(inspection) = &state.inspection {
        validate_inspection(inspection, state.capability.device_sdk)?;
    }
    if let Some(status) = &state.install_status {
        if !valid_token(&status.operation_id) || !valid_token(&status.selection_id) {
            return Err(AndroidPackageError::InvalidState(
                "install status contains an invalid opaque identifier",
            ));
        }
        let inspection = state
            .inspection
            .as_ref()
            .ok_or(AndroidPackageError::InvalidState(
                "install status has no matching inspection",
            ))?;
        if status.selection_id != inspection.selection_id {
            return Err(AndroidPackageError::InvalidState(
                "install status refers to another selection",
            ));
        }
        if status
            .technical_detail
            .as_ref()
            .is_some_and(|detail| detail.len() > 2_048)
        {
            return Err(AndroidPackageError::InvalidState(
                "install status detail is unreasonably long",
            ));
        }
    }
    Ok(state)
}

fn validate_inspection(
    inspection: &AndroidPackageInspection,
    device_sdk: Option<u32>,
) -> Result<(), AndroidPackageError> {
    if !valid_token(&inspection.selection_id) {
        return Err(AndroidPackageError::InvalidState(
            "inspection contains an invalid selection identifier",
        ));
    }
    if inspection.display_name.trim().is_empty()
        || inspection.display_name.len() > 512
        || inspection.application_label.trim().is_empty()
        || inspection.application_label.len() > 512
        || inspection.package_name.trim().is_empty()
        || inspection.package_name.len() > 255
        || inspection.version_code.trim().is_empty()
        || inspection.version_code.len() > 32
        || !inspection
            .version_code
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        return Err(AndroidPackageError::InvalidState(
            "inspection contains invalid package identity",
        ));
    }
    if inspection.size_bytes == 0 || !valid_sha256(&inspection.sha256) {
        return Err(AndroidPackageError::InvalidState(
            "inspection contains invalid file evidence",
        ));
    }
    if inspection
        .signing_certificate_sha256
        .iter()
        .any(|fingerprint| !valid_sha256(fingerprint))
    {
        return Err(AndroidPackageError::InvalidState(
            "inspection contains an invalid signing fingerprint",
        ));
    }
    if inspection.signing_certificate_sha256.len() > 32 {
        return Err(AndroidPackageError::InvalidState(
            "inspection contains too many signing fingerprints",
        ));
    }
    if inspection.installable != inspection.block_reason.is_none() {
        return Err(AndroidPackageError::InvalidState(
            "inspection installability contradicts its block reason",
        ));
    }
    if inspection.installable && inspection.signing_certificate_sha256.is_empty() {
        return Err(AndroidPackageError::InvalidState(
            "an installable package has no signing fingerprint",
        ));
    }
    if inspection.installable
        && inspection
            .minimum_sdk
            .zip(device_sdk)
            .is_some_and(|(minimum, device)| minimum > device)
    {
        return Err(AndroidPackageError::InvalidState(
            "an installable package requires a newer Android SDK",
        ));
    }
    Ok(())
}

fn valid_token(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn block_reason(inspection: &AndroidPackageInspection) -> &'static str {
    match inspection.block_reason {
        Some(dla_domain::android_package::AndroidPackageBlockReason::IncompatibleSdk) => {
            "requires a newer Android version"
        }
        Some(dla_domain::android_package::AndroidPackageBlockReason::SplitPackage) => {
            "split APK sets are not supported"
        }
        Some(dla_domain::android_package::AndroidPackageBlockReason::SelfUpdate) => {
            "the launcher cannot update itself through this feature"
        }
        Some(dla_domain::android_package::AndroidPackageBlockReason::MissingSignature) => {
            "the APK has no readable signing certificate"
        }
        None => "native inspection refused it",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_domain::android_package::{
        AndroidPackageBlockReason, AndroidPackageCapability, AndroidPackageInstallState,
        AndroidPackageInstallStatus,
    };

    use super::*;

    struct RecordingPlatform {
        state: Mutex<AndroidPackageState>,
        requests: Mutex<Vec<String>>,
    }

    impl AndroidPackagePlatform for RecordingPlatform {
        fn read_state(&self) -> Result<AndroidPackageState, AndroidPackageError> {
            Ok(self.state.lock().expect("state").clone())
        }

        fn select_and_inspect(&self) -> Result<AndroidPackageState, AndroidPackageError> {
            self.read_state()
        }

        fn clear_selection(&self) -> Result<AndroidPackageState, AndroidPackageError> {
            self.read_state()
        }

        fn open_source_approval(&self) -> Result<AndroidPackageState, AndroidPackageError> {
            self.read_state()
        }

        fn request_install(
            &self,
            selection_id: &str,
        ) -> Result<AndroidPackageState, AndroidPackageError> {
            self.requests
                .lock()
                .expect("requests")
                .push(selection_id.to_owned());
            self.read_state()
        }
    }

    #[test]
    fn refuses_native_install_until_source_approval_is_ready() {
        let platform = Arc::new(RecordingPlatform {
            state: Mutex::new(state(
                AndroidPackageCapabilityStatus::ApprovalRequired,
                inspection(true, None),
            )),
            requests: Mutex::new(Vec::new()),
        });
        let service = AndroidPackageService::new(platform.clone());

        assert!(matches!(
            service.request_install(),
            Err(AndroidPackageError::SourceApprovalRequired)
        ));
        assert!(platform.requests.lock().expect("requests").is_empty());
    }

    #[test]
    fn refuses_blocked_inspection_before_native_install() {
        let platform = Arc::new(RecordingPlatform {
            state: Mutex::new(state(
                AndroidPackageCapabilityStatus::Ready,
                inspection(false, Some(AndroidPackageBlockReason::SplitPackage)),
            )),
            requests: Mutex::new(Vec::new()),
        });
        let service = AndroidPackageService::new(platform.clone());

        assert!(matches!(
            service.request_install(),
            Err(AndroidPackageError::BlockedSelection(_))
        ));
        assert!(platform.requests.lock().expect("requests").is_empty());
    }

    #[test]
    fn passes_only_the_native_opaque_selection_identifier() {
        let platform = Arc::new(RecordingPlatform {
            state: Mutex::new(state(
                AndroidPackageCapabilityStatus::Ready,
                inspection(true, None),
            )),
            requests: Mutex::new(Vec::new()),
        });
        let service = AndroidPackageService::new(platform.clone());

        service.request_install().expect("install request");
        assert_eq!(
            platform.requests.lock().expect("requests").as_slice(),
            ["12345678-1234-1234-1234-123456789abc"]
        );
    }

    #[test]
    fn rejects_contradictory_native_inspection() {
        let platform = Arc::new(RecordingPlatform {
            state: Mutex::new(state(
                AndroidPackageCapabilityStatus::Ready,
                inspection(true, Some(AndroidPackageBlockReason::SelfUpdate)),
            )),
            requests: Mutex::new(Vec::new()),
        });
        let service = AndroidPackageService::new(platform);

        assert!(matches!(
            service.read_state(),
            Err(AndroidPackageError::InvalidState(_))
        ));
    }

    #[test]
    fn refuses_source_settings_while_an_install_is_pending() {
        let mut current = state(
            AndroidPackageCapabilityStatus::Ready,
            inspection(true, None),
        );
        current.install_status = Some(AndroidPackageInstallStatus {
            operation_id: "87654321-4321-4321-4321-cba987654321".to_owned(),
            selection_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
            state: AndroidPackageInstallState::AwaitingUserConfirmation,
            technical_detail: None,
        });
        let platform = Arc::new(RecordingPlatform {
            state: Mutex::new(current),
            requests: Mutex::new(Vec::new()),
        });
        let service = AndroidPackageService::new(platform);

        assert!(matches!(
            service.open_source_approval(),
            Err(AndroidPackageError::InstallInProgress)
        ));
    }

    fn state(
        capability: AndroidPackageCapabilityStatus,
        inspection: AndroidPackageInspection,
    ) -> AndroidPackageState {
        AndroidPackageState {
            capability: AndroidPackageCapability {
                status: capability,
                device_sdk: Some(36),
            },
            install_status: None,
            inspection: Some(inspection),
        }
    }

    fn inspection(
        installable: bool,
        block_reason: Option<AndroidPackageBlockReason>,
    ) -> AndroidPackageInspection {
        AndroidPackageInspection {
            selection_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
            display_name: "fixture.apk".to_owned(),
            application_label: "Fixture".to_owned(),
            package_name: "org.dlaproject.fixture".to_owned(),
            version_name: Some("1.0".to_owned()),
            version_code: "1".to_owned(),
            size_bytes: 1024,
            sha256: "a".repeat(64),
            minimum_sdk: Some(24),
            target_sdk: Some(36),
            signing_certificate_sha256: vec!["b".repeat(64)],
            installable,
            block_reason,
        }
    }
}
