#[cfg(desktop)]
use std::path::Path;
use std::sync::Arc;

use dla_application::catalog_import::{
    ActivateCatalogGenerationRequest, CatalogGenerationSummary, CatalogImportCancellationToken,
    CatalogImportCounters, CatalogImportError, CatalogImportOperationKind, CatalogImportPreview,
    CatalogImportProgress, CatalogImportProgressSink, CatalogImportService, CatalogImportStage,
    ExecuteCatalogImportRequest,
};
#[cfg(desktop)]
use dla_catalog_import::ApprovedCatalogPackage;
use dla_catalog_import::CatalogPackageAccessRegistry;
use tauri::AppHandle;
use uuid::Uuid;

use crate::operation::{BackgroundOperation, OperationProgress, ProgressPublisher};

pub const CATALOG_IMPORT_PROGRESS_EVENT: &str = "catalog-import-progress";

impl OperationProgress for CatalogImportProgress {
    type Stage = CatalogImportStage;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn stage(&self) -> CatalogImportStage {
        self.stage
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.stage,
            CatalogImportStage::Completed
                | CatalogImportStage::Cancelled
                | CatalogImportStage::Failed
        )
    }
}

impl CatalogImportProgressSink for ProgressPublisher<CatalogImportProgress> {
    fn publish(&self, progress: &CatalogImportProgress) -> Result<(), CatalogImportError> {
        self.record(progress);
        Ok(())
    }
}

type CatalogWork = dyn FnOnce(
        &CatalogImportService,
        String,
        &CatalogImportCancellationToken,
        &dyn CatalogImportProgressSink,
    ) -> Result<(), CatalogImportError>
    + Send;

pub struct CatalogImportController {
    service: Arc<CatalogImportService>,
    #[cfg(desktop)]
    access: Arc<CatalogPackageAccessRegistry>,
    operation: BackgroundOperation<CatalogImportProgress, String, CatalogImportError>,
}

impl CatalogImportController {
    pub fn new(
        service: Arc<CatalogImportService>,
        _access: Arc<CatalogPackageAccessRegistry>,
        app: AppHandle,
    ) -> Self {
        Self {
            service,
            #[cfg(desktop)]
            access: _access,
            operation: BackgroundOperation::new(
                app,
                CATALOG_IMPORT_PROGRESS_EVENT,
                "dla::catalog",
                "catalog_operation",
            ),
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
        let initial = CatalogImportProgress {
            operation_id: Uuid::new_v4().to_string(),
            operation_kind: CatalogImportOperationKind::Import,
            snapshot_id: preview.manifest.snapshot_id,
            stage: CatalogImportStage::Queued,
            counters: CatalogImportCounters {
                total_bytes: preview.uncompressed_bytes,
                ..CatalogImportCounters::default()
            },
            current_payload: String::new(),
            detail: "Catalog import queued".to_owned(),
        };
        self.run(
            initial,
            Box::new(move |service, operation_id, cancellation, progress| {
                service
                    .execute(
                        ExecuteCatalogImportRequest {
                            operation_id,
                            access_handle,
                        },
                        cancellation,
                        progress,
                    )
                    .map(|_| ())
            }),
        )
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
        let initial = CatalogImportProgress {
            operation_id: Uuid::new_v4().to_string(),
            operation_kind: CatalogImportOperationKind::Activation,
            snapshot_id: generation.snapshot_id,
            stage: CatalogImportStage::Queued,
            counters: CatalogImportCounters {
                unique_works: generation.work_count,
                roms: generation.rom_count,
                ..CatalogImportCounters::default()
            },
            current_payload: String::new(),
            detail: "Catalog activation queued".to_owned(),
        };
        self.run(
            initial,
            Box::new(move |service, operation_id, cancellation, progress| {
                service
                    .activate(
                        ActivateCatalogGenerationRequest {
                            operation_id,
                            generation_id,
                        },
                        cancellation,
                        progress,
                    )
                    .map(|_| ())
            }),
        )
    }

    fn run(
        self: &Arc<Self>,
        initial: CatalogImportProgress,
        work: Box<CatalogWork>,
    ) -> Result<CatalogImportProgress, CatalogImportError> {
        let cancellation = CatalogImportCancellationToken::default();
        if !self
            .operation
            .begin(initial.snapshot_id.clone(), &cancellation, &initial)
        {
            return Err(CatalogImportError::AlreadyRunning);
        }
        let kind = initial.operation_kind;
        let operation_id = initial.operation_id.clone();
        log::info!(
            target: "dla::catalog",
            "event=catalog_{}_started operation_id={operation_id} snapshot_id={}",
            match kind {
                CatalogImportOperationKind::Import => "import",
                CatalogImportOperationKind::Activation => "activation",
            },
            initial.snapshot_id
        );

        let controller = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            let result = work(
                &controller.service,
                operation_id.clone(),
                &cancellation,
                &controller.operation.progress,
            );
            controller
                .operation
                .finish(&operation_id, result, |latest, snapshot_id, error| {
                    CatalogImportProgress {
                        operation_id: operation_id.clone(),
                        operation_kind: kind,
                        snapshot_id: snapshot_id.unwrap_or_default(),
                        stage: if matches!(error, CatalogImportError::Cancelled) {
                            CatalogImportStage::Cancelled
                        } else {
                            CatalogImportStage::Failed
                        },
                        counters: latest
                            .map(|progress| progress.counters.clone())
                            .unwrap_or_default(),
                        current_payload: latest
                            .map(|progress| progress.current_payload.clone())
                            .unwrap_or_default(),
                        detail: error.to_string(),
                    }
                });
        });
        Ok(initial)
    }

    pub fn cancel(&self, operation_id: &str) -> bool {
        self.operation.cancel(operation_id)
    }

    pub fn latest(&self) -> Option<CatalogImportProgress> {
        self.operation.progress.latest()
    }

    pub fn list_generations(&self) -> Result<Vec<CatalogGenerationSummary>, CatalogImportError> {
        self.service.list_generations()
    }

    /// Removing takes the slot too: no generation goes while an import or an
    /// activation runs.
    pub fn remove_generation(&self, generation_id: &str) -> Result<(), CatalogImportError> {
        let operation_id = Uuid::new_v4().to_string();
        if !self.operation.slot.claim(
            &operation_id,
            &CatalogImportCancellationToken::default(),
            String::new(),
        ) {
            return Err(CatalogImportError::AlreadyRunning);
        }
        let result = self.service.remove_generation(generation_id);
        self.operation.slot.release(&operation_id);
        result
    }
}
