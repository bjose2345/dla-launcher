//! The texts other plugins of an RPG Maker MV or MZ game keep in their
//! settings (`js/plugins.js`), for the settings the launcher's settings
//! plugin has a rule for. The rules are read from that plugin's own text, so
//! the reader and the plugin go by one list.

use std::{borrow::Cow, collections::BTreeMap};

use dla_application::translation::has_japanese;
use serde_json::Value;

use crate::rpg_maker;

const RULES_START: &str = "const RULES = ";
const DEFAULTS_START: &str = "const DEFAULTS = ";
const LIST_END: &str = "\n    };";
const ENEMY_BOOK: &str = "NUUN_EnemyBook";
/// The entry of an enemy book page that draws a note's text. The setting
/// that names its tag names a tag of picture files for other entries.
const ENEMY_BOOK_DESCRIPTION: u64 = 70;
const SKILL_TREE: &str = "SkillTreeConfig";
/// The script the skill tree's configuration is written in, in the plugin
/// folder: code, not settings.
pub const SKILL_TREE_SCRIPT: &str = "SkillTreeConfig.js";
const SKILL_TREE_START: &str = "loadSkillTreeConfig";

#[derive(Debug, Default, Eq, PartialEq)]
pub struct SettingTexts {
    /// Labels and sentences, each translated whole: what the settings say,
    /// and the words a plugin's code draws where a setting says nothing.
    pub texts: Vec<String>,
    /// Pages of several paragraphs, translated by their parts.
    pub pages: Vec<String>,
    /// The tags of an enemy's note that the enemy book draws as text.
    pub enemy_book_tags: Vec<String>,
    /// Whether the list turns on the skill tree's configuration script.
    pub skill_tree: bool,
}

/// What the skill tree's configuration says to the player.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct SkillTreeTexts {
    /// The name each tree is listed under.
    pub names: Vec<String>,
    /// What a tree is for, and a note a skill of a tree may have.
    pub notes: Vec<String>,
}

/// What the plugins a list turns on show from their settings. A list that
/// cannot be read has none.
pub fn read(plugin_list: &str) -> SettingTexts {
    let rules = list(RULES_START);
    let defaults = list(DEFAULTS_START);
    let mut found = SettingTexts::default();
    for plugin in rpg_maker::entries(plugin_list).unwrap_or_default() {
        if plugin["status"].as_bool() != Some(true) {
            continue;
        }
        let name = plugin["name"].as_str().unwrap_or_default();
        let settings = &plugin["parameters"];
        for rule in rules.get(name).into_iter().flatten() {
            let (shape, path) = match rule.split_once(':') {
                Some(("list", path)) => (Shape::List, path),
                Some(("page", path)) => (Shape::Page, path),
                _ => (Shape::Text, rule.as_str()),
            };
            let keys = path.split('.').collect::<Vec<_>>();
            visit(settings, &keys, &mut |text| match shape {
                Shape::Text => found.texts.push(text.to_owned()),
                Shape::List => found.texts.extend(text.split(',').map(str::to_owned)),
                Shape::Page => found.pages.push(text.to_owned()),
            });
        }
        found
            .texts
            .extend(defaults.get(name).into_iter().flatten().cloned());
        if name == ENEMY_BOOK {
            found.enemy_book_tags = enemy_book_tags(settings);
        }
        found.skill_tree |= name == SKILL_TREE;
    }
    found.texts.retain(|text| has_japanese(text));
    found.pages.retain(|page| has_japanese(page));
    found
}

/// The configuration is an object of lists. A tree is a row of
/// `skillTreeTypes` (inside an actor's `types`): its key, its name, what it
/// is for. A skill is a row of `skillTreeInfo`: its key, the skill, its
/// cost, and after those an icon and a note. The keys are what the lists and
/// events' scripts link trees and skills by, so they are no text.
pub fn skill_tree_texts(script: &str) -> SkillTreeTexts {
    let script = script
        .find(SKILL_TREE_START)
        .map_or(script, |start| &script[start..]);
    let string = |row: &[Option<String>], at: usize| row.get(at).cloned().flatten();
    let trees = rows(script, "skillTreeTypes", 3);
    let skills = rows(script, "skillTreeInfo", 2);
    SkillTreeTexts {
        names: trees.iter().filter_map(|row| string(row, 1)).collect(),
        notes: trees
            .iter()
            .filter_map(|row| string(row, 2))
            .chain(skills.iter().filter_map(|row| string(row, 4)))
            .collect(),
    }
}

/// The rows of a list of a script (`name: [ … ]`), a row being a list at
/// `depth` counted from the list itself: for each element the string it is,
/// or `None` for anything else. Comments are passed over.
fn rows(script: &str, list: &str, depth: usize) -> Vec<Vec<Option<String>>> {
    let label = format!("{list}:");
    let written = script.match_indices(&label).find(|(at, _)| {
        let line = script[..*at].rfind('\n').map_or(0, |end| end + 1);
        !script[line..*at].contains("//")
    });
    let Some((start, _)) = written else {
        return Vec::new();
    };
    let text = &script[start + label.len()..];
    if !text.trim_start().starts_with('[') {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut element: Option<Option<String>> = None;
    let mut level = 0;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        let rest = &text[at..];
        if rest.starts_with("//") {
            at += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("/*") {
            at += rest.find("*/").map_or(rest.len(), |end| end + 2);
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => {
                let Some(end) = closing_quote(bytes, at) else {
                    break;
                };
                if level == depth {
                    element = Some(match element {
                        None => string_of(&text[at..=end]),
                        Some(_) => None,
                    });
                }
                at = end;
            }
            b'[' => {
                level += 1;
                if level == depth {
                    row.clear();
                    element = None;
                } else if level == depth + 1 {
                    element = Some(None);
                }
            }
            b']' => {
                if level == depth {
                    row.extend(element.take());
                    rows.push(std::mem::take(&mut row));
                }
                level -= 1;
                if level == 0 {
                    break;
                }
            }
            b',' if level == depth => row.push(element.take().flatten()),
            _ if level == depth && !byte.is_ascii_whitespace() => element = Some(None),
            _ => {}
        }
        at += 1;
    }
    rows
}

fn closing_quote(bytes: &[u8], open: usize) -> Option<usize> {
    let mut at = open + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 1,
            quote if quote == bytes[open] => return Some(at),
            _ => {}
        }
        at += 1;
    }
    None
}

/// The text of a string as a script writes it. One with escapes JSON does
/// not know, or with a part filled in as the script runs, is not read.
fn string_of(written: &str) -> Option<String> {
    if written.starts_with('"') {
        return serde_json::from_str(written).ok();
    }
    let text = &written[1..written.len() - 1];
    (!text.contains('\\') && !text.contains("${")).then(|| text.to_owned())
}

#[derive(Clone, Copy)]
enum Shape {
    Text,
    List,
    Page,
}

/// A list of the settings plugin, which writes it as JSON: what each
/// plugin's settings hold (`RULES`), or what its code draws (`DEFAULTS`).
fn list(start: &str) -> BTreeMap<String, Vec<String>> {
    let script = rpg_maker::PLUGINS[0].1;
    script
        .split_once(start)
        .and_then(|(_, rest)| rest.split_once(LIST_END))
        .and_then(|(rules, _)| serde_json::from_str(&format!("{rules}\n}}")).ok())
        .unwrap_or_default()
}

/// The texts at a path of keys, where `*` is every entry of a list or a
/// struct. A setting that holds a list or a struct holds it as JSON.
fn visit(owner: &Value, keys: &[&str], found: &mut impl FnMut(&str)) {
    let Some((key, rest)) = keys.split_first() else {
        if let Some(text) = owner.as_str() {
            found(&literal(text));
        }
        return;
    };
    let decoded = owner.as_str().map(serde_json::from_str::<Value>);
    let owner = match &decoded {
        Some(Ok(inner)) => inner,
        Some(Err(_)) => return,
        None => owner,
    };
    match owner {
        Value::Object(entries) if *key == "*" => {
            entries.values().for_each(|inner| visit(inner, rest, found));
        }
        Value::Array(entries) if *key == "*" => {
            entries.iter().for_each(|inner| visit(inner, rest, found));
        }
        Value::Object(entries) => {
            if let Some(inner) = entries.get(*key) {
                visit(inner, rest, found);
            }
        }
        Value::Array(entries) => {
            if let Some(inner) = key.parse::<usize>().ok().and_then(|at| entries.get(at)) {
                visit(inner, rest, found);
            }
        }
        _ => {}
    }
}

/// A setting may hold its text as a string of code: `"…"` (a note) or `'…'`.
fn literal(value: &str) -> Cow<'_, str> {
    if value.starts_with('"')
        && let Ok(Value::String(text)) = serde_json::from_str(value)
    {
        return Cow::Owned(text);
    }
    value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .filter(|text| !text.contains(['\'', '\\']))
        .map_or(Cow::Borrowed(value), Cow::Borrowed)
}

fn enemy_book_tags(settings: &Value) -> Vec<String> {
    let decoded = |value: &Value| match value.as_str() {
        Some(held) => serde_json::from_str::<Value>(held).unwrap_or_default(),
        None => value.clone(),
    };
    let mut tags = Vec::new();
    for setting in settings
        .as_object()
        .into_iter()
        .flat_map(|all| all.values())
    {
        for held in decoded(setting).as_array().into_iter().flatten() {
            let entry = decoded(held);
            let select = &entry["DateSelect"];
            let description = select
                .as_u64()
                .or_else(|| select.as_str().and_then(|number| number.parse().ok()))
                == Some(ENEMY_BOOK_DESCRIPTION);
            if let Some(tag) = entry["textMethod"].as_str()
                && description
                && !tag.is_empty()
                && !tags.iter().any(|known| known == tag)
            {
                tags.push(tag.to_owned());
            }
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn list_of(plugins: Value) -> String {
        format!("var $plugins =\n{plugins};\n")
    }

    #[test]
    fn reads_the_rules_from_the_settings_plugin() {
        let rules = list(RULES_START);
        assert!(rules["QuestSystem"].contains(&"page:QuestDatas.*.DetailNote".to_owned()));
        assert!(rules["ARTM_TMDescriptionExMZ"].contains(&"list:scopeValue".to_owned()));
        assert_eq!(
            rules["TorigoyaMZ_NotifyMessage_AddonGetItem"],
            [
                "baseGainSingleMessage",
                "baseGainMultiMessage",
                "baseGainMoneyMessage"
            ]
        );
    }

    #[test]
    fn finds_the_texts_the_rules_name_and_nothing_else() {
        let quests = json!([
            json!({ "Title": "生き抜く風", "DetailNote": "\"\\\\C[16]【場所】\\\\C[0]\\n日の出の丘\"", "VariableId": "12" }).to_string(),
            json!({ "Title": "Second", "DetailNote": "", "VariableId": "13" }).to_string(),
        ]);
        let found = read(&list_of(json!([
            { "name": "QuestSystem", "status": true, "parameters": {
                "QuestDatas": quests.to_string(),
                "Text": json!({ "RewardText": "【報酬】：", "MenuQuestSystemText": "ジャーナル" }).to_string(),
                "EnabledQuestMenuSwitchId": "0"
            } },
            { "name": "ARTM_TMDescriptionExMZ", "status": true, "parameters": {
                "plainText": "解説",
                "hitTypeValue": "必中,物理,OK",
                "width": "816"
            } },
            { "name": "NUUN_MenuScreenEX", "status": true, "parameters": {
                "HelpList": json!([json!({ "HelpCommandName": "'アイテム'", "HelpCommandText": "所持品を見る。" }).to_string()]).to_string()
            } },
            { "name": "CounterExtend", "status": true, "parameters": { "CounterList": "[\"{\\\"Id\\\":\\\"回復\\\"}\"]" } },
            { "name": "SkillTree", "status": false, "parameters": { "OpenedNodeText": "習得済み" } }
        ])));
        assert_eq!(
            found.texts[..8],
            [
                "生き抜く風",
                "ジャーナル",
                "【報酬】：",
                "解説",
                "必中",
                "物理",
                "アイテム",
                "所持品を見る。"
            ]
        );
        assert_eq!(found.texts[8..], list(DEFAULTS_START)["NUUN_MenuScreenEX"]);
        assert_eq!(found.pages, ["\\C[16]【場所】\\C[0]\n日の出の丘"]);
        assert!(found.enemy_book_tags.is_empty());
    }

    #[test]
    fn takes_the_enemy_books_description_tags_from_its_pages() {
        let page = |entries: Value| {
            Value::String(
                entries
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| Value::String(entry.to_string()))
                    .collect::<Value>()
                    .to_string(),
            )
        };
        let found = read(&list_of(json!([
            { "name": "NUUN_EnemyBook", "status": true, "parameters": {
                "CommandName": "モンスター図鑑",
                "PageList1": page(json!([
                    { "DateSelect": "1", "paramName": "弱点", "textMethod": "" },
                    { "DateSelect": "251", "textMethod": "pictures" }
                ])),
                "PageList3": page(json!([{ "DateSelect": "70", "textMethod": "text" }])),
                "PageList9": page(json!([
                    { "DateSelect": 70, "textMethod": "desc" },
                    { "DateSelect": "70", "textMethod": "text" }
                ]))
            } }
        ])));
        assert_eq!(found.texts[..2], ["モンスター図鑑", "弱点"]);
        assert_eq!(found.texts[2..], list(DEFAULTS_START)["NUUN_EnemyBook"]);
        assert_eq!(found.enemy_book_tags, ["text", "desc"]);
    }

    #[test]
    fn reads_the_skill_trees_names_and_notes_and_leaves_its_keys() {
        let script = r#"/*:
 * skillTreeTypes: [ ["見本", "見本の名", "見本の説明"] ]
 */
const loadSkillTreeConfig = () => {
return {
// skillTreeTypes: [ ["古い", "古い名", "古い説明"] ],
skillTreeTypes: [
    {
        actorId: 1,
        types: [
            ["サン１", "曙光の先触れ", "光を攻撃に転じる。"], // ["消した", "消した名", "消した説明"],
            ["サン２", '循環する命', "光を[防御]に転じる。", false],
        ]
    },
    { actorId: 3, types: [["エムナル１", "天使の寵愛", "すべてを" + "癒す"]] },
],
skillTreeMapId: { "サン１": 9 },
skillTreeInfo: [
    ["キュア", 21, 2],
    ["レイ", 36, 2, ["img", "光"], "光の矢を放つ。"],
    /* ["消した技", 1, 1, ["icon", 5], "消した説明"], */
    ["オーブ", 34, 6, ["icon", 12]],
],
skillTreeDerivative: { "サン１": [["キュア", ["レイ"]]] },
};
};
"#;
        assert_eq!(
            skill_tree_texts(script),
            SkillTreeTexts {
                names: vec![
                    "曙光の先触れ".to_owned(),
                    "循環する命".to_owned(),
                    "天使の寵愛".to_owned()
                ],
                notes: vec![
                    "光を攻撃に転じる。".to_owned(),
                    "光を[防御]に転じる。".to_owned(),
                    "光の矢を放つ。".to_owned()
                ],
            }
        );
        assert_eq!(
            skill_tree_texts("const other = 1;"),
            SkillTreeTexts::default()
        );
        assert_eq!(
            skill_tree_texts("skillTreeTypes: makeTypes(),"),
            SkillTreeTexts::default()
        );
    }

    #[test]
    fn adds_the_words_a_plugins_code_draws_when_the_plugin_is_on() {
        let defaults = list(DEFAULTS_START);
        assert!(defaults["NUUN_EnemyBook"].contains(&"ドロップアイテム".to_owned()));
        let found = read(&list_of(json!([
            { "name": "NUUN_StatusScreen", "status": true, "parameters": {} },
            { "name": "NUUN_EnemyBook", "status": false, "parameters": {} }
        ])));
        assert_eq!(found.texts, defaults["NUUN_StatusScreen"]);
        assert!(found.texts.contains(&"←→キー:項目の切替".to_owned()));
    }

    #[test]
    fn a_list_that_cannot_be_read_has_no_texts() {
        assert_eq!(read("var plugins = 1;"), SettingTexts::default());
    }
}
