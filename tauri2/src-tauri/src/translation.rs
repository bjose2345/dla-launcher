//! Game translation: the player's translator, the local relay game plugins
//! reach it through, and the translation of each installed game.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use dla_application::{
    game_translation::{
        GameTranslationService, GameTranslationView, PreparationHost, PreparationState, RelayAccess,
    },
    launch::LaunchService,
    translation::{
        BatchStyle, SourceText, TranslationError, TranslationPacks, TranslationService,
        TranslatorBatch, Urgency,
    },
};
use dla_domain::{
    installation::InstallationId,
    translation::{TextMarkup, TranslatorSettings},
};
use dla_translation_relay::{
    NamesRequest, RelayError, RelayHandler, TranslateRequest, TranslationRelay,
};
use dla_translator::{ArchivePacks, FolderFiles, SavedTranslator, TranslatorSettingsFile};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::{AppState, blocking};

pub const GAME_TRANSLATION_CHANGED_EVENT: &str = "game-translation-changed";
const TEST_LINE: &str = "オープニングを飛ばしますか？";
// Translated games are checked for new lines once the launcher has settled.
const CHECK_AFTER_START: Duration = Duration::from_secs(20);

pub struct TranslationController {
    service: Arc<TranslationService>,
    games: Arc<GameTranslationService>,
    settings: TranslatorSettingsFile,
    saved: Mutex<SavedTranslator>,
    // Started with the first translated game, so the launcher opens no port
    // for players who never use translation.
    relay: Mutex<Option<TranslationRelay>>,
    only_when_idle: Arc<AtomicBool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationSettingsView {
    pub translator: Option<TranslatorSettings>,
    pub deepl_key_saved: bool,
    pub prepare_only_when_idle: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatorTestResult {
    pub source: String,
    pub english: String,
    pub seconds: f64,
}

impl TranslationController {
    pub fn new(
        service: Arc<TranslationService>,
        games: Arc<GameTranslationService>,
        settings: TranslatorSettingsFile,
        launch: Arc<LaunchService>,
        app: AppHandle,
    ) -> Self {
        let saved = settings.read().unwrap_or_else(|error| {
            log::warn!(target: "dla::translation", "event=settings_unreadable error={error}");
            SavedTranslator::default()
        });
        service.set_translator(saved.translator());
        let only_when_idle = Arc::new(AtomicBool::new(saved.prepare_only_when_idle));
        let host = TauriPreparationHost {
            app,
            launch,
            only_when_idle: Arc::clone(&only_when_idle),
        };
        let worker = Arc::clone(&games);
        if let Err(error) = thread::Builder::new()
            .name("dla-translation-prepare".to_owned())
            .spawn(move || {
                thread::sleep(CHECK_AFTER_START);
                if let Err(error) = worker.request_all() {
                    log::warn!(target: "dla::translation", "event=check_failed error={error}");
                }
                worker.run_preparations(&host);
            })
        {
            log::warn!(target: "dla::translation", "event=prepare_worker_failed error={error}");
        }
        Self {
            service,
            games,
            settings,
            saved: Mutex::new(saved),
            relay: Mutex::new(None),
            only_when_idle,
        }
    }

    pub fn games(&self) -> &GameTranslationService {
        &self.games
    }

    pub fn settings(&self) -> TranslationSettingsView {
        view(&self.saved.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// `deepl_key`: `None` keeps the saved key, an empty one forgets it.
    /// `prepare_only_when_idle`: `None` keeps the saved choice.
    pub fn save_settings(
        &self,
        translator: Option<TranslatorSettings>,
        deepl_key: Option<String>,
        prepare_only_when_idle: Option<bool>,
    ) -> Result<TranslationSettingsView, String> {
        let mut saved = self.saved.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = SavedTranslator {
            translator,
            deepl_key: saved.deepl_key.clone(),
            prepare_only_when_idle: prepare_only_when_idle.unwrap_or(saved.prepare_only_when_idle),
        };
        if let Some(key) = deepl_key {
            next.deepl_key = Some(key.trim().to_owned()).filter(|key| !key.is_empty());
        }
        self.settings
            .save(&next)
            .map_err(|error| format!("the translation settings could not be saved: {error}"))?;
        let translator = next.translator();
        let ready = translator.is_some();
        self.service.set_translator(translator);
        self.only_when_idle
            .store(next.prepare_only_when_idle, Ordering::SeqCst);
        *saved = next;
        let view = view(&saved);
        drop(saved);
        // Games that waited for a translator start now.
        if ready && let Err(error) = self.games.request_all() {
            log::warn!(target: "dla::translation", "event=check_failed error={error}");
        }
        Ok(view)
    }

    /// Translates one line with the given settings, so the player sees that
    /// the translator works and how fast it is on this PC.
    pub fn test(
        &self,
        translator: TranslatorSettings,
        deepl_key: Option<String>,
    ) -> Result<TranslatorTestResult, String> {
        let saved_key = self
            .saved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .deepl_key
            .clone();
        let candidate = SavedTranslator {
            translator: Some(translator),
            deepl_key: deepl_key
                .map(|key| key.trim().to_owned())
                .filter(|key| !key.is_empty())
                .or(saved_key),
            prepare_only_when_idle: false,
        };
        let translator = candidate
            .translator()
            .ok_or_else(|| "Enter an address, or a DeepL key, first.".to_owned())?;
        let started = Instant::now();
        let lines = [TEST_LINE.to_owned()];
        let reply = translator
            .translate(&TranslatorBatch {
                style: BatchStyle::Dialogue,
                lines: &lines,
                names: &[],
                readings: &[],
                note: None,
                terms: &[],
            })
            .map_err(|error| error.to_string())?;
        let english = reply
            .into_iter()
            .next()
            .flatten()
            .ok_or_else(|| "The translator answered, but with no translation.".to_owned())?;
        Ok(TranslatorTestResult {
            source: TEST_LINE.to_owned(),
            english,
            seconds: started.elapsed().as_secs_f64(),
        })
    }

    /// Before a game starts. A translation problem never keeps a game from
    /// starting: the game then plays with what it has.
    pub fn before_launch(&self, installation_id: &InstallationId) {
        match self
            .games
            .prepare_launch(installation_id, || self.relay_access())
        {
            Ok(true) => log::info!(
                target: "dla::translation",
                "event=game_translated installation_id={}",
                installation_id.0
            ),
            Ok(false) => {}
            Err(error) => log::warn!(
                target: "dla::translation",
                "event=game_translation_unavailable installation_id={} error={error}",
                installation_id.0
            ),
        }
    }

    fn relay_access(&self) -> Option<RelayAccess> {
        let mut relay = self.relay.lock().unwrap_or_else(PoisonError::into_inner);
        if relay.is_none() {
            match TranslationRelay::start(Arc::new(RelayBridge(Arc::clone(&self.service)))) {
                Ok(started) => {
                    log::info!(
                        target: "dla::translation",
                        "event=relay_started port={}",
                        started.address().port()
                    );
                    *relay = Some(started);
                }
                Err(error) => {
                    log::warn!(target: "dla::translation", "event=relay_failed error={error}");
                    return None;
                }
            }
        }
        relay.as_ref().map(|relay| RelayAccess {
            address: format!("http://{}", relay.address()),
            token: relay.token().to_owned(),
        })
    }
}

/// Where the DLA archive's translations come from: the archive, through the
/// account signed in for Install. `DLA_TRANSLATION_PACKS` names a folder
/// standing for `/overlays/dlsite` instead, while testing.
pub fn pack_source(
    #[cfg(desktop)] source: Option<(
        Arc<dla_delivery::OverlayClient>,
        Option<Arc<crate::delivery::Forward>>,
    )>,
) -> Option<Arc<dyn TranslationPacks>> {
    if let Some(folder) =
        std::env::var_os("DLA_TRANSLATION_PACKS").filter(|value| !value.is_empty())
    {
        return Some(Arc::new(ArchivePacks::new(Arc::new(FolderFiles::new(
            folder.into(),
        )))));
    }
    #[cfg(desktop)]
    if let Some((client, forward)) = source {
        return Some(Arc::new(ArchivePacks::new(Arc::new(
            archive::OverlayFiles { client, forward },
        ))));
    }
    None
}

/// The archive as the account signed in for Install may read it.
#[cfg(desktop)]
mod archive {
    use std::sync::Arc;

    use dla_application::delivery::DeliveryError;
    use dla_delivery::OverlayClient;
    use dla_domain::delivery::DeliveryProblem;
    use dla_translator::{ArchiveFiles, FileError};

    pub struct OverlayFiles {
        pub client: Arc<OverlayClient>,
        pub forward: Option<Arc<crate::delivery::Forward>>,
    }

    impl ArchiveFiles for OverlayFiles {
        fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, FileError> {
            if !self.client.signed_in() {
                return Err(FileError::SignedOut);
            }
            if let Some(forward) = &self.forward {
                forward.ensure().map_err(file_error)?;
            }
            self.client.read(path, limit).map_err(file_error)
        }
    }

    pub fn file_error(error: DeliveryError) -> FileError {
        match &error {
            DeliveryError::Problem { problem, .. } => match problem {
                DeliveryProblem::SignInRequired | DeliveryProblem::Credentials => {
                    FileError::SignedOut
                }
                DeliveryProblem::Forbidden => FileError::Forbidden,
                DeliveryProblem::NotInArchive => FileError::NotFound,
                _ => FileError::Other(error.to_string()),
            },
            _ => FileError::Other(error.to_string()),
        }
    }
}

fn view(saved: &SavedTranslator) -> TranslationSettingsView {
    TranslationSettingsView {
        translator: saved.translator.clone(),
        deepl_key_saved: saved.deepl_key.is_some(),
        prepare_only_when_idle: saved.prepare_only_when_idle,
    }
}

/// The launcher around the worker that translates games ahead of play.
struct TauriPreparationHost {
    app: AppHandle,
    launch: Arc<LaunchService>,
    only_when_idle: Arc<AtomicBool>,
}

impl PreparationHost for TauriPreparationHost {
    fn now(&self) -> String {
        dla_sqlite::current_timestamp()
    }

    fn game_running(&self) -> bool {
        self.launch.is_running()
    }

    fn only_when_idle(&self) -> bool {
        self.only_when_idle.load(Ordering::SeqCst)
    }

    fn changed(&self, view: &GameTranslationView) {
        if let Some(preparation) = &view.preparation {
            match preparation.state {
                PreparationState::Ready => log::info!(
                    target: "dla::translation",
                    "event=game_prepared installation_id={} texts={} missing={}",
                    view.installation_id.0,
                    preparation.total,
                    preparation.missing
                ),
                PreparationState::Failed => log::warn!(
                    target: "dla::translation",
                    "event=game_preparation_failed installation_id={} error={}",
                    view.installation_id.0,
                    preparation.error.as_deref().unwrap_or_default()
                ),
                _ => {}
            }
        }
        if let Err(error) = self.app.emit(GAME_TRANSLATION_CHANGED_EVENT, view) {
            log::warn!(target: "dla::translation", "event=emit_failed error={error}");
        }
    }
}

/// Game plugins' requests, answered by the translation service. Their text
/// is RPG Maker's: the only plugin so far.
struct RelayBridge(Arc<TranslationService>);

impl RelayHandler for RelayBridge {
    fn translate(&self, request: TranslateRequest) -> Result<Vec<Option<String>>, RelayError> {
        let texts = request
            .texts
            .into_iter()
            .map(|text| SourceText {
                text: text.text,
                kind: text.kind,
            })
            .collect::<Vec<_>>();
        self.0
            .translate(
                &request.work,
                &texts,
                TextMarkup::RpgMaker,
                urgency(request.urgent),
            )
            .map_err(relay_error)
    }

    fn names(&self, request: NamesRequest) -> Result<HashMap<String, String>, RelayError> {
        self.0
            .ensure_names(&request.work, &request.names, urgency(request.urgent))
            .map(|names| names.into_iter().collect())
            .map_err(relay_error)
    }
}

fn urgency(urgent: bool) -> Urgency {
    if urgent {
        Urgency::Now
    } else {
        Urgency::Background
    }
}

fn relay_error(error: TranslationError) -> RelayError {
    log::warn!(target: "dla::translation", "event=translation_failed error={error}");
    match error {
        TranslationError::NoTranslator => RelayError::NoTranslator,
        TranslationError::Translator(error) => RelayError::Translator(error.to_string()),
        TranslationError::Persistence(error) | TranslationError::Pack(error) => {
            RelayError::Internal(error)
        }
    }
}

#[tauri::command]
pub async fn read_translation_settings(
    state: tauri::State<'_, AppState>,
) -> Result<TranslationSettingsView, String> {
    Ok(state.translation.settings())
}

#[tauri::command]
pub async fn save_translation_settings(
    state: tauri::State<'_, AppState>,
    translator: Option<TranslatorSettings>,
    deepl_key: Option<String>,
    prepare_only_when_idle: Option<bool>,
) -> Result<TranslationSettingsView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || translation.save_settings(translator, deepl_key, prepare_only_when_idle)).await
}

#[tauri::command]
pub async fn test_translator(
    state: tauri::State<'_, AppState>,
    translator: TranslatorSettings,
    deepl_key: Option<String>,
) -> Result<TranslatorTestResult, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || translation.test(translator, deepl_key)).await
}

#[tauri::command]
pub async fn read_game_translation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<GameTranslationView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || translation.games().read(&InstallationId(installation_id))).await
}

#[tauri::command]
pub async fn turn_on_game_translation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<GameTranslationView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        translation.games().turn_on(
            &InstallationId(installation_id),
            dla_sqlite::current_timestamp(),
        )
    })
    .await
}

#[tauri::command]
pub async fn show_game_translation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
    shown: bool,
) -> Result<GameTranslationView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        translation
            .games()
            .show(&InstallationId(installation_id), shown)
    })
    .await
}

#[tauri::command]
pub async fn remove_game_translation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<(), String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || translation.games().remove(&InstallationId(installation_id))).await
}

#[tauri::command]
pub async fn prepare_game_translation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<GameTranslationView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        translation
            .games()
            .request_preparation(&InstallationId(installation_id))
    })
    .await
}

#[tauri::command]
pub async fn translate_game_again(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<GameTranslationView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        translation
            .games()
            .translate_again(&InstallationId(installation_id))
    })
    .await
}

#[tauri::command]
pub async fn pause_game_translation(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<GameTranslationView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        translation
            .games()
            .pause_preparation(&InstallationId(installation_id))
    })
    .await
}

#[tauri::command]
pub async fn check_translation_pack(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<GameTranslationView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        translation
            .games()
            .check_pack(&InstallationId(installation_id))
    })
    .await
}

/// Every work the DLA archive has a translation of, and when its newest was
/// made, for the library to mark them.
#[tauri::command]
pub async fn read_translated_works(
    state: tauri::State<'_, AppState>,
) -> Result<BTreeMap<String, String>, String> {
    let translation = Arc::clone(&state.translation);
    let works = blocking(move || translation.games().translated_works()).await;
    match &works {
        Ok(works) => {
            log::info!(target: "dla::translation", "event=translated_works count={}", works.len())
        }
        Err(error) => {
            log::warn!(target: "dla::translation", "event=translated_works_failed error={error}")
        }
    }
    works
}

#[tauri::command]
pub async fn download_translation_pack(
    state: tauri::State<'_, AppState>,
    installation_id: String,
) -> Result<GameTranslationView, String> {
    let translation = Arc::clone(&state.translation);
    blocking(move || {
        let installation_id = InstallationId(installation_id);
        let result = translation.games().download_pack(&installation_id);
        match &result {
            Ok(view) => log::info!(
                target: "dla::translation",
                "event=pack_downloaded installation_id={} lines={}",
                installation_id.0,
                view.pack
                    .as_ref()
                    .and_then(|pack| pack.installed.as_ref())
                    .map_or(0, |installed| installed.lines)
            ),
            Err(error) => log::warn!(
                target: "dla::translation",
                "event=pack_download_failed installation_id={} error={error}",
                installation_id.0
            ),
        }
        result
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(desktop)]
    #[test]
    fn reads_the_archives_answers_for_packs() {
        use dla_application::delivery::DeliveryError;
        use dla_domain::delivery::DeliveryProblem;
        use dla_translator::FileError;

        let answer = |problem| archive::file_error(DeliveryError::problem(problem, "detail"));
        assert_eq!(answer(DeliveryProblem::Forbidden), FileError::Forbidden);
        assert_eq!(answer(DeliveryProblem::NotInArchive), FileError::NotFound);
        assert_eq!(
            answer(DeliveryProblem::SignInRequired),
            FileError::SignedOut
        );
        assert_eq!(
            answer(DeliveryProblem::Unreachable),
            FileError::Other("detail".to_owned())
        );
    }
}
