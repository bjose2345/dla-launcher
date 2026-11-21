//! What RPG Maker's three RGSS generations share: where a game keeps its
//! data (an encrypted archive, or its `Data` folder), the Ruby Marshal
//! scripts file the launcher's script goes first in, the translations file
//! that script reads, and the place it writes. The generations differ in
//! their archive format, file names and Ruby version
//! ([`Generation`]); what each one's data holds is read by `vx_ace_data`
//! and `xp_vx_data`.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use dla_application::game_translation::GameTranslationError;
use dla_domain::{
    game_engine::GameEngine,
    installation::RelativePath,
    translation::{GameFileChange, GameFileChangeKind},
};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use sha2::{Digest, Sha256};

use crate::{
    rgss_texts::script_literals,
    rgss3a::{self, Patch},
    rgssad,
    ruby_files::{RubyFiles, with_stand_ins},
    ruby_marshal::{self, Strings, Value, Writer},
    script_texts::ScriptTexts,
    vx_ace, xp_vx,
};

/// The launcher's own script section in a game's scripts.
pub const OUR_SECTION: i64 = 77_000_001;
const SECTION_TITLE: &str = "DLA Translation";
pub const PLACE: &str = "place.txt";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Generation {
    /// RGSS1, Ruby 1.8: `Game.rgssad`, `Data/*.rxdata`.
    Xp,
    /// RGSS2, Ruby 1.8: `Game.rgss2a`, `Data/*.rvdata`.
    Vx,
    /// RGSS3, Ruby 1.9: `Game.rgss3a`, `Data/*.rvdata2`.
    VxAce,
}

impl Generation {
    pub fn of(engine: GameEngine) -> Option<Self> {
        match engine {
            GameEngine::RpgMakerXp => Some(Self::Xp),
            GameEngine::RpgMakerVx => Some(Self::Vx),
            GameEngine::RpgMakerVxAce => Some(Self::VxAce),
            _ => None,
        }
    }

    fn archive_extension(self) -> &'static str {
        match self {
            Self::Xp => "rgssad",
            Self::Vx => "rgss2a",
            Self::VxAce => "rgss3a",
        }
    }

    fn archive_format(self) -> ArchiveFormat {
        match self {
            Self::Xp | Self::Vx => ArchiveFormat::Rgssad,
            Self::VxAce => ArchiveFormat::Rgss3a,
        }
    }

    pub fn data_extension(self) -> &'static str {
        match self {
            Self::Xp => "rxdata",
            Self::Vx => "rvdata",
            Self::VxAce => "rvdata2",
        }
    }

    /// A data file by its name (`System`), as a path in the game.
    pub fn file(self, name: &str) -> String {
        format!("Data/{name}.{}", self.data_extension())
    }

    pub fn scripts(self) -> String {
        self.file("Scripts")
    }

    pub fn strings(self) -> Strings {
        match self {
            Self::Xp | Self::Vx => Strings::Plain,
            Self::VxAce => Strings::Tagged,
        }
    }

    /// The launcher's script for the generation's Ruby and engine.
    pub fn script(self) -> &'static str {
        match self {
            Self::Xp | Self::Vx => xp_vx::SCRIPT,
            Self::VxAce => vx_ace::SCRIPT,
        }
    }

    pub fn fingerprint(self) -> [u8; 32] {
        Sha256::digest(self.script().as_bytes()).into()
    }

    /// The translations file the script reads, in the launcher's folder.
    pub fn translations_file(self) -> &'static str {
        match self {
            Self::Xp | Self::Vx => xp_vx::TRANSLATIONS,
            Self::VxAce => vx_ace::TRANSLATIONS,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArchiveFormat {
    Rgss3a,
    Rgssad,
}

impl ArchiveFormat {
    fn of_file(path: &Path) -> io::Result<Self> {
        let mut header = [0; 8];
        fs::File::open(path)?.read_exact(&mut header)?;
        match &header {
            rgss3a::HEADER => Ok(Self::Rgss3a),
            rgssad::HEADER => Ok(Self::Rgssad),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not an RPG Maker archive",
            )),
        }
    }

    fn find_patch(self, path: &Path) -> io::Result<Option<Patch>> {
        match self {
            Self::Rgss3a => rgss3a::find_patch(path),
            Self::Rgssad => rgssad::find_patch(path),
        }
    }

    fn patch_in_use(self, path: &Path, patch: &Patch) -> io::Result<bool> {
        match self {
            Self::Rgss3a => rgss3a::patch_in_use(path, patch),
            Self::Rgssad => rgssad::patch_in_use(path, patch),
        }
    }

    fn remove_patch(self, path: &Path) -> io::Result<bool> {
        match self {
            Self::Rgss3a => rgss3a::remove_patch(path),
            Self::Rgssad => rgssad::remove_patch(path),
        }
    }

    fn add_patch(
        self,
        path: &Path,
        scripts_name: &str,
        scripts: &[u8],
        fingerprint: [u8; 32],
    ) -> io::Result<Patch> {
        match self {
            Self::Rgss3a => rgss3a::add_patch(path, scripts, fingerprint),
            Self::Rgssad => rgssad::add_patch(path, scripts_name, scripts, fingerprint),
        }
    }
}

/// The image formats the engine reads.
const IMAGE_EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "bmp"];

/// An open archive of either format.
pub enum ArchiveFile {
    Rgss3a(rgss3a::Archive),
    Rgssad(rgssad::Archive),
}

impl ArchiveFile {
    fn open(path: &Path, format: ArchiveFormat) -> io::Result<Self> {
        Ok(match format {
            ArchiveFormat::Rgss3a => Self::Rgss3a(rgss3a::Archive::open(path)?),
            ArchiveFormat::Rgssad => Self::Rgssad(rgssad::Archive::open(path)?),
        })
    }

    /// Every file's name, with `/` between folders.
    fn names(&self) -> Vec<String> {
        match self {
            Self::Rgss3a(archive) => archive
                .entries()
                .iter()
                .map(|entry| entry.name.replace('\\', "/"))
                .collect(),
            Self::Rgssad(archive) => archive
                .entries()
                .iter()
                .map(|entry| entry.name.replace('\\', "/"))
                .collect(),
        }
    }

    fn has(&self, name: &str) -> bool {
        match self {
            Self::Rgss3a(archive) => archive.entry(name).is_some(),
            Self::Rgssad(archive) => archive.entry(name).is_some(),
        }
    }

    fn read(&self, path: &Path, name: &str) -> io::Result<Option<Vec<u8>>> {
        match self {
            Self::Rgss3a(archive) => archive
                .entry(name)
                .map(|entry| archive.read(path, entry))
                .transpose(),
            Self::Rgssad(archive) => archive
                .entry(name)
                .map(|entry| archive.read(path, entry))
                .transpose(),
        }
    }
}

/// Where a game keeps its data.
pub struct GameData {
    generation: Generation,
    source: Source,
}

enum Source {
    Archive { path: PathBuf, archive: ArchiveFile },
    Folder(PathBuf),
}

impl GameData {
    /// The archive in the game's folder, or else its `Data` folder.
    pub fn find(game: &Path, generation: Generation) -> io::Result<Self> {
        let source = match archive_in(game, generation)? {
            Some(path) => {
                let archive = ArchiveFile::open(&path, generation.archive_format())?;
                Source::Archive { path, archive }
            }
            None => Source::Folder(game.to_owned()),
        };
        Ok(Self { generation, source })
    }

    /// A file by its path in the game (`Data/Map001.rvdata2`); `None` when
    /// the game has no such file.
    pub fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match &self.source {
            Source::Archive { path, archive } => archive.read(path, name),
            Source::Folder(game) => match find_case_insensitive(game, name)? {
                Some(path) => fs::read(path).map(Some),
                None => Ok(None),
            },
        }
    }

    /// Whether the game has an image, by its path under `Graphics` without
    /// the extension (`Pictures/Menu`): in its archive, or as a file beside
    /// it, which the engine reads first.
    pub fn has_image(&self, path: &str) -> io::Result<bool> {
        let (game, archive) = match &self.source {
            Source::Archive { path, archive } => (path.parent(), Some(archive)),
            Source::Folder(game) => (Some(game.as_path()), None),
        };
        for extension in IMAGE_EXTENSIONS {
            let name = format!("Graphics/{path}.{extension}");
            let beside = match game {
                Some(game) => find_case_insensitive(game, &name)?,
                None => None,
            };
            if archive.is_some_and(|archive| archive.has(&name))
                || beside.is_some_and(|file| file.is_file())
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The maps' numbers and file paths, in order.
    pub(crate) fn maps(&self) -> io::Result<Vec<(u32, String)>> {
        let names = match &self.source {
            Source::Archive { archive, .. } => archive.names(),
            Source::Folder(game) => match find_case_insensitive(game, "Data")? {
                Some(data) => fs::read_dir(data)?
                    .filter_map(Result::ok)
                    .filter_map(|entry| {
                        entry
                            .file_name()
                            .to_str()
                            .map(|name| format!("Data/{name}"))
                    })
                    .collect(),
                None => Vec::new(),
            },
        };
        let suffix = format!(".{}", self.generation.data_extension());
        let mut maps = names
            .into_iter()
            .filter_map(|name| {
                let number = name
                    .rsplit('/')
                    .next()?
                    .strip_prefix("Map")?
                    .strip_suffix(suffix.as_str())?;
                (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
                    .then(|| number.parse().ok().map(|id| (id, name.clone())))
                    .flatten()
            })
            .collect::<Vec<_>>();
        maps.sort();
        Ok(maps)
    }

    pub(crate) fn load(&self, name: &str) -> io::Result<Value> {
        match self.read(name)? {
            Some(bytes) => ruby_marshal::read(&bytes).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("{name}: {error}"))
            }),
            None => Ok(Value::Nil),
        }
    }
}

/// The archive's path, if the game has one.
pub fn archive_in(game: &Path, generation: Generation) -> io::Result<Option<PathBuf>> {
    let wanted = generation.archive_extension();
    let mut found = Vec::new();
    for entry in fs::read_dir(game)? {
        let path = entry?.path();
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case(wanted))
            && path.is_file()
        {
            found.push(path);
        }
    }
    found.sort();
    Ok(found.into_iter().next())
}

/// Puts the launcher's script first in the game's scripts. `game` is the
/// folder with `Game.exe`; `relative` turns a path in it into one the
/// change records. With an archive, the archive is patched in place; a game
/// without one has its scripts file replaced and the original kept, like MV
/// and MZ's plugin list.
pub fn install(
    game: &Path,
    generation: Generation,
    relative: impl Fn(&str) -> Result<RelativePath, GameTranslationError>,
    previous: &[GameFileChange],
) -> Result<GameFileChange, GameTranslationError> {
    match archive_in(game, generation).map_err(GameTranslationError::folder)? {
        Some(path) => {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    GameTranslationError::folder("the archive's name is not readable")
                })?;
            let patch = patch_archive(&path, generation).map_err(|error| {
                GameTranslationError::folder(format!("{name} could not be changed: {error}"))
            })?;
            Ok(GameFileChange {
                path: relative(name)?,
                kind: GameFileChangeKind::Patched {
                    at: patch.fields_at,
                    original: patch.original_fields,
                    original_bytes: patch.original_bytes,
                    written_sha256: patch.written_sha256,
                    written_bytes: patch.written_bytes,
                },
            })
        }
        None => replace_scripts_file(game, generation, relative, previous),
    }
}

fn patch_archive(path: &Path, generation: Generation) -> io::Result<Patch> {
    let format = generation.archive_format();
    let fingerprint = generation.fingerprint();
    if let Some(patch) = format.find_patch(path)? {
        if patch.fingerprint == fingerprint && format.patch_in_use(path, &patch)? {
            return Ok(patch);
        }
        // An older script, or a patch left halfway.
        format.remove_patch(path)?;
    }
    let scripts_name = generation.scripts();
    let archive = ArchiveFile::open(path, format)?;
    let original = archive.read(path, &scripts_name)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("the archive has no {}", scripts_name.replace('/', "\\")),
        )
    })?;
    let scripts = with_our_section(&original, generation)?;
    format.add_patch(path, &scripts_name, &scripts, fingerprint)
}

fn replace_scripts_file(
    game: &Path,
    generation: Generation,
    relative: impl Fn(&str) -> Result<RelativePath, GameTranslationError>,
    previous: &[GameFileChange],
) -> Result<GameFileChange, GameTranslationError> {
    let scripts_name = generation.scripts();
    let path = game.join(&scripts_name);
    let recorded = relative(&scripts_name)?;
    let original = match previous.iter().find(|change| change.path == recorded) {
        Some(GameFileChange {
            kind: GameFileChangeKind::Changed { original, .. },
            ..
        }) => original.clone(),
        _ => fs::read(&path).map_err(|error| {
            GameTranslationError::folder(format!("{scripts_name} could not be read: {error}"))
        })?,
    };
    let scripts = with_our_section(&original, generation).map_err(GameTranslationError::folder)?;
    let written = hex::encode(Sha256::digest(&scripts));
    if fs::read(&path).ok().as_deref() != Some(scripts.as_slice()) {
        crate::replace(&path, &scripts)?;
    }
    Ok(GameFileChange {
        path: recorded,
        kind: GameFileChangeKind::Changed {
            original,
            written_sha256: written,
            written_bytes: scripts.len() as u64,
        },
    })
}

/// Removes the launcher's patch from an archive of either format. Returns
/// whether there was one.
pub fn remove_archive_patch(path: &Path) -> io::Result<bool> {
    ArchiveFormat::of_file(path)?.remove_patch(path)
}

/// The game's script sections with the launcher's first; an earlier copy of
/// the launcher's is dropped.
pub fn with_our_section(original: &[u8], generation: Generation) -> io::Result<Vec<u8>> {
    let invalid = |message: &str| io::Error::new(io::ErrorKind::InvalidData, message.to_owned());
    let strings = generation.strings();
    let sections = ruby_marshal::read(original).map_err(|error| invalid(&error.to_string()))?;
    let sections = sections
        .items()
        .iter()
        .filter(|section| section.items().first().and_then(Value::as_integer) != Some(OUR_SECTION))
        .collect::<Vec<_>>();
    let mut code = ZlibEncoder::new(Vec::new(), Compression::default());
    code.write_all(generation.script().as_bytes())?;
    let code = code.finish()?;

    let mut writer = Writer::new();
    writer.array(sections.len() + 1);
    writer.array(3);
    writer.integer(OUR_SECTION);
    writer.string(SECTION_TITLE, strings);
    writer.binary(&code);
    for section in sections {
        let parts = section.items();
        let (Some(id), Some(title), Some(code)) = (
            parts.first().and_then(Value::as_integer),
            parts.get(1).and_then(Value::as_bytes),
            parts.get(2).and_then(Value::as_bytes),
        ) else {
            return Err(invalid(
                "a script section is laid out in a way the launcher does not know",
            ));
        };
        writer.array(parts.len());
        writer.integer(id);
        match (strings, std::str::from_utf8(title)) {
            (Strings::Tagged, Ok(title)) => writer.text(title),
            _ => writer.binary(title),
        }
        writer.binary(code);
        // A section may carry more than the engine reads (a note in plain
        // text); it goes back as it came.
        for extra in &parts[3..] {
            let Some(extra) = extra.as_bytes() else {
                return Err(invalid(
                    "a script section is laid out in a way the launcher does not know",
                ));
            };
            writer.binary(extra);
        }
    }
    Ok(writer.finish())
}

/// What a game's scripts hold for a translation.
pub struct Scripts {
    /// The Japanese strings in the scripts but `Vocab`'s, and the ones the
    /// Ruby files those scripts load use as identifiers: what the scripts
    /// draw, and the database names they compare, which must stay Japanese
    /// in the data.
    pub strings: HashSet<String>,
    /// Those files.
    pub files: RubyFiles,
    /// The sentences the scripts themselves put together.
    pub shown: ScriptTexts,
}

impl Scripts {
    /// Whether a text of the game's data stays Japanese there, its English
    /// only drawn: a string of the scripts, or a name every part of which
    /// is one (`インプ/緑`, a class and its colour: scripts split such a
    /// name at `/` and compare the parts).
    pub fn keeps(&self, text: &str) -> bool {
        self.strings.contains(text)
            || text.contains('/')
                && !text.contains('\n')
                && text.split('/').all(|part| self.strings.contains(part))
    }
}

pub fn strings_in_scripts(game: &Path, generation: Generation) -> io::Result<Scripts> {
    let data = GameData::find(game, generation)?;
    strings_and_files(
        game,
        &script_sections(&data.load(&generation.scripts())?),
        generation,
    )
}

/// The same for scripts already read, as (title, source).
pub fn strings_and_files(
    game: &Path,
    sections: &[(String, String)],
    generation: Generation,
) -> io::Result<Scripts> {
    let mut strings = script_literals(sections);
    let files = RubyFiles::read(game, sections, &strings)?;
    strings.extend(files.used().iter().cloned());
    // Only the script for XP and VX hands the engine the copies.
    let shown = match generation {
        Generation::Xp | Generation::Vx => ScriptTexts::read(sections, files.used()),
        Generation::VxAce => ScriptTexts::default(),
    };
    Ok(Scripts {
        strings,
        files,
        shown,
    })
}

/// The game's script sections in their order as (title, source), without
/// the launcher's. One the launcher cannot read has no source: the others
/// keep their place.
pub fn script_sections(scripts: &Value) -> Vec<(String, String)> {
    scripts
        .items()
        .iter()
        .filter(|section| section.items().first().and_then(Value::as_integer) != Some(OUR_SECTION))
        .map(|section| {
            let parts = section.items();
            let title = parts
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let source = parts
                .get(2)
                .and_then(Value::as_bytes)
                .and_then(inflate)
                .unwrap_or_default();
            (title, source)
        })
        .collect()
}

pub fn inflate(bytes: &[u8]) -> Option<String> {
    let mut source = Vec::new();
    ZlibDecoder::new(bytes).read_to_end(&mut source).ok()?;
    Some(String::from_utf8_lossy(&source).into_owned())
}

/// The translations as the script reads them: a Ruby hash with `texts` and
/// `names`, each from the Japanese to the English, and `kept`, the texts the
/// game's scripts use, whose English is only drawn: the data keeps the
/// Japanese the scripts compare. `pictures` are the images that have an
/// English version in the launcher's folder, as paths under the game's
/// `Graphics`: the script cannot list a folder of Japanese names on every
/// Windows. `letters` are the letters the game draws as codes: the English
/// holds what stands for them, which the script draws as the letters.
pub fn translations_file(
    texts: &BTreeMap<String, String>,
    names: &BTreeMap<String, String>,
    kept: &[String],
    pictures: &[&str],
    letters: &str,
    generation: Generation,
) -> Vec<u8> {
    let strings = generation.strings();
    let mut writer = Writer::new();
    writer.hash(5);
    writer.string("version", strings);
    writer.integer(1);
    for (key, entries) in [("texts", texts), ("names", names)] {
        writer.string(key, strings);
        writer.hash(entries.len());
        for (japanese, english) in entries {
            writer.string(japanese, strings);
            writer.string(&with_stand_ins(english, letters, &[], &[]), strings);
        }
    }
    writer.string("kept", strings);
    writer.array(kept.len());
    for text in kept {
        writer.string(text, strings);
    }
    writer.string("pictures", strings);
    writer.array(pictures.len());
    for path in pictures {
        writer.string(path, strings);
    }
    writer.finish()
}

/// Where the player is, as the script last wrote it.
pub fn place(data_folder: &Path) -> Option<String> {
    let place = fs::read_to_string(data_folder.join(PLACE)).ok()?;
    let place = place.trim();
    (place.starts_with("map:") && place.len() < 32).then(|| place.to_owned())
}

pub(crate) fn find_case_insensitive(root: &Path, relative: &str) -> io::Result<Option<PathBuf>> {
    let mut path = root.to_owned();
    for part in relative.split(['/', '\\']) {
        let exact = path.join(part);
        if exact.exists() {
            path = exact;
            continue;
        }
        let Ok(entries) = fs::read_dir(&path) else {
            return Ok(None);
        };
        let found = entries.filter_map(Result::ok).find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(part))
        });
        match found {
            Some(entry) => path = entry.path(),
            None => return Ok(None),
        }
    }
    Ok(Some(path))
}

/// Builds a scripts file for tests: sections of (id, title, source).
#[cfg(test)]
pub(crate) fn scripts_file(sections: &[(i64, &str, &str)], strings: Strings) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.array(sections.len());
    for (id, title, source) in sections {
        let mut code = ZlibEncoder::new(Vec::new(), Compression::default());
        code.write_all(source.as_bytes()).unwrap();
        writer.array(3);
        writer.integer(*id);
        writer.string(title, strings);
        writer.binary(&code.finish().unwrap());
    }
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECTIONS: [(i64, &str, &str); 2] = [
        (
            12_345,
            "Vocab",
            "module Vocab\n  Attack = \"%sの攻撃！\"\nend\n",
        ),
        (67_890, "Main", "rgss_main { SceneManager.run }\n"),
    ];

    #[test]
    fn puts_the_script_first_once_and_keeps_the_games_sections() {
        for generation in [Generation::Xp, Generation::Vx, Generation::VxAce] {
            let game = scripts_file(&SECTIONS, generation.strings());
            let patched = with_our_section(&game, generation).unwrap();
            let again = with_our_section(&patched, generation).unwrap();
            assert_eq!(again, patched);
            let sections = ruby_marshal::read(&patched).unwrap();
            let first = &sections.items()[0];
            assert_eq!(first.items()[0].as_integer(), Some(OUR_SECTION));
            assert_eq!(first.items()[1].as_str(), Some(SECTION_TITLE));
            assert_eq!(
                inflate(first.items()[2].as_bytes().unwrap()).as_deref(),
                Some(generation.script())
            );
            assert_eq!(
                script_sections(&sections),
                SECTIONS
                    .iter()
                    .map(|(_, title, source)| ((*title).to_owned(), (*source).to_owned()))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn keeps_what_a_section_carries_beyond_its_code() {
        let mut code = ZlibEncoder::new(Vec::new(), Compression::default());
        code.write_all(b"p 1\n").unwrap();
        let code = code.finish().unwrap();
        let mut writer = Writer::new();
        writer.array(1);
        writer.array(4);
        writer.integer(12_345);
        writer.binary(b"Debug");
        writer.binary(&code);
        writer.binary(b"p 1 if DEBUG\r\n");
        let patched = with_our_section(&writer.finish(), Generation::Xp).unwrap();
        let sections = ruby_marshal::read(&patched).unwrap();
        let kept = sections.items()[1].items();
        assert_eq!(kept.len(), 4);
        assert_eq!(kept[2].as_bytes(), Some(&code[..]));
        assert_eq!(kept[3].as_bytes(), Some(&b"p 1 if DEBUG\r\n"[..]));
    }

    #[test]
    fn writes_strings_as_each_generations_ruby_reads_them() {
        // Ruby 1.9 strings carry their encoding (`I"…"` with the symbol
        // `E`, written `:\x06E`); Ruby 1.8 strings are plain.
        let tagged =
            with_our_section(&scripts_file(&SECTIONS, Strings::Tagged), Generation::VxAce).unwrap();
        let plain =
            with_our_section(&scripts_file(&SECTIONS, Strings::Plain), Generation::Xp).unwrap();
        assert!(tagged.windows(3).any(|window| window == b":\x06E"));
        assert!(!plain.windows(3).any(|window| window == b":\x06E"));
    }

    #[test]
    fn keeps_the_names_whose_parts_the_scripts_compare() {
        let directory = tempfile::tempdir().unwrap();
        let sections = [(
            "RPG::Class".to_owned(),
            "bonus if self.name == \"インプ\" && self.color == \"緑\"\n".to_owned(),
        )];
        let scripts = strings_and_files(directory.path(), &sections, Generation::Xp).unwrap();
        assert!(scripts.keeps("インプ"));
        assert!(scripts.keeps("インプ/緑"));
        // An enemy's class number and level are not strings of the scripts.
        assert!(!scripts.keeps("インプ/21/3"));
        assert!(!scripts.keeps("スライム/緑"));
    }

    #[test]
    fn writes_translations_ruby_reads_as_hashes() {
        let texts = BTreeMap::from([("剣".to_owned(), "Sword".to_owned())]);
        let names = BTreeMap::from([("ルーク".to_owned(), "Luke".to_owned())]);
        let kept = ["古い鍵".to_owned()];
        for generation in [Generation::Xp, Generation::VxAce] {
            let bytes = translations_file(
                &texts,
                &names,
                &kept,
                &["Pictures/メニュー"],
                "",
                generation,
            );
            assert_eq!(
                bytes.windows(3).any(|window| window == b":\x06E"),
                generation == Generation::VxAce
            );
            let file = ruby_marshal::read(&bytes).unwrap();
            let Value::Hash(pairs) = file else {
                panic!("a hash");
            };
            assert_eq!(
                pairs[0],
                (Value::String(b"version".to_vec()), Value::Integer(1))
            );
            assert_eq!(
                pairs[1].1,
                Value::Hash(vec![(
                    Value::String("剣".as_bytes().to_vec()),
                    Value::String(b"Sword".to_vec())
                )])
            );
            assert_eq!(pairs[2].0, Value::String(b"names".to_vec()));
            assert_eq!(
                pairs[3],
                (
                    Value::String(b"kept".to_vec()),
                    Value::Array(vec![Value::String("古い鍵".as_bytes().to_vec())])
                )
            );
            assert_eq!(
                pairs[4],
                (
                    Value::String(b"pictures".to_vec()),
                    Value::Array(vec![Value::String("Pictures/メニュー".as_bytes().to_vec())])
                )
            );
        }
        // A game that draws `w` as a code: the letter in a word is written
        // as what stands for it, and a code keeps its own letters.
        let texts = BTreeMap::from([("剣".to_owned(), "\\w[2]Sword".to_owned())]);
        let file = translations_file(&texts, &names, &kept, &[], "w", Generation::Xp);
        let Value::Hash(pairs) = ruby_marshal::read(&file).unwrap() else {
            panic!("a hash");
        };
        assert_eq!(
            pairs[1].1,
            Value::Hash(vec![(
                Value::String("剣".as_bytes().to_vec()),
                Value::String("\\w[2]S\u{f877}ord".as_bytes().to_vec())
            )])
        );
    }

    #[test]
    fn knows_each_generations_files() {
        assert_eq!(Generation::Xp.scripts(), "Data/Scripts.rxdata");
        assert_eq!(Generation::Vx.file("Map001"), "Data/Map001.rvdata");
        assert_eq!(Generation::VxAce.scripts(), "Data/Scripts.rvdata2");
        assert_eq!(Generation::of(GameEngine::RpgMakerXp), Some(Generation::Xp));
        assert_eq!(Generation::of(GameEngine::RpgMakerMv), None);
        assert_ne!(
            Generation::Xp.fingerprint(),
            Generation::VxAce.fingerprint()
        );
        assert_eq!(Generation::Xp.fingerprint(), Generation::Vx.fingerprint());
    }

    #[test]
    fn replaces_the_scripts_file_of_a_game_without_an_archive_and_keeps_the_original() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path();
        fs::create_dir_all(game.join("Data")).unwrap();
        let original = scripts_file(&SECTIONS, Strings::Plain);
        fs::write(game.join("Data/Scripts.rxdata"), &original).unwrap();
        let relative = |path: &str| RelativePath::parse(path).map_err(GameTranslationError::folder);
        let change = install(game, Generation::Xp, relative, &[]).unwrap();
        assert_eq!(change.path.as_str(), "Data/Scripts.rxdata");
        let GameFileChangeKind::Changed {
            original: recorded,
            written_bytes,
            ..
        } = &change.kind
        else {
            panic!("a changed file");
        };
        assert_eq!(recorded, &original);
        let written = fs::read(game.join("Data/Scripts.rxdata")).unwrap();
        assert_eq!(written.len() as u64, *written_bytes);
        let sections = ruby_marshal::read(&written).unwrap();
        assert_eq!(sections.items().len(), 3);
        // A second install keeps the first's original.
        let again = install(
            game,
            Generation::Xp,
            relative,
            std::slice::from_ref(&change),
        )
        .unwrap();
        assert_eq!(again, change);
    }

    #[test]
    fn knows_the_images_a_game_has_in_its_archive_and_beside_it() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path();
        fs::write(
            game.join("Game.rgss3a"),
            rgss3a::build(&[("Graphics\\Pictures\\メニュー.png", b"image")]),
        )
        .unwrap();
        fs::create_dir_all(game.join("Graphics/Titles1")).unwrap();
        fs::write(game.join("Graphics/Titles1/Title.JPG"), b"image").unwrap();
        let data = GameData::find(game, Generation::VxAce).unwrap();
        assert!(data.has_image("pictures/メニュー").unwrap());
        assert!(data.has_image("Titles1/Title").unwrap());
        assert!(!data.has_image("Pictures/Title").unwrap());
        assert!(!data.has_image("Titles1").unwrap());
    }

    #[test]
    fn patches_the_archive_of_either_format_and_takes_the_patch_off_by_its_header() {
        for (generation, archive) in [
            (
                Generation::Xp,
                rgssad::build(&[(
                    "Data\\Scripts.rxdata",
                    &scripts_file(&SECTIONS, Strings::Plain)[..],
                )]),
            ),
            (
                Generation::VxAce,
                rgss3a::build(&[(
                    "Data\\Scripts.rvdata2",
                    &scripts_file(&SECTIONS, Strings::Tagged)[..],
                )]),
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let game = directory.path();
            let path = game.join(format!("Game.{}", generation.archive_extension()));
            fs::write(&path, &archive).unwrap();
            let relative =
                |path: &str| RelativePath::parse(path).map_err(GameTranslationError::folder);
            let change = install(game, generation, relative, &[]).unwrap();
            assert_eq!(
                change.path.as_str(),
                path.file_name().unwrap().to_str().unwrap()
            );
            assert!(matches!(change.kind, GameFileChangeKind::Patched { .. }));
            let data = GameData::find(game, generation).unwrap();
            let scripts = data.load(&generation.scripts()).unwrap();
            assert_eq!(
                scripts.items()[0].items()[0].as_integer(),
                Some(OUR_SECTION)
            );
            assert_eq!(install(game, generation, relative, &[]).unwrap(), change);
            assert!(remove_archive_patch(&path).unwrap());
            assert_eq!(fs::read(&path).unwrap(), archive);
            assert!(!remove_archive_patch(&path).unwrap());
        }
    }
}
