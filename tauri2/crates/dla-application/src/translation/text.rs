use std::collections::HashMap;

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
/// English needs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedText {
    pub line: String,
    codes: Vec<String>,
    before: String,
    after: String,
}

impl PreparedText {
    pub fn new(text: &str, markup: TextMarkup) -> Self {
        match markup {
            TextMarkup::RpgMaker => rpg_maker(text, true),
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
        };
        (earlier.line == sent).then_some(earlier)
    }

    /// Whether a reply can stand as this text's translation: every
    /// placeholder kept, the same numbers (as digits or words), no Japanese
    /// left.
    pub fn accepts(&self, reply: &str) -> bool {
        placeholders(reply) == placeholders(&self.line)
            && keeps_numbers(&self.line, reply)
            && !has_leftover_japanese(reply)
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
        let mut finished = String::with_capacity(reply.len());
        let mut rest = reply;
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
                        && let Some(close) = text_line[position..].find(']')
                    {
                        end = position + close + 1;
                        while characters.peek().is_some_and(|&(next, _)| next < end) {
                            characters.next();
                        }
                    }
                    add_code(&mut line, text_line[at..end].to_owned());
                }
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
    held_back(pieces, hold_back)
}

/// Holds back the layout codes at the text's start and end whose kind
/// appears only once (so they apply to everything after them), and numbers
/// the other codes in order.
fn held_back(pieces: Vec<Piece>, hold_back: bool) -> PreparedText {
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
            LAYOUT_CODES.contains(&kind.as_str()) && kinds.get(&kind) == Some(&1)
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
