use serde::{Deserialize, Serialize};

use crate::{
    game_engine::GameEngine,
    installation::{InstallationId, RelativePath},
};

/// The language games are translated into. Others come later as more values.
pub const TRANSLATION_LANGUAGE: &str = "en";

/// What a piece of game text is. Dialogue is translated scene by scene with
/// its neighbours; menu words and database entries as short terms.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextKind {
    Dialogue,
    Choice,
    Menu,
    Database,
    Addon,
}

impl TextKind {
    pub fn is_dialogue(self) -> bool {
        matches!(self, Self::Dialogue | Self::Choice)
    }
}

/// The game engine's own codes inside its text, which must reach the player
/// unchanged.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMarkup {
    RpgMaker,
    /// WOLF RPG Editor: `\c[2]`, `\cself[8]`, `\f[\cself[19]]`, `\^`, `<C>`.
    Wolf,
}

impl TextMarkup {
    pub fn of(engine: GameEngine) -> Self {
        match engine {
            GameEngine::WolfRpg => Self::Wolf,
            _ => Self::RpgMaker,
        }
    }
}

/// Where a translation came from. When a text has several, the player's own
/// edit wins, then reviewed packs, then machine-made packs, then the
/// player's translator.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationOrigin {
    Edit,
    PackReviewed,
    PackMachine,
    Local,
    /// The player's translator, from before the player asked to translate
    /// the game again: still shown, but translated again as if missing.
    Stale,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum TranslatorSettings {
    /// Ollama, LM Studio, llama.cpp or any other server with the
    /// OpenAI-compatible chat API, usually on the player's own PC.
    LocalModel { address: String, model: String },
    /// The player's DeepL API key is kept apart from these settings.
    #[serde(rename = "deepl")]
    DeepL { free_plan: bool },
}

/// A game translated with the launcher's plugin, and everything the launcher
/// changed in its folder for that, so removing the translation puts every
/// file back as it was.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameTranslation {
    pub installation_id: InstallationId,
    pub engine: GameEngine,
    /// The engine's own folder inside the installation (for RPG Maker MV,
    /// the `www` folder); `None` when that is the installation's folder.
    pub game_folder: Option<RelativePath>,
    /// Off plays the game in Japanese and keeps the translation.
    pub shown: bool,
    /// A fingerprint of the plugin files written, so a newer plugin from the
    /// launcher replaces them.
    pub plugin: String,
    pub changes: Vec<GameFileChange>,
    pub turned_on_at: String,
    /// The player paused translating the game ahead of play.
    pub paused: bool,
    /// The last time the whole game was translated ahead of play.
    pub prepared: Option<PreparedTranslation>,
    /// The translation from our GitHub in use, if one was downloaded.
    pub pack: Option<InstalledPack>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstalledPack {
    /// When the pack was made; a newer pack replaces it.
    pub made: String,
    pub lines: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedTranslation {
    pub at: String,
    /// The translator's model or service.
    pub with: String,
    pub texts: u64,
    /// Texts the translator gave nothing usable for; they are tried again
    /// the next time the game is prepared.
    pub missing: u64,
}

/// A file or folder of a game the launcher added or changed. Paths are
/// relative to the folder the installation is played from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameFileChange {
    pub path: RelativePath,
    pub kind: GameFileChangeKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameFileChangeKind {
    /// Added by the launcher; a folder includes everything inside it.
    Added,
    /// A game file the launcher changed, with its original content.
    Changed {
        original: Vec<u8>,
        written_sha256: String,
        written_bytes: u64,
    },
    /// A game file too large to keep a copy of (an RPG Maker XP, VX or VX
    /// Ace archive), changed in place: `original` held the bytes at `at`,
    /// and the file grew from `original_bytes` to `written_bytes`. The hash
    /// covers the bytes written at `at` and everything after
    /// `original_bytes`.
    Patched {
        at: u64,
        original: Vec<u8>,
        original_bytes: u64,
        written_sha256: String,
        written_bytes: u64,
    },
    /// A game file or folder the launcher moved to `to`, untouched, to put
    /// its own in its place (a WOLF RPG Editor data archive or folder):
    /// whatever is at the path now is the launcher's.
    Moved { to: RelativePath },
    /// A file of the game the launcher took out of `from`, an executable
    /// that holds the game's files packed inside it, and wrote where the
    /// game reads it from. The program among them starts in place of
    /// `from`: its `starts` is the SHA-256 of the list of files `from` held.
    Unpacked {
        from: RelativePath,
        sha256: String,
        bytes: u64,
        starts: Option<String>,
    },
}

/// The program that starts in place of the executable a game was unpacked
/// from, which reads only the files packed inside it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnpackedProgram {
    pub in_place_of: RelativePath,
    pub path: RelativePath,
    pub sha256: String,
}

impl GameFileChange {
    /// Whether `path` is this added or unpacked file, or lies inside this
    /// added folder.
    pub fn covers(&self, path: &str) -> bool {
        match self.kind {
            GameFileChangeKind::Added => inside(path, &self.path).is_some(),
            GameFileChangeKind::Unpacked { .. } => path == self.path.as_str(),
            _ => false,
        }
    }

    /// This file as the program that starts in place of the executable it
    /// was unpacked from.
    pub fn unpacked_program(&self) -> Option<UnpackedProgram> {
        match &self.kind {
            GameFileChangeKind::Unpacked {
                from,
                sha256,
                starts: Some(_),
                ..
            } => Some(UnpackedProgram {
                in_place_of: from.clone(),
                path: self.path.clone(),
                sha256: sha256.clone(),
            }),
            _ => None,
        }
    }

    /// Where the game's own file `path` is now, when it is this moved file
    /// or lies inside this moved folder.
    pub fn moved(&self, path: &str) -> Option<RelativePath> {
        let GameFileChangeKind::Moved { to } = &self.kind else {
            return None;
        };
        let rest = inside(path, &self.path)?;
        RelativePath::parse(format!("{}{rest}", to.as_str())).ok()
    }
}

/// What follows `folder` in `path`: nothing for the path itself, `/…` for
/// one inside it.
fn inside<'a>(path: &'a str, folder: &RelativePath) -> Option<&'a str> {
    path.strip_prefix(folder.as_str())
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_added_folder_covers_what_is_inside_it_only() {
        let folder = GameFileChange {
            path: RelativePath::parse("www/dla-translation").unwrap(),
            kind: GameFileChangeKind::Added,
        };
        assert!(folder.covers("www/dla-translation"));
        assert!(folder.covers("www/dla-translation/log.txt"));
        assert!(!folder.covers("www/dla-translation-old/log.txt"));
        assert!(!folder.covers("www/js/plugins.js"));
        let changed = GameFileChange {
            path: RelativePath::parse("www/js/plugins.js").unwrap(),
            kind: GameFileChangeKind::Changed {
                original: Vec::new(),
                written_sha256: String::new(),
                written_bytes: 0,
            },
        };
        assert!(!changed.covers("www/js/plugins.js"));
    }

    #[test]
    fn a_moved_file_or_folder_says_where_the_games_own_files_are() {
        let moved = |path: &str, to: &str| GameFileChange {
            path: RelativePath::parse(path).unwrap(),
            kind: GameFileChangeKind::Moved {
                to: RelativePath::parse(to).unwrap(),
            },
        };
        let archive = moved(
            "sys/Data/BasicData.wolf",
            "sys/Data/BasicData.wolf.dla-original",
        );
        assert_eq!(
            archive.moved("sys/Data/BasicData.wolf").unwrap().as_str(),
            "sys/Data/BasicData.wolf.dla-original"
        );
        assert_eq!(archive.moved("sys/Data/BasicData.wolf.bak"), None);
        assert!(!archive.covers("sys/Data/BasicData.wolf"));
        let folder = moved("Data/MapData", "Data/MapData.dla-original");
        assert_eq!(
            folder.moved("Data/MapData/Map001.mps").unwrap().as_str(),
            "Data/MapData.dla-original/Map001.mps"
        );
        assert_eq!(folder.moved("Data/MapDataOld/Map001.mps"), None);
    }

    #[test]
    fn an_unpacked_file_covers_itself_and_the_program_says_what_it_replaces() {
        let unpacked = |path: &str, starts: Option<&str>| GameFileChange {
            path: RelativePath::parse(path).unwrap(),
            kind: GameFileChangeKind::Unpacked {
                from: RelativePath::parse("game/Packed.exe").unwrap(),
                sha256: "ab".repeat(32),
                bytes: 7,
                starts: starts.map(str::to_owned),
            },
        };
        let data = unpacked("game/data/System.json", None);
        assert!(data.covers("game/data/System.json"));
        assert!(!data.covers("game/data/System.json/x"));
        assert!(!data.covers("game/data/Map001.json"));
        assert_eq!(data.unpacked_program(), None);
        let program = unpacked("game/Game.exe", Some("list"))
            .unpacked_program()
            .unwrap();
        assert_eq!(program.in_place_of.as_str(), "game/Packed.exe");
        assert_eq!(program.path.as_str(), "game/Game.exe");
        assert_eq!(program.sha256, "ab".repeat(32));
    }

    #[test]
    fn translator_settings_use_the_ui_field_names() {
        let settings = TranslatorSettings::DeepL { free_plan: true };
        assert_eq!(
            serde_json::to_value(&settings).unwrap(),
            serde_json::json!({ "kind": "deepl", "freePlan": true })
        );
        let local: TranslatorSettings = serde_json::from_value(serde_json::json!({
            "kind": "localModel", "address": "http://127.0.0.1:11434/v1", "model": "qwen3:4b"
        }))
        .unwrap();
        assert!(matches!(local, TranslatorSettings::LocalModel { .. }));
    }
}
