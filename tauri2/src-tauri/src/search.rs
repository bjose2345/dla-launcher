use std::sync::{Arc, Mutex};

use dla_application::search::{
    CatalogSearchService, SearchCacheCleanupReport, SearchError, SearchRebuildCancellationToken,
    SearchRebuildProgress, SearchRebuildProgressSink, SearchRebuildStage,
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

pub const SEARCH_REBUILD_PROGRESS_EVENT: &str = "search-rebuild-progress";

struct TauriSearchRebuildProgressSink {
    app: AppHandle,
    latest: Arc<Mutex<Option<SearchRebuildProgress>>>,
}

impl TauriSearchRebuildProgressSink {
    fn new(app: AppHandle, latest: Arc<Mutex<Option<SearchRebuildProgress>>>) -> Self {
        Self { app, latest }
    }
}

impl SearchRebuildProgressSink for TauriSearchRebuildProgressSink {
    fn publish(&self, progress: &SearchRebuildProgress) -> Result<(), SearchError> {
        *self
            .latest
            .lock()
            .map_err(|error| SearchError::index(error.to_string()))? = Some(progress.clone());
        if let Err(error) = self.app.emit(SEARCH_REBUILD_PROGRESS_EVENT, progress) {
            log::warn!(
                target: "dla::search",
                "event=search_rebuild_progress_emit_failed error={error}"
            );
        }
        Ok(())
    }
}

struct ActiveSearchRebuild {
    operation_id: String,
    cancellation: SearchRebuildCancellationToken,
}

pub struct SearchIndexController {
    service: Arc<CatalogSearchService>,
    progress: Arc<TauriSearchRebuildProgressSink>,
    latest: Arc<Mutex<Option<SearchRebuildProgress>>>,
    active: Mutex<Option<ActiveSearchRebuild>>,
}

impl SearchIndexController {
    pub fn new(service: Arc<CatalogSearchService>, app: AppHandle) -> Self {
        let latest = Arc::new(Mutex::new(None));
        Self {
            service,
            progress: Arc::new(TauriSearchRebuildProgressSink::new(
                app,
                Arc::clone(&latest),
            )),
            latest,
            active: Mutex::new(None),
        }
    }

    pub fn start(self: &Arc<Self>) -> Result<SearchRebuildProgress, SearchError> {
        let operation_id = Uuid::new_v4().to_string();
        let cancellation = SearchRebuildCancellationToken::default();
        {
            let mut active = self
                .active
                .lock()
                .map_err(|error| SearchError::index(error.to_string()))?;
            if active.is_some() {
                return Err(SearchError::AlreadyBuilding);
            }
            *active = Some(ActiveSearchRebuild {
                operation_id: operation_id.clone(),
                cancellation: cancellation.clone(),
            });
        }
        let initial = SearchRebuildProgress {
            operation_id: operation_id.clone(),
            stage: SearchRebuildStage::Queued,
            indexed_documents: 0,
            total_documents: 0,
            detail: "Search index rebuild queued".to_owned(),
        };
        if let Err(error) = self.progress.publish(&initial) {
            self.release(&operation_id);
            return Err(error);
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
                controller.progress.as_ref(),
            );
            controller.finish(&operation_id, result.map(|_| ()))
        });
        Ok(initial)
    }

    pub fn cancel(&self, operation_id: &str) -> Result<bool, SearchError> {
        let active = self
            .active
            .lock()
            .map_err(|error| SearchError::index(error.to_string()))?;
        let Some(operation) = active.as_ref() else {
            return Ok(false);
        };
        if operation.operation_id != operation_id {
            return Ok(false);
        }
        operation.cancellation.cancel();
        log::info!(
            target: "dla::search",
            "event=search_rebuild_cancel_requested operation_id={operation_id}"
        );
        Ok(true)
    }

    pub fn latest(&self) -> Result<Option<SearchRebuildProgress>, SearchError> {
        self.latest
            .lock()
            .map(|latest| latest.clone())
            .map_err(|error| SearchError::index(error.to_string()))
    }

    pub fn cleanup(&self) -> Result<SearchCacheCleanupReport, SearchError> {
        if self
            .active
            .lock()
            .map_err(|error| SearchError::index(error.to_string()))?
            .is_some()
        {
            return Err(SearchError::AlreadyBuilding);
        }
        self.service.cleanup()
    }

    fn finish(&self, operation_id: &str, result: Result<(), SearchError>) {
        match result {
            Ok(()) => log::info!(
                target: "dla::search",
                "event=search_rebuild_completed operation_id={operation_id}"
            ),
            Err(error) => {
                log::warn!(
                    target: "dla::search",
                    "event=search_rebuild_stopped operation_id={operation_id} error={error}"
                );
                let latest = self.latest().ok().flatten();
                let terminal = SearchRebuildProgress {
                    operation_id: operation_id.to_owned(),
                    stage: if matches!(error, SearchError::Cancelled) {
                        SearchRebuildStage::Cancelled
                    } else {
                        SearchRebuildStage::Failed
                    },
                    indexed_documents: latest
                        .as_ref()
                        .filter(|progress| progress.operation_id == operation_id)
                        .map_or(0, |progress| progress.indexed_documents),
                    total_documents: latest
                        .as_ref()
                        .filter(|progress| progress.operation_id == operation_id)
                        .map_or(0, |progress| progress.total_documents),
                    detail: error.to_string(),
                };
                if let Err(publish_error) = self.progress.publish(&terminal) {
                    log::error!(
                        target: "dla::search",
                        "event=search_rebuild_terminal_publish_failed operation_id={operation_id} error={publish_error}"
                    );
                }
            }
        }
        self.release(operation_id);
    }

    fn release(&self, operation_id: &str) {
        match self.active.lock() {
            Ok(mut active)
                if active
                    .as_ref()
                    .is_some_and(|operation| operation.operation_id == operation_id) =>
            {
                *active = None;
            }
            Ok(_) => {}
            Err(error) => log::error!(
                target: "dla::search",
                "event=search_rebuild_release_failed operation_id={operation_id} error={error}"
            ),
        }
    }
}
