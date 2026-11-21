use std::{borrow::Cow, collections::HashMap};

use dla_domain::translation::TextMarkup;
use sha2::{Digest, Sha256};

/// A text's identity in the cache and in shared packs: a hash of its exact
/// Japanese, so packs never carry the game's own script.
pub fn text_key(text: &str) -> String {
    hex::encode(&Sha256::digest(text.as_bytes())[..16])
}

pub fn has_japanese(text: &str) -> bool {
    text.chars().any(is_japanese)
}

/// Japanese left in a translation. The middle dot and the long-vowel mark are
/// punctuation in an English line, not leftovers.
const RUNAWAY_FACTOR: usize = 12;
const RUNAWAY_ALLOWANCE: usize = 80;

pub fn has_leftover_japanese(text: &str) -> bool {
    text.chars()
        .any(|character| is_japanese(character) && !matches!(character, '・' | 'ー'))
}

fn is_japanese(character: char) -> bool {
    matches!(
        character,
        '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{ff66}'..='\u{ff9f}'
    )
}

/// Whether a text uses a glossary term. A katakana term counts only where a
/// katakana word starts with it: ポリンカード uses ポリン, but メッセージ
/// does not use セージ, nor ハイヒール ヒール.
pub fn uses_term(text: &str, term: &str) -> bool {
    if !term.starts_with(is_katakana) {
        return text.contains(term);
    }
    text.match_indices(term)
        .any(|(at, _)| !text[..at].ends_with(is_katakana))
}

fn is_katakana(character: char) -> bool {
    matches!(character, '\u{30a1}'..='\u{30fa}' | 'ー' | '\u{ff66}'..='\u{ff9f}')
}

/// A game text as the translator sees it: engine codes become placeholders
/// (`{1}`, `{2}`) it keeps in place, pauses are dropped and line breaks are
/// removed, since the game wraps the English to its own window. A layout
/// code that applies to the whole rest of the text and starts or ends it (a
/// face change, an icon, a single colour) is held back and put back where it
/// was: models drop placeholders at a line's edges, and nothing in the
/// English needs it. So are the parts of a name that a game's scripts read
/// (`name_parts`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedText {
    pub line: String,
    codes: Vec<String>,
    before: String,
    after: String,
    /// The game's scripts split this name at `/`: the English adds none.
    divided: bool,
}

impl PreparedText {
    pub fn new(text: &str, markup: TextMarkup) -> Self {
        match markup {
            TextMarkup::RpgMaker => rpg_maker(text, true),
            TextMarkup::Wolf => wolf(text, true),
        }
    }

    /// The text as a batch made by this or an earlier launcher sent it: the
    /// preparation whose line is `sent` (launchers before 2026-11-03 held no
    /// codes back), or `None` when neither is.
    pub fn as_sent(text: &str, markup: TextMarkup, sent: &str) -> Option<Self> {
        let current = Self::new(text, markup);
        if current.line == sent {
            return Some(current);
        }
        let earlier = match markup {
            TextMarkup::RpgMaker => rpg_maker(text, false),
            TextMarkup::Wolf => wolf(text, false),
        };
        (earlier.line == sent).then_some(earlier)
    }

    /// Whether a reply can stand as this text's translation: every
    /// placeholder kept, the same numbers (as digits or words), no Japanese
    /// left, and no code of its own making. A translator that has seen
    /// codes writes some where there are none (`\_`, or the `_` of one):
    /// a backslash or an underscore the line does not have is one.
    pub fn accepts(&self, reply: &str) -> bool {
        !self.runs_away(reply)
            && placeholders(reply) == placeholders(&self.line)
            && keeps_numbers(&self.line, reply)
            && !has_leftover_japanese(reply)
            && ['\\', '_']
                .iter()
                .all(|sign| reply.matches(*sign).count() <= self.line.matches(*sign).count())
    }

    /// Whether a reply is a translator that went on repeating itself
    /// (3,195 letters of `Nnnn…` for a cry of nine): English runs to a few
    /// times the letters of its Japanese, and to more only for a word or
    /// two. Such a reply is never kept, not even as an imperfect one.
    pub fn runs_away(&self, reply: &str) -> bool {
        reply.chars().count() > self.line.chars().count() * RUNAWAY_FACTOR + RUNAWAY_ALLOWANCE
    }

    /// The reply a translator should give for this text when `english` is
    /// its translation (a line of an official English release): its lines
    /// joined, and its codes as this text's placeholders. `None` when the
    /// English has other codes or the reply would not be accepted.
    pub fn reply_for(&self, english: &str, markup: TextMarkup) -> Option<String> {
        let joined = english
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let theirs = match markup {
            TextMarkup::RpgMaker => rpg_maker(&joined, true),
            TextMarkup::Wolf => wolf(&joined, true),
        };
        if theirs.before != self.before || theirs.after != self.after {
            return None;
        }
        // Each of the English's codes takes the number of the same code in
        // the Japanese, so a name may move within the line.
        let mut used = vec![false; self.codes.len()];
        let mut numbers = Vec::with_capacity(theirs.codes.len());
        for code in &theirs.codes {
            let at = (0..self.codes.len()).find(|&at| !used[at] && self.codes[at] == *code)?;
            used[at] = true;
            numbers.push(at + 1);
        }
        if used.contains(&false) {
            return None;
        }
        let mut reply = String::with_capacity(theirs.line.len());
        let mut rest = theirs.line.as_str();
        while let Some(start) = rest.find('{') {
            reply.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match placeholder_at(after) {
                Some((number, length)) => {
                    reply.push_str(&format!("{{{}}}", numbers[number - 1]));
                    rest = &after[length + 1..];
                }
                None => {
                    reply.push('{');
                    rest = after;
                }
            }
        }
        reply.push_str(rest);
        let reply = reply.trim().to_owned();
        self.accepts(&reply).then_some(reply)
    }

    pub fn finish(&self, reply: &str) -> String {
        let reply = if self.divided {
            Cow::Owned(reply.replace('/', "-"))
        } else {
            Cow::Borrowed(reply)
        };
        let mut finished = String::with_capacity(reply.len());
        let mut rest = reply.as_ref();
        while let Some(start) = rest.find('{') {
            finished.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match placeholder_at(after) {
                Some((number, length)) => {
                    if let Some(code) = number.checked_sub(1).and_then(|at| self.codes.get(at)) {
                        finished.push_str(code);
                    }
                    rest = &after[length + 1..];
                }
                None => {
                    finished.push('{');
                    rest = after;
                }
            }
        }
        finished.push_str(rest);
        let tidy = finished.replace('ー', "-");
        let english = collapse_dots(&tidy)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        format!("{}{english}{}", self.before, self.after)
    }
}

enum Piece {
    Text(String),
    Code(String),
}

/// Codes that change how text looks, not what it says: VX Ace's colours,
/// icons and text size, MZ's font size and position, and the face change
/// games add (`\F[3]`).
const LAYOUT_CODES: [&str; 9] = ["C", "F", "FS", "FB", "FI", "I", "PX", "PY", "size"];

fn code_kind(code: &str) -> String {
    let Some(rest) = code.strip_prefix('\\') else {
        return "format".to_owned();
    };
    let letters = rest
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect::<String>();
    match (letters.is_empty(), rest.chars().next()) {
        (false, _) => letters.to_ascii_uppercase(),
        (true, Some('{' | '}')) => "size".to_owned(),
        (true, Some(other)) => other.to_string(),
        (true, None) => String::new(),
    }
}

fn rpg_maker(text: &str, hold_back: bool) -> PreparedText {
    let (mark, text, tail) = if hold_back {
        name_parts(text)
    } else {
        ("", text, "")
    };
    let mut pieces = Vec::new();
    let mut line = String::new();
    // VX Ace's battle messages are format strings: `%s` and `%d` become
    // numbered (`%1$s`), so the English may put them in another order.
    let mut formats = 0;
    let mut add_code = |line: &mut String, code: String| {
        if !line.is_empty() {
            pieces.push(Piece::Text(std::mem::take(line)));
        }
        pieces.push(Piece::Code(code));
    };
    let breaks = text.matches('\n').count();
    for (index, text_line) in text.split('\n').enumerate() {
        let text_line = if index == 0 {
            text_line
        } else {
            text_line.trim_start_matches(|c: char| c.is_whitespace())
        };
        let mut characters = text_line.char_indices().peekable();
        while let Some((at, character)) = characters.next() {
            match (character, characters.peek().map(|&(_, next)| next)) {
                ('\\', Some(next)) if ".|!^><".contains(next) => {
                    characters.next();
                }
                // `\_` before a line break takes the break out, in the
                // message script many XP games use (XRXS). The English is
                // wrapped anew, and the script draws a `\_` left without
                // its break.
                ('\\', Some('_')) if index < breaks && &text_line[at..] == "\\_" => {
                    characters.next();
                }
                ('\\', Some(next)) if "{}$\\".contains(next) => {
                    characters.next();
                    add_code(&mut line, format!("\\{next}"));
                }
                ('\\', Some(next)) if next.is_ascii_alphabetic() => {
                    let mut end = at + 1;
                    while let Some(&(position, letter)) = characters.peek() {
                        if !letter.is_ascii_alphabetic() {
                            break;
                        }
                        end = position + letter.len_utf8();
                        characters.next();
                    }
                    if let Some(&(position, '[')) = characters.peek()
                        && let Some(close) = closing_bracket(&text_line[position..])
                    {
                        end = position + close + 1;
                        while characters.peek().is_some_and(|&(next, _)| next < end) {
                            characters.next();
                        }
                    }
                    add_code(&mut line, text_line[at..end].to_owned());
                }
                // A value a game's Ruby puts into the line (`#{target}`), in
                // texts read from Ruby files.
                ('#', Some('{')) => match ruby_value_length(&text_line[at..]) {
                    Some(length) => {
                        let end = at + length;
                        while characters.peek().is_some_and(|&(next, _)| next < end) {
                            characters.next();
                        }
                        add_code(&mut line, text_line[at..end].to_owned());
                    }
                    None => line.push(character),
                },
                ('%', Some(kind @ ('s' | 'd'))) => {
                    characters.next();
                    formats += 1;
                    add_code(&mut line, format!("%{formats}${kind}"));
                }
                ('%', Some('%')) => {
                    characters.next();
                    add_code(&mut line, "%%".to_owned());
                }
                ('%', Some(digit)) if digit.is_ascii_digit() => {
                    characters.next();
                    add_code(&mut line, format!("%{digit}"));
                }
                _ => line.push(character),
            }
        }
    }
    if !line.is_empty() {
        pieces.push(Piece::Text(line));
    }
    let mut prepared = held_back(pieces, hold_back, &LAYOUT_CODES, code_kind);
    prepared.before.insert_str(0, mark);
    prepared.after.push_str(tail);
    prepared.divided = !tail.is_empty() && !text.contains('/');
    prepared
}

/// The parts of a one-line name that a game's scripts read and a player
/// does not: marks before it (`【data】`, tested with `include?`) and
/// numbers after it (`サキュバス/10/5`, split at `/`). Returns the marks,
/// the name and the numbers. A translator that drops a mark or adds a `/`
/// changes what the scripts read, so it sees the name only.
pub fn name_parts(text: &str) -> (&str, &str, &str) {
    let whole = ("", text, "");
    if text.contains(['\n', '\r']) {
        return whole;
    }
    let mut head = text;
    while let Some((before, part)) = head.rsplit_once('/') {
        let number = part.bytes().any(|byte| byte.is_ascii_digit())
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".+-".contains(&byte));
        if !number {
            break;
        }
        head = before;
    }
    let mut tail = &text[head.len()..];
    // `1/2` is a fraction, not a name with a number after it.
    let fraction = head.ends_with(|last: char| last.is_ascii_digit())
        && tail.bytes().skip(1).all(|byte| byte.is_ascii_digit());
    if fraction {
        (head, tail) = (text, "");
    }
    let mut marks = 0;
    while let Some(rest) = head[marks..].strip_prefix('【')
        && let Some(end) = rest.find('】')
        && end > 0
        && rest[..end]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        marks += '【'.len_utf8() + end + '】'.len_utf8();
    }
    if marks == head.len() {
        return whole;
    }
    (&head[..marks], &head[marks..], tail)
}

/// Where the `]` is that closes the `[` `text` starts with. What a code
/// takes may be a code itself (`\name[\v[2]]`): cut at the first `]`, the
/// second one would be left in the line as text.
fn closing_bracket(text: &str) -> Option<usize> {
    let mut depth = 0;
    for (at, character) in text.char_indices() {
        match character {
            '[' => depth += 1,
            ']' if depth == 1 => return Some(at),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// How long the `#{…}` at the start of `text` is, to the `}` that closes it.
fn ruby_value_length(text: &str) -> Option<usize> {
    let mut depth = 0;
    for (at, character) in text.char_indices().skip(1) {
        match character {
            '{' => depth += 1,
            '}' if depth == 1 => return Some(at + 1),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// WOLF RPG Editor's codes that change how text looks or is shown, not
/// what it says: colour, size, font, edge, spacing, position, drawing at
/// once (`\>`), closing by itself (`\^`), and a symbol drawn in a font of
/// its own.
const WOLF_LAYOUT_CODES: [&str; 18] = [
    "C",
    "F",
    "FONT",
    "E",
    "M",
    "SP",
    "SPACE",
    "AX",
    "AY",
    "MX",
    "MY",
    "A",
    ">",
    "<",
    "^",
    "-",
    WOLF_ALIGNMENT,
    WOLF_SYMBOL,
];
const WOLF_ALIGNMENT: &str = "alignment";
const WOLF_SYMBOL: &str = "symbol";

/// How long the WOLF RPG Editor code at the start of `text` is, if one
/// starts it: a backslash with letters or one sign and, in brackets, what
/// it takes, which may hold codes itself (`\f[\cself[19]]`); or a line's
/// alignment (`<C>`).
pub fn wolf_code_length(text: &str) -> Option<usize> {
    if ["<C>", "<R>", "<L>"]
        .iter()
        .any(|alignment| text.starts_with(alignment))
    {
        return Some(3);
    }
    let rest = text.strip_prefix('\\')?;
    let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    let name = if letters > 0 {
        letters
    } else {
        rest.chars().next()?.len_utf8()
    };
    let mut length = 1 + name;
    if text[length..].starts_with('[') {
        let mut depth = 0;
        for (at, character) in text[length..].char_indices() {
            match character {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        length += at + 1;
                        break;
                    }
                }
                '\n' => break,
                _ => {}
            }
        }
    }
    Some(length)
}

fn wolf_code_kind(code: &str) -> String {
    let mut rest = code;
    while let Some(length) = wolf_code_length(rest) {
        rest = &rest[length..];
    }
    if !rest.is_empty() {
        WOLF_SYMBOL.to_owned()
    } else if code.starts_with('<') {
        WOLF_ALIGNMENT.to_owned()
    } else {
        code_kind(code)
    }
}

/// A sign games draw between codes that pick its font and size (a heart
/// from a symbol font): neither a letter, a digit, a space nor Japanese.
fn is_symbol(text: &str) -> bool {
    let mut characters = text.chars();
    matches!(
        (characters.next(), characters.next()),
        (Some(sign), None) if !sign.is_alphanumeric() && !sign.is_whitespace() && !is_japanese(sign)
    )
}

fn wolf(text: &str, hold_back: bool) -> PreparedText {
    let mut pieces = Vec::<Piece>::new();
    let add_text = |pieces: &mut Vec<Piece>, text: &str| match pieces.last_mut() {
        Some(Piece::Text(last)) => last.push_str(text),
        _ => pieces.push(Piece::Text(text.to_owned())),
    };
    for (index, line) in text.split('\n').enumerate() {
        let line = line.trim_end_matches('\r');
        let mut rest = if index == 0 {
            line
        } else {
            line.trim_start_matches(|c: char| c.is_whitespace())
        };
        while let Some(character) = rest.chars().next() {
            let Some(length) = wolf_code_length(rest) else {
                add_text(&mut pieces, &rest[..character.len_utf8()]);
                rest = &rest[character.len_utf8()..];
                continue;
            };
            let code = &rest[..length];
            rest = &rest[length..];
            match code {
                // Waits; the English has other places to breathe.
                "\\." | "\\!" | "\\|" => {}
                // A reading over a word: the word itself is the text.
                _ if code.starts_with("\\r[") && code.ends_with(']') => {
                    let word = code[3..code.len() - 1]
                        .split(',')
                        .next()
                        .unwrap_or_default();
                    add_text(&mut pieces, word);
                }
                _ => pieces.push(Piece::Code(code.to_owned())),
            }
        }
    }
    // A symbol and the codes around it are one code to keep together.
    let mut joined = Vec::<Piece>::with_capacity(pieces.len());
    let mut at = 0;
    while at < pieces.len() {
        let run = pieces[at..]
            .iter()
            .take_while(|piece| match piece {
                Piece::Code(_) => true,
                Piece::Text(text) => is_symbol(text),
            })
            .count();
        let run = &pieces[at..at + run];
        let has_symbol = run.iter().any(|piece| matches!(piece, Piece::Text(_)));
        let code_before = matches!(run.first(), Some(Piece::Code(_)));
        let code_after = matches!(run.last(), Some(Piece::Code(_)));
        if has_symbol && code_before && code_after {
            joined.push(Piece::Code(
                run.iter()
                    .map(|piece| match piece {
                        Piece::Code(text) | Piece::Text(text) => text.as_str(),
                    })
                    .collect(),
            ));
            at += run.len();
        } else {
            joined.push(match &pieces[at] {
                Piece::Code(code) => Piece::Code(code.clone()),
                Piece::Text(text) => Piece::Text(text.clone()),
            });
            at += 1;
        }
    }
    // Text pieces a code-only run split apart are joined again.
    let mut pieces = Vec::<Piece>::with_capacity(joined.len());
    for piece in joined {
        match (pieces.last_mut(), piece) {
            (Some(Piece::Text(last)), Piece::Text(text)) => last.push_str(&text),
            (_, piece) => pieces.push(piece),
        }
    }
    held_back(pieces, hold_back, &WOLF_LAYOUT_CODES, wolf_code_kind)
}

/// Holds back the layout codes at the text's start and end whose kind
/// appears only once (so they apply to everything after them), and numbers
/// the other codes in order.
fn held_back(
    pieces: Vec<Piece>,
    hold_back: bool,
    layout: &[&str],
    code_kind: fn(&str) -> String,
) -> PreparedText {
    let mut kinds = HashMap::<String, usize>::new();
    for piece in &pieces {
        if let Piece::Code(code) = piece {
            *kinds.entry(code_kind(code)).or_default() += 1;
        }
    }
    let movable = |piece: &Piece| match piece {
        Piece::Code(_) if !hold_back => false,
        Piece::Code(code) => {
            let kind = code_kind(code);
            layout.contains(&kind.as_str()) && kinds.get(&kind) == Some(&1)
        }
        Piece::Text(_) => false,
    };
    let start = pieces.iter().take_while(|piece| movable(piece)).count();
    let end = pieces.len()
        - pieces[start..]
            .iter()
            .rev()
            .take_while(|piece| movable(piece))
            .count();
    let joined = |pieces: &[Piece]| {
        pieces
            .iter()
            .map(|piece| match piece {
                Piece::Code(code) | Piece::Text(code) => code.as_str(),
            })
            .collect::<String>()
    };
    let mut line = String::new();
    let mut codes = Vec::new();
    for piece in &pieces[start..end] {
        match piece {
            Piece::Text(text) => line.push_str(text),
            Piece::Code(code) => {
                codes.push(code.clone());
                line.push_str(&format!("{{{}}}", codes.len()));
            }
        }
    }
    PreparedText {
        line,
        codes,
        before: joined(&pieces[..start]),
        after: joined(&pieces[end..]),
        divided: false,
    }
}

fn placeholder_at(after_brace: &str) -> Option<(usize, usize)> {
    let digits = after_brace.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0 && after_brace.as_bytes().get(digits) == Some(&b'}'))
        .then(|| {
            after_brace[..digits]
                .parse()
                .ok()
                .map(|number| (number, digits))
        })
        .flatten()
}

fn placeholders(text: &str) -> Vec<usize> {
    let mut found = text
        .match_indices('{')
        .filter_map(|(at, _)| placeholder_at(&text[at + 1..]).map(|(number, _)| number))
        .collect::<Vec<_>>();
    found.sort_unstable();
    found.dedup();
    found
}

fn numbers(text: &str) -> Vec<String> {
    let mut without_placeholders = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        without_placeholders.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        rest = match placeholder_at(after) {
            Some((_, length)) => &after[length + 1..],
            None => after,
        };
    }
    without_placeholders.push_str(rest);
    let mut found = Vec::new();
    let mut current = String::new();
    for character in without_placeholders.chars() {
        match ascii_digit(character) {
            Some(digit) => current.push(digit),
            None if !current.is_empty() => found.push(std::mem::take(&mut current)),
            None => {}
        }
    }
    if !current.is_empty() {
        found.push(current);
    }
    found.sort_unstable();
    found
}

/// Whether the reply keeps the line's numbers, as digits or as English
/// words ("three days" for `３日`), and adds none of its own. Each word
/// stands for one number.
fn keeps_numbers(line: &str, reply: &str) -> bool {
    let mut missing = numbers(line);
    for number in numbers(reply) {
        match missing.iter().position(|kept| *kept == number) {
            Some(at) => {
                missing.remove(at);
            }
            None => return false,
        }
    }
    let mut words = reply
        .split(|character: char| !character.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .map(|word| Some(word.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    missing.iter().all(|number| {
        let spellings = number.parse().map_or(&[][..], number_words);
        match words.iter_mut().find(|word| {
            word.as_deref()
                .is_some_and(|word| spellings.contains(&word))
        }) {
            Some(word) => {
                *word = None;
                true
            }
            None => false,
        }
    })
}

fn number_words(number: u32) -> &'static [&'static str] {
    match number {
        0 => &["zero"],
        1 => &["one", "once", "single", "first"],
        2 => &["two", "twice", "second"],
        3 => &["three", "third"],
        4 => &["four", "fourth"],
        5 => &["five", "fifth"],
        6 => &["six", "sixth"],
        7 => &["seven", "seventh"],
        8 => &["eight", "eighth"],
        9 => &["nine", "ninth"],
        10 => &["ten", "tenth"],
        11 => &["eleven", "eleventh"],
        12 => &["twelve", "twelfth", "dozen"],
        13 => &["thirteen", "thirteenth"],
        14 => &["fourteen", "fourteenth"],
        15 => &["fifteen", "fifteenth"],
        16 => &["sixteen", "sixteenth"],
        17 => &["seventeen", "seventeenth"],
        18 => &["eighteen", "eighteenth"],
        19 => &["nineteen", "nineteenth"],
        20 => &["twenty", "twentieth"],
        100 => &["hundred"],
        1000 => &["thousand"],
        _ => &[],
    }
}

fn ascii_digit(character: char) -> Option<char> {
    match character {
        '0'..='9' => Some(character),
        '０'..='９' => char::from_u32(u32::from(character) - u32::from('０') + u32::from('0')),
        _ => None,
    }
}

fn collapse_dots(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len());
    let mut in_run = false;
    for character in text.chars() {
        if character == '・' {
            if !in_run {
                collapsed.push_str("...");
            }
            in_run = true;
        } else {
            collapsed.push(character);
            in_run = false;
        }
    }
    collapsed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(text: &str) -> PreparedText {
        PreparedText::new(text, TextMarkup::RpgMaker)
    }

    #[test]
    fn swaps_codes_for_placeholders_and_drops_pauses_and_line_breaks() {
        let text =
            prepared("そういえば名乗ってなかったわね\\!\n\u{3000}特に\\C[2]妖魔\\c[0]を狩る");
        assert_eq!(
            text.line,
            "そういえば名乗ってなかったわね特に{1}妖魔{2}を狩る"
        );
        assert_eq!(
            text.finish("By the way, I hunt {1}demons{2} in particular."),
            "By the way, I hunt \\C[2]demons\\c[0] in particular."
        );
    }

    #[test]
    fn keeps_other_plugins_codes_and_template_slots() {
        let text = prepared("\\c[2]\\name\\c[0] ×\\count を手に入れた！");
        assert_eq!(text.line, "{1}{2}{3} ×{4} を手に入れた！");
        assert_eq!(
            text.finish("You've obtained {1}{2}{3} ×{4}!"),
            "You've obtained \\c[2]\\name\\c[0] ×\\count!"
        );
        assert_eq!(
            prepared("%1は%2を手に入れた！").line,
            "{1}は{2}を手に入れた！"
        );
        assert_eq!(prepared("\\{大声\\}").line, "{1}大声{2}");
    }

    #[test]
    fn keeps_the_values_ruby_puts_into_a_line() {
        let text = prepared("「ふふ、#{target}はここだね。\n　後は#{names[\"自分\"]}に任せて\\H」");
        assert_eq!(text.line, "「ふふ、{1}はここだね。後は{2}に任せて{3}」");
        assert_eq!(
            text.finish("\"Hehe, {1}, right here. Leave the rest to {2}{3}\""),
            "\"Hehe, #{target}, right here. Leave the rest to #{names[\"自分\"]}\\H\""
        );
        assert!(!text.accepts("\"Hehe, right here. Leave the rest to {2}{3}\""));
        assert_eq!(prepared("残り #{ 個").line, "残り #{ 個");
    }

    #[test]
    fn takes_a_code_with_a_code_in_its_brackets_whole() {
        let text = prepared("\\name[\\v[2]]\\_\nルーンの材料を探してる。");
        assert_eq!(text.line, "{1}ルーンの材料を探してる。");
        assert_eq!(
            text.finish("{1}I'm looking for rune materials."),
            "\\name[\\v[2]]I'm looking for rune materials."
        );
        // A bracket never closed is no code's.
        assert_eq!(
            prepared("\\name[\\v[2]こんにちは").line,
            "{1}[{2}こんにちは"
        );
    }

    #[test]
    fn refuses_a_reply_with_a_code_of_its_own_making() {
        let text = prepared("\\name[アンジュ]ホームポート用マニュアル");
        assert_eq!(text.line, "{1}ホームポート用マニュアル");
        assert!(text.accepts("{1}Homeport Manual"));
        assert!(!text.accepts("{1}_Homeport Manual"));
        assert!(!text.accepts("{1}\\_Homeport Manual"));
        // What the line itself has may stay.
        assert!(prepared("save_data を読む").accepts("Read save_data"));
        let cry = prepared("んおおおぉぉっ♥♥");
        assert!(cry.accepts("Nnooooohh♥♥"));
        assert!(cry.runs_away(&"n".repeat(3195)) && !cry.accepts(&"n".repeat(3195)));
        assert!(!prepared("雷").runs_away("A bolt of lightning from a clear sky"));
    }

    #[test]
    fn drops_the_code_that_takes_a_line_break_out() {
        let text = prepared("\\name[アンジュ]\\_\nルーンを\\_\nあげよう。\\_");
        assert_eq!(text.line, "{1}ルーンをあげよう。\\_");
        assert_eq!(
            text.finish("{1}I'll give you a rune.\\_"),
            "\\name[アンジュ]I'll give you a rune.\\_"
        );
        assert_eq!(prepared("a\\_b\nc").line, "a\\_bc");
    }

    #[test]
    fn holds_back_the_parts_of_a_name_that_scripts_read() {
        let enemy = prepared("【data】尻尾持ち/02/1");
        assert_eq!(enemy.line, "尻尾持ち");
        assert_eq!(enemy.finish("Tail Holder"), "【data】Tail Holder/02/1");

        let map = prepared("外地A 出入り口/15AE");
        assert_eq!(map.line, "外地A 出入り口");
        assert_eq!(
            map.finish("Barren Land A Entrance/Exit"),
            "Barren Land A Entrance-Exit/15AE"
        );
        assert_eq!(prepared("獄界C 小部屋2/10C/0").line, "獄界C 小部屋2");
        assert_eq!(prepared("調整用/-5/-5").line, "調整用");

        let marked = prepared("【data】人間/男");
        assert_eq!(marked.line, "人間/男");
        assert_eq!(marked.finish("Human/Male"), "【data】Human/Male");
        for text in ["残り1/2", "【注意】触れるな", "はい/いいえ"] {
            assert_eq!(prepared(text).line, text);
        }
        assert_eq!(prepared("一行目/12\n二行目").line, "一行目/12二行目");
        assert_eq!(
            prepared("魔法使い/12").reply_for("Mage/12", TextMarkup::RpgMaker),
            Some("Mage".to_owned())
        );
    }

    #[test]
    fn numbers_vx_ace_format_codes_so_the_english_may_reorder_them() {
        let text = prepared("%sは %s のダメージを受けた！（100%%）");
        assert_eq!(text.line, "{1}は {2} のダメージを受けた！（100{3}）");
        assert_eq!(
            text.finish("{1} took {2} damage! (100{3})"),
            "%1$s took %2$s damage! (100%%)"
        );
        assert!(text.accepts("{2} damage to {1}! (100{3})"));
        assert_eq!(
            text.finish("{2} damage to {1}! (100{3})"),
            "%2$s damage to %1$s! (100%%)"
        );
    }

    #[test]
    fn accepts_only_replies_that_keep_placeholders_and_numbers() {
        let text = prepared("\\C[1]スポーツ\\C[0]の本（100）");
        assert!(text.accepts("{1}Sports{2} book (100)"));
        assert!(!text.accepts("{1}Sports{2} book (10及)"));
        assert!(!text.accepts("Sports book (100)"));
        assert!(!text.accepts("{1}Sports{2} book (10)"));
        assert!(prepared("全角の１００円").accepts("100 yen"));
    }

    #[test]
    fn accepts_numbers_written_as_words() {
        let days = prepared("３日後に戻る");
        assert!(days.accepts("I'll be back in three days."));
        assert!(days.accepts("I'll be back in 3 days."));
        assert!(!days.accepts("I'll be back in a few days."));
        assert!(!days.accepts("I'll be back in four days."));
        assert!(prepared("1回だけ").accepts("Only once."));
        assert!(prepared("100年前").accepts("A hundred years ago."));
        // Each word stands for one number, and the reply adds none.
        let pair = prepared("1人と1匹");
        assert!(pair.accepts("One person and one dog."));
        assert!(!pair.accepts("One person and a dog."));
        assert!(!prepared("本を読む").accepts("Read two books, 2 times."));
    }

    #[test]
    fn finds_a_katakana_term_only_where_a_word_starts_with_it() {
        assert!(uses_term("ポリンカード", "ポリン"));
        assert!(uses_term("「セージ」と話す", "セージ"));
        assert!(uses_term("ﾎﾟﾘﾝ", "ﾎﾟﾘﾝ"));
        assert!(!uses_term("メッセージ", "セージ"));
        assert!(!uses_term("ハイヒール", "ヒール"));
        assert!(uses_term("ハイヒールとヒール", "ヒール"));
        assert!(uses_term("盗蟲カード", "盗蟲"));
    }

    #[test]
    fn holds_back_layout_codes_that_start_or_end_the_whole_text() {
        // A face change before the line: the model need not keep it.
        let face = prepared("\\F[3]「ありがとう」\\.");
        assert_eq!(face.line, "「ありがとう」");
        assert!(face.accepts("\"Thank you\""));
        assert_eq!(face.finish("\"Thank you\""), "\\F[3]\"Thank you\"");
        let icon = prepared("\\I[5]薬草\\C[1]");
        assert_eq!(icon.line, "薬草");
        assert_eq!(icon.finish("Herb"), "\\I[5]Herb\\C[1]");
        // A code in the middle, a name, or one of a pair stays a placeholder.
        assert_eq!(prepared("あ\\F[2]い").line, "あ{1}い");
        assert_eq!(prepared("\\N[1]は走った").line, "{1}は走った");
        assert_eq!(prepared("\\C[2]妖魔\\C[0]を狩る").line, "{1}妖魔{2}を狩る");
        assert_eq!(prepared("\\{大声\\}").line, "{1}大声{2}");
    }

    #[test]
    fn reads_a_reply_to_a_batch_an_earlier_launcher_sent() {
        let earlier = PreparedText::as_sent("\\F[3]はい", TextMarkup::RpgMaker, "{1}はい").unwrap();
        assert!(earlier.accepts("{1}Yes"));
        assert!(!earlier.accepts("Yes"));
        assert_eq!(earlier.finish("{1}Yes"), "\\F[3]Yes");
        let current = PreparedText::as_sent("\\F[3]はい", TextMarkup::RpgMaker, "はい").unwrap();
        assert_eq!(current.finish("Yes"), "\\F[3]Yes");
        assert_eq!(
            PreparedText::as_sent("はい", TextMarkup::RpgMaker, "いいえ"),
            None
        );
    }

    #[test]
    fn treats_japanese_dots_as_punctuation_not_leftovers() {
        let text = prepared("うーん・・・");
        assert!(text.accepts("Hmm・・・"));
        assert_eq!(text.finish("Hmm・・・ well"), "Hmm... well");
        assert!(!text.accepts("Hmm うーん"));
    }

    #[test]
    fn finish_drops_unknown_placeholders_and_extra_spaces() {
        let text = prepared("\\C[2]赤\\c[0]");
        assert_eq!(text.finish("  {1}Red  {2} {9} "), "\\C[2]Red \\c[0]");
        assert_eq!(text.finish("braces {stay} {1}"), "braces {stay} \\C[2]");
    }

    #[test]
    fn reads_wolf_rpg_editors_codes() {
        let prepare = |text: &str| PreparedText::new(text, TextMarkup::Wolf);
        // A colour inside the line is a placeholder; a pause is dropped;
        // lines are joined.
        let text = prepare("\\c[2]しっかり\\c[0]と…\\.\n　洗うんだよ♪");
        assert_eq!(text.line, "{1}しっかり{2}と…洗うんだよ♪");
        assert_eq!(
            text.finish("{1}Properly{2}... wash it♪"),
            "\\c[2]Properly\\c[0]... wash it♪"
        );
        // Codes inside a code's brackets belong to it, and what the whole
        // text starts and ends with is held back.
        let text = prepare("\\>\\f[\\cself[19]]所持数\\ax[\\cself[13]]は\\cself[97]個\\^");
        assert_eq!(text.line, "所持数{1}は{2}個");
        assert_eq!(
            text.finish("Held{1}: {2}"),
            "\\>\\f[\\cself[19]]Held\\ax[\\cself[13]]: \\cself[97]\\^"
        );
        // A symbol drawn in its own font stays one piece with its codes.
        let text = prepare("そりゃ大変だぁ\\f[11]\\font[2]㌢\\font[0]\\f[12]");
        assert_eq!(text.line, "そりゃ大変だぁ");
        assert_eq!(
            text.finish("Oh, that's rough."),
            "Oh, that's rough.\\f[11]\\font[2]㌢\\font[0]\\f[12]"
        );
        let text = prepare("いいわ\\font[2]㌢\\font[0]もっと…\\font[2]㌢\\font[0]");
        assert_eq!(text.line, "いいわ{1}もっと…{2}");
        // A reading over a word leaves the word; a centred line, a database
        // value and a string variable are placeholders.
        let text = prepare("<C>\\r[魔育,まいく]日誌は\\cdb[21:78:0]と\\s[7]");
        assert_eq!(text.line, "魔育日誌は{1}と{2}");
        assert!(text.accepts("The Maiku diary is {1} and {2}"));
        assert!(!text.accepts("The Maiku diary is {1}"));
        assert_eq!(
            text.finish("The Maiku diary is {1} and {2}"),
            "<C>The Maiku diary is \\cdb[21:78:0] and \\s[7]"
        );
        assert_eq!(wolf_code_length("\\-[1]x"), Some(5));
        assert_eq!(wolf_code_length("\\\\n"), Some(2));
        assert_eq!(wolf_code_length("<<PAGE>>"), None);
        assert_eq!(wolf_code_length("text"), None);
    }

    #[test]
    fn keys_are_stable_hashes_of_the_exact_text() {
        assert_eq!(text_key("はい").len(), 32);
        assert_eq!(text_key("はい"), text_key("はい"));
        assert_ne!(text_key("はい"), text_key("はい "));
    }

    #[test]
    fn recognises_japanese_text() {
        assert!(has_japanese("ニューゲーム"));
        assert!(has_japanese("Lv 漢字"));
        assert!(!has_japanese("New Game"));
    }

    #[test]
    fn gives_the_reply_an_official_translation_stands_for() {
        let markup = TextMarkup::RpgMaker;
        let given = PreparedText::new("\\C[2]剣\\C[0]を\\N[1]にあげる", markup);
        assert_eq!(given.line, "{1}剣{2}を{3}にあげる");
        assert_eq!(
            given
                .reply_for("Give the \\C[2]sword\\C[0]\nto \\N[1].", markup)
                .as_deref(),
            Some("Give the {1}sword{2} to {3}.")
        );
        let names = PreparedText::new("\\N[1]と\\N[2]の家", markup);
        assert_eq!(
            names
                .reply_for("The house of \\N[2] and \\N[1]", markup)
                .as_deref(),
            Some("The house of {2} and {1}")
        );
        assert_eq!(names.reply_for("The house of \\N[1]", markup), None);
        assert_eq!(given.reply_for("Give the sword to \\N[2].", markup), None);
    }
}
