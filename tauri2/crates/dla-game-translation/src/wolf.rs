//! WOLF RPG Editor: reading a game's texts and writing their English into
//! its data. The engine has no script layer, so nothing is added to the
//! game: its two data folders are written again in English ([`install`])
//! from the game's own, which stay as they were. The data files of
//! versions 2 and 3 are read; of the archives a game packs them into, those
//! the editor wrote up to 2.24, so a game of version 3 is read when it
//! ships its folders unpacked. Newer archives and protected data are
//! refused.

mod commands;
mod common_events;
mod database;
mod dxa;
mod game_dat;
mod install;
mod map;
mod stream;
mod text;
mod texts;
mod usage;
mod wrap;

use std::{collections::BTreeMap, io, path::Path};

use dla_application::game_translation::{GameTexts, GameTranslationError};
use sha2::{Digest, Sha256};

use install::Originals;
pub use install::{Moved, restore};
use texts::{GameData, Translations};

/// What the launcher writes into a game depends on these rules alone; a
/// change to them writes every translated game again at its next start.
const RULES: &str = "wolf-2";
const OTHER_VERSION: &str =
    "this version of WOLF RPG Editor (its archives from 2.25 on, and protected data, are not read)";

pub fn fingerprint() -> String {
    hex::encode(&Sha256::digest(RULES.as_bytes())[..8])
}

/// Moves the game's data aside and writes the folders the engine reads
/// instead; `launcher_folder` is the launcher's own folder in the game.
pub fn install(game: &Path, launcher_folder: &Path) -> Result<Vec<Moved>, GameTranslationError> {
    let install = || {
        // Nothing moves for a game whose data the launcher cannot read.
        if !install::is_applied(game)? {
            read(game)?;
        }
        install::install(game, launcher_folder)
    };
    install().map_err(|error| refusal(error, "could not be prepared"))
}

/// Whether the game's data is of a kind the launcher reads, told from its
/// archives' headers and the first bytes of its title file alone.
pub fn is_readable(game: &Path) -> bool {
    Originals::find(game).is_ok_and(|originals| originals.is_plain())
}

pub fn read_texts(game: &Path) -> Result<GameTexts, GameTranslationError> {
    read(game)
        .map(|mut data| data.texts())
        .map_err(|error| refusal(error, "could not be read"))
}

/// Writes the game's data with the English of every text that has one, or
/// as the game had it when the translation is not shown.
pub fn write_translations(
    game: &Path,
    launcher_folder: &Path,
    texts: &BTreeMap<String, String>,
    names: &BTreeMap<String, String>,
    shown: bool,
) -> Result<(), GameTranslationError> {
    let write = || {
        let mut data = read(game)?;
        if shown {
            data.translate(&Translations { texts, names });
        }
        install::write(game, launcher_folder, &data.files())
    };
    write().map_err(|error| refusal(error, "could not be written"))
}

fn read(game: &Path) -> io::Result<GameData> {
    GameData::read(&Originals::find(game)?)
}

/// A game the launcher does not read is refused as one it cannot
/// translate yet, not reported as a damaged folder.
fn refusal(error: io::Error, what: &str) -> GameTranslationError {
    if error.kind() != io::ErrorKind::Unsupported {
        return GameTranslationError::folder(format!("the game's data {what}: {error}"));
    }
    GameTranslationError::Unsupported(OTHER_VERSION)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf};

    use dla_application::{
        game_translation::{GameFolder, GameSession},
        translation::WorkTranslations,
    };
    use dla_domain::{
        game_engine::GameEngine,
        installation::InstallationId,
        translation::{GameTranslation, TextKind},
    };

    use dla_application::game_translation::GameTranslationError;
    use dla_domain::{
        game_engine::GameEngineDetection,
        translation::{GameFileChange, GameFileChangeKind},
    };
    use sha2::{Digest, Sha256};
    use tempfile::{TempDir, tempdir};
    use walkdir::WalkDir;

    use super::{
        commands::testing::message,
        common_events::{CommonEvents, testing::common_events},
        dxa,
        game_dat::GameDat,
        install::Originals,
        map::testing::map,
        text::decode,
        texts::{
            BASIC_DATA, Files, GameData, MAP_DATA,
            testing::{Memory, game, game_with_speakers, newer_game},
        },
    };
    use crate::{DesktopGameFolder, rgss::find_case_insensitive};

    const KEY: dxa::Key = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

    /// The small game of the text tests in a `Game` folder, its two data
    /// folders as archives or as folders, with an archive of pictures.
    fn game_on_disk(archives: bool) -> TempDir {
        on_disk(&game(), archives)
    }

    fn on_disk(files: &Memory, archives: bool) -> TempDir {
        let directory = tempdir().unwrap();
        let data = directory.path().join("Game/Data");
        fs::create_dir_all(&data).unwrap();
        fs::write(directory.path().join("Game/Game.exe"), "exe").unwrap();
        fs::write(data.join("Picture.wolf"), "pictures").unwrap();
        for folder in [BASIC_DATA, MAP_DATA] {
            let inside = files
                .0
                .iter()
                .filter(|((in_folder, _), _)| in_folder == folder)
                .map(|((_, name), bytes)| (name.as_str(), bytes.as_slice()))
                .collect::<Vec<_>>();
            if archives {
                fs::write(
                    data.join(format!("{folder}.wolf")),
                    dxa::build(KEY, &inside),
                )
                .unwrap();
            } else {
                fs::create_dir_all(data.join(folder)).unwrap();
                for (name, bytes) in inside {
                    fs::write(data.join(folder).join(name), bytes).unwrap();
                }
            }
        }
        directory
    }

    /// What the same game holds when all its data is in one archive: the
    /// two folders with text, pictures and sound in folders of their own,
    /// and a file outside every folder.
    fn files_of_one_archive() -> Vec<(String, Vec<u8>)> {
        let mut files = game()
            .0
            .iter()
            .map(|((folder, name), bytes)| (format!("{folder}/{name}"), bytes.clone()))
            .collect::<Vec<_>>();
        for (name, bytes) in [
            ("Picture/title.png", "a title"),
            ("Picture/Faces/主人公.png", "a face"),
            ("BGM/town.ogg", "a tune"),
            ("key.csv", "a,b"),
        ] {
            files.push((name.to_owned(), bytes.as_bytes().to_vec()));
        }
        files
    }

    fn one_archive(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let files = files
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        dxa::build(KEY, &files)
    }

    /// The game with all its data in `Data.wolf` beside its exe, and no
    /// `Data` folder.
    fn game_in_one_archive() -> TempDir {
        let directory = tempdir().unwrap();
        let game = directory.path().join("Game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("Game.exe"), "exe").unwrap();
        fs::write(game.join("Data.wolf"), one_archive(&files_of_one_archive())).unwrap();
        directory
    }

    /// The files of one of the archives the launcher wrote, by their names.
    fn files_in_archive(path: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
        let archive = dxa::Archive::open(path).unwrap();
        assert_eq!(archive.key(), KEY);
        archive
            .entries()
            .iter()
            .map(|entry| (entry.name.clone(), archive.read(path, entry).unwrap()))
            .collect()
    }

    fn snapshot(directory: &TempDir) -> BTreeMap<String, Vec<u8>> {
        WalkDir::new(directory.path())
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| {
                let name = entry.path().strip_prefix(directory.path()).unwrap();
                (
                    name.to_str()
                        .unwrap()
                        .replace(std::path::MAIN_SEPARATOR, "/"),
                    fs::read(entry.path()).unwrap(),
                )
            })
            .collect()
    }

    fn root(directory: &TempDir) -> &str {
        directory.path().to_str().unwrap()
    }

    fn detect(directory: &TempDir) -> GameEngineDetection {
        let detection = DesktopGameFolder
            .detect_engine(root(directory))
            .unwrap()
            .unwrap();
        assert_eq!(detection.engine, GameEngine::WolfRpg);
        assert_eq!(detection.root.as_ref().unwrap().as_str(), "Game");
        detection
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
            turned_on_at: "2026-11-10T10:00:00Z".to_owned(),
            paused: false,
            prepared: None,
            pack: None,
        }
    }

    fn session(texts: &[(&str, &str)]) -> GameSession {
        GameSession {
            work: "RJ1".to_owned(),
            shown: true,
            relay: None,
            translations: WorkTranslations {
                speakers: Vec::new(),
                pictures: Vec::new(),
                texts: texts
                    .iter()
                    .map(|(japanese, english)| ((*japanese).to_owned(), (*english).to_owned()))
                    .collect(),
                names: Vec::new(),
            },
        }
    }

    fn kinds(changes: &[GameFileChange]) -> Vec<(&str, String)> {
        changes
            .iter()
            .map(|change| {
                let kind = match &change.kind {
                    GameFileChangeKind::Added => "added".to_owned(),
                    GameFileChangeKind::Moved { to } => format!("moved to {}", to.as_str()),
                    _ => "other".to_owned(),
                };
                (change.path.as_str(), kind)
            })
            .collect()
    }

    fn first_message(directory: &TempDir) -> String {
        let bytes = fs::read(directory.path().join("Game/Data/BasicData/CommonEvent.dat")).unwrap();
        let events = CommonEvents::parse(&bytes).unwrap();
        decode(&events.events[0].commands[0].strings[0]).unwrap()
    }

    const JAPANESE: &str = "@1\nこんにちは、\n旅の人。";
    const ENGLISH: [(&str, &str); 2] = [
        ("サキュバスの館", "The Succubus Mansion"),
        ("こんにちは、\n旅の人。", "Hello there, traveller."),
    ];

    #[test]
    fn moves_the_archives_aside_and_puts_them_back_without_a_trace() {
        let directory = game_on_disk(true);
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        assert_eq!(
            kinds(&changes),
            [
                ("Game/Data/BasicData", "added".to_owned()),
                (
                    "Game/Data/BasicData.wolf",
                    "moved to Game/Data/BasicData.wolf.dla-original".to_owned()
                ),
                ("Game/Data/MapData", "added".to_owned()),
                (
                    "Game/Data/MapData.wolf",
                    "moved to Game/Data/MapData.wolf.dla-original".to_owned()
                ),
                ("Game/dla-translation", "added".to_owned()),
            ]
        );
        // The game's own archives are whole under their other names, and
        // the folders hold every file as the game had it.
        let applied = snapshot(&directory);
        for archive in ["BasicData.wolf", "MapData.wolf"] {
            assert!(!applied.contains_key(&format!("Game/Data/{archive}")));
            assert_eq!(
                applied[&format!("Game/Data/{archive}.dla-original")],
                before[&format!("Game/Data/{archive}")]
            );
        }
        for ((in_folder, name), bytes) in &game().0 {
            assert_eq!(&applied[&format!("Game/Data/{in_folder}/{name}")], bytes);
        }
        assert_eq!(applied["Game/Data/Picture.wolf"], b"pictures");
        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );

        let translation = translation(&detection, changes.clone());
        let texts = folder.read_texts(root(&directory), &translation).unwrap();
        assert_eq!(texts.texts[0].text, "薬草");
        assert!(folder.reads_translations_from_folder(GameEngine::WolfRpg));
        assert_eq!(folder.current_place(root(&directory), &translation), None);

        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();
        assert_eq!(first_message(&directory), "@1\nHello there, traveller.");
        // The title is the game's name for its saves and stays.
        let data = directory.path().join("Game/Data");
        assert_eq!(
            fs::read(data.join("BasicData/Game.dat")).unwrap(),
            game().get(BASIC_DATA, "Game.dat")
        );
        // The texts are still read from the game's own data.
        assert_eq!(
            folder.read_texts(root(&directory), &translation).unwrap(),
            texts
        );

        // Hidden, the game reads its own Japanese again.
        let mut hidden = translation.clone();
        hidden.shown = false;
        folder
            .write_translations(root(&directory), &hidden, &session(&ENGLISH).translations)
            .unwrap();
        assert_eq!(first_message(&directory), JAPANESE);

        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn translates_a_game_that_ships_folders_and_keeps_what_an_update_wrote() {
        let directory = game_on_disk(false);
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        assert_eq!(
            kinds(&changes),
            [
                (
                    "Game/Data/BasicData",
                    "moved to Game/Data/BasicData.dla-original".to_owned()
                ),
                (
                    "Game/Data/MapData",
                    "moved to Game/Data/MapData.dla-original".to_owned()
                ),
                ("Game/dla-translation", "added".to_owned()),
            ]
        );
        let translation = translation(&detection, changes.clone());
        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();
        assert_eq!(first_message(&directory), "@1\nHello there, traveller.");
        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );
        assert_eq!(first_message(&directory), "@1\nHello there, traveller.");

        // A game update writes a new map and a new file into the folders
        // the engine reads, which are the launcher's now.
        let data = directory.path().join("Game/Data");
        let updated = map(vec![vec![message("新しい町へようこそ。")]]).write();
        fs::write(data.join("MapData/Town.mps"), &updated).unwrap();
        fs::write(data.join("BasicData/icon001.png"), "new").unwrap();
        folder
            .install(root(&directory), &detection, &changes)
            .unwrap();
        let texts = folder.read_texts(root(&directory), &translation).unwrap();
        assert!(
            texts
                .texts
                .iter()
                .any(|text| text.text == "新しい町へようこそ。")
        );
        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();
        assert_eq!(first_message(&directory), "@1\nHello there, traveller.");

        folder.remove(root(&directory), &changes).unwrap();
        let mut expected = before;
        expected.insert("Game/Data/MapData/Town.mps".to_owned(), updated);
        expected.insert(
            "Game/Data/BasicData/icon001.png".to_owned(),
            b"new".to_vec(),
        );
        assert_eq!(snapshot(&directory), expected);
    }

    #[test]
    fn translates_a_game_of_version_3_5_that_ships_folders_and_puts_it_back() {
        let directory = on_disk(&newer_game(), false);
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        assert!(folder.can_translate(root(&directory), &detection));
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        let translation = translation(&detection, changes.clone());
        let texts = folder.read_texts(root(&directory), &translation).unwrap();
        assert_eq!(texts, GameData::read(&game()).unwrap().texts());
        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();

        let data = directory.path().join("Game/Data");
        let bytes = fs::read(data.join("BasicData/CommonEvent.dat")).unwrap();
        assert_eq!(bytes[..11], *b"\0W\0\0OLUFC\0\x93");
        let events = CommonEvents::parse(&bytes).unwrap();
        assert_eq!(
            events.events[0].commands[0].strings[0],
            b"@1\nHello there, traveller."
        );
        // The files without text are there as the game had them.
        assert_eq!(
            fs::read(data.join("BasicData/Game.dat")).unwrap(),
            before["Game/Data/BasicData/Game.dat"]
        );
        assert_eq!(
            folder.read_texts(root(&directory), &translation).unwrap(),
            texts
        );

        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn writes_the_names_of_speakers_into_the_system_databases_names_and_puts_them_back() {
        for archives in [true, false] {
            let directory = on_disk(&game_with_speakers(), archives);
            let before = snapshot(&directory);
            let folder = DesktopGameFolder::new();
            let detection = detect(&directory);
            let changes = folder.install(root(&directory), &detection, &[]).unwrap();
            let translation = translation(&detection, changes.clone());
            folder
                .write_session(
                    root(&directory),
                    &translation,
                    &session(&[("男Ａ", "Man A")]),
                )
                .unwrap();
            let names = fs::read(
                directory
                    .path()
                    .join("Game/Data/BasicData/SysDatabase.project"),
            )
            .unwrap();
            let has = |word: &str| {
                let word = super::text::encode(word);
                names.windows(word.len()).any(|bytes| bytes == word)
            };
            assert!(has("Man A") && !has("男Ａ") && has("受付"), "{archives}");
            // Shown again as the game had it.
            let mut hidden = translation.clone();
            hidden.shown = false;
            folder
                .write_session(root(&directory), &hidden, &session(&[("男Ａ", "Man A")]))
                .unwrap();
            assert_eq!(
                fs::read(
                    directory
                        .path()
                        .join("Game/Data/BasicData/SysDatabase.project")
                )
                .unwrap(),
                game_with_speakers().get(BASIC_DATA, "SysDatabase.project")
            );
            folder.remove(root(&directory), &changes).unwrap();
            assert_eq!(snapshot(&directory), before, "{archives}");
        }
    }

    #[test]
    fn never_takes_its_own_english_for_the_games_original() {
        let directory = game_on_disk(false);
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        let translation = translation(&detection, changes.clone());
        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();
        let other = [("こんにちは、\n旅の人。", "Good day, traveller.")];
        folder
            .write_session(root(&directory), &translation, &session(&other))
            .unwrap();
        assert_eq!(first_message(&directory), "@1\nGood day, traveller.");

        // The record knows what the file holds and what it held before.
        let record_file = directory.path().join("Game/dla-translation/wolf.json");
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(&record_file).unwrap()).unwrap();
        let hashes = record["written"]["BasicData/CommonEvent.dat"]
            .as_array()
            .unwrap();
        assert_eq!(hashes.len(), 2);
        assert_eq!(
            hashes[1],
            hex::encode(Sha256::digest(
                fs::read(directory.path().join("Game/Data/BasicData/CommonEvent.dat")).unwrap()
            ))
        );

        // Without the record, the English in the folder is not an update.
        fs::remove_file(&record_file).unwrap();
        folder
            .install(root(&directory), &detection, &changes)
            .unwrap();
        let original = &before["Game/Data/BasicData/CommonEvent.dat"];
        assert_eq!(
            &fs::read(
                directory
                    .path()
                    .join("Game/Data/BasicData.dla-original/CommonEvent.dat")
            )
            .unwrap(),
            original
        );
        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn finishes_what_a_stop_left_halfway() {
        let directory = game_on_disk(true);
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        let data = directory.path().join("Game/Data");
        let aside = |name: &str| data.join(format!("{name}.wolf.dla-original"));
        let archive = |name: &str| data.join(format!("{name}.wolf"));

        // Stopped before the archive moved: the folder is written, the
        // archive still in its place.
        fs::rename(aside("BasicData"), archive("BasicData")).unwrap();
        fs::write(data.join("BasicData/Game.dat"), "half").unwrap();
        fs::create_dir_all(data.join("MapData.dla-new")).unwrap();
        fs::write(data.join("MapData.dla-new/Town.mps"), "half").unwrap();
        fs::remove_dir_all(data.join("MapData")).unwrap();
        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );
        assert!(!archive("BasicData").exists());
        assert_eq!(first_message(&directory), JAPANESE);
        assert!(data.join("MapData/Town.mps").is_file());

        // Stopped while removing: the archives are back, a folder is left.
        fs::rename(aside("BasicData"), archive("BasicData")).unwrap();
        fs::rename(aside("MapData"), archive("MapData")).unwrap();
        fs::remove_dir_all(data.join("MapData")).unwrap();
        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn takes_the_archive_a_game_update_put_back() {
        let directory = game_on_disk(true);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        let translation = translation(&detection, changes.clone());
        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();

        let mut updated = game();
        updated.put(
            BASIC_DATA,
            "CommonEvent.dat",
            common_events(vec![vec![message("更新された。")]]).write(),
        );
        let inside = updated
            .0
            .iter()
            .filter(|((in_folder, _), _)| in_folder == BASIC_DATA)
            .map(|((_, name), bytes)| (name.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        let archive = dxa::build(KEY, &inside);
        let data = directory.path().join("Game/Data");
        fs::write(data.join("BasicData.wolf"), &archive).unwrap();

        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );
        assert_eq!(
            fs::read(data.join("BasicData.wolf.dla-original")).unwrap(),
            archive
        );
        assert_eq!(first_message(&directory), "更新された。");

        // Put back again by an update, then removed: the newer one stays.
        fs::write(data.join("BasicData.wolf"), &archive).unwrap();
        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(fs::read(data.join("BasicData.wolf")).unwrap(), archive);
        assert!(!data.join("BasicData.wolf.dla-original").exists());
        assert!(!data.join("BasicData").exists());
        assert!(data.join("MapData.wolf").is_file());
    }

    #[test]
    fn writes_a_game_in_one_archive_the_way_the_engine_reads_it_and_puts_it_back() {
        let directory = game_in_one_archive();
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        assert!(folder.can_translate(root(&directory), &detection));
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        // The game had no `Data` folder: all of it is the launcher's.
        assert_eq!(
            kinds(&changes),
            [
                ("Game/Data", "added".to_owned()),
                (
                    "Game/Data.wolf",
                    "moved to Game/Data.wolf.dla-original".to_owned()
                ),
                ("Game/dla-translation", "added".to_owned()),
            ]
        );
        let applied = snapshot(&directory);
        let mut expected = game()
            .0
            .keys()
            .map(|(in_folder, name)| format!("Game/Data/{in_folder}/{name}"))
            .collect::<Vec<_>>();
        expected.extend(
            [
                "Game/Data.wolf.dla-original",
                "Game/Data/BGM.wolf",
                "Game/Data/Picture.wolf",
                "Game/Data/key.csv",
                "Game/Game.exe",
                "Game/dla-translation/wolf.json",
            ]
            .map(str::to_owned),
        );
        expected.sort();
        assert_eq!(applied.keys().cloned().collect::<Vec<_>>(), expected);
        assert_eq!(
            applied["Game/Data.wolf.dla-original"],
            before["Game/Data.wolf"]
        );
        for ((in_folder, name), bytes) in &game().0 {
            assert_eq!(&applied[&format!("Game/Data/{in_folder}/{name}")], bytes);
        }
        assert_eq!(applied["Game/Data/key.csv"], b"a,b");
        // Every other folder is an archive of its own with the same key.
        let data = directory.path().join("Game/Data");
        assert_eq!(
            files_in_archive(&data.join("Picture.wolf")),
            BTreeMap::from([
                ("title.png".to_owned(), b"a title".to_vec()),
                ("Faces/主人公.png".to_owned(), b"a face".to_vec()),
            ])
        );
        assert_eq!(
            files_in_archive(&data.join("BGM.wolf")),
            BTreeMap::from([("town.ogg".to_owned(), b"a tune".to_vec())])
        );
        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );
        assert_eq!(snapshot(&directory), applied);

        let translation = translation(&detection, changes.clone());
        let texts = folder.read_texts(root(&directory), &translation).unwrap();
        assert_eq!(texts.texts[0].text, "薬草");
        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();
        assert_eq!(first_message(&directory), "@1\nHello there, traveller.");
        assert_eq!(
            folder.read_texts(root(&directory), &translation).unwrap(),
            texts
        );

        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
        assert!(!data.exists());
    }

    #[test]
    fn keeps_what_the_game_has_in_its_data_folder_beside_the_one_archive() {
        let directory = game_in_one_archive();
        let data = directory.path().join("Game/Data");
        // Fonts are read by Windows and lie in the folder; a file may be
        // there as the archive has it; a folder there is one the engine
        // never read.
        fs::create_dir_all(data.join("Picture")).unwrap();
        fs::write(data.join("font.ttf"), "a font").unwrap();
        fs::write(data.join("key.csv"), "a,b").unwrap();
        fs::write(data.join("Picture/old.png"), "old").unwrap();
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        assert_eq!(
            kinds(&changes),
            [
                (
                    "Game/Data.wolf",
                    "moved to Game/Data.wolf.dla-original".to_owned()
                ),
                ("Game/Data/BGM.wolf", "added".to_owned()),
                ("Game/Data/BasicData", "added".to_owned()),
                ("Game/Data/MapData", "added".to_owned()),
                ("Game/Data/Picture.wolf", "added".to_owned()),
                ("Game/dla-translation", "added".to_owned()),
            ]
        );
        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );
        let translation = translation(&detection, changes.clone());
        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();
        assert_eq!(first_message(&directory), "@1\nHello there, traveller.");

        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn finishes_a_game_in_one_archive_that_a_stop_left_halfway() {
        let directory = game_in_one_archive();
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        let applied = snapshot(&directory);
        let game = directory.path().join("Game");
        let data = game.join("Data");
        let (archive, aside) = (game.join("Data.wolf"), game.join("Data.wolf.dla-original"));

        // Stopped before the archive moved: some of it written, one
        // archive half built, the game's own still in its place.
        fs::rename(&aside, &archive).unwrap();
        fs::remove_file(data.join("Picture.wolf")).unwrap();
        fs::remove_dir_all(data.join("MapData")).unwrap();
        fs::write(data.join("BGM.wolf.dla-new"), "half").unwrap();
        fs::write(data.join("BGM.wolf"), "half").unwrap();
        fs::write(data.join("BasicData/Game.dat"), "half").unwrap();
        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );
        assert_eq!(snapshot(&directory), applied);

        // The archive aside and a part gone: only that part is written.
        fs::remove_file(data.join("Picture.wolf")).unwrap();
        fs::remove_dir_all(data.join("MapData")).unwrap();
        let translation = translation(&detection, changes.clone());
        folder
            .write_translations(
                root(&directory),
                &translation,
                &session(&ENGLISH).translations,
            )
            .unwrap_err();
        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );
        assert_eq!(snapshot(&directory), applied);

        // Stopped while removing: the archive is back, the rest is left.
        fs::rename(&aside, &archive).unwrap();
        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
        assert!(!data.exists());
        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn leaves_nothing_in_the_data_folder_when_a_part_cannot_be_written() {
        let directory = game_in_one_archive();
        let data = directory.path().join("Game/Data");
        // Something in the way of the archive the launcher builds.
        fs::create_dir_all(data.join("Picture.wolf.dla-new")).unwrap();
        fs::write(data.join("font.ttf"), "a font").unwrap();
        let before = snapshot(&directory);
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        assert!(matches!(
            folder.install(root(&directory), &detection, &[]),
            Err(GameTranslationError::Folder(_))
        ));
        let mut left = snapshot(&directory);
        assert!(left.remove("Game/dla-translation/wolf.json").is_some());
        assert_eq!(left, before);
        assert!(!data.join("BasicData").exists());

        // With the way clear, the same game is written whole.
        fs::remove_dir(data.join("Picture.wolf.dla-new")).unwrap();
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        assert!(data.join("Picture.wolf").is_file());
        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(snapshot(&directory), before);
    }

    #[test]
    fn takes_the_one_archive_a_game_update_put_back() {
        let directory = game_in_one_archive();
        let folder = DesktopGameFolder::new();
        let detection = detect(&directory);
        let changes = folder.install(root(&directory), &detection, &[]).unwrap();
        let translation = translation(&detection, changes.clone());
        folder
            .write_session(root(&directory), &translation, &session(&ENGLISH))
            .unwrap();

        // The update has a new event, new voices and another key file,
        // and no music any more.
        let mut updated = files_of_one_archive();
        updated.retain(|(name, _)| !name.starts_with("BGM/"));
        for (name, bytes) in &mut updated {
            match name.as_str() {
                "BasicData/CommonEvent.dat" => {
                    *bytes = common_events(vec![vec![message("更新された。")]]).write();
                }
                "key.csv" => *bytes = b"c,d".to_vec(),
                _ => {}
            }
        }
        updated.push(("Voice/hello.ogg".to_owned(), b"a voice".to_vec()));
        let archive = one_archive(&updated);
        let game = directory.path().join("Game");
        fs::write(game.join("Data.wolf"), &archive).unwrap();

        assert_eq!(
            folder
                .install(root(&directory), &detection, &changes)
                .unwrap(),
            changes
        );
        assert!(!game.join("Data.wolf").exists());
        assert_eq!(
            fs::read(game.join("Data.wolf.dla-original")).unwrap(),
            archive
        );
        assert_eq!(first_message(&directory), "更新された。");
        assert!(!game.join("Data/BGM.wolf").exists());
        assert_eq!(
            files_in_archive(&game.join("Data/Voice.wolf")),
            BTreeMap::from([("hello.ogg".to_owned(), b"a voice".to_vec())])
        );
        assert_eq!(fs::read(game.join("Data/key.csv")).unwrap(), b"c,d");

        // Put back again by an update, then removed: the newer one stays.
        fs::write(game.join("Data.wolf"), &archive).unwrap();
        folder.remove(root(&directory), &changes).unwrap();
        assert_eq!(
            snapshot(&directory),
            BTreeMap::from([
                ("Game/Data.wolf".to_owned(), archive),
                ("Game/Game.exe".to_owned(), b"exe".to_vec()),
            ])
        );
        assert!(!game.join("Data").exists());
    }

    #[test]
    fn refuses_games_it_does_not_read_and_changes_nothing() {
        let folder = DesktopGameFolder::new();
        let readable = game_on_disk(false);
        assert!(folder.can_translate(root(&readable), &detect(&readable)));
        let refused = |directory: &TempDir, reason: &str| {
            let before = snapshot(directory);
            let detection = detect(directory);
            assert!(!folder.can_translate(root(directory), &detection));
            let error = folder
                .install(root(directory), &detection, &[])
                .unwrap_err();
            match error {
                GameTranslationError::Unsupported(what) => assert_eq!(what, reason),
                other => panic!("{other}"),
            }
            assert_eq!(snapshot(directory), before);
        };

        // A newer editor's archive (DXA version 8).
        let newer = game_on_disk(true);
        let mut archive = vec![0; 96];
        archive[..4].copy_from_slice(b"DX\x08\x00");
        fs::write(newer.path().join("Game/Data/MapData.wolf"), archive).unwrap();
        refused(&newer, super::OTHER_VERSION);

        // Protected data files in a readable archive.
        let protected = game_on_disk(true);
        let mut files = game();
        files.put(BASIC_DATA, "Game.dat", vec![0x5a; 80]);
        files.put(BASIC_DATA, "CommonEvent.dat", vec![0x5a; 80]);
        let inside = files
            .0
            .iter()
            .filter(|((in_folder, _), _)| in_folder == BASIC_DATA)
            .map(|((_, name), bytes)| (name.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        fs::write(
            protected.path().join("Game/Data/BasicData.wolf"),
            dxa::build(KEY, &inside),
        )
        .unwrap();
        refused(&protected, super::OTHER_VERSION);

        // Everything in one archive of a newer editor, and one whose
        // data files are protected.
        let newer = game_in_one_archive();
        let mut archive = vec![0; 96];
        archive[..4].copy_from_slice(b"DX\x08\x00");
        fs::write(newer.path().join("Game/Data.wolf"), archive).unwrap();
        refused(&newer, super::OTHER_VERSION);
        let protected = game_in_one_archive();
        let mut files = files_of_one_archive();
        for (name, bytes) in &mut files {
            if name == "BasicData/Game.dat" || name == "BasicData/CommonEvent.dat" {
                *bytes = vec![0x5a; 80];
            }
        }
        fs::write(protected.path().join("Game/Data.wolf"), one_archive(&files)).unwrap();
        refused(&protected, super::OTHER_VERSION);

        // A folder beside the archive that the launcher did not write.
        let not_the_launchers = |directory: &TempDir| {
            let before = snapshot(directory);
            let detection = detect(directory);
            assert!(matches!(
                folder.install(root(directory), &detection, &[]),
                Err(GameTranslationError::Folder(_))
            ));
            assert_eq!(snapshot(directory), before);
        };
        let both = game_on_disk(true);
        fs::create_dir_all(both.path().join("Game/Data/BasicData")).unwrap();
        fs::write(both.path().join("Game/Data/BasicData/Game.dat"), "theirs").unwrap();
        not_the_launchers(&both);

        // Beside the one archive: a data folder, an archive or a file of
        // the game's own that the engine would read in the launcher's
        // stead once the archive moved.
        for (name, bytes) in [
            ("BasicData/Game.dat", "theirs"),
            ("MapData.wolf", "theirs"),
            ("Picture.wolf", "theirs"),
            ("key.csv", "another"),
        ] {
            let both = game_in_one_archive();
            let path = both.path().join("Game/Data").join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
            not_the_launchers(&both);
        }
    }

    /// On a copy of a WOLF RPG Editor game (`DLA_WOLF_GAME`, the folder an
    /// installation is played from): checks that every file with text is
    /// written back with the data it had, reads its texts and applies
    /// made-up English twice. With `DLA_WOLF_TITLE` it also writes that
    /// title into the launcher's folder, so the game's window says whether
    /// it loads the launcher's data. `DLA_WOLF_LIST` names a file to list
    /// the texts in. Run again with `DLA_WOLF_REMOVE=1` to put the game
    /// back.
    #[test]
    #[ignore = "needs a copy of a WOLF RPG Editor game"]
    fn smoke_test_on_a_game_copy() {
        let root = PathBuf::from(std::env::var("DLA_WOLF_GAME").unwrap());
        let root_text = root.to_str().unwrap();
        let folder = crate::DesktopGameFolder::new();
        let detection = folder.detect_engine(root_text).unwrap().unwrap();
        assert_eq!(detection.engine, GameEngine::WolfRpg);
        let game = match &detection.root {
            Some(game) => root.join(game.as_str()),
            None => root.clone(),
        };
        if std::env::var_os("DLA_WOLF_REMOVE").is_some() {
            let changes = folder.install(root_text, &detection, &[]).unwrap();
            folder.remove(root_text, &changes).unwrap();
            eprintln!("removed {changes:?}");
            return;
        }

        let originals = Originals::find(&game).unwrap();
        let mut data = GameData::read(&originals).unwrap();
        // A packed file is compared unpacked: the editor's packer and the
        // launcher's write the same data as other bytes.
        let unpacked = |name: &str, bytes: Vec<u8>| {
            let head = match name.to_ascii_lowercase() {
                name if name.ends_with(".mps") && bytes[20] >= 0x65 => 25,
                name if name.ends_with(".dat") && [0x93, 0xc4].contains(&bytes[10]) => 11,
                _ => return bytes,
            };
            let mut all = bytes[..head].to_vec();
            all.extend(super::stream::unpack(&bytes[head..]).unwrap());
            all
        };
        for file in data.files() {
            assert_eq!(
                unpacked(&file.name, file.bytes),
                unpacked(&file.name, originals.read(file.folder, &file.name).unwrap()),
                "{}/{} is not written back as it was",
                file.folder,
                file.name
            );
        }
        let texts = data.texts();
        let mut kinds = BTreeMap::<String, (usize, usize)>::new();
        for text in &texts.texts {
            let count = kinds.entry(format!("{:?}", text.kind)).or_default();
            count.0 += 1;
            count.1 += text.text.chars().count();
        }
        eprintln!(
            "{} files written back exactly; room {:?}; texts {} {kinds:?}; names {}; texts with a place {}",
            data.files().len(),
            data.room(),
            texts.texts.len(),
            texts.names.len(),
            texts.places.len()
        );
        if let Some(list) = std::env::var_os("DLA_WOLF_LIST") {
            let lines = texts
                .texts
                .iter()
                .map(|text| format!("{:?}\t{}", text.kind, text.text.replace('\n', "\\n")))
                .collect::<Vec<_>>();
            fs::write(list, lines.join("\n")).unwrap();
        }

        let started = std::time::Instant::now();
        let changes = folder.install(root_text, &detection, &[]).unwrap();
        eprintln!("moved aside and written in {:?}", started.elapsed());
        assert_eq!(
            folder.install(root_text, &detection, &changes).unwrap(),
            changes
        );
        eprintln!("changes {changes:?}");
        let translations = WorkTranslations {
            speakers: Vec::new(),
            pictures: Vec::new(),
            texts: texts
                .texts
                .iter()
                .enumerate()
                .map(|(at, text)| {
                    let english = match text.kind {
                        TextKind::Dialogue => format!(
                            "EN{at} the quick brown fox jumps over the lazy dog and keeps running for a good while longer"
                        ),
                        _ => format!("EN{at} text"),
                    };
                    (text.text.clone(), english)
                })
                .collect(),
            names: Vec::new(),
        };
        let translation = GameTranslation {
            installation_id: InstallationId("smoke".to_owned()),
            engine: detection.engine,
            game_folder: detection.root.clone(),
            shown: true,
            plugin: folder.plugin(detection.engine).unwrap(),
            changes,
            turned_on_at: String::new(),
            paused: false,
            prepared: None,
            pack: None,
        };
        let session = GameSession {
            work: "smoke".to_owned(),
            shown: true,
            relay: None,
            translations,
        };
        folder
            .write_session(root_text, &translation, &session)
            .unwrap();
        // What was written reads as the same game with English in it.
        let after = folder.read_texts(root_text, &translation).unwrap();
        assert_eq!(after, texts);
        // The launcher leaves the title alone; the check writes one into
        // the launcher's folder to see in the game's window which data the
        // engine loads. The engine then takes the saves for another game's.
        if let Ok(title) = std::env::var("DLA_WOLF_TITLE") {
            let path = find_case_insensitive(&game, "Data/BasicData/Game.dat")
                .unwrap()
                .unwrap();
            let mut game_dat = GameDat::parse(&fs::read(&path).unwrap()).unwrap();
            game_dat.set_title(game_dat.encoding.encode(&title));
            fs::write(path, game_dat.write()).unwrap();
        }
    }
}
