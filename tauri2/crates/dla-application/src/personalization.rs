use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use dla_domain::{
    CatalogWork,
    installation::{Installation, LaunchActionKind},
    library::{
        LocalPersonalization, PersonalizationAnchor, PersonalizedRecommendationItem,
        WorkPreference, WorkPreferenceKind,
    },
};
use thiserror::Error;

use crate::{
    catalog::{CatalogError, CatalogReader},
    installation::{InstallationLibraryError, InstallationStore},
    library_shelves::{LibraryActivityReader, LibraryShelvesError},
    recommendation::{CatalogRecommendationService, CatalogRecommendations},
};

const PERSONALIZATION_LANE_LIMIT: usize = 12;
const MAXIMUM_ACTIVITY_ANCHORS: usize = 8;
const MAXIMUM_EXPLANATION_ANCHORS: usize = 2;
pub const BECAUSE_YOU_MINIMUM: usize = 2;
pub const VOICE_MIX_MINIMUM: usize = 2;

#[derive(Debug, Error)]
pub enum WorkPreferenceError {
    #[error("work code cannot be empty")]
    EmptyWorkCode,
    #[error("work preference persistence failed: {0}")]
    Persistence(String),
}

impl WorkPreferenceError {
    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait WorkPreferenceStore: Send + Sync {
    fn read_work_preference(
        &self,
        work_code: &str,
    ) -> Result<Option<WorkPreference>, WorkPreferenceError>;
    fn list_work_preferences(&self) -> Result<Vec<WorkPreference>, WorkPreferenceError>;
    fn replace_work_preference(
        &self,
        work_code: &str,
        preference: Option<WorkPreferenceKind>,
        updated_at: &str,
    ) -> Result<Option<WorkPreference>, WorkPreferenceError>;
}

pub struct WorkPreferenceService {
    store: Arc<dyn WorkPreferenceStore>,
}

impl WorkPreferenceService {
    pub fn new(store: Arc<dyn WorkPreferenceStore>) -> Self {
        Self { store }
    }

    pub fn read(&self, work_code: &str) -> Result<Option<WorkPreference>, WorkPreferenceError> {
        self.store
            .read_work_preference(normalize_work_code(work_code)?)
    }

    pub fn list(&self) -> Result<Vec<WorkPreference>, WorkPreferenceError> {
        self.store.list_work_preferences()
    }

    pub fn replace(
        &self,
        work_code: &str,
        preference: Option<WorkPreferenceKind>,
        updated_at: &str,
    ) -> Result<Option<WorkPreference>, WorkPreferenceError> {
        self.store
            .replace_work_preference(normalize_work_code(work_code)?, preference, updated_at)
    }
}

pub trait ContextualRecommendationProvider: Send + Sync {
    fn read_contextual(&self, work_code: &str) -> Result<CatalogRecommendations, CatalogError>;
}

impl ContextualRecommendationProvider for CatalogRecommendationService {
    fn read_contextual(&self, work_code: &str) -> Result<CatalogRecommendations, CatalogError> {
        self.read(work_code)
    }
}

#[derive(Debug, Error)]
pub enum LocalPersonalizationError {
    #[error(transparent)]
    Preference(#[from] WorkPreferenceError),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
    #[error(transparent)]
    Activity(#[from] LibraryShelvesError),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}

pub struct LocalPersonalizationService {
    preferences: Arc<dyn WorkPreferenceStore>,
    installations: Arc<dyn InstallationStore>,
    activity: Arc<dyn LibraryActivityReader>,
    catalog: Arc<dyn CatalogReader>,
    recommendations: Arc<dyn ContextualRecommendationProvider>,
}

impl LocalPersonalizationService {
    pub fn new(
        preferences: Arc<dyn WorkPreferenceStore>,
        installations: Arc<dyn InstallationStore>,
        activity: Arc<dyn LibraryActivityReader>,
        catalog: Arc<dyn CatalogReader>,
        recommendations: Arc<dyn ContextualRecommendationProvider>,
    ) -> Self {
        Self {
            preferences,
            installations,
            activity,
            catalog,
            recommendations,
        }
    }

    pub fn read(&self) -> Result<LocalPersonalization, LocalPersonalizationError> {
        let preferences = self.preferences.list_work_preferences()?;
        let installations = self.installations.list()?;
        let activity = self.activity.read_library_activity()?;
        let preference_by_code = preferences
            .iter()
            .map(|preference| (canonical(&preference.work_code), preference.preference))
            .collect::<HashMap<_, _>>();
        let installed_codes = installations
            .iter()
            .filter_map(Installation::effective_catalog_work_code)
            .map(canonical)
            .collect::<HashSet<_>>();
        let favorites = self.read_favorites(&preferences)?;
        let activity_anchors = self.read_activity_anchors(&installations, activity.recent)?;
        let activity_work_count = activity_anchors.len();
        let voice_activity_work_count = activity_anchors
            .iter()
            .filter(|anchor| anchor.action == LaunchActionKind::PlayAudio)
            .count();
        let anchors = activity_anchors
            .iter()
            .take(MAXIMUM_ACTIVITY_ANCHORS)
            .cloned()
            .collect::<Vec<_>>();

        let because_you = if activity_work_count >= BECAUSE_YOU_MINIMUM {
            self.rank_lane(&anchors, &installed_codes, &preference_by_code, false)?
        } else {
            Vec::new()
        };
        let voice_anchors = activity_anchors
            .iter()
            .filter(|anchor| anchor.action == LaunchActionKind::PlayAudio)
            .take(MAXIMUM_ACTIVITY_ANCHORS)
            .cloned()
            .collect::<Vec<_>>();
        let voice_mix = if voice_activity_work_count >= VOICE_MIX_MINIMUM {
            self.rank_lane(&voice_anchors, &installed_codes, &preference_by_code, true)?
        } else {
            Vec::new()
        };

        Ok(LocalPersonalization {
            favorites,
            because_you,
            voice_mix,
            activity_work_count,
            voice_activity_work_count,
            because_you_minimum: BECAUSE_YOU_MINIMUM,
            voice_mix_minimum: VOICE_MIX_MINIMUM,
        })
    }

    fn read_favorites(
        &self,
        preferences: &[WorkPreference],
    ) -> Result<Vec<CatalogWork>, CatalogError> {
        let mut favorites = Vec::new();
        for preference in preferences
            .iter()
            .filter(|preference| preference.preference == WorkPreferenceKind::Favorite)
        {
            if let Some(detail) = self.catalog.read(&preference.work_code)? {
                favorites.push(detail.work);
            }
        }
        Ok(favorites)
    }

    fn read_activity_anchors(
        &self,
        installations: &[Installation],
        mut recent: Vec<dla_domain::library::LibraryRecentActivity>,
    ) -> Result<Vec<ActivityAnchor>, CatalogError> {
        recent.sort_by(|left, right| {
            right
                .occurred_at
                .cmp(&left.occurred_at)
                .then_with(|| left.installation_id.cmp(&right.installation_id))
        });
        let work_by_installation = installations
            .iter()
            .filter_map(|installation| {
                installation
                    .effective_catalog_work_code()
                    .map(|code| (installation.id.clone(), code.to_owned()))
            })
            .collect::<HashMap<_, _>>();
        let mut seen = HashSet::new();
        let mut anchors = Vec::new();
        for activity in recent {
            let Some(action) = activity.action else {
                continue;
            };
            let Some(work_code) = work_by_installation.get(&activity.installation_id) else {
                continue;
            };
            if !seen.insert(canonical(work_code)) {
                continue;
            }
            let Some(work) = self.catalog.read(work_code)? else {
                continue;
            };
            anchors.push(ActivityAnchor {
                work: work.work,
                action,
            });
        }
        Ok(anchors)
    }

    fn rank_lane(
        &self,
        anchors: &[ActivityAnchor],
        installed_codes: &HashSet<String>,
        preferences: &HashMap<String, WorkPreferenceKind>,
        voice_only: bool,
    ) -> Result<Vec<PersonalizedRecommendationItem>, CatalogError> {
        let anchor_codes = anchors
            .iter()
            .map(|anchor| canonical(&anchor.work.code))
            .collect::<HashSet<_>>();
        let mut candidates = HashMap::<String, AggregatedCandidate>::new();

        for (anchor_index, anchor) in anchors.iter().enumerate() {
            let recommendations = match self.recommendations.read_contextual(&anchor.work.code) {
                Ok(recommendations) => recommendations,
                Err(CatalogError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            let mut seen_for_anchor = HashSet::new();
            for item in recommendations
                .lanes
                .into_iter()
                .flat_map(|lane| lane.items)
            {
                let code = canonical(&item.work.code);
                if !seen_for_anchor.insert(code.clone())
                    || installed_codes.contains(&code)
                    || anchor_codes.contains(&code)
                    || preferences.contains_key(&code)
                    || (voice_only && !is_voice_work(&item.work))
                {
                    continue;
                }
                let recency_bonus = ((MAXIMUM_ACTIVITY_ANCHORS - anchor_index) as u32) * 100;
                let candidate = candidates
                    .entry(code)
                    .or_insert_with(|| AggregatedCandidate {
                        work: item.work,
                        score: 0,
                        anchors: Vec::new(),
                    });
                candidate.score = candidate
                    .score
                    .saturating_add(item.score)
                    .saturating_add(recency_bonus);
                if candidate.anchors.len() < MAXIMUM_EXPLANATION_ANCHORS {
                    candidate.anchors.push(PersonalizationAnchor {
                        work_code: anchor.work.code.clone(),
                        title: preferred_title(&anchor.work),
                        action: anchor.action,
                    });
                }
            }
        }

        let mut ranked = candidates
            .into_values()
            .map(|candidate| PersonalizedRecommendationItem {
                work: candidate.work,
                score: candidate.score,
                anchors: candidate.anchors,
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| compare_release_dates(&left.work, &right.work))
                .then_with(|| left.work.code.cmp(&right.work.code))
        });
        ranked.truncate(PERSONALIZATION_LANE_LIMIT);
        Ok(ranked)
    }
}

#[derive(Clone)]
struct ActivityAnchor {
    work: CatalogWork,
    action: LaunchActionKind,
}

struct AggregatedCandidate {
    work: CatalogWork,
    score: u32,
    anchors: Vec<PersonalizationAnchor>,
}

fn normalize_work_code(work_code: &str) -> Result<&str, WorkPreferenceError> {
    let work_code = work_code.trim();
    if work_code.is_empty() {
        Err(WorkPreferenceError::EmptyWorkCode)
    } else {
        Ok(work_code)
    }
}

fn preferred_title(work: &CatalogWork) -> String {
    if work.title_english.trim().is_empty() {
        work.title.clone()
    } else {
        work.title_english.clone()
    }
}

fn compare_release_dates(left: &CatalogWork, right: &CatalogWork) -> Ordering {
    right
        .release_date
        .cmp(&left.release_date)
        .then_with(|| left.code.cmp(&right.code))
}

fn canonical(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn is_voice_work(work: &CatalogWork) -> bool {
    work.categories.iter().any(|category| {
        contains_voice_signal(&category.code)
            || contains_voice_signal(&category.name)
            || contains_voice_signal(&category.name_english)
    }) || work
        .tags
        .iter()
        .any(|tag| contains_voice_signal(&tag.name) || contains_voice_signal(&tag.name_english))
}

fn contains_voice_signal(value: &str) -> bool {
    let canonical = value.to_lowercase();
    canonical.contains("voice")
        || canonical.contains("asmr")
        || canonical.contains("audio")
        || value.contains("音声")
        || value.contains("ボイス")
}

#[cfg(test)]
mod tests {
    use dla_domain::{
        CatalogDescriptions, CatalogRomContents, CatalogWorkDetail, Category,
        installation::{
            InstallationDetection, InstallationId, InstallationOverrides, InstallationPlatform,
            InstallationStatus, ManualCatalogIdentity,
        },
        library::{LibraryActivityKind, LibraryRecentActivity},
    };

    use crate::{
        catalog::{CatalogContext, CatalogContextQuery, CatalogPage, CatalogQuery},
        library_shelves::LibraryActivitySnapshot,
        recommendation::{
            CatalogRecommendationItem, CatalogRecommendationLane, CatalogRecommendationLaneKey,
        },
    };

    use super::*;

    #[test]
    fn empty_work_codes_are_rejected_at_the_application_boundary() {
        assert!(matches!(
            normalize_work_code("  "),
            Err(WorkPreferenceError::EmptyWorkCode)
        ));
    }

    #[test]
    fn favorites_are_visible_without_fabricating_activity() {
        let favorite = work("RJFAVORITE", false);
        let anchor = work("RJANCHOR", false);
        let service = service(
            vec![preference(&favorite.code, WorkPreferenceKind::Favorite)],
            vec![installation("anchor", &anchor.code)],
            vec![activity(
                "anchor",
                LaunchActionKind::LaunchExecutable,
                "2026-08-09T11:00:00Z",
            )],
            vec![favorite.clone(), anchor],
            HashMap::new(),
        );

        let result = service.read().expect("local personalization");

        assert_eq!(result.favorites, vec![favorite]);
        assert_eq!(result.activity_work_count, 1);
        assert_eq!(result.voice_activity_work_count, 0);
        assert!(result.because_you.is_empty());
        assert!(result.voice_mix.is_empty());
    }

    #[test]
    fn activity_lanes_are_explainable_filtered_and_deduplicated() {
        let anchor_a = work("RJA", true);
        let anchor_b = work("RJB", true);
        let installed = work("RJINSTALLED", true);
        let favorite = work("RJFAVORITE", true);
        let dismissed = work("RJDISMISSED", true);
        let voice = work("RJVOICE", true);
        let game = work("RJGAME", false);
        let recommendations = HashMap::from([
            (
                canonical(&anchor_a.code),
                recommendation(
                    &anchor_a.code,
                    vec![
                        item(voice.clone(), 600),
                        item(game.clone(), 500),
                        item(installed.clone(), 1_000),
                        item(favorite.clone(), 1_000),
                        item(dismissed.clone(), 1_000),
                        item(anchor_b.clone(), 1_000),
                    ],
                ),
            ),
            (
                canonical(&anchor_b.code),
                recommendation(&anchor_b.code, vec![item(voice.clone(), 700)]),
            ),
        ]);
        let service = service(
            vec![
                preference(&favorite.code, WorkPreferenceKind::Favorite),
                preference(&dismissed.code, WorkPreferenceKind::NotInterested),
            ],
            vec![
                installation("anchor-a", &anchor_a.code),
                installation("anchor-b", &anchor_b.code),
                installation("installed", &installed.code),
            ],
            vec![
                activity(
                    "anchor-a",
                    LaunchActionKind::PlayAudio,
                    "2026-08-09T10:00:00Z",
                ),
                activity(
                    "anchor-b",
                    LaunchActionKind::PlayAudio,
                    "2026-08-09T11:00:00Z",
                ),
            ],
            vec![anchor_a, anchor_b, installed, favorite.clone(), dismissed],
            recommendations,
        );

        let result = service.read().expect("local personalization");
        let because_codes = result
            .because_you
            .iter()
            .map(|item| item.work.code.as_str())
            .collect::<Vec<_>>();
        let voice_codes = result
            .voice_mix
            .iter()
            .map(|item| item.work.code.as_str())
            .collect::<Vec<_>>();

        assert_eq!(result.favorites, vec![favorite]);
        assert_eq!(result.activity_work_count, 2);
        assert_eq!(result.voice_activity_work_count, 2);
        assert_eq!(because_codes, vec!["RJVOICE", "RJGAME"]);
        assert_eq!(voice_codes, vec!["RJVOICE"]);
        assert_eq!(result.because_you[0].anchors.len(), 2);
    }

    fn service(
        preferences: Vec<WorkPreference>,
        installations: Vec<Installation>,
        recent: Vec<LibraryRecentActivity>,
        works: Vec<CatalogWork>,
        recommendations: HashMap<String, CatalogRecommendations>,
    ) -> LocalPersonalizationService {
        LocalPersonalizationService::new(
            Arc::new(MemoryPreferenceStore { preferences }),
            Arc::new(MemoryInstallationStore { installations }),
            Arc::new(MemoryActivityReader {
                snapshot: LibraryActivitySnapshot {
                    launch_totals: Vec::new(),
                    recent,
                    resumes: Vec::new(),
                },
            }),
            Arc::new(MemoryCatalogReader {
                works: works
                    .into_iter()
                    .map(|work| (canonical(&work.code), detail(work)))
                    .collect(),
            }),
            Arc::new(MemoryRecommendationProvider { recommendations }),
        )
    }

    struct MemoryPreferenceStore {
        preferences: Vec<WorkPreference>,
    }

    impl WorkPreferenceStore for MemoryPreferenceStore {
        fn read_work_preference(
            &self,
            work_code: &str,
        ) -> Result<Option<WorkPreference>, WorkPreferenceError> {
            Ok(self
                .preferences
                .iter()
                .find(|preference| preference.work_code.eq_ignore_ascii_case(work_code))
                .cloned())
        }

        fn list_work_preferences(&self) -> Result<Vec<WorkPreference>, WorkPreferenceError> {
            Ok(self.preferences.clone())
        }

        fn replace_work_preference(
            &self,
            _work_code: &str,
            _preference: Option<WorkPreferenceKind>,
            _updated_at: &str,
        ) -> Result<Option<WorkPreference>, WorkPreferenceError> {
            unreachable!()
        }
    }

    struct MemoryInstallationStore {
        installations: Vec<Installation>,
    }

    impl InstallationStore for MemoryInstallationStore {
        fn create(&self, _installation: &Installation) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }

        fn create_or_refresh(
            &self,
            _installation: &Installation,
        ) -> Result<Installation, InstallationLibraryError> {
            unreachable!()
        }

        fn read(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<Installation>, InstallationLibraryError> {
            Ok(self
                .installations
                .iter()
                .find(|installation| installation.id == *installation_id)
                .cloned())
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(self.installations.clone())
        }

        fn replace_detection(
            &self,
            _installation_id: &InstallationId,
            _detection: &InstallationDetection,
            _status: InstallationStatus,
            _updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }

        fn replace_overrides(
            &self,
            _installation_id: &InstallationId,
            _overrides: &InstallationOverrides,
            _status: InstallationStatus,
            _updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }
    }

    struct MemoryActivityReader {
        snapshot: LibraryActivitySnapshot,
    }

    impl LibraryActivityReader for MemoryActivityReader {
        fn read_library_activity(&self) -> Result<LibraryActivitySnapshot, LibraryShelvesError> {
            Ok(self.snapshot.clone())
        }
    }

    struct MemoryCatalogReader {
        works: HashMap<String, CatalogWorkDetail>,
    }

    impl CatalogReader for MemoryCatalogReader {
        fn browse(&self, _query: &CatalogQuery) -> Result<CatalogPage, CatalogError> {
            unreachable!()
        }

        fn context(&self, _query: &CatalogContextQuery) -> Result<CatalogContext, CatalogError> {
            unreachable!()
        }

        fn read(&self, code: &str) -> Result<Option<CatalogWorkDetail>, CatalogError> {
            Ok(self.works.get(&canonical(code)).cloned())
        }

        fn read_rom_contents(
            &self,
            _work_code: &str,
            _rom_position: usize,
        ) -> Result<Option<CatalogRomContents>, CatalogError> {
            unreachable!()
        }
    }

    struct MemoryRecommendationProvider {
        recommendations: HashMap<String, CatalogRecommendations>,
    }

    impl ContextualRecommendationProvider for MemoryRecommendationProvider {
        fn read_contextual(&self, work_code: &str) -> Result<CatalogRecommendations, CatalogError> {
            self.recommendations
                .get(&canonical(work_code))
                .cloned()
                .ok_or_else(|| CatalogError::NotFound(work_code.to_owned()))
        }
    }

    fn installation(id: &str, work_code: &str) -> Installation {
        Installation {
            id: InstallationId(id.to_owned()),
            scan_root_id: None,
            root_path: format!("/library/{id}"),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: InstallationStatus::Ready,
                content_items: Vec::new(),
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                catalog_identity: Some(ManualCatalogIdentity::CatalogWork {
                    work_code: work_code.to_owned(),
                }),
                custom_title: None,
                preferred_action: None,
                content_items: Vec::new(),
                reviewed_at: Some("2026-08-09T00:00:00Z".to_owned()),
            },
            discovered_at: "2026-08-09T00:00:00Z".to_owned(),
            updated_at: "2026-08-09T00:00:00Z".to_owned(),
        }
    }

    fn activity(
        installation_id: &str,
        action: LaunchActionKind,
        occurred_at: &str,
    ) -> LibraryRecentActivity {
        LibraryRecentActivity {
            installation_id: InstallationId(installation_id.to_owned()),
            action: Some(action),
            kind: if action == LaunchActionKind::LaunchExecutable {
                LibraryActivityKind::ExecutableLaunch
            } else {
                LibraryActivityKind::MediaSession
            },
            occurred_at: occurred_at.to_owned(),
            active: false,
        }
    }

    fn preference(work_code: &str, preference: WorkPreferenceKind) -> WorkPreference {
        WorkPreference {
            work_code: work_code.to_owned(),
            preference,
            updated_at: "2026-08-09T00:00:00Z".to_owned(),
        }
    }

    fn work(code: &str, voice: bool) -> CatalogWork {
        CatalogWork {
            code: code.to_owned(),
            source_code: code.to_owned(),
            title: code.to_owned(),
            title_english: code.to_owned(),
            added_date: "2026-08-09".to_owned(),
            release_date: "2026-08-09".to_owned(),
            updated_date: "2026-08-09".to_owned(),
            age_rating: "R18".to_owned(),
            release_type: "digital".to_owned(),
            main_image_urls: Vec::new(),
            thumbnail_urls: Vec::new(),
            circles: Vec::new(),
            categories: vec![if voice {
                Category {
                    code: "voice".to_owned(),
                    name: "音声".to_owned(),
                    name_english: "Voice / ASMR".to_owned(),
                }
            } else {
                Category {
                    code: "game".to_owned(),
                    name: "ゲーム".to_owned(),
                    name_english: "Game".to_owned(),
                }
            }],
            tags: Vec::new(),
            synthetic: false,
        }
    }

    fn detail(work: CatalogWork) -> CatalogWorkDetail {
        CatalogWorkDetail {
            work,
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

    fn recommendation(
        anchor_work_code: &str,
        items: Vec<CatalogRecommendationItem>,
    ) -> CatalogRecommendations {
        CatalogRecommendations {
            anchor_work_code: anchor_work_code.to_owned(),
            lanes: vec![CatalogRecommendationLane {
                key: CatalogRecommendationLaneKey::Similar,
                items,
            }],
        }
    }

    fn item(work: CatalogWork, score: u32) -> CatalogRecommendationItem {
        CatalogRecommendationItem {
            work,
            score,
            reasons: Vec::new(),
        }
    }
}
