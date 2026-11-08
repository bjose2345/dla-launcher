//! Translation jobs: the launcher's own requests for a game's texts it has not
//! translated yet, in a zip another PC runs against its model, and the replies
//! that PC sends back, checked and stored the way the launcher's preparation
//! stores them. For translating on a faster PC during development.
//!
//! ```text
//! translation_job glossary <launcher-data> <work> <glossary.tsv>
//! translation_job edit <launcher-data> <work> <installation-folder> <text> <english>
//! translation_job edits <launcher-data> <work> <installation-folder> <edits.jsonl>
//! translation_job export [--redo-model <model>] [--redo-terms] <launcher-data> <work> <installation-folder> <job.zip>
//! translation_job import [--keep-imperfect] <launcher-data> <work> <installation-folder> <job.zip> <results.zip>
//! translation_job pack --version <YYYY-MM-DD> <launcher-data> <work> <installation-folder> <overlays-dlsite-copy>
//! ```
//!
//! `<launcher-data>` is the launcher's data folder, with `translations.sqlite`;
//! `<work>` is the work code the cache knows the game by (`RJ112559`).
//!
//! `glossary` replaces the work's glossary with a UTF-8 file: one
//! `japanese<TAB>english` term a line, an optional `note<TAB>…` line the
//! translator is told about the game, and `#` comments.
//!
//! `edit` stores the player's own wording for a text the game shows, also
//! the game's own English (`MOBDICTIONARY` as `BESTIARY`), and updates the
//! game's copy. `edits` stores many at once from a UTF-8 file, one
//! `{"ja": text, "en": english}` a line (a `key` beside them must be the
//! text's), and updates the game's copy once.
//!
//! `--redo-model` marks the lines a weaker model made (`qwen3:4b-instruct`)
//! stale, as translating the game again does, so they go in the job too;
//! `--redo-terms` marks the lines whose Japanese uses a glossary term. They
//! stay shown until a reply replaces them; the launcher's own preparation
//! would translate them again as well.
//!
//! `pack` writes the work's translation for the DLA archive into a local copy
//! of its `/overlays/dlsite` folder: `<work>/translations/<version>/en/`,
//! where `--version` is the day the version of the game translated came out
//! (`2013-08-25` for RJ112559 4.1). Packing that version again replaces its
//! pack, made later; other versions' folders stay as they are. It records the
//! time in `index.json`, the list the launcher badges the library from.
//! As a work's fixes, the folder holds `manifest.json`, `SHA256SUMS.txt` and
//! the zip, named as a fix is by the work and its CRC-32
//! (`RJ112559-1a2b3c4d-en.zip`: `texts.jsonl`, `names.jsonl`,
//! `glossary.json`). Upload those three with `manifest.json` last, then the
//! list; a version packed again has a new zip, and the old one may stay.
//!
//! A job holds `job.json` (`work`, the sampling, the name spellings),
//! `requests.jsonl` (`id`, `group`, `lines`, `max_tokens`, `messages`: what
//! the other PC sends) and `batches.jsonl` (`id`, `style` and the `keys` of
//! the texts each request covers). The results hold `results.jsonl`
//! (`{"id", "content"}`, or `{"id", "error"}`) and `run.json` (`model`).
//! Importing keeps a reply only when it passes the launcher's checks; the
//! texts left are in the next export. On the last round, `--keep-imperfect`
//! keeps the replies that still fail them, as the launcher's own second try
//! does: an imperfect line (a lost colour code) reads better than Japanese.
//! A text that already shows a line (a stale one, or one an earlier round
//! kept) takes only a reply that passes them.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use dla_application::{
    game_translation::{GameFolder, GameTexts},
    translation::{
        BatchStyle, Glossary, NewTranslation, PlannedBatch, PreparedText, SourceText,
        TranslationService, TranslationStore, text_key, uses_term,
    },
};
use dla_domain::{
    installation::InstallationId,
    translation::{GameTranslation, TextKind, TextMarkup, TranslationOrigin},
};
use dla_game_translation::DesktopGameFolder;
use dla_sqlite::SqliteTranslationStore;
use dla_translator::{NameList, TEMPERATURE, TOP_P, batch_request, read_batch_reply};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const FORMAT: u64 = 1;

fn main() {
    let mut arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let keep_imperfect = match arguments
        .iter()
        .position(|argument| argument == "--keep-imperfect")
    {
        Some(at) => {
            arguments.remove(at);
            true
        }
        None => false,
    };
    let redo_model = match arguments
        .iter()
        .position(|argument| argument == "--redo-model")
    {
        Some(at) if at + 1 < arguments.len() => {
            arguments.remove(at);
            Some(arguments.remove(at))
        }
        _ => None,
    };
    let version = match arguments
        .iter()
        .position(|argument| argument == "--version")
    {
        Some(at) if at + 1 < arguments.len() => {
            arguments.remove(at);
            Some(arguments.remove(at))
        }
        _ => None,
    };
    let redo_terms = match arguments
        .iter()
        .position(|argument| argument == "--redo-terms")
    {
        Some(at) => {
            arguments.remove(at);
            true
        }
        None => false,
    };
    let result = match arguments.first().map(String::as_str) {
        Some("glossary") if arguments.len() == 4 => glossary(
            Path::new(&arguments[1]),
            &arguments[2],
            Path::new(&arguments[3]),
        ),
        Some("edit") if arguments.len() == 6 => edit(
            Path::new(&arguments[1]),
            &arguments[2],
            &arguments[3],
            &arguments[4],
            &arguments[5],
        ),
        Some("edits") if arguments.len() == 5 => edits(
            Path::new(&arguments[1]),
            &arguments[2],
            &arguments[3],
            Path::new(&arguments[4]),
        ),
        Some("export") if arguments.len() == 5 => export(
            Path::new(&arguments[1]),
            &arguments[2],
            &arguments[3],
            Path::new(&arguments[4]),
            redo_model.as_deref(),
            redo_terms,
        ),
        Some("import") if arguments.len() == 6 => import(
            Path::new(&arguments[1]),
            &arguments[2],
            &arguments[3],
            Path::new(&arguments[4]),
            Path::new(&arguments[5]),
            keep_imperfect,
        ),
        Some("pack") if arguments.len() == 5 && version.is_some() => pack(
            Path::new(&arguments[1]),
            &arguments[2],
            &arguments[3],
            Path::new(&arguments[4]),
            version.as_deref().unwrap_or_default(),
        ),
        _ => Err("usage: see the top of examples/translation_job.rs".to_owned()),
    };
    if let Err(error) = result {
        eprintln!("translation_job: {error}");
        std::process::exit(1);
    }
}

/// The work's translation as a pack for the DLA archive: every line keyed by
/// the hash of its Japanese (a text without Japanese, such as the game's own
/// English reworded, comes with its source), the name spellings and the
/// glossary.
fn pack(data: &Path, work: &str, folder: &str, copy: &Path, version: &str) -> Result<(), String> {
    if work.is_empty() || !work.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(format!("{work} is not a work code"));
    }
    let day = version.len() == 10
        && version.bytes().enumerate().all(|(at, byte)| {
            if at == 4 || at == 7 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        });
    if !day {
        return Err(format!(
            "{version} is not a day (YYYY-MM-DD, when the game's version came out)"
        ));
    }
    let out = copy
        .join(work)
        .join("translations")
        .join(version)
        .join("en");
    let replaced = out.join("manifest.json").exists();
    let made = now_to_the_second();
    let (store, service) = open(data)?;
    let (_, game) = game_texts(folder)?;

    let all = store.all(work).map_err(text)?;
    let keys = all
        .iter()
        .map(|(source, _)| text_key(source))
        .collect::<Vec<_>>();
    let best = store.best(work, &keys).map_err(text)?;
    let mut texts = String::new();
    let mut lines = 0_u64;
    let mut reviewed = 0_u64;
    for ((source, english), key) in all.iter().zip(&keys) {
        let checked = best.get(key).is_some_and(|line| {
            matches!(
                line.origin,
                TranslationOrigin::Edit | TranslationOrigin::PackReviewed
            )
        });
        let mut line = json!({"key": key, "text": english});
        if checked {
            line["reviewed"] = json!(true);
            reviewed += 1;
        }
        // The game's own text the reader would not list (no Japanese) goes
        // with its line; Japanese never does.
        if !dla_application::translation::has_japanese(source) {
            line["source"] = json!(source);
        }
        texts.push_str(&json_line(&line)?);
        lines += 1;
    }
    let mut names = String::new();
    for (japanese, english) in service.names(work).map_err(text)? {
        names.push_str(&json_line(
            &json!({"key": text_key(&japanese), "text": english}),
        )?);
    }
    let glossary = store.glossary(work).map_err(text)?;
    let mut files = vec![("texts.jsonl", texts), ("names.jsonl", names)];
    if glossary.note.is_some() || !glossary.terms.is_empty() {
        files.push((
            "glossary.json",
            serde_json::to_string_pretty(&json!({
                "note": glossary.note,
                "terms": glossary.terms,
            }))
            .map_err(text)?,
        ));
    }

    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in &files {
        zip.start_file(*name, SimpleFileOptions::default())
            .map_err(text)?;
        zip.write_all(content.as_bytes()).map_err(text)?;
    }
    let zip = zip.finish().map_err(text)?.into_inner();
    let mut crc = flate2::Crc::new();
    crc.update(&zip);
    let zip_name = format!("{work}-{:08x}-en.zip", crc.sum());
    let zip_sha256 = hex::encode(Sha256::digest(&zip));
    std::fs::create_dir_all(&out).map_err(text)?;
    std::fs::write(out.join(&zip_name), &zip).map_err(text)?;
    // As a work's fixes have it, for checking by hand with `sha256sum -c`.
    std::fs::write(
        out.join("SHA256SUMS.txt"),
        format!("{zip_sha256}  {zip_name}\n"),
    )
    .map_err(text)?;
    let covered = game
        .texts
        .iter()
        .filter(|source| best.contains_key(&text_key(&source.text)))
        .count();
    // Written last: a pack without it is not one yet.
    let manifest = json!({
        "format": 2,
        "work": work,
        "version": version,
        "language": "en",
        "made": made,
        "lines": lines,
        "reviewed": reviewed,
        "game": {"texts": game.texts.len(), "covered": covered},
        "file": {"name": zip_name, "bytes": zip.len(), "sha256": zip_sha256},
    });
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).map_err(text)?,
    )
    .map_err(text)?;

    // The list the launcher marks the library from: when each work's newest
    // translation was made.
    let list_path = copy.join("index.json");
    let mut list = match std::fs::read(&list_path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).map_err(text)?,
        Err(_) => json!({"format": 1, "works": {}}),
    };
    list["works"][work]["en"] = json!(made);
    let mut versions: Vec<String> = list["versions"][work]["en"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if !versions.iter().any(|value| value == version) {
        versions.push(version.to_owned());
    }
    versions.sort();
    list["versions"][work]["en"] = json!(versions);
    std::fs::write(
        &list_path,
        serde_json::to_string_pretty(&list).map_err(text)?,
    )
    .map_err(text)?;

    println!(
        "{work} {version}: {lines} lines ({reviewed} checked by a person), covering {covered} \
         of the game's {} texts, made {made}{}, in {}",
        game.texts.len(),
        if replaced {
            ", replacing the pack before"
        } else {
            ""
        },
        out.display()
    );
    println!(
        "upload {zip_name}, SHA256SUMS.txt and then manifest.json from {} to \
         /overlays/dlsite/{work}/translations/{version}/en/, then {} to /overlays/dlsite/",
        out.display(),
        list_path.display()
    );
    Ok(())
}

fn glossary(data: &Path, work: &str, file: &Path) -> Result<(), String> {
    let (store, _) = open(data)?;
    let mut glossary = Glossary::default();
    let content = std::fs::read_to_string(file).map_err(text)?;
    for (at, line) in content.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((japanese, english)) = line.split_once('\t') else {
            return Err(format!(
                "line {}: no tab between the Japanese and the English",
                at + 1
            ));
        };
        let (japanese, english) = (japanese.trim(), english.trim());
        if japanese == "note" {
            glossary.note = Some(english.to_owned());
        } else if !japanese.is_empty() && !english.is_empty() {
            glossary
                .terms
                .push((japanese.to_owned(), english.to_owned()));
        }
    }
    store.save_glossary(work, &glossary).map_err(text)?;
    println!(
        "{work}: {} glossary terms{}",
        glossary.terms.len(),
        if glossary.note.is_some() {
            " and a note"
        } else {
            ""
        }
    );
    Ok(())
}

fn export(
    data: &Path,
    work: &str,
    folder: &str,
    out: &Path,
    redo_model: Option<&str>,
    redo_terms: bool,
) -> Result<(), String> {
    let (store, service) = open(data)?;
    if let Some(model) = redo_model {
        let keys = store.keys_made_with(work, model).map_err(text)?;
        let marked = store.mark_stale(work, &keys).map_err(text)?;
        println!("marked {marked} lines made with {model} to be translated again");
    }
    let (_, texts) = game_texts(folder)?;
    if redo_terms {
        let terms = store.glossary(work).map_err(text)?.terms;
        let using = texts
            .texts
            .iter()
            .filter(|source| {
                terms
                    .iter()
                    .any(|(japanese, _)| uses_term(&source.text, japanese))
            })
            .map(|source| source.text.clone())
            .collect::<Vec<_>>();
        let marked = service.mark_stale(work, &using).map_err(text)?;
        println!("marked {marked} lines using glossary terms to be translated again");
    }
    let names = service.names(work).map_err(text)?;
    let spelled = names
        .iter()
        .map(|(japanese, _)| japanese.as_str())
        .collect::<Vec<_>>();
    let unspelled = texts
        .names
        .iter()
        .filter(|name| !spelled.contains(&name.as_str()))
        .count();
    if unspelled > 0 {
        eprintln!(
            "note: {unspelled} character names are not spelled yet; let the launcher prepare the \
             game until its names are done, or the job's English may spell them differently"
        );
    }
    let batches = service
        .plan(work, &texts.texts, TextMarkup::RpgMaker)
        .map_err(text)?;
    if batches.is_empty() {
        return Err(format!("{work} has nothing left to translate"));
    }

    let mut requests = String::new();
    let mut covered = String::new();
    let mut lines = 0;
    for (at, batch) in batches.iter().enumerate() {
        let id = format!("{:06}", at + 1);
        let (messages, max_tokens) = batch_request(&batch.request());
        let style = style_name(batch.style);
        requests.push_str(&json_line(&json!({
            "id": id,
            "group": style,
            "lines": batch.texts.len(),
            "max_tokens": max_tokens,
            "messages": messages,
        }))?);
        covered.push_str(&json_line(&json!({
            "id": id,
            "style": style,
            "keys": batch.texts.iter().map(|text| text_key(&text.text)).collect::<Vec<_>>(),
        }))?);
        lines += batch.texts.len();
    }
    let job = json!({
        "format": FORMAT,
        "work": work,
        "created": now(),
        "requests": batches.len(),
        "lines": lines,
        "temperature": TEMPERATURE,
        "top_p": TOP_P,
        "names": names,
    });
    let mut zip = ZipWriter::new(File::create(out).map_err(text)?);
    for (name, content) in [
        (
            "job.json",
            serde_json::to_string_pretty(&job).map_err(text)?,
        ),
        ("requests.jsonl", requests),
        ("batches.jsonl", covered),
    ] {
        zip.start_file(name, SimpleFileOptions::default())
            .map_err(text)?;
        zip.write_all(content.as_bytes()).map_err(text)?;
    }
    zip.finish().map_err(text)?;
    println!(
        "exported {} requests covering {lines} texts to {}",
        batches.len(),
        out.display()
    );
    Ok(())
}

fn import(
    data: &Path,
    work: &str,
    folder: &str,
    job_path: &Path,
    results_path: &Path,
    keep_imperfect: bool,
) -> Result<(), String> {
    let (store, service) = open(data)?;
    let (translation, texts) = game_texts(folder)?;
    let by_key = texts
        .texts
        .iter()
        .map(|text| (text_key(&text.text), text.clone()))
        .collect::<HashMap<_, _>>();

    let mut job = ZipArchive::new(File::open(job_path).map_err(text)?).map_err(text)?;
    let header: Value = serde_json::from_str(&read_entry(&mut job, "job.json")?).map_err(text)?;
    if header["work"].as_str() != Some(work) {
        return Err(format!("the job is for {}, not {work}", header["work"]));
    }
    let names =
        serde_json::from_value::<Vec<(String, String)>>(header["names"].clone()).map_err(text)?;
    // The lines each request sent, as the launcher that made the job
    // prepared them. Split only where the request joined them: a database
    // text may keep a `\r` of its own line breaks, even at its end.
    let mut sent = HashMap::new();
    for line in read_entry(&mut job, "requests.jsonl")?.lines() {
        let request: Value = serde_json::from_str(line).map_err(text)?;
        let lines = request["messages"][1]["content"]
            .as_str()
            .unwrap_or_default()
            .split('\n')
            .map(|line| {
                line.split_once(". ")
                    .map_or(line, |(_, rest)| rest)
                    .to_owned()
            })
            .collect::<Vec<_>>();
        sent.insert(request["id"].as_str().unwrap_or_default().to_owned(), lines);
    }
    let mut batches = HashMap::new();
    for line in read_entry(&mut job, "batches.jsonl")?.lines() {
        let entry: Value = serde_json::from_str(line).map_err(text)?;
        let keys = serde_json::from_value::<Vec<String>>(entry["keys"].clone()).map_err(text)?;
        let style = match entry["style"].as_str() {
            Some("dialogue") => BatchStyle::Dialogue,
            _ => BatchStyle::Terms,
        };
        batches.insert(
            entry["id"].as_str().unwrap_or_default().to_owned(),
            (style, keys),
        );
    }

    let mut results = ZipArchive::new(File::open(results_path).map_err(text)?).map_err(text)?;
    let run: Value =
        serde_json::from_str(&read_entry(&mut results, "run.json")?).unwrap_or(Value::Null);
    let model = run["model"]
        .as_str()
        .unwrap_or("translation job")
        .to_owned();
    let replies = read_entry(&mut results, "results.jsonl")?;

    let (mut accepted, mut rejected, mut errors, mut unmatched) = (0, 0, 0, 0);
    for line in BufReader::new(replies.as_bytes()).lines() {
        let line = line.map_err(text)?;
        if line.trim().is_empty() {
            continue;
        }
        let result: Value = serde_json::from_str(&line).map_err(text)?;
        let id = result["id"].as_str().unwrap_or_default();
        let Some((style, keys)) = batches.get(id) else {
            unmatched += 1;
            continue;
        };
        let Some(content) = result["content"].as_str() else {
            errors += keys.len();
            continue;
        };
        // A text the game no longer has (an update), or a line this launcher
        // cannot match to what was sent, leaves its batch out.
        let lines = sent.get(id).cloned().unwrap_or_default();
        let Some(sources) = keys
            .iter()
            .map(|key| by_key.get(key).cloned())
            .collect::<Option<Vec<SourceText>>>()
        else {
            unmatched += keys.len();
            continue;
        };
        let Some(prepared) = sources
            .iter()
            .zip(&lines)
            .map(|(source, line)| PreparedText::as_sent(&source.text, TextMarkup::RpgMaker, line))
            .collect::<Option<Vec<_>>>()
            .filter(|prepared| prepared.len() == sources.len())
        else {
            unmatched += keys.len();
            continue;
        };
        let batch = PlannedBatch {
            style: *style,
            texts: sources,
            prepared,
            lines,
            names: names.clone(),
            readings: Vec::new(),
            // Reading the reply needs only the lines and the names.
            note: None,
            terms: Vec::new(),
        };
        let replies = read_batch_reply(content, &batch.request());
        // A line already shown passed the checks once: it stays rather than
        // an imperfect reply.
        let shown = if keep_imperfect {
            store.best(work, keys).map_err(text)?.into_keys().collect()
        } else {
            HashSet::new()
        };
        let (strict, lenient): (Vec<usize>, Vec<usize>) =
            (0..batch.texts.len()).partition(|&at| !keep_imperfect || shown.contains(&keys[at]));
        for (positions, lenient) in [(strict, false), (lenient, true)] {
            if positions.is_empty() {
                continue;
            }
            let replies = positions
                .iter()
                .map(|&at| replies.get(at).cloned().flatten())
                .collect::<Vec<_>>();
            let recorded = service
                .record(work, &batch, &positions, &replies, &model, lenient)
                .map_err(text)?;
            accepted += recorded.accepted.len();
            rejected += recorded.rejected.len();
        }
    }
    println!(
        "imported {accepted} lines made with {model}; {rejected} failed the checks, {errors} were \
         not answered and {unmatched} no longer match the game: they go in the next export"
    );

    write_copy(&service, work, folder, &translation)
}

/// Stores the player's own wording for a text the game shows, Japanese or
/// not (the game's own English `MOBDICTIONARY` as `BESTIARY`).
fn edit(data: &Path, work: &str, folder: &str, source: &str, english: &str) -> Result<(), String> {
    let (store, service) = open(data)?;
    let key = text_key(source);
    store
        .save(
            work,
            &[NewTranslation {
                key: &key,
                source,
                kind: TextKind::Menu,
                text: english,
                origin: TranslationOrigin::Edit,
                model: None,
            }],
        )
        .map_err(text)?;
    println!("{work}: {source} is shown as {english}");
    let (translation, _) = game_texts(folder)?;
    write_copy(&service, work, folder, &translation)
}

fn edits(data: &Path, work: &str, folder: &str, file: &Path) -> Result<(), String> {
    let (store, service) = open(data)?;
    let content = std::fs::read_to_string(file).map_err(text)?;
    let mut lines = Vec::new();
    for (at, line) in content.split('\n').enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(line).map_err(|error| format!("line {}: {error}", at + 1))?;
        let (Some(source), Some(english)) = (value["ja"].as_str(), value["en"].as_str()) else {
            return Err(format!("line {} needs \"ja\" and \"en\"", at + 1));
        };
        let key = text_key(source);
        if value["key"].as_str().is_some_and(|given| given != key) {
            return Err(format!("line {}: the key is not its text's", at + 1));
        }
        if english.trim().is_empty() {
            return Err(format!("line {}: the English is empty", at + 1));
        }
        lines.push((key, source.to_owned(), english.to_owned()));
    }
    let translations = lines
        .iter()
        .map(|(key, source, english)| NewTranslation {
            key,
            source,
            kind: TextKind::Menu,
            text: english,
            origin: TranslationOrigin::Edit,
            model: None,
        })
        .collect::<Vec<_>>();
    store.save(work, &translations).map_err(text)?;
    println!(
        "{work}: stored {} of the player's own wordings",
        lines.len()
    );
    let (translation, _) = game_texts(folder)?;
    write_copy(&service, work, folder, &translation)
}

/// The game's own copy, when translation is on for it.
fn write_copy(
    service: &TranslationService,
    work: &str,
    folder: &str,
    translation: &GameTranslation,
) -> Result<(), String> {
    let game = game_folder(folder, translation);
    if game.join("dla-translation").is_dir() {
        DesktopGameFolder::new()
            .write_translations(folder, translation, &service.all(work).map_err(text)?)
            .map_err(text)?;
        println!("updated the game's copy of its translations");
    }
    Ok(())
}

fn open(data: &Path) -> Result<(Arc<SqliteTranslationStore>, TranslationService), String> {
    let store =
        Arc::new(SqliteTranslationStore::open(&data.join("translations.sqlite")).map_err(text)?);
    let service =
        TranslationService::new(store.clone()).with_name_readings(Arc::new(NameList::new()));
    Ok((store, service))
}

/// The game's texts, read the way the launcher reads them for its engine.
fn game_texts(folder: &str) -> Result<(GameTranslation, GameTexts), String> {
    let games = DesktopGameFolder::new();
    let detection = games
        .detect_engine(folder)
        .map_err(text)?
        .ok_or_else(|| format!("{folder} holds no game the launcher can translate"))?;
    let translation = GameTranslation {
        installation_id: InstallationId("translation-job".to_owned()),
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
    let texts = games.read_texts(folder, &translation).map_err(text)?;
    Ok((translation, texts))
}

fn game_folder(folder: &str, translation: &GameTranslation) -> PathBuf {
    match &translation.game_folder {
        Some(game) => Path::new(folder).join(game.as_str()),
        None => PathBuf::from(folder),
    }
}

fn style_name(style: BatchStyle) -> &'static str {
    match style {
        BatchStyle::Dialogue => "dialogue",
        BatchStyle::Terms => "terms",
    }
}

fn read_entry(zip: &mut ZipArchive<File>, name: &str) -> Result<String, String> {
    let mut content = String::new();
    zip.by_name(name)
        .map_err(|error| format!("{name}: {error}"))?
        .read_to_string(&mut content)
        .map_err(text)?;
    Ok(content)
}

fn json_line(value: &Value) -> Result<String, String> {
    Ok(format!("{}\n", serde_json::to_string(value).map_err(text)?))
}

fn now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Now in UTC without parts of a second, so the times sort as text.
fn now_to_the_second() -> String {
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
