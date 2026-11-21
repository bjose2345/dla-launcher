//! The sentences an RPG Maker XP or VX game's own scripts put together
//! (`text = "#{name}　を手に入れた！"`, a battle's narration). Its data never
//! holds them, and what reaches the screen is not what a script holds, so
//! no table of whole lines could swap them there.
//!
//! The launcher does not change the game's scripts. It writes copies of the
//! sections that hold such sentences, with the English in them, into its own
//! folder, and its script, which runs first, hands the engine a copy in
//! place of a section (`DLA_Translate.swap_sections`): only for the very
//! source the copy was made from, and only when the copy is Ruby the engine
//! reads.
//!
//! A string is such a sentence when it is the value of an assignment, a
//! piece joined to one, or an item a list takes in; reads as a sentence (a
//! value Ruby puts in, sentence punctuation, a label's colon); and is used
//! nowhere as an identifier. Single words stay as they are: scripts compare
//! them, and the launcher's script already draws the English of the ones
//! drawn whole.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::Path,
};

use dla_application::{
    game_translation::GameTranslationError,
    translation::{SourceText, has_japanese},
};
use flate2::Crc;

use crate::ruby_files::{
    Literal, Text, encoded, kind, literals, read_as_ruby, remove_others, with_stand_ins,
};

/// Where the copies go, in the launcher's folder in the game.
pub const FOLDER: &str = "sections";
/// The copies there are, each with what the source it was made from sums
/// to and how long that is: the launcher's script checks both.
const LIST: &str = "list.txt";
/// The escapes Ruby gives a meaning; a backslash before another letter is
/// dropped, and the string holds the letter alone.
const RUBY_ESCAPES: &[u8] = b"nsrtabefvxucCM";

#[derive(Default)]
pub struct ScriptTexts {
    /// The sections with such a sentence, by their place among the game's.
    sections: Vec<(usize, String)>,
    texts: Vec<String>,
    shown: HashSet<String>,
    letters: String,
}

impl ScriptTexts {
    /// `sections` are the game's scripts in their order, as (title, source),
    /// and `used` the strings its Ruby files use as identifiers.
    pub fn read(sections: &[(String, String)], used: &HashSet<String>) -> Self {
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();
        let mut identifiers = HashSet::new();
        let mut letters = BTreeSet::new();
        let mut with_texts = Vec::new();
        for (index, (_, source)) in sections.iter().enumerate() {
            letters.extend(code_letters(source));
            let Some(literals) = literals(source).filter(|literals| {
                // A string broken over lines is rare, and what a reader
                // that lost its place would find: such a section stays.
                literals
                    .iter()
                    .all(|literal| !source[literal.body.clone()].contains('\n'))
            }) else {
                continue;
            };
            // A source that is not UTF-8 was read with a mark for what is
            // not: a copy of it would not be the game's script.
            let exact = !source.contains('\u{fffd}');
            let mut any = false;
            for literal in &literals {
                let body = &source[literal.body.clone()];
                if !has_japanese(body) {
                    continue;
                }
                match shown(literal, source).filter(|_| exact) {
                    Some(text) => {
                        any = true;
                        if seen.insert(text.text.clone()) {
                            candidates.push(text.text);
                        }
                    }
                    None => {
                        if let Some(text) = read_as_ruby(body, literal.quote) {
                            identifiers.insert(text.text);
                        }
                    }
                }
            }
            if any {
                with_texts.push((index, source.clone()));
            }
        }
        let texts = candidates
            .into_iter()
            .filter(|text| !identifiers.contains(text) && !used.contains(text))
            .collect::<Vec<_>>();
        Self {
            shown: texts.iter().cloned().collect(),
            sections: with_texts,
            texts,
            letters: letters.into_iter().collect(),
        }
    }

    /// The sentences, in the order of the game's scripts.
    pub fn texts(&self) -> impl Iterator<Item = SourceText> + '_ {
        self.texts.iter().map(|text| SourceText {
            text: text.clone(),
            kind: kind(text),
        })
    }

    /// The letters the game's scripts hold as codes: the ones in strings
    /// that are nothing but letters after a backslash Ruby drops (`"\w"` is
    /// `w`, which a window compares each character with). No English word
    /// may hold them as they are.
    pub fn letters(&self) -> &str {
        &self.letters
    }

    /// Writes a copy of every section that has English for a sentence of
    /// its own into `folder`, lists them, and takes away the copies no
    /// section needs any more. Returns how many copies there are.
    pub fn write(
        &self,
        folder: &Path,
        english: &BTreeMap<String, String>,
    ) -> Result<usize, GameTranslationError> {
        let mut kept = HashSet::new();
        let mut list = String::new();
        for (index, source) in &self.sections {
            let Some(copy) = self.translated(source, english) else {
                continue;
            };
            let target = folder.join(format!("{index:04}.rb"));
            fs::create_dir_all(folder).map_err(GameTranslationError::folder)?;
            if fs::read(&target).ok().as_deref() != Some(copy.as_bytes()) {
                crate::replace(&target, copy.as_bytes())?;
            }
            kept.insert(target);
            let mut sum = Crc::new();
            sum.update(source.as_bytes());
            list.push_str(&format!("{index} {} {}\n", sum.sum(), source.len()));
        }
        let copies = kept.len();
        if copies > 0 {
            let target = folder.join(LIST);
            if fs::read(&target).ok().as_deref() != Some(list.as_bytes()) {
                crate::replace(&target, list.as_bytes())?;
            }
            kept.insert(target);
        }
        remove_others(folder, &kept);
        Ok(copies)
    }

    fn translated(&self, source: &str, english: &BTreeMap<String, String>) -> Option<String> {
        let mut copy = String::with_capacity(source.len());
        let mut from = 0;
        for literal in literals(source)? {
            let Some(text) = shown(&literal, source) else {
                continue;
            };
            let Some(english) = english
                .get(&text.text)
                .filter(|english| self.shown.contains(&text.text) && **english != text.text)
            else {
                continue;
            };
            // A translator gives one line: the breaks the sentence starts
            // and ends with are the rows the scripts put it in.
            let before = &text.text[..text.text.len() - text.text.trim_start_matches('\n').len()];
            let after = &text.text[text.text.trim_end_matches('\n').len()..];
            let english = english.trim_matches('\n');
            // Japanese joins its pieces without a space.
            let joined = before.is_empty() && joins(literal.before.trim_end());
            let space = match english.chars().next() {
                Some(first) if joined && first.is_ascii_alphanumeric() => " ",
                _ => "",
            };
            let english = format!("{before}{space}{english}{after}");
            copy.push_str(&source[from..literal.body.start - 1]);
            copy.push('"');
            copy.push_str(&encoded(
                &with_stand_ins(&english, &self.letters, &text.values, &text.bare),
                &text.values,
                &text.bare,
            ));
            copy.push('"');
            from = literal.body.end + 1;
        }
        (from > 0).then(|| {
            copy.push_str(&source[from..]);
            copy
        })
    }
}

/// Whether the string after `before` is added to what is already there
/// (`a + "…"`, `text += "…"`, `text << "…"`).
fn joins(before: &str) -> bool {
    before.ends_with('+') || before.ends_with("+=") || before.ends_with("<<")
}

/// The letters of the strings in `source` that are nothing but letters
/// after a backslash (`"\w"`, `"\w\q"`, `"\n\m"`), but the ones Ruby
/// gives a meaning. A letter that only follows a backslash inside longer
/// strings is left out: it may be a part of anything.
fn code_letters(source: &str) -> Vec<char> {
    let bytes = source.as_bytes();
    let mut letters = Vec::new();
    let mut at = 0;
    while let Some(start) = bytes[at..].iter().position(|&byte| byte == b'"') {
        let mut end = at + start + 1;
        let mut found = Vec::new();
        while bytes.get(end) == Some(&b'\\')
            && bytes.get(end + 1).is_some_and(u8::is_ascii_alphabetic)
        {
            found.push(bytes[end + 1]);
            end += 2;
        }
        if !found.is_empty() && bytes.get(end) == Some(&b'"') {
            letters.extend(
                found
                    .into_iter()
                    .filter(|letter| !RUBY_ESCAPES.contains(letter))
                    .map(char::from),
            );
            at = end + 1;
        } else {
            at += start + 1;
        }
    }
    letters
}

/// The string as a sentence the game's scripts show, when it is one.
fn shown<'a>(literal: &Literal<'_>, source: &'a str) -> Option<Text<'a>> {
    if !(shows(literal.before.trim_end()) && ends_there(literal.after.trim_start())) {
        return None;
    }
    let text = read_as_ruby(&source[literal.body.clone()], literal.quote)?;
    let sentence = !text.values.is_empty()
        || text
            .text
            .contains(['。', '！', '？', '…', '、', '「', '」', '!', '?'])
        || text.text.ends_with([':', '：']);
    let name_of_a_file = text.text.contains('/')
        || text.text.rsplit_once('.').is_some_and(|(_, ending)| {
            (2..=4).contains(&ending.len())
                && ending.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
    // `"%sを倒した！" % name`: an English that lost the `%s` would stop the
    // game.
    let format = text.text.match_indices('%').any(|(at, _)| {
        text.text[at + 1..]
            .starts_with(|next: char| next.is_ascii_alphanumeric() || "-+.".contains(next))
    });
    (has_japanese(&text.text) && sentence && !name_of_a_file && !format).then_some(text)
}

/// Whether a string after `before` is one the scripts show: what a name is
/// given (`text = `, `text += `, `text << `, `key => `), what a method hands
/// back, a piece joined on (`name + `), what a method is called with
/// (`lines.push(`, `SR_Util.announce(`) unless the method looks the string
/// up or reads a file by it, or an item of a list in one of those places.
fn shows(before: &str) -> bool {
    if given(before) || before.ends_with('+') || ends_with_word(before, "return") {
        return true;
    }
    if !before.ends_with(['(', '[', ',']) {
        return false;
    }
    let Some(open) = unclosed(before) else {
        return false;
    };
    let head = before[..open].trim_end();
    match before.as_bytes()[open] {
        b'[' => {
            given(head)
                || ends_with_word(head, "return")
                || head.strip_suffix('(').is_some_and(takes_in)
        }
        _ => takes_in(head),
    }
}

/// Methods that look a string up, take it apart or run it, and words that
/// are no method at all.
const NOT_SHOWN_BY: [&str; 36] = [
    "include",
    "index",
    "rindex",
    "match",
    "scan",
    "sub",
    "gsub",
    "delete",
    "tr",
    "split",
    "count",
    "slice",
    "search",
    "key",
    "fetch",
    "send",
    "eval",
    "require",
    "load",
    "open",
    "const_get",
    "instance_variable_get",
    "instance_variable_set",
    "respond_to",
    "exist",
    "if",
    "unless",
    "while",
    "until",
    "elsif",
    "when",
    "and",
    "or",
    "not",
    "p",
    "print",
];
/// What reads a file by the string it is given.
const READS_FILES: [&str; 7] = [
    "Cache",
    "Audio",
    "Bitmap",
    "File",
    "Dir",
    "load_data",
    "save_data",
];

fn given(before: &str) -> bool {
    before.ends_with("=>")
        || before.ends_with("<<")
        || before
            .strip_suffix('=')
            .is_some_and(|target| !target.ends_with(['=', '!', '<', '>']))
}

fn ends_with_word(before: &str, word: &str) -> bool {
    before.strip_suffix(word).is_some_and(|rest| {
        !rest.ends_with(|last: char| last.is_ascii_alphanumeric() || last == '_' || last == '.')
    })
}

/// Whether `head`, the code up to a `(`, calls a method that shows what
/// it is given, as far as its name tells: not one that asks (`…?`), looks
/// the string up or reads a file.
fn takes_in(head: &str) -> bool {
    let head = head.trim_end();
    let name = &head[head
        .rfind(|character: char| !(character.is_ascii_alphanumeric() || "_?!".contains(character)))
        .map_or(0, |at| at + 1)..];
    let method = name.trim_end_matches('!');
    !method.ends_with('?')
        && !NOT_SHOWN_BY.contains(&method)
        && !READS_FILES.iter().any(|reader| head.contains(reader))
}

/// Where the bracket that the end of `before` is inside opens.
fn unclosed(before: &str) -> Option<usize> {
    let mut depth = 0;
    for (at, byte) in before.bytes().enumerate().rev() {
        match byte {
            b')' | b']' => depth += 1,
            b'(' | b'[' if depth == 0 => return Some(at),
            b'(' | b'[' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Whether the string is all of a value: nothing follows it that would
/// read it (`== x`, `.size`, `% name`).
fn ends_there(after: &str) -> bool {
    after.is_empty()
        || after.starts_with(['#', '+', ',', ';', ')', ']', '}', '\\'])
        || ["if", "unless", "while", "until"].iter().any(|word| {
            after
                .strip_prefix(word)
                .is_some_and(|rest| rest.starts_with([' ', '\t', '(']))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dla_domain::translation::TextKind;

    const BATTLE: &str = concat!(
        "class Game_Battler\r\n",
        "  def narrate(skill)\r\n",
        "    text = \"\\\\T[#{id}]#{$data_items[id].name}　を手に入れた！\"\r\n",
        "    text += \"#{myname}は、\\n\\m#{targetname}に押し倒された！\\w\\q\" if pinned\r\n",
        "    action = premess + \"口で愛撫した！\"\r\n",
        "    lines = [\"満足げな笑みを浮かべている……！\", \"くすくすと笑っている……！\"]\r\n",
        "    lines.push(\"#{master}に微笑んだ……！\") if smiling\r\n",
        "    SR_Util.announce(\"#{name}の\\n運気が戻ってきた！\")\r\n",
        "    RPG::Cache.picture(\"立ち絵、通常！\")\r\n",
        "    wait if text.include?(\"を手に入れた！\")\r\n",
        "    type = \"アイテム:\"\r\n",
        "    text += \"\\n#{name}は倒れた……\\n\"\r\n",
        "    return \"しかし#{name}には効果が無かった！\" if m != \"しかし#{name}には効果が無かった！\"\r\n",
        "    return \"効果は抜群だ！\" if self.have_ability?(\"過敏な身体\")\r\n",
        "    @name = \"無我夢中\"\r\n",
        "    action.gsub!(\"愛撫\", \"優しく愛撫、\")\r\n",
        "    p \"使用不可：#{skill.name}\" if $DEBUG\r\n",
        "    face = \"顔/笑い！\"\r\n",
        "    format = \"%sを倒した！\" % name\r\n",
        "    text.gsub!(\"\\q\\m\", \"\\m\\q\")\r\n",
        "    if c == \"\\w\"\r\n",
        "      wait if text =~ /[\"']/ && c != \"\\H\"\r\n",
        "    end\r\n",
        "  end\r\n",
        "end\r\n",
    );

    fn sections(source: &str) -> Vec<(String, String)> {
        vec![
            ("Empty".to_owned(), String::new()),
            ("Game_Battler".to_owned(), source.to_owned()),
        ]
    }

    #[test]
    fn takes_the_sentences_scripts_put_together_and_leaves_what_they_compare() {
        let scripts = ScriptTexts::read(&sections(BATTLE), &HashSet::new());
        assert_eq!(
            scripts
                .texts()
                .map(|text| (text.text, text.kind))
                .collect::<Vec<_>>(),
            [
                (
                    "\\T[#{id}]#{$data_items[id].name}　を手に入れた！".to_owned(),
                    TextKind::Dialogue
                ),
                (
                    "#{myname}は、\n\\m#{targetname}に押し倒された！\\w\\q".to_owned(),
                    TextKind::Dialogue
                ),
                ("口で愛撫した！".to_owned(), TextKind::Dialogue),
                (
                    "満足げな笑みを浮かべている……！".to_owned(),
                    TextKind::Dialogue
                ),
                ("くすくすと笑っている……！".to_owned(), TextKind::Dialogue),
                ("#{master}に微笑んだ……！".to_owned(), TextKind::Dialogue),
                (
                    "#{name}の\n運気が戻ってきた！".to_owned(),
                    TextKind::Dialogue
                ),
                ("アイテム:".to_owned(), TextKind::Addon),
                ("\n#{name}は倒れた……\n".to_owned(), TextKind::Dialogue),
                ("効果は抜群だ！".to_owned(), TextKind::Dialogue),
            ]
        );
        assert_eq!(scripts.letters(), "Hmqw");

        // A sentence a Ruby file uses as an identifier stays too.
        let used = HashSet::from(["効果は抜群だ！".to_owned()]);
        let scripts = ScriptTexts::read(&sections(BATTLE), &used);
        assert!(scripts.texts().all(|text| text.text != "効果は抜群だ！"));
    }

    #[test]
    fn writes_copies_with_the_english_as_strings_and_nothing_else() {
        let scripts = ScriptTexts::read(&sections(BATTLE), &HashSet::new());
        let english = BTreeMap::from([
            (
                "\\T[#{id}]#{$data_items[id].name}　を手に入れた！".to_owned(),
                "\\T[#{id}]#{$data_items[id].name} obtained! \"Wow\" #{exit}".to_owned(),
            ),
            (
                "#{myname}は、\n\\m#{targetname}に押し倒された！\\w\\q".to_owned(),
                "\\m#{targetname} pushed #{myname} down, somehow!\\w\\q".to_owned(),
            ),
            (
                "口で愛撫した！".to_owned(),
                "caressed with the mouth!".to_owned(),
            ),
            ("アイテム:".to_owned(), "Item:".to_owned()),
            (
                "\n#{name}は倒れた……\n".to_owned(),
                "#{name} fell...".to_owned(),
            ),
            ("無我夢中".to_owned(), "Lost in the moment".to_owned()),
        ]);
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join(FOLDER);
        assert_eq!(scripts.write(&folder, &english).unwrap(), 1);

        let copy = fs::read_to_string(folder.join("0001.rb")).unwrap();
        let w = '\u{f877}';
        let m = '\u{f86d}';
        let h = '\u{f868}';
        // Only `H`, `m`, `q` and `w` are codes: `h` is not.
        assert!(!copy.contains(h));
        assert!(copy.contains(&format!(
            "text = \"\\\\T[#{{id}}]#{{$data_items[id].name}} obtained! \\\"Wo{w}\\\" \\#{{exit}}\"\r\n"
        )));
        assert!(copy.contains(&format!(
            "text += \"\\m#{{targetname}} pushed #{{myname}} do{w}n, so{m}eho{w}!\\w\\q\" if pinned\r\n"
        )));
        assert!(copy.contains(&format!(
            "action = premess + \" caressed {w}ith the {m}outh!\"\r\n"
        )));
        assert!(copy.contains(&format!("type = \"Ite{m}:\"\r\n")));
        // The rows a sentence starts and ends are kept.
        assert!(copy.contains("text += \"\\n#{name} fell...\\n\"\r\n"));
        // What the scripts compare is as the game wrote it.
        assert!(copy.contains("@name = \"無我夢中\"\r\n"));
        assert!(copy.contains("if m != \"しかし#{name}には効果が無かった！\"\r\n"));
        assert!(copy.contains("if c == \"\\w\"\r\n"));
        let source_sum = {
            let mut sum = Crc::new();
            sum.update(BATTLE.as_bytes());
            sum.sum()
        };
        assert_eq!(
            fs::read_to_string(folder.join(LIST)).unwrap(),
            format!("1 {source_sum} {}\n", BATTLE.len())
        );

        // No English left: the copies go.
        assert_eq!(scripts.write(&folder, &BTreeMap::new()).unwrap(), 0);
        assert!(!folder.join("0001.rb").exists());
        assert!(!folder.join(LIST).exists());
    }

    #[test]
    fn leaves_a_section_it_cannot_follow() {
        let broken = "text = \"一行目、\n二行目！\"\n";
        let scripts = ScriptTexts::read(&sections(broken), &HashSet::new());
        assert_eq!(scripts.texts().count(), 0);
        let heredoc =
            "text = <<EOS\n「こんにちは！」\nEOS\nlabel = \"名前：\"\nwait if c == \"\\y\"\n";
        let scripts = ScriptTexts::read(&sections(heredoc), &HashSet::new());
        assert_eq!(scripts.texts().count(), 0);
        assert_eq!(scripts.letters(), "y");
    }
}
