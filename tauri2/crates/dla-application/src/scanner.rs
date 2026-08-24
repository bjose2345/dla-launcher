use dla_domain::scanner::{
    DiscoveredEntry, ScanCounters, ScanEntry, ScanIssue, ScanIssueCode, ScanMatchOutcome,
    ScanOptions, ScanProgress, ScanResult, ScanRoot, ScanRootId, ScanSession, ScanSessionId,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

use crate::identity::{ArchiveHash, ArchiveHashAlgorithm};

const DEFAULT_RESULT_LIMIT: usize = 60;
const MAXIMUM_RESULT_LIMIT: usize = 240;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanRootPreferenceSource {
    Configured,
    PlatformDefault,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanRootLocation {
    pub platform: String,
    pub display_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRootPreference {
    pub platform: String,
    pub display_path: Option<String>,
    pub source: ScanRootPreferenceSource,
    pub available: bool,
    pub can_prepare: bool,
}

pub trait ScanRootPreferenceRepository: Send + Sync {
    fn read_scan_root_preference(
        &self,
        platform: &str,
    ) -> Result<Option<ScanRootLocation>, ScannerError>;
    fn save_scan_root_preference(&self, location: &ScanRootLocation) -> Result<(), ScannerError>;
    fn clear_scan_root_preference(&self, platform: &str) -> Result<(), ScannerError>;
}

pub trait ScanRootLocationProvider: Send + Sync {
    fn platform(&self) -> String;
    fn default_root(&self) -> Option<ScanRootLocation>;
    fn is_directory(&self, location: &ScanRootLocation) -> bool;
    fn create_default_root(&self, location: &ScanRootLocation) -> Result<(), ScannerError>;
}

pub struct ScanRootPreferenceService {
    repository: Arc<dyn ScanRootPreferenceRepository>,
    locations: Arc<dyn ScanRootLocationProvider>,
}

impl ScanRootPreferenceService {
    pub fn new(
        repository: Arc<dyn ScanRootPreferenceRepository>,
        locations: Arc<dyn ScanRootLocationProvider>,
    ) -> Self {
        Self {
            repository,
            locations,
        }
    }

    pub fn read(&self) -> Result<ScanRootPreference, ScannerError> {
        let platform = self.locations.platform();
        let configured = self.repository.read_scan_root_preference(&platform)?;
        let (source, location) = match configured {
            Some(location) => (ScanRootPreferenceSource::Configured, Some(location)),
            None => match self.locations.default_root() {
                Some(location) => (ScanRootPreferenceSource::PlatformDefault, Some(location)),
                None => (ScanRootPreferenceSource::Unavailable, None),
            },
        };
        let available = location
            .as_ref()
            .is_some_and(|location| self.locations.is_directory(location));
        Ok(ScanRootPreference {
            platform,
            display_path: location.map(|location| location.display_path),
            source,
            available,
            can_prepare: available || source == ScanRootPreferenceSource::PlatformDefault,
        })
    }

    pub fn configure(
        &self,
        location: ScanRootLocation,
    ) -> Result<ScanRootPreference, ScannerError> {
        if location.platform != self.locations.platform() {
            return Err(ScannerError::InvalidRequest(
                "the selected scan root belongs to a different platform".to_owned(),
            ));
        }
        if !self.locations.is_directory(&location) {
            return Err(ScannerError::RootUnavailable(
                "the selected scan root is no longer available".to_owned(),
            ));
        }
        self.repository.save_scan_root_preference(&location)?;
        self.read()
    }

    pub fn reset(&self) -> Result<ScanRootPreference, ScannerError> {
        self.repository
            .clear_scan_root_preference(&self.locations.platform())?;
        self.read()
    }

    pub fn prepare(&self) -> Result<ScanRootLocation, ScannerError> {
        let preference = self.read()?;
        let location = ScanRootLocation {
            platform: preference.platform,
            display_path: preference.display_path.ok_or_else(|| {
                ScannerError::RootUnavailable(
                    "this platform does not provide a default scan root".to_owned(),
                )
            })?,
        };
        if !preference.available {
            if preference.source != ScanRootPreferenceSource::PlatformDefault {
                return Err(ScannerError::RootUnavailable(
                    "the configured scan root is no longer available; choose it again".to_owned(),
                ));
            }
            self.locations.create_default_root(&location)?;
        }
        if !self.locations.is_directory(&location) {
            return Err(ScannerError::RootUnavailable(
                "the default scan root could not be prepared".to_owned(),
            ));
        }
        Ok(location)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilesystemScanRequest {
    pub session_id: ScanSessionId,
    pub root_id: ScanRootId,
    pub access_handle: String,
    pub options: ScanOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareScanRequest {
    pub platform: String,
    pub path_key: String,
    pub display_path: String,
    pub access_handle: String,
    pub options: ScanOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveHashRequest {
    pub access_handle: String,
    pub relative_path: String,
    pub algorithm: ArchiveHashAlgorithm,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArchiveHashError {
    Cancelled,
    Source(ScanSourceIssue),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanSourceIssue {
    pub relative_path: Option<String>,
    pub code: ScanIssueCode,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanWriteBatch {
    pub session_id: ScanSessionId,
    pub entries: Vec<ScanEntry>,
    pub results: Vec<ScanResult>,
    pub issues: Vec<ScanIssue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSessionView {
    pub root: ScanRoot,
    pub session: ScanSession,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ScanResultRequest {
    pub session_id: String,
    pub outcome: Option<ScanMatchOutcome>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanResultQuery {
    pub session_id: ScanSessionId,
    pub outcome: Option<ScanMatchOutcome>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResultItem {
    pub result: ScanResult,
    pub relative_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResultPage {
    pub items: Vec<ScanResultItem>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ScanIssueRequest {
    pub session_id: String,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanIssueQuery {
    pub session_id: ScanSessionId,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanIssuePage {
    pub items: Vec<ScanIssue>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Error)]
pub enum ScannerError {
    #[error("scan root is unavailable: {0}")]
    RootUnavailable(String),
    #[error("scan was cancelled")]
    Cancelled,
    #[error("filesystem scan failed: {0}")]
    Filesystem(String),
    #[error("scan persistence failed: {0}")]
    Persistence(String),
    #[error("catalog identity lookup failed: {0}")]
    Catalog(String),
    #[error("invalid scan request: {0}")]
    InvalidRequest(String),
}

impl ScannerError {
    pub fn filesystem(error: impl std::fmt::Display) -> Self {
        Self::Filesystem(error.to_string())
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait ScanCancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

pub trait ScanProgressSink: Send + Sync {
    fn publish(&self, progress: &ScanProgress) -> Result<(), ScannerError>;
}

pub trait ScanClock: Send + Sync {
    fn now(&self) -> String;
}

pub trait ScanIdentifiers: Send + Sync {
    fn stable_id(&self, namespace: &str, value: &str) -> String;
    fn new_session_id(&self) -> ScanSessionId;
}

pub trait ArchiveHasher: Send + Sync {
    fn hash(
        &self,
        request: &ArchiveHashRequest,
        cancellation: &dyn ScanCancellation,
    ) -> Result<ArchiveHash, ArchiveHashError>;
}

pub trait FilesystemScanObserver: Send + Sync {
    fn discovered(&self, entry: DiscoveredEntry) -> Result<(), ScannerError>;
    fn issue(&self, issue: ScanSourceIssue) -> Result<(), ScannerError>;
}

pub trait FilesystemScanner: Send + Sync {
    fn scan(
        &self,
        request: &FilesystemScanRequest,
        observer: &dyn FilesystemScanObserver,
        cancellation: &dyn ScanCancellation,
    ) -> Result<ScanCounters, ScannerError>;
}

pub trait ScanRepository: Send + Sync {
    fn save_root(&self, root: &ScanRoot) -> Result<(), ScannerError>;
    fn begin_session(&self, session: &ScanSession) -> Result<(), ScannerError>;
    fn record_batch(&self, batch: &ScanWriteBatch) -> Result<(), ScannerError>;
    fn update_session(&self, session: &ScanSession) -> Result<(), ScannerError>;
    fn read_root(&self, root_id: &ScanRootId) -> Result<Option<ScanRoot>, ScannerError>;
    fn read_session(&self, session_id: &ScanSessionId)
    -> Result<Option<ScanSession>, ScannerError>;
    fn read_latest_session(&self) -> Result<Option<ScanSession>, ScannerError>;
    fn browse_results(&self, query: &ScanResultQuery) -> Result<ScanResultPage, ScannerError>;
    fn browse_issues(&self, query: &ScanIssueQuery) -> Result<ScanIssuePage, ScannerError>;
    fn interrupt_active_sessions(&self, interrupted_at: &str) -> Result<usize, ScannerError>;
}

pub fn normalize_result_request(
    request: ScanResultRequest,
) -> Result<ScanResultQuery, ScannerError> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return Err(ScannerError::InvalidRequest(
            "session ID is required".to_owned(),
        ));
    }
    Ok(ScanResultQuery {
        session_id: ScanSessionId(session_id.to_owned()),
        outcome: request.outcome,
        limit: match request.limit {
            0 => DEFAULT_RESULT_LIMIT,
            value => value.min(MAXIMUM_RESULT_LIMIT),
        },
        offset: request.offset,
    })
}

pub fn normalize_issue_request(request: ScanIssueRequest) -> Result<ScanIssueQuery, ScannerError> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return Err(ScannerError::InvalidRequest(
            "session ID is required".to_owned(),
        ));
    }
    Ok(ScanIssueQuery {
        session_id: ScanSessionId(session_id.to_owned()),
        limit: match request.limit {
            0 => DEFAULT_RESULT_LIMIT,
            value => value.min(MAXIMUM_RESULT_LIMIT),
        },
        offset: request.offset,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct MemoryPreferenceRepository {
        location: Mutex<Option<ScanRootLocation>>,
    }

    impl ScanRootPreferenceRepository for MemoryPreferenceRepository {
        fn read_scan_root_preference(
            &self,
            platform: &str,
        ) -> Result<Option<ScanRootLocation>, ScannerError> {
            Ok(self
                .location
                .lock()
                .expect("preference lock")
                .clone()
                .filter(|location| location.platform == platform))
        }

        fn save_scan_root_preference(
            &self,
            location: &ScanRootLocation,
        ) -> Result<(), ScannerError> {
            *self.location.lock().expect("preference lock") = Some(location.clone());
            Ok(())
        }

        fn clear_scan_root_preference(&self, platform: &str) -> Result<(), ScannerError> {
            let mut location = self.location.lock().expect("preference lock");
            if location
                .as_ref()
                .is_some_and(|location| location.platform == platform)
            {
                *location = None;
            }
            Ok(())
        }
    }

    struct MemoryRootLocations {
        available: Mutex<bool>,
    }

    impl ScanRootLocationProvider for MemoryRootLocations {
        fn platform(&self) -> String {
            "linux".to_owned()
        }

        fn default_root(&self) -> Option<ScanRootLocation> {
            Some(ScanRootLocation {
                platform: "linux".to_owned(),
                display_path: "/home/example/My Works".to_owned(),
            })
        }

        fn is_directory(&self, location: &ScanRootLocation) -> bool {
            location.platform == "linux"
                && location.display_path == "/home/example/My Works"
                && *self.available.lock().expect("availability lock")
        }

        fn create_default_root(&self, location: &ScanRootLocation) -> Result<(), ScannerError> {
            if location.display_path != "/home/example/My Works" {
                return Err(ScannerError::InvalidRequest(
                    "unexpected default root".to_owned(),
                ));
            }
            *self.available.lock().expect("availability lock") = true;
            Ok(())
        }
    }

    #[test]
    fn prepares_the_platform_default_only_after_an_explicit_scan_request() {
        let repository = Arc::new(MemoryPreferenceRepository {
            location: Mutex::new(None),
        });
        let locations = Arc::new(MemoryRootLocations {
            available: Mutex::new(false),
        });
        let service = ScanRootPreferenceService::new(repository, locations);

        let before = service.read().expect("read default preference");
        assert!(!before.available);
        assert!(before.can_prepare);

        let prepared = service.prepare().expect("prepare default root");
        assert_eq!(prepared.display_path, "/home/example/My Works");
        assert!(service.read().expect("read prepared preference").available);
    }

    #[test]
    fn normalizes_scan_result_pages() {
        let query = normalize_result_request(ScanResultRequest {
            session_id: "  scan-1  ".to_owned(),
            limit: usize::MAX,
            ..ScanResultRequest::default()
        })
        .expect("valid result request");

        assert_eq!(query.session_id, ScanSessionId("scan-1".to_owned()));
        assert_eq!(query.limit, MAXIMUM_RESULT_LIMIT);
    }

    #[test]
    fn rejects_a_missing_session_identity() {
        let error = normalize_result_request(ScanResultRequest::default())
            .expect_err("session identity must be required");
        assert!(matches!(error, ScannerError::InvalidRequest(_)));
    }

    #[test]
    fn normalizes_scan_issue_pages() {
        let query = normalize_issue_request(ScanIssueRequest {
            session_id: " scan-2 ".to_owned(),
            limit: usize::MAX,
            offset: 12,
        })
        .expect("valid issue request");

        assert_eq!(query.session_id, ScanSessionId("scan-2".to_owned()));
        assert_eq!(query.limit, MAXIMUM_RESULT_LIMIT);
        assert_eq!(query.offset, 12);
    }
}
