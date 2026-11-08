//! Translation packs fetched by exact paths from the read-only overlay service.
//! `index.json` publishes available game versions; no directory listing is used.

use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, ErrorKind, Read},
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use dla_application::translation::{
    Glossary, PackLine, PackLookup, PackOffer, TranslationError, TranslationPack, TranslationPacks,
    has_japanese,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

/// Every work's overlays: translations now, file fixes later.
pub const OVERLAYS: &str = "/overlays/dlsite";
/// The list of works with a pack.
pub const INDEX: &str = "/overlays/dlsite/index.json";
const LANGUAGE: &str = "en";
const FORMAT: u32 = 2;
const MAX_SMALL_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ZIP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_PACK_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LINE_CHARS: usize = 4_000;
// The list changes when we publish; reading it once an hour is plenty.
const INDEX_LIFETIME: Duration = Duration::from_secs(60 * 60);
const MANIFEST_FILE: &str = "manifest.json";
const TEXTS_FILE: &str = "texts.jsonl";
const NAMES_FILE: &str = "names.jsonl";
const GLOSSARY_FILE: &str = "glossary.json";

/// Why a file in the archive could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileError {
    NotFound,
    /// The account may not read it.
    Forbidden,
    /// Nobody is signed in to DLA.
    SignedOut,
    Other(String),
}

/// The DLA archive as the signed-in account may read it, or a folder laid
/// out the same way on this PC while testing.
pub trait ArchiveFiles: Send + Sync {
    /// A file whole, refused when longer than `limit`.
    fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, FileError>;
}

/// A folder standing for `/overlays/dlsite`, as `DLA_TRANSLATION_PACKS` names one.
pub struct FolderFiles {
    root: PathBuf,
}

impl FolderFiles {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, path: &str) -> Result<PathBuf, FileError> {
        let relative = path
            .strip_prefix("/overlays/dlsite/")
            .filter(|relative| {
                relative.split('/').all(|part| {
                    !part.is_empty() && !part.starts_with('.') && !part.contains(['\\', '%', '\0'])
                })
            })
            .ok_or(FileError::NotFound)?;
        Ok(self.root.join(relative))
    }
}

impl ArchiveFiles for FolderFiles {
    fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, FileError> {
        let file = fs::File::open(self.path(path)?).map_err(file_error)?;
        let mut bytes = Vec::new();
        file.take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(file_error)?;
        if bytes.len() as u64 > limit {
            return Err(FileError::Other(format!("{path} is larger than expected")));
        }
        Ok(bytes)
    }
}

fn file_error(error: std::io::Error) -> FileError {
    match error.kind() {
        ErrorKind::NotFound => FileError::NotFound,
        ErrorKind::PermissionDenied => FileError::Forbidden,
        _ => FileError::Other(error.to_string()),
    }
}

pub struct ArchivePacks {
    files: Arc<dyn ArchiveFiles>,
    index: Mutex<Option<(Instant, BTreeMap<String, String>)>>,
}

impl ArchivePacks {
    pub fn new(files: Arc<dyn ArchiveFiles>) -> Self {
        Self {
            files,
            index: Mutex::new(None),
        }
    }

    fn manifest(&self, folder: &str) -> Result<Manifest, FileError> {
        let bytes = self
            .files
            .read(&format!("{folder}/{MANIFEST_FILE}"), MAX_SMALL_BYTES)?;
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|error| FileError::Other(format!("{MANIFEST_FILE}: {error}")))?;
        if manifest.format != FORMAT || manifest.language != LANGUAGE {
            return Err(FileError::Other(
                "the pack is for another version of the launcher".to_owned(),
            ));
        }
        Ok(manifest)
    }

    /// The pack's zip, checked against `manifest.json`.
    fn zip(&self, folder: &str, manifest: &Manifest) -> Result<Vec<u8>, TranslationError> {
        let listed = &manifest.file;
        let name = listed.name.as_str();
        let plain = !name.is_empty()
            && name.ends_with(".zip")
            && !name.contains(['/', '\\'])
            && !name.starts_with('.');
        if !plain || listed.bytes > MAX_ZIP_BYTES {
            return Err(pack_error(format!(
                "{MANIFEST_FILE} does not name a pack zip"
            )));
        }
        let bytes = self
            .files
            .read(&format!("{folder}/{name}"), listed.bytes)
            .map_err(|error| pack_error(format!("{name}: {}", describe(&error))))?;
        if bytes.len() as u64 != listed.bytes
            || hex::encode(Sha256::digest(&bytes)) != listed.sha256.to_ascii_lowercase()
        {
            return Err(pack_error(format!(
                "{name} does not match {MANIFEST_FILE}; it may still be uploading"
            )));
        }
        Ok(bytes)
    }
}

impl TranslationPacks for ArchivePacks {
    fn find(&self, work: &str, version: Option<&str>) -> Result<PackLookup, TranslationError> {
        if !safe_work(work) {
            return Ok(PackLookup::None);
        }
        let translations = format!("{OVERLAYS}/{work}/translations");
        let index = match self.files.read(INDEX, MAX_SMALL_BYTES) {
            Ok(bytes) => parse_index(&bytes)?,
            Err(error) => return lookup_failure(error),
        };
        let mut folders = index
            .versions
            .as_ref()
            .and_then(|versions| versions.get(work))
            .and_then(|languages| languages.get(LANGUAGE))
            .cloned()
            .unwrap_or_default();
        // An old format-1 index lacks versions. A known installed version can
        // still be requested directly while the publisher rebuilds the index.
        if index.versions.is_none()
            && let Some(version) = version.filter(|value| is_day(value))
        {
            folders.push(version.to_owned());
        }
        for chosen in in_turn(folders, version) {
            let folder = format!("{translations}/{chosen}/{LANGUAGE}");
            let manifest = match self.manifest(&folder) {
                Ok(manifest) => manifest,
                // A version still uploading, or without this language: the
                // next one may do.
                Err(FileError::NotFound) => continue,
                Err(error) => return lookup_failure(error),
            };
            if manifest.work != work || manifest.version != chosen {
                return Err(pack_error(format!(
                    "the pack in {chosen} is for {} {}",
                    manifest.work, manifest.version
                )));
            }
            return Ok(PackLookup::Offer(PackOffer {
                work: work.to_owned(),
                version: chosen,
                folder,
                lines: manifest.lines,
                made: manifest.made,
            }));
        }
        Ok(PackLookup::None)
    }

    fn download(&self, offer: &PackOffer) -> Result<TranslationPack, TranslationError> {
        let manifest = self
            .manifest(&offer.folder)
            .map_err(|error| pack_error(describe(&error)))?;
        if manifest.work != offer.work {
            return Err(pack_error(format!("the pack is for {}", manifest.work)));
        }
        let zip = self.zip(&offer.folder, &manifest)?;
        let mut zip = ZipArchive::new(Cursor::new(zip))
            .map_err(|error| pack_error(format!("{}: {error}", manifest.file.name)))?;
        let glossary = match entry(&mut zip, GLOSSARY_FILE) {
            Ok(bytes) => Some(read_glossary(&bytes)?),
            Err(None) => None,
            Err(Some(error)) => return Err(error),
        };
        let mut required = |name: &str| {
            entry(&mut zip, name)
                .map_err(|error| error.unwrap_or_else(|| pack_error(format!("no {name} in it"))))
        };
        let texts = lines(&required(TEXTS_FILE)?, TEXTS_FILE)?;
        let names = lines(&required(NAMES_FILE)?, NAMES_FILE)?;
        Ok(TranslationPack {
            work: manifest.work,
            // A better pack may have replaced the offered one since.
            made: manifest.made,
            texts,
            names,
            glossary,
        })
    }

    fn translated_works(&self) -> Result<BTreeMap<String, String>, TranslationError> {
        let mut cached = self.index.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((read_at, works)) = cached.as_ref()
            && read_at.elapsed() < INDEX_LIFETIME
        {
            return Ok(works.clone());
        }
        let works = match self.files.read(INDEX, MAX_SMALL_BYTES) {
            Ok(bytes) => read_index(&bytes)?,
            // Not signed in yet, nothing published yet, or not for this
            // account: nothing to mark, and only a list read is kept, so one
            // published since shows the next time the library asks.
            Err(FileError::SignedOut | FileError::NotFound | FileError::Forbidden) => {
                return Ok(BTreeMap::new());
            }
            Err(FileError::Other(message)) => return Err(pack_error(message)),
        };
        *cached = Some((Instant::now(), works.clone()));
        Ok(works)
    }
}

fn lookup_failure(error: FileError) -> Result<PackLookup, TranslationError> {
    match error {
        FileError::NotFound => Ok(PackLookup::None),
        FileError::Forbidden => Ok(PackLookup::ForElite),
        FileError::SignedOut => Ok(PackLookup::SignedOut),
        FileError::Other(message) => Err(pack_error(message)),
    }
}

fn describe(error: &FileError) -> String {
    match error {
        FileError::NotFound => "not in the archive".to_owned(),
        FileError::Forbidden => "this account may not download it".to_owned(),
        FileError::SignedOut => "sign in to DLA first".to_owned(),
        FileError::Other(message) => message.clone(),
    }
}

/// A work code as the archive names folders (`RJ112559`, `VJ01006892`).
fn safe_work(work: &str) -> bool {
    !work.is_empty() && work.len() <= 32 && work.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// The versions to try in turn: `version` first when the archive has it,
/// then the newest; only folders named by a day (`2013-08-25`) are versions.
fn in_turn(folders: Vec<String>, version: Option<&str>) -> Vec<String> {
    let mut versions: Vec<String> = folders.into_iter().filter(|name| is_day(name)).collect();
    versions.sort_unstable_by(|one, other| {
        let installed = |name: &String| Some(name.as_str()) == version;
        installed(other)
            .cmp(&installed(one))
            .then_with(|| other.cmp(one))
    });
    versions
}

fn is_day(text: &str) -> bool {
    text.len() == 10
        && text.bytes().enumerate().all(|(at, byte)| {
            if at == 4 || at == 7 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

/// A file of the zip whole, `Err(None)` when it has none by that name; its
/// checksum is checked as it is read.
fn entry(
    zip: &mut ZipArchive<Cursor<Vec<u8>>>,
    name: &str,
) -> Result<Vec<u8>, Option<TranslationError>> {
    let file = match zip.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Err(None),
        Err(error) => return Err(Some(pack_error(format!("{name}: {error}")))),
    };
    if file.size() > MAX_PACK_FILE_BYTES {
        return Err(Some(pack_error(format!("{name} is larger than expected"))));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PACK_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| Some(pack_error(format!("{name}: {error}"))))?;
    Ok(bytes)
}

#[derive(Deserialize)]
struct Manifest {
    format: u32,
    work: String,
    /// The day the game's version came out, as its folder is named.
    version: String,
    language: String,
    /// When the translation was made, in UTC (`2026-11-03T12:00:00Z`).
    made: String,
    #[serde(default)]
    lines: u64,
    file: ListedFile,
}

#[derive(Deserialize)]
struct ListedFile {
    name: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct Line {
    key: String,
    text: String,
    #[serde(default)]
    reviewed: bool,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize)]
struct GlossaryFile {
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    terms: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct IndexFile {
    format: u32,
    #[serde(default)]
    works: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    versions: Option<BTreeMap<String, BTreeMap<String, Vec<String>>>>,
}

/// Each work with a pack in the launcher's language, and when its newest
/// translation was made.
fn parse_index(bytes: &[u8]) -> Result<IndexFile, TranslationError> {
    let index: IndexFile = serde_json::from_slice(bytes)
        .map_err(|error| pack_error(format!("index.json: {error}")))?;
    if index.format != 1 {
        return Err(pack_error(
            "the list of packs is for another version of the launcher",
        ));
    }
    Ok(index)
}

fn read_index(bytes: &[u8]) -> Result<BTreeMap<String, String>, TranslationError> {
    Ok(parse_index(bytes)?
        .works
        .into_iter()
        .filter(|(work, _)| safe_work(work))
        .filter_map(|(work, languages)| {
            languages
                .get(LANGUAGE)
                .filter(|made| made.get(..10).is_some_and(is_day))
                .map(|made| (work, made.clone()))
        })
        .collect())
}

fn read_glossary(bytes: &[u8]) -> Result<Glossary, TranslationError> {
    let file: GlossaryFile = serde_json::from_slice(bytes)
        .map_err(|error| pack_error(format!("{GLOSSARY_FILE}: {error}")))?;
    Ok(Glossary {
        note: file.note.filter(|note| !note.trim().is_empty()),
        terms: file
            .terms
            .into_iter()
            .filter(|(japanese, english)| !japanese.trim().is_empty() && !english.trim().is_empty())
            .collect(),
    })
}

fn lines(bytes: &[u8], name: &str) -> Result<Vec<PackLine>, TranslationError> {
    let text = std::str::from_utf8(bytes).map_err(|_| pack_error(format!("{name} is not text")))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(at, line)| {
            let line: Line = serde_json::from_str(line)
                .map_err(|error| pack_error(format!("{name} line {}: {error}", at + 1)))?;
            let key_ok = line.key.len() == 32
                && line
                    .key
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
            // A pack holds no Japanese: only a text without any comes with it.
            let source_ok = line
                .source
                .as_deref()
                .is_none_or(|source| !has_japanese(source));
            if !key_ok
                || !source_ok
                || line.text.trim().is_empty()
                || line.text.chars().count() > MAX_LINE_CHARS
            {
                return Err(pack_error(format!("{name} line {} is not valid", at + 1)));
            }
            Ok(PackLine {
                key: line.key,
                text: line.text,
                reviewed: line.reviewed,
                source: line.source,
            })
        })
        .collect()
}

fn pack_error(message: impl Into<String>) -> TranslationError {
    TranslationError::Pack(message.into())
}

#[cfg(test)]
mod tests {
    use dla_application::translation::text_key;
    use serde_json::json;

    use super::*;

    /// Writes a pack the way the publishing tool does, into a folder standing
    /// for `/overlays/dlsite`.
    fn publish(root: &std::path::Path, work: &str, version: &str, made: &str, texts: &str) {
        let folder = root.join(format!("{work}/translations/{version}/en"));
        fs::create_dir_all(&folder).unwrap();
        let names = format!("{}\n", json!({"key": text_key("ユキナ"), "text": "Yukina"}));
        let glossary =
            json!({"note": "A parody of Ragnarok Online.", "terms": [["ポリン", "Poring"]]})
                .to_string();
        let name = format!("{work}-en-{version}.zip");
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (entry, content) in [
            (TEXTS_FILE, texts),
            (NAMES_FILE, &names),
            (GLOSSARY_FILE, &glossary),
        ] {
            zip.start_file(entry, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, content.as_bytes()).unwrap();
        }
        let zip = zip.finish().unwrap().into_inner();
        fs::write(folder.join(&name), &zip).unwrap();
        let manifest = json!({"format": 2, "work": work, "version": version, "language": "en",
            "made": made, "lines": texts.lines().count(),
            "file": {"name": name, "bytes": zip.len(), "sha256": hex::encode(Sha256::digest(&zip))}});
        fs::write(folder.join(MANIFEST_FILE), manifest.to_string()).unwrap();
        publish_version(root, work, version, made);
    }

    fn publish_version(root: &std::path::Path, work: &str, version: &str, made: &str) {
        let file = root.join("index.json");
        let mut index: serde_json::Value = fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| json!({"format": 1, "works": {}, "versions": {}}));
        index["works"][work]["en"] = json!(made);
        let mut versions: Vec<String> = index["versions"][work]["en"]
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
        index["versions"][work]["en"] = json!(versions);
        fs::write(file, index.to_string()).unwrap();
    }

    fn line(japanese: &str, english: &str) -> String {
        format!("{}\n", json!({"key": text_key(japanese), "text": english}))
    }

    fn offer(packs: &ArchivePacks, work: &str, version: Option<&str>) -> PackOffer {
        match packs.find(work, version).unwrap() {
            PackLookup::Offer(offer) => offer,
            other => panic!("an offer, not {other:?}"),
        }
    }

    #[test]
    fn offers_the_installed_versions_pack_or_else_the_newest_versions() {
        let root = tempfile::tempdir().unwrap();
        publish(
            root.path(),
            "RJ1",
            "2013-02-01",
            "2026-11-01T10:00:00Z",
            &line("あ", "Old"),
        );
        let reworded = format!(
            "{}\n",
            json!({"key": text_key("MOBDICTIONARY"), "text": "BESTIARY", "reviewed": true,
                "source": "MOBDICTIONARY"})
        );
        publish(
            root.path(),
            "RJ1",
            "2013-08-25",
            "2026-11-03T12:00:00Z",
            &(line("あ", "Ah") + &reworded),
        );
        fs::create_dir_all(root.path().join("RJ1/translations/drafts")).unwrap();
        // A newer version still uploading: no manifest yet.
        fs::create_dir_all(root.path().join("RJ1/translations/2014-06-01/en")).unwrap();
        let packs = ArchivePacks::new(Arc::new(FolderFiles::new(root.path().to_owned())));

        let newest = offer(&packs, "RJ1", None);
        assert_eq!(
            (newest.version.as_str(), newest.made.as_str(), newest.lines),
            ("2013-08-25", "2026-11-03T12:00:00Z", 2)
        );
        assert_eq!(
            offer(&packs, "RJ1", Some("2013-02-01")).version,
            "2013-02-01"
        );
        // A version without a pack of its own gets the newest version's.
        assert_eq!(
            offer(&packs, "RJ1", Some("2014-01-01")).version,
            "2013-08-25"
        );
        assert_eq!(packs.find("RJ2", None).unwrap(), PackLookup::None);
        assert_eq!(packs.find("../RJ1", None).unwrap(), PackLookup::None);

        let pack = packs.download(&newest).unwrap();
        assert_eq!(pack.made, "2026-11-03T12:00:00Z");
        assert_eq!(pack.texts[0].text, "Ah");
        assert_eq!(pack.texts[1].source.as_deref(), Some("MOBDICTIONARY"));
        assert_eq!(pack.names[0].text, "Yukina");
        assert_eq!(
            pack.glossary.unwrap().terms,
            [("ポリン".to_owned(), "Poring".to_owned())]
        );

        // A better translation of the same version replaces the pack in its
        // folder; what was offered before downloads the new one.
        publish(
            root.path(),
            "RJ1",
            "2013-08-25",
            "2026-11-05T08:00:00Z",
            &line("あ", "Ah!"),
        );
        let pack = packs.download(&newest).unwrap();
        assert_eq!(
            (pack.made.as_str(), pack.texts[0].text.as_str()),
            ("2026-11-05T08:00:00Z", "Ah!")
        );
    }

    #[test]
    fn refuses_a_pack_that_does_not_match_its_manifest_or_folder_or_holds_japanese() {
        let root = tempfile::tempdir().unwrap();
        publish(
            root.path(),
            "RJ1",
            "2013-08-25",
            "2026-11-03T12:00:00Z",
            &line("あ", "Ah"),
        );
        let folder = root.path().join("RJ1/translations/2013-08-25/en");
        let packs = ArchivePacks::new(Arc::new(FolderFiles::new(root.path().to_owned())));
        let first = offer(&packs, "RJ1", None);
        let zip = folder.join("RJ1-en-2013-08-25.zip");
        let whole = fs::read(&zip).unwrap();
        fs::write(&zip, &whole[..whole.len() / 2]).unwrap();
        assert!(packs.download(&first).is_err());

        let japanese = format!(
            "{}\n",
            json!({"key": text_key("あ"), "text": "Ah", "source": "あ"})
        );
        publish(
            root.path(),
            "RJ1",
            "2013-09-01",
            "2026-11-04T12:00:00Z",
            &japanese,
        );
        assert!(packs.download(&offer(&packs, "RJ1", None)).is_err());

        // A pack copied into another version's folder is not that version's.
        fs::create_dir_all(root.path().join("RJ1/translations/2014-01-01/en")).unwrap();
        fs::copy(
            folder.join(MANIFEST_FILE),
            root.path()
                .join("RJ1/translations/2014-01-01/en")
                .join(MANIFEST_FILE),
        )
        .unwrap();
        publish_version(root.path(), "RJ1", "2014-01-01", "2026-11-05");
        assert!(packs.find("RJ1", None).is_err());
    }

    /// The archive answering one way for every path.
    struct Refusing(FileError);

    impl ArchiveFiles for Refusing {
        fn read(&self, _path: &str, _limit: u64) -> Result<Vec<u8>, FileError> {
            Err(self.0.clone())
        }
    }

    #[test]
    fn tells_an_elite_pack_from_a_missing_one_and_a_signed_out_player() {
        let answer =
            |error: FileError| ArchivePacks::new(Arc::new(Refusing(error))).find("RJ1", None);
        assert_eq!(answer(FileError::Forbidden).unwrap(), PackLookup::ForElite);
        assert_eq!(answer(FileError::NotFound).unwrap(), PackLookup::None);
        assert_eq!(answer(FileError::SignedOut).unwrap(), PackLookup::SignedOut);
        assert!(answer(FileError::Other("down".to_owned())).is_err());
        let works = ArchivePacks::new(Arc::new(Refusing(FileError::SignedOut)));
        assert!(works.translated_works().unwrap().is_empty());
    }

    #[test]
    fn lists_the_works_with_a_pack_in_the_launchers_language() {
        let root = tempfile::tempdir().unwrap();
        let index = json!({"format": 1, "works": {
            "RJ112559": {"en": "2026-11-03T12:00:00Z"},
            "RJ2": {"es": "2026-11-01"},
            "../x": {"en": "2026-11-03"},
            "RJ3": {"en": "soon"}
        }});
        fs::write(root.path().join("index.json"), index.to_string()).unwrap();
        let packs = ArchivePacks::new(Arc::new(FolderFiles::new(root.path().to_owned())));
        assert_eq!(
            packs.translated_works().unwrap(),
            BTreeMap::from([("RJ112559".to_owned(), "2026-11-03T12:00:00Z".to_owned())])
        );
    }

    #[test]
    fn marks_the_works_once_the_list_is_published() {
        let root = tempfile::tempdir().unwrap();
        let packs = ArchivePacks::new(Arc::new(FolderFiles::new(root.path().to_owned())));
        assert!(packs.translated_works().unwrap().is_empty());
        let index = json!({"format": 1, "works": {"RJ112559": {"en": "2026-11-03T10:50:49Z"}}});
        fs::write(root.path().join("index.json"), index.to_string()).unwrap();
        assert_eq!(packs.translated_works().unwrap().len(), 1);
    }

    #[test]
    fn a_legacy_index_can_use_a_known_version_without_directory_discovery() {
        let root = tempfile::tempdir().unwrap();
        publish(
            root.path(),
            "RJ1",
            "2013-08-25",
            "2026-11-07T12:00:00Z",
            &line("あ", "Ah"),
        );
        let file = root.path().join("index.json");
        let mut index: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        index.as_object_mut().unwrap().remove("versions");
        fs::write(file, index.to_string()).unwrap();
        let packs = ArchivePacks::new(Arc::new(FolderFiles::new(root.path().to_owned())));
        assert_eq!(packs.find("RJ1", None).unwrap(), PackLookup::None);
        assert_eq!(
            offer(&packs, "RJ1", Some("2013-08-25")).version,
            "2013-08-25"
        );
        assert!(
            FolderFiles::new(root.path().to_owned())
                .path("/overlays/dlsite/..\\outside")
                .is_err()
        );
    }

    #[test]
    fn a_modern_index_does_not_probe_unpublished_work_folders() {
        let root = tempfile::tempdir().unwrap();
        publish(
            root.path(),
            "RJ1",
            "2013-08-25",
            "2026-11-07T12:00:00Z",
            &line("あ", "Ah"),
        );
        let misplaced = root.path().join("RJ2/translations/2026-10-30/en");
        fs::create_dir_all(&misplaced).unwrap();
        fs::copy(
            root.path()
                .join("RJ1/translations/2013-08-25/en/manifest.json"),
            misplaced.join("manifest.json"),
        )
        .unwrap();
        let packs = ArchivePacks::new(Arc::new(FolderFiles::new(root.path().to_owned())));
        assert_eq!(
            packs.find("RJ2", Some("2026-10-30")).unwrap(),
            PackLookup::None
        );
    }

    #[test]
    fn takes_only_folders_named_by_a_day_as_versions() {
        let names = || {
            [
                "2013-08-25",
                "2014-01-01",
                "2014-01-01-2",
                "drafts",
                "2015-1-01",
                "2013-02-01",
            ]
            .map(str::to_owned)
            .to_vec()
        };
        assert_eq!(
            in_turn(names(), None),
            ["2014-01-01", "2013-08-25", "2013-02-01"]
        );
        assert_eq!(
            in_turn(names(), Some("2013-08-25")),
            ["2013-08-25", "2014-01-01", "2013-02-01"]
        );
        assert_eq!(
            in_turn(names(), Some("drafts")),
            ["2014-01-01", "2013-08-25", "2013-02-01"]
        );
        assert!(in_turn(Vec::new(), Some("2013-08-25")).is_empty());
    }
}
