use std::io;

use dla_application::package_preparation::{PackagePreparationError, PackagePreparationStore};
use dla_domain::{
    installation::{InstallationId, RelativePath},
    package::{
        ArchiveRetentionPolicy, PackageLaunchCandidate, PackageSourceSet,
        PreparedPackageInstallation,
    },
};
use rusqlite::{OptionalExtension, Row, params, params_from_iter, types::Type};

use crate::SqliteLibraryStore;

impl PackagePreparationStore for SqliteLibraryStore {
    fn read_prepared_package(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<PreparedPackageInstallation>, PackagePreparationError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT destination_root, content_root, preferred_action, source_set,
                            archive_retention, sources_deleted, source_cleanup_error,
                            installed_file_count, installed_bytes, prepared_at
                    FROM library_prepared_package
                     WHERE installation_id = ?1",
                    params![installation_id.0],
                    |row| read_prepared_package_row(row, installation_id.clone(), 0),
                )
                .optional()
        })
        .map_err(PackagePreparationError::persistence)
    }

    fn read_prepared_packages(
        &self,
        installation_ids: &[InstallationId],
    ) -> Result<Vec<PreparedPackageInstallation>, PackagePreparationError> {
        if installation_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_connection(|connection| {
            let placeholders = vec!["?"; installation_ids.len()].join(",");
            let sql = format!(
                "SELECT installation_id, destination_root, content_root, preferred_action,
                        source_set, archive_retention, sources_deleted, source_cleanup_error,
                        installed_file_count, installed_bytes, prepared_at
                 FROM library_prepared_package
                 WHERE installation_id IN ({placeholders})"
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(
                params_from_iter(installation_ids.iter().map(|id| &id.0)),
                |row| {
                    let installation_id = InstallationId(row.get(0)?);
                    read_prepared_package_row(row, installation_id, 1)
                },
            )?;
            rows.collect()
        })
        .map_err(PackagePreparationError::persistence)
    }

    fn save_prepared_package(
        &self,
        prepared: &PreparedPackageInstallation,
    ) -> Result<(), PackagePreparationError> {
        let preferred_action = prepared
            .preferred_action
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(PackagePreparationError::persistence)?;
        let source_set = serde_json::to_string(&prepared.source_set)
            .map_err(PackagePreparationError::persistence)?;
        let installed_file_count = to_sqlite_u64(prepared.installed_file_count)?;
        let installed_bytes = to_sqlite_u64(prepared.installed_bytes)?;
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO library_prepared_package
                 (installation_id, destination_root, content_root, preferred_action, source_set,
                  archive_retention, sources_deleted, source_cleanup_error, installed_file_count,
                  installed_bytes, prepared_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(installation_id) DO UPDATE SET
                    destination_root = excluded.destination_root,
                    content_root = excluded.content_root,
                    preferred_action = excluded.preferred_action,
                    source_set = excluded.source_set,
                    archive_retention = excluded.archive_retention,
                    sources_deleted = excluded.sources_deleted,
                    source_cleanup_error = excluded.source_cleanup_error,
                    installed_file_count = excluded.installed_file_count,
                    installed_bytes = excluded.installed_bytes,
                    prepared_at = excluded.prepared_at",
                params![
                    prepared.installation_id.0,
                    prepared.destination_root,
                    prepared.content_root.as_ref().map(RelativePath::as_str),
                    preferred_action,
                    source_set,
                    retention(prepared.archive_retention),
                    prepared.sources_deleted,
                    prepared.source_cleanup_error,
                    installed_file_count,
                    installed_bytes,
                    prepared.prepared_at,
                ],
            )?;
            Ok(())
        })
        .map_err(PackagePreparationError::persistence)
    }
}

fn read_prepared_package_row(
    row: &Row<'_>,
    installation_id: InstallationId,
    offset: usize,
) -> rusqlite::Result<PreparedPackageInstallation> {
    let content_root = row
        .get::<_, Option<String>>(offset + 1)?
        .map(RelativePath::parse)
        .transpose()
        .map_err(|error| conversion_error(offset + 1, error))?;
    let preferred_action = row
        .get::<_, Option<String>>(offset + 2)?
        .map(|value| parse_json::<PackageLaunchCandidate>(offset + 2, &value))
        .transpose()?;
    let source_set =
        parse_json::<PackageSourceSet>(offset + 3, &row.get::<_, String>(offset + 3)?)?;
    let retention = row.get::<_, String>(offset + 4)?;
    let file_count = row.get::<_, i64>(offset + 7)?;
    let installed_bytes = row.get::<_, i64>(offset + 8)?;
    Ok(PreparedPackageInstallation {
        installation_id,
        destination_root: row.get(offset)?,
        content_root,
        preferred_action,
        source_set,
        archive_retention: parse_retention(offset + 4, &retention)?,
        sources_deleted: row.get(offset + 5)?,
        source_cleanup_error: row.get(offset + 6)?,
        installed_file_count: sqlite_u64(offset + 7, file_count)?,
        installed_bytes: sqlite_u64(offset + 8, installed_bytes)?,
        prepared_at: row.get(offset + 9)?,
    })
}

fn retention(value: ArchiveRetentionPolicy) -> &'static str {
    match value {
        ArchiveRetentionPolicy::Keep => "keep",
        ArchiveRetentionPolicy::DeleteAfterVerifiedInstall => "delete_after_verified_install",
    }
}

fn parse_retention(column: usize, value: &str) -> rusqlite::Result<ArchiveRetentionPolicy> {
    match value {
        "keep" => Ok(ArchiveRetentionPolicy::Keep),
        "delete_after_verified_install" => Ok(ArchiveRetentionPolicy::DeleteAfterVerifiedInstall),
        _ => Err(conversion_error(
            column,
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown archive retention policy",
            ),
        )),
    }
}

fn parse_json<T: serde::de::DeserializeOwned>(column: usize, value: &str) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|error| conversion_error(column, error))
}

fn to_sqlite_u64(value: u64) -> Result<i64, PackagePreparationError> {
    i64::try_from(value).map_err(PackagePreparationError::persistence)
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
    use dla_domain::{
        installation::{
            InferenceConfidence, Installation, InstallationDetection, InstallationId,
            InstallationOverrides, InstallationPlatform, InstallationStatus, LaunchActionKind,
            RelativePath,
        },
        package::{
            ArchiveRetentionPolicy, PackageLaunchCandidate, PackageSourceSet, PackageSourceSetKind,
            PreparedPackageInstallation, SourceArtifact, SourceArtifactKind,
        },
        scanner::ScanEntryId,
    };
    use tempfile::tempdir;

    use dla_application::installation::InstallationStore;

    use super::*;

    #[test]
    fn prepared_package_round_trips_independently_from_catalog_state() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let installation_id = InstallationId("installation-package".to_owned());
        store
            .create(&Installation {
                id: installation_id.clone(),
                scan_root_id: None,
                root_path: "/synthetic/source".to_owned(),
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
                discovered_at: "2026-08-08T00:00:00Z".to_owned(),
                updated_at: "2026-08-08T00:00:00Z".to_owned(),
            })
            .expect("installation");
        let source = SourceArtifact {
            scan_entry_id: ScanEntryId("entry".to_owned()),
            kind: SourceArtifactKind::Archive,
            relative_path: RelativePath::parse("RJ000001.part01.exe").expect("source path"),
            size_bytes: Some(12),
            sha256: Some("a".repeat(64)),
        };
        let prepared = PreparedPackageInstallation {
            installation_id: installation_id.clone(),
            destination_root: "/synthetic/library/RJ000001".to_owned(),
            content_root: Some(RelativePath::parse("Work").expect("content root")),
            preferred_action: Some(PackageLaunchCandidate {
                action: LaunchActionKind::LaunchExecutable,
                relative_path: RelativePath::parse("Work/Game.exe").expect("action path"),
                supported_platforms: vec![InstallationPlatform::Windows],
                confidence: InferenceConfidence::High,
                reason_codes: vec!["fixture".to_owned()],
                expected_sha256: Some("b".repeat(64)),
            }),
            source_set: PackageSourceSet {
                kind: PackageSourceSetKind::MultipartRarSfx,
                volumes: vec![source],
            },
            archive_retention: ArchiveRetentionPolicy::DeleteAfterVerifiedInstall,
            sources_deleted: true,
            source_cleanup_error: None,
            installed_file_count: 42,
            installed_bytes: 2048,
            prepared_at: "2026-08-08T00:01:00Z".to_owned(),
        };
        store
            .save_prepared_package(&prepared)
            .expect("save prepared package");

        let restored = store
            .read_prepared_package(&installation_id)
            .expect("read prepared package")
            .expect("prepared package");
        assert_eq!(restored, prepared);
    }
}
