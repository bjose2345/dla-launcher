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
    /// A game file too large to keep a copy of (an RPG Maker VX Ace
    /// archive), changed in place: `original` held the bytes at `at`, and the
    /// file grew from `original_bytes` to `written_bytes`. The hash covers
    /// the bytes written at `at` and everything after `original_bytes`.
    Patched {
        at: u64,
        original: Vec<u8>,
        original_bytes: u64,
        written_sha256: String,
        written_bytes: u64,
    },
}

impl GameFileChange {
    /// Whether `path` is this added file or lies inside this added folder.
    pub fn covers(&self, path: &str) -> bool {
        matches!(self.kind, GameFileChangeKind::Added)
            && path
                .strip_prefix(self.path.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }
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
