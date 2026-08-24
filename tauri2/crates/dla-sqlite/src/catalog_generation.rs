use dla_application::catalog_import::{
    CatalogGenerationKind, CatalogGenerationState, CatalogGenerationSummary, CatalogImportError,
    CatalogPackageProfile,
};
use rusqlite::{OptionalExtension, params};

use crate::SqliteLibraryStore;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredCatalogGeneration {
    pub summary: CatalogGenerationSummary,
    pub catalog_path: String,
}

impl SqliteLibraryStore {
    pub fn initialize_embedded_catalog(
        &self,
        generation: &StoredCatalogGeneration,
    ) -> Result<(), CatalogImportError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            upsert_generation(&transaction, generation)?;
            transaction.execute(
                "INSERT INTO catalog_active_generation (singleton, generation_id)
                 VALUES (1, ?1)
                 ON CONFLICT(singleton) DO NOTHING",
                params![generation.summary.id],
            )?;
            transaction.commit()
        })
        .map_err(CatalogImportError::persistence)
    }

    pub fn register_catalog_generation(
        &self,
        generation: &StoredCatalogGeneration,
    ) -> Result<(), CatalogImportError> {
        self.with_connection(|connection| upsert_generation(connection, generation))
            .map_err(CatalogImportError::persistence)
    }

    pub fn activate_catalog_generation(
        &self,
        generation_id: &str,
    ) -> Result<(), CatalogImportError> {
        self.with_connection(|connection| {
            let exists = connection
                .query_row(
                    "SELECT 1 FROM catalog_generation WHERE generation_id = ?1",
                    params![generation_id],
                    |row| row.get::<_, bool>(0),
                )
                .optional()?;
            if exists.is_none() {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "catalog generation {generation_id} does not exist"
                )));
            }
            let transaction = connection.transaction()?;
            transaction.execute(
                "UPDATE catalog_generation
                 SET failed = 0, failure_detail = ''
                 WHERE generation_id = ?1",
                params![generation_id],
            )?;
            transaction.execute(
                "UPDATE catalog_active_generation SET generation_id = ?1 WHERE singleton = 1",
                params![generation_id],
            )?;
            transaction.commit()
        })
        .map_err(CatalogImportError::persistence)
    }

    pub fn mark_catalog_generation_failed(
        &self,
        generation_id: &str,
        detail: &str,
    ) -> Result<(), CatalogImportError> {
        self.with_connection(|connection| {
            connection.execute(
                "UPDATE catalog_generation
                 SET failed = 1, failure_detail = ?2
                 WHERE generation_id = ?1",
                params![generation_id, detail],
            )?;
            Ok(())
        })
        .map_err(CatalogImportError::persistence)
    }

    pub fn delete_catalog_generation(
        &self,
        generation_id: &str,
    ) -> Result<bool, CatalogImportError> {
        self.with_connection(|connection| {
            connection.execute(
                "DELETE FROM catalog_generation
                 WHERE generation_id = ?1
                   AND generation_kind = 'imported'
                   AND generation_id NOT IN (
                       SELECT generation_id FROM catalog_active_generation
                   )",
                params![generation_id],
            )
        })
        .map(|deleted| deleted == 1)
        .map_err(CatalogImportError::persistence)
    }

    pub fn read_active_catalog_generation(
        &self,
    ) -> Result<StoredCatalogGeneration, CatalogImportError> {
        self.with_connection(|connection| {
            let mut generation = connection.query_row(
                &format!(
                    "{} JOIN catalog_active_generation active
                     ON active.generation_id = generation.generation_id
                     WHERE active.singleton = 1",
                    generation_select()
                ),
                [],
                scan_generation,
            )?;
            generation.summary.state = CatalogGenerationState::Active;
            Ok(generation)
        })
        .map_err(CatalogImportError::persistence)
    }

    pub fn read_catalog_generation(
        &self,
        generation_id: &str,
    ) -> Result<StoredCatalogGeneration, CatalogImportError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    &format!(
                        "{} WHERE generation.generation_id = ?1",
                        generation_select()
                    ),
                    params![generation_id],
                    scan_generation,
                )
                .optional()
        })
        .map_err(CatalogImportError::persistence)?
        .ok_or_else(|| CatalogImportError::GenerationNotFound(generation_id.to_owned()))
    }

    pub fn list_catalog_generations(
        &self,
    ) -> Result<Vec<StoredCatalogGeneration>, CatalogImportError> {
        self.with_connection(|connection| {
            let active_id = connection.query_row(
                "SELECT generation_id FROM catalog_active_generation WHERE singleton = 1",
                [],
                |row| row.get::<_, String>(0),
            )?;
            let mut statement = connection.prepare(&format!(
                "{} ORDER BY generation.imported_at DESC, generation.generation_id DESC",
                generation_select()
            ))?;
            let rows = statement.query_map([], scan_generation)?;
            let mut generations = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            for generation in &mut generations {
                if generation.summary.id == active_id {
                    generation.summary.state = CatalogGenerationState::Active;
                }
            }
            Ok(generations)
        })
        .map_err(CatalogImportError::persistence)
    }
}

fn upsert_generation(
    connection: &rusqlite::Connection,
    generation: &StoredCatalogGeneration,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO catalog_generation
         (generation_id, snapshot_id, generation_kind, profile, source_name, package_name,
          imported_at, work_count, rom_count, database_bytes, fields_json, catalog_path, failed,
          failure_detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(generation_id) DO UPDATE SET
           snapshot_id = excluded.snapshot_id,
           profile = excluded.profile,
           source_name = excluded.source_name,
           package_name = excluded.package_name,
           imported_at = excluded.imported_at,
           work_count = excluded.work_count,
           rom_count = excluded.rom_count,
           database_bytes = excluded.database_bytes,
           fields_json = excluded.fields_json,
           catalog_path = excluded.catalog_path,
           failed = excluded.failed,
           failure_detail = excluded.failure_detail",
        params![
            generation.summary.id,
            generation.summary.snapshot_id,
            kind_name(generation.summary.kind),
            profile_name(generation.summary.profile),
            generation.summary.source_name,
            generation.summary.package_name,
            generation.summary.imported_at,
            generation.summary.work_count as i64,
            generation.summary.rom_count as i64,
            generation.summary.database_bytes as i64,
            serde_json::to_string(&generation.summary.fields).map_err(to_sql_error)?,
            generation.catalog_path,
            generation.summary.state == CatalogGenerationState::Failed,
            generation.summary.failure_detail,
        ],
    )?;
    Ok(())
}

fn generation_select() -> &'static str {
    "SELECT generation.generation_id, generation.snapshot_id, generation.generation_kind,
            generation.profile, generation.source_name, generation.package_name,
            generation.imported_at, generation.work_count, generation.rom_count,
            generation.database_bytes, generation.fields_json, generation.catalog_path,
            generation.failed, generation.failure_detail
     FROM catalog_generation generation"
}

fn scan_generation(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredCatalogGeneration> {
    let failed = row.get::<_, bool>(12)?;
    Ok(StoredCatalogGeneration {
        summary: CatalogGenerationSummary {
            id: row.get(0)?,
            snapshot_id: row.get(1)?,
            kind: parse_kind(&row.get::<_, String>(2)?)?,
            state: if failed {
                CatalogGenerationState::Failed
            } else {
                CatalogGenerationState::Available
            },
            profile: parse_profile(&row.get::<_, String>(3)?)?,
            source_name: row.get(4)?,
            package_name: row.get(5)?,
            imported_at: row.get(6)?,
            work_count: row.get::<_, i64>(7)? as u64,
            rom_count: row.get::<_, i64>(8)? as u64,
            database_bytes: row.get::<_, i64>(9)? as u64,
            fields: serde_json::from_str(&row.get::<_, String>(10)?).map_err(to_sql_error)?,
            failure_detail: row.get(13)?,
        },
        catalog_path: row.get(11)?,
    })
}

fn kind_name(kind: CatalogGenerationKind) -> &'static str {
    match kind {
        CatalogGenerationKind::Embedded => "embedded",
        CatalogGenerationKind::Imported => "imported",
    }
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

fn parse_kind(value: &str) -> rusqlite::Result<CatalogGenerationKind> {
    match value {
        "embedded" => Ok(CatalogGenerationKind::Embedded),
        "imported" => Ok(CatalogGenerationKind::Imported),
        _ => Err(rusqlite::Error::InvalidParameterName(format!(
            "unknown catalog generation kind {value}"
        ))),
    }
}

fn parse_profile(value: &str) -> rusqlite::Result<CatalogPackageProfile> {
    match value {
        "compact" => Ok(CatalogPackageProfile::Compact),
        "full" | "complete" | "enriched" => Ok(CatalogPackageProfile::Full),
        "custom" => Ok(CatalogPackageProfile::Custom),
        _ => Err(rusqlite::Error::InvalidParameterName(format!(
            "unknown catalog package profile {value}"
        ))),
    }
}

fn to_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_profiles_use_the_current_vocabulary() {
        assert_eq!(profile_name(CatalogPackageProfile::Full), "full");
        assert_eq!(profile_name(CatalogPackageProfile::LegacyComplete), "full");
        assert_eq!(profile_name(CatalogPackageProfile::LegacyEnriched), "full");
        assert_eq!(
            parse_profile("complete").expect("legacy complete profile"),
            CatalogPackageProfile::Full
        );
        assert_eq!(
            parse_profile("enriched").expect("legacy enriched profile"),
            CatalogPackageProfile::Full
        );
    }
}
