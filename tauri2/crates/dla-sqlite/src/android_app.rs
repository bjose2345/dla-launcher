use dla_application::android_app::{AndroidAppAssociationStore, AndroidAppError};
use dla_domain::android_app::{AndroidAppAssociation, AndroidAppAssociationId};
use rusqlite::{OptionalExtension, Row, params, types::Type};

use crate::SqliteLibraryStore;

impl AndroidAppAssociationStore for SqliteLibraryStore {
    fn read(
        &self,
        association_id: &AndroidAppAssociationId,
    ) -> Result<Option<AndroidAppAssociation>, AndroidAppError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    &format!("{} WHERE association_id = ?1", association_select()),
                    [&association_id.0],
                    association_from_row,
                )
                .optional()
        })
        .map_err(AndroidAppError::persistence)
    }

    fn read_by_work_code(
        &self,
        work_code: &str,
    ) -> Result<Option<AndroidAppAssociation>, AndroidAppError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    &format!(
                        "{} WHERE work_code = ?1 COLLATE NOCASE",
                        association_select()
                    ),
                    [work_code],
                    association_from_row,
                )
                .optional()
        })
        .map_err(AndroidAppError::persistence)
    }

    fn read_by_package_name(
        &self,
        package_name: &str,
    ) -> Result<Option<AndroidAppAssociation>, AndroidAppError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    &format!("{} WHERE package_name = ?1", association_select()),
                    [package_name],
                    association_from_row,
                )
                .optional()
        })
        .map_err(AndroidAppError::persistence)
    }

    fn list(&self) -> Result<Vec<AndroidAppAssociation>, AndroidAppError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(&format!(
                "{} ORDER BY updated_at DESC, association_id",
                association_select()
            ))?;
            statement
                .query_map([], association_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(AndroidAppError::persistence)
    }

    fn save(&self, association: &AndroidAppAssociation) -> Result<(), AndroidAppError> {
        association.validate()?;
        let certificates = serde_json::to_string(&association.expected_signing_certificate_sha256)
            .map_err(AndroidAppError::persistence)?;
        let launch_count =
            i64::try_from(association.launch_count).map_err(AndroidAppError::persistence)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO library_work
                 (work_code, marker, note, progress_kind, progress_value, created_at, updated_at)
                 VALUES (?1, '', '', '', NULL, ?2, ?2)
                 ON CONFLICT(work_code) DO NOTHING",
                params![association.work_code, association.associated_at],
            )?;
            transaction.execute(
                "INSERT INTO library_android_app_association
                 (association_id, work_code, package_name, application_label,
                  expected_signing_certificate_sha256, associated_version_name,
                  associated_version_code, associated_at, updated_at, last_launched_at,
                  launch_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(association_id) DO UPDATE SET
                    work_code = excluded.work_code,
                    package_name = excluded.package_name,
                    application_label = excluded.application_label,
                    expected_signing_certificate_sha256 = excluded.expected_signing_certificate_sha256,
                    associated_version_name = excluded.associated_version_name,
                    associated_version_code = excluded.associated_version_code,
                    updated_at = excluded.updated_at,
                    last_launched_at = excluded.last_launched_at,
                    launch_count = excluded.launch_count",
                params![
                    association.id.0,
                    association.work_code,
                    association.package_name,
                    association.application_label,
                    certificates,
                    association.associated_version_name,
                    association.associated_version_code,
                    association.associated_at,
                    association.updated_at,
                    association.last_launched_at,
                    launch_count,
                ],
            )?;
            transaction.commit()
        })
        .map_err(AndroidAppError::persistence)
    }

    fn remove(&self, association_id: &AndroidAppAssociationId) -> Result<bool, AndroidAppError> {
        self.with_connection(|connection| {
            Ok(connection.execute(
                "DELETE FROM library_android_app_association WHERE association_id = ?1",
                [&association_id.0],
            )? > 0)
        })
        .map_err(AndroidAppError::persistence)
    }

    fn record_launch(
        &self,
        association_id: &AndroidAppAssociationId,
        launched_at: &str,
    ) -> Result<AndroidAppAssociation, AndroidAppError> {
        self.with_connection(|connection| {
            let changed = connection.execute(
                "UPDATE library_android_app_association
                 SET last_launched_at = ?2, updated_at = ?2, launch_count = launch_count + 1
                 WHERE association_id = ?1",
                params![association_id.0, launched_at],
            )?;
            if changed == 0 {
                return Ok(None);
            }
            connection
                .query_row(
                    &format!("{} WHERE association_id = ?1", association_select()),
                    [&association_id.0],
                    association_from_row,
                )
                .optional()
        })
        .map_err(AndroidAppError::persistence)?
        .ok_or_else(|| AndroidAppError::NotFound(association_id.0.clone()))
    }
}

fn association_select() -> &'static str {
    "SELECT association_id, work_code, package_name, application_label,
            expected_signing_certificate_sha256, associated_version_name,
            associated_version_code, associated_at, updated_at, last_launched_at,
            launch_count
     FROM library_android_app_association"
}

fn association_from_row(row: &Row<'_>) -> rusqlite::Result<AndroidAppAssociation> {
    let certificates = serde_json::from_str(&row.get::<_, String>(4)?)
        .map_err(|error| conversion_error(4, error))?;
    let launch_count = row.get::<_, i64>(10)?;
    let association = AndroidAppAssociation {
        id: AndroidAppAssociationId(row.get(0)?),
        work_code: row.get(1)?,
        package_name: row.get(2)?,
        application_label: row.get(3)?,
        expected_signing_certificate_sha256: certificates,
        associated_version_name: row.get(5)?,
        associated_version_code: row.get(6)?,
        associated_at: row.get(7)?,
        updated_at: row.get(8)?,
        last_launched_at: row.get(9)?,
        launch_count: u64::try_from(launch_count).map_err(|error| conversion_error(10, error))?,
    };
    association
        .validate()
        .map_err(|error| conversion_error(0, error))?;
    Ok(association)
}

fn conversion_error(
    column: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn association_survives_reopen_without_catalog_state() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("library.sqlite");
        let association = fixture();
        {
            let store = SqliteLibraryStore::open(&path).expect("library store");
            store.save(&association).expect("save association");
            let launched = store
                .record_launch(&association.id, "2026-08-22T12:01:00Z")
                .expect("record launch");
            assert_eq!(launched.launch_count, 1);
        }

        let store = SqliteLibraryStore::open(&path).expect("reopen library store");
        let stored = store
            .read(&association.id)
            .expect("read association")
            .expect("stored association");
        assert_eq!(stored.package_name, "org.dlaproject.fixture");
        assert_eq!(stored.launch_count, 1);
        assert!(store.remove(&association.id).expect("remove"));
        assert!(store.read(&association.id).expect("read removed").is_none());
    }

    fn fixture() -> AndroidAppAssociation {
        AndroidAppAssociation {
            id: AndroidAppAssociationId("android-app-1234567890".to_owned()),
            work_code: "RJ01326398".to_owned(),
            package_name: "org.dlaproject.fixture".to_owned(),
            application_label: "Fixture".to_owned(),
            expected_signing_certificate_sha256: vec!["a".repeat(64)],
            associated_version_name: Some("1.0".to_owned()),
            associated_version_code: "1".to_owned(),
            associated_at: "2026-08-22T12:00:00Z".to_owned(),
            updated_at: "2026-08-22T12:00:00Z".to_owned(),
            last_launched_at: None,
            launch_count: 0,
        }
    }
}
