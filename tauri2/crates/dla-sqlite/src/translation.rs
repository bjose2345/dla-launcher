use std::{collections::HashMap, path::Path, sync::Mutex};

use dla_application::translation::{
    Glossary, NewTranslation, StoredTranslation, TranslationError, TranslationStore,
};
use dla_domain::translation::{TRANSLATION_LANGUAGE, TextKind, TranslationOrigin};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use rusqlite_migration::{M, Migrations};

use crate::database;

const TRANSLATION_MIGRATION_LIST: &[M<'static>] = &[
    M::up(include_str!(
        "../migrations/translation/0001_create_translations.sql"
    )),
    M::up(include_str!(
        "../migrations/translation/0002_add_glossaries.sql"
    )),
];
const TRANSLATION_MIGRATIONS: Migrations<'static> =
    Migrations::from_slice(TRANSLATION_MIGRATION_LIST);
// SQLite's default limit on `?` parameters is 32766; two go to the work and
// the language.
const KEYS_PER_QUERY: usize = 500;

/// The player's translation cache, `translations.sqlite`, kept apart from the
/// library so it can be backed up, moved or reset on its own.
pub struct SqliteTranslationStore {
    connection: Mutex<Connection>,
}

impl SqliteTranslationStore {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Ok(Self {
            connection: Mutex::new(database::open(path, &TRANSLATION_MIGRATIONS)?),
        })
    }

    /// The keys of the work's lines the player's translator made with
    /// `model` and still shows: for marking them stale, so a better model
    /// translates them again.
    pub fn keys_made_with(&self, work: &str, model: &str) -> Result<Vec<String>, TranslationError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT key FROM translation
                 WHERE work = ?1 AND language = ?2 AND origin = 'local' AND model = ?3
                 ORDER BY key",
            )?;
            statement
                .query_map(params![work, TRANSLATION_LANGUAGE, model], |row| row.get(0))?
                .collect()
        })
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> rusqlite::Result<T>,
    ) -> Result<T, TranslationError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|error| TranslationError::persistence(error.to_string()))?;
        operation(&mut connection).map_err(TranslationError::persistence)
    }
}

impl TranslationStore for SqliteTranslationStore {
    fn best(
        &self,
        work: &str,
        keys: &[String],
    ) -> Result<HashMap<String, StoredTranslation>, TranslationError> {
        self.with_connection(|connection| {
            let mut best = HashMap::<String, StoredTranslation>::new();
            for chunk in keys.chunks(KEYS_PER_QUERY) {
                let slots = vec!["?"; chunk.len()].join(",");
                let mut statement = connection.prepare_cached(&format!(
                    "SELECT key, origin, text FROM translation
                     WHERE work = ? AND language = ? AND key IN ({slots})"
                ))?;
                let values = [work, TRANSLATION_LANGUAGE]
                    .into_iter()
                    .chain(chunk.iter().map(String::as_str));
                let rows = statement.query_map(params_from_iter(values), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?;
                for row in rows {
                    let (key, origin, text) = row?;
                    let Some(origin) = origin_from(&origin) else {
                        continue;
                    };
                    let better = best.get(&key).is_none_or(|current| origin < current.origin);
                    if better {
                        best.insert(key, StoredTranslation { text, origin });
                    }
                }
            }
            Ok(best)
        })
    }

    fn save(
        &self,
        work: &str,
        translations: &[NewTranslation<'_>],
    ) -> Result<(), TranslationError> {
        let now = database::now_rfc3339();
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            {
                let mut text = transaction.prepare_cached(
                    "INSERT INTO translation_text (work, key, source, kind) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (work, key) DO UPDATE SET
                       source = excluded.source, kind = excluded.kind
                     WHERE translation_text.source = ''",
                )?;
                let mut translation = transaction.prepare_cached(
                    "INSERT INTO translation (work, key, language, origin, text, pack, model, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)
                     ON CONFLICT (work, key, language, origin) DO UPDATE SET
                       text = excluded.text, model = excluded.model, created_at = excluded.created_at",
                )?;
                // A new translation from the player's translator replaces a stale one.
                let mut stale = transaction.prepare_cached(
                    "DELETE FROM translation
                     WHERE work = ?1 AND key = ?2 AND language = ?3 AND origin = 'stale'",
                )?;
                for entry in translations {
                    text.execute(params![work, entry.key, entry.source, kind_name(entry.kind)])?;
                    translation.execute(params![
                        work,
                        entry.key,
                        TRANSLATION_LANGUAGE,
                        origin_name(entry.origin),
                        entry.text,
                        entry.model,
                        now,
                    ])?;
                    if entry.origin == TranslationOrigin::Local {
                        stale.execute(params![work, entry.key, TRANSLATION_LANGUAGE])?;
                    }
                }
            }
            transaction.commit()
        })
    }

    fn mark_stale(&self, work: &str, keys: &[String]) -> Result<u64, TranslationError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            let mut marked = 0;
            for chunk in keys.chunks(KEYS_PER_QUERY) {
                let slots = vec!["?"; chunk.len()].join(",");
                // Only lines shown: a pack's line or an edit above one keeps it
                // out. A line marked before and not translated since is replaced.
                marked += transaction.execute(
                    &format!(
                        "UPDATE OR REPLACE translation SET origin = 'stale'
                         WHERE work = ? AND language = ? AND origin = 'local' AND key IN ({slots})
                           AND NOT EXISTS (
                             SELECT 1 FROM translation AS better
                             WHERE better.work = translation.work AND better.key = translation.key
                               AND better.language = translation.language
                               AND better.origin IN ('edit', 'pack_reviewed', 'pack_machine'))"
                    ),
                    params_from_iter(
                        [work, TRANSLATION_LANGUAGE]
                            .into_iter()
                            .chain(chunk.iter().map(String::as_str)),
                    ),
                )? as u64;
            }
            transaction.commit()?;
            Ok(marked)
        })
    }

    fn names(&self, work: &str) -> Result<Vec<(String, String)>, TranslationError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT japanese, text FROM translation_name
                 WHERE work = ?1 AND language = ?2 ORDER BY japanese",
            )?;
            statement
                .query_map(params![work, TRANSLATION_LANGUAGE], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect()
        })
    }

    fn save_names(&self, work: &str, names: &[(String, String)]) -> Result<(), TranslationError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            {
                let mut statement = transaction.prepare_cached(
                    "INSERT INTO translation_name (work, japanese, language, text) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (work, japanese, language) DO UPDATE SET text = excluded.text",
                )?;
                for (japanese, english) in names {
                    statement.execute(params![work, japanese, TRANSLATION_LANGUAGE, english])?;
                }
            }
            transaction.commit()
        })
    }

    fn glossary(&self, work: &str) -> Result<Glossary, TranslationError> {
        self.with_connection(|connection| {
            let note = connection
                .query_row(
                    "SELECT text FROM translation_note WHERE work = ?1 AND language = ?2",
                    params![work, TRANSLATION_LANGUAGE],
                    |row| row.get(0),
                )
                .optional()?;
            let mut statement = connection.prepare_cached(
                "SELECT japanese, text FROM translation_term
                 WHERE work = ?1 AND language = ?2 ORDER BY japanese",
            )?;
            let terms = statement
                .query_map(params![work, TRANSLATION_LANGUAGE], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect::<rusqlite::Result<_>>()?;
            Ok(Glossary { note, terms })
        })
    }

    fn save_glossary(&self, work: &str, glossary: &Glossary) -> Result<(), TranslationError> {
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            for table in ["translation_note", "translation_term"] {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE work = ?1 AND language = ?2"),
                    params![work, TRANSLATION_LANGUAGE],
                )?;
            }
            if let Some(note) = &glossary.note {
                transaction.execute(
                    "INSERT INTO translation_note (work, language, text) VALUES (?1, ?2, ?3)",
                    params![work, TRANSLATION_LANGUAGE, note],
                )?;
            }
            {
                let mut term = transaction.prepare_cached(
                    "INSERT INTO translation_term (work, japanese, language, text) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (work, japanese, language) DO UPDATE SET text = excluded.text",
                )?;
                for (japanese, english) in &glossary.terms {
                    term.execute(params![work, japanese, TRANSLATION_LANGUAGE, english])?;
                }
            }
            transaction.commit()
        })
    }

    fn all(&self, work: &str) -> Result<Vec<(String, String)>, TranslationError> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare_cached(
                "SELECT translation.key, translation_text.source, translation.origin,
                        translation.text
                 FROM translation
                 JOIN translation_text USING (work, key)
                 WHERE translation.work = ?1 AND translation.language = ?2
                   AND translation_text.source != ''",
            )?;
            let rows = statement.query_map(params![work, TRANSLATION_LANGUAGE], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            let mut best = HashMap::<String, (TranslationOrigin, String, String)>::new();
            for row in rows {
                let (key, source, origin, text) = row?;
                let Some(origin) = origin_from(&origin) else {
                    continue;
                };
                if best.get(&key).is_none_or(|current| origin < current.0) {
                    best.insert(key, (origin, source, text));
                }
            }
            let mut all = best
                .into_values()
                .map(|(_, source, text)| (source, text))
                .collect::<Vec<_>>();
            all.sort();
            Ok(all)
        })
    }

    // A pack line whose Japanese this PC has not seen gets an empty source,
    // filled in when the game shows it.
    fn save_pack(
        &self,
        work: &str,
        pack: &str,
        translations: &[NewTranslation<'_>],
    ) -> Result<(), TranslationError> {
        let now = database::now_rfc3339();
        self.with_connection(|connection| {
            let transaction = connection.transaction()?;
            transaction.execute(
                "DELETE FROM translation
                 WHERE work = ?1 AND language = ?2 AND origin IN (?3, ?4)",
                params![
                    work,
                    TRANSLATION_LANGUAGE,
                    origin_name(TranslationOrigin::PackReviewed),
                    origin_name(TranslationOrigin::PackMachine),
                ],
            )?;
            {
                let mut text = transaction.prepare_cached(
                    "INSERT INTO translation_text (work, key, source, kind) VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (work, key) DO UPDATE SET
                       source = excluded.source, kind = excluded.kind
                     WHERE translation_text.source = '' AND excluded.source != ''",
                )?;
                let mut translation = transaction.prepare_cached(
                    "INSERT INTO translation (work, key, language, origin, text, pack, model, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7)
                     ON CONFLICT (work, key, language, origin) DO UPDATE SET
                       text = excluded.text, pack = excluded.pack, created_at = excluded.created_at",
                )?;
                for entry in translations {
                    text.execute(params![work, entry.key, entry.source, kind_name(entry.kind)])?;
                    translation.execute(params![
                        work,
                        entry.key,
                        TRANSLATION_LANGUAGE,
                        origin_name(entry.origin),
                        entry.text,
                        pack,
                        now,
                    ])?;
                }
            }
            transaction.commit()
        })
    }
}

fn kind_name(kind: TextKind) -> &'static str {
    match kind {
        TextKind::Dialogue => "dialogue",
        TextKind::Choice => "choice",
        TextKind::Menu => "menu",
        TextKind::Database => "database",
        TextKind::Addon => "addon",
    }
}

fn origin_name(origin: TranslationOrigin) -> &'static str {
    match origin {
        TranslationOrigin::Edit => "edit",
        TranslationOrigin::PackReviewed => "pack_reviewed",
        TranslationOrigin::PackMachine => "pack_machine",
        TranslationOrigin::Local => "local",
        TranslationOrigin::Stale => "stale",
    }
}

fn origin_from(name: &str) -> Option<TranslationOrigin> {
    match name {
        "edit" => Some(TranslationOrigin::Edit),
        "pack_reviewed" => Some(TranslationOrigin::PackReviewed),
        "pack_machine" => Some(TranslationOrigin::PackMachine),
        "local" => Some(TranslationOrigin::Local),
        "stale" => Some(TranslationOrigin::Stale),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use dla_application::translation::text_key;
    use tempfile::tempdir;

    use super::*;

    fn entry<'a>(
        key: &'a str,
        source: &'a str,
        text: &'a str,
        origin: TranslationOrigin,
    ) -> NewTranslation<'a> {
        NewTranslation {
            key,
            source,
            kind: TextKind::Dialogue,
            text,
            origin,
            model: Some("qwen3:4b-instruct"),
        }
    }

    #[test]
    fn a_newer_pack_replaces_the_older_and_wins_over_the_translator() {
        let folder = tempdir().unwrap();
        let store =
            SqliteTranslationStore::open(&folder.path().join("translations.sqlite")).unwrap();
        let yes = text_key("はい");
        let hint = text_key("右上のヒント");
        let old = text_key("古い行");
        store
            .save(
                "RJ1",
                &[entry(&yes, "はい", "Yes", TranslationOrigin::Local)],
            )
            .unwrap();
        store
            .save_pack(
                "RJ1",
                "2026-10-01",
                &[entry(
                    &old,
                    "古い行",
                    "Old line",
                    TranslationOrigin::PackMachine,
                )],
            )
            .unwrap();
        store
            .save_pack(
                "RJ1",
                "2026-10-30",
                &[
                    entry(&yes, "はい", "Sure", TranslationOrigin::PackReviewed),
                    // Text the game shows only while playing: no Japanese yet.
                    entry(&hint, "", "Hint", TranslationOrigin::PackMachine),
                ],
            )
            .unwrap();

        let best = store
            .best("RJ1", &[yes.clone(), hint.clone(), old.clone()])
            .unwrap();
        assert_eq!(best[&yes].text, "Sure");
        assert_eq!(best[&hint].text, "Hint");
        assert!(!best.contains_key(&old));
        // The game's copy holds only lines whose Japanese is known…
        assert_eq!(
            store.all("RJ1").unwrap(),
            vec![("はい".to_owned(), "Sure".to_owned())]
        );
        // …until the game shows the text and its Japanese arrives.
        store
            .save(
                "RJ1",
                &[entry(
                    &hint,
                    "右上のヒント",
                    "Hint (model)",
                    TranslationOrigin::Local,
                )],
            )
            .unwrap();
        assert_eq!(
            store.all("RJ1").unwrap(),
            vec![
                ("はい".to_owned(), "Sure".to_owned()),
                ("右上のヒント".to_owned(), "Hint".to_owned())
            ]
        );
    }

    #[test]
    fn keeps_translations_per_work_and_prefers_the_players_edit() {
        let folder = tempdir().unwrap();
        let store =
            SqliteTranslationStore::open(&folder.path().join("translations.sqlite")).unwrap();
        let key = text_key("はい");
        store
            .save(
                "RJ1",
                &[entry(&key, "はい", "Yes", TranslationOrigin::Local)],
            )
            .unwrap();
        store
            .save(
                "RJ1",
                &[entry(&key, "はい", "Sure", TranslationOrigin::Edit)],
            )
            .unwrap();
        store
            .save(
                "RJ2",
                &[entry(&key, "はい", "Yep", TranslationOrigin::PackMachine)],
            )
            .unwrap();

        let best = store
            .best("RJ1", &[key.clone(), text_key("いいえ")])
            .unwrap();
        assert_eq!(best.len(), 1);
        assert_eq!(
            best[&key],
            StoredTranslation {
                text: "Sure".to_owned(),
                origin: TranslationOrigin::Edit
            }
        );
        assert_eq!(
            store.best("RJ2", std::slice::from_ref(&key)).unwrap()[&key].text,
            "Yep"
        );
        assert_eq!(
            store.all("RJ1").unwrap(),
            vec![("はい".to_owned(), "Sure".to_owned())]
        );
    }

    #[test]
    fn a_newer_translation_from_the_same_origin_replaces_the_older_one() {
        let folder = tempdir().unwrap();
        let path = folder.path().join("translations.sqlite");
        let key = text_key("剣");
        {
            let store = SqliteTranslationStore::open(&path).unwrap();
            store
                .save(
                    "RJ1",
                    &[entry(&key, "剣", "Blade", TranslationOrigin::Local)],
                )
                .unwrap();
            store
                .save(
                    "RJ1",
                    &[entry(&key, "剣", "Sword", TranslationOrigin::Local)],
                )
                .unwrap();
        }
        let reopened = SqliteTranslationStore::open(&path).unwrap();
        assert_eq!(
            reopened.best("RJ1", std::slice::from_ref(&key)).unwrap()[&key].text,
            "Sword"
        );
    }

    #[test]
    fn a_stale_line_stays_until_translated_again_and_packs_are_never_stale() {
        let folder = tempdir().unwrap();
        let store =
            SqliteTranslationStore::open(&folder.path().join("translations.sqlite")).unwrap();
        let (sword, yes) = (text_key("剣"), text_key("はい"));
        store
            .save(
                "RJ1",
                &[
                    entry(&sword, "剣", "Blade", TranslationOrigin::Local),
                    entry(&yes, "はい", "Yes", TranslationOrigin::Local),
                ],
            )
            .unwrap();
        store
            .save_pack(
                "RJ1",
                "2026-10-30",
                &[entry(&yes, "はい", "Yes!", TranslationOrigin::PackReviewed)],
            )
            .unwrap();

        // Marking twice changes nothing more; the pack's line hides the other.
        let keys = [sword.clone(), yes.clone()];
        assert_eq!(store.mark_stale("RJ1", &keys).unwrap(), 1);
        assert_eq!(store.mark_stale("RJ1", &keys).unwrap(), 0);
        let best = store.best("RJ1", &keys).unwrap();
        assert_eq!(
            (best[&sword].origin, best[&sword].text.as_str()),
            (TranslationOrigin::Stale, "Blade")
        );
        assert_eq!(best[&yes].origin, TranslationOrigin::PackReviewed);
        assert!(
            store
                .all("RJ1")
                .unwrap()
                .contains(&("剣".to_owned(), "Blade".to_owned()))
        );

        store
            .save(
                "RJ1",
                &[entry(&sword, "剣", "Sword", TranslationOrigin::Local)],
            )
            .unwrap();
        let best = store.best("RJ1", std::slice::from_ref(&sword)).unwrap();
        assert_eq!(
            (best[&sword].origin, best[&sword].text.as_str()),
            (TranslationOrigin::Local, "Sword")
        );
        let stale_rows = store
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM translation WHERE origin = 'stale'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(stale_rows, 0);
    }

    #[test]
    fn keeps_a_glossary_per_work_and_replaces_it_whole() {
        let folder = tempdir().unwrap();
        let store =
            SqliteTranslationStore::open(&folder.path().join("translations.sqlite")).unwrap();
        let term = |japanese: &str, english: &str| (japanese.to_owned(), english.to_owned());
        let first = Glossary {
            note: Some("A parody of Ragnarok Online.".to_owned()),
            terms: vec![term("ポリン", "Poring"), term("プロンテラ", "Prontera")],
        };
        store.save_glossary("RJ1", &first).unwrap();
        assert_eq!(
            store.glossary("RJ1").unwrap(),
            Glossary {
                note: first.note.clone(),
                terms: vec![term("プロンテラ", "Prontera"), term("ポリン", "Poring")],
            }
        );
        assert_eq!(store.glossary("RJ2").unwrap(), Glossary::default());

        let second = Glossary {
            note: None,
            terms: vec![term("プパ", "Pupa")],
        };
        store.save_glossary("RJ1", &second).unwrap();
        assert_eq!(store.glossary("RJ1").unwrap(), second);
    }

    #[test]
    fn finds_the_lines_a_model_made() {
        let folder = tempdir().unwrap();
        let store =
            SqliteTranslationStore::open(&folder.path().join("translations.sqlite")).unwrap();
        let (sword, yes, shield) = (text_key("剣"), text_key("はい"), text_key("盾"));
        store
            .save(
                "RJ1",
                &[
                    entry(&sword, "剣", "Blade", TranslationOrigin::Local),
                    NewTranslation {
                        model: Some("qwen3.8-27b"),
                        ..entry(&yes, "はい", "Yes", TranslationOrigin::Local)
                    },
                    entry(&shield, "盾", "Shield", TranslationOrigin::Local),
                ],
            )
            .unwrap();
        store
            .save(
                "RJ2",
                &[entry(&sword, "剣", "Sword", TranslationOrigin::Local)],
            )
            .unwrap();
        store
            .mark_stale("RJ1", std::slice::from_ref(&shield))
            .unwrap();

        // A stale line is already to be translated again.
        assert_eq!(
            store.keys_made_with("RJ1", "qwen3:4b-instruct").unwrap(),
            vec![sword]
        );
        assert_eq!(
            store.keys_made_with("RJ1", "qwen3.8-27b").unwrap(),
            vec![yes]
        );
    }

    #[test]
    fn looks_up_more_keys_than_one_query_can_hold() {
        let folder = tempdir().unwrap();
        let store =
            SqliteTranslationStore::open(&folder.path().join("translations.sqlite")).unwrap();
        let texts = (0..1200)
            .map(|number| format!("台詞{number}"))
            .collect::<Vec<_>>();
        let keys = texts.iter().map(|text| text_key(text)).collect::<Vec<_>>();
        let entries = texts
            .iter()
            .zip(&keys)
            .map(|(text, key)| entry(key, text, "Line", TranslationOrigin::Local))
            .collect::<Vec<_>>();
        store.save("RJ1", &entries).unwrap();
        assert_eq!(store.best("RJ1", &keys).unwrap().len(), 1200);
    }

    #[test]
    fn keeps_a_name_list_per_work() {
        let folder = tempdir().unwrap();
        let store =
            SqliteTranslationStore::open(&folder.path().join("translations.sqlite")).unwrap();
        store
            .save_names("RJ1", &[("ユキナ".to_owned(), "Yukina".to_owned())])
            .unwrap();
        store
            .save_names("RJ1", &[("ユキナ".to_owned(), "Yukina S.".to_owned())])
            .unwrap();
        assert_eq!(
            store.names("RJ1").unwrap(),
            vec![("ユキナ".to_owned(), "Yukina S.".to_owned())]
        );
        assert!(store.names("RJ2").unwrap().is_empty());
    }
}
