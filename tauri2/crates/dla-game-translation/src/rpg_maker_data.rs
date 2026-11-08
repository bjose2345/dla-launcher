//! The texts an RPG Maker MV or MZ game shows, read from its `data` folder
//! in the form the plugin looks them up while the game runs: a message is
//! its lines joined with `\n`, menu words and database entries as they are.

use std::{collections::HashSet, fs, io, path::Path};

use dla_application::{
    game_translation::GameTexts,
    translation::{SourceText, has_japanese},
};
use dla_domain::translation::TextKind;
use serde_json::Value;

const SHOW_TEXT: u64 = 101;
const TEXT_LINE: u64 = 401;
const SHOW_CHOICES: u64 = 102;
const DATABASE: [&str; 8] = [
    "Actors.json",
    "Classes.json",
    "Items.json",
    "Weapons.json",
    "Armors.json",
    "Skills.json",
    "States.json",
    "Enemies.json",
];
const DATABASE_FIELDS: [&str; 8] = [
    "name",
    "nickname",
    "profile",
    "description",
    "message1",
    "message2",
    "message3",
    "message4",
];
const SYSTEM_LISTS: [&str; 5] = [
    "equipTypes",
    "skillTypes",
    "weaponTypes",
    "armorTypes",
    "elements",
];

pub fn read(data: &Path) -> io::Result<GameTexts> {
    let system = read_json(&data.join("System.json"))?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "the game has no data/System.json")
    })?;
    let mut texts = Texts::default();
    let terms = &system["terms"];
    for group in ["basic", "commands", "params"] {
        texts.each(&terms[group], TextKind::Menu);
    }
    if let Some(messages) = terms["messages"].as_object() {
        messages
            .values()
            .for_each(|message| texts.text(message, TextKind::Menu));
    }
    for list in SYSTEM_LISTS {
        texts.each(&system[list], TextKind::Menu);
    }
    texts.text(&system["currencyUnit"], TextKind::Menu);
    texts.text(&system["gameTitle"], TextKind::Menu);

    for table in DATABASE {
        let actors = table == "Actors.json";
        for row in rows(&read_json(&data.join(table))?.unwrap_or_default()) {
            // Actors' names join the name list, so dialogue spells them alike.
            if actors {
                texts.name(&row["name"]);
            }
            for field in DATABASE_FIELDS {
                if !(actors && field == "name") {
                    texts.text(&row[field], TextKind::Database);
                }
            }
        }
    }
    for event in rows(&read_json(&data.join("CommonEvents.json"))?.unwrap_or_default()) {
        texts.commands(&event["list"]);
    }
    for troop in rows(&read_json(&data.join("Troops.json"))?.unwrap_or_default()) {
        for page in rows(&troop["pages"]) {
            texts.commands(&page["list"]);
        }
    }
    for map in maps(data)? {
        let map = read_json(&map)?.unwrap_or_default();
        for event in rows(&map["events"]) {
            for page in rows(&event["pages"]) {
                texts.commands(&page["list"]);
            }
        }
    }
    Ok(texts.into())
}

#[derive(Default)]
struct Texts {
    seen: HashSet<String>,
    texts: Vec<SourceText>,
    seen_names: HashSet<String>,
    names: Vec<String>,
}

impl Texts {
    fn text(&mut self, value: &Value, kind: TextKind) {
        if let Some(text) = value.as_str()
            && has_japanese(text)
            && self.seen.insert(text.to_owned())
        {
            self.texts.push(SourceText {
                text: text.to_owned(),
                kind,
            });
        }
    }

    fn each(&mut self, values: &Value, kind: TextKind) {
        for value in values.as_array().into_iter().flatten() {
            self.text(value, kind);
        }
    }

    fn name(&mut self, value: &Value) {
        if let Some(name) = value.as_str()
            && has_japanese(name)
            && self.seen_names.insert(name.to_owned())
        {
            self.names.push(name.to_owned());
        }
    }

    /// An event's commands: messages with their speakers, and choices.
    fn commands(&mut self, list: &Value) {
        let commands = list.as_array().map(Vec::as_slice).unwrap_or_default();
        for (at, command) in commands.iter().enumerate() {
            match command["code"].as_u64() {
                Some(SHOW_TEXT) => {
                    // MZ keeps the speaker's name with the message.
                    self.name(&command["parameters"][4]);
                    let lines = commands[at + 1..]
                        .iter()
                        .take_while(|line| line["code"].as_u64() == Some(TEXT_LINE))
                        .map(|line| line["parameters"][0].as_str().unwrap_or_default())
                        .collect::<Vec<_>>();
                    self.text(&Value::String(lines.join("\n")), TextKind::Dialogue);
                }
                Some(SHOW_CHOICES) => self.each(&command["parameters"][0], TextKind::Choice),
                _ => {}
            }
        }
    }
}

impl From<Texts> for GameTexts {
    fn from(texts: Texts) -> Self {
        Self {
            texts: texts.texts,
            names: texts.names,
            places: Default::default(),
        }
    }
}

/// Database tables start with `null`, and maps leave gaps for deleted events.
fn rows(value: &Value) -> impl Iterator<Item = &Value> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| row.is_object())
}

fn maps(data: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let mut maps = fs::read_dir(data)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("Map"))
                .and_then(|name| name.strip_suffix(".json"))
                .is_some_and(|number| {
                    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
                })
        })
        .collect::<Vec<_>>();
    maps.sort();
    Ok(maps)
}

/// `None` for a file the game does not have; a file that is not JSON (some
/// games encrypt theirs) counts as empty.
fn read_json(path: &Path) -> io::Result<Option<Value>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let json = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Ok(Some(serde_json::from_slice(json).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn write(data: &Path, name: &str, value: Value) {
        fs::write(data.join(name), serde_json::to_vec(&value).unwrap()).unwrap();
    }

    fn command(code: u64, parameters: Value) -> Value {
        json!({ "code": code, "indent": 0, "parameters": parameters })
    }

    #[test]
    fn finds_every_text_the_plugin_shows_once() {
        let folder = tempdir().unwrap();
        let data = folder.path();
        write(
            data,
            "System.json",
            json!({
                "gameTitle": "退魔士ユキナ",
                "currencyUnit": "G",
                "elements": ["", "炎"],
                "terms": {
                    "basic": ["レベル", "Lv"],
                    "commands": ["アイテム", null],
                    "params": ["最大HP"],
                    "messages": { "obtainItem": "%1を手に入れた！" }
                }
            }),
        );
        write(
            data,
            "Actors.json",
            json!([null, { "name": "ユキナ", "nickname": "退魔士", "profile": "" }]),
        );
        write(
            data,
            "Items.json",
            json!([null, { "name": "ポーション", "description": "HPを回復する。" }]),
        );
        let dialogue = [
            command(SHOW_TEXT, json!(["", 0, 0, 2, "ユキナ"])),
            command(TEXT_LINE, json!(["こんにちは"])),
            command(TEXT_LINE, json!(["\\C[2]元気？\\C[0]"])),
            command(SHOW_CHOICES, json!([["はい", "いいえ"], 1])),
            command(SHOW_TEXT, json!(["", 0, 0, 2])),
            command(TEXT_LINE, json!(["こんにちは"])),
            command(TEXT_LINE, json!(["\\C[2]元気？\\C[0]"])),
            command(0, json!([])),
        ];
        write(
            data,
            "Map001.json",
            json!({ "events": [null, { "pages": [{ "list": dialogue }] }] }),
        );
        write(
            data,
            "CommonEvents.json",
            json!([null, { "list": [command(SHOW_TEXT, json!(["", 0, 0, 2])), command(TEXT_LINE, json!(["共通"]))] }]),
        );
        write(
            data,
            "Troops.json",
            json!([null, { "pages": [{ "list": [command(SHOW_CHOICES, json!([["戦う"], 0]))] }] }]),
        );
        // Not a map, and not JSON.
        write(data, "MapInfos.json", json!([null, { "name": "地図" }]));
        fs::write(data.join("Map002.json"), b"encrypted").unwrap();

        let texts = read(data).unwrap();
        let found = texts
            .texts
            .iter()
            .map(|text| (text.text.as_str(), text.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                ("レベル", TextKind::Menu),
                ("アイテム", TextKind::Menu),
                ("最大HP", TextKind::Menu),
                ("%1を手に入れた！", TextKind::Menu),
                ("炎", TextKind::Menu),
                ("退魔士ユキナ", TextKind::Menu),
                ("退魔士", TextKind::Database),
                ("ポーション", TextKind::Database),
                ("HPを回復する。", TextKind::Database),
                ("共通", TextKind::Dialogue),
                ("戦う", TextKind::Choice),
                ("こんにちは\n\\C[2]元気？\\C[0]", TextKind::Dialogue),
                ("はい", TextKind::Choice),
                ("いいえ", TextKind::Choice),
            ]
        );
        assert_eq!(texts.names, ["ユキナ"]);
    }

    #[test]
    fn needs_the_system_file() {
        let folder = tempdir().unwrap();
        assert_eq!(
            read(folder.path()).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
