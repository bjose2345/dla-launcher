use dla_application::own_work::{OwnWorkDetailsStore, OwnWorkError};
use dla_domain::{
    installation::InstallationId,
    own_work::{OwnWorkDetails, OwnWorkKind},
};
use rusqlite::{OptionalExtension, params, types::Type};

use crate::SqliteLibraryStore;

const SELECT_DETAILS: &str = "SELECT installation_id, circle, code, release_date, kind, tags,
        description, cover, samples, updated_at
     FROM library_own_work_details";

impl OwnWorkDetailsStore for SqliteLibraryStore {
    fn read_own_work_details(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<OwnWorkDetails>, OwnWorkError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    &format!("{SELECT_DETAILS} WHERE installation_id = ?1"),
                    [&installation_id.0],
                    details_from_row,
                )
                .optional()
        })
        .map_err(OwnWorkError::persistence)
    }

    fn list_own_work_details(&self) -> Result<Vec<OwnWorkDetails>, OwnWorkError> {
        self.with_connection(|connection| {
            let mut statement =
                connection.prepare(&format!("{SELECT_DETAILS} ORDER BY installation_id"))?;
            statement
                .query_map([], details_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(OwnWorkError::persistence)
    }

    fn save_own_work_details(&self, details: &OwnWorkDetails) -> Result<(), OwnWorkError> {
        let tags = serde_json::to_string(&details.tags).map_err(OwnWorkError::persistence)?;
        let samples = serde_json::to_string(&details.samples).map_err(OwnWorkError::persistence)?;
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO library_own_work_details
                 (installation_id, circle, code, release_date, kind, tags, description, cover,
                  samples, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(installation_id) DO UPDATE SET
                     circle = excluded.circle,
                     code = excluded.code,
                     release_date = excluded.release_date,
                     kind = excluded.kind,
                     tags = excluded.tags,
                     description = excluded.description,
                     cover = excluded.cover,
                     samples = excluded.samples,
                     updated_at = excluded.updated_at",
                params![
                    details.installation_id.0,
                    details.circle,
                    details.code,
                    details.release_date,
                    details.kind.map(kind_value),
                    tags,
                    details.description,
                    details.cover,
                    samples,
                    details.updated_at,
                ],
            )?;
            Ok(())
        })
        .map_err(OwnWorkError::persistence)
    }
}

fn details_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OwnWorkDetails> {
    Ok(OwnWorkDetails {
        installation_id: InstallationId(row.get(0)?),
        circle: row.get(1)?,
        code: row.get(2)?,
        release_date: row.get(3)?,
        kind: row
            .get::<_, Option<String>>(4)?
            .map(|value| parse_kind(4, &value))
            .transpose()?,
        tags: json_list(row, 5)?,
        description: row.get(6)?,
        cover: row.get(7)?,
        samples: json_list(row, 8)?,
        updated_at: row.get(9)?,
    })
}

fn json_list(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Vec<String>> {
    let value = row.get::<_, String>(column)?;
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
    })
}

fn kind_value(kind: OwnWorkKind) -> &'static str {
    match kind {
        OwnWorkKind::Images => "images",
        OwnWorkKind::Manga => "manga",
        OwnWorkKind::Game => "game",
        OwnWorkKind::Voice => "voice",
        OwnWorkKind::Video => "video",
        OwnWorkKind::Document => "document",
    }
}

fn parse_kind(column: usize, value: &str) -> rusqlite::Result<OwnWorkKind> {
    match value {
        "images" => Ok(OwnWorkKind::Images),
        "manga" => Ok(OwnWorkKind::Manga),
        "game" => Ok(OwnWorkKind::Game),
        "voice" => Ok(OwnWorkKind::Voice),
        "video" => Ok(OwnWorkKind::Video),
        "document" => Ok(OwnWorkKind::Document),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            column,
            Type::Text,
            format!("invalid own work kind: {value}").into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn insert_installation(store: &SqliteLibraryStore, installation_id: &str) {
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO library_installation
                     (installation_id, root_path, platform, status, identity_confidence,
                      identity_reason_codes, suggested_status, discovered_at, updated_at)
                     VALUES (?1, '/synthetic/library/own', 'unknown', 'ready', NULL, '[]',
                             'ready', '2026-10-28T00:00:00Z', '2026-10-28T00:00:00Z')",
                    [installation_id],
                )
            })
            .expect("installation");
    }

    fn details(installation_id: &str) -> OwnWorkDetails {
        OwnWorkDetails {
            installation_id: InstallationId(installation_id.to_owned()),
            circle: Some("A circle".to_owned()),
            code: None,
            release_date: Some("2023-03-12".to_owned()),
            kind: Some(OwnWorkKind::Images),
            tags: vec!["CG".to_owned(), "Bonus".to_owned()],
            description: None,
            cover: Some("a".repeat(64)),
            samples: vec!["b".repeat(64), "c".repeat(64)],
            updated_at: "2026-10-28T01:00:00Z".to_owned(),
        }
    }

    #[test]
    fn own_details_survive_reopen_update_and_go_with_their_installation() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("library.sqlite");
        {
            let store = SqliteLibraryStore::open(&path).expect("library store");
            insert_installation(&store, "installation-1");
            insert_installation(&store, "installation-2");
            store
                .save_own_work_details(&details("installation-1"))
                .expect("saved");
            store
                .save_own_work_details(&details("installation-2"))
                .expect("saved");
        }

        let store = SqliteLibraryStore::open(&path).expect("reopened store");
        let id = InstallationId("installation-1".to_owned());
        assert_eq!(
            store.read_own_work_details(&id).expect("read"),
            Some(details("installation-1"))
        );

        let changed = OwnWorkDetails {
            kind: None,
            tags: Vec::new(),
            samples: Vec::new(),
            circle: None,
            ..details("installation-1")
        };
        store.save_own_work_details(&changed).expect("updated");
        assert_eq!(
            store.read_own_work_details(&id).expect("read"),
            Some(changed)
        );

        store
            .with_connection(|connection| {
                connection.execute(
                    "DELETE FROM library_installation WHERE installation_id = 'installation-2'",
                    [],
                )
            })
            .expect("installation removed");
        assert_eq!(
            store
                .list_own_work_details()
                .expect("listed")
                .into_iter()
                .map(|details| details.installation_id.0)
                .collect::<Vec<_>>(),
            vec!["installation-1".to_owned()]
        );
        assert!(
            store
                .save_own_work_details(&details("installation-missing"))
                .is_err()
        );
    }
}
