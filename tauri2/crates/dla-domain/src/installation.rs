use std::{collections::HashSet, fmt};

use serde::{Deserialize, Serialize};

use crate::package::PackageInspection;
use crate::scanner::{ScanMatchConfidence, ScanRootId, ScanSessionId};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct InstallationId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LaunchCandidateId(pub String);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RelativePath(String);

impl RelativePath {
    pub fn parse(value: impl Into<String>) -> Result<Self, RelativePathError> {
        let value = value.into();
        if value.is_empty() {
            return Err(RelativePathError::Empty);
        }
        if value.starts_with('/')
            || value.starts_with('\\')
            || value.contains('\\')
            || value.as_bytes().get(1) == Some(&b':')
        {
            return Err(RelativePathError::NotPortable);
        }
        if value
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            return Err(RelativePathError::UnsafeSegment);
        }
        if value.contains('\0') {
            return Err(RelativePathError::NotPortable);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RelativePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RelativePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RelativePathError {
    Empty,
    NotPortable,
    UnsafeSegment,
}

impl fmt::Display for RelativePathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("relative path is empty"),
            Self::NotPortable => formatter.write_str("relative path is absolute or not portable"),
            Self::UnsafeSegment => formatter.write_str("relative path contains an unsafe segment"),
        }
    }
}

impl std::error::Error for RelativePathError {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationPlatform {
    Windows,
    Linux,
    Macos,
    Android,
    Ios,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationStatus {
    Ready,
    NeedsReview,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Executable,
    Audio,
    Image,
    Pdf,
    Video,
    Archive,
    AndroidPackage,
    Directory,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceConfidence {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchActionKind {
    LaunchExecutable,
    PlayAudio,
    ReadImages,
    OpenDocument,
    PlayVideo,
    OpenArchive,
    OpenAndroidPackage,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(tag = "kind", content = "path", rename_all = "snake_case")]
pub enum LaunchTarget {
    InstallationRoot,
    RelativePath(RelativePath),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogIdentity {
    pub work_code: String,
    pub confidence: ScanMatchConfidence,
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentItem {
    pub relative_path: RelativePath,
    pub path_key: String,
    pub media_type: MediaType,
    pub size_bytes: Option<u64>,
    pub modified_at: Option<String>,
    pub confidence: InferenceConfidence,
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchCandidate {
    pub id: LaunchCandidateId,
    pub action: LaunchActionKind,
    pub target: LaunchTarget,
    pub supported_platforms: Vec<InstallationPlatform>,
    pub confidence: InferenceConfidence,
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualLaunchSelection {
    pub action: LaunchActionKind,
    pub target: LaunchTarget,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ManualCatalogIdentity {
    CatalogWork {
        #[serde(rename = "workCode")]
        work_code: String,
    },
    Unidentified,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentItemOverride {
    pub relative_path: RelativePath,
    pub media_type: Option<MediaType>,
    pub ignored: bool,
    pub order: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationOverrides {
    pub catalog_identity: Option<ManualCatalogIdentity>,
    pub custom_title: Option<String>,
    pub preferred_action: Option<ManualLaunchSelection>,
    pub content_items: Vec<ContentItemOverride>,
    pub reviewed_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationDetection {
    pub source_scan_session_id: Option<ScanSessionId>,
    pub catalog_identity: Option<CatalogIdentity>,
    pub suggested_status: InstallationStatus,
    pub content_items: Vec<ContentItem>,
    pub launch_candidates: Vec<LaunchCandidate>,
    #[serde(default)]
    pub package_inspection: Option<PackageInspection>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub id: InstallationId,
    pub scan_root_id: Option<ScanRootId>,
    pub root_path: String,
    pub platform: InstallationPlatform,
    pub status: InstallationStatus,
    pub detection: InstallationDetection,
    pub overrides: InstallationOverrides,
    pub discovered_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallationError {
    EmptyIdentity(&'static str),
    EmptyRootPath,
    DuplicateContentPath(String),
    DuplicateCandidateId(String),
    EmptyPathKey(String),
    EmptyReasonCodes(String),
    EmptySupportedPlatforms(String),
    MissingCandidateTarget(String),
    EmptyContentOverride(String),
    EmptyManualCatalogIdentity,
    EmptyReviewTimestamp,
    InconsistentStatus,
}

impl fmt::Display for InstallationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyIdentity(field) => write!(formatter, "{field} is required"),
            Self::EmptyRootPath => formatter.write_str("installation root path is required"),
            Self::DuplicateContentPath(path) => {
                write!(formatter, "duplicate content path: {path}")
            }
            Self::DuplicateCandidateId(id) => write!(formatter, "duplicate candidate ID: {id}"),
            Self::EmptyPathKey(path) => write!(formatter, "content path key is empty: {path}"),
            Self::EmptyReasonCodes(subject) => {
                write!(formatter, "inference reason codes are required: {subject}")
            }
            Self::EmptySupportedPlatforms(id) => {
                write!(formatter, "candidate has no supported platform: {id}")
            }
            Self::MissingCandidateTarget(path) => {
                write!(
                    formatter,
                    "candidate target is not detected content: {path}"
                )
            }
            Self::EmptyContentOverride(path) => {
                write!(formatter, "content override has no changes: {path}")
            }
            Self::EmptyManualCatalogIdentity => {
                formatter.write_str("manual catalog work code is required")
            }
            Self::EmptyReviewTimestamp => formatter.write_str("review timestamp is empty"),
            Self::InconsistentStatus => formatter
                .write_str("installation status does not match its detection and overrides"),
        }
    }
}

impl std::error::Error for InstallationError {}

impl InstallationDetection {
    pub fn validate(&self) -> Result<(), InstallationError> {
        if let Some(identity) = &self.catalog_identity {
            if identity.work_code.trim().is_empty() {
                return Err(InstallationError::EmptyIdentity("catalog work code"));
            }
            if !valid_reason_codes(&identity.reason_codes) {
                return Err(InstallationError::EmptyReasonCodes(
                    identity.work_code.clone(),
                ));
            }
        }

        let mut content_paths = HashSet::new();
        let mut relative_paths = HashSet::new();
        for item in &self.content_items {
            if item.path_key.trim().is_empty() {
                return Err(InstallationError::EmptyPathKey(
                    item.relative_path.to_string(),
                ));
            }
            if !valid_reason_codes(&item.reason_codes) {
                return Err(InstallationError::EmptyReasonCodes(
                    item.relative_path.to_string(),
                ));
            }
            if !content_paths.insert(item.path_key.clone()) {
                return Err(InstallationError::DuplicateContentPath(
                    item.relative_path.to_string(),
                ));
            }
            if !relative_paths.insert(item.relative_path.clone()) {
                return Err(InstallationError::DuplicateContentPath(
                    item.relative_path.to_string(),
                ));
            }
        }

        let mut candidate_ids = HashSet::new();
        for candidate in &self.launch_candidates {
            if candidate.id.0.trim().is_empty() {
                return Err(InstallationError::EmptyIdentity("launch candidate ID"));
            }
            if !candidate_ids.insert(candidate.id.0.clone()) {
                return Err(InstallationError::DuplicateCandidateId(
                    candidate.id.0.clone(),
                ));
            }
            if candidate.supported_platforms.is_empty() {
                return Err(InstallationError::EmptySupportedPlatforms(
                    candidate.id.0.clone(),
                ));
            }
            if !valid_reason_codes(&candidate.reason_codes) {
                return Err(InstallationError::EmptyReasonCodes(candidate.id.0.clone()));
            }
            if let LaunchTarget::RelativePath(target) = &candidate.target
                && !relative_paths.contains(target)
            {
                return Err(InstallationError::MissingCandidateTarget(
                    target.to_string(),
                ));
            }
        }

        Ok(())
    }
}

impl Installation {
    pub fn effective_catalog_work_code(&self) -> Option<&str> {
        match &self.overrides.catalog_identity {
            Some(ManualCatalogIdentity::CatalogWork { work_code }) => Some(work_code),
            Some(ManualCatalogIdentity::Unidentified) => None,
            None => self
                .detection
                .catalog_identity
                .as_ref()
                .map(|identity| identity.work_code.as_str()),
        }
    }

    pub fn validate(&self) -> Result<(), InstallationError> {
        if self.id.0.trim().is_empty() {
            return Err(InstallationError::EmptyIdentity("installation ID"));
        }
        if self.root_path.trim().is_empty() {
            return Err(InstallationError::EmptyRootPath);
        }
        self.detection.validate()?;
        self.overrides.validate()?;
        if self.status != review_status(&self.detection, &self.overrides, self.platform) {
            return Err(InstallationError::InconsistentStatus);
        }
        Ok(())
    }

    pub fn replace_detection(
        &mut self,
        detection: InstallationDetection,
        updated_at: String,
    ) -> Result<(), InstallationError> {
        detection.validate()?;
        self.status = review_status(&detection, &self.overrides, self.platform);
        self.detection = detection;
        self.updated_at = updated_at;
        Ok(())
    }

    pub fn replace_overrides(
        &mut self,
        overrides: InstallationOverrides,
        updated_at: String,
    ) -> Result<(), InstallationError> {
        overrides.validate()?;
        self.status = review_status(&self.detection, &overrides, self.platform);
        self.overrides = overrides;
        self.updated_at = updated_at;
        Ok(())
    }
}

impl InstallationOverrides {
    pub fn validate(&self) -> Result<(), InstallationError> {
        if matches!(
            &self.catalog_identity,
            Some(ManualCatalogIdentity::CatalogWork { work_code }) if work_code.trim().is_empty()
        ) {
            return Err(InstallationError::EmptyManualCatalogIdentity);
        }
        if self
            .reviewed_at
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(InstallationError::EmptyReviewTimestamp);
        }
        let mut paths = HashSet::new();
        for item in &self.content_items {
            if !paths.insert(item.relative_path.clone()) {
                return Err(InstallationError::DuplicateContentPath(
                    item.relative_path.to_string(),
                ));
            }
            if item.media_type.is_none() && !item.ignored && item.order.is_none() {
                return Err(InstallationError::EmptyContentOverride(
                    item.relative_path.to_string(),
                ));
            }
        }
        Ok(())
    }
}

fn valid_reason_codes(reason_codes: &[String]) -> bool {
    !reason_codes.is_empty() && reason_codes.iter().all(|code| !code.trim().is_empty())
}

pub fn review_status(
    detection: &InstallationDetection,
    overrides: &InstallationOverrides,
    platform: InstallationPlatform,
) -> InstallationStatus {
    if detection.catalog_identity.is_none() && overrides.catalog_identity.is_none() {
        return InstallationStatus::NeedsReview;
    }
    let Some(preferred) = &overrides.preferred_action else {
        return detection.suggested_status;
    };
    let LaunchTarget::RelativePath(target) = &preferred.target else {
        return InstallationStatus::Ready;
    };
    let target_key = match platform {
        InstallationPlatform::Windows => target.as_str().to_lowercase(),
        _ => target.as_str().to_owned(),
    };
    let target_exists = detection
        .content_items
        .iter()
        .any(|item| item.path_key == target_key);
    let target_is_ignored = overrides.content_items.iter().any(|item| {
        let override_key = match platform {
            InstallationPlatform::Windows => item.relative_path.as_str().to_lowercase(),
            _ => item.relative_path.as_str().to_owned(),
        };
        override_key == target_key && item.ignored
    });
    if target_exists && !target_is_ignored {
        InstallationStatus::Ready
    } else {
        InstallationStatus::NeedsReview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: &str) -> RelativePath {
        RelativePath::parse(value).expect("valid fixture path")
    }

    fn detection(paths: &[&str]) -> InstallationDetection {
        InstallationDetection {
            source_scan_session_id: None,
            catalog_identity: None,
            suggested_status: InstallationStatus::Ready,
            content_items: paths
                .iter()
                .map(|value| ContentItem {
                    relative_path: path(value),
                    path_key: value.to_ascii_lowercase(),
                    media_type: MediaType::Executable,
                    size_bytes: Some(2),
                    modified_at: None,
                    confidence: InferenceConfidence::High,
                    reason_codes: vec!["file_extension".to_owned()],
                })
                .collect(),
            launch_candidates: Vec::new(),
            package_inspection: None,
        }
    }

    #[test]
    fn portable_relative_paths_reject_escape_and_native_absolute_forms() {
        for value in [
            "",
            "/tmp/game.exe",
            "../game.exe",
            "dir/../game.exe",
            "C:/game.exe",
            "dir\\game.exe",
        ] {
            assert!(RelativePath::parse(value).is_err(), "accepted {value}");
        }
        assert_eq!(
            RelativePath::parse("RJ000001/Game.exe")
                .expect("portable path")
                .as_str(),
            "RJ000001/Game.exe"
        );
    }

    #[test]
    fn rescanning_preserves_a_manual_action_and_marks_a_missing_target_for_review() {
        let preferred_path = path("Game.exe");
        let overrides = InstallationOverrides {
            catalog_identity: Some(ManualCatalogIdentity::Unidentified),
            custom_title: Some("My title".to_owned()),
            preferred_action: Some(ManualLaunchSelection {
                action: LaunchActionKind::LaunchExecutable,
                target: LaunchTarget::RelativePath(preferred_path.clone()),
            }),
            content_items: vec![ContentItemOverride {
                relative_path: preferred_path,
                media_type: Some(MediaType::Executable),
                ignored: false,
                order: None,
            }],
            ..InstallationOverrides::default()
        };
        let mut installation = Installation {
            id: InstallationId("installation-1".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/library/RJ000001".to_owned(),
            platform: InstallationPlatform::Windows,
            status: InstallationStatus::Ready,
            detection: detection(&["Game.exe"]),
            overrides: overrides.clone(),
            discovered_at: "2026-08-07T00:00:00Z".to_owned(),
            updated_at: "2026-08-07T00:00:00Z".to_owned(),
        };

        installation
            .replace_detection(
                detection(&["Launcher.exe"]),
                "2026-08-07T01:00:00Z".to_owned(),
            )
            .expect("replacement detection");

        assert_eq!(installation.overrides, overrides);
        assert_eq!(installation.status, InstallationStatus::NeedsReview);
    }

    #[test]
    fn generated_candidates_must_target_detected_content() {
        let mut invalid = detection(&["Game.exe"]);
        invalid.launch_candidates.push(LaunchCandidate {
            id: LaunchCandidateId("missing-game".to_owned()),
            action: LaunchActionKind::LaunchExecutable,
            target: LaunchTarget::RelativePath(path("Missing.exe")),
            supported_platforms: vec![InstallationPlatform::Windows],
            confidence: InferenceConfidence::High,
            reason_codes: vec!["preferred_executable_name".to_owned()],
        });

        assert_eq!(
            invalid.validate(),
            Err(InstallationError::MissingCandidateTarget(
                "Missing.exe".to_owned()
            ))
        );
    }

    #[test]
    fn ignored_manual_target_requires_review() {
        let game = path("Game.exe");
        let overrides = InstallationOverrides {
            catalog_identity: Some(ManualCatalogIdentity::Unidentified),
            preferred_action: Some(ManualLaunchSelection {
                action: LaunchActionKind::LaunchExecutable,
                target: LaunchTarget::RelativePath(game.clone()),
            }),
            content_items: vec![ContentItemOverride {
                relative_path: game,
                media_type: None,
                ignored: true,
                order: None,
            }],
            ..InstallationOverrides::default()
        };

        assert_eq!(
            review_status(
                &detection(&["Game.exe"]),
                &overrides,
                InstallationPlatform::Windows
            ),
            InstallationStatus::NeedsReview
        );
    }

    #[test]
    fn windows_manual_targets_use_the_detected_path_key() {
        let overrides = InstallationOverrides {
            catalog_identity: Some(ManualCatalogIdentity::Unidentified),
            preferred_action: Some(ManualLaunchSelection {
                action: LaunchActionKind::LaunchExecutable,
                target: LaunchTarget::RelativePath(path("GAME.EXE")),
            }),
            ..InstallationOverrides::default()
        };

        assert_eq!(
            review_status(
                &detection(&["Game.exe"]),
                &overrides,
                InstallationPlatform::Windows
            ),
            InstallationStatus::Ready
        );
    }

    #[test]
    fn effective_catalog_identity_honors_manual_work_and_unidentified_overrides() {
        let mut detected = detection(&[]);
        detected.catalog_identity = Some(CatalogIdentity {
            work_code: "RJ000001".to_owned(),
            confidence: ScanMatchConfidence::Exact,
            reason_codes: vec!["archive_sha256_match".to_owned()],
        });
        let mut installation = Installation {
            id: InstallationId("installation-identity".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/library/RJ000001".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: detected,
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-08-09T00:00:00Z".to_owned(),
            updated_at: "2026-08-09T00:00:00Z".to_owned(),
        };

        assert_eq!(installation.effective_catalog_work_code(), Some("RJ000001"));
        installation.overrides.catalog_identity = Some(ManualCatalogIdentity::CatalogWork {
            work_code: "RJ000002".to_owned(),
        });
        assert_eq!(installation.effective_catalog_work_code(), Some("RJ000002"));
        installation.overrides.catalog_identity = Some(ManualCatalogIdentity::Unidentified);
        assert_eq!(installation.effective_catalog_work_code(), None);
    }
}
