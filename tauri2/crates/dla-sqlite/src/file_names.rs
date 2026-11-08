//! Paths inside a work that the launcher stored, given their new names when
//! an older install's wrongly read file names are fixed. The file list and
//! start-file choices found in the work are renamed too rather than found
//! again, so the work stays set up as it was.

use dla_application::maintenance::{InstalledPathStore, LibraryMaintenanceError};
use dla_domain::{
    installation::{InstallationId, RelativePath},
    package::{PackageInspection, PackageLaunchCandidate},
};
use rusqlite::{OptionalExtension, Transaction, params, types::Type};

use crate::SqliteLibraryStore;

type Rename<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Columns that hold one path inside a work, and which rows are the work's.
const PATH_COLUMNS: [(&str, &str, &str); 10] = [
    (
        "library_launch_candidate",
        "target_path",
        "installation_id = ?1",
    ),
    (
        "library_installation_override",
        "preferred_target_path",
        "installation_id = ?1",
    ),
    (
        "library_content_item_override",
        "relative_path",
        "installation_id = ?1",
    ),
    (
        "library_media_session_item",
        "relative_path",
        "session_id IN (SELECT session_id FROM library_media_session WHERE installation_id = ?1)",
    ),
    (
        "library_media_resume",
        "relative_path",
        "installation_id = ?1",
    ),
    (
        "library_audio_track",
        "relative_path",
        "installation_id = ?1",
    ),
    (
        "library_media_queue_state",
        "current_relative_path",
        "current_installation_id = ?1",
    ),
    (
        "library_launch_activity",
        "target_path",
        "installation_id = ?1",
    ),
    (
        "library_game_translation",
        "game_folder",
        "installation_id = ?1",
    ),
    (
        "library_game_translation_change",
        "path",
        "installation_id = ?1",
    ),
];

impl InstalledPathStore for SqliteLibraryStore {
    fn rename_paths(
        &self,
        installation_id: &InstallationId,
        rename: Rename<'_>,
    ) -> Result<(), LibraryMaintenanceError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let id = installation_id.0.as_str();
            for (table, column, belongs) in PATH_COLUMNS {
                rename_column(&transaction, table, column, belongs, id, rename)?;
            }
            rename_content_items(&transaction, id, rename)?;
            rename_package(&transaction, id, rename)?;
            rename_own_work(&transaction, id, rename)?;
            transaction.commit()
        })
        .map_err(LibraryMaintenanceError::persistence)
    }
}

fn rename_column(
    transaction: &Transaction<'_>,
    table: &str,
    column: &str,
    belongs: &str,
    id: &str,
    rename: Rename<'_>,
) -> rusqlite::Result<()> {
    let mut select = transaction.prepare(&format!(
        "SELECT DISTINCT {column} FROM {table} WHERE {belongs} AND {column} IS NOT NULL"
    ))?;
    let paths = select
        .query_map(params![id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // A row whose new path another row has already keeps its old one.
    let mut update = transaction.prepare(&format!(
        "UPDATE OR IGNORE {table} SET {column} = ?2 WHERE {belongs} AND {column} = ?3"
    ))?;
    for path in paths {
        if let Some(renamed) = rename(&path) {
            update.execute(params![id, renamed, path])?;
        }
    }
    Ok(())
}

/// The work's file list. Each file's key is its path, lower-cased on Windows
/// as a scan makes it.
fn rename_content_items(
    transaction: &Transaction<'_>,
    id: &str,
    rename: Rename<'_>,
) -> rusqlite::Result<()> {
    let windows = transaction.query_row(
        "SELECT platform = 'windows' FROM library_installation WHERE installation_id = ?1",
        params![id],
        |row| row.get::<_, bool>(0),
    )?;
    let mut select = transaction
        .prepare("SELECT relative_path FROM library_content_item WHERE installation_id = ?1")?;
    let paths = select
        .query_map(params![id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut update = transaction.prepare(
        "UPDATE OR IGNORE library_content_item SET relative_path = ?2, path_key = ?3
         WHERE installation_id = ?1 AND relative_path = ?4",
    )?;
    for path in paths {
        if let Some(renamed) = rename(&path) {
            let key = if windows {
                renamed.to_lowercase()
            } else {
                renamed.clone()
            };
            update.execute(params![id, renamed, key, path])?;
        }
    }
    Ok(())
}

/// The install's folder and start file, and the archive scan they came from.
fn rename_package(
    transaction: &Transaction<'_>,
    id: &str,
    rename: Rename<'_>,
) -> rusqlite::Result<()> {
    let prepared = transaction
        .query_row(
            "SELECT content_root, preferred_action
             FROM library_prepared_package WHERE installation_id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;
    if let Some((content_root, action)) = prepared {
        let content_root = content_root.map(|root| rename(&root).unwrap_or(root));
        let action = action
            .map(|json| {
                let mut action = from_json::<PackageLaunchCandidate>(&json)?;
                rename_path(&mut action.relative_path, rename);
                to_json(&action)
            })
            .transpose()?;
        transaction.execute(
            "UPDATE library_prepared_package SET content_root = ?2, preferred_action = ?3
             WHERE installation_id = ?1",
            params![id, content_root, action],
        )?;
    }
    let inspection = transaction
        .query_row(
            "SELECT inspection FROM library_package_inspection WHERE installation_id = ?1",
            params![id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(json) = inspection {
        let mut inspection = from_json::<PackageInspection>(&json)?;
        let plan = &mut inspection.install_plan;
        let classification = &mut inspection.classification;
        for root in [
            &mut inspection.common_root,
            &mut classification.content_root,
            &mut plan.content_root,
        ]
        .into_iter()
        .flatten()
        {
            rename_path(root, rename);
        }
        for candidate in classification
            .launch_candidates
            .iter_mut()
            .chain(plan.preferred_action.as_mut())
        {
            rename_path(&mut candidate.relative_path, rename);
        }
        transaction.execute(
            "UPDATE library_package_inspection SET inspection = ?2 WHERE installation_id = ?1",
            params![id, to_json(&inspection)?],
        )?;
    }
    Ok(())
}

/// The cover and samples chosen from the work's own pictures.
fn rename_own_work(
    transaction: &Transaction<'_>,
    id: &str,
    rename: Rename<'_>,
) -> rusqlite::Result<()> {
    let details = transaction
        .query_row(
            "SELECT cover, samples FROM library_own_work_details WHERE installation_id = ?1",
            params![id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((cover, samples)) = details else {
        return Ok(());
    };
    let cover = cover.map(|cover| rename(&cover).unwrap_or(cover));
    let samples = from_json::<Vec<String>>(&samples)?
        .into_iter()
        .map(|sample| rename(&sample).unwrap_or(sample))
        .collect::<Vec<_>>();
    transaction.execute(
        "UPDATE library_own_work_details SET cover = ?2, samples = ?3 WHERE installation_id = ?1",
        params![id, cover, to_json(&samples)?],
    )?;
    Ok(())
}

fn rename_path(path: &mut RelativePath, rename: Rename<'_>) {
    if let Some(renamed) =
        rename(path.as_str()).and_then(|renamed| RelativePath::parse(renamed).ok())
    {
        *path = renamed;
    }
}

fn from_json<T: serde::de::DeserializeOwned>(json: &str) -> rusqlite::Result<T> {
    serde_json::from_str(json)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error)))
}

fn to_json<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))
}

#[cfg(test)]
mod tests {
    use dla_application::{
        installation::InstallationStore, package_preparation::PackagePreparationStore,
    };
    use dla_domain::{
        installation::{
            ContentItem, InferenceConfidence, Installation, InstallationDetection,
            InstallationOverrides, InstallationPlatform, InstallationStatus, LaunchActionKind,
            LaunchCandidate, LaunchCandidateId, LaunchTarget, MediaType,
        },
        package::{
            ArchiveRetentionPolicy, PackageSourceSet, PackageSourceSetKind,
            PreparedPackageInstallation, SourceArtifact, SourceArtifactKind,
        },
        scanner::ScanEntryId,
    };
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn renames_the_paths_stored_inside_one_work() {
        let directory = tempdir().expect("temporary directory");
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
            .expect("library store");
        let id = InstallationId("installation-1".to_owned());
        let other = InstallationId("installation-2".to_owned());
        for installation_id in [&id, &other] {
            store
                .create(&Installation {
                    id: installation_id.clone(),
                    scan_root_id: None,
                    root_path: format!("/library/{}", installation_id.0),
                    platform: InstallationPlatform::Windows,
                    status: InstallationStatus::NeedsReview,
                    detection: InstallationDetection {
                        source_scan_session_id: None,
                        catalog_identity: None,
                        suggested_status: InstallationStatus::NeedsReview,
                        content_items: vec![ContentItem {
                            relative_path: RelativePath::parse("Old/Game.exe").expect("path"),
                            path_key: "old/game.exe".to_owned(),
                            media_type: MediaType::Executable,
                            size_bytes: None,
                            modified_at: None,
                            confidence: InferenceConfidence::High,
                            reason_codes: vec!["executable_extension".to_owned()],
                        }],
                        launch_candidates: vec![LaunchCandidate {
                            id: LaunchCandidateId("launch-game".to_owned()),
                            action: LaunchActionKind::LaunchExecutable,
                            target: LaunchTarget::RelativePath(
                                RelativePath::parse("Old/Game.exe").expect("path"),
                            ),
                            supported_platforms: vec![InstallationPlatform::Windows],
                            confidence: InferenceConfidence::High,
                            reason_codes: vec!["preferred_executable_name".to_owned()],
                        }],
                        package_inspection: None,
                    },
                    overrides: InstallationOverrides::default(),
                    discovered_at: "2026-10-30T00:00:00Z".to_owned(),
                    updated_at: "2026-10-30T00:00:00Z".to_owned(),
                })
                .expect("installation");
        }
        let path = |value: &str| RelativePath::parse(value).expect("path");
        store
            .save_prepared_package(&PreparedPackageInstallation {
                installation_id: id.clone(),
                destination_root: "/library/RJ000001".to_owned(),
                content_root: Some(path("Old")),
                preferred_action: Some(PackageLaunchCandidate {
                    action: LaunchActionKind::LaunchExecutable,
                    relative_path: path("Old/Game.exe"),
                    supported_platforms: vec![InstallationPlatform::Windows],
                    confidence: InferenceConfidence::High,
                    reason_codes: vec![],
                    expected_sha256: None,
                }),
                source_set: PackageSourceSet {
                    kind: PackageSourceSetKind::SingleArchive,
                    volumes: vec![SourceArtifact {
                        scan_entry_id: ScanEntryId("entry".to_owned()),
                        kind: SourceArtifactKind::Archive,
                        relative_path: path("RJ000001.zip"),
                        size_bytes: None,
                        sha256: None,
                    }],
                },
                archive_retention: ArchiveRetentionPolicy::Keep,
                sources_deleted: false,
                source_cleanup_error: None,
                installed_file_count: 1,
                installed_bytes: 1,
                prepared_at: "2026-10-30T00:00:00Z".to_owned(),
            })
            .expect("prepared package");
        store
            .with_connection(|connection| {
                for installation_id in [&id, &other] {
                    connection.execute(
                        "UPDATE library_installation_override
                         SET preferred_action_kind = 'launch_executable',
                             preferred_target_kind = 'relative_path',
                             preferred_target_path = 'Old/Game.exe'
                         WHERE installation_id = ?1",
                        params![installation_id.0],
                    )?;
                    connection.execute(
                        "INSERT INTO library_media_resume
                         (installation_id, action_kind, relative_path, position_ms, completed,
                          updated_at)
                         VALUES (?1, 'play_audio', 'Old/voice.ogg', 1000, 0, 'now')",
                        params![installation_id.0],
                    )?;
                }
                Ok(())
            })
            .expect("stored paths");

        store
            .rename_paths(&id, &|path| {
                path.strip_prefix("Old").map(|rest| format!("作品{rest}"))
            })
            .expect("rename");

        let prepared = store
            .read_prepared_package(&id)
            .expect("read")
            .expect("prepared package");
        assert_eq!(prepared.content_root, Some(path("作品")));
        assert_eq!(
            prepared.preferred_action.map(|action| action.relative_path),
            Some(path("作品/Game.exe"))
        );
        let stored = |installation_id: &InstallationId| {
            store
                .with_connection(|connection| {
                    connection.query_row(
                        "SELECT o.preferred_target_path, r.relative_path
                         FROM library_installation_override o
                         JOIN library_media_resume r USING (installation_id)
                         WHERE installation_id = ?1",
                        params![installation_id.0],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                })
                .expect("stored paths")
        };
        assert_eq!(
            stored(&id),
            ("作品/Game.exe".to_owned(), "作品/voice.ogg".to_owned())
        );
        // Another work's paths stay.
        assert_eq!(
            stored(&other),
            ("Old/Game.exe".to_owned(), "Old/voice.ogg".to_owned())
        );
        // The file list keeps its Windows key, and the start choices follow.
        let found = |installation_id: &InstallationId| {
            let installation = store
                .read(installation_id)
                .expect("read")
                .expect("installation");
            let item = &installation.detection.content_items[0];
            (
                item.relative_path.as_str().to_owned(),
                item.path_key.clone(),
                installation.detection.launch_candidates[0].target.clone(),
            )
        };
        assert_eq!(
            found(&id),
            (
                "作品/Game.exe".to_owned(),
                "作品/game.exe".to_owned(),
                LaunchTarget::RelativePath(path("作品/Game.exe"))
            )
        );
        assert_eq!(
            found(&other),
            (
                "Old/Game.exe".to_owned(),
                "old/game.exe".to_owned(),
                LaunchTarget::RelativePath(path("Old/Game.exe"))
            )
        );
    }
}
