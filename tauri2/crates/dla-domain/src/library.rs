use serde::{Deserialize, Serialize};

use crate::{
    CatalogWork,
    installation::{Installation, InstallationId, LaunchActionKind},
    media::MediaResume,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPreferenceKind {
    Favorite,
    NotInterested,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkPreference {
    pub work_code: String,
    pub preference: WorkPreferenceKind,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalizationAnchor {
    pub work_code: String,
    pub title: String,
    pub action: LaunchActionKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalizedRecommendationItem {
    pub work: CatalogWork,
    pub score: u32,
    pub anchors: Vec<PersonalizationAnchor>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPersonalization {
    pub favorites: Vec<CatalogWork>,
    pub because_you: Vec<PersonalizedRecommendationItem>,
    pub voice_mix: Vec<PersonalizedRecommendationItem>,
    pub activity_work_count: usize,
    pub voice_activity_work_count: usize,
    pub because_you_minimum: usize,
    pub voice_mix_minimum: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryActivityKind {
    ExecutableLaunch,
    MediaSession,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRecentActivity {
    pub installation_id: InstallationId,
    pub action: Option<LaunchActionKind>,
    pub kind: LibraryActivityKind,
    pub occurred_at: String,
    pub active: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryLaunchTotals {
    pub installation_id: InstallationId,
    pub launch_count: u32,
    pub total_duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryShelves {
    pub installations: Vec<Installation>,
    pub recent: Vec<LibraryRecentActivity>,
    pub continue_items: Vec<MediaResume>,
    pub never_launched: Vec<InstallationId>,
    pub unfinished: Vec<MediaResume>,
    pub launch_totals: Vec<LibraryLaunchTotals>,
}
