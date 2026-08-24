use std::io;

use dla_application::launch::{LaunchActivityStore, LaunchError};
use dla_domain::{
    installation::{InstallationId, LaunchActionKind},
    launch::{LaunchActivity, LaunchActivityId, LaunchActivityStatus, LaunchAdapter},
};
use rusqlite::{Connection, OptionalExtension, Row, params, types::Type};

use crate::SqliteLibraryStore;

const ACTIVITY_COLUMNS: &str = "activity_id, installation_id, action_kind, target_path, adapter, status, process_id, error, \
     attempted_at, started_at, ended_at, duration_ms, exit_code, stop_requested_at";

impl LaunchActivityStore for SqliteLibraryStore {
    fn begin_launch_activity(&self, activity: &LaunchActivity) -> Result<(), LaunchError> {
        let result = self
            .with_connection(|connection| {
                let transaction = connection.transaction()?;
                let existing = transaction
                    .query_row(
                        "SELECT activity_id
                         FROM library_launch_activity
                         WHERE installation_id = ?1
                           AND status IN ('starting', 'running', 'stopping')
                         LIMIT 1",
                        [activity.installation_id.0.as_str()],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                if existing.is_none() {
                    insert_activity(&transaction, activity, false)?;
                }
                transaction.commit()?;
                Ok(existing)
            })
            .map_err(LaunchError::persistence)?;
        match result {
            Some(activity_id) => Err(LaunchError::AlreadyRunning(activity_id)),
            None => Ok(()),
        }
    }

    fn save_launch_activity(&self, activity: &LaunchActivity) -> Result<(), LaunchError> {
        self.with_connection(|connection| insert_activity(connection, activity, true))
            .map_err(LaunchError::persistence)
    }

    fn read_launch_activity(
        &self,
        activity_id: &LaunchActivityId,
    ) -> Result<Option<LaunchActivity>, LaunchError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    &format!(
                        "SELECT {ACTIVITY_COLUMNS}
                         FROM library_launch_activity
                         WHERE activity_id = ?1"
                    ),
                    [activity_id.0.as_str()],
                    activity_from_row,
                )
                .optional()
        })
        .map_err(LaunchError::persistence)
    }

    fn list_launch_activities(
        &self,
        installation_id: Option<&InstallationId>,
        limit: u32,
    ) -> Result<Vec<LaunchActivity>, LaunchError> {
        self.with_connection(|connection| {
            let sql = match installation_id {
                Some(_) => format!(
                    "SELECT {ACTIVITY_COLUMNS}
                     FROM library_launch_activity
                     WHERE installation_id = ?1
                     ORDER BY attempted_at DESC, activity_id DESC
                     LIMIT ?2"
                ),
                None => format!(
                    "SELECT {ACTIVITY_COLUMNS}
                     FROM library_launch_activity
                     ORDER BY attempted_at DESC, activity_id DESC
                     LIMIT ?1"
                ),
            };
            let mut statement = connection.prepare(&sql)?;
            let rows = match installation_id {
                Some(installation_id) => statement.query_map(
                    params![installation_id.0.as_str(), i64::from(limit)],
                    activity_from_row,
                )?,
                None => statement.query_map([i64::from(limit)], activity_from_row)?,
            };
            rows.collect()
        })
        .map_err(LaunchError::persistence)
    }

    fn interrupt_active_launches(
        &self,
        interrupted_at: &str,
        reason: &str,
    ) -> Result<u64, LaunchError> {
        self.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE library_launch_activity
                     SET status = 'interrupted', error = ?1, ended_at = ?2
                     WHERE status IN ('starting', 'running', 'stopping')",
                    params![reason, interrupted_at],
                )
                .map(|count| count as u64)
        })
        .map_err(LaunchError::persistence)
    }
}

fn insert_activity(
    connection: &Connection,
    activity: &LaunchActivity,
    update_existing: bool,
) -> rusqlite::Result<()> {
    let conflict = if update_existing {
        "ON CONFLICT(activity_id) DO UPDATE SET
            action_kind = excluded.action_kind,
            target_path = excluded.target_path,
            adapter = excluded.adapter,
            status = excluded.status,
            process_id = excluded.process_id,
            error = excluded.error,
            started_at = excluded.started_at,
            ended_at = excluded.ended_at,
            duration_ms = excluded.duration_ms,
            exit_code = excluded.exit_code,
            stop_requested_at = excluded.stop_requested_at"
    } else {
        ""
    };
    connection.execute(
        &format!(
            "INSERT INTO library_launch_activity ({ACTIVITY_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             {conflict}"
        ),
        params![
            activity.id.0.as_str(),
            activity.installation_id.0.as_str(),
            activity.action.map(action_kind),
            activity.target_path.as_deref(),
            activity.adapter.map(adapter),
            launch_status(activity.status),
            activity.process_id.map(i64::from),
            activity.error.as_deref(),
            activity.attempted_at.as_str(),
            activity.started_at.as_deref(),
            activity.ended_at.as_deref(),
            activity
                .duration_ms
                .and_then(|value| i64::try_from(value).ok()),
            activity.exit_code,
            activity.stop_requested_at.as_deref(),
        ],
    )?;
    Ok(())
}

fn activity_from_row(row: &Row<'_>) -> rusqlite::Result<LaunchActivity> {
    let process_id = row
        .get::<_, Option<i64>>(6)?
        .map(|value| numeric_conversion(6, value, u32::try_from))
        .transpose()?;
    let duration_ms = row
        .get::<_, Option<i64>>(11)?
        .map(|value| numeric_conversion(11, value, u64::try_from))
        .transpose()?;
    Ok(LaunchActivity {
        id: LaunchActivityId(row.get(0)?),
        installation_id: InstallationId(row.get(1)?),
        action: row
            .get::<_, Option<String>>(2)?
            .map(|value| parse_action(2, &value))
            .transpose()?,
        target_path: row.get(3)?,
        adapter: row
            .get::<_, Option<String>>(4)?
            .map(|value| parse_adapter(4, &value))
            .transpose()?,
        status: parse_status(5, &row.get::<_, String>(5)?)?,
        process_id,
        error: row.get(7)?,
        attempted_at: row.get(8)?,
        started_at: row.get(9)?,
        ended_at: row.get(10)?,
        duration_ms,
        exit_code: row.get(12)?,
        stop_requested_at: row.get(13)?,
    })
}

fn numeric_conversion<T>(
    column: usize,
    value: i64,
    convert: impl FnOnce(i64) -> Result<T, std::num::TryFromIntError>,
) -> rusqlite::Result<T> {
    convert(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Integer, Box::new(error))
    })
}

pub(crate) fn action_kind(value: LaunchActionKind) -> &'static str {
    match value {
        LaunchActionKind::LaunchExecutable => "launch_executable",
        LaunchActionKind::PlayAudio => "play_audio",
        LaunchActionKind::ReadImages => "read_images",
        LaunchActionKind::OpenDocument => "open_document",
        LaunchActionKind::PlayVideo => "play_video",
        LaunchActionKind::OpenArchive => "open_archive",
        LaunchActionKind::OpenAndroidPackage => "open_android_package",
    }
}

pub(crate) fn parse_action(column: usize, value: &str) -> rusqlite::Result<LaunchActionKind> {
    match value {
        "launch_executable" => Ok(LaunchActionKind::LaunchExecutable),
        "play_audio" => Ok(LaunchActionKind::PlayAudio),
        "read_images" => Ok(LaunchActionKind::ReadImages),
        "open_document" => Ok(LaunchActionKind::OpenDocument),
        "play_video" => Ok(LaunchActionKind::PlayVideo),
        "open_archive" => Ok(LaunchActionKind::OpenArchive),
        "open_android_package" => Ok(LaunchActionKind::OpenAndroidPackage),
        _ => Err(invalid_text(column, "launch action", value)),
    }
}

fn adapter(value: LaunchAdapter) -> &'static str {
    match value {
        LaunchAdapter::WindowsNative => "windows_native",
        LaunchAdapter::LinuxNative => "linux_native",
        LaunchAdapter::LinuxWine => "linux_wine",
    }
}

fn parse_adapter(column: usize, value: &str) -> rusqlite::Result<LaunchAdapter> {
    match value {
        "windows_native" => Ok(LaunchAdapter::WindowsNative),
        "linux_native" => Ok(LaunchAdapter::LinuxNative),
        "linux_wine" => Ok(LaunchAdapter::LinuxWine),
        _ => Err(invalid_text(column, "launch adapter", value)),
    }
}

fn launch_status(value: LaunchActivityStatus) -> &'static str {
    match value {
        LaunchActivityStatus::Starting => "starting",
        LaunchActivityStatus::Running => "running",
        LaunchActivityStatus::Stopping => "stopping",
        LaunchActivityStatus::Exited => "exited",
        LaunchActivityStatus::Failed => "failed",
        LaunchActivityStatus::Stopped => "stopped",
        LaunchActivityStatus::Interrupted => "interrupted",
    }
}

fn parse_status(column: usize, value: &str) -> rusqlite::Result<LaunchActivityStatus> {
    match value {
        "starting" => Ok(LaunchActivityStatus::Starting),
        "running" => Ok(LaunchActivityStatus::Running),
        "stopping" => Ok(LaunchActivityStatus::Stopping),
        "exited" => Ok(LaunchActivityStatus::Exited),
        "failed" => Ok(LaunchActivityStatus::Failed),
        "stopped" => Ok(LaunchActivityStatus::Stopped),
        "interrupted" => Ok(LaunchActivityStatus::Interrupted),
        _ => Err(invalid_text(column, "launch status", value)),
    }
}

fn invalid_text(column: usize, kind: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        Type::Text,
        Box::new(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown {kind}: {value}"),
        )),
    )
}

#[cfg(test)]
mod tests {
    use dla_application::installation::InstallationStore;
    use dla_domain::{
        installation::{
            Installation, InstallationDetection, InstallationId, InstallationOverrides,
            InstallationPlatform, InstallationStatus, LaunchActionKind,
        },
        launch::{LaunchActivityId, LaunchActivityStatus, LaunchAdapter},
    };
    use tempfile::tempdir;

    use super::*;

    fn store_with_installation() -> (tempfile::TempDir, SqliteLibraryStore, InstallationId) {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let installation_id = InstallationId("installation-launch".to_owned());
        store
            .create(&Installation {
                id: installation_id.clone(),
                scan_root_id: None,
                root_path: "/synthetic/game".to_owned(),
                platform: InstallationPlatform::Linux,
                status: InstallationStatus::NeedsReview,
                detection: InstallationDetection {
                    source_scan_session_id: None,
                    catalog_identity: None,
                    suggested_status: InstallationStatus::NeedsReview,
                    content_items: vec![],
                    launch_candidates: vec![],
                    package_inspection: None,
                },
                overrides: InstallationOverrides::default(),
                discovered_at: "2026-08-09T00:00:00Z".to_owned(),
                updated_at: "2026-08-09T00:00:00Z".to_owned(),
            })
            .expect("installation");
        (directory, store, installation_id)
    }

    fn starting_activity(id: &str, installation_id: InstallationId) -> LaunchActivity {
        LaunchActivity {
            id: LaunchActivityId(id.to_owned()),
            installation_id,
            action: Some(LaunchActionKind::LaunchExecutable),
            target_path: Some("Game.exe".to_owned()),
            adapter: None,
            status: LaunchActivityStatus::Starting,
            process_id: None,
            error: None,
            attempted_at: "2026-08-09T01:00:00Z".to_owned(),
            started_at: None,
            ended_at: None,
            duration_ms: None,
            exit_code: None,
            stop_requested_at: None,
        }
    }

    #[test]
    fn launch_activity_moves_through_running_and_exit_without_losing_the_attempt() {
        let (_directory, store, installation_id) = store_with_installation();
        let mut activity = starting_activity("launch-1", installation_id.clone());
        store
            .begin_launch_activity(&activity)
            .expect("starting activity");
        activity.adapter = Some(LaunchAdapter::LinuxWine);
        activity.status = LaunchActivityStatus::Running;
        activity.process_id = Some(42);
        activity.started_at = Some("2026-08-09T01:00:01Z".to_owned());
        store
            .save_launch_activity(&activity)
            .expect("running activity");
        activity.status = LaunchActivityStatus::Exited;
        activity.ended_at = Some("2026-08-09T01:01:01Z".to_owned());
        activity.duration_ms = Some(60_000);
        activity.exit_code = Some(0);
        store
            .save_launch_activity(&activity)
            .expect("exited activity");

        let persisted = store
            .read_launch_activity(&activity.id)
            .expect("read activity")
            .expect("activity");
        assert_eq!(persisted.status, LaunchActivityStatus::Exited);
        assert_eq!(persisted.process_id, Some(42));
        assert_eq!(persisted.duration_ms, Some(60_000));
        assert_eq!(persisted.exit_code, Some(0));
        assert_eq!(persisted.attempted_at, "2026-08-09T01:00:00Z");
        assert_eq!(
            store
                .list_launch_activities(Some(&installation_id), 10)
                .expect("history"),
            vec![persisted]
        );
    }

    #[test]
    fn only_one_active_launch_is_allowed_for_an_installation() {
        let (_directory, store, installation_id) = store_with_installation();
        store
            .begin_launch_activity(&starting_activity("launch-1", installation_id.clone()))
            .expect("first launch");

        assert!(matches!(
            store.begin_launch_activity(&starting_activity("launch-2", installation_id)),
            Err(LaunchError::AlreadyRunning(activity_id)) if activity_id == "launch-1"
        ));
    }

    #[test]
    fn restart_reconciliation_marks_active_rows_interrupted() {
        let (_directory, store, installation_id) = store_with_installation();
        let activity = starting_activity("launch-1", installation_id);
        store
            .begin_launch_activity(&activity)
            .expect("starting activity");

        assert_eq!(
            store
                .interrupt_active_launches(
                    "2026-08-09T02:00:00Z",
                    "launcher restarted before exit",
                )
                .expect("interrupt"),
            1
        );
        let interrupted = store
            .read_launch_activity(&activity.id)
            .expect("read")
            .expect("activity");
        assert_eq!(interrupted.status, LaunchActivityStatus::Interrupted);
        assert_eq!(
            interrupted.error.as_deref(),
            Some("launcher restarted before exit")
        );
        assert_eq!(
            interrupted.ended_at.as_deref(),
            Some("2026-08-09T02:00:00Z")
        );
    }
}
