use dla_application::maintenance::{LibraryMaintenanceError, LibraryMaintenanceStore};
use dla_domain::{
    installation::InstallationId,
    maintenance::{InstallationHealthIssue, InstallationHealthReport, InstallationHealthState},
};
use rusqlite::{OptionalExtension, Row, params, params_from_iter, types::Type};

use crate::SqliteLibraryStore;

impl LibraryMaintenanceStore for SqliteLibraryStore {
    fn read_installation_health(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<InstallationHealthReport>, LibraryMaintenanceError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT state, managed, repairable, checked_root, expected_files,
                            present_files, missing_files, modified_files, inaccessible_files,
                            unexpected_files, issues, checked_at
                     FROM library_installation_health
                     WHERE installation_id = ?1",
                    params![installation_id.0],
                    |row| read_health_row(row, installation_id.clone(), 0),
                )
                .optional()
        })
        .map_err(LibraryMaintenanceError::persistence)
    }

    fn read_installation_healths(
        &self,
        installation_ids: &[InstallationId],
    ) -> Result<Vec<InstallationHealthReport>, LibraryMaintenanceError> {
        if installation_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_connection(|connection| {
            let placeholders = vec!["?"; installation_ids.len()].join(",");
            let sql = format!(
                "SELECT installation_id, state, managed, repairable, checked_root,
                        expected_files, present_files, missing_files, modified_files,
                        inaccessible_files, unexpected_files, issues, checked_at
                 FROM library_installation_health
                 WHERE installation_id IN ({placeholders})"
            );
            let mut statement = connection.prepare(&sql)?;
            statement
                .query_map(
                    params_from_iter(installation_ids.iter().map(|id| &id.0)),
                    |row| read_health_row(row, InstallationId(row.get::<_, String>(0)?), 1),
                )?
                .collect()
        })
        .map_err(LibraryMaintenanceError::persistence)
    }

    fn save_installation_health(
        &self,
        report: &InstallationHealthReport,
    ) -> Result<(), LibraryMaintenanceError> {
        let issues =
            serde_json::to_string(&report.issues).map_err(LibraryMaintenanceError::persistence)?;
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO library_installation_health
                 (installation_id, state, managed, repairable, checked_root, expected_files,
                  present_files, missing_files, modified_files, inaccessible_files,
                  unexpected_files, issues, checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(installation_id) DO UPDATE SET
                    state = excluded.state,
                    managed = excluded.managed,
                    repairable = excluded.repairable,
                    checked_root = excluded.checked_root,
                    expected_files = excluded.expected_files,
                    present_files = excluded.present_files,
                    missing_files = excluded.missing_files,
                    modified_files = excluded.modified_files,
                    inaccessible_files = excluded.inaccessible_files,
                    unexpected_files = excluded.unexpected_files,
                    issues = excluded.issues,
                    checked_at = excluded.checked_at",
                params![
                    report.installation_id.0,
                    health_state(report.state),
                    report.managed,
                    report.repairable,
                    report.checked_root,
                    to_sqlite_u64(report.expected_files)?,
                    to_sqlite_u64(report.present_files)?,
                    to_sqlite_u64(report.missing_files)?,
                    to_sqlite_u64(report.modified_files)?,
                    to_sqlite_u64(report.inaccessible_files)?,
                    to_sqlite_u64(report.unexpected_files)?,
                    issues,
                    report.checked_at,
                ],
            )?;
            Ok(())
        })
        .map_err(LibraryMaintenanceError::persistence)
    }

    fn replace_installation_root(
        &self,
        installation_id: &InstallationId,
        root_path: &str,
        updated_at: &str,
    ) -> Result<(), LibraryMaintenanceError> {
        self.with_connection(|connection| {
            let changed = connection.execute(
                "UPDATE library_installation
                 SET root_path = ?2, updated_at = ?3
                 WHERE installation_id = ?1",
                params![installation_id.0, root_path, updated_at],
            )?;
            if changed == 1 {
                Ok(())
            } else {
                Err(rusqlite::Error::QueryReturnedNoRows)
            }
        })
        .map_err(LibraryMaintenanceError::persistence)
    }

    fn remove_installation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<(), LibraryMaintenanceError> {
        self.with_connection(|connection| {
            let changed = connection.execute(
                "DELETE FROM library_installation WHERE installation_id = ?1",
                params![installation_id.0],
            )?;
            if changed == 1 {
                Ok(())
            } else {
                Err(rusqlite::Error::QueryReturnedNoRows)
            }
        })
        .map_err(LibraryMaintenanceError::persistence)
    }

    fn installation_is_active(
        &self,
        installation_id: &InstallationId,
    ) -> Result<bool, LibraryMaintenanceError> {
        self.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM library_launch_activity
                    WHERE installation_id = ?1 AND status IN ('starting', 'running', 'stopping')
                 ) OR EXISTS(
                    SELECT 1 FROM library_media_session
                    WHERE installation_id = ?1 AND status IN ('active', 'paused')
                 )",
                params![installation_id.0],
                |row| row.get(0),
            )
        })
        .map_err(LibraryMaintenanceError::persistence)
    }
}

fn read_health_row(
    row: &Row<'_>,
    installation_id: InstallationId,
    offset: usize,
) -> rusqlite::Result<InstallationHealthReport> {
    let state = row.get::<_, String>(offset)?;
    let expected_files = row.get::<_, i64>(offset + 4)?;
    let present_files = row.get::<_, i64>(offset + 5)?;
    let missing_files = row.get::<_, i64>(offset + 6)?;
    let modified_files = row.get::<_, i64>(offset + 7)?;
    let inaccessible_files = row.get::<_, i64>(offset + 8)?;
    let unexpected_files = row.get::<_, i64>(offset + 9)?;
    let issues =
        serde_json::from_str::<Vec<InstallationHealthIssue>>(&row.get::<_, String>(offset + 10)?)
            .map_err(|error| conversion_error(offset + 10, error))?;
    Ok(InstallationHealthReport {
        installation_id,
        state: parse_health_state(offset, &state)?,
        managed: row.get(offset + 1)?,
        repairable: row.get(offset + 2)?,
        checked_root: row.get(offset + 3)?,
        expected_files: sqlite_u64(offset + 4, expected_files)?,
        present_files: sqlite_u64(offset + 5, present_files)?,
        missing_files: sqlite_u64(offset + 6, missing_files)?,
        modified_files: sqlite_u64(offset + 7, modified_files)?,
        inaccessible_files: sqlite_u64(offset + 8, inaccessible_files)?,
        unexpected_files: sqlite_u64(offset + 9, unexpected_files)?,
        issues,
        checked_at: row.get(offset + 11)?,
    })
}

fn health_state(state: InstallationHealthState) -> &'static str {
    match state {
        InstallationHealthState::Unknown => "unknown",
        InstallationHealthState::Healthy => "healthy",
        InstallationHealthState::MissingFiles => "missing_files",
        InstallationHealthState::ModifiedFiles => "modified_files",
        InstallationHealthState::Moved => "moved",
        InstallationHealthState::Inaccessible => "inaccessible",
        InstallationHealthState::NeedsReview => "needs_review",
        InstallationHealthState::Repairable => "repairable",
    }
}

fn parse_health_state(column: usize, value: &str) -> rusqlite::Result<InstallationHealthState> {
    match value {
        "unknown" => Ok(InstallationHealthState::Unknown),
        "healthy" => Ok(InstallationHealthState::Healthy),
        "missing_files" => Ok(InstallationHealthState::MissingFiles),
        "modified_files" => Ok(InstallationHealthState::ModifiedFiles),
        "moved" => Ok(InstallationHealthState::Moved),
        "inaccessible" => Ok(InstallationHealthState::Inaccessible),
        "needs_review" => Ok(InstallationHealthState::NeedsReview),
        "repairable" => Ok(InstallationHealthState::Repairable),
        _ => Err(conversion_error(
            column,
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "unknown installation health state",
            ),
        )),
    }
}

fn to_sqlite_u64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| conversion_error(0, error))
}

fn sqlite_u64(column: usize, value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| conversion_error(column, error))
}

fn conversion_error(
    column: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use dla_application::installation::InstallationStore;
    use dla_domain::installation::{
        Installation, InstallationDetection, InstallationOverrides, InstallationPlatform,
        InstallationStatus,
    };
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn relocation_preserves_review_and_resume_state() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let installation = installation();
        InstallationStore::create(&store, &installation).expect("create installation");
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO library_media_resume
                     (installation_id, action_kind, relative_path, position_ms, duration_ms,
                      completed, updated_at)
                     VALUES (?1, 'read_images', 'page-01.png', 7, 20, 0, ?2)",
                    params![installation.id.0, installation.updated_at],
                )?;
                Ok(())
            })
            .expect("resume state");

        LibraryMaintenanceStore::replace_installation_root(
            &store,
            &installation.id,
            "/library/moved-work",
            "2026-08-16T01:00:00Z",
        )
        .expect("relocate");

        let restored = InstallationStore::read(&store, &installation.id)
            .expect("read installation")
            .expect("installation");
        assert_eq!(restored.id, installation.id);
        assert_eq!(restored.root_path, "/library/moved-work");
        assert_eq!(restored.overrides, installation.overrides);
        let resume = store
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT relative_path, position_ms FROM library_media_resume
                     WHERE installation_id = ?1 AND action_kind = 'read_images'",
                    params![installation.id.0],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
            })
            .expect("read resume");
        assert_eq!(resume, ("page-01.png".to_owned(), 7));
    }

    #[test]
    fn health_round_trips_and_is_removed_with_its_installation() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let installation = installation();
        InstallationStore::create(&store, &installation).expect("create installation");
        let report = InstallationHealthReport {
            installation_id: installation.id.clone(),
            state: InstallationHealthState::Repairable,
            managed: true,
            repairable: true,
            checked_root: "/library/work".to_owned(),
            expected_files: 3,
            present_files: 2,
            missing_files: 1,
            modified_files: 0,
            inaccessible_files: 0,
            unexpected_files: 0,
            issues: vec![InstallationHealthIssue {
                kind: dla_domain::maintenance::InstallationHealthIssueKind::Missing,
                relative_path: Some(
                    dla_domain::installation::RelativePath::parse("missing.bin")
                        .expect("missing path"),
                ),
                detail: "indexed file is missing on disk".to_owned(),
            }],
            checked_at: "2026-08-16T01:00:00Z".to_owned(),
        };

        LibraryMaintenanceStore::save_installation_health(&store, &report).expect("save health");
        assert_eq!(
            LibraryMaintenanceStore::read_installation_health(&store, &installation.id)
                .expect("read health"),
            Some(report),
        );

        LibraryMaintenanceStore::remove_installation(&store, &installation.id)
            .expect("remove installation");
        assert!(
            LibraryMaintenanceStore::read_installation_health(&store, &installation.id)
                .expect("read removed health")
                .is_none()
        );
    }

    fn installation() -> Installation {
        Installation {
            id: InstallationId("installation-1".to_owned()),
            scan_root_id: None,
            root_path: "/library/work".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::NeedsReview,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: InstallationStatus::NeedsReview,
                content_items: Vec::new(),
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                custom_title: Some("My preserved title".to_owned()),
                reviewed_at: Some("2026-08-16T00:00:00Z".to_owned()),
                ..InstallationOverrides::default()
            },
            discovered_at: "2026-08-16T00:00:00Z".to_owned(),
            updated_at: "2026-08-16T00:00:00Z".to_owned(),
        }
    }
}
