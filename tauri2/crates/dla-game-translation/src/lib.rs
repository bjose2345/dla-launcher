//! The launcher's translation plugins in games' folders: finding a game's
//! engine, adding the plugin, handing it what it needs before each start,
//! and putting every file back when translation is removed.

pub mod pairing;
mod rgss3a;
mod rpg_maker;
mod rpg_maker_data;
mod ruby_marshal;
mod vx_ace;
mod vx_ace_data;

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use dla_application::{
    game_translation::{GameFolder, GameSession, GameTexts, GameTranslationError, RelayAccess},
    translation::WorkTranslations,
};
use dla_detection::detect_game_engine;
use dla_domain::{
    game_engine::{GameEngine, GameEngineDetection},
    installation::RelativePath,
    translation::{GameFileChange, GameFileChangeKind, GameTranslation},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

/// The plugin's own folder inside the game's: what the launcher hands it,
/// the translations it shows, and its log.
const DATA_FOLDER: &str = "dla-translation";
const PLUGIN_LIST: &str = "js/plugins.js";
/// RPG Maker's own: maps, events and the database.
const DATA_FOLDER_OF_GAME: &str = "data";
const PLUGIN_FOLDER: &str = "js/plugins";
// Engines keep their files near the top; a deep or huge folder is not walked
// to the end.
const DETECTION_DEPTH: usize = 6;
const DETECTION_FILES: usize = 50_000;

#[derive(Default)]
pub struct DesktopGameFolder;

impl DesktopGameFolder {
    pub fn new() -> Self {
        Self
    }
}

impl GameFolder for DesktopGameFolder {
    fn detect_engine(
        &self,
        root: &str,
    ) -> Result<Option<GameEngineDetection>, GameTranslationError> {
        let root = Path::new(root);
        if !root.is_dir() {
            return Err(GameTranslationError::folder(format!(
                "{} is not a folder",
                root.display()
            )));
        }
        let files = WalkDir::new(root)
            .max_depth(DETECTION_DEPTH)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter_map(|entry| portable(root, entry.path()))
            .take(DETECTION_FILES)
            .collect::<Vec<_>>();
        Ok(detect_game_engine(files.iter().map(String::as_str)))
    }

    fn plugin(&self, engine: GameEngine) -> Option<String> {
        match engine {
            GameEngine::RpgMakerMz | GameEngine::RpgMakerMv => {
                let mut digest = Sha256::new();
                for (name, script) in rpg_maker::PLUGINS {
                    digest.update(name.as_bytes());
                    digest.update(script.as_bytes());
                }
                Some(hex::encode(&digest.finalize()[..8]))
            }
            GameEngine::RpgMakerVxAce => Some(hex::encode(&vx_ace::fingerprint()[..8])),
            _ => None,
        }
    }

    fn install(
        &self,
        root: &str,
        detection: &GameEngineDetection,
        previous: &[GameFileChange],
    ) -> Result<Vec<GameFileChange>, GameTranslationError> {
        if self.plugin(detection.engine).is_none() {
            return Err(GameTranslationError::Unsupported(detection.engine.label()));
        }
        let folder = GameFiles::new(root, detection.root.as_ref());
        if detection.engine == GameEngine::RpgMakerVxAce {
            fs::create_dir_all(folder.absolute(DATA_FOLDER))
                .map_err(GameTranslationError::folder)?;
            let mut changes = vec![
                vx_ace::install(&folder.absolute(""), |path| folder.relative(path), previous)?,
                GameFileChange {
                    path: folder.relative(DATA_FOLDER)?,
                    kind: GameFileChangeKind::Added,
                },
            ];
            changes.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
            return Ok(changes);
        }
        let list = folder.read_text(PLUGIN_LIST)?;
        let mut changes = Vec::new();
        // The plugins are in place before the list names them.
        for (name, script) in rpg_maker::PLUGINS {
            let path = format!("{PLUGIN_FOLDER}/{name}");
            changes.push(folder.write(&path, script.as_bytes(), previous)?);
        }
        let data = folder.relative(DATA_FOLDER)?;
        fs::create_dir_all(folder.absolute(DATA_FOLDER)).map_err(GameTranslationError::folder)?;
        changes.push(GameFileChange {
            path: data,
            kind: GameFileChangeKind::Added,
        });
        let listed = rpg_maker::lists_our_plugins(&list).map_err(|_| unknown_list())?;
        let path = folder.relative(PLUGIN_LIST)?;
        let earlier = previous.iter().find(|change| change.path == path);
        let change = match (listed, earlier) {
            // Already listed: keep the original recorded when it was added.
            (true, Some(earlier)) => GameFileChange {
                kind: written(earlier.kind.clone(), list.as_bytes()),
                path,
            },
            (true, None) => GameFileChange {
                kind: GameFileChangeKind::Changed {
                    original: rpg_maker::remove_our_plugins(&list)
                        .map_err(|_| unknown_list())?
                        .into_bytes(),
                    written_sha256: sha256(list.as_bytes()),
                    written_bytes: list.len() as u64,
                },
                path,
            },
            // Not listed (yet, or no longer after a game update).
            (false, _) => {
                let added = rpg_maker::add_our_plugins(&list).map_err(|_| unknown_list())?;
                folder.replace(PLUGIN_LIST, added.as_bytes())?;
                GameFileChange {
                    kind: GameFileChangeKind::Changed {
                        original: list.into_bytes(),
                        written_sha256: sha256(added.as_bytes()),
                        written_bytes: added.len() as u64,
                    },
                    path,
                }
            }
        };
        changes.push(change);
        changes.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
        Ok(changes)
    }

    fn remove(&self, root: &str, changes: &[GameFileChange]) -> Result<(), GameTranslationError> {
        let root = Path::new(root);
        // The list stops naming the plugins before their files go.
        for change in changes {
            let path = root.join(change.path.as_str());
            match &change.kind {
                GameFileChangeKind::Changed {
                    original,
                    written_sha256,
                    ..
                } => restore(&path, original, written_sha256)?,
                // The archive's own record of the patch puts it back; an
                // archive a game update replaced has none.
                GameFileChangeKind::Patched { .. } if path.is_file() => {
                    rgss3a::remove_patch(&path).map_err(|error| {
                        GameTranslationError::folder(format!("{}: {error}", path.display()))
                    })?;
                }
                _ => {}
            }
        }
        for change in changes {
            if change.kind == GameFileChangeKind::Added {
                delete(&root.join(change.path.as_str()))?;
            }
        }
        Ok(())
    }

    fn write_session(
        &self,
        root: &str,
        translation: &GameTranslation,
        session: &GameSession,
    ) -> Result<(), GameTranslationError> {
        let folder = GameFiles::new(root, translation.game_folder.as_ref());
        let data = folder.absolute(DATA_FOLDER);
        fs::create_dir_all(&data).map_err(GameTranslationError::folder)?;
        let launcher = json!({
            "version": 1,
            "work": session.work,
            "shown": session.shown,
            "relay": session.relay.as_ref().map(|RelayAccess { address, token }| json!({
                "address": address,
                "token": token,
            })),
        });
        replace(
            &data.join("launcher.json"),
            &serde_json::to_vec_pretty(&launcher).map_err(GameTranslationError::folder)?,
        )?;
        self.write_translations(root, translation, &session.translations)
    }

    fn read_texts(
        &self,
        root: &str,
        translation: &GameTranslation,
    ) -> Result<GameTexts, GameTranslationError> {
        if self.plugin(translation.engine).is_none() {
            return Err(GameTranslationError::Unsupported(
                translation.engine.label(),
            ));
        }
        let folder = GameFiles::new(root, translation.game_folder.as_ref());
        let texts = if translation.engine == GameEngine::RpgMakerVxAce {
            vx_ace_data::read(&folder.absolute(""))
        } else {
            rpg_maker_data::read(&folder.absolute(DATA_FOLDER_OF_GAME))
        };
        texts.map_err(|error| {
            GameTranslationError::folder(format!("the game's texts could not be read: {error}"))
        })
    }

    fn write_translations(
        &self,
        root: &str,
        translation: &GameTranslation,
        translations: &WorkTranslations,
    ) -> Result<(), GameTranslationError> {
        let folder = GameFiles::new(root, translation.game_folder.as_ref());
        let data = folder.absolute(DATA_FOLDER);
        fs::create_dir_all(&data).map_err(GameTranslationError::folder)?;
        // The game's copy may know lines the launcher has since forgotten
        // (a reset cache); the launcher's own win.
        let copy = data.join("translations.json");
        let existing = fs::read(&copy)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null);
        let texts = merge(&existing["texts"], &translations.texts);
        let names = merge(&existing["names"], &translations.names);
        if translation.engine == GameEngine::RpgMakerVxAce {
            // The texts the game's scripts use keep their Japanese in the
            // data; the script draws their English.
            let in_scripts =
                vx_ace_data::strings_in_scripts(&folder.absolute("")).map_err(|error| {
                    GameTranslationError::folder(format!(
                        "the game's scripts could not be read: {error}"
                    ))
                })?;
            let kept = texts
                .keys()
                .filter(|text| in_scripts.contains(*text))
                .cloned()
                .collect::<Vec<_>>();
            // The script's Ruby has no JSON; it reads this one.
            replace(
                &data.join(vx_ace::TRANSLATIONS),
                &vx_ace::translations_file(&texts, &names, &kept),
            )?;
        }
        let merged = json!({
            "version": 1,
            "texts": texts,
            "names": names,
        });
        replace(
            &copy,
            &serde_json::to_vec(&merged).map_err(GameTranslationError::folder)?,
        )
    }

    fn reads_translations_from_folder(&self, engine: GameEngine) -> bool {
        engine == GameEngine::RpgMakerVxAce
    }

    fn current_place(&self, root: &str, translation: &GameTranslation) -> Option<String> {
        (translation.engine == GameEngine::RpgMakerVxAce)
            .then(|| {
                let folder = GameFiles::new(root, translation.game_folder.as_ref());
                vx_ace::place(&folder.absolute(DATA_FOLDER))
            })
            .flatten()
    }
}

/// A game's files, by paths relative to the engine's own folder.
struct GameFiles {
    root: PathBuf,
    game_folder: Option<RelativePath>,
}

impl GameFiles {
    fn new(root: &str, game_folder: Option<&RelativePath>) -> Self {
        Self {
            root: PathBuf::from(root),
            game_folder: game_folder.cloned(),
        }
    }

    /// The path relative to the installation, as changes record it.
    fn relative(&self, path: &str) -> Result<RelativePath, GameTranslationError> {
        let joined = match &self.game_folder {
            Some(folder) => format!("{}/{path}", folder.as_str()),
            None => path.to_owned(),
        };
        RelativePath::parse(joined).map_err(GameTranslationError::folder)
    }

    fn absolute(&self, path: &str) -> PathBuf {
        match &self.game_folder {
            Some(folder) => self.root.join(folder.as_str()).join(path),
            None => self.root.join(path),
        }
    }

    fn read_text(&self, path: &str) -> Result<String, GameTranslationError> {
        fs::read_to_string(self.absolute(path)).map_err(|error| {
            GameTranslationError::folder(format!("{path} could not be read: {error}"))
        })
    }

    fn replace(&self, path: &str, bytes: &[u8]) -> Result<(), GameTranslationError> {
        replace(&self.absolute(path), bytes)
    }

    /// Writes one of the launcher's files. A file the game had at that path
    /// before is recorded with its original content.
    fn write(
        &self,
        path: &str,
        bytes: &[u8],
        previous: &[GameFileChange],
    ) -> Result<GameFileChange, GameTranslationError> {
        let relative = self.relative(path)?;
        let absolute = self.absolute(path);
        let kind = match previous.iter().find(|change| change.path == relative) {
            Some(earlier) => written(earlier.kind.clone(), bytes),
            None => match fs::read(&absolute) {
                Ok(original) => written(
                    GameFileChangeKind::Changed {
                        original,
                        written_sha256: String::new(),
                        written_bytes: 0,
                    },
                    bytes,
                ),
                Err(error) if error.kind() == io::ErrorKind::NotFound => GameFileChangeKind::Added,
                Err(error) => return Err(GameTranslationError::folder(error)),
            },
        };
        replace(&absolute, bytes)?;
        Ok(GameFileChange {
            path: relative,
            kind,
        })
    }
}

fn written(kind: GameFileChangeKind, bytes: &[u8]) -> GameFileChangeKind {
    match kind {
        GameFileChangeKind::Added => GameFileChangeKind::Added,
        GameFileChangeKind::Changed { original, .. } => GameFileChangeKind::Changed {
            original,
            written_sha256: sha256(bytes),
            written_bytes: bytes.len() as u64,
        },
        // Only archives are patched, and never written whole.
        patched @ GameFileChangeKind::Patched { .. } => patched,
    }
}

/// Puts a changed file back. When it changed again since (the player or a
/// game update), only the launcher's plugins come out of a plugin list, and
/// any other file is left as it is.
fn restore(path: &Path, original: &[u8], written_sha256: &str) -> Result<(), GameTranslationError> {
    let current = match fs::read(path) {
        Ok(current) => current,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return replace(path, original);
        }
        Err(error) => return Err(GameTranslationError::folder(error)),
    };
    if sha256(&current) == written_sha256 {
        return replace(path, original);
    }
    if path.ends_with(PLUGIN_LIST)
        && let Ok(list) = String::from_utf8(current)
        && rpg_maker::lists_our_plugins(&list).unwrap_or(false)
    {
        let removed = rpg_maker::remove_our_plugins(&list).map_err(|_| unknown_list())?;
        return replace(path, removed.as_bytes());
    }
    Ok(())
}

fn delete(path: &Path) -> Result<(), GameTranslationError> {
    let result = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) => Err(error),
    };
    match result {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            Err(GameTranslationError::folder(error))
        }
        _ => Ok(()),
    }
}

/// Writes the whole file or nothing: a game started meanwhile never reads
/// half of it.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), GameTranslationError> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".dla-new");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes)
        .and_then(|()| fs::rename(&temporary, path))
        .map_err(|error| {
            let _ = fs::remove_file(&temporary);
            GameTranslationError::folder(format!("{}: {error}", path.display()))
        })
}

fn merge(existing: &Value, launcher: &[(String, String)]) -> BTreeMap<String, String> {
    let mut merged = existing
        .as_object()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(japanese, english)| {
                    english
                        .as_str()
                        .map(|english| (japanese.clone(), english.to_owned()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    for (japanese, english) in launcher {
        merged.insert(japanese.clone(), english.clone());
    }
    merged
}

fn portable(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts = relative
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    Some(parts.join("/"))
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn unknown_list() -> GameTranslationError {
    GameTranslationError::folder(format!(
        "the game's plugin list ({PLUGIN_LIST}) is laid out in a way the launcher does not know"
    ))
}

#[cfg(test)]
mod tests {
    use dla_application::translation::WorkTranslations;
    use dla_domain::installation::InstallationId;
    use tempfile::{TempDir, tempdir};

    use super::*;

    const LIST: &str = "// Generated by RPG Maker.\n// Do not edit this file directly.\nvar $plugins =\n[\n{\"name\":\"A\",\"status\":true,\"description\":\"\",\"parameters\":{}}\n];\n";

    /// An RPG Maker MV game: the engine's files are in `Game/www`.
    fn mv_game() -> TempDir {
        let directory = tempdir().unwrap();
        let www = directory.path().join("Game/www");
        fs::create_dir_all(www.join("js/plugins")).unwrap();
        fs::write(www.join("js/rpg_core.js"), "core").unwrap();
        fs::write(www.join("js/plugins.js"), LIST).unwrap();
        fs::write(www.join("js/plugins/A.js"), "a").unwrap();
        fs::write(directory.path().join("Game/Game.exe"), "exe").unwrap();
        directory
    }

    fn root(directory: &TempDir) -> &str {
        directory.path().to_str().unwrap()
    }

    fn snapshot(directory: &TempDir) -> BTreeMap<String, Vec<u8>> {
        WalkDir::new(directory.path())
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| {
                (
                    portable(directory.path(), entry.path()).unwrap(),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect()
    }

    fn translation(
        detection: &GameEngineDetection,
        changes: Vec<GameFileChange>,
    ) -> GameTranslation {
        GameTranslation {
            installation_id: InstallationId("installation-1".to_owned()),
            engine: detection.engine,
            game_folder: detection.root.clone(),
            shown: true,
            plugin: DesktopGameFolder.plugin(detection.engine).unwrap(),
            changes,
            turned_on_at: "2026-10-29T10:00:00Z".to_owned(),
            paused: false,
            prepared: None,
            pack: None,
        }
    }

    #[test]
    fn installs_into_the_engine_folder_and_removes_without_a_trace() {
        let game = mv_game();
        let before = snapshot(&game);
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::RpgMakerMv);
        assert_eq!(detection.root.as_ref().unwrap().as_str(), "Game/www");

        let changes = folder.install(root(&game), &detection, &[]).unwrap();
        let paths = changes
            .iter()
            .map(|change| {
                (
                    change.path.as_str(),
                    change.kind == GameFileChangeKind::Added,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            [
                ("Game/www/dla-translation", true),
                ("Game/www/js/plugins.js", false),
                ("Game/www/js/plugins/DLA_Translate.js", true),
                ("Game/www/js/plugins/DLA_TranslateSettings.js", true),
            ]
        );
        let list = fs::read_to_string(game.path().join("Game/www/js/plugins.js")).unwrap();
        assert!(rpg_maker::lists_our_plugins(&list).unwrap());
        assert!(
            fs::read_to_string(game.path().join("Game/www/js/plugins/DLA_Translate.js"))
                .unwrap()
                .contains("@plugindesc")
        );

        // A second install (a launch) changes nothing and keeps the original.
        assert_eq!(
            folder.install(root(&game), &detection, &changes).unwrap(),
            changes
        );

        folder
            .write_session(
                root(&game),
                &translation(&detection, changes.clone()),
                &GameSession {
                    work: "RJ1".to_owned(),
                    shown: true,
                    relay: None,
                    translations: WorkTranslations::default(),
                },
            )
            .unwrap();
        fs::write(game.path().join("Game/www/dla-translation/log.txt"), "log").unwrap();

        folder.remove(root(&game), &changes).unwrap();
        assert_eq!(snapshot(&game), before);
    }

    #[test]
    fn re_adds_the_plugins_after_a_game_update_and_restores_the_updated_list() {
        let game = mv_game();
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        let changes = folder.install(root(&game), &detection, &[]).unwrap();

        let updated = LIST.replace("\"A\"", "\"B\"");
        fs::write(game.path().join("Game/www/js/plugins.js"), &updated).unwrap();
        let after_update = folder.install(root(&game), &detection, &changes).unwrap();
        let list = fs::read_to_string(game.path().join("Game/www/js/plugins.js")).unwrap();
        assert!(rpg_maker::lists_our_plugins(&list).unwrap());

        folder.remove(root(&game), &after_update).unwrap();
        assert_eq!(
            fs::read_to_string(game.path().join("Game/www/js/plugins.js")).unwrap(),
            updated
        );
    }

    #[test]
    fn takes_only_its_plugins_out_of_a_list_changed_since() {
        let game = mv_game();
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        let changes = folder.install(root(&game), &detection, &[]).unwrap();
        let path = game.path().join("Game/www/js/plugins.js");
        let edited = fs::read_to_string(&path).unwrap().replace(
            "\"name\":\"A\",\"status\":true",
            "\"name\":\"A\",\"status\":false",
        );
        fs::write(&path, &edited).unwrap();

        folder.remove(root(&game), &changes).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            LIST.replace("\"status\":true", "\"status\":false")
        );
        assert!(
            !game
                .path()
                .join("Game/www/js/plugins/DLA_Translate.js")
                .exists()
        );
    }

    #[test]
    fn hands_the_plugin_the_relay_and_keeps_lines_only_the_game_knows() {
        let game = mv_game();
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        let changes = folder.install(root(&game), &detection, &[]).unwrap();
        let data = game.path().join("Game/www/dla-translation");
        fs::write(
            data.join("translations.json"),
            r#"{"version":1,"texts":{"いいえ":"No","はい":"Yeah"},"names":{}}"#,
        )
        .unwrap();

        folder
            .write_session(
                root(&game),
                &translation(&detection, changes),
                &GameSession {
                    work: "RJ1".to_owned(),
                    shown: true,
                    relay: Some(RelayAccess {
                        address: "http://127.0.0.1:4000".to_owned(),
                        token: "secret".to_owned(),
                    }),
                    translations: WorkTranslations {
                        texts: vec![("はい".to_owned(), "Yes".to_owned())],
                        names: vec![("ユキナ".to_owned(), "Yukina".to_owned())],
                    },
                },
            )
            .unwrap();

        let launcher: Value =
            serde_json::from_slice(&fs::read(data.join("launcher.json")).unwrap()).unwrap();
        assert_eq!(launcher["work"], "RJ1");
        assert_eq!(launcher["shown"], true);
        assert_eq!(launcher["relay"]["address"], "http://127.0.0.1:4000");
        assert_eq!(launcher["relay"]["token"], "secret");
        let copy: Value =
            serde_json::from_slice(&fs::read(data.join("translations.json")).unwrap()).unwrap();
        assert_eq!(copy["texts"], json!({ "いいえ": "No", "はい": "Yes" }));
        assert_eq!(copy["names"], json!({ "ユキナ": "Yukina" }));
    }

    #[test]
    fn reads_the_texts_from_the_engine_folder() {
        let game = mv_game();
        let data = game.path().join("Game/www/data");
        fs::create_dir_all(&data).unwrap();
        fs::write(
            data.join("System.json"),
            r#"{"terms":{"commands":["アイテム"]}}"#,
        )
        .unwrap();
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        let texts = folder
            .read_texts(root(&game), &translation(&detection, Vec::new()))
            .unwrap();
        assert_eq!(texts.texts[0].text, "アイテム");
    }

    #[test]
    fn has_no_plugin_for_other_engines() {
        let directory = tempdir().unwrap();
        fs::create_dir_all(directory.path().join("Game_Data")).unwrap();
        fs::write(directory.path().join("UnityPlayer.dll"), "x").unwrap();
        fs::write(directory.path().join("Game_Data/globalgamemanagers"), "x").unwrap();
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&directory)).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::UnityMono);
        assert_eq!(folder.plugin(detection.engine), None);
        assert!(matches!(
            folder.install(root(&directory), &detection, &[]),
            Err(GameTranslationError::Unsupported("Unity"))
        ));
    }
}
