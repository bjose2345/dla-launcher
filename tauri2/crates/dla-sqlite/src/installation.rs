use std::io;

use dla_application::installation::{InstallationLibraryError, InstallationStore};
use dla_domain::{
    installation::{
        CatalogIdentity, ContentItem, ContentItemOverride, InferenceConfidence, Installation,
        InstallationDetection, InstallationId, InstallationOverrides, InstallationPlatform,
        InstallationStatus, LaunchActionKind, LaunchCandidate, LaunchCandidateId, LaunchTarget,
        ManualCatalogIdentity, ManualLaunchSelection, MediaType, RelativePath,
    },
    package::PackageInspection,
    scanner::{ScanMatchConfidence, ScanRootId, ScanSessionId},
};
use rusqlite::{OptionalExtension, Row, Transaction, params, types::Type};

use crate::SqliteLibraryStore;

impl InstallationStore for SqliteLibraryStore {
    fn create(&self, installation: &Installation) -> Result<(), InstallationLibraryError> {
        installation.validate()?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            insert_installation(&transaction, installation)?;
            insert_detection(&transaction, &installation.id, &installation.detection)?;
            insert_overrides(
                &transaction,
                &installation.id,
                &installation.overrides,
                &installation.updated_at,
            )?;
            transaction.commit()
        })
        .map_err(InstallationLibraryError::persistence)
    }

    fn create_or_refresh(
        &self,
        installation: &Installation,
    ) -> Result<Installation, InstallationLibraryError> {
        installation.validate()?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let stored = if let Some(mut existing) =
                read_full_installation(&transaction, &installation.id)?
            {
                if existing.scan_root_id != installation.scan_root_id
                    || existing.root_path != installation.root_path
                    || existing.platform != installation.platform
                {
                    return Err(invalid_value(0, "installation source", &installation.id.0));
                }
                let updated_at = existing
                    .updated_at
                    .clone()
                    .max(installation.updated_at.clone());
                existing
                    .replace_detection(installation.detection.clone(), updated_at)
                    .map_err(|error| conversion_error(0, error))?;
                replace_detection_in_transaction(
                    &transaction,
                    &existing.id,
                    &existing.detection,
                    existing.status,
                    &existing.updated_at,
                )?;
                existing
            } else {
                insert_installation(&transaction, installation)?;
                insert_detection(&transaction, &installation.id, &installation.detection)?;
                insert_overrides(
                    &transaction,
                    &installation.id,
                    &installation.overrides,
                    &installation.updated_at,
                )?;
                installation.clone()
            };
            transaction.commit()?;
            Ok(stored)
        })
        .map_err(InstallationLibraryError::persistence)
    }

    fn read(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<Installation>, InstallationLibraryError> {
        self.with_connection(|connection| read_full_installation(connection, installation_id))
            .map_err(InstallationLibraryError::persistence)
    }

    fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
        self.with_connection(|connection| {
            let installation_ids = {
                let mut statement = connection.prepare(
                    "SELECT installation_id
                     FROM library_installation
                     ORDER BY updated_at DESC, installation_id",
                )?;
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            installation_ids
                .into_iter()
                .map(|id| {
                    read_full_installation(connection, &InstallationId(id))?
                        .ok_or(rusqlite::Error::QueryReturnedNoRows)
                })
                .collect()
        })
        .map_err(InstallationLibraryError::persistence)
    }

    fn find_by_work_code(
        &self,
        work_code: &str,
    ) -> Result<Vec<Installation>, InstallationLibraryError> {
        self.with_connection(|connection| {
            let installation_ids = {
                let mut statement = connection.prepare(
                    "SELECT installation.installation_id
                     FROM library_installation AS installation
                     JOIN library_installation_override AS override
                       ON override.installation_id = installation.installation_id
                     WHERE override.identity_override_kind = 'catalog_work'
                       AND override.identity_work_code = ?1
                     UNION ALL
                     SELECT installation.installation_id
                     FROM library_installation AS installation
                     JOIN library_installation_override AS override
                       ON override.installation_id = installation.installation_id
                     WHERE override.identity_override_kind IS NULL
                       AND installation.work_code = ?1",
                )?;
                statement
                    .query_map(params![work_code], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            let mut installations = installation_ids
                .into_iter()
                .map(|id| {
                    read_full_installation(connection, &InstallationId(id))?
                        .ok_or(rusqlite::Error::QueryReturnedNoRows)
                })
                .collect::<rusqlite::Result<Vec<_>>>()?;
            installations.sort_by(|left, right| {
                right
                    .updated_at
                    .cmp(&left.updated_at)
                    .then_with(|| left.id.0.cmp(&right.id.0))
            });
            Ok(installations)
        })
        .map_err(InstallationLibraryError::persistence)
    }

    fn replace_detection(
        &self,
        installation_id: &InstallationId,
        detection: &InstallationDetection,
        status: InstallationStatus,
        updated_at: &str,
    ) -> Result<(), InstallationLibraryError> {
        detection.validate()?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            replace_detection_in_transaction(
                &transaction,
                installation_id,
                detection,
                status,
                updated_at,
            )?;
            transaction.commit()
        })
        .map_err(InstallationLibraryError::persistence)
    }

    fn replace_overrides(
        &self,
        installation_id: &InstallationId,
        overrides: &InstallationOverrides,
        status: InstallationStatus,
        updated_at: &str,
    ) -> Result<(), InstallationLibraryError> {
        overrides.validate()?;
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let changed = transaction.execute(
                "UPDATE library_installation SET status = ?2, updated_at = ?3
                 WHERE installation_id = ?1",
                params![installation_id.0, installation_status(status), updated_at],
            )?;
            require_changed(changed)?;
            transaction.execute(
                "DELETE FROM library_installation_override WHERE installation_id = ?1",
                params![installation_id.0],
            )?;
            transaction.execute(
                "DELETE FROM library_content_item_override WHERE installation_id = ?1",
                params![installation_id.0],
            )?;
            insert_overrides(&transaction, installation_id, overrides, updated_at)?;
            transaction.commit()
        })
        .map_err(InstallationLibraryError::persistence)
    }
}

fn read_full_installation(
    connection: &rusqlite::Connection,
    installation_id: &InstallationId,
) -> rusqlite::Result<Option<Installation>> {
    let Some(mut installation) = connection
        .query_row(
            "SELECT installation_id, scan_root_id, source_scan_session_id,
                    root_path, platform, status, work_code, identity_confidence,
                    identity_reason_codes, suggested_status, discovered_at, updated_at
             FROM library_installation
             WHERE installation_id = ?1",
            params![installation_id.0],
            read_installation,
        )
        .optional()?
    else {
        return Ok(None);
    };
    installation.detection.content_items = read_content_items(connection, installation_id)?;
    installation.detection.launch_candidates = read_launch_candidates(connection, installation_id)?;
    installation.detection.package_inspection =
        read_package_inspection(connection, installation_id)?;
    installation.overrides = read_overrides(connection, installation_id)?;
    installation
        .validate()
        .map_err(|error| conversion_error(0, error))?;
    Ok(Some(installation))
}

fn replace_detection_in_transaction(
    transaction: &Transaction<'_>,
    installation_id: &InstallationId,
    detection: &InstallationDetection,
    status: InstallationStatus,
    updated_at: &str,
) -> rusqlite::Result<()> {
    let (work_code, identity_confidence, identity_reasons) =
        identity_parts(detection.catalog_identity.as_ref())?;
    let changed = transaction.execute(
        "UPDATE library_installation SET
            source_scan_session_id = ?2,
            status = ?3,
            work_code = ?4,
            identity_confidence = ?5,
            identity_reason_codes = ?6,
            suggested_status = ?7,
            updated_at = ?8
         WHERE installation_id = ?1",
        params![
            installation_id.0,
            detection
                .source_scan_session_id
                .as_ref()
                .map(|id| id.0.as_str()),
            installation_status(status),
            work_code,
            identity_confidence,
            identity_reasons,
            installation_status(detection.suggested_status),
            updated_at,
        ],
    )?;
    require_changed(changed)?;
    transaction.execute(
        "DELETE FROM library_content_item WHERE installation_id = ?1",
        params![installation_id.0],
    )?;
    transaction.execute(
        "DELETE FROM library_launch_candidate WHERE installation_id = ?1",
        params![installation_id.0],
    )?;
    transaction.execute(
        "DELETE FROM library_package_inspection WHERE installation_id = ?1",
        params![installation_id.0],
    )?;
    insert_detection(transaction, installation_id, detection)
}

fn insert_installation(
    transaction: &Transaction<'_>,
    installation: &Installation,
) -> rusqlite::Result<()> {
    let (work_code, identity_confidence, identity_reasons) =
        identity_parts(installation.detection.catalog_identity.as_ref())?;
    transaction.execute(
        "INSERT INTO library_installation
         (installation_id, scan_root_id, source_scan_session_id, root_path, platform, status,
          work_code, identity_confidence, identity_reason_codes, suggested_status,
          discovered_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            installation.id.0,
            installation.scan_root_id.as_ref().map(|id| id.0.as_str()),
            installation
                .detection
                .source_scan_session_id
                .as_ref()
                .map(|id| id.0.as_str()),
            installation.root_path,
            installation_platform(installation.platform),
            installation_status(installation.status),
            work_code,
            identity_confidence,
            identity_reasons,
            installation_status(installation.detection.suggested_status),
            installation.discovered_at,
            installation.updated_at,
        ],
    )?;
    Ok(())
}

fn insert_detection(
    transaction: &Transaction<'_>,
    installation_id: &InstallationId,
    detection: &InstallationDetection,
) -> rusqlite::Result<()> {
    for item in &detection.content_items {
        transaction.execute(
            "INSERT INTO library_content_item
             (installation_id, relative_path, path_key, media_type, size_bytes, modified_at,
              confidence, reason_codes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                installation_id.0,
                item.relative_path.as_str(),
                item.path_key,
                media_type(item.media_type),
                item.size_bytes.map(sqlite_u64).transpose()?,
                item.modified_at,
                inference_confidence(item.confidence),
                json(&item.reason_codes)?,
            ],
        )?;
    }
    for candidate in &detection.launch_candidates {
        let (target_kind, target_path) = target_parts(&candidate.target);
        transaction.execute(
            "INSERT INTO library_launch_candidate
             (installation_id, candidate_id, action_kind, target_kind, target_path,
              supported_platforms, confidence, reason_codes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                installation_id.0,
                candidate.id.0,
                launch_action_kind(candidate.action),
                target_kind,
                target_path,
                json(&candidate.supported_platforms)?,
                inference_confidence(candidate.confidence),
                json(&candidate.reason_codes)?,
            ],
        )?;
    }
    if let Some(inspection) = &detection.package_inspection {
        transaction.execute(
            "INSERT INTO library_package_inspection
             (installation_id, inspection, inspected_at)
             VALUES (?1, ?2, ?3)",
            params![
                installation_id.0,
                json(inspection)?,
                inspection.inspected_at,
            ],
        )?;
    }
    Ok(())
}

fn insert_overrides(
    transaction: &Transaction<'_>,
    installation_id: &InstallationId,
    overrides: &InstallationOverrides,
    updated_at: &str,
) -> rusqlite::Result<()> {
    let (identity_kind, identity_work_code) = manual_identity_parts(&overrides.catalog_identity);
    let (action_kind, target_kind, target_path) = match &overrides.preferred_action {
        Some(selection) => {
            let (target_kind, target_path) = target_parts(&selection.target);
            (
                Some(launch_action_kind(selection.action)),
                Some(target_kind),
                target_path,
            )
        }
        None => (None, None, None),
    };
    transaction.execute(
        "INSERT INTO library_installation_override
         (installation_id, identity_override_kind, identity_work_code, custom_title,
          preferred_action_kind, preferred_target_kind, preferred_target_path, reviewed_at,
          updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            installation_id.0,
            identity_kind,
            identity_work_code,
            overrides.custom_title,
            action_kind,
            target_kind,
            target_path,
            overrides.reviewed_at,
            updated_at,
        ],
    )?;
    for item in &overrides.content_items {
        transaction.execute(
            "INSERT INTO library_content_item_override
             (installation_id, relative_path, media_type, ignored, sequence_order)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                installation_id.0,
                item.relative_path.as_str(),
                item.media_type.map(media_type),
                item.ignored,
                item.order.map(i64::from),
            ],
        )?;
    }
    Ok(())
}

fn read_installation(row: &Row<'_>) -> rusqlite::Result<Installation> {
    let work_code = row.get::<_, Option<String>>(6)?;
    let confidence = row.get::<_, Option<String>>(7)?;
    let reason_codes = parse_json::<Vec<String>>(row, 8)?;
    let catalog_identity = match (work_code, confidence) {
        (Some(work_code), Some(confidence)) => Some(CatalogIdentity {
            work_code,
            confidence: parse_match_confidence(7, &confidence)?,
            reason_codes,
        }),
        (None, None) => None,
        _ => {
            return Err(conversion_error(
                6,
                io::Error::new(io::ErrorKind::InvalidData, "incomplete catalog identity"),
            ));
        }
    };
    let platform = row.get::<_, String>(4)?;
    let status = row.get::<_, String>(5)?;
    let suggested_status = row.get::<_, String>(9)?;
    Ok(Installation {
        id: InstallationId(row.get(0)?),
        scan_root_id: row.get::<_, Option<String>>(1)?.map(ScanRootId),
        root_path: row.get(3)?,
        platform: parse_platform(4, &platform)?,
        status: parse_installation_status(5, &status)?,
        detection: InstallationDetection {
            source_scan_session_id: row.get::<_, Option<String>>(2)?.map(ScanSessionId),
            catalog_identity,
            suggested_status: parse_installation_status(9, &suggested_status)?,
            content_items: Vec::new(),
            launch_candidates: Vec::new(),
            package_inspection: None,
        },
        overrides: InstallationOverrides::default(),
        discovered_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn read_package_inspection(
    connection: &rusqlite::Connection,
    installation_id: &InstallationId,
) -> rusqlite::Result<Option<PackageInspection>> {
    connection
        .query_row(
            "SELECT inspection
             FROM library_package_inspection
             WHERE installation_id = ?1",
            params![installation_id.0],
            |row| parse_json(row, 0),
        )
        .optional()
}

fn read_content_items(
    connection: &rusqlite::Connection,
    installation_id: &InstallationId,
) -> rusqlite::Result<Vec<ContentItem>> {
    let mut statement = connection.prepare(
        "SELECT relative_path, path_key, media_type, size_bytes, modified_at,
                confidence, reason_codes
         FROM library_content_item
         WHERE installation_id = ?1
         ORDER BY relative_path",
    )?;
    statement
        .query_map(params![installation_id.0], |row| {
            let relative_path = row.get::<_, String>(0)?;
            let media_type_value = row.get::<_, String>(2)?;
            let confidence = row.get::<_, String>(5)?;
            let size = row.get::<_, Option<i64>>(3)?;
            Ok(ContentItem {
                relative_path: RelativePath::parse(relative_path)
                    .map_err(|error| conversion_error(0, error))?,
                path_key: row.get(1)?,
                media_type: parse_media_type(2, &media_type_value)?,
                size_bytes: size.map(|value| numeric_conversion(3, value)).transpose()?,
                modified_at: row.get(4)?,
                confidence: parse_inference_confidence(5, &confidence)?,
                reason_codes: parse_json(row, 6)?,
            })
        })?
        .collect()
}

fn read_launch_candidates(
    connection: &rusqlite::Connection,
    installation_id: &InstallationId,
) -> rusqlite::Result<Vec<LaunchCandidate>> {
    let mut statement = connection.prepare(
        "SELECT candidate_id, action_kind, target_kind, target_path,
                supported_platforms, confidence, reason_codes
         FROM library_launch_candidate
         WHERE installation_id = ?1
         ORDER BY candidate_id",
    )?;
    statement
        .query_map(params![installation_id.0], |row| {
            let action = row.get::<_, String>(1)?;
            let target_kind = row.get::<_, String>(2)?;
            let target_path = row.get::<_, Option<String>>(3)?;
            let confidence = row.get::<_, String>(5)?;
            Ok(LaunchCandidate {
                id: LaunchCandidateId(row.get(0)?),
                action: parse_launch_action_kind(1, &action)?,
                target: parse_target(2, &target_kind, target_path)?,
                supported_platforms: parse_json(row, 4)?,
                confidence: parse_inference_confidence(5, &confidence)?,
                reason_codes: parse_json(row, 6)?,
            })
        })?
        .collect()
}

fn read_overrides(
    connection: &rusqlite::Connection,
    installation_id: &InstallationId,
) -> rusqlite::Result<InstallationOverrides> {
    let base = connection
        .query_row(
            "SELECT identity_override_kind, identity_work_code, custom_title,
                    preferred_action_kind, preferred_target_kind, preferred_target_path,
                    reviewed_at
             FROM library_installation_override
             WHERE installation_id = ?1",
            params![installation_id.0],
            |row| {
                let identity_kind = row.get::<_, Option<String>>(0)?;
                let identity_work_code = row.get::<_, Option<String>>(1)?;
                let catalog_identity =
                    parse_manual_identity(0, identity_kind.as_deref(), identity_work_code)?;
                let action = row.get::<_, Option<String>>(3)?;
                let target_kind = row.get::<_, Option<String>>(4)?;
                let target_path = row.get::<_, Option<String>>(5)?;
                let preferred_action = match (action, target_kind) {
                    (Some(action), Some(target_kind)) => Some(ManualLaunchSelection {
                        action: parse_launch_action_kind(3, &action)?,
                        target: parse_target(4, &target_kind, target_path)?,
                    }),
                    (None, None) => None,
                    _ => {
                        return Err(conversion_error(
                            3,
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "incomplete preferred action",
                            ),
                        ));
                    }
                };
                Ok(InstallationOverrides {
                    catalog_identity,
                    custom_title: row.get(2)?,
                    preferred_action,
                    content_items: Vec::new(),
                    reviewed_at: row.get(6)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default();

    let mut statement = connection.prepare(
        "SELECT relative_path, media_type, ignored, sequence_order
         FROM library_content_item_override
         WHERE installation_id = ?1
         ORDER BY relative_path",
    )?;
    let content_items = statement
        .query_map(params![installation_id.0], |row| {
            let path = row.get::<_, String>(0)?;
            let media_type_value = row.get::<_, Option<String>>(1)?;
            Ok(ContentItemOverride {
                relative_path: RelativePath::parse(path)
                    .map_err(|error| conversion_error(0, error))?,
                media_type: media_type_value
                    .as_deref()
                    .map(|value| parse_media_type(1, value))
                    .transpose()?,
                ignored: row.get(2)?,
                order: row
                    .get::<_, Option<i64>>(3)?
                    .map(|value| numeric_conversion(3, value))
                    .transpose()?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(InstallationOverrides {
        content_items,
        ..base
    })
}

fn identity_parts(
    identity: Option<&CatalogIdentity>,
) -> rusqlite::Result<(Option<&str>, Option<&'static str>, String)> {
    match identity {
        Some(identity) => Ok((
            Some(identity.work_code.as_str()),
            Some(scan_match_confidence(identity.confidence)),
            json(&identity.reason_codes)?,
        )),
        None => Ok((None, None, "[]".to_owned())),
    }
}

fn manual_identity_parts(
    identity: &Option<ManualCatalogIdentity>,
) -> (Option<&'static str>, Option<&str>) {
    match identity {
        Some(ManualCatalogIdentity::CatalogWork { work_code }) => {
            (Some("catalog_work"), Some(work_code.as_str()))
        }
        Some(ManualCatalogIdentity::Unidentified) => (Some("unidentified"), None),
        None => (None, None),
    }
}

fn parse_manual_identity(
    index: usize,
    kind: Option<&str>,
    work_code: Option<String>,
) -> rusqlite::Result<Option<ManualCatalogIdentity>> {
    match (kind, work_code) {
        (None, None) => Ok(None),
        (Some("catalog_work"), Some(work_code)) if !work_code.trim().is_empty() => {
            Ok(Some(ManualCatalogIdentity::CatalogWork { work_code }))
        }
        (Some("unidentified"), None) => Ok(Some(ManualCatalogIdentity::Unidentified)),
        _ => Err(conversion_error(
            index,
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid manual catalog identity",
            ),
        )),
    }
}

fn target_parts(target: &LaunchTarget) -> (&'static str, Option<&str>) {
    match target {
        LaunchTarget::InstallationRoot => ("installation_root", None),
        LaunchTarget::RelativePath(path) => ("relative_path", Some(path.as_str())),
    }
}

fn parse_target(index: usize, kind: &str, path: Option<String>) -> rusqlite::Result<LaunchTarget> {
    match (kind, path) {
        ("installation_root", None) => Ok(LaunchTarget::InstallationRoot),
        ("relative_path", Some(path)) => RelativePath::parse(path)
            .map(LaunchTarget::RelativePath)
            .map_err(|error| conversion_error(index, error)),
        _ => Err(conversion_error(
            index,
            io::Error::new(io::ErrorKind::InvalidData, "invalid launch target"),
        )),
    }
}

fn installation_platform(value: InstallationPlatform) -> &'static str {
    match value {
        InstallationPlatform::Windows => "windows",
        InstallationPlatform::Linux => "linux",
        InstallationPlatform::Macos => "macos",
        InstallationPlatform::Android => "android",
        InstallationPlatform::Ios => "ios",
        InstallationPlatform::Unknown => "unknown",
    }
}

fn parse_platform(index: usize, value: &str) -> rusqlite::Result<InstallationPlatform> {
    match value {
        "windows" => Ok(InstallationPlatform::Windows),
        "linux" => Ok(InstallationPlatform::Linux),
        "macos" => Ok(InstallationPlatform::Macos),
        "android" => Ok(InstallationPlatform::Android),
        "ios" => Ok(InstallationPlatform::Ios),
        "unknown" => Ok(InstallationPlatform::Unknown),
        _ => Err(invalid_value(index, "installation platform", value)),
    }
}

fn installation_status(value: InstallationStatus) -> &'static str {
    match value {
        InstallationStatus::Ready => "ready",
        InstallationStatus::NeedsReview => "needs_review",
    }
}

fn parse_installation_status(index: usize, value: &str) -> rusqlite::Result<InstallationStatus> {
    match value {
        "ready" => Ok(InstallationStatus::Ready),
        "needs_review" => Ok(InstallationStatus::NeedsReview),
        _ => Err(invalid_value(index, "installation status", value)),
    }
}

pub(crate) fn media_type(value: MediaType) -> &'static str {
    match value {
        MediaType::Executable => "executable",
        MediaType::Audio => "audio",
        MediaType::Image => "image",
        MediaType::Pdf => "pdf",
        MediaType::Video => "video",
        MediaType::Archive => "archive",
        MediaType::AndroidPackage => "android_package",
        MediaType::Directory => "directory",
        MediaType::Unknown => "unknown",
    }
}

pub(crate) fn parse_media_type(index: usize, value: &str) -> rusqlite::Result<MediaType> {
    match value {
        "executable" => Ok(MediaType::Executable),
        "audio" => Ok(MediaType::Audio),
        "image" => Ok(MediaType::Image),
        "pdf" => Ok(MediaType::Pdf),
        "video" => Ok(MediaType::Video),
        "archive" => Ok(MediaType::Archive),
        "android_package" => Ok(MediaType::AndroidPackage),
        "directory" => Ok(MediaType::Directory),
        "unknown" => Ok(MediaType::Unknown),
        _ => Err(invalid_value(index, "media type", value)),
    }
}

fn inference_confidence(value: InferenceConfidence) -> &'static str {
    match value {
        InferenceConfidence::Low => "low",
        InferenceConfidence::Medium => "medium",
        InferenceConfidence::High => "high",
    }
}

fn parse_inference_confidence(index: usize, value: &str) -> rusqlite::Result<InferenceConfidence> {
    match value {
        "low" => Ok(InferenceConfidence::Low),
        "medium" => Ok(InferenceConfidence::Medium),
        "high" => Ok(InferenceConfidence::High),
        _ => Err(invalid_value(index, "inference confidence", value)),
    }
}

fn launch_action_kind(value: LaunchActionKind) -> &'static str {
    match value {
        LaunchActionKind::LaunchExecutable => "launch_executable",
        LaunchActionKind::PlayAudio => "play_audio",
        LaunchActionKind::ReadImages => "read_images",
        LaunchActionKind::OpenDocument => "open_document",
        LaunchActionKind::PlayVideo => "play_video",
        LaunchActionKind::OpenArchive => "open_archive",
        LaunchActionKind::OpenAndroidPackage => "open_android_package",
    }
}

fn parse_launch_action_kind(index: usize, value: &str) -> rusqlite::Result<LaunchActionKind> {
    match value {
        "launch_executable" => Ok(LaunchActionKind::LaunchExecutable),
        "play_audio" => Ok(LaunchActionKind::PlayAudio),
        "read_images" => Ok(LaunchActionKind::ReadImages),
        "open_document" => Ok(LaunchActionKind::OpenDocument),
        "play_video" => Ok(LaunchActionKind::PlayVideo),
        "open_archive" => Ok(LaunchActionKind::OpenArchive),
        "open_android_package" => Ok(LaunchActionKind::OpenAndroidPackage),
        _ => Err(invalid_value(index, "launch action", value)),
    }
}

fn scan_match_confidence(value: ScanMatchConfidence) -> &'static str {
    match value {
        ScanMatchConfidence::Possible => "possible",
        ScanMatchConfidence::Strong => "strong",
        ScanMatchConfidence::Exact => "exact",
    }
}

fn parse_match_confidence(index: usize, value: &str) -> rusqlite::Result<ScanMatchConfidence> {
    match value {
        "possible" => Ok(ScanMatchConfidence::Possible),
        "strong" => Ok(ScanMatchConfidence::Strong),
        "exact" => Ok(ScanMatchConfidence::Exact),
        _ => Err(invalid_value(index, "scan confidence", value)),
    }
}

fn json<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value).map_err(|error| conversion_error(0, error))
}

fn parse_json<T: serde::de::DeserializeOwned>(row: &Row<'_>, index: usize) -> rusqlite::Result<T> {
    let value = row.get::<_, String>(index)?;
    serde_json::from_str(&value).map_err(|error| conversion_error(index, error))
}

fn sqlite_u64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|error| conversion_error(0, error))
}

fn numeric_conversion<T>(index: usize, value: i64) -> rusqlite::Result<T>
where
    T: TryFrom<i64>,
    T::Error: std::error::Error + Send + Sync + 'static,
{
    T::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
    })
}

fn require_changed(changed: usize) -> rusqlite::Result<()> {
    if changed == 0 {
        Err(rusqlite::Error::QueryReturnedNoRows)
    } else {
        Ok(())
    }
}

fn invalid_value(index: usize, name: &str, value: &str) -> rusqlite::Error {
    conversion_error(
        index,
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid {name}: {value}"),
        ),
    )
}

fn conversion_error(
    index: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dla_application::installation::InstallationLibrary;
    use dla_domain::{
        package::{
            ArchiveFormat, ArchiveRetentionPolicy, InstallPlan, PackageClassification,
            PackageContentKind, PackageLaunchCandidate, PackageSafety, SourceArtifact,
            SourceArtifactKind,
        },
        scanner::ScanEntryId,
    };
    use tempfile::tempdir;

    use super::*;

    fn path(value: &str) -> RelativePath {
        RelativePath::parse(value).expect("fixture path")
    }

    fn detection(paths: &[&str]) -> InstallationDetection {
        InstallationDetection {
            source_scan_session_id: None,
            catalog_identity: Some(CatalogIdentity {
                work_code: "RJ01326398".to_owned(),
                confidence: ScanMatchConfidence::Strong,
                reason_codes: vec!["code_in_directory_name".to_owned()],
            }),
            suggested_status: InstallationStatus::Ready,
            content_items: paths
                .iter()
                .map(|value| ContentItem {
                    relative_path: path(value),
                    path_key: value.to_ascii_lowercase(),
                    media_type: MediaType::Executable,
                    size_bytes: Some(7),
                    modified_at: Some("2026-08-07T00:00:00Z".to_owned()),
                    confidence: InferenceConfidence::High,
                    reason_codes: vec!["file_extension".to_owned()],
                })
                .collect(),
            launch_candidates: Vec::new(),
            package_inspection: None,
        }
    }

    fn package_inspection() -> PackageInspection {
        let action = PackageLaunchCandidate {
            action: LaunchActionKind::LaunchExecutable,
            relative_path: path("Work/Game.exe"),
            supported_platforms: vec![InstallationPlatform::Windows],
            confidence: InferenceConfidence::High,
            reason_codes: vec!["conventional_game_executable".to_owned()],
            expected_sha256: None,
        };
        PackageInspection {
            source: SourceArtifact {
                scan_entry_id: ScanEntryId("entry-package".to_owned()),
                kind: SourceArtifactKind::Archive,
                relative_path: path("RJ01326398.zip"),
                size_bytes: Some(128),
                sha256: Some("fixture-sha256".to_owned()),
            },
            source_set: None,
            catalog_release: None,
            format: ArchiveFormat::Zip,
            safety: PackageSafety::Safe,
            entry_count: 3,
            file_count: 3,
            directory_count: 0,
            total_compressed_bytes: 128,
            total_uncompressed_bytes: 256,
            common_root: Some(path("Work")),
            issues: Vec::new(),
            classification: PackageClassification {
                content_kind: PackageContentKind::WindowsGame,
                engine: Some("RPG Maker / NW.js".to_owned()),
                platform: InstallationPlatform::Windows,
                confidence: InferenceConfidence::High,
                reason_codes: vec!["rpg_maker_layout".to_owned()],
                content_root: Some(path("Work")),
                launch_candidates: vec![action.clone()],
            },
            install_plan: InstallPlan {
                requires_extraction: true,
                content_root: Some(path("Work")),
                preferred_action: Some(action),
                archive_retention: ArchiveRetentionPolicy::Keep,
            },
            inspected_at: "2026-08-08T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn sqlite_round_trips_read_only_package_inspection() {
        let directory = tempdir().expect("temporary directory");
        let store = Arc::new(
            SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
                .expect("library store"),
        );
        let library = InstallationLibrary::new(store);
        let mut detected = detection(&["RJ01326398.zip"]);
        detected.package_inspection = Some(package_inspection());
        let installation = Installation {
            id: InstallationId("installation-package".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/library".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: detected,
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-08-08T00:00:00Z".to_owned(),
            updated_at: "2026-08-08T00:00:00Z".to_owned(),
        };

        library.create(&installation).expect("create installation");
        let restored = library
            .read(&installation.id)
            .expect("read installation")
            .expect("stored installation");

        assert_eq!(
            restored.detection.package_inspection,
            installation.detection.package_inspection
        );
    }

    #[test]
    fn sqlite_refresh_replaces_detection_but_preserves_manual_state() {
        let directory = tempdir().expect("temporary directory");
        let store = Arc::new(
            SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
                .expect("library store"),
        );
        let library = InstallationLibrary::new(store);
        let game = path("Game.exe");
        let installation = Installation {
            id: InstallationId("installation-1".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/library/RJ01326398".to_owned(),
            platform: InstallationPlatform::Windows,
            status: InstallationStatus::Ready,
            detection: detection(&["Game.exe"]),
            overrides: InstallationOverrides {
                custom_title: Some("Preferred title".to_owned()),
                preferred_action: Some(ManualLaunchSelection {
                    action: LaunchActionKind::LaunchExecutable,
                    target: LaunchTarget::RelativePath(game.clone()),
                }),
                content_items: vec![ContentItemOverride {
                    relative_path: game,
                    media_type: Some(MediaType::Executable),
                    ignored: false,
                    order: Some(0),
                }],
                ..InstallationOverrides::default()
            },
            discovered_at: "2026-08-07T00:00:00Z".to_owned(),
            updated_at: "2026-08-07T00:00:00Z".to_owned(),
        };
        library.create(&installation).expect("create installation");

        library
            .replace_detection(
                &installation.id,
                detection(&["Launcher.exe"]),
                "2026-08-07T01:00:00Z".to_owned(),
            )
            .expect("replace detection");
        let restored = library
            .read(&installation.id)
            .expect("read installation")
            .expect("stored installation");

        assert_eq!(restored.status, InstallationStatus::NeedsReview);
        assert_eq!(restored.overrides, installation.overrides);
        assert_eq!(restored.detection.content_items.len(), 1);
        assert_eq!(
            restored.detection.content_items[0].relative_path.as_str(),
            "Launcher.exe"
        );
    }

    #[test]
    fn installation_identity_does_not_require_a_library_or_catalog_work_row() {
        let directory = tempdir().expect("temporary directory");
        let store = Arc::new(
            SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
                .expect("library store"),
        );
        let library = InstallationLibrary::new(store);
        let installation = Installation {
            id: InstallationId("installation-independent".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/library/RJ01326398".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: detection(&[]),
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-08-07T00:00:00Z".to_owned(),
            updated_at: "2026-08-07T00:00:00Z".to_owned(),
        };

        library.create(&installation).expect("independent identity");
        assert!(
            library
                .read(&installation.id)
                .expect("read installation")
                .is_some()
        );
    }

    #[test]
    fn work_lookup_uses_the_effective_catalog_identity() {
        let directory = tempdir().expect("temporary directory");
        let store = Arc::new(
            SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
                .expect("library store"),
        );
        let library = InstallationLibrary::new(store);
        let base = Installation {
            id: InstallationId("installation-detected".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/library/detected".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: detection(&[]),
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-08-09T00:00:00Z".to_owned(),
            updated_at: "2026-08-09T00:00:00Z".to_owned(),
        };
        let mut reassigned = base.clone();
        reassigned.id = InstallationId("installation-reassigned".to_owned());
        reassigned.root_path = "/synthetic/library/reassigned".to_owned();
        reassigned
            .detection
            .catalog_identity
            .as_mut()
            .expect("detected identity")
            .work_code = "RJ09999999".to_owned();
        reassigned.overrides.catalog_identity = Some(ManualCatalogIdentity::CatalogWork {
            work_code: "RJ01326398".to_owned(),
        });
        reassigned.updated_at = "2026-08-09T01:00:00Z".to_owned();
        let mut unidentified = base.clone();
        unidentified.id = InstallationId("installation-unidentified".to_owned());
        unidentified.root_path = "/synthetic/library/unidentified".to_owned();
        unidentified.overrides.catalog_identity = Some(ManualCatalogIdentity::Unidentified);
        unidentified.updated_at = "2026-08-09T02:00:00Z".to_owned();

        library.create(&base).expect("detected installation");
        library
            .create(&reassigned)
            .expect("reassigned installation");
        library
            .create(&unidentified)
            .expect("unidentified installation");

        let matches = library
            .find_by_work_code("rj01326398")
            .expect("work installations");
        assert_eq!(
            matches
                .iter()
                .map(|installation| installation.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["installation-reassigned", "installation-detected"]
        );
    }

    #[test]
    fn sqlite_round_trips_review_identity_timestamp_and_content_corrections() {
        let directory = tempdir().expect("temporary directory");
        let store = Arc::new(
            SqliteLibraryStore::open(&directory.path().join("library.sqlite"))
                .expect("library store"),
        );
        let library = InstallationLibrary::new(store);
        let installation = Installation {
            id: InstallationId("installation-reviewed".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/library/reviewed".to_owned(),
            platform: InstallationPlatform::Windows,
            status: InstallationStatus::Ready,
            detection: detection(&["Game.exe"]),
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-08-07T00:00:00Z".to_owned(),
            updated_at: "2026-08-07T00:00:00Z".to_owned(),
        };
        library.create(&installation).expect("create installation");
        let overrides = InstallationOverrides {
            catalog_identity: Some(ManualCatalogIdentity::Unidentified),
            custom_title: Some("Reviewed game".to_owned()),
            preferred_action: Some(ManualLaunchSelection {
                action: LaunchActionKind::LaunchExecutable,
                target: LaunchTarget::RelativePath(path("Game.exe")),
            }),
            content_items: vec![ContentItemOverride {
                relative_path: path("Game.exe"),
                media_type: Some(MediaType::Unknown),
                ignored: false,
                order: Some(3),
            }],
            reviewed_at: Some("2026-08-07T02:00:00Z".to_owned()),
        };

        library
            .replace_overrides(
                &installation.id,
                overrides.clone(),
                "2026-08-07T02:00:00Z".to_owned(),
            )
            .expect("save review");
        let restored = library
            .read(&installation.id)
            .expect("read installation")
            .expect("stored installation");

        assert_eq!(restored.status, InstallationStatus::Ready);
        assert_eq!(restored.overrides, overrides);
        assert_eq!(restored.updated_at, "2026-08-07T02:00:00Z");
    }
}
