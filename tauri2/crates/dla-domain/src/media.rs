use serde::{Deserialize, Serialize};

use crate::installation::{InstallationId, LaunchActionKind, MediaType, RelativePath};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MediaSessionId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSessionKind {
    Work,
    PersonalizedVoice,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRepeatMode {
    Off,
    All,
    One,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaSessionStatus {
    Active,
    Paused,
    Completed,
    Closed,
    Interrupted,
    Failed,
}

impl MediaSessionStatus {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Active | Self::Paused)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSessionItem {
    pub ordinal: u32,
    pub installation_id: InstallationId,
    pub work_code: Option<String>,
    pub relative_path: RelativePath,
    pub media_type: MediaType,
    pub size_bytes: Option<u64>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub bonus: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexedAudioTrack {
    pub installation_id: InstallationId,
    pub work_code: Option<String>,
    pub relative_path: RelativePath,
    pub size_bytes: Option<u64>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub bonus: bool,
    pub sort_order: u32,
    pub indexed_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaProgress {
    pub item_ordinal: u32,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub completed: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSession {
    pub id: MediaSessionId,
    pub kind: MediaSessionKind,
    pub installation_id: InstallationId,
    pub action: LaunchActionKind,
    pub status: MediaSessionStatus,
    pub repeat_mode: MediaRepeatMode,
    pub shuffle: bool,
    pub items: Vec<MediaSessionItem>,
    pub progress: MediaProgress,
    pub opened_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaQueueState {
    pub kind: MediaSessionKind,
    pub installation_id: Option<InstallationId>,
    pub current_installation_id: InstallationId,
    pub current_relative_path: RelativePath,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub completed: bool,
    pub repeat_mode: MediaRepeatMode,
    pub shuffle: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaResume {
    pub installation_id: InstallationId,
    pub action: LaunchActionKind,
    pub relative_path: RelativePath,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub completed: bool,
    pub updated_at: String,
}
