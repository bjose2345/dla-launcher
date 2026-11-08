use dla_application::{
    game_translation::{GameTranslationError, GameTranslationStore},
    maintenance::{LauncherFileChanges, LibraryMaintenanceError},
};
use dla_domain::{
    game_engine::GameEngine,
    installation::{InstallationId, RelativePath},
    translation::{
        GameFileChange, GameFileChangeKind, GameTranslation, InstalledPack, PreparedTranslation,
    },
};
use rusqlite::{Connection, OptionalExtension, params, types::Type};

use crate::SqliteLibraryStore;

impl GameTranslationStore for SqliteLibraryStore {
    fn read_game_translation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<GameTranslation>, GameTranslationError> {
        self.with_connection(|connection| {
            let Some(mut translation) = connection
                .query_row(
                    &format!("{SELECT_TRANSLATION} WHERE installation_id = ?1"),
                    [&installation_id.0],
                    translation_from_row,
                )
                .optional()?
            else {
                return Ok(None);
            };
            translation.changes = read_changes(connection, installation_id)?;
            Ok(Some(translation))
        })
        .map_err(GameTranslationError::persistence)
    }

    fn list_game_translations(&self) -> Result<Vec<GameTranslation>, GameTranslationError> {
        self.with_connection(|connection| {
            let mut translations = connection
                .prepare(&format!("{SELECT_TRANSLATION} ORDER BY turned_on_at"))?
                .query_map([], translation_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for translation in &mut translations {
                translation.changes = read_changes(connection, &translation.installation_id)?;
            }
            Ok(translations)
        })
        .map_err(GameTranslationError::persistence)
    }

    fn save_game_translation(
        &self,
        translation: &GameTranslation,
    ) -> Result<(), GameTranslationError> {
        let engine = engine_name(translation.engine)?;
        let prepared = translation.prepared.as_ref();
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO library_game_translation
                 (installation_id, engine, game_folder, shown, plugin, turned_on_at, paused,
                  prepared_at, prepared_with, prepared_texts, prepared_missing, pack_made,
                  pack_lines)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(installation_id) DO UPDATE SET
                     engine = excluded.engine,
                     game_folder = excluded.game_folder,
                     shown = excluded.shown,
                     plugin = excluded.plugin,
                     paused = excluded.paused,
                     prepared_at = excluded.prepared_at,
                     prepared_with = excluded.prepared_with,
                     prepared_texts = excluded.prepared_texts,
                     prepared_missing = excluded.prepared_missing,
                     pack_made = excluded.pack_made,
                     pack_lines = excluded.pack_lines",
                params![
                    translation.installation_id.0,
                    engine,
                    translation.game_folder.as_ref().map(RelativePath::as_str),
                    translation.shown,
                    translation.plugin,
                    translation.turned_on_at,
                    translation.paused,
                    prepared.map(|prepared| prepared.at.as_str()),
                    prepared.map(|prepared| prepared.with.as_str()),
                    prepared.map(|prepared| count(prepared.texts)),
                    prepared.map(|prepared| count(prepared.missing)),
                    translation.pack.as_ref().map(|pack| pack.made.as_str()),
                    translation.pack.as_ref().map(|pack| count(pack.lines)),
                ],
            )?;
            transaction.execute(
                "DELETE FROM library_game_translation_change WHERE installation_id = ?1",
                [&translation.installation_id.0],
            )?;
            {
                let mut statement = transaction.prepare_cached(
                    "INSERT INTO library_game_translation_change
                     (installation_id, path, original, written_sha256, written_bytes,
                      patched_at, original_bytes)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for change in &translation.changes {
                    let (original, written_sha256, written_bytes, patched_at, original_bytes) =
                        match &change.kind {
                            GameFileChangeKind::Added => (None, None, None, None, None),
                            GameFileChangeKind::Changed {
                                original,
                                written_sha256,
                                written_bytes,
                            } => (
                                Some(original.as_slice()),
                                Some(written_sha256.as_str()),
                                Some(count(*written_bytes)),
                                None,
                                None,
                            ),
                            GameFileChangeKind::Patched {
                                at,
                                original,
                                original_bytes,
                                written_sha256,
                                written_bytes,
                            } => (
                                Some(original.as_slice()),
                                Some(written_sha256.as_str()),
                                Some(count(*written_bytes)),
                                Some(count(*at)),
                                Some(count(*original_bytes)),
                            ),
                        };
                    statement.execute(params![
                        translation.installation_id.0,
                        change.path.as_str(),
                        original,
                        written_sha256,
                        written_bytes,
                        patched_at,
                        original_bytes,
                    ])?;
                }
            }
            transaction.commit()
        })
        .map_err(GameTranslationError::persistence)
    }

    fn remove_game_translation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<(), GameTranslationError> {
        self.with_connection(|connection| {
            connection.execute(
                "DELETE FROM library_game_translation WHERE installation_id = ?1",
                [&installation_id.0],
            )
        })
        .map(|_| ())
        .map_err(GameTranslationError::persistence)
    }
}

impl LauncherFileChanges for SqliteLibraryStore {
    fn launcher_file_changes(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Vec<GameFileChange>, LibraryMaintenanceError> {
        self.with_connection(|connection| read_changes(connection, installation_id))
            .map_err(LibraryMaintenanceError::persistence)
    }
}

const SELECT_TRANSLATION: &str = "SELECT installation_id, engine, game_folder, shown, plugin,
        turned_on_at, paused, prepared_at, prepared_with, prepared_texts, prepared_missing,
        pack_made, pack_lines
     FROM library_game_translation";

/// Everything but the changes, which come from their own table.
fn translation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameTranslation> {
    let engine = row.get::<_, String>(1)?;
    let prepared = match (
        row.get::<_, Option<String>>(7)?,
        row.get::<_, Option<String>>(8)?,
    ) {
        (Some(at), Some(with)) => Some(PreparedTranslation {
            at,
            with,
            texts: uncount(row.get(9)?),
            missing: uncount(row.get(10)?),
        }),
        _ => None,
    };
    Ok(GameTranslation {
        installation_id: InstallationId(row.get(0)?),
        engine: serde_json::from_value(serde_json::Value::String(engine))
            .map_err(|error| conversion_error(1, error))?,
        game_folder: row
            .get::<_, Option<String>>(2)?
            .map(RelativePath::parse)
            .transpose()
            .map_err(|error| conversion_error(2, error))?,
        shown: row.get(3)?,
        plugin: row.get(4)?,
        changes: Vec::new(),
        turned_on_at: row.get(5)?,
        paused: row.get(6)?,
        prepared,
        pack: row
            .get::<_, Option<String>>(11)?
            .map(|made| -> rusqlite::Result<InstalledPack> {
                Ok(InstalledPack {
                    made,
                    lines: uncount(row.get(12)?),
                })
            })
            .transpose()?,
    })
}

fn count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn uncount(value: Option<i64>) -> u64 {
    value
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or_default()
}

fn read_changes(
    connection: &Connection,
    installation_id: &InstallationId,
) -> rusqlite::Result<Vec<GameFileChange>> {
    let mut statement = connection.prepare_cached(
        "SELECT path, original, written_sha256, written_bytes, patched_at, original_bytes
         FROM library_game_translation_change
         WHERE installation_id = ?1 ORDER BY path",
    )?;
    statement
        .query_map([&installation_id.0], |row| {
            let path = RelativePath::parse(row.get::<_, String>(0)?)
                .map_err(|error| conversion_error(0, error))?;
            let written_sha256 = row.get::<_, Option<String>>(2)?.unwrap_or_default();
            let written_bytes = uncount(row.get(3)?);
            let kind = match (
                row.get::<_, Option<Vec<u8>>>(1)?,
                row.get::<_, Option<i64>>(4)?,
            ) {
                (None, _) => GameFileChangeKind::Added,
                (Some(original), None) => GameFileChangeKind::Changed {
                    original,
                    written_sha256,
                    written_bytes,
                },
                (Some(original), Some(at)) => GameFileChangeKind::Patched {
                    at: uncount(Some(at)),
                    original,
                    original_bytes: uncount(row.get(5)?),
                    written_sha256,
                    written_bytes,
                },
            };
            Ok(GameFileChange { path, kind })
        })?
        .collect()
}

fn engine_name(engine: GameEngine) -> Result<String, GameTranslationError> {
    match serde_json::to_value(engine).map_err(GameTranslationError::persistence)? {
        serde_json::Value::String(name) => Ok(name),
        _ => Err(GameTranslationError::persistence("engine has no name")),
    }
}

fn conversion_error(
    column: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use rusqlite::params;
    use tempfile::tempdir;

    use super::*;

    fn translation() -> GameTranslation {
        GameTranslation {
            installation_id: InstallationId("installation-1".to_owned()),
            engine: GameEngine::RpgMakerMv,
            game_folder: Some(RelativePath::parse("Game/www").unwrap()),
            shown: true,
            plugin: "plugin-1".to_owned(),
            changes: vec![
                GameFileChange {
                    path: RelativePath::parse("Game/www/Game.rgss3a").unwrap(),
                    kind: GameFileChangeKind::Patched {
                        at: 4369,
                        original: vec![0xcf, 0x2c, 0x6c, 0x00],
                        original_bytes: 1_397_518_180,
                        written_sha256: "def".to_owned(),
                        written_bytes: 1_397_914_211,
                    },
                },
                GameFileChange {
                    path: RelativePath::parse("Game/www/dla-translation").unwrap(),
                    kind: GameFileChangeKind::Added,
                },
                GameFileChange {
                    path: RelativePath::parse("Game/www/js/plugins.js").unwrap(),
                    kind: GameFileChangeKind::Changed {
                        original: "var $plugins = [];".as_bytes().to_vec(),
                        written_sha256: "abc".to_owned(),
                        written_bytes: 120,
                    },
                },
            ],
            turned_on_at: "2026-10-29T10:00:00Z".to_owned(),
            paused: false,
            prepared: None,
            pack: None,
        }
    }

    fn store_with_installation() -> (tempfile::TempDir, SqliteLibraryStore) {
        let directory = tempdir().unwrap();
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite")).unwrap();
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO library_work (work_code, created_at, updated_at)
                     VALUES ('RJ1', '2026-10-29T00:00:00Z', '2026-10-29T00:00:00Z')",
                    [],
                )?;
                connection.execute(
                    "INSERT INTO library_installation
                     (installation_id, work_code, root_path, platform, status,
                      identity_confidence, identity_reason_codes, suggested_status,
                      discovered_at, updated_at)
                     VALUES (?1, 'RJ1', '/library/RJ1', 'windows', 'ready', 'strong',
                             '[\"fixture_identity\"]', 'ready', ?2, ?2)",
                    params!["installation-1", "2026-10-29T00:00:00Z"],
                )
            })
            .unwrap();
        (directory, store)
    }

    #[test]
    fn keeps_every_change_with_the_original_bytes() {
        let (_directory, store) = store_with_installation();
        let id = InstallationId("installation-1".to_owned());
        assert_eq!(store.read_game_translation(&id).unwrap(), None);
        store.save_game_translation(&translation()).unwrap();
        assert_eq!(
            store.read_game_translation(&id).unwrap(),
            Some(translation())
        );
        assert_eq!(
            store.launcher_file_changes(&id).unwrap(),
            translation().changes
        );

        let mut updated = translation();
        updated.shown = false;
        updated.paused = true;
        updated.prepared = Some(PreparedTranslation {
            at: "2026-10-29T11:00:00Z".to_owned(),
            with: "qwen3:4b-instruct".to_owned(),
            texts: 875,
            missing: 2,
        });
        updated.pack = Some(InstalledPack {
            made: "2026-10-30".to_owned(),
            lines: 1551,
        });
        updated.changes.truncate(1);
        store.save_game_translation(&updated).unwrap();
        assert_eq!(
            store.read_game_translation(&id).unwrap(),
            Some(updated.clone())
        );
        assert_eq!(store.list_game_translations().unwrap(), vec![updated]);

        store.remove_game_translation(&id).unwrap();
        assert_eq!(store.read_game_translation(&id).unwrap(), None);
        assert!(store.launcher_file_changes(&id).unwrap().is_empty());
    }

    #[test]
    fn forgets_the_translation_with_the_installation() {
        let (_directory, store) = store_with_installation();
        store.save_game_translation(&translation()).unwrap();
        store
            .with_connection(|connection| {
                connection.execute(
                    "DELETE FROM library_installation WHERE installation_id = 'installation-1'",
                    [],
                )
            })
            .unwrap();
        let id = InstallationId("installation-1".to_owned());
        assert_eq!(store.read_game_translation(&id).unwrap(), None);
        assert!(store.launcher_file_changes(&id).unwrap().is_empty());
    }
}
