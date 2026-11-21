//! Fitting English into WOLF RPG Editor's windows. The engine draws a line
//! as it is written and never wraps it, so the launcher breaks the English
//! itself: a message at the width the game's own lines reach, over as many
//! messages as its lines need, and any other text over the lines its
//! Japanese had. Widths count in halves of a kanji: the fonts the games use
//! draw a Latin letter half as wide.

use dla_application::translation::wolf_code_length as code_length;

use crate::wrap::all_but_the_largest;

/// The room a message has: columns of half a kanji, and lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Room {
    pub columns: usize,
    pub lines: usize,
}

// The default screen is 320 pixels wide with 12-pixel text, 53 half-width
// letters from edge to edge; a message window keeps a margin on both sides
// and often a face. 40 fits all of those, and three lines fit the smallest
// window the editor's own system draws.
const DEFAULT_ROOM: Room = Room {
    columns: 40,
    lines: 3,
};
const FEWEST_MESSAGES: usize = 30;
const COLUMNS: std::ops::RangeInclusive<usize> = 24..=96;
const LINES: std::ops::RangeInclusive<usize> = 2..=6;
/// What a variable written into a line is taken to need.
const VARIABLE_COLUMNS: usize = 6;

impl Room {
    /// The room the game's own messages use: their widest line once the
    /// widest hundredth is left out (a developer's note, a line that runs
    /// past the window), and the most lines at least a hundredth of them
    /// have.
    pub fn of<'a>(messages: impl IntoIterator<Item = &'a str>) -> Self {
        let mut widths = Vec::new();
        let mut heights = Vec::new();
        for message in messages {
            let lines = message.lines().collect::<Vec<_>>();
            heights.push(lines.len());
            widths.extend(lines.into_iter().map(columns));
        }
        if heights.len() < FEWEST_MESSAGES {
            return DEFAULT_ROOM;
        }
        match (all_but_the_largest(widths), all_but_the_largest(heights)) {
            (Some(width), Some(height)) => Self {
                columns: width.clamp(*COLUMNS.start(), *COLUMNS.end()),
                lines: height.clamp(*LINES.start(), *LINES.end()),
            },
            _ => DEFAULT_ROOM,
        }
    }
}

fn writes_a_value(code: &str) -> bool {
    let name = code
        .trim_start_matches('\\')
        .split('[')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "v" | "s" | "self" | "cself" | "sys" | "cdb" | "udb" | "sdb" | "r"
    ) || name.starts_with('v')
}

/// The width of a line as drawn, in halves of a kanji.
pub fn columns(line: &str) -> usize {
    let mut width = 0;
    let mut rest = line;
    while let Some(character) = rest.chars().next() {
        if let Some(length) = code_length(rest) {
            if writes_a_value(&rest[..length]) {
                width += VARIABLE_COLUMNS;
            }
            rest = &rest[length..];
            continue;
        }
        width += match character {
            '\r' => 0,
            ' '..='~' | '\u{ff61}'..='\u{ff9f}' => 1,
            _ => 2,
        };
        rest = &rest[character.len_utf8()..];
    }
    width
}

/// The codes a text starts with, which a later message of the same text
/// starts with again (a colour, a size).
pub fn leading_codes(text: &str) -> &str {
    let mut length = 0;
    while let Some(code) = code_length(&text[length..]) {
        if writes_a_value(&text[length..length + code]) {
            break;
        }
        length += code;
    }
    &text[..length]
}

/// Breaks a text at spaces into lines of `width` at most, then as evenly
/// as that number of lines allows; a word wider than the width has a line
/// to itself.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    crate::wrap::wrap(text, width, &columns)
}

/// Breaks a text at `width` when that takes `lines` lines at most, and
/// else into `lines` lines as narrow as they can be: wider than the
/// Japanese was, never more lines than it had.
pub fn wrap_into(text: &str, lines: usize, width: usize) -> Vec<String> {
    crate::wrap::wrap_into(text, lines, width, &columns)
}

/// A message's English as the messages it takes: each a window's lines,
/// the later ones starting with the codes the first started with.
pub fn pages(english: &str, room: Room) -> Vec<String> {
    let lines = wrap(english, room.columns);
    let lead = leading_codes(english);
    lines
        .chunks(room.lines)
        .enumerate()
        .map(|(page, lines)| {
            let text = lines.join("\n");
            if page == 0 {
                text
            } else {
                format!("{lead}{text}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_lines_as_the_game_draws_them() {
        assert_eq!(columns("あいう"), 6);
        assert_eq!(columns("Hello, ﾐｭｽｶ"), 11);
        assert_eq!(columns("\\c[2]赤\\c[0] text\\^"), 7);
        assert_eq!(columns("\\f[\\cself[19]]所持数"), 6);
        assert_eq!(columns("<C>\\E中央"), 4);
        assert_eq!(columns("\\cself[8] left"), VARIABLE_COLUMNS + 5);
        assert_eq!(code_length("\\font[2]㌢"), Some(8));
        assert_eq!(code_length("\\\\n"), Some(2));
        assert_eq!(code_length("plain"), None);
        assert_eq!(leading_codes("\\E\\c[3]Run. \\c[0]Now"), "\\E\\c[3]");
        assert_eq!(leading_codes("\\cself[5] is here"), "");
    }

    #[test]
    fn takes_the_room_from_the_games_own_messages() {
        let mut messages = vec!["短い行\n二行目"; 97];
        messages.push("とても長い開発者向けの注意書きがここにずっとずっと続いていく行");
        messages.push("一\n二\n三");
        messages.push("一\n二\n三\n四");
        // 26 columns is the widest line but for the hundredth left out.
        messages.push("これは十三文字の行ですよね");
        messages.push("これは十三文字の行ですよね");
        let room = Room::of(messages.iter().copied());
        assert_eq!(
            room,
            Room {
                columns: 26,
                lines: 3
            }
        );
        assert_eq!(Room::of(["一行だけ"]), DEFAULT_ROOM);
        let narrow = vec!["はい"; 40];
        assert_eq!(Room::of(narrow.iter().copied()).columns, 24);
    }

    #[test]
    fn wraps_evenly_and_keeps_codes_with_their_words() {
        assert_eq!(
            wrap("The door is locked. \\c[2]Find the key\\c[0] first.", 26),
            ["The door is locked.", "\\c[2]Find the key\\c[0] first."]
        );
        // Two lines either way: the narrowest two, not a full one and a word.
        assert_eq!(
            wrap("I should hurry back before she wakes up", 30),
            ["I should hurry back", "before she wakes up"]
        );
        assert_eq!(
            wrap("Supercalifragilistic word", 10),
            ["Supercalifragilistic", "word"]
        );
        assert_eq!(wrap("Yes", 40), ["Yes"]);
        assert!(wrap("", 40).is_empty());
    }

    #[test]
    fn keeps_to_the_lines_the_japanese_had() {
        assert_eq!(
            wrap_into("Restores a little health. Tastes bitter.", 2, 16),
            ["Restores a little", "health. Tastes bitter."]
        );
        assert_eq!(wrap_into("Potion", 1, 4), ["Potion"]);
        assert_eq!(
            wrap_into("A key to the cellar", 2, 40),
            ["A key to the cellar"]
        );
    }

    #[test]
    fn turns_a_long_message_into_several() {
        let room = Room {
            columns: 20,
            lines: 2,
        };
        assert_eq!(
            pages(
                "\\E\\c[4]One two three four five six seven eight nine ten eleven twelve.\\^",
                room
            ),
            [
                "\\E\\c[4]One two three four\nfive six seven",
                "\\E\\c[4]eight nine ten\neleven twelve.\\^"
            ]
        );
        assert_eq!(pages("Short.", room), ["Short."]);
    }
}
