use serde::{Deserialize, Serialize};

use crate::installation::{InstallationId, RelativePath};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationHealthState {
    Unknown,
    Healthy,
    MissingFiles,
    ModifiedFiles,
    Moved,
    Inaccessible,
    NeedsReview,
    Repairable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationHealthIssueKind {
    Missing,
    Modified,
    Inaccessible,
    Unexpected,
    InvalidOwnershipMarker,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationHealthIssue {
    pub kind: InstallationHealthIssueKind,
    pub relative_path: Option<RelativePath>,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationHealthReport {
    pub installation_id: InstallationId,
    pub state: InstallationHealthState,
    pub managed: bool,
    pub repairable: bool,
    pub checked_root: String,
    pub expected_files: u64,
    pub present_files: u64,
    pub missing_files: u64,
    pub modified_files: u64,
    pub inaccessible_files: u64,
    pub unexpected_files: u64,
    pub issues: Vec<InstallationHealthIssue>,
    pub checked_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceCleanupReport {
    pub removed_staging_directories: u64,
    pub removed_repair_directories: u64,
    pub restored_source_files: u64,
    pub retained_paths: Vec<String>,
}

/// A work's file names that an older install read wrongly: Japanese
/// (Shift-JIS) names read as CP437, which turned them into symbols.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNameReport {
    pub installation_id: InstallationId,
    /// Names still wrong.
    pub wrong: u64,
    /// Names given back just now.
    pub fixed: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFileNameReport {
    /// Works whose names were fixed.
    pub works: u64,
    pub files: u64,
    /// Works with wrong names left as they were: in use, or a name could not
    /// be given back.
    pub skipped: u64,
}

/// A file whose name an older install read wrongly, and the name it has in
/// the archive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileNameFix {
    pub from: RelativePath,
    pub to: RelativePath,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpectedInstallationFile {
    pub relative_path: RelativePath,
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationInventoryEntry {
    pub relative_path: RelativePath,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilesystemHealthSnapshot {
    pub root_exists: bool,
    pub root_accessible: bool,
    pub ownership_marker_valid: bool,
    pub present_files: u64,
    pub present_bytes: u64,
    pub missing_files: u64,
    pub modified_files: u64,
    pub inaccessible_files: u64,
    pub unexpected_files: u64,
    pub issues: Vec<InstallationHealthIssue>,
}
