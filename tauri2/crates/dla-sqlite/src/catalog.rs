use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use dla_application::catalog::{
    CatalogContext, CatalogContextQuery, CatalogDayBucket, CatalogError, CatalogFacet,
    CatalogFacetFilters, CatalogFacets, CatalogMonthBucket, CatalogPage, CatalogQuery,
    CatalogReader, CatalogSnapshot, CatalogTimeline,
};
use dla_application::identity::{
    ArchiveHash, ArchiveHashAlgorithm, CatalogArchiveIdentity, CatalogIdentityError,
    CatalogIdentityReader,
};
use dla_application::recommendation::{
    CatalogRecommendationReader, RecommendationCandidate, RecommendationCandidatePool,
    RecommendationFacetFrequency,
};
use dla_application::search::{
    CatalogIndexSource, CatalogSearchDocument, CatalogSearchReader, SearchError, SearchShortcut,
    SearchShortcutKind,
};
use dla_catalog::CatalogFixture;
use dla_domain::{
    CatalogDescriptionVersion, CatalogDescriptions, CatalogRanking, CatalogRating,
    CatalogRelatedWork, CatalogRelationDirection, CatalogRom, CatalogRomContents, CatalogRomEntry,
    CatalogWork, CatalogWorkDetail, Category, NamedReference,
};
use rusqlite::{
    Connection, OptionalExtension, Row, Transaction, params, params_from_iter,
    types::{Type, Value},
};
use rusqlite_migration::{M, Migrations};

use crate::database;

const CATALOG_MIGRATION_LIST: &[M<'static>] = &[M::up(include_str!("../schema/catalog.sql"))];
pub(crate) const CATALOG_MIGRATIONS: Migrations<'static> =
    Migrations::from_slice(CATALOG_MIGRATION_LIST);

pub struct SqliteCatalogStore {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl SqliteCatalogStore {
    pub fn open(path: &Path, fixture: &CatalogFixture) -> Result<Self, CatalogError> {
        let mut connection =
            database::open(path, &CATALOG_MIGRATIONS).map_err(CatalogError::persistence)?;
        replace_fixture_if_needed(&mut connection, fixture).map_err(CatalogError::persistence)?;
        Ok(Self {
            path: path.to_path_buf(),
            connection: Mutex::new(connection),
        })
    }

    pub fn open_existing(path: &Path) -> Result<Self, CatalogError> {
        let connection =
            database::open(path, &CATALOG_MIGRATIONS).map_err(CatalogError::persistence)?;
        let snapshot_exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM catalog_snapshot WHERE singleton = 1)",
                [],
                |row| row.get::<_, bool>(0),
            )
            .map_err(CatalogError::persistence)?;
        if !snapshot_exists {
            return Err(CatalogError::persistence(
                "catalog database does not contain an activated snapshot",
            ));
        }
        Ok(Self {
            path: path.to_path_buf(),
            connection: Mutex::new(connection),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, CatalogError> {
        let connection = self
            .connection
            .lock()
            .map_err(|error| CatalogError::persistence(error.to_string()))?;
        operation(&connection).map_err(CatalogError::persistence)
    }
}

pub struct ReloadableCatalogStore {
    active: RwLock<Arc<SqliteCatalogStore>>,
}

impl ReloadableCatalogStore {
    pub fn new(active: Arc<SqliteCatalogStore>) -> Self {
        Self {
            active: RwLock::new(active),
        }
    }

    pub fn replace(
        &self,
        replacement: Arc<SqliteCatalogStore>,
    ) -> Result<Arc<SqliteCatalogStore>, CatalogError> {
        let mut active = self
            .active
            .write()
            .map_err(|error| CatalogError::persistence(error.to_string()))?;
        Ok(std::mem::replace(&mut *active, replacement))
    }

    pub fn current(&self) -> Result<Arc<SqliteCatalogStore>, CatalogError> {
        self.active
            .read()
            .map(|active| Arc::clone(&active))
            .map_err(|error| CatalogError::persistence(error.to_string()))
    }
}

impl CatalogReader for ReloadableCatalogStore {
    fn browse(&self, query: &CatalogQuery) -> Result<CatalogPage, CatalogError> {
        self.current()?.browse(query)
    }

    fn context(&self, query: &CatalogContextQuery) -> Result<CatalogContext, CatalogError> {
        self.current()?.context(query)
    }

    fn read(&self, code: &str) -> Result<Option<CatalogWorkDetail>, CatalogError> {
        self.current()?.read(code)
    }

    fn read_works(&self, codes: &[String]) -> Result<Vec<CatalogWork>, CatalogError> {
        self.current()?.read_works(codes)
    }

    fn read_rom_contents(
        &self,
        work_code: &str,
        rom_position: usize,
    ) -> Result<Option<CatalogRomContents>, CatalogError> {
        self.current()?.read_rom_contents(work_code, rom_position)
    }
}

impl CatalogRecommendationReader for ReloadableCatalogStore {
    fn read_recommendation_candidates(
        &self,
        work_code: &str,
        same_circle_limit: usize,
        similar_limit: usize,
    ) -> Result<Option<RecommendationCandidatePool>, CatalogError> {
        self.current()?
            .read_recommendation_candidates(work_code, same_circle_limit, similar_limit)
    }
}

impl CatalogIndexSource for ReloadableCatalogStore {
    fn snapshot(&self) -> Result<CatalogSnapshot, SearchError> {
        self.current().map_err(SearchError::source)?.snapshot()
    }

    fn read_search_batch(
        &self,
        after_work_code: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CatalogSearchDocument>, SearchError> {
        self.current()
            .map_err(SearchError::source)?
            .read_search_batch(after_work_code, limit)
    }
}

impl CatalogSearchReader for ReloadableCatalogStore {
    fn search_shortcuts(
        &self,
        text: &str,
        limit: usize,
    ) -> Result<Vec<SearchShortcut>, SearchError> {
        self.current()
            .map_err(SearchError::source)?
            .search_shortcuts(text, limit)
    }
}

impl CatalogIdentityReader for ReloadableCatalogStore {
    fn read_works_by_codes(
        &self,
        work_codes: &[String],
    ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
        self.current()
            .map_err(CatalogIdentityError::persistence)?
            .read_works_by_codes(work_codes)
    }

    fn resolve_archive_hash(
        &self,
        hash: &ArchiveHash,
    ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
        self.current()
            .map_err(CatalogIdentityError::persistence)?
            .resolve_archive_hash(hash)
    }

    fn find_archive_candidates_by_size(
        &self,
        size: &str,
        limit: usize,
    ) -> Result<Vec<CatalogArchiveIdentity>, CatalogIdentityError> {
        self.current()
            .map_err(CatalogIdentityError::persistence)?
            .find_archive_candidates_by_size(size, limit)
    }
}

impl CatalogReader for SqliteCatalogStore {
    fn browse(&self, query: &CatalogQuery) -> Result<CatalogPage, CatalogError> {
        if !query.search.is_empty() {
            return Err(CatalogError::TextSearchRequiresIndex);
        }
        self.with_connection(|connection| {
            let total = count_works(connection, query)?;
            let unfiltered_total = count_unfiltered_scope(connection, query)?;
            let items = browse_works(connection, query)?;
            let facets = if query.offset == 0 && query.month.is_none() {
                read_facets(connection, query)?
            } else {
                CatalogFacets::default()
            };
            let day_buckets = read_day_buckets(connection, query)?;
            let categories = facets.categories.clone();
            let tags = facets.genres.clone();
            let snapshot = read_snapshot(connection)?;

            Ok(CatalogPage {
                has_more: query.offset + items.len() < total,
                items,
                total,
                unfiltered_total,
                limit: query.limit,
                offset: query.offset,
                categories,
                tags,
                facets,
                day_buckets,
                snapshot,
            })
        })
    }

    fn context(&self, query: &CatalogContextQuery) -> Result<CatalogContext, CatalogError> {
        self.with_connection(|connection| read_catalog_context(connection, query))
    }

    fn read(&self, code: &str) -> Result<Option<CatalogWorkDetail>, CatalogError> {
        self.with_connection(|connection| read_work_detail(connection, code))
    }

    fn read_works(&self, codes: &[String]) -> Result<Vec<CatalogWork>, CatalogError> {
        self.with_connection(|connection| read_works_by_code(connection, codes))
    }

    fn read_rom_contents(
        &self,
        work_code: &str,
        rom_position: usize,
    ) -> Result<Option<CatalogRomContents>, CatalogError> {
        self.with_connection(|connection| read_rom_contents(connection, work_code, rom_position))
    }
}

impl CatalogRecommendationReader for SqliteCatalogStore {
    fn read_recommendation_candidates(
        &self,
        work_code: &str,
        same_circle_limit: usize,
        similar_limit: usize,
    ) -> Result<Option<RecommendationCandidatePool>, CatalogError> {
        self.with_connection(|connection| {
            read_recommendation_candidate_pool(
                connection,
                work_code,
                same_circle_limit,
                similar_limit,
            )
        })
    }
}

impl CatalogIndexSource for SqliteCatalogStore {
    fn snapshot(&self) -> Result<CatalogSnapshot, SearchError> {
        self.with_connection(read_snapshot)
            .map_err(SearchError::source)
    }

    fn read_search_batch(
        &self,
        after_work_code: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CatalogSearchDocument>, SearchError> {
        self.with_connection(|connection| {
            read_search_documents(connection, after_work_code.unwrap_or_default(), limit)
        })
        .map_err(SearchError::source)
    }
}

impl CatalogSearchReader for SqliteCatalogStore {
    fn search_shortcuts(
        &self,
        text: &str,
        limit: usize,
    ) -> Result<Vec<SearchShortcut>, SearchError> {
        self.with_connection(|connection| read_search_shortcuts(connection, text, limit))
            .map_err(SearchError::source)
    }
}

impl CatalogIdentityReader for SqliteCatalogStore {
    fn read_works_by_codes(
        &self,
        work_codes: &[String],
    ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
        self.with_connection(|connection| read_works_by_code(connection, work_codes))
            .map_err(CatalogIdentityError::persistence)
    }

    fn resolve_archive_hash(
        &self,
        hash: &ArchiveHash,
    ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
        self.with_connection(|connection| read_works_by_archive_hash(connection, hash))
            .map_err(CatalogIdentityError::persistence)
    }

    fn find_archive_candidates_by_size(
        &self,
        size: &str,
        limit: usize,
    ) -> Result<Vec<CatalogArchiveIdentity>, CatalogIdentityError> {
        self.with_connection(|connection| read_archive_candidates_by_size(connection, size, limit))
            .map_err(CatalogIdentityError::persistence)
    }
}

fn replace_fixture_if_needed(
    connection: &mut Connection,
    fixture: &CatalogFixture,
) -> rusqlite::Result<()> {
    let current_snapshot = connection
        .query_row(
            "SELECT snapshot_id FROM catalog_snapshot WHERE singleton = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if current_snapshot.as_deref() == Some(&fixture.snapshot_id) {
        return Ok(());
    }

    let transaction = connection.transaction()?;
    for statement in [
        "DELETE FROM catalog_work",
        "DELETE FROM catalog_relation_type",
        "DELETE FROM catalog_file_format",
        "DELETE FROM catalog_language",
        "DELETE FROM catalog_miscellany",
        "DELETE FROM catalog_circle",
        "DELETE FROM catalog_category",
        "DELETE FROM catalog_tag",
        "DELETE FROM catalog_snapshot",
    ] {
        transaction.execute(statement, [])?;
    }

    for work in &fixture.works {
        insert_work(&transaction, work)?;
    }
    for relation in &fixture.relations {
        transaction.execute(
            "INSERT INTO catalog_relation_type (relation_type_code, label)
             VALUES (?1, ?2)
             ON CONFLICT(relation_type_code) DO UPDATE SET label = excluded.label",
            params![relation.relation_type_code, relation.relation_type_label],
        )?;
        transaction.execute(
            "INSERT INTO catalog_work_relation
             (parent_work_code, child_work_code, relation_type_code)
             VALUES (?1, ?2, ?3)",
            params![
                relation.parent_work_code,
                relation.child_work_code,
                relation.relation_type_code
            ],
        )?;
    }
    for rom in &fixture.rom_contents {
        insert_rom_contents(&transaction, rom)?;
    }

    let real_works = fixture
        .works
        .iter()
        .filter(|work| !work.work.synthetic)
        .count();
    let synthetic_works = fixture.works.len() - real_works;
    transaction.execute(
        "INSERT INTO catalog_snapshot
         (singleton, snapshot_id, schema_version, real_work_count, synthetic_work_count, imported_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![
            fixture.snapshot_id,
            i64::from(fixture.schema_version),
            real_works as i64,
            synthetic_works as i64,
            database::now_rfc3339(),
        ],
    )?;
    transaction.commit()
}

fn insert_rom_contents(
    transaction: &Transaction<'_>,
    fixture: &dla_catalog::CatalogRomContentsFixture,
) -> rusqlite::Result<()> {
    let contents = &fixture.contents;
    transaction.execute(
        "INSERT INTO catalog_rom_content_scan
         (work_code, rom_position, status, archive_format, entry_count,
          total_uncompressed_size, truncated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            fixture.work_code,
            fixture.rom_position as i64,
            contents.status,
            contents.archive_format,
            contents.entry_count.map(|count| count as i64),
            contents.total_uncompressed_size,
            contents.truncated,
        ],
    )?;
    for entry in &contents.entries {
        transaction.execute(
            "INSERT INTO catalog_rom_content_entry
             (work_code, rom_position, entry_index, path, extension, is_directory,
              size, crc32, md5, sha1, sha256, hash_status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                fixture.work_code,
                fixture.rom_position as i64,
                entry.entry_index as i64,
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
    }
    Ok(())
}

pub(crate) fn insert_work(
    transaction: &Connection,
    detail: &CatalogWorkDetail,
) -> rusqlite::Result<()> {
    let work = &detail.work;
    let main_image_urls = serde_json::to_string(&work.main_image_urls)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
    let thumbnail_urls = serde_json::to_string(&work.thumbnail_urls)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
    let sample_image_urls = serde_json::to_string(&detail.sample_image_urls)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
    let rating_rankings = serde_json::to_string(
        &detail
            .rating
            .as_ref()
            .map(|rating| rating.rankings.as_slice())
            .unwrap_or_default(),
    )
    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
    transaction.execute(
        "INSERT INTO catalog_work
         (work_code, source_code, title, title_english, added_date, release_date, updated_date, age_rating, release_type, main_image_urls, thumbnail_urls, is_synthetic, sample_image_urls, rating_score, rating_count, total_sales, rating_rankings)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            work.code,
            work.source_code,
            work.title,
            work.title_english,
            work.added_date,
            work.release_date,
            work.updated_date,
            work.age_rating,
            work.release_type,
            main_image_urls,
            thumbnail_urls,
            work.synthetic,
            sample_image_urls,
            detail.rating.as_ref().map(|rating| rating.score),
            detail
                .rating
                .as_ref()
                .and_then(|rating| rating.rating_count)
                .map(|count| count as i64),
            detail
                .rating
                .as_ref()
                .and_then(|rating| rating.total_sales)
                .map(|count| count as i64),
            rating_rankings,
        ],
    )?;

    for (position, circle) in work.circles.iter().enumerate() {
        let circle_id = upsert_named_reference(
            transaction,
            "catalog_circle",
            "circle_id",
            &circle.name,
            &circle.name_english,
        )?;
        transaction.execute(
            "INSERT INTO catalog_work_circle (work_code, circle_id, position) VALUES (?1, ?2, ?3)",
            params![work.code, circle_id, position as i64],
        )?;
    }

    for (position, category) in work.categories.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_category (category_code, name, name_english)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(category_code) DO NOTHING",
            params![category.code, category.name, category.name_english],
        )?;
        transaction.execute(
            "INSERT INTO catalog_work_category (work_code, category_code, position) VALUES (?1, ?2, ?3)",
            params![work.code, category.code, position as i64],
        )?;
    }

    for (position, tag) in work.tags.iter().enumerate() {
        let tag_id = upsert_named_reference(
            transaction,
            "catalog_tag",
            "tag_id",
            &tag.name,
            &tag.name_english,
        )?;
        transaction.execute(
            "INSERT INTO catalog_work_tag (work_code, tag_id, position) VALUES (?1, ?2, ?3)",
            params![work.code, tag_id, position as i64],
        )?;
    }

    insert_categories(
        transaction,
        &work.code,
        "catalog_file_format",
        "file_format_code",
        "catalog_work_file_format",
        &detail.file_formats,
    )?;
    insert_categories(
        transaction,
        &work.code,
        "catalog_language",
        "language_code",
        "catalog_work_language",
        &detail.supported_languages,
    )?;
    insert_categories(
        transaction,
        &work.code,
        "catalog_miscellany",
        "miscellany_code",
        "catalog_work_miscellany",
        &detail.miscellanies,
    )?;

    for (position, rom) in detail.roms.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_rom
             (work_code, position, name, size, crc, md5, sha1, sha256, file_count, update_date, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                work.code,
                position as i64,
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
    }

    Ok(())
}

fn insert_categories(
    transaction: &Connection,
    work_code: &str,
    value_table: &str,
    code_column: &str,
    join_table: &str,
    values: &[Category],
) -> rusqlite::Result<()> {
    for (position, value) in values.iter().enumerate() {
        transaction.execute(
            &format!(
                "INSERT INTO {value_table} ({code_column}, name, name_english)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT({code_column}) DO UPDATE SET
                     name = excluded.name,
                     name_english = excluded.name_english"
            ),
            params![value.code, value.name, value.name_english],
        )?;
        transaction.execute(
            &format!(
                "INSERT INTO {join_table} (work_code, {code_column}, position)
                 VALUES (?1, ?2, ?3)"
            ),
            params![work_code, value.code, position as i64],
        )?;
    }
    Ok(())
}

fn upsert_named_reference(
    transaction: &Connection,
    table: &str,
    id_column: &str,
    name: &str,
    name_english: &str,
) -> rusqlite::Result<i64> {
    transaction.execute(
        &format!(
            "INSERT INTO {table} (name, name_english) VALUES (?1, ?2) ON CONFLICT(name, name_english) DO NOTHING"
        ),
        params![name, name_english],
    )?;
    transaction.query_row(
        &format!("SELECT {id_column} FROM {table} WHERE name = ?1 AND name_english = ?2"),
        params![name, name_english],
        |row| row.get(0),
    )
}

fn count_works(connection: &Connection, query: &CatalogQuery) -> rusqlite::Result<usize> {
    let predicate = build_predicate(query, None);
    connection.query_row(
        &format!(
            "SELECT count(*) FROM catalog_work work WHERE {}",
            predicate.sql
        ),
        params_from_iter(predicate.values.iter()),
        |row| row.get::<_, i64>(0).map(|value| value as usize),
    )
}

fn count_unfiltered_scope(
    connection: &Connection,
    query: &CatalogQuery,
) -> rusqlite::Result<usize> {
    if query.month.is_none() {
        return count_works(connection, query);
    }
    let predicate = build_scoped_predicate(
        &CatalogFacetFilters::default(),
        query.timeline,
        query.month.as_ref(),
        query.day.as_ref(),
        None,
    );
    connection.query_row(
        &format!(
            "SELECT count(*) FROM catalog_work work WHERE {}",
            predicate.sql
        ),
        params_from_iter(predicate.values.iter()),
        |row| row.get::<_, i64>(0).map(|value| value as usize),
    )
}

fn browse_works(
    connection: &Connection,
    query: &CatalogQuery,
) -> rusqlite::Result<Vec<CatalogWork>> {
    let mut predicate = build_predicate(query, None);
    predicate.values.push(Value::Integer(query.limit as i64));
    predicate.values.push(Value::Integer(query.offset as i64));
    let sql = format!(
        "{}
         WHERE {}
         ORDER BY {}
         LIMIT ? OFFSET ?",
        work_select(),
        predicate.sql,
        sort_expression(&query.sort, query.timeline, query.month.is_some()),
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(predicate.values.iter()), scan_work)?;
    rows.collect()
}

fn read_catalog_context(
    connection: &Connection,
    query: &CatalogContextQuery,
) -> rusqlite::Result<CatalogContext> {
    let column = query.timeline.date_column();
    let (min_month, max_month) = connection.query_row(
        &format!(
            "SELECT COALESCE(min(substr({column}, 1, 7)), ''),
                    COALESCE(max(substr({column}, 1, 7)), '')
             FROM catalog_work work
             WHERE work.is_synthetic = 0 AND {column} <> ''"
        ),
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )?;

    let mut predicate = build_facet_predicate(&query.facets, None);
    predicate
        .sql
        .push_str(&format!(" AND work.is_synthetic = 0 AND {column} <> ''"));
    let mut statement = connection.prepare(&format!(
        "SELECT substr({column}, 1, 7), count(*)
         FROM catalog_work work
         WHERE {}
         GROUP BY substr({column}, 1, 7)
         ORDER BY substr({column}, 1, 7)",
        predicate.sql
    ))?;
    let months = statement
        .query_map(params_from_iter(predicate.values.iter()), |row| {
            Ok(CatalogMonthBucket {
                month: row.get(0)?,
                count: row.get::<_, i64>(1)? as usize,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let default_month = max_month.clone();
    let facet_query = CatalogQuery {
        search: String::new(),
        facets: query.facets.clone(),
        sort: "release_desc".to_owned(),
        timeline: query.timeline,
        month: None,
        day: None,
        limit: 1,
        offset: 0,
    };

    Ok(CatalogContext {
        min_month,
        max_month,
        default_month,
        months,
        facets: read_facets(connection, &facet_query)?,
        snapshot: read_snapshot(connection)?,
    })
}

fn read_day_buckets(
    connection: &Connection,
    query: &CatalogQuery,
) -> rusqlite::Result<Vec<CatalogDayBucket>> {
    let Some(month) = query.month.as_ref() else {
        return Ok(Vec::new());
    };
    if query.day.is_some() {
        return Ok(Vec::new());
    }
    let predicate = build_predicate(query, None);
    let column = query.timeline.date_column();
    let mut statement = connection.prepare(&format!(
        "SELECT substr({column}, 1, 10), count(*)
         FROM catalog_work work
         WHERE {}
         GROUP BY substr({column}, 1, 10)
         ORDER BY substr({column}, 1, 10)",
        predicate.sql
    ))?;
    let counts = statement
        .query_map(params_from_iter(predicate.values.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?
        .collect::<rusqlite::Result<std::collections::HashMap<_, _>>>()?;
    Ok((1..=month.days())
        .map(|day| {
            let date = month.day(day);
            CatalogDayBucket {
                count: counts.get(&date).copied().unwrap_or(0),
                day: date,
            }
        })
        .collect())
}

fn read_work(connection: &Connection, code: &str) -> rusqlite::Result<Option<CatalogWork>> {
    connection
        .query_row(
            &format!("{} WHERE work.work_code = ?1 COLLATE NOCASE", work_select()),
            params![code],
            scan_work,
        )
        .optional()
}

fn read_works_by_code(
    connection: &Connection,
    work_codes: &[String],
) -> rusqlite::Result<Vec<CatalogWork>> {
    if work_codes.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "{} WHERE work.work_code COLLATE NOCASE IN ({})",
        work_select(),
        placeholders(work_codes.len())
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(work_codes.iter()), scan_work)?;
    rows.collect()
}

fn read_works_by_archive_hash(
    connection: &Connection,
    hash: &ArchiveHash,
) -> rusqlite::Result<Vec<CatalogWork>> {
    let hash_column = match hash.algorithm {
        ArchiveHashAlgorithm::Md5 => "md5",
        ArchiveHashAlgorithm::Sha1 => "sha1",
        ArchiveHashAlgorithm::Sha256 => "sha256",
    };
    let mut statement = connection.prepare(&format!(
        "SELECT DISTINCT work_code
         FROM catalog_rom
         WHERE {hash_column} = ?1 COLLATE NOCASE
         ORDER BY work_code COLLATE NOCASE"
    ))?;
    let work_codes = statement
        .query_map(params![hash.digest], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    read_works_by_code(connection, &work_codes)
}

fn read_archive_candidates_by_size(
    connection: &Connection,
    size: &str,
    limit: usize,
) -> rusqlite::Result<Vec<CatalogArchiveIdentity>> {
    let limit = i64::try_from(limit)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
    let mut statement = connection.prepare(
        "SELECT work_code, position, name, size, md5, sha1, sha256
         FROM catalog_rom
         WHERE size = ?1
         ORDER BY work_code COLLATE NOCASE, position
         LIMIT ?2",
    )?;
    let rows = statement.query_map(params![size, limit], |row| {
        Ok(CatalogArchiveIdentity {
            work_code: row.get(0)?,
            rom_position: row.get::<_, i64>(1)? as usize,
            name: row.get(2)?,
            size: row.get(3)?,
            md5: row.get(4)?,
            sha1: row.get(5)?,
            sha256: row.get(6)?,
        })
    })?;
    rows.collect()
}

fn read_search_shortcuts(
    connection: &Connection,
    text: &str,
    limit: usize,
) -> rusqlite::Result<Vec<SearchShortcut>> {
    let pattern = format!("%{}%", escape_like(text));
    let prefix = format!("{}%", escape_like(text));
    let mut shortcuts = read_named_shortcuts(
        connection,
        NamedShortcutSource {
            value_table: "catalog_tag",
            work_table: "catalog_work_tag",
            id_column: "tag_id",
            kind: SearchShortcutKind::Genre,
        },
        text,
        &pattern,
        &prefix,
        limit,
    )?;
    shortcuts.extend(read_named_shortcuts(
        connection,
        NamedShortcutSource {
            value_table: "catalog_circle",
            work_table: "catalog_work_circle",
            id_column: "circle_id",
            kind: SearchShortcutKind::Circle,
        },
        text,
        &pattern,
        &prefix,
        limit,
    )?);
    Ok(shortcuts)
}

#[derive(Clone, Copy)]
struct NamedShortcutSource {
    value_table: &'static str,
    work_table: &'static str,
    id_column: &'static str,
    kind: SearchShortcutKind,
}

fn read_named_shortcuts(
    connection: &Connection,
    source: NamedShortcutSource,
    text: &str,
    pattern: &str,
    prefix: &str,
    limit: usize,
) -> rusqlite::Result<Vec<SearchShortcut>> {
    let NamedShortcutSource {
        value_table,
        work_table,
        id_column,
        kind,
    } = source;
    let mut statement = connection.prepare(&format!(
        "SELECT value.name, value.name_english, count(DISTINCT work_value.work_code)
         FROM {value_table} value
         JOIN {work_table} work_value USING ({id_column})
         WHERE value.name LIKE ?1 ESCAPE '\\' COLLATE NOCASE
            OR value.name_english LIKE ?1 ESCAPE '\\' COLLATE NOCASE
         GROUP BY value.{id_column}, value.name, value.name_english
         ORDER BY
           CASE
             WHEN value.name = ?2 COLLATE NOCASE OR value.name_english = ?2 COLLATE NOCASE THEN 0
             WHEN value.name LIKE ?3 ESCAPE '\\' COLLATE NOCASE
               OR value.name_english LIKE ?3 ESCAPE '\\' COLLATE NOCASE THEN 1
             ELSE 2
           END,
           count(DISTINCT work_value.work_code) DESC,
           value.name COLLATE NOCASE
         LIMIT ?4"
    ))?;
    let rows = statement.query_map(params![pattern, text, prefix, limit as i64], |row| {
        let label = row.get::<_, String>(0)?;
        Ok(SearchShortcut {
            kind,
            key: label.clone(),
            label,
            label_english: row.get(1)?,
            count: row.get::<_, i64>(2)? as usize,
        })
    })?;
    rows.collect()
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn read_work_detail(
    connection: &Connection,
    code: &str,
) -> rusqlite::Result<Option<CatalogWorkDetail>> {
    let Some(work) = read_work(connection, code)? else {
        return Ok(None);
    };
    let (sample_image_urls, rating_score, rating_count, total_sales, rating_rankings) = connection
        .query_row(
            "SELECT sample_image_urls, rating_score, rating_count, total_sales, rating_rankings
             FROM catalog_work
             WHERE work_code = ?1 COLLATE NOCASE",
            params![work.code],
            |row| {
                Ok((
                    decode_json::<Vec<String>>(row.get(0)?, 0)?,
                    row.get::<_, Option<f64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    decode_json::<Vec<CatalogRanking>>(row.get(4)?, 4)?,
                ))
            },
        )?;
    let rating = rating_score.map(|score| CatalogRating {
        score,
        rating_count: rating_count.map(|count| count as u64),
        total_sales: total_sales.map(|count| count as u64),
        rankings: rating_rankings,
    });
    let (descriptions_included, description_versions) = connection.query_row(
        "SELECT
           EXISTS(SELECT 1 FROM catalog_field_presence WHERE field_id = 'work.descriptions'),
           (SELECT description_versions FROM catalog_work_enrichment WHERE work_code = ?1)",
        params![work.code],
        |row| {
            let included = row.get::<_, bool>(0)?;
            let versions = row
                .get::<_, Option<String>>(1)?
                .map(|value| decode_json::<Vec<CatalogDescriptionVersion>>(value, 1))
                .transpose()?
                .unwrap_or_default();
            Ok((included, versions))
        },
    )?;

    Ok(Some(CatalogWorkDetail {
        file_formats: read_categories(
            connection,
            "catalog_work_file_format",
            "catalog_file_format",
            "file_format_code",
            &work.code,
        )?,
        supported_languages: read_categories(
            connection,
            "catalog_work_language",
            "catalog_language",
            "language_code",
            &work.code,
        )?,
        miscellanies: read_categories(
            connection,
            "catalog_work_miscellany",
            "catalog_miscellany",
            "miscellany_code",
            &work.code,
        )?,
        roms: read_roms(connection, &work.code)?,
        related_works: read_related_works(connection, &work.code)?,
        sample_image_urls,
        rating,
        descriptions: CatalogDescriptions {
            included: descriptions_included,
            versions: description_versions,
        },
        work,
    }))
}

const SAME_CIRCLE_RECOMMENDATION_SQL: &str = "WITH anchor_circles AS (
         SELECT circle_id
         FROM catalog_work_circle
         WHERE work_code = ?1
     )
     SELECT candidate.work_code
     FROM anchor_circles
     JOIN catalog_work_circle candidate USING (circle_id)
     JOIN catalog_work work ON work.work_code = candidate.work_code
     WHERE candidate.work_code <> ?1
     GROUP BY candidate.work_code
     ORDER BY work.release_date DESC, candidate.work_code ASC
     LIMIT ?2";

const SIMILAR_RECOMMENDATION_SQL: &str = "WITH candidate_signal(work_code, weight) AS (
         SELECT candidate.work_code, 8
         FROM catalog_work_tag anchor
         JOIN catalog_work_tag candidate USING (tag_id)
         WHERE anchor.work_code = ?1 AND candidate.work_code <> ?1
         UNION ALL
         SELECT candidate.work_code, 5
         FROM catalog_work_category anchor
         JOIN catalog_work_category candidate USING (category_code)
         WHERE anchor.work_code = ?1 AND candidate.work_code <> ?1
         UNION ALL
         SELECT candidate.work_code, 2
         FROM catalog_work_miscellany anchor
         JOIN catalog_work_miscellany candidate USING (miscellany_code)
         WHERE anchor.work_code = ?1 AND candidate.work_code <> ?1
         UNION ALL
         SELECT candidate.work_code, 1
         FROM catalog_work_file_format anchor
         JOIN catalog_work_file_format candidate USING (file_format_code)
         WHERE anchor.work_code = ?1 AND candidate.work_code <> ?1
         UNION ALL
         SELECT candidate.work_code, 1
         FROM catalog_work_language anchor
         JOIN catalog_work_language candidate USING (language_code)
         WHERE anchor.work_code = ?1 AND candidate.work_code <> ?1
     )
     SELECT signal.work_code
     FROM candidate_signal signal
     JOIN catalog_work work ON work.work_code = signal.work_code
     WHERE NOT EXISTS (
         SELECT 1
         FROM catalog_work_circle anchor_circle
         JOIN catalog_work_circle candidate_circle USING (circle_id)
         WHERE anchor_circle.work_code = ?1
           AND candidate_circle.work_code = signal.work_code
     )
     GROUP BY signal.work_code
     ORDER BY sum(signal.weight) DESC, work.release_date DESC, signal.work_code ASC
     LIMIT ?2";

fn read_recommendation_candidate_pool(
    connection: &Connection,
    work_code: &str,
    same_circle_limit: usize,
    similar_limit: usize,
) -> rusqlite::Result<Option<RecommendationCandidatePool>> {
    let Some(anchor) = read_work_detail(connection, work_code)? else {
        return Ok(None);
    };
    let same_circle_codes = read_recommendation_candidate_codes(
        connection,
        SAME_CIRCLE_RECOMMENDATION_SQL,
        &anchor.work.code,
        same_circle_limit,
    )?;
    let similar_codes = read_recommendation_candidate_codes(
        connection,
        SIMILAR_RECOMMENDATION_SQL,
        &anchor.work.code,
        similar_limit,
    )?;
    let snapshot = read_snapshot(connection)?;
    Ok(Some(RecommendationCandidatePool {
        same_circle: hydrate_recommendation_candidates(connection, &same_circle_codes)?,
        similar: hydrate_recommendation_candidates(connection, &similar_codes)?,
        tag_frequencies: read_anchor_tag_frequencies(connection, &anchor.work.code)?,
        catalog_size: snapshot.real_works + snapshot.synthetic_works,
        anchor,
    }))
}

fn read_recommendation_candidate_codes(
    connection: &Connection,
    sql: &str,
    work_code: &str,
    limit: usize,
) -> rusqlite::Result<Vec<String>> {
    let limit = i64::try_from(limit)
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(error.into()))?;
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params![work_code, limit], |row| row.get(0))?;
    rows.collect()
}

fn hydrate_recommendation_candidates(
    connection: &Connection,
    work_codes: &[String],
) -> rusqlite::Result<Vec<RecommendationCandidate>> {
    if work_codes.is_empty() {
        return Ok(Vec::new());
    }
    let mut works = read_works_by_code(connection, work_codes)?
        .into_iter()
        .map(|work| (work.code.to_lowercase(), work))
        .collect::<HashMap<_, _>>();
    let mut file_formats = read_candidate_categories(
        connection,
        "catalog_work_file_format",
        "catalog_file_format",
        "file_format_code",
        work_codes,
    )?;
    let mut supported_languages = read_candidate_categories(
        connection,
        "catalog_work_language",
        "catalog_language",
        "language_code",
        work_codes,
    )?;
    let mut miscellanies = read_candidate_categories(
        connection,
        "catalog_work_miscellany",
        "catalog_miscellany",
        "miscellany_code",
        work_codes,
    )?;

    Ok(work_codes
        .iter()
        .filter_map(|work_code| {
            let key = work_code.to_lowercase();
            Some(RecommendationCandidate {
                work: works.remove(&key)?,
                file_formats: file_formats.remove(&key).unwrap_or_default(),
                supported_languages: supported_languages.remove(&key).unwrap_or_default(),
                miscellanies: miscellanies.remove(&key).unwrap_or_default(),
            })
        })
        .collect())
}

fn read_candidate_categories(
    connection: &Connection,
    join_table: &str,
    value_table: &str,
    code_column: &str,
    work_codes: &[String],
) -> rusqlite::Result<HashMap<String, Vec<Category>>> {
    let sql = format!(
        "SELECT work_value.work_code, value.{code_column}, value.name, value.name_english
         FROM {join_table} work_value
         JOIN {value_table} value USING ({code_column})
         WHERE work_value.work_code IN ({})
         ORDER BY work_value.work_code, work_value.position",
        placeholders(work_codes.len())
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(work_codes.iter()), |row| {
        Ok((
            row.get::<_, String>(0)?,
            Category {
                code: row.get(1)?,
                name: row.get(2)?,
                name_english: row.get(3)?,
            },
        ))
    })?;
    let mut categories = HashMap::<String, Vec<Category>>::new();
    for row in rows {
        let (work_code, category) = row?;
        categories
            .entry(work_code.to_lowercase())
            .or_default()
            .push(category);
    }
    Ok(categories)
}

fn read_anchor_tag_frequencies(
    connection: &Connection,
    work_code: &str,
) -> rusqlite::Result<Vec<RecommendationFacetFrequency>> {
    let mut statement = connection.prepare(
        "SELECT tag.name, count(candidate.work_code)
         FROM catalog_work_tag anchor
         JOIN catalog_tag tag USING (tag_id)
         JOIN catalog_work_tag candidate USING (tag_id)
         WHERE anchor.work_code = ?1
         GROUP BY tag.tag_id, tag.name
         ORDER BY tag.name",
    )?;
    let rows = statement.query_map(params![work_code], |row| {
        Ok(RecommendationFacetFrequency {
            key: row.get(0)?,
            count: row.get::<_, i64>(1)? as usize,
        })
    })?;
    rows.collect()
}

fn read_categories(
    connection: &Connection,
    join_table: &str,
    value_table: &str,
    code_column: &str,
    work_code: &str,
) -> rusqlite::Result<Vec<Category>> {
    let sql = format!(
        "SELECT value.{code_column}, value.name, value.name_english
         FROM {join_table} work_value
         JOIN {value_table} value USING ({code_column})
         WHERE work_value.work_code = ?1
         ORDER BY work_value.position"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![work_code], |row| {
        Ok(Category {
            code: row.get(0)?,
            name: row.get(1)?,
            name_english: row.get(2)?,
        })
    })?;
    rows.collect()
}

fn read_roms(connection: &Connection, work_code: &str) -> rusqlite::Result<Vec<CatalogRom>> {
    let mut statement = connection.prepare(
        "SELECT name, size, crc, md5, sha1, sha256, file_count, update_date, version
         FROM catalog_rom
         WHERE work_code = ?1
         ORDER BY position",
    )?;
    let rows = statement.query_map(params![work_code], |row| {
        Ok(CatalogRom {
            name: row.get(0)?,
            size: row.get(1)?,
            crc: row.get(2)?,
            md5: row.get(3)?,
            sha1: row.get(4)?,
            sha256: row.get(5)?,
            file_count: row.get::<_, Option<i64>>(6)?.map(|count| count as u64),
            update_date: row.get(7)?,
            version: row.get(8)?,
        })
    })?;
    rows.collect()
}

fn read_rom_contents(
    connection: &Connection,
    work_code: &str,
    rom_position: usize,
) -> rusqlite::Result<Option<CatalogRomContents>> {
    let scan = connection
        .query_row(
            "SELECT status, archive_format, entry_count, total_uncompressed_size, truncated
             FROM catalog_rom_content_scan
             WHERE work_code = ?1 AND rom_position = ?2",
            params![work_code, rom_position as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((status, archive_format, entry_count, total_uncompressed_size, truncated)) = scan
    else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT entry_index, path, extension, is_directory, size, crc32, md5, sha1, sha256,
                hash_status
         FROM catalog_rom_content_entry
         WHERE work_code = ?1 AND rom_position = ?2
         ORDER BY entry_index",
    )?;
    let rows = statement.query_map(params![work_code, rom_position as i64], |row| {
        Ok(CatalogRomEntry {
            entry_index: row.get::<_, i64>(0)? as u64,
            path: row.get(1)?,
            extension: row.get(2)?,
            is_directory: row.get(3)?,
            size: row.get(4)?,
            crc32: row.get(5)?,
            md5: row.get(6)?,
            sha1: row.get(7)?,
            sha256: row.get(8)?,
            hash_status: row.get(9)?,
        })
    })?;
    let entries = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(CatalogRomContents {
        status,
        archive_format,
        entry_count: entry_count.map(|count| count as u64),
        total_uncompressed_size,
        truncated,
        entries,
    }))
}

fn read_related_works(
    connection: &Connection,
    work_code: &str,
) -> rusqlite::Result<Vec<CatalogRelatedWork>> {
    let mut statement = connection.prepare(
        "SELECT related.work_code, related.title, related.title_english,
                relation.relation_type_code, relation_type.label,
                relation.direction, related.thumbnail_urls
         FROM (
             SELECT child_work_code AS related_code, relation_type_code,
                    'parent' AS direction, 1 AS direction_order
             FROM catalog_work_relation
             WHERE parent_work_code = ?1
             UNION ALL
             SELECT parent_work_code, relation_type_code,
                    'child' AS direction, 0 AS direction_order
             FROM catalog_work_relation
             WHERE child_work_code = ?1
             UNION ALL
             SELECT sibling.child_work_code, sibling.relation_type_code,
                    'sibling' AS direction, 2 AS direction_order
             FROM catalog_work_relation mine
             JOIN catalog_work_relation sibling
               ON sibling.parent_work_code = mine.parent_work_code
              AND sibling.child_work_code <> mine.child_work_code
             WHERE mine.child_work_code = ?1
         ) relation
         JOIN catalog_work related ON related.work_code = relation.related_code
         JOIN catalog_relation_type relation_type
           ON relation_type.relation_type_code = relation.relation_type_code
         ORDER BY relation.direction_order, relation_type.label, related.work_code",
    )?;
    let rows = statement.query_map(params![work_code], |row| {
        let direction = match row.get::<_, String>(5)?.as_str() {
            "child" => CatalogRelationDirection::Child,
            "sibling" => CatalogRelationDirection::Sibling,
            _ => CatalogRelationDirection::Parent,
        };
        Ok(CatalogRelatedWork {
            code: row.get(0)?,
            title: row.get(1)?,
            title_english: row.get(2)?,
            relation_type_code: row.get(3)?,
            relation_type_label: row.get(4)?,
            direction,
            thumbnail_urls: decode_json(row.get(6)?, 6)?,
        })
    })?;
    let mut seen = std::collections::HashSet::new();
    let mut related = Vec::new();
    for row in rows {
        let item = row?;
        if seen.insert(item.code.to_lowercase()) {
            related.push(item);
        }
    }
    Ok(related)
}

fn work_select() -> &'static str {
    "SELECT
            work.work_code,
            work.source_code,
            work.title,
            work.title_english,
            work.added_date,
            work.release_date,
            work.updated_date,
            work.age_rating,
            work.release_type,
            work.main_image_urls,
            work.thumbnail_urls,
            work.is_synthetic,
            COALESCE((
                SELECT json_group_array(json_object('name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT circle.name, circle.name_english
                    FROM catalog_work_circle work_circle
                    JOIN catalog_circle circle USING (circle_id)
                    WHERE work_circle.work_code = work.work_code
                    ORDER BY work_circle.position
                ) ordered
            ), '[]'),
            COALESCE((
                SELECT json_group_array(json_object('code', ordered.category_code, 'name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT category.category_code, category.name, category.name_english
                    FROM catalog_work_category work_category
                    JOIN catalog_category category USING (category_code)
                    WHERE work_category.work_code = work.work_code
                    ORDER BY work_category.position
                ) ordered
            ), '[]'),
            COALESCE((
                SELECT json_group_array(json_object('name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT tag.name, tag.name_english
                    FROM catalog_work_tag work_tag
                    JOIN catalog_tag tag USING (tag_id)
                    WHERE work_tag.work_code = work.work_code
                    ORDER BY work_tag.position
                ) ordered
            ), '[]')
         FROM catalog_work work
         LEFT JOIN catalog_work_enrichment enrichment USING (work_code)"
}

fn scan_work(row: &Row<'_>) -> rusqlite::Result<CatalogWork> {
    let main_image_urls = decode_json::<Vec<String>>(row.get(9)?, 9)?;
    let thumbnail_urls = decode_json::<Vec<String>>(row.get(10)?, 10)?;
    let circles = decode_json::<Vec<NamedReference>>(row.get(12)?, 12)?;
    let categories = decode_json::<Vec<Category>>(row.get(13)?, 13)?;
    let tags = decode_json::<Vec<NamedReference>>(row.get(14)?, 14)?;
    Ok(CatalogWork {
        code: row.get(0)?,
        source_code: row.get(1)?,
        title: row.get(2)?,
        title_english: row.get(3)?,
        added_date: row.get(4)?,
        release_date: row.get(5)?,
        updated_date: row.get(6)?,
        age_rating: row.get(7)?,
        release_type: row.get(8)?,
        main_image_urls,
        thumbnail_urls,
        circles,
        categories,
        tags,
        synthetic: row.get(11)?,
    })
}

fn decode_json<T: serde::de::DeserializeOwned>(
    value: String,
    column: usize,
) -> rusqlite::Result<T> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, error.into())
    })
}

fn read_facets(connection: &Connection, query: &CatalogQuery) -> rusqlite::Result<CatalogFacets> {
    Ok(CatalogFacets {
        ages: age_facets(connection, query)?,
        languages: association_facets(
            connection,
            query,
            FacetGroup::Languages,
            "catalog_language",
            "catalog_work_language",
            "language_code",
            "language_code",
        )?,
        categories: association_facets(
            connection,
            query,
            FacetGroup::Categories,
            "catalog_category",
            "catalog_work_category",
            "category_code",
            "category_code",
        )?,
        genres: association_facets(
            connection,
            query,
            FacetGroup::Genres,
            "catalog_tag",
            "catalog_work_tag",
            "tag_id",
            "name",
        )?,
        file_types: association_facets(
            connection,
            query,
            FacetGroup::FileTypes,
            "catalog_file_format",
            "catalog_work_file_format",
            "file_format_code",
            "file_format_code",
        )?,
        miscellanies: association_facets(
            connection,
            query,
            FacetGroup::Miscellanies,
            "catalog_miscellany",
            "catalog_work_miscellany",
            "miscellany_code",
            "miscellany_code",
        )?,
        circles: association_facets(
            connection,
            query,
            FacetGroup::Circles,
            "catalog_circle",
            "catalog_work_circle",
            "circle_id",
            "name",
        )?,
    })
}

fn age_facets(
    connection: &Connection,
    query: &CatalogQuery,
) -> rusqlite::Result<Vec<CatalogFacet>> {
    let predicate = build_predicate(query, Some(FacetGroup::Ages));
    let mut facets = scan_facets(
        connection,
        &format!(
            "SELECT work.age_rating, work.age_rating, work.age_rating, count(*)
             FROM catalog_work work
             WHERE work.age_rating <> '' AND {}
             GROUP BY work.age_rating
             ORDER BY count(*) DESC, work.age_rating",
            predicate.sql
        ),
        &predicate.values,
    )?;
    for facet in &mut facets {
        let label = match facet.key.as_str() {
            "all_ages" => "All Ages",
            "r15" => "R15",
            "r18" => "R18",
            value => value,
        };
        facet.label = label.to_owned();
        facet.label_english = label.to_owned();
    }
    Ok(facets)
}

fn association_facets(
    connection: &Connection,
    query: &CatalogQuery,
    group: FacetGroup,
    value_table: &str,
    join_table: &str,
    join_key: &str,
    filter_key: &str,
) -> rusqlite::Result<Vec<CatalogFacet>> {
    let predicate = build_predicate(query, Some(group));
    scan_facets(
        connection,
        &format!(
            "SELECT value.{filter_key}, value.name, value.name_english,
                    count(DISTINCT work_value.work_code)
             FROM {value_table} value
             JOIN {join_table} work_value
               ON work_value.{join_key} = value.{join_key}
             JOIN catalog_work work ON work.work_code = work_value.work_code
             WHERE {}
             GROUP BY value.{join_key}, value.{filter_key}, value.name, value.name_english
             ORDER BY count(DISTINCT work_value.work_code) DESC,
                      value.name COLLATE NOCASE, value.{filter_key}",
            predicate.sql
        ),
        &predicate.values,
    )
}

fn scan_facets(
    connection: &Connection,
    sql: &str,
    values: &[Value],
) -> rusqlite::Result<Vec<CatalogFacet>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params_from_iter(values.iter()), |row| {
        Ok(CatalogFacet {
            key: row.get(0)?,
            label: row.get(1)?,
            label_english: row.get(2)?,
            count: row.get::<_, i64>(3)? as usize,
        })
    })?;
    rows.collect()
}

fn read_snapshot(connection: &Connection) -> rusqlite::Result<CatalogSnapshot> {
    connection.query_row(
        "SELECT snapshot_id, real_work_count, synthetic_work_count FROM catalog_snapshot WHERE singleton = 1",
        [],
        |row| {
            Ok(CatalogSnapshot {
                id: row.get(0)?,
                real_works: row.get::<_, i64>(1)? as usize,
                synthetic_works: row.get::<_, i64>(2)? as usize,
            })
        },
    )
}

fn read_search_documents(
    connection: &Connection,
    after_work_code: &str,
    limit: usize,
) -> rusqlite::Result<Vec<CatalogSearchDocument>> {
    let mut statement = connection.prepare(
        "SELECT
            work.work_code,
            work.source_code,
            work.title,
            work.title_english,
            work.added_date,
            work.release_date,
            work.updated_date,
            work.age_rating,
            COALESCE((
                SELECT json_group_array(json_object('name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT circle.name, circle.name_english
                    FROM catalog_work_circle work_circle
                    JOIN catalog_circle circle USING (circle_id)
                    WHERE work_circle.work_code = work.work_code
                    ORDER BY work_circle.position
                ) ordered
            ), '[]'),
            COALESCE((
                SELECT json_group_array(json_object('code', ordered.category_code, 'name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT category.category_code, category.name, category.name_english
                    FROM catalog_work_category work_category
                    JOIN catalog_category category USING (category_code)
                    WHERE work_category.work_code = work.work_code
                    ORDER BY work_category.position
                ) ordered
            ), '[]'),
            COALESCE((
                SELECT json_group_array(json_object('name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT tag.name, tag.name_english
                    FROM catalog_work_tag work_tag
                    JOIN catalog_tag tag USING (tag_id)
                    WHERE work_tag.work_code = work.work_code
                    ORDER BY work_tag.position
                ) ordered
            ), '[]'),
            COALESCE((
                SELECT json_group_array(json_object('code', ordered.file_format_code, 'name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT format.file_format_code, format.name, format.name_english
                    FROM catalog_work_file_format work_format
                    JOIN catalog_file_format format USING (file_format_code)
                    WHERE work_format.work_code = work.work_code
                    ORDER BY work_format.position
                ) ordered
            ), '[]'),
            COALESCE((
                SELECT json_group_array(json_object('code', ordered.language_code, 'name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT language.language_code, language.name, language.name_english
                    FROM catalog_work_language work_language
                    JOIN catalog_language language USING (language_code)
                    WHERE work_language.work_code = work.work_code
                    ORDER BY work_language.position
                ) ordered
            ), '[]'),
            COALESCE((
                SELECT json_group_array(json_object('code', ordered.miscellany_code, 'name', ordered.name, 'nameEnglish', ordered.name_english))
                FROM (
                    SELECT miscellany.miscellany_code, miscellany.name, miscellany.name_english
                    FROM catalog_work_miscellany work_miscellany
                    JOIN catalog_miscellany miscellany USING (miscellany_code)
                    WHERE work_miscellany.work_code = work.work_code
                    ORDER BY work_miscellany.position
                ) ordered
            ), '[]')
         FROM catalog_work work
         WHERE work.work_code > ?1 COLLATE NOCASE
         ORDER BY work.work_code COLLATE NOCASE
         LIMIT ?2",
    )?;
    let rows = statement.query_map(params![after_work_code, limit as i64], |row| {
        Ok(CatalogSearchDocument {
            work_code: row.get(0)?,
            source_code: row.get(1)?,
            title: row.get(2)?,
            title_english: row.get(3)?,
            added_date: row.get(4)?,
            release_date: row.get(5)?,
            updated_date: row.get(6)?,
            age_rating: row.get(7)?,
            circles: decode_json(row.get(8)?, 8)?,
            categories: decode_json(row.get(9)?, 9)?,
            tags: decode_json(row.get(10)?, 10)?,
            file_formats: decode_json(row.get(11)?, 11)?,
            supported_languages: decode_json(row.get(12)?, 12)?,
            miscellanies: decode_json(row.get(13)?, 13)?,
        })
    })?;
    rows.collect()
}

struct Predicate {
    sql: String,
    values: Vec<Value>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FacetGroup {
    Ages,
    Languages,
    Categories,
    Genres,
    FileTypes,
    Miscellanies,
    Circles,
}

fn build_predicate(query: &CatalogQuery, omitted_group: Option<FacetGroup>) -> Predicate {
    build_scoped_predicate(
        &query.facets,
        query.timeline,
        query.month.as_ref(),
        query.day.as_ref(),
        omitted_group,
    )
}

fn build_scoped_predicate(
    facets: &CatalogFacetFilters,
    timeline: CatalogTimeline,
    month: Option<&dla_application::catalog::CatalogMonth>,
    day: Option<&dla_application::catalog::CatalogDay>,
    omitted_group: Option<FacetGroup>,
) -> Predicate {
    let mut predicate = build_facet_predicate(facets, omitted_group);
    let column = timeline.date_column();
    if let Some(day) = day {
        predicate
            .sql
            .push_str(&format!(" AND {column} >= ? AND {column} < ?"));
        predicate.values.push(Value::Text(day.as_str().to_owned()));
        predicate.values.push(Value::Text(day.end().to_owned()));
    } else if let Some(month) = month {
        predicate
            .sql
            .push_str(&format!(" AND {column} >= ? AND {column} < ?"));
        predicate.values.push(Value::Text(month.start()));
        predicate.values.push(Value::Text(month.end()));
    }
    predicate
}

fn build_facet_predicate(
    facets: &CatalogFacetFilters,
    omitted_group: Option<FacetGroup>,
) -> Predicate {
    let mut conditions = vec!["1 = 1".to_owned()];
    let mut values = Vec::new();

    if omitted_group != Some(FacetGroup::Ages) {
        add_direct_filter(
            &mut conditions,
            &mut values,
            "work.age_rating",
            &facets.ages,
        );
    }
    if omitted_group != Some(FacetGroup::Languages) {
        add_association_filter(
            &mut conditions,
            &mut values,
            "catalog_work_language",
            "catalog_language",
            "language_code",
            "language_code",
            &facets.languages,
        );
    }
    if omitted_group != Some(FacetGroup::Categories) {
        add_association_filter(
            &mut conditions,
            &mut values,
            "catalog_work_category",
            "catalog_category",
            "category_code",
            "category_code",
            &facets.categories,
        );
    }
    if omitted_group != Some(FacetGroup::Genres) {
        add_association_filter(
            &mut conditions,
            &mut values,
            "catalog_work_tag",
            "catalog_tag",
            "tag_id",
            "name",
            &facets.genres,
        );
    }
    if omitted_group != Some(FacetGroup::FileTypes) {
        add_association_filter(
            &mut conditions,
            &mut values,
            "catalog_work_file_format",
            "catalog_file_format",
            "file_format_code",
            "file_format_code",
            &facets.file_types,
        );
    }
    if omitted_group != Some(FacetGroup::Miscellanies) {
        add_association_filter(
            &mut conditions,
            &mut values,
            "catalog_work_miscellany",
            "catalog_miscellany",
            "miscellany_code",
            "miscellany_code",
            &facets.miscellanies,
        );
    }
    if omitted_group != Some(FacetGroup::Circles) {
        add_association_filter(
            &mut conditions,
            &mut values,
            "catalog_work_circle",
            "catalog_circle",
            "circle_id",
            "name",
            &facets.circles,
        );
    }

    Predicate {
        sql: conditions.join(" AND "),
        values,
    }
}

fn add_direct_filter(
    conditions: &mut Vec<String>,
    values: &mut Vec<Value>,
    column: &'static str,
    selection: &dla_application::catalog::CatalogFacetSelection,
) {
    if !selection.include.is_empty() {
        let condition = format!(
            "{column} COLLATE NOCASE IN ({})",
            placeholders(selection.include.len())
        );
        conditions.push(condition);
        values.extend(selection.include.iter().cloned().map(Value::Text));
    }
    if !selection.exclude.is_empty() {
        let condition = format!(
            "{column} COLLATE NOCASE NOT IN ({})",
            placeholders(selection.exclude.len())
        );
        conditions.push(condition);
        values.extend(selection.exclude.iter().cloned().map(Value::Text));
    }
}

fn add_association_filter(
    conditions: &mut Vec<String>,
    values: &mut Vec<Value>,
    join_table: &'static str,
    value_table: &'static str,
    join_key: &'static str,
    filter_key: &'static str,
    selection: &dla_application::catalog::CatalogFacetSelection,
) {
    if !selection.include.is_empty() {
        let condition = association_condition(
            "EXISTS",
            join_table,
            value_table,
            join_key,
            filter_key,
            selection.include.len(),
        );
        conditions.push(condition);
        values.extend(selection.include.iter().cloned().map(Value::Text));
    }
    if !selection.exclude.is_empty() {
        let condition = association_condition(
            "NOT EXISTS",
            join_table,
            value_table,
            join_key,
            filter_key,
            selection.exclude.len(),
        );
        conditions.push(condition);
        values.extend(selection.exclude.iter().cloned().map(Value::Text));
    }
}

fn association_condition(
    operator: &str,
    join_table: &str,
    value_table: &str,
    join_key: &str,
    filter_key: &str,
    value_count: usize,
) -> String {
    format!(
        "{operator} (
            SELECT 1
            FROM {join_table} selected_work_value
            JOIN {value_table} selected_value
              ON selected_value.{join_key} = selected_work_value.{join_key}
            WHERE selected_work_value.work_code = work.work_code
              AND selected_value.{filter_key} COLLATE NOCASE IN ({})
        )",
        placeholders(value_count)
    )
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn sort_expression(sort: &str, timeline: CatalogTimeline, scoped_month: bool) -> String {
    let timeline_column = timeline.date_column();
    match sort {
        "release_asc" if scoped_month => {
            format!("{timeline_column} ASC, work.work_code ASC")
        }
        "release_asc" => format!(
            "work.is_synthetic ASC, {timeline_column} = '', {timeline_column} ASC, work.work_code ASC"
        ),
        "title_asc" => {
            "work.is_synthetic ASC, work.title COLLATE NOCASE ASC, work.work_code ASC".to_owned()
        }
        "title_desc" => {
            "work.is_synthetic ASC, work.title COLLATE NOCASE DESC, work.work_code ASC".to_owned()
        }
        "favorites" => {
            "work.is_synthetic ASC, enrichment.favorites_count IS NULL ASC, enrichment.favorites_count DESC, work.added_date = '' ASC, work.added_date DESC, work.work_code DESC".to_owned()
        }
        "code_asc" => "work.is_synthetic ASC, work.work_code ASC".to_owned(),
        "code_desc" => "work.is_synthetic ASC, work.work_code DESC".to_owned(),
        _ if scoped_month => format!("{timeline_column} DESC, work.work_code DESC"),
        _ => format!(
            "work.is_synthetic ASC, {timeline_column} = '', {timeline_column} DESC, work.work_code DESC"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dla_application::catalog::{
        BrowseRequest, CatalogContextRequest, CatalogFacetFilters, CatalogFacetSelection,
        CatalogService,
    };
    use dla_application::recommendation::{
        CatalogRecommendationLaneKey, CatalogRecommendationService,
    };
    use dla_catalog::{CatalogFixture, CatalogRomContentsFixture};
    use dla_domain::{
        CatalogRating, CatalogRelation, CatalogRom, CatalogRomContents, CatalogRomEntry, Category,
        NamedReference,
    };
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn opens_the_empty_production_catalog() {
        let directory = tempdir().expect("temporary directory");
        let fixture = dla_catalog::empty();
        let store = Arc::new(
            SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
                .expect("catalog store"),
        );
        let service = CatalogService::new(store);

        let page = browse(&service, "", "", "code_asc", "release", 20, 0);
        assert_eq!(page.total, 0);
        assert_eq!(page.snapshot.real_works, 0);
        assert_eq!(page.snapshot.synthetic_works, 0);
    }

    #[test]
    fn imports_and_queries_the_shared_fixture() {
        let directory = tempdir().expect("temporary directory");
        let fixture = production_test_fixture();
        let store = Arc::new(
            SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
                .expect("catalog store"),
        );
        let service = CatalogService::new(store.clone());

        let page = browse(&service, "", "", "code_asc", "release", 60, 0);
        assert_eq!(page.total, 12);
        assert_eq!(page.snapshot.real_works, 12);
        assert_eq!(page.snapshot.synthetic_works, 0);
        assert!(page.categories.len() >= 8);
        assert_eq!(page.categories, page.facets.categories);
        assert_eq!(page.tags, page.facets.genres);
        assert!(page.facets.ages.iter().any(|facet| facet.key == "r18"));
        assert!(!page.facets.languages.is_empty());
        assert!(!page.facets.file_types.is_empty());
        assert!(!page.facets.miscellanies.is_empty());
        assert!(!page.facets.circles.is_empty());
        assert!(!page.has_more);
        assert!(!page.items[0].synthetic);

        assert_eq!(
            browse(&service, "", "タグ11", "code_asc", "release", 20, 0).items[0].code,
            "DLA-SYNTH-0008"
        );

        let text_search_error = service
            .browse(BrowseRequest {
                search: "図書館".to_owned(),
                ..BrowseRequest::default()
            })
            .expect_err("SQLite text search must not run");
        assert!(matches!(
            text_search_error,
            CatalogError::TextSearchRequiresIndex
        ));

        let age_filtered = browse_with_filters(
            &service,
            CatalogFacetFilters {
                ages: CatalogFacetSelection {
                    include: vec!["all_ages".to_owned(), "r15".to_owned()],
                    exclude: Vec::new(),
                },
                ..CatalogFacetFilters::default()
            },
        );
        assert!(age_filtered.total > 0);
        assert!(
            age_filtered
                .items
                .iter()
                .all(|work| { work.age_rating == "all_ages" || work.age_rating == "r15" })
        );

        let exemplar_filters = service.read("DLA-SYNTH-0008").expect("filter exemplar");
        let age = exemplar_filters.work.age_rating.clone();
        let language = exemplar_filters.supported_languages[0].code.clone();
        let category = exemplar_filters.work.categories[0].code.clone();
        let genre = exemplar_filters.work.tags[0].name.clone();
        let file_type = exemplar_filters.file_formats[0].code.clone();
        let miscellany = exemplar_filters.miscellanies[0].code.clone();
        let circle = exemplar_filters.work.circles[0].name.clone();
        let combined = browse_with_filters(
            &service,
            CatalogFacetFilters {
                ages: CatalogFacetSelection {
                    include: vec![age],
                    exclude: Vec::new(),
                },
                languages: CatalogFacetSelection {
                    include: vec![language],
                    exclude: Vec::new(),
                },
                categories: CatalogFacetSelection {
                    include: vec![category.clone()],
                    exclude: Vec::new(),
                },
                genres: CatalogFacetSelection {
                    include: vec![genre.clone()],
                    exclude: Vec::new(),
                },
                file_types: CatalogFacetSelection {
                    include: vec![file_type],
                    exclude: Vec::new(),
                },
                miscellanies: CatalogFacetSelection {
                    include: vec![miscellany],
                    exclude: Vec::new(),
                },
                circles: CatalogFacetSelection {
                    include: vec![circle],
                    exclude: Vec::new(),
                },
            },
        );
        assert!(combined.total > 0);
        assert!(
            combined
                .items
                .iter()
                .any(|work| work.code == "DLA-SYNTH-0008")
        );
        assert!(combined.items.iter().all(|work| {
            work.categories.iter().any(|value| value.code == category)
                && work.tags.iter().any(|value| value.name == genre)
        }));

        let excluded = browse_with_filters(
            &service,
            CatalogFacetFilters {
                genres: CatalogFacetSelection {
                    include: Vec::new(),
                    exclude: vec![genre.clone()],
                },
                ..CatalogFacetFilters::default()
            },
        );
        assert!(excluded.total < page.total);
        assert!(
            excluded
                .items
                .iter()
                .all(|work| work.tags.iter().all(|value| value.name != genre))
        );

        let added = browse(&service, "", "", "release_desc", "added", 60, 0);
        assert!(
            added
                .items
                .windows(2)
                .all(|pair| pair[0].added_date >= pair[1].added_date)
        );
        let updated = browse(&service, "", "", "release_desc", "updated", 60, 0);
        assert!(
            updated
                .items
                .iter()
                .any(|work| !work.updated_date.is_empty())
        );

        {
            let connection = store.connection.lock().expect("catalog connection");
            for (code, favorites, added_date) in [
                ("DLA-SYNTH-0002", 900, "2026-08-02"),
                ("DLA-SYNTH-0003", 900, "2026-08-01"),
                ("DLA-SYNTH-0004", 400, "2026-08-03"),
            ] {
                connection
                    .execute(
                        "INSERT INTO catalog_work_enrichment (work_code, favorites_count)
                         VALUES (?1, ?2)
                         ON CONFLICT(work_code) DO UPDATE SET favorites_count = excluded.favorites_count",
                        params![code, favorites],
                    )
                    .expect("favorites enrichment");
                connection
                    .execute(
                        "UPDATE catalog_work SET added_date = ?2 WHERE work_code = ?1",
                        params![code, added_date],
                    )
                    .expect("favorites tie-break date");
            }
        }
        let favorites = browse(&service, "", "", "favorites", "added", 3, 0);
        assert_eq!(
            favorites
                .items
                .iter()
                .map(|work| work.code.as_str())
                .collect::<Vec<_>>(),
            vec!["DLA-SYNTH-0002", "DLA-SYNTH-0003", "DLA-SYNTH-0004"]
        );

        let continuation = browse(&service, "", "", "code_asc", "release", 6, 6);
        assert_eq!(continuation.items.len(), 6);
        assert!(continuation.categories.is_empty());
        assert!(continuation.tags.is_empty());

        let work = service.read("dla-synth-0008").expect("work detail");
        assert_eq!(work.work.code, "DLA-SYNTH-0008");
        assert!(work.work.tags.len() > 10);
        assert!(!work.work.categories.is_empty());

        let exemplar = service.read("DLA-SYNTH-0008").expect("rich work detail");
        assert_eq!(exemplar.sample_image_urls.len(), 1);
        assert_eq!(exemplar.roms.len(), 1);
        assert_eq!(exemplar.related_works.len(), 1);
        assert_eq!(
            exemplar
                .rating
                .as_ref()
                .and_then(|rating| rating.rating_count),
            Some(42)
        );
        assert_eq!(exemplar.file_formats[0].code, "ZIP");
        assert_eq!(exemplar.supported_languages[0].code, "ENG");
        let contents = service
            .read_rom_contents("DLA-SYNTH-0008", 0)
            .expect("ROM contents");
        assert_eq!(contents.entries.len(), 1);
        assert_eq!(contents.entries[0].path, "game/readme.txt");
    }

    #[test]
    fn scopes_month_navigation_and_daily_density_in_sqlite() {
        let directory = tempdir().expect("temporary directory");
        let fixture = production_test_fixture();
        let store = Arc::new(
            SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
                .expect("catalog store"),
        );
        let service = CatalogService::new(store.clone());
        let context = service
            .context(CatalogContextRequest {
                timeline: "release".to_owned(),
                ..CatalogContextRequest::default()
            })
            .expect("catalog context");

        assert!(!context.months.is_empty());
        assert_eq!(
            context.default_month,
            context.months.last().expect("latest month").month
        );
        assert_eq!(
            context.min_month,
            context.months.first().expect("earliest month").month
        );
        assert_eq!(context.max_month, context.default_month);
        assert!(!context.facets.categories.is_empty());

        let page = service
            .browse(BrowseRequest {
                sort: "release_desc".to_owned(),
                timeline: "release".to_owned(),
                month: context.default_month.clone(),
                limit: 24,
                ..BrowseRequest::default()
            })
            .expect("monthly page");
        let scoped_total = page
            .day_buckets
            .iter()
            .map(|bucket| bucket.count)
            .sum::<usize>();

        assert_eq!(page.items.len(), page.total.min(24));
        assert_eq!(page.total, scoped_total);
        assert_eq!(page.day_buckets.len(), 31);
        assert!(page.facets.categories.is_empty());
        assert!(
            page.items
                .iter()
                .all(|work| work.release_date.starts_with(&context.default_month))
        );
        assert!(page.unfiltered_total >= page.total);

        let populated_day = page
            .day_buckets
            .iter()
            .find(|bucket| bucket.count > 0)
            .expect("populated day");
        let day = service
            .browse(BrowseRequest {
                sort: "release_desc".to_owned(),
                timeline: "release".to_owned(),
                month: context.default_month.clone(),
                day: populated_day.day.clone(),
                limit: 12,
                ..BrowseRequest::default()
            })
            .expect("daily page");
        assert_eq!(day.total, populated_day.count);
        assert!(day.day_buckets.is_empty());
        assert!(
            day.items
                .iter()
                .all(|work| work.release_date.starts_with(&populated_day.day))
        );

        let connection = store.connection.lock().expect("catalog connection");
        let plan = query_plan(
            &connection,
            &format!(
                "SELECT work_code FROM catalog_work work
                 WHERE release_date >= '{}-01' AND release_date < '{}-32'
                 ORDER BY work.release_date DESC, work.work_code DESC
                 LIMIT 24",
                context.default_month, context.default_month
            ),
        );
        assert!(plan.contains("catalog_work_release_month_browse"), "{plan}");
    }

    #[test]
    fn returns_no_stale_month_when_filters_have_no_matches() {
        let directory = tempdir().expect("temporary directory");
        let fixture = production_test_fixture();
        let store = Arc::new(
            SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
                .expect("catalog store"),
        );
        let service = CatalogService::new(store);
        let context = service
            .context(CatalogContextRequest {
                facets: CatalogFacetFilters {
                    circles: CatalogFacetSelection {
                        include: vec!["circle-that-does-not-exist".to_owned()],
                        exclude: Vec::new(),
                    },
                    ..CatalogFacetFilters::default()
                },
                timeline: "release".to_owned(),
                ..CatalogContextRequest::default()
            })
            .expect("empty context");

        assert!(context.months.is_empty());
        assert!(!context.min_month.is_empty());
        assert!(!context.max_month.is_empty());
        assert_eq!(context.default_month, context.max_month);
    }

    #[test]
    fn replaces_a_changed_snapshot_atomically() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("catalog.sqlite");
        let fixture = production_test_fixture();
        drop(SqliteCatalogStore::open(&path, &fixture).expect("first catalog store"));

        let mut replacement = fixture.clone();
        replacement.snapshot_id.push_str("-replacement");
        replacement.works.truncate(1);
        replacement.relations.clear();
        replacement.rom_contents.clear();
        let store =
            Arc::new(SqliteCatalogStore::open(&path, &replacement).expect("replacement store"));
        let service = CatalogService::new(store);
        let page = browse(&service, "", "", "code_asc", "release", 10, 0);
        assert_eq!(page.total, 1);
        assert_eq!(page.snapshot.id, replacement.snapshot_id);
    }

    #[test]
    fn resolves_catalog_identity_without_search_index_ownership() {
        let directory = tempdir().expect("temporary directory");
        let fixture = production_test_fixture();
        let store = SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
            .expect("catalog store");
        let hash =
            ArchiveHash::parse("AF752B95D170411F60FD279016C06877879C6DD5D7F9F9152FE584EE8EA5F557")
                .expect("fixture archive hash");

        let exact = store.resolve_archive_hash(&hash).expect("exact identity");
        let candidates = store
            .find_archive_candidates_by_size("12", 16)
            .expect("size candidates");

        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].code, "DLA-SYNTH-0008");
        assert!(candidates.iter().any(|candidate| {
            candidate.work_code == "DLA-SYNTH-0008"
                && candidate.sha256
                    == "af752b95d170411f60fd279016c06877879c6dd5d7f9f9152fe584ee8ea5f557"
        }));

        let connection = store.connection.lock().expect("catalog connection");
        let size_plan = query_plan(
            &connection,
            "SELECT work_code, position, name, size, md5, sha1, sha256
             FROM catalog_rom
             WHERE size = '12'
             ORDER BY work_code COLLATE NOCASE, position
             LIMIT 16",
        );
        let hash_plan = query_plan(
            &connection,
            "SELECT DISTINCT work_code
             FROM catalog_rom
             WHERE sha256 = 'af752b95d170411f60fd279016c06877879c6dd5d7f9f9152fe584ee8ea5f557' COLLATE NOCASE
             ORDER BY work_code COLLATE NOCASE",
        );
        assert!(size_plan.contains("catalog_rom_size_identity"));
        assert!(hash_plan.contains("catalog_rom_sha256_identity"));
    }

    #[test]
    fn builds_bounded_explainable_recommendations_with_indexed_candidates() {
        let directory = tempdir().expect("temporary directory");
        let fixture = production_test_fixture();
        let store = Arc::new(
            SqliteCatalogStore::open(&directory.path().join("catalog.sqlite"), &fixture)
                .expect("catalog store"),
        );
        let service = CatalogRecommendationService::new(store.clone());

        let recommendations = service.read("DLA-SYNTH-0008").expect("recommendations");

        let same_circle = recommendations
            .lanes
            .iter()
            .find(|lane| lane.key == CatalogRecommendationLaneKey::SameCircle)
            .expect("same-circle lane");
        let similar = recommendations
            .lanes
            .iter()
            .find(|lane| lane.key == CatalogRecommendationLaneKey::Similar)
            .expect("similar lane");
        assert!(!same_circle.items.is_empty());
        assert!(!similar.items.is_empty());
        assert!(same_circle.items.len() <= 12);
        assert!(similar.items.len() <= 12);
        let detail = store
            .read("DLA-SYNTH-0008")
            .expect("catalog detail")
            .expect("catalog work");
        let related = detail
            .related_works
            .iter()
            .map(|work| work.code.to_lowercase())
            .collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        for item in same_circle.items.iter().chain(&similar.items) {
            assert_ne!(item.work.code, "DLA-SYNTH-0008");
            assert!(!related.contains(&item.work.code.to_lowercase()));
            assert!(seen.insert(item.work.code.to_lowercase()));
            assert!(!item.reasons.is_empty());
        }

        let connection = store.connection.lock().expect("catalog connection");
        let same_circle_plan = recommendation_query_plan(
            &connection,
            SAME_CIRCLE_RECOMMENDATION_SQL,
            "DLA-SYNTH-0008",
            48,
        );
        let similar_plan = recommendation_query_plan(
            &connection,
            SIMILAR_RECOMMENDATION_SQL,
            "DLA-SYNTH-0008",
            160,
        );
        assert!(same_circle_plan.contains("catalog_work_circle_circle"));
        for index in [
            "catalog_work_tag_tag",
            "catalog_work_category_category",
            "catalog_work_miscellany_value",
            "catalog_work_file_format_value",
            "catalog_work_language_value",
        ] {
            assert!(
                similar_plan.contains(index),
                "missing {index}: {similar_plan}"
            );
        }
    }

    fn production_test_fixture() -> CatalogFixture {
        let mut fixture = dla_catalog::load_test_fixture().expect("fixture");
        for detail in &mut fixture.works {
            detail.work.synthetic = false;
        }

        let rich = fixture
            .works
            .iter_mut()
            .find(|detail| detail.work.code == "DLA-SYNTH-0008")
            .expect("rich synthetic work");
        rich.work.main_image_urls = vec!["https://example.invalid/main.webp".to_owned()];
        rich.sample_image_urls = vec!["https://example.invalid/sample.webp".to_owned()];
        rich.file_formats = vec![category("ZIP", "ZIP archive")];
        rich.supported_languages = vec![category("ENG", "English")];
        rich.miscellanies = vec![category("VOICE", "Voice included")];
        rich.roms = vec![CatalogRom {
            name: "synthetic-payload.zip".to_owned(),
            size: "12".to_owned(),
            crc: String::new(),
            md5: String::new(),
            sha1: String::new(),
            sha256: "af752b95d170411f60fd279016c06877879c6dd5d7f9f9152fe584ee8ea5f557".to_owned(),
            file_count: Some(1),
            update_date: "2026-08-01".to_owned(),
            version: "1".to_owned(),
        }];
        rich.rating = Some(CatalogRating {
            score: 4.5,
            rating_count: Some(42),
            total_sales: Some(128),
            rankings: Vec::new(),
        });

        let recommendation_candidate = fixture
            .works
            .iter_mut()
            .find(|detail| detail.work.code == "DLA-SYNTH-0011")
            .expect("recommendation candidate");
        recommendation_candidate.work.circles.push(NamedReference {
            name: "Fixture Circle".to_owned(),
            name_english: "Fixture Circle".to_owned(),
        });
        recommendation_candidate.work.tags.push(NamedReference {
            name: "タグ12".to_owned(),
            name_english: "Tag 12".to_owned(),
        });
        let similar_candidate = fixture
            .works
            .iter_mut()
            .find(|detail| detail.work.code == "DLA-SYNTH-0002")
            .expect("similar recommendation candidate");
        similar_candidate.work.tags.push(NamedReference {
            name: "タグ12".to_owned(),
            name_english: "Tag 12".to_owned(),
        });

        fixture.relations.push(CatalogRelation {
            parent_work_code: "DLA-SYNTH-0008".to_owned(),
            child_work_code: "DLA-SYNTH-0010".to_owned(),
            relation_type_code: "fixture-related".to_owned(),
            relation_type_label: "Fixture related".to_owned(),
        });
        fixture.rom_contents.push(CatalogRomContentsFixture {
            work_code: "DLA-SYNTH-0008".to_owned(),
            rom_position: 0,
            contents: CatalogRomContents {
                status: "complete".to_owned(),
                archive_format: "zip".to_owned(),
                entry_count: Some(1),
                total_uncompressed_size: Some("12".to_owned()),
                truncated: false,
                entries: vec![CatalogRomEntry {
                    entry_index: 0,
                    path: "game/readme.txt".to_owned(),
                    extension: "txt".to_owned(),
                    is_directory: false,
                    size: Some("12".to_owned()),
                    crc32: String::new(),
                    md5: String::new(),
                    sha1: String::new(),
                    sha256: String::new(),
                    hash_status: "not_requested".to_owned(),
                }],
            },
        });
        fixture
    }

    fn category(code: &str, name: &str) -> Category {
        Category {
            code: code.to_owned(),
            name: name.to_owned(),
            name_english: name.to_owned(),
        }
    }

    fn query_plan(connection: &Connection, sql: &str) -> String {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("query plan");
        statement
            .query_map([], |row| row.get::<_, String>(3))
            .expect("query plan rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("query plan details")
            .join("\n")
    }

    fn recommendation_query_plan(
        connection: &Connection,
        sql: &str,
        work_code: &str,
        limit: i64,
    ) -> String {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("recommendation query plan");
        statement
            .query_map(params![work_code, limit], |row| row.get::<_, String>(3))
            .expect("recommendation query plan rows")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("recommendation query plan details")
            .join("\n")
    }

    fn browse(
        service: &CatalogService,
        search: &str,
        tag: &str,
        sort: &str,
        timeline: &str,
        limit: usize,
        offset: usize,
    ) -> CatalogPage {
        service
            .browse(BrowseRequest {
                search: search.to_owned(),
                category: String::new(),
                tag: tag.to_owned(),
                facets: CatalogFacetFilters::default(),
                sort: sort.to_owned(),
                timeline: timeline.to_owned(),
                month: String::new(),
                day: String::new(),
                limit,
                offset,
            })
            .expect("browse")
    }

    fn browse_with_filters(service: &CatalogService, facets: CatalogFacetFilters) -> CatalogPage {
        service
            .browse(BrowseRequest {
                search: String::new(),
                category: String::new(),
                tag: String::new(),
                facets,
                sort: "code_asc".to_owned(),
                timeline: "release".to_owned(),
                month: String::new(),
                day: String::new(),
                limit: 120,
                offset: 0,
            })
            .expect("filtered browse")
    }
}
