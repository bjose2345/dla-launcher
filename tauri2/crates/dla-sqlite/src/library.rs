use std::{path::Path, sync::Mutex};

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::database;

// Public builds ship the schema as of 2026-08-27 as version 1; every later
// migration follows in order, so alpha.2 libraries upgrade in place.
const LIBRARY_MIGRATION_LIST: &[M<'static>] = &[
    M::up(include_str!("../schema/library.sql")),
    M::up(include_str!(
        "../migrations/library/0019_add_delivery_jobs.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0020_add_scan_purpose_and_ignored_findings.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0021_add_own_work_details.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0022_add_game_translations.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0023_add_launch_options.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0024_add_launch_option_video_player.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0025_add_launch_option_audio_player.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0026_add_game_translation_patches.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0027_add_launch_option_window_scale.sql"
    )),
    M::up(include_str!(
        "../migrations/library/0028_add_launch_option_refresh_rate.sql"
    )),
];
const LIBRARY_MIGRATIONS: Migrations<'static> = Migrations::from_slice(LIBRARY_MIGRATION_LIST);
const STATEMENT_CACHE_CAPACITY: usize = 64;

pub struct SqliteLibraryStore {
    connection: Mutex<Connection>,
}

impl SqliteLibraryStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let connection = database::open(path, &LIBRARY_MIGRATIONS)?;
        connection.set_prepared_statement_cache_capacity(STATEMENT_CACHE_CAPACITY);
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn health_check(&self) -> rusqlite::Result<()> {
        self.with_connection(|connection| connection.query_row("SELECT 1", [], |_| Ok(())))
    }

    pub(crate) fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| rusqlite::Error::InvalidParameterName(error.to_string()))?;
        operation(&mut connection)
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn creates_and_reopens_current_empty_library_schema() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("library.sqlite");
        let now = database::now_rfc3339();
        {
            let store = SqliteLibraryStore::open(&path).expect("library store");
            let connection = store.connection.lock().expect("library lock");
            connection
                .execute(
                    "INSERT INTO library_work
                     (work_code, marker, note, progress_kind, progress_value, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    params!["DLA-SYNTH-LOCAL", "playing", "private note", "track", 4.0, now],
                )
                .expect("insert library work");
            connection
                .execute(
                    "INSERT INTO library_installation
                     (installation_id, work_code, root_path, platform, status,
                      identity_confidence, identity_reason_codes, suggested_status,
                      discovered_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, 'ready', 'strong',
                             '[\"fixture_identity\"]', 'ready', ?5, ?5)",
                    params![
                        "installation-1",
                        "DLA-SYNTH-LOCAL",
                        "/synthetic/library/work",
                        "linux",
                        now,
                    ],
                )
                .expect("insert installation");
        }

        let reopened = SqliteLibraryStore::open(&path).expect("reopened library store");
        let connection = reopened.connection.lock().expect("library lock");
        let (note, root): (String, String) = connection
            .query_row(
                "SELECT work.note, installation.root_path
                 FROM library_work work
                 JOIN library_installation installation USING (work_code)
                 WHERE work.work_code = ?1",
                ["DLA-SYNTH-LOCAL"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read library state");
        assert_eq!(note, "private note");
        assert_eq!(root, "/synthetic/library/work");
    }
}
