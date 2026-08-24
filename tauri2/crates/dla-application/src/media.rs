use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Arc,
};

use dla_detection::classify_package_media_paths;
use dla_domain::{
    installation::{
        ContentItem, Installation, InstallationId, InstallationStatus, LaunchActionKind,
        LaunchTarget, MediaType, RelativePath,
    },
    library::WorkPreferenceKind,
    media::{
        IndexedAudioTrack, MediaProgress, MediaQueueState, MediaRepeatMode, MediaResume,
        MediaSession, MediaSessionId, MediaSessionItem, MediaSessionKind, MediaSessionStatus,
    },
    package::PreparedPackageInstallation,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    installation::{InstallationLibraryError, InstallationStore},
    library_shelves::{LibraryActivityReader, LibraryShelvesError},
    package_preparation::{PackagePreparationError, PackagePreparationStore},
    personalization::{WorkPreferenceError, WorkPreferenceStore},
};

const PERSONALIZED_VOICE_MINIMUM_WORKS: usize = 2;
const PERSONALIZED_VOICE_QUEUE_LIMIT: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenMediaSessionRequest {
    pub installation_id: InstallationId,
    pub session_id: MediaSessionId,
    pub opened_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateMediaProgressRequest {
    pub session_id: MediaSessionId,
    pub item_ordinal: u32,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub completed: bool,
    pub status: MediaSessionStatus,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateMediaQueueSettingsRequest {
    pub session_id: MediaSessionId,
    pub repeat_mode: MediaRepeatMode,
    pub shuffle: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenPersonalizedVoiceQueueRequest {
    pub session_id: MediaSessionId,
    pub opened_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaInventoryItem {
    pub relative_path: RelativePath,
    pub media_type: MediaType,
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaAssetDescriptor {
    pub root_path: String,
    pub item: MediaSessionItem,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioWaveform {
    pub peaks: Vec<f32>,
    pub duration_ms: u64,
}

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("media request is missing {0}")]
    InvalidRequest(&'static str),
    #[error("installation was not found: {0}")]
    InstallationNotFound(String),
    #[error("media session was not found: {0}")]
    SessionNotFound(String),
    #[error("installation must be ready before its media can be opened")]
    NeedsReview,
    #[error("installation must be explicitly reviewed before its media can be opened")]
    NotReviewed,
    #[error("installation has no explicit media action")]
    MissingAction,
    #[error("the selected action is not a media player or reader action")]
    UnsupportedAction,
    #[error("the selected media target does not exist in this installation")]
    MissingContentTarget,
    #[error("the selected media target was ignored during installation review")]
    IgnoredTarget,
    #[error("no supported media files were found for the selected action")]
    EmptyInventory,
    #[error("a personalized voice queue requires listening activity from at least {0} works")]
    InsufficientVoiceActivity(usize),
    #[error("media item was not found in this session: {0}")]
    ItemNotFound(u32),
    #[error("media session is already closed")]
    SessionClosed,
    #[error("invalid media progress state")]
    InvalidProgressState,
    #[error("media inventory failed: {0}")]
    Inventory(String),
    #[error("audio waveform failed: {0}")]
    Waveform(String),
    #[error("media persistence failed: {0}")]
    Persistence(String),
    #[error(transparent)]
    Preference(#[from] WorkPreferenceError),
    #[error(transparent)]
    Activity(#[from] LibraryShelvesError),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
    #[error(transparent)]
    Package(#[from] PackagePreparationError),
}

impl MediaError {
    pub fn inventory(error: impl std::fmt::Display) -> Self {
        Self::Inventory(error.to_string())
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }

    pub fn waveform(error: impl std::fmt::Display) -> Self {
        Self::Waveform(error.to_string())
    }
}

pub trait MediaInventoryReader: Send + Sync {
    fn read_inventory(&self, root_path: &str) -> Result<Vec<MediaInventoryItem>, MediaError>;
}

pub trait AudioWaveformReader: Send + Sync {
    fn read_waveform(
        &self,
        root_path: &str,
        relative_path: &RelativePath,
        bucket_count: u32,
    ) -> Result<AudioWaveform, MediaError>;
}

pub trait MediaSessionStore: Send + Sync {
    fn create_media_session(&self, session: &MediaSession) -> Result<(), MediaError>;
    fn save_media_session(&self, session: &MediaSession) -> Result<(), MediaError>;
    fn read_media_session(
        &self,
        session_id: &MediaSessionId,
    ) -> Result<Option<MediaSession>, MediaError>;
    fn read_open_media_session(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<MediaSession>, MediaError>;
    fn read_open_personalized_media_session(&self) -> Result<Option<MediaSession>, MediaError>;
    fn read_media_queue_state(
        &self,
        kind: MediaSessionKind,
        installation_id: Option<&InstallationId>,
    ) -> Result<Option<MediaQueueState>, MediaError>;
    fn read_media_resume(
        &self,
        installation_id: &InstallationId,
        action: LaunchActionKind,
    ) -> Result<Option<MediaResume>, MediaError>;
    fn interrupt_open_media_sessions(
        &self,
        interrupted_at: &str,
        reason: &str,
    ) -> Result<u64, MediaError>;
}

pub trait AudioTrackStore: Send + Sync {
    fn replace_audio_tracks(
        &self,
        installation_id: &InstallationId,
        tracks: &[IndexedAudioTrack],
    ) -> Result<(), MediaError>;
    fn list_audio_tracks(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Vec<IndexedAudioTrack>, MediaError>;
    fn list_all_audio_tracks(&self) -> Result<Vec<IndexedAudioTrack>, MediaError>;
}

pub struct MediaService {
    installations: Arc<dyn InstallationStore>,
    preparations: Arc<dyn PackagePreparationStore>,
    sessions: Arc<dyn MediaSessionStore>,
    audio_tracks: Arc<dyn AudioTrackStore>,
    inventory: Arc<dyn MediaInventoryReader>,
    waveforms: Arc<dyn AudioWaveformReader>,
}

impl MediaService {
    pub fn new(
        installations: Arc<dyn InstallationStore>,
        preparations: Arc<dyn PackagePreparationStore>,
        sessions: Arc<dyn MediaSessionStore>,
        audio_tracks: Arc<dyn AudioTrackStore>,
        inventory: Arc<dyn MediaInventoryReader>,
        waveforms: Arc<dyn AudioWaveformReader>,
    ) -> Self {
        Self {
            installations,
            preparations,
            sessions,
            audio_tracks,
            inventory,
            waveforms,
        }
    }

    pub fn reconcile_after_restart(&self, interrupted_at: &str) -> Result<u64, MediaError> {
        self.sessions.interrupt_open_media_sessions(
            interrupted_at,
            "the launcher restarted before the media session was closed",
        )
    }

    pub fn read_prepared_package(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<PreparedPackageInstallation>, MediaError> {
        let Some(prepared) = self.preparations.read_prepared_package(installation_id)? else {
            return Ok(None);
        };
        Ok(Some(self.recover_prepared_action(prepared)?))
    }

    fn recover_prepared_action(
        &self,
        mut prepared: PreparedPackageInstallation,
    ) -> Result<PreparedPackageInstallation, MediaError> {
        if prepared.preferred_action.is_none() {
            let inventory = self.inventory.read_inventory(&prepared.destination_root)?;
            let paths = inventory
                .into_iter()
                .map(|item| item.relative_path)
                .collect::<Vec<_>>();
            if let Some(classification) = classify_package_media_paths(&paths)
                && let Some(action) = classification.launch_candidates.first()
            {
                prepared.content_root = classification.content_root;
                prepared.preferred_action = Some(action.clone());
            }
        }
        Ok(prepared)
    }

    pub fn read_prepared_packages(
        &self,
        installation_ids: &[InstallationId],
    ) -> Result<Vec<PreparedPackageInstallation>, MediaError> {
        let mut seen = HashSet::new();
        let mut normalized = Vec::with_capacity(installation_ids.len());
        for installation_id in installation_ids {
            let installation_id = installation_id.0.trim();
            if installation_id.is_empty() || !seen.insert(installation_id.to_owned()) {
                continue;
            }
            normalized.push(InstallationId(installation_id.to_owned()));
        }
        let mut prepared = self
            .preparations
            .read_prepared_packages(&normalized)?
            .into_iter()
            .map(|package| (package.installation_id.clone(), package))
            .collect::<HashMap<_, _>>();
        normalized
            .into_iter()
            .filter_map(|installation_id| prepared.remove(&installation_id))
            .map(|package| self.recover_prepared_action(package))
            .collect()
    }

    pub fn open(&self, request: OpenMediaSessionRequest) -> Result<MediaSession, MediaError> {
        validate_open_request(&request)?;
        if let Some(session) = self
            .sessions
            .read_open_media_session(&request.installation_id)?
        {
            return Ok(session);
        }
        let installation = self
            .installations
            .read(&request.installation_id)?
            .ok_or_else(|| MediaError::InstallationNotFound(request.installation_id.0.clone()))?;
        validate_installation(&installation)?;
        let selection = self.resolve_selection(&installation)?;
        let items = self.resolve_items(&installation, &selection, &request.opened_at, true)?;
        let queue_state = self
            .sessions
            .read_media_queue_state(MediaSessionKind::Work, Some(&request.installation_id))?;
        let progress = initial_progress(
            &items,
            queue_state.as_ref(),
            self.sessions
                .read_media_resume(&request.installation_id, selection.action)?,
            &request.opened_at,
        );
        let session = MediaSession {
            id: request.session_id,
            kind: MediaSessionKind::Work,
            installation_id: request.installation_id,
            action: selection.action,
            status: MediaSessionStatus::Active,
            repeat_mode: queue_state
                .as_ref()
                .map_or(MediaRepeatMode::Off, |state| state.repeat_mode),
            shuffle: queue_state.as_ref().is_some_and(|state| state.shuffle),
            items,
            progress,
            opened_at: request.opened_at.clone(),
            updated_at: request.opened_at,
            ended_at: None,
            error: None,
        };
        self.sessions.create_media_session(&session)?;
        Ok(session)
    }

    pub fn list_items(
        &self,
        installation_id: &InstallationId,
        indexed_at: &str,
    ) -> Result<Vec<MediaSessionItem>, MediaError> {
        let installation = self
            .installations
            .read(installation_id)?
            .ok_or_else(|| MediaError::InstallationNotFound(installation_id.0.clone()))?;
        validate_installation(&installation)?;
        let selection = self.resolve_selection(&installation)?;
        self.resolve_items(&installation, &selection, indexed_at, false)
    }

    pub fn read_audio_waveform(
        &self,
        installation_id: &InstallationId,
        ordinal: u32,
        bucket_count: u32,
        indexed_at: &str,
    ) -> Result<AudioWaveform, MediaError> {
        if !(16..=2_048).contains(&bucket_count) {
            return Err(MediaError::InvalidRequest("waveform bucket count"));
        }
        let installation = self
            .installations
            .read(installation_id)?
            .ok_or_else(|| MediaError::InstallationNotFound(installation_id.0.clone()))?;
        validate_installation(&installation)?;
        let selection = self.resolve_selection(&installation)?;
        if selection.action != LaunchActionKind::PlayAudio {
            return Err(MediaError::UnsupportedAction);
        }
        let item = self
            .resolve_items(&installation, &selection, indexed_at, false)?
            .into_iter()
            .find(|item| item.ordinal == ordinal)
            .ok_or(MediaError::ItemNotFound(ordinal))?;
        self.waveforms
            .read_waveform(&selection.root_path, &item.relative_path, bucket_count)
    }

    pub fn update_queue_settings(
        &self,
        request: UpdateMediaQueueSettingsRequest,
    ) -> Result<MediaSession, MediaError> {
        if request.updated_at.trim().is_empty() {
            return Err(MediaError::InvalidRequest("updated timestamp"));
        }
        let mut session = self.read(&request.session_id)?;
        if !session.status.is_open() {
            return Err(MediaError::SessionClosed);
        }
        session.repeat_mode = request.repeat_mode;
        session.shuffle = request.shuffle;
        session.updated_at = request.updated_at;
        self.sessions.save_media_session(&session)?;
        Ok(session)
    }

    pub fn read(&self, session_id: &MediaSessionId) -> Result<MediaSession, MediaError> {
        self.sessions
            .read_media_session(session_id)?
            .ok_or_else(|| MediaError::SessionNotFound(session_id.0.clone()))
    }

    pub fn update_progress(
        &self,
        request: UpdateMediaProgressRequest,
    ) -> Result<MediaSession, MediaError> {
        if request.updated_at.trim().is_empty() {
            return Err(MediaError::InvalidRequest("updated timestamp"));
        }
        if !matches!(
            request.status,
            MediaSessionStatus::Active | MediaSessionStatus::Paused | MediaSessionStatus::Completed
        ) {
            return Err(MediaError::InvalidProgressState);
        }
        if request.completed != (request.status == MediaSessionStatus::Completed) {
            return Err(MediaError::InvalidProgressState);
        }
        let mut session = self.read(&request.session_id)?;
        if !session.status.is_open() {
            return Err(MediaError::SessionClosed);
        }
        if !session
            .items
            .iter()
            .any(|item| item.ordinal == request.item_ordinal)
        {
            return Err(MediaError::ItemNotFound(request.item_ordinal));
        }
        let position_ms = request.duration_ms.map_or(request.position_ms, |duration| {
            request.position_ms.min(duration)
        });
        session.progress = MediaProgress {
            item_ordinal: request.item_ordinal,
            position_ms,
            duration_ms: request.duration_ms,
            completed: request.completed,
            updated_at: request.updated_at.clone(),
        };
        session.status = request.status;
        session.updated_at = request.updated_at.clone();
        if request.status == MediaSessionStatus::Completed {
            session.ended_at = Some(request.updated_at);
        }
        self.sessions.save_media_session(&session)?;
        Ok(session)
    }

    pub fn close(
        &self,
        session_id: &MediaSessionId,
        closed_at: String,
    ) -> Result<MediaSession, MediaError> {
        if closed_at.trim().is_empty() {
            return Err(MediaError::InvalidRequest("closed timestamp"));
        }
        let mut session = self.read(session_id)?;
        if session.status.is_open() {
            session.status = MediaSessionStatus::Closed;
            session.updated_at = closed_at.clone();
            session.ended_at = Some(closed_at);
            self.sessions.save_media_session(&session)?;
        }
        Ok(session)
    }

    pub fn resolve_asset(
        &self,
        session_id: &MediaSessionId,
        ordinal: u32,
    ) -> Result<MediaAssetDescriptor, MediaError> {
        let session = self.read(session_id)?;
        if matches!(
            session.status,
            MediaSessionStatus::Closed
                | MediaSessionStatus::Interrupted
                | MediaSessionStatus::Failed
        ) {
            return Err(MediaError::SessionClosed);
        }
        let item = session
            .items
            .into_iter()
            .find(|item| item.ordinal == ordinal)
            .ok_or(MediaError::ItemNotFound(ordinal))?;
        let installation = self
            .installations
            .read(&item.installation_id)?
            .ok_or_else(|| MediaError::InstallationNotFound(item.installation_id.0.clone()))?;
        let root_path = self
            .preparations
            .read_prepared_package(&installation.id)?
            .map_or(installation.root_path, |prepared| prepared.destination_root);
        Ok(MediaAssetDescriptor { root_path, item })
    }

    fn resolve_selection(&self, installation: &Installation) -> Result<MediaSelection, MediaError> {
        if let Some(prepared) = self.read_prepared_package(&installation.id)? {
            let action = prepared.preferred_action.ok_or(MediaError::MissingAction)?;
            return Ok(MediaSelection {
                root_path: prepared.destination_root,
                action: action.action,
                target: LaunchTarget::RelativePath(action.relative_path),
                prepared: true,
            });
        }
        let selected = installation
            .overrides
            .preferred_action
            .clone()
            .ok_or(MediaError::MissingAction)?;
        Ok(MediaSelection {
            root_path: installation.root_path.clone(),
            action: selected.action,
            target: selected.target,
            prepared: false,
        })
    }

    fn resolve_items(
        &self,
        installation: &Installation,
        selection: &MediaSelection,
        indexed_at: &str,
        persist_audio_tracks: bool,
    ) -> Result<Vec<MediaSessionItem>, MediaError> {
        let media_type = media_type_for_action(selection.action)?;
        let inventory = if selection.prepared {
            self.inventory.read_inventory(&selection.root_path)?
        } else {
            direct_inventory(installation)
        };
        let mut matching = inventory
            .into_iter()
            .filter(|item| item.media_type == media_type)
            .filter(|item| {
                target_includes(selection.action, &selection.target, &item.relative_path)
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            if ignored_target(installation, &selection.target) {
                return Err(MediaError::IgnoredTarget);
            }
            return Err(MediaError::EmptyInventory);
        }
        let manual_order = installation
            .overrides
            .content_items
            .iter()
            .filter_map(|item| item.order.map(|order| (item.relative_path.clone(), order)))
            .collect::<BTreeMap<_, _>>();
        if selection.action == LaunchActionKind::PlayAudio {
            let tracks = ordered_audio_tracks(installation, matching, &manual_order, indexed_at)?;
            if persist_audio_tracks {
                self.audio_tracks
                    .replace_audio_tracks(&installation.id, &tracks)?;
            }
            return tracks
                .into_iter()
                .enumerate()
                .map(|(ordinal, track)| media_item_from_audio_track(ordinal, track))
                .collect();
        }
        matching.sort_by(|left, right| compare_media_items(left, right, &manual_order));
        matching
            .into_iter()
            .enumerate()
            .map(|(ordinal, item)| {
                Ok(MediaSessionItem {
                    ordinal: checked_ordinal(ordinal)?,
                    installation_id: installation.id.clone(),
                    work_code: installation
                        .effective_catalog_work_code()
                        .map(str::to_owned),
                    relative_path: item.relative_path,
                    media_type: item.media_type,
                    size_bytes: item.size_bytes,
                    disc_number: None,
                    track_number: None,
                    bonus: false,
                })
            })
            .collect()
    }

    fn index_audio_items(
        &self,
        installation: &Installation,
        indexed_at: &str,
    ) -> Result<Vec<MediaSessionItem>, MediaError> {
        validate_installation(installation)?;
        let selection = self.resolve_selection(installation)?;
        if selection.action != LaunchActionKind::PlayAudio {
            return Err(MediaError::UnsupportedAction);
        }
        self.resolve_items(installation, &selection, indexed_at, true)
    }
}

pub struct PersonalizedVoiceQueueService {
    media: Arc<MediaService>,
    installations: Arc<dyn InstallationStore>,
    sessions: Arc<dyn MediaSessionStore>,
    preferences: Arc<dyn WorkPreferenceStore>,
    activity: Arc<dyn LibraryActivityReader>,
}

impl PersonalizedVoiceQueueService {
    pub fn new(
        media: Arc<MediaService>,
        installations: Arc<dyn InstallationStore>,
        sessions: Arc<dyn MediaSessionStore>,
        preferences: Arc<dyn WorkPreferenceStore>,
        activity: Arc<dyn LibraryActivityReader>,
    ) -> Self {
        Self {
            media,
            installations,
            sessions,
            preferences,
            activity,
        }
    }

    pub fn open(
        &self,
        request: OpenPersonalizedVoiceQueueRequest,
    ) -> Result<MediaSession, MediaError> {
        if request.session_id.0.trim().is_empty() {
            return Err(MediaError::InvalidRequest("session ID"));
        }
        if request.opened_at.trim().is_empty() {
            return Err(MediaError::InvalidRequest("opened timestamp"));
        }
        if let Some(session) = self.sessions.read_open_personalized_media_session()? {
            return Ok(session);
        }

        let preferences = self
            .preferences
            .list_work_preferences()?
            .into_iter()
            .map(|preference| {
                (
                    canonical_work_code(&preference.work_code),
                    preference.preference,
                )
            })
            .collect::<HashMap<_, _>>();
        let mut recent = self.activity.read_library_activity()?.recent;
        recent.sort_by(|left, right| {
            right
                .occurred_at
                .cmp(&left.occurred_at)
                .then_with(|| left.installation_id.cmp(&right.installation_id))
        });
        let listening_rank = recent
            .into_iter()
            .filter(|activity| activity.action == Some(LaunchActionKind::PlayAudio))
            .enumerate()
            .map(|(rank, activity)| (activity.installation_id, rank))
            .collect::<HashMap<_, _>>();

        let mut candidates = Vec::new();
        for installation in self.installations.list()? {
            let work_code = installation
                .effective_catalog_work_code()
                .map(str::to_owned);
            let preference = work_code
                .as_deref()
                .and_then(|code| preferences.get(&canonical_work_code(code)))
                .copied();
            if preference == Some(WorkPreferenceKind::NotInterested) {
                continue;
            }
            let listened_rank = listening_rank.get(&installation.id).copied();
            let favorite = preference == Some(WorkPreferenceKind::Favorite);
            if listened_rank.is_none() && !favorite {
                continue;
            }
            let items = match self
                .media
                .index_audio_items(&installation, &request.opened_at)
            {
                Ok(items) => items,
                Err(
                    MediaError::NeedsReview
                    | MediaError::NotReviewed
                    | MediaError::MissingAction
                    | MediaError::UnsupportedAction
                    | MediaError::MissingContentTarget
                    | MediaError::IgnoredTarget
                    | MediaError::EmptyInventory,
                ) => continue,
                Err(error) => return Err(error),
            };
            candidates.push(VoiceQueueCandidate {
                installation_id: installation.id,
                work_code,
                listened_rank,
                favorite,
                items: items.into(),
            });
        }

        let listened_works = candidates
            .iter()
            .filter(|candidate| candidate.listened_rank.is_some())
            .map(VoiceQueueCandidate::identity_key)
            .collect::<HashSet<_>>()
            .len();
        if listened_works < PERSONALIZED_VOICE_MINIMUM_WORKS {
            return Err(MediaError::InsufficientVoiceActivity(
                PERSONALIZED_VOICE_MINIMUM_WORKS,
            ));
        }
        candidates.sort_by(|left, right| {
            right
                .favorite
                .cmp(&left.favorite)
                .then_with(|| compare_optional_ranks(left.listened_rank, right.listened_rank))
                .then_with(|| left.identity_key().cmp(&right.identity_key()))
        });
        let items = interleave_voice_queue(candidates)?;
        if items.is_empty() {
            return Err(MediaError::EmptyInventory);
        }
        let queue_state = self
            .sessions
            .read_media_queue_state(MediaSessionKind::PersonalizedVoice, None)?;
        let progress = initial_progress(&items, queue_state.as_ref(), None, &request.opened_at);
        let session = MediaSession {
            id: request.session_id,
            kind: MediaSessionKind::PersonalizedVoice,
            installation_id: items[0].installation_id.clone(),
            action: LaunchActionKind::PlayAudio,
            status: MediaSessionStatus::Active,
            repeat_mode: queue_state
                .as_ref()
                .map_or(MediaRepeatMode::Off, |state| state.repeat_mode),
            shuffle: queue_state.as_ref().is_some_and(|state| state.shuffle),
            items,
            progress,
            opened_at: request.opened_at.clone(),
            updated_at: request.opened_at,
            ended_at: None,
            error: None,
        };
        self.sessions.create_media_session(&session)?;
        Ok(session)
    }
}

struct VoiceQueueCandidate {
    installation_id: InstallationId,
    work_code: Option<String>,
    listened_rank: Option<usize>,
    favorite: bool,
    items: VecDeque<MediaSessionItem>,
}

impl VoiceQueueCandidate {
    fn identity_key(&self) -> String {
        self.work_code
            .as_deref()
            .map(canonical_work_code)
            .unwrap_or_else(|| self.installation_id.0.clone())
    }
}

fn compare_optional_ranks(left: Option<usize>, right: Option<usize>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn interleave_voice_queue(
    mut candidates: Vec<VoiceQueueCandidate>,
) -> Result<Vec<MediaSessionItem>, MediaError> {
    let mut items = Vec::new();
    while items.len() < PERSONALIZED_VOICE_QUEUE_LIMIT {
        let mut added = false;
        for candidate in &mut candidates {
            if let Some(mut item) = candidate.items.pop_front() {
                item.ordinal = checked_ordinal(items.len())?;
                items.push(item);
                added = true;
                if items.len() == PERSONALIZED_VOICE_QUEUE_LIMIT {
                    break;
                }
            }
        }
        if !added {
            break;
        }
    }
    Ok(items)
}

fn canonical_work_code(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

struct MediaSelection {
    root_path: String,
    action: LaunchActionKind,
    target: LaunchTarget,
    prepared: bool,
}

fn validate_open_request(request: &OpenMediaSessionRequest) -> Result<(), MediaError> {
    if request.installation_id.0.trim().is_empty() {
        return Err(MediaError::InvalidRequest("installation ID"));
    }
    if request.session_id.0.trim().is_empty() {
        return Err(MediaError::InvalidRequest("session ID"));
    }
    if request.opened_at.trim().is_empty() {
        return Err(MediaError::InvalidRequest("opened timestamp"));
    }
    Ok(())
}

fn validate_installation(installation: &Installation) -> Result<(), MediaError> {
    if installation.status != InstallationStatus::Ready {
        return Err(MediaError::NeedsReview);
    }
    if installation.overrides.reviewed_at.is_none() {
        return Err(MediaError::NotReviewed);
    }
    Ok(())
}

fn media_type_for_action(action: LaunchActionKind) -> Result<MediaType, MediaError> {
    match action {
        LaunchActionKind::PlayAudio => Ok(MediaType::Audio),
        LaunchActionKind::ReadImages => Ok(MediaType::Image),
        LaunchActionKind::OpenDocument => Ok(MediaType::Pdf),
        LaunchActionKind::PlayVideo => Ok(MediaType::Video),
        _ => Err(MediaError::UnsupportedAction),
    }
}

fn direct_inventory(installation: &Installation) -> Vec<MediaInventoryItem> {
    installation
        .detection
        .content_items
        .iter()
        .filter_map(|item| effective_direct_item(installation, item))
        .collect()
}

fn effective_direct_item(
    installation: &Installation,
    item: &ContentItem,
) -> Option<MediaInventoryItem> {
    let override_value = installation
        .overrides
        .content_items
        .iter()
        .find(|candidate| candidate.relative_path == item.relative_path);
    if override_value.is_some_and(|value| value.ignored) {
        return None;
    }
    Some(MediaInventoryItem {
        relative_path: item.relative_path.clone(),
        media_type: override_value
            .and_then(|value| value.media_type)
            .unwrap_or(item.media_type),
        size_bytes: item.size_bytes,
    })
}

fn target_includes(
    action: LaunchActionKind,
    target: &LaunchTarget,
    candidate: &RelativePath,
) -> bool {
    let LaunchTarget::RelativePath(target) = target else {
        return true;
    };
    if matches!(
        action,
        LaunchActionKind::PlayAudio | LaunchActionKind::ReadImages
    ) {
        return parent_path(target.as_str()) == parent_path(candidate.as_str());
    }
    target == candidate
}

fn parent_path(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn ignored_target(installation: &Installation, target: &LaunchTarget) -> bool {
    let LaunchTarget::RelativePath(target) = target else {
        return false;
    };
    installation
        .overrides
        .content_items
        .iter()
        .any(|item| item.relative_path == *target && item.ignored)
}

fn initial_progress(
    items: &[MediaSessionItem],
    queue_state: Option<&MediaQueueState>,
    resume: Option<MediaResume>,
    opened_at: &str,
) -> MediaProgress {
    let restored_queue = queue_state
        .filter(|state| !state.completed)
        .and_then(|state| {
            items
                .iter()
                .find(|item| {
                    item.installation_id == state.current_installation_id
                        && item.relative_path == state.current_relative_path
                })
                .map(|item| (item.ordinal, state.position_ms, state.duration_ms))
        });
    let restored_resume = resume
        .filter(|resume| !resume.completed)
        .and_then(|resume| {
            items
                .iter()
                .find(|item| item.relative_path == resume.relative_path)
                .map(|item| (item.ordinal, resume.position_ms, resume.duration_ms))
        });
    let (item_ordinal, position_ms, duration_ms) =
        restored_queue.or(restored_resume).unwrap_or((0, 0, None));
    MediaProgress {
        item_ordinal,
        position_ms,
        duration_ms,
        completed: false,
        updated_at: opened_at.to_owned(),
    }
}

fn ordered_audio_tracks(
    installation: &Installation,
    items: Vec<MediaInventoryItem>,
    manual_order: &BTreeMap<RelativePath, u32>,
    indexed_at: &str,
) -> Result<Vec<IndexedAudioTrack>, MediaError> {
    let mut tracks = items
        .into_iter()
        .map(|item| {
            let metadata = audio_ordering_metadata(item.relative_path.as_str());
            (item, metadata)
        })
        .collect::<Vec<_>>();
    tracks.sort_by(|(left, left_metadata), (right, right_metadata)| {
        compare_manual_order(&left.relative_path, &right.relative_path, manual_order)
            .then_with(|| left_metadata.bonus.cmp(&right_metadata.bonus))
            .then_with(|| compare_optional_numbers(left_metadata.disc, right_metadata.disc))
            .then_with(|| compare_optional_numbers(left_metadata.track, right_metadata.track))
            .then_with(|| natural_cmp(left.relative_path.as_str(), right.relative_path.as_str()))
    });
    tracks
        .into_iter()
        .enumerate()
        .map(|(sort_order, (item, metadata))| {
            Ok(IndexedAudioTrack {
                installation_id: installation.id.clone(),
                work_code: installation
                    .effective_catalog_work_code()
                    .map(str::to_owned),
                relative_path: item.relative_path,
                size_bytes: item.size_bytes,
                disc_number: metadata.disc,
                track_number: metadata.track,
                bonus: metadata.bonus,
                sort_order: checked_ordinal(sort_order)?,
                indexed_at: indexed_at.to_owned(),
            })
        })
        .collect()
}

fn media_item_from_audio_track(
    ordinal: usize,
    track: IndexedAudioTrack,
) -> Result<MediaSessionItem, MediaError> {
    Ok(MediaSessionItem {
        ordinal: checked_ordinal(ordinal)?,
        installation_id: track.installation_id,
        work_code: track.work_code,
        relative_path: track.relative_path,
        media_type: MediaType::Audio,
        size_bytes: track.size_bytes,
        disc_number: track.disc_number,
        track_number: track.track_number,
        bonus: track.bonus,
    })
}

fn compare_media_items(
    left: &MediaInventoryItem,
    right: &MediaInventoryItem,
    manual_order: &BTreeMap<RelativePath, u32>,
) -> Ordering {
    compare_manual_order(&left.relative_path, &right.relative_path, manual_order)
        .then_with(|| natural_cmp(left.relative_path.as_str(), right.relative_path.as_str()))
}

fn compare_manual_order(
    left: &RelativePath,
    right: &RelativePath,
    manual_order: &BTreeMap<RelativePath, u32>,
) -> Ordering {
    match (manual_order.get(left), manual_order.get(right)) {
        (Some(left), Some(right)) => left.cmp(right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_numbers(left: Option<u32>, right: Option<u32>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[derive(Clone, Copy)]
struct AudioOrderingMetadata {
    disc: Option<u32>,
    track: Option<u32>,
    bonus: bool,
}

fn audio_ordering_metadata(path: &str) -> AudioOrderingMetadata {
    let normalized = path.to_lowercase();
    let bonus = ["bonus", "extra", "tokuten", "omake", "特典", "おまけ"]
        .iter()
        .any(|signal| normalized.contains(signal));
    let disc = number_after_any(&normalized, &["disc", "disk", "cd", "ディスク"]);
    let filename = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    let track = number_after_any(filename, &["track", "trk", "tr"])
        .or_else(|| first_ascii_number(filename));
    AudioOrderingMetadata { disc, track, bonus }
}

fn number_after_any(value: &str, signals: &[&str]) -> Option<u32> {
    signals.iter().find_map(|signal| {
        let start = value.find(signal)? + signal.len();
        first_ascii_number(&value[start..])
    })
}

fn first_ascii_number(value: &str) -> Option<u32> {
    let start = value.find(|character: char| character.is_ascii_digit())?;
    let digits = value[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits.parse().ok()
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let left = left.to_lowercase();
    let right = right.to_lowercase();
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() && right_index < right.len() {
        let left_digit = left.as_bytes()[left_index].is_ascii_digit();
        let right_digit = right.as_bytes()[right_index].is_ascii_digit();
        if left_digit && right_digit {
            let left_end = digit_run_end(&left, left_index);
            let right_end = digit_run_end(&right, right_index);
            let left_number = left[left_index..left_end]
                .parse::<u64>()
                .unwrap_or(u64::MAX);
            let right_number = right[right_index..right_end]
                .parse::<u64>()
                .unwrap_or(u64::MAX);
            let ordering = left_number
                .cmp(&right_number)
                .then_with(|| (left_end - left_index).cmp(&(right_end - right_index)));
            if ordering != Ordering::Equal {
                return ordering;
            }
            left_index = left_end;
            right_index = right_end;
            continue;
        }
        let left_character = left[left_index..].chars().next().expect("left character");
        let right_character = right[right_index..]
            .chars()
            .next()
            .expect("right character");
        let ordering = left_character.cmp(&right_character);
        if ordering != Ordering::Equal {
            return ordering;
        }
        left_index += left_character.len_utf8();
        right_index += right_character.len_utf8();
    }
    left.len().cmp(&right.len())
}

fn digit_run_end(value: &str, start: usize) -> usize {
    value[start..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count()
        + start
}

fn checked_ordinal(value: usize) -> Result<u32, MediaError> {
    u32::try_from(value).map_err(|_| MediaError::InvalidRequest("media queue is too large"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_domain::installation::{
        ContentItemOverride, InferenceConfidence, InstallationDetection, InstallationOverrides,
        InstallationPlatform, ManualCatalogIdentity, ManualLaunchSelection,
    };
    use dla_domain::package::{
        ArchiveRetentionPolicy, PackageSourceSet, PackageSourceSetKind, PreparedPackageInstallation,
    };

    use super::*;

    struct MemoryInstallationStore {
        installation: Installation,
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
            Ok((self.installation.id == *installation_id).then(|| self.installation.clone()))
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(vec![self.installation.clone()])
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

    #[derive(Default)]
    struct MemoryPreparationStore;

    impl PackagePreparationStore for MemoryPreparationStore {
        fn read_prepared_package(
            &self,
            _installation_id: &InstallationId,
        ) -> Result<Option<PreparedPackageInstallation>, PackagePreparationError> {
            Ok(None)
        }

        fn save_prepared_package(
            &self,
            _prepared: &PreparedPackageInstallation,
        ) -> Result<(), PackagePreparationError> {
            unreachable!()
        }
    }

    struct FixedPreparationStore {
        prepared: PreparedPackageInstallation,
    }

    impl PackagePreparationStore for FixedPreparationStore {
        fn read_prepared_package(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<PreparedPackageInstallation>, PackagePreparationError> {
            Ok((self.prepared.installation_id == *installation_id).then(|| self.prepared.clone()))
        }

        fn save_prepared_package(
            &self,
            _prepared: &PreparedPackageInstallation,
        ) -> Result<(), PackagePreparationError> {
            unreachable!()
        }
    }

    struct MemorySessionStore {
        session: Mutex<Option<MediaSession>>,
        resume: Option<MediaResume>,
    }

    impl MediaSessionStore for MemorySessionStore {
        fn create_media_session(&self, session: &MediaSession) -> Result<(), MediaError> {
            *self.session.lock().expect("media session") = Some(session.clone());
            Ok(())
        }

        fn save_media_session(&self, session: &MediaSession) -> Result<(), MediaError> {
            *self.session.lock().expect("media session") = Some(session.clone());
            Ok(())
        }

        fn read_media_session(
            &self,
            session_id: &MediaSessionId,
        ) -> Result<Option<MediaSession>, MediaError> {
            Ok(self
                .session
                .lock()
                .expect("media session")
                .as_ref()
                .filter(|session| session.id == *session_id)
                .cloned())
        }

        fn read_open_media_session(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<MediaSession>, MediaError> {
            Ok(self
                .session
                .lock()
                .expect("media session")
                .as_ref()
                .filter(|session| {
                    session.installation_id == *installation_id && session.status.is_open()
                })
                .cloned())
        }

        fn read_open_personalized_media_session(&self) -> Result<Option<MediaSession>, MediaError> {
            Ok(self
                .session
                .lock()
                .expect("media session")
                .as_ref()
                .filter(|session| {
                    session.kind == MediaSessionKind::PersonalizedVoice && session.status.is_open()
                })
                .cloned())
        }

        fn read_media_queue_state(
            &self,
            _kind: MediaSessionKind,
            _installation_id: Option<&InstallationId>,
        ) -> Result<Option<MediaQueueState>, MediaError> {
            Ok(None)
        }

        fn read_media_resume(
            &self,
            installation_id: &InstallationId,
            action: LaunchActionKind,
        ) -> Result<Option<MediaResume>, MediaError> {
            Ok(self
                .resume
                .as_ref()
                .filter(|resume| {
                    resume.installation_id == *installation_id && resume.action == action
                })
                .cloned())
        }

        fn interrupt_open_media_sessions(
            &self,
            _interrupted_at: &str,
            _reason: &str,
        ) -> Result<u64, MediaError> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct MemoryAudioTrackStore;

    impl AudioTrackStore for MemoryAudioTrackStore {
        fn replace_audio_tracks(
            &self,
            _installation_id: &InstallationId,
            _tracks: &[IndexedAudioTrack],
        ) -> Result<(), MediaError> {
            Ok(())
        }

        fn list_audio_tracks(
            &self,
            _installation_id: &InstallationId,
        ) -> Result<Vec<IndexedAudioTrack>, MediaError> {
            Ok(Vec::new())
        }

        fn list_all_audio_tracks(&self) -> Result<Vec<IndexedAudioTrack>, MediaError> {
            Ok(Vec::new())
        }
    }

    #[derive(Default)]
    struct UnusedInventory;

    impl MediaInventoryReader for UnusedInventory {
        fn read_inventory(&self, _root_path: &str) -> Result<Vec<MediaInventoryItem>, MediaError> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct UnusedWaveforms;

    impl AudioWaveformReader for UnusedWaveforms {
        fn read_waveform(
            &self,
            _root_path: &str,
            _relative_path: &RelativePath,
            _bucket_count: u32,
        ) -> Result<AudioWaveform, MediaError> {
            unreachable!()
        }
    }

    struct FixedInventory {
        items: Vec<MediaInventoryItem>,
    }

    impl MediaInventoryReader for FixedInventory {
        fn read_inventory(&self, _root_path: &str) -> Result<Vec<MediaInventoryItem>, MediaError> {
            Ok(self.items.clone())
        }
    }

    fn content(path: &str, media_type: MediaType) -> ContentItem {
        ContentItem {
            relative_path: RelativePath::parse(path).expect("fixture path"),
            path_key: path.to_owned(),
            media_type,
            size_bytes: Some(1_024),
            modified_at: None,
            confidence: InferenceConfidence::High,
            reason_codes: vec!["file_extension".to_owned()],
        }
    }

    fn installation_fixture() -> Installation {
        let first = RelativePath::parse("disc/01.flac").expect("first track");
        let second = RelativePath::parse("disc/02.flac").expect("second track");
        Installation {
            id: InstallationId("installation-audio".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/audio".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: InstallationStatus::Ready,
                content_items: vec![
                    content(first.as_str(), MediaType::Audio),
                    content(second.as_str(), MediaType::Audio),
                    content("disc/cover.webp", MediaType::Image),
                ],
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                catalog_identity: Some(ManualCatalogIdentity::Unidentified),
                custom_title: None,
                preferred_action: Some(ManualLaunchSelection {
                    action: LaunchActionKind::PlayAudio,
                    target: LaunchTarget::RelativePath(first.clone()),
                }),
                content_items: vec![
                    ContentItemOverride {
                        relative_path: second,
                        media_type: None,
                        ignored: false,
                        order: Some(0),
                    },
                    ContentItemOverride {
                        relative_path: first,
                        media_type: None,
                        ignored: false,
                        order: Some(1),
                    },
                ],
                reviewed_at: Some("2026-08-09T10:00:00Z".to_owned()),
            },
            discovered_at: "2026-08-09T09:00:00Z".to_owned(),
            updated_at: "2026-08-09T10:00:00Z".to_owned(),
        }
    }

    fn service_with_resume(resume: Option<MediaResume>) -> MediaService {
        let installation = installation_fixture();
        MediaService::new(
            Arc::new(MemoryInstallationStore { installation }),
            Arc::new(MemoryPreparationStore),
            Arc::new(MemorySessionStore {
                session: Mutex::new(None),
                resume,
            }),
            Arc::new(MemoryAudioTrackStore),
            Arc::new(UnusedInventory),
            Arc::new(UnusedWaveforms),
        )
    }

    #[test]
    fn opens_reviewed_media_in_manual_order_and_restores_matching_resume() {
        let installation = installation_fixture();
        let service = service_with_resume(Some(MediaResume {
            installation_id: installation.id.clone(),
            action: LaunchActionKind::PlayAudio,
            relative_path: RelativePath::parse("disc/01.flac").expect("resume path"),
            position_ms: 42_000,
            duration_ms: Some(180_000),
            completed: false,
            updated_at: "2026-08-09T11:00:00Z".to_owned(),
        }));

        let session = service
            .open(OpenMediaSessionRequest {
                installation_id: installation.id,
                session_id: MediaSessionId("media-audio".to_owned()),
                opened_at: "2026-08-09T12:00:00Z".to_owned(),
            })
            .expect("open media session");

        assert_eq!(session.items.len(), 2);
        assert_eq!(session.items[0].relative_path.as_str(), "disc/02.flac");
        assert_eq!(session.items[1].relative_path.as_str(), "disc/01.flac");
        assert_eq!(session.progress.item_ordinal, 1);
        assert_eq!(session.progress.position_ms, 42_000);
        assert_eq!(session.progress.duration_ms, Some(180_000));
    }

    #[test]
    fn recovers_a_playable_album_from_an_older_prepared_package_without_an_action() {
        let installation = installation_fixture();
        let installation_id = installation.id.clone();
        let prepared = PreparedPackageInstallation {
            installation_id: installation_id.clone(),
            destination_root: "/synthetic/prepared/RJ01678999".to_owned(),
            content_root: None,
            preferred_action: None,
            source_set: PackageSourceSet {
                kind: PackageSourceSetKind::SingleArchive,
                volumes: Vec::new(),
            },
            archive_retention: ArchiveRetentionPolicy::Keep,
            sources_deleted: false,
            source_cleanup_error: None,
            installed_file_count: 5,
            installed_bytes: 245,
            prepared_at: "2026-08-13T00:00:00Z".to_owned(),
        };
        let inventory_item = |path: &str, media_type| MediaInventoryItem {
            relative_path: RelativePath::parse(path).expect("inventory path"),
            media_type,
            size_bytes: Some(1),
        };
        let service = MediaService::new(
            Arc::new(MemoryInstallationStore { installation }),
            Arc::new(FixedPreparationStore { prepared }),
            Arc::new(MemorySessionStore {
                session: Mutex::new(None),
                resume: None,
            }),
            Arc::new(MemoryAudioTrackStore),
            Arc::new(FixedInventory {
                items: vec![
                    inventory_item("mp3/sa02_01.mp3", MediaType::Audio),
                    inventory_item("mp3/sa02_02.mp3", MediaType::Audio),
                    inventory_item("wav/sa02_01.wav", MediaType::Audio),
                    inventory_item("wav/sa02_02.wav", MediaType::Audio),
                    inventory_item("omake/cover.jpg", MediaType::Image),
                ],
            }),
            Arc::new(UnusedWaveforms),
        );

        let projected = service
            .read_prepared_package(&installation_id)
            .expect("read prepared package")
            .expect("prepared package");
        let action = projected.preferred_action.expect("recovered action");
        assert_eq!(action.action, LaunchActionKind::PlayAudio);
        assert_eq!(action.relative_path.as_str(), "mp3/sa02_01.mp3");
        assert_eq!(
            projected.content_root.as_ref().map(RelativePath::as_str),
            Some("mp3")
        );

        let session = service
            .open(OpenMediaSessionRequest {
                installation_id,
                session_id: MediaSessionId("media-prepared-audio".to_owned()),
                opened_at: "2026-08-13T01:00:00Z".to_owned(),
            })
            .expect("open recovered media session");
        assert_eq!(
            session
                .items
                .iter()
                .map(|item| item.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["mp3/sa02_01.mp3", "mp3/sa02_02.mp3"]
        );
    }

    #[test]
    fn orders_disc_tracks_naturally_and_places_bonus_content_last() {
        let installation = installation_fixture();
        let items = vec![
            MediaInventoryItem {
                relative_path: RelativePath::parse("bonus/01.flac").expect("bonus"),
                media_type: MediaType::Audio,
                size_bytes: None,
            },
            MediaInventoryItem {
                relative_path: RelativePath::parse("disc2/track10.flac").expect("track 10"),
                media_type: MediaType::Audio,
                size_bytes: None,
            },
            MediaInventoryItem {
                relative_path: RelativePath::parse("disc2/track2.flac").expect("track 2"),
                media_type: MediaType::Audio,
                size_bytes: None,
            },
            MediaInventoryItem {
                relative_path: RelativePath::parse("disc1/track03.flac").expect("disc 1"),
                media_type: MediaType::Audio,
                size_bytes: None,
            },
        ];

        let tracks = ordered_audio_tracks(
            &installation,
            items,
            &BTreeMap::new(),
            "2026-08-10T00:00:00Z",
        )
        .expect("ordered tracks");

        assert_eq!(
            tracks
                .iter()
                .map(|track| track.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "disc1/track03.flac",
                "disc2/track2.flac",
                "disc2/track10.flac",
                "bonus/01.flac"
            ]
        );
        assert!(tracks.last().is_some_and(|track| track.bonus));
    }

    #[test]
    fn personalized_voice_queue_interleaves_works_without_losing_track_order() {
        let candidate = |installation: &str, work: &str, paths: &[&str]| VoiceQueueCandidate {
            installation_id: InstallationId(installation.to_owned()),
            work_code: Some(work.to_owned()),
            listened_rank: Some(0),
            favorite: false,
            items: paths
                .iter()
                .enumerate()
                .map(|(ordinal, path)| MediaSessionItem {
                    ordinal: u32::try_from(ordinal).expect("ordinal"),
                    installation_id: InstallationId(installation.to_owned()),
                    work_code: Some(work.to_owned()),
                    relative_path: RelativePath::parse(*path).expect("track path"),
                    media_type: MediaType::Audio,
                    size_bytes: None,
                    disc_number: Some(1),
                    track_number: Some(u32::try_from(ordinal + 1).expect("track")),
                    bonus: false,
                })
                .collect(),
        };

        let queue = interleave_voice_queue(vec![
            candidate("installation-a", "RJA", &["01.flac", "02.flac"]),
            candidate("installation-b", "RJB", &["01.mp3", "02.mp3"]),
        ])
        .expect("voice queue");

        assert_eq!(
            queue
                .iter()
                .map(|item| item.installation_id.0.as_str())
                .collect::<Vec<_>>(),
            vec![
                "installation-a",
                "installation-b",
                "installation-a",
                "installation-b"
            ]
        );
        assert_eq!(
            queue.iter().map(|item| item.ordinal).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
    }

    #[test]
    fn rejects_progress_when_completed_flag_and_session_status_disagree() {
        let installation = installation_fixture();
        let service = service_with_resume(None);
        let session = service
            .open(OpenMediaSessionRequest {
                installation_id: installation.id,
                session_id: MediaSessionId("media-progress".to_owned()),
                opened_at: "2026-08-09T12:00:00Z".to_owned(),
            })
            .expect("open media session");

        let error = service
            .update_progress(UpdateMediaProgressRequest {
                session_id: session.id,
                item_ordinal: 0,
                position_ms: 120_000,
                duration_ms: Some(120_000),
                completed: false,
                status: MediaSessionStatus::Completed,
                updated_at: "2026-08-09T12:02:00Z".to_owned(),
            })
            .expect_err("inconsistent progress must fail");

        assert!(matches!(error, MediaError::InvalidProgressState));
    }
}
