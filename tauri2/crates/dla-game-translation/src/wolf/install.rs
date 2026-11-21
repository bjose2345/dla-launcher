//! A translated WOLF RPG Editor game on disk. The engine reads a data
//! folder from its archive (`Data/BasicData.wolf`) when there is one and
//! from the folder itself (`Data/BasicData/`) only when there is none, so
//! the launcher moves the game's own data aside under a name the engine
//! does not look for (`….dla-original`) and writes the folder: every file
//! as the game had it, and the ones with text in English. The game's own
//! data is never written to; putting it back is a rename. A game that
//! ships folders has them moved aside the same way. The archives of
//! pictures and sound are not touched.
//!
//! A game may keep everything in one archive beside its `Data` folder
//! (`Data.wolf`), and the engine then reads nothing from the folder. That
//! archive is moved aside whole, and what it held is written into `Data`
//! the way the engine looks for it next: the two folders with text as
//! folders, every other folder as an archive of its own with the bytes
//! copied as they are stored, and the files outside every folder as files.
//!
//! Each step leaves a state the next start or a removal finishes from: the
//! record of what the launcher writes comes first, a folder or an archive
//! is built under another name and renamed whole, and the game's data
//! moves aside last.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use super::{
    dxa::{Archive, Entry},
    game_dat,
    stream::invalid,
    texts::{BASIC_DATA, DataFile, Files, GAME_DAT, MAP_DATA},
};
use crate::rgss::find_case_insensitive;

const DATA: &str = "Data";
const ARCHIVE_EXTENSION: &str = "wolf";
const ASIDE: &str = ".dla-original";
const BUILDING: &str = ".dla-new";
/// In the launcher's own folder in the game: which data folders it wrote,
/// for each file with text the hashes of what it wrote last and of what
/// that replaced, and for a game in one archive what else it put into the
/// `Data` folder and whether it made that folder.
const RECORD: &str = "wolf.json";
pub const FOLDERS: [&str; 2] = [BASIC_DATA, MAP_DATA];

/// Where one data folder of the game is, and where its original went.
struct Place {
    /// The folder the engine reads once no archive stands in its way.
    folder: PathBuf,
    archive: PathBuf,
    archive_aside: PathBuf,
    folder_aside: PathBuf,
}

/// The game's own data for one folder.
enum Original {
    /// An archive, and the folder inside it (`BasicData/`) when it is the
    /// one that holds all the game's data.
    Archive {
        path: PathBuf,
        archive: Archive,
        inside: String,
    },
    Folder(PathBuf),
}

/// What the launcher did to a game's data, for the record of changes: the
/// game's own (`from`) is at `to`, and what stands at `added` is the
/// launcher's.
pub struct Moved {
    pub from: PathBuf,
    pub to: PathBuf,
    pub added: Vec<PathBuf>,
}

/// Where a game keeps its data.
enum Layout {
    /// All of it in one archive beside the `Data` folder.
    Whole(Whole),
    /// In the `Data` folder, an archive or a folder for each kind.
    Parts(PathBuf),
}

/// The one archive that holds all of a game's data.
struct Whole {
    archive: PathBuf,
    aside: PathBuf,
    /// The folder the engine reads once the archive is out of its way.
    data: PathBuf,
}

/// One thing the engine looks for in the `Data` folder of a game whose one
/// archive moved aside.
enum Part {
    /// A folder with text, written as a folder.
    Folder(String),
    /// Any other folder, written as an archive of its own.
    Archive(String),
    /// A file outside every folder.
    File(String),
}

fn layout(game: &Path) -> io::Result<Layout> {
    let (archive, aside) = own_and_aside(game, &format!("{DATA}.{ARCHIVE_EXTENSION}"))?;
    let data = find_case_insensitive(game, DATA)?;
    if archive.is_file() || aside.is_file() {
        let data = data.unwrap_or_else(|| archive.with_extension(""));
        return Ok(Layout::Whole(Whole {
            archive,
            aside,
            data,
        }));
    }
    match data {
        Some(data) if data.is_dir() => Ok(Layout::Parts(data)),
        _ => Err(invalid("the game has no Data folder")),
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Where the game has `wanted` in `folder` and where it goes when moved
/// aside. What was moved aside keeps the spelling the game gave it.
fn own_and_aside(folder: &Path, wanted: &str) -> io::Result<(PathBuf, PathBuf)> {
    if let Some(aside) = find_case_insensitive(folder, &format!("{wanted}{ASIDE}"))? {
        let own = aside.to_string_lossy();
        let own = PathBuf::from(&own[..own.len() - ASIDE.len()]);
        return Ok((own, aside));
    }
    let own = find_case_insensitive(folder, wanted)?.unwrap_or_else(|| folder.join(wanted));
    Ok((own.clone(), with_suffix(&own, ASIDE)))
}

impl Place {
    fn find(data: &Path, name: &str) -> io::Result<Self> {
        let (archive, archive_aside) = own_and_aside(data, &format!("{name}.{ARCHIVE_EXTENSION}"))?;
        let (folder, folder_aside) = own_and_aside(data, name)?;
        Ok(Self {
            folder,
            archive,
            archive_aside,
            folder_aside,
        })
    }

    /// The game's own data: an archive in its place (also one a game
    /// update put back), else what was moved aside, else the folder.
    fn original(&self) -> io::Result<Original> {
        if self.archive.is_file() {
            Original::archive(&self.archive)
        } else if self.archive_aside.is_file() {
            Original::archive(&self.archive_aside)
        } else if self.folder_aside.is_dir() {
            Ok(Original::Folder(self.folder_aside.clone()))
        } else if self.folder.is_dir() {
            Ok(Original::Folder(self.folder.clone()))
        } else {
            Err(invalid(format!(
                "the game has no {}",
                self.folder.display()
            )))
        }
    }

    fn is_applied(&self) -> bool {
        !self.archive.is_file() && (self.archive_aside.is_file() || self.folder_aside.is_dir())
    }
}

impl Original {
    /// An archive of one data folder.
    fn archive(path: &Path) -> io::Result<Self> {
        Ok(Self::Archive {
            path: path.to_owned(),
            archive: Archive::open(path)?,
            inside: String::new(),
        })
    }

    /// The folder `name` of the one archive that holds all the data.
    fn inside(path: &Path, archive: &Archive, name: &str) -> io::Result<Self> {
        let folder = archive
            .entries()
            .iter()
            .filter_map(|entry| entry.name.split_once('/'))
            .find(|(folder, _)| folder.eq_ignore_ascii_case(name))
            .ok_or_else(|| invalid(format!("{} has no {name}", path.display())))?
            .0;
        Ok(Self::Archive {
            path: path.to_owned(),
            archive: archive.clone(),
            inside: format!("{folder}/"),
        })
    }

    /// The files of an archive that belong to this folder, by their names
    /// in it.
    fn entries<'a>(
        archive: &'a Archive,
        inside: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a Entry)> {
        archive
            .entries()
            .iter()
            .filter_map(move |entry| Some((entry.name.strip_prefix(inside)?, entry)))
    }

    fn names(&self) -> io::Result<Vec<String>> {
        match self {
            Self::Archive {
                archive, inside, ..
            } => Ok(Self::entries(archive, inside)
                .map(|(name, _)| name.to_owned())
                .collect()),
            Self::Folder(folder) => Ok(files_in(folder)?.into_keys().collect()),
        }
    }

    fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        match self {
            Self::Archive {
                path,
                archive,
                inside,
            } => {
                let (_, entry) = Self::entries(archive, inside)
                    .find(|(in_folder, _)| *in_folder == name)
                    .ok_or_else(|| invalid(format!("the archive has no {name}")))?;
                archive.read(path, entry)
            }
            Self::Folder(folder) => fs::read(folder.join(name)),
        }
    }

    /// Writes every file into a new folder `to`.
    fn copy_to(&self, to: &Path) -> io::Result<()> {
        match self {
            Self::Archive {
                path,
                archive,
                inside,
            } => {
                for (name, entry) in Self::entries(archive, inside) {
                    let target = to.join(name);
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(target, archive.read(path, entry)?)?;
                }
            }
            Self::Folder(folder) => {
                for (name, path) in files_in(folder)? {
                    let target = to.join(name);
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::copy(path, target)?;
                }
            }
        }
        fs::create_dir_all(to)
    }
}

/// A folder's files by their path inside it, with `/` between folders.
fn files_in(folder: &Path) -> io::Result<BTreeMap<String, PathBuf>> {
    let mut files = BTreeMap::new();
    for entry in WalkDir::new(folder).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .strip_prefix(folder)
            .ok()
            .and_then(|relative| {
                relative
                    .components()
                    .map(|part| part.as_os_str().to_str())
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| invalid("a file's name in the game's data is not readable"))?
            .join("/");
        files.insert(name, entry.path().to_owned());
    }
    Ok(files)
}

/// The game's own data, wherever it is now.
pub struct Originals {
    basic: Original,
    maps: Original,
}

impl Originals {
    /// Fails for an archive the launcher does not read.
    pub fn find(game: &Path) -> io::Result<Self> {
        match layout(game)? {
            Layout::Whole(whole) => {
                let path = whole.original();
                let archive = Archive::open(path)?;
                Ok(Self {
                    basic: Original::inside(path, &archive, BASIC_DATA)?,
                    maps: Original::inside(path, &archive, MAP_DATA)?,
                })
            }
            Layout::Parts(data) => Ok(Self {
                basic: Place::find(&data, BASIC_DATA)?.original()?,
                maps: Place::find(&data, MAP_DATA)?.original()?,
            }),
        }
    }

    /// Whether the title file is not a protected one; the other data
    /// files are protected together with it.
    pub fn is_plain(&self) -> bool {
        let title = || {
            let names = self.basic.names().ok()?;
            let name = names
                .iter()
                .find(|name| name.eq_ignore_ascii_case(GAME_DAT))?;
            self.basic.read(name).ok()
        };
        title().is_none_or(|bytes| game_dat::has_known_start(&bytes))
    }

    fn of(&self, folder: &str) -> &Original {
        if folder == BASIC_DATA {
            &self.basic
        } else {
            &self.maps
        }
    }
}

impl Files for Originals {
    fn names(&self, folder: &str) -> io::Result<Vec<String>> {
        self.of(folder).names()
    }

    fn read(&self, folder: &str, name: &str) -> io::Result<Vec<u8>> {
        self.of(folder).read(name)
    }
}

#[derive(Default)]
struct Record {
    folders: Vec<String>,
    written: BTreeMap<String, Vec<String>>,
    /// For a game in one archive: the archives and files the launcher put
    /// into the `Data` folder, by their names there.
    added: Vec<String>,
    /// Whether the launcher made the `Data` folder.
    made_data_folder: bool,
}

impl Record {
    fn read(launcher_folder: &Path) -> Self {
        let record = fs::read(launcher_folder.join(RECORD))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null);
        let names = |list: &Value| {
            list.as_array()
                .into_iter()
                .flatten()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .collect()
        };
        Self {
            folders: names(&record["folders"]),
            added: names(&record["added"]),
            made_data_folder: record["madeDataFolder"].as_bool().unwrap_or(false),
            written: record["written"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(file, hashes)| {
                    let hashes = hashes
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|hash| hash.as_str().map(str::to_owned))
                        .collect();
                    (file.clone(), hashes)
                })
                .collect(),
        }
    }

    fn write(&self, launcher_folder: &Path) -> io::Result<()> {
        fs::create_dir_all(launcher_folder)?;
        let record = json!({
            "version": 1,
            "folders": self.folders,
            "written": self.written,
            "added": self.added,
            "madeDataFolder": self.made_data_folder,
        });
        replace(
            &launcher_folder.join(RECORD),
            &serde_json::to_vec_pretty(&record).map_err(io::Error::other)?,
        )
    }

    fn owns(&self, folder: &str) -> bool {
        self.folders.iter().any(|owned| owned == folder)
    }

    /// Whether the launcher wrote this part of a game in one archive.
    fn wrote(&self, part: &Part) -> bool {
        let named = |names: &[String], name: &str| {
            names.iter().any(|owned| owned.eq_ignore_ascii_case(name))
        };
        match part {
            Part::Folder(name) => named(&self.folders, name),
            part => named(&self.added, &part.name()),
        }
    }
}

fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = with_suffix(path, BUILDING);
    fs::write(&temporary, bytes)
        .and_then(|()| fs::rename(&temporary, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
}

fn remove_folder(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Builds `folder` from the game's own data under another name and
/// renames it into place.
fn build(original: &Original, folder: &Path) -> io::Result<()> {
    let building = with_suffix(folder, BUILDING);
    remove_folder(&building)?;
    original.copy_to(&building)?;
    remove_folder(folder)?;
    fs::rename(&building, folder)
}

/// The folders the launcher writes the files with text into, when its
/// folders are in place for every data folder.
fn applied_folders(game: &Path) -> io::Result<Option<Vec<(&'static str, PathBuf)>>> {
    let mut folders = Vec::new();
    match layout(game)? {
        Layout::Whole(whole) => {
            if whole.archive.is_file() || !whole.aside.is_file() {
                return Ok(None);
            }
            for name in FOLDERS {
                folders.push((name, whole.in_data(name)?));
            }
        }
        Layout::Parts(data) => {
            for name in FOLDERS {
                let place = Place::find(&data, name)?;
                if !place.is_applied() {
                    return Ok(None);
                }
                folders.push((name, place.folder));
            }
        }
    }
    Ok(folders
        .iter()
        .all(|(_, folder)| folder.is_dir())
        .then_some(folders))
}

/// Whether the launcher's folders are in place for every data folder.
pub fn is_applied(game: &Path) -> io::Result<bool> {
    Ok(applied_folders(game)?.is_some())
}

/// Moves the game's own data aside and puts the folders the engine then
/// reads in its place, as the game had them; [`write`] puts the English
/// in. Called again (each start), it finishes what a stop left halfway,
/// takes in what a game update changed, and otherwise changes nothing.
pub fn install(game: &Path, launcher_folder: &Path) -> io::Result<Vec<Moved>> {
    let data = match layout(game)? {
        Layout::Whole(whole) => return whole.install(launcher_folder).map(|moved| vec![moved]),
        Layout::Parts(data) => data,
    };
    let places = FOLDERS
        .iter()
        .map(|name| Place::find(&data, name).map(|place| (*name, place)))
        .collect::<io::Result<Vec<_>>>()?;
    // Every archive is one the launcher reads before anything changes.
    for (_, place) in &places {
        place.original()?;
    }
    let mut record = Record::read(launcher_folder);
    let mut moved = Vec::new();
    for (name, place) in &places {
        if place.archive.is_file() || place.archive_aside.is_file() {
            if place.archive.is_file() {
                // An archive back in its place is a game update's: the
                // one moved aside before it is no longer the game's.
                if place.archive_aside.is_file() {
                    fs::remove_file(&place.archive_aside)?;
                }
                if place.folder.is_dir() && !record.owns(name) {
                    return Err(invalid(format!(
                        "the game has both {} and a folder of that name; the launcher does not know which one it uses",
                        place.archive.display()
                    )));
                }
                if !record.owns(name) {
                    record.folders.push((*name).to_owned());
                    record.write(launcher_folder)?;
                }
                build(&Original::archive(&place.archive)?, &place.folder)?;
                fs::rename(&place.archive, &place.archive_aside)?;
            } else if !place.folder.is_dir() {
                build(&Original::archive(&place.archive_aside)?, &place.folder)?;
            }
            moved.push(Moved {
                from: place.archive.clone(),
                to: place.archive_aside.clone(),
                added: vec![place.folder.clone()],
            });
            continue;
        }
        if place.folder_aside.is_dir() {
            if place.folder.is_dir() {
                take_in_changes(name, place, &record)?;
            } else {
                build(&Original::Folder(place.folder_aside.clone()), &place.folder)?;
            }
        } else {
            if !place.folder.is_dir() {
                return Err(invalid(format!(
                    "the game has no {}",
                    place.folder.display()
                )));
            }
            if !record.owns(name) {
                record.folders.push((*name).to_owned());
                record.write(launcher_folder)?;
            }
            let building = with_suffix(&place.folder, BUILDING);
            remove_folder(&building)?;
            Original::Folder(place.folder.clone()).copy_to(&building)?;
            fs::rename(&place.folder, &place.folder_aside)?;
            fs::rename(&building, &place.folder)?;
        }
        moved.push(Moved {
            from: place.folder.clone(),
            to: place.folder_aside.clone(),
            added: Vec::new(),
        });
    }
    Ok(moved)
}

impl Part {
    /// Its name in the `Data` folder.
    fn name(&self) -> String {
        match self {
            Self::Folder(name) | Self::File(name) => name.clone(),
            Self::Archive(name) => format!("{name}.{ARCHIVE_EXTENSION}"),
        }
    }
}

impl Whole {
    /// The game's own archive: in its place (also one a game update put
    /// back), else moved aside.
    fn original(&self) -> &Path {
        if self.archive.is_file() {
            &self.archive
        } else {
            &self.aside
        }
    }

    /// Where `name` is in the `Data` folder, or goes.
    fn in_data(&self, name: &str) -> io::Result<PathBuf> {
        let found = if self.data.is_dir() {
            find_case_insensitive(&self.data, name)?
        } else {
            None
        };
        Ok(found.unwrap_or_else(|| self.data.join(name)))
    }

    /// What the engine looks for in the `Data` folder once the archive is
    /// out of its way: all that the archive holds, a part for each folder
    /// and file of its top level.
    fn parts(archive: &Archive) -> Vec<Part> {
        let mut parts = Vec::<Part>::new();
        for entry in archive.entries() {
            let part = match entry.name.split_once('/') {
                Some((folder, _))
                    if FOLDERS.iter().any(|text| text.eq_ignore_ascii_case(folder)) =>
                {
                    Part::Folder(folder.to_owned())
                }
                Some((folder, _)) => Part::Archive(folder.to_owned()),
                None => Part::File(entry.name.clone()),
            };
            if !parts.iter().any(|known| known.name() == part.name()) {
                parts.push(part);
            }
        }
        parts
    }

    /// Moves the archive aside and writes what it held into the `Data`
    /// folder. With the archive in its place, for the first time, after a
    /// stop halfway or after a game update, every part is written again;
    /// with it aside, only a part that is missing.
    fn install(&self, launcher_folder: &Path) -> io::Result<Moved> {
        let source = self.original();
        let archive = Archive::open(source)?;
        let mut record = Record::read(launcher_folder);
        let mut parts = Self::parts(&archive);
        if self.archive.is_file() {
            parts = self.parts_to_write(source, &archive, parts, &record)?;
            self.write_all(&archive, &parts, &mut record, launcher_folder)?;
        } else {
            parts.retain(|part| !matches!(part, Part::File(_)) || record.wrote(part));
            for part in &parts {
                let path = self.in_data(&part.name())?;
                let there = match part {
                    Part::Folder(_) => path.is_dir(),
                    Part::Archive(_) | Part::File(_) => path.is_file(),
                };
                if !there {
                    self.write(source, &archive, part)?;
                }
            }
        }
        let added = if record.made_data_folder {
            vec![self.data.clone()]
        } else {
            parts
                .iter()
                .map(|part| self.in_data(&part.name()))
                .collect::<io::Result<_>>()?
        };
        Ok(Moved {
            from: self.archive.clone(),
            to: self.aside.clone(),
            added,
        })
    }

    /// Writes every part from the archive in its place, then moves it
    /// aside. When a part cannot be written, the parts go again: the
    /// engine reads none of them while the archive is in its place, and
    /// they may be most of a full disk.
    fn write_all(
        &self,
        archive: &Archive,
        parts: &[Part],
        record: &mut Record,
        launcher_folder: &Path,
    ) -> io::Result<()> {
        let first_time = record.folders.is_empty() && record.added.is_empty();
        if first_time {
            self.check_room()?;
            record.made_data_folder = !self.data.is_dir();
        }
        // An archive back in its place is a game update's: the one moved
        // aside before it is no longer the game's, and what the launcher
        // wrote from that one for a folder the game no longer has goes
        // while the record still names it.
        remove_file(&self.aside)?;
        for name in &record.added {
            if !parts
                .iter()
                .any(|part| part.name().eq_ignore_ascii_case(name))
            {
                remove_file(&self.in_data(name)?)?;
            }
        }
        let (folders, added) = parts
            .iter()
            .partition::<Vec<_>, _>(|part| matches!(part, Part::Folder(_)));
        record.folders = folders.into_iter().map(Part::name).collect();
        record.added = added.into_iter().map(Part::name).collect();
        record.write(launcher_folder)?;
        parts
            .iter()
            .try_for_each(|part| self.write(&self.archive, archive, part))
            .and_then(|()| fs::rename(&self.archive, &self.aside))
            .inspect_err(|_| {
                for part in parts {
                    let _ = self.remove(part);
                }
                if record.made_data_folder {
                    let _ = fs::remove_dir(&self.data);
                }
            })
    }

    /// The archive's data is on the disk a second time once its parts are
    /// written, the archives as large as their folders were.
    fn check_room(&self) -> io::Result<()> {
        let needed = fs::metadata(&self.archive)?.len();
        let folder = self.archive.parent().unwrap_or(&self.archive);
        if fs2::available_space(folder).is_ok_and(|free| free < needed) {
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                format!(
                    "the disk has no room to write the data of {} a second time ({} MB)",
                    self.archive.display(),
                    needed.div_ceil(1024 * 1024)
                ),
            ));
        }
        Ok(())
    }

    /// The parts the launcher writes, with the archive still in its place:
    /// all but a file the game also keeps in the folder as the archive has
    /// it. Anything else of the game's own where a part goes would be read
    /// in the part's stead, or lost under it, and the game is refused.
    fn parts_to_write(
        &self,
        source: &Path,
        archive: &Archive,
        parts: Vec<Part>,
        record: &Record,
    ) -> io::Result<Vec<Part>> {
        let both = |path: &Path| {
            invalid(format!(
                "the game has both {} and {}; the launcher does not know which one it uses",
                self.archive.display(),
                path.display()
            ))
        };
        let mut to_write = Vec::new();
        for part in parts {
            let path = self.in_data(&part.name())?;
            let ours = record.wrote(&part);
            match &part {
                Part::Folder(name) => {
                    let archive = self.in_data(&format!("{name}.{ARCHIVE_EXTENSION}"))?;
                    if archive.exists() {
                        return Err(both(&archive));
                    }
                    if path.exists() && !ours {
                        return Err(both(&path));
                    }
                }
                Part::Archive(_) => {
                    if path.exists() && !ours {
                        return Err(both(&path));
                    }
                }
                Part::File(name) => {
                    if path.exists() && !ours {
                        let same = path.is_file()
                            && fs::read(&path)? == Self::file(source, archive, name)?;
                        if !same {
                            return Err(both(&path));
                        }
                        continue;
                    }
                }
            }
            to_write.push(part);
        }
        Ok(to_write)
    }

    fn file(source: &Path, archive: &Archive, name: &str) -> io::Result<Vec<u8>> {
        let entry = archive
            .entries()
            .iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| invalid(format!("the archive has no {name}")))?;
        archive.read(source, entry)
    }

    /// Writes a part under another name and renames it into place.
    fn write(&self, source: &Path, archive: &Archive, part: &Part) -> io::Result<()> {
        fs::create_dir_all(&self.data)?;
        let path = self.in_data(&part.name())?;
        match part {
            Part::Folder(name) => build(&Original::inside(source, archive, name)?, &path),
            Part::Archive(name) => {
                let building = with_suffix(&path, BUILDING);
                archive
                    .write_folder(source, name, &building)
                    .and_then(|()| fs::rename(&building, &path))
                    .inspect_err(|_| {
                        let _ = fs::remove_file(&building);
                    })
            }
            Part::File(name) => replace(&path, &Self::file(source, archive, name)?),
        }
    }

    fn remove(&self, part: &Part) -> io::Result<()> {
        let path = self.in_data(&part.name())?;
        match part {
            Part::Folder(_) => {
                remove_folder(&with_suffix(&path, BUILDING))?;
                remove_folder(&path)
            }
            Part::Archive(_) | Part::File(_) => remove_file(&path),
        }
    }
}

/// A game that ships folders is updated by files written into them, which
/// are the launcher's folders now: a file there that is neither the
/// original nor what the launcher wrote is the game's new one, and joins
/// the originals moved aside. Without the record of what the launcher
/// wrote, its own English could not be told from an update and would
/// become the game's original, so nothing is taken in then.
fn take_in_changes(name: &str, place: &Place, record: &Record) -> io::Result<()> {
    let folder = format!("{name}/");
    if !record.written.keys().any(|file| file.starts_with(&folder)) {
        return Ok(());
    }
    let originals = files_in(&place.folder_aside)?;
    for (file, path) in files_in(&place.folder)? {
        if file.ends_with(BUILDING) {
            continue;
        }
        let current = fs::read(&path)?;
        let ours = record
            .written
            .get(&format!("{name}/{file}"))
            .is_some_and(|written| written.contains(&sha256(&current)));
        let original = match originals.get(&file) {
            Some(original) => Some(fs::read(original)?),
            None => None,
        };
        if ours || original.as_deref() == Some(current.as_slice()) {
            continue;
        }
        let target = place.folder_aside.join(&file);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        replace(&target, &current)?;
    }
    Ok(())
}

/// Writes the files with text into the launcher's data folders. Only files
/// that differ are written, each whole or not at all, so a game started
/// meanwhile never reads half of one. The record of what is written comes
/// first and keeps the hash of what each file held before, so a stop
/// between the two leaves no file the launcher would not know as its own.
pub fn write(game: &Path, launcher_folder: &Path, files: &[DataFile]) -> io::Result<()> {
    let folders = applied_folders(game)?.ok_or_else(|| {
        invalid("the game's data folders are not the launcher's to write: the translation is not in place")
    })?;
    let mut record = Record::read(launcher_folder);
    let before = record.written.clone();
    let mut differing = Vec::new();
    for (name, folder) in folders {
        for file in files.iter().filter(|file| file.folder == name) {
            let path = folder.join(&file.name);
            let current = fs::read(&path).ok();
            let mut hashes = vec![sha256(&file.bytes)];
            if let Some(current) = current.filter(|current| *current != file.bytes) {
                hashes.insert(0, sha256(&current));
                differing.push((path, file));
            } else if !path.is_file() {
                differing.push((path, file));
            }
            record
                .written
                .insert(format!("{name}/{}", file.name), hashes);
        }
    }
    if record.written != before {
        record.write(launcher_folder)?;
    }
    for (path, file) in differing {
        replace(&path, &file.bytes)?;
    }
    Ok(())
}

/// Puts a moved archive or folder back. `from` is where the game had it
/// and `to` where the launcher moved it; the launcher's folder for an
/// archive is removed by the caller as one it added.
pub fn restore(from: &Path, to: &Path, launcher_folder: &Path) -> io::Result<()> {
    if to.is_file() {
        // A game update put a newer archive back: that one stays.
        if from.is_file() {
            return fs::remove_file(to);
        }
        return fs::rename(to, from);
    }
    if !to.is_dir() {
        return Ok(());
    }
    if from.is_dir() {
        let name = from
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| {
                FOLDERS
                    .iter()
                    .find(|folder| folder.eq_ignore_ascii_case(name))
            });
        if let (Some(name), Some(data)) = (name, from.parent()) {
            take_in_changes(
                name,
                &Place::find(data, name)?,
                &Record::read(launcher_folder),
            )?;
        }
        remove_folder(from)?;
    }
    fs::rename(to, from)
}
