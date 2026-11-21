//! RPG Maker XP (RGSS1) and VX (RGSS2), both on Ruby 1.8: the launcher's
//! script for them, and a check on a copy of a real game. Putting the
//! script into a game is `rgss::install`.

pub const SCRIPT: &str = include_str!("../plugins/rpg-maker-xp-vx/DLA_Translate.rb");
/// One name for both: the script tells the engines apart itself.
pub const TRANSLATIONS: &str = "translations.marshal";

#[cfg(test)]
mod tests {
    use std::fs;

    use dla_application::{
        game_translation::{GameFolder, GameSession},
        translation::{PreparedText, WorkTranslations},
    };
    use dla_domain::{
        installation::InstallationId,
        translation::{GameTranslation, TextMarkup},
    };
    use sha2::{Digest, Sha256};

    use crate::{
        rgss::{Generation, archive_in},
        rgssad,
    };

    /// On a copy of an XP or VX game (`DLA_XPVX_GAME`): reads its texts,
    /// patches its archive (or scripts file) twice and hands it made-up
    /// English, so running the copy shows whether the script hooks in. With
    /// `DLA_XPVX_ENGLISH`, a game's `translations.json`, its lines are used
    /// for the texts it has. Run again with `DLA_XPVX_REMOVE=1` to take it
    /// all off.
    #[test]
    #[ignore = "needs a copy of an RPG Maker XP or VX game"]
    fn smoke_test_on_a_game_copy() {
        let game = std::path::PathBuf::from(std::env::var("DLA_XPVX_GAME").unwrap());
        let root = game.to_str().unwrap();
        let folder = crate::DesktopGameFolder::new();
        let detection = folder.detect_engine(root).unwrap().unwrap();
        let generation = Generation::of(detection.engine).unwrap();
        assert!(matches!(generation, Generation::Xp | Generation::Vx));
        let archive = archive_in(&game, generation).unwrap();
        if std::env::var_os("DLA_XPVX_REMOVE").is_some() {
            let changes = folder.install(root, &detection, &[]).unwrap();
            folder.remove(root, &changes).unwrap();
            if let Some(archive) = archive {
                assert_eq!(rgssad::find_patch(&archive).unwrap(), None);
                eprintln!(
                    "removed; archive sha256 {}",
                    hex::encode(Sha256::digest(fs::read(&archive).unwrap()))
                );
            }
            return;
        }
        let texts = crate::xp_vx_data::read(&game, generation).unwrap();
        let dialogue = texts
            .texts
            .iter()
            .filter(|text| text.kind.is_dialogue())
            .count();
        eprintln!(
            "{}: texts {} (dialogue {dialogue}), names {}, texts with a place {}",
            detection.engine.label(),
            texts.texts.len(),
            texts.names.len(),
            texts.places.len()
        );
        let changes = folder.install(root, &detection, &[]).unwrap();
        assert_eq!(folder.install(root, &detection, &changes).unwrap(), changes);
        eprintln!("changes {changes:?}");

        let scripts = crate::rgss::strings_in_scripts(&game, generation).unwrap();
        eprintln!(
            "sentences in the scripts {}, letters drawn as codes {:?}, names kept for their parts {}",
            scripts.shown.texts().count(),
            scripts.shown.letters(),
            texts
                .texts
                .iter()
                .filter(|text| !scripts.strings.contains(&text.text) && scripts.keeps(&text.text))
                .count()
        );
        // English words where the Japanese was, every code and value kept
        // in its place, as a translator's reply has them.
        let fake = |at: usize, japanese: &str| {
            let prepared = PreparedText::new(japanese, TextMarkup::RpgMaker);
            let mut reply = format!("EN{at}");
            let mut rest = prepared.line.as_str();
            while !rest.is_empty() {
                let code = rest
                    .strip_prefix('{')
                    .and_then(|after| after.split_once('}'))
                    .filter(|(number, _)| {
                        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                    });
                match code {
                    Some((number, after)) => {
                        reply.push_str(&format!("{{{number}}}"));
                        rest = after;
                    }
                    None => {
                        let end = rest
                            .char_indices()
                            .skip(1)
                            .find(|(_, character)| *character == '{')
                            .map_or(rest.len(), |(at, _)| at);
                        reply.push_str(" why my own words");
                        rest = &rest[end..];
                    }
                }
            }
            prepared.finish(&reply)
        };
        let real = std::env::var_os("DLA_XPVX_ENGLISH")
            .map(|path| {
                serde_json::from_slice::<serde_json::Value>(&fs::read(path).unwrap()).unwrap()
            })
            .unwrap_or_default();
        let real = |table: &str, japanese: &str| real[table][japanese].as_str().map(str::to_owned);
        let translations = WorkTranslations {
            speakers: Vec::new(),
            pictures: Vec::new(),
            texts: texts
                .texts
                .iter()
                .enumerate()
                .map(|(at, text)| {
                    let english = real("texts", &text.text).unwrap_or_else(|| fake(at, &text.text));
                    (text.text.clone(), english)
                })
                .collect(),
            names: texts
                .names
                .iter()
                .enumerate()
                .map(|(at, name)| {
                    let english = real("names", name).unwrap_or_else(|| format!("Name{at}"));
                    (name.clone(), english)
                })
                .collect(),
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
        folder.write_session(root, &translation, &session).unwrap();
    }
}
