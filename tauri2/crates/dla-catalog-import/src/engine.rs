use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use dla_application::{
    catalog_import::{
        ActivateCatalogGenerationRequest, CatalogGenerationKind, CatalogGenerationSummary,
        CatalogImportCancellationToken, CatalogImportCounters, CatalogImportError,
        CatalogImportOperationKind, CatalogImportOutcome, CatalogImportPreview,
        CatalogImportProgress, CatalogImportProgressSink, CatalogImportStage, CatalogImporter,
        ExecuteCatalogImportRequest,
    },
    search::CatalogSearchService,
};
use dla_sqlite::{
    CatalogDatabaseFinalizeProgress, CatalogDatabaseFinalizeStage, ReloadableCatalogStore,
    SqliteCatalogImportWriter, SqliteCatalogStore, SqliteLibraryStore, StoredCatalogGeneration,
    database_size,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{
    CatalogPackageAccessRegistry, DEFAULT_CATALOG_PACKAGE_FILENAME, import_package_payloads,
    inspect_package,
};

const GENERATIONS_DIRECTORY: &str = "catalog/generations";

#[derive(Clone, Copy)]
struct ProgressOperation<'a> {
    id: &'a str,
    kind: CatalogImportOperationKind,
}

pub struct CatalogImportAdapter {
    data_directory: PathBuf,
    access: Arc<CatalogPackageAccessRegistry>,
    catalog: Arc<ReloadableCatalogStore>,
    library: Arc<SqliteLibraryStore>,
    search: Arc<CatalogSearchService>,
}

impl CatalogImportAdapter {
    pub fn new(
        data_directory: PathBuf,
        access: Arc<CatalogPackageAccessRegistry>,
        catalog: Arc<ReloadableCatalogStore>,
        library: Arc<SqliteLibraryStore>,
        search: Arc<CatalogSearchService>,
    ) -> Result<Self, CatalogImportError> {
        let generations = data_directory.join(GENERATIONS_DIRECTORY);
        fs::create_dir_all(&generations).map_err(CatalogImportError::persistence)?;
        clean_abandoned_imports(&generations, library.as_ref())?;
        Ok(Self {
            data_directory,
            access,
            catalog,
            library,
            search,
        })
    }

    pub fn access_registry(&self) -> Arc<CatalogPackageAccessRegistry> {
        Arc::clone(&self.access)
    }

    fn preview(&self, access_handle: &str) -> Result<CatalogImportPreview, CatalogImportError> {
        let path = self.access.resolve(access_handle)?;
        let inspected = inspect_package(&path, &self.data_directory)?;
        let display_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(DEFAULT_CATALOG_PACKAGE_FILENAME)
            .to_owned();
        Ok(CatalogImportPreview {
            access_handle: access_handle.to_owned(),
            display_name,
            compressed_bytes: inspected.compressed_bytes,
            uncompressed_bytes: inspected.uncompressed_bytes,
            required_disk_bytes: inspected.required_disk_bytes,
            available_disk_bytes: inspected.available_disk_bytes,
            compatible: inspected.blocking_issues.is_empty(),
            blocking_issues: inspected.blocking_issues,
            warnings: inspected.warnings,
            manifest: inspected.manifest,
            omitted_fields: inspected.omitted_fields,
        })
    }

    fn import(
        &self,
        request: ExecuteCatalogImportRequest,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError> {
        let preview = self.preview(&request.access_handle)?;
        let package_name = preview.display_name.clone();
        let manifest = &preview.manifest;
        let operation = ProgressOperation {
            id: &request.operation_id,
            kind: CatalogImportOperationKind::Import,
        };
        let mut counters = CatalogImportCounters {
            total_bytes: preview.uncompressed_bytes,
            ..CatalogImportCounters::default()
        };
        publish(
            progress,
            operation,
            &manifest.snapshot_id,
            CatalogImportStage::Validating,
            &counters,
            "manifest.json",
            "Validating package compatibility and available disk space",
        )?;
        if !preview.compatible {
            return Err(CatalogImportError::Incompatible(
                preview.blocking_issues.join("; "),
            ));
        }
        check_cancelled(cancellation)?;

        let generation_id = Uuid::new_v4().to_string();
        let generations = self.data_directory.join(GENERATIONS_DIRECTORY);
        let building_directory = generations.join(format!(".building-{generation_id}"));
        let generation_directory = generations.join(&generation_id);
        fs::create_dir(&building_directory).map_err(CatalogImportError::persistence)?;
        let candidate_path = building_directory.join("catalog.sqlite");

        let build_result = (|| {
            let package_path = self.access.resolve(&request.access_handle)?;
            let mut writer = SqliteCatalogImportWriter::create(&candidate_path)
                .map_err(CatalogImportError::persistence)?;
            let stats = import_package_payloads(
                &package_path,
                manifest,
                &mut writer,
                cancellation,
                |stage, payload, updated| {
                    counters = updated.clone();
                    publish(
                        progress,
                        operation,
                        &manifest.snapshot_id,
                        stage,
                        &counters,
                        payload,
                        stage_detail(stage),
                    )
                },
            )?;
            counters = stats.counters;
            check_cancelled(cancellation)?;
            let imported_at = now_rfc3339();
            let database_counts =
                writer.finish(manifest, &imported_at, cancellation, |finalization| {
                    let (stage, payload, detail) = finalization_status(finalization);
                    publish(
                        progress,
                        operation,
                        &manifest.snapshot_id,
                        stage,
                        &counters,
                        payload,
                        &detail,
                    )
                })?;
            fs::write(
                building_directory.join("manifest.json"),
                serde_json::to_vec_pretty(manifest).map_err(CatalogImportError::invalid)?,
            )
            .map_err(CatalogImportError::persistence)?;
            check_cancelled(cancellation)?;
            fs::rename(&building_directory, &generation_directory)
                .map_err(CatalogImportError::persistence)?;

            let catalog_path = format!("{GENERATIONS_DIRECTORY}/{generation_id}/catalog.sqlite");
            let summary = CatalogGenerationSummary {
                id: generation_id.clone(),
                snapshot_id: manifest.snapshot_id.clone(),
                kind: CatalogGenerationKind::Imported,
                state: dla_application::catalog_import::CatalogGenerationState::Available,
                profile: manifest.profile.canonical(),
                source_name: manifest.source.name.clone(),
                package_name,
                imported_at,
                work_count: database_counts.unique_works,
                rom_count: database_counts.roms,
                database_bytes: database_size(&generation_directory.join("catalog.sqlite")),
                fields: manifest.fields.clone(),
                failure_detail: String::new(),
            };
            self.library
                .register_catalog_generation(&StoredCatalogGeneration {
                    summary: summary.clone(),
                    catalog_path,
                })?;
            Ok(summary)
        })();

        let summary = match build_result {
            Ok(summary) => summary,
            Err(error) => {
                if building_directory.exists() {
                    let _ = fs::remove_dir_all(&building_directory);
                }
                if generation_directory.exists() {
                    let _ = fs::remove_dir_all(&generation_directory);
                }
                return Err(error);
            }
        };

        match self.activate_summary(operation, summary, counters, cancellation, progress) {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                if !matches!(error, CatalogImportError::Cancelled) {
                    let _ = self
                        .library
                        .mark_catalog_generation_failed(&generation_id, &error.to_string());
                }
                Err(error)
            }
        }
    }

    fn activate_generation(
        &self,
        request: ActivateCatalogGenerationRequest,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError> {
        let generation = self
            .library
            .read_catalog_generation(&request.generation_id)?;
        let counters = CatalogImportCounters {
            unique_works: generation.summary.work_count,
            roms: generation.summary.rom_count,
            ..CatalogImportCounters::default()
        };
        let operation = ProgressOperation {
            id: &request.operation_id,
            kind: CatalogImportOperationKind::Activation,
        };
        self.activate_summary(
            operation,
            generation.summary,
            counters,
            cancellation,
            progress,
        )
    }

    fn remove_generation(&self, generation_id: &str) -> Result<(), CatalogImportError> {
        let generation = self.library.read_catalog_generation(generation_id)?;
        self.ensure_generation_is_removable(&generation)?;
        let generation_directory = self.generation_directory(&generation)?;
        let deleting_directory =
            generation_directory.with_file_name(format!(".deleting-{generation_id}"));
        if deleting_directory.exists() {
            return Err(CatalogImportError::persistence(format!(
                "catalog generation {generation_id} already has pending cleanup"
            )));
        }

        let quarantined = match fs::symlink_metadata(&generation_directory) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                fs::rename(&generation_directory, &deleting_directory)
                    .map_err(CatalogImportError::persistence)?;
                true
            }
            Ok(_) => {
                return Err(CatalogImportError::persistence(
                    "catalog generation path is not a directory",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(CatalogImportError::persistence(error)),
        };

        let deletion = self.library.delete_catalog_generation(generation_id);
        match deletion {
            Ok(true) => {}
            Ok(false) => {
                restore_quarantined_generation(
                    quarantined,
                    &deleting_directory,
                    &generation_directory,
                )?;
                let current = self.library.read_catalog_generation(generation_id)?;
                self.ensure_generation_is_removable(&current)?;
                return Err(CatalogImportError::persistence(format!(
                    "catalog generation {generation_id} could not be removed"
                )));
            }
            Err(error) => {
                restore_quarantined_generation(
                    quarantined,
                    &deleting_directory,
                    &generation_directory,
                )?;
                return Err(error);
            }
        }

        if quarantined {
            fs::remove_dir_all(&deleting_directory).map_err(|error| {
                CatalogImportError::persistence(format!(
                    "catalog history was removed, but its files could not be cleaned up: {error}"
                ))
            })?;
        }
        Ok(())
    }

    fn ensure_generation_is_removable(
        &self,
        generation: &StoredCatalogGeneration,
    ) -> Result<(), CatalogImportError> {
        if generation.summary.kind == CatalogGenerationKind::Embedded {
            return Err(CatalogImportError::CannotRemoveEmbeddedGeneration);
        }
        if self.library.read_active_catalog_generation()?.summary.id == generation.summary.id {
            return Err(CatalogImportError::CannotRemoveActiveGeneration);
        }
        Ok(())
    }

    fn generation_directory(
        &self,
        generation: &StoredCatalogGeneration,
    ) -> Result<PathBuf, CatalogImportError> {
        let generation_id = &generation.summary.id;
        let mut id_components = Path::new(generation_id).components();
        if !matches!(id_components.next(), Some(Component::Normal(_)))
            || id_components.next().is_some()
        {
            return Err(CatalogImportError::persistence(
                "catalog generation contains an unsafe identifier",
            ));
        }
        let expected_path = format!("{GENERATIONS_DIRECTORY}/{generation_id}/catalog.sqlite");
        if generation.catalog_path != expected_path {
            return Err(CatalogImportError::persistence(
                "catalog generation contains an unexpected database path",
            ));
        }
        Ok(self
            .data_directory
            .join(GENERATIONS_DIRECTORY)
            .join(generation_id))
    }

    fn activate_summary(
        &self,
        operation: ProgressOperation<'_>,
        summary: CatalogGenerationSummary,
        counters: CatalogImportCounters,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError> {
        check_cancelled(cancellation)?;
        publish(
            progress,
            operation,
            &summary.snapshot_id,
            CatalogImportStage::ActivatingCatalog,
            &counters,
            "catalog.sqlite",
            "Opening the candidate catalog generation",
        )?;
        let stored = self.library.read_catalog_generation(&summary.id)?;
        let path = resolve_catalog_path(&self.data_directory, &stored.catalog_path)?;
        let replacement = Arc::new(
            SqliteCatalogStore::open_existing(&path).map_err(CatalogImportError::persistence)?,
        );
        let previous = self
            .catalog
            .replace(replacement)
            .map_err(CatalogImportError::persistence)?;

        if let Err(error) = publish(
            progress,
            operation,
            &summary.snapshot_id,
            CatalogImportStage::RebuildingSearch,
            &counters,
            "search-index",
            "Rebuilding the Tantivy search index",
        ) {
            self.restore_previous(previous, &error.to_string())?;
            return Err(error);
        }
        let search_status = match self.search.rebuild() {
            Ok(status) => status,
            Err(error) => {
                self.restore_previous(previous, &error.to_string())?;
                return Err(CatalogImportError::Search(error.to_string()));
            }
        };
        if let Err(error) = self.library.activate_catalog_generation(&summary.id) {
            self.restore_previous(previous, &error.to_string())?;
            return Err(error);
        }
        let mut active = self.library.read_catalog_generation(&summary.id)?.summary;
        active.state = dla_application::catalog_import::CatalogGenerationState::Active;
        let _ = publish(
            progress,
            operation,
            &summary.snapshot_id,
            CatalogImportStage::Completed,
            &counters,
            "",
            "Catalog generation activated",
        );
        Ok(CatalogImportOutcome {
            generation: active,
            search_documents: search_status.indexed_documents,
        })
    }

    fn restore_previous(
        &self,
        previous: Arc<SqliteCatalogStore>,
        activation_error: &str,
    ) -> Result<(), CatalogImportError> {
        self.catalog
            .replace(previous)
            .map_err(CatalogImportError::persistence)?;
        self.search.rebuild().map_err(|rollback_error| {
            CatalogImportError::Search(format!(
                "activation failed ({activation_error}); restoring the previous search index also failed ({rollback_error})"
            ))
        })?;
        Ok(())
    }
}

impl CatalogImporter for CatalogImportAdapter {
    fn inspect(&self, access_handle: &str) -> Result<CatalogImportPreview, CatalogImportError> {
        self.preview(access_handle)
    }

    fn execute(
        &self,
        request: ExecuteCatalogImportRequest,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError> {
        self.import(request, cancellation, progress)
    }

    fn list_generations(&self) -> Result<Vec<CatalogGenerationSummary>, CatalogImportError> {
        self.library.list_catalog_generations().map(|generations| {
            generations
                .into_iter()
                .map(|generation| generation.summary)
                .collect()
        })
    }

    fn remove_generation(&self, generation_id: &str) -> Result<(), CatalogImportError> {
        CatalogImportAdapter::remove_generation(self, generation_id)
    }

    fn activate(
        &self,
        request: ActivateCatalogGenerationRequest,
        cancellation: &CatalogImportCancellationToken,
        progress: &dyn CatalogImportProgressSink,
    ) -> Result<CatalogImportOutcome, CatalogImportError> {
        self.activate_generation(request, cancellation, progress)
    }
}

fn publish(
    sink: &dyn CatalogImportProgressSink,
    operation: ProgressOperation<'_>,
    snapshot_id: &str,
    stage: CatalogImportStage,
    counters: &CatalogImportCounters,
    current_payload: &str,
    detail: &str,
) -> Result<(), CatalogImportError> {
    sink.publish(&CatalogImportProgress {
        operation_id: operation.id.to_owned(),
        operation_kind: operation.kind,
        snapshot_id: snapshot_id.to_owned(),
        stage,
        counters: counters.clone(),
        current_payload: current_payload.to_owned(),
        detail: detail.to_owned(),
    })
}

fn stage_detail(stage: CatalogImportStage) -> &'static str {
    match stage {
        CatalogImportStage::BuildingCatalog => "Streaming catalog records",
        CatalogImportStage::ApplyingEnrichment => "Applying Full or Custom catalog details",
        CatalogImportStage::ApplyingRelations => "Applying work relationships",
        _ => "Processing catalog package",
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

fn finalization_status(
    progress: CatalogDatabaseFinalizeProgress,
) -> (CatalogImportStage, &'static str, String) {
    match progress.stage {
        CatalogDatabaseFinalizeStage::UpdatingRomCounts => (
            CatalogImportStage::FinalizingCatalog,
            "rom-file-counts",
            format!(
                "Finalizing archive file counts ({}/{})",
                progress.completed, progress.total
            ),
        ),
        CatalogDatabaseFinalizeStage::ValidatingCounts => (
            CatalogImportStage::FinalizingCatalog,
            "manifest-counts",
            "Comparing imported records with the package manifest".to_owned(),
        ),
        CatalogDatabaseFinalizeStage::WritingMetadata => (
            CatalogImportStage::FinalizingCatalog,
            "catalog-metadata",
            "Writing catalog generation metadata".to_owned(),
        ),
        CatalogDatabaseFinalizeStage::Committing => (
            CatalogImportStage::FinalizingCatalog,
            "catalog.sqlite",
            "Committing the candidate catalog transaction".to_owned(),
        ),
        CatalogDatabaseFinalizeStage::Checkpointing => (
            CatalogImportStage::CheckpointingCatalog,
            "catalog.sqlite-wal",
            "Checkpointing SQLite and reclaiming the import journal".to_owned(),
        ),
        CatalogDatabaseFinalizeStage::CheckingIntegrity => (
            CatalogImportStage::ValidatingCatalog,
            "integrity-check",
            "Checking SQLite page and index integrity".to_owned(),
        ),
        CatalogDatabaseFinalizeStage::CheckingForeignKeys => (
            CatalogImportStage::ValidatingCatalog,
            "foreign-key-check",
            "Checking catalog relationships and foreign keys".to_owned(),
        ),
    }
}

fn clean_abandoned_imports(
    generations: &Path,
    library: &SqliteLibraryStore,
) -> Result<(), CatalogImportError> {
    for entry in fs::read_dir(generations).map_err(CatalogImportError::persistence)? {
        let entry = entry.map_err(CatalogImportError::persistence)?;
        let file_type = entry.file_type().map_err(CatalogImportError::persistence)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".building-") && file_type.is_dir() {
            fs::remove_dir_all(entry.path()).map_err(CatalogImportError::persistence)?;
            continue;
        }
        let Some(generation_id) = name.strip_prefix(".deleting-") else {
            continue;
        };
        if generation_id.is_empty() || !file_type.is_dir() {
            continue;
        }
        let generation_directory = generations.join(generation_id);
        match library.read_catalog_generation(generation_id) {
            Ok(_) if generation_directory.exists() => {
                fs::remove_dir_all(entry.path()).map_err(CatalogImportError::persistence)?;
            }
            Ok(_) => {
                fs::rename(entry.path(), generation_directory)
                    .map_err(CatalogImportError::persistence)?;
            }
            Err(CatalogImportError::GenerationNotFound(_)) => {
                fs::remove_dir_all(entry.path()).map_err(CatalogImportError::persistence)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn restore_quarantined_generation(
    quarantined: bool,
    deleting_directory: &Path,
    generation_directory: &Path,
) -> Result<(), CatalogImportError> {
    if quarantined {
        fs::rename(deleting_directory, generation_directory)
            .map_err(CatalogImportError::persistence)?;
    }
    Ok(())
}

pub fn resolve_catalog_path(
    data_directory: &Path,
    relative_path: &str,
) -> Result<PathBuf, CatalogImportError> {
    let relative = Path::new(relative_path);
    if relative_path.is_empty()
        || relative.is_absolute()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(CatalogImportError::persistence(
            "catalog generation contains an unsafe database path",
        ));
    }
    Ok(data_directory.join(relative))
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("RFC 3339 timestamp")
}
