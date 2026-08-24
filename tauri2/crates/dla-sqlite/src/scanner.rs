use std::io;

use dla_application::{
    installation_from_scan::{
        InstallationScanResultScope, InstallationScanSelection, InstallationScanSource,
        InstallationScanSourceError,
    },
    scanner::{
        ScanIssuePage, ScanIssueQuery, ScanRepository, ScanResultItem, ScanResultPage,
        ScanResultQuery, ScanRootLocation, ScanRootPreferenceRepository, ScanWriteBatch,
        ScannerError,
    },
};
use dla_domain::scanner::{
    ScanCounters, ScanEntry, ScanEntryId, ScanEntryKind, ScanEntryPresence, ScanEvidence,
    ScanEvidenceKind, ScanHashPolicy, ScanIssue, ScanIssueCode, ScanMatchCandidate,
    ScanMatchConfidence, ScanMatchOutcome, ScanOptions, ScanResult, ScanResultId, ScanRoot,
    ScanRootId, ScanSession, ScanSessionId, ScanStatus,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, types::Type};

use crate::SqliteLibraryStore;

impl ScanRootPreferenceRepository for SqliteLibraryStore {
    fn read_scan_root_preference(
        &self,
        platform: &str,
    ) -> Result<Option<ScanRootLocation>, ScannerError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT platform, display_path
                     FROM library_scanner_preference
                     WHERE platform = ?1",
                    [platform],
                    |row| {
                        Ok(ScanRootLocation {
                            platform: row.get(0)?,
                            display_path: row.get(1)?,
                        })
                    },
                )
                .optional()
        })
        .map_err(ScannerError::persistence)
    }

    fn save_scan_root_preference(&self, location: &ScanRootLocation) -> Result<(), ScannerError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO library_scanner_preference (platform, display_path, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(platform) DO UPDATE SET
                    display_path = excluded.display_path,
                    updated_at = excluded.updated_at",
                params![
                    location.platform,
                    location.display_path,
                    crate::database::now_rfc3339(),
                ],
            )?;
            Ok(())
        })
        .map_err(ScannerError::persistence)
    }

    fn clear_scan_root_preference(&self, platform: &str) -> Result<(), ScannerError> {
        self.with_connection(|connection| {
            connection.execute(
                "DELETE FROM library_scanner_preference WHERE platform = ?1",
                [platform],
            )?;
            Ok(())
        })
        .map_err(ScannerError::persistence)
    }
}

impl ScanRepository for SqliteLibraryStore {
    fn save_root(&self, root: &ScanRoot) -> Result<(), ScannerError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO library_scan_root
                 (root_id, platform, path_key, display_path, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(root_id) DO UPDATE SET
                    platform = excluded.platform,
                    path_key = excluded.path_key,
                    display_path = excluded.display_path,
                    updated_at = excluded.updated_at",
                params![
                    root.id.0,
                    root.platform,
                    root.path_key,
                    root.display_path,
                    root.created_at,
                    root.updated_at,
                ],
            )?;
            Ok(())
        })
        .map_err(ScannerError::persistence)
    }

    fn begin_session(&self, session: &ScanSession) -> Result<(), ScannerError> {
        self.with_connection(|connection| insert_scan_session(connection, session))
            .map_err(ScannerError::persistence)
    }

    fn record_batch(&self, batch: &ScanWriteBatch) -> Result<(), ScannerError> {
        validate_batch(batch)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            for entry in &batch.entries {
                upsert_scan_entry(&transaction, entry)?;
            }
            for result in &batch.results {
                upsert_scan_result(&transaction, result)?;
            }
            for issue in &batch.issues {
                upsert_scan_issue(&transaction, issue)?;
            }
            transaction.commit()
        })
        .map_err(ScannerError::persistence)
    }

    fn update_session(&self, session: &ScanSession) -> Result<(), ScannerError> {
        self.with_connection(|connection| {
            let changed = connection.execute(
                "UPDATE library_scan_session SET
                    status = ?2,
                    follow_symlinks = ?3,
                    hash_policy = ?4,
                    worker_limit = ?5,
                    discovered_files = ?6,
                    discovered_directories = ?7,
                    inspected_files = ?8,
                    matched = ?9,
                    ambiguous = ?10,
                    unmatched = ?11,
                    recoverable_errors = ?12,
                    finished_at = ?13,
                    fatal_error_code = ?14,
                    fatal_error_message = ?15
                 WHERE session_id = ?1",
                params![
                    session.id.0,
                    scan_status(session.status),
                    session.options.follow_symlinks,
                    scan_hash_policy(session.options.hash_policy),
                    i64::from(session.options.worker_limit),
                    sqlite_u64(session.counters.discovered_files)?,
                    sqlite_u64(session.counters.discovered_directories)?,
                    sqlite_u64(session.counters.inspected_files)?,
                    sqlite_u64(session.counters.matched)?,
                    sqlite_u64(session.counters.ambiguous)?,
                    sqlite_u64(session.counters.unmatched)?,
                    sqlite_u64(session.counters.recoverable_errors)?,
                    session.finished_at,
                    session.fatal_error_code.map(scan_issue_code),
                    session.fatal_error_message,
                ],
            )?;
            if changed == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            Ok(())
        })
        .map_err(ScannerError::persistence)
    }

    fn read_root(&self, root_id: &ScanRootId) -> Result<Option<ScanRoot>, ScannerError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT root_id, platform, path_key, display_path, created_at, updated_at
                     FROM library_scan_root
                     WHERE root_id = ?1",
                    params![root_id.0],
                    |row| {
                        Ok(ScanRoot {
                            id: ScanRootId(row.get(0)?),
                            platform: row.get(1)?,
                            path_key: row.get(2)?,
                            display_path: row.get(3)?,
                            created_at: row.get(4)?,
                            updated_at: row.get(5)?,
                        })
                    },
                )
                .optional()
        })
        .map_err(ScannerError::persistence)
    }

    fn read_session(
        &self,
        session_id: &ScanSessionId,
    ) -> Result<Option<ScanSession>, ScannerError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    &format!("{} WHERE session_id = ?1", scan_session_select()),
                    params![session_id.0],
                    read_scan_session,
                )
                .optional()
        })
        .map_err(ScannerError::persistence)
    }

    fn read_latest_session(&self) -> Result<Option<ScanSession>, ScannerError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    &format!(
                        "{} ORDER BY started_at DESC, session_id DESC LIMIT 1",
                        scan_session_select()
                    ),
                    [],
                    read_scan_session,
                )
                .optional()
        })
        .map_err(ScannerError::persistence)
    }

    fn browse_results(&self, query: &ScanResultQuery) -> Result<ScanResultPage, ScannerError> {
        self.with_connection(|connection| read_scan_results(connection, query))
            .map_err(ScannerError::persistence)
    }

    fn browse_issues(&self, query: &ScanIssueQuery) -> Result<ScanIssuePage, ScannerError> {
        self.with_connection(|connection| read_scan_issues(connection, query))
            .map_err(ScannerError::persistence)
    }

    fn interrupt_active_sessions(&self, interrupted_at: &str) -> Result<usize, ScannerError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE library_scan_session
                 SET status = 'interrupted', finished_at = ?1
                 WHERE status IN ('queued', 'running')",
                params![interrupted_at],
            )
        })
        .map_err(ScannerError::persistence)
    }
}

impl InstallationScanSource for SqliteLibraryStore {
    fn load(
        &self,
        session_id: &ScanSessionId,
        selected_result_id: &ScanResultId,
    ) -> Result<Option<InstallationScanSelection>, InstallationScanSourceError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let selection =
                read_installation_scan_selection(&transaction, session_id, selected_result_id)?;
            transaction.commit()?;
            Ok(selection)
        })
        .map_err(InstallationScanSourceError::persistence)
    }
}

fn read_installation_scan_selection(
    connection: &Connection,
    session_id: &ScanSessionId,
    selected_result_id: &ScanResultId,
) -> rusqlite::Result<Option<InstallationScanSelection>> {
    let Some(session) = connection
        .query_row(
            &format!("{} WHERE session_id = ?1", scan_session_select()),
            params![session_id.0],
            read_scan_session,
        )
        .optional()?
    else {
        return Ok(None);
    };
    let root = connection.query_row(
        "SELECT root_id, platform, path_key, display_path, created_at, updated_at
         FROM library_scan_root
         WHERE root_id = ?1",
        params![session.root_id.0],
        |row| {
            Ok(ScanRoot {
                id: ScanRootId(row.get(0)?),
                platform: row.get(1)?,
                path_key: row.get(2)?,
                display_path: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        },
    )?;
    let mut selected_results = read_scan_result_rows(
        connection,
        "WHERE result.result_id = ?1",
        params![selected_result_id.0],
    )?;
    let Some(mut selected_result) = selected_results.pop().map(|item| item.result) else {
        return Ok(None);
    };
    selected_result.candidates = read_scan_candidates(connection, &selected_result.id)?;
    selected_result.evidence = read_scan_evidence(connection, &selected_result.id)?;

    let entries = read_session_entries(connection, &session)?;
    let result_scopes = read_session_result_scopes(connection, &session.id)?;
    Ok(Some(InstallationScanSelection {
        root,
        session,
        selected_result,
        entries,
        result_scopes,
    }))
}

fn read_session_entries(
    connection: &Connection,
    session: &ScanSession,
) -> rusqlite::Result<Vec<ScanEntry>> {
    let mut statement = connection.prepare(
        "SELECT entry_id, root_id, relative_path, path_key, kind, extension, size, modified_at,
                presence, first_seen_session_id, last_seen_session_id, created_at, updated_at
         FROM library_scan_entry
         WHERE root_id = ?1 AND presence = 'present' AND last_seen_session_id = ?2
         ORDER BY path_key, entry_id",
    )?;
    let rows = statement.query_map(params![session.root_id.0, session.id.0], |row| {
        Ok(ScanEntry {
            id: ScanEntryId(row.get(0)?),
            root_id: ScanRootId(row.get(1)?),
            relative_path: row.get(2)?,
            path_key: row.get(3)?,
            kind: parse_scan_entry_kind(&row.get::<_, String>(4)?)?,
            extension: row.get(5)?,
            size: row.get(6)?,
            modified_at: row.get(7)?,
            presence: parse_scan_entry_presence(&row.get::<_, String>(8)?)?,
            first_seen_session_id: row.get::<_, Option<String>>(9)?.map(ScanSessionId),
            last_seen_session_id: row.get::<_, Option<String>>(10)?.map(ScanSessionId),
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    })?;
    rows.collect()
}

fn read_session_result_scopes(
    connection: &Connection,
    session_id: &ScanSessionId,
) -> rusqlite::Result<Vec<InstallationScanResultScope>> {
    let mut statement = connection.prepare(
        "SELECT candidate_entry_id,
                CASE WHEN outcome = 'matched' THEN upper(trim(selected_work_code)) END
         FROM library_scan_result
         WHERE session_id = ?1 AND candidate_entry_id IS NOT NULL
         ORDER BY candidate_entry_id, result_id",
    )?;
    let rows = statement.query_map(params![session_id.0], |row| {
        Ok(InstallationScanResultScope {
            candidate_entry_id: ScanEntryId(row.get(0)?),
            matched_work_code: row.get(1)?,
        })
    })?;
    rows.collect()
}

fn validate_batch(batch: &ScanWriteBatch) -> Result<(), ScannerError> {
    if batch
        .entries
        .iter()
        .any(|entry| entry.last_seen_session_id.as_ref() != Some(&batch.session_id))
        || batch
            .results
            .iter()
            .any(|result| result.session_id != batch.session_id)
        || batch
            .issues
            .iter()
            .any(|issue| issue.session_id != batch.session_id)
        || batch.results.iter().any(|result| {
            result
                .evidence
                .iter()
                .any(|evidence| evidence.result_id != result.id)
        })
    {
        return Err(ScannerError::InvalidRequest(
            "scan batch identities do not share one session and result scope".to_owned(),
        ));
    }
    Ok(())
}

fn insert_scan_session(connection: &Connection, session: &ScanSession) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO library_scan_session
         (session_id, root_id, status, follow_symlinks, hash_policy, worker_limit,
          discovered_files, discovered_directories, inspected_files, matched, ambiguous,
          unmatched, recoverable_errors, started_at, finished_at, fatal_error_code,
          fatal_error_message)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            session.id.0,
            session.root_id.0,
            scan_status(session.status),
            session.options.follow_symlinks,
            scan_hash_policy(session.options.hash_policy),
            i64::from(session.options.worker_limit),
            sqlite_u64(session.counters.discovered_files)?,
            sqlite_u64(session.counters.discovered_directories)?,
            sqlite_u64(session.counters.inspected_files)?,
            sqlite_u64(session.counters.matched)?,
            sqlite_u64(session.counters.ambiguous)?,
            sqlite_u64(session.counters.unmatched)?,
            sqlite_u64(session.counters.recoverable_errors)?,
            session.started_at,
            session.finished_at,
            session.fatal_error_code.map(scan_issue_code),
            session.fatal_error_message,
        ],
    )?;
    Ok(())
}

fn upsert_scan_entry(transaction: &Transaction<'_>, entry: &ScanEntry) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO library_scan_entry
         (entry_id, root_id, relative_path, path_key, kind, extension, size, modified_at,
          presence, first_seen_session_id, last_seen_session_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(entry_id) DO UPDATE SET
            root_id = excluded.root_id,
            relative_path = excluded.relative_path,
            path_key = excluded.path_key,
            kind = excluded.kind,
            extension = excluded.extension,
            size = excluded.size,
            modified_at = excluded.modified_at,
            presence = excluded.presence,
            first_seen_session_id = coalesce(library_scan_entry.first_seen_session_id, excluded.first_seen_session_id),
            last_seen_session_id = excluded.last_seen_session_id,
            updated_at = excluded.updated_at",
        params![
            entry.id.0,
            entry.root_id.0,
            entry.relative_path,
            entry.path_key,
            scan_entry_kind(entry.kind),
            entry.extension,
            entry.size,
            entry.modified_at,
            scan_entry_presence(entry.presence),
            entry.first_seen_session_id.as_ref().map(|id| &id.0),
            entry.last_seen_session_id.as_ref().map(|id| &id.0),
            entry.created_at,
            entry.updated_at,
        ],
    )?;
    Ok(())
}

fn upsert_scan_result(transaction: &Transaction<'_>, result: &ScanResult) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO library_scan_result
         (result_id, session_id, candidate_entry_id, outcome, selected_work_code,
          confidence, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(result_id) DO UPDATE SET
            session_id = excluded.session_id,
            candidate_entry_id = excluded.candidate_entry_id,
            outcome = excluded.outcome,
            selected_work_code = excluded.selected_work_code,
            confidence = excluded.confidence,
            updated_at = excluded.updated_at",
        params![
            result.id.0,
            result.session_id.0,
            result.candidate_entry_id.as_ref().map(|id| &id.0),
            scan_match_outcome(result.outcome),
            result.selected_work_code,
            result.confidence.map(scan_match_confidence),
            result.created_at,
            result.updated_at,
        ],
    )?;
    transaction.execute(
        "DELETE FROM library_scan_candidate WHERE result_id = ?1",
        params![result.id.0],
    )?;
    transaction.execute(
        "DELETE FROM library_scan_evidence WHERE result_id = ?1",
        params![result.id.0],
    )?;
    for candidate in &result.candidates {
        let reason_codes = serde_json::to_string(&candidate.reason_codes)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
        transaction.execute(
            "INSERT INTO library_scan_candidate
             (result_id, work_code, confidence, reason_codes, rank)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                result.id.0,
                candidate.work_code,
                scan_match_confidence(candidate.confidence),
                reason_codes,
                i64::from(candidate.rank),
            ],
        )?;
    }
    for evidence in &result.evidence {
        transaction.execute(
            "INSERT INTO library_scan_evidence
             (evidence_id, result_id, source_entry_id, kind, normalized_value,
              reason_code, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                evidence.id,
                result.id.0,
                evidence.source_entry_id.as_ref().map(|id| &id.0),
                scan_evidence_kind(evidence.kind),
                evidence.normalized_value,
                evidence.reason_code,
                evidence.created_at,
            ],
        )?;
    }
    Ok(())
}

fn upsert_scan_issue(transaction: &Transaction<'_>, issue: &ScanIssue) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO library_scan_issue
         (issue_id, session_id, entry_id, relative_path, code, message, recoverable, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(issue_id) DO UPDATE SET
            entry_id = excluded.entry_id,
            relative_path = excluded.relative_path,
            code = excluded.code,
            message = excluded.message,
            recoverable = excluded.recoverable",
        params![
            issue.id,
            issue.session_id.0,
            issue.entry_id.as_ref().map(|id| &id.0),
            issue.relative_path,
            scan_issue_code(issue.code),
            issue.message,
            issue.recoverable,
            issue.created_at,
        ],
    )?;
    Ok(())
}

fn read_scan_results(
    connection: &Connection,
    query: &ScanResultQuery,
) -> rusqlite::Result<ScanResultPage> {
    let outcome = query.outcome.map(scan_match_outcome);
    let total = if let Some(outcome) = outcome {
        connection.query_row(
            "SELECT count(*) FROM library_scan_result WHERE session_id = ?1 AND outcome = ?2",
            params![query.session_id.0, outcome],
            |row| row.get::<_, i64>(0),
        )?
    } else {
        connection.query_row(
            "SELECT count(*) FROM library_scan_result WHERE session_id = ?1",
            params![query.session_id.0],
            |row| row.get::<_, i64>(0),
        )?
    };
    let total = usize::try_from(total).map_err(|error| invalid_text(error.to_string()))?;
    let limit = sqlite_usize(query.limit)?;
    let offset = sqlite_usize(query.offset)?;

    let mut results = if let Some(outcome) = outcome {
        read_scan_result_rows(
            connection,
            "WHERE result.session_id = ?1 AND result.outcome = ?2 ORDER BY result.updated_at DESC, result.result_id LIMIT ?3 OFFSET ?4",
            params![query.session_id.0, outcome, limit, offset],
        )?
    } else {
        read_scan_result_rows(
            connection,
            "WHERE result.session_id = ?1 ORDER BY result.updated_at DESC, result.result_id LIMIT ?2 OFFSET ?3",
            params![query.session_id.0, limit, offset],
        )?
    };
    for item in &mut results {
        item.result.candidates = read_scan_candidates(connection, &item.result.id)?;
        item.result.evidence = read_scan_evidence(connection, &item.result.id)?;
    }
    Ok(ScanResultPage {
        items: results,
        total,
        limit: query.limit,
        offset: query.offset,
    })
}

fn read_scan_result_rows<P>(
    connection: &Connection,
    clause: &str,
    parameters: P,
) -> rusqlite::Result<Vec<ScanResultItem>>
where
    P: rusqlite::Params,
{
    let mut statement = connection.prepare(&format!(
        "SELECT result.result_id, result.session_id, result.candidate_entry_id, result.outcome,
                result.selected_work_code, result.confidence, result.created_at, result.updated_at,
                entry.relative_path
         FROM library_scan_result result
         LEFT JOIN library_scan_entry entry ON entry.entry_id = result.candidate_entry_id
         {clause}"
    ))?;
    let rows = statement.query_map(parameters, |row| {
        Ok(ScanResultItem {
            result: ScanResult {
                id: ScanResultId(row.get(0)?),
                session_id: ScanSessionId(row.get(1)?),
                candidate_entry_id: row.get::<_, Option<String>>(2)?.map(ScanEntryId),
                outcome: parse_scan_match_outcome(&row.get::<_, String>(3)?)?,
                selected_work_code: row.get(4)?,
                confidence: row
                    .get::<_, Option<String>>(5)?
                    .map(|value| parse_scan_match_confidence(&value))
                    .transpose()?,
                candidates: Vec::new(),
                evidence: Vec::new(),
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            },
            relative_path: row.get(8)?,
        })
    })?;
    rows.collect()
}

fn read_scan_issues(
    connection: &Connection,
    query: &ScanIssueQuery,
) -> rusqlite::Result<ScanIssuePage> {
    let total = connection.query_row(
        "SELECT count(*) FROM library_scan_issue WHERE session_id = ?1",
        params![query.session_id.0],
        |row| row.get::<_, i64>(0),
    )?;
    let total = usize::try_from(total).map_err(|error| invalid_text(error.to_string()))?;
    let mut statement = connection.prepare(
        "SELECT issue_id, entry_id, relative_path, code, message, recoverable, created_at
         FROM library_scan_issue
         WHERE session_id = ?1
         ORDER BY created_at DESC, issue_id
         LIMIT ?2 OFFSET ?3",
    )?;
    let rows = statement.query_map(
        params![
            query.session_id.0,
            sqlite_usize(query.limit)?,
            sqlite_usize(query.offset)?
        ],
        |row| {
            Ok(ScanIssue {
                id: row.get(0)?,
                session_id: query.session_id.clone(),
                entry_id: row.get::<_, Option<String>>(1)?.map(ScanEntryId),
                relative_path: row.get(2)?,
                code: parse_scan_issue_code(&row.get::<_, String>(3)?)?,
                message: row.get(4)?,
                recoverable: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )?;
    Ok(ScanIssuePage {
        items: rows.collect::<rusqlite::Result<Vec<_>>>()?,
        total,
        limit: query.limit,
        offset: query.offset,
    })
}

fn read_scan_candidates(
    connection: &Connection,
    result_id: &ScanResultId,
) -> rusqlite::Result<Vec<ScanMatchCandidate>> {
    let mut statement = connection.prepare(
        "SELECT work_code, confidence, reason_codes, rank
         FROM library_scan_candidate
         WHERE result_id = ?1
         ORDER BY rank",
    )?;
    let rows = statement.query_map(params![result_id.0], |row| {
        let reason_codes = serde_json::from_str(&row.get::<_, String>(2)?)
            .map_err(|error| invalid_text(error.to_string()))?;
        Ok(ScanMatchCandidate {
            work_code: row.get(0)?,
            confidence: parse_scan_match_confidence(&row.get::<_, String>(1)?)?,
            reason_codes,
            rank: read_u32(row, 3)?,
        })
    })?;
    rows.collect()
}

fn read_scan_evidence(
    connection: &Connection,
    result_id: &ScanResultId,
) -> rusqlite::Result<Vec<ScanEvidence>> {
    let mut statement = connection.prepare(
        "SELECT evidence_id, source_entry_id, kind, normalized_value, reason_code, created_at
         FROM library_scan_evidence
         WHERE result_id = ?1
         ORDER BY evidence_id",
    )?;
    let rows = statement.query_map(params![result_id.0], |row| {
        Ok(ScanEvidence {
            id: row.get(0)?,
            result_id: result_id.clone(),
            source_entry_id: row.get::<_, Option<String>>(1)?.map(ScanEntryId),
            kind: parse_scan_evidence_kind(&row.get::<_, String>(2)?)?,
            normalized_value: row.get(3)?,
            reason_code: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    rows.collect()
}

fn scan_session_select() -> &'static str {
    "SELECT session_id, root_id, status, follow_symlinks, hash_policy, worker_limit,
            discovered_files, discovered_directories, inspected_files, matched, ambiguous,
            unmatched, recoverable_errors, started_at, finished_at, fatal_error_code,
            fatal_error_message
     FROM library_scan_session"
}

fn read_scan_session(row: &Row<'_>) -> rusqlite::Result<ScanSession> {
    Ok(ScanSession {
        id: ScanSessionId(row.get(0)?),
        root_id: ScanRootId(row.get(1)?),
        status: parse_scan_status(&row.get::<_, String>(2)?)?,
        options: ScanOptions {
            follow_symlinks: row.get(3)?,
            hash_policy: parse_scan_hash_policy(&row.get::<_, String>(4)?)?,
            worker_limit: read_u16(row, 5)?,
        },
        counters: ScanCounters {
            discovered_files: read_u64(row, 6)?,
            discovered_directories: read_u64(row, 7)?,
            inspected_files: read_u64(row, 8)?,
            matched: read_u64(row, 9)?,
            ambiguous: read_u64(row, 10)?,
            unmatched: read_u64(row, 11)?,
            recoverable_errors: read_u64(row, 12)?,
        },
        started_at: row.get(13)?,
        finished_at: row.get(14)?,
        fatal_error_code: row
            .get::<_, Option<String>>(15)?
            .map(|value| parse_scan_issue_code(&value))
            .transpose()?,
        fatal_error_message: row.get(16)?,
    })
}

fn scan_status(value: ScanStatus) -> &'static str {
    match value {
        ScanStatus::Queued => "queued",
        ScanStatus::Running => "running",
        ScanStatus::Completed => "completed",
        ScanStatus::Cancelled => "cancelled",
        ScanStatus::Interrupted => "interrupted",
        ScanStatus::Failed => "failed",
    }
}

fn parse_scan_status(value: &str) -> rusqlite::Result<ScanStatus> {
    match value {
        "queued" => Ok(ScanStatus::Queued),
        "running" => Ok(ScanStatus::Running),
        "completed" => Ok(ScanStatus::Completed),
        "cancelled" => Ok(ScanStatus::Cancelled),
        "interrupted" => Ok(ScanStatus::Interrupted),
        "failed" => Ok(ScanStatus::Failed),
        value => Err(invalid_text(value)),
    }
}

fn scan_entry_kind(value: ScanEntryKind) -> &'static str {
    match value {
        ScanEntryKind::File => "file",
        ScanEntryKind::Directory => "directory",
    }
}

fn parse_scan_entry_kind(value: &str) -> rusqlite::Result<ScanEntryKind> {
    match value {
        "file" => Ok(ScanEntryKind::File),
        "directory" => Ok(ScanEntryKind::Directory),
        value => Err(invalid_text(value)),
    }
}

fn scan_entry_presence(value: ScanEntryPresence) -> &'static str {
    match value {
        ScanEntryPresence::Present => "present",
        ScanEntryPresence::Missing => "missing",
    }
}

fn parse_scan_entry_presence(value: &str) -> rusqlite::Result<ScanEntryPresence> {
    match value {
        "present" => Ok(ScanEntryPresence::Present),
        "missing" => Ok(ScanEntryPresence::Missing),
        value => Err(invalid_text(value)),
    }
}

fn scan_hash_policy(value: ScanHashPolicy) -> &'static str {
    match value {
        ScanHashPolicy::CandidateArchives => "candidate_archives",
    }
}

fn parse_scan_hash_policy(value: &str) -> rusqlite::Result<ScanHashPolicy> {
    match value {
        "candidate_archives" => Ok(ScanHashPolicy::CandidateArchives),
        value => Err(invalid_text(value)),
    }
}

fn scan_evidence_kind(value: ScanEvidenceKind) -> &'static str {
    match value {
        ScanEvidenceKind::ProductCode => "product_code",
        ScanEvidenceKind::ArchiveMd5 => "archive_md5",
        ScanEvidenceKind::ArchiveSha1 => "archive_sha1",
        ScanEvidenceKind::ArchiveSha256 => "archive_sha256",
        ScanEvidenceKind::Filename => "filename",
    }
}

fn parse_scan_evidence_kind(value: &str) -> rusqlite::Result<ScanEvidenceKind> {
    match value {
        "product_code" => Ok(ScanEvidenceKind::ProductCode),
        "archive_md5" => Ok(ScanEvidenceKind::ArchiveMd5),
        "archive_sha1" => Ok(ScanEvidenceKind::ArchiveSha1),
        "archive_sha256" => Ok(ScanEvidenceKind::ArchiveSha256),
        "filename" => Ok(ScanEvidenceKind::Filename),
        value => Err(invalid_text(value)),
    }
}

fn scan_match_confidence(value: ScanMatchConfidence) -> &'static str {
    match value {
        ScanMatchConfidence::Possible => "possible",
        ScanMatchConfidence::Strong => "strong",
        ScanMatchConfidence::Exact => "exact",
    }
}

fn parse_scan_match_confidence(value: &str) -> rusqlite::Result<ScanMatchConfidence> {
    match value {
        "possible" => Ok(ScanMatchConfidence::Possible),
        "strong" => Ok(ScanMatchConfidence::Strong),
        "exact" => Ok(ScanMatchConfidence::Exact),
        value => Err(invalid_text(value)),
    }
}

fn scan_match_outcome(value: ScanMatchOutcome) -> &'static str {
    match value {
        ScanMatchOutcome::Matched => "matched",
        ScanMatchOutcome::Ambiguous => "ambiguous",
        ScanMatchOutcome::Unmatched => "unmatched",
    }
}

fn parse_scan_match_outcome(value: &str) -> rusqlite::Result<ScanMatchOutcome> {
    match value {
        "matched" => Ok(ScanMatchOutcome::Matched),
        "ambiguous" => Ok(ScanMatchOutcome::Ambiguous),
        "unmatched" => Ok(ScanMatchOutcome::Unmatched),
        value => Err(invalid_text(value)),
    }
}

fn scan_issue_code(value: ScanIssueCode) -> &'static str {
    match value {
        ScanIssueCode::RootUnavailable => "root_unavailable",
        ScanIssueCode::PermissionDenied => "permission_denied",
        ScanIssueCode::EntryVanished => "entry_vanished",
        ScanIssueCode::UnsupportedEntry => "unsupported_entry",
        ScanIssueCode::Io => "io",
        ScanIssueCode::Persistence => "persistence",
    }
}

fn parse_scan_issue_code(value: &str) -> rusqlite::Result<ScanIssueCode> {
    match value {
        "root_unavailable" => Ok(ScanIssueCode::RootUnavailable),
        "permission_denied" => Ok(ScanIssueCode::PermissionDenied),
        "entry_vanished" => Ok(ScanIssueCode::EntryVanished),
        "unsupported_entry" => Ok(ScanIssueCode::UnsupportedEntry),
        "io" => Ok(ScanIssueCode::Io),
        "persistence" => Ok(ScanIssueCode::Persistence),
        value => Err(invalid_text(value)),
    }
}

fn invalid_text(value: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        Type::Text,
        io::Error::new(io::ErrorKind::InvalidData, value.into()).into(),
    )
}

fn sqlite_u64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))
}

fn sqlite_usize(value: usize) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))
}

fn read_u64(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|error| invalid_text(error.to_string()))
}

fn read_u32(row: &Row<'_>, index: usize) -> rusqlite::Result<u32> {
    u32::try_from(row.get::<_, i64>(index)?).map_err(|error| invalid_text(error.to_string()))
}

fn read_u16(row: &Row<'_>, index: usize) -> rusqlite::Result<u16> {
    u16::try_from(row.get::<_, i64>(index)?).map_err(|error| invalid_text(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dla_application::{
        installation::{InstallationLibrary, InstallationStore},
        installation_from_scan::{CreateInstallationFromScanRequest, InstallationFromScanService},
    };
    use dla_domain::installation::{InstallationOverrides, InstallationStatus};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn persists_and_clears_the_scanner_root_preference() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("library.sqlite");
        let store = SqliteLibraryStore::open(&path).expect("library store");
        let location = ScanRootLocation {
            platform: "linux".to_owned(),
            display_path: "/fixtures/my-works".to_owned(),
        };

        store
            .save_scan_root_preference(&location)
            .expect("save scanner root preference");
        drop(store);

        let reopened = SqliteLibraryStore::open(&path).expect("reopen library store");
        assert_eq!(
            reopened
                .read_scan_root_preference("linux")
                .expect("read scanner root preference"),
            Some(location)
        );
        reopened
            .clear_scan_root_preference("linux")
            .expect("clear scanner root preference");
        assert_eq!(
            reopened
                .read_scan_root_preference("linux")
                .expect("read cleared scanner root preference"),
            None
        );
    }

    #[test]
    fn persists_and_restores_scan_evidence() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("library.sqlite");
        let store = SqliteLibraryStore::open(&path).expect("library store");
        let now = "2026-08-05T12:00:00Z".to_owned();
        let root = scan_root(&now);
        let mut session = scan_session(&now);
        store.save_root(&root).expect("save root");
        store.begin_session(&session).expect("begin session");

        let entry_id = ScanEntryId("entry-1".to_owned());
        let result_id = ScanResultId("result-1".to_owned());
        store
            .record_batch(&ScanWriteBatch {
                session_id: session.id.clone(),
                entries: vec![ScanEntry {
                    id: entry_id.clone(),
                    root_id: root.id.clone(),
                    relative_path: "RJ01326398/RJ01326398.zip".to_owned(),
                    path_key: "rj01326398/rj01326398.zip".to_owned(),
                    kind: ScanEntryKind::File,
                    extension: "zip".to_owned(),
                    size: Some("2960788199".to_owned()),
                    modified_at: Some(now.clone()),
                    presence: ScanEntryPresence::Present,
                    first_seen_session_id: Some(session.id.clone()),
                    last_seen_session_id: Some(session.id.clone()),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                }],
                results: vec![ScanResult {
                    id: result_id.clone(),
                    session_id: session.id.clone(),
                    candidate_entry_id: Some(entry_id.clone()),
                    outcome: ScanMatchOutcome::Matched,
                    selected_work_code: Some("RJ01326398".to_owned()),
                    confidence: Some(ScanMatchConfidence::Exact),
                    candidates: vec![ScanMatchCandidate {
                        work_code: "RJ01326398".to_owned(),
                        confidence: ScanMatchConfidence::Exact,
                        reason_codes: vec!["archive_sha256_match".to_owned()],
                        rank: 1,
                    }],
                    evidence: vec![ScanEvidence {
                        id: "evidence-1".to_owned(),
                        result_id: result_id.clone(),
                        source_entry_id: Some(entry_id.clone()),
                        kind: ScanEvidenceKind::ArchiveSha256,
                        normalized_value:
                            "a23fb4e87995bdeafbe48594d25a22609c07588775385121e26bfa73525b875a"
                                .to_owned(),
                        reason_code: "archive_sha256_match".to_owned(),
                        created_at: now.clone(),
                    }],
                    created_at: now.clone(),
                    updated_at: now.clone(),
                }],
                issues: vec![ScanIssue {
                    id: "issue-1".to_owned(),
                    session_id: session.id.clone(),
                    entry_id: None,
                    relative_path: Some("unreadable".to_owned()),
                    code: ScanIssueCode::PermissionDenied,
                    message: "permission denied".to_owned(),
                    recoverable: true,
                    created_at: now.clone(),
                }],
            })
            .expect("record scan batch");

        session.status = ScanStatus::Completed;
        session.counters.discovered_files = 1;
        session.counters.inspected_files = 1;
        session.counters.matched = 1;
        session.counters.recoverable_errors = 1;
        session.finished_at = Some("2026-08-05T12:00:01Z".to_owned());
        store.update_session(&session).expect("complete session");
        drop(store);

        let reopened = SqliteLibraryStore::open(&path).expect("reopened library store");
        let restored = reopened
            .read_latest_session()
            .expect("read session")
            .expect("stored session");
        let page = reopened
            .browse_results(&ScanResultQuery {
                session_id: restored.id.clone(),
                outcome: Some(ScanMatchOutcome::Matched),
                limit: 60,
                offset: 0,
            })
            .expect("browse scan results");

        assert_eq!(restored.status, ScanStatus::Completed);
        assert_eq!(restored.counters.recoverable_errors, 1);
        assert_eq!(page.total, 1);
        assert_eq!(
            page.items[0].relative_path.as_deref(),
            Some("RJ01326398/RJ01326398.zip")
        );
        assert_eq!(page.items[0].result.candidates[0].work_code, "RJ01326398");
        assert_eq!(
            page.items[0].result.evidence[0].source_entry_id,
            Some(entry_id)
        );
    }

    #[test]
    fn creates_and_refreshes_an_installation_from_persisted_scan_evidence() {
        let directory = tempdir().expect("temporary directory");
        let store = Arc::new(
            SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
                .expect("library store"),
        );
        let now = "2026-08-07T12:00:00Z".to_owned();
        let mut root = scan_root(&now);
        root.platform = "windows".to_owned();
        root.path_key = "c:/fixtures/game".to_owned();
        root.display_path = "C:\\fixtures\\game".to_owned();
        let mut session = scan_session(&now);
        store.save_root(&root).expect("save root");
        store.begin_session(&session).expect("begin session");

        let entry_id = ScanEntryId("entry-game".to_owned());
        let result_id = ScanResultId("result-game".to_owned());
        store
            .record_batch(&ScanWriteBatch {
                session_id: session.id.clone(),
                entries: vec![ScanEntry {
                    id: entry_id.clone(),
                    root_id: root.id.clone(),
                    relative_path: "Game.exe".to_owned(),
                    path_key: "game.exe".to_owned(),
                    kind: ScanEntryKind::File,
                    extension: "exe".to_owned(),
                    size: Some("7".to_owned()),
                    modified_at: Some(now.clone()),
                    presence: ScanEntryPresence::Present,
                    first_seen_session_id: Some(session.id.clone()),
                    last_seen_session_id: Some(session.id.clone()),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                }],
                results: vec![ScanResult {
                    id: result_id.clone(),
                    session_id: session.id.clone(),
                    candidate_entry_id: Some(entry_id),
                    outcome: ScanMatchOutcome::Matched,
                    selected_work_code: Some("RJ01326398".to_owned()),
                    confidence: Some(ScanMatchConfidence::Exact),
                    candidates: vec![ScanMatchCandidate {
                        work_code: "RJ01326398".to_owned(),
                        confidence: ScanMatchConfidence::Exact,
                        reason_codes: vec!["archive_sha256_match".to_owned()],
                        rank: 1,
                    }],
                    evidence: Vec::new(),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                }],
                issues: Vec::new(),
            })
            .expect("record scan batch");
        session.status = ScanStatus::Completed;
        session.counters.discovered_files = 1;
        session.counters.inspected_files = 1;
        session.counters.matched = 1;
        session.finished_at = Some("2026-08-07T12:00:01Z".to_owned());
        store.update_session(&session).expect("complete session");

        let source: Arc<dyn InstallationScanSource> = store.clone();
        let installation_store: Arc<dyn InstallationStore> = store.clone();
        let service = InstallationFromScanService::new(source, installation_store);
        let request = CreateInstallationFromScanRequest {
            session_id: session.id.clone(),
            selected_result_id: result_id,
        };
        let created = service
            .create_or_refresh(request.clone())
            .expect("create installation");

        assert_eq!(created.status, InstallationStatus::Ready);
        assert_eq!(created.detection.content_items.len(), 1);
        assert_eq!(created.detection.launch_candidates.len(), 1);
        let library_store: Arc<dyn InstallationStore> = store.clone();
        let library = InstallationLibrary::new(library_store);
        let overrides = InstallationOverrides {
            custom_title: Some("My preferred title".to_owned()),
            ..InstallationOverrides::default()
        };
        library
            .replace_overrides(
                &created.id,
                overrides.clone(),
                "2026-08-07T12:00:02Z".to_owned(),
            )
            .expect("save overrides");

        let refreshed = service
            .create_or_refresh(request)
            .expect("refresh installation");
        let restored = library
            .read(&created.id)
            .expect("read installation")
            .expect("persisted installation");

        assert_eq!(refreshed, restored);
        assert_eq!(restored.overrides, overrides);
        assert_eq!(restored.discovered_at, created.discovered_at);
        assert_eq!(restored.updated_at, "2026-08-07T12:00:02Z");
    }

    #[test]
    fn creates_a_second_installation_after_a_multi_work_rescan_of_the_same_root() {
        let directory = tempdir().expect("temporary directory");
        let store = Arc::new(
            SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
                .expect("library store"),
        );
        let now = "2026-08-13T05:00:00Z";
        let root = scan_root(now);
        store.save_root(&root).expect("save root");
        let first_session = record_matched_session(
            &store,
            &root,
            "session-first",
            &[(
                "entry-first",
                "result-first",
                "RJ01678657.zip",
                "RJ01678657",
            )],
            now,
        );
        let source: Arc<dyn InstallationScanSource> = store.clone();
        let installation_store: Arc<dyn InstallationStore> = store.clone();
        let service = InstallationFromScanService::new(source, installation_store);
        let first = service
            .create_or_refresh(CreateInstallationFromScanRequest {
                session_id: first_session,
                selected_result_id: ScanResultId("result-first".to_owned()),
            })
            .expect("first installation");
        let library_store: Arc<dyn InstallationStore> = store.clone();
        let library = InstallationLibrary::new(library_store);
        let first_overrides = InstallationOverrides {
            custom_title: Some("Installed first work".to_owned()),
            ..InstallationOverrides::default()
        };
        library
            .replace_overrides(
                &first.id,
                first_overrides.clone(),
                "2026-08-13T05:00:02Z".to_owned(),
            )
            .expect("first work overrides");

        let second_session = record_matched_session(
            &store,
            &root,
            "session-second",
            &[
                (
                    "entry-first",
                    "result-first-rescan",
                    "RJ01678657.zip",
                    "RJ01678657",
                ),
                (
                    "entry-second",
                    "result-second",
                    "RJ01678999.zip",
                    "RJ01678999",
                ),
            ],
            "2026-08-13T05:01:00Z",
        );
        let second = service
            .create_or_refresh(CreateInstallationFromScanRequest {
                session_id: second_session.clone(),
                selected_result_id: ScanResultId("result-second".to_owned()),
            })
            .expect("second installation");
        let refreshed_first = service
            .create_or_refresh(CreateInstallationFromScanRequest {
                session_id: second_session,
                selected_result_id: ScanResultId("result-first-rescan".to_owned()),
            })
            .expect("refresh legacy first installation");
        let restored_first = library
            .read(&first.id)
            .expect("read first installation")
            .expect("first installation remains");

        assert_ne!(first.id, second.id);
        assert_eq!(refreshed_first.id, first.id);
        assert_eq!(library.list().expect("installation list").len(), 2);
        assert_eq!(restored_first.overrides, first_overrides);
        assert_eq!(
            restored_first.effective_catalog_work_code(),
            Some("RJ01678657")
        );
        assert_eq!(second.effective_catalog_work_code(), Some("RJ01678999"));
        assert_eq!(second.detection.content_items.len(), 1);
        assert_eq!(
            second.detection.content_items[0].relative_path.as_str(),
            "RJ01678999.zip"
        );
    }

    #[test]
    fn marks_only_active_sessions_as_interrupted() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let now = "2026-08-05T12:00:00Z".to_owned();
        let root = scan_root(&now);
        store.save_root(&root).expect("save root");
        let running = scan_session(&now);
        let mut queued = scan_session(&now);
        queued.id = ScanSessionId("session-queued".to_owned());
        queued.status = ScanStatus::Queued;
        let mut completed = scan_session(&now);
        completed.id = ScanSessionId("session-completed".to_owned());
        completed.status = ScanStatus::Completed;
        completed.finished_at = Some(now.clone());
        store.begin_session(&running).expect("running session");
        store.begin_session(&queued).expect("queued session");
        store.begin_session(&completed).expect("completed session");

        assert_eq!(
            store
                .interrupt_active_sessions("2026-08-05T12:01:00Z")
                .expect("interrupt sessions"),
            2
        );
        assert_eq!(
            store
                .read_session(&running.id)
                .expect("read running session")
                .expect("stored running session")
                .status,
            ScanStatus::Interrupted
        );
        assert_eq!(
            store
                .read_session(&queued.id)
                .expect("read queued session")
                .expect("stored queued session")
                .status,
            ScanStatus::Interrupted
        );
        assert_eq!(
            store
                .read_session(&completed.id)
                .expect("read completed session")
                .expect("stored completed session")
                .status,
            ScanStatus::Completed
        );
        assert_eq!(
            store
                .interrupt_active_sessions("2026-08-05T12:02:00Z")
                .expect("repeat interruption"),
            0
        );
    }

    #[test]
    fn rejects_cross_session_write_batches() {
        let error = validate_batch(&ScanWriteBatch {
            session_id: ScanSessionId("session-1".to_owned()),
            entries: Vec::new(),
            results: Vec::new(),
            issues: vec![ScanIssue {
                id: "issue-1".to_owned(),
                session_id: ScanSessionId("session-2".to_owned()),
                entry_id: None,
                relative_path: None,
                code: ScanIssueCode::Io,
                message: "different session".to_owned(),
                recoverable: true,
                created_at: "2026-08-05T12:00:00Z".to_owned(),
            }],
        })
        .expect_err("mixed session batch must fail");

        assert!(matches!(error, ScannerError::InvalidRequest(_)));
    }

    fn scan_root(now: &str) -> ScanRoot {
        ScanRoot {
            id: ScanRootId("root-1".to_owned()),
            platform: "linux".to_owned(),
            path_key: "/fixtures/library".to_owned(),
            display_path: "/fixtures/library".to_owned(),
            created_at: now.to_owned(),
            updated_at: now.to_owned(),
        }
    }

    fn scan_session(now: &str) -> ScanSession {
        ScanSession {
            id: ScanSessionId("session-1".to_owned()),
            root_id: ScanRootId("root-1".to_owned()),
            status: ScanStatus::Running,
            options: ScanOptions::default(),
            counters: ScanCounters::default(),
            started_at: now.to_owned(),
            finished_at: None,
            fatal_error_code: None,
            fatal_error_message: None,
        }
    }

    fn record_matched_session(
        store: &SqliteLibraryStore,
        root: &ScanRoot,
        session_id: &str,
        works: &[(&str, &str, &str, &str)],
        now: &str,
    ) -> ScanSessionId {
        let session_id = ScanSessionId(session_id.to_owned());
        let mut session = scan_session(now);
        session.id = session_id.clone();
        store
            .begin_session(&session)
            .expect("begin matched session");
        let entries = works
            .iter()
            .map(|(entry_id, _, relative_path, _)| ScanEntry {
                id: ScanEntryId((*entry_id).to_owned()),
                root_id: root.id.clone(),
                relative_path: (*relative_path).to_owned(),
                path_key: relative_path.to_ascii_lowercase(),
                kind: ScanEntryKind::File,
                extension: "zip".to_owned(),
                size: Some("7".to_owned()),
                modified_at: Some(now.to_owned()),
                presence: ScanEntryPresence::Present,
                first_seen_session_id: Some(session_id.clone()),
                last_seen_session_id: Some(session_id.clone()),
                created_at: now.to_owned(),
                updated_at: now.to_owned(),
            })
            .collect();
        let results = works
            .iter()
            .map(|(entry_id, result_id, _, work_code)| ScanResult {
                id: ScanResultId((*result_id).to_owned()),
                session_id: session_id.clone(),
                candidate_entry_id: Some(ScanEntryId((*entry_id).to_owned())),
                outcome: ScanMatchOutcome::Matched,
                selected_work_code: Some((*work_code).to_owned()),
                confidence: Some(ScanMatchConfidence::Exact),
                candidates: vec![ScanMatchCandidate {
                    work_code: (*work_code).to_owned(),
                    confidence: ScanMatchConfidence::Exact,
                    reason_codes: vec!["archive_sha256_match".to_owned()],
                    rank: 1,
                }],
                evidence: Vec::new(),
                created_at: now.to_owned(),
                updated_at: now.to_owned(),
            })
            .collect();
        store
            .record_batch(&ScanWriteBatch {
                session_id: session_id.clone(),
                entries,
                results,
                issues: Vec::new(),
            })
            .expect("record matched session");
        session.status = ScanStatus::Completed;
        session.counters.discovered_files = works.len() as u64;
        session.counters.inspected_files = works.len() as u64;
        session.counters.matched = works.len() as u64;
        session.finished_at = Some(now.to_owned());
        store
            .update_session(&session)
            .expect("complete matched session");
        session_id
    }
}
