//! The texts an RPG Maker VX Ace game shows, read from its archive or its
//! `Data` folder in the form the launcher's script looks them up while the
//! game runs: a message is its lines joined with `\n`, menu words and
//! database entries as they are.
//!
//! Each map's dialogue, and the shared events the map calls, are listed
//! under the map (`map:12`), so they are translated next once the player is
//! there. The Japanese strings in the game's own scripts are read too (a
//! label, a menu's help); the script shows their English only where they are
//! drawn, so a database name the scripts compare stays Japanese in the data
//! ([`strings_in_scripts`]).

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

use dla_application::{
    game_translation::GameTexts,
    translation::{SourceText, has_japanese},
};
use dla_domain::translation::TextKind;
use flate2::read::ZlibDecoder;

use crate::{
    rgss3a::Archive,
    ruby_marshal::{self, Value},
};

const SHOW_TEXT: i64 = 101;
const TEXT_LINE: i64 = 401;
const SHOW_CHOICES: i64 = 102;
const SCROLL_TEXT: i64 = 105;
const SCROLL_LINE: i64 = 405;
const CALL_COMMON_EVENT: i64 = 117;
const CHANGE_NAME: i64 = 320;
const CHANGE_NICKNAME: i64 = 324;
const SCRIPT: i64 = 355;
const SCRIPT_LINE: i64 = 655;
/// The launcher's own script section in a patched archive.
pub const OUR_SECTION: i64 = 77_000_001;
const VOCAB: &str = "Vocab";

/// Where a VX Ace game keeps its data.
pub enum GameData {
    Archive { path: PathBuf, archive: Archive },
    Folder(PathBuf),
}

impl GameData {
    /// The archive in the game's folder (named after its `.exe`), or else
    /// its `Data` folder.
    pub fn find(game: &Path) -> io::Result<Self> {
        if let Some(path) = archive_in(game)? {
            let archive = Archive::open(&path)?;
            return Ok(Self::Archive { path, archive });
        }
        Ok(Self::Folder(game.to_owned()))
    }

    /// A file by its path in the game (`Data/Map001.rvdata2`); `None` when
    /// the game has no such file.
    pub fn read(&self, name: &str) -> io::Result<Option<Vec<u8>>> {
        match self {
            Self::Archive { path, archive } => archive
                .entry(name)
                .map(|entry| archive.read(path, entry))
                .transpose(),
            Self::Folder(game) => match find_case_insensitive(game, name)? {
                Some(path) => fs::read(path).map(Some),
                None => Ok(None),
            },
        }
    }

    pub(crate) fn maps(&self) -> io::Result<Vec<(u32, String)>> {
        let names = match self {
            Self::Archive { archive, .. } => archive
                .entries()
                .iter()
                .map(|entry| entry.name.replace('\\', "/"))
                .collect::<Vec<_>>(),
            Self::Folder(game) => match find_case_insensitive(game, "Data")? {
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
        let mut maps = names
            .into_iter()
            .filter_map(|name| {
                let number = name
                    .rsplit('/')
                    .next()?
                    .strip_prefix("Map")?
                    .strip_suffix(".rvdata2")?;
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
pub fn archive_in(game: &Path) -> io::Result<Option<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(game)? {
        let path = entry?.path();
        if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rgss3a"))
            && path.is_file()
        {
            found.push(path);
        }
    }
    found.sort();
    Ok(found.into_iter().next())
}

/// The Japanese strings in the game's scripts but `Vocab`'s: what the
/// scripts draw, and the database names they compare, which must stay
/// Japanese in the data.
pub fn strings_in_scripts(game: &Path) -> io::Result<HashSet<String>> {
    let data = GameData::find(game)?;
    Ok(script_literals(&script_sections(
        &data.load("Data/Scripts.rvdata2")?,
    )))
}

fn script_literals(scripts: &[(String, String)]) -> HashSet<String> {
    scripts
        .iter()
        .filter(|(title, _)| title != VOCAB)
        .flat_map(|(_, source)| script_strings(source))
        .filter(|text| has_japanese(text))
        .collect()
}

/// A script's string that is drawn as it is: a file name or a note tag
/// (`<封印>`) never is, and one character alone (人, 個) only beside a number.
fn drawn_whole(text: &str) -> bool {
    has_japanese(text) && !text.contains('/') && !text.starts_with('<') && text.chars().count() > 1
}

pub fn read(game: &Path) -> io::Result<GameTexts> {
    let data = GameData::find(game)?;
    let scripts = script_sections(&data.load("Data/Scripts.rvdata2")?);
    let kept = script_literals(&scripts);
    let mut texts = Texts {
        kept,
        ..Texts::default()
    };

    let system = data.load("Data/System.rvdata2")?;
    if system == Value::Nil {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the game has no Data/System.rvdata2",
        ));
    }
    for field in ["@basic", "@params", "@etypes", "@commands"] {
        texts.each(system.field("@terms").field(field), TextKind::Menu);
    }
    for list in ["@elements", "@skill_types", "@weapon_types", "@armor_types"] {
        texts.each(system.field(list), TextKind::Menu);
    }
    texts.text(system.field("@currency_unit"), TextKind::Menu);
    texts.text(system.field("@game_title"), TextKind::Menu);
    for (_, source) in &scripts {
        for text in vocab_strings(source) {
            texts.add(text, TextKind::Menu);
        }
    }

    let tables: [(&str, &[&str]); 8] = [
        ("Actors", &["@nickname", "@description"]),
        ("Classes", &[]),
        ("Skills", &["@description", "@message1", "@message2"]),
        ("Items", &["@description"]),
        ("Weapons", &["@description"]),
        ("Armors", &["@description"]),
        ("Enemies", &[]),
        (
            "States",
            &["@message1", "@message2", "@message3", "@message4"],
        ),
    ];
    for (table, fields) in tables {
        for row in data.load(&format!("Data/{table}.rvdata2"))?.items() {
            // Actors' names join the name list, so dialogue spells them alike.
            if table == "Actors" {
                texts.name(row.field("@name"));
            } else {
                texts.database_name(row.field("@name"));
            }
            for field in fields {
                texts.text(row.field(field), TextKind::Database);
            }
        }
    }
    let mut drawn = texts
        .kept
        .iter()
        .filter(|text| drawn_whole(text))
        .cloned()
        .collect::<Vec<_>>();
    drawn.sort();
    for text in drawn {
        texts.add(text, TextKind::Addon);
    }

    let common_events = data.load("Data/CommonEvents.rvdata2")?;
    let mut calls = HashMap::<u32, BTreeSet<u32>>::new();
    for event in common_events.items() {
        let Some(id) = event
            .field("@id")
            .as_integer()
            .and_then(|id| u32::try_from(id).ok())
        else {
            continue;
        };
        texts.place = Some(format!("common:{id}"));
        calls.insert(id, texts.commands(event.field("@list")));
    }
    for troop in data.load("Data/Troops.rvdata2")?.items() {
        texts.place = None;
        for page in troop.field("@pages").items() {
            texts.commands(page.field("@list"));
        }
    }
    for (id, name) in data.maps()? {
        let map = data.load(&name)?;
        let place = format!("map:{id}");
        texts.place = Some(place.clone());
        texts.text(map.field("@display_name"), TextKind::Menu);
        let mut called = BTreeSet::new();
        for event in map.field("@events").values() {
            for page in event.field("@pages").items() {
                called.extend(texts.commands(page.field("@list")));
            }
        }
        // The shared events this map calls, and the ones those call.
        let mut seen = HashSet::new();
        let mut next = called.into_iter().collect::<Vec<_>>();
        while let Some(common) = next.pop() {
            if seen.insert(common) {
                texts.add_place(&format!("common:{common}"), &place);
                next.extend(calls.get(&common).into_iter().flatten().copied());
            }
        }
    }
    Ok(texts.into())
}

#[derive(Default)]
struct Texts {
    kept: HashSet<String>,
    seen: HashSet<String>,
    texts: Vec<SourceText>,
    seen_names: HashSet<String>,
    names: Vec<String>,
    place: Option<String>,
    places: BTreeMap<String, BTreeSet<String>>,
    // Texts by the shared event they belong to, to list them under maps.
    by_place: HashMap<String, BTreeSet<String>>,
}

impl Texts {
    fn text(&mut self, value: &Value, kind: TextKind) {
        if let Some(text) = value.as_str() {
            self.add(text.to_owned(), kind);
        }
    }

    fn add(&mut self, text: String, kind: TextKind) {
        if !has_japanese(&text) {
            return;
        }
        if let Some(place) = &self.place {
            self.places
                .entry(text.clone())
                .or_default()
                .insert(place.clone());
            self.by_place
                .entry(place.clone())
                .or_default()
                .insert(text.clone());
        }
        if self.seen.insert(text.clone()) {
            self.texts.push(SourceText { text, kind });
        }
    }

    fn each(&mut self, values: &Value, kind: TextKind) {
        for value in values.items() {
            self.text(value, kind);
        }
    }

    fn name(&mut self, value: &Value) {
        if let Some(name) = value.as_str()
            && has_japanese(name)
            && !self.kept.contains(name)
            && self.seen_names.insert(name.to_owned())
        {
            self.names.push(name.to_owned());
        }
    }

    // A name the scripts compare is translated too: the launcher's script
    // shows it in English where it is drawn and leaves the data Japanese.
    fn database_name(&mut self, value: &Value) {
        self.text(value, TextKind::Database);
    }

    /// An event's commands: messages, scrolling text and choices. Returns
    /// the shared events it calls.
    fn commands(&mut self, list: &Value) -> BTreeSet<u32> {
        let commands = list.items();
        let mut called = BTreeSet::new();
        // A script call `Pictures[n].permitw` (CACAO's picture extension)
        // writes the next scrolling text into a picture, a line an item: an
        // option menu's entries, read one by one.
        let mut into_picture = false;
        for (at, command) in commands.iter().enumerate() {
            let parameters = command.field("@parameters").items();
            match command.field("@code").as_integer() {
                Some(code @ (SCRIPT | SCRIPT_LINE)) => {
                    if parameters
                        .first()
                        .and_then(Value::as_str)
                        .is_some_and(|code| code.contains(".permitw"))
                    {
                        into_picture = true;
                    }
                    // A call's strings the game's scripts draw, such as a
                    // popup's heading (`Window_Getinfo.new(id, type, text)`):
                    // the launcher's script draws them in English, and the
                    // call stays as it is.
                    if code == SCRIPT {
                        let source = std::iter::once(command)
                            .chain(commands[at + 1..].iter().take_while(|line| {
                                line.field("@code").as_integer() == Some(SCRIPT_LINE)
                            }))
                            .filter_map(|line| line.field("@parameters").items().first()?.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        for text in script_strings(&source) {
                            if drawn_whole(&text) {
                                self.add(text, TextKind::Addon);
                            }
                        }
                    }
                }
                Some(SCROLL_TEXT) if into_picture => {
                    into_picture = false;
                    for line in commands[at + 1..]
                        .iter()
                        .take_while(|line| line.field("@code").as_integer() == Some(SCROLL_LINE))
                    {
                        if let Some(text) = line.field("@parameters").items().first() {
                            self.text(text, TextKind::Menu);
                        }
                    }
                }
                Some(code @ (SHOW_TEXT | SCROLL_TEXT)) => {
                    let line_code = if code == SHOW_TEXT {
                        TEXT_LINE
                    } else {
                        SCROLL_LINE
                    };
                    let lines = commands[at + 1..]
                        .iter()
                        .take_while(|line| line.field("@code").as_integer() == Some(line_code))
                        .map(|line| {
                            line.field("@parameters")
                                .items()
                                .first()
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        })
                        .collect::<Vec<_>>();
                    self.add(lines.join("\n"), TextKind::Dialogue);
                }
                Some(SHOW_CHOICES) => {
                    if let Some(choices) = parameters.first() {
                        self.each(choices, TextKind::Choice);
                    }
                }
                Some(CHANGE_NAME | CHANGE_NICKNAME) => {
                    if let Some(name) = parameters.get(1) {
                        self.name(name);
                    }
                }
                Some(CALL_COMMON_EVENT) => {
                    if let Some(id) = parameters
                        .first()
                        .and_then(Value::as_integer)
                        .and_then(|id| u32::try_from(id).ok())
                    {
                        called.insert(id);
                    }
                }
                _ => {}
            }
        }
        called
    }

    fn add_place(&mut self, owner: &str, place: &str) {
        for text in self.by_place.get(owner).cloned().unwrap_or_default() {
            self.places
                .entry(text)
                .or_default()
                .insert(place.to_owned());
        }
    }
}

impl From<Texts> for GameTexts {
    fn from(texts: Texts) -> Self {
        Self {
            texts: texts.texts,
            names: texts.names,
            places: texts
                .places
                .into_iter()
                .map(|(text, places)| (text, places.into_iter().collect()))
                .collect(),
        }
    }
}

/// The game's script sections as (title, source), without the launcher's.
pub fn script_sections(scripts: &Value) -> Vec<(String, String)> {
    scripts
        .items()
        .iter()
        .filter_map(|section| {
            let parts = section.items();
            if parts.first().and_then(Value::as_integer) == Some(OUR_SECTION) {
                return None;
            }
            let title = parts
                .get(1)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let source = inflate(parts.get(2)?.as_bytes()?)?;
            Some((title, source))
        })
        .collect()
}

pub fn inflate(bytes: &[u8]) -> Option<String> {
    let mut source = Vec::new();
    io::Read::read_to_end(&mut ZlibDecoder::new(bytes), &mut source).ok()?;
    Some(String::from_utf8_lossy(&source).into_owned())
}

/// The Japanese strings assigned to constants in `module Vocab` blocks (the
/// `Vocab` script, and games' own scripts adding to it): the battle messages
/// and menu words the game's code uses.
fn vocab_strings(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    // The indent of the `module Vocab` line the block is in, if any.
    let mut block: Option<usize> = None;
    for line in source.lines() {
        let indent = line.len() - line.trim_start().len();
        let code = line.trim();
        match block {
            None => {
                if code == "module Vocab" || code.starts_with("module Vocab ") {
                    block = Some(indent);
                }
            }
            Some(start) if indent == start && code == "end" => block = None,
            Some(_) => {
                let Some((name, value)) = code.split_once('=') else {
                    continue;
                };
                let name = name.trim();
                if name.starts_with(|first: char| first.is_ascii_uppercase())
                    && name
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_')
                    && let Some(text) = quoted(value.trim())
                    && has_japanese(&text)
                {
                    found.push(text);
                }
            }
        }
    }
    found
}

/// A double-quoted Ruby string as Ruby reads it: `\\` and `\"` stand for
/// themselves. Strings with other escapes or interpolation are left out.
fn quoted(value: &str) -> Option<String> {
    let mut characters = value.strip_prefix('"')?.chars();
    let mut text = String::new();
    loop {
        match characters.next()? {
            '"' => break,
            '\\' => match characters.next()? {
                escaped @ ('\\' | '"') => text.push(escaped),
                _ => return None,
            },
            '#' if characters.clone().next() == Some('{') => return None,
            character => text.push(character),
        }
    }
    Some(text)
}

/// The `"…"` and `'…'` strings of a Ruby script as Ruby reads them, without
/// its comments and `=begin` blocks; a `#{…}` splits a string into the
/// pieces around it. Ruby's rarer quotes (`%q()`, heredocs) are left out.
fn script_strings(source: &str) -> Vec<String> {
    let mut code = String::with_capacity(source.len());
    let mut in_block = false;
    for line in source.lines() {
        if in_block {
            in_block = !line.starts_with("=end");
        } else if line.starts_with("=begin") {
            in_block = true;
        } else {
            code.push_str(line);
        }
        code.push('\n');
    }
    let mut found = Vec::new();
    let mut characters = code.chars();
    while let Some(character) = characters.next() {
        match character {
            '#' => {
                characters.by_ref().find(|&next| next == '\n');
            }
            '"' => double_quoted(&mut characters, &mut found),
            '\'' => {
                let mut text = String::new();
                while let Some(next) = characters.next() {
                    match next {
                        '\'' => break,
                        '\\' => match characters.next() {
                            Some(escaped @ ('\\' | '\'')) => text.push(escaped),
                            Some(other) => {
                                text.push('\\');
                                text.push(other);
                            }
                            None => break,
                        },
                        other => text.push(other),
                    }
                }
                found.push(text);
            }
            _ => {}
        }
    }
    found.retain(|text| !text.is_empty());
    found
}

fn double_quoted(characters: &mut std::str::Chars<'_>, found: &mut Vec<String>) {
    let mut text = String::new();
    while let Some(next) = characters.next() {
        match next {
            '"' => break,
            '\\' => match characters.next() {
                Some('n') => text.push('\n'),
                Some('t') => text.push('\t'),
                Some('e') => text.push('\u{1b}'),
                Some('s') => text.push(' '),
                Some(other) => text.push(other),
                None => break,
            },
            '#' if characters.clone().next() == Some('{') => {
                found.push(std::mem::take(&mut text));
                let mut depth = 0;
                for inside in characters.by_ref() {
                    match inside {
                        '{' => depth += 1,
                        '}' if depth == 1 => break,
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
            }
            other => text.push(other),
        }
    }
    found.push(text);
}

fn find_case_insensitive(root: &Path, relative: &str) -> io::Result<Option<PathBuf>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rgss3a, ruby_marshal::Writer};
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn command(writer: &mut Writer, code: i64, parameters: &[&str]) {
        writer.object("RPG::EventCommand", 2);
        writer.symbol("@code");
        writer.integer(code);
        writer.symbol("@parameters");
        writer.array(parameters.len());
        for parameter in parameters {
            match parameter.parse::<i64>() {
                Ok(number) => writer.integer(number),
                Err(_) => writer.text(parameter),
            }
        }
    }

    fn file(build: impl FnOnce(&mut Writer)) -> Vec<u8> {
        let mut writer = Writer::new();
        build(&mut writer);
        writer.finish()
    }

    fn scripts() -> Vec<u8> {
        let sources = [
            (
                "Vocab",
                "module Vocab\n  Attack = \"%sの攻撃！\"\n  ObtainGold = \"お金を %s\\\\G 手に入れた！\"\nend\n",
            ),
            (
                "Shop add-on",
                "module Vocab\n  Sell = \"売却\"\nend\nclass Game_Party\n  def key?\n    has_item_named?(\"古い鍵\")\n  end\nend\n",
            ),
        ];
        file(|writer| {
            writer.array(sources.len());
            for (id, (title, source)) in sources.iter().enumerate() {
                let mut code = ZlibEncoder::new(Vec::new(), Compression::default());
                code.write_all(source.as_bytes()).unwrap();
                writer.array(3);
                writer.integer(id as i64 + 1);
                writer.text(title);
                writer.binary(&code.finish().unwrap());
            }
        })
    }

    fn game(directory: &Path) {
        let system = file(|writer| {
            writer.object("RPG::System", 3);
            writer.symbol("@game_title");
            writer.text("冒険の旅");
            writer.symbol("@currency_unit");
            writer.text("G");
            writer.symbol("@terms");
            writer.object("RPG::System::Terms", 1);
            writer.symbol("@commands");
            writer.array(2);
            writer.text("戦う");
            writer.text("逃げる");
        });
        let actors = file(|writer| {
            writer.array(2);
            writer.text("");
            writer.object("RPG::Actor", 2);
            writer.symbol("@name");
            writer.text("ルーク");
            writer.symbol("@nickname");
            writer.text("見習い騎士");
        });
        let items = file(|writer| {
            writer.array(3);
            writer.text("");
            for name in ["古い鍵", "薬草"] {
                writer.object("RPG::Item", 2);
                writer.symbol("@name");
                writer.text(name);
                writer.symbol("@description");
                writer.text("どこかの扉を開ける。");
            }
        });
        let common_events = file(|writer| {
            writer.array(2);
            writer.text("");
            writer.object("RPG::CommonEvent", 2);
            writer.symbol("@id");
            writer.integer(1);
            writer.symbol("@list");
            writer.array(11);
            command(writer, 101, &["", "0", "0", "2"]);
            command(writer, 401, &["宿屋へようこそ。"]);
            // A popup a script draws, its heading in the call; a sound's
            // file name and the comment are not drawn.
            command(writer, 355, &["type = 0 # 種類"]);
            command(writer, 655, &["text = \"アイテム獲得！\""]);
            command(writer, 655, &["Audio.se_play(\"Audio/SE/決定\")"]);
            command(writer, 655, &["e.push(Window_Getinfo.new(1, type, text))"]);
            // An option menu written into a picture, a line an entry.
            command(writer, 355, &["Pictures[12].permitw"]);
            command(writer, 105, &["2", "0"]);
            command(writer, 405, &["\\{断面図の表示"]);
            command(writer, 405, &[""]);
            command(writer, 405, &["ボイス"]);
        });
        let map = file(|writer| {
            writer.object("RPG::Map", 2);
            writer.symbol("@display_name");
            writer.text("始まりの村");
            writer.symbol("@events");
            writer.hash(1);
            writer.integer(1);
            writer.object("RPG::Event", 1);
            writer.symbol("@pages");
            writer.array(1);
            writer.object("RPG::Event::Page", 1);
            writer.symbol("@list");
            writer.array(7);
            command(writer, 101, &["Actor1", "0", "0", "2"]);
            command(writer, 401, &["\\N[1]、起きて！"]);
            command(writer, 401, &["朝だよ。"]);
            // Show Choices: the choices, then the cancel choice.
            writer.object("RPG::EventCommand", 2);
            writer.symbol("@code");
            writer.integer(102);
            writer.symbol("@parameters");
            writer.array(2);
            writer.array(2);
            writer.text("はい");
            writer.text("いいえ");
            writer.integer(2);
            command(writer, 117, &["1"]);
            command(writer, 105, &["2", "0"]);
            command(writer, 405, &["遠い昔のこと……"]);
        });
        let archive = rgss3a::build(&[
            ("Data\\System.rvdata2", &system),
            ("Data\\Scripts.rvdata2", &scripts()),
            ("Data\\Actors.rvdata2", &actors),
            ("Data\\Items.rvdata2", &items),
            ("Data\\CommonEvents.rvdata2", &common_events),
            ("Data\\Map001.rvdata2", &map),
        ]);
        fs::write(directory.join("Game.rgss3a"), archive).unwrap();
    }

    #[test]
    fn reads_what_the_script_looks_up_with_where_it_appears() {
        let directory = tempfile::tempdir().unwrap();
        game(directory.path());
        let texts = read(directory.path()).unwrap();
        let listed = texts
            .texts
            .iter()
            .map(|text| (text.text.as_str(), text.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            listed,
            [
                ("戦う", TextKind::Menu),
                ("逃げる", TextKind::Menu),
                ("冒険の旅", TextKind::Menu),
                ("%sの攻撃！", TextKind::Menu),
                ("お金を %s\\G 手に入れた！", TextKind::Menu),
                ("売却", TextKind::Menu),
                ("見習い騎士", TextKind::Database),
                // The add-on compares 古い鍵 by name: translated, it stays
                // Japanese in the data and English only where it is drawn.
                ("古い鍵", TextKind::Database),
                ("どこかの扉を開ける。", TextKind::Database),
                ("薬草", TextKind::Database),
                ("宿屋へようこそ。", TextKind::Dialogue),
                ("アイテム獲得！", TextKind::Addon),
                ("\\{断面図の表示", TextKind::Menu),
                ("ボイス", TextKind::Menu),
                ("始まりの村", TextKind::Menu),
                ("\\N[1]、起きて！\n朝だよ。", TextKind::Dialogue),
                ("はい", TextKind::Choice),
                ("いいえ", TextKind::Choice),
                ("遠い昔のこと……", TextKind::Dialogue),
            ]
        );
        assert_eq!(texts.names, ["ルーク"]);
        let places = |text: &str| texts.places.get(text).cloned().unwrap_or_default();
        assert_eq!(places("\\N[1]、起きて！\n朝だよ。"), ["map:1"]);
        // A shared event is listed under the map that calls it.
        assert_eq!(places("宿屋へようこそ。"), ["common:1", "map:1"]);
        assert_eq!(places("アイテム獲得！"), ["common:1", "map:1"]);
        assert!(places("薬草").is_empty());
        let mut in_scripts = strings_in_scripts(directory.path())
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>();
        in_scripts.sort();
        assert_eq!(in_scripts, ["古い鍵", "売却"]);
    }

    #[test]
    fn finds_the_strings_of_a_ruby_script() {
        let source = concat!(
            "=begin\n",
            "\"説明の中\"\n",
            "=end\n",
            "draw_text(rect, \"現在地：\") # \"注釈\"\n",
            "TEXT[6] = \"今までに戦った\\\"女の子\\\"の一覧です\"\n",
            "name = \"#{actor.name}の部屋\\\\C[1]\"\n",
            "note = 'it\\'s 単引用'\n",
        );
        assert_eq!(
            script_strings(source),
            [
                "現在地：",
                "今までに戦った\"女の子\"の一覧です",
                "の部屋\\C[1]",
                "it's 単引用",
            ]
        );
    }

    #[test]
    fn reads_ruby_strings_as_ruby_does() {
        assert_eq!(
            quoted(r#""お金を %s\\G 手に入れた！""#).as_deref(),
            Some(r"お金を %s\G 手に入れた！")
        );
        assert_eq!(
            quoted(r#""「\"はい\"」" # comment"#).as_deref(),
            Some("「\"はい\"」")
        );
        assert_eq!(quoted(r##""#{name}の攻撃""##), None);
        assert_eq!(quoted(r#""改行\nあり""#), None);
        assert_eq!(quoted("'単引用'"), None);
    }
}
