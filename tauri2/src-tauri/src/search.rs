use std::sync::Arc;

use dla_application::search::{
    CatalogSearchService, SearchCacheCleanupReport, SearchError, SearchRebuildCancellationToken,
    SearchRebuildProgress, SearchRebuildProgressSink, SearchRebuildStage,
};
use tauri::AppHandle;
use uuid::Uuid;

use crate::operation::{BackgroundOperation, OperationProgress, ProgressPublisher};

pub const SEARCH_REBUILD_PROGRESS_EVENT: &str = "search-rebuild-progress";

impl OperationProgress for SearchRebuildProgress {
    type Stage = SearchRebuildStage;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn stage(&self) -> SearchRebuildStage {
        self.stage
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.stage,
            SearchRebuildStage::Completed
                | SearchRebuildStage::Cancelled
                | SearchRebuildStage::Failed
        )
    }
}

impl SearchRebuildProgressSink for ProgressPublisher<SearchRebuildProgress> {
    fn publish(&self, progress: &SearchRebuildProgress) -> Result<(), SearchError> {
        self.record(progress);
        Ok(())
    }
}

pub struct SearchIndexController {
    service: Arc<CatalogSearchService>,
    operation: BackgroundOperation<SearchRebuildProgress, (), SearchError>,
}

impl SearchIndexController {
    pub fn new(service: Arc<CatalogSearchService>, app: AppHandle) -> Self {
        Self {
            service,
            operation: BackgroundOperation::new(
                app,
                SEARCH_REBUILD_PROGRESS_EVENT,
                "dla::search",
                "search_rebuild",
            ),
        }
    }

    pub fn start(self: &Arc<Self>) -> Result<SearchRebuildProgress, SearchError> {
        let operation_id = Uuid::new_v4().to_string();
        let cancellation = SearchRebuildCancellationToken::default();
        let initial = SearchRebuildProgress {
            operation_id: operation_id.clone(),
            stage: SearchRebuildStage::Queued,
            indexed_documents: 0,
            total_documents: 0,
            detail: "Search index rebuild queued".to_owned(),
        };
        if !self.operation.begin((), &cancellation, &initial) {
            return Err(SearchError::AlreadyBuilding);
        }
        log::info!(
            target: "dla::search",
            "event=search_rebuild_started operation_id={operation_id}"
        );

        let controller = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            let result = controller.service.rebuild_with_progress(
                &operation_id,
                &cancellation,
                &controller.operation.progress,
            );
            controller
                .operation
                .finish(&operation_id, result.map(|_| ()), |latest, _, error| {
                    SearchRebuildProgress {
                        operation_id: operation_id.clone(),
                        stage: if matches!(error, SearchError::Cancelled) {
                            SearchRebuildStage::Cancelled
                        } else {
                            SearchRebuildStage::Failed
                        },
                        indexed_documents: latest.map_or(0, |progress| progress.indexed_documents),
                        total_documents: latest.map_or(0, |progress| progress.total_documents),
                        detail: error.to_string(),
                    }
                });
        });
        Ok(initial)
    }

    pub fn cancel(&self, operation_id: &str) -> bool {
        self.operation.cancel(operation_id)
    }

    pub fn latest(&self) -> Option<SearchRebuildProgress> {
        self.operation.progress.latest()
    }

    pub fn cleanup(&self) -> Result<SearchCacheCleanupReport, SearchError> {
        if self.operation.slot.is_busy() {
            return Err(SearchError::AlreadyBuilding);
        }
        self.service.cleanup()
    }
}
