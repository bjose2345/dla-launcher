use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, RwLock, TryLockError},
    time::{SystemTime, UNIX_EPOCH},
};

use dla_application::{
    catalog::{CatalogFacetFilters, CatalogFacetSelection, CatalogSnapshot},
    search::{
        CatalogIndexSource, CatalogSearchDocument, CatalogSearchIndex, SearchCacheCleanupReport,
        SearchError, SearchIndexPage, SearchIndexState, SearchIndexStatus, SearchMatch,
        SearchQuery, SearchRebuildCancellationToken, SearchRebuildProgress,
        SearchRebuildProgressSink, SearchRebuildStage, index_batch_size,
    },
};
use lindera::dictionary::{DictionaryKind, load_embedded_dictionary};
use lindera::{mode::Mode, segmenter::Segmenter};
use lindera_tantivy::tokenizer::LinderaTokenizer;
use serde::{Deserialize, Serialize};
use tantivy::{
    Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term,
    collector::{Count, TopDocs},
    query::{BooleanQuery, BoostQuery, Occur, Query, QueryParser, RegexQuery, TermQuery},
    schema::{
        Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions, Value,
    },
    tokenizer::{LowerCaser, TextAnalyzer},
};

const SEARCH_SCHEMA_VERSION: u32 = 1;
const JAPANESE_TOKENIZER: &str = "dla_ja_ipadic";
const INDEX_WRITER_MEMORY_BUDGET: usize = 20_000_000;
const RETAIN_COMPLETE_GENERATIONS: usize = 2;

pub struct TantivyCatalogSearch {
    root: PathBuf,
    active: RwLock<Option<ActiveIndex>>,
    status: Mutex<SearchIndexStatus>,
    rebuild_lock: Mutex<()>,
}

impl TantivyCatalogSearch {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, SearchError> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(SearchError::index)?;
        let _ = cleanup_managed_generations(&root, None, false);
        let mut load_errors = Vec::new();
        let active = load_latest_generation(&root, &mut load_errors);
        let active_path = active.as_ref().map(|active| active.path.as_path());
        let _ = cleanup_managed_generations(&root, active_path, true);
        let status = match &active {
            Some(active) => active.status(),
            None if load_errors.is_empty() => {
                SearchIndexStatus::missing(root.display().to_string())
            }
            None => SearchIndexStatus {
                state: SearchIndexState::Failed,
                schema_version: SEARCH_SCHEMA_VERSION,
                catalog_snapshot_id: String::new(),
                indexed_documents: 0,
                generation: String::new(),
                index_path: root.display().to_string(),
                detail: load_errors.join("; "),
            },
        };
        Ok(Self {
            root,
            active: RwLock::new(active),
            status: Mutex::new(status),
            rebuild_lock: Mutex::new(()),
        })
    }

    fn build_generation(
        &self,
        source: &dyn CatalogIndexSource,
        snapshot: &CatalogSnapshot,
        generation: &str,
        operation_id: &str,
        cancellation: &SearchRebuildCancellationToken,
        progress: &dyn SearchRebuildProgressSink,
    ) -> Result<ActiveIndex, SearchError> {
        let building_path = self.root.join(format!(".building-{generation}"));
        let generation_path = self.root.join(generation);
        fs::create_dir(&building_path).map_err(SearchError::index)?;
        let expected_documents = snapshot.real_works + snapshot.synthetic_works;
        let result = (|| {
            let (schema, fields) = build_schema();
            let index = Index::create_in_dir(&building_path, schema).map_err(SearchError::index)?;
            register_japanese_tokenizer(&index)?;
            let mut writer = index
                .writer(INDEX_WRITER_MEMORY_BUDGET)
                .map_err(SearchError::index)?;
            let indexed_documents = index_documents(
                source,
                &mut writer,
                fields,
                operation_id,
                expected_documents,
                cancellation,
                progress,
            )?;
            if indexed_documents != expected_documents {
                return Err(SearchError::index(format!(
                    "catalog contains {expected_documents} works but {indexed_documents} were indexed"
                )));
            }
            cancellation.check()?;
            progress.publish(&SearchRebuildProgress {
                operation_id: operation_id.to_owned(),
                stage: SearchRebuildStage::Committing,
                indexed_documents,
                total_documents: expected_documents,
                detail: "Saving the new search index".to_owned(),
            })?;
            writer.commit().map_err(SearchError::index)?;
            writer.wait_merging_threads().map_err(SearchError::index)?;
            cancellation.check()?;

            let metadata = GenerationMetadata {
                schema_version: SEARCH_SCHEMA_VERSION,
                catalog_snapshot_id: snapshot.id.clone(),
                indexed_documents,
                generation: generation.to_owned(),
            };
            let metadata_json = serde_json::to_vec_pretty(&metadata).map_err(SearchError::index)?;
            fs::write(building_path.join("metadata.json"), metadata_json)
                .map_err(SearchError::index)?;
            fs::rename(&building_path, &generation_path).map_err(SearchError::index)?;
            load_generation(&generation_path)
        })();
        if result.is_err() && building_path.exists() {
            let _ = fs::remove_dir_all(&building_path);
        }
        result
    }

    fn set_status(&self, status: SearchIndexStatus) {
        *self.status.lock().expect("search status lock") = status;
    }

    fn run_rebuild(
        &self,
        source: &dyn CatalogIndexSource,
        operation_id: &str,
        cancellation: &SearchRebuildCancellationToken,
        progress: &dyn SearchRebuildProgressSink,
    ) -> Result<SearchIndexStatus, SearchError> {
        let _rebuild = match self.rebuild_lock.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(SearchError::AlreadyBuilding),
            Err(TryLockError::Poisoned(error)) => {
                return Err(SearchError::index(error.to_string()));
            }
        };
        cancellation.check()?;
        let snapshot = source.snapshot()?;
        let generation = generation_name(&snapshot.id);
        let previous_status = self.status();
        let expected_documents = snapshot.real_works + snapshot.synthetic_works;
        self.set_status(SearchIndexStatus {
            state: SearchIndexState::Building,
            schema_version: SEARCH_SCHEMA_VERSION,
            catalog_snapshot_id: snapshot.id.clone(),
            indexed_documents: 0,
            generation: generation.clone(),
            index_path: self.root.display().to_string(),
            detail: "building search index".to_owned(),
        });
        progress.publish(&SearchRebuildProgress {
            operation_id: operation_id.to_owned(),
            stage: SearchRebuildStage::Indexing,
            indexed_documents: 0,
            total_documents: expected_documents,
            detail: "Indexing catalog works".to_owned(),
        })?;

        match self.build_generation(
            source,
            &snapshot,
            &generation,
            operation_id,
            cancellation,
            progress,
        ) {
            Ok(active) => {
                let status = active.status();
                *self.active.write().expect("active search index lock") = Some(active);
                self.set_status(status.clone());
                progress.publish(&SearchRebuildProgress {
                    operation_id: operation_id.to_owned(),
                    stage: SearchRebuildStage::Cleaning,
                    indexed_documents: status.indexed_documents,
                    total_documents: expected_documents,
                    detail: "Removing old search cache generations".to_owned(),
                })?;
                let active_path = Path::new(&status.index_path);
                let _ = cleanup_managed_generations(&self.root, Some(active_path), true);
                progress.publish(&SearchRebuildProgress {
                    operation_id: operation_id.to_owned(),
                    stage: SearchRebuildStage::Completed,
                    indexed_documents: status.indexed_documents,
                    total_documents: expected_documents,
                    detail: "Search index is ready".to_owned(),
                })?;
                Ok(status)
            }
            Err(error) => {
                if matches!(error, SearchError::Cancelled) {
                    self.set_status(previous_status);
                } else {
                    self.set_status(SearchIndexStatus {
                        state: SearchIndexState::Failed,
                        schema_version: SEARCH_SCHEMA_VERSION,
                        catalog_snapshot_id: snapshot.id,
                        indexed_documents: 0,
                        generation,
                        index_path: self.root.display().to_string(),
                        detail: error.to_string(),
                    });
                }
                Err(error)
            }
        }
    }
}

struct SilentProgress;

impl SearchRebuildProgressSink for SilentProgress {
    fn publish(&self, _progress: &SearchRebuildProgress) -> Result<(), SearchError> {
        Ok(())
    }
}

impl CatalogSearchIndex for TantivyCatalogSearch {
    fn status(&self) -> SearchIndexStatus {
        self.status.lock().expect("search status lock").clone()
    }

    fn rebuild(&self, source: &dyn CatalogIndexSource) -> Result<SearchIndexStatus, SearchError> {
        self.run_rebuild(
            source,
            "synchronous",
            &SearchRebuildCancellationToken::default(),
            &SilentProgress,
        )
    }

    fn rebuild_with_progress(
        &self,
        source: &dyn CatalogIndexSource,
        operation_id: &str,
        cancellation: &SearchRebuildCancellationToken,
        progress: &dyn SearchRebuildProgressSink,
    ) -> Result<SearchIndexStatus, SearchError> {
        self.run_rebuild(source, operation_id, cancellation, progress)
    }

    fn cleanup(&self) -> Result<SearchCacheCleanupReport, SearchError> {
        let _rebuild = match self.rebuild_lock.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::WouldBlock) => return Err(SearchError::AlreadyBuilding),
            Err(TryLockError::Poisoned(error)) => {
                return Err(SearchError::index(error.to_string()));
            }
        };
        let active = self.active.read().expect("active search index lock");
        cleanup_managed_generations(
            &self.root,
            active.as_ref().map(|active| active.path.as_path()),
            true,
        )
    }

    fn search(&self, query: &SearchQuery) -> Result<SearchIndexPage, SearchError> {
        let active = self.active.read().expect("active search index lock");
        let active = active.as_ref().ok_or_else(|| SearchError::Unavailable {
            state: self.status().state,
            detail: self.status().detail,
        })?;
        active.search(query)
    }
}

struct ActiveIndex {
    path: PathBuf,
    index: Index,
    reader: IndexReader,
    fields: SearchFields,
    metadata: GenerationMetadata,
}

impl ActiveIndex {
    fn status(&self) -> SearchIndexStatus {
        SearchIndexStatus {
            state: SearchIndexState::Ready,
            schema_version: self.metadata.schema_version,
            catalog_snapshot_id: self.metadata.catalog_snapshot_id.clone(),
            indexed_documents: self.metadata.indexed_documents,
            generation: self.metadata.generation.clone(),
            index_path: self.path.display().to_string(),
            detail: "search index is ready".to_owned(),
        }
    }

    fn search(&self, query: &SearchQuery) -> Result<SearchIndexPage, SearchError> {
        let searcher = self.reader.searcher();
        let compiled = build_query(&self.index, self.fields, query);
        let total = searcher
            .search(compiled.as_ref(), &Count)
            .map_err(SearchError::index)?;
        let top_docs = searcher
            .search(
                compiled.as_ref(),
                &TopDocs::with_limit(query.limit)
                    .and_offset(query.offset)
                    .order_by_score(),
            )
            .map_err(SearchError::index)?;
        let mut matches = Vec::with_capacity(top_docs.len());
        for (score, address) in top_docs {
            let document: TantivyDocument = searcher.doc(address).map_err(SearchError::index)?;
            let work_code = document
                .get_first(self.fields.work_code)
                .and_then(|value| value.as_str())
                .ok_or_else(|| SearchError::index("search document has no work code"))?;
            matches.push(SearchMatch {
                work_code: work_code.to_owned(),
                score,
            });
        }
        Ok(SearchIndexPage {
            matches,
            total,
            limit: query.limit,
            offset: query.offset,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationMetadata {
    schema_version: u32,
    catalog_snapshot_id: String,
    indexed_documents: usize,
    generation: String,
}

#[derive(Clone, Copy)]
struct SearchFields {
    work_code: Field,
    source_code: Field,
    title: Field,
    title_english: Field,
    circle: Field,
    circle_english: Field,
    circle_filter: Field,
    tag: Field,
    tag_english: Field,
    tag_filter: Field,
    age: Field,
    language: Field,
    category: Field,
    file_type: Field,
    miscellany: Field,
    added_date: Field,
    release_date: Field,
    updated_date: Field,
}

fn build_schema() -> (Schema, SearchFields) {
    let mut builder = Schema::builder();
    let japanese = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(JAPANESE_TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let work_code = builder.add_text_field("work_code", STRING | STORED);
    let source_code = builder.add_text_field("source_code", STRING);
    let title = builder.add_text_field("title", japanese.clone());
    let title_english = builder.add_text_field("title_english", japanese.clone());
    let circle = builder.add_text_field("circle", japanese.clone());
    let circle_english = builder.add_text_field("circle_english", japanese.clone());
    let circle_filter = builder.add_text_field("circle_filter", STRING);
    let tag = builder.add_text_field("tag", japanese.clone());
    let tag_english = builder.add_text_field("tag_english", japanese);
    let tag_filter = builder.add_text_field("tag_filter", STRING);
    let age = builder.add_text_field("age", STRING);
    let language = builder.add_text_field("language", STRING);
    let category = builder.add_text_field("category", STRING);
    let file_type = builder.add_text_field("file_type", STRING);
    let miscellany = builder.add_text_field("miscellany", STRING);
    let added_date = builder.add_text_field("added_date", STRING);
    let release_date = builder.add_text_field("release_date", STRING);
    let updated_date = builder.add_text_field("updated_date", STRING);
    let fields = SearchFields {
        work_code,
        source_code,
        title,
        title_english,
        circle,
        circle_english,
        circle_filter,
        tag,
        tag_english,
        tag_filter,
        age,
        language,
        category,
        file_type,
        miscellany,
        added_date,
        release_date,
        updated_date,
    };
    (builder.build(), fields)
}

fn fields_from_schema(schema: &Schema) -> Result<SearchFields, SearchError> {
    let field = |name| schema.get_field(name).map_err(SearchError::index);
    Ok(SearchFields {
        work_code: field("work_code")?,
        source_code: field("source_code")?,
        title: field("title")?,
        title_english: field("title_english")?,
        circle: field("circle")?,
        circle_english: field("circle_english")?,
        circle_filter: field("circle_filter")?,
        tag: field("tag")?,
        tag_english: field("tag_english")?,
        tag_filter: field("tag_filter")?,
        age: field("age")?,
        language: field("language")?,
        category: field("category")?,
        file_type: field("file_type")?,
        miscellany: field("miscellany")?,
        added_date: field("added_date")?,
        release_date: field("release_date")?,
        updated_date: field("updated_date")?,
    })
}

fn register_japanese_tokenizer(index: &Index) -> Result<(), SearchError> {
    let dictionary =
        load_embedded_dictionary(DictionaryKind::IPADIC).map_err(SearchError::index)?;
    let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
    let analyzer = TextAnalyzer::builder(LinderaTokenizer::from_segmenter(segmenter))
        .filter(LowerCaser)
        .build();
    index.tokenizers().register(JAPANESE_TOKENIZER, analyzer);
    Ok(())
}

fn index_documents(
    source: &dyn CatalogIndexSource,
    writer: &mut IndexWriter,
    fields: SearchFields,
    operation_id: &str,
    total_documents: usize,
    cancellation: &SearchRebuildCancellationToken,
    progress: &dyn SearchRebuildProgressSink,
) -> Result<usize, SearchError> {
    let mut after_work_code = None;
    let mut indexed = 0;
    loop {
        cancellation.check()?;
        let documents = source.read_search_batch(after_work_code.as_deref(), index_batch_size())?;
        if documents.is_empty() {
            break;
        }
        let next_work_code = documents
            .last()
            .map(|document| document.work_code.clone())
            .ok_or_else(|| SearchError::source("catalog search batch has no cursor"))?;
        if after_work_code.as_deref() == Some(next_work_code.as_str()) {
            return Err(SearchError::source("catalog search cursor did not advance"));
        }
        for document in &documents {
            cancellation.check()?;
            writer
                .add_document(map_document(fields, document))
                .map_err(SearchError::index)?;
        }
        indexed += documents.len();
        after_work_code = Some(next_work_code);
        progress.publish(&SearchRebuildProgress {
            operation_id: operation_id.to_owned(),
            stage: SearchRebuildStage::Indexing,
            indexed_documents: indexed,
            total_documents,
            detail: "Indexing catalog works".to_owned(),
        })?;
        if documents.len() < index_batch_size() {
            break;
        }
    }
    Ok(indexed)
}

fn map_document(fields: SearchFields, source: &CatalogSearchDocument) -> TantivyDocument {
    let mut document = TantivyDocument::new();
    document.add_text(fields.work_code, &source.work_code);
    document.add_text(fields.source_code, &source.source_code);
    document.add_text(fields.title, &source.title);
    document.add_text(fields.title_english, &source.title_english);
    document.add_text(fields.age, normalize_filter(&source.age_rating));
    document.add_text(fields.added_date, &source.added_date);
    document.add_text(fields.release_date, &source.release_date);
    document.add_text(fields.updated_date, &source.updated_date);
    for circle in &source.circles {
        document.add_text(fields.circle, &circle.name);
        document.add_text(fields.circle_english, &circle.name_english);
        document.add_text(fields.circle_filter, normalize_filter(&circle.name));
    }
    for tag in &source.tags {
        document.add_text(fields.tag, &tag.name);
        document.add_text(fields.tag_english, &tag.name_english);
        document.add_text(fields.tag_filter, normalize_filter(&tag.name));
    }
    for category in &source.categories {
        document.add_text(fields.category, normalize_filter(&category.code));
    }
    for language in &source.supported_languages {
        document.add_text(fields.language, normalize_filter(&language.code));
    }
    for file_type in &source.file_formats {
        document.add_text(fields.file_type, normalize_filter(&file_type.code));
    }
    for miscellany in &source.miscellanies {
        document.add_text(fields.miscellany, normalize_filter(&miscellany.code));
    }
    document
}

fn build_query(index: &Index, fields: SearchFields, query: &SearchQuery) -> Box<dyn Query> {
    let text_fields = vec![
        fields.work_code,
        fields.source_code,
        fields.title,
        fields.title_english,
        fields.circle,
        fields.circle_english,
        fields.tag,
        fields.tag_english,
    ];
    let mut exact_parser = QueryParser::for_index(index, text_fields.clone());
    configure_boosts(&mut exact_parser, fields);
    let (exact, _) = exact_parser.parse_query_lenient(&query.text);

    let mut fuzzy_parser = QueryParser::for_index(index, text_fields);
    configure_boosts(&mut fuzzy_parser, fields);
    for field in [
        fields.title,
        fields.title_english,
        fields.circle,
        fields.circle_english,
        fields.tag,
        fields.tag_english,
    ] {
        fuzzy_parser.set_field_fuzzy(field, false, fuzzy_distance(&query.text), true);
    }
    let (fuzzy, _) = fuzzy_parser.parse_query_lenient(&query.text);

    let mut relevance = vec![
        (
            Occur::Should,
            Box::new(BoostQuery::new(exact, 3.0)) as Box<dyn Query>,
        ),
        (Occur::Should, fuzzy),
    ];
    if query.text.split_whitespace().count() > 1 {
        let phrase = format!("\"{}\"", query.text.replace('"', "\\\""));
        let (phrase_query, _) = exact_parser.parse_query_lenient(&phrase);
        relevance.push((Occur::Should, Box::new(BoostQuery::new(phrase_query, 2.0))));
    }
    if let Some(code) = work_code_prefix(&query.text) {
        let pattern = format!("{code}.*");
        for field in [fields.work_code, fields.source_code] {
            if let Ok(prefix) = RegexQuery::from_pattern(&pattern, field) {
                relevance.push((
                    Occur::Should,
                    Box::new(BoostQuery::new(Box::new(prefix), 8.0)),
                ));
            }
        }
    }
    let mut clauses = vec![(
        Occur::Must,
        Box::new(BooleanQuery::new(relevance)) as Box<dyn Query>,
    )];
    add_filters(&mut clauses, fields, &query.facets);
    Box::new(BooleanQuery::new(clauses))
}

fn configure_boosts(parser: &mut QueryParser, fields: SearchFields) {
    parser.set_field_boost(fields.work_code, 12.0);
    parser.set_field_boost(fields.source_code, 10.0);
    parser.set_field_boost(fields.title, 5.0);
    parser.set_field_boost(fields.title_english, 5.0);
    parser.set_field_boost(fields.circle, 3.0);
    parser.set_field_boost(fields.circle_english, 3.0);
    parser.set_field_boost(fields.tag, 1.5);
    parser.set_field_boost(fields.tag_english, 1.5);
}

fn add_filters(
    clauses: &mut Vec<(Occur, Box<dyn Query>)>,
    fields: SearchFields,
    filters: &CatalogFacetFilters,
) {
    for (field, selection) in [
        (fields.age, &filters.ages),
        (fields.language, &filters.languages),
        (fields.category, &filters.categories),
        (fields.tag_filter, &filters.genres),
        (fields.file_type, &filters.file_types),
        (fields.miscellany, &filters.miscellanies),
        (fields.circle_filter, &filters.circles),
    ] {
        add_filter_group(clauses, field, selection);
    }
}

fn add_filter_group(
    clauses: &mut Vec<(Occur, Box<dyn Query>)>,
    field: Field,
    selection: &CatalogFacetSelection,
) {
    if !selection.include.is_empty() {
        let included = selection
            .include
            .iter()
            .map(|value| {
                (
                    Occur::Should,
                    Box::new(TermQuery::new(
                        Term::from_field_text(field, &normalize_filter(value)),
                        IndexRecordOption::Basic,
                    )) as Box<dyn Query>,
                )
            })
            .collect();
        clauses.push((Occur::Must, Box::new(BooleanQuery::new(included))));
    }
    for value in &selection.exclude {
        clauses.push((
            Occur::MustNot,
            Box::new(TermQuery::new(
                Term::from_field_text(field, &normalize_filter(value)),
                IndexRecordOption::Basic,
            )),
        ));
    }
}

fn fuzzy_distance(text: &str) -> u8 {
    if text.chars().count() >= 8 { 2 } else { 1 }
}

fn work_code_prefix(text: &str) -> Option<String> {
    let code = text.trim().to_ascii_uppercase();
    let prefix = code.get(..2)?;
    let digits = code.get(2..)?;
    if matches!(prefix, "RJ" | "BJ" | "VJ")
        && !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        Some(code)
    } else {
        None
    }
}

fn normalize_filter(value: &str) -> String {
    value.trim().to_lowercase()
}

fn generation_name(snapshot_id: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let snapshot = snapshot_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '_'
            }
        })
        .take(48)
        .collect::<String>();
    format!("generation-{timestamp}-{snapshot}")
}

fn load_latest_generation(root: &Path, errors: &mut Vec<String>) -> Option<ActiveIndex> {
    let mut paths = match fs::read_dir(root) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_dir()
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("generation-"))
            })
            .collect::<Vec<_>>(),
        Err(error) => {
            errors.push(error.to_string());
            return None;
        }
    };
    paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    for path in paths {
        match load_generation(&path) {
            Ok(active) => return Some(active),
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    None
}

fn load_generation(path: &Path) -> Result<ActiveIndex, SearchError> {
    let metadata = serde_json::from_slice::<GenerationMetadata>(
        &fs::read(path.join("metadata.json")).map_err(SearchError::index)?,
    )
    .map_err(SearchError::index)?;
    if metadata.schema_version != SEARCH_SCHEMA_VERSION {
        return Err(SearchError::index(format!(
            "unsupported search schema version {}",
            metadata.schema_version
        )));
    }
    let index = Index::open_in_dir(path).map_err(SearchError::index)?;
    register_japanese_tokenizer(&index)?;
    let fields = fields_from_schema(&index.schema())?;
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()
        .map_err(SearchError::index)?;
    Ok(ActiveIndex {
        path: path.to_owned(),
        index,
        reader,
        fields,
        metadata,
    })
}

fn cleanup_managed_generations(
    root: &Path,
    active_path: Option<&Path>,
    prune_complete: bool,
) -> Result<SearchCacheCleanupReport, SearchError> {
    let entries = fs::read_dir(root).map_err(SearchError::index)?;
    let mut incomplete = Vec::new();
    let mut complete = Vec::new();
    for entry in entries {
        let entry = entry.map_err(SearchError::index)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(SearchError::index)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(".building-generation-") {
            incomplete.push(path);
        } else if name.starts_with("generation-") {
            complete.push(path);
        }
    }

    let mut report = SearchCacheCleanupReport::default();
    for path in incomplete {
        report.reclaimed_bytes = report
            .reclaimed_bytes
            .saturating_add(directory_size(&path)?);
        fs::remove_dir_all(path).map_err(SearchError::index)?;
        report.removed_incomplete_generations += 1;
    }

    complete.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    if !prune_complete {
        report.retained_complete_generations = complete.len();
        return Ok(report);
    }
    let mut retained = Vec::new();
    if let Some(active_path) = active_path
        && complete.iter().any(|path| path == active_path)
    {
        retained.push(active_path.to_path_buf());
    }
    for path in &complete {
        if retained.len() >= RETAIN_COMPLETE_GENERATIONS {
            break;
        }
        if !retained.contains(path) {
            retained.push(path.clone());
        }
    }
    for path in complete {
        if retained.contains(&path) {
            continue;
        }
        report.reclaimed_bytes = report
            .reclaimed_bytes
            .saturating_add(directory_size(&path)?);
        fs::remove_dir_all(path).map_err(SearchError::index)?;
        report.removed_complete_generations += 1;
    }
    report.retained_complete_generations = retained.len();
    Ok(report)
}

fn directory_size(path: &Path) -> Result<u64, SearchError> {
    let mut total = 0_u64;
    for entry in fs::read_dir(path).map_err(SearchError::index)? {
        let entry = entry.map_err(SearchError::index)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(SearchError::index)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            total = total.saturating_add(directory_size(&entry.path())?);
        } else if metadata.is_file() {
            total = total.saturating_add(metadata.len());
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use dla_application::{
        catalog::{CatalogFacetFilters, CatalogFacetSelection, CatalogReader},
        search::{
            CatalogSearchService, SearchIndexState, SearchRequest, SearchShortcutKind,
            SearchShortcutRequest,
        },
    };
    use dla_sqlite::SqliteCatalogStore;
    use tempfile::tempdir;

    use super::*;

    struct CancelAfterFirstBatch {
        cancellation: SearchRebuildCancellationToken,
        events: Mutex<Vec<SearchRebuildProgress>>,
    }

    impl SearchRebuildProgressSink for CancelAfterFirstBatch {
        fn publish(&self, progress: &SearchRebuildProgress) -> Result<(), SearchError> {
            self.events.lock().expect("events").push(progress.clone());
            if progress.stage == SearchRebuildStage::Indexing && progress.indexed_documents > 0 {
                self.cancellation.cancel();
            }
            Ok(())
        }
    }

    #[test]
    fn indexes_reopens_and_searches_the_synthetic_catalog_fixture() {
        let directory = tempdir().expect("temporary directory");
        let fixture = dla_catalog::load_test_fixture().expect("catalog fixture");
        let store = Arc::new(
            SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
                .expect("catalog store"),
        );
        let index_path = directory.path().join("search");
        let index = Arc::new(TantivyCatalogSearch::open(&index_path).expect("search index"));
        let service = CatalogSearchService::new(store.clone(), store.clone(), store.clone(), index);

        assert_eq!(service.status().state, SearchIndexState::Missing);
        let status = service.rebuild().expect("index rebuild");
        assert_eq!(status.state, SearchIndexState::Ready);
        assert_eq!(status.indexed_documents, 12);

        let exact = search(&service, "DLA-SYNTH-0008", CatalogFacetFilters::default());
        assert_eq!(exact.items[0].work.code, "DLA-SYNTH-0008");

        let japanese = search(&service, "多数タグ", CatalogFacetFilters::default());
        assert!(
            japanese
                .items
                .iter()
                .any(|result| result.work.code == "DLA-SYNTH-0008")
        );

        let detail = store
            .read("DLA-SYNTH-0008")
            .expect("catalog read")
            .expect("work");

        let tag_shortcuts = service
            .shortcuts(SearchShortcutRequest {
                text: "タグ12".to_owned(),
                limit: 6,
            })
            .expect("tag shortcuts");
        assert!(tag_shortcuts.iter().any(|shortcut| {
            shortcut.kind == SearchShortcutKind::Genre && shortcut.key == "タグ12"
        }));
        let circle_shortcuts = service
            .shortcuts(SearchShortcutRequest {
                text: "Fixture".to_owned(),
                limit: 6,
            })
            .expect("circle shortcuts");
        assert!(circle_shortcuts.iter().any(|shortcut| {
            shortcut.kind == SearchShortcutKind::Circle && shortcut.key == "Fixture Circle"
        }));

        let category = detail.work.categories[0].code.clone();
        let filtered = search(
            &service,
            "多数タグ",
            CatalogFacetFilters {
                categories: CatalogFacetSelection {
                    include: vec![category],
                    exclude: Vec::new(),
                },
                ..CatalogFacetFilters::default()
            },
        );
        assert!(
            filtered
                .items
                .iter()
                .any(|result| result.work.code == "DLA-SYNTH-0008")
        );

        let filtered_tag = search(
            &service,
            "多数タグ",
            CatalogFacetFilters {
                genres: CatalogFacetSelection {
                    include: vec!["タグ12".to_owned()],
                    exclude: Vec::new(),
                },
                ..CatalogFacetFilters::default()
            },
        );
        assert!(
            filtered_tag
                .items
                .iter()
                .any(|result| result.work.code == "DLA-SYNTH-0008")
        );

        let excluded_tag = search(
            &service,
            "多数タグ",
            CatalogFacetFilters {
                genres: CatalogFacetSelection {
                    include: Vec::new(),
                    exclude: vec!["タグ12".to_owned()],
                },
                ..CatalogFacetFilters::default()
            },
        );
        assert!(
            excluded_tag
                .items
                .iter()
                .all(|result| result.work.code != "DLA-SYNTH-0008")
        );

        let filtered_circle = search(
            &service,
            "複数カテゴリ",
            CatalogFacetFilters {
                circles: CatalogFacetSelection {
                    include: vec!["Cross Genre".to_owned()],
                    exclude: Vec::new(),
                },
                ..CatalogFacetFilters::default()
            },
        );
        assert_eq!(filtered_circle.items[0].work.code, "DLA-SYNTH-0011");

        let reopened = Arc::new(TantivyCatalogSearch::open(&index_path).expect("reopened index"));
        let reopened_service =
            CatalogSearchService::new(store.clone(), store.clone(), store, reopened);
        assert_eq!(reopened_service.status().state, SearchIndexState::Ready);
        assert_eq!(
            search(
                &reopened_service,
                "DLA-SYNTH-0008",
                CatalogFacetFilters::default()
            )
            .items[0]
                .work
                .code,
            "DLA-SYNTH-0008"
        );
    }

    #[test]
    fn cancellation_removes_the_incomplete_generation_and_preserves_the_active_index() {
        let directory = tempdir().expect("temporary directory");
        let fixture = dla_catalog::load_test_fixture().expect("catalog fixture");
        let store = Arc::new(
            SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
                .expect("catalog store"),
        );
        let index_path = directory.path().join("search");
        let index = Arc::new(TantivyCatalogSearch::open(&index_path).expect("search index"));
        let service = CatalogSearchService::new(store.clone(), store.clone(), store, index);
        let ready = service.rebuild().expect("initial search index");
        let cancellation = SearchRebuildCancellationToken::default();
        let progress = CancelAfterFirstBatch {
            cancellation: cancellation.clone(),
            events: Mutex::new(Vec::new()),
        };

        let error = service
            .rebuild_with_progress("cancelled-rebuild", &cancellation, &progress)
            .expect_err("cancelled rebuild");

        assert!(matches!(error, SearchError::Cancelled));
        assert_eq!(service.status().state, SearchIndexState::Ready);
        assert_eq!(service.status().generation, ready.generation);
        assert!(
            fs::read_dir(&index_path)
                .expect("search directory")
                .filter_map(Result::ok)
                .all(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".building-"))
        );
    }

    #[test]
    fn cleanup_removes_incomplete_and_old_generations_without_deleting_the_active_one() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        let active = root.join("generation-100-active");
        for name in [
            ".building-generation-400-interrupted",
            "generation-300-newest",
            "generation-200-old",
            "generation-100-active",
        ] {
            let path = root.join(name);
            fs::create_dir(&path).expect("generation directory");
            fs::write(path.join("payload"), b"derived cache").expect("payload");
        }

        let report = cleanup_managed_generations(root, Some(&active), true).expect("cleanup");

        assert_eq!(report.removed_incomplete_generations, 1);
        assert_eq!(report.removed_complete_generations, 1);
        assert_eq!(report.retained_complete_generations, 2);
        assert!(active.is_dir());
        assert!(root.join("generation-300-newest").is_dir());
        assert!(!root.join("generation-200-old").exists());
        assert!(!root.join(".building-generation-400-interrupted").exists());
        assert!(report.reclaimed_bytes > 0);
    }

    fn search(
        service: &CatalogSearchService,
        text: &str,
        facets: CatalogFacetFilters,
    ) -> dla_application::search::SearchResponse {
        service
            .search(SearchRequest {
                text: text.to_owned(),
                facets,
                limit: 30,
                offset: 0,
            })
            .expect("search response")
    }
}
