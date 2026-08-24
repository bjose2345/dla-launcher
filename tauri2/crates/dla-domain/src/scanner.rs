use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ScanRootId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ScanSessionId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ScanEntryId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ScanResultId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Queued,
    Running,
    Completed,
    Cancelled,
    Interrupted,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanEntryKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanEntryPresence {
    Present,
    Missing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanHashPolicy {
    CandidateArchives,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanEvidenceKind {
    ProductCode,
    ArchiveMd5,
    ArchiveSha1,
    ArchiveSha256,
    Filename,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMatchConfidence {
    Possible,
    Strong,
    Exact,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMatchOutcome {
    Matched,
    Ambiguous,
    Unmatched,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanIssueCode {
    RootUnavailable,
    PermissionDenied,
    EntryVanished,
    UnsupportedEntry,
    Io,
    Persistence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRoot {
    pub id: ScanRootId,
    pub platform: String,
    pub path_key: String,
    pub display_path: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOptions {
    pub follow_symlinks: bool,
    pub hash_policy: ScanHashPolicy,
    pub worker_limit: u16,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_symlinks: false,
            hash_policy: ScanHashPolicy::CandidateArchives,
            worker_limit: 4,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCounters {
    pub discovered_files: u64,
    pub discovered_directories: u64,
    pub inspected_files: u64,
    pub matched: u64,
    pub ambiguous: u64,
    pub unmatched: u64,
    pub recoverable_errors: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSession {
    pub id: ScanSessionId,
    pub root_id: ScanRootId,
    pub status: ScanStatus,
    pub options: ScanOptions,
    pub counters: ScanCounters,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub fatal_error_code: Option<ScanIssueCode>,
    pub fatal_error_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredEntry {
    pub relative_path: String,
    pub path_key: String,
    pub kind: ScanEntryKind,
    pub extension: String,
    pub size: Option<String>,
    pub modified_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEntry {
    pub id: ScanEntryId,
    pub root_id: ScanRootId,
    pub relative_path: String,
    pub path_key: String,
    pub kind: ScanEntryKind,
    pub extension: String,
    pub size: Option<String>,
    pub modified_at: Option<String>,
    pub presence: ScanEntryPresence,
    pub first_seen_session_id: Option<ScanSessionId>,
    pub last_seen_session_id: Option<ScanSessionId>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEvidenceObservation {
    pub kind: ScanEvidenceKind,
    pub normalized_value: String,
    pub reason_code: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedScanIdentity {
    pub work_code: String,
    pub confidence: ScanMatchConfidence,
    pub reason_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMatchCandidate {
    pub work_code: String,
    pub confidence: ScanMatchConfidence,
    pub reason_codes: Vec<String>,
    pub rank: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanMatchDecision {
    pub outcome: ScanMatchOutcome,
    pub selected_work_code: Option<String>,
    pub confidence: Option<ScanMatchConfidence>,
    pub candidates: Vec<ScanMatchCandidate>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEvidence {
    pub id: String,
    pub result_id: ScanResultId,
    pub source_entry_id: Option<ScanEntryId>,
    pub kind: ScanEvidenceKind,
    pub normalized_value: String,
    pub reason_code: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub id: ScanResultId,
    pub session_id: ScanSessionId,
    pub candidate_entry_id: Option<ScanEntryId>,
    pub outcome: ScanMatchOutcome,
    pub selected_work_code: Option<String>,
    pub confidence: Option<ScanMatchConfidence>,
    pub candidates: Vec<ScanMatchCandidate>,
    pub evidence: Vec<ScanEvidence>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanIssue {
    pub id: String,
    pub session_id: ScanSessionId,
    pub entry_id: Option<ScanEntryId>,
    pub relative_path: Option<String>,
    pub code: ScanIssueCode,
    pub message: String,
    pub recoverable: bool,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub session_id: ScanSessionId,
    pub status: ScanStatus,
    pub counters: ScanCounters,
    pub current_relative_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanner_defaults_never_follow_links() {
        let options = ScanOptions::default();
        assert!(!options.follow_symlinks);
        assert_eq!(options.hash_policy, ScanHashPolicy::CandidateArchives);
        assert_eq!(options.worker_limit, 4);
    }

    #[test]
    fn confidence_order_is_deterministic() {
        assert!(ScanMatchConfidence::Exact > ScanMatchConfidence::Strong);
        assert!(ScanMatchConfidence::Strong > ScanMatchConfidence::Possible);
    }
}
