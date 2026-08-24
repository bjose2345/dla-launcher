use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use dla_detection::{ProductCodeSource, detect_product_codes, resolve_scan_identities};
use dla_domain::scanner::{
    DiscoveredEntry, ResolvedScanIdentity, ScanCounters, ScanEntry, ScanEntryId, ScanEntryKind,
    ScanEntryPresence, ScanEvidence, ScanEvidenceKind, ScanEvidenceObservation, ScanIssue,
    ScanIssueCode, ScanMatchConfidence, ScanMatchOutcome, ScanProgress, ScanResult, ScanResultId,
    ScanRoot, ScanRootId, ScanSession, ScanStatus,
};

use crate::{
    identity::{ArchiveHash, ArchiveHashAlgorithm, CatalogArchiveIdentity, CatalogIdentityReader},
    scanner::{
        ArchiveHashError, ArchiveHashRequest, ArchiveHasher, FilesystemScanObserver,
        FilesystemScanRequest, FilesystemScanner, PrepareScanRequest, ScanCancellation, ScanClock,
        ScanIdentifiers, ScanIssuePage, ScanIssueRequest, ScanProgressSink, ScanRepository,
        ScanResultPage, ScanResultRequest, ScanSessionView, ScanSourceIssue, ScanWriteBatch,
        ScannerError, normalize_issue_request, normalize_result_request,
    },
};

const DEFAULT_BATCH_SIZE: usize = 32;
const ARCHIVE_CANDIDATE_LIMIT: usize = 128;
const MAXIMUM_WORKER_LIMIT: u16 = 32;

pub struct PreparedScan {
    pub root: ScanRoot,
    pub session: ScanSession,
    pub filesystem_request: FilesystemScanRequest,
}

impl PreparedScan {
    pub fn view(&self) -> ScanSessionView {
        ScanSessionView {
            root: self.root.clone(),
            session: self.session.clone(),
        }
    }
}

pub struct ScanExecutionService {
    filesystem: Arc<dyn FilesystemScanner>,
    archive_hasher: Arc<dyn ArchiveHasher>,
    catalog: Arc<dyn CatalogIdentityReader>,
    repository: Arc<dyn ScanRepository>,
    progress: Arc<dyn ScanProgressSink>,
    clock: Arc<dyn ScanClock>,
    identifiers: Arc<dyn ScanIdentifiers>,
    batch_size: usize,
}

impl ScanExecutionService {
    pub fn new(
        filesystem: Arc<dyn FilesystemScanner>,
        archive_hasher: Arc<dyn ArchiveHasher>,
        catalog: Arc<dyn CatalogIdentityReader>,
        repository: Arc<dyn ScanRepository>,
        progress: Arc<dyn ScanProgressSink>,
        clock: Arc<dyn ScanClock>,
        identifiers: Arc<dyn ScanIdentifiers>,
    ) -> Self {
        Self {
            filesystem,
            archive_hasher,
            catalog,
            repository,
            progress,
            clock,
            identifiers,
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }

    pub fn prepare(&self, request: PrepareScanRequest) -> Result<PreparedScan, ScannerError> {
        validate_prepare_request(&request)?;
        let now = self.clock.now();
        let root_id = ScanRootId(self.identifiers.stable_id(
            "root",
            &format!("{}\0{}", request.platform, request.path_key),
        ));
        let root = ScanRoot {
            id: root_id.clone(),
            platform: request.platform,
            path_key: request.path_key,
            display_path: request.display_path,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        let session = ScanSession {
            id: self.identifiers.new_session_id(),
            root_id: root_id.clone(),
            status: ScanStatus::Queued,
            options: request.options.clone(),
            counters: ScanCounters::default(),
            started_at: now,
            finished_at: None,
            fatal_error_code: None,
            fatal_error_message: None,
        };
        self.repository.save_root(&root)?;
        self.repository.begin_session(&session)?;
        self.progress.publish(&ScanProgress {
            session_id: session.id.clone(),
            status: session.status,
            counters: session.counters.clone(),
            current_relative_path: None,
        })?;

        Ok(PreparedScan {
            filesystem_request: FilesystemScanRequest {
                session_id: session.id.clone(),
                root_id,
                access_handle: request.access_handle,
                options: request.options,
            },
            root,
            session,
        })
    }

    pub fn execute(
        &self,
        mut prepared: PreparedScan,
        cancellation: &dyn ScanCancellation,
    ) -> Result<ScanSessionView, ScannerError> {
        prepared.session.status = ScanStatus::Running;
        self.repository.update_session(&prepared.session)?;
        self.progress.publish(&ScanProgress {
            session_id: prepared.session.id.clone(),
            status: prepared.session.status,
            counters: prepared.session.counters.clone(),
            current_relative_path: None,
        })?;

        let observer = ExecutionObserver {
            request: &prepared.filesystem_request,
            archive_hasher: self.archive_hasher.as_ref(),
            catalog: self.catalog.as_ref(),
            repository: self.repository.as_ref(),
            progress: self.progress.as_ref(),
            clock: self.clock.as_ref(),
            identifiers: self.identifiers.as_ref(),
            cancellation,
            batch_size: self.batch_size,
            state: Mutex::new(ExecutionState {
                session: prepared.session,
                pending_entries: Vec::new(),
                pending_results: Vec::new(),
                pending_issues: Vec::new(),
                current_relative_path: None,
            }),
        };

        let scan_result =
            self.filesystem
                .scan(&prepared.filesystem_request, &observer, cancellation);
        let terminal = match scan_result {
            Ok(_) if cancellation.is_cancelled() => TerminalScan::Cancelled,
            Ok(_) => TerminalScan::Completed,
            Err(ScannerError::Cancelled) => TerminalScan::Cancelled,
            Err(error) => TerminalScan::Failed(error),
        };
        let session = observer.finish(terminal)?;
        Ok(ScanSessionView {
            root: prepared.root,
            session,
        })
    }

    pub fn read_latest(&self) -> Result<Option<ScanSessionView>, ScannerError> {
        let Some(session) = self.repository.read_latest_session()? else {
            return Ok(None);
        };
        let root = self
            .repository
            .read_root(&session.root_id)?
            .ok_or_else(|| ScannerError::Persistence("scan root is missing".to_owned()))?;
        Ok(Some(ScanSessionView { root, session }))
    }

    pub fn browse_results(
        &self,
        request: ScanResultRequest,
    ) -> Result<ScanResultPage, ScannerError> {
        self.repository
            .browse_results(&normalize_result_request(request)?)
    }

    pub fn browse_issues(&self, request: ScanIssueRequest) -> Result<ScanIssuePage, ScannerError> {
        self.repository
            .browse_issues(&normalize_issue_request(request)?)
    }

    pub fn interrupt_active_sessions(&self) -> Result<usize, ScannerError> {
        self.repository.interrupt_active_sessions(&self.clock.now())
    }
}

enum TerminalScan {
    Completed,
    Cancelled,
    Failed(ScannerError),
}

struct ExecutionState {
    session: ScanSession,
    pending_entries: Vec<ScanEntry>,
    pending_results: Vec<ScanResult>,
    pending_issues: Vec<ScanIssue>,
    current_relative_path: Option<String>,
}

struct ExecutionObserver<'a> {
    request: &'a FilesystemScanRequest,
    archive_hasher: &'a dyn ArchiveHasher,
    catalog: &'a dyn CatalogIdentityReader,
    repository: &'a dyn ScanRepository,
    progress: &'a dyn ScanProgressSink,
    clock: &'a dyn ScanClock,
    identifiers: &'a dyn ScanIdentifiers,
    cancellation: &'a dyn ScanCancellation,
    batch_size: usize,
    state: Mutex<ExecutionState>,
}

impl FilesystemScanObserver for ExecutionObserver<'_> {
    fn discovered(&self, discovered: DiscoveredEntry) -> Result<(), ScannerError> {
        if self.cancellation.is_cancelled() {
            return Err(ScannerError::Cancelled);
        }
        let now = self.clock.now();
        let entry = self.scan_entry(discovered, &now);
        let (result, issues) = if entry.kind == ScanEntryKind::File {
            let inspection = self.inspect_file(&entry, &now)?;
            (Some(inspection.result), inspection.issues)
        } else {
            (None, Vec::new())
        };
        let mut state = self.lock_state()?;
        state.current_relative_path = Some(entry.relative_path.clone());
        match entry.kind {
            ScanEntryKind::File => state.session.counters.discovered_files += 1,
            ScanEntryKind::Directory => state.session.counters.discovered_directories += 1,
        }
        state.pending_entries.push(entry);
        if let Some(result) = result {
            state.session.counters.inspected_files += 1;
            increment_outcome(&mut state.session.counters, result.outcome);
            state.pending_results.push(result);
        }
        for issue in issues {
            if issue.recoverable {
                state.session.counters.recoverable_errors += 1;
            }
            state.pending_issues.push(issue);
        }
        self.flush_if_full(&mut state)
    }

    fn issue(&self, issue: ScanSourceIssue) -> Result<(), ScannerError> {
        let now = self.clock.now();
        let issue = self.scan_issue(issue, None, &now);
        let mut state = self.lock_state()?;
        if issue.recoverable {
            state.session.counters.recoverable_errors += 1;
        }
        state.current_relative_path = issue.relative_path.clone();
        state.pending_issues.push(issue);
        self.flush_if_full(&mut state)
    }
}

impl ExecutionObserver<'_> {
    fn scan_entry(&self, discovered: DiscoveredEntry, now: &str) -> ScanEntry {
        let id = ScanEntryId(self.identifiers.stable_id(
            "entry",
            &format!("{}\0{}", self.request.root_id.0, discovered.path_key),
        ));
        ScanEntry {
            id,
            root_id: self.request.root_id.clone(),
            relative_path: discovered.relative_path,
            path_key: discovered.path_key,
            kind: discovered.kind,
            extension: discovered.extension,
            size: discovered.size,
            modified_at: discovered.modified_at,
            presence: ScanEntryPresence::Present,
            first_seen_session_id: Some(self.request.session_id.clone()),
            last_seen_session_id: Some(self.request.session_id.clone()),
            created_at: now.to_owned(),
            updated_at: now.to_owned(),
        }
    }

    fn inspect_file(&self, entry: &ScanEntry, now: &str) -> Result<FileInspection, ScannerError> {
        let mut observations = path_observations(&entry.relative_path);
        let work_codes = observations
            .iter()
            .filter(|item| item.kind == ScanEvidenceKind::ProductCode)
            .map(|item| item.normalized_value.clone())
            .collect::<Vec<_>>();
        let works = self
            .catalog
            .read_works_by_codes(&work_codes)
            .map_err(|error| ScannerError::Catalog(error.to_string()))?;
        let mut identities = works
            .into_iter()
            .map(|work| ResolvedScanIdentity {
                reason_codes: observations
                    .iter()
                    .filter(|item| {
                        item.kind == ScanEvidenceKind::ProductCode
                            && item.normalized_value.eq_ignore_ascii_case(&work.code)
                    })
                    .map(|item| item.reason_code.clone())
                    .collect(),
                work_code: work.code,
                confidence: ScanMatchConfidence::Strong,
            })
            .collect::<Vec<_>>();
        let mut issues = Vec::new();

        if let Some(size) = entry.size.as_deref() {
            let candidates = self
                .catalog
                .find_archive_candidates_by_size(size, ARCHIVE_CANDIDATE_LIMIT)
                .map_err(|error| ScannerError::Catalog(error.to_string()))?;
            if let Some(algorithm) = strongest_candidate_algorithm(&candidates) {
                let hash_request = ArchiveHashRequest {
                    access_handle: self.request.access_handle.clone(),
                    relative_path: entry.relative_path.clone(),
                    algorithm,
                };
                match self.archive_hasher.hash(&hash_request, self.cancellation) {
                    Ok(hash) => {
                        let exact = self
                            .catalog
                            .resolve_archive_hash(&hash)
                            .map_err(|error| ScannerError::Catalog(error.to_string()))?;
                        let reason_code = hash_reason_code(hash.algorithm, !exact.is_empty());
                        observations.push(ScanEvidenceObservation {
                            kind: hash_evidence_kind(hash.algorithm),
                            normalized_value: hash.digest,
                            reason_code: reason_code.to_owned(),
                        });
                        identities.extend(exact.into_iter().map(|work| ResolvedScanIdentity {
                            work_code: work.code,
                            confidence: ScanMatchConfidence::Exact,
                            reason_codes: vec![reason_code.to_owned()],
                        }));
                    }
                    Err(ArchiveHashError::Cancelled) => return Err(ScannerError::Cancelled),
                    Err(ArchiveHashError::Source(source)) if !source.recoverable => {
                        return Err(ScannerError::RootUnavailable(source.message));
                    }
                    Err(ArchiveHashError::Source(source)) => {
                        issues.push(self.scan_issue(source, Some(entry.id.clone()), now));
                    }
                }
            }
        }

        deduplicate_observations(&mut observations);
        let decision = resolve_scan_identities(&identities);
        let result_id = ScanResultId(self.identifiers.stable_id(
            "result",
            &format!("{}\0{}", self.request.session_id.0, entry.id.0),
        ));
        let evidence = observations
            .into_iter()
            .map(|observation| ScanEvidence {
                id: self.identifiers.stable_id(
                    "evidence",
                    &format!(
                        "{}\0{:?}\0{}\0{}",
                        result_id.0,
                        observation.kind,
                        observation.normalized_value,
                        observation.reason_code
                    ),
                ),
                result_id: result_id.clone(),
                source_entry_id: Some(entry.id.clone()),
                kind: observation.kind,
                normalized_value: observation.normalized_value,
                reason_code: observation.reason_code,
                created_at: now.to_owned(),
            })
            .collect();
        Ok(FileInspection {
            result: ScanResult {
                id: result_id,
                session_id: self.request.session_id.clone(),
                candidate_entry_id: Some(entry.id.clone()),
                outcome: decision.outcome,
                selected_work_code: decision.selected_work_code,
                confidence: decision.confidence,
                candidates: decision.candidates,
                evidence,
                created_at: now.to_owned(),
                updated_at: now.to_owned(),
            },
            issues,
        })
    }

    fn scan_issue(
        &self,
        source: ScanSourceIssue,
        entry_id: Option<ScanEntryId>,
        now: &str,
    ) -> ScanIssue {
        let value = format!(
            "{}\0{:?}\0{}\0{}",
            self.request.session_id.0,
            source.code,
            source.relative_path.as_deref().unwrap_or_default(),
            source.message
        );
        ScanIssue {
            id: self.identifiers.stable_id("issue", &value),
            session_id: self.request.session_id.clone(),
            entry_id,
            relative_path: source.relative_path,
            code: source.code,
            message: source.message,
            recoverable: source.recoverable,
            created_at: now.to_owned(),
        }
    }

    fn finish(&self, terminal: TerminalScan) -> Result<ScanSession, ScannerError> {
        let mut state = self.lock_state()?;
        self.flush_locked(&mut state, false)?;
        let now = self.clock.now();
        state.session.finished_at = Some(now);
        state.current_relative_path = None;
        match terminal {
            TerminalScan::Completed => state.session.status = ScanStatus::Completed,
            TerminalScan::Cancelled => state.session.status = ScanStatus::Cancelled,
            TerminalScan::Failed(error) => {
                state.session.status = ScanStatus::Failed;
                let (code, message) = fatal_scan_error(error);
                state.session.fatal_error_code = Some(code);
                state.session.fatal_error_message = Some(message);
            }
        }
        self.repository.update_session(&state.session)?;
        self.publish_locked(&state)?;
        Ok(state.session.clone())
    }

    fn flush_if_full(&self, state: &mut ExecutionState) -> Result<(), ScannerError> {
        let pending =
            state.pending_entries.len() + state.pending_results.len() + state.pending_issues.len();
        if pending >= self.batch_size {
            self.flush_locked(state, true)?;
        }
        Ok(())
    }

    fn flush_locked(&self, state: &mut ExecutionState, publish: bool) -> Result<(), ScannerError> {
        if !state.pending_entries.is_empty()
            || !state.pending_results.is_empty()
            || !state.pending_issues.is_empty()
        {
            self.repository.record_batch(&ScanWriteBatch {
                session_id: self.request.session_id.clone(),
                entries: std::mem::take(&mut state.pending_entries),
                results: std::mem::take(&mut state.pending_results),
                issues: std::mem::take(&mut state.pending_issues),
            })?;
            self.repository.update_session(&state.session)?;
        }
        if publish {
            self.publish_locked(state)?;
        }
        Ok(())
    }

    fn publish_locked(&self, state: &ExecutionState) -> Result<(), ScannerError> {
        self.progress.publish(&ScanProgress {
            session_id: state.session.id.clone(),
            status: state.session.status,
            counters: state.session.counters.clone(),
            current_relative_path: state.current_relative_path.clone(),
        })
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, ExecutionState>, ScannerError> {
        self.state
            .lock()
            .map_err(|error| ScannerError::Persistence(error.to_string()))
    }
}

struct FileInspection {
    result: ScanResult,
    issues: Vec<ScanIssue>,
}

fn validate_prepare_request(request: &PrepareScanRequest) -> Result<(), ScannerError> {
    if request.platform.trim().is_empty()
        || request.path_key.trim().is_empty()
        || request.display_path.trim().is_empty()
        || request.access_handle.trim().is_empty()
    {
        return Err(ScannerError::InvalidRequest(
            "scan root identity and access handle are required".to_owned(),
        ));
    }
    if request.options.worker_limit == 0 || request.options.worker_limit > MAXIMUM_WORKER_LIMIT {
        return Err(ScannerError::InvalidRequest(format!(
            "worker limit must be between 1 and {MAXIMUM_WORKER_LIMIT}"
        )));
    }
    Ok(())
}

fn path_observations(relative_path: &str) -> Vec<ScanEvidenceObservation> {
    let path = Path::new(relative_path);
    let components = path.components().collect::<Vec<_>>();
    let mut observations = Vec::new();
    for component in components.iter().take(components.len().saturating_sub(1)) {
        observations.extend(detect_product_codes(
            &component.as_os_str().to_string_lossy(),
            ProductCodeSource::DirectoryName,
        ));
    }
    if let Some(name) = path.file_name() {
        let name = name.to_string_lossy();
        observations.extend(detect_product_codes(&name, ProductCodeSource::FileName));
        observations.push(ScanEvidenceObservation {
            kind: ScanEvidenceKind::Filename,
            normalized_value: name.to_lowercase(),
            reason_code: "filename_observed".to_owned(),
        });
    }
    deduplicate_observations(&mut observations);
    observations
}

fn deduplicate_observations(observations: &mut Vec<ScanEvidenceObservation>) {
    observations.sort_by(|left, right| {
        evidence_kind_order(left.kind)
            .cmp(&evidence_kind_order(right.kind))
            .then_with(|| left.normalized_value.cmp(&right.normalized_value))
            .then_with(|| left.reason_code.cmp(&right.reason_code))
    });
    observations.dedup_by(|left, right| {
        left.kind == right.kind
            && left.normalized_value == right.normalized_value
            && left.reason_code == right.reason_code
    });
}

fn evidence_kind_order(kind: ScanEvidenceKind) -> u8 {
    match kind {
        ScanEvidenceKind::ProductCode => 0,
        ScanEvidenceKind::ArchiveMd5 => 1,
        ScanEvidenceKind::ArchiveSha1 => 2,
        ScanEvidenceKind::ArchiveSha256 => 3,
        ScanEvidenceKind::Filename => 4,
    }
}

fn strongest_candidate_algorithm(
    candidates: &[CatalogArchiveIdentity],
) -> Option<ArchiveHashAlgorithm> {
    [
        ArchiveHashAlgorithm::Sha256,
        ArchiveHashAlgorithm::Sha1,
        ArchiveHashAlgorithm::Md5,
    ]
    .into_iter()
    .find(|algorithm| {
        candidates.iter().any(|candidate| {
            let value = match algorithm {
                ArchiveHashAlgorithm::Md5 => &candidate.md5,
                ArchiveHashAlgorithm::Sha1 => &candidate.sha1,
                ArchiveHashAlgorithm::Sha256 => &candidate.sha256,
            };
            ArchiveHash::parse(value).is_some_and(|hash| hash.algorithm == *algorithm)
        })
    })
}

fn hash_evidence_kind(algorithm: ArchiveHashAlgorithm) -> ScanEvidenceKind {
    match algorithm {
        ArchiveHashAlgorithm::Md5 => ScanEvidenceKind::ArchiveMd5,
        ArchiveHashAlgorithm::Sha1 => ScanEvidenceKind::ArchiveSha1,
        ArchiveHashAlgorithm::Sha256 => ScanEvidenceKind::ArchiveSha256,
    }
}

fn hash_reason_code(algorithm: ArchiveHashAlgorithm, matched: bool) -> &'static str {
    match (algorithm, matched) {
        (ArchiveHashAlgorithm::Md5, true) => "archive_md5_match",
        (ArchiveHashAlgorithm::Sha1, true) => "archive_sha1_match",
        (ArchiveHashAlgorithm::Sha256, true) => "archive_sha256_match",
        (ArchiveHashAlgorithm::Md5, false) => "archive_md5_mismatch",
        (ArchiveHashAlgorithm::Sha1, false) => "archive_sha1_mismatch",
        (ArchiveHashAlgorithm::Sha256, false) => "archive_sha256_mismatch",
    }
}

fn increment_outcome(counters: &mut ScanCounters, outcome: ScanMatchOutcome) {
    match outcome {
        ScanMatchOutcome::Matched => counters.matched += 1,
        ScanMatchOutcome::Ambiguous => counters.ambiguous += 1,
        ScanMatchOutcome::Unmatched => counters.unmatched += 1,
    }
}

fn fatal_scan_error(error: ScannerError) -> (ScanIssueCode, String) {
    let code = match error {
        ScannerError::RootUnavailable(_) => ScanIssueCode::RootUnavailable,
        ScannerError::Persistence(_) | ScannerError::Catalog(_) => ScanIssueCode::Persistence,
        ScannerError::Cancelled => ScanIssueCode::Io,
        ScannerError::Filesystem(_) | ScannerError::InvalidRequest(_) => ScanIssueCode::Io,
    };
    (code, error.to_string())
}
