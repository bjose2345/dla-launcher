use rusqlite::params;

use crate::SqliteLibraryStore;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HistoryPruneReport {
    pub scan_sessions: usize,
    pub media_sessions: usize,
}

impl SqliteLibraryStore {
    /// Removes finished scan and media sessions last touched before
    /// `finished_before`, keeping the newest session of every scan root and of
    /// every installation, which the scanner page and the library shelves
    /// still show. Scan entries, resume positions, queue settings and launch
    /// activity are state rather than history and stay.
    pub fn prune_history(&self, finished_before: &str) -> rusqlite::Result<HistoryPruneReport> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let scan_sessions = transaction.execute(
                "DELETE FROM library_scan_session
                 WHERE status IN ('completed', 'cancelled', 'interrupted', 'failed')
                   AND finished_at < ?1
                   AND session_id NOT IN (
                       SELECT session_id FROM (
                           SELECT session_id,
                                  ROW_NUMBER() OVER (
                                      PARTITION BY root_id
                                      ORDER BY started_at DESC, session_id DESC
                                  ) AS session_rank
                           FROM library_scan_session
                       )
                       WHERE session_rank = 1
                   )",
                params![finished_before],
            )?;
            // The shelves rank sessions per installation through their current
            // item; the newest by that rule is what they show, so it stays.
            let media_sessions = transaction.execute(
                "DELETE FROM library_media_session
                 WHERE status IN ('completed', 'closed', 'interrupted', 'failed')
                   AND updated_at < ?1
                   AND session_id NOT IN (
                       SELECT session_id FROM (
                           SELECT session.session_id,
                                  ROW_NUMBER() OVER (
                                      PARTITION BY COALESCE(item.installation_id, session.installation_id)
                                      ORDER BY session.updated_at DESC, session.session_id DESC
                                  ) AS session_rank
                           FROM library_media_session session
                           LEFT JOIN library_media_session_item item
                             ON item.session_id = session.session_id
                            AND item.ordinal = session.current_item_ordinal
                       )
                       WHERE session_rank = 1
                   )",
                params![finished_before],
            )?;
            transaction.commit()?;
            Ok(HistoryPruneReport {
                scan_sessions,
                media_sessions,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use dla_application::{
        installation::InstallationStore, media::MediaSessionStore, scanner::ScanRepository,
    };
    use dla_domain::{
        installation::{
            Installation, InstallationDetection, InstallationId, InstallationOverrides,
            InstallationPlatform, InstallationStatus, LaunchActionKind, MediaType, RelativePath,
        },
        media::{
            MediaProgress, MediaRepeatMode, MediaSession, MediaSessionId, MediaSessionItem,
            MediaSessionKind, MediaSessionStatus,
        },
        scanner::{
            ScanCounters, ScanOptions, ScanRoot, ScanRootId, ScanSession, ScanSessionId, ScanStatus,
        },
    };
    use tempfile::tempdir;

    use super::*;

    const CUTOFF: &str = "2026-08-01T00:00:00Z";

    #[test]
    fn prunes_old_finished_scan_sessions_but_keeps_each_roots_newest() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        store.save_root(&scan_root("root-1")).expect("root one");
        store.save_root(&scan_root("root-2")).expect("root two");
        let older = scan_session("session-older", "root-1", "2026-07-01T00:00:00Z", true);
        let newer = scan_session("session-newer", "root-1", "2026-07-02T00:00:00Z", true);
        let running = scan_session("session-running", "root-1", "2026-07-03T00:00:00Z", false);
        let only = scan_session("session-only", "root-2", "2026-06-01T00:00:00Z", true);
        for session in [&older, &newer, &running, &only] {
            store.begin_session(session).expect("begin session");
        }

        let report = store.prune_history(CUTOFF).expect("prune");

        assert_eq!(report.scan_sessions, 2);
        let stored = |id: &ScanSessionId| store.read_session(id).expect("read session").is_some();
        assert!(!stored(&older.id));
        assert!(!stored(&newer.id));
        assert!(stored(&running.id));
        assert!(stored(&only.id));
    }

    #[test]
    fn prunes_old_closed_media_sessions_but_keeps_the_installations_newest() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let installation_id = InstallationId("installation-media".to_owned());
        store
            .create(&installation(&installation_id))
            .expect("installation");
        let older = media_session(
            "media-older",
            &installation_id,
            "2026-06-01T00:00:00Z",
            MediaSessionStatus::Closed,
        );
        let newer = media_session(
            "media-newer",
            &installation_id,
            "2026-06-02T00:00:00Z",
            MediaSessionStatus::Closed,
        );
        let paused = media_session(
            "media-paused",
            &installation_id,
            "2026-05-01T00:00:00Z",
            MediaSessionStatus::Paused,
        );
        for session in [&older, &newer, &paused] {
            store
                .create_media_session(session)
                .expect("create media session");
        }

        let report = store.prune_history(CUTOFF).expect("prune");

        assert_eq!(report.media_sessions, 1);
        let stored = |id: &MediaSessionId| {
            store
                .read_media_session(id)
                .expect("read session")
                .is_some()
        };
        assert!(!stored(&older.id));
        assert!(stored(&newer.id));
        assert!(stored(&paused.id));
        assert_eq!(
            store.prune_history(CUTOFF).expect("second prune"),
            HistoryPruneReport::default()
        );
    }

    fn scan_root(id: &str) -> ScanRoot {
        ScanRoot {
            id: ScanRootId(id.to_owned()),
            platform: "linux".to_owned(),
            path_key: format!("/fixtures/{id}"),
            display_path: format!("/fixtures/{id}"),
            created_at: "2026-06-01T00:00:00Z".to_owned(),
            updated_at: "2026-06-01T00:00:00Z".to_owned(),
        }
    }

    fn scan_session(id: &str, root_id: &str, started_at: &str, finished: bool) -> ScanSession {
        ScanSession {
            id: ScanSessionId(id.to_owned()),
            root_id: ScanRootId(root_id.to_owned()),
            status: if finished {
                ScanStatus::Completed
            } else {
                ScanStatus::Running
            },
            options: ScanOptions::default(),
            counters: ScanCounters::default(),
            started_at: started_at.to_owned(),
            finished_at: finished.then(|| started_at.to_owned()),
            fatal_error_code: None,
            fatal_error_message: None,
        }
    }

    fn installation(id: &InstallationId) -> Installation {
        Installation {
            id: id.clone(),
            scan_root_id: None,
            root_path: "/synthetic/audio".to_owned(),
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
            overrides: InstallationOverrides {
                reviewed_at: Some("2026-05-01T00:00:00Z".to_owned()),
                ..InstallationOverrides::default()
            },
            discovered_at: "2026-05-01T00:00:00Z".to_owned(),
            updated_at: "2026-05-01T00:00:00Z".to_owned(),
        }
    }

    fn media_session(
        id: &str,
        installation_id: &InstallationId,
        updated_at: &str,
        status: MediaSessionStatus,
    ) -> MediaSession {
        let open = matches!(
            status,
            MediaSessionStatus::Active | MediaSessionStatus::Paused
        );
        MediaSession {
            id: MediaSessionId(id.to_owned()),
            kind: MediaSessionKind::Work,
            installation_id: installation_id.clone(),
            action: LaunchActionKind::PlayAudio,
            status,
            repeat_mode: MediaRepeatMode::Off,
            shuffle: false,
            items: vec![MediaSessionItem {
                ordinal: 0,
                installation_id: installation_id.clone(),
                work_code: None,
                relative_path: RelativePath::parse("disc/track01.flac").expect("path"),
                media_type: MediaType::Audio,
                size_bytes: Some(1024),
                disc_number: None,
                track_number: Some(1),
                bonus: false,
            }],
            progress: MediaProgress {
                item_ordinal: 0,
                position_ms: 0,
                duration_ms: None,
                completed: false,
                updated_at: updated_at.to_owned(),
            },
            opened_at: updated_at.to_owned(),
            updated_at: updated_at.to_owned(),
            ended_at: (!open).then(|| updated_at.to_owned()),
            error: None,
        }
    }
}
