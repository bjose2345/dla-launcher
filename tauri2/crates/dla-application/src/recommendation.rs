use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use dla_domain::{CatalogWork, CatalogWorkDetail, Category, NamedReference};
use serde::Serialize;

use crate::catalog::CatalogError;

const SAME_CIRCLE_CANDIDATE_LIMIT: usize = 48;
const SIMILAR_CANDIDATE_LIMIT: usize = 160;
const LANE_LIMIT: usize = 12;
const MAXIMUM_REASONS: usize = 4;
const MINIMUM_SIMILAR_SCORE: u32 = 400;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogRecommendationLaneKey {
    SameCircle,
    Similar,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogRecommendationReasonKind {
    SameCircle,
    SharedTag,
    SharedCategory,
    SharedMiscellany,
    SharedFileFormat,
    SharedLanguage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecommendationReason {
    pub kind: CatalogRecommendationReasonKind,
    pub key: String,
    pub label: String,
    pub label_english: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecommendationItem {
    pub work: CatalogWork,
    pub score: u32,
    pub reasons: Vec<CatalogRecommendationReason>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecommendationLane {
    pub key: CatalogRecommendationLaneKey,
    pub items: Vec<CatalogRecommendationItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRecommendations {
    pub anchor_work_code: String,
    pub lanes: Vec<CatalogRecommendationLane>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecommendationCandidate {
    pub work: CatalogWork,
    pub file_formats: Vec<Category>,
    pub supported_languages: Vec<Category>,
    pub miscellanies: Vec<Category>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecommendationFacetFrequency {
    pub key: String,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecommendationCandidatePool {
    pub anchor: CatalogWorkDetail,
    pub same_circle: Vec<RecommendationCandidate>,
    pub similar: Vec<RecommendationCandidate>,
    pub tag_frequencies: Vec<RecommendationFacetFrequency>,
    pub catalog_size: usize,
}

pub trait CatalogRecommendationReader: Send + Sync {
    fn read_recommendation_candidates(
        &self,
        work_code: &str,
        same_circle_limit: usize,
        similar_limit: usize,
    ) -> Result<Option<RecommendationCandidatePool>, CatalogError>;
}

pub struct CatalogRecommendationService {
    reader: Arc<dyn CatalogRecommendationReader>,
}

impl CatalogRecommendationService {
    pub fn new(reader: Arc<dyn CatalogRecommendationReader>) -> Self {
        Self { reader }
    }

    pub fn read(&self, work_code: &str) -> Result<CatalogRecommendations, CatalogError> {
        let work_code = work_code.trim();
        if work_code.is_empty() {
            return Err(CatalogError::NotFound(work_code.to_owned()));
        }
        let pool = self
            .reader
            .read_recommendation_candidates(
                work_code,
                SAME_CIRCLE_CANDIDATE_LIMIT,
                SIMILAR_CANDIDATE_LIMIT,
            )?
            .ok_or_else(|| CatalogError::NotFound(work_code.to_owned()))?;
        Ok(rank_recommendations(pool))
    }
}

#[derive(Clone)]
struct WeightedReason {
    weight: u32,
    reason: CatalogRecommendationReason,
}

fn rank_recommendations(pool: RecommendationCandidatePool) -> CatalogRecommendations {
    let anchor_code = canonical(&pool.anchor.work.code);
    let related_codes = pool
        .anchor
        .related_works
        .iter()
        .map(|work| canonical(&work.code))
        .collect::<HashSet<_>>();
    let anchor_circle_keys = named_keys(&pool.anchor.work.circles);
    let tag_frequencies = pool
        .tag_frequencies
        .iter()
        .map(|frequency| (canonical(&frequency.key), frequency.count))
        .collect::<HashMap<_, _>>();

    let mut same_circle = pool
        .same_circle
        .into_iter()
        .filter(|candidate| candidate_is_available(candidate, &anchor_code, &related_codes))
        .filter_map(|candidate| {
            let shared = shared_named(&pool.anchor.work.circles, &candidate.work.circles);
            let reason = shared.into_iter().next()?;
            Some(CatalogRecommendationItem {
                work: candidate.work,
                score: 1_000,
                reasons: vec![CatalogRecommendationReason {
                    kind: CatalogRecommendationReasonKind::SameCircle,
                    key: reason.name.clone(),
                    label: reason.name,
                    label_english: reason.name_english,
                }],
            })
        })
        .collect::<Vec<_>>();
    sort_items(&mut same_circle);
    deduplicate_items(&mut same_circle);
    same_circle.truncate(LANE_LIMIT);

    let same_circle_codes = same_circle
        .iter()
        .map(|item| canonical(&item.work.code))
        .collect::<HashSet<_>>();
    let mut similar = pool
        .similar
        .into_iter()
        .filter(|candidate| candidate_is_available(candidate, &anchor_code, &related_codes))
        .filter(|candidate| !same_circle_codes.contains(&canonical(&candidate.work.code)))
        .filter(|candidate| {
            candidate
                .work
                .circles
                .iter()
                .all(|circle| !anchor_circle_keys.contains(&canonical(&circle.name)))
        })
        .filter_map(|candidate| {
            score_candidate(&pool.anchor, candidate, &tag_frequencies, pool.catalog_size)
        })
        .collect::<Vec<_>>();
    sort_items(&mut similar);
    deduplicate_items(&mut similar);
    similar.truncate(LANE_LIMIT);

    let mut lanes = Vec::new();
    if !same_circle.is_empty() {
        lanes.push(CatalogRecommendationLane {
            key: CatalogRecommendationLaneKey::SameCircle,
            items: same_circle,
        });
    }
    if !similar.is_empty() {
        lanes.push(CatalogRecommendationLane {
            key: CatalogRecommendationLaneKey::Similar,
            items: similar,
        });
    }
    CatalogRecommendations {
        anchor_work_code: pool.anchor.work.code,
        lanes,
    }
}

fn candidate_is_available(
    candidate: &RecommendationCandidate,
    anchor_code: &str,
    related_codes: &HashSet<String>,
) -> bool {
    let code = canonical(&candidate.work.code);
    code != anchor_code && !related_codes.contains(&code)
}

fn score_candidate(
    anchor: &CatalogWorkDetail,
    candidate: RecommendationCandidate,
    tag_frequencies: &HashMap<String, usize>,
    catalog_size: usize,
) -> Option<CatalogRecommendationItem> {
    let mut weighted_reasons = Vec::new();

    for tag in shared_named(&anchor.work.tags, &candidate.work.tags) {
        let weight = tag_weight(
            tag_frequencies
                .get(&canonical(&tag.name))
                .copied()
                .unwrap_or(catalog_size),
            catalog_size,
        );
        weighted_reasons.push(WeightedReason {
            weight,
            reason: CatalogRecommendationReason {
                kind: CatalogRecommendationReasonKind::SharedTag,
                key: tag.name.clone(),
                label: tag.name,
                label_english: tag.name_english,
            },
        });
    }
    append_category_reasons(
        &mut weighted_reasons,
        &anchor.work.categories,
        &candidate.work.categories,
        500,
        CatalogRecommendationReasonKind::SharedCategory,
    );
    append_category_reasons(
        &mut weighted_reasons,
        &anchor.miscellanies,
        &candidate.miscellanies,
        200,
        CatalogRecommendationReasonKind::SharedMiscellany,
    );
    append_category_reasons(
        &mut weighted_reasons,
        &anchor.file_formats,
        &candidate.file_formats,
        150,
        CatalogRecommendationReasonKind::SharedFileFormat,
    );
    append_category_reasons(
        &mut weighted_reasons,
        &anchor.supported_languages,
        &candidate.supported_languages,
        100,
        CatalogRecommendationReasonKind::SharedLanguage,
    );

    let score = weighted_reasons
        .iter()
        .fold(0_u32, |total, reason| total.saturating_add(reason.weight));
    if score < MINIMUM_SIMILAR_SCORE {
        return None;
    }
    weighted_reasons.sort_by(|left, right| {
        right
            .weight
            .cmp(&left.weight)
            .then_with(|| left.reason.label.cmp(&right.reason.label))
            .then_with(|| left.reason.key.cmp(&right.reason.key))
    });
    let mut seen = HashSet::new();
    let reasons = weighted_reasons
        .into_iter()
        .filter(|weighted| seen.insert((weighted.reason.kind, canonical(&weighted.reason.key))))
        .take(MAXIMUM_REASONS)
        .map(|weighted| weighted.reason)
        .collect();
    Some(CatalogRecommendationItem {
        work: candidate.work,
        score,
        reasons,
    })
}

fn append_category_reasons(
    target: &mut Vec<WeightedReason>,
    anchor: &[Category],
    candidate: &[Category],
    weight: u32,
    kind: CatalogRecommendationReasonKind,
) {
    let candidate_keys = candidate
        .iter()
        .map(|value| canonical(&value.code))
        .collect::<HashSet<_>>();
    for value in anchor {
        if candidate_keys.contains(&canonical(&value.code)) {
            target.push(WeightedReason {
                weight,
                reason: CatalogRecommendationReason {
                    kind,
                    key: value.code.clone(),
                    label: value.name.clone(),
                    label_english: value.name_english.clone(),
                },
            });
        }
    }
}

fn shared_named(anchor: &[NamedReference], candidate: &[NamedReference]) -> Vec<NamedReference> {
    let candidate_keys = named_keys(candidate);
    anchor
        .iter()
        .filter(|value| candidate_keys.contains(&canonical(&value.name)))
        .cloned()
        .collect()
}

fn named_keys(values: &[NamedReference]) -> HashSet<String> {
    values.iter().map(|value| canonical(&value.name)).collect()
}

fn tag_weight(frequency: usize, catalog_size: usize) -> u32 {
    let catalog_size = catalog_size.max(1);
    let percent_numerator = frequency.saturating_mul(100);
    if percent_numerator <= catalog_size {
        900
    } else if percent_numerator <= catalog_size.saturating_mul(5) {
        750
    } else if percent_numerator <= catalog_size.saturating_mul(20) {
        600
    } else {
        400
    }
}

fn sort_items(items: &mut [CatalogRecommendationItem]) {
    items.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| compare_release_date(&left.work, &right.work))
            .then_with(|| left.work.code.cmp(&right.work.code))
    });
}

fn compare_release_date(left: &CatalogWork, right: &CatalogWork) -> Ordering {
    right.release_date.cmp(&left.release_date)
}

fn deduplicate_items(items: &mut Vec<CatalogRecommendationItem>) {
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert(canonical(&item.work.code)));
}

fn canonical(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use dla_domain::{
        CatalogDescriptions, CatalogRelatedWork, CatalogRelationDirection, CatalogWorkDetail,
    };

    use super::*;

    struct RecordingReader {
        pool: RecommendationCandidatePool,
        limits: Mutex<Option<(usize, usize)>>,
    }

    impl CatalogRecommendationReader for RecordingReader {
        fn read_recommendation_candidates(
            &self,
            _work_code: &str,
            same_circle_limit: usize,
            similar_limit: usize,
        ) -> Result<Option<RecommendationCandidatePool>, CatalogError> {
            *self.limits.lock().expect("recommendation limits") =
                Some((same_circle_limit, similar_limit));
            Ok(Some(self.pool.clone()))
        }
    }

    #[test]
    fn uses_bounded_candidate_pools() {
        let reader = Arc::new(RecordingReader {
            pool: pool(),
            limits: Mutex::new(None),
        });
        let service = CatalogRecommendationService::new(reader.clone());

        service.read(" RJ100 ").expect("recommendations");

        assert_eq!(
            *reader.limits.lock().expect("recommendation limits"),
            Some((SAME_CIRCLE_CANDIDATE_LIMIT, SIMILAR_CANDIDATE_LIMIT))
        );
    }

    #[test]
    fn keeps_contextual_lanes_distinct_and_excludes_relations() {
        let mut pool = pool();
        pool.same_circle = vec![candidate("RJ200", "2026-02-01", &["Circle A"], &[])];
        pool.similar = vec![
            candidate("RJ200", "2026-02-01", &["Circle A"], &["Rare"]),
            candidate("RJ300", "2026-03-01", &["Circle B"], &["Rare"]),
            candidate("RJ400", "2026-04-01", &["Circle B"], &["Rare"]),
        ];
        pool.anchor.related_works = vec![CatalogRelatedWork {
            code: "RJ400".to_owned(),
            title: String::new(),
            title_english: String::new(),
            relation_type_code: "series".to_owned(),
            relation_type_label: "Series".to_owned(),
            direction: CatalogRelationDirection::Sibling,
            thumbnail_urls: Vec::new(),
        }];

        let result = rank_recommendations(pool);

        assert_eq!(result.lanes.len(), 2);
        assert_eq!(
            result.lanes[0].key,
            CatalogRecommendationLaneKey::SameCircle
        );
        assert_eq!(result.lanes[0].items[0].work.code, "RJ200");
        assert_eq!(result.lanes[1].key, CatalogRecommendationLaneKey::Similar);
        assert_eq!(result.lanes[1].items[0].work.code, "RJ300");
    }

    #[test]
    fn rewards_rare_tags_more_than_common_tags() {
        let mut pool = pool();
        pool.similar = vec![
            candidate("RJ200", "2026-01-01", &["Circle B"], &["Common"]),
            candidate("RJ300", "2025-01-01", &["Circle C"], &["Rare"]),
        ];

        let result = rank_recommendations(pool);

        assert_eq!(result.lanes[0].items[0].work.code, "RJ300");
        assert!(result.lanes[0].items[0].score > result.lanes[0].items[1].score);
    }

    #[test]
    fn applies_stable_release_date_and_code_ties() {
        let mut pool = pool();
        pool.similar = vec![
            candidate("RJ300", "2026-01-01", &["Circle B"], &["Rare"]),
            candidate("RJ200", "2026-01-01", &["Circle C"], &["Rare"]),
        ];

        let result = rank_recommendations(pool);

        assert_eq!(
            result.lanes[0]
                .items
                .iter()
                .map(|item| item.work.code.as_str())
                .collect::<Vec<_>>(),
            vec!["RJ200", "RJ300"]
        );
    }

    #[test]
    fn omits_lanes_when_imported_facets_are_absent() {
        let mut pool = pool();
        pool.anchor.work.circles.clear();
        pool.anchor.work.categories.clear();
        pool.anchor.work.tags.clear();
        pool.anchor.file_formats.clear();
        pool.anchor.supported_languages.clear();
        pool.anchor.miscellanies.clear();
        pool.same_circle = vec![candidate("RJ200", "2026-01-01", &[], &[])];
        pool.similar = vec![candidate("RJ300", "2026-01-01", &[], &[])];

        let result = rank_recommendations(pool);

        assert!(result.lanes.is_empty());
    }

    fn pool() -> RecommendationCandidatePool {
        let mut anchor = detail("RJ100", "2026-01-01", &["Circle A"], &["Rare", "Common"]);
        anchor.work.categories = vec![category("RPG", "Role-playing")];
        anchor.file_formats = vec![category("EXE", "Application")];
        anchor.supported_languages = vec![category("JPN", "Japanese")];
        anchor.miscellanies = vec![category("ORW", "Original Work")];
        RecommendationCandidatePool {
            anchor,
            same_circle: Vec::new(),
            similar: Vec::new(),
            tag_frequencies: vec![
                RecommendationFacetFrequency {
                    key: "Rare".to_owned(),
                    count: 5,
                },
                RecommendationFacetFrequency {
                    key: "Common".to_owned(),
                    count: 500,
                },
            ],
            catalog_size: 1_000,
        }
    }

    fn candidate(
        code: &str,
        release_date: &str,
        circles: &[&str],
        tags: &[&str],
    ) -> RecommendationCandidate {
        let detail = detail(code, release_date, circles, tags);
        RecommendationCandidate {
            work: detail.work,
            file_formats: vec![category("EXE", "Application")],
            supported_languages: vec![category("JPN", "Japanese")],
            miscellanies: vec![category("ORW", "Original Work")],
        }
    }

    fn detail(
        code: &str,
        release_date: &str,
        circles: &[&str],
        tags: &[&str],
    ) -> CatalogWorkDetail {
        CatalogWorkDetail {
            work: CatalogWork {
                code: code.to_owned(),
                source_code: "DL".to_owned(),
                title: code.to_owned(),
                title_english: code.to_owned(),
                added_date: release_date.to_owned(),
                release_date: release_date.to_owned(),
                updated_date: release_date.to_owned(),
                age_rating: "r18".to_owned(),
                release_type: "digital".to_owned(),
                main_image_urls: Vec::new(),
                thumbnail_urls: Vec::new(),
                circles: circles.iter().map(|value| named(value)).collect(),
                categories: Vec::new(),
                tags: tags.iter().map(|value| named(value)).collect(),
                synthetic: false,
            },
            sample_image_urls: Vec::new(),
            file_formats: Vec::new(),
            supported_languages: Vec::new(),
            miscellanies: Vec::new(),
            roms: Vec::new(),
            related_works: Vec::new(),
            rating: None,
            descriptions: CatalogDescriptions::default(),
        }
    }

    fn named(value: &str) -> NamedReference {
        NamedReference {
            name: value.to_owned(),
            name_english: value.to_owned(),
        }
    }

    fn category(code: &str, name: &str) -> Category {
        Category {
            code: code.to_owned(),
            name: name.to_owned(),
            name_english: name.to_owned(),
        }
    }
}
