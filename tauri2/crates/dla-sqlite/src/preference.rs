use dla_application::personalization::{WorkPreferenceError, WorkPreferenceStore};
use dla_domain::library::{WorkPreference, WorkPreferenceKind};
use rusqlite::{OptionalExtension, params, types::Type};

use crate::SqliteLibraryStore;

impl WorkPreferenceStore for SqliteLibraryStore {
    fn read_work_preference(
        &self,
        work_code: &str,
    ) -> Result<Option<WorkPreference>, WorkPreferenceError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT work_code, preference, updated_at
                     FROM library_work_preference
                     WHERE work_code = ?1",
                    [work_code],
                    preference_from_row,
                )
                .optional()
        })
        .map_err(WorkPreferenceError::persistence)
    }

    fn list_work_preferences(&self) -> Result<Vec<WorkPreference>, WorkPreferenceError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT work_code, preference, updated_at
                 FROM library_work_preference
                 ORDER BY updated_at DESC, work_code",
            )?;
            statement
                .query_map([], preference_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(WorkPreferenceError::persistence)
    }

    fn replace_work_preference(
        &self,
        work_code: &str,
        preference: Option<WorkPreferenceKind>,
        updated_at: &str,
    ) -> Result<Option<WorkPreference>, WorkPreferenceError> {
        self.with_connection(|connection| {
            match preference {
                Some(preference) => {
                    connection.execute(
                        "INSERT INTO library_work_preference (work_code, preference, updated_at)
                         VALUES (?1, ?2, ?3)
                         ON CONFLICT(work_code) DO UPDATE SET
                             preference = excluded.preference,
                             updated_at = excluded.updated_at",
                        params![work_code, preference_value(preference), updated_at],
                    )?;
                }
                None => {
                    connection.execute(
                        "DELETE FROM library_work_preference WHERE work_code = ?1",
                        [work_code],
                    )?;
                }
            }
            connection
                .query_row(
                    "SELECT work_code, preference, updated_at
                     FROM library_work_preference
                     WHERE work_code = ?1",
                    [work_code],
                    preference_from_row,
                )
                .optional()
        })
        .map_err(WorkPreferenceError::persistence)
    }
}

fn preference_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkPreference> {
    let value = row.get::<_, String>(1)?;
    Ok(WorkPreference {
        work_code: row.get(0)?,
        preference: parse_preference(1, &value)?,
        updated_at: row.get(2)?,
    })
}

fn preference_value(preference: WorkPreferenceKind) -> &'static str {
    match preference {
        WorkPreferenceKind::Favorite => "favorite",
        WorkPreferenceKind::NotInterested => "not_interested",
    }
}

fn parse_preference(column: usize, value: &str) -> rusqlite::Result<WorkPreferenceKind> {
    match value {
        "favorite" => Ok(WorkPreferenceKind::Favorite),
        "not_interested" => Ok(WorkPreferenceKind::NotInterested),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            column,
            Type::Text,
            format!("invalid work preference: {value}").into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn preferences_survive_reopen_and_replace_without_catalog_rows() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("library.sqlite");
        {
            let store = SqliteLibraryStore::open(&path).expect("library store");
            let favorite = store
                .replace_work_preference(
                    "rj01326398",
                    Some(WorkPreferenceKind::Favorite),
                    "2026-08-09T10:00:00Z",
                )
                .expect("favorite")
                .expect("stored favorite");
            assert_eq!(favorite.work_code, "rj01326398");
            assert_eq!(favorite.preference, WorkPreferenceKind::Favorite);
            store
                .replace_work_preference(
                    "RJ01326398",
                    Some(WorkPreferenceKind::NotInterested),
                    "2026-08-09T11:00:00Z",
                )
                .expect("replace preference");
        }

        let store = SqliteLibraryStore::open(&path).expect("reopened library store");
        let stored = store
            .read_work_preference("RJ01326398")
            .expect("read preference")
            .expect("stored preference");
        assert_eq!(stored.preference, WorkPreferenceKind::NotInterested);
        assert_eq!(store.list_work_preferences().expect("list").len(), 1);
        assert!(
            store
                .replace_work_preference("RJ01326398", None, "2026-08-09T12:00:00Z")
                .expect("clear")
                .is_none()
        );
    }
}
