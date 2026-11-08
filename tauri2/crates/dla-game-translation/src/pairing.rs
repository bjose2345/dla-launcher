//! Pairs each text of a game with the same text in its official English
//! release, to teach the translator from real translations. Both are the same
//! RPG Maker project with the text replaced, so an event's commands line up
//! once their text is set aside, and the system and database line up by
//! table, row and field. A message the translator split, merged or moved has
//! no partner and is left out rather than guessed.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use dla_application::{game_translation::GameFolder, translation::has_japanese};
use dla_domain::{game_engine::GameEngine, translation::TextKind};
use serde_json::Value as Json;

use crate::{DesktopGameFolder, ruby_marshal::Value as Ruby, vx_ace_data::GameData};

const SHOW_TEXT: i64 = 101;
const TEXT_LINE: i64 = 401;
const SHOW_CHOICES: i64 = 102;
const SCROLL_TEXT: i64 = 105;
const SCROLL_LINE: i64 = 405;

/// A text and its official English.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pair {
    /// Where it is: `map:3/event:12/page:0`, `db:Items:5:description`.
    pub place: String,
    pub kind: TextKind,
    pub japanese: String,
    pub english: String,
    /// Both have the same control codes (`\C[2]`, `\N[1]`…).
    pub codes_match: bool,
    /// The face shown with a message (`Actor1`), which tells who speaks.
    pub face: Option<String>,
}

#[derive(Debug, Default)]
pub struct Pairing {
    pub pairs: Vec<Pair>,
    /// Texts on either side with no partner on the other.
    pub unpaired: usize,
    /// Texts left out, by why.
    pub dropped: BTreeMap<&'static str, usize>,
}

/// Pairs a game's Japanese copy with its official English copy.
pub fn pair_games(japanese: &Path, english: &Path) -> io::Result<Pairing> {
    let japanese = Side::open(japanese)?;
    let english = Side::open(english)?;
    Ok(pair(&japanese.texts()?, &english.texts()?))
}

/// A copy of the game, where its data is.
enum Side {
    MvMz(PathBuf),
    VxAce(GameData),
}

impl Side {
    fn open(game: &Path) -> io::Result<Self> {
        let folder = DesktopGameFolder::new();
        let detection = folder
            .detect_engine(&game.to_string_lossy())
            .map_err(|error| io::Error::other(error.to_string()))?
            .ok_or_else(|| io::Error::other(format!("no RPG Maker game in {}", game.display())))?;
        let root = detection
            .root
            .as_ref()
            .map_or_else(|| game.to_owned(), |root| game.join(root.as_str()));
        match detection.engine {
            GameEngine::RpgMakerMv | GameEngine::RpgMakerMz => Ok(Self::MvMz(root.join("data"))),
            GameEngine::RpgMakerVxAce => Ok(Self::VxAce(GameData::find(&root)?)),
            other => Err(io::Error::other(format!(
                "{other:?} games are not paired yet"
            ))),
        }
    }

    fn texts(&self) -> io::Result<Texts> {
        match self {
            Self::MvMz(data) => mv_mz_texts(data),
            Self::VxAce(data) => vx_ace_texts(data),
        }
    }
}

/// A game's texts: those found by their place, and the event lists, whose
/// commands are lined up between the two copies.
#[derive(Default)]
struct Texts {
    placed: BTreeMap<String, (TextKind, String)>,
    lists: BTreeMap<String, Vec<Unit>>,
}

impl Texts {
    fn place(&mut self, place: String, kind: TextKind, text: Option<&str>) {
        if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
            self.placed.insert(place, (kind, text.to_owned()));
        }
    }
}

/// An event command as both engines store it: its code and its parameters'
/// strings (a list of choices stays a list).
struct Command {
    code: i64,
    parameters: Vec<Parameter>,
}

enum Parameter {
    Text(String),
    Texts(Vec<String>),
    Other,
}

impl Command {
    fn text(&self, at: usize) -> Option<&str> {
        match self.parameters.get(at) {
            Some(Parameter::Text(text)) => Some(text),
            _ => None,
        }
    }
}

/// What an event shows, in order, and the other commands around it, which
/// line the two copies up.
#[derive(Clone, Debug)]
enum Unit {
    Message {
        face: String,
        speaker: Option<String>,
        text: String,
    },
    Choices(Vec<String>),
    Scroll(String),
    Command(i64),
}

impl Unit {
    /// What must be the same in both copies for two units to pair.
    fn token(&self) -> String {
        match self {
            Unit::Message { face, .. } => format!("{SHOW_TEXT}:{face}"),
            Unit::Choices(choices) => format!("{SHOW_CHOICES}:{}", choices.len()),
            Unit::Scroll(_) => SCROLL_TEXT.to_string(),
            Unit::Command(code) => code.to_string(),
        }
    }
}

fn units(commands: &[Command]) -> Vec<Unit> {
    let lines = |from: usize, code: i64| {
        commands[from..]
            .iter()
            .take_while(|command| command.code == code)
            .map(|command| command.text(0).unwrap_or_default().to_owned())
            .collect::<Vec<_>>()
    };
    let mut units = Vec::new();
    let mut at = 0;
    while at < commands.len() {
        let command = &commands[at];
        at += 1;
        match command.code {
            SHOW_TEXT => {
                let text = lines(at, TEXT_LINE);
                at += text.len();
                units.push(Unit::Message {
                    face: command.text(0).unwrap_or_default().to_owned(),
                    // MZ names the speaker in the message itself.
                    speaker: command.text(4).map(str::to_owned),
                    text: text.join("\n"),
                });
            }
            SCROLL_TEXT => {
                let text = lines(at, SCROLL_LINE);
                at += text.len();
                units.push(Unit::Scroll(text.join("\n")));
            }
            SHOW_CHOICES => units.push(Unit::Choices(match command.parameters.first() {
                Some(Parameter::Texts(choices)) => choices.clone(),
                _ => Vec::new(),
            })),
            TEXT_LINE | SCROLL_LINE => {}
            code => units.push(Unit::Command(code)),
        }
    }
    units
}

/// The pairs of positions the two sequences share in order (the longest
/// common sequence).
fn line_up(japanese: &[String], english: &[String]) -> Vec<(usize, usize)> {
    let (rows, columns) = (japanese.len(), english.len());
    let mut longest = vec![0_u32; (rows + 1) * (columns + 1)];
    let at = |row: usize, column: usize| row * (columns + 1) + column;
    for row in (0..rows).rev() {
        for column in (0..columns).rev() {
            longest[at(row, column)] = if japanese[row] == english[column] {
                longest[at(row + 1, column + 1)] + 1
            } else {
                longest[at(row + 1, column)].max(longest[at(row, column + 1)])
            };
        }
    }
    let (mut row, mut column, mut shared) = (0, 0, Vec::new());
    while row < rows && column < columns {
        if japanese[row] == english[column] {
            shared.push((row, column));
            row += 1;
            column += 1;
        } else if longest[at(row + 1, column)] >= longest[at(row, column + 1)] {
            row += 1;
        } else {
            column += 1;
        }
    }
    shared
}

fn pair(japanese: &Texts, english: &Texts) -> Pairing {
    let mut pairing = Pairing::default();
    for (place, (kind, text)) in &japanese.placed {
        match english.placed.get(place) {
            Some((_, translated)) => pairing.add(place.clone(), *kind, text, translated),
            None => pairing.unpaired += 1,
        }
    }
    pairing.unpaired += english
        .placed
        .keys()
        .filter(|place| !japanese.placed.contains_key(*place))
        .count();
    for (place, japanese_units) in &japanese.lists {
        let english_units = english.lists.get(place).map_or(&[][..], Vec::as_slice);
        let tokens = |units: &[Unit]| units.iter().map(Unit::token).collect::<Vec<_>>();
        let shared = line_up(&tokens(japanese_units), &tokens(english_units));
        let texts = |units: &[Unit]| {
            units
                .iter()
                .filter(|unit| !matches!(unit, Unit::Command(_)))
                .count()
        };
        // A message beside one with no partner was split or merged: its
        // partner holds only part of the text, so neither is used.
        let lone = |units: &[Unit], matched: &[usize], at: usize| {
            [at.checked_sub(1), Some(at + 1)]
                .into_iter()
                .flatten()
                .any(|near| {
                    matches!(units.get(near), Some(Unit::Message { .. }))
                        && !matched.contains(&near)
                })
        };
        let rows = shared.iter().map(|(row, _)| *row).collect::<Vec<_>>();
        let columns = shared.iter().map(|(_, column)| *column).collect::<Vec<_>>();
        let mut paired = 0;
        for (row, column) in shared {
            if matches!(japanese_units[row], Unit::Message { .. })
                && (lone(japanese_units, &rows, row) || lone(english_units, &columns, column))
            {
                continue;
            }
            let at = format!("{place}/{row}");
            match (&japanese_units[row], &english_units[column]) {
                (
                    Unit::Message {
                        face,
                        speaker,
                        text,
                    },
                    Unit::Message {
                        speaker: english_speaker,
                        text: translated,
                        ..
                    },
                ) => {
                    paired += 1;
                    let added = pairing.pairs.len();
                    pairing.add(at.clone(), TextKind::Dialogue, text, translated);
                    if let (Some(speaker), Some(english_speaker)) = (speaker, english_speaker) {
                        pairing.add(
                            format!("{at}/speaker"),
                            TextKind::Menu,
                            speaker,
                            english_speaker,
                        );
                    }
                    for pair in &mut pairing.pairs[added..] {
                        pair.face = Some(face.clone()).filter(|face| !face.is_empty());
                    }
                }
                (Unit::Scroll(text), Unit::Scroll(translated)) => {
                    paired += 1;
                    pairing.add(at, TextKind::Dialogue, text, translated);
                }
                (Unit::Choices(choices), Unit::Choices(translated)) => {
                    paired += 1;
                    for (number, (choice, english_choice)) in
                        choices.iter().zip(translated).enumerate()
                    {
                        pairing.add(
                            format!("{at}/{number}"),
                            TextKind::Choice,
                            choice,
                            english_choice,
                        );
                    }
                }
                _ => {}
            }
        }
        pairing.unpaired += texts(japanese_units) + texts(english_units) - 2 * paired;
    }
    pairing
}

impl Pairing {
    fn add(&mut self, place: String, kind: TextKind, japanese: &str, english: &str) {
        let why = if !has_japanese(japanese) {
            Some("no Japanese")
        } else if english.trim().is_empty() {
            Some("no English")
        } else if english == japanese {
            Some("left as it was")
        } else if has_japanese(english) {
            Some("English still has Japanese")
        } else if names(japanese) != names(english) {
            // A name or a number shown in the line must stay.
            Some("names or numbers differ")
        } else {
            None
        };
        if let Some(why) = why {
            *self.dropped.entry(why).or_default() += 1;
            return;
        }
        self.pairs.push(Pair {
            place,
            kind,
            japanese: japanese.to_owned(),
            english: english.to_owned(),
            codes_match: codes(japanese) == codes(english),
            face: None,
        });
    }
}

/// A text's control codes with a value (`\C[2]`, `\N[1]`), in order.
fn codes(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('\\') {
        rest = &rest[start + 1..];
        let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        if letters > 0
            && rest[letters..].starts_with('[')
            && let Some(end) = rest[letters..].find(']')
        {
            found.push(format!(
                "\\{}",
                rest[..letters + end + 1].to_ascii_uppercase()
            ));
        }
    }
    found
}

/// The codes that put a name, a number or an icon in a line, sorted: a
/// translation may move them but never drop them.
fn names(text: &str) -> Vec<String> {
    let mut found = codes(text)
        .into_iter()
        .filter(|code| {
            ["\\N[", "\\V[", "\\P[", "\\I[", "\\G"]
                .iter()
                .any(|kind| code.starts_with(kind))
        })
        .collect::<Vec<_>>();
    found.sort();
    found
}

// ── RPG Maker MV and MZ: JSON files in `data` ─────────────────────────

const MV_TABLES: [(&str, &[&str]); 8] = [
    ("Actors", &["name", "nickname", "profile"]),
    ("Classes", &["name"]),
    ("Skills", &["name", "description", "message1", "message2"]),
    ("Items", &["name", "description"]),
    ("Weapons", &["name", "description"]),
    ("Armors", &["name", "description"]),
    ("Enemies", &["name"]),
    (
        "States",
        &["name", "message1", "message2", "message3", "message4"],
    ),
];

fn mv_mz_texts(data: &Path) -> io::Result<Texts> {
    let read = |name: &str| -> io::Result<Json> {
        match fs::read(data.join(name)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| io::Error::other(format!("{name}: {error}"))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Json::Null),
            Err(error) => Err(error),
        }
    };
    let mut texts = Texts::default();
    let system = read("System.json")?;
    for group in ["basic", "commands", "params"] {
        for (at, text) in json_items(&system["terms"][group]).enumerate() {
            texts.place(
                format!("system:{group}:{at}"),
                TextKind::Menu,
                text.as_str(),
            );
        }
    }
    if let Some(messages) = system["terms"]["messages"].as_object() {
        for (key, text) in messages {
            texts.place(
                format!("system:messages:{key}"),
                TextKind::Menu,
                text.as_str(),
            );
        }
    }
    for list in [
        "equipTypes",
        "skillTypes",
        "weaponTypes",
        "armorTypes",
        "elements",
    ] {
        for (at, text) in json_items(&system[list]).enumerate() {
            texts.place(format!("system:{list}:{at}"), TextKind::Menu, text.as_str());
        }
    }
    texts.place(
        "system:gameTitle".to_owned(),
        TextKind::Menu,
        system["gameTitle"].as_str(),
    );
    texts.place(
        "system:currencyUnit".to_owned(),
        TextKind::Menu,
        system["currencyUnit"].as_str(),
    );
    for (table, fields) in MV_TABLES {
        for row in json_items(&read(&format!("{table}.json"))?) {
            let Some(id) = row["id"].as_i64() else {
                continue;
            };
            for field in fields {
                texts.place(
                    format!("db:{table}:{id}:{field}"),
                    TextKind::Database,
                    row[*field].as_str(),
                );
            }
        }
    }
    for event in json_items(&read("CommonEvents.json")?) {
        if let Some(id) = event["id"].as_i64() {
            texts.lists.insert(
                format!("common:{id}"),
                units(&json_commands(&event["list"])),
            );
        }
    }
    for troop in json_items(&read("Troops.json")?) {
        let Some(id) = troop["id"].as_i64() else {
            continue;
        };
        for (page, content) in json_items(&troop["pages"]).enumerate() {
            texts.lists.insert(
                format!("troop:{id}/page:{page}"),
                units(&json_commands(&content["list"])),
            );
        }
    }
    let mut maps = fs::read_dir(data)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            let number = name.strip_prefix("Map")?.strip_suffix(".json")?;
            number.parse::<u32>().ok().map(|id| (id, name))
        })
        .collect::<Vec<_>>();
    maps.sort();
    for (id, name) in maps {
        let map = read(&name)?;
        for event in json_items(&map["events"]) {
            let Some(event_id) = event["id"].as_i64() else {
                continue;
            };
            for (page, content) in json_items(&event["pages"]).enumerate() {
                texts.lists.insert(
                    format!("map:{id}/event:{event_id}/page:{page}"),
                    units(&json_commands(&content["list"])),
                );
            }
        }
    }
    Ok(texts)
}

fn json_items(value: &Json) -> impl Iterator<Item = &Json> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| !item.is_null())
}

fn json_commands(list: &Json) -> Vec<Command> {
    json_items(list)
        .filter_map(|command| {
            Some(Command {
                code: command["code"].as_i64()?,
                parameters: json_items(&command["parameters"])
                    .map(|parameter| match parameter {
                        Json::String(text) => Parameter::Text(text.clone()),
                        Json::Array(items) => Parameter::Texts(
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_owned))
                                .collect(),
                        ),
                        _ => Parameter::Other,
                    })
                    .collect(),
            })
        })
        .collect()
}

// ── RPG Maker VX Ace: Ruby data in `Data`, often inside the archive ───

const VX_ACE_TABLES: [(&str, &[&str]); 8] = [
    ("Actors", &["@name", "@nickname", "@description"]),
    ("Classes", &["@name"]),
    (
        "Skills",
        &["@name", "@description", "@message1", "@message2"],
    ),
    ("Items", &["@name", "@description"]),
    ("Weapons", &["@name", "@description"]),
    ("Armors", &["@name", "@description"]),
    ("Enemies", &["@name"]),
    (
        "States",
        &["@name", "@message1", "@message2", "@message3", "@message4"],
    ),
];

fn vx_ace_texts(data: &GameData) -> io::Result<Texts> {
    let mut texts = Texts::default();
    let system = data.load("Data/System.rvdata2")?;
    for group in ["@basic", "@params", "@etypes", "@commands"] {
        for (at, text) in system
            .field("@terms")
            .field(group)
            .items()
            .iter()
            .enumerate()
        {
            texts.place(
                format!("system:{group}:{at}"),
                TextKind::Menu,
                text.as_str(),
            );
        }
    }
    for list in ["@elements", "@skill_types", "@weapon_types", "@armor_types"] {
        for (at, text) in system.field(list).items().iter().enumerate() {
            texts.place(format!("system:{list}:{at}"), TextKind::Menu, text.as_str());
        }
    }
    for field in ["@game_title", "@currency_unit"] {
        texts.place(
            format!("system:{field}"),
            TextKind::Menu,
            system.field(field).as_str(),
        );
    }
    for (table, fields) in VX_ACE_TABLES {
        for row in data.load(&format!("Data/{table}.rvdata2"))?.items() {
            let Some(id) = row.field("@id").as_integer() else {
                continue;
            };
            for field in fields {
                texts.place(
                    format!("db:{table}:{id}:{field}"),
                    TextKind::Database,
                    row.field(field).as_str(),
                );
            }
        }
    }
    for event in data.load("Data/CommonEvents.rvdata2")?.items() {
        if let Some(id) = event.field("@id").as_integer() {
            texts.lists.insert(
                format!("common:{id}"),
                units(&ruby_commands(event.field("@list"))),
            );
        }
    }
    for troop in data.load("Data/Troops.rvdata2")?.items() {
        let Some(id) = troop.field("@id").as_integer() else {
            continue;
        };
        for (page, content) in troop.field("@pages").items().iter().enumerate() {
            texts.lists.insert(
                format!("troop:{id}/page:{page}"),
                units(&ruby_commands(content.field("@list"))),
            );
        }
    }
    for (id, name) in data.maps()? {
        let map = data.load(&name)?;
        for event in map.field("@events").values() {
            let Some(event_id) = event.field("@id").as_integer() else {
                continue;
            };
            for (page, content) in event.field("@pages").items().iter().enumerate() {
                texts.lists.insert(
                    format!("map:{id}/event:{event_id}/page:{page}"),
                    units(&ruby_commands(content.field("@list"))),
                );
            }
        }
    }
    Ok(texts)
}

fn ruby_commands(list: &Ruby) -> Vec<Command> {
    list.items()
        .iter()
        .filter_map(|command| {
            Some(Command {
                code: command.field("@code").as_integer()?,
                parameters: command
                    .field("@parameters")
                    .items()
                    .iter()
                    .map(|parameter| match parameter {
                        Ruby::String(_) => {
                            Parameter::Text(parameter.as_str().unwrap_or_default().to_owned())
                        }
                        Ruby::Array(items) => Parameter::Texts(
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(str::to_owned))
                                .collect(),
                        ),
                        _ => Parameter::Other,
                    })
                    .collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn command(code: i64, parameters: Json) -> Json {
        json!({"code": code, "indent": 0, "parameters": parameters})
    }

    /// A tiny MV game's data: a common event with two messages and a
    /// choice, and an item.
    fn write(
        data: &Path,
        messages: &[(&str, &[&str])],
        choices: [&str; 2],
        item: &str,
        extra: bool,
    ) {
        fs::create_dir_all(data).unwrap();
        let mut list = vec![command(230, json!([30]))];
        if extra {
            // The English release added a sound before the first message.
            list.push(command(250, json!([{"name": "Bell"}])));
        }
        for (face, lines) in messages {
            list.push(command(101, json!([face, 0, 0, 2])));
            list.extend(lines.iter().map(|line| command(401, json!([line]))));
        }
        list.push(command(102, json!([choices, 1, 0, 2, 0])));
        list.push(command(0, json!([])));
        let events = json!([null, {"id": 1, "name": "Inn", "list": list}]);
        fs::write(data.join("CommonEvents.json"), events.to_string()).unwrap();
        let items = json!([null, {"id": 1, "name": item, "description": ""}]);
        fs::write(data.join("Items.json"), items.to_string()).unwrap();
        fs::write(
            data.join("System.json"),
            json!({"terms": {"basic": ["レベル"]}}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn pairs_the_same_messages_and_leaves_out_one_the_translator_split() {
        let root = tempfile::tempdir().unwrap();
        let (japanese, english) = (root.path().join("ja"), root.path().join("en"));
        write(
            &japanese,
            &[
                ("Actor1", &["宿屋へようこそ。", "\\N[1]さん、一泊どう？"]),
                ("Actor2", &["長い話なんだけど…聞いてくれる？"]),
            ],
            ["はい", "いいえ"],
            "薬草",
            false,
        );
        write(
            &english,
            &[
                (
                    "Actor1",
                    &["Welcome to the inn.", "Staying the night, \\N[1]?"],
                ),
                // Split into two windows: no partner, left out.
                ("Actor2", &["It's a long story..."]),
                ("Actor2", &["Will you hear me out?"]),
            ],
            ["Yes", "No"],
            "Herb",
            true,
        );
        let pairing = pair(
            &mv_mz_texts(&japanese).unwrap(),
            &mv_mz_texts(&english).unwrap(),
        );
        let found = pairing
            .pairs
            .iter()
            .map(|pair| (pair.kind, pair.japanese.as_str(), pair.english.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                (TextKind::Database, "薬草", "Herb"),
                (
                    TextKind::Dialogue,
                    "宿屋へようこそ。\n\\N[1]さん、一泊どう？",
                    "Welcome to the inn.\nStaying the night, \\N[1]?"
                ),
                (TextKind::Choice, "はい", "Yes"),
                (TextKind::Choice, "いいえ", "No"),
            ]
        );
        // The split message: one Japanese and two English units, unpaired;
        // the system word was left as it was.
        assert_eq!(pairing.unpaired, 3);
        assert_eq!(pairing.dropped, BTreeMap::from([("left as it was", 1)]));
        assert!(pairing.pairs.iter().all(|pair| pair.codes_match));
    }

    #[test]
    fn leaves_out_lines_still_in_japanese_or_missing_a_name() {
        let mut pairing = Pairing::default();
        pairing.add("a".into(), TextKind::Dialogue, "ありがとう", "ありがとう");
        pairing.add(
            "b".into(),
            TextKind::Dialogue,
            "\\N[1]、ありがとう",
            "Thank you!",
        );
        pairing.add(
            "c".into(),
            TextKind::Dialogue,
            "\\C[2]剣\\C[0]をくれ",
            "Give me the sword",
        );
        pairing.add(
            "d".into(),
            TextKind::Dialogue,
            "あの城へ",
            "To that castle 城",
        );
        assert_eq!(pairing.pairs.len(), 1);
        assert!(!pairing.pairs[0].codes_match);
        assert_eq!(
            pairing.dropped,
            BTreeMap::from([
                ("English still has Japanese", 1),
                ("left as it was", 1),
                ("names or numbers differ", 1),
            ])
        );
    }

    #[test]
    fn lines_up_two_sequences_by_what_they_share() {
        let tokens = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
        assert_eq!(
            line_up(&tokens("a b c d"), &tokens("a x b d")),
            [(0, 0), (1, 2), (3, 3)]
        );
    }
}
