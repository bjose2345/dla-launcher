//! The texts a WOLF RPG Editor game shows, and the same data with their
//! English written in. The game has no script layer to swap a line while
//! it runs, so both walk the data the same way and a translation is the
//! data written again: messages (their `@…` first line picks the speaker's
//! face and stays), choices, pictures drawn as text, the strings events
//! set or hand to a common event where they are only ever shown
//! ([`Usage`]) and the databases' plain string fields. Comments, file names
//! and the words events compare or look up are never touched, and neither
//! is the title: the engine knows a game's saves by it, and sets the system
//! save of a game whose title changed aside as another game's.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    io,
};

use dla_application::{
    game_translation::GameTexts,
    translation::{SourceText, has_japanese},
};
use dla_domain::translation::TextKind;

use super::{
    commands::{Command, PICTURE, SHOW_CHOICES, SHOW_MESSAGE},
    common_events::CommonEvents,
    database::{Database, FILE_NAME_FIELD, NAMED_BY_FIRST_STRING, NAMED_BY_HAND, Project},
    game_dat,
    map::Map,
    stream::{invalid, unsupported},
    text::{Encoding, plain_english},
    usage::{Base, Scope, Usage, is_picture_text, looks_like_file},
    wrap::{Room, columns, pages, wrap_into},
};

pub const BASIC_DATA: &str = "BasicData";
pub const MAP_DATA: &str = "MapData";
pub const GAME_DAT: &str = "Game.dat";
const COMMON_EVENTS: &str = "CommonEvent.dat";
/// The user and the changeable database. The system database holds the
/// engine's own settings, file names and the names of variables: of it
/// only rows' names an event shows are text.
const DATABASES: [(&str, Base); 2] = [("DataBase", Base::User), ("CDataBase", Base::Changeable)];
const SYSTEM_DATABASE: &str = "SysDatabase";
const MAP_EXTENSION: &str = ".mps";
/// The heroes of the editor's own game system: their names join the name
/// list, so dialogue spells them alike.
const HEROES: &str = "主人公ステータス";
const SENTENCE_MARKS: [char; 8] = ['。', '、', '！', '？', '!', '?', '…', '♪'];
/// A text that goes on from a name before it (`の攻撃！`).
const PARTICLES: [char; 9] = ['は', 'が', 'の', 'を', 'に', 'へ', 'と', 'も', 'で'];

/// Where the game's two data folders are read from: an archive or a folder.
pub trait Files {
    fn names(&self, folder: &str) -> io::Result<Vec<String>>;
    fn read(&self, folder: &str, name: &str) -> io::Result<Vec<u8>>;
}

pub struct DataFile {
    pub folder: &'static str,
    pub name: String,
    pub bytes: Vec<u8>,
}

struct DatabaseFile {
    base: Base,
    file: String,
    names_file: String,
    rows: Database,
    names: Project,
}

impl DatabaseFile {
    /// The types whose rows' names are text, each with its number and its
    /// name: named by hand, and read by an event to be shown.
    fn types_with_shown_row_names<'a>(
        &'a self,
        usage: &'a Usage,
    ) -> impl Iterator<Item = (usize, String)> + 'a {
        let types = self.rows.types.iter().zip(&self.names.types);
        types
            .enumerate()
            .filter_map(move |(number, (rows, names))| {
                let name = self.rows.encoding.decode(&names.name)?;
                (rows.row_naming == NAMED_BY_HAND
                    && usage.reads_row_names(self.base, number, &name))
                .then_some((number, name))
            })
    }
}

pub struct GameData {
    common_events: (String, CommonEvents),
    databases: Vec<DatabaseFile>,
    system: Option<DatabaseFile>,
    maps: Vec<(String, Map)>,
    map_places: Vec<String>,
    usage: Usage,
    room: Room,
}

pub struct Translations<'a> {
    pub texts: &'a BTreeMap<String, String>,
    pub names: &'a BTreeMap<String, String>,
}

trait Visitor {
    fn place(&mut self, _place: Option<String>) {}
    fn calls(&mut self, _common_event: u32) {}
    fn name(&mut self, name: &str) -> Option<String>;
    fn text(&mut self, text: &str, kind: TextKind) -> Option<String>;
}

impl GameData {
    pub fn read(files: &dyn Files) -> io::Result<Self> {
        let basic = files.names(BASIC_DATA)?;
        let find = |wanted: &str| {
            basic
                .iter()
                .find(|name| name.eq_ignore_ascii_case(wanted))
                .cloned()
        };
        let common_events = find(COMMON_EVENTS)
            .ok_or_else(|| invalid(format!("the game has no {BASIC_DATA}/{COMMON_EVENTS}")))?;
        let common_events = (
            common_events.clone(),
            CommonEvents::parse(&files.read(BASIC_DATA, &common_events)?)?,
        );
        // The title file is left alone, but tells a protected game.
        if let Some(name) = find(GAME_DAT)
            && !game_dat::has_known_start(&files.read(BASIC_DATA, &name)?)
        {
            return Err(unsupported(
                "Game.dat is of a version the launcher does not read, or protected",
            ));
        }
        let database = |name: &str, base: Base| -> io::Result<Option<DatabaseFile>> {
            let (Some(rows), Some(names)) = (
                find(&format!("{name}.dat")),
                find(&format!("{name}.project")),
            ) else {
                return Ok(None);
            };
            Ok(Some(DatabaseFile {
                base,
                rows: Database::parse(&files.read(BASIC_DATA, &rows)?)?,
                names: Project::parse(&files.read(BASIC_DATA, &names)?)?,
                file: rows,
                names_file: names,
            }))
        };
        let mut databases = Vec::new();
        for (name, base) in DATABASES {
            databases.extend(database(name, base)?);
        }
        let system = database(SYSTEM_DATABASE, Base::System).ok().flatten();
        let map_ids = system.as_ref().map(map_ids).unwrap_or_default();

        let mut map_names = files
            .names(MAP_DATA)?
            .into_iter()
            .filter(|name| name.to_ascii_lowercase().ends_with(MAP_EXTENSION))
            .collect::<Vec<_>>();
        map_names.sort();
        let mut maps = Vec::with_capacity(map_names.len());
        for name in map_names {
            let map = Map::parse(&files.read(MAP_DATA, &name)?).map_err(|error| {
                io::Error::new(error.kind(), format!("{MAP_DATA}/{name}: {error}"))
            })?;
            maps.push((name, map));
        }
        let map_places = maps
            .iter()
            .map(|(name, _)| match map_ids.get(&name.to_lowercase()) {
                Some(id) => format!("map:{id}"),
                None => format!("map:{}", &name[..name.len() - MAP_EXTENSION.len()]),
            })
            .collect();

        // A game the editor converted has every file in the newer encoding;
        // one that mixes the two was put together by hand.
        let encoding = common_events.1.encoding;
        if databases.iter().any(|file| file.rows.encoding != encoding)
            || maps.iter().any(|(_, map)| map.encoding != encoding)
        {
            return Err(unsupported(
                "the game's data files are not all in one text encoding",
            ));
        }
        let system = system.filter(|system| system.rows.encoding == encoding);
        let mut usage = Usage::of(&common_events.1, &maps);
        for database in &databases {
            keep_database_words(database, &mut usage);
        }
        let room = Room::of(
            message_lists(&common_events.1, &maps)
                .flatten()
                .filter(|command| command.id == SHOW_MESSAGE)
                .filter_map(|command| encoding.decode(command.strings.first()?))
                .map(|text| text.replace("\r\n", "\n"))
                .filter_map(|text| Message::of(&text).map(|message| message.body.to_owned()))
                .collect::<Vec<_>>()
                .iter()
                .map(String::as_str),
        );
        Ok(Self {
            common_events,
            databases,
            system,
            maps,
            map_places,
            usage,
            room,
        })
    }

    #[cfg(test)]
    pub fn room(&self) -> Room {
        self.room
    }

    /// Every text of the game, in the order it is walked, each listed
    /// under the maps it appears on (`map:3`): a map's own events and the
    /// common events they call.
    pub fn texts(&mut self) -> GameTexts {
        let mut collector = Collector::default();
        self.walk(&mut collector);
        collector.finish()
    }

    /// Writes the English of every text that has one into the data.
    pub fn translate(&mut self, translations: &Translations<'_>) {
        self.walk(&mut Translator(translations));
    }

    /// The files that hold text, as the data now is.
    pub fn files(&self) -> Vec<DataFile> {
        let basic = |name: &str, bytes| DataFile {
            folder: BASIC_DATA,
            name: name.to_owned(),
            bytes,
        };
        let mut files = Vec::new();
        files.push(basic(&self.common_events.0, self.common_events.1.write()));
        for database in &self.databases {
            files.push(basic(&database.file, database.rows.write()));
        }
        for database in self.databases.iter().chain(&self.system) {
            if database
                .types_with_shown_row_names(&self.usage)
                .next()
                .is_some()
            {
                files.push(basic(&database.names_file, database.names.write()));
            }
        }
        for (name, map) in &self.maps {
            files.push(DataFile {
                folder: MAP_DATA,
                name: name.clone(),
                bytes: map.write(),
            });
        }
        files
    }

    fn walk(&mut self, visitor: &mut dyn Visitor) {
        let Self {
            common_events,
            databases,
            system,
            maps,
            map_places,
            usage,
            room,
        } = self;
        visitor.place(None);
        for database in databases.iter_mut() {
            walk_database(database, usage, visitor);
        }
        for database in databases.iter_mut().chain(system) {
            walk_row_names(database, usage, visitor);
        }
        let walker = Walker {
            usage,
            room: *room,
            encoding: common_events.1.encoding,
        };
        for event in &mut common_events.1.events {
            visitor.place(Some(format!("common:{}", event.id)));
            walker.commands(Scope::CommonEvent(event.id), &mut event.commands, visitor);
        }
        for ((_, map), place) in maps.iter_mut().zip(map_places.iter()) {
            visitor.place(Some(place.clone()));
            for event in &mut map.events {
                for page in &mut event.pages {
                    walker.commands(Scope::MapEvent, &mut page.commands, visitor);
                }
            }
        }
        visitor.place(None);
    }
}

fn message_lists<'a>(
    common_events: &'a CommonEvents,
    maps: &'a [(String, Map)],
) -> impl Iterator<Item = &'a Vec<Command>> {
    common_events
        .events
        .iter()
        .map(|event| &event.commands)
        .chain(maps.iter().flat_map(|(_, map)| {
            map.events
                .iter()
                .flat_map(|event| event.pages.iter().map(|page| &page.commands))
        }))
}

/// Each map file's number: its row in the system database's first type,
/// whose first string is the file.
fn map_ids(system: &DatabaseFile) -> HashMap<String, usize> {
    let Some(rows) = system.rows.types.first() else {
        return HashMap::new();
    };
    let Some((_, place)) = rows.string_fields().next() else {
        return HashMap::new();
    };
    rows.rows
        .iter()
        .enumerate()
        .filter_map(|(id, row)| {
            let file = system.rows.encoding.decode(row.strings.get(place)?)?;
            let name = file.rsplit(['/', '\\']).next()?.to_lowercase();
            (!name.is_empty()).then_some((name, id))
        })
        .collect()
}

/// What a database calls its types, fields and hand-named rows is what
/// events look values up by, and a file field's value is a file's name.
fn keep_database_words(database: &DatabaseFile, usage: &mut Usage) {
    let encoding = database.rows.encoding;
    for (rows, names) in database.rows.types.iter().zip(&database.names.types) {
        let words = std::iter::once(&names.name).chain(&names.fields).chain(
            names
                .rows
                .iter()
                .filter(|_| rows.row_naming != NAMED_BY_FIRST_STRING),
        );
        usage.database_words.extend(
            words
                .filter_map(|word| encoding.decode(word))
                .filter(|word| !word.is_empty()),
        );
        for (field, place) in rows.string_fields() {
            let is_file = names.field_kinds.get(field) == Some(&FILE_NAME_FIELD);
            for value in rows
                .rows
                .iter()
                .filter_map(|row| encoding.decode(row.strings.get(place)?))
            {
                usage.read_database_value(&value, is_file);
            }
        }
    }
}

fn walk_database(database: &mut DatabaseFile, usage: &Usage, visitor: &mut dyn Visitor) {
    let encoding = database.rows.encoding;
    for (rows, names) in database.rows.types.iter_mut().zip(&database.names.types) {
        let heroes = encoding.decode(&names.name).as_deref() == Some(HEROES);
        let fields = rows.string_fields().collect::<Vec<_>>();
        for (index, &(field, place)) in fields.iter().enumerate() {
            // Only fields typed in by hand: not a file's name, not a
            // reference to another type.
            if names.field_kinds.get(field).is_some_and(|&kind| kind != 0) {
                continue;
            }
            for row in &mut rows.rows {
                let Some(value) = row
                    .strings
                    .get(place)
                    .and_then(|value| encoding.decode(value))
                else {
                    continue;
                };
                if !has_japanese(&value) || looks_like_file(&value) || usage.keeps(&value) {
                    continue;
                }
                let english = if heroes && index == 0 && !value.contains('\n') {
                    visitor.name(value.trim())
                } else {
                    rewrite(
                        &value,
                        Some(TextKind::Database),
                        false,
                        Some(usage),
                        visitor,
                    )
                };
                if let Some(english) = english {
                    row.strings[place] = encoding.encode(&plain_english(&english));
                }
            }
        }
    }
}

/// The names of rows an event reads to show them, as a game's own message
/// window does with a speaker's. A name the events use as a word of their
/// own stays, as a look-up by name would no longer find its row.
fn walk_row_names(database: &mut DatabaseFile, usage: &Usage, visitor: &mut dyn Visitor) {
    let encoding = database.rows.encoding;
    let base = database.base;
    for (number, type_name) in database
        .types_with_shown_row_names(usage)
        .collect::<Vec<_>>()
    {
        for name in &mut database.names.types[number].rows {
            let Some(text) = encoding.decode(name) else {
                continue;
            };
            if !has_japanese(&text)
                || looks_like_file(&text)
                || !usage.shows_row_name(base, number, &type_name, &text)
            {
                continue;
            }
            if let Some(english) = rewrite(&text, Some(TextKind::Database), false, None, visitor) {
                *name = encoding.encode(&plain_english(&english));
            }
        }
    }
}

struct Walker<'a> {
    usage: &'a Usage,
    room: Room,
    encoding: Encoding,
}

impl Walker<'_> {
    fn commands(&self, scope: Scope, commands: &mut Vec<Command>, visitor: &mut dyn Visitor) {
        let encoding = self.encoding;
        let mut at = 0;
        while at < commands.len() {
            let command = &mut commands[at];
            at += 1;
            match command.id {
                SHOW_MESSAGE => {
                    let Some(pages) = self.message(command, visitor) else {
                        continue;
                    };
                    let mut pages = pages.into_iter();
                    let Some(first) = pages.next() else {
                        continue;
                    };
                    command.strings[0] = first;
                    // English that needs more lines than the window has
                    // goes on in messages of its own.
                    let template = command.clone();
                    for page in pages {
                        let mut next = template.clone();
                        next.strings[0] = page;
                        commands.insert(at, next);
                        at += 1;
                    }
                }
                SHOW_CHOICES => {
                    for string in &mut command.strings {
                        if let Some(choice) = encoding.decode(string)
                            && has_japanese(&choice)
                            && let Some(english) = visitor.text(choice.trim(), TextKind::Choice)
                        {
                            *string = encoding.encode(&plain_english(&english));
                        }
                    }
                }
                PICTURE if is_picture_text(command) => {
                    if let Some(string) = command.strings.first_mut()
                        && let Some(text) = encoding.decode(string)
                        && let Some(english) =
                            rewrite(&text, Some(TextKind::Menu), false, None, visitor)
                    {
                        *string = encoding.encode(&plain_english(&english));
                    }
                }
                _ => {
                    if let Some(event) = self.usage.called(command) {
                        visitor.calls(event);
                    }
                    for literal in self.usage.literals(scope, command) {
                        let Some(text) = encoding.decode(&command.strings[literal.string]) else {
                            continue;
                        };
                        if !has_japanese(&text)
                            || looks_like_file(&text)
                            || !self.usage.is_shown(literal.target, &text)
                        {
                            continue;
                        }
                        if let Some(english) = rewrite(&text, None, literal.appended, None, visitor)
                        {
                            command.strings[literal.string] =
                                encoding.encode(&plain_english(&english));
                        }
                    }
                }
            }
        }
    }

    fn message(&self, command: &Command, visitor: &mut dyn Visitor) -> Option<Vec<Vec<u8>>> {
        let text = self
            .encoding
            .decode(command.strings.first()?)?
            .replace("\r\n", "\n");
        let message = Message::of(&text)?;
        if let Some(speaker) = message.speaker.and_then(|line| line.strip_prefix('@'))
            && has_japanese(speaker)
        {
            visitor.name(speaker.trim());
        }
        if !has_japanese(message.body) {
            return None;
        }
        let english = visitor.text(message.body, TextKind::Dialogue)?;
        Some(
            pages(&plain_english(&english), self.room)
                .into_iter()
                .map(|page| match message.speaker {
                    Some(speaker) => self.encoding.encode(&format!("{speaker}\n{page}")),
                    None => self.encoding.encode(&page),
                })
                .collect(),
        )
    }
}

/// A message as the editor's own system reads it: a first line starting
/// with `@` picks the speaker's face and window and is not shown.
struct Message<'a> {
    speaker: Option<&'a str>,
    body: &'a str,
}

impl<'a> Message<'a> {
    fn of(text: &'a str) -> Option<Self> {
        let (speaker, body) = match text.split_once('\n') {
            Some((first, rest)) if first.starts_with('@') => (Some(first), rest),
            None if text.starts_with('@') => return None,
            _ => (None, text),
        };
        let body = body.trim_end();
        (!body.is_empty()).then_some(Self { speaker, body })
    }
}

/// Writes English into a string that is not a message, part by part:
/// lines that read as prose together (one of them has a sentence's marks)
/// are one text, wrapped again over the lines they had; other lines are a
/// label each and keep their indent; lines without Japanese stay.
fn rewrite(
    original: &str,
    kind: Option<TextKind>,
    appended: bool,
    usage: Option<&Usage>,
    visitor: &mut dyn Visitor,
) -> Option<String> {
    let newline = if original.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lines = original
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    let is_kept = |text: &str| usage.is_some_and(|usage| usage.keeps(text));
    let kind_of = |text: &str| {
        kind.unwrap_or(if text.contains(SENTENCE_MARKS) {
            TextKind::Dialogue
        } else {
            TextKind::Menu
        })
    };
    let mut written = Vec::with_capacity(lines.len());
    let mut changed = false;
    let mut at = 0;
    while at < lines.len() {
        let group = lines[at..]
            .iter()
            .take_while(|line| has_japanese(line))
            .count();
        if group == 0 {
            written.push(lines[at].to_owned());
            at += 1;
            continue;
        }
        let group = &lines[at..at + group];
        at += group.len();
        let prose = group.len() > 1 && group.iter().any(|line| line.contains(SENTENCE_MARKS));
        if prose {
            let text = group.join("\n");
            match visitor.text(&text, kind_of(&text)) {
                Some(english) => {
                    let width = group.iter().map(|line| columns(line)).max().unwrap_or(0);
                    written.extend(wrap_into(&english, group.len(), width));
                    changed = true;
                }
                None => written.extend(group.iter().map(|line| (*line).to_owned())),
            }
            continue;
        }
        for line in group {
            let text = line.trim();
            let english = (!is_kept(text))
                .then(|| visitor.text(text, kind_of(text)))
                .flatten();
            match english {
                Some(english) => {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    let after = &line[line.trim_end().len()..];
                    let joins = lines.len() == 1
                        && (appended || follows_a_name(text))
                        && english.starts_with(|first: char| first.is_ascii_alphanumeric());
                    let space = if joins { " " } else { "" };
                    written.push(format!("{indent}{space}{english}{after}"));
                    changed = true;
                }
                None => written.push((*line).to_owned()),
            }
        }
    }
    changed.then(|| written.join(newline))
}

fn follows_a_name(text: &str) -> bool {
    text.starts_with(PARTICLES)
        && text.chars().count() >= 4
        && text.ends_with(['。', '！', '？', '!', '?', '…'])
}

#[derive(Default)]
struct Collector {
    place: Option<String>,
    seen: HashSet<String>,
    texts: Vec<SourceText>,
    seen_names: HashSet<String>,
    names: Vec<String>,
    places: BTreeMap<String, BTreeSet<String>>,
    by_place: HashMap<String, BTreeSet<String>>,
    calls: HashMap<String, BTreeSet<u32>>,
}

impl Visitor for Collector {
    fn place(&mut self, place: Option<String>) {
        self.place = place;
    }

    fn calls(&mut self, common_event: u32) {
        if let Some(place) = &self.place {
            self.calls
                .entry(place.clone())
                .or_default()
                .insert(common_event);
        }
    }

    fn name(&mut self, name: &str) -> Option<String> {
        if has_japanese(name) && self.seen_names.insert(name.to_owned()) {
            self.names.push(name.to_owned());
        }
        None
    }

    fn text(&mut self, text: &str, kind: TextKind) -> Option<String> {
        if !has_japanese(text) {
            return None;
        }
        if let Some(place) = &self.place {
            self.places
                .entry(text.to_owned())
                .or_default()
                .insert(place.clone());
            self.by_place
                .entry(place.clone())
                .or_default()
                .insert(text.to_owned());
        }
        if self.seen.insert(text.to_owned()) {
            self.texts.push(SourceText {
                text: text.to_owned(),
                kind,
            });
        }
        None
    }
}

impl Collector {
    fn finish(mut self) -> GameTexts {
        let maps = self
            .calls
            .keys()
            .chain(self.by_place.keys())
            .filter(|place| place.starts_with("map:"))
            .cloned()
            .collect::<BTreeSet<_>>();
        for map in maps {
            let mut seen = HashSet::new();
            let mut next = self
                .calls
                .get(&map)
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            while let Some(common_event) = next.pop() {
                if !seen.insert(common_event) {
                    continue;
                }
                let place = format!("common:{common_event}");
                for text in self.by_place.get(&place).into_iter().flatten() {
                    self.places
                        .entry(text.clone())
                        .or_default()
                        .insert(map.clone());
                }
                next.extend(self.calls.get(&place).into_iter().flatten().copied());
            }
        }
        GameTexts {
            texts: self.texts,
            names: self.names,
            places: self
                .places
                .into_iter()
                .map(|(text, places)| (text, places.into_iter().collect()))
                .collect(),
        }
    }
}

struct Translator<'a, 'b>(&'a Translations<'b>);

impl Visitor for Translator<'_, '_> {
    fn name(&mut self, name: &str) -> Option<String> {
        self.0
            .names
            .get(name)
            .or_else(|| self.0.texts.get(name))
            .filter(|english| !english.trim().is_empty())
            .cloned()
    }

    fn text(&mut self, text: &str, _kind: TextKind) -> Option<String> {
        self.0
            .texts
            .get(text)
            .or_else(|| self.0.names.get(text))
            .filter(|english| !english.trim().is_empty())
            .cloned()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::wolf::{
        common_events::testing::{common_events, newer as newer_events},
        database::testing::{
            Field, database, database_type, newer as newer_database, number, text,
        },
        game_dat::testing::game_dat,
        map::testing::{map, newer as newer_map},
        text::in_utf8,
    };

    /// A game's two data folders in memory.
    #[derive(Clone, Default)]
    pub struct Memory(pub BTreeMap<(String, String), Vec<u8>>);

    impl Memory {
        pub fn put(&mut self, folder: &str, name: &str, bytes: Vec<u8>) {
            self.0.insert((folder.to_owned(), name.to_owned()), bytes);
        }

        pub fn get(&self, folder: &str, name: &str) -> &[u8] {
            &self.0[&(folder.to_owned(), name.to_owned())]
        }
    }

    impl Files for Memory {
        fn names(&self, folder: &str) -> io::Result<Vec<String>> {
            Ok(self
                .0
                .keys()
                .filter(|(in_folder, _)| in_folder == folder)
                .map(|(_, name)| name.clone())
                .collect())
        }

        fn read(&self, folder: &str, name: &str) -> io::Result<Vec<u8>> {
            self.0
                .get(&(folder.to_owned(), name.to_owned()))
                .cloned()
                .ok_or_else(|| invalid(format!("no {folder}/{name}")))
        }
    }

    /// A small game: a title, items and heroes, a common event with a
    /// message, a choice and a shown string, and a town map that calls it.
    pub fn game() -> Memory {
        build(false)
    }

    /// The same game as version 3.5 of the editor keeps it: UTF-8, most
    /// files packed, one database not.
    pub fn newer_game() -> Memory {
        in_utf8(|| build(true))
    }

    /// The small game with a message window of its own: a common event
    /// that reads a speaker's name, the name of a row of the system
    /// database's second type, and shows it; another finds a row by name.
    pub fn game_with_speakers() -> Memory {
        use crate::wolf::commands::{
            DATABASE, PICTURE,
            testing::{command, message},
        };
        let mut files = game();
        let events = common_events(vec![
            vec![message("@2\nこんにちは、\n旅の人。")],
            vec![
                command(
                    DATABASE,
                    &[1, 1_600_010, 0xffff_fffd, 0x1100, 1_600_005],
                    &["", "", "", ""],
                ),
                command(PICTURE, &[0x20, 1], &["\\f[17]\\cself[5]"]),
                command(
                    DATABASE,
                    &[1, 0, 0, 0x2_1100, 2_000_000],
                    &["", "", "受付", ""],
                ),
            ],
        ]);
        files.put(BASIC_DATA, "CommonEvent.dat", events.write());
        let faces = Field {
            name: "顔画像ファイル",
            kind: FILE_NAME_FIELD,
            strings: Some(vec!["", "", "tatie/a.png", "tatie/b.png", ""]),
        };
        let (rows, names) = database(vec![
            database_type(
                "マップ設定",
                0,
                &["町", "洞窟"],
                vec![Field {
                    name: "マップファイル名",
                    kind: FILE_NAME_FIELD,
                    strings: Some(vec!["MapData/Cave.mps", "MapData/Town.mps"]),
                }],
            ),
            database_type(
                "顔グラフィック名",
                0,
                &["声", "\\s[5]", "男Ａ", "受付", "tatie/男.png"],
                vec![faces],
            ),
        ]);
        files.put(BASIC_DATA, "SysDatabase.dat", rows.write());
        files.put(BASIC_DATA, "SysDatabase.project", names.write());
        files
    }

    fn build(newer: bool) -> Memory {
        use crate::wolf::commands::{
            CALL_EVENT, COMMENT, SET_STRING, SHOW_CHOICES,
            testing::{command, message},
        };
        let events_file = |events: CommonEvents| match newer {
            true => newer_events(events).write(),
            false => events.write(),
        };
        let rows_file = |rows: Database, packed: bool| match newer {
            true => newer_database(rows, packed).write(),
            false => rows.write(),
        };
        let mut files = Memory::default();
        let mut title = game_dat("サキュバスの館");
        if newer {
            title[9] = b'U';
        }
        files.put(BASIC_DATA, "Game.dat", title);
        files.put(BASIC_DATA, "icon000.png", b"png".to_vec());
        let events = common_events(vec![
            vec![
                message("@1\nこんにちは、\n旅の人。"),
                command(COMMENT, &[], &["ここは注釈"]),
                command(SHOW_CHOICES, &[2], &["はい", "いいえ"]),
                command(SET_STRING, &[1_600_009, 0x100], &["が足りない！"]),
                message("\\cself[9]"),
            ],
            Vec::new(),
        ]);
        files.put(BASIC_DATA, "CommonEvent.dat", events_file(events));
        let (rows, names) = database(vec![
            database_type(
                "アイテム",
                NAMED_BY_FIRST_STRING,
                &["薬草", "鍵"],
                vec![
                    text("名前", &["薬草", "鍵"]),
                    number("値段"),
                    text("説明", &["HPを少し回復する。\r\nとても苦い。", ""]),
                    Field {
                        name: "画像",
                        kind: FILE_NAME_FIELD,
                        strings: Some(vec!["Picture/薬草.png", ""]),
                    },
                    text("使用時", &["の傷が癒えた！", ""]),
                ],
            ),
            database_type("場所", 0, &["二階"], vec![text("名", &["居間"])]),
        ]);
        files.put(BASIC_DATA, "DataBase.dat", rows_file(rows, true));
        files.put(BASIC_DATA, "DataBase.project", names.write());
        let (rows, names) = database(vec![database_type(
            HEROES,
            NAMED_BY_FIRST_STRING,
            &["アリス"],
            vec![
                text("キャラ名", &["アリス"]),
                text("肩書き", &["見習い魔女"]),
            ],
        )]);
        files.put(BASIC_DATA, "CDataBase.dat", rows_file(rows, false));
        files.put(BASIC_DATA, "CDataBase.project", names.write());
        let (rows, names) = database(vec![database_type(
            "マップ設定",
            0,
            &["町", "洞窟"],
            vec![Field {
                name: "マップファイル名",
                kind: FILE_NAME_FIELD,
                strings: Some(vec!["MapData/Cave.mps", "MapData/Town.mps"]),
            }],
        )]);
        files.put(BASIC_DATA, "SysDatabase.dat", rows_file(rows, true));
        files.put(BASIC_DATA, "SysDatabase.project", names.write());
        let town = map(vec![vec![
            message("町へようこそ。"),
            command(CALL_EVENT, &[500_000, 0], &[]),
        ]]);
        files.put(
            MAP_DATA,
            "Town.mps",
            match newer {
                true => newer_map(town, 3).write(),
                false => town.write(),
            },
        );
        files
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::*, *};
    use crate::wolf::{
        commands::{
            CALL_EVENT, CALL_EVENT_BY_NAME, DATABASE, DEBUG_MESSAGE, PICTURE, SET_STRING,
            STRING_CONDITION,
            testing::{command, message},
        },
        common_events::testing::common_events,
        map::testing::map,
        text::{decode, encode, in_utf8},
    };

    fn english(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(japanese, english)| ((*japanese).to_owned(), (*english).to_owned()))
            .collect()
    }

    fn translated(files: &Memory, texts: &[(&str, &str)], names: &[(&str, &str)]) -> Memory {
        let mut data = GameData::read(files).unwrap();
        data.translate(&Translations {
            texts: &english(texts),
            names: &english(names),
        });
        let mut written = files.clone();
        for file in data.files() {
            written.put(file.folder, &file.name, file.bytes);
        }
        written
    }

    fn with_events(events: Vec<Vec<Command>>) -> Memory {
        let mut files = game();
        files.put(BASIC_DATA, "CommonEvent.dat", common_events(events).write());
        files
    }

    fn common_event_strings(files: &Memory, event: usize) -> Vec<Vec<String>> {
        CommonEvents::parse(files.get(BASIC_DATA, "CommonEvent.dat"))
            .unwrap()
            .events[event]
            .commands
            .iter()
            .map(|command| {
                command
                    .strings
                    .iter()
                    .map(|string| decode(string).unwrap())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn collects_what_the_player_reads_and_nothing_else() {
        let texts = GameData::read(&game()).unwrap().texts();
        let found = texts
            .texts
            .iter()
            .map(|text| (text.text.as_str(), text.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                ("薬草", TextKind::Database),
                ("鍵", TextKind::Database),
                ("HPを少し回復する。\nとても苦い。", TextKind::Database),
                ("の傷が癒えた！", TextKind::Database),
                ("居間", TextKind::Database),
                ("見習い魔女", TextKind::Database),
                ("こんにちは、\n旅の人。", TextKind::Dialogue),
                ("はい", TextKind::Choice),
                ("いいえ", TextKind::Choice),
                ("が足りない！", TextKind::Dialogue),
                ("町へようこそ。", TextKind::Dialogue),
            ]
        );
        assert_eq!(texts.names, ["アリス"]);
    }

    #[test]
    fn lists_each_text_under_the_maps_that_show_it() {
        // The cave calls event 1 by name, which calls event 2; the town
        // calls event 0. `Extra.mps` is not in the system database.
        let mut files = with_events(vec![
            vec![message("共通０")],
            vec![message("共通１"), command(CALL_EVENT, &[500_002, 0], &[])],
            vec![message("共通２")],
            vec![message("誰も呼ばない")],
        ]);
        files.put(
            MAP_DATA,
            "Cave.mps",
            map(vec![vec![
                message("洞窟だ。"),
                command(CALL_EVENT_BY_NAME, &[0, 0], &["コモン1"]),
            ]])
            .write(),
        );
        files.put(
            MAP_DATA,
            "Extra.mps",
            map(vec![vec![message("おまけ")]]).write(),
        );
        let texts = GameData::read(&files).unwrap().texts();
        let places = |text: &str| texts.places.get(text).cloned().unwrap_or_default();
        assert_eq!(places("洞窟だ。"), ["map:0"]);
        assert_eq!(places("町へようこそ。"), ["map:1"]);
        assert_eq!(places("おまけ"), ["map:Extra"]);
        assert_eq!(places("共通０"), ["common:0", "map:1"]);
        assert_eq!(places("共通１"), ["common:1", "map:0"]);
        assert_eq!(places("共通２"), ["common:2", "map:0"]);
        assert_eq!(places("誰も呼ばない"), ["common:3"]);
        assert!(places("薬草").is_empty());
    }

    #[test]
    fn leaves_comments_file_names_and_the_words_events_use() {
        let files = with_events(vec![vec![
            command(DEBUG_MESSAGE, &[], &["デバッグ中"]),
            command(PICTURE, &[0, 1, 0], &["Picture/黒い影.png"]),
            command(PICTURE, &[0x20, 2, 0], &["\\c[2]セーブしますか"]),
            command(PICTURE, &[0x21, 2, 0], &[]),
            // A word the events compare, set into a variable that is shown.
            command(SET_STRING, &[3_000_007, 0], &["二階"]),
            message("\\s[7]にいる。"),
            command(STRING_CONDITION, &[1, 3_000_007], &["二階"]),
            // A database row looked up by its name.
            command(
                DATABASE,
                &[0, 0, 0, 0x21000, 1_600_010],
                &["", "アイテム", "鍵", ""],
            ),
            command(SET_STRING, &[1_600_006, 0], &["BGM/戦闘.ogg"]),
        ]]);
        let texts = GameData::read(&files).unwrap().texts();
        let found = texts
            .texts
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                "薬草",
                "HPを少し回復する。\nとても苦い。",
                "の傷が癒えた！",
                "居間",
                "見習い魔女",
                "\\c[2]セーブしますか",
                "\\s[7]にいる。",
                "町へようこそ。",
            ]
        );
    }

    #[test]
    fn keeps_a_name_that_is_a_file_of_a_game_that_builds_file_names() {
        let files = with_events(vec![vec![command(
            PICTURE,
            &[0, 1, 0],
            &["Picture/\\cself[5].png"],
        )]]);
        let texts = GameData::read(&files).unwrap().texts();
        assert!(texts.texts.iter().all(|text| text.text != "薬草"));
        assert!(texts.texts.iter().any(|text| text.text == "居間"));
    }

    #[test]
    fn a_string_a_database_writes_into_a_file_name_is_not_text() {
        use crate::wolf::database::testing::{Field, database, database_type};
        let mut files = with_events(vec![vec![
            command(SET_STRING, &[3_000_004, 0], &["笑顔"]),
            message("\\s[4]だね。"),
        ]]);
        let shown = |files: &Memory| {
            GameData::read(files)
                .unwrap()
                .texts()
                .texts
                .iter()
                .any(|text| text.text == "笑顔")
        };
        assert!(shown(&files));
        let (rows, names) = database(vec![database_type(
            "立ち絵",
            0,
            &["表情"],
            vec![Field {
                name: "画像",
                kind: FILE_NAME_FIELD,
                strings: Some(vec!["Picture/顔_\\s[4].png"]),
            }],
        )]);
        files.put(BASIC_DATA, "DataBase.dat", rows.write());
        files.put(BASIC_DATA, "DataBase.project", names.write());
        assert!(!shown(&files));
    }

    #[test]
    fn writes_the_game_back_as_it_was_without_translations() {
        let files = game();
        let written = translated(&files, &[], &[]);
        assert_eq!(written.0, files.0);
        // Only the files with text are written at all.
        let names = GameData::read(&files)
            .unwrap()
            .files()
            .into_iter()
            .map(|file| format!("{}/{}", file.folder, file.name))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "BasicData/CommonEvent.dat",
                "BasicData/DataBase.dat",
                "BasicData/CDataBase.dat",
                "MapData/Town.mps",
            ]
        );
    }

    #[test]
    fn writes_english_where_the_japanese_was() {
        let files = game();
        let written = translated(
            &files,
            &[
                ("サキュバスの館", "The Succubus Mansion"),
                ("薬草", "Herb’s leaf"),
                (
                    "HPを少し回復する。\nとても苦い。",
                    "Restores a little HP. It tastes very bitter.",
                ),
                ("の傷が癒えた！", "’s wounds have healed!"),
                ("こんにちは、\n旅の人。", "Hello there, traveller."),
                ("はい", "Yes"),
                ("が足りない！", "is not enough!"),
                ("町へようこそ。", "Welcome to the town."),
            ],
            &[("アリス", "Alice")],
        );
        assert_eq!(
            common_event_strings(&written, 0),
            [
                vec!["@1\nHello there, traveller."],
                vec!["ここは注釈"],
                vec!["Yes", "いいえ"],
                vec![" is not enough!"],
                vec!["\\cself[9]"],
            ]
        );
        let items = Database::parse(written.get(BASIC_DATA, "DataBase.dat")).unwrap();
        let row = |row: usize| {
            items.types[0].rows[row]
                .strings
                .iter()
                .map(|string| decode(string).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            row(0),
            [
                "Herb's leaf",
                "Restores a little HP.\r\nIt tastes very bitter.",
                "Picture/薬草.png",
                "'s wounds have healed!"
            ]
        );
        assert_eq!(row(1)[0], "鍵");
        let heroes = Database::parse(written.get(BASIC_DATA, "CDataBase.dat")).unwrap();
        assert_eq!(heroes.types[0].rows[0].strings[0], b"Alice");
        assert_eq!(heroes.types[0].rows[0].strings[1], encode("見習い魔女"));
        // The title file, the project files and everything without text
        // are not written.
        for name in [
            "Game.dat",
            "DataBase.project",
            "CDataBase.project",
            "icon000.png",
        ] {
            assert_eq!(written.get(BASIC_DATA, name), files.get(BASIC_DATA, name));
        }
        // English in the data reads as the same game: nothing new to
        // translate, and the Japanese left is still found.
        let again = GameData::read(&written).unwrap().texts();
        let left = again
            .texts
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(left, ["鍵", "居間", "見習い魔女", "いいえ"]);
    }

    #[test]
    fn a_message_too_long_for_the_window_goes_on_in_the_next() {
        let long = "One two three four five six seven eight nine ten eleven twelve thirteen \
                    fourteen fifteen sixteen seventeen eighteen nineteen twenty twenty-one \
                    twenty-two twenty-three twenty-four twenty-five twenty-six.";
        let files = with_events(vec![vec![
            message("@3\n\\c[2]長い長い話。"),
            message("次の話。"),
        ]]);
        let written = translated(
            &files,
            &[
                ("\\c[2]長い長い話。", &format!("\\c[2]{long}")),
                ("次の話。", "The next one."),
            ],
            &[],
        );
        let strings = common_event_strings(&written, 0);
        assert_eq!(strings.len(), 3);
        let pages = [&strings[0][0], &strings[1][0]];
        for page in pages {
            let (speaker, body) = page.split_once('\n').unwrap();
            assert_eq!(speaker, "@3");
            assert!(body.starts_with("\\c[2]"));
            assert!(body.lines().count() <= 3);
            assert!(body.lines().all(|line| columns(line) <= 40), "{body}");
        }
        let rejoined = pages
            .iter()
            .map(|page| {
                page.split_once('\n')
                    .unwrap()
                    .1
                    .trim_start_matches("\\c[2]")
            })
            .flat_map(str::lines)
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            rejoined,
            long.split_whitespace().collect::<Vec<_>>().join(" ")
        );
        assert_eq!(strings[2], ["The next one."]);
    }

    #[test]
    fn translates_labels_a_line_each_and_prose_as_one() {
        let files = with_events(vec![vec![
            command(
                PICTURE,
                &[0x20, 1, 0],
                &["　はじめから\nつづきから\nver 1.02\nおわる"],
            ),
            command(
                PICTURE,
                &[0x20, 2, 0],
                &["窓から逃げるのは無理だ。\n下には化け物がいる。\n\nでも他に道はない。"],
            ),
        ]]);
        let written = translated(
            &files,
            &[
                ("はじめから", "New game"),
                ("おわる", "Quit"),
                (
                    "窓から逃げるのは無理だ。\n下には化け物がいる。",
                    "Escaping through the window is impossible. A monster waits below.",
                ),
            ],
            &[],
        );
        assert_eq!(
            common_event_strings(&written, 0),
            [
                vec!["　New game\nつづきから\nver 1.02\nQuit"],
                vec![
                    "Escaping through the window is\nimpossible. A monster waits below.\n\nでも他に道はない。"
                ],
            ]
        );
    }

    #[test]
    fn reads_and_translates_a_game_of_version_3_5_as_it_does_an_older_one() {
        let files = newer_game();
        assert_eq!(
            files.get(BASIC_DATA, "CommonEvent.dat")[..11],
            *b"\0W\0\0OLUFC\0\x93"
        );
        assert_eq!(files.get(BASIC_DATA, "DataBase.dat")[10], 0xc4);
        assert_eq!(files.get(BASIC_DATA, "CDataBase.dat")[10], 0xc2);
        assert_eq!(files.get(MAP_DATA, "Town.mps")[16..21], *b"U\0\0\0g");
        assert_eq!(
            GameData::read(&files).unwrap().texts(),
            GameData::read(&game()).unwrap().texts()
        );
        assert_eq!(translated(&files, &[], &[]).0, files.0);

        let pairs = [
            ("薬草", "Herb’s leaf"),
            ("こんにちは、\n旅の人。", "Hello there, traveller."),
            ("はい", "Yes"),
            ("が足りない！", "is not enough!"),
            ("町へようこそ。", "Welcome to the town."),
        ];
        let written = translated(&files, &pairs, &[("アリス", "Alice")]);
        // Each file keeps the form it had: packed where it was, and UTF-8.
        for (folder, name) in [
            (BASIC_DATA, "CommonEvent.dat"),
            (BASIC_DATA, "DataBase.dat"),
            (BASIC_DATA, "CDataBase.dat"),
            (MAP_DATA, "Town.mps"),
        ] {
            assert_eq!(
                written.get(folder, name)[..11],
                files.get(folder, name)[..11],
                "{name}"
            );
        }
        let strings = |files: &Memory| in_utf8(|| common_event_strings(files, 0));
        assert_eq!(
            strings(&written),
            [
                vec!["@1\nHello there, traveller."],
                vec!["ここは注釈"],
                vec!["Yes", "いいえ"],
                vec![" is not enough!"],
                vec!["\\cself[9]"],
            ]
        );
        let town = Map::parse(written.get(MAP_DATA, "Town.mps")).unwrap();
        assert_eq!(
            town.events[0].pages[0].commands[0].strings[0],
            b"Welcome to the town."
        );
        let heroes = Database::parse(written.get(BASIC_DATA, "CDataBase.dat")).unwrap();
        assert_eq!(heroes.types[0].rows[0].strings[0], b"Alice");
        // The same English, whichever version the game is.
        let older = translated(&game(), &pairs, &[("アリス", "Alice")]);
        assert_eq!(strings(&written), common_event_strings(&older, 0));
        assert_eq!(
            GameData::read(&written).unwrap().texts(),
            GameData::read(&older).unwrap().texts()
        );
    }

    #[test]
    fn translates_the_names_of_rows_an_event_reads_to_show() {
        let files = game_with_speakers();
        let texts = GameData::read(&files).unwrap().texts();
        let kind_of = |wanted: &str| {
            texts
                .texts
                .iter()
                .find(|text| text.text == wanted)
                .map(|text| text.kind)
        };
        assert_eq!(kind_of("男Ａ"), Some(TextKind::Database));
        assert_eq!(kind_of("声"), Some(TextKind::Database));
        // Found by its name, a file's name, a code, and a type whose names
        // no event shows.
        for left in ["受付", "tatie/男.png", "\\s[5]", "町"] {
            assert_eq!(kind_of(left), None, "{left}");
        }
        assert_eq!(translated(&files, &[], &[]).0, files.0);

        let written = translated(&files, &[("男Ａ", "Man A"), ("声", "Voice")], &[]);
        let names = |files: &Memory, of: usize| {
            Project::parse(files.get(BASIC_DATA, "SysDatabase.project"))
                .unwrap()
                .types[of]
                .rows
                .iter()
                .map(|name| decode(name).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&written, 1),
            ["Voice", "\\s[5]", "Man A", "受付", "tatie/男.png"]
        );
        assert_eq!(names(&written, 0), ["町", "洞窟"]);
        // Only the names changed, and only that file.
        let mut expected = files.clone();
        expected.put(
            BASIC_DATA,
            "SysDatabase.project",
            written.get(BASIC_DATA, "SysDatabase.project").to_vec(),
        );
        assert_eq!(written.0, expected.0);
        // A game whose events show no row's name has no such file written.
        assert!(
            GameData::read(&game())
                .unwrap()
                .files()
                .iter()
                .all(|file| !file.name.ends_with(".project"))
        );
    }

    #[test]
    fn refuses_a_game_whose_files_are_not_in_one_encoding() {
        let mut files = newer_game();
        files.put(
            MAP_DATA,
            "Town.mps",
            game().get(MAP_DATA, "Town.mps").to_vec(),
        );
        assert_eq!(
            GameData::read(&files).err().unwrap().kind(),
            io::ErrorKind::Unsupported
        );
        let mut files = game();
        files.put(
            BASIC_DATA,
            "CDataBase.dat",
            newer_game().get(BASIC_DATA, "CDataBase.dat").to_vec(),
        );
        assert_eq!(
            GameData::read(&files).err().unwrap().kind(),
            io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn refuses_protected_data_and_reads_past_an_unknown_title_file() {
        let mut files = game();
        files.put(
            BASIC_DATA,
            "Game.dat",
            b"\0W\0\0OL\0FM\0 not the known layout".to_vec(),
        );
        let texts = GameData::read(&files).unwrap().texts();
        assert_eq!(texts.texts[0].text, "薬草");

        let mut protected = game();
        protected.put(BASIC_DATA, "Game.dat", vec![0x7c; 64]);
        assert_eq!(
            GameData::read(&protected).err().unwrap().kind(),
            io::ErrorKind::Unsupported
        );
        let mut protected = game();
        protected.put(BASIC_DATA, "DataBase.dat", vec![0x7c; 64]);
        assert_eq!(
            GameData::read(&protected).err().unwrap().kind(),
            io::ErrorKind::Unsupported
        );
        let mut missing = game();
        missing
            .0
            .remove(&(BASIC_DATA.to_owned(), "CommonEvent.dat".to_owned()));
        assert_eq!(
            GameData::read(&missing).err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
