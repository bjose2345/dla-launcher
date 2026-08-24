use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use std::collections::HashMap;

use dla_domain::{CatalogWork, Category, NamedReference};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::{CatalogFacetFilters, CatalogSnapshot, normalize_facet_filters};
use crate::identity::{ArchiveHash, CatalogIdentityReader};

const DEFAULT_LIMIT: usize = 30;
const MAXIMUM_LIMIT: usize = 120;
const INDEX_BATCH_SIZE: usize = 256;
const DEFAULT_SHORTCUT_LIMIT: usize = 8;
const MAXIMUM_SHORTCUT_LIMIT: usize = 24;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchRequest {
    pub text: String,
    pub facets: CatalogFacetFilters,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchQuery {
    pub text: String,
    pub facets: CatalogFacetFilters,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub work_code: String,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchIndexPage {
    pub matches: Vec<SearchMatch>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchQueryKind {
    Text,
    ArchiveHash,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultItem {
    pub work: CatalogWork,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub items: Vec<SearchResultItem>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub query_kind: SearchQueryKind,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchShortcutRequest {
    pub text: String,
    pub limit: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchShortcutKind {
    Genre,
    Circle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchShortcut {
    pub kind: SearchShortcutKind,
    pub key: String,
    pub label: String,
    pub label_english: String,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchIndexState {
    Missing,
    Building,
    Ready,
    Stale,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchRebuildStage {
    Queued,
    Indexing,
    Committing,
    Cleaning,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRebuildProgress {
    pub operation_id: String,
    pub stage: SearchRebuildStage,
    pub indexed_documents: usize,
    pub total_documents: usize,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCacheCleanupReport {
    pub removed_incomplete_generations: usize,
    pub removed_complete_generations: usize,
    pub reclaimed_bytes: u64,
    pub retained_complete_generations: usize,
}

#[derive(Clone, Default)]
pub struct SearchRebuildCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl SearchRebuildCancellationToken {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn check(&self) -> Result<(), SearchError> {
        if self.cancelled.load(Ordering::Acquire) {
            Err(SearchError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchIndexStatus {
    pub state: SearchIndexState,
    pub schema_version: u32,
    pub catalog_snapshot_id: String,
    pub indexed_documents: usize,
    pub generation: String,
    pub index_path: String,
    pub detail: String,
}

impl SearchIndexStatus {
    pub fn missing(index_path: impl Into<String>) -> Self {
        Self {
            state: SearchIndexState::Missing,
            schema_version: 0,
            catalog_snapshot_id: String::new(),
            indexed_documents: 0,
            generation: String::new(),
            index_path: index_path.into(),
            detail: "search index has not been built".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogSearchDocument {
    pub work_code: String,
    pub source_code: String,
    pub title: String,
    pub title_english: String,
    pub added_date: String,
    pub release_date: String,
    pub updated_date: String,
    pub age_rating: String,
    pub circles: Vec<NamedReference>,
    pub categories: Vec<Category>,
    pub tags: Vec<NamedReference>,
    pub file_formats: Vec<Category>,
    pub supported_languages: Vec<Category>,
    pub miscellanies: Vec<Category>,
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search query is empty")]
    EmptyQuery,
    #[error("search index is {state:?}: {detail}")]
    Unavailable {
        state: SearchIndexState,
        detail: String,
    },
    #[error("another search index rebuild is already running")]
    AlreadyBuilding,
    #[error("search index rebuild was cancelled")]
    Cancelled,
    #[error("catalog search source failed: {0}")]
    Source(String),
    #[error("catalog search index failed: {0}")]
    Index(String),
}

impl SearchError {
    pub fn source(error: impl std::fmt::Display) -> Self {
        Self::Source(error.to_string())
    }

    pub fn index(error: impl std::fmt::Display) -> Self {
        Self::Index(error.to_string())
    }
}

pub trait CatalogIndexSource: Send + Sync {
    fn snapshot(&self) -> Result<CatalogSnapshot, SearchError>;
    fn read_search_batch(
        &self,
        after_work_code: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CatalogSearchDocument>, SearchError>;
}

pub trait SearchRebuildProgressSink: Send + Sync {
    fn publish(&self, progress: &SearchRebuildProgress) -> Result<(), SearchError>;
}

struct SilentSearchRebuildProgress;

impl SearchRebuildProgressSink for SilentSearchRebuildProgress {
    fn publish(&self, _progress: &SearchRebuildProgress) -> Result<(), SearchError> {
        Ok(())
    }
}

pub trait CatalogSearchIndex: Send + Sync {
    fn status(&self) -> SearchIndexStatus;
    fn rebuild(&self, source: &dyn CatalogIndexSource) -> Result<SearchIndexStatus, SearchError>;
    fn rebuild_with_progress(
        &self,
        source: &dyn CatalogIndexSource,
        operation_id: &str,
        cancellation: &SearchRebuildCancellationToken,
        progress: &dyn SearchRebuildProgressSink,
    ) -> Result<SearchIndexStatus, SearchError> {
        cancellation.check()?;
        let status = self.rebuild(source)?;
        progress.publish(&SearchRebuildProgress {
            operation_id: operation_id.to_owned(),
            stage: SearchRebuildStage::Completed,
            indexed_documents: status.indexed_documents,
            total_documents: status.indexed_documents,
            detail: "Search index is ready".to_owned(),
        })?;
        Ok(status)
    }
    fn cleanup(&self) -> Result<SearchCacheCleanupReport, SearchError> {
        Ok(SearchCacheCleanupReport::default())
    }
    fn search(&self, query: &SearchQuery) -> Result<SearchIndexPage, SearchError>;
}

pub trait CatalogSearchReader: Send + Sync {
    fn search_shortcuts(
        &self,
        text: &str,
        limit: usize,
    ) -> Result<Vec<SearchShortcut>, SearchError>;
}

pub struct CatalogSearchService {
    source: Arc<dyn CatalogIndexSource>,
    identity: Arc<dyn CatalogIdentityReader>,
    reader: Arc<dyn CatalogSearchReader>,
    index: Arc<dyn CatalogSearchIndex>,
}

impl CatalogSearchService {
    pub fn new(
        source: Arc<dyn CatalogIndexSource>,
        identity: Arc<dyn CatalogIdentityReader>,
        reader: Arc<dyn CatalogSearchReader>,
        index: Arc<dyn CatalogSearchIndex>,
    ) -> Self {
        Self {
            source,
            identity,
            reader,
            index,
        }
    }

    pub fn status(&self) -> SearchIndexStatus {
        let mut status = self.index.status();
        if status.state != SearchIndexState::Ready {
            return status;
        }
        match self.source.snapshot() {
            Ok(snapshot) if snapshot.id != status.catalog_snapshot_id => {
                status.state = SearchIndexState::Stale;
                status.detail = format!(
                    "index snapshot {} does not match catalog snapshot {}",
                    status.catalog_snapshot_id, snapshot.id
                );
            }
            Ok(_) => {}
            Err(error) => {
                status.state = SearchIndexState::Failed;
                status.detail = error.to_string();
            }
        }
        status
    }

    pub fn rebuild(&self) -> Result<SearchIndexStatus, SearchError> {
        self.index.rebuild_with_progress(
            self.source.as_ref(),
            "synchronous",
            &SearchRebuildCancellationToken::default(),
            &SilentSearchRebuildProgress,
        )
    }

    pub fn rebuild_with_progress(
        &self,
        operation_id: &str,
        cancellation: &SearchRebuildCancellationToken,
        progress: &dyn SearchRebuildProgressSink,
    ) -> Result<SearchIndexStatus, SearchError> {
        self.index
            .rebuild_with_progress(self.source.as_ref(), operation_id, cancellation, progress)
    }

    pub fn cleanup(&self) -> Result<SearchCacheCleanupReport, SearchError> {
        self.index.cleanup()
    }

    pub fn search(&self, request: SearchRequest) -> Result<SearchResponse, SearchError> {
        let query = normalize(request)?;
        if let Some(hash) = ArchiveHash::parse(&query.text) {
            let works = self
                .identity
                .resolve_archive_hash(&hash)
                .map_err(SearchError::source)?;
            let total = works.len();
            let items = works
                .into_iter()
                .skip(query.offset)
                .take(query.limit)
                .map(|work| SearchResultItem { work, score: 1.0 })
                .collect();
            return Ok(SearchResponse {
                items,
                total,
                limit: query.limit,
                offset: query.offset,
                query_kind: SearchQueryKind::ArchiveHash,
            });
        }
        let status = self.status();
        if status.state != SearchIndexState::Ready {
            return Err(SearchError::Unavailable {
                state: status.state,
                detail: status.detail,
            });
        }
        let ranked = self.index.search(&query)?;
        let work_codes = ranked
            .matches
            .iter()
            .map(|result| result.work_code.clone())
            .collect::<Vec<_>>();
        let works = self
            .identity
            .read_works_by_codes(&work_codes)
            .map_err(SearchError::source)?;
        let mut works_by_code = works
            .into_iter()
            .map(|work| (work.code.to_ascii_lowercase(), work))
            .collect::<HashMap<_, _>>();
        let items = ranked
            .matches
            .into_iter()
            .filter_map(|result| {
                works_by_code
                    .remove(&result.work_code.to_ascii_lowercase())
                    .map(|work| SearchResultItem {
                        work,
                        score: result.score,
                    })
            })
            .collect();
        Ok(SearchResponse {
            items,
            total: ranked.total,
            limit: ranked.limit,
            offset: ranked.offset,
            query_kind: SearchQueryKind::Text,
        })
    }

    pub fn shortcuts(
        &self,
        request: SearchShortcutRequest,
    ) -> Result<Vec<SearchShortcut>, SearchError> {
        let text = request.text.trim();
        if text.is_empty() {
            return Err(SearchError::EmptyQuery);
        }
        let limit = match request.limit {
            0 => DEFAULT_SHORTCUT_LIMIT,
            value => value.min(MAXIMUM_SHORTCUT_LIMIT),
        };
        self.reader.search_shortcuts(text, limit)
    }
}

pub fn index_batch_size() -> usize {
    INDEX_BATCH_SIZE
}

fn normalize(request: SearchRequest) -> Result<SearchQuery, SearchError> {
    let text = request.text.trim().to_owned();
    if text.is_empty() {
        return Err(SearchError::EmptyQuery);
    }
    Ok(SearchQuery {
        text,
        facets: normalize_facet_filters(request.facets),
        limit: match request.limit {
            0 => DEFAULT_LIMIT,
            value => value.min(MAXIMUM_LIMIT),
        },
        offset: request.offset,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct Source {
        snapshot: Mutex<CatalogSnapshot>,
    }

    impl CatalogIndexSource for Source {
        fn snapshot(&self) -> Result<CatalogSnapshot, SearchError> {
            Ok(self.snapshot.lock().expect("snapshot lock").clone())
        }

        fn read_search_batch(
            &self,
            _after_work_code: Option<&str>,
            _limit: usize,
        ) -> Result<Vec<CatalogSearchDocument>, SearchError> {
            Ok(Vec::new())
        }
    }

    struct Index {
        status: SearchIndexStatus,
    }

    struct Identity;

    impl CatalogIdentityReader for Identity {
        fn read_works_by_codes(
            &self,
            _work_codes: &[String],
        ) -> Result<Vec<CatalogWork>, crate::identity::CatalogIdentityError> {
            Ok(Vec::new())
        }

        fn resolve_archive_hash(
            &self,
            _hash: &ArchiveHash,
        ) -> Result<Vec<CatalogWork>, crate::identity::CatalogIdentityError> {
            Ok(Vec::new())
        }

        fn find_archive_candidates_by_size(
            &self,
            _size: &str,
            _limit: usize,
        ) -> Result<
            Vec<crate::identity::CatalogArchiveIdentity>,
            crate::identity::CatalogIdentityError,
        > {
            Ok(Vec::new())
        }
    }

    struct Reader;

    impl CatalogSearchReader for Reader {
        fn search_shortcuts(
            &self,
            _text: &str,
            _limit: usize,
        ) -> Result<Vec<SearchShortcut>, SearchError> {
            Ok(Vec::new())
        }
    }

    impl CatalogSearchIndex for Index {
        fn status(&self) -> SearchIndexStatus {
            self.status.clone()
        }

        fn rebuild(
            &self,
            _source: &dyn CatalogIndexSource,
        ) -> Result<SearchIndexStatus, SearchError> {
            Ok(self.status.clone())
        }

        fn search(&self, query: &SearchQuery) -> Result<SearchIndexPage, SearchError> {
            Ok(SearchIndexPage {
                matches: Vec::new(),
                total: 0,
                limit: query.limit,
                offset: query.offset,
            })
        }
    }

    #[test]
    fn reports_a_ready_index_as_stale_when_the_catalog_changes() {
        let source = Arc::new(Source {
            snapshot: Mutex::new(CatalogSnapshot {
                id: "catalog-v2".to_owned(),
                real_works: 1,
                synthetic_works: 0,
            }),
        });
        let index = Arc::new(Index {
            status: SearchIndexStatus {
                state: SearchIndexState::Ready,
                schema_version: 1,
                catalog_snapshot_id: "catalog-v1".to_owned(),
                indexed_documents: 1,
                generation: "one".to_owned(),
                index_path: "/index".to_owned(),
                detail: "ready".to_owned(),
            },
        });
        let service =
            CatalogSearchService::new(source, Arc::new(Identity), Arc::new(Reader), index);

        let status = service.status();

        assert_eq!(status.state, SearchIndexState::Stale);
        assert!(status.detail.contains("catalog-v2"));
    }

    #[test]
    fn refuses_to_search_when_the_index_is_missing() {
        let source = Arc::new(Source {
            snapshot: Mutex::new(CatalogSnapshot {
                id: "catalog-v1".to_owned(),
                real_works: 1,
                synthetic_works: 0,
            }),
        });
        let index = Arc::new(Index {
            status: SearchIndexStatus::missing("/index"),
        });
        let service =
            CatalogSearchService::new(source, Arc::new(Identity), Arc::new(Reader), index);

        let error = service
            .search(SearchRequest {
                text: "図書館".to_owned(),
                ..SearchRequest::default()
            })
            .expect_err("missing index must fail");

        assert!(matches!(
            error,
            SearchError::Unavailable {
                state: SearchIndexState::Missing,
                ..
            }
        ));
    }

    #[test]
    fn resolves_an_exact_archive_hash_without_a_text_index() {
        let source = Arc::new(Source {
            snapshot: Mutex::new(CatalogSnapshot {
                id: "catalog-v1".to_owned(),
                real_works: 1,
                synthetic_works: 0,
            }),
        });
        let index = Arc::new(Index {
            status: SearchIndexStatus::missing("/index"),
        });
        let service =
            CatalogSearchService::new(source, Arc::new(Identity), Arc::new(Reader), index);

        let response = service
            .search(SearchRequest {
                text: "0123456789abcdef0123456789abcdef".to_owned(),
                ..SearchRequest::default()
            })
            .expect("hash resolution");

        assert_eq!(response.query_kind, SearchQueryKind::ArchiveHash);
    }
}
