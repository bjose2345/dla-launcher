//! The texts an RPG Maker MV or MZ game shows, read from its `data` folder
//! and its plugins' settings in the form the plugin looks them up while the
//! game runs: a message is its lines joined with `\n`, menu words and
//! database entries as they are.

use std::{collections::HashSet, fs, io, path::Path};

use dla_application::{
    game_translation::GameTexts,
    translation::{SourceText, has_japanese},
};
use dla_domain::translation::TextKind;
use serde_json::Value;

use crate::rpg_maker_settings::{self, SKILL_TREE_SCRIPT};

const SHOW_TEXT: u64 = 101;
const TEXT_LINE: u64 = 401;
const SHOW_CHOICES: u64 = 102;
const PLUGIN_COMMAND: u64 = 357;
const CONTROL_VARIABLES: u64 = 122;
const SCRIPT: u64 = 355;
const SCRIPT_LINE: u64 = 655;
/// The script an event starts a status page of the CBR_EroStatus plugin
/// with, and the line of it that holds a text of the page.
const STATUS_SCRIPT: &str = "CBR-エロステータス";
const STATUS_TEXT: &str = "テキスト-";
/// The plugin commands (MZ) known to show an argument as text: the plugin,
/// its command, the argument, what the text is translated as, and its shape.
const COMMAND_TEXTS: [(&str, &str, &str, TextKind, CommandShape); 5] = [
    (
        "TextPicture",
        "set",
        "text",
        TextKind::Dialogue,
        CommandShape::Page,
    ),
    (
        "TorigoyaMZ_NotifyMessage",
        "notify",
        "message",
        TextKind::Dialogue,
        CommandShape::Line,
    ),
    (
        "Mano_CurrencyUnit",
        "setWalletItem",
        "unit",
        TextKind::Menu,
        CommandShape::Line,
    ),
    (
        "Mano_CurrencyUnit",
        "setWalletVariable",
        "unit",
        TextKind::Menu,
        CommandShape::Line,
    ),
    (
        "QuestSystem",
        "ChangeDetail",
        "DetailNote",
        TextKind::Database,
        CommandShape::Note,
    ),
];

#[derive(Clone, Copy)]
enum CommandShape {
    Line,
    /// Translated by its paragraphs: TextPicture draws its text as a picture.
    Page,
    /// A page the argument holds as a JSON string (a quest's notes).
    Note,
}
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
/// The tables whose rows a glossary lists (the SceneGlossary plugin), and
/// the tags of a row's note that hold what its pages say: `SG説明`,
/// `SGDescription2`, and so on.
const GLOSSARY_TABLES: [&str; 3] = ["Items.json", "Weapons.json", "Armors.json"];
const GLOSSARY_PREFIX: &str = "SG";
const GLOSSARY_TEXTS: [&str; 4] = ["説明", "Description", "共通説明", "CommonDescription"];
/// The tags that name the categories a row is listed under, apart by
/// commas. The glossary compares them, so they stay in the data: the plugin
/// draws their English.
const GLOSSARY_CATEGORIES: [&str; 2] = ["SGカテゴリ", "SGCategory"];
/// The tables whose rows TMDescriptionEx shows a page of, and the tags of a
/// row's note that hold what the page says beyond the row's parameters.
const DESCRIPTION_TABLES: [&str; 4] = ["Items.json", "Weapons.json", "Armors.json", "Skills.json"];
const DESCRIPTION_TAGS: [&str; 2] = ["dPlnText", "dText"];
const ENEMIES: &str = "Enemies.json";
const SYSTEM_LISTS: [&str; 5] = [
    "equipTypes",
    "skillTypes",
    "weaponTypes",
    "armorTypes",
    "elements",
];

pub fn read(data: &Path, plugin_list: &Path, plugins: &Path) -> io::Result<GameTexts> {
    let settings = read_script(plugin_list)?
        .map(|list| rpg_maker_settings::read(&list))
        .unwrap_or_default();
    let skill_tree = match settings.skill_tree {
        true => read_script(&plugins.join(SKILL_TREE_SCRIPT))?,
        false => None,
    }
    .map(|script| rpg_maker_settings::skill_tree_texts(&script))
    .unwrap_or_default();
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
    for text in &settings.texts {
        texts.add(text, TextKind::Addon);
    }
    for page in &settings.pages {
        texts.page(page, TextKind::Database);
    }
    for name in &skill_tree.names {
        texts.add(name, TextKind::Menu);
    }
    for note in &skill_tree.notes {
        texts.add(note, TextKind::Database);
    }

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
            let glossary = GLOSSARY_TABLES.contains(&table);
            let descriptions = DESCRIPTION_TABLES.contains(&table);
            let book = table == ENEMIES && !settings.enemy_book_tags.is_empty();
            if !(glossary || descriptions || book) {
                continue;
            }
            for (tag, value) in note_tags(row["note"].as_str().unwrap_or_default()) {
                if (glossary && is_glossary_text(tag))
                    || (descriptions && DESCRIPTION_TAGS.contains(&tag))
                    || (book && settings.enemy_book_tags.iter().any(|known| known == tag))
                {
                    texts.page(value, TextKind::Database);
                } else if glossary && GLOSSARY_CATEGORIES.contains(&tag) {
                    for category in value.split(',') {
                        texts.add(category, TextKind::Menu);
                    }
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
        // The name a map shows on entering it, and a save made there in the
        // list of saves.
        texts.text(&map["displayName"], TextKind::Menu);
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
        if let Some(text) = value.as_str() {
            self.add(text, kind);
        }
    }

    fn add(&mut self, text: &str, kind: TextKind) {
        if has_japanese(text) && self.seen.insert(text.to_owned()) {
            self.texts.push(SourceText {
                text: text.to_owned(),
                kind,
            });
        }
    }

    /// A page is translated by its title and paragraphs, as the plugin puts
    /// it together again.
    fn page(&mut self, page: &str, kind: TextKind) {
        for block in page_blocks(page) {
            self.add(&block, kind);
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
                // A variable set to a string is a text to show (`\V[n]` in a
                // message or on a status page): its English is put in where
                // the code is turned into its value.
                Some(CONTROL_VARIABLES) => {
                    let parameters = &command["parameters"];
                    let set_by_script =
                        parameters[2].as_u64() == Some(0) && parameters[3].as_u64() == Some(4);
                    if let Some(text) = parameters[4]
                        .as_str()
                        .filter(|_| set_by_script)
                        .and_then(string_literal)
                    {
                        self.add(&text, TextKind::Database);
                    }
                }
                Some(SCRIPT) if command["parameters"][0].as_str() == Some(STATUS_SCRIPT) => {
                    commands[at + 1..]
                        .iter()
                        .take_while(|line| line["code"].as_u64() == Some(SCRIPT_LINE))
                        .filter_map(|line| {
                            line["parameters"][0].as_str()?.strip_prefix(STATUS_TEXT)
                        })
                        .for_each(|text| self.add(text, TextKind::Menu));
                }
                Some(PLUGIN_COMMAND) => {
                    let parameters = &command["parameters"];
                    let known = COMMAND_TEXTS.iter().find(|(plugin, name, ..)| {
                        parameters[0].as_str() == Some(plugin)
                            && parameters[1].as_str() == Some(name)
                    });
                    let Some(&(_, _, argument, kind, shape)) = known else {
                        continue;
                    };
                    let Some(text) = parameters[3][argument].as_str() else {
                        continue;
                    };
                    match shape {
                        CommandShape::Line => self.add(text, kind),
                        CommandShape::Page => self.page(text, kind),
                        CommandShape::Note => {
                            if let Ok(note) = serde_json::from_str::<String>(text) {
                                self.page(&note, kind);
                            }
                        }
                    }
                }
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
/// The tags of a note that have a value, as the engine reads them into a
/// row's `meta` (`/<([^<>:]+)(:?)([^>]*)>/g`): the value is what the plugin
/// that shows it gets, so it is the text to translate, byte for byte.
fn note_tags(note: &str) -> Vec<(&str, &str)> {
    let mut tags = Vec::new();
    let mut rest = note;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let name = after.find(['<', '>', ':']).unwrap_or(after.len());
        if name == 0 {
            rest = after;
            continue;
        }
        let Some(close) = after[name..].find('>') else {
            break;
        };
        if after[name..].starts_with(':') {
            tags.push((&after[..name], &after[name + 1..name + close]));
        }
        rest = &after[name + close + 1..];
    }
    tags
}

/// The string a script is when it is one string and nothing else
/// (`"text"`, `'text'`), as the plugin reads it.
fn string_literal(script: &str) -> Option<String> {
    let script = script.trim();
    let script = script.strip_suffix(';').unwrap_or(script).trim();
    if script.starts_with('"') {
        return serde_json::from_str(script).ok();
    }
    script
        .strip_prefix('\'')?
        .strip_suffix('\'')
        .filter(|text| !text.contains(['\'', '\\']))
        .map(str::to_owned)
}

/// What a glossary page is translated by, as the plugin puts the page
/// together again: its title and its paragraphs, each a run of lines under
/// one indent (spaces, also the wide ones) with no empty line among them,
/// without the indent. Translated whole, a page comes back as one paragraph.
fn page_blocks(page: &str) -> Vec<String> {
    let indent = |line: &str| line.len() - line.trim_start_matches([' ', '\u{3000}']).len();
    let lines = page.split('\n').collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let own = indent(lines[at]);
        let mut end = at + 1;
        if own < lines[at].len() {
            while end < lines.len()
                && indent(lines[end]) == own
                && lines[end][..own] == lines[at][..own]
                && own < lines[end].len()
            {
                end += 1;
            }
        }
        blocks.push(
            lines[at..end]
                .iter()
                .map(|line| &line[own..])
                .collect::<Vec<_>>()
                .join("\n"),
        );
        at = end;
    }
    blocks
}

fn is_glossary_text(tag: &str) -> bool {
    tag.strip_prefix(GLOSSARY_PREFIX).is_some_and(|name| {
        let name = name.trim_end_matches(|digit: char| digit.is_ascii_digit());
        GLOSSARY_TEXTS.contains(&name)
    })
}

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

/// `None` for a script the game does not have.
fn read_script(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(script) => Ok(Some(script)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
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
        let data = &folder.path().join("data");
        let plugin_folder = folder.path().join("js/plugins");
        let plugin_list = folder.path().join("js/plugins.js");
        fs::create_dir(data).unwrap();
        fs::create_dir_all(&plugin_folder).unwrap();
        fs::write(
            plugin_folder.join(SKILL_TREE_SCRIPT),
            "const loadSkillTreeConfig = () => ({\nskillTreeTypes: [{ actorId: 1, types: [[\"剣１\", \"剣の道\", \"剣を極める。\"]] }],\nskillTreeInfo: [[\"斬る\", 3, 1]],\n});\n",
        )
        .unwrap();
        let book_page = |entry: &str, tag: &str| {
            json!([json!({ "DateSelect": entry, "textMethod": tag }).to_string()]).to_string()
        };
        let quests = json!([json!({
            "Title": "生き抜く風",
            "DetailNote": "\"\\\\C[16]【場所】\\\\C[0]\\n日の出の丘\""
        })
        .to_string()]);
        let plugins = json!([
            { "name": "NUUN_EnemyBook", "status": true, "parameters": {
                "CommandName": "モンスター図鑑",
                "PageList1": book_page("251", "pictures"),
                "PageList3": book_page("70", "text")
            } },
            { "name": "QuestSystem", "status": true, "parameters": { "QuestDatas": quests.to_string() } },
            { "name": "SkillTreeConfig", "status": true, "parameters": {} }
        ]);
        fs::write(&plugin_list, format!("var $plugins =\n{plugins};\n")).unwrap();
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
            json!([null, {
                "name": "ポーション",
                "description": "HPを回復する。",
                "note": "<SG種別:1>\n<SG説明:\\C[16]【解説】\\C[0]\n　体力を戻す薬\n　苦い\n\n　飲みすぎない>\n<SG説明2:二枚目>\n<SGカテゴリ:薬,飲み物>\n<メモ:使わない>"
            }]),
        );
        write(
            data,
            "Skills.json",
            json!([null, {
                "name": "",
                "note": "<SG説明:技の表には無い>\n<dPlnText:技の解説\n二行目\n\n技の段>"
            }]),
        );
        write(
            data,
            "Enemies.json",
            json!([null, {
                "name": "スライム",
                "note": "<text:ぷるぷるした魔物>\n<desc:別の頁>\n<CategoryKey:草原>\n<pictures:絵一,絵二>"
            }]),
        );
        let dialogue = [
            command(SHOW_TEXT, json!(["", 0, 0, 2, "ユキナ"])),
            command(TEXT_LINE, json!(["こんにちは"])),
            command(TEXT_LINE, json!(["\\C[2]元気？\\C[0]"])),
            command(SHOW_CHOICES, json!([["はい", "いいえ"], 1])),
            command(
                PLUGIN_COMMAND,
                json!(["TextPicture", "set", "設定", { "text": "\n\n　　日記の一行目\n　　二行目\n\n　　次の段" }]),
            ),
            command(
                PLUGIN_COMMAND,
                json!(["TorigoyaMZ_NotifyMessage", "notify", "通知", { "message": "\\C[24]手記\\C[0] 更新" }]),
            ),
            command(
                PLUGIN_COMMAND,
                json!(["FilterControllerMZ", "setFilter", "設定", { "filterId": "ブラー" }]),
            ),
            command(
                PLUGIN_COMMAND,
                json!(["Mano_CurrencyUnit", "setWalletItem", "設定", { "itemId": "5", "unit": "個" }]),
            ),
            command(
                PLUGIN_COMMAND,
                json!(["QuestSystem", "ChangeDetail", "設定", {
                    "VariableId": "12",
                    "Detail": "",
                    "DetailNote": "\"新しい頁\\n\\n続き\""
                }]),
            ),
            command(SCRIPT, json!(["CBR-エロステータス"])),
            command(SCRIPT_LINE, json!(["テキスト-\\C[16]状態の欄"])),
            command(SCRIPT_LINE, json!(["x-30"])),
            command(SCRIPT, json!(["CBR-エロステータス"])),
            command(SCRIPT_LINE, json!(["テキスト-\\V[87]"])),
            command(SCRIPT, json!(["$gameMessage.add(\"台本\")"])),
            command(CONTROL_VARIABLES, json!([87, 87, 0, 4, " \"変数の文\"; "])),
            command(CONTROL_VARIABLES, json!([88, 88, 0, 4, "'引用符の文'"])),
            command(
                CONTROL_VARIABLES,
                json!([89, 89, 0, 4, "\"式\" + \"の文\""]),
            ),
            command(CONTROL_VARIABLES, json!([90, 90, 1, 4, "\"足す文\""])),
            command(SHOW_TEXT, json!(["", 0, 0, 2])),
            command(TEXT_LINE, json!(["こんにちは"])),
            command(TEXT_LINE, json!(["\\C[2]元気？\\C[0]"])),
            command(0, json!([])),
        ];
        write(
            data,
            "Map001.json",
            json!({ "displayName": "荒れた古戦場", "events": [null, { "pages": [{ "list": dialogue }] }] }),
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

        let texts = read(data, &plugin_list, &plugin_folder).unwrap();
        // The words the enemy book's code draws come with the plugin.
        let book = json!([{ "name": "NUUN_EnemyBook", "status": true, "parameters": {} }]);
        let drawn = rpg_maker_settings::read(&format!("var $plugins =\n{book};\n")).texts;
        assert!(drawn.contains(&"ドロップアイテム".to_owned()));
        assert!(drawn.iter().all(|word| {
            texts
                .texts
                .iter()
                .any(|text| &text.text == word && text.kind == TextKind::Addon)
        }));
        let found = texts
            .texts
            .iter()
            .filter(|text| !drawn.contains(&text.text))
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
                ("モンスター図鑑", TextKind::Addon),
                ("生き抜く風", TextKind::Addon),
                ("\\C[16]【場所】\\C[0]\n日の出の丘", TextKind::Database),
                ("剣の道", TextKind::Menu),
                ("剣を極める。", TextKind::Database),
                ("退魔士", TextKind::Database),
                ("ポーション", TextKind::Database),
                ("HPを回復する。", TextKind::Database),
                ("\\C[16]【解説】\\C[0]", TextKind::Database),
                ("体力を戻す薬\n苦い", TextKind::Database),
                ("飲みすぎない", TextKind::Database),
                ("二枚目", TextKind::Database),
                ("薬", TextKind::Menu),
                ("飲み物", TextKind::Menu),
                ("技の解説\n二行目", TextKind::Database),
                ("技の段", TextKind::Database),
                ("スライム", TextKind::Database),
                ("ぷるぷるした魔物", TextKind::Database),
                ("共通", TextKind::Dialogue),
                ("戦う", TextKind::Choice),
                ("荒れた古戦場", TextKind::Menu),
                ("こんにちは\n\\C[2]元気？\\C[0]", TextKind::Dialogue),
                ("はい", TextKind::Choice),
                ("いいえ", TextKind::Choice),
                ("日記の一行目\n二行目", TextKind::Dialogue),
                ("次の段", TextKind::Dialogue),
                ("\\C[24]手記\\C[0] 更新", TextKind::Dialogue),
                ("個", TextKind::Menu),
                ("新しい頁", TextKind::Database),
                ("続き", TextKind::Database),
                ("\\C[16]状態の欄", TextKind::Menu),
                ("変数の文", TextKind::Database),
                ("引用符の文", TextKind::Database),
            ]
        );
        assert_eq!(texts.names, ["ユキナ"]);
    }

    #[test]
    fn reads_the_tags_of_a_note_as_the_engine_does() {
        assert_eq!(
            note_tags("<a:1>text<b>\n<c:two\nlines><:x><d:<e:5>> <f:"),
            [("a", "1"), ("c", "two\nlines"), ("d", "<e:5")]
        );
        assert_eq!(note_tags("<a<b:1>><c:2>"), [("c", "2")]);
        assert!(note_tags("no tags > here <").is_empty());
        assert!(is_glossary_text("SG説明") && is_glossary_text("SGDescription12"));
        assert!(is_glossary_text("SG共通説明") && !is_glossary_text("SGカテゴリ"));
        assert!(!is_glossary_text("説明") && !is_glossary_text("SG説明文"));
    }

    #[test]
    fn takes_a_glossary_page_apart_at_its_indents_and_empty_lines() {
        assert_eq!(
            page_blocks("題\n　一\n　二\n\n　三\n  四\n\u{3000} 五\n題二\n　"),
            ["題", "一\n二", "", "三", "四", "五", "題二", ""]
        );
        assert_eq!(page_blocks("一行"), ["一行"]);
        assert_eq!(page_blocks(""), [""]);
    }

    #[test]
    fn needs_the_system_file() {
        let folder = tempdir().unwrap();
        assert_eq!(
            read(
                folder.path(),
                &folder.path().join("js/plugins.js"),
                &folder.path().join("js/plugins")
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
    }
}
