//! Builds training data for the translator from paired translations: each
//! example is the exact request the launcher sends its translator for a
//! batch, answered with the official English (or, for the test game, the
//! player's own wording). The model trained on it stays private.
//!
//! ```text
//! training_data [--note <text>] [--test <launcher-data> <work> <installation-folder>] <out-folder> <pairs.jsonl>...
//! ```
//!
//! `<pairs.jsonl>` are written by `pair_translations`. Each work is planned
//! in batches by the launcher's own planner, told its glossary (the official
//! names of its items, skills, monsters, classes and states) and its
//! characters' names, so the model learns to follow the glossary it is given
//! rather than one release's renames. `--note` is what the translator is told
//! about the games (`A parody of Ragnarok Online.`).
//!
//! `faces.txt` lists each work's faces and their line counts.
//!
//! `--test` builds `test.jsonl` from a held-out work's lines the player
//! reworded in the launcher; it must not be among the pairs. The folder gets
//! `train.jsonl`, `test.jsonl` and `faces.txt`, one
//! `{"work", "style", "messages"}` a line with the system, user and
//! assistant messages.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Write,
    path::Path,
    sync::Arc,
};

use dla_application::{
    game_translation::GameFolder,
    translation::{
        BatchStyle, Glossary, PlannedBatch, SourceText, TranslationService, TranslationStore,
        has_japanese, text_key, uses_term,
    },
};
use dla_domain::{
    installation::InstallationId,
    translation::{GameTranslation, TextKind, TextMarkup, TranslationOrigin},
};
use dla_game_translation::DesktopGameFolder;
use dla_sqlite::SqliteTranslationStore;
use dla_translator::{NameList, batch_request};
use serde_json::{Value, json};

const NAME_TABLES: [&str; 7] = [
    "Classes", "Skills", "Items", "Weapons", "Armors", "Enemies", "States",
];

fn main() {
    let mut arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let mut take = |flag: &str, count: usize| -> Option<Vec<String>> {
        let at = arguments.iter().position(|argument| argument == flag)?;
        (at + count < arguments.len()).then(|| arguments.drain(at..=at + count).skip(1).collect())
    };
    let note = take("--note", 1).map(|values| values[0].clone());
    let test = take("--test", 3);
    if arguments.len() < 2 {
        eprintln!("usage: see the top of examples/training_data.rs");
        std::process::exit(2);
    }
    let result = run(
        note.as_deref(),
        test.as_deref(),
        Path::new(&arguments[0]),
        &arguments[1..],
    );
    if let Err(error) = result {
        eprintln!("training_data: {error}");
        std::process::exit(1);
    }
}

fn run(
    note: Option<&str>,
    test: Option<&[String]>,
    out: &Path,
    pairs_files: &[String],
) -> Result<(), String> {
    let mut works = BTreeMap::<String, Vec<Value>>::new();
    for file in pairs_files {
        for (at, line) in fs::read_to_string(file)
            .map_err(text)?
            .split('\n')
            .enumerate()
        {
            if line.trim().is_empty() {
                continue;
            }
            let pair: Value = serde_json::from_str(line)
                .map_err(|error| format!("{file} line {}: {error}", at + 1))?;
            let work = pair["work"].as_str().unwrap_or("unknown").to_owned();
            works.entry(work).or_default().push(pair);
        }
    }
    if let Some([_, test_work, _]) = test
        && works.contains_key(test_work)
    {
        return Err(format!(
            "{test_work} is the test game: it must not be trained on"
        ));
    }
    fs::create_dir_all(out).map_err(text)?;
    let mut train = fs::File::create(out.join("train.jsonl")).map_err(text)?;
    let mut faces_file = fs::File::create(out.join("faces.txt")).map_err(text)?;
    for (work, pairs) in &works {
        let mut faces = BTreeMap::<&str, usize>::new();
        for pair in pairs {
            if let Some(face) = pair["face"].as_str() {
                *faces.entry(face).or_default() += 1;
            }
        }
        writeln!(faces_file, "# {work}: face, lines").map_err(text)?;
        for (face, count) in &faces {
            writeln!(faces_file, "{face}\t{count}").map_err(text)?;
        }
        let glossary = Glossary {
            note: note.map(str::to_owned),
            terms: official_names(pairs),
        };
        let names = pairs
            .iter()
            .filter(|pair| {
                let place = pair["place"].as_str().unwrap_or_default();
                place.starts_with("db:Actors:") && place.ends_with("name")
            })
            .filter_map(|pair| {
                Some((
                    pair["ja"].as_str()?.to_owned(),
                    pair["en"].as_str()?.to_owned(),
                ))
            })
            .collect::<Vec<_>>();
        let (texts, english) = best_english(pairs);
        let built = build(work, &texts, &english, &glossary, &names, &mut train)?;
        println!(
            "{work}: {} pairs, {} texts; {built}",
            pairs.len(),
            texts.len()
        );
    }
    if let Some([data, work, folder]) = test {
        let mut test_file = fs::File::create(out.join("test.jsonl")).map_err(text)?;
        let store = SqliteTranslationStore::open(&Path::new(data).join("translations.sqlite"))
            .map_err(text)?;
        let glossary = store.glossary(work).map_err(text)?;
        let names = store.names(work).map_err(text)?;
        let all = store.all(work).map_err(text)?;
        let keys = all
            .iter()
            .map(|(source, _)| text_key(source))
            .collect::<Vec<_>>();
        let best = store.best(work, &keys).map_err(text)?;
        let reworded = all
            .iter()
            .zip(&keys)
            .filter(|(_, key)| {
                best.get(*key)
                    .is_some_and(|line| line.origin == TranslationOrigin::Edit)
            })
            .map(|((source, english), _)| (source.clone(), english.clone()))
            .collect::<HashMap<_, _>>();
        let texts = game_texts(folder)?
            .into_iter()
            .filter(|text| reworded.contains_key(&text.text))
            .collect::<Vec<_>>();
        let built = build(work, &texts, &reworded, &glossary, &names, &mut test_file)?;
        println!("{work} (test): {} reworded texts; {built}", texts.len());
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// Plans the work's texts as the launcher would and writes each batch,
/// answered with the English, as one example; returns what was written.
fn build(
    work: &str,
    texts: &[SourceText],
    english: &HashMap<String, String>,
    glossary: &Glossary,
    names: &[(String, String)],
    out: &mut fs::File,
) -> Result<String, String> {
    let folder = tempfile::tempdir().map_err(text)?;
    let store = Arc::new(
        SqliteTranslationStore::open(&folder.path().join("translations.sqlite")).map_err(text)?,
    );
    store.save_glossary(work, glossary).map_err(text)?;
    store.save_names(work, names).map_err(text)?;
    let service =
        TranslationService::new(store.clone()).with_name_readings(Arc::new(NameList::new()));
    let markup = TextMarkup::RpgMaker;
    let mut examples = BTreeMap::<&'static str, (usize, usize)>::new();
    let mut unfit = 0;
    for batch in service.plan(work, texts, markup).map_err(text)? {
        let mut kept = PlannedBatch {
            texts: Vec::new(),
            prepared: Vec::new(),
            lines: Vec::new(),
            terms: Vec::new(),
            ..batch.clone()
        };
        let mut replies = Vec::new();
        for ((source, prepared), line) in batch.texts.iter().zip(&batch.prepared).zip(&batch.lines)
        {
            match english
                .get(&source.text)
                .and_then(|english| prepared.reply_for(english, markup))
            {
                Some(reply) => {
                    kept.texts.push(source.clone());
                    kept.prepared.push(prepared.clone());
                    kept.lines.push(line.clone());
                    replies.push(reply);
                }
                None => unfit += 1,
            }
        }
        if replies.is_empty() {
            continue;
        }
        kept.terms = batch
            .terms
            .iter()
            .filter(|(japanese, _)| {
                kept.texts
                    .iter()
                    .any(|text| uses_term(&text.text, japanese))
            })
            .cloned()
            .collect();
        let (messages, _) = batch_request(&kept.request());
        let mut messages = messages.as_array().cloned().unwrap_or_default();
        let answer = replies
            .iter()
            .enumerate()
            .map(|(at, reply)| format!("{}. {reply}", at + 1))
            .collect::<Vec<_>>()
            .join("\n");
        messages.push(json!({"role": "assistant", "content": answer}));
        let style = match batch.style {
            BatchStyle::Dialogue => "dialogue",
            BatchStyle::Terms => "terms",
        };
        writeln!(
            out,
            "{}",
            json!({"work": work, "style": style, "messages": messages})
        )
        .map_err(text)?;
        let counts = examples.entry(style).or_default();
        counts.0 += 1;
        counts.1 += replies.len();
    }
    let summary = examples
        .iter()
        .map(|(style, (batches, lines))| format!("{batches} {style} examples ({lines} lines)"))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!("{summary}; {unfit} lines whose codes did not fit"))
}

/// Each Japanese text once, with the English it is given most often.
fn best_english(pairs: &[Value]) -> (Vec<SourceText>, HashMap<String, String>) {
    let mut seen = HashSet::new();
    let mut texts = Vec::new();
    let mut counts = HashMap::<String, HashMap<String, usize>>::new();
    for pair in pairs {
        let (Some(japanese), Some(english)) = (pair["ja"].as_str(), pair["en"].as_str()) else {
            continue;
        };
        *counts
            .entry(japanese.to_owned())
            .or_default()
            .entry(english.to_owned())
            .or_default() += 1;
        if seen.insert(japanese.to_owned()) {
            texts.push(SourceText {
                text: japanese.to_owned(),
                kind: kind(pair["kind"].as_str().unwrap_or_default()),
            });
        }
    }
    let english = counts
        .into_iter()
        .filter_map(|(japanese, english)| {
            let best = english.into_iter().max_by_key(|(_, count)| *count)?.0;
            Some((japanese, best))
        })
        .collect();
    (texts, english)
}

/// The official English names of the work's items, skills, monsters,
/// classes and states, as its glossary.
fn official_names(pairs: &[Value]) -> Vec<(String, String)> {
    let mut terms = BTreeMap::new();
    for pair in pairs {
        let place = pair["place"].as_str().unwrap_or_default();
        let mut parts = place.split(':');
        let (Some("db"), Some(table), Some(_), Some(field)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if !NAME_TABLES.contains(&table) || !field.trim_start_matches('@').eq("name") {
            continue;
        }
        if let (Some(japanese), Some(english)) = (pair["ja"].as_str(), pair["en"].as_str())
            && has_japanese(japanese)
            && !japanese.contains('\\')
            && japanese.chars().count() <= 16
        {
            terms
                .entry(japanese.to_owned())
                .or_insert_with(|| english.to_owned());
        }
    }
    terms.into_iter().collect()
}

fn kind(name: &str) -> TextKind {
    match name {
        "dialogue" => TextKind::Dialogue,
        "choice" => TextKind::Choice,
        "database" => TextKind::Database,
        _ => TextKind::Menu,
    }
}

/// The game's texts as the launcher reads them.
fn game_texts(folder: &str) -> Result<Vec<SourceText>, String> {
    let games = DesktopGameFolder::new();
    let detection = games
        .detect_engine(folder)
        .map_err(text)?
        .ok_or_else(|| format!("{folder} holds no game the launcher can translate"))?;
    let translation = GameTranslation {
        installation_id: InstallationId("training-data".to_owned()),
        engine: detection.engine,
        game_folder: detection.root,
        shown: true,
        plugin: String::new(),
        changes: Vec::new(),
        turned_on_at: String::new(),
        paused: false,
        prepared: None,
        pack: None,
    };
    Ok(games.read_texts(folder, &translation).map_err(text)?.texts)
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
