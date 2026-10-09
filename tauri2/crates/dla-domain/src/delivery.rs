use serde::{Deserialize, Serialize};

use crate::installation::InstallationId;

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DeliveryJobId(pub String);

/// One archive of the catalog release being installed. The remote path is
/// known once the archive has been found on the DLA server by its SHA-256.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryArchive {
    /// The catalog archive name, which multipart volumes need on disk.
    pub file_name: String,
    pub size: u64,
    /// Lowercase hexadecimal.
    pub sha256: String,
    pub remote_path: Option<String>,
    /// Downloaded, verified and renamed to `file_name`.
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Queued,
    Downloading,
    Installing,
    Paused,
    Installed,
    /// The archives are in the Library but need a decision before Play.
    NeedsReview,
    Failed,
    Cancelled,
}

impl DeliveryStatus {
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Self::Installed | Self::NeedsReview | Self::Failed | Self::Cancelled
        )
    }
}

/// Why a job stopped, as a stable code the interface localizes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryProblem {
    SignInRequired,
    Credentials,
    TooManyAttempts,
    Unreachable,
    /// The catalog release has no complete set of archives with SHA-256
    /// values, or the server does not hold them.
    NotInArchive,
    /// The account's monthly download allowance is used up.
    Allowance,
    /// The account is already downloading from another device.
    Busy,
    DiskSpace,
    /// A downloaded archive did not match its expected SHA-256.
    Corrupt,
    InstallFolder,
    Install,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryJob {
    pub id: DeliveryJobId,
    pub work_code: String,
    /// The work's release date, which suggests the archive month to search.
    pub release_date: String,
    pub version: String,
    pub update_date: String,
    pub archives: Vec<DeliveryArchive>,
    /// The parent folder installations are prepared into.
    pub install_folder: String,
    pub status: DeliveryStatus,
    pub received_bytes: u64,
    pub problem: Option<DeliveryProblem>,
    pub detail: Option<String>,
    pub installation_id: Option<InstallationId>,
    pub created_at: String,
    pub updated_at: String,
}

impl DeliveryJob {
    pub fn total_bytes(&self) -> u64 {
        self.archives.iter().map(|archive| archive.size).sum()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryPhase {
    Locating,
    /// Re-reading a part saved earlier before continuing it.
    Checking,
    Receiving,
    Verifying,
    /// Extracting the archives into the install folder.
    Unpacking,
    Installing,
}

/// Transient progress of the running job, published to the interface.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryProgress {
    pub job_id: DeliveryJobId,
    pub work_code: String,
    pub phase: DeliveryPhase,
    pub received_bytes: u64,
    pub total_bytes: u64,
}
