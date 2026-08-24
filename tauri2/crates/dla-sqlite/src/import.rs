use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use dla_application::catalog_import::{
    CatalogImportCancellationToken, CatalogImportError, CatalogPackageManifest,
    CatalogPackageProfile,
};
use dla_domain::{CatalogRelation, CatalogRom, CatalogRomEntry, CatalogWorkDetail};
use rusqlite::{Connection, params};
use serde_json::{Map, Value};

use crate::{
    catalog::{CATALOG_MIGRATIONS, insert_work},
    database,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CatalogDatabaseCounts {
    pub unique_works: u64,
    pub roms: u64,
    pub files: u64,
    pub relations: u64,
}

pub struct SqliteCatalogImportWriter {
    connection: Connection,
    work_codes: HashMap<String, String>,
    next_rom_positions: HashMap<String, usize>,
    rom_file_counts: HashMap<(String, usize), u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogDatabaseFinalizeStage {
    UpdatingRomCounts,
    ValidatingCounts,
    WritingMetadata,
    Committing,
    Checkpointing,
    CheckingIntegrity,
    CheckingForeignKeys,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CatalogDatabaseFinalizeProgress {
    pub stage: CatalogDatabaseFinalizeStage,
    pub completed: u64,
    pub total: u64,
}

const UPDATE_CONTENT_SCAN_ENTRY_COUNT: &str = "UPDATE catalog_rom_content_scan SET entry_count = ?3 WHERE work_code = ?1 AND rom_position = ?2";
const UPDATE_ROM_FILE_COUNT: &str =
    "UPDATE catalog_rom SET file_count = ?3 WHERE work_code = ?1 AND position = ?2";

impl SqliteCatalogImportWriter {
    pub fn create(path: &Path) -> rusqlite::Result<Self> {
        if path.exists() {
            return Err(rusqlite::Error::ToSqlConversionFailure(
                format!("candidate catalog already exists: {}", path.display()).into(),
            ));
        }
        let connection = database::open(path, &CATALOG_MIGRATIONS)?;
        connection.execute_batch("BEGIN IMMEDIATE")?;
        Ok(Self {
            connection,
            work_codes: HashMap::new(),
            next_rom_positions: HashMap::new(),
            rom_file_counts: HashMap::new(),
        })
    }

    pub fn ensure_work(&mut self, mut detail: CatalogWorkDetail) -> rusqlite::Result<bool> {
        let normalized = detail.work.code.to_ascii_lowercase();
        if self.work_codes.contains_key(&normalized) {
            return Ok(false);
        }
        let canonical = detail.work.code.clone();
        detail.roms.clear();
        insert_work(&self.connection, &detail)?;
        self.work_codes.insert(normalized, canonical.clone());
        self.next_rom_positions.insert(canonical, 0);
        Ok(true)
    }

    pub fn insert_rom(&mut self, work_code: &str, rom: &CatalogRom) -> rusqlite::Result<usize> {
        let canonical = self.canonical_work_code(work_code)?;
        let position = self
            .next_rom_positions
            .get_mut(&canonical)
            .ok_or_else(|| unknown_work_error(work_code))?;
        let current = *position;
        self.connection.execute(
            "INSERT INTO catalog_rom
             (work_code, position, name, size, crc, md5, sha1, sha256, file_count, update_date, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                canonical,
                current as i64,
                rom.name,
                rom.size,
                rom.crc,
                rom.md5,
                rom.sha1,
                rom.sha256,
                rom.file_count.map(|count| count as i64),
                rom.update_date,
                rom.version,
            ],
        )?;
        *position += 1;
        Ok(current)
    }

    pub fn apply_dat_metadata(
        &self,
        work_code: &str,
        site_name: &str,
        drm_values: &[String],
    ) -> rusqlite::Result<()> {
        let canonical = self.canonical_work_code(work_code)?;
        self.ensure_enrichment(&canonical)?;
        self.connection.execute(
            "UPDATE catalog_work_enrichment
             SET site_name = ?2, drm_values = ?3
             WHERE work_code = ?1",
            params![
                canonical,
                site_name,
                serde_json::to_string(drm_values).map_err(to_sql_error)?
            ],
        )?;
        Ok(())
    }

    pub fn insert_rom_file(
        &mut self,
        work_code: &str,
        rom_position: usize,
        entry: &CatalogRomEntry,
    ) -> rusqlite::Result<()> {
        let canonical = self.canonical_work_code(work_code)?;
        let key = (canonical.clone(), rom_position);
        let count = self.rom_file_counts.entry(key).or_default();
        self.connection.execute(
            "INSERT INTO catalog_rom_content_entry
             (work_code, rom_position, entry_index, path, extension, is_directory,
              size, crc32, md5, sha1, sha256, hash_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                canonical,
                rom_position as i64,
                *count as i64,
                entry.path,
                entry.extension,
                entry.is_directory,
                entry.size,
                entry.crc32,
                entry.md5,
                entry.sha1,
                entry.sha256,
                entry.hash_status,
            ],
        )?;
        *count += 1;
        Ok(())
    }

    pub fn initialize_rom_contents(
        &mut self,
        work_code: &str,
        rom_position: usize,
    ) -> rusqlite::Result<()> {
        let canonical = self.canonical_work_code(work_code)?;
        self.connection.execute(
            "INSERT INTO catalog_rom_content_scan
             (work_code, rom_position, status, archive_format, entry_count,
              total_uncompressed_size, truncated)
             VALUES (?1, ?2, 'complete', '', 0, NULL, 0)
             ON CONFLICT(work_code, rom_position) DO NOTHING",
            params![canonical, rom_position as i64],
        )?;
        self.rom_file_counts
            .entry((canonical, rom_position))
            .or_default();
        Ok(())
    }

    pub fn apply_enrichment(
        &self,
        work_code: &str,
        fields: &Map<String, Value>,
    ) -> rusqlite::Result<()> {
        let canonical = self.canonical_work_code(work_code)?;

        self.ensure_enrichment(&canonical)?;
        self.connection.execute(
            "UPDATE catalog_work_enrichment SET raw_fields = ?2
             WHERE work_code = ?1",
            params![
                canonical,
                serde_json::to_string(fields).map_err(to_sql_error)?
            ],
        )?;

        for (field, value) in fields {
            match field.as_str() {
                "work.titleEnglish" => self.update_text(&canonical, "title_english", value)?,
                "work.addedDate" => self.update_text(&canonical, "added_date", value)?,
                "work.updatedDate" => self.update_text(&canonical, "updated_date", value)?,
                "work.ageRating" => self.update_text(&canonical, "age_rating", value)?,
                "work.releaseType" => self.update_text(&canonical, "release_type", value)?,
                "work.images.main" => self.update_json(&canonical, "main_image_urls", value)?,
                "work.images.thumbnail" => self.update_json(&canonical, "thumbnail_urls", value)?,
                "work.images.samples" => self.update_json(&canonical, "sample_image_urls", value)?,
                "work.rating.score" => self.update_number(&canonical, "rating_score", value)?,
                "work.rating.count" => self.update_integer(&canonical, "rating_count", value)?,
                "work.rating.totalSales" => self.update_integer(&canonical, "total_sales", value)?,
                "work.rating.rankings" => self.update_json(&canonical, "rating_rankings", value)?,
                "work.titleKana" => self.update_enrichment_text(&canonical, "title_kana", value)?,
                "work.titleRomaji" => {
                    self.update_enrichment_text(&canonical, "title_romaji", value)?
                }
                "work.sourceUrl" => self.update_enrichment_text(&canonical, "source_url", value)?,
                "work.descriptions" => {
                    self.update_enrichment_json(&canonical, "description_versions", value)?
                }
                "work.rating.favorites" => {
                    self.update_enrichment_integer(&canonical, "favorites_count", value)?
                }
                "rom.fileCount" => self.update_rom_values(&canonical, value, |connection, code, position, value| {
                    let count = optional_u64(value)?.map(|count| count as i64);
                    connection.execute(
                        UPDATE_ROM_FILE_COUNT,
                        params![code, position as i64, count],
                    )?;
                    Ok(())
                })?,
                "rom.updateDate" => self.update_rom_values(&canonical, value, |connection, code, position, value| {
                    let date = optional_text(value)?.unwrap_or_default();
                    connection.execute(
                        "UPDATE catalog_rom SET update_date = ?3 WHERE work_code = ?1 AND position = ?2",
                        params![code, position as i64, date],
                    )?;
                    Ok(())
                })?,
                _ => {}
            }
        }
        Ok(())
    }

    pub fn insert_relation(&self, relation: &CatalogRelation) -> rusqlite::Result<()> {
        let parent_work_code = self.canonical_work_code(&relation.parent_work_code)?;
        let child_work_code = self.canonical_work_code(&relation.child_work_code)?;
        self.connection.execute(
            "INSERT INTO catalog_relation_type (relation_type_code, label)
             VALUES (?1, ?2)
             ON CONFLICT(relation_type_code) DO UPDATE SET label = excluded.label",
            params![relation.relation_type_code, relation.relation_type_label],
        )?;
        self.connection.execute(
            "INSERT INTO catalog_work_relation
             (parent_work_code, child_work_code, relation_type_code)
             VALUES (?1, ?2, ?3)",
            params![
                parent_work_code,
                child_work_code,
                relation.relation_type_code
            ],
        )?;
        Ok(())
    }

    pub fn finish(
        self,
        manifest: &CatalogPackageManifest,
        imported_at: &str,
        cancellation: &CatalogImportCancellationToken,
        mut on_progress: impl FnMut(CatalogDatabaseFinalizeProgress) -> Result<(), CatalogImportError>,
    ) -> Result<CatalogDatabaseCounts, CatalogImportError> {
        let sqlite_cancellation = cancellation.clone();
        map_sqlite_result(
            self.connection
                .progress_handler(10_000, Some(move || sqlite_cancellation.is_cancelled())),
            cancellation,
        )?;

        let total_roms = self.rom_file_counts.len() as u64;
        on_progress(finalize_progress(
            CatalogDatabaseFinalizeStage::UpdatingRomCounts,
            0,
            total_roms,
        ))?;
        {
            let mut update_content_scan = map_sqlite_result(
                self.connection.prepare(UPDATE_CONTENT_SCAN_ENTRY_COUNT),
                cancellation,
            )?;
            let mut update_rom =
                map_sqlite_result(self.connection.prepare(UPDATE_ROM_FILE_COUNT), cancellation)?;
            for (index, ((work_code, rom_position), count)) in
                self.rom_file_counts.iter().enumerate()
            {
                check_cancelled(cancellation)?;
                map_sqlite_result(
                    update_content_scan.execute(params![
                        work_code,
                        *rom_position as i64,
                        *count as i64
                    ]),
                    cancellation,
                )?;
                map_sqlite_result(
                    update_rom.execute(params![work_code, *rom_position as i64, *count as i64]),
                    cancellation,
                )?;
                let completed = index as u64 + 1;
                if completed.is_multiple_of(256) || completed == total_roms {
                    on_progress(finalize_progress(
                        CatalogDatabaseFinalizeStage::UpdatingRomCounts,
                        completed,
                        total_roms,
                    ))?;
                }
            }
        }

        check_cancelled(cancellation)?;
        on_progress(finalize_progress(
            CatalogDatabaseFinalizeStage::ValidatingCounts,
            0,
            0,
        ))?;
        let counts = map_sqlite_result(read_counts(&self.connection), cancellation)?;
        map_sqlite_result(validate_manifest_counts(manifest, counts), cancellation)?;

        on_progress(finalize_progress(
            CatalogDatabaseFinalizeStage::WritingMetadata,
            0,
            0,
        ))?;
        map_sqlite_result(self.connection.execute(
            "INSERT INTO catalog_snapshot
             (singleton, snapshot_id, schema_version, real_work_count, synthetic_work_count, imported_at)
             VALUES (1, ?1, ?2, ?3, 0, ?4)",
            params![
                manifest.snapshot_id,
                i64::from(manifest.catalog_schema_version),
                counts.unique_works as i64,
                imported_at,
            ],
        ), cancellation)?;
        let manifest_json =
            serde_json::to_string(manifest).map_err(CatalogImportError::persistence)?;
        map_sqlite_result(
            self.connection.execute(
                "INSERT INTO catalog_import_metadata
             (singleton, package_format_version, profile, source_id, source_name, manifest_json)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)",
                params![
                    i64::from(manifest.format_version),
                    profile_name(manifest.profile),
                    manifest.source.id,
                    manifest.source.name,
                    manifest_json,
                ],
            ),
            cancellation,
        )?;
        for field in &manifest.fields {
            check_cancelled(cancellation)?;
            map_sqlite_result(
                self.connection.execute(
                    "INSERT INTO catalog_field_presence (field_id) VALUES (?1)",
                    params![field],
                ),
                cancellation,
            )?;
        }

        check_cancelled(cancellation)?;
        on_progress(finalize_progress(
            CatalogDatabaseFinalizeStage::Committing,
            0,
            0,
        ))?;
        map_sqlite_result(self.connection.execute_batch("COMMIT"), cancellation)?;

        check_cancelled(cancellation)?;
        on_progress(finalize_progress(
            CatalogDatabaseFinalizeStage::Checkpointing,
            0,
            0,
        ))?;
        map_sqlite_result(
            self.connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)"),
            cancellation,
        )?;

        check_cancelled(cancellation)?;
        on_progress(finalize_progress(
            CatalogDatabaseFinalizeStage::CheckingIntegrity,
            0,
            0,
        ))?;
        map_sqlite_result(validate_integrity(&self.connection), cancellation)?;

        check_cancelled(cancellation)?;
        on_progress(finalize_progress(
            CatalogDatabaseFinalizeStage::CheckingForeignKeys,
            0,
            0,
        ))?;
        map_sqlite_result(validate_foreign_keys(&self.connection), cancellation)?;
        map_sqlite_result(
            self.connection.progress_handler(0, None::<fn() -> bool>),
            cancellation,
        )?;
        Ok(counts)
    }

    fn update_text(&self, work_code: &str, column: &str, value: &Value) -> rusqlite::Result<()> {
        let value = optional_text(value)?.unwrap_or_default();
        self.connection.execute(
            &format!("UPDATE catalog_work SET {column} = ?2 WHERE work_code = ?1"),
            params![work_code, value],
        )?;
        Ok(())
    }

    fn update_json(&self, work_code: &str, column: &str, value: &Value) -> rusqlite::Result<()> {
        let value = if value.is_null() {
            "[]".to_owned()
        } else {
            serde_json::to_string(value).map_err(to_sql_error)?
        };
        self.connection.execute(
            &format!("UPDATE catalog_work SET {column} = ?2 WHERE work_code = ?1"),
            params![work_code, value],
        )?;
        Ok(())
    }

    fn update_number(&self, work_code: &str, column: &str, value: &Value) -> rusqlite::Result<()> {
        let value = optional_f64(value)?;
        self.connection.execute(
            &format!("UPDATE catalog_work SET {column} = ?2 WHERE work_code = ?1"),
            params![work_code, value],
        )?;
        Ok(())
    }

    fn update_integer(&self, work_code: &str, column: &str, value: &Value) -> rusqlite::Result<()> {
        let value = optional_u64(value)?.map(|number| number as i64);
        self.connection.execute(
            &format!("UPDATE catalog_work SET {column} = ?2 WHERE work_code = ?1"),
            params![work_code, value],
        )?;
        Ok(())
    }

    fn ensure_enrichment(&self, work_code: &str) -> rusqlite::Result<()> {
        self.connection.execute(
            "INSERT INTO catalog_work_enrichment (work_code) VALUES (?1)
             ON CONFLICT(work_code) DO NOTHING",
            params![work_code],
        )?;
        Ok(())
    }

    fn update_enrichment_text(
        &self,
        work_code: &str,
        column: &str,
        value: &Value,
    ) -> rusqlite::Result<()> {
        self.ensure_enrichment(work_code)?;
        let value = optional_text(value)?;
        self.connection.execute(
            &format!("UPDATE catalog_work_enrichment SET {column} = ?2 WHERE work_code = ?1"),
            params![work_code, value],
        )?;
        Ok(())
    }

    fn update_enrichment_json(
        &self,
        work_code: &str,
        column: &str,
        value: &Value,
    ) -> rusqlite::Result<()> {
        self.ensure_enrichment(work_code)?;
        let value = if value.is_null() {
            None
        } else {
            Some(serde_json::to_string(value).map_err(to_sql_error)?)
        };
        self.connection.execute(
            &format!("UPDATE catalog_work_enrichment SET {column} = ?2 WHERE work_code = ?1"),
            params![work_code, value],
        )?;
        Ok(())
    }

    fn update_enrichment_integer(
        &self,
        work_code: &str,
        column: &str,
        value: &Value,
    ) -> rusqlite::Result<()> {
        self.ensure_enrichment(work_code)?;
        let value = optional_u64(value)?.map(|number| number as i64);
        self.connection.execute(
            &format!("UPDATE catalog_work_enrichment SET {column} = ?2 WHERE work_code = ?1"),
            params![work_code, value],
        )?;
        Ok(())
    }

    fn update_rom_values(
        &self,
        work_code: &str,
        value: &Value,
        mut update: impl FnMut(&Connection, &str, usize, &Value) -> rusqlite::Result<()>,
    ) -> rusqlite::Result<()> {
        let rom_count = self.connection.query_row(
            "SELECT count(*) FROM catalog_rom WHERE work_code = ?1",
            params![work_code],
            |row| row.get::<_, i64>(0),
        )? as usize;

        if value.is_null() {
            for position in 0..rom_count {
                update(&self.connection, work_code, position, &Value::Null)?;
            }
            return Ok(());
        }

        let entries = value.as_array().ok_or_else(|| {
            rusqlite::Error::ToSqlConversionFailure(
                "per-ROM enrichment value must be an array or null".into(),
            )
        })?;
        if entries.len() != rom_count {
            return Err(rusqlite::Error::ToSqlConversionFailure(
                format!(
                    "per-ROM enrichment for {work_code} has {} entries; expected {rom_count}",
                    entries.len()
                )
                .into(),
            ));
        }

        let mut positions = HashSet::new();
        for entry in entries {
            let object = entry.as_object().ok_or_else(|| {
                rusqlite::Error::ToSqlConversionFailure(
                    "per-ROM enrichment entries must be objects".into(),
                )
            })?;
            let position = object
                .get("position")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    rusqlite::Error::ToSqlConversionFailure(
                        "per-ROM enrichment position must be a non-negative integer".into(),
                    )
                })? as usize;
            if position >= rom_count || !positions.insert(position) {
                return Err(rusqlite::Error::ToSqlConversionFailure(
                    format!("invalid or duplicate ROM position {position} for {work_code}").into(),
                ));
            }
            let entry_value = object.get("value").ok_or_else(|| {
                rusqlite::Error::ToSqlConversionFailure(
                    "per-ROM enrichment entry is missing value".into(),
                )
            })?;
            update(&self.connection, work_code, position, entry_value)?;
        }
        Ok(())
    }

    fn canonical_work_code(&self, work_code: &str) -> rusqlite::Result<String> {
        self.work_codes
            .get(&work_code.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| unknown_work_error(work_code))
    }
}

fn read_counts(connection: &Connection) -> rusqlite::Result<CatalogDatabaseCounts> {
    Ok(CatalogDatabaseCounts {
        unique_works: count(connection, "catalog_work")?,
        roms: count(connection, "catalog_rom")?,
        files: count(connection, "catalog_rom_content_entry")?,
        relations: count(connection, "catalog_work_relation")?,
    })
}

fn count(connection: &Connection, table: &str) -> rusqlite::Result<u64> {
    connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get::<_, i64>(0).map(|value| value as u64)
    })
}

fn validate_manifest_counts(
    manifest: &CatalogPackageManifest,
    actual: CatalogDatabaseCounts,
) -> rusqlite::Result<()> {
    for (label, expected, observed) in [
        (
            "unique works",
            manifest.counts.unique_works,
            actual.unique_works,
        ),
        ("ROMs", manifest.counts.roms, actual.roms),
        ("files", manifest.counts.files, actual.files),
        ("relations", manifest.counts.relations, actual.relations),
    ] {
        if expected != observed {
            return Err(rusqlite::Error::ToSqlConversionFailure(
                format!("manifest declares {expected} {label}, imported {observed}").into(),
            ));
        }
    }
    Ok(())
}

fn validate_integrity(connection: &Connection) -> rusqlite::Result<()> {
    let integrity =
        connection.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?;
    if integrity != "ok" {
        return Err(rusqlite::Error::ToSqlConversionFailure(
            format!("SQLite integrity check failed: {integrity}").into(),
        ));
    }
    Ok(())
}

fn validate_foreign_keys(connection: &Connection) -> rusqlite::Result<()> {
    let foreign_key_violations =
        connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| {
            row.get::<_, i64>(0)
        })?;
    if foreign_key_violations != 0 {
        return Err(rusqlite::Error::ToSqlConversionFailure(
            format!("SQLite foreign key check found {foreign_key_violations} violations").into(),
        ));
    }
    Ok(())
}

fn finalize_progress(
    stage: CatalogDatabaseFinalizeStage,
    completed: u64,
    total: u64,
) -> CatalogDatabaseFinalizeProgress {
    CatalogDatabaseFinalizeProgress {
        stage,
        completed,
        total,
    }
}

fn check_cancelled(
    cancellation: &CatalogImportCancellationToken,
) -> Result<(), CatalogImportError> {
    if cancellation.is_cancelled() {
        Err(CatalogImportError::Cancelled)
    } else {
        Ok(())
    }
}

fn map_sqlite_result<T>(
    result: rusqlite::Result<T>,
    cancellation: &CatalogImportCancellationToken,
) -> Result<T, CatalogImportError> {
    match result {
        Ok(value) => Ok(value),
        Err(_) if cancellation.is_cancelled() => Err(CatalogImportError::Cancelled),
        Err(error) => Err(CatalogImportError::persistence(error)),
    }
}

fn unknown_work_error(work_code: &str) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(
        format!("catalog import references unknown work {work_code}").into(),
    )
}

fn optional_text(value: &Value) -> rusqlite::Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_str()
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| rusqlite::Error::ToSqlConversionFailure("expected a string or null".into()))
}

fn optional_u64(value: &Value) -> rusqlite::Result<Option<u64>> {
    if value.is_null() {
        return Ok(None);
    }
    value.as_u64().map(Some).ok_or_else(|| {
        rusqlite::Error::ToSqlConversionFailure("expected an unsigned integer or null".into())
    })
}

fn optional_f64(value: &Value) -> rusqlite::Result<Option<f64>> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_f64()
        .map(Some)
        .ok_or_else(|| rusqlite::Error::ToSqlConversionFailure("expected a number or null".into()))
}

fn profile_name(profile: CatalogPackageProfile) -> &'static str {
    match profile {
        CatalogPackageProfile::Compact => "compact",
        CatalogPackageProfile::Full
        | CatalogPackageProfile::LegacyComplete
        | CatalogPackageProfile::LegacyEnriched => "full",
        CatalogPackageProfile::Custom => "custom",
    }
}

fn to_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(error.into())
}

pub fn database_size(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn finalization_updates_use_composite_primary_key_indexes() {
        let directory = tempdir().expect("temporary directory");
        let writer = SqliteCatalogImportWriter::create(&directory.path().join("catalog.sqlite"))
            .expect("catalog writer");

        for sql in [UPDATE_CONTENT_SCAN_ENTRY_COUNT, UPDATE_ROM_FILE_COUNT] {
            let query = format!("EXPLAIN QUERY PLAN {sql}");
            let mut statement = writer.connection.prepare(&query).expect("query plan");
            let details = statement
                .query_map(params!["RJ000001", 0_i64, 1_i64], |row| {
                    row.get::<_, String>(3)
                })
                .expect("query plan rows")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("query plan details");
            assert!(details.iter().any(|detail| detail.contains("SEARCH")));
            assert!(!details.iter().any(|detail| detail.starts_with("SCAN ")));
        }
    }

    #[test]
    fn sqlite_progress_handler_interrupts_long_validation_work() {
        let connection = Connection::open_in_memory().expect("in-memory database");
        let cancellation = CatalogImportCancellationToken::default();
        let callback_cancellation = cancellation.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = Arc::clone(&calls);
        connection
            .progress_handler(
                100,
                Some(move || {
                    if callback_calls.fetch_add(1, Ordering::Relaxed) >= 4 {
                        callback_cancellation.cancel();
                    }
                    callback_cancellation.is_cancelled()
                }),
            )
            .expect("progress handler");

        let result = connection.query_row(
            "WITH RECURSIVE sequence(value) AS (
                 VALUES(1) UNION ALL SELECT value + 1 FROM sequence WHERE value < 100000000
             ) SELECT sum(value) FROM sequence",
            [],
            |row| row.get::<_, i64>(0),
        );

        assert!(result.is_err());
        assert!(cancellation.is_cancelled());
        assert!(calls.load(Ordering::Relaxed) >= 5);
    }
}
