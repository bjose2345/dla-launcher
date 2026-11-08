//! The player's choice of translator, kept in the launcher's data folder.
//! The DeepL key stays there too; it is never written into a game folder.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use dla_application::translation::Translator;
use dla_domain::translation::TranslatorSettings;
use serde::{Deserialize, Serialize};

use crate::{DeepLTranslator, LocalModelTranslator};

#[derive(Clone, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTranslator {
    pub translator: Option<TranslatorSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deepl_key: Option<String>,
    /// Translate games ahead of play only while no game runs.
    #[serde(default)]
    pub prepare_only_when_idle: bool,
}

impl fmt::Debug for SavedTranslator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SavedTranslator")
            .field("translator", &self.translator)
            .field("deepl_key", &self.deepl_key.as_ref().map(|_| "(saved)"))
            .field("prepare_only_when_idle", &self.prepare_only_when_idle)
            .finish()
    }
}

impl SavedTranslator {
    /// The translator these settings describe, when they are complete.
    pub fn translator(&self) -> Option<Arc<dyn Translator>> {
        match self.translator.as_ref()? {
            TranslatorSettings::LocalModel { address, model } => {
                (!address.trim().is_empty()).then(|| {
                    Arc::new(LocalModelTranslator::new(address, model)) as Arc<dyn Translator>
                })
            }
            TranslatorSettings::DeepL { free_plan } => self
                .deepl_key
                .as_deref()
                .filter(|key| !key.trim().is_empty())
                .map(|key| Arc::new(DeepLTranslator::new(key, *free_plan)) as Arc<dyn Translator>),
        }
    }
}

pub struct TranslatorSettingsFile {
    path: PathBuf,
}

impl TranslatorSettingsFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn read(&self) -> io::Result<SavedTranslator> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(SavedTranslator::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, saved: &SavedTranslator) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut temporary = self.path.as_os_str().to_owned();
        temporary.push(".new");
        let temporary = PathBuf::from(temporary);
        write_private(
            &temporary,
            &serde_json::to_vec_pretty(saved).map_err(io::Error::other)?,
        )?;
        fs::rename(&temporary, &self.path)
    }
}

// Only the player's own account may read a file holding an API key.
#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_choice_and_the_key_between_starts() {
        let folder = tempfile::tempdir().unwrap();
        let file = TranslatorSettingsFile::new(folder.path().join("preferences/translation.json"));
        assert_eq!(file.read().unwrap(), SavedTranslator::default());
        let saved = SavedTranslator {
            translator: Some(TranslatorSettings::DeepL { free_plan: true }),
            deepl_key: Some("key:fx".to_owned()),
            prepare_only_when_idle: true,
        };
        file.save(&saved).unwrap();
        assert_eq!(file.read().unwrap(), saved);
        assert_eq!(saved.translator().unwrap().name(), "DeepL");
        assert!(!format!("{saved:?}").contains("key:fx"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(folder.path().join("preferences/translation.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn has_no_translator_until_the_settings_are_complete() {
        assert!(SavedTranslator::default().translator().is_none());
        let no_key = SavedTranslator {
            translator: Some(TranslatorSettings::DeepL { free_plan: false }),
            deepl_key: None,
            prepare_only_when_idle: false,
        };
        assert!(no_key.translator().is_none());
        let local = SavedTranslator {
            translator: Some(TranslatorSettings::LocalModel {
                address: "http://127.0.0.1:11434/v1".to_owned(),
                model: "qwen3:4b-instruct".to_owned(),
            }),
            deepl_key: None,
            prepare_only_when_idle: false,
        };
        assert_eq!(local.translator().unwrap().name(), "qwen3:4b-instruct");
    }
}
