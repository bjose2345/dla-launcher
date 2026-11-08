//! Plays games translated through the launcher's own code, without its
//! screens: adds the plugin to each game, starts the relay with a translator
//! and serves the games' requests until stopped. For trying the plugin on
//! real games during development.
//!
//! ```text
//! play_translated serve [--prepare] [--packs <folder>] <cache.sqlite> <translator-address> <model> <game-folder>=<work>...
//! play_translated remove <cache.sqlite> <game-folder>=<work>...
//! play_translated seed   <cache.sqlite> <work> <translations.json>
//! ```
//!
//! `serve` keeps what it changed in each game next to the cache, and
//! `remove` puts those files back. With `--prepare`, `serve` also translates
//! the games ahead of play, one after the other, as the launcher does; with
//! `--packs`, it first takes each game's translation from a folder standing
//! for the DLA archive's `/dlsite` (its `_overlays` layout).
//! `seed` loads translations from an earlier test plugin's
//! `translations.json` (`{"text": {...}, "glossary": {...}}`).

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use dla_application::{
    game_translation::{
        GameFolder, GameSession, GameTranslationError, GameTranslationService,
        GameTranslationStore, GameTranslationView, PreparationHost, RelayAccess,
    },
    installation::{InstallationLibraryError, InstallationStore},
    package_preparation::{PackagePreparationError, PackagePreparationStore},
    translation::{
        NewTranslation, SourceText, TranslationService, TranslationStore, Urgency, text_key,
    },
};
use dla_domain::{
    installation::{
        Installation, InstallationDetection, InstallationId, InstallationOverrides,
        InstallationPlatform, InstallationStatus, ManualCatalogIdentity, RelativePath,
    },
    package::PreparedPackageInstallation,
    translation::{
        GameFileChange, GameFileChangeKind, GameTranslation, TextKind, TextMarkup,
        TranslationOrigin,
    },
};
use dla_game_translation::DesktopGameFolder;
use dla_sqlite::SqliteTranslationStore;
use dla_translation_relay::{
    NamesRequest, RelayError, RelayHandler, TranslateRequest, TranslationRelay,
};
use dla_translator::{ArchivePacks, FolderFiles, LocalModelTranslator};
use serde_json::{Value, json};

fn main() {
    let mut arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let prepare = take_flag(&mut arguments, "--prepare");
    let packs = take_option(&mut arguments, "--packs");
    let result = match arguments.first().map(String::as_str) {
        Some("serve") if arguments.len() >= 5 => serve(
            &arguments[1],
            &arguments[2],
            &arguments[3],
            &arguments[4..],
            prepare,
            packs.as_deref(),
        ),
        Some("remove") if arguments.len() >= 3 => remove(&arguments[1], &arguments[2..]),
        Some("seed") if arguments.len() == 4 => seed(&arguments[1], &arguments[2], &arguments[3]),
        _ => Err("usage: see the top of examples/play_translated.rs".to_owned()),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn take_flag(arguments: &mut Vec<String>, flag: &str) -> bool {
    let found = arguments.iter().any(|argument| argument == flag);
    arguments.retain(|argument| argument != flag);
    found
}

fn take_option(arguments: &mut Vec<String>, option: &str) -> Option<String> {
    let at = arguments.iter().position(|argument| argument == option)?;
    arguments.remove(at);
    (at < arguments.len()).then(|| arguments.remove(at))
}

fn serve(
    cache: &str,
    address: &str,
    model: &str,
    games: &[String],
    prepare: bool,
    packs: Option<&str>,
) -> Result<(), String> {
    let store: Arc<dyn TranslationStore> =
        Arc::new(SqliteTranslationStore::open(Path::new(cache)).map_err(text)?);
    let service = Arc::new(
        TranslationService::new(store)
            .with_name_readings(Arc::new(dla_translator::NameList::new())),
    );
    service.set_translator(Some(Arc::new(LocalModelTranslator::new(address, model))));
    let relay = TranslationRelay::start(Arc::new(Bridge(Arc::clone(&service)))).map_err(text)?;
    let access = RelayAccess {
        address: format!("http://{}", relay.address()),
        token: relay.token().to_owned(),
    };
    let folder = DesktopGameFolder::new();
    let mut prepared = Vec::new();
    for game in games {
        let (root, work) = game_and_work(game)?;
        let detection = folder
            .detect_engine(&root)
            .map_err(text)?
            .ok_or("no engine found")?;
        let previous = read_changes(&changes_file(cache, &work)).unwrap_or_default();
        let changes = folder.install(&root, &detection, &previous).map_err(text)?;
        write_changes(&changes_file(cache, &work), &changes)?;
        let translation = GameTranslation {
            installation_id: InstallationId(work.clone()),
            engine: detection.engine,
            game_folder: detection.root.clone(),
            shown: true,
            plugin: folder
                .plugin(detection.engine)
                .ok_or("no plugin for this engine")?,
            changes,
            turned_on_at: String::new(),
            paused: false,
            prepared: None,
            pack: None,
        };
        let session = GameSession {
            work: work.clone(),
            shown: true,
            relay: Some(access.clone()),
            translations: service.all(&work).map_err(text)?,
        };
        folder
            .write_session(&root, &translation, &session)
            .map_err(text)?;
        println!(
            "{work}: {} in {}, {} translations known",
            detection.engine.label(),
            detection
                .root
                .as_ref()
                .map_or("(top)", RelativePath::as_str),
            session.translations.texts.len()
        );
        prepared.push((installation(&root, &work), translation));
    }
    println!("relay on {}; serving until stopped", access.address);
    if prepare || packs.is_some() {
        let ids = prepared
            .iter()
            .map(|(installation, _)| installation.id.clone())
            .collect::<Vec<_>>();
        let games = GameTranslationService::new(
            Arc::new(Installations(
                prepared
                    .iter()
                    .map(|(installation, _)| installation.clone())
                    .collect(),
            )),
            Arc::new(NotPrepared),
            Arc::new(Records(std::sync::Mutex::new(
                prepared
                    .into_iter()
                    .map(|(_, translation)| translation)
                    .collect(),
            ))),
            Arc::new(DesktopGameFolder::new()),
            Arc::clone(&service),
        );
        let games = match packs {
            Some(folder) => games.with_packs(Arc::new(ArchivePacks::new(Arc::new(
                FolderFiles::new(folder.into()),
            )))),
            None => games,
        };
        let with_packs = packs.is_some();
        thread::spawn(move || {
            for id in ids {
                if with_packs {
                    match games.check_pack(&id).and_then(|_| games.download_pack(&id)) {
                        Ok(view) => println!("{} pack {:?}", id.0, view.pack),
                        Err(error) => println!("{} pack not used: {error}", id.0),
                    }
                }
                if prepare {
                    games.prepare(&id, &Console(Instant::now()));
                }
            }
        });
    }
    loop {
        thread::sleep(Duration::from_secs(3600));
    }
}

fn remove(cache: &str, games: &[String]) -> Result<(), String> {
    let folder = DesktopGameFolder::new();
    for game in games {
        let (root, work) = game_and_work(game)?;
        let file = changes_file(cache, &work);
        let changes = read_changes(&file)?;
        folder.remove(&root, &changes).map_err(text)?;
        fs::remove_file(&file).map_err(text)?;
        println!("{work}: {} changes put back", changes.len());
    }
    Ok(())
}

fn seed(cache: &str, work: &str, prototype: &str) -> Result<(), String> {
    let store = SqliteTranslationStore::open(Path::new(cache)).map_err(text)?;
    let saved: Value = serde_json::from_slice(&fs::read(prototype).map_err(text)?).map_err(text)?;
    let texts = saved["text"]
        .as_object()
        .map(|texts| {
            texts
                .iter()
                .filter_map(|(japanese, english)| {
                    english
                        .as_str()
                        .map(|english| (japanese.clone(), english.to_owned()))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let keys = texts
        .iter()
        .map(|(japanese, _)| text_key(japanese))
        .collect::<Vec<_>>();
    let entries = texts
        .iter()
        .zip(&keys)
        .map(|((japanese, english), key)| NewTranslation {
            key,
            source: japanese,
            kind: if japanese.contains('\n') || japanese.chars().count() > 24 {
                TextKind::Dialogue
            } else {
                TextKind::Database
            },
            text: english,
            origin: TranslationOrigin::Local,
            model: Some("prototype"),
        })
        .collect::<Vec<_>>();
    store.save(work, &entries).map_err(text)?;
    let names = saved["glossary"]
        .as_object()
        .map(|names| {
            names
                .iter()
                .filter_map(|(japanese, english)| {
                    english
                        .as_str()
                        .map(|english| (japanese.clone(), english.to_owned()))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    store.save_names(work, &names).map_err(text)?;
    println!(
        "{work}: {} translations and {} names loaded",
        entries.len(),
        names.len()
    );
    Ok(())
}

struct Bridge(Arc<TranslationService>);

impl RelayHandler for Bridge {
    fn translate(&self, request: TranslateRequest) -> Result<Vec<Option<String>>, RelayError> {
        let started = Instant::now();
        let count = request.texts.len();
        let texts = request
            .texts
            .into_iter()
            .map(|text| SourceText {
                text: text.text,
                kind: text.kind,
            })
            .collect::<Vec<_>>();
        let result = self
            .0
            .translate(
                &request.work,
                &texts,
                TextMarkup::RpgMaker,
                urgency(request.urgent),
            )
            .map_err(|error| RelayError::Translator(error.to_string()));
        println!(
            "{} translate {count} ({}) in {:.1}s: {}",
            request.work,
            if request.urgent {
                "urgent"
            } else {
                "background"
            },
            started.elapsed().as_secs_f64(),
            match &result {
                Ok(found) => format!("{} translated", found.iter().flatten().count()),
                Err(error) => format!("{error:?}"),
            }
        );
        result
    }

    fn names(&self, request: NamesRequest) -> Result<HashMap<String, String>, RelayError> {
        let started = Instant::now();
        let result = self
            .0
            .ensure_names(&request.work, &request.names, urgency(request.urgent))
            .map(|names| names.into_iter().collect::<HashMap<_, _>>())
            .map_err(|error| RelayError::Translator(error.to_string()));
        println!(
            "{} names {} in {:.1}s",
            request.work,
            request.names.len(),
            started.elapsed().as_secs_f64()
        );
        result
    }
}

fn urgency(urgent: bool) -> Urgency {
    if urgent {
        Urgency::Now
    } else {
        Urgency::Background
    }
}

fn game_and_work(argument: &str) -> Result<(String, String), String> {
    let (root, work) = argument
        .rsplit_once('=')
        .ok_or("games are given as <game-folder>=<work>")?;
    Ok((root.to_owned(), work.to_owned()))
}

fn changes_file(cache: &str, work: &str) -> PathBuf {
    Path::new(cache).with_file_name(format!("changes-{work}.json"))
}

fn write_changes(file: &Path, changes: &[GameFileChange]) -> Result<(), String> {
    let entries = changes
        .iter()
        .map(|change| match &change.kind {
            GameFileChangeKind::Added => json!({ "path": change.path.as_str() }),
            GameFileChangeKind::Changed {
                original,
                written_sha256,
                written_bytes,
            } => json!({
                "path": change.path.as_str(),
                "original": hex::encode(original),
                "writtenSha256": written_sha256,
                "writtenBytes": written_bytes,
            }),
            GameFileChangeKind::Patched {
                at,
                original,
                original_bytes,
                written_sha256,
                written_bytes,
            } => json!({
                "path": change.path.as_str(),
                "patchedAt": at,
                "original": hex::encode(original),
                "originalBytes": original_bytes,
                "writtenSha256": written_sha256,
                "writtenBytes": written_bytes,
            }),
        })
        .collect::<Vec<_>>();
    fs::write(file, serde_json::to_vec_pretty(&entries).map_err(text)?).map_err(text)
}

fn read_changes(file: &Path) -> Result<Vec<GameFileChange>, String> {
    let entries: Vec<Value> =
        serde_json::from_slice(&fs::read(file).map_err(text)?).map_err(text)?;
    entries
        .iter()
        .map(|entry| {
            let path =
                RelativePath::parse(entry["path"].as_str().unwrap_or_default()).map_err(text)?;
            let written_sha256 = entry["writtenSha256"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let written_bytes = entry["writtenBytes"].as_u64().unwrap_or_default();
            let kind = match (entry["original"].as_str(), entry["patchedAt"].as_u64()) {
                (None, _) => GameFileChangeKind::Added,
                (Some(original), None) => GameFileChangeKind::Changed {
                    original: hex::decode(original).map_err(text)?,
                    written_sha256,
                    written_bytes,
                },
                (Some(original), Some(at)) => GameFileChangeKind::Patched {
                    at,
                    original: hex::decode(original).map_err(text)?,
                    original_bytes: entry["originalBytes"].as_u64().unwrap_or_default(),
                    written_sha256,
                    written_bytes,
                },
            };
            Ok(GameFileChange { path, kind })
        })
        .collect()
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The games being served, as the launcher's Library would hold them.
fn installation(root: &str, work: &str) -> Installation {
    Installation {
        id: InstallationId(work.to_owned()),
        scan_root_id: None,
        root_path: root.to_owned(),
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
                work_code: work.to_owned(),
            }),
            ..InstallationOverrides::default()
        },
        discovered_at: String::new(),
        updated_at: String::new(),
    }
}

struct Installations(Vec<Installation>);

impl InstallationStore for Installations {
    fn create(&self, _installation: &Installation) -> Result<(), InstallationLibraryError> {
        Err(InstallationLibraryError::persistence("read only"))
    }
    fn create_or_refresh(
        &self,
        _installation: &Installation,
    ) -> Result<Installation, InstallationLibraryError> {
        Err(InstallationLibraryError::persistence("read only"))
    }
    fn read(&self, id: &InstallationId) -> Result<Option<Installation>, InstallationLibraryError> {
        Ok(self
            .0
            .iter()
            .find(|installation| installation.id == *id)
            .cloned())
    }
    fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
        Ok(self.0.clone())
    }
    fn replace_detection(
        &self,
        _installation_id: &InstallationId,
        _detection: &InstallationDetection,
        _status: InstallationStatus,
        _updated_at: &str,
    ) -> Result<(), InstallationLibraryError> {
        Err(InstallationLibraryError::persistence("read only"))
    }
    fn replace_overrides(
        &self,
        _installation_id: &InstallationId,
        _overrides: &InstallationOverrides,
        _status: InstallationStatus,
        _updated_at: &str,
    ) -> Result<(), InstallationLibraryError> {
        Err(InstallationLibraryError::persistence("read only"))
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
        Ok(())
    }
}

struct Records(std::sync::Mutex<Vec<GameTranslation>>);

impl GameTranslationStore for Records {
    fn read_game_translation(
        &self,
        id: &InstallationId,
    ) -> Result<Option<GameTranslation>, GameTranslationError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|translation| translation.installation_id == *id)
            .cloned())
    }
    fn list_game_translations(&self) -> Result<Vec<GameTranslation>, GameTranslationError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save_game_translation(
        &self,
        translation: &GameTranslation,
    ) -> Result<(), GameTranslationError> {
        let mut translations = self.0.lock().unwrap();
        translations.retain(|kept| kept.installation_id != translation.installation_id);
        translations.push(translation.clone());
        Ok(())
    }
    fn remove_game_translation(&self, id: &InstallationId) -> Result<(), GameTranslationError> {
        self.0
            .lock()
            .unwrap()
            .retain(|kept| kept.installation_id != *id);
        Ok(())
    }
}

/// Prints the progress the launcher would show.
struct Console(Instant);

impl PreparationHost for Console {
    fn now(&self) -> String {
        format!("{:.0}s after start", self.0.elapsed().as_secs_f64())
    }
    fn game_running(&self) -> bool {
        false
    }
    fn only_when_idle(&self) -> bool {
        false
    }
    fn changed(&self, view: &GameTranslationView) {
        let Some(preparation) = &view.preparation else {
            return;
        };
        let groups = preparation
            .groups
            .iter()
            .map(|group| format!("{:?} {}/{}", group.group, group.done, group.total))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{} prepare {:?} {}/{} [{groups}] left {} missing {} at {:.0}s{}",
            view.installation_id.0,
            preparation.state,
            preparation.done,
            preparation.total,
            preparation
                .seconds_left
                .map_or_else(|| "-".to_owned(), |seconds| format!("{seconds}s")),
            preparation.missing,
            self.0.elapsed().as_secs_f64(),
            preparation
                .error
                .as_ref()
                .map_or_else(String::new, |error| format!(" error: {error}"))
        );
    }
}
