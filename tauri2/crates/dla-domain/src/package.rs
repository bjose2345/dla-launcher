use serde::{Deserialize, Serialize};

use crate::{
    CatalogRom, CatalogRomContents,
    installation::{InferenceConfidence, InstallationPlatform, LaunchActionKind, RelativePath},
    scanner::ScanEntryId,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceArtifactKind {
    Archive,
    Directory,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFormat {
    Zip,
    Rar,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSourceSetKind {
    SingleArchive,
    MultipartRar,
    MultipartRarSfx,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageSafety {
    Safe,
    Unsafe,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageContentKind {
    WindowsGame,
    WindowsApplication,
    AudioCollection,
    ImageCollection,
    VideoCollection,
    AndroidApplication,
    MixedMedia,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveRetentionPolicy {
    Keep,
    DeleteAfterVerifiedInstall,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceArtifact {
    pub scan_entry_id: ScanEntryId,
    pub kind: SourceArtifactKind,
    pub relative_path: RelativePath,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageSourceSet {
    pub kind: PackageSourceSetKind,
    pub volumes: Vec<SourceArtifact>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageIssue {
    pub code: String,
    pub entry_index: Option<u64>,
    pub path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManifestEntry {
    pub entry_index: u64,
    pub relative_path: Option<RelativePath>,
    pub raw_name: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub encrypted: bool,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub crc32: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManifest {
    pub format: ArchiveFormat,
    pub entries: Vec<PackageManifestEntry>,
    pub file_count: u64,
    pub directory_count: u64,
    pub total_compressed_bytes: u64,
    pub total_uncompressed_bytes: u64,
    pub common_root: Option<RelativePath>,
    pub safety: PackageSafety,
    pub issues: Vec<PackageIssue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogPackageContext {
    pub work_code: String,
    pub category_names: Vec<String>,
    pub file_format_names: Vec<String>,
    pub rom_position: usize,
    pub rom_count: usize,
    pub rom: CatalogRom,
    pub contents: Option<CatalogRomContents>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPackageRelease {
    pub rom_position: usize,
    pub rom_count: usize,
    pub name: String,
    pub version: String,
    pub update_date: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageLaunchCandidate {
    pub action: LaunchActionKind,
    pub relative_path: RelativePath,
    pub supported_platforms: Vec<InstallationPlatform>,
    pub confidence: InferenceConfidence,
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageClassification {
    pub content_kind: PackageContentKind,
    pub engine: Option<String>,
    pub platform: InstallationPlatform,
    pub confidence: InferenceConfidence,
    pub reason_codes: Vec<String>,
    pub content_root: Option<RelativePath>,
    pub launch_candidates: Vec<PackageLaunchCandidate>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub requires_extraction: bool,
    pub content_root: Option<RelativePath>,
    pub preferred_action: Option<PackageLaunchCandidate>,
    pub archive_retention: ArchiveRetentionPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageInspection {
    pub source: SourceArtifact,
    #[serde(default)]
    pub source_set: Option<PackageSourceSet>,
    #[serde(default)]
    pub catalog_release: Option<CatalogPackageRelease>,
    pub format: ArchiveFormat,
    pub safety: PackageSafety,
    pub entry_count: u64,
    pub file_count: u64,
    pub directory_count: u64,
    pub total_compressed_bytes: u64,
    pub total_uncompressed_bytes: u64,
    pub common_root: Option<RelativePath>,
    pub issues: Vec<PackageIssue>,
    pub classification: PackageClassification,
    pub install_plan: InstallPlan,
    pub inspected_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackagePreparationStage {
    Queued,
    Validating,
    Extracting,
    Verifying,
    Activating,
    CleaningSources,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagePreparationCounters {
    pub total_bytes: u64,
    pub processed_bytes: u64,
    pub total_files: u64,
    pub processed_files: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackagePreparationProgress {
    pub operation_id: String,
    pub installation_id: crate::installation::InstallationId,
    pub stage: PackagePreparationStage,
    pub counters: PackagePreparationCounters,
    pub current_path: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedPackageInstallation {
    pub installation_id: crate::installation::InstallationId,
    pub destination_root: String,
    pub content_root: Option<RelativePath>,
    pub preferred_action: Option<PackageLaunchCandidate>,
    pub source_set: PackageSourceSet,
    pub archive_retention: ArchiveRetentionPolicy,
    pub sources_deleted: bool,
    pub source_cleanup_error: Option<String>,
    pub installed_file_count: u64,
    pub installed_bytes: u64,
    pub prepared_at: String,
}
