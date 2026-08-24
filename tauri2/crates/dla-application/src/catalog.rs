use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use dla_domain::{CatalogRomContents, CatalogWork, CatalogWorkDetail};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEFAULT_LIMIT: usize = 60;
const MAXIMUM_LIMIT: usize = 120;
const DEFAULT_SORT: &str = "release_desc";
const DEFAULT_TIMELINE: &str = "added";
const MAXIMUM_FACET_VALUES: usize = 128;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct BrowseRequest {
    pub search: String,
    pub category: String,
    pub tag: String,
    pub facets: CatalogFacetFilters,
    pub sort: String,
    pub timeline: String,
    pub month: String,
    pub day: String,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogContextRequest {
    pub category: String,
    pub tag: String,
    pub facets: CatalogFacetFilters,
    pub timeline: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogFacetFilters {
    pub ages: CatalogFacetSelection,
    pub languages: CatalogFacetSelection,
    pub categories: CatalogFacetSelection,
    pub genres: CatalogFacetSelection,
    pub file_types: CatalogFacetSelection,
    pub miscellanies: CatalogFacetSelection,
    pub circles: CatalogFacetSelection,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogFacetSelection {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogQuery {
    pub search: String,
    pub facets: CatalogFacetFilters,
    pub sort: String,
    pub timeline: CatalogTimeline,
    pub month: Option<CatalogMonth>,
    pub day: Option<CatalogDay>,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogContextQuery {
    pub facets: CatalogFacetFilters,
    pub timeline: CatalogTimeline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogTimeline {
    Added,
    Release,
    Updated,
}

impl CatalogTimeline {
    pub fn date_column(self) -> &'static str {
        match self {
            Self::Added => "work.added_date",
            Self::Release => "work.release_date",
            Self::Updated => "work.updated_date",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogMonth {
    value: String,
    next: String,
    days: u8,
}

impl CatalogMonth {
    pub fn parse(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 7 || bytes[4] != b'-' {
            return None;
        }
        let year = value[..4].parse::<u16>().ok()?;
        let month = value[5..].parse::<u8>().ok()?;
        if year == 0 || year >= 9999 || !(1..=12).contains(&month) {
            return None;
        }
        let (next_year, next_month) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };
        Some(Self {
            value: value.to_owned(),
            next: format!("{next_year:04}-{next_month:02}"),
            days: days_in_month(year, month),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn start(&self) -> String {
        format!("{}-01", self.value)
    }

    pub fn end(&self) -> String {
        format!("{}-01", self.next)
    }

    pub fn days(&self) -> u8 {
        self.days
    }

    pub fn day(&self, day: u8) -> String {
        format!("{}-{day:02}", self.value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogDay {
    value: String,
    next: String,
}

impl CatalogDay {
    pub fn parse(value: &str, month: &CatalogMonth) -> Option<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || &value[..7] != month.as_str()
        {
            return None;
        }
        let day = value[8..].parse::<u8>().ok()?;
        if day == 0 || day > month.days() {
            return None;
        }
        let next = if day == month.days() {
            month.end()
        } else {
            month.day(day + 1)
        };
        Some(Self {
            value: value.to_owned(),
            next,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn end(&self) -> &str {
        &self.next
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFacet {
    pub key: String,
    pub label: String,
    pub label_english: String,
    pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSnapshot {
    pub id: String,
    pub real_works: usize,
    pub synthetic_works: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogMonthBucket {
    pub month: String,
    pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDayBucket {
    pub day: String,
    pub count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogContext {
    pub min_month: String,
    pub max_month: String,
    pub default_month: String,
    pub months: Vec<CatalogMonthBucket>,
    pub facets: CatalogFacets,
    pub snapshot: CatalogSnapshot,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFacets {
    pub ages: Vec<CatalogFacet>,
    pub languages: Vec<CatalogFacet>,
    pub categories: Vec<CatalogFacet>,
    pub genres: Vec<CatalogFacet>,
    pub file_types: Vec<CatalogFacet>,
    pub miscellanies: Vec<CatalogFacet>,
    pub circles: Vec<CatalogFacet>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPage {
    pub items: Vec<CatalogWork>,
    pub total: usize,
    pub unfiltered_total: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
    pub categories: Vec<CatalogFacet>,
    pub tags: Vec<CatalogFacet>,
    pub facets: CatalogFacets,
    pub day_buckets: Vec<CatalogDayBucket>,
    pub snapshot: CatalogSnapshot,
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog work not found: {0}")]
    NotFound(String),
    #[error("catalog text search is available only through the search index")]
    TextSearchRequiresIndex,
    #[error("invalid catalog date scope: {0}")]
    InvalidDateScope(String),
    #[error("catalog persistence failed: {0}")]
    Persistence(String),
}

impl CatalogError {
    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait CatalogReader: Send + Sync {
    fn browse(&self, query: &CatalogQuery) -> Result<CatalogPage, CatalogError>;
    fn context(&self, query: &CatalogContextQuery) -> Result<CatalogContext, CatalogError>;
    fn read(&self, code: &str) -> Result<Option<CatalogWorkDetail>, CatalogError>;
    fn read_works(&self, codes: &[String]) -> Result<Vec<CatalogWork>, CatalogError> {
        let mut works = Vec::with_capacity(codes.len());
        for code in codes {
            if let Some(detail) = self.read(code)? {
                works.push(detail.work);
            }
        }
        Ok(works)
    }
    fn read_rom_contents(
        &self,
        work_code: &str,
        rom_position: usize,
    ) -> Result<Option<CatalogRomContents>, CatalogError>;
}

pub struct CatalogService {
    reader: Arc<dyn CatalogReader>,
}

impl CatalogService {
    pub fn new(reader: Arc<dyn CatalogReader>) -> Self {
        Self { reader }
    }

    pub fn browse(&self, request: BrowseRequest) -> Result<CatalogPage, CatalogError> {
        self.reader.browse(&normalize(request)?)
    }

    pub fn context(&self, request: CatalogContextRequest) -> Result<CatalogContext, CatalogError> {
        self.reader.context(&normalize_context(request))
    }

    pub fn read(&self, code: &str) -> Result<CatalogWorkDetail, CatalogError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(CatalogError::NotFound(code.to_owned()));
        }
        self.reader
            .read(code)?
            .ok_or_else(|| CatalogError::NotFound(code.to_owned()))
    }

    pub fn read_works(&self, codes: &[String]) -> Result<Vec<CatalogWork>, CatalogError> {
        let mut seen = HashSet::new();
        let mut normalized = Vec::with_capacity(codes.len());
        for code in codes {
            let code = code.trim();
            let key = code.to_ascii_uppercase();
            if code.is_empty() || !seen.insert(key) {
                continue;
            }
            normalized.push(code.to_owned());
        }
        let mut works = self
            .reader
            .read_works(&normalized)?
            .into_iter()
            .map(|work| (work.code.to_ascii_uppercase(), work))
            .collect::<HashMap<_, _>>();
        Ok(normalized
            .into_iter()
            .filter_map(|code| works.remove(&code.to_ascii_uppercase()))
            .collect())
    }

    pub fn read_rom_contents(
        &self,
        work_code: &str,
        rom_position: usize,
    ) -> Result<CatalogRomContents, CatalogError> {
        let work_code = work_code.trim();
        if work_code.is_empty() {
            return Err(CatalogError::NotFound(work_code.to_owned()));
        }
        self.reader
            .read_rom_contents(work_code, rom_position)?
            .ok_or_else(|| CatalogError::NotFound(format!("{work_code} ROM {rom_position}")))
    }
}

fn normalize(request: BrowseRequest) -> Result<CatalogQuery, CatalogError> {
    let limit = match request.limit {
        0 => DEFAULT_LIMIT,
        value => value.min(MAXIMUM_LIMIT),
    };
    let sort = match request.sort.as_str() {
        "release_asc" | "release_desc" | "title_asc" | "title_desc" | "favorites" | "code_asc"
        | "code_desc" => request.sort,
        _ => DEFAULT_SORT.to_owned(),
    };
    let timeline = normalize_timeline(&request.timeline);

    let mut facets = normalize_facet_filters(request.facets);
    push_unique(&mut facets.categories.include, request.category.trim());
    push_unique(&mut facets.genres.include, request.tag.trim());

    let month = if request.month.trim().is_empty() {
        None
    } else {
        Some(
            CatalogMonth::parse(request.month.trim())
                .ok_or_else(|| CatalogError::InvalidDateScope(request.month.trim().to_owned()))?,
        )
    };
    let day = if request.day.trim().is_empty() {
        None
    } else {
        let selected_month = month
            .as_ref()
            .ok_or_else(|| CatalogError::InvalidDateScope(request.day.trim().to_owned()))?;
        Some(
            CatalogDay::parse(request.day.trim(), selected_month)
                .ok_or_else(|| CatalogError::InvalidDateScope(request.day.trim().to_owned()))?,
        )
    };

    Ok(CatalogQuery {
        search: request.search.trim().to_owned(),
        facets,
        sort,
        timeline,
        month,
        day,
        limit,
        offset: request.offset,
    })
}

fn normalize_context(request: CatalogContextRequest) -> CatalogContextQuery {
    let mut facets = normalize_facet_filters(request.facets);
    push_unique(&mut facets.categories.include, request.category.trim());
    push_unique(&mut facets.genres.include, request.tag.trim());
    CatalogContextQuery {
        facets,
        timeline: normalize_timeline(&request.timeline),
    }
}

fn normalize_timeline(timeline: &str) -> CatalogTimeline {
    match timeline {
        DEFAULT_TIMELINE => CatalogTimeline::Added,
        "release" => CatalogTimeline::Release,
        "updated" => CatalogTimeline::Updated,
        _ => CatalogTimeline::Added,
    }
}

pub(crate) fn normalize_facet_filters(filters: CatalogFacetFilters) -> CatalogFacetFilters {
    CatalogFacetFilters {
        ages: normalize_selection(filters.ages),
        languages: normalize_selection(filters.languages),
        categories: normalize_selection(filters.categories),
        genres: normalize_selection(filters.genres),
        file_types: normalize_selection(filters.file_types),
        miscellanies: normalize_selection(filters.miscellanies),
        circles: normalize_selection(filters.circles),
    }
}

fn normalize_selection(selection: CatalogFacetSelection) -> CatalogFacetSelection {
    let mut include = Vec::new();
    for value in selection.include {
        push_unique(&mut include, value.trim());
    }
    let mut exclude = Vec::new();
    for value in selection.exclude {
        let value = value.trim();
        if !contains_ascii_case(&include, value) {
            push_unique(&mut exclude, value);
        }
    }
    CatalogFacetSelection { include, exclude }
}

fn push_unique(values: &mut Vec<String>, value: &str) {
    if value.is_empty()
        || values.len() >= MAXIMUM_FACET_VALUES
        || contains_ascii_case(values, value)
    {
        return;
    }
    values.push(value.to_owned());
}

fn contains_ascii_case(values: &[String], value: &str) -> bool {
    values
        .iter()
        .any(|current| current.eq_ignore_ascii_case(value))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct RecordingReader {
        query: Mutex<Option<CatalogQuery>>,
        work: Option<CatalogWorkDetail>,
    }

    impl CatalogReader for RecordingReader {
        fn browse(&self, query: &CatalogQuery) -> Result<CatalogPage, CatalogError> {
            self.query
                .lock()
                .expect("query lock")
                .replace(query.clone());
            Ok(CatalogPage {
                items: Vec::new(),
                total: 0,
                unfiltered_total: 0,
                limit: query.limit,
                offset: query.offset,
                has_more: false,
                categories: Vec::new(),
                tags: Vec::new(),
                facets: CatalogFacets::default(),
                day_buckets: Vec::new(),
                snapshot: CatalogSnapshot {
                    id: "test".to_owned(),
                    real_works: 0,
                    synthetic_works: 0,
                },
            })
        }

        fn context(&self, _query: &CatalogContextQuery) -> Result<CatalogContext, CatalogError> {
            Ok(CatalogContext {
                min_month: String::new(),
                max_month: String::new(),
                default_month: String::new(),
                months: Vec::new(),
                facets: CatalogFacets::default(),
                snapshot: CatalogSnapshot {
                    id: "test".to_owned(),
                    real_works: 0,
                    synthetic_works: 0,
                },
            })
        }

        fn read(&self, _code: &str) -> Result<Option<CatalogWorkDetail>, CatalogError> {
            Ok(self.work.clone())
        }

        fn read_works(&self, codes: &[String]) -> Result<Vec<CatalogWork>, CatalogError> {
            Ok(self
                .work
                .as_ref()
                .filter(|detail| {
                    codes
                        .iter()
                        .any(|code| code.eq_ignore_ascii_case(&detail.work.code))
                })
                .map(|detail| vec![detail.work.clone()])
                .unwrap_or_default())
        }

        fn read_rom_contents(
            &self,
            _work_code: &str,
            _rom_position: usize,
        ) -> Result<Option<CatalogRomContents>, CatalogError> {
            Ok(None)
        }
    }

    #[test]
    fn normalizes_the_command_request_before_using_the_reader() {
        let reader = Arc::new(RecordingReader {
            query: Mutex::new(None),
            work: None,
        });
        let service = CatalogService::new(reader.clone());

        service
            .browse(BrowseRequest {
                search: "  図書館  ".to_owned(),
                category: " SOU ".to_owned(),
                tag: " 睡眠 ".to_owned(),
                facets: CatalogFacetFilters {
                    ages: CatalogFacetSelection {
                        include: vec![" r18 ".to_owned(), "R18".to_owned()],
                        exclude: vec![" r15 ".to_owned(), "r18".to_owned()],
                    },
                    ..CatalogFacetFilters::default()
                },
                sort: "random".to_owned(),
                timeline: "unknown".to_owned(),
                month: "2026-02".to_owned(),
                day: "2026-02-28".to_owned(),
                limit: 500,
                offset: 9,
            })
            .expect("browse result");

        assert_eq!(
            reader.query.lock().expect("query lock").as_ref(),
            Some(&CatalogQuery {
                search: "図書館".to_owned(),
                facets: CatalogFacetFilters {
                    ages: CatalogFacetSelection {
                        include: vec!["r18".to_owned()],
                        exclude: vec!["r15".to_owned()],
                    },
                    categories: CatalogFacetSelection {
                        include: vec!["SOU".to_owned()],
                        exclude: Vec::new(),
                    },
                    genres: CatalogFacetSelection {
                        include: vec!["睡眠".to_owned()],
                        exclude: Vec::new(),
                    },
                    ..CatalogFacetFilters::default()
                },
                sort: "release_desc".to_owned(),
                timeline: CatalogTimeline::Added,
                month: CatalogMonth::parse("2026-02"),
                day: CatalogDay::parse(
                    "2026-02-28",
                    &CatalogMonth::parse("2026-02").expect("month"),
                ),
                limit: 120,
                offset: 9,
            })
        );
    }

    #[test]
    fn validates_month_and_day_scopes_before_using_the_reader() {
        let reader = Arc::new(RecordingReader {
            query: Mutex::new(None),
            work: None,
        });
        let service = CatalogService::new(reader);

        let invalid_month = service
            .browse(BrowseRequest {
                month: "2026-13".to_owned(),
                ..BrowseRequest::default()
            })
            .expect_err("invalid month");
        let mismatched_day = service
            .browse(BrowseRequest {
                month: "2026-02".to_owned(),
                day: "2026-03-01".to_owned(),
                ..BrowseRequest::default()
            })
            .expect_err("mismatched day");

        assert!(matches!(invalid_month, CatalogError::InvalidDateScope(_)));
        assert!(matches!(mismatched_day, CatalogError::InvalidDateScope(_)));
        assert_eq!(
            CatalogMonth::parse("2024-02").expect("leap month").days(),
            29
        );
        assert_eq!(
            CatalogMonth::parse("2100-02")
                .expect("century month")
                .days(),
            28
        );
    }

    #[test]
    fn accepts_catalog_favorites_sort() {
        let query = normalize(BrowseRequest {
            sort: "favorites".to_owned(),
            ..BrowseRequest::default()
        })
        .expect("normalized favorites query");

        assert_eq!(query.sort, "favorites");
        assert_eq!(query.timeline, CatalogTimeline::Added);
    }

    #[test]
    fn reports_a_missing_work_without_exposing_persistence_details() {
        let reader = Arc::new(RecordingReader {
            query: Mutex::new(None),
            work: None,
        });
        let service = CatalogService::new(reader);

        let error = service.read(" UNKNOWN ").expect_err("missing work");

        assert!(matches!(error, CatalogError::NotFound(code) if code == "UNKNOWN"));
    }
}
