use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AndroidPackageCapabilityStatus {
    Unavailable,
    ApprovalRequired,
    Ready,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidPackageCapability {
    pub status: AndroidPackageCapabilityStatus,
    pub device_sdk: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AndroidPackageBlockReason {
    IncompatibleSdk,
    SplitPackage,
    SelfUpdate,
    MissingSignature,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidPackageInspection {
    pub selection_id: String,
    pub display_name: String,
    pub application_label: String,
    pub package_name: String,
    pub version_name: Option<String>,
    pub version_code: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub minimum_sdk: Option<u32>,
    pub target_sdk: Option<u32>,
    pub signing_certificate_sha256: Vec<String>,
    pub installable: bool,
    pub block_reason: Option<AndroidPackageBlockReason>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AndroidPackageInstallState {
    ApprovalRequired,
    Preparing,
    AwaitingUserConfirmation,
    Installed,
    Cancelled,
    Failed,
}

impl AndroidPackageInstallState {
    pub fn is_pending(self) -> bool {
        matches!(self, Self::Preparing | Self::AwaitingUserConfirmation)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidPackageInstallStatus {
    pub operation_id: String,
    pub selection_id: String,
    pub state: AndroidPackageInstallState,
    pub technical_detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidPackageState {
    pub capability: AndroidPackageCapability,
    pub inspection: Option<AndroidPackageInspection>,
    pub install_status: Option<AndroidPackageInstallStatus>,
}

impl AndroidPackageState {
    pub fn unavailable() -> Self {
        Self {
            capability: AndroidPackageCapability {
                status: AndroidPackageCapabilityStatus::Unavailable,
                device_sdk: None,
            },
            inspection: None,
            install_status: None,
        }
    }
}
