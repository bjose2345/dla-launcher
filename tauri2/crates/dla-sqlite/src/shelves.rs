use std::collections::HashMap;

use dla_application::library_shelves::{
    LibraryActivityReader, LibraryActivitySnapshot, LibraryShelvesError,
};
use dla_domain::{
    installation::InstallationId,
    library::{LibraryActivityKind, LibraryLaunchTotals, LibraryRecentActivity},
};
use rusqlite::Row;

use crate::{SqliteLibraryStore, launch::parse_action, media::resume_from_row};

impl LibraryActivityReader for SqliteLibraryStore {
    fn read_library_activity(&self) -> Result<LibraryActivitySnapshot, LibraryShelvesError> {
        self.with_connection(|connection| {
            let mut recent = HashMap::new();
            let mut launch_statement = connection.prepare(
                "SELECT installation_id, action_kind, started_at,
                        status IN ('running', 'stopping')
                 FROM (
                     SELECT installation_id, action_kind, started_at, status, activity_id,
                            ROW_NUMBER() OVER (
                                PARTITION BY installation_id
                                ORDER BY started_at DESC, activity_id DESC
                            ) AS activity_rank
                     FROM library_launch_activity
                     WHERE started_at IS NOT NULL
                 )
                 WHERE activity_rank = 1",
            )?;
            for activity in launch_statement.query_map([], |row| {
                recent_from_row(row, LibraryActivityKind::ExecutableLaunch)
            })? {
                let activity = activity?;
                recent.insert(activity.installation_id.clone(), activity);
            }

            let mut media_statement = connection.prepare(
                "SELECT installation_id, action_kind, updated_at,
                        status IN ('active', 'paused')
                 FROM (
                     SELECT COALESCE(item.installation_id, session.installation_id) AS installation_id,
                            session.action_kind, session.updated_at, session.status,
                            session.session_id,
                            ROW_NUMBER() OVER (
                                PARTITION BY COALESCE(item.installation_id, session.installation_id)
                                ORDER BY session.updated_at DESC, session.session_id DESC
                            ) AS activity_rank
                     FROM library_media_session session
                     LEFT JOIN library_media_session_item item
                       ON item.session_id = session.session_id
                      AND item.ordinal = session.current_item_ordinal
                 )
                 WHERE activity_rank = 1",
            )?;
            for activity in media_statement.query_map([], |row| {
                recent_from_row(row, LibraryActivityKind::MediaSession)
            })? {
                let activity = activity?;
                match recent.get(&activity.installation_id) {
                    Some(existing) if existing.occurred_at > activity.occurred_at => {}
                    Some(existing)
                        if existing.occurred_at == activity.occurred_at
                            && existing.kind == LibraryActivityKind::MediaSession => {}
                    _ => {
                        recent.insert(activity.installation_id.clone(), activity);
                    }
                }
            }

            let mut resume_statement = connection.prepare(
                "SELECT installation_id, action_kind, relative_path, position_ms,
                        duration_ms, completed, updated_at
                 FROM library_media_resume
                 ORDER BY updated_at DESC, installation_id, action_kind",
            )?;
            let resumes = resume_statement
                .query_map([], resume_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut totals_statement = connection.prepare(
                "SELECT installation_id, COUNT(*), COALESCE(SUM(COALESCE(duration_ms, 0)), 0)
                 FROM library_launch_activity
                 WHERE started_at IS NOT NULL
                 GROUP BY installation_id",
            )?;
            let launch_totals = totals_statement
                .query_map([], |row| {
                    Ok(LibraryLaunchTotals {
                        installation_id: InstallationId(row.get(0)?),
                        launch_count: row.get(1)?,
                        total_duration_ms: row.get::<_, i64>(2)?.max(0) as u64,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            Ok(LibraryActivitySnapshot {
                recent: recent.into_values().collect(),
                resumes,
                launch_totals,
            })
        })
        .map_err(LibraryShelvesError::persistence)
    }
}

fn recent_from_row(
    row: &Row<'_>,
    kind: LibraryActivityKind,
) -> rusqlite::Result<LibraryRecentActivity> {
    Ok(LibraryRecentActivity {
        installation_id: InstallationId(row.get(0)?),
        action: row
            .get::<_, Option<String>>(1)?
            .map(|value| parse_action(1, &value))
            .transpose()?,
        kind,
        occurred_at: row.get(2)?,
        active: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use dla_application::{
        installation::InstallationStore, launch::LaunchActivityStore, media::MediaSessionStore,
    };
    use dla_domain::{
        installation::{
            Installation, InstallationDetection, InstallationOverrides, InstallationPlatform,
            InstallationStatus, LaunchActionKind, ManualCatalogIdentity, MediaType, RelativePath,
        },
        launch::{LaunchActivity, LaunchActivityId, LaunchActivityStatus, LaunchAdapter},
        media::{
            MediaProgress, MediaRepeatMode, MediaSession, MediaSessionId, MediaSessionItem,
            MediaSessionKind, MediaSessionStatus,
        },
    };
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn reads_latest_real_activity_and_resume_state_per_installation() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        store
            .create(&installation("game"))
            .expect("game installation");
        store
            .create(&installation("media"))
            .expect("media installation");
        store
            .create(&installation("failed"))
            .expect("failed installation");
        store
            .save_launch_activity(&launch_activity(
                "game-launch",
                "game",
                Some("2026-08-08T10:00:00Z"),
            ))
            .expect("successful launch activity");
        store
            .save_launch_activity(&launch_activity(
                "game-launch-second",
                "game",
                Some("2026-08-08T12:00:00Z"),
            ))
            .expect("second successful launch activity");
        store
            .save_launch_activity(&launch_activity("failed-launch", "failed", None))
            .expect("failed preflight activity");
        store
            .create_media_session(&media_session())
            .expect("media session");

        let snapshot = store.read_library_activity().expect("activity snapshot");
        let mut recent = snapshot.recent;
        recent.sort_by(|left, right| left.installation_id.cmp(&right.installation_id));

        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].installation_id.0, "game");
        assert_eq!(recent[0].kind, LibraryActivityKind::ExecutableLaunch);
        assert_eq!(recent[1].installation_id.0, "media");
        assert_eq!(recent[1].kind, LibraryActivityKind::MediaSession);
        assert!(recent[1].active);
        assert_eq!(snapshot.resumes.len(), 1);
        assert_eq!(snapshot.resumes[0].installation_id.0, "media");
        assert_eq!(snapshot.resumes[0].position_ms, 15_000);

        assert_eq!(
            snapshot.launch_totals,
            vec![LibraryLaunchTotals {
                installation_id: InstallationId("game".to_owned()),
                launch_count: 2,
                total_duration_ms: 1_200_000,
            }]
        );
    }

    fn installation(id: &str) -> Installation {
        Installation {
            id: InstallationId(id.to_owned()),
            scan_root_id: None,
            root_path: format!("/library/{id}"),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: InstallationStatus::Ready,
                content_items: Vec::new(),
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                catalog_identity: Some(ManualCatalogIdentity::CatalogWork {
                    work_code: format!("RJ{id}"),
                }),
                custom_title: None,
                preferred_action: None,
                content_items: Vec::new(),
                reviewed_at: Some("2026-08-08T09:00:00Z".to_owned()),
            },
            discovered_at: "2026-08-08T09:00:00Z".to_owned(),
            updated_at: "2026-08-08T09:00:00Z".to_owned(),
        }
    }

    fn launch_activity(
        activity_id: &str,
        installation_id: &str,
        started_at: Option<&str>,
    ) -> LaunchActivity {
        let successful = started_at.is_some();
        LaunchActivity {
            id: LaunchActivityId(activity_id.to_owned()),
            installation_id: InstallationId(installation_id.to_owned()),
            action: Some(LaunchActionKind::LaunchExecutable),
            target_path: Some("Game.exe".to_owned()),
            adapter: successful.then_some(LaunchAdapter::LinuxWine),
            status: if successful {
                LaunchActivityStatus::Exited
            } else {
                LaunchActivityStatus::Failed
            },
            process_id: successful.then_some(42),
            error: (!successful).then(|| "preflight failed".to_owned()),
            attempted_at: "2026-08-08T09:59:00Z".to_owned(),
            started_at: started_at.map(str::to_owned),
            ended_at: Some("2026-08-08T10:10:00Z".to_owned()),
            duration_ms: successful.then_some(600_000),
            exit_code: successful.then_some(0),
            stop_requested_at: None,
        }
    }

    fn media_session() -> MediaSession {
        MediaSession {
            id: MediaSessionId("media-session".to_owned()),
            kind: MediaSessionKind::Work,
            installation_id: InstallationId("media".to_owned()),
            action: LaunchActionKind::PlayAudio,
            status: MediaSessionStatus::Active,
            repeat_mode: MediaRepeatMode::Off,
            shuffle: false,
            items: vec![MediaSessionItem {
                ordinal: 0,
                installation_id: InstallationId("media".to_owned()),
                work_code: Some("RJmedia".to_owned()),
                relative_path: RelativePath::parse("track.flac").expect("relative path"),
                media_type: MediaType::Audio,
                size_bytes: Some(128),
                disc_number: None,
                track_number: Some(1),
                bonus: false,
            }],
            progress: MediaProgress {
                item_ordinal: 0,
                position_ms: 15_000,
                duration_ms: Some(60_000),
                completed: false,
                updated_at: "2026-08-08T11:00:00Z".to_owned(),
            },
            opened_at: "2026-08-08T10:55:00Z".to_owned(),
            updated_at: "2026-08-08T11:00:00Z".to_owned(),
            ended_at: None,
            error: None,
        }
    }
}
