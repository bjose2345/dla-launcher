use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[cfg(desktop)]
use std::path::Path;

use dla_application::catalog_import::{
    ActivateCatalogGenerationRequest, CatalogGenerationSummary, CatalogImportCancellationToken,
    CatalogImportCounters, CatalogImportError, CatalogImportOperationKind, CatalogImportPreview,
    CatalogImportProgress, CatalogImportProgressSink, CatalogImportService, CatalogImportStage,
    ExecuteCatalogImportRequest,
};
#[cfg(desktop)]
use dla_catalog_import::ApprovedCatalogPackage;
use dla_catalog_import::CatalogPackageAccessRegistry;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

pub const CATALOG_IMPORT_PROGRESS_EVENT: &str = "catalog-import-progress";

pub struct TauriCatalogImportProgressSink {
    app: AppHandle,
    latest: Arc<Mutex<Option<CatalogImportProgress>>>,
}

impl TauriCatalogImportProgressSink {
    fn new(app: AppHandle, latest: Arc<Mutex<Option<CatalogImportProgress>>>) -> Self {
        Self { app, latest }
    }
}

impl CatalogImportProgressSink for TauriCatalogImportProgressSink {
    fn publish(&self, progress: &CatalogImportProgress) -> Result<(), CatalogImportError> {
        *self
            .latest
            .lock()
            .map_err(|error| CatalogImportError::persistence(error.to_string()))? =
            Some(progress.clone());
        self.app
            .emit(CATALOG_IMPORT_PROGRESS_EVENT, progress)
            .map_err(CatalogImportError::persistence)
    }
}

struct ActiveOperation {
    snapshot_id: String,
    cancellation: CatalogImportCancellationToken,
}

pub struct CatalogImportController {
    service: Arc<CatalogImportService>,
    #[cfg(desktop)]
    access: Arc<CatalogPackageAccessRegistry>,
    progress: Arc<TauriCatalogImportProgressSink>,
    latest: Arc<Mutex<Option<CatalogImportProgress>>>,
    active: Mutex<HashMap<String, ActiveOperation>>,
}

impl CatalogImportController {
    pub fn new(
        service: Arc<CatalogImportService>,
        _access: Arc<CatalogPackageAccessRegistry>,
        app: AppHandle,
    ) -> Self {
        let latest = Arc::new(Mutex::new(None));
        Self {
            service,
            #[cfg(desktop)]
            access: _access,
            progress: Arc::new(TauriCatalogImportProgressSink::new(
                app,
                Arc::clone(&latest),
            )),
            latest,
            active: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(desktop)]
    pub fn approve_package(
        &self,
        path: &Path,
    ) -> Result<ApprovedCatalogPackage, CatalogImportError> {
        self.access.approve(path)
    }

    pub fn inspect(&self, access_handle: &str) -> Result<CatalogImportPreview, CatalogImportError> {
        self.service.inspect(access_handle)
    }

    pub fn start_import(
        self: &Arc<Self>,
        access_handle: String,
    ) -> Result<CatalogImportProgress, CatalogImportError> {
        let preview = self.service.inspect(&access_handle)?;
        let operation_id = Uuid::new_v4().to_string();
        let initial = CatalogImportProgress {
            operation_id: operation_id.clone(),
            operation_kind: CatalogImportOperationKind::Import,
            snapshot_id: preview.manifest.snapshot_id.clone(),
            stage: CatalogImportStage::Queued,
            counters: CatalogImportCounters {
                total_bytes: preview.uncompressed_bytes,
                ..CatalogImportCounters::default()
            },
            current_payload: String::new(),
            detail: "Catalog import queued".to_owned(),
        };
        let cancellation = CatalogImportCancellationToken::default();
        self.claim(
            operation_id.clone(),
            preview.manifest.snapshot_id,
            cancellation.clone(),
        )?;
        log::info!(
            target: "dla::catalog",
            "event=catalog_import_started operation_id={} snapshot_id={}",
            operation_id,
            initial.snapshot_id
        );
        if let Err(error) = self.progress.publish(&initial) {
            self.release(&operation_id);
            return Err(error);
        }

        let controller = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            let result = controller.service.execute(
                ExecuteCatalogImportRequest {
                    operation_id: operation_id.clone(),
                    access_handle,
                },
                &cancellation,
                controller.progress.as_ref(),
            );
            controller.finish(
                &operation_id,
                CatalogImportOperationKind::Import,
                result.map(|_| ()),
            )
        });
        Ok(initial)
    }

    pub fn start_activation(
        self: &Arc<Self>,
        generation_id: String,
    ) -> Result<CatalogImportProgress, CatalogImportError> {
        let generation = self
            .service
            .list_generations()?
            .into_iter()
            .find(|generation| generation.id == generation_id)
            .ok_or_else(|| CatalogImportError::GenerationNotFound(generation_id.clone()))?;
        let operation_id = Uuid::new_v4().to_string();
        let initial = CatalogImportProgress {
            operation_id: operation_id.clone(),
            operation_kind: CatalogImportOperationKind::Activation,
            snapshot_id: generation.snapshot_id.clone(),
            stage: CatalogImportStage::Queued,
            counters: CatalogImportCounters {
                unique_works: generation.work_count,
                roms: generation.rom_count,
                ..CatalogImportCounters::default()
            },
            current_payload: String::new(),
            detail: "Catalog activation queued".to_owned(),
        };
        let cancellation = CatalogImportCancellationToken::default();
        self.claim(
            operation_id.clone(),
            generation.snapshot_id,
            cancellation.clone(),
        )?;
        log::info!(
            target: "dla::catalog",
            "event=catalog_activation_started operation_id={} generation_id={}",
            operation_id,
            generation_id
        );
        if let Err(error) = self.progress.publish(&initial) {
            self.release(&operation_id);
            return Err(error);
        }

        let controller = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            let result = controller.service.activate(
                ActivateCatalogGenerationRequest {
                    operation_id: operation_id.clone(),
                    generation_id,
                },
                &cancellation,
                controller.progress.as_ref(),
            );
            controller.finish(
                &operation_id,
                CatalogImportOperationKind::Activation,
                result.map(|_| ()),
            )
        });
        Ok(initial)
    }

    pub fn cancel(&self, operation_id: &str) -> Result<bool, CatalogImportError> {
        let active = self
            .active
            .lock()
            .map_err(|error| CatalogImportError::persistence(error.to_string()))?;
        let Some(operation) = active.get(operation_id) else {
            return Ok(false);
        };
        operation.cancellation.cancel();
        log::info!(target: "dla::catalog", "event=catalog_operation_cancel_requested operation_id={operation_id}");
        Ok(true)
    }

    pub fn latest(&self) -> Result<Option<CatalogImportProgress>, CatalogImportError> {
        self.latest
            .lock()
            .map(|latest| latest.clone())
            .map_err(|error| CatalogImportError::persistence(error.to_string()))
    }

    pub fn list_generations(&self) -> Result<Vec<CatalogGenerationSummary>, CatalogImportError> {
        self.service.list_generations()
    }

    pub fn remove_generation(&self, generation_id: &str) -> Result<(), CatalogImportError> {
        let operation_id = Uuid::new_v4().to_string();
        self.claim(
            operation_id.clone(),
            String::new(),
            CatalogImportCancellationToken::default(),
        )?;
        let result = self.service.remove_generation(generation_id);
        self.release(&operation_id);
        result
    }

    fn claim(
        &self,
        operation_id: String,
        snapshot_id: String,
        cancellation: CatalogImportCancellationToken,
    ) -> Result<(), CatalogImportError> {
        let mut active = self
            .active
            .lock()
            .map_err(|error| CatalogImportError::persistence(error.to_string()))?;
        if !active.is_empty() {
            return Err(CatalogImportError::AlreadyRunning);
        }
        active.insert(
            operation_id,
            ActiveOperation {
                snapshot_id,
                cancellation,
            },
        );
        Ok(())
    }

    fn finish(
        &self,
        operation_id: &str,
        operation_kind: CatalogImportOperationKind,
        result: Result<(), CatalogImportError>,
    ) {
        match &result {
            Ok(()) => log::info!(
                target: "dla::catalog",
                "event=catalog_operation_completed operation_id={operation_id}"
            ),
            Err(error) => log::warn!(
                target: "dla::catalog",
                "event=catalog_operation_stopped operation_id={operation_id} error={error}"
            ),
        }
        if let Err(error) = result {
            let latest = self.latest().ok().flatten();
            let snapshot_id = self
                .active
                .lock()
                .ok()
                .and_then(|active| {
                    active
                        .get(operation_id)
                        .map(|item| item.snapshot_id.clone())
                })
                .unwrap_or_default();
            let terminal = CatalogImportProgress {
                operation_id: operation_id.to_owned(),
                operation_kind,
                snapshot_id,
                stage: if matches!(error, CatalogImportError::Cancelled) {
                    CatalogImportStage::Cancelled
                } else {
                    CatalogImportStage::Failed
                },
                counters: latest
                    .as_ref()
                    .filter(|progress| progress.operation_id == operation_id)
                    .map(|progress| progress.counters.clone())
                    .unwrap_or_default(),
                current_payload: latest
                    .as_ref()
                    .filter(|progress| progress.operation_id == operation_id)
                    .map(|progress| progress.current_payload.clone())
                    .unwrap_or_default(),
                detail: error.to_string(),
            };
            if let Err(publish_error) = self.progress.publish(&terminal) {
                log::error!("could not publish catalog import failure: {publish_error}");
            }
        }
        self.release(operation_id);
    }

    fn release(&self, operation_id: &str) {
        match self.active.lock() {
            Ok(mut active) => {
                active.remove(operation_id);
            }
            Err(error) => log::error!("could not release catalog import operation: {error}"),
        }
    }
}
