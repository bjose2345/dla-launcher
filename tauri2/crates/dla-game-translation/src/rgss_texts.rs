//! Collecting the texts of an RGSS game from its event commands and its
//! scripts, the way each generation's reader walks them: a message is its
//! lines joined with `\n`, menu words and database entries as they are.
//! Each map's dialogue, and the shared events the map calls, are listed
//! under the map (`map:12`), so they are translated next once the player is
//! there. The Japanese strings in the game's own scripts are read too (a
//! label, a menu's help); the script shows their English only where they
//! are drawn, so a database name the scripts compare stays Japanese in the
//! data.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use dla_application::{
    game_translation::GameTexts,
    translation::{SourceText, has_japanese},
};
use dla_domain::translation::TextKind;

use crate::ruby_marshal::Value;

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
const VOCAB: &str = "Vocab";

#[derive(Default)]
pub struct Texts {
    pub kept: HashSet<String>,
    /// RPG Maker XP keeps a message's first line in the Show Text command
    /// itself; VX and VX Ace keep every line in the commands after it.
    pub text_in_show_text: bool,
    pub place: Option<String>,
    seen: HashSet<String>,
    texts: Vec<SourceText>,
    seen_names: HashSet<String>,
    names: Vec<String>,
    places: BTreeMap<String, BTreeSet<String>>,
    // Texts by the shared event they belong to, to list them under maps.
    by_place: HashMap<String, BTreeSet<String>>,
}

impl Texts {
    /// Starts with the strings the game's scripts use (`kept`).
    pub fn new(kept: HashSet<String>, text_in_show_text: bool) -> Self {
        Self {
            kept,
            text_in_show_text,
            ..Self::default()
        }
    }

    pub fn text(&mut self, value: &Value, kind: TextKind) {
        if let Some(text) = value.as_str() {
            self.add(text.to_owned(), kind);
        }
    }

    pub fn add(&mut self, text: String, kind: TextKind) {
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

    pub fn each(&mut self, values: &Value, kind: TextKind) {
        for value in values.items() {
            self.text(value, kind);
        }
    }

    /// Every text field of an object (a generation's menu words).
    pub fn fields(&mut self, object: &Value, kind: TextKind) {
        if let Value::Object { fields, .. } = object {
            for (_, value) in fields {
                self.text(value, kind);
            }
        }
    }

    pub fn name(&mut self, value: &Value) {
        if let Some(name) = value.as_str()
            && has_japanese(name)
            && !self.kept.contains(name)
            && self.seen_names.insert(name.to_owned())
        {
            self.names.push(name.to_owned());
        }
    }

    /// A name the scripts compare is translated too: the launcher's script
    /// shows it in English where it is drawn and leaves the data Japanese.
    pub fn database_name(&mut self, value: &Value) {
        self.text(value, TextKind::Database);
    }

    /// The Japanese strings of the game's scripts that are drawn whole.
    pub fn drawn_script_strings(&mut self) {
        let mut drawn = self
            .kept
            .iter()
            .filter(|text| drawn_whole(text))
            .cloned()
            .collect::<Vec<_>>();
        drawn.sort();
        for text in drawn {
            self.add(text, TextKind::Addon);
        }
    }

    /// An event's commands: messages, scrolling text and choices. Returns
    /// the shared events it calls.
    pub fn commands(&mut self, list: &Value) -> BTreeSet<u32> {
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
                    let first = (code == SHOW_TEXT && self.text_in_show_text).then(|| {
                        parameters
                            .first()
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    });
                    let lines = first
                        .into_iter()
                        .chain(
                            commands[at + 1..]
                                .iter()
                                .take_while(|line| {
                                    line.field("@code").as_integer() == Some(line_code)
                                })
                                .map(|line| {
                                    line.field("@parameters")
                                        .items()
                                        .first()
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                }),
                        )
                        .collect::<Vec<_>>();
                    let message = lines.join("\n");
                    for speaker in speakers(&message) {
                        if has_japanese(speaker)
                            && !self.kept.contains(speaker)
                            && self.seen_names.insert(speaker.to_owned())
                        {
                            self.names.push(speaker.to_owned());
                        }
                    }
                    self.add(message, TextKind::Dialogue);
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

    /// The shared events (`@list` each, by id) and the troops and maps of a
    /// game, with each map's dialogue listed under the map together with
    /// the shared events it calls, and the ones those call.
    pub fn events(
        &mut self,
        common_events: &Value,
        troops: &Value,
        maps: impl IntoIterator<Item = (u32, Value)>,
        map_texts: impl Fn(&mut Self, &Value),
    ) {
        let mut calls = HashMap::<u32, BTreeSet<u32>>::new();
        for event in common_events.items() {
            let Some(id) = event
                .field("@id")
                .as_integer()
                .and_then(|id| u32::try_from(id).ok())
            else {
                continue;
            };
            self.place = Some(format!("common:{id}"));
            calls.insert(id, self.commands(event.field("@list")));
        }
        for troop in troops.items() {
            self.place = None;
            for page in troop.field("@pages").items() {
                self.commands(page.field("@list"));
            }
        }
        for (id, map) in maps {
            let place = format!("map:{id}");
            self.place = Some(place.clone());
            map_texts(self, &map);
            let mut called = BTreeSet::new();
            for event in map.field("@events").values() {
                for page in event.field("@pages").items() {
                    called.extend(self.commands(page.field("@list")));
                }
            }
            let mut seen = HashSet::new();
            let mut next = called.into_iter().collect::<Vec<_>>();
            while let Some(common) = next.pop() {
                if seen.insert(common) {
                    self.add_place(&format!("common:{common}"), &place);
                    next.extend(calls.get(&common).into_iter().flatten().copied());
                }
            }
        }
        self.place = None;
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

/// The names a message gives its speakers in `\name[…]`, the code of the
/// message scripts that show a speaker's name in a box of its own. The
/// message keeps the code as it is, and the box draws the name: with the
/// other names, it is drawn in English. A name that is itself a code
/// (`\name[\v[2]]`) is not one to translate.
fn speakers(message: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = message;
    while let Some(at) = rest.find("\\name[").or_else(|| rest.find("\\Name[")) {
        let name = &rest[at + 6..];
        let Some(end) = name.find([']', '\n']) else {
            break;
        };
        if name[end..].starts_with(']') && !name[..end].contains(['\\', '[']) {
            found.push(&name[..end]);
        }
        rest = &name[end..];
    }
    found
}

/// The Japanese strings in the game's scripts but `Vocab`'s: what the
/// scripts draw, and the database names they compare, which must stay
/// Japanese in the data.
pub fn script_literals(scripts: &[(String, String)]) -> HashSet<String> {
    scripts
        .iter()
        .filter(|(title, _)| title != VOCAB)
        .flat_map(|(_, source)| script_strings(source))
        .filter(|text| has_japanese(text))
        .collect()
}

/// A script's string that is drawn as it is: a file name or a note tag
/// (`<封印>`) never is, and one character alone (人, 個) only beside a number.
pub fn drawn_whole(text: &str) -> bool {
    has_japanese(text) && !text.contains('/') && !text.starts_with('<') && text.chars().count() > 1
}

/// The Japanese strings assigned to constants in `module Vocab` blocks (the
/// `Vocab` script, and games' own scripts adding to it): the battle messages
/// and menu words the game's code uses.
pub fn vocab_strings(source: &str) -> Vec<String> {
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
pub fn script_strings(source: &str) -> Vec<String> {
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

/// An event command for tests.
#[cfg(test)]
pub(crate) fn command(writer: &mut crate::ruby_marshal::Writer, code: i64, parameters: &[&str]) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_speakers_a_message_names() {
        assert_eq!(
            speakers("\\name[アンジュ]\\_\nいえいえ。\\Name[寝室の使用方法]"),
            ["アンジュ", "寝室の使用方法"]
        );
        // A name held in a variable, and a code never closed.
        assert!(speakers("\\name[\\v[2]]こんにちは\n\\name[途中").is_empty());
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
