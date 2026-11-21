//! The launcher's translation plugins in games' folders: finding a game's
//! engine, adding the plugin, handing it what it needs before each start,
//! and putting every file back when translation is removed.

mod enigma;
pub mod pairing;
mod rgss;
mod rgss3a;
mod rgss_texts;
mod rgssad;
mod rpg_maker;
mod rpg_maker_data;
mod rpg_maker_settings;
mod ruby_files;
mod ruby_marshal;
mod script_texts;
mod vx_ace;
mod vx_ace_data;
mod wolf;
mod wrap;
mod xp_vx;
mod xp_vx_data;

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

use crate::rgss::Generation;

/// The plugin's own folder inside the game's: what the launcher hands it,
/// the translations it shows, and its log.
const DATA_FOLDER: &str = "dla-translation";
/// English images in the plugin's folder, laid out as the game's own `img`
/// (RPG Maker MV and MZ) or `Graphics` (VX Ace).
const PICTURES_FOLDER: &str = "img";
const PLUGIN_LIST: &str = "js/plugins.js";
/// RPG Maker's own: maps, events and the database.
const DATA_FOLDER_OF_GAME: &str = "data";
const PLUGIN_FOLDER: &str = "js/plugins";
// Engines keep their files near the top; a deep or huge folder is not walked
// to the end.
const DETECTION_DEPTH: usize = 6;
const DETECTION_FILES: usize = 50_000;
/// How many executables of a folder are looked into for a packed game.
const PACKED_PROBES: usize = 16;

#[derive(Default)]
pub struct DesktopGameFolder;

impl DesktopGameFolder {
    pub fn new() -> Self {
        Self
    }

    /// Whether the game has the image an English one would stand in for:
    /// `path` is the image's under the game's `img` (RPG Maker MV and MZ)
    /// or `Graphics` (VX Ace), without its extension. Other engines show no
    /// English images.
    pub fn has_image(
        &self,
        root: &str,
        translation: &GameTranslation,
        path: &str,
    ) -> Result<bool, GameTranslationError> {
        let folder = GameFiles::new(root, translation.game_folder.as_ref());
        match translation.engine {
            // Plain, or encrypted as MZ (`.png_`) and MV (`.rpgmvp`) keep them.
            GameEngine::RpgMakerMv | GameEngine::RpgMakerMz => {
                Ok(["png", "png_", "rpgmvp"].iter().any(|extension| {
                    folder
                        .absolute(&format!("{PICTURES_FOLDER}/{path}.{extension}"))
                        .is_file()
                }))
            }
            GameEngine::RpgMakerVxAce => {
                rgss::GameData::find(&folder.absolute(""), Generation::VxAce)
                    .and_then(|data| data.has_image(path))
                    .map_err(GameTranslationError::folder)
            }
            engine => Err(GameTranslationError::Unsupported(engine.label())),
        }
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
        Ok(detect_game_engine(files.iter().map(String::as_str))
            .or_else(|| packed_detection(root, &files)))
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
            GameEngine::WolfRpg => Some(wolf::fingerprint()),
            engine => {
                Generation::of(engine).map(|generation| hex::encode(&generation.fingerprint()[..8]))
            }
        }
    }

    fn can_translate(&self, root: &str, detection: &GameEngineDetection) -> bool {
        if let Some((_, packed)) = packed_game(Path::new(root), detection) {
            return packed.program.is_some() && unpacks(detection.engine);
        }
        match detection.engine {
            GameEngine::WolfRpg => {
                wolf::is_readable(&GameFiles::new(root, detection.root.as_ref()).absolute(""))
            }
            engine => self.plugin(engine).is_some(),
        }
    }

    fn unpack_bytes(&self, root: &str, detection: &GameEngineDetection) -> Option<u64> {
        let root = Path::new(root);
        let (from, packed) = packed_game(root, detection)?;
        let folder = root.join(from.as_str());
        let folder = folder.parent()?;
        let missing = packed
            .files
            .iter()
            .filter(|file| !folder.join(&file.path).exists())
            .map(|file| file.bytes)
            .sum::<u64>();
        (missing > 0).then_some(missing)
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
        if detection.engine == GameEngine::WolfRpg {
            let recorded = |path: &Path| {
                portable(Path::new(root), path)
                    .ok_or_else(|| GameTranslationError::folder("a path is not readable"))
                    .and_then(|path| {
                        RelativePath::parse(path).map_err(GameTranslationError::folder)
                    })
            };
            let mut changes = vec![GameFileChange {
                path: folder.relative(DATA_FOLDER)?,
                kind: GameFileChangeKind::Added,
            }];
            for moved in wolf::install(&folder.absolute(""), &folder.absolute(DATA_FOLDER))? {
                changes.push(GameFileChange {
                    path: recorded(&moved.from)?,
                    kind: GameFileChangeKind::Moved {
                        to: recorded(&moved.to)?,
                    },
                });
                for added in &moved.added {
                    changes.push(GameFileChange {
                        path: recorded(added)?,
                        kind: GameFileChangeKind::Added,
                    });
                }
            }
            changes.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
            return Ok(changes);
        }
        if let Some(generation) = Generation::of(detection.engine) {
            fs::create_dir_all(folder.absolute(DATA_FOLDER))
                .map_err(GameTranslationError::folder)?;
            let mut changes = vec![
                rgss::install(
                    &folder.absolute(""),
                    generation,
                    |path| folder.relative(path),
                    previous,
                )?,
                GameFileChange {
                    path: folder.relative(DATA_FOLDER)?,
                    kind: GameFileChangeKind::Added,
                },
            ];
            changes.sort_by(|left, right| left.path.as_str().cmp(right.path.as_str()));
            return Ok(changes);
        }
        let (unpacked, previous) = unpack(Path::new(root), detection, previous)?;
        let previous = previous.as_slice();
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
        // One record a path: a file the launcher then changed is that change.
        let unpacked = unpacked
            .into_iter()
            .filter(|file| !changes.iter().any(|change| change.path == file.path))
            .collect::<Vec<_>>();
        changes.extend(unpacked);
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
                    rgss::remove_archive_patch(&path).map_err(|error| {
                        GameTranslationError::folder(format!("{}: {error}", path.display()))
                    })?;
                }
                // The game's own data goes back before the launcher's
                // folders go, so the game is whole at every step.
                GameFileChangeKind::Moved { to } => {
                    let launcher_folder = path
                        .parent()
                        .and_then(Path::parent)
                        .map(|game| game.join(DATA_FOLDER))
                        .unwrap_or_default();
                    wolf::restore(&path, &root.join(to.as_str()), &launcher_folder).map_err(
                        |error| {
                            GameTranslationError::folder(format!("{}: {error}", path.display()))
                        },
                    )?;
                }
                _ => {}
            }
        }
        for change in changes {
            if change.kind == GameFileChangeKind::Added {
                delete(&root.join(change.path.as_str()))?;
            }
        }
        // What was unpacked goes only while the executable still holds it:
        // without the executable these files are the game.
        let from = changes.iter().find_map(|change| match &change.kind {
            GameFileChangeKind::Unpacked { from, .. } => Some(from),
            _ => None,
        });
        if let Some(from) = from {
            let executable = root.join(from.as_str());
            if let Ok(Some(packed)) = enigma::read(&executable) {
                let folder = executable.parent().unwrap_or(root);
                for change in changes {
                    let GameFileChangeKind::Changed { original, .. } = &change.kind else {
                        continue;
                    };
                    let path = root.join(change.path.as_str());
                    // A changed file put back is one the executable holds.
                    if let Some(held) = portable(folder, &path)
                        && packed.holds(&executable, &held, original)
                        && fs::read(&path).is_ok_and(|current| current == *original)
                    {
                        delete(&path)?;
                    }
                }
                remove_unpacked(root, changes)?;
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
        self.write(root, translation, &session.translations, session.shown)
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
        if translation.engine == GameEngine::WolfRpg {
            return wolf::read_texts(&folder.absolute(""));
        }
        let texts = match Generation::of(translation.engine) {
            Some(Generation::VxAce) => vx_ace_data::read(&folder.absolute("")),
            Some(generation) => xp_vx_data::read(&folder.absolute(""), generation),
            None => rpg_maker_data::read(
                &folder.absolute(DATA_FOLDER_OF_GAME),
                &folder.absolute(PLUGIN_LIST),
                &folder.absolute(PLUGIN_FOLDER),
            ),
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
        self.write(root, translation, translations, false)
    }

    fn reads_translations_from_folder(&self, engine: GameEngine) -> bool {
        engine == GameEngine::WolfRpg || Generation::of(engine).is_some()
    }

    fn current_place(&self, root: &str, translation: &GameTranslation) -> Option<String> {
        Generation::of(translation.engine).and_then(|_| {
            let folder = GameFiles::new(root, translation.game_folder.as_ref());
            rgss::place(&folder.absolute(DATA_FOLDER))
        })
    }
}

impl DesktopGameFolder {
    /// Writes the translations the game's plugin reads. `starting` is for a
    /// game about to start with its English shown: the Ruby files it loads
    /// from its own folder and its script sections are read once, while it
    /// starts, so their copies are written only then.
    fn write(
        &self,
        root: &str,
        translation: &GameTranslation,
        translations: &WorkTranslations,
        starting: bool,
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
        if let Some(generation) = Generation::of(translation.engine) {
            // The texts the game's scripts use keep their Japanese in the
            // data; the script draws their English.
            let scripts =
                rgss::strings_in_scripts(&folder.absolute(""), generation).map_err(|error| {
                    GameTranslationError::folder(format!(
                        "the game's scripts could not be read: {error}"
                    ))
                })?;
            let letters = scripts.shown.letters();
            if starting {
                scripts
                    .files
                    .write(&data.join(ruby_files::FOLDER), &texts, letters)?;
                scripts
                    .shown
                    .write(&data.join(script_texts::FOLDER), &texts)?;
            }
            let kept = texts
                .keys()
                .filter(|text| scripts.keeps(text))
                .cloned()
                .collect::<Vec<_>>();
            let pictures = translations
                .pictures
                .iter()
                .map(|picture| picture.path.as_str())
                .collect::<Vec<_>>();
            // The script's Ruby has no JSON; it reads this one.
            replace(
                &data.join(generation.translations_file()),
                &rgss::translations_file(&texts, &names, &kept, &pictures, letters, generation),
            )?;
        }
        if translation.engine == GameEngine::WolfRpg {
            wolf::write_translations(
                &folder.absolute(""),
                &data,
                &texts,
                &names,
                translation.shown,
            )?;
        }
        let merged = json!({
            "version": 1,
            "texts": texts,
            "names": names,
            "speakers": translations.speakers,
        });
        replace(
            &copy,
            &serde_json::to_vec(&merged).map_err(GameTranslationError::folder)?,
        )?;
        write_pictures(&data.join(PICTURES_FOLDER), &translations.pictures)
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
        // Only archives are patched or moved, and never written whole; an
        // unpacked file the launcher writes is recorded as changed instead.
        kept @ (GameFileChangeKind::Patched { .. }
        | GameFileChangeKind::Moved { .. }
        | GameFileChangeKind::Unpacked { .. }) => kept,
    }
}

fn is_executable(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".exe")
}

/// The engines whose games the launcher unpacks: their plugin goes into
/// loose files, and their program runs the game from its folder.
fn unpacks(engine: GameEngine) -> bool {
    matches!(engine, GameEngine::RpgMakerMv | GameEngine::RpgMakerMz)
}

/// The paths of the files an executable holds, from the folder its own
/// path is from.
fn held_paths(executable: &str, packed: &enigma::Packed) -> Vec<String> {
    let folder = executable.rsplit_once('/').map(|(folder, _)| folder);
    packed
        .files
        .iter()
        .map(|file| match folder {
            Some(folder) => format!("{folder}/{}", file.path),
            None => file.path.clone(),
        })
        .collect()
}

/// The engine of a game whose files are inside one of the executables
/// found (Enigma Virtual Box).
fn packed_detection(root: &Path, files: &[String]) -> Option<GameEngineDetection> {
    files
        .iter()
        .filter(|path| is_executable(path))
        .take(PACKED_PROBES)
        .find_map(|path| {
            let packed = enigma::read(&root.join(path)).ok()??;
            detect_game_engine(held_paths(path, &packed).iter().map(String::as_str))
        })
}

/// The executable that holds the files of the game found, in the engine's
/// folder or the one above it (RPG Maker MV's `www`).
fn packed_game(
    root: &Path,
    detection: &GameEngineDetection,
) -> Option<(RelativePath, enigma::Packed)> {
    let game = detection.root.as_ref().map_or("", RelativePath::as_str);
    let above = game.rsplit_once('/').map_or("", |(above, _)| above);
    let folders = if game.is_empty() {
        vec![game]
    } else {
        vec![game, above]
    };
    folders.into_iter().find_map(|folder| {
        let mut executables = fs::read_dir(root.join(folder))
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| is_executable(name))
            .collect::<Vec<_>>();
        executables.sort();
        executables
            .into_iter()
            .take(PACKED_PROBES)
            .find_map(|name| {
                let path = if folder.is_empty() {
                    name
                } else {
                    format!("{folder}/{name}")
                };
                let packed = enigma::read(&root.join(&path)).ok()??;
                let held = held_paths(&path, &packed);
                let found = detect_game_engine(held.iter().map(String::as_str))?;
                if found.engine != detection.engine || found.root != detection.root {
                    return None;
                }
                Some((RelativePath::parse(path).ok()?, packed))
            })
    })
}

/// Takes the game's files out of the executable that holds them, when one
/// does: the files as recorded, and the earlier changes the engine's own
/// install goes by. Nothing is done again while the executable holds the
/// list it held; one a game update replaced is unpacked anew.
fn unpack(
    root: &Path,
    detection: &GameEngineDetection,
    previous: &[GameFileChange],
) -> Result<(Vec<GameFileChange>, Vec<GameFileChange>), GameTranslationError> {
    let earlier = previous
        .iter()
        .filter(|change| matches!(change.kind, GameFileChangeKind::Unpacked { .. }))
        .cloned()
        .collect::<Vec<_>>();
    let from = earlier.iter().find_map(|change| match &change.kind {
        GameFileChangeKind::Unpacked { from, .. } => Some(from.clone()),
        _ => None,
    });
    let found = match from {
        Some(from) => enigma::read(&root.join(from.as_str()))
            .ok()
            .flatten()
            .map(|packed| (from, packed)),
        None => packed_game(root, detection),
    };
    // Not packed, or the executable is gone and the folder is the game.
    let Some((from, packed)) = found else {
        return Ok((earlier, previous.to_vec()));
    };
    let list = earlier.iter().find_map(|change| match &change.kind {
        GameFileChangeKind::Unpacked { starts, .. } => starts.as_deref(),
        _ => None,
    });
    if list == Some(packed.list_sha256.as_str()) {
        return Ok((earlier, previous.to_vec()));
    }
    let executable = root.join(from.as_str());
    let folder = from.as_str().rsplit_once('/').map(|(folder, _)| folder);
    let recorded = |path: &str| {
        RelativePath::parse(match folder {
            Some(folder) => format!("{folder}/{path}"),
            None => path.to_owned(),
        })
        .map_err(GameTranslationError::folder)
    };
    let mut previous = previous.to_vec();
    if !earlier.is_empty() {
        let held = |change: &GameFileChange| {
            packed
                .files
                .iter()
                .any(|file| recorded(&file.path).is_ok_and(|path| path == change.path))
        };
        // What the old executable held goes, the launcher's changes to it
        // first; a file the player changed stays.
        for change in &previous {
            if let GameFileChangeKind::Changed {
                original,
                written_sha256,
                ..
            } = &change.kind
                && held(change)
            {
                let path = root.join(change.path.as_str());
                restore(&path, original, written_sha256)?;
                if fs::read(&path).is_ok_and(|current| current == *original) {
                    delete(&path)?;
                }
            }
        }
        remove_unpacked(root, &earlier)?;
        previous.retain(|change| match change.kind {
            GameFileChangeKind::Unpacked { .. } => false,
            GameFileChangeKind::Changed { .. } => !held(change),
            _ => true,
        });
    }
    let program = packed.program.ok_or_else(|| {
        GameTranslationError::folder(
            "the game's executable does not hold the program it was made from",
        )
    })?;
    let program = &packed.files[program].path;
    let unpacked = enigma::unpack(&executable, &packed)
        .map_err(GameTranslationError::folder)?
        .into_iter()
        .map(|file| {
            Ok(GameFileChange {
                path: recorded(&file.path)?,
                kind: GameFileChangeKind::Unpacked {
                    from: from.clone(),
                    starts: (file.path == *program).then(|| packed.list_sha256.clone()),
                    sha256: file.sha256,
                    bytes: file.bytes,
                },
            })
        })
        .collect::<Result<Vec<_>, GameTranslationError>>()?;
    Ok((unpacked, previous))
}

/// Deletes the unpacked files that are still as they were written.
fn remove_unpacked(root: &Path, changes: &[GameFileChange]) -> Result<(), GameTranslationError> {
    let written = changes
        .iter()
        .filter_map(|change| match &change.kind {
            GameFileChangeKind::Unpacked { sha256, bytes, .. } => {
                Some((root.join(change.path.as_str()), *bytes, sha256.as_str()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    enigma::remove(root, &written).map_err(GameTranslationError::folder)
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

/// Makes a folder hold exactly these images: the launcher's are written
/// where they changed, and one it no longer has is taken away, so the game
/// goes back to its own.
fn write_pictures(
    folder: &Path,
    pictures: &[dla_application::translation::TranslatedPicture],
) -> Result<(), GameTranslationError> {
    dla_application::translation::validate_pictures(pictures)
        .map_err(GameTranslationError::folder)?;
    let wanted = pictures
        .iter()
        .map(|picture| (folder.join(format!("{}.png", picture.path)), &picture.png))
        .collect::<BTreeMap<_, _>>();
    if folder.is_dir() {
        let kept = WalkDir::new(folder)
            .contents_first(true)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.path() != folder);
        for entry in kept {
            // A folder that still holds an image is not empty and stays.
            let _ = match entry.file_type().is_dir() {
                true => fs::remove_dir(entry.path()),
                false if wanted.contains_key(entry.path()) => Ok(()),
                false => fs::remove_file(entry.path()),
            };
        }
    }
    for (path, png) in wanted {
        if fs::read(&path).is_ok_and(|written| &written == png) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(GameTranslationError::folder)?;
        }
        replace(&path, png)?;
    }
    if pictures.is_empty() {
        let _ = fs::remove_dir(folder);
    }
    Ok(())
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

    /// An RPG Maker MZ game inside `Game/Packed.exe`, with a save the game
    /// wrote beside it.
    fn packed_game(list: &str, system: &str) -> TempDir {
        let directory = tempdir().unwrap();
        fs::create_dir_all(directory.path().join("Game/save")).unwrap();
        fs::write(
            directory.path().join("Game/Packed.exe"),
            enigma::tests::packed_game(7, &packed_files(list, system)),
        )
        .unwrap();
        fs::write(directory.path().join("Game/save/file1.rmmzsave"), "save").unwrap();
        directory
    }

    fn packed_files<'a>(list: &'a str, system: &'a str) -> Vec<(&'a str, &'a [u8])> {
        vec![
            ("js/rmmz_core.js", b"core".as_slice()),
            ("js/plugins.js", list.as_bytes()),
            ("js/plugins/A.js", b"a".as_slice()),
            ("data/System.json", system.as_bytes()),
            ("save/config.rmmzsave", b"as shipped".as_slice()),
        ]
    }

    fn kinds(changes: &[GameFileChange]) -> Vec<(&str, &str)> {
        changes
            .iter()
            .map(|change| {
                let kind = match &change.kind {
                    GameFileChangeKind::Added => "added",
                    GameFileChangeKind::Changed { .. } => "changed",
                    GameFileChangeKind::Unpacked { starts: None, .. } => "unpacked",
                    GameFileChangeKind::Unpacked { .. } => "program",
                    _ => "other",
                };
                (change.path.as_str(), kind)
            })
            .collect()
    }

    #[test]
    fn unpacks_a_game_packed_into_its_executable_and_removes_without_a_trace() {
        let game = packed_game(LIST, "{}");
        let before = snapshot(&game);
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::RpgMakerMz);
        assert_eq!(detection.root.as_ref().unwrap().as_str(), "Game");
        assert!(folder.can_translate(root(&game), &detection));
        assert_eq!(
            folder.unpack_bytes(root(&game), &detection),
            Some(1024 + 4 + LIST.len() as u64 + 1 + 2 + 10)
        );

        let changes = folder.install(root(&game), &detection, &[]).unwrap();
        assert_eq!(folder.unpack_bytes(root(&game), &detection), None);
        assert_eq!(
            kinds(&changes),
            [
                ("Game/Game.exe", "program"),
                ("Game/data/System.json", "unpacked"),
                ("Game/dla-translation", "added"),
                ("Game/js/plugins.js", "changed"),
                ("Game/js/plugins/A.js", "unpacked"),
                ("Game/js/plugins/DLA_Translate.js", "added"),
                ("Game/js/plugins/DLA_TranslateSettings.js", "added"),
                ("Game/js/rmmz_core.js", "unpacked"),
                ("Game/save/config.rmmzsave", "unpacked"),
            ]
        );
        let program = changes
            .iter()
            .find_map(GameFileChange::unpacked_program)
            .unwrap();
        assert_eq!(program.in_place_of.as_str(), "Game/Packed.exe");
        assert_eq!(program.path.as_str(), "Game/Game.exe");
        assert_eq!(
            program.sha256,
            sha256(&fs::read(game.path().join("Game/Game.exe")).unwrap())
        );
        let list = fs::read_to_string(game.path().join("Game/js/plugins.js")).unwrap();
        assert!(rpg_maker::lists_our_plugins(&list).unwrap());
        assert_eq!(
            folder.detect_engine(root(&game)).unwrap().unwrap(),
            detection
        );

        // A launch unpacks nothing again.
        assert_eq!(
            folder.install(root(&game), &detection, &changes).unwrap(),
            changes
        );

        // The player's settings and saves are theirs, also where the
        // executable had a file of its own.
        fs::write(
            game.path().join("Game/save/config.rmmzsave"),
            "the player's",
        )
        .unwrap();
        fs::write(game.path().join("Game/save/file2.rmmzsave"), "save 2").unwrap();
        folder.remove(root(&game), &changes).unwrap();
        let mut expected = before;
        expected.insert(
            "Game/save/config.rmmzsave".to_owned(),
            b"the player's".to_vec(),
        );
        expected.insert("Game/save/file2.rmmzsave".to_owned(), b"save 2".to_vec());
        assert_eq!(snapshot(&game), expected);
        assert!(!game.path().join("Game/js").exists());
        assert!(!game.path().join("Game/data").exists());
    }

    #[test]
    fn unpacks_anew_an_executable_a_game_update_replaced() {
        let game = packed_game(LIST, "{}");
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        let changes = folder.install(root(&game), &detection, &[]).unwrap();

        let updated = LIST.replace("\"A\"", "\"B\"");
        let mut files = packed_files(&updated, "{\"versionId\":2}");
        files.push(("data/Map002.json", b"{}"));
        let executable = enigma::tests::packed_game(7, &files);
        fs::write(game.path().join("Game/Packed.exe"), &executable).unwrap();
        let after_update = folder.install(root(&game), &detection, &changes).unwrap();

        assert_ne!(after_update, changes);
        assert_eq!(
            fs::read_to_string(game.path().join("Game/data/System.json")).unwrap(),
            "{\"versionId\":2}"
        );
        assert!(game.path().join("Game/data/Map002.json").exists());
        let list = fs::read_to_string(game.path().join("Game/js/plugins.js")).unwrap();
        assert!(rpg_maker::lists_our_plugins(&list).unwrap() && list.contains("\"B\""));
        let original = after_update.iter().find_map(|change| match &change.kind {
            GameFileChangeKind::Changed { original, .. } => Some(original.clone()),
            _ => None,
        });
        assert_eq!(original, Some(updated.into_bytes()));
        assert_eq!(
            folder
                .install(root(&game), &detection, &after_update)
                .unwrap(),
            after_update
        );

        folder.remove(root(&game), &after_update).unwrap();
        assert_eq!(
            snapshot(&game),
            BTreeMap::from([
                ("Game/Packed.exe".to_owned(), executable),
                ("Game/save/file1.rmmzsave".to_owned(), b"save".to_vec()),
            ])
        );
    }

    #[test]
    fn what_was_unpacked_stays_once_the_executable_is_gone() {
        let game = packed_game(LIST, "{}");
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        let changes = folder.install(root(&game), &detection, &[]).unwrap();
        fs::remove_file(game.path().join("Game/Packed.exe")).unwrap();

        assert_eq!(
            folder.detect_engine(root(&game)).unwrap().unwrap(),
            detection
        );
        assert_eq!(
            folder.install(root(&game), &detection, &changes).unwrap(),
            changes
        );
        folder.remove(root(&game), &changes).unwrap();
        assert_eq!(
            fs::read_to_string(game.path().join("Game/js/plugins.js")).unwrap(),
            LIST
        );
        assert!(game.path().join("Game/Game.exe").exists());
        assert!(game.path().join("Game/js/rmmz_core.js").exists());
        assert!(
            !game
                .path()
                .join("Game/js/plugins/DLA_Translate.js")
                .exists()
        );
        assert!(!game.path().join("Game/dla-translation").exists());
    }

    #[test]
    fn a_packed_game_without_its_program_or_of_another_engine_is_not_offered() {
        let directory = tempdir().unwrap();
        let folder = DesktopGameFolder::new();
        fs::write(
            directory.path().join("Packed.exe"),
            enigma::tests::packed_files(7, &packed_files(LIST, "{}")),
        )
        .unwrap();
        let detection = folder.detect_engine(root(&directory)).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::RpgMakerMz);
        assert_eq!(detection.root, None);
        assert!(!folder.can_translate(root(&directory), &detection));
        assert!(folder.install(root(&directory), &detection, &[]).is_err());
        assert_eq!(snapshot(&directory).len(), 1);

        fs::write(
            directory.path().join("Packed.exe"),
            enigma::tests::packed_game(7, &[("Data/BasicData.wolf", b"wolf")]),
        )
        .unwrap();
        let detection = folder.detect_engine(root(&directory)).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::WolfRpg);
        assert!(!folder.can_translate(root(&directory), &detection));
    }

    /// On a copy of a game packed into one executable (`DLA_PACKED_GAME`,
    /// the folder the copy is in): unpacks it and adds the plugin, and with
    /// `DLA_PACKED_REMOVE` takes both out again.
    #[test]
    #[ignore = "needs a copy of a game packed into one executable"]
    fn smoke_test_on_a_packed_game_copy() {
        let game = std::env::var("DLA_PACKED_GAME").unwrap();
        let files = || {
            WalkDir::new(&game)
                .into_iter()
                .filter_map(Result::ok)
                .count()
        };
        let before = files();
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(&game).unwrap().unwrap();
        println!(
            "{:?} in {:?}, can translate: {}",
            detection.engine,
            detection.root.as_ref().map(RelativePath::as_str),
            folder.can_translate(&game, &detection)
        );
        let started = std::time::Instant::now();
        let changes = folder.install(&game, &detection, &[]).unwrap();
        let unpacked = changes
            .iter()
            .filter(|change| matches!(change.kind, GameFileChangeKind::Unpacked { .. }))
            .count();
        println!(
            "{unpacked} files unpacked of {} changes in {:.1}s, {} files and folders where {before} were",
            changes.len(),
            started.elapsed().as_secs_f64(),
            files()
        );
        let program = changes
            .iter()
            .find_map(GameFileChange::unpacked_program)
            .unwrap();
        println!(
            "starts {} in place of {}",
            program.path.as_str(),
            program.in_place_of.as_str()
        );
        let started = std::time::Instant::now();
        assert_eq!(
            folder.install(&game, &detection, &changes).unwrap(),
            changes
        );
        println!("a launch takes {:.2}s", started.elapsed().as_secs_f64());
        if std::env::var_os("DLA_PACKED_REMOVE").is_some() {
            let started = std::time::Instant::now();
            folder.remove(&game, &changes).unwrap();
            println!(
                "removed in {:.1}s: {} files and folders",
                started.elapsed().as_secs_f64(),
                files()
            );
            assert_eq!(files(), before);
        }
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
                        speakers: vec![dla_application::translation::MessageSpeakerOverride {
                            map: 1,
                            event: 2,
                            page: 0,
                            command: 4,
                            key: dla_application::translation::text_key("はい"),
                            expected_speaker: dla_application::translation::text_key("ユキナ"),
                            text: "Other Speaker".into(),
                        }],
                        pictures: vec![dla_application::translation::TranslatedPicture {
                            path: "pictures/Namecut02".to_owned(),
                            png: b"\x89PNG\r\n\x1a\nEnglish".to_vec(),
                        }],
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
        assert_eq!(copy["speakers"][0]["text"], "Other Speaker");
        assert_eq!(copy["speakers"][0]["command"], 4);
        assert_eq!(
            fs::read(data.join("img/pictures/Namecut02.png")).unwrap(),
            b"\x89PNG\r\n\x1a\nEnglish"
        );
    }

    #[test]
    fn a_games_english_images_follow_what_the_launcher_has() {
        use dla_application::translation::TranslatedPicture;
        let directory = tempdir().unwrap();
        let folder = directory.path().join("dla-translation/img");
        let picture = |path: &str, mark: &[u8]| TranslatedPicture {
            path: path.to_owned(),
            png: [b"\x89PNG\r\n\x1a\n".as_slice(), mark].concat(),
        };
        let first = [
            picture("pictures/Namecut02", b"one"),
            picture("pictures/名前/Mapname01", b"two"),
            picture("titles1/Title", b"three"),
        ];
        write_pictures(&folder, &first).unwrap();
        assert_eq!(
            fs::read(folder.join("pictures/名前/Mapname01.png")).unwrap(),
            first[1].png
        );

        // A changed image is written again, one the launcher no longer has
        // is taken away with its folder, and the rest is left alone.
        let kept = folder.join("titles1/Title.png");
        let written = fs::metadata(&kept).unwrap().modified().unwrap();
        let second = [picture("pictures/Namecut02", b"changed"), first[2].clone()];
        write_pictures(&folder, &second).unwrap();
        assert_eq!(
            fs::read(folder.join("pictures/Namecut02.png")).unwrap(),
            second[0].png
        );
        assert!(!folder.join("pictures/名前").exists());
        assert_eq!(fs::metadata(&kept).unwrap().modified().unwrap(), written);

        assert!(write_pictures(&folder, &[picture("../outside", b"no")]).is_err());
        assert!(
            !directory
                .path()
                .join("dla-translation/outside.png")
                .exists()
        );

        write_pictures(&folder, &[]).unwrap();
        assert!(!folder.exists());
    }

    #[test]
    #[ignore = "needs DLA_TRANSLATION_PACKS, DLA_TRANSLATION_GAME, DLA_TRANSLATION_WORK and DLA_TRANSLATION_VERSION"]
    fn a_published_speaker_pack_covers_the_game_and_round_trips_through_a_fresh_cache() {
        use dla_application::translation::{PackLookup, TranslationPacks, TranslationService};
        use dla_sqlite::SqliteTranslationStore;
        use dla_translator::{ArchivePacks, FolderFiles};
        use std::sync::Arc;

        let env = |name| std::env::var(name).unwrap();
        let game = env("DLA_TRANSLATION_GAME");
        let work = env("DLA_TRANSLATION_WORK");
        let version = env("DLA_TRANSLATION_VERSION");
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(&game).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::RpgMakerMz);
        let mut translation = translation(&detection, Vec::new());
        let sources = folder.read_texts(&game, &translation).unwrap();
        let packs = ArchivePacks::new(Arc::new(FolderFiles::new(PathBuf::from(env(
            "DLA_TRANSLATION_PACKS",
        )))));
        let PackLookup::Offer(offer) = packs.find(&work, Some(&version)).unwrap() else {
            panic!("no pack offered")
        };
        let pack = packs.download(&offer).unwrap();
        let temporary = tempdir().unwrap();
        let store = Arc::new(
            SqliteTranslationStore::open(&temporary.path().join("translations.sqlite")).unwrap(),
        );
        let service = TranslationService::new(store);
        assert_eq!(
            service
                .import_pack(&pack, &sources.texts, &sources.names)
                .unwrap() as usize,
            sources.texts.len()
        );
        let all = service.all(&work).unwrap();
        assert_eq!(all.texts.len(), pack.texts.len());
        assert_eq!(all.names.len(), pack.names.len());
        assert_eq!(all.speakers, pack.speakers);
        assert!(!all.speakers.is_empty());
        for name in &sources.names {
            assert!(
                all.names.iter().any(|(source, _)| source == name),
                "untranslated speaker: {name}"
            );
        }
        translation.game_folder = None;
        folder
            .write_translations(root(&temporary), &translation, &all)
            .unwrap();
        let copy: Value = serde_json::from_slice(
            &fs::read(temporary.path().join("dla-translation/translations.json")).unwrap(),
        )
        .unwrap();
        for (source, english) in &all.texts {
            assert_eq!(copy["texts"][source], *english);
        }
        for (source, english) in &all.names {
            assert_eq!(copy["names"][source], *english);
        }
        assert_eq!(
            copy["speakers"],
            serde_json::to_value(&pack.speakers).unwrap()
        );
        println!(
            "Verified {} texts, {} names and {} message speaker overrides through pack download, a fresh cache and a fresh game copy",
            all.texts.len(),
            all.names.len(),
            all.speakers.len()
        );
    }

    #[test]
    #[ignore = "needs DLA_TRANSLATION_PACKS, DLA_TRANSLATION_GAME, DLA_TRANSLATION_WORK and DLA_TRANSLATION_VERSION"]
    fn a_published_pack_with_images_covers_the_game_and_round_trips_through_a_fresh_cache() {
        use dla_application::translation::{PackLookup, TranslationPacks, TranslationService};
        use dla_sqlite::SqliteTranslationStore;
        use dla_translator::{ArchivePacks, FolderFiles};
        use std::sync::Arc;

        let env = |name| std::env::var(name).unwrap();
        let game = env("DLA_TRANSLATION_GAME");
        let work = env("DLA_TRANSLATION_WORK");
        let version = env("DLA_TRANSLATION_VERSION");
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(&game).unwrap().unwrap();
        let translation = translation(&detection, Vec::new());
        let sources = folder.read_texts(&game, &translation).unwrap();
        let packs = ArchivePacks::new(Arc::new(FolderFiles::new(PathBuf::from(env(
            "DLA_TRANSLATION_PACKS",
        )))));
        let PackLookup::Offer(offer) = packs.find(&work, Some(&version)).unwrap() else {
            panic!("no pack offered")
        };
        let pack = packs.download(&offer).unwrap();
        assert!(!pack.pictures.is_empty());
        for picture in &pack.pictures {
            assert!(
                folder
                    .has_image(&game, &translation, &picture.path)
                    .unwrap(),
                "the game has no image {}",
                picture.path
            );
        }
        let temporary = tempdir().unwrap();
        let store = Arc::new(
            SqliteTranslationStore::open(&temporary.path().join("translations.sqlite")).unwrap(),
        );
        let service = TranslationService::new(store);
        assert_eq!(
            service
                .import_pack(&pack, &sources.texts, &sources.names)
                .unwrap() as usize,
            sources.texts.len()
        );
        let all = service.all(&work).unwrap();
        assert_eq!(all.texts.len(), pack.texts.len());
        assert_eq!(all.names.len(), pack.names.len());
        assert_eq!(all.pictures, pack.pictures);
        println!(
            "Verified {} texts, {} names and {} images through pack download and a fresh cache",
            all.texts.len(),
            all.names.len(),
            all.pictures.len()
        );
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

    /// An RPG Maker XP game in an archive, in a `Game` folder.
    fn xp_game() -> TempDir {
        let directory = tempdir().unwrap();
        let game = directory.path().join("Game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("Game.exe"), "exe").unwrap();
        fs::write(
            game.join("Game.ini"),
            "[Game]\r\nLibrary=RGSS104E.dll\r\nScripts=Data\\Scripts.rxdata\r\n",
        )
        .unwrap();
        fs::write(game.join("RGSS104E.dll"), "dll").unwrap();
        let system = rgss_system(&[("@words", "攻撃")]);
        let scripts = rgss::scripts_file(
            &[(1, "Main", "$scene = Scene_Title.new\n")],
            ruby_marshal::Strings::Plain,
        );
        fs::write(
            game.join("Game.rgssad"),
            rgssad::build(&[
                ("Data\\System.rxdata", &system),
                ("Data\\Scripts.rxdata", &scripts),
            ]),
        )
        .unwrap();
        directory
    }

    /// An RPG Maker VX game without an archive, at the top of the folder.
    fn vx_game() -> TempDir {
        let directory = tempdir().unwrap();
        let data = directory.path().join("Data");
        fs::create_dir_all(&data).unwrap();
        fs::write(directory.path().join("Game.exe"), "exe").unwrap();
        fs::write(
            data.join("System.rvdata"),
            rgss_system(&[("@terms", "戦う")]),
        )
        .unwrap();
        fs::write(
            data.join("Scripts.rvdata"),
            rgss::scripts_file(
                &[(1, "Main", "Scene_Title.new.main\n")],
                ruby_marshal::Strings::Plain,
            ),
        )
        .unwrap();
        directory
    }

    /// A System file whose menu words object has one field, `@attack`.
    fn rgss_system(words: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = ruby_marshal::Writer::new();
        writer.object("RPG::System", words.len());
        for (field, attack) in words {
            writer.symbol(field);
            writer.object("RPG::System::Words", 1);
            writer.symbol("@attack");
            writer.binary(attack.as_bytes());
        }
        writer.finish()
    }

    fn ruby_hash<'a>(value: &'a ruby_marshal::Value, key: &str) -> &'a ruby_marshal::Value {
        let ruby_marshal::Value::Hash(pairs) = value else {
            panic!("a hash");
        };
        pairs
            .iter()
            .find(|(name, _)| name.as_str() == Some(key))
            .map(|(_, value)| value)
            .unwrap()
    }

    #[test]
    fn patches_an_xp_archive_and_restores_it_without_a_trace() {
        let game = xp_game();
        let before = snapshot(&game);
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::RpgMakerXp);
        assert_eq!(detection.root.as_ref().unwrap().as_str(), "Game");
        assert_eq!(
            folder.plugin(GameEngine::RpgMakerXp),
            folder.plugin(GameEngine::RpgMakerVx)
        );
        assert_ne!(
            folder.plugin(GameEngine::RpgMakerXp),
            folder.plugin(GameEngine::RpgMakerVxAce)
        );

        let changes = folder.install(root(&game), &detection, &[]).unwrap();
        let paths = changes
            .iter()
            .map(|change| {
                (
                    change.path.as_str(),
                    matches!(change.kind, GameFileChangeKind::Patched { .. }),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            [("Game/Game.rgssad", true), ("Game/dla-translation", false)]
        );
        assert_eq!(
            folder.install(root(&game), &detection, &changes).unwrap(),
            changes
        );
        let translation = translation(&detection, changes.clone());
        let texts = folder.read_texts(root(&game), &translation).unwrap();
        assert_eq!(texts.texts[0].text, "攻撃");
        assert!(folder.reads_translations_from_folder(GameEngine::RpgMakerXp));

        folder
            .write_session(
                root(&game),
                &translation,
                &GameSession {
                    work: "RJ2".to_owned(),
                    shown: true,
                    relay: None,
                    translations: WorkTranslations {
                        speakers: Vec::new(),
                        pictures: Vec::new(),
                        texts: vec![("攻撃".to_owned(), "Attack".to_owned())],
                        names: Vec::new(),
                    },
                },
            )
            .unwrap();
        let data = game.path().join("Game/dla-translation");
        let file =
            ruby_marshal::read(&fs::read(data.join("translations.marshal")).unwrap()).unwrap();
        assert_eq!(
            ruby_hash(ruby_hash(&file, "texts"), "攻撃").as_str(),
            Some("Attack")
        );
        fs::write(data.join("place.txt"), "map:3").unwrap();
        assert_eq!(
            folder.current_place(root(&game), &translation).as_deref(),
            Some("map:3")
        );

        folder.remove(root(&game), &changes).unwrap();
        assert_eq!(snapshot(&game), before);
    }

    #[test]
    fn replaces_the_scripts_file_of_a_vx_game_without_an_archive() {
        let game = vx_game();
        let before = snapshot(&game);
        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&game)).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::RpgMakerVx);
        assert_eq!(detection.root, None);

        let changes = folder.install(root(&game), &detection, &[]).unwrap();
        let paths = changes
            .iter()
            .map(|change| {
                (
                    change.path.as_str(),
                    matches!(change.kind, GameFileChangeKind::Changed { .. }),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            [("Data/Scripts.rvdata", true), ("dla-translation", false)]
        );
        let scripts =
            ruby_marshal::read(&fs::read(game.path().join("Data/Scripts.rvdata")).unwrap())
                .unwrap();
        assert_eq!(scripts.items().len(), 2);
        assert_eq!(
            scripts.items()[0].items()[0].as_integer(),
            Some(rgss::OUR_SECTION)
        );
        let texts = folder
            .read_texts(root(&game), &translation(&detection, changes.clone()))
            .unwrap();
        assert_eq!(texts.texts[0].text, "戦う");

        folder.remove(root(&game), &changes).unwrap();
        assert_eq!(snapshot(&game), before);
    }

    #[test]
    fn translates_the_ruby_files_a_game_loads_through_copies_of_them() {
        let directory = tempdir().unwrap();
        let game = directory.path();
        fs::create_dir_all(game.join("Data")).unwrap();
        fs::create_dir_all(game.join("System/talk/01_Witch")).unwrap();
        fs::write(game.join("Game.exe"), "exe").unwrap();
        fs::write(
            game.join("Game.ini"),
            "[Game]\r\nLibrary=RGSS104J.dll\r\nScripts=Data\\Scripts.rxdata\r\n",
        )
        .unwrap();
        fs::write(game.join("RGSS104J.dll"), "dll").unwrap();
        fs::write(
            game.join("Data/System.rxdata"),
            rgss_system(&[("@words", "攻撃")]),
        )
        .unwrap();
        fs::write(
            game.join("Data/Scripts.rxdata"),
            rgss::scripts_file(
                &[(
                    1,
                    "Scene_Title",
                    "Dir.glob(\"System/talk/*/*.rb\").sort.each { |name| load(name) }\n",
                )],
                ruby_marshal::Strings::Plain,
            ),
        )
        .unwrap();
        let talk = concat!(
            "tx[\"tx1\"] = \"「#{target}、またね」\" if $msg.tag == \"逃走\"\r\n",
            "@target = \"お兄さん\"\r\n",
        );
        let file = "System/talk/01_Witch/Talk.rb";
        fs::write(game.join(file), talk).unwrap();
        let before = snapshot(&directory);

        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&directory)).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::RpgMakerXp);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        let translation = translation(&detection, changes.clone());
        let texts = folder.read_texts(root(&directory), &translation).unwrap();
        assert_eq!(
            texts
                .texts
                .iter()
                .map(|text| text.text.as_str())
                .collect::<Vec<_>>(),
            ["攻撃", "逃走", "「#{target}、またね」", "お兄さん"]
        );

        let session = GameSession {
            work: "RJ000001".to_owned(),
            shown: true,
            relay: None,
            translations: WorkTranslations {
                speakers: Vec::new(),
                pictures: Vec::new(),
                texts: vec![
                    (
                        "「#{target}、またね」".to_owned(),
                        "\"See you, #{target}\"".to_owned(),
                    ),
                    ("お兄さん".to_owned(), "Mister".to_owned()),
                    ("逃走".to_owned(), "Escape".to_owned()),
                ],
                names: Vec::new(),
            },
        };
        // While translations arrive the game is not starting: no copies.
        folder
            .write_translations(root(&directory), &translation, &session.translations)
            .unwrap();
        let copy = game.join("dla-translation/scripts").join(file);
        assert!(!copy.exists());
        folder
            .write_session(root(&directory), &translation, &session)
            .unwrap();
        assert_eq!(
            fs::read_to_string(&copy).unwrap(),
            concat!(
                "tx[\"tx1\"] = \"\\\"See you, #{target}\\\"\" if $msg.tag == \"逃走\"\r\n",
                "@target = \"Mister\"\r\n",
            )
        );
        assert_eq!(fs::read_to_string(game.join(file)).unwrap(), talk);
        // What the file compares stays Japanese in the data too.
        let written = ruby_marshal::read(
            &fs::read(game.join("dla-translation/translations.marshal")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            ruby_hash(&written, "kept"),
            &ruby_marshal::Value::Array(vec![ruby_marshal::Value::String(
                "逃走".as_bytes().to_vec()
            )])
        );

        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn translates_the_sentences_a_games_scripts_put_together() {
        let directory = tempdir().unwrap();
        let game = directory.path();
        fs::create_dir_all(game.join("Data")).unwrap();
        fs::write(game.join("Game.exe"), "exe").unwrap();
        fs::write(
            game.join("Game.ini"),
            "[Game]\r\nLibrary=RGSS104J.dll\r\nScripts=Data\\Scripts.rxdata\r\n",
        )
        .unwrap();
        fs::write(game.join("RGSS104J.dll"), "dll").unwrap();
        fs::write(
            game.join("Data/System.rxdata"),
            rgss_system(&[("@words", "攻撃")]),
        )
        .unwrap();
        let party = concat!(
            "def gain(id)\r\n",
            "  text = \"#{$data_items[id].name}　を手に入れた！\"\r\n",
            "  wait if c == \"\\w\"\r\n",
            "  heavy = self.name == \"インプ\"\r\n",
            "end\r\n",
        );
        fs::write(
            game.join("Data/Scripts.rxdata"),
            rgss::scripts_file(
                &[
                    (1, "Main", "$scene = Scene_Title.new\n"),
                    (2, "Game_Party", party),
                ],
                ruby_marshal::Strings::Plain,
            ),
        )
        .unwrap();
        let before = snapshot(&directory);

        let folder = DesktopGameFolder::new();
        let detection = folder.detect_engine(root(&directory)).unwrap().unwrap();
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        let translation = translation(&detection, changes.clone());
        let texts = folder.read_texts(root(&directory), &translation).unwrap();
        let sentence = "#{$data_items[id].name}　を手に入れた！";
        assert_eq!(
            texts
                .texts
                .iter()
                .map(|text| text.text.as_str())
                .collect::<Vec<_>>(),
            ["攻撃", "　を手に入れた！", "インプ", sentence]
        );

        let session = GameSession {
            work: "RJ000001".to_owned(),
            shown: true,
            relay: None,
            translations: WorkTranslations {
                speakers: Vec::new(),
                pictures: Vec::new(),
                texts: vec![
                    ("攻撃".to_owned(), "Blow".to_owned()),
                    (
                        sentence.to_owned(),
                        "#{$data_items[id].name} was found!".to_owned(),
                    ),
                ],
                names: Vec::new(),
            },
        };
        folder
            .write_translations(root(&directory), &translation, &session.translations)
            .unwrap();
        let sections = game.join("dla-translation/sections");
        assert!(!sections.exists());
        folder
            .write_session(root(&directory), &translation, &session)
            .unwrap();
        // The game's second section, its `w` a code: the letter in a word
        // is what stands for it.
        assert_eq!(
            fs::read_to_string(sections.join("0001.rb")).unwrap(),
            party.replace(sentence, "#{$data_items[id].name} \u{f877}as found!")
        );
        assert!(
            fs::read_to_string(sections.join("list.txt"))
                .unwrap()
                .starts_with("1 ")
        );
        let written = ruby_marshal::read(
            &fs::read(game.join("dla-translation/translations.marshal")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            ruby_hash(ruby_hash(&written, "texts"), "攻撃"),
            &ruby_marshal::Value::String("Blo\u{f877}".as_bytes().to_vec())
        );

        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
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
