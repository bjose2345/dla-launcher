//! The Ruby files an RPG Maker XP, VX or VX Ace game loads from its own
//! folder while it starts (`Dir.glob("System/talk/*/*.rb")` and `load`). A
//! game that keeps dialogue there keeps it outside its data, where
//! `rgss_texts` never sees it.
//!
//! The launcher does not change those files. It writes copies holding the
//! English into its own folder, and its script makes the game load a copy in
//! place of the file (`DLA_Translate.script_for`), or the file itself when
//! the copy fails.
//!
//! Only a string that is the whole value of an assignment or of a hash entry
//! is text (`tx["tx1"] = "…"`, `@target = "…"`). A string used any other way,
//! in these files or in the game's scripts (compared, a `when`, passed to a
//! method, in a list), is an identifier: it stays as it is and joins the
//! strings the game's scripts use, so the data keeps its Japanese too.
//!
//! A copy is Ruby the game runs, so the English is written as a string's
//! contents and nothing else: quotes and backslashes escaped, and a `#{…}`
//! kept as code only when the Japanese string has the very same one.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs, io,
    ops::{Range, RangeInclusive},
    path::{Path, PathBuf},
};

use dla_application::{
    game_translation::GameTranslationError,
    translation::{SourceText, has_japanese, name_parts},
};
use dla_domain::translation::TextKind;
use walkdir::WalkDir;

use crate::{rgss_texts::script_strings, wrap};

/// Where the copies go, in the launcher's folder in the game.
pub const FOLDER: &str = "scripts";
const LARGEST_FILE: u64 = 16 * 1024 * 1024;
const MOST_FILES: usize = 20_000;
const FEWEST_TEXTS: usize = 30;
// XP's and VX's message windows: four rows, about 24 kanji across.
const DEFAULT_ROOM: Room = Room {
    columns: 48,
    lines: 4,
};
const COLUMNS: RangeInclusive<usize> = 24..=96;
const LINES: RangeInclusive<usize> = 2..=6;
/// What a value Ruby puts into a line (`#{target}`) is taken to need: a
/// name of four kanji.
const VALUE_COLUMNS: usize = 8;
/// Stands for a value while a line is measured and broken, so a value with
/// spaces in it (`#{count + 1}`) stays whole.
const FIRST_MARK: u32 = 0xe000;
const LAST_MARK: u32 = 0xf7ff;

/// The room the files' own texts use: columns of half a kanji, and lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Room {
    columns: usize,
    lines: usize,
}

pub struct RubyFiles {
    files: Vec<(String, PathBuf)>,
    texts: Vec<String>,
    shown: HashSet<String>,
    used: HashSet<String>,
    room: Room,
}

impl RubyFiles {
    /// `sections` are the game's scripts as (title, source) and `in_scripts`
    /// their Japanese strings.
    pub fn read(
        game: &Path,
        sections: &[(String, String)],
        in_scripts: &HashSet<String>,
    ) -> io::Result<Self> {
        let files = loaded(game, sections);
        let mut assigned = Vec::new();
        let mut seen = HashSet::new();
        let mut used = HashSet::new();
        let mut widths = Vec::new();
        let mut heights = Vec::new();
        for (_, path) in &files {
            let Some(source) = source(path)? else {
                continue;
            };
            let Some(literals) = literals(&source) else {
                continue;
            };
            for literal in literals {
                if !has_japanese(&source[literal.body.clone()]) {
                    continue;
                }
                match literal.text(&source) {
                    Some(text) => {
                        if seen.insert(text.text.clone()) {
                            let marked = marked(&text.text, &text.values);
                            heights.push(marked.lines().count());
                            widths.extend(marked.lines().map(columns));
                            assigned.push(text.text);
                        }
                    }
                    None => used.extend(
                        script_strings(&source[literal.body.start - 1..literal.body.end + 1])
                            .into_iter()
                            .filter(|piece| has_japanese(piece)),
                    ),
                }
            }
        }
        let texts = assigned
            .into_iter()
            .filter(|text| !used.contains(text) && !in_scripts.contains(text))
            .collect::<Vec<_>>();
        let room = match (
            texts.len() >= FEWEST_TEXTS,
            wrap::all_but_the_largest(widths),
            wrap::all_but_the_largest(heights),
        ) {
            (true, Some(width), Some(height)) => Room {
                columns: width.clamp(*COLUMNS.start(), *COLUMNS.end()),
                lines: height.clamp(*LINES.start(), *LINES.end()),
            },
            _ => DEFAULT_ROOM,
        };
        Ok(Self {
            shown: texts.iter().cloned().collect(),
            files,
            texts,
            used,
            room,
        })
    }

    /// The texts the files show, in the order the game loads them.
    pub fn texts(&self) -> impl Iterator<Item = SourceText> + '_ {
        self.texts.iter().map(|text| SourceText {
            text: text.clone(),
            kind: kind(text),
        })
    }

    /// The Japanese strings the files use as identifiers.
    pub fn used(&self) -> &HashSet<String> {
        &self.used
    }

    /// Writes a copy of every file that has English for a text of its own
    /// into `folder`, and takes away the copies no file needs any more.
    /// Returns how many copies there are. `letters` are the letters the game
    /// draws as codes.
    pub fn write(
        &self,
        folder: &Path,
        english: &BTreeMap<String, String>,
        letters: &str,
    ) -> Result<usize, GameTranslationError> {
        let mut copies = HashSet::new();
        for (relative, path) in &self.files {
            let Some(source) = source(path).map_err(GameTranslationError::folder)? else {
                continue;
            };
            let Some(copy) = self.translated(&source, english, letters) else {
                continue;
            };
            let target = folder.join(relative);
            if fs::read(&target).ok().as_deref() != Some(copy.as_bytes()) {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(GameTranslationError::folder)?;
                }
                crate::replace(&target, copy.as_bytes())?;
            }
            copies.insert(target);
        }
        remove_others(folder, &copies);
        Ok(copies.len())
    }

    fn translated(
        &self,
        source: &str,
        english: &BTreeMap<String, String>,
        letters: &str,
    ) -> Option<String> {
        let mut copy = String::with_capacity(source.len());
        let mut from = 0;
        for literal in literals(source)? {
            let Some(text) = literal.text(source) else {
                continue;
            };
            let Some(english) = english
                .get(&text.text)
                .filter(|english| self.shown.contains(&text.text) && **english != text.text)
            else {
                continue;
            };
            let lines = text.text.lines().count().max(self.room.lines);
            let marked = wrap::wrap_into(
                &marked(english, &text.values),
                lines,
                self.room.columns,
                &columns,
            )
            .join("\n");
            copy.push_str(&source[from..literal.body.start - 1]);
            copy.push('"');
            copy.push_str(&encoded(
                &with_stand_ins(
                    &unmarked(&marked, &text.values),
                    letters,
                    &text.values,
                    &text.bare,
                ),
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

/// Takes the launcher's copies out of `folder` but for `copies`.
pub(crate) fn remove_others(folder: &Path, copies: &HashSet<PathBuf>) {
    for entry in WalkDir::new(folder)
        .contents_first(true)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if entry.file_type().is_dir() {
            // Only an empty folder goes.
            let _ = fs::remove_dir(path);
        } else if !copies.contains(path) {
            let _ = fs::remove_file(path);
        }
    }
}

pub(crate) fn kind(text: &str) -> TextKind {
    if text.contains(['\n', '「', '」', '。', '！', '？', '…', '!', '?']) {
        TextKind::Dialogue
    } else {
        TextKind::Addon
    }
}

/// A file's source when it is one the launcher reads: UTF-8, as the engines
/// take their scripts, and not huge.
fn source(path: &Path) -> io::Result<Option<String>> {
    if fs::metadata(path)?.len() > LARGEST_FILE {
        return Ok(None);
    }
    Ok(String::from_utf8(fs::read(path)?).ok())
}

/// The game's files that a path or pattern ending in `.rb` among the
/// strings of its scripts names, by their path in the game.
fn loaded(game: &Path, sections: &[(String, String)]) -> Vec<(String, PathBuf)> {
    let mut patterns = BTreeSet::new();
    for (_, source) in sections {
        if source.contains(".rb") {
            patterns.extend(
                script_strings(source)
                    .iter()
                    .filter_map(|text| pattern(text)),
            );
        }
    }
    let mut found = BTreeMap::new();
    for pattern in &patterns {
        let parts = pattern.split('/').collect::<Vec<_>>();
        matching(game, "", &parts, &mut found);
    }
    found.into_iter().collect()
}

/// A string of the game's scripts as a pattern of files in the game's
/// folder, when it is one: `System/talk/*/*.rb`, `Mod\SCRP_Mod.rb`.
fn pattern(text: &str) -> Option<String> {
    let text = text.replace('\\', "/");
    let text = text.trim_start_matches("./");
    let usable = text.len() > 3
        && text.to_ascii_lowercase().ends_with(".rb")
        && !text.contains([':', '{', '['])
        && text
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    usable.then(|| text.to_owned())
}

fn matching(folder: &Path, relative: &str, parts: &[&str], found: &mut BTreeMap<String, PathBuf>) {
    let Some((part, rest)) = parts.split_first() else {
        return;
    };
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let ours = relative.is_empty() && name.eq_ignore_ascii_case(crate::DATA_FOLDER);
        if ours || name.starts_with('.') || !matches(part, name) {
            continue;
        }
        let path = entry.path();
        let relative = if relative.is_empty() {
            name.to_owned()
        } else {
            format!("{relative}/{name}")
        };
        if !rest.is_empty() {
            if path.is_dir() {
                matching(&path, &relative, rest, found);
            }
        } else if path.is_file() && found.len() < MOST_FILES {
            found.insert(relative, path);
        }
    }
}

/// Whether a file's name fits one part of a pattern, where `*` stands for
/// any run of characters and `?` for one. Windows does not tell capitals
/// from small letters in names, so neither does this.
fn matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let name = name.to_ascii_lowercase().chars().collect::<Vec<_>>();
    let (mut at, mut in_name) = (0, 0);
    let mut star = None;
    while in_name < name.len() {
        if at < pattern.len() && (pattern[at] == '?' || pattern[at] == name[in_name]) {
            at += 1;
            in_name += 1;
        } else if at < pattern.len() && pattern[at] == '*' {
            star = Some((at, in_name));
            at += 1;
        } else if let Some((star_at, taken)) = star {
            at = star_at + 1;
            in_name = taken + 1;
            star = Some((star_at, taken + 1));
        } else {
            return false;
        }
    }
    pattern[at..].iter().all(|&character| character == '*')
}

/// A `"…"` or `'…'` string in a Ruby source.
pub(crate) struct Literal<'a> {
    /// What is between its quotes, as the source has it.
    pub(crate) body: Range<usize>,
    pub(crate) quote: u8,
    /// The code on its line before it and after it.
    pub(crate) before: &'a str,
    pub(crate) after: &'a str,
}

/// A string as Ruby reads it, with every `#{…}` left as written, and those
/// `#{…}` one by one. A letter after one backslash, which Ruby reads as the
/// letter alone, keeps its backslash (`\m`): games draw such letters as
/// codes, and a translator has to see them as codes too.
pub(crate) struct Text<'a> {
    pub(crate) text: String,
    pub(crate) values: Vec<&'a str>,
    /// Those letters.
    pub(crate) bare: Vec<char>,
}

impl Literal<'_> {
    /// The string as a text the game shows, when it is one: the whole value
    /// of an assignment or of a hash entry, with Japanese in it, and not a
    /// file's name.
    fn text<'a>(&self, source: &'a str) -> Option<Text<'a>> {
        let before = self.before.trim_end();
        let assigned = before.ends_with("=>")
            || before
                .strip_suffix('=')
                .is_some_and(|target| !target.ends_with(['=', '!', '<', '>']));
        let after = self.after.trim_start();
        let whole = after.is_empty()
            || after.starts_with(['#', '+', ',', ';', '}'])
            || ["if", "unless", "while", "until"].iter().any(|word| {
                after
                    .strip_prefix(word)
                    .is_some_and(|rest| rest.starts_with([' ', '\t', '(']))
            });
        if !assigned || !whole {
            return None;
        }
        let text = read_as_ruby(&source[self.body.clone()], self.quote)?;
        let name_of_a_file = text.text.contains('/')
            || text.text.rsplit_once('.').is_some_and(|(_, ending)| {
                (2..=4).contains(&ending.len())
                    && ending.bytes().all(|byte| byte.is_ascii_alphanumeric())
            });
        (has_japanese(&text.text) && !name_of_a_file).then_some(text)
    }
}

/// The strings of a Ruby source outside its comments. `None` for a source
/// with Ruby this does not follow (a heredoc, a `%w()` list, a command in
/// backticks, a string that never ends): such a file is left alone whole.
pub(crate) fn literals(source: &str) -> Option<Vec<Literal<'_>>> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    let mut line = 0;
    while at < bytes.len() {
        if at == line {
            if bytes[at..].starts_with(b"=begin") {
                let end = source[at..].find("\n=end").map(|end| at + end + 1)?;
                at = source[end..]
                    .find('\n')
                    .map_or(bytes.len(), |next| end + next);
                continue;
            }
            if source[at..].lines().next() == Some("__END__") {
                break;
            }
        }
        match bytes[at] {
            b'\n' => {
                at += 1;
                line = at;
            }
            b'#' => {
                at = source[at..]
                    .find('\n')
                    .map_or(bytes.len(), |next| at + next)
            }
            // `$"` and `$'` are variables, and `?"` is a character.
            b'"' | b'\'' if at > 0 && matches!(bytes[at - 1], b'$' | b'?') => at += 1,
            quote @ (b'"' | b'\'') => {
                let end = string_end(bytes, at)?;
                let rest = source[end..]
                    .find('\n')
                    .map_or(bytes.len(), |next| end + next);
                found.push(Literal {
                    body: at + 1..end - 1,
                    quote,
                    before: &source[line..at],
                    after: &source[end..rest],
                });
                if let Some(broken) = source[at..end].rfind('\n') {
                    line = at + broken + 1;
                }
                at = end;
            }
            b'`' => return None,
            b'<' if heredoc_starts(&bytes[at..]) => return None,
            b'%' if percent_literal_starts(bytes, at) => return None,
            b'/' => at = regexp_end(bytes, line, at).unwrap_or(at + 1),
            _ => at += 1,
        }
    }
    Some(found)
}

/// Where the regular expression starting at `at` ends, past its closing
/// `/`, when a `/` there starts one and it ends on its line; a quote inside
/// one starts no string. `line` is where the line starts.
fn regexp_end(bytes: &[u8], line: usize, at: usize) -> Option<usize> {
    let before = bytes[line..at].trim_ascii_end();
    let spaced = before.len() < at - line;
    let next = bytes.get(at + 1).copied()?;
    let starts = match before.last() {
        None => true,
        Some(b'(' | b',' | b'=' | b'~' | b'!' | b'|' | b'&' | b'?' | b':' | b'{' | b'[' | b';') => {
            true
        }
        Some(last) if last.is_ascii_alphanumeric() || *last == b'_' => {
            let word = before
                .rsplit(|byte: &u8| !(byte.is_ascii_alphanumeric() || *byte == b'_'))
                .next()
                .unwrap_or_default();
            [
                &b"if"[..],
                b"unless",
                b"when",
                b"while",
                b"until",
                b"and",
                b"or",
                b"not",
                b"return",
                b"then",
                b"else",
                b"elsif",
            ]
            .contains(&word)
                // `text.gsub! /x/, ""`: a method taking it without brackets.
                || spaced && !matches!(next, b' ' | b'\t' | b'=')
        }
        Some(_) => false,
    };
    if !starts {
        return None;
    }
    let mut in_class = false;
    let mut end = at + 1;
    while end < bytes.len() {
        match bytes[end] {
            b'\n' => return None,
            b'\\' => end += 1,
            b'[' => in_class = true,
            b']' => in_class = false,
            b'/' if !in_class => return Some(end + 1),
            _ => {}
        }
        end += 1;
    }
    None
}

fn heredoc_starts(bytes: &[u8]) -> bool {
    let Some(rest) = bytes.strip_prefix(b"<<") else {
        return false;
    };
    let rest = rest
        .strip_prefix(b"-")
        .or_else(|| rest.strip_prefix(b"~"))
        .unwrap_or(rest);
    rest.first()
        .is_some_and(|&next| next.is_ascii_uppercase() || matches!(next, b'_' | b'"' | b'\''))
}

fn percent_literal_starts(bytes: &[u8], at: usize) -> bool {
    let after_a_value = at > 0
        && (bytes[at - 1].is_ascii_alphanumeric() || matches!(bytes[at - 1], b'_' | b')' | b']'));
    !after_a_value
        && matches!(bytes.get(at + 1), Some(b'w' | b'W' | b'q' | b'Q' | b'r'))
        && matches!(
            bytes.get(at + 2),
            Some(b'(' | b'[' | b'{' | b'<' | b'|' | b'!' | b'/')
        )
}

/// Where the string starting at `start` ends, past its closing quote.
fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let quote = bytes[start];
    let mut at = start + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            byte if byte == quote => return Some(at + 1),
            b'#' if quote == b'"' && bytes.get(at + 1) == Some(&b'{') => {
                at = value_end(bytes, at + 2)?;
            }
            _ => at += 1,
        }
    }
    None
}

/// Where the `#{…}` whose code starts at `start` ends, past its `}`.
fn value_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1;
    let mut at = start;
    while at < bytes.len() {
        match bytes[at] {
            b'{' => {
                depth += 1;
                at += 1;
            }
            b'}' => {
                depth -= 1;
                at += 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            b'"' | b'\'' => at = string_end(bytes, at)?,
            _ => at += 1,
        }
    }
    None
}

/// A string's body as Ruby reads it. `None` for one with an escape that
/// stands for something this does not write back the same (`\t`, `\x41`),
/// with Ruby's short `#$name`, or broken over lines of a Windows file.
pub(crate) fn read_as_ruby(body: &str, quote: u8) -> Option<Text<'_>> {
    let mut text = String::with_capacity(body.len());
    let mut values = Vec::new();
    // Where in `text` a backslash stands that Ruby would drop, and where
    // the values are.
    let mut dropped = Vec::new();
    let mut value_spans = Vec::<Range<usize>>::new();
    let bytes = body.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        let character = body[at..].chars().next()?;
        match (character, bytes.get(at + 1).copied()) {
            ('\r', _) => return None,
            ('\\', Some(next)) if quote == b'\'' => {
                if !matches!(next, b'\\' | b'\'') {
                    text.push('\\');
                }
                let escaped = body[at + 1..].chars().next()?;
                text.push(escaped);
                at += 1 + escaped.len_utf8();
            }
            ('\\', Some(b'n')) => {
                text.push('\n');
                at += 2;
            }
            ('\\', Some(next))
                if next.is_ascii_digit()
                    || matches!(
                        next,
                        b'a' | b'b'
                            | b'e'
                            | b'f'
                            | b'r'
                            | b's'
                            | b't'
                            | b'v'
                            | b'x'
                            | b'u'
                            | b'c'
                            | b'C'
                            | b'M'
                            | b'\r'
                            | b'\n'
                    ) =>
            {
                return None;
            }
            ('\\', Some(next)) if next.is_ascii_alphabetic() => {
                // After a backslash that is one (`\\\H`), the letter alone
                // already reads as a code.
                if !text.ends_with('\\') {
                    dropped.push(text.len());
                    text.push('\\');
                }
                text.push(char::from(next));
                at += 2;
            }
            ('\\', Some(_)) => {
                let escaped = body[at + 1..].chars().next()?;
                text.push(escaped);
                at += 1 + escaped.len_utf8();
            }
            ('\\', None) => return None,
            ('#', Some(b'{')) if quote == b'"' => {
                let end = value_end(bytes, at + 2)?;
                values.push(&body[at..end]);
                value_spans.push(text.len()..text.len() + end - at);
                text.push_str(&body[at..end]);
                at = end;
            }
            ('#', Some(b'$' | b'@')) if quote == b'"' => return None,
            _ => {
                text.push(character);
                at += character.len_utf8();
            }
        }
    }
    // The same letter after a backslash Ruby drops and after one it keeps
    // (`\H` and `\\H`) would read alike here.
    let mut bare = Vec::new();
    let mut kept = Vec::new();
    for (at, _) in text.match_indices('\\') {
        if value_spans.iter().any(|span| span.contains(&at)) {
            continue;
        }
        let Some(letter) = text[at + 1..]
            .chars()
            .next()
            .filter(char::is_ascii_alphabetic)
        else {
            continue;
        };
        let letters = if dropped.contains(&at) {
            &mut bare
        } else {
            &mut kept
        };
        if !letters.contains(&letter) {
            letters.push(letter);
        }
    }
    if bare.iter().any(|letter| kept.contains(letter)) {
        return None;
    }
    Some(Text { text, values, bare })
}

/// English as the body of a `"…"` string that reads back as it. A `#{…}` is
/// kept as code only when it is one of `values`, the Japanese string's own;
/// any other `#` that would start code is written as the character. A
/// backslash before one of `bare`, the letters the Japanese string has after
/// a backslash Ruby drops, is written as that one backslash.
pub(crate) fn encoded(english: &str, values: &[&str], bare: &[char]) -> String {
    let mut body = String::with_capacity(english.len() + 8);
    let mut rest = english;
    while let Some(character) = rest.chars().next() {
        if let Some(value) = values.iter().find(|value| rest.starts_with(**value)) {
            body.push_str(value);
            rest = &rest[value.len()..];
            continue;
        }
        match character {
            '\\' if rest[1..]
                .chars()
                .next()
                .is_some_and(|letter| bare.contains(&letter)) =>
            {
                body.push('\\')
            }
            '\\' => body.push_str("\\\\"),
            '"' => body.push_str("\\\""),
            '\n' => body.push_str("\\n"),
            '\t' => body.push(' '),
            '#' if rest[1..].starts_with(['{', '$', '@']) => body.push_str("\\#"),
            control if control.is_control() => {}
            other => body.push(other),
        }
        rest = &rest[character.len_utf8()..];
    }
    body
}

/// The first of the characters that stand for letters: `stand_in('A')`
/// is `FIRST_STAND_IN + 'A'`.
const FIRST_STAND_IN: u32 = 0xf800;

fn stand_in(letter: char) -> char {
    char::from_u32(FIRST_STAND_IN + u32::from(letter)).unwrap_or(letter)
}

/// How long the code at the start of `text` is, after its backslash: its
/// letters and, in brackets, what it takes, which may hold codes itself
/// (`name[\v[2]]`).
fn code_length(text: &str) -> usize {
    let letters = text.bytes().take_while(u8::is_ascii_alphabetic).count();
    if !text[letters..].starts_with('[') {
        return letters;
    }
    let mut depth = 0;
    for (at, character) in text[letters..].char_indices() {
        match character {
            '[' => depth += 1,
            ']' if depth == 1 => return letters + at + 1,
            ']' => depth -= 1,
            '\n' => break,
            _ => {}
        }
    }
    letters
}

/// English for a game that draws some letters as codes (`letters`: a `w`
/// in its battle text is a wait): each such letter in a word as the
/// character that stands for it, which the launcher's script draws as the
/// letter. Codes and the values Ruby puts in stay as they are, and so do
/// the parts of a name the game's scripts read (`【data】`, `/15AE`).
pub(crate) fn with_stand_ins(
    english: &str,
    letters: &str,
    values: &[&str],
    bare: &[char],
) -> String {
    if letters.is_empty() {
        return english.to_owned();
    }
    let (marks, words, tail) = name_parts(english);
    let mut text = String::with_capacity(english.len() + 16);
    text.push_str(marks);
    let mut rest = words;
    while let Some(character) = rest.chars().next() {
        let length = if let Some(value) = values.iter().find(|value| rest.starts_with(**value)) {
            value.len()
        } else if character == '\\' {
            match rest[1..].chars().next() {
                Some(letter) if bare.contains(&letter) => 2,
                _ => 1 + code_length(&rest[1..]),
            }
        } else {
            0
        };
        if length > 0 {
            text.push_str(&rest[..length]);
            rest = &rest[length..];
            continue;
        }
        text.push(if letters.contains(character) {
            stand_in(character)
        } else {
            character
        });
        rest = &rest[character.len_utf8()..];
    }
    text.push_str(tail);
    text
}

fn mark(at: usize) -> Option<char> {
    u32::try_from(at)
        .ok()
        .and_then(|at| at.checked_add(FIRST_MARK))
        .filter(|mark| *mark <= LAST_MARK)
        .and_then(char::from_u32)
}

/// A text with each of its values as one character.
fn marked(text: &str, values: &[&str]) -> String {
    let mut marked = text.to_owned();
    for (at, value) in values.iter().enumerate() {
        if let Some(mark) = mark(at) {
            marked = marked.replace(value, mark.encode_utf8(&mut [0; 4]));
        }
    }
    marked
}

fn unmarked(text: &str, values: &[&str]) -> String {
    let mut unmarked = text.to_owned();
    for (at, value) in values.iter().enumerate() {
        if let Some(mark) = mark(at) {
            unmarked = unmarked.replace(mark, value);
        }
    }
    unmarked
}

/// The width of a line as drawn, in halves of a kanji: a code of the
/// game's message window (`\H`, `\C[2]`) draws nothing.
fn columns(line: &str) -> usize {
    let mut width = 0;
    let mut characters = line.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.peek().copied() {
                Some(letter) if letter.is_ascii_alphabetic() => {
                    while characters.next_if(char::is_ascii_alphabetic).is_some() {}
                    if characters.next_if_eq(&'[').is_some() {
                        characters.by_ref().find(|&next| next == ']');
                    }
                }
                Some(_) => {
                    characters.next();
                    width += 1;
                }
                None => {}
            },
            mark if (FIRST_MARK..=LAST_MARK).contains(&u32::from(mark)) => width += VALUE_COLUMNS,
            ' '..='~' | '\u{ff61}'..='\u{ff9f}' => width += 1,
            _ => width += 2,
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;

    const TALK: &str = concat!(
        "=begin\r\n",
        " tx[\"tx1\"] = \"説明の中\"\r\n",
        "=end\r\n",
        "class MsgWitch < MsgBase\r\n",
        " def msg_attack\r\n",
        "  tx={}\r\n",
        "  tx[\"tx1\"] = \"「…………\\\\H」\"\r\n",
        "  tx[\"tx1\"] = \"「ふふ、#{target}はここだね。\\n　後は#{myname}に任せて\\\\H」\" if $msg.t_enemy.have_ability?(\"大切な人\")\r\n",
        "  #  tx[\"tx1\"] = \"消した行\"\r\n",
        "  case $msg.at_type\r\n",
        "  when \"キス\"\r\n",
        "   tx[\"tx2\"] = \"「どう？」\" + \"TALKTEXT\"\r\n",
        "   tx[\"md\"] = \"2\" if $msg.tag == \"好意\"\r\n",
        "  end\r\n",
        "  @target = \"お兄さん\"\r\n",
        "  @myname = \"私\"\r\n",
        "  names = [\"私\", \"あたし\"]\r\n",
        "  face = \"顔/笑い\"\r\n",
        "  voice = \"声.ogg\"\r\n",
        "  tx = {\"tx1\" => 'それで\\'いい\\'の'}\r\n",
        "  ms.push(tx)\r\n",
        " end\r\n",
        "end\r\n",
    );

    fn game(files: &[(&str, &str)]) -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        for (name, source) in files {
            let path = directory.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        directory
    }

    fn sections(source: &str) -> Vec<(String, String)> {
        vec![("Scene_Title".to_owned(), source.to_owned())]
    }

    const LOADER: &str = concat!(
        "Dir.glob(\"System/talk/*/*.rb\").sort.each { |name| load(name) }\n",
        "load \"mod\\\\SCRP_Mod.rb\" if FileTest.exist?(\"Mod/SCRP_Mod.rb\")\n",
        "load \"../outside.rb\"\n",
    );

    #[test]
    fn keeps_a_letter_ruby_reads_alone_with_its_backslash() {
        let text = read_as_ruby("、\\n\\m#{name}に\\w\\q", b'"').unwrap();
        assert_eq!(text.text, "、\n\\m#{name}に\\w\\q");
        assert_eq!(text.bare, ['m', 'w', 'q']);
        assert_eq!(
            encoded("\\m#{name} \\C[2]now\\w\\q", &text.values, &text.bare),
            "\\m#{name} \\\\C[2]now\\w\\q"
        );
        // `\\H` is a backslash and `H`; so is `\\\H`, as Ruby reads it.
        let text = read_as_ruby("っ\\\\H\\\\\\H」", b'"').unwrap();
        assert_eq!(text.text, "っ\\H\\H」");
        assert!(text.bare.is_empty());
        // The same letter both ways would read alike: such a string stays.
        assert!(read_as_ruby("\\\\H と \\H", b'"').is_none());
    }

    #[test]
    fn stands_in_for_the_letters_a_game_draws_as_codes() {
        let w = '\u{f877}';
        let d = '\u{f864}';
        assert_eq!(with_stand_ins("Sword", "", &[], &[]), "Sword");
        assert_eq!(
            with_stand_ins("\\w[2]Sword of #{w}\\w wow", "w", &["#{w}"], &['w']),
            format!("\\w[2]S{w}ord of #{{w}}\\w {w}o{w}")
        );
        // What the game's scripts read in a name is not a word.
        assert_eq!(
            with_stand_ins("【data】Sword Holder/15dw", "dw", &[], &[]),
            format!("【data】S{w}or{d} Hol{d}er/15dw")
        );
    }

    #[test]
    fn finds_the_files_the_games_scripts_load() {
        let game = game(&[
            ("System/talk/01_Witch/Attack.rb", ""),
            ("System/talk/01_Witch/notes.txt", ""),
            ("System/talk/01_Witch/old/Attack.rb", ""),
            ("System/talk/Loose.rb", ""),
            ("System/Other.rb", ""),
            ("Mod/SCRP_Mod.rb", ""),
            ("dla-translation/scripts/System/talk/01_Witch/Attack.rb", ""),
        ]);
        let found = loaded(game.path(), &sections(LOADER));
        assert_eq!(
            found
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["Mod/SCRP_Mod.rb", "System/talk/01_Witch/Attack.rb"]
        );
        assert!(matches("*.rb", "A_Attack.RB"));
        assert!(matches("Base_??.rb", "base_05.rb"));
        assert!(!matches("Base_??.rb", "Base_105.rb"));
        assert!(matches("*_Witch_*", "37_Witch_03"));
        assert_eq!(pattern("C:/Games/x.rb"), None);
        assert_eq!(pattern(".rb"), None);
    }

    #[test]
    fn takes_assigned_strings_as_texts_and_the_others_as_identifiers() {
        let game = game(&[("System/talk/01_Witch/Attack.rb", TALK)]);
        let in_scripts = HashSet::from(["お兄さん".to_owned()]);
        let files = RubyFiles::read(game.path(), &sections(LOADER), &in_scripts).unwrap();
        let texts = files.texts().collect::<Vec<_>>();
        assert_eq!(
            texts
                .iter()
                .map(|text| (text.text.as_str(), text.kind))
                .collect::<Vec<_>>(),
            [
                (
                    "「ふふ、#{target}はここだね。\n　後は#{myname}に任せて\\H」",
                    TextKind::Dialogue
                ),
                ("「どう？」", TextKind::Dialogue),
                ("それで'いい'の", TextKind::Addon),
            ]
        );
        // 私 is also in a list, お兄さん in the game's scripts: both stay.
        let mut used = files.used().iter().map(String::as_str).collect::<Vec<_>>();
        used.sort_unstable();
        assert_eq!(
            used,
            [
                "あたし",
                "キス",
                "声.ogg",
                "大切な人",
                "好意",
                "私",
                "顔/笑い"
            ]
        );
        assert_eq!(files.room, DEFAULT_ROOM);
    }

    #[test]
    fn writes_copies_with_the_english_and_takes_stale_ones_away() {
        let game = game(&[
            ("System/talk/01_Witch/Attack.rb", TALK),
            (
                "System/talk/02_Imp/Attack.rb",
                "tx[\"tx1\"] = \"「まだ」\"\r\n",
            ),
            ("dla-translation/scripts/System/talk/09_Gone/Attack.rb", "x"),
        ]);
        let files = RubyFiles::read(game.path(), &sections(LOADER), &HashSet::new()).unwrap();
        let folder = game.path().join("dla-translation/scripts");
        let english = BTreeMap::from([
            (
                "「ふふ、#{target}はここだね。\n　後は#{myname}に任せて\\H」".to_owned(),
                "\"Hehe, #{target}, right here. Leave the rest to #{myname}, #{exit}, #$x 100%\\H\""
                    .to_owned(),
            ),
            ("それで'いい'の".to_owned(), "Is that 'fine'?".to_owned()),
            ("私".to_owned(), "I".to_owned()),
            ("キス".to_owned(), "Kiss".to_owned()),
        ]);
        assert_eq!(files.write(&folder, &english, "").unwrap(), 1);
        let copy = fs::read_to_string(folder.join("System/talk/01_Witch/Attack.rb")).unwrap();
        let expected = TALK
            .replace(
                "\"「ふふ、#{target}はここだね。\\n　後は#{myname}に任せて\\\\H」\"",
                concat!(
                    "\"\\\"Hehe, #{target}, right here. Leave the\\n",
                    "rest to #{myname}, \\#{exit}, \\#$x 100%\\\\H\\\"\""
                ),
            )
            .replace("'それで\\'いい\\'の'", "\"Is that 'fine'?\"");
        assert_eq!(copy, expected);
        assert!(!folder.join("System/talk/02_Imp").exists());
        assert!(!folder.join("System/talk/09_Gone").exists());

        // Nothing translated: no copies, and the folder is left empty.
        assert_eq!(files.write(&folder, &BTreeMap::new(), "").unwrap(), 0);
        assert!(!folder.join("System").exists());
    }

    #[test]
    fn reads_ruby_strings_where_they_are() {
        let source = concat!(
            "$\".delete(File.basename(__FILE__))\n",
            "#\"\n",
            "a = \"外#{list[\"中\"].size}側\" # \"注釈\"\n",
            "b = \"二行に\n",
            "わたる\"\n",
            "c = 'it''s'\n",
            "__END__\n",
            "d = \"読まれない\"\n",
        );
        let found = literals(source).unwrap();
        assert_eq!(
            found
                .iter()
                .map(|literal| &source[literal.body.clone()])
                .collect::<Vec<_>>(),
            ["外#{list[\"中\"].size}側", "二行に\nわたる", "it", "s"]
        );
        assert_eq!(found[0].before, "a = ");
        assert_eq!(found[0].after, " # \"注釈\"");
        let text = found[0].text(source).unwrap();
        assert_eq!(text.text, "外#{list[\"中\"].size}側");
        assert_eq!(text.values, ["#{list[\"中\"].size}"]);
        assert_eq!(found[1].text(source).unwrap().text, "二行に\nわたる");
        for unknown in [
            "text = <<EOS\n日本語\nEOS\n",
            "list = %w(一 二)\n",
            "name = `whoami`\n",
            "text = \"終わらない\n",
        ] {
            assert!(literals(unknown).is_none(), "{unknown}");
        }
        assert!(literals("shown = total % 100\ncount <<= 1\n").is_some());
        for odd in ["\"タブ\\t\"", "\"#$name さん\"", "\"色\\e[1m\""] {
            assert!(
                read_as_ruby(&odd[1..odd.len() - 1], b'"').is_none(),
                "{odd}"
            );
        }
    }

    #[test]
    fn breaks_the_english_into_the_lines_the_window_has() {
        assert_eq!(columns("「あ」\\H\\C[2]ab\\\\"), 9);
        assert_eq!(
            columns(&marked("#{a + 1}は", &["#{a + 1}"])),
            VALUE_COLUMNS + 2
        );
        let mut talk = String::new();
        for at in 0..40 {
            talk.push_str(&format!(
                "tx[\"tx1\"] = \"「これは十二文字の行です{at:02}\\n　二行目」\"\n"
            ));
        }
        let game = game(&[("System/talk/01_Witch/Attack.rb", &talk)]);
        let files = RubyFiles::read(game.path(), &sections(LOADER), &HashSet::new()).unwrap();
        assert_eq!(
            files.room,
            Room {
                columns: 26,
                lines: 2
            }
        );
        let english = BTreeMap::from([(
            "「これは十二文字の行です00\n　二行目」".to_owned(),
            "This is a line of twelve characters, and a second one after it.".to_owned(),
        )]);
        let copy = files.translated(&talk, &english, "").unwrap();
        let line = copy.lines().next().unwrap();
        // Two lines as the Japanese had, wider than the window: not a third.
        assert_eq!(
            line,
            "tx[\"tx1\"] = \"This is a line of twelve characters,\\nand a second one after it.\""
        );
    }
}
