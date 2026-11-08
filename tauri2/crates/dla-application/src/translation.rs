use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::{
        Arc, Mutex, PoisonError, RwLock,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use dla_domain::translation::{TextKind, TextMarkup, TranslationOrigin};
use thiserror::Error;

mod text;

pub use text::{PreparedText, has_japanese, has_leftover_japanese, text_key, uses_term};

const DIALOGUE_BATCH: usize = 10;
const TERM_BATCH: usize = 25;
const NAME_BATCH: usize = 30;
const URGENT_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Error)]
pub enum TranslationError {
    #[error("no translator is set up")]
    NoTranslator,
    #[error(transparent)]
    Translator(#[from] TranslatorError),
    #[error("translations could not be stored: {0}")]
    Persistence(String),
    #[error("the translation from our GitHub could not be used: {0}")]
    Pack(String),
}

impl TranslationError {
    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

#[derive(Debug, Error)]
pub enum TranslatorError {
    #[error("the translator could not be reached: {0}")]
    Unreachable(String),
    #[error("the translator refused the request: {0}")]
    Refused(String),
    #[error("the translator's reply could not be read: {0}")]
    UnreadableReply(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceText {
    pub text: String,
    pub kind: TextKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchStyle {
    Dialogue,
    Terms,
}

pub struct TranslatorBatch<'a> {
    pub style: BatchStyle,
    pub lines: &'a [String],
    pub names: &'a [(String, String)],
    /// Other names in the lines, with the readings a name dictionary gives.
    pub readings: &'a [(String, Vec<String>)],
    /// What the work's [`Glossary`] says about the game.
    pub note: Option<&'a str>,
    /// The work's glossary terms the lines use.
    pub terms: &'a [(String, String)],
}

/// What a work's translator is told besides its names: a note about the
/// game (the original a parody follows) and the words it writes one way
/// (プロンテラ = Prontera), so every line uses the same English.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Glossary {
    pub note: Option<String>,
    pub terms: Vec<(String, String)>,
}

/// One request to the player's translator, before it is sent: the texts it
/// covers, each as the translator sees it, and what it is told about names.
/// A translation job hands the same requests to another PC.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedBatch {
    pub style: BatchStyle,
    pub texts: Vec<SourceText>,
    /// Each text as the translator sees it, and its line in the request.
    pub prepared: Vec<PreparedText>,
    pub lines: Vec<String>,
    pub names: Vec<(String, String)>,
    pub readings: Vec<(String, Vec<String>)>,
    pub note: Option<String>,
    pub terms: Vec<(String, String)>,
}

impl PlannedBatch {
    pub fn request(&self) -> TranslatorBatch<'_> {
        self.request_for(&self.lines)
    }

    /// The request for some of the batch's lines, as a second try sends.
    pub fn request_for<'a>(&'a self, lines: &'a [String]) -> TranslatorBatch<'a> {
        TranslatorBatch {
            style: self.style,
            lines,
            names: &self.names,
            readings: &self.readings,
            note: self.note.as_deref(),
            terms: &self.terms,
        }
    }
}

/// What came of a batch's replies: the English kept for each Japanese text,
/// and the positions in the batch whose reply was missing or failed the checks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecordedReplies {
    pub accepted: HashMap<String, String>,
    pub rejected: Vec<usize>,
}

/// A name for the translator to spell, with a name dictionary's readings of
/// it, likeliest first, when it has any.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameToSpell {
    pub japanese: String,
    pub readings: Vec<String>,
}

/// What a name dictionary knows of a Japanese name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NameReading {
    /// Its English spellings, likeliest first.
    pub spellings: Vec<String>,
    /// A Japanese name with only one reading: spelled without asking the
    /// translator.
    pub certain: bool,
}

/// A dictionary of Japanese names. Small models read a kanji name by guess
/// (小野塚 came out as "Otonashi") and spell katakana names letter by letter
/// ("Marsh" for マーシャ).
pub trait NameReadings: Send + Sync {
    /// A whole name, or a full name of a surname and a given name.
    fn look_up(&self, name: &str) -> Option<NameReading>;

    /// The names it can pick out of a line of text, each once.
    fn find_in(&self, line: &str) -> Vec<(String, NameReading)>;
}

pub trait Translator: Send + Sync {
    /// The model or service, recorded with each translation it makes.
    fn name(&self) -> String;

    /// One translation per line, in order; `None` where the reply had none.
    fn translate(
        &self,
        batch: &TranslatorBatch<'_>,
    ) -> Result<Vec<Option<String>>, TranslatorError>;

    fn spell_names(
        &self,
        names: &[NameToSpell],
    ) -> Result<HashMap<String, String>, TranslatorError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredTranslation {
    pub text: String,
    pub origin: TranslationOrigin,
}

pub struct NewTranslation<'a> {
    pub key: &'a str,
    pub source: &'a str,
    pub kind: TextKind,
    pub text: &'a str,
    pub origin: TranslationOrigin,
    pub model: Option<&'a str>,
}

pub trait TranslationStore: Send + Sync {
    /// The best translation of each key that has one (see
    /// [`TranslationOrigin`] for the order).
    fn best(
        &self,
        work: &str,
        keys: &[String],
    ) -> Result<HashMap<String, StoredTranslation>, TranslationError>;

    fn save(&self, work: &str, translations: &[NewTranslation<'_>])
    -> Result<(), TranslationError>;

    fn names(&self, work: &str) -> Result<Vec<(String, String)>, TranslationError>;

    fn save_names(&self, work: &str, names: &[(String, String)]) -> Result<(), TranslationError>;

    fn glossary(&self, work: &str) -> Result<Glossary, TranslationError>;

    /// Replaces the work's glossary.
    fn save_glossary(&self, work: &str, glossary: &Glossary) -> Result<(), TranslationError>;

    /// Every text of the work that has a translation, with its best one.
    fn all(&self, work: &str) -> Result<Vec<(String, String)>, TranslationError>;

    /// Marks the player's translator's lines of these keys as stale, to be
    /// translated again. Returns how many lines it marked.
    fn mark_stale(&self, work: &str, keys: &[String]) -> Result<u64, TranslationError>;

    /// Puts in a pack's lines, replacing those of the work's earlier pack.
    fn save_pack(
        &self,
        work: &str,
        pack: &str,
        translations: &[NewTranslation<'_>],
    ) -> Result<(), TranslationError>;
}

/// A work's lines translated ahead of time, downloaded from the DLA archive.
/// Each line is keyed by the [`text_key`] of its Japanese, which the pack
/// does not hold, so a pack made for one version of a game fits the next:
/// lines the newer version changed or added are not in it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranslationPack {
    pub work: String,
    /// When it was made; it names the pack in the cache.
    pub made: String,
    pub texts: Vec<PackLine>,
    pub names: Vec<PackLine>,
    /// The glossary it was made with, for the player's translator to follow
    /// in the lines the pack lacks.
    pub glossary: Option<Glossary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackLine {
    pub key: String,
    pub text: String,
    /// Checked by a person, not only machine translated.
    pub reviewed: bool,
    /// The text itself, for a text with no Japanese the reader would not list
    /// (the game's own English a person reworded: `MOBDICTIONARY`).
    pub source: Option<String>,
}

/// A pack in the DLA archive for a work: the one made for the version of the
/// game installed, or else for its newest version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackOffer {
    pub work: String,
    /// The game's version it was made for, as the day that version came out
    /// (`2013-08-25`); it names the pack's folder.
    pub version: String,
    /// Where its files are.
    pub folder: String,
    pub lines: u64,
    /// When the translation was made; a pack made later replaces it.
    pub made: String,
}

/// What the DLA archive has for a work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackLookup {
    Offer(PackOffer),
    None,
    /// It has one, for Elite members only.
    ForElite,
    /// Nothing can be asked before the player signs in to DLA.
    SignedOut,
}

/// Where packs come from: the DLA archive, or a folder laid out the same way
/// on this PC while testing.
pub trait TranslationPacks: Send + Sync {
    /// The pack for `version` of the game (the day it came out), when the
    /// launcher knows it and the archive has one, or else for its newest.
    fn find(&self, work: &str, version: Option<&str>) -> Result<PackLookup, TranslationError>;

    fn download(&self, offer: &PackOffer) -> Result<TranslationPack, TranslationError>;

    /// Every work with a pack, and when its newest was made, for the library
    /// to mark them. Empty before the player signs in.
    fn translated_works(&self) -> Result<BTreeMap<String, String>, TranslationError>;
}

/// Everything translated for a work: what a game's own copy holds.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkTranslations {
    pub texts: Vec<(String, String)>,
    pub names: Vec<(String, String)>,
}

/// A player waiting in the game goes before anything translated ahead of play.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Urgency {
    Now,
    Background,
}

pub struct TranslationService {
    store: Arc<dyn TranslationStore>,
    translator: RwLock<Option<Arc<dyn Translator>>>,
    name_readings: Option<Arc<dyn NameReadings>>,
    // The player's translator gets one request at a time, so a laptop
    // running a local model is never flooded.
    turn: Mutex<()>,
    urgent_waiting: AtomicUsize,
}

impl TranslationService {
    pub fn new(store: Arc<dyn TranslationStore>) -> Self {
        Self {
            store,
            translator: RwLock::new(None),
            name_readings: None,
            turn: Mutex::new(()),
            urgent_waiting: AtomicUsize::new(0),
        }
    }

    pub fn with_name_readings(mut self, readings: Arc<dyn NameReadings>) -> Self {
        self.name_readings = Some(readings);
        self
    }

    pub fn set_translator(&self, translator: Option<Arc<dyn Translator>>) {
        *self
            .translator
            .write()
            .unwrap_or_else(PoisonError::into_inner) = translator;
    }

    pub fn has_translator(&self) -> bool {
        self.translator().is_some()
    }

    /// Puts a pack's lines in the cache, replacing those of an earlier pack.
    /// `texts` and `names` are what the game shows, so its lines can go into
    /// the game's own copy; other lines (text seen only while playing) are
    /// kept by key. Returns how many of the pack's lines the game shows.
    pub fn import_pack(
        &self,
        pack: &TranslationPack,
        texts: &[SourceText],
        names: &[String],
    ) -> Result<u64, TranslationError> {
        let shown = texts
            .iter()
            .map(|text| (text_key(&text.text), text))
            .collect::<HashMap<_, _>>();
        let entries = pack
            .texts
            .iter()
            .map(|line| {
                let source = shown.get(&line.key);
                // A text without Japanese comes with the pack, where its key fits.
                let carried = line
                    .source
                    .as_deref()
                    .filter(|text| !has_japanese(text) && text_key(text) == line.key);
                NewTranslation {
                    key: &line.key,
                    source: source.map_or(carried.unwrap_or(""), |source| source.text.as_str()),
                    kind: source.map_or(TextKind::Addon, |source| source.kind),
                    text: &line.text,
                    origin: if line.reviewed {
                        TranslationOrigin::PackReviewed
                    } else {
                        TranslationOrigin::PackMachine
                    },
                    model: None,
                }
            })
            .collect::<Vec<_>>();
        self.store.save_pack(&pack.work, &pack.made, &entries)?;
        let named = names
            .iter()
            .map(|name| (text_key(name), name))
            .collect::<HashMap<_, _>>();
        let spellings = pack
            .names
            .iter()
            .filter_map(|line| {
                named
                    .get(&line.key)
                    .map(|name| ((*name).clone(), line.text.clone()))
            })
            .collect::<Vec<_>>();
        self.store.save_names(&pack.work, &spellings)?;
        if let Some(glossary) = &pack.glossary {
            self.store.save_glossary(&pack.work, glossary)?;
        }
        Ok(pack
            .texts
            .iter()
            .filter(|line| shown.contains_key(&line.key))
            .count() as u64)
    }

    /// The model or service translations are made with now.
    pub fn translator_name(&self) -> Option<String> {
        self.translator().map(|translator| translator.name())
    }

    /// The work's name list: each character's Japanese name and its spelling.
    pub fn names(&self, work: &str) -> Result<Vec<(String, String)>, TranslationError> {
        self.store.names(work)
    }

    /// Translations already known, without asking the translator. A stale
    /// line counts as unknown, so it is translated again.
    pub fn known(
        &self,
        work: &str,
        texts: &[String],
    ) -> Result<Vec<Option<String>>, TranslationError> {
        let keys = texts.iter().map(|text| text_key(text)).collect::<Vec<_>>();
        let found = self.store.best(work, &keys)?;
        Ok(texts
            .iter()
            .zip(&keys)
            .map(|(text, key)| {
                if has_japanese(text) {
                    found
                        .get(key)
                        .filter(|stored| stored.origin != TranslationOrigin::Stale)
                        .map(|stored| stored.text.clone())
                } else {
                    Some(text.clone())
                }
            })
            .collect())
    }

    /// Marks the lines of `texts` that the player's translator made as stale,
    /// so they are translated again while still shown until then. Lines from
    /// packs and the player's edits stay. Returns how many it marked.
    pub fn mark_stale(&self, work: &str, texts: &[String]) -> Result<u64, TranslationError> {
        let keys = texts
            .iter()
            .filter(|text| has_japanese(text))
            .map(|text| text_key(text))
            .collect::<Vec<_>>();
        self.store.mark_stale(work, &keys)
    }

    pub fn all(&self, work: &str) -> Result<WorkTranslations, TranslationError> {
        Ok(WorkTranslations {
            texts: self.store.all(work)?,
            names: self.store.names(work)?,
        })
    }

    /// Translates the texts the cache does not have yet and stores them.
    /// Each result is `None` when the translator gave nothing usable for it,
    /// even after a second try.
    pub fn translate(
        &self,
        work: &str,
        texts: &[SourceText],
        markup: TextMarkup,
        urgency: Urgency,
    ) -> Result<Vec<Option<String>>, TranslationError> {
        let sources = texts
            .iter()
            .map(|text| text.text.clone())
            .collect::<Vec<_>>();
        let mut results = self.known(work, &sources)?;
        if results.iter().all(Option::is_some) {
            return Ok(results);
        }
        let translator = self.translator().ok_or(TranslationError::NoTranslator)?;
        for batch in self.plan(work, texts, markup)? {
            let done = self.translate_batch(translator.as_ref(), work, &batch, urgency)?;
            for (index, text) in texts.iter().enumerate() {
                if results[index].is_none()
                    && let Some(english) = done.get(text.text.as_str())
                {
                    results[index] = Some(english.clone());
                }
            }
        }
        Ok(results)
    }

    /// The requests that would translate the texts the cache does not have
    /// yet: dialogue in batches of its own, menu words and the database
    /// apart, each told the work's name spellings, its glossary's note and
    /// the glossary terms its texts use.
    pub fn plan(
        &self,
        work: &str,
        texts: &[SourceText],
        markup: TextMarkup,
    ) -> Result<Vec<PlannedBatch>, TranslationError> {
        let sources = texts
            .iter()
            .map(|text| text.text.clone())
            .collect::<Vec<_>>();
        let known = self.known(work, &sources)?;
        let mut seen = HashSet::new();
        let missing = (0..texts.len())
            .filter(|&index| known[index].is_none() && seen.insert(texts[index].text.as_str()))
            .collect::<Vec<_>>();
        if missing.is_empty() {
            return Ok(Vec::new());
        }
        let names = self.store.names(work)?;
        let glossary = self.store.glossary(work)?;
        let mut batches = Vec::new();
        for (style, size) in [
            (BatchStyle::Dialogue, DIALOGUE_BATCH),
            (BatchStyle::Terms, TERM_BATCH),
        ] {
            let group = missing
                .iter()
                .copied()
                .filter(|&index| (style == BatchStyle::Dialogue) == texts[index].kind.is_dialogue())
                .collect::<Vec<_>>();
            for chunk in group.chunks(size) {
                let chunk = chunk.iter().map(|&index| &texts[index]).collect::<Vec<_>>();
                let prepared = chunk
                    .iter()
                    .map(|text| PreparedText::new(&text.text, markup))
                    .collect::<Vec<_>>();
                let terms = glossary
                    .terms
                    .iter()
                    .filter(|(japanese, _)| {
                        chunk.iter().any(|text| uses_term(&text.text, japanese))
                    })
                    .cloned()
                    .collect();
                batches.push(PlannedBatch {
                    style,
                    lines: prepared.iter().map(|text| text.line.clone()).collect(),
                    prepared,
                    readings: self.readings_in(&chunk, &names),
                    texts: chunk.into_iter().cloned().collect(),
                    names: names.clone(),
                    note: glossary.note.clone(),
                    terms,
                });
            }
        }
        Ok(batches)
    }

    /// Keeps the replies to the batch's texts at `positions` that pass the
    /// checks, with the engine's codes put back, and stores them as the
    /// player's translator's lines made with `model`. `lenient` keeps a reply
    /// that fails the checks too: an imperfect line reads better than
    /// Japanese.
    pub fn record(
        &self,
        work: &str,
        batch: &PlannedBatch,
        positions: &[usize],
        replies: &[Option<String>],
        model: &str,
        lenient: bool,
    ) -> Result<RecordedReplies, TranslationError> {
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();
        for (position, &at) in positions.iter().enumerate() {
            let prepared = &batch.prepared[at];
            match replies.get(position).cloned().flatten() {
                Some(reply) if lenient || prepared.accepts(&reply) => {
                    accepted.push((at, prepared.finish(&reply)));
                }
                _ => rejected.push(at),
            }
        }
        let keys = accepted
            .iter()
            .map(|(at, _)| text_key(&batch.texts[*at].text))
            .collect::<Vec<_>>();
        let entries = accepted
            .iter()
            .zip(&keys)
            .map(|((at, english), key)| NewTranslation {
                key,
                source: &batch.texts[*at].text,
                kind: batch.texts[*at].kind,
                text: english,
                origin: TranslationOrigin::Local,
                model: Some(model),
            })
            .collect::<Vec<_>>();
        self.store.save(work, &entries)?;
        Ok(RecordedReplies {
            accepted: accepted
                .into_iter()
                .map(|(at, english)| (batch.texts[at].text.clone(), english))
                .collect(),
            rejected,
        })
    }

    /// Fills the work's name list with any names it does not have yet.
    pub fn ensure_names(
        &self,
        work: &str,
        names: &[String],
        urgency: Urgency,
    ) -> Result<Vec<(String, String)>, TranslationError> {
        let mut known = self.store.names(work)?;
        let have = known
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<HashSet<_>>();
        let mut missing = names
            .iter()
            .filter(|name| has_japanese(name) && !have.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        missing.sort();
        missing.dedup();
        // A name the dictionary reads only one way needs no translator.
        let mut unsure = Vec::new();
        let mut certain = Vec::new();
        for name in missing {
            match self
                .name_readings
                .as_ref()
                .and_then(|names| names.look_up(&name))
            {
                Some(reading) if reading.certain && !reading.spellings.is_empty() => {
                    certain.push((name, reading.spellings[0].clone()));
                }
                reading => unsure.push(NameToSpell {
                    japanese: name,
                    readings: reading.map(|reading| reading.spellings).unwrap_or_default(),
                }),
            }
        }
        if !certain.is_empty() {
            self.store.save_names(work, &certain)?;
            known.extend(certain);
        }
        if unsure.is_empty() {
            return Ok(known);
        }
        let translator = self.translator().ok_or(TranslationError::NoTranslator)?;
        for chunk in unsure.chunks(NAME_BATCH) {
            let spelled = self.take_turn(urgency, || translator.spell_names(chunk))?;
            let added = chunk
                .iter()
                .map(|NameToSpell { japanese: name, .. }| {
                    let english = spelled
                        .get(name)
                        .filter(|english| {
                            !english.trim().is_empty() && !has_leftover_japanese(english)
                        })
                        .map_or_else(|| name.clone(), |english| label_case(english.trim()));
                    (name.clone(), english)
                })
                .collect::<Vec<_>>();
            self.store.save_names(work, &added)?;
            known.extend(added);
        }
        Ok(known)
    }

    /// Names the dictionary finds in the texts that the work's name list
    /// does not spell already.
    fn readings_in(
        &self,
        texts: &[&SourceText],
        names: &[(String, String)],
    ) -> Vec<(String, Vec<String>)> {
        let Some(dictionary) = &self.name_readings else {
            return Vec::new();
        };
        let mut found: Vec<(String, Vec<String>)> = Vec::new();
        for text in texts {
            for (name, reading) in dictionary.find_in(&text.text) {
                let known = names.iter().any(|(japanese, _)| *japanese == name)
                    || found.iter().any(|(seen, _)| *seen == name);
                if !known {
                    found.push((name, reading.spellings));
                }
            }
        }
        found
    }

    fn translator(&self) -> Option<Arc<dyn Translator>> {
        self.translator
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sends the batch, then once more the lines whose reply was missing or
    /// failed the checks; the second reply is kept even if it fails them.
    fn translate_batch(
        &self,
        translator: &dyn Translator,
        work: &str,
        batch: &PlannedBatch,
        urgency: Urgency,
    ) -> Result<HashMap<String, String>, TranslationError> {
        let model = translator.name();
        let mut accepted = HashMap::new();
        let mut pending = (0..batch.texts.len()).collect::<Vec<_>>();
        for attempt in 0..2 {
            if pending.is_empty() {
                break;
            }
            let lines = pending
                .iter()
                .map(|&at| batch.lines[at].clone())
                .collect::<Vec<_>>();
            let replies =
                self.take_turn(urgency, || translator.translate(&batch.request_for(&lines)))?;
            let recorded = self.record(work, batch, &pending, &replies, &model, attempt == 1)?;
            accepted.extend(recorded.accepted);
            pending = recorded.rejected;
        }
        Ok(accepted)
    }

    fn take_turn<T>(&self, urgency: Urgency, request: impl FnOnce() -> T) -> T {
        match urgency {
            Urgency::Now => {
                self.urgent_waiting.fetch_add(1, Ordering::SeqCst);
                let turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
                self.urgent_waiting.fetch_sub(1, Ordering::SeqCst);
                let result = request();
                drop(turn);
                result
            }
            Urgency::Background => loop {
                while self.urgent_waiting.load(Ordering::SeqCst) > 0 {
                    thread::sleep(URGENT_POLL);
                }
                let turn = self.turn.lock().unwrap_or_else(PoisonError::into_inner);
                if self.urgent_waiting.load(Ordering::SeqCst) == 0 {
                    let result = request();
                    drop(turn);
                    return result;
                }
            },
        }
    }
}

/// Speaker labels keep their small words lower case, as English releases
/// write them: "Lady of the Village", not "Lady Of The Village".
fn label_case(name: &str) -> String {
    const SMALL_WORDS: [&str; 11] = [
        "a", "an", "and", "at", "for", "from", "in", "of", "on", "the", "to",
    ];
    name.split(' ')
        .enumerate()
        .map(|(at, word)| {
            let lower = word.to_lowercase();
            if at > 0 && SMALL_WORDS.contains(&lower.as_str()) {
                lower
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MemoryStore {
        translations: Mutex<HashMap<(String, String), StoredTranslation>>,
        sources: Mutex<HashMap<(String, String), String>>,
        names: Mutex<Vec<(String, String)>>,
        glossary: Mutex<Glossary>,
    }

    impl TranslationStore for MemoryStore {
        fn best(
            &self,
            work: &str,
            keys: &[String],
        ) -> Result<HashMap<String, StoredTranslation>, TranslationError> {
            let translations = self.translations.lock().unwrap();
            Ok(keys
                .iter()
                .filter_map(|key| {
                    translations
                        .get(&(work.to_owned(), key.clone()))
                        .map(|stored| (key.clone(), stored.clone()))
                })
                .collect())
        }

        fn save(
            &self,
            work: &str,
            translations: &[NewTranslation<'_>],
        ) -> Result<(), TranslationError> {
            let mut stored = self.translations.lock().unwrap();
            let mut sources = self.sources.lock().unwrap();
            for translation in translations {
                sources.insert(
                    (work.to_owned(), translation.key.to_owned()),
                    translation.source.to_owned(),
                );
                stored.insert(
                    (work.to_owned(), translation.key.to_owned()),
                    StoredTranslation {
                        text: translation.text.to_owned(),
                        origin: translation.origin,
                    },
                );
            }
            Ok(())
        }

        fn mark_stale(&self, work: &str, keys: &[String]) -> Result<u64, TranslationError> {
            let mut stored = self.translations.lock().unwrap();
            let mut marked = 0;
            for key in keys {
                if let Some(translation) = stored.get_mut(&(work.to_owned(), key.clone()))
                    && translation.origin == TranslationOrigin::Local
                {
                    translation.origin = TranslationOrigin::Stale;
                    marked += 1;
                }
            }
            Ok(marked)
        }

        fn names(&self, _work: &str) -> Result<Vec<(String, String)>, TranslationError> {
            Ok(self.names.lock().unwrap().clone())
        }

        fn glossary(&self, _work: &str) -> Result<Glossary, TranslationError> {
            Ok(self.glossary.lock().unwrap().clone())
        }

        fn save_glossary(&self, _work: &str, glossary: &Glossary) -> Result<(), TranslationError> {
            *self.glossary.lock().unwrap() = glossary.clone();
            Ok(())
        }

        fn save_names(
            &self,
            _work: &str,
            names: &[(String, String)],
        ) -> Result<(), TranslationError> {
            self.names.lock().unwrap().extend_from_slice(names);
            Ok(())
        }

        fn save_pack(
            &self,
            work: &str,
            _pack: &str,
            translations: &[NewTranslation<'_>],
        ) -> Result<(), TranslationError> {
            self.save(work, translations)
        }

        fn all(&self, work: &str) -> Result<Vec<(String, String)>, TranslationError> {
            let sources = self.sources.lock().unwrap();
            let mut all = self
                .translations
                .lock()
                .unwrap()
                .iter()
                .filter(|((stored_work, _), _)| stored_work == work)
                .filter_map(|(entry, stored)| {
                    sources
                        .get(entry)
                        .map(|source| (source.clone(), stored.text.clone()))
                })
                .collect::<Vec<_>>();
            all.sort();
            Ok(all)
        }
    }

    type Reply = Box<dyn Fn(&TranslatorBatch<'_>) -> Vec<Option<String>> + Send + Sync>;
    type Readings = Vec<(String, Vec<String>)>;

    struct ScriptedTranslator {
        reply: Reply,
        batches: Mutex<Vec<(BatchStyle, Vec<String>, usize)>>,
        readings: Mutex<Vec<Readings>>,
        spelled: Mutex<Vec<NameToSpell>>,
    }

    impl ScriptedTranslator {
        fn new(
            reply: impl Fn(&TranslatorBatch<'_>) -> Vec<Option<String>> + Send + Sync + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                reply: Box::new(reply),
                batches: Mutex::new(Vec::new()),
                readings: Mutex::new(Vec::new()),
                spelled: Mutex::new(Vec::new()),
            })
        }

        fn batches(&self) -> Vec<(BatchStyle, Vec<String>, usize)> {
            self.batches.lock().unwrap().clone()
        }
    }

    impl Translator for ScriptedTranslator {
        fn name(&self) -> String {
            "test-model".to_owned()
        }

        fn translate(
            &self,
            batch: &TranslatorBatch<'_>,
        ) -> Result<Vec<Option<String>>, TranslatorError> {
            self.batches.lock().unwrap().push((
                batch.style,
                batch.lines.to_vec(),
                batch.names.len(),
            ));
            self.readings.lock().unwrap().push(batch.readings.to_vec());
            Ok((self.reply)(batch))
        }

        fn spell_names(
            &self,
            names: &[NameToSpell],
        ) -> Result<HashMap<String, String>, TranslatorError> {
            self.spelled.lock().unwrap().extend(names.iter().cloned());
            // Takes the dictionary's first reading, as told to.
            Ok(names
                .iter()
                .map(|name| {
                    (
                        name.japanese.clone(),
                        if name.japanese == "ユキナ" {
                            "Yukina".to_owned()
                        } else {
                            name.readings.first().cloned().unwrap_or_default()
                        },
                    )
                })
                .collect())
        }
    }

    fn service_with(translator: Arc<ScriptedTranslator>) -> (TranslationService, Arc<MemoryStore>) {
        let store = Arc::new(MemoryStore::default());
        let service = TranslationService::new(store.clone());
        service.set_translator(Some(translator));
        (service, store)
    }

    fn text(text: &str, kind: TextKind) -> SourceText {
        SourceText {
            text: text.to_owned(),
            kind,
        }
    }

    fn english(batch: &TranslatorBatch<'_>) -> Vec<Option<String>> {
        batch
            .lines
            .iter()
            .map(|line| Some(format!("EN[{}]", line.replace(|c: char| !c.is_ascii(), ""))))
            .collect()
    }

    #[test]
    fn translates_dialogue_and_terms_in_separate_batches_with_codes_restored() {
        let translator = ScriptedTranslator::new(english);
        let (service, _) = service_with(translator.clone());
        let texts = [
            text("特に\\C[2]妖魔\\c[0]を狩る", TextKind::Dialogue),
            text("アイテム", TextKind::Menu),
            text("はい", TextKind::Choice),
        ];
        let results = service
            .translate("RJ1", &texts, TextMarkup::RpgMaker, Urgency::Now)
            .unwrap();
        assert_eq!(
            results,
            vec![
                Some(
                    "EN[{1}{2}]"
                        .replace("{1}", "\\C[2]")
                        .replace("{2}", "\\c[0]")
                ),
                Some("EN[]".to_owned()),
                Some("EN[]".to_owned()),
            ]
        );
        let batches = translator.batches();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].0, BatchStyle::Dialogue);
        assert_eq!(
            batches[0].1,
            vec!["特に{1}妖魔{2}を狩る".to_owned(), "はい".to_owned()]
        );
        assert_eq!(batches[1].0, BatchStyle::Terms);
    }

    #[test]
    fn tells_each_batch_the_glossary_terms_its_texts_use() {
        let (service, store) = service_with(ScriptedTranslator::new(english));
        let note = "A parody of Ragnarok Online.";
        let term = |japanese: &str, english: &str| (japanese.to_owned(), english.to_owned());
        *store.glossary.lock().unwrap() = Glossary {
            note: Some(note.to_owned()),
            terms: vec![term("プロンテラ", "Prontera"), term("ポリン", "Poring")],
        };
        let texts = [
            text("プロンテラへ行こう", TextKind::Dialogue),
            text("ポリンカード", TextKind::Database),
            text("はい", TextKind::Choice),
        ];
        let batches = service.plan("RJ1", &texts, TextMarkup::RpgMaker).unwrap();
        let told = batches
            .iter()
            .map(|batch| (batch.request().note, batch.request().terms.to_vec()))
            .collect::<Vec<_>>();
        assert_eq!(
            told,
            vec![
                (Some(note), vec![term("プロンテラ", "Prontera")]),
                (Some(note), vec![term("ポリン", "Poring")]),
            ]
        );
    }

    #[test]
    fn answers_from_the_cache_and_passes_english_through() {
        let translator = ScriptedTranslator::new(english);
        let (service, _) = service_with(translator.clone());
        let texts = [text("はい", TextKind::Choice), text("OK", TextKind::Choice)];
        service
            .translate("RJ1", &texts, TextMarkup::RpgMaker, Urgency::Now)
            .unwrap();
        let again = service
            .translate("RJ1", &texts, TextMarkup::RpgMaker, Urgency::Now)
            .unwrap();
        assert_eq!(again, vec![Some("EN[]".to_owned()), Some("OK".to_owned())]);
        assert_eq!(translator.batches().len(), 1);
        assert_eq!(
            service.all("RJ1").unwrap().texts,
            vec![("はい".to_owned(), "EN[]".to_owned())]
        );
    }

    #[test]
    fn asks_again_once_for_a_reply_that_fails_the_checks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let translator = ScriptedTranslator::new(move |batch| {
            let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
            batch
                .lines
                .iter()
                .map(|_| {
                    Some(
                        if first {
                            "Sports book (10及)"
                        } else {
                            "Sports book (100)"
                        }
                        .to_owned(),
                    )
                })
                .collect()
        });
        let (service, _) = service_with(translator.clone());
        let results = service
            .translate(
                "RJ1",
                &[text("スポーツの本（100）", TextKind::Choice)],
                TextMarkup::RpgMaker,
                Urgency::Now,
            )
            .unwrap();
        assert_eq!(results, vec![Some("Sports book (100)".to_owned())]);
        assert_eq!(translator.batches().len(), 2);
    }

    #[test]
    fn keeps_a_second_imperfect_reply_but_never_stores_a_missing_one() {
        let translator = ScriptedTranslator::new(|batch| {
            batch
                .lines
                .iter()
                .map(|line| line.contains("本").then(|| "A book (3)".to_owned()))
                .collect()
        });
        let (service, store) = service_with(translator);
        let results = service
            .translate(
                "RJ1",
                &[
                    text("本（三冊）", TextKind::Database),
                    text("剣", TextKind::Database),
                ],
                TextMarkup::RpgMaker,
                Urgency::Background,
            )
            .unwrap();
        assert_eq!(results, vec![Some("A book (3)".to_owned()), None]);
        assert_eq!(store.translations.lock().unwrap().len(), 1);
    }

    #[test]
    fn translates_a_repeated_text_once() {
        let translator = ScriptedTranslator::new(english);
        let (service, _) = service_with(translator.clone());
        let texts = [
            text("はい", TextKind::Choice),
            text("はい", TextKind::Choice),
        ];
        let results = service
            .translate("RJ1", &texts, TextMarkup::RpgMaker, Urgency::Now)
            .unwrap();
        assert_eq!(
            results,
            vec![Some("EN[]".to_owned()), Some("EN[]".to_owned())]
        );
        assert_eq!(translator.batches()[0].1.len(), 1);
    }

    #[test]
    fn needs_a_translator_only_for_texts_it_does_not_know() {
        let store = Arc::new(MemoryStore::default());
        let service = TranslationService::new(store);
        assert!(matches!(
            service.translate(
                "RJ1",
                &[text("はい", TextKind::Choice)],
                TextMarkup::RpgMaker,
                Urgency::Now
            ),
            Err(TranslationError::NoTranslator)
        ));
        assert_eq!(
            service
                .translate(
                    "RJ1",
                    &[text("OK", TextKind::Choice)],
                    TextMarkup::RpgMaker,
                    Urgency::Now
                )
                .unwrap(),
            vec![Some("OK".to_owned())]
        );
    }

    #[test]
    fn a_pack_wins_over_the_translator_and_keeps_lines_the_game_files_lack() {
        let translator = ScriptedTranslator::new(english);
        let (service, store) = service_with(translator.clone());
        service
            .translate(
                "RJ1",
                &[text("はい", TextKind::Choice)],
                TextMarkup::RpgMaker,
                Urgency::Now,
            )
            .unwrap();
        let pack = TranslationPack {
            work: "RJ1".to_owned(),
            made: "2026-10-30".to_owned(),
            texts: vec![
                PackLine {
                    key: text_key("はい"),
                    text: "Sure".to_owned(),
                    reviewed: true,
                    source: None,
                },
                PackLine {
                    key: text_key("右上のヒント"),
                    text: "Hint".to_owned(),
                    reviewed: false,
                    source: None,
                },
                // The game's own English, reworded: it comes with the pack.
                PackLine {
                    key: text_key("MOBDICTIONARY"),
                    text: "BESTIARY".to_owned(),
                    reviewed: true,
                    source: Some("MOBDICTIONARY".to_owned()),
                },
            ],
            names: vec![PackLine {
                key: text_key("ユキナ"),
                text: "Yukina".to_owned(),
                reviewed: true,
                source: None,
            }],
            glossary: Some(Glossary {
                note: Some("A parody of Ragnarok Online.".to_owned()),
                terms: vec![("ポリン".to_owned(), "Poring".to_owned())],
            }),
        };
        let shown = service
            .import_pack(
                &pack,
                &[text("はい", TextKind::Choice)],
                &["ユキナ".to_owned()],
            )
            .unwrap();
        assert_eq!(shown, 1);
        let stored = store.translations.lock().unwrap();
        assert_eq!(
            stored[&("RJ1".to_owned(), text_key("はい"))].origin,
            TranslationOrigin::PackReviewed
        );
        drop(stored);
        assert_eq!(
            service.known("RJ1", &["右上のヒント".to_owned()]).unwrap(),
            vec![Some("Hint".to_owned())]
        );
        assert_eq!(
            service.names("RJ1").unwrap(),
            vec![("ユキナ".to_owned(), "Yukina".to_owned())]
        );
        assert!(
            service
                .all("RJ1")
                .unwrap()
                .texts
                .contains(&("MOBDICTIONARY".to_owned(), "BESTIARY".to_owned()))
        );
        assert_eq!(store.glossary.lock().unwrap().terms.len(), 1);
    }

    #[test]
    fn writes_speaker_labels_the_way_english_releases_do() {
        assert_eq!(label_case("Lady Of The Village"), "Lady of the Village");
        assert_eq!(label_case("Old Man From Village"), "Old Man from Village");
        assert_eq!(label_case("The Drunkard"), "The Drunkard");
        assert_eq!(label_case("Yukina"), "Yukina");
    }

    #[test]
    fn spells_names_once_and_hands_them_to_later_batches() {
        let translator = ScriptedTranslator::new(english);
        let (service, store) = service_with(translator.clone());
        let names = service
            .ensure_names(
                "RJ1",
                &[
                    "ユキナ".to_owned(),
                    "謎の男".to_owned(),
                    "ユキナ".to_owned(),
                ],
                Urgency::Now,
            )
            .unwrap();
        assert_eq!(
            names,
            vec![
                ("ユキナ".to_owned(), "Yukina".to_owned()),
                ("謎の男".to_owned(), "謎の男".to_owned())
            ]
        );
        service
            .ensure_names("RJ1", &["ユキナ".to_owned()], Urgency::Now)
            .unwrap();
        assert_eq!(store.names.lock().unwrap().len(), 2);
        service
            .translate(
                "RJ1",
                &[text("こんにちは", TextKind::Dialogue)],
                TextMarkup::RpgMaker,
                Urgency::Now,
            )
            .unwrap();
        assert_eq!(translator.batches()[0].2, 2);
    }

    /// A name dictionary that knows a few names.
    struct Dictionary;

    impl NameReadings for Dictionary {
        fn look_up(&self, name: &str) -> Option<NameReading> {
            let reading = |spellings: &[&str], certain| NameReading {
                spellings: spellings
                    .iter()
                    .map(|&spelling| spelling.to_owned())
                    .collect(),
                certain,
            };
            match name {
                "翔太" => Some(reading(&["Shouta"], true)),
                "由香" => Some(reading(&["Yuka", "Yukari"], false)),
                _ => None,
            }
        }

        fn find_in(&self, line: &str) -> Vec<(String, NameReading)> {
            ["小野塚ゆか", "翔太"]
                .into_iter()
                .filter(|name| line.contains(name))
                .map(|name| {
                    (
                        name.to_owned(),
                        NameReading {
                            spellings: vec![format!("{name} in English")],
                            certain: false,
                        },
                    )
                })
                .collect()
        }
    }

    #[test]
    fn spells_names_the_dictionary_is_sure_of_and_gives_the_translator_the_readings() {
        let translator = ScriptedTranslator::new(english);
        let store = Arc::new(MemoryStore::default());
        let service =
            TranslationService::new(store.clone()).with_name_readings(Arc::new(Dictionary));
        service.set_translator(Some(translator.clone()));

        let names = service
            .ensure_names(
                "RJ1",
                &["翔太".to_owned(), "由香".to_owned(), "謎の男".to_owned()],
                Urgency::Now,
            )
            .unwrap();
        assert_eq!(
            names,
            [("翔太", "Shouta"), ("由香", "Yuka"), ("謎の男", "謎の男")]
                .map(|(japanese, english)| (japanese.to_owned(), english.to_owned()))
        );
        // Only the names it is unsure of go to the translator, with readings.
        assert_eq!(
            *translator.spelled.lock().unwrap(),
            [
                NameToSpell {
                    japanese: "由香".to_owned(),
                    readings: vec!["Yuka".to_owned(), "Yukari".to_owned()],
                },
                NameToSpell {
                    japanese: "謎の男".to_owned(),
                    readings: Vec::new(),
                },
            ]
        );

        // Names in a line go along with their readings, but not one the
        // work's name list spells already.
        service
            .translate(
                "RJ1",
                &[text("小野塚ゆかと翔太が来た。", TextKind::Dialogue)],
                TextMarkup::RpgMaker,
                Urgency::Now,
            )
            .unwrap();
        assert_eq!(
            translator.readings.lock().unwrap()[0],
            [(
                "小野塚ゆか".to_owned(),
                vec!["小野塚ゆか in English".to_owned()]
            )]
        );
    }
}
