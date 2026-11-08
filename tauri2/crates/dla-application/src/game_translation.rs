//! Turning translation on and off for one installed game: the launcher adds
//! its plugin to the game's folder, translates the whole game ahead of play,
//! hands the plugin the relay and the translations known so far before each
//! start, and puts every file back on removal.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use dla_domain::{
    game_engine::{GameEngine, GameEngineDetection},
    installation::{Installation, InstallationId},
    translation::{
        GameFileChange, GameTranslation, InstalledPack, PreparedTranslation, TextKind, TextMarkup,
    },
};
use serde::Serialize;
use thiserror::Error;

use crate::{
    installation::{InstallationLibraryError, InstallationStore},
    package_preparation::{PackagePreparationError, PackagePreparationStore},
    translation::{
        PackLookup, SourceText, TranslationError, TranslationPacks, TranslationService,
        TranslatorError, Urgency, WorkTranslations,
    },
};

// Batches while nobody plays, and smaller ones while a game runs: a player
// waiting for a line goes next, once the batch in progress is done.
const DIALOGUE_BATCH: usize = 10;
const TERM_BATCH: usize = 25;
const DIALOGUE_BATCH_WHILE_PLAYING: usize = 4;
const TERM_BATCH_WHILE_PLAYING: usize = 10;
const IDLE_CHECK: Duration = Duration::from_secs(5);
// How often the game's copy of the translations is written while preparing,
// for plugins that read only that copy.
const COPY_INTERVAL: Duration = Duration::from_secs(60);
// The time left comes from the latest batches, so it follows a game starting
// or the translator slowing down, and goes by length: a text costs its
// characters plus this many for its line. With Qwen3 4B in the Windows VM,
// menu words and dialogue then took 0.08-0.10 s per unit alike; counted per
// text, the quick menus had promised "3 min left" for 10 minutes.
const RECENT_BATCHES: usize = 4;
const TEXT_OVERHEAD: u64 = 10;

#[derive(Debug, Error)]
pub enum GameTranslationError {
    #[error("installation was not found: {0}")]
    NotFound(String),
    #[error("the launcher cannot translate games made with {0} yet")]
    Unsupported(&'static str),
    #[error("the launcher found no game engine it knows in this folder")]
    NoEngine,
    #[error("the game folder could not be changed: {0}")]
    Folder(String),
    #[error("game translation persistence failed: {0}")]
    Persistence(String),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
    #[error(transparent)]
    Package(#[from] PackagePreparationError),
    #[error(transparent)]
    Translation(#[from] TranslationError),
}

impl GameTranslationError {
    pub fn folder(error: impl std::fmt::Display) -> Self {
        Self::Folder(error.to_string())
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }

    /// What went wrong, in terms the player can act on.
    pub fn problem(&self) -> PreparationProblem {
        match self {
            Self::Translation(TranslationError::Translator(error)) => match error {
                TranslatorError::Unreachable(_) => PreparationProblem::TranslatorUnreachable,
                TranslatorError::Refused(_) => PreparationProblem::TranslatorRefused,
                TranslatorError::UnreadableReply(_) => PreparationProblem::TranslatorReply,
            },
            Self::NotFound(_) | Self::NoEngine | Self::Folder(_) => PreparationProblem::GameFiles,
            _ => PreparationProblem::Other,
        }
    }
}

pub trait GameTranslationStore: Send + Sync {
    fn read_game_translation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<GameTranslation>, GameTranslationError>;

    fn list_game_translations(&self) -> Result<Vec<GameTranslation>, GameTranslationError>;

    fn save_game_translation(
        &self,
        translation: &GameTranslation,
    ) -> Result<(), GameTranslationError>;

    fn remove_game_translation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<(), GameTranslationError>;
}

/// Where a game plugin reaches the launcher while the game runs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayAccess {
    pub address: String,
    pub token: String,
}

/// What the plugin reads when the game starts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameSession {
    pub work: String,
    pub shown: bool,
    pub relay: Option<RelayAccess>,
    pub translations: WorkTranslations,
}

/// Every text a game's plugin can show, found in the game's own files the
/// way the plugin finds them, and the characters' names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GameTexts {
    pub texts: Vec<SourceText>,
    pub names: Vec<String>,
    /// Where in the game each dialogue text appears (`map:12` for a map and
    /// the shared events it calls), for engines whose plugin says where the
    /// player is; empty for the others.
    pub places: HashMap<String, Vec<String>>,
}

/// A game's folder on disk, for the engines the launcher has a plugin for.
pub trait GameFolder: Send + Sync {
    fn detect_engine(
        &self,
        root: &str,
    ) -> Result<Option<GameEngineDetection>, GameTranslationError>;

    /// A fingerprint of the launcher's plugin for the engine; `None` when
    /// there is none yet.
    fn plugin(&self, engine: GameEngine) -> Option<String>;

    /// Adds the plugin, or brings it up to date. `previous` holds the
    /// changes of an earlier install, whose original files are kept.
    fn install(
        &self,
        root: &str,
        detection: &GameEngineDetection,
        previous: &[GameFileChange],
    ) -> Result<Vec<GameFileChange>, GameTranslationError>;

    /// Puts back every changed file and deletes every added one.
    fn remove(&self, root: &str, changes: &[GameFileChange]) -> Result<(), GameTranslationError>;

    fn read_texts(
        &self,
        root: &str,
        translation: &GameTranslation,
    ) -> Result<GameTexts, GameTranslationError>;

    /// The game's own copy of its translations, for starts without the
    /// launcher.
    fn write_translations(
        &self,
        root: &str,
        translation: &GameTranslation,
        translations: &WorkTranslations,
    ) -> Result<(), GameTranslationError>;

    fn write_session(
        &self,
        root: &str,
        translation: &GameTranslation,
        session: &GameSession,
    ) -> Result<(), GameTranslationError>;

    /// Whether the engine's plugin shows only the translations in the game's
    /// copy, with no relay to ask while the game runs; the copy is then
    /// written as preparing goes, not only at its end.
    fn reads_translations_from_folder(&self, _engine: GameEngine) -> bool {
        false
    }

    /// Where the player is in the game, as the plugin last wrote it (a key
    /// of [`GameTexts::places`]).
    fn current_place(&self, _root: &str, _translation: &GameTranslation) -> Option<String> {
        None
    }
}

/// The launcher around the preparation worker.
pub trait PreparationHost: Send + Sync {
    fn now(&self) -> String;

    /// Whether a game the launcher started is running.
    fn game_running(&self) -> bool;

    /// The player's choice to prepare only while no game runs.
    fn only_when_idle(&self) -> bool;

    /// Called as preparation goes on, and when it ends.
    fn changed(&self, view: &GameTranslationView);
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTranslationView {
    pub installation_id: InstallationId,
    pub engine: Option<GameEngine>,
    pub engine_label: Option<&'static str>,
    pub supported: bool,
    pub on: bool,
    pub shown: bool,
    pub translator_ready: bool,
    pub preparation: Option<PreparationView>,
    pub pack: Option<PackView>,
}

/// The translation from the DLA archive: the one in use, and a newer one to
/// download, once the launcher has looked.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackView {
    pub available: Option<PackSummary>,
    pub installed: Option<PackSummary>,
    /// Why the player cannot have the archive's translation yet.
    pub access: Option<PackAccess>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackAccess {
    /// The archive has one, for Elite members.
    ForElite,
    /// The player has not signed in to DLA.
    SignIn,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackSummary {
    pub lines: u64,
    pub made: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationState {
    Preparing,
    /// In line behind another game, or holding while a game runs.
    Waiting,
    Paused,
    Ready,
    NeedsTranslator,
    Failed,
}

/// Why a preparation stopped, for the card to say in the player's language;
/// the error itself goes along as the technical detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparationProblem {
    TranslatorUnreachable,
    TranslatorRefused,
    TranslatorReply,
    GameFiles,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextGroup {
    Menus,
    NamesAndDescriptions,
    Dialogue,
}

const GROUPS: [TextGroup; 3] = [
    TextGroup::Menus,
    TextGroup::NamesAndDescriptions,
    TextGroup::Dialogue,
];

impl TextGroup {
    fn of(kind: TextKind) -> Self {
        match kind {
            TextKind::Menu | TextKind::Addon => Self::Menus,
            TextKind::Database => Self::NamesAndDescriptions,
            TextKind::Dialogue | TextKind::Choice => Self::Dialogue,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupProgress {
    pub group: TextGroup,
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationView {
    pub state: PreparationState,
    pub groups: Vec<GroupProgress>,
    pub done: u64,
    pub total: u64,
    pub seconds_left: Option<u64>,
    pub prepared_at: Option<String>,
    pub prepared_with: Option<String>,
    /// Texts the translator gave nothing usable for in the last run.
    pub missing: u64,
    pub problem: Option<PreparationProblem>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<InstallationId>,
    current: Option<InstallationId>,
    stop_current: bool,
    shut_down: bool,
    // The progress of the game being prepared, and how a failed run ended.
    live: HashMap<InstallationId, PreparationView>,
    // Games prepared since the launcher started. Any other game with lines
    // left is queued when looked at, in case queueing them all at start failed.
    ran: HashSet<InstallationId>,
    // Games asked for again while being prepared: queued once the run ends.
    again: HashSet<InstallationId>,
}

pub struct GameTranslationService {
    installations: Arc<dyn InstallationStore>,
    preparations: Arc<dyn PackagePreparationStore>,
    store: Arc<dyn GameTranslationStore>,
    folder: Arc<dyn GameFolder>,
    translations: Arc<TranslationService>,
    packs: Option<Arc<dyn TranslationPacks>>,
    // What our GitHub had for each game when last asked.
    offers: Mutex<HashMap<InstallationId, PackLookup>>,
    queue: Mutex<Queue>,
    wake: Condvar,
}

impl GameTranslationService {
    pub fn new(
        installations: Arc<dyn InstallationStore>,
        preparations: Arc<dyn PackagePreparationStore>,
        store: Arc<dyn GameTranslationStore>,
        folder: Arc<dyn GameFolder>,
        translations: Arc<TranslationService>,
    ) -> Self {
        Self {
            installations,
            preparations,
            store,
            folder,
            translations,
            packs: None,
            offers: Mutex::new(HashMap::new()),
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        }
    }

    pub fn with_packs(mut self, packs: Arc<dyn TranslationPacks>) -> Self {
        self.packs = Some(packs);
        self
    }

    pub fn read(
        &self,
        installation_id: &InstallationId,
    ) -> Result<GameTranslationView, GameTranslationError> {
        let (installation, root) = self.installation(installation_id)?;
        let translation = self.store.read_game_translation(installation_id)?;
        let engine = match &translation {
            Some(translation) => Some(translation.engine),
            None => self
                .folder
                .detect_engine(&root)?
                .map(|detection| detection.engine),
        };
        let preparation = match &translation {
            Some(translation) => Some(self.preparation(translation, &installation, &root)?),
            None => None,
        };
        // Also while translation is off: the archive's translation needs no
        // translator of the player's own.
        let pack = self
            .packs
            .as_ref()
            .map(|_| self.pack_view(installation_id, translation.as_ref()));
        Ok(GameTranslationView {
            installation_id: installation_id.clone(),
            engine,
            engine_label: engine.map(GameEngine::label),
            supported: engine.is_some_and(|engine| self.folder.plugin(engine).is_some()),
            on: translation.is_some(),
            shown: translation.as_ref().is_some_and(|value| value.shown),
            translator_ready: self.translations.has_translator(),
            preparation,
            pack,
        })
    }

    /// Asks the DLA archive whether it has a translation of the game.
    pub fn check_pack(
        &self,
        installation_id: &InstallationId,
    ) -> Result<GameTranslationView, GameTranslationError> {
        let (installation, _) = self.installation(installation_id)?;
        if let Some(packs) = &self.packs {
            let found = packs.find(&work(&installation), installed_version(&installation))?;
            self.lock_offers().insert(installation_id.clone(), found);
        }
        self.read(installation_id)
    }

    /// Every work the DLA archive has a translation of, for the library.
    pub fn translated_works(&self) -> Result<BTreeMap<String, String>, GameTranslationError> {
        match &self.packs {
            Some(packs) => Ok(packs.translated_works()?),
            None => Ok(BTreeMap::new()),
        }
    }

    /// Downloads the game's translation from the DLA archive and uses it: its
    /// lines win over the player's translator, which still translates what
    /// the pack lacks.
    pub fn download_pack(
        &self,
        installation_id: &InstallationId,
    ) -> Result<GameTranslationView, GameTranslationError> {
        let packs = self.packs.as_ref().ok_or_else(|| {
            TranslationError::Pack("there is no place to get packs from".to_owned())
        })?;
        let (installation, root) = self.installation(installation_id)?;
        let work = work(&installation);
        let lookup = match self.lock_offers().get(installation_id).cloned() {
            Some(PackLookup::Offer(offer)) => PackLookup::Offer(offer),
            _ => packs.find(&work, installed_version(&installation))?,
        };
        let PackLookup::Offer(offer) = lookup else {
            return Err(TranslationError::Pack(format!(
                "the DLA archive has no translation of {work} for this account"
            ))
            .into());
        };
        let pack = packs.download(&offer)?;
        if pack.work != work {
            return Err(TranslationError::Pack(format!("the pack is for {}", pack.work)).into());
        }
        let mut translation = self.record(installation_id)?;
        // A game whose files cannot be read still gets the lines by key.
        let texts = self
            .folder
            .read_texts(&root, &translation)
            .unwrap_or_default();
        self.translations
            .import_pack(&pack, &texts.texts, &texts.names)?;
        translation.pack = Some(InstalledPack {
            made: pack.made.clone(),
            lines: pack.texts.len() as u64,
        });
        self.store.save_game_translation(&translation)?;
        self.folder
            .write_translations(&root, &translation, &self.translations.all(&work)?)?;
        // Lines the pack lacks still get translated, and progress is counted again.
        self.lock_queue().live.remove(installation_id);
        if !translation.paused {
            self.enqueue(installation_id);
        }
        self.read(installation_id)
    }

    /// Adds the plugin and starts translating the whole game ahead of play.
    pub fn turn_on(
        &self,
        installation_id: &InstallationId,
        turned_on_at: String,
    ) -> Result<GameTranslationView, GameTranslationError> {
        if self.store.read_game_translation(installation_id)?.is_none() {
            let (installation, root) = self.installation(installation_id)?;
            let detection = self
                .folder
                .detect_engine(&root)?
                .ok_or(GameTranslationError::NoEngine)?;
            let plugin = self
                .folder
                .plugin(detection.engine)
                .ok_or(GameTranslationError::Unsupported(detection.engine.label()))?;
            let changes = self.folder.install(&root, &detection, &[])?;
            let translation = GameTranslation {
                installation_id: installation_id.clone(),
                engine: detection.engine,
                game_folder: detection.root,
                shown: true,
                plugin,
                changes,
                turned_on_at,
                paused: false,
                prepared: None,
                pack: None,
            };
            if let Err(error) = self.store.save_game_translation(&translation) {
                // Unrecorded changes could never be removed again.
                let _ = self.folder.remove(&root, &translation.changes);
                return Err(error);
            }
            self.write_session(&installation, &root, &translation, None)?;
            self.enqueue(installation_id);
        }
        self.read(installation_id)
    }

    pub fn show(
        &self,
        installation_id: &InstallationId,
        shown: bool,
    ) -> Result<GameTranslationView, GameTranslationError> {
        let (installation, root) = self.installation(installation_id)?;
        let mut translation = self.record(installation_id)?;
        translation.shown = shown;
        self.store.save_game_translation(&translation)?;
        self.write_session(&installation, &root, &translation, None)?;
        self.read(installation_id)
    }

    /// Puts the game's files back as they were. The translations stay in the
    /// launcher, ready if translation is turned on again.
    pub fn remove(&self, installation_id: &InstallationId) -> Result<(), GameTranslationError> {
        let Some(translation) = self.store.read_game_translation(installation_id)? else {
            return Ok(());
        };
        self.stop(installation_id);
        let (_, root) = self.installation(installation_id)?;
        self.folder.remove(&root, &translation.changes)?;
        self.store.remove_game_translation(installation_id)?;
        self.lock_queue().live.remove(installation_id);
        Ok(())
    }

    /// Translates the game ahead of play: resumes a paused preparation, and
    /// after a game update translates only the new lines.
    pub fn request_preparation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<GameTranslationView, GameTranslationError> {
        let mut translation = self.record(installation_id)?;
        if translation.paused {
            translation.paused = false;
            self.store.save_game_translation(&translation)?;
        }
        self.lock_queue().live.remove(installation_id);
        self.enqueue(installation_id);
        self.read(installation_id)
    }

    /// Translates the game again with the player's translator, for example
    /// after its translator or the launcher's instructions to it improved.
    /// Its lines are marked stale and prepared again; each stays shown until
    /// its new translation replaces it. Lines from our GitHub, the player's
    /// edits and the name list stay as they are.
    pub fn translate_again(
        &self,
        installation_id: &InstallationId,
    ) -> Result<GameTranslationView, GameTranslationError> {
        let translation = self.record(installation_id)?;
        let (installation, root) = self.installation(installation_id)?;
        let texts = self.folder.read_texts(&root, &translation)?;
        let sources = texts
            .texts
            .iter()
            .map(|text| text.text.clone())
            .collect::<Vec<_>>();
        self.translations
            .mark_stale(&work(&installation), &sources)?;
        self.request_preparation(installation_id)
    }

    /// Stops after the batch in progress, until the player resumes.
    pub fn pause_preparation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<GameTranslationView, GameTranslationError> {
        let mut translation = self.record(installation_id)?;
        translation.paused = true;
        self.store.save_game_translation(&translation)?;
        self.stop(installation_id);
        self.read(installation_id)
    }

    /// Queues every translated game that is not paused, so lines added by a
    /// game update, or left untranslated last time, get translated.
    pub fn request_all(&self) -> Result<(), GameTranslationError> {
        for translation in self.store.list_game_translations()? {
            if !translation.paused {
                self.enqueue(&translation.installation_id);
            }
        }
        Ok(())
    }

    /// Ends [`Self::run_preparations`] after the batch in progress.
    pub fn shut_down(&self) {
        let mut queue = self.lock_queue();
        queue.shut_down = true;
        queue.stop_current = true;
        self.wake.notify_all();
    }

    /// The preparation worker: prepares queued games one at a time until
    /// [`Self::shut_down`].
    pub fn run_preparations(&self, host: &dyn PreparationHost) {
        loop {
            let installation_id = {
                let mut queue = self.lock_queue();
                loop {
                    if queue.shut_down {
                        return;
                    }
                    if let Some(next) = queue.waiting.pop_front() {
                        break next;
                    }
                    queue = self
                        .wake
                        .wait(queue)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            };
            self.prepare(&installation_id, host);
        }
    }

    /// Translates everything in the game the cache does not have yet: names
    /// first, so every line spells them alike, then menus, names and
    /// descriptions, and the dialogue.
    pub fn prepare(&self, installation_id: &InstallationId, host: &dyn PreparationHost) {
        {
            let mut queue = self.lock_queue();
            queue.waiting.retain(|waiting| waiting != installation_id);
            queue.current = Some(installation_id.clone());
            queue.stop_current = false;
            queue.ran.insert(installation_id.clone());
        }
        let result = self.prepare_game(installation_id, host);
        {
            let mut queue = self.lock_queue();
            queue.current = None;
            queue.stop_current = false;
            if queue.again.remove(installation_id) {
                queue.waiting.push_back(installation_id.clone());
                self.wake.notify_all();
            }
            match result {
                Ok(()) | Err(GameTranslationError::Translation(TranslationError::NoTranslator)) => {
                    queue.live.remove(installation_id);
                }
                Err(error) => {
                    let view = queue
                        .live
                        .entry(installation_id.clone())
                        .or_insert_with(|| Progress::default().view(None));
                    view.state = PreparationState::Failed;
                    view.seconds_left = None;
                    view.problem = Some(error.problem());
                    view.error = Some(error.to_string());
                }
            }
        }
        if let Ok(view) = self.read(installation_id) {
            host.changed(&view);
        }
    }

    /// Before the game starts: brings the plugin up to date and hands it the
    /// relay and the translations known so far. Returns whether the game is
    /// translated.
    pub fn prepare_launch(
        &self,
        installation_id: &InstallationId,
        relay: impl FnOnce() -> Option<RelayAccess>,
    ) -> Result<bool, GameTranslationError> {
        let Some(mut translation) = self.store.read_game_translation(installation_id)? else {
            return Ok(false);
        };
        let (installation, root) = self.installation(installation_id)?;
        let plugin =
            self.folder
                .plugin(translation.engine)
                .ok_or(GameTranslationError::Unsupported(
                    translation.engine.label(),
                ))?;
        // A newer plugin, or a game update that replaced the plugin list.
        let detection = GameEngineDetection {
            engine: translation.engine,
            root: translation.game_folder.clone(),
        };
        let changes = self
            .folder
            .install(&root, &detection, &translation.changes)?;
        if plugin != translation.plugin || changes != translation.changes {
            translation.plugin = plugin;
            translation.changes = changes;
            self.store.save_game_translation(&translation)?;
        }
        let relay = if translation.shown { relay() } else { None };
        self.write_session(&installation, &root, &translation, relay)?;
        Ok(translation.shown)
    }

    fn prepare_game(
        &self,
        installation_id: &InstallationId,
        host: &dyn PreparationHost,
    ) -> Result<(), GameTranslationError> {
        let Some(translation) = self.store.read_game_translation(installation_id)? else {
            return Ok(());
        };
        if translation.paused {
            return Ok(());
        }
        let (installation, root) = self.installation(installation_id)?;
        let work = work(&installation);
        let texts = self.folder.read_texts(&root, &translation)?;
        let mut progress = self.count(&work, &texts)?;
        let Some(model) = self.translations.translator_name() else {
            if progress.done() != progress.total() {
                return Err(TranslationError::NoTranslator.into());
            }
            return self.folder.write_translations(
                &root,
                &translation,
                &self.translations.all(&work)?,
            );
        };
        progress.state = Some(PreparationState::Preparing);
        progress.started = true;
        self.publish(installation_id, &progress, &translation, host);

        if !progress.unknown_names.is_empty() {
            if !self.keep_going(installation_id, host, &mut progress, &translation) {
                return Ok(());
            }
            let began = Instant::now();
            self.translations
                .ensure_names(&work, &texts.names, Urgency::Background)?;
            let names = std::mem::take(&mut progress.unknown_names);
            progress.record(
                TextGroup::NamesAndDescriptions,
                names.len() as u64,
                0,
                cost(names.iter().map(String::as_str)),
                began.elapsed(),
            );
            self.publish(installation_id, &progress, &translation, host);
        }
        let copy_as_it_goes = self
            .folder
            .reads_translations_from_folder(translation.engine);
        let mut copied = Instant::now();
        let mut place = None;
        for group in GROUPS {
            let mut pending = std::mem::take(&mut progress.pending[group.index()]);
            let mut at = 0;
            while at < pending.len() {
                if !self.keep_going(installation_id, host, &mut progress, &translation) {
                    return Ok(());
                }
                if group == TextGroup::Dialogue && !texts.places.is_empty() {
                    let current = self.folder.current_place(&root, &translation);
                    if current.is_some() && current != place {
                        place = current;
                        // What the player is about to see goes next.
                        pending[at..].sort_by_key(|text| {
                            !texts.places.get(&text.text).is_some_and(|places| {
                                places.iter().any(|at| Some(at) == place.as_ref())
                            })
                        });
                    }
                }
                if copy_as_it_goes && copied.elapsed() >= COPY_INTERVAL {
                    self.folder.write_translations(
                        &root,
                        &translation,
                        &self.translations.all(&work)?,
                    )?;
                    copied = Instant::now();
                }
                let size = batch_size(group, host.game_running());
                let batch = &pending[at..pending.len().min(at + size)];
                let began = Instant::now();
                let results = self.translations.translate(
                    &work,
                    batch,
                    TextMarkup::RpgMaker,
                    Urgency::Background,
                )?;
                let missing = results.iter().filter(|result| result.is_none()).count();
                progress.record(
                    group,
                    batch.len() as u64,
                    missing as u64,
                    cost(batch.iter().map(|text| text.text.as_str())),
                    began.elapsed(),
                );
                at += batch.len();
                self.publish(installation_id, &progress, &translation, host);
            }
        }

        // The player may have changed the translation meanwhile.
        let Some(mut translation) = self.store.read_game_translation(installation_id)? else {
            return Ok(());
        };
        translation.prepared = Some(PreparedTranslation {
            at: host.now(),
            with: model,
            texts: progress.total(),
            missing: progress.missing,
        });
        self.store.save_game_translation(&translation)?;
        self.folder
            .write_translations(&root, &translation, &self.translations.all(&work)?)
    }

    /// Whether to go on: not once the player paused or removed the
    /// translation, or the launcher is closing. Holds while a game runs when
    /// the player chose to prepare only then.
    fn keep_going(
        &self,
        installation_id: &InstallationId,
        host: &dyn PreparationHost,
        progress: &mut Progress,
        translation: &GameTranslation,
    ) -> bool {
        let mut holding = false;
        loop {
            {
                let queue = self.lock_queue();
                if queue.shut_down || queue.stop_current {
                    return false;
                }
                if holding {
                    let _ = self
                        .wake
                        .wait_timeout(queue, IDLE_CHECK)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            }
            let hold = host.only_when_idle() && host.game_running();
            if hold != holding {
                holding = hold;
                progress.state = Some(if hold {
                    PreparationState::Waiting
                } else {
                    PreparationState::Preparing
                });
                self.publish(installation_id, progress, translation, host);
            }
            if !hold {
                return true;
            }
        }
    }

    fn publish(
        &self,
        installation_id: &InstallationId,
        progress: &Progress,
        translation: &GameTranslation,
        host: &dyn PreparationHost,
    ) {
        let view = progress.view(translation.prepared.as_ref());
        self.lock_queue()
            .live
            .insert(installation_id.clone(), view.clone());
        host.changed(&GameTranslationView {
            installation_id: installation_id.clone(),
            engine: Some(translation.engine),
            engine_label: Some(translation.engine.label()),
            supported: true,
            on: true,
            shown: translation.shown,
            translator_ready: self.translations.has_translator(),
            preparation: Some(view),
            pack: self
                .packs
                .as_ref()
                .map(|_| self.pack_view(installation_id, Some(translation))),
        });
    }

    /// Where a game's preparation stands, without translating anything.
    fn preparation(
        &self,
        translation: &GameTranslation,
        installation: &Installation,
        root: &str,
    ) -> Result<PreparationView, GameTranslationError> {
        let (live, queued, ran) = {
            let queue = self.lock_queue();
            (
                queue.live.get(&translation.installation_id).cloned(),
                queue.current.as_ref() == Some(&translation.installation_id)
                    || queue.waiting.contains(&translation.installation_id),
                queue.ran.contains(&translation.installation_id),
            )
        };
        if let Some(live) = live {
            return Ok(live);
        }
        let mut progress = match self.folder.read_texts(root, translation) {
            Ok(texts) => self.count(&work(installation), &texts)?,
            Err(error) => {
                let mut view = Progress::default().view(translation.prepared.as_ref());
                view.state = PreparationState::Failed;
                view.problem = Some(error.problem());
                view.error = Some(error.to_string());
                return Ok(view);
            }
        };
        let unknown = progress.total() - progress.done();
        progress.state = Some(if translation.paused {
            PreparationState::Paused
        } else if unknown == 0 {
            PreparationState::Ready
        } else if queued {
            PreparationState::Waiting
        } else if translation
            .prepared
            .as_ref()
            .is_some_and(|prepared| unknown <= prepared.missing)
        {
            PreparationState::Ready
        } else if !self.translations.has_translator() {
            PreparationState::NeedsTranslator
        } else {
            if !ran {
                self.enqueue(&translation.installation_id);
            }
            PreparationState::Waiting
        });
        Ok(progress.view(translation.prepared.as_ref()))
    }

    /// What the cache already has of the game's texts.
    fn count(&self, work: &str, texts: &GameTexts) -> Result<Progress, GameTranslationError> {
        let mut progress = Progress::default();
        let sources = texts
            .texts
            .iter()
            .map(|text| text.text.clone())
            .collect::<Vec<_>>();
        for (text, known) in texts
            .texts
            .iter()
            .zip(self.translations.known(work, &sources)?)
        {
            let group = TextGroup::of(text.kind).index();
            progress.totals[group] += 1;
            if known.is_some() {
                progress.done[group] += 1;
            } else {
                progress.left += cost([text.text.as_str()]);
                progress.pending[group].push(text.clone());
            }
        }
        let named = self
            .translations
            .names(work)?
            .into_iter()
            .map(|(japanese, _)| japanese)
            .collect::<HashSet<_>>();
        let names = TextGroup::NamesAndDescriptions.index();
        for name in &texts.names {
            progress.totals[names] += 1;
            if named.contains(name) {
                progress.done[names] += 1;
            } else {
                progress.left += cost([name.as_str()]);
                progress.unknown_names.push(name.clone());
            }
        }
        Ok(progress)
    }

    fn enqueue(&self, installation_id: &InstallationId) {
        let mut queue = self.lock_queue();
        if queue.current.as_ref() == Some(installation_id) {
            queue.again.insert(installation_id.clone());
        } else if !queue.waiting.contains(installation_id) {
            queue.waiting.push_back(installation_id.clone());
            self.wake.notify_all();
        }
    }

    fn stop(&self, installation_id: &InstallationId) {
        let mut queue = self.lock_queue();
        queue.waiting.retain(|waiting| waiting != installation_id);
        queue.again.remove(installation_id);
        if queue.current.as_ref() == Some(installation_id) {
            queue.stop_current = true;
            self.wake.notify_all();
        }
    }

    fn pack_view(
        &self,
        installation_id: &InstallationId,
        translation: Option<&GameTranslation>,
    ) -> PackView {
        let in_use = translation.and_then(|translation| translation.pack.as_ref());
        let installed = in_use.map(|pack| PackSummary {
            lines: pack.lines,
            made: pack.made.clone(),
        });
        let offers = self.lock_offers();
        let lookup = offers.get(installation_id);
        let available = match lookup {
            // The pack for the game's version, or one made since.
            Some(PackLookup::Offer(offer)) if in_use.is_none_or(|pack| offer.made != pack.made) => {
                Some(PackSummary {
                    lines: offer.lines,
                    made: offer.made.clone(),
                })
            }
            _ => None,
        };
        let access = match lookup {
            Some(PackLookup::ForElite) => Some(PackAccess::ForElite),
            Some(PackLookup::SignedOut) => Some(PackAccess::SignIn),
            _ => None,
        };
        PackView {
            available,
            installed,
            access,
        }
    }

    fn lock_offers(&self) -> MutexGuard<'_, HashMap<InstallationId, PackLookup>> {
        self.offers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_queue(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record(
        &self,
        installation_id: &InstallationId,
    ) -> Result<GameTranslation, GameTranslationError> {
        self.store
            .read_game_translation(installation_id)?
            .ok_or_else(|| GameTranslationError::NotFound(installation_id.0.clone()))
    }

    fn write_session(
        &self,
        installation: &Installation,
        root: &str,
        translation: &GameTranslation,
        relay: Option<RelayAccess>,
    ) -> Result<(), GameTranslationError> {
        let work = work(installation);
        let translations = self.translations.all(&work)?;
        self.folder.write_session(
            root,
            translation,
            &GameSession {
                work,
                shown: translation.shown,
                relay,
                translations,
            },
        )
    }

    /// The installation and the folder it is played from.
    fn installation(
        &self,
        installation_id: &InstallationId,
    ) -> Result<(Installation, String), GameTranslationError> {
        let installation = self
            .installations
            .read(installation_id)?
            .ok_or_else(|| GameTranslationError::NotFound(installation_id.0.clone()))?;
        let root = match self.preparations.read_prepared_package(installation_id)? {
            Some(prepared) => prepared.destination_root,
            None => installation.root_path.clone(),
        };
        Ok((installation, root))
    }
}

#[derive(Default)]
struct Progress {
    state: Option<PreparationState>,
    totals: [u64; 3],
    done: [u64; 3],
    pending: [Vec<SourceText>; 3],
    unknown_names: Vec<String>,
    missing: u64,
    started: bool,
    /// What is left to translate, in the units of `cost`.
    left: u64,
    /// The latest batches: how long each took and what it cost.
    recent: VecDeque<(Duration, u64)>,
}

impl Progress {
    fn record(&mut self, group: TextGroup, done: u64, missing: u64, cost: u64, took: Duration) {
        self.done[group.index()] += done;
        self.missing += missing;
        self.left = self.left.saturating_sub(cost);
        if self.recent.len() == RECENT_BATCHES {
            self.recent.pop_front();
        }
        self.recent.push_back((took, cost));
    }

    fn total(&self) -> u64 {
        self.totals.iter().sum()
    }

    fn done(&self) -> u64 {
        self.done.iter().sum()
    }

    fn view(&self, prepared: Option<&PreparedTranslation>) -> PreparationView {
        let (took, cost) = self
            .recent
            .iter()
            .fold((Duration::ZERO, 0), |(took, cost), batch| {
                (took + batch.0, cost + batch.1)
            });
        let seconds_left = (cost > 0 && self.left > 0)
            .then(|| (took.as_secs_f64() * self.left as f64 / cost as f64).ceil() as u64);
        PreparationView {
            state: self.state.unwrap_or(PreparationState::Waiting),
            groups: GROUPS
                .iter()
                .map(|&group| GroupProgress {
                    group,
                    done: self.done[group.index()],
                    total: self.totals[group.index()],
                })
                .collect(),
            done: self.done(),
            total: self.total(),
            seconds_left,
            prepared_at: prepared.map(|prepared| prepared.at.clone()),
            prepared_with: prepared.map(|prepared| prepared.with.clone()),
            missing: if self.started {
                self.missing
            } else {
                prepared.map_or(0, |prepared| prepared.missing)
            },
            problem: None,
            error: None,
        }
    }
}

/// What texts cost the translator, for the time left.
fn cost<'a>(texts: impl IntoIterator<Item = &'a str>) -> u64 {
    texts
        .into_iter()
        .map(|text| text.chars().count() as u64 + TEXT_OVERHEAD)
        .sum()
}

fn batch_size(group: TextGroup, game_running: bool) -> usize {
    match (group, game_running) {
        (TextGroup::Dialogue, false) => DIALOGUE_BATCH,
        (TextGroup::Dialogue, true) => DIALOGUE_BATCH_WHILE_PLAYING,
        (_, false) => TERM_BATCH,
        (_, true) => TERM_BATCH_WHILE_PLAYING,
    }
}

/// The cache key of an installation's translations: its DLsite work, so
/// every version of a work shares them.
fn work(installation: &Installation) -> String {
    installation.effective_catalog_work_code().map_or_else(
        || format!("installation:{}", installation.id.0),
        str::to_owned,
    )
}

/// The day the installed version of the game came out, when it was installed
/// from a DLA package that says so; packs are kept per version by that day.
fn installed_version(installation: &Installation) -> Option<&str> {
    let release = installation
        .detection
        .package_inspection
        .as_ref()?
        .catalog_release
        .as_ref()?;
    let day = release.update_date.trim().get(..10)?;
    is_day(day).then_some(day)
}

fn is_day(text: &str) -> bool {
    text.bytes().enumerate().all(|(index, byte)| {
        if index == 4 || index == 7 {
            byte == b'-'
        } else {
            byte.is_ascii_digit()
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            OnceLock,
            atomic::{AtomicBool, Ordering},
        },
    };

    use dla_domain::{
        installation::{
            InstallationDetection, InstallationOverrides, InstallationPlatform, InstallationStatus,
            ManualCatalogIdentity, RelativePath,
        },
        package::PreparedPackageInstallation,
        translation::{GameFileChangeKind, TranslationOrigin},
    };

    use super::*;
    use crate::translation::{
        BatchStyle, Glossary, NameToSpell, NewTranslation, PackLine, PackOffer, StoredTranslation,
        TranslationPack, TranslationStore, Translator, TranslatorBatch, TranslatorError, text_key,
    };

    struct Installations(Installation);

    impl InstallationStore for Installations {
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
            Ok(Some(self.0.clone()).filter(|installation| installation.id == *installation_id))
        }
        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(vec![self.0.clone()])
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

    struct NotPrepared;

    impl PackagePreparationStore for NotPrepared {
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

    #[derive(Default)]
    struct Records(Mutex<HashMap<InstallationId, GameTranslation>>);

    impl Records {
        fn get(&self) -> GameTranslation {
            self.0.lock().unwrap()[&id()].clone()
        }
    }

    impl GameTranslationStore for Records {
        fn read_game_translation(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<GameTranslation>, GameTranslationError> {
            Ok(self.0.lock().unwrap().get(installation_id).cloned())
        }
        fn list_game_translations(&self) -> Result<Vec<GameTranslation>, GameTranslationError> {
            Ok(self.0.lock().unwrap().values().cloned().collect())
        }
        fn save_game_translation(
            &self,
            translation: &GameTranslation,
        ) -> Result<(), GameTranslationError> {
            self.0
                .lock()
                .unwrap()
                .insert(translation.installation_id.clone(), translation.clone());
            Ok(())
        }
        fn remove_game_translation(
            &self,
            installation_id: &InstallationId,
        ) -> Result<(), GameTranslationError> {
            self.0.lock().unwrap().remove(installation_id);
            Ok(())
        }
    }

    struct Folder {
        engine: GameEngine,
        plugin: Mutex<String>,
        texts: GameTexts,
        installs: Mutex<Vec<Vec<GameFileChange>>>,
        removed: Mutex<Vec<Vec<GameFileChange>>>,
        sessions: Mutex<Vec<GameSession>>,
        copies: Mutex<Vec<WorkTranslations>>,
        place: Mutex<Option<String>>,
    }

    impl Folder {
        fn new(engine: GameEngine) -> Arc<Self> {
            Self::with_texts(engine, GameTexts::default())
        }

        fn with_texts(engine: GameEngine, texts: GameTexts) -> Arc<Self> {
            Arc::new(Self {
                engine,
                plugin: Mutex::new("plugin-1".to_owned()),
                texts,
                installs: Mutex::default(),
                removed: Mutex::default(),
                sessions: Mutex::default(),
                copies: Mutex::default(),
                place: Mutex::default(),
            })
        }
    }

    fn changes() -> Vec<GameFileChange> {
        vec![
            GameFileChange {
                path: RelativePath::parse("js/plugins/DLA_Translate.js").unwrap(),
                kind: GameFileChangeKind::Added,
            },
            GameFileChange {
                path: RelativePath::parse("js/plugins.js").unwrap(),
                kind: GameFileChangeKind::Changed {
                    original: b"var $plugins = [];".to_vec(),
                    written_sha256: "written".to_owned(),
                    written_bytes: 40,
                },
            },
        ]
    }

    impl GameFolder for Folder {
        fn detect_engine(
            &self,
            root: &str,
        ) -> Result<Option<GameEngineDetection>, GameTranslationError> {
            assert_eq!(root, "/library/RJ01");
            Ok(Some(GameEngineDetection {
                engine: self.engine,
                root: None,
            }))
        }
        fn plugin(&self, engine: GameEngine) -> Option<String> {
            matches!(engine, GameEngine::RpgMakerMz | GameEngine::RpgMakerMv)
                .then(|| self.plugin.lock().unwrap().clone())
        }
        fn install(
            &self,
            _root: &str,
            _detection: &GameEngineDetection,
            previous: &[GameFileChange],
        ) -> Result<Vec<GameFileChange>, GameTranslationError> {
            self.installs.lock().unwrap().push(previous.to_vec());
            Ok(changes())
        }
        fn remove(
            &self,
            _root: &str,
            changes: &[GameFileChange],
        ) -> Result<(), GameTranslationError> {
            self.removed.lock().unwrap().push(changes.to_vec());
            Ok(())
        }
        fn read_texts(
            &self,
            _root: &str,
            _translation: &GameTranslation,
        ) -> Result<GameTexts, GameTranslationError> {
            Ok(self.texts.clone())
        }
        fn write_translations(
            &self,
            _root: &str,
            _translation: &GameTranslation,
            translations: &WorkTranslations,
        ) -> Result<(), GameTranslationError> {
            self.copies.lock().unwrap().push(translations.clone());
            Ok(())
        }
        fn write_session(
            &self,
            _root: &str,
            _translation: &GameTranslation,
            session: &GameSession,
        ) -> Result<(), GameTranslationError> {
            self.sessions.lock().unwrap().push(session.clone());
            Ok(())
        }
        fn current_place(&self, _root: &str, _translation: &GameTranslation) -> Option<String> {
            self.place.lock().unwrap().clone()
        }
    }

    /// The cache, keyed like the real one.
    #[derive(Default)]
    struct Cache {
        texts: Mutex<HashMap<(String, String), (String, String)>>,
        names: Mutex<Vec<(String, String)>>,
        // Lines from a pack, and the player's translator's lines to redo.
        packed: Mutex<HashSet<(String, String)>>,
        stale: Mutex<HashSet<(String, String)>>,
    }

    impl TranslationStore for Cache {
        fn best(
            &self,
            work: &str,
            keys: &[String],
        ) -> Result<HashMap<String, StoredTranslation>, TranslationError> {
            let texts = self.texts.lock().unwrap();
            let (packed, stale) = (self.packed.lock().unwrap(), self.stale.lock().unwrap());
            Ok(keys
                .iter()
                .filter_map(|key| {
                    let id = (work.to_owned(), key.clone());
                    texts.get(&id).map(|(_, text)| {
                        let origin = if packed.contains(&id) {
                            TranslationOrigin::PackMachine
                        } else if stale.contains(&id) {
                            TranslationOrigin::Stale
                        } else {
                            TranslationOrigin::Local
                        };
                        (
                            key.clone(),
                            StoredTranslation {
                                text: text.clone(),
                                origin,
                            },
                        )
                    })
                })
                .collect())
        }
        fn save(
            &self,
            work: &str,
            translations: &[NewTranslation<'_>],
        ) -> Result<(), TranslationError> {
            let mut texts = self.texts.lock().unwrap();
            for translation in translations {
                let id = (work.to_owned(), translation.key.to_owned());
                self.stale.lock().unwrap().remove(&id);
                texts.insert(
                    id,
                    (translation.source.to_owned(), translation.text.to_owned()),
                );
            }
            Ok(())
        }
        fn mark_stale(&self, work: &str, keys: &[String]) -> Result<u64, TranslationError> {
            let texts = self.texts.lock().unwrap();
            let packed = self.packed.lock().unwrap();
            let mut stale = self.stale.lock().unwrap();
            let mut marked = 0;
            for key in keys {
                let id = (work.to_owned(), key.clone());
                if texts.contains_key(&id) && !packed.contains(&id) && stale.insert(id) {
                    marked += 1;
                }
            }
            Ok(marked)
        }
        fn names(&self, _work: &str) -> Result<Vec<(String, String)>, TranslationError> {
            Ok(self.names.lock().unwrap().clone())
        }
        fn save_names(
            &self,
            _work: &str,
            names: &[(String, String)],
        ) -> Result<(), TranslationError> {
            self.names.lock().unwrap().extend_from_slice(names);
            Ok(())
        }
        fn glossary(&self, _work: &str) -> Result<Glossary, TranslationError> {
            Ok(Glossary::default())
        }
        fn save_glossary(&self, _work: &str, _glossary: &Glossary) -> Result<(), TranslationError> {
            Ok(())
        }
        fn save_pack(
            &self,
            work: &str,
            _pack: &str,
            translations: &[NewTranslation<'_>],
        ) -> Result<(), TranslationError> {
            self.save(work, translations)?;
            self.packed.lock().unwrap().extend(
                translations
                    .iter()
                    .map(|translation| (work.to_owned(), translation.key.to_owned())),
            );
            Ok(())
        }
        fn all(&self, work: &str) -> Result<Vec<(String, String)>, TranslationError> {
            let mut all = self
                .texts
                .lock()
                .unwrap()
                .iter()
                .filter(|((stored_work, _), _)| stored_work == work)
                .map(|(_, (source, text))| (source.clone(), text.clone()))
                .collect::<Vec<_>>();
            all.sort();
            Ok(all)
        }
    }

    /// Answers every line in English, except lines with 失敗; records what
    /// it was asked, in order.
    #[derive(Default)]
    struct Echo {
        calls: Mutex<Vec<String>>,
        batches: Mutex<Vec<Vec<String>>>,
        unreachable: AtomicBool,
        // Answers with this instead of "In English", like a better translator.
        wording: Mutex<Option<String>>,
    }

    impl Translator for Echo {
        fn name(&self) -> String {
            "echo".to_owned()
        }
        fn translate(
            &self,
            batch: &TranslatorBatch<'_>,
        ) -> Result<Vec<Option<String>>, TranslatorError> {
            if self.unreachable.load(Ordering::SeqCst) {
                return Err(TranslatorError::Unreachable(
                    "connection refused".to_owned(),
                ));
            }
            let style = match batch.style {
                BatchStyle::Dialogue => "dialogue",
                BatchStyle::Terms => "terms",
            };
            self.calls
                .lock()
                .unwrap()
                .push(format!("{style} {}", batch.lines.len()));
            self.batches.lock().unwrap().push(batch.lines.to_vec());
            Ok(batch
                .lines
                .iter()
                .map(|line| {
                    (!line.contains("失敗")).then(|| {
                        self.wording
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(|| "In English".to_owned())
                    })
                })
                .collect())
        }
        fn spell_names(
            &self,
            names: &[NameToSpell],
        ) -> Result<HashMap<String, String>, TranslatorError> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("names {}", names.len()));
            Ok(names
                .iter()
                .map(|name| (name.japanese.clone(), "Yukina".to_owned()))
                .collect())
        }
    }

    #[derive(Default)]
    struct Host {
        playing: AtomicBool,
        views: Mutex<Vec<GameTranslationView>>,
        // Pauses the game once this many views were published.
        pause_after: OnceLock<(usize, Arc<GameTranslationService>)>,
    }

    impl PreparationHost for Host {
        fn now(&self) -> String {
            "2026-10-29T12:00:00Z".to_owned()
        }
        fn game_running(&self) -> bool {
            self.playing.load(Ordering::SeqCst)
        }
        fn only_when_idle(&self) -> bool {
            false
        }
        fn changed(&self, view: &GameTranslationView) {
            let count = {
                let mut views = self.views.lock().unwrap();
                views.push(view.clone());
                views.len()
            };
            if let Some((after, service)) = self.pause_after.get()
                && count == *after
            {
                service.pause_preparation(&id()).unwrap();
            }
        }
    }

    fn installation() -> Installation {
        Installation {
            id: id(),
            scan_root_id: None,
            root_path: "/library/RJ01".to_owned(),
            platform: InstallationPlatform::Windows,
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
                    work_code: "RJ01".to_owned(),
                }),
                ..InstallationOverrides::default()
            },
            discovered_at: "2026-10-29T00:00:00Z".to_owned(),
            updated_at: "2026-10-29T00:00:00Z".to_owned(),
        }
    }

    struct Setup {
        service: Arc<GameTranslationService>,
        records: Arc<Records>,
        folder: Arc<Folder>,
        cache: Arc<Cache>,
        echo: Arc<Echo>,
        translations: Arc<TranslationService>,
    }

    fn setup(folder: Arc<Folder>) -> Setup {
        let records = Arc::new(Records::default());
        let cache = Arc::new(Cache::default());
        let echo = Arc::new(Echo::default());
        let translations = Arc::new(TranslationService::new(cache.clone()));
        translations.set_translator(Some(echo.clone()));
        let service = Arc::new(GameTranslationService::new(
            Arc::new(Installations(installation())),
            Arc::new(NotPrepared),
            records.clone(),
            folder.clone(),
            translations.clone(),
        ));
        Setup {
            service,
            records,
            folder,
            cache,
            echo,
            translations,
        }
    }

    fn id() -> InstallationId {
        InstallationId("installation-1".to_owned())
    }

    fn relay() -> Option<RelayAccess> {
        Some(RelayAccess {
            address: "http://127.0.0.1:4000".to_owned(),
            token: "secret".to_owned(),
        })
    }

    const DIALOGUE: [&str; 12] = [
        "あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ", "さ", "し",
    ];

    fn game_texts(dialogue: &[&str]) -> GameTexts {
        let text = |text: &str, kind| SourceText {
            text: text.to_owned(),
            kind,
        };
        let mut texts = vec![
            text("剣", TextKind::Database),
            text("アイテム", TextKind::Menu),
            text("盾", TextKind::Database),
        ];
        texts.extend(dialogue.iter().map(|line| text(line, TextKind::Dialogue)));
        GameTexts {
            texts,
            names: vec!["ユキナ".to_owned()],
            places: HashMap::new(),
        }
    }

    fn turned_on(texts: GameTexts) -> Setup {
        let setup = setup(Folder::with_texts(GameEngine::RpgMakerMz, texts));
        setup
            .service
            .turn_on(&id(), "2026-10-29T10:00:00Z".to_owned())
            .unwrap();
        setup
    }

    fn state(setup: &Setup) -> PreparationView {
        setup.service.read(&id()).unwrap().preparation.unwrap()
    }

    #[test]
    fn turning_on_records_every_change_and_queues_the_game() {
        let setup = turned_on(game_texts(&[]));
        let view = setup.service.read(&id()).unwrap();
        assert!(view.on && view.shown && view.supported && view.translator_ready);
        assert_eq!(view.engine_label, Some("RPG Maker MZ"));
        let preparation = view.preparation.unwrap();
        assert_eq!(preparation.state, PreparationState::Waiting);
        assert_eq!((preparation.done, preparation.total), (0, 4));
        let record = setup.records.get();
        assert_eq!(record.changes, changes());
        assert_eq!(record.plugin, "plugin-1");
        let sessions = setup.folder.sessions.lock().unwrap();
        assert_eq!(sessions[0].work, "RJ01");
        assert_eq!(sessions[0].relay, None);
        drop(sessions);
        // Turning on twice changes nothing.
        setup
            .service
            .turn_on(&id(), "2026-10-29T11:00:00Z".to_owned())
            .unwrap();
        assert_eq!(setup.folder.installs.lock().unwrap().len(), 1);
    }

    #[test]
    fn leaves_games_without_a_plugin_untouched() {
        let setup = setup(Folder::new(GameEngine::UnityMono));
        let view = setup.service.read(&id()).unwrap();
        assert_eq!(view.engine, Some(GameEngine::UnityMono));
        assert!(!view.supported && !view.on && view.preparation.is_none());
        assert!(matches!(
            setup
                .service
                .turn_on(&id(), "2026-10-29T10:00:00Z".to_owned()),
            Err(GameTranslationError::Unsupported("Unity"))
        ));
        assert!(setup.folder.installs.lock().unwrap().is_empty());
        assert!(setup.records.0.lock().unwrap().is_empty());
    }

    #[test]
    fn a_launch_updates_the_plugin_and_hands_over_the_relay_only_when_shown() {
        let setup = turned_on(GameTexts::default());
        *setup.folder.plugin.lock().unwrap() = "plugin-2".to_owned();
        assert!(setup.service.prepare_launch(&id(), relay).unwrap());
        assert_eq!(setup.folder.installs.lock().unwrap()[1], changes());
        assert_eq!(setup.records.get().plugin, "plugin-2");
        assert_eq!(setup.folder.sessions.lock().unwrap()[1].relay, relay());

        setup.service.show(&id(), false).unwrap();
        let asked = std::cell::Cell::new(false);
        assert!(
            !setup
                .service
                .prepare_launch(&id(), || {
                    asked.set(true);
                    relay()
                })
                .unwrap()
        );
        assert!(!asked.get());
        let sessions = setup.folder.sessions.lock().unwrap();
        assert!(!sessions.last().unwrap().shown);
        assert_eq!(sessions.last().unwrap().relay, None);
    }

    #[test]
    fn removing_puts_back_what_was_recorded_and_forgets_the_game() {
        let setup = setup(Folder::new(GameEngine::RpgMakerMz));
        setup.service.remove(&id()).unwrap();
        assert!(setup.folder.removed.lock().unwrap().is_empty());
        assert!(!setup.service.prepare_launch(&id(), relay).unwrap());

        setup
            .service
            .turn_on(&id(), "2026-10-29T10:00:00Z".to_owned())
            .unwrap();
        setup.service.remove(&id()).unwrap();
        assert_eq!(setup.folder.removed.lock().unwrap()[0], changes());
        assert!(setup.records.0.lock().unwrap().is_empty());
        // Nothing is left in line to prepare.
        setup.service.prepare(&id(), &Host::default());
        assert!(setup.echo.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn prepares_names_first_then_menus_descriptions_and_dialogue() {
        let setup = turned_on(game_texts(&DIALOGUE));
        let host = Host::default();
        setup.service.prepare(&id(), &host);

        assert_eq!(
            *setup.echo.calls.lock().unwrap(),
            ["names 1", "terms 1", "terms 2", "dialogue 10", "dialogue 2"]
        );
        let prepared = setup.records.get().prepared.unwrap();
        assert_eq!(
            prepared,
            PreparedTranslation {
                at: "2026-10-29T12:00:00Z".to_owned(),
                with: "echo".to_owned(),
                texts: 16,
                missing: 0,
            }
        );
        let view = state(&setup);
        assert_eq!(view.state, PreparationState::Ready);
        assert_eq!((view.done, view.total), (16, 16));
        assert_eq!(
            view.groups,
            vec![
                GroupProgress {
                    group: TextGroup::Menus,
                    done: 1,
                    total: 1
                },
                GroupProgress {
                    group: TextGroup::NamesAndDescriptions,
                    done: 3,
                    total: 3
                },
                GroupProgress {
                    group: TextGroup::Dialogue,
                    done: 12,
                    total: 12
                },
            ]
        );
        assert_eq!(view.prepared_with.as_deref(), Some("echo"));
        // Progress after each step, then the end.
        let views = host.views.lock().unwrap();
        let done = views
            .iter()
            .map(|view| view.preparation.as_ref().unwrap().done)
            .collect::<Vec<_>>();
        assert_eq!(done, [0, 1, 2, 4, 14, 16, 16]);
        assert_eq!(
            views.last().unwrap().preparation.as_ref().unwrap().state,
            PreparationState::Ready
        );
        // The game's own copy has everything.
        let copies = setup.folder.copies.lock().unwrap();
        assert_eq!(copies[0].texts.len(), 15);
        assert_eq!(copies[0].names.len(), 1);
    }

    #[test]
    fn translates_the_dialogue_of_the_place_the_player_is_at_first() {
        let mut texts = game_texts(&DIALOGUE);
        for line in ["け", "こ", "さ"] {
            texts.places.insert(
                line.to_owned(),
                vec!["map:1".to_owned(), "map:2".to_owned()],
            );
        }
        texts
            .places
            .insert("あ".to_owned(), vec!["map:3".to_owned()]);
        let setup = turned_on(texts);
        *setup.folder.place.lock().unwrap() = Some("map:2".to_owned());
        setup.service.prepare(&id(), &Host::default());

        let batches = setup.echo.batches.lock().unwrap();
        let dialogue = &batches[batches.len() - 2];
        assert_eq!(dialogue[..4], ["け", "こ", "さ", "あ"]);
        assert_eq!(batches.last().unwrap(), &["く", "し"]);
    }

    #[test]
    fn sends_smaller_batches_while_a_game_runs() {
        let setup = turned_on(game_texts(&DIALOGUE));
        let host = Host::default();
        host.playing.store(true, Ordering::SeqCst);
        setup.service.prepare(&id(), &host);
        assert_eq!(
            setup.echo.calls.lock().unwrap()[3..],
            ["dialogue 4", "dialogue 4", "dialogue 4"]
        );
    }

    #[test]
    fn estimates_the_time_left_from_the_latest_batches_by_length() {
        let menu = cost(["アイテム"]);
        let line = cost(["今日はどこへ行こうか。駅前のカフェにする？"]);
        assert_eq!((menu, line), (14, 31));
        let mut progress = Progress {
            started: true,
            left: 25 * menu + 40 * line,
            ..Progress::default()
        };
        assert_eq!(progress.view(None).seconds_left, None);

        // 25 menu words in 35 s: the 40 longer lines left take longer.
        progress.record(TextGroup::Menus, 25, 0, 25 * menu, Duration::from_secs(35));
        assert_eq!(progress.view(None).seconds_left, Some(124));

        // A game starts and the translator slows down: the menus drop out of
        // the estimate once enough newer batches are in.
        for _ in 0..RECENT_BATCHES {
            progress.record(TextGroup::Dialogue, 5, 0, 5 * line, Duration::from_secs(60));
        }
        assert_eq!(progress.view(None).seconds_left, Some(240));
    }

    #[test]
    fn counts_lines_left_untranslated_and_tries_them_again_next_time() {
        let setup = turned_on(game_texts(&["あ", "失敗"]));
        setup.service.prepare(&id(), &Host::default());
        let view = state(&setup);
        assert_eq!(view.state, PreparationState::Ready);
        assert_eq!((view.done, view.total, view.missing), (5, 6, 1));
        assert_eq!(setup.records.get().prepared.unwrap().missing, 1);

        setup.echo.calls.lock().unwrap().clear();
        setup.service.request_preparation(&id()).unwrap();
        setup.service.prepare(&id(), &Host::default());
        // Asked twice in each run: a second try for a reply that failed.
        assert_eq!(
            *setup.echo.calls.lock().unwrap(),
            ["dialogue 1", "dialogue 1"]
        );
    }

    #[test]
    fn a_pause_stops_after_the_batch_in_progress_until_resumed() {
        let setup = turned_on(game_texts(&DIALOGUE));
        let host = Host::default();
        // Views: start, names, menus, descriptions, first dialogue batch.
        let _ = host.pause_after.set((5, setup.service.clone()));
        setup.service.prepare(&id(), &host);

        assert_eq!(setup.echo.calls.lock().unwrap().len(), 4);
        let record = setup.records.get();
        assert!(record.paused && record.prepared.is_none());
        let view = state(&setup);
        assert_eq!(view.state, PreparationState::Paused);
        assert_eq!((view.done, view.total), (14, 16));

        // A paused game is not prepared, even when asked by the worker.
        setup.service.prepare(&id(), &Host::default());
        assert_eq!(setup.echo.calls.lock().unwrap().len(), 4);

        setup.service.request_preparation(&id()).unwrap();
        assert!(!setup.records.get().paused);
        setup.service.prepare(&id(), &Host::default());
        assert_eq!(
            setup.echo.calls.lock().unwrap()[4..],
            ["dialogue 2".to_owned()]
        );
        assert_eq!(state(&setup).state, PreparationState::Ready);
    }

    #[test]
    fn says_what_is_missing_when_it_cannot_prepare() {
        let setup = turned_on(game_texts(&["あ"]));
        setup.translations.set_translator(None);
        setup.service.prepare(&id(), &Host::default());
        assert_eq!(state(&setup).state, PreparationState::NeedsTranslator);
        assert!(setup.cache.texts.lock().unwrap().is_empty());

        setup.translations.set_translator(Some(setup.echo.clone()));
        setup.echo.unreachable.store(true, Ordering::SeqCst);
        setup.service.prepare(&id(), &Host::default());
        let view = state(&setup);
        assert_eq!(view.state, PreparationState::Failed);
        assert_eq!(
            view.problem,
            Some(PreparationProblem::TranslatorUnreachable)
        );
        assert!(view.error.unwrap().contains("connection refused"));

        // Asking again starts over.
        setup.echo.unreachable.store(false, Ordering::SeqCst);
        setup.service.request_preparation(&id()).unwrap();
        setup.service.prepare(&id(), &Host::default());
        assert_eq!(state(&setup).state, PreparationState::Ready);
    }

    /// The DLA archive, with one pack for RJ01, or another answer.
    struct Packs {
        made: String,
        answer: Option<PackLookup>,
        downloads: Mutex<usize>,
    }

    impl TranslationPacks for Packs {
        fn find(&self, work: &str, _version: Option<&str>) -> Result<PackLookup, TranslationError> {
            if let Some(answer) = &self.answer {
                return Ok(answer.clone());
            }
            Ok(if work == "RJ01" {
                PackLookup::Offer(PackOffer {
                    work: work.to_owned(),
                    version: "2013-08-25".to_owned(),
                    folder: "/dlsite/_overlays/RJ01/translations/2013-08-25/en".to_owned(),
                    lines: 2,
                    made: self.made.clone(),
                })
            } else {
                PackLookup::None
            })
        }
        fn download(&self, offer: &PackOffer) -> Result<TranslationPack, TranslationError> {
            *self.downloads.lock().unwrap() += 1;
            let line = |japanese: &str, english: &str| PackLine {
                key: text_key(japanese),
                text: english.to_owned(),
                reviewed: true,
                source: None,
            };
            Ok(TranslationPack {
                work: offer.work.clone(),
                made: offer.made.clone(),
                texts: vec![line("あ", "Ah"), line("右上のヒント", "Hint")],
                names: vec![line("ユキナ", "Yukina")],
                glossary: None,
            })
        }
        fn translated_works(&self) -> Result<BTreeMap<String, String>, TranslationError> {
            Ok(BTreeMap::from([("RJ01".to_owned(), self.made.clone())]))
        }
    }

    #[test]
    fn downloads_the_archives_translation_and_translates_only_the_rest() {
        let packs = Arc::new(Packs {
            made: "2026-10-30".to_owned(),
            answer: None,
            downloads: Mutex::default(),
        });
        let records = Arc::new(Records::default());
        let folder = Folder::with_texts(GameEngine::RpgMakerMz, game_texts(&["あ", "い"]));
        let cache = Arc::new(Cache::default());
        let echo = Arc::new(Echo::default());
        let translations = Arc::new(TranslationService::new(cache.clone()));
        translations.set_translator(Some(echo.clone()));
        let service = GameTranslationService::new(
            Arc::new(Installations(installation())),
            Arc::new(NotPrepared),
            records.clone(),
            folder.clone(),
            translations,
        )
        .with_packs(packs.clone());
        service
            .turn_on(&id(), "2026-10-29T10:00:00Z".to_owned())
            .unwrap();
        assert_eq!(
            service.read(&id()).unwrap().pack,
            Some(PackView {
                available: None,
                installed: None,
                access: None,
            })
        );

        let checked = service.check_pack(&id()).unwrap().pack.unwrap();
        let summary = PackSummary {
            lines: 2,
            made: "2026-10-30".to_owned(),
        };
        assert_eq!(checked.available, Some(summary.clone()));

        let downloaded = service.download_pack(&id()).unwrap().pack.unwrap();
        assert_eq!(downloaded.installed, Some(summary));
        assert_eq!(downloaded.available, None);
        assert_eq!(
            records.get().pack,
            Some(InstalledPack {
                made: "2026-10-30".to_owned(),
                lines: 2
            })
        );
        // The game's own copy holds the pack's lines and names.
        let copy = folder.copies.lock().unwrap().last().cloned().unwrap();
        assert!(copy.texts.contains(&("あ".to_owned(), "Ah".to_owned())));
        assert_eq!(copy.names, vec![("ユキナ".to_owned(), "Yukina".to_owned())]);

        // Preparing translates only what the pack lacks.
        service.prepare(&id(), &Host::default());
        assert_eq!(
            *echo.calls.lock().unwrap(),
            ["terms 1", "terms 2", "dialogue 1"]
        );
        assert_eq!(state_of(&service).state, PreparationState::Ready);
    }

    #[test]
    fn an_archive_pack_needs_no_translator_when_it_covers_all_texts_and_names() {
        for (extra_text, extra_name) in [(false, false), (true, false), (false, true)] {
            let mut texts = GameTexts {
                texts: vec![SourceText {
                    text: "あ".to_owned(),
                    kind: TextKind::Dialogue,
                }],
                names: vec!["ユキナ".to_owned()],
                ..GameTexts::default()
            };
            if extra_text {
                texts.texts.push(SourceText {
                    text: "い".to_owned(),
                    kind: TextKind::Dialogue,
                });
            }
            if extra_name {
                texts.names.push("アリス".to_owned());
            }
            let setup = turned_on(texts);
            setup.translations.set_translator(None);
            let service = GameTranslationService::new(
                Arc::new(Installations(installation())),
                Arc::new(NotPrepared),
                setup.records.clone(),
                setup.folder.clone(),
                setup.translations.clone(),
            )
            .with_packs(Arc::new(Packs {
                made: "2026-10-30".to_owned(),
                answer: None,
                downloads: Mutex::default(),
            }));
            let complete = !extra_text && !extra_name;

            let downloaded = service.download_pack(&id()).unwrap();

            assert!(!downloaded.translator_ready);
            assert!(downloaded.pack.unwrap().installed.is_some());
            if complete {
                let preparation = downloaded.preparation.unwrap();
                assert_eq!(preparation.state, PreparationState::Ready);
                assert_eq!((preparation.done, preparation.total), (2, 2));
            }
            let host = Host::default();
            service.prepare(&id(), &host);
            let expected = if complete {
                PreparationState::Ready
            } else {
                PreparationState::NeedsTranslator
            };
            assert_eq!(state_of(&service).state, expected);
            assert_eq!(
                host.views
                    .lock()
                    .unwrap()
                    .last()
                    .unwrap()
                    .preparation
                    .as_ref()
                    .unwrap()
                    .state,
                expected
            );
            assert!(setup.echo.calls.lock().unwrap().is_empty());
            let copies = setup.folder.copies.lock().unwrap();
            let copy = copies.last().unwrap();
            assert!(copy.texts.contains(&("あ".to_owned(), "Ah".to_owned())));
            assert_eq!(copy.names, vec![("ユキナ".to_owned(), "Yukina".to_owned())]);
        }
    }

    #[test]
    fn offers_only_a_newer_pack_than_the_one_in_use() {
        let packs = Arc::new(Packs {
            made: "2026-10-30".to_owned(),
            answer: None,
            downloads: Mutex::default(),
        });
        let setup = turned_on(game_texts(&["あ"]));
        let service = GameTranslationService::new(
            Arc::new(Installations(installation())),
            Arc::new(NotPrepared),
            setup.records.clone(),
            setup.folder.clone(),
            setup.translations.clone(),
        )
        .with_packs(packs);
        let mut record = setup.records.get();
        record.pack = Some(InstalledPack {
            made: "2026-10-30".to_owned(),
            lines: 2,
        });
        setup.records.save_game_translation(&record).unwrap();
        assert_eq!(
            service.check_pack(&id()).unwrap().pack.unwrap().available,
            None
        );
        record.pack = Some(InstalledPack {
            made: "2026-10-01".to_owned(),
            lines: 2,
        });
        setup.records.save_game_translation(&record).unwrap();
        assert!(
            service
                .check_pack(&id())
                .unwrap()
                .pack
                .unwrap()
                .available
                .is_some()
        );
    }

    #[test]
    fn says_why_the_archives_translation_is_not_offered() {
        let setup = turned_on(game_texts(&["あ"]));
        let view_with = |answer: PackLookup| {
            let service = GameTranslationService::new(
                Arc::new(Installations(installation())),
                Arc::new(NotPrepared),
                setup.records.clone(),
                setup.folder.clone(),
                setup.translations.clone(),
            )
            .with_packs(Arc::new(Packs {
                made: "2026-10-30".to_owned(),
                answer: Some(answer),
                downloads: Mutex::default(),
            }));
            service.check_pack(&id()).unwrap().pack.unwrap()
        };
        let for_elite = view_with(PackLookup::ForElite);
        assert_eq!(
            (for_elite.available, for_elite.access),
            (None, Some(PackAccess::ForElite))
        );
        assert_eq!(
            view_with(PackLookup::SignedOut).access,
            Some(PackAccess::SignIn)
        );
        assert_eq!(view_with(PackLookup::None).access, None);
    }

    fn state_of(service: &GameTranslationService) -> PreparationView {
        service.read(&id()).unwrap().preparation.unwrap()
    }

    #[test]
    fn the_worker_prepares_queued_games_until_shut_down() {
        let setup = turned_on(game_texts(&["あ", "い"]));
        let service = setup.service.clone();
        let host = Arc::new(Host::default());
        let worker = {
            let host = host.clone();
            std::thread::spawn(move || service.run_preparations(host.as_ref()))
        };
        for _ in 0..200 {
            if setup.records.get().prepared.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state(&setup).state, PreparationState::Ready);
        setup.service.shut_down();
        worker.join().unwrap();

        // Every translated game is checked again, e.g. after an update.
        setup.service.request_all().unwrap();
        assert_eq!(state(&setup).state, PreparationState::Ready);
        assert_eq!(setup.service.lock_queue().waiting, [id()]);
    }

    #[test]
    fn translates_again_with_a_better_translator_and_keeps_the_packs_lines() {
        let setup = turned_on(game_texts(&["あ", "い"]));
        setup.service.prepare(&id(), &Host::default());
        let sword = text_key("剣");
        setup
            .cache
            .save_pack(
                "RJ01",
                "2026-10-30",
                &[NewTranslation {
                    key: &sword,
                    source: "剣",
                    kind: TextKind::Database,
                    text: "Sword",
                    origin: TranslationOrigin::PackReviewed,
                    model: None,
                }],
            )
            .unwrap();
        assert_eq!(state(&setup).state, PreparationState::Ready);
        *setup.echo.wording.lock().unwrap() = Some("In better English".to_owned());
        setup.echo.calls.lock().unwrap().clear();

        let view = setup.service.translate_again(&id()).unwrap();

        // Only the pack's line and the name list count as done.
        let preparation = view.preparation.unwrap();
        assert_eq!(preparation.state, PreparationState::Waiting);
        assert_eq!((preparation.done, preparation.total), (2, 6));
        // The old lines stay shown until they are replaced.
        let shown = setup.translations.all("RJ01").unwrap().texts;
        assert!(shown.iter().any(|(_, text)| text == "In English"));

        setup.service.prepare(&id(), &Host::default());

        assert_eq!(state(&setup).state, PreparationState::Ready);
        for (source, text) in setup.translations.all("RJ01").unwrap().texts {
            let expected = if source == "剣" {
                "Sword"
            } else {
                "In better English"
            };
            assert_eq!(text, expected, "{source}");
        }
        let sent = setup
            .echo
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.rsplit(' ').next().unwrap().parse::<usize>().unwrap())
            .sum::<usize>();
        assert_eq!(sent, 4, "everything but the pack's line, once");
    }

    #[test]
    fn a_request_while_the_game_is_prepared_runs_once_that_run_ends() {
        let setup = turned_on(game_texts(&["あ"]));
        {
            let mut queue = setup.service.lock_queue();
            queue.waiting.clear();
            queue.current = Some(id());
        }
        setup.service.request_preparation(&id()).unwrap();
        assert!(setup.service.lock_queue().waiting.is_empty());
        setup.service.lock_queue().current = None;

        setup.service.prepare(&id(), &Host::default());

        assert_eq!(setup.service.lock_queue().waiting, [id()]);
    }

    #[test]
    fn a_game_left_out_of_the_queue_is_queued_when_looked_at() {
        let setup = turned_on(game_texts(&["あ"]));
        // The launcher started again, and queueing every game failed.
        let restarted = GameTranslationService::new(
            Arc::new(Installations(installation())),
            Arc::new(NotPrepared),
            setup.records.clone(),
            setup.folder.clone(),
            setup.translations.clone(),
        );
        assert_eq!(state_of(&restarted).state, PreparationState::Waiting);
        assert_eq!(restarted.lock_queue().waiting, [id()]);

        // A game prepared since the start is not queued again on its own.
        restarted.prepare(&id(), &Host::default());
        assert_eq!(state_of(&restarted).state, PreparationState::Ready);
        setup.cache.texts.lock().unwrap().clear();
        state_of(&restarted);
        assert!(restarted.lock_queue().waiting.is_empty());
    }
}
