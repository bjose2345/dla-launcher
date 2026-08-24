use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

pub const CATALOG_PACKAGE_FORMAT: &str = "dla.catalog-package";
pub const CATALOG_PACKAGE_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CatalogPackageProfile {
    Compact,
    Full,
    Custom,
    #[serde(rename = "complete")]
    LegacyComplete,
    #[serde(rename = "enriched")]
    LegacyEnriched,
}

impl CatalogPackageProfile {
    pub const fn canonical(self) -> Self {
        match self {
            Self::LegacyComplete | Self::LegacyEnriched => Self::Full,
            profile => profile,
        }
    }
}

impl Serialize for CatalogPackageProfile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(match self {
            Self::Compact => "compact",
            Self::Full | Self::LegacyComplete | Self::LegacyEnriched => "full",
            Self::Custom => "custom",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogPayloadKind {
    Dat,
    Enrichment,
    Relations,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CatalogPackageSource {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CatalogPackageCounts {
    pub work_entries: u64,
    pub unique_works: u64,
    pub roms: u64,
    pub files: u64,
    pub relations: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CatalogPayloadDescriptor {
    pub path: String,
    pub kind: CatalogPayloadKind,
    pub media_type: String,
    pub records: u64,
    pub uncompressed_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CatalogPackageManifest {
    pub format: String,
    pub format_version: u32,
    pub catalog_schema_version: u32,
    pub minimum_launcher_version: String,
    pub snapshot_id: String,
    pub created_at: String,
    pub profile: CatalogPackageProfile,
    pub source: CatalogPackageSource,
    pub fields: Vec<String>,
    pub counts: CatalogPackageCounts,
    pub payloads: Vec<CatalogPayloadDescriptor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogImportPreview {
    pub access_handle: String,
    pub display_name: String,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub required_disk_bytes: u64,
    pub available_disk_bytes: u64,
    pub compatible: bool,
    pub blocking_issues: Vec<String>,
    pub warnings: Vec<String>,
    pub manifest: CatalogPackageManifest,
    pub omitted_fields: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogImportStage {
    Queued,
    Validating,
    BuildingCatalog,
    ApplyingEnrichment,
    ApplyingRelations,
    FinalizingCatalog,
    CheckpointingCatalog,
    ValidatingCatalog,
    ActivatingCatalog,
    RebuildingSearch,
    Completed,
    Cancelled,
    Failed,
}

impl CatalogImportStage {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogImportCounters {
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub work_entries: u64,
    pub unique_works: u64,
    pub roms: u64,
    pub files: u64,
    pub relations: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogImportProgress {
    pub operation_id: String,
    pub operation_kind: CatalogImportOperationKind,
    pub snapshot_id: String,
    pub stage: CatalogImportStage,
    pub counters: CatalogImportCounters,
    pub current_payload: String,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogImportOperationKind {
    Import,
    Activation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogGenerationKind {
    Embedded,
    Imported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogGenerationState {
    Active,
    Available,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGenerationSummary {
    pub id: String,
    pub snapshot_id: String,
    pub kind: CatalogGenerationKind,
    pub state: CatalogGenerationState,
    pub profile: CatalogPackageProfile,
    pub source_name: String,
    pub package_name: String,
    pub imported_at: String,
    pub work_count: u64,
    pub rom_count: u64,
    pub database_bytes: u64,
    pub fields: Vec<String>,
    pub failure_detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogImportOutcome {
    pub generation: CatalogGenerationSummary,
    pub search_documents: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteCatalogImportRequest {
    pub operation_id: String,
    pub access_handle: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivateCatalogGenerationRequest {
    pub operation_id: String,
    pub generation_id: String,
}

#[derive(Debug, Error)]
pub enum CatalogImportError {
    #[error("invalid catalog package: {0}")]
    InvalidPackage(String),
    #[error("catalog package is incompatible: {0}")]
    Incompatible(String),
    #[error("catalog import was cancelled")]
    Cancelled,
    #[error("catalog import access failed: {0}")]
    Access(String),
    #[error("catalog import persistence failed: {0}")]
    Persistence(String),
    #[error("catalog import search rebuild failed: {0}")]
    Search(String),
    #[error("catalog generation was not found: {0}")]
    GenerationNotFound(String),
    #[error("the active catalog generation cannot be removed")]
    CannotRemoveActiveGeneration,
    #[error("the embedded catalog generation cannot be removed")]
    CannotRemoveEmbeddedGeneration,
    #[error("another catalog operation is already running")]
    AlreadyRunning,
}

impl CatalogImportError {
    pub fn invalid(error: impl std::fmt::Display) -> Self {
        Self::InvalidPackage(error.to_string())
    }

    pub fn access(error: impl std::fmt::Display) -> Self {
        Self::Access(error.to_string())
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

#[derive(Clone, Default)]
pub struct CatalogImportCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CatalogImportCancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

pub trait CatalogImportProgressSink: Send + Sync {
    fn publish(&self, progress: &CatalogImportProgress) -> Result<(), CatalogImportError>;
}

pub trait CatalogImporter: Send + Sync {
    fn inspect(&self, access_handle: &str) -> Result<CatalogImportPreview, CatalogImportError>;
    fn execute(
        &self,
        request: ExecuteCatalogImportRequest,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError>;
    fn list_generations(&self) -> Result<Vec<CatalogGenerationSummary>, CatalogImportError>;
    fn remove_generation(&self, generation_id: &str) -> Result<(), CatalogImportError>;
    fn activate(
        &self,
        request: ActivateCatalogGenerationRequest,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError>;
}

pub struct CatalogImportService {
    importer: Arc<dyn CatalogImporter>,
}

impl CatalogImportService {
    pub fn new(importer: Arc<dyn CatalogImporter>) -> Self {
        Self { importer }
    }

    pub fn inspect(&self, access_handle: &str) -> Result<CatalogImportPreview, CatalogImportError> {
        self.importer.inspect(access_handle)
    }

    pub fn execute(
        &self,
        request: ExecuteCatalogImportRequest,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError> {
        self.importer.execute(request, cancellation, progress)
    }

    pub fn list_generations(&self) -> Result<Vec<CatalogGenerationSummary>, CatalogImportError> {
        self.importer.list_generations()
    }

    pub fn remove_generation(&self, generation_id: &str) -> Result<(), CatalogImportError> {
        self.importer.remove_generation(generation_id)
    }

    pub fn activate(
        &self,
        request: ActivateCatalogGenerationRequest,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError> {
        self.importer.activate(request, cancellation, progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_import_stages_are_explicit() {
        assert!(CatalogImportStage::Completed.is_terminal());
        assert!(CatalogImportStage::Cancelled.is_terminal());
        assert!(CatalogImportStage::Failed.is_terminal());
        assert!(!CatalogImportStage::RebuildingSearch.is_terminal());
    }
}
