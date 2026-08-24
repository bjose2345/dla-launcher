use std::io;

use dla_application::media::{AudioTrackStore, MediaError, MediaSessionStore};
use dla_domain::{
    installation::{InstallationId, LaunchActionKind, RelativePath},
    media::{
        IndexedAudioTrack, MediaProgress, MediaQueueState, MediaRepeatMode, MediaResume,
        MediaSession, MediaSessionId, MediaSessionItem, MediaSessionKind, MediaSessionStatus,
    },
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params, types::Type};

use crate::{
    SqliteLibraryStore,
    installation::{media_type, parse_media_type},
    launch::{action_kind, parse_action},
};

const SESSION_COLUMNS: &str = "session_id, session_kind, installation_id, action_kind, status, \
     repeat_mode, shuffle_enabled, current_item_ordinal, position_ms, duration_ms, completed, \
     opened_at, updated_at, ended_at, error";

impl MediaSessionStore for SqliteLibraryStore {
    fn create_media_session(&self, session: &MediaSession) -> Result<(), MediaError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            insert_session(&transaction, session)?;
            for item in &session.items {
                transaction.execute(
                    "INSERT INTO library_media_session_item
                     (session_id, ordinal, installation_id, work_code, relative_path, media_type,
                      size_bytes, disc_number, track_number, bonus)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        session.id.0.as_str(),
                        i64::from(item.ordinal),
                        item.installation_id.0.as_str(),
                        item.work_code.as_deref(),
                        item.relative_path.as_str(),
                        media_type(item.media_type),
                        optional_u64(item.size_bytes)?,
                        item.disc_number.map(i64::from),
                        item.track_number.map(i64::from),
                        item.bonus,
                    ],
                )?;
            }
            upsert_resume(&transaction, session)?;
            upsert_queue_state(&transaction, session)?;
            transaction.commit()
        })
        .map_err(MediaError::persistence)
    }

    fn save_media_session(&self, session: &MediaSession) -> Result<(), MediaError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let changed = transaction.execute(
                "UPDATE library_media_session
                 SET status = ?2,
                     repeat_mode = ?3,
                     shuffle_enabled = ?4,
                     current_item_ordinal = ?5,
                     position_ms = ?6,
                     duration_ms = ?7,
                     completed = ?8,
                     updated_at = ?9,
                     ended_at = ?10,
                     error = ?11
                 WHERE session_id = ?1",
                params![
                    session.id.0.as_str(),
                    session_status(session.status),
                    repeat_mode(session.repeat_mode),
                    session.shuffle,
                    i64::from(session.progress.item_ordinal),
                    required_u64(session.progress.position_ms)?,
                    optional_u64(session.progress.duration_ms)?,
                    session.progress.completed,
                    session.updated_at.as_str(),
                    session.ended_at.as_deref(),
                    session.error.as_deref(),
                ],
            )?;
            if changed == 0 {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            upsert_resume(&transaction, session)?;
            upsert_queue_state(&transaction, session)?;
            transaction.commit()
        })
        .map_err(MediaError::persistence)
    }

    fn read_media_session(
        &self,
        session_id: &MediaSessionId,
    ) -> Result<Option<MediaSession>, MediaError> {
        self.with_connection(|connection| read_session(connection, session_id.0.as_str()))
            .map_err(MediaError::persistence)
    }

    fn read_open_media_session(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<MediaSession>, MediaError> {
        self.with_connection(|connection| {
            let session_id = connection
                .query_row(
                    "SELECT session_id
                     FROM library_media_session
                     WHERE session_kind = 'work'
                       AND installation_id = ?1
                       AND status IN ('active', 'paused')
                     LIMIT 1",
                    [installation_id.0.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            session_id
                .map(|session_id| read_session(connection, &session_id))
                .transpose()
                .map(Option::flatten)
        })
        .map_err(MediaError::persistence)
    }

    fn read_open_personalized_media_session(&self) -> Result<Option<MediaSession>, MediaError> {
        self.with_connection(|connection| {
            let session_id = connection
                .query_row(
                    "SELECT session_id
                     FROM library_media_session
                     WHERE session_kind = 'personalized_voice'
                       AND status IN ('active', 'paused')
                     LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            session_id
                .map(|session_id| read_session(connection, &session_id))
                .transpose()
                .map(Option::flatten)
        })
        .map_err(MediaError::persistence)
    }

    fn read_media_queue_state(
        &self,
        kind: MediaSessionKind,
        installation_id: Option<&InstallationId>,
    ) -> Result<Option<MediaQueueState>, MediaError> {
        let scope_key = queue_scope_key(kind, installation_id)?;
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT session_kind, installation_id, current_installation_id,
                            current_relative_path, position_ms, duration_ms, completed,
                            repeat_mode, shuffle_enabled, updated_at
                     FROM library_media_queue_state
                     WHERE scope_key = ?1",
                    [scope_key],
                    queue_state_from_row,
                )
                .optional()
        })
        .map_err(MediaError::persistence)
    }

    fn read_media_resume(
        &self,
        installation_id: &InstallationId,
        action: LaunchActionKind,
    ) -> Result<Option<MediaResume>, MediaError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT installation_id, action_kind, relative_path, position_ms,
                            duration_ms, completed, updated_at
                     FROM library_media_resume
                     WHERE installation_id = ?1 AND action_kind = ?2",
                    params![installation_id.0.as_str(), action_kind(action)],
                    resume_from_row,
                )
                .optional()
        })
        .map_err(MediaError::persistence)
    }

    fn interrupt_open_media_sessions(
        &self,
        interrupted_at: &str,
        reason: &str,
    ) -> Result<u64, MediaError> {
        self.with_connection(|connection| {
            connection
                .execute(
                    "UPDATE library_media_session
                     SET status = 'interrupted', updated_at = ?1, ended_at = ?1, error = ?2
                     WHERE status IN ('active', 'paused')",
                    params![interrupted_at, reason],
                )
                .map(|count| count as u64)
        })
        .map_err(MediaError::persistence)
    }
}

impl AudioTrackStore for SqliteLibraryStore {
    fn replace_audio_tracks(
        &self,
        installation_id: &InstallationId,
        tracks: &[IndexedAudioTrack],
    ) -> Result<(), MediaError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "DELETE FROM library_audio_track WHERE installation_id = ?1",
                [installation_id.0.as_str()],
            )?;
            for track in tracks {
                if track.installation_id != *installation_id {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "audio track belongs to another installation".to_owned(),
                    ));
                }
                transaction.execute(
                    "INSERT INTO library_audio_track
                     (installation_id, work_code, relative_path, size_bytes, disc_number,
                      track_number, bonus, sort_order, indexed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        track.installation_id.0.as_str(),
                        track.work_code.as_deref(),
                        track.relative_path.as_str(),
                        optional_u64(track.size_bytes)?,
                        track.disc_number.map(i64::from),
                        track.track_number.map(i64::from),
                        track.bonus,
                        i64::from(track.sort_order),
                        track.indexed_at.as_str(),
                    ],
                )?;
            }
            transaction.commit()
        })
        .map_err(MediaError::persistence)
    }

    fn list_audio_tracks(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Vec<IndexedAudioTrack>, MediaError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT installation_id, work_code, relative_path, size_bytes, disc_number,
                        track_number, bonus, sort_order, indexed_at
                 FROM library_audio_track
                 WHERE installation_id = ?1
                 ORDER BY sort_order, relative_path",
            )?;
            statement
                .query_map([installation_id.0.as_str()], indexed_audio_track_from_row)?
                .collect()
        })
        .map_err(MediaError::persistence)
    }

    fn list_all_audio_tracks(&self) -> Result<Vec<IndexedAudioTrack>, MediaError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT installation_id, work_code, relative_path, size_bytes, disc_number,
                        track_number, bonus, sort_order, indexed_at
                 FROM library_audio_track
                 ORDER BY installation_id, sort_order, relative_path",
            )?;
            statement
                .query_map([], indexed_audio_track_from_row)?
                .collect()
        })
        .map_err(MediaError::persistence)
    }
}

fn insert_session(transaction: &Transaction<'_>, session: &MediaSession) -> rusqlite::Result<()> {
    transaction.execute(
        &format!(
            "INSERT INTO library_media_session ({SESSION_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"
        ),
        params![
            session.id.0.as_str(),
            session_kind(session.kind),
            session.installation_id.0.as_str(),
            action_kind(session.action),
            session_status(session.status),
            repeat_mode(session.repeat_mode),
            session.shuffle,
            i64::from(session.progress.item_ordinal),
            required_u64(session.progress.position_ms)?,
            optional_u64(session.progress.duration_ms)?,
            session.progress.completed,
            session.opened_at.as_str(),
            session.updated_at.as_str(),
            session.ended_at.as_deref(),
            session.error.as_deref(),
        ],
    )?;
    Ok(())
}

fn upsert_resume(transaction: &Transaction<'_>, session: &MediaSession) -> rusqlite::Result<()> {
    let item = session
        .items
        .iter()
        .find(|item| item.ordinal == session.progress.item_ordinal)
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    transaction.execute(
        "INSERT INTO library_media_resume
         (installation_id, action_kind, relative_path, position_ms, duration_ms, completed, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(installation_id, action_kind) DO UPDATE SET
             relative_path = excluded.relative_path,
             position_ms = excluded.position_ms,
             duration_ms = excluded.duration_ms,
             completed = excluded.completed,
             updated_at = excluded.updated_at",
        params![
            item.installation_id.0.as_str(),
            action_kind(session.action),
            item.relative_path.as_str(),
            required_u64(session.progress.position_ms)?,
            optional_u64(session.progress.duration_ms)?,
            session.progress.completed,
            session.progress.updated_at.as_str(),
        ],
    )?;
    Ok(())
}

fn upsert_queue_state(
    transaction: &Transaction<'_>,
    session: &MediaSession,
) -> rusqlite::Result<()> {
    let item = session
        .items
        .iter()
        .find(|item| item.ordinal == session.progress.item_ordinal)
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    let installation_id =
        (session.kind == MediaSessionKind::Work).then_some(session.installation_id.0.as_str());
    let scope_key = queue_scope_key(session.kind, Some(&session.installation_id))
        .map_err(|error| rusqlite::Error::InvalidParameterName(error.to_string()))?;
    transaction.execute(
        "INSERT INTO library_media_queue_state
         (scope_key, session_kind, installation_id, current_installation_id,
          current_relative_path, position_ms, duration_ms, completed, repeat_mode,
          shuffle_enabled, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(scope_key) DO UPDATE SET
             current_installation_id = excluded.current_installation_id,
             current_relative_path = excluded.current_relative_path,
             position_ms = excluded.position_ms,
             duration_ms = excluded.duration_ms,
             completed = excluded.completed,
             repeat_mode = excluded.repeat_mode,
             shuffle_enabled = excluded.shuffle_enabled,
             updated_at = excluded.updated_at",
        params![
            scope_key,
            session_kind(session.kind),
            installation_id,
            item.installation_id.0.as_str(),
            item.relative_path.as_str(),
            required_u64(session.progress.position_ms)?,
            optional_u64(session.progress.duration_ms)?,
            session.progress.completed,
            repeat_mode(session.repeat_mode),
            session.shuffle,
            session.progress.updated_at.as_str(),
        ],
    )?;
    Ok(())
}

fn read_session(
    connection: &Connection,
    session_id: &str,
) -> rusqlite::Result<Option<MediaSession>> {
    let stored = connection
        .query_row(
            &format!(
                "SELECT {SESSION_COLUMNS}
                 FROM library_media_session
                 WHERE session_id = ?1"
            ),
            [session_id],
            stored_session_from_row,
        )
        .optional()?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT ordinal, installation_id, work_code, relative_path, media_type, size_bytes,
                disc_number, track_number, bonus
         FROM library_media_session_item
         WHERE session_id = ?1
         ORDER BY ordinal",
    )?;
    let items = statement
        .query_map([session_id], session_item_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(MediaSession {
        id: stored.id,
        kind: stored.kind,
        installation_id: stored.installation_id,
        action: stored.action,
        status: stored.status,
        repeat_mode: stored.repeat_mode,
        shuffle: stored.shuffle,
        items,
        progress: stored.progress,
        opened_at: stored.opened_at,
        updated_at: stored.updated_at,
        ended_at: stored.ended_at,
        error: stored.error,
    }))
}

struct StoredSession {
    id: MediaSessionId,
    kind: MediaSessionKind,
    installation_id: InstallationId,
    action: LaunchActionKind,
    status: MediaSessionStatus,
    repeat_mode: MediaRepeatMode,
    shuffle: bool,
    progress: MediaProgress,
    opened_at: String,
    updated_at: String,
    ended_at: Option<String>,
    error: Option<String>,
}

fn stored_session_from_row(row: &Row<'_>) -> rusqlite::Result<StoredSession> {
    let updated_at = row.get::<_, String>(12)?;
    Ok(StoredSession {
        id: MediaSessionId(row.get(0)?),
        kind: parse_session_kind(1, &row.get::<_, String>(1)?)?,
        installation_id: InstallationId(row.get(2)?),
        action: parse_action(3, &row.get::<_, String>(3)?)?,
        status: parse_session_status(4, &row.get::<_, String>(4)?)?,
        repeat_mode: parse_repeat_mode(5, &row.get::<_, String>(5)?)?,
        shuffle: row.get(6)?,
        progress: MediaProgress {
            item_ordinal: numeric_conversion(7, row.get(7)?, u32::try_from)?,
            position_ms: numeric_conversion(8, row.get(8)?, u64::try_from)?,
            duration_ms: row
                .get::<_, Option<i64>>(9)?
                .map(|value| numeric_conversion(9, value, u64::try_from))
                .transpose()?,
            completed: row.get(10)?,
            updated_at: updated_at.clone(),
        },
        opened_at: row.get(11)?,
        updated_at,
        ended_at: row.get(13)?,
        error: row.get(14)?,
    })
}

fn session_item_from_row(row: &Row<'_>) -> rusqlite::Result<MediaSessionItem> {
    Ok(MediaSessionItem {
        ordinal: numeric_conversion(0, row.get(0)?, u32::try_from)?,
        installation_id: InstallationId(row.get(1)?),
        work_code: row.get(2)?,
        relative_path: parse_relative_path(3, row.get(3)?)?,
        media_type: parse_media_type(4, &row.get::<_, String>(4)?)?,
        size_bytes: row
            .get::<_, Option<i64>>(5)?
            .map(|value| numeric_conversion(5, value, u64::try_from))
            .transpose()?,
        disc_number: row
            .get::<_, Option<i64>>(6)?
            .map(|value| numeric_conversion(6, value, u32::try_from))
            .transpose()?,
        track_number: row
            .get::<_, Option<i64>>(7)?
            .map(|value| numeric_conversion(7, value, u32::try_from))
            .transpose()?,
        bonus: row.get(8)?,
    })
}

fn indexed_audio_track_from_row(row: &Row<'_>) -> rusqlite::Result<IndexedAudioTrack> {
    Ok(IndexedAudioTrack {
        installation_id: InstallationId(row.get(0)?),
        work_code: row.get(1)?,
        relative_path: parse_relative_path(2, row.get(2)?)?,
        size_bytes: row
            .get::<_, Option<i64>>(3)?
            .map(|value| numeric_conversion(3, value, u64::try_from))
            .transpose()?,
        disc_number: row
            .get::<_, Option<i64>>(4)?
            .map(|value| numeric_conversion(4, value, u32::try_from))
            .transpose()?,
        track_number: row
            .get::<_, Option<i64>>(5)?
            .map(|value| numeric_conversion(5, value, u32::try_from))
            .transpose()?,
        bonus: row.get(6)?,
        sort_order: numeric_conversion(7, row.get(7)?, u32::try_from)?,
        indexed_at: row.get(8)?,
    })
}

fn queue_state_from_row(row: &Row<'_>) -> rusqlite::Result<MediaQueueState> {
    Ok(MediaQueueState {
        kind: parse_session_kind(0, &row.get::<_, String>(0)?)?,
        installation_id: row.get::<_, Option<String>>(1)?.map(InstallationId),
        current_installation_id: InstallationId(row.get(2)?),
        current_relative_path: parse_relative_path(3, row.get(3)?)?,
        position_ms: numeric_conversion(4, row.get(4)?, u64::try_from)?,
        duration_ms: row
            .get::<_, Option<i64>>(5)?
            .map(|value| numeric_conversion(5, value, u64::try_from))
            .transpose()?,
        completed: row.get(6)?,
        repeat_mode: parse_repeat_mode(7, &row.get::<_, String>(7)?)?,
        shuffle: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

pub(crate) fn resume_from_row(row: &Row<'_>) -> rusqlite::Result<MediaResume> {
    Ok(MediaResume {
        installation_id: InstallationId(row.get(0)?),
        action: parse_action(1, &row.get::<_, String>(1)?)?,
        relative_path: parse_relative_path(2, row.get(2)?)?,
        position_ms: numeric_conversion(3, row.get(3)?, u64::try_from)?,
        duration_ms: row
            .get::<_, Option<i64>>(4)?
            .map(|value| numeric_conversion(4, value, u64::try_from))
            .transpose()?,
        completed: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn parse_relative_path(column: usize, value: String) -> rusqlite::Result<RelativePath> {
    RelativePath::parse(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
    })
}

fn required_u64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

fn optional_u64(value: Option<u64>) -> rusqlite::Result<Option<i64>> {
    value.map(required_u64).transpose()
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

fn session_status(value: MediaSessionStatus) -> &'static str {
    match value {
        MediaSessionStatus::Active => "active",
        MediaSessionStatus::Paused => "paused",
        MediaSessionStatus::Completed => "completed",
        MediaSessionStatus::Closed => "closed",
        MediaSessionStatus::Interrupted => "interrupted",
        MediaSessionStatus::Failed => "failed",
    }
}

fn session_kind(value: MediaSessionKind) -> &'static str {
    match value {
        MediaSessionKind::Work => "work",
        MediaSessionKind::PersonalizedVoice => "personalized_voice",
    }
}

fn parse_session_kind(column: usize, value: &str) -> rusqlite::Result<MediaSessionKind> {
    match value {
        "work" => Ok(MediaSessionKind::Work),
        "personalized_voice" => Ok(MediaSessionKind::PersonalizedVoice),
        _ => invalid_text(column, format!("unknown media session kind: {value}")),
    }
}

fn repeat_mode(value: MediaRepeatMode) -> &'static str {
    match value {
        MediaRepeatMode::Off => "off",
        MediaRepeatMode::All => "all",
        MediaRepeatMode::One => "one",
    }
}

fn parse_repeat_mode(column: usize, value: &str) -> rusqlite::Result<MediaRepeatMode> {
    match value {
        "off" => Ok(MediaRepeatMode::Off),
        "all" => Ok(MediaRepeatMode::All),
        "one" => Ok(MediaRepeatMode::One),
        _ => invalid_text(column, format!("unknown media repeat mode: {value}")),
    }
}

fn queue_scope_key(
    kind: MediaSessionKind,
    installation_id: Option<&InstallationId>,
) -> Result<String, MediaError> {
    match kind {
        MediaSessionKind::Work => installation_id
            .map(|installation_id| format!("work:{}", installation_id.0))
            .ok_or(MediaError::InvalidRequest("queue installation ID")),
        MediaSessionKind::PersonalizedVoice => Ok("personalized_voice".to_owned()),
    }
}

fn parse_session_status(column: usize, value: &str) -> rusqlite::Result<MediaSessionStatus> {
    match value {
        "active" => Ok(MediaSessionStatus::Active),
        "paused" => Ok(MediaSessionStatus::Paused),
        "completed" => Ok(MediaSessionStatus::Completed),
        "closed" => Ok(MediaSessionStatus::Closed),
        "interrupted" => Ok(MediaSessionStatus::Interrupted),
        "failed" => Ok(MediaSessionStatus::Failed),
        _ => invalid_text(column, format!("unknown media session status: {value}")),
    }
}

fn invalid_text<T>(column: usize, message: String) -> rusqlite::Result<T> {
    Err(rusqlite::Error::FromSqlConversionFailure(
        column,
        Type::Text,
        Box::new(io::Error::new(io::ErrorKind::InvalidData, message)),
    ))
}

#[cfg(test)]
mod tests {
    use dla_application::{
        installation::InstallationStore,
        media::{AudioTrackStore, MediaSessionStore},
    };
    use dla_domain::installation::{
        Installation, InstallationDetection, InstallationOverrides, InstallationPlatform,
        InstallationStatus, MediaType,
    };
    use tempfile::tempdir;

    use super::*;

    fn session_fixture(store: &SqliteLibraryStore) -> MediaSession {
        let installation_id = InstallationId("installation-media".to_owned());
        store
            .create(&Installation {
                id: installation_id.clone(),
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
                    reviewed_at: Some("2026-08-09T00:00:00Z".to_owned()),
                    ..InstallationOverrides::default()
                },
                discovered_at: "2026-08-09T00:00:00Z".to_owned(),
                updated_at: "2026-08-09T00:00:00Z".to_owned(),
            })
            .expect("installation");
        MediaSession {
            id: MediaSessionId("media-1".to_owned()),
            kind: MediaSessionKind::Work,
            installation_id,
            action: LaunchActionKind::PlayAudio,
            status: MediaSessionStatus::Active,
            repeat_mode: MediaRepeatMode::Off,
            shuffle: false,
            items: vec![MediaSessionItem {
                ordinal: 0,
                installation_id: InstallationId("installation-media".to_owned()),
                work_code: Some("RJMEDIA".to_owned()),
                relative_path: RelativePath::parse("disc/track01.flac").expect("path"),
                media_type: MediaType::Audio,
                size_bytes: Some(1024),
                disc_number: Some(1),
                track_number: Some(1),
                bonus: false,
            }],
            progress: MediaProgress {
                item_ordinal: 0,
                position_ms: 0,
                duration_ms: None,
                completed: false,
                updated_at: "2026-08-09T00:01:00Z".to_owned(),
            },
            opened_at: "2026-08-09T00:01:00Z".to_owned(),
            updated_at: "2026-08-09T00:01:00Z".to_owned(),
            ended_at: None,
            error: None,
        }
    }

    #[test]
    fn media_session_round_trips_and_updates_resume() {
        let directory = tempdir().expect("temporary directory");
        let database_path = directory.path().join("library.sqlite");
        let store = SqliteLibraryStore::open(&database_path).expect("library store");
        let mut session = session_fixture(&store);
        store
            .create_media_session(&session)
            .expect("create session");
        session.status = MediaSessionStatus::Paused;
        session.repeat_mode = MediaRepeatMode::All;
        session.shuffle = true;
        session.progress.position_ms = 42_000;
        session.progress.duration_ms = Some(180_000);
        session.progress.updated_at = "2026-08-09T00:02:00Z".to_owned();
        session.updated_at = session.progress.updated_at.clone();
        store.save_media_session(&session).expect("save session");

        assert_eq!(
            store.read_media_session(&session.id).expect("read session"),
            Some(session.clone())
        );
        let resume = store
            .read_media_resume(&session.installation_id, session.action)
            .expect("read resume")
            .expect("resume");
        assert_eq!(resume.position_ms, 42_000);
        assert_eq!(resume.duration_ms, Some(180_000));
        assert_eq!(resume.relative_path.as_str(), "disc/track01.flac");
        let queue = store
            .read_media_queue_state(MediaSessionKind::Work, Some(&session.installation_id))
            .expect("read queue")
            .expect("queue state");
        assert_eq!(queue.position_ms, 42_000);
        assert_eq!(queue.current_relative_path.as_str(), "disc/track01.flac");
        assert_eq!(queue.repeat_mode, MediaRepeatMode::All);
        assert!(queue.shuffle);

        drop(store);
        let reopened = SqliteLibraryStore::open(&database_path).expect("reopened library store");
        let restored = reopened
            .read_media_queue_state(MediaSessionKind::Work, Some(&session.installation_id))
            .expect("read restored queue")
            .expect("restored queue state");
        assert_eq!(restored.position_ms, 42_000);
        assert_eq!(restored.repeat_mode, MediaRepeatMode::All);
        assert!(restored.shuffle);
    }

    #[test]
    fn indexed_audio_tracks_replace_atomically_and_round_trip_ordering_metadata() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let session = session_fixture(&store);
        let tracks = [IndexedAudioTrack {
            installation_id: session.installation_id.clone(),
            work_code: Some("RJMEDIA".to_owned()),
            relative_path: RelativePath::parse("disc/track01.flac").expect("path"),
            size_bytes: Some(1024),
            disc_number: Some(1),
            track_number: Some(1),
            bonus: false,
            sort_order: 0,
            indexed_at: "2026-08-10T00:00:00Z".to_owned(),
        }];

        store
            .replace_audio_tracks(&session.installation_id, &tracks)
            .expect("replace tracks");

        assert_eq!(
            store
                .list_audio_tracks(&session.installation_id)
                .expect("list tracks"),
            tracks
        );
        assert_eq!(store.list_all_audio_tracks().expect("all tracks"), tracks);
    }

    #[test]
    fn restart_reconciliation_preserves_progress_and_closes_open_session() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let session = session_fixture(&store);
        store
            .create_media_session(&session)
            .expect("create session");

        assert_eq!(
            store
                .interrupt_open_media_sessions("2026-08-09T01:00:00Z", "restart")
                .expect("interrupt"),
            1
        );
        let interrupted = store
            .read_media_session(&session.id)
            .expect("read session")
            .expect("session");
        assert_eq!(interrupted.status, MediaSessionStatus::Interrupted);
        assert_eq!(interrupted.error.as_deref(), Some("restart"));
        assert_eq!(interrupted.progress.position_ms, 0);
        assert_eq!(
            store
                .read_open_media_session(&session.installation_id)
                .expect("open session"),
            None
        );
    }

    #[test]
    fn database_rejects_inconsistent_completed_session_state() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let mut session = session_fixture(&store);
        session.status = MediaSessionStatus::Completed;
        session.ended_at = Some("2026-08-09T00:02:00Z".to_owned());

        let error = store
            .create_media_session(&session)
            .expect_err("completed status requires completed progress");

        assert!(matches!(error, MediaError::Persistence(_)));
        assert_eq!(
            store.read_media_session(&session.id).expect("read session"),
            None
        );
    }

    #[test]
    fn oversized_media_values_fail_instead_of_becoming_null() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let mut session = session_fixture(&store);
        session.items[0].size_bytes = Some(u64::MAX);

        let error = store
            .create_media_session(&session)
            .expect_err("SQLite cannot represent an unsigned 64-bit maximum");

        assert!(matches!(error, MediaError::Persistence(_)));
        assert_eq!(
            store.read_media_session(&session.id).expect("read session"),
            None
        );
    }
}
