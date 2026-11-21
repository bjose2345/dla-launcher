//! The game's strings as text. WOLF RPG Editor 2.x keeps them in code page
//! 932 (Shift-JIS as Windows reads it), which has no accented letters and
//! draws its own quotation marks and dashes as wide as a kanji; version 3
//! keeps them in UTF-8 and says so with one byte among a file's first.

use encoding_rs::SHIFT_JIS;

const UTF8_MARK: u8 = b'U';

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encoding {
    ShiftJis,
    Utf8,
}

impl Encoding {
    /// The encoding a file's first bytes name: they are `plain` but for
    /// the byte at `mark`, which is zero in a 2.x file.
    pub fn marked(start: &[u8], plain: &[u8], mark: usize) -> Option<Self> {
        if start.len() != plain.len()
            || start[..mark] != plain[..mark]
            || start[mark + 1..] != plain[mark + 1..]
        {
            return None;
        }
        match start[mark] {
            0 => Some(Self::ShiftJis),
            UTF8_MARK => Some(Self::Utf8),
            _ => None,
        }
    }

    pub fn mark(self) -> u8 {
        match self {
            Self::ShiftJis => 0,
            Self::Utf8 => UTF8_MARK,
        }
    }

    /// `None` for bytes that are not text in the encoding: left as they
    /// are.
    pub fn decode(self, bytes: &[u8]) -> Option<String> {
        match self {
            Self::ShiftJis => {
                let (text, _, wrong) = SHIFT_JIS.decode(bytes);
                (!wrong).then(|| text.into_owned())
            }
            Self::Utf8 => std::str::from_utf8(bytes).ok().map(str::to_owned),
        }
    }

    /// Text as the game stores it. What code page 932 lacks becomes `?`.
    pub fn encode(self, text: &str) -> Vec<u8> {
        if self == Self::Utf8 {
            return text.as_bytes().to_vec();
        }
        let mut bytes = Vec::with_capacity(text.len());
        let mut buffer = [0; 4];
        for character in text.chars() {
            let (encoded, _, unmappable) = SHIFT_JIS.encode(character.encode_utf8(&mut buffer));
            if unmappable {
                bytes.push(b'?');
            } else {
                bytes.extend_from_slice(&encoded);
            }
        }
        bytes
    }
}

#[cfg(test)]
thread_local! {
    static FIXTURES: std::cell::Cell<Encoding> = const { std::cell::Cell::new(Encoding::ShiftJis) };
}

/// The fixtures' strings are 2.x data unless they are built `in_utf8`.
#[cfg(test)]
pub fn decode(bytes: &[u8]) -> Option<String> {
    FIXTURES.get().decode(bytes)
}

#[cfg(test)]
pub fn encode(text: &str) -> Vec<u8> {
    FIXTURES.get().encode(text)
}

#[cfg(test)]
pub fn in_utf8<T>(build: impl FnOnce() -> T) -> T {
    FIXTURES.set(Encoding::Utf8);
    let built = build();
    FIXTURES.set(Encoding::ShiftJis);
    built
}

/// English as the game's fonts draw it at half width: typographic marks
/// become their plain forms and accented letters lose their accents.
pub fn plain_english(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '‘' | '’' | '′' => plain.push('\''),
            '“' | '”' | '″' => plain.push('"'),
            '–' | '—' | '―' | '‐' | '‑' => plain.push('-'),
            '…' => plain.push_str("..."),
            '\u{a0}' | '\u{2009}' | '\u{202f}' => plain.push(' '),
            'à'..='å' => plain.push('a'),
            'è'..='ë' => plain.push('e'),
            'ì'..='ï' => plain.push('i'),
            'ò'..='ö' => plain.push('o'),
            'ù'..='ü' => plain.push('u'),
            'ç' => plain.push('c'),
            'ñ' => plain.push('n'),
            other => plain.push(other),
        }
    }
    plain
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_games_code_page() {
        let bytes = encode("サキュバスAbc①");
        assert_eq!(bytes[..2], [0x83, 0x54]);
        assert_eq!(decode(&bytes).as_deref(), Some("サキュバスAbc①"));
        assert_eq!(decode(&[0x83]), None);
        assert_eq!(encode("naïve ☃"), b"na?ve ?");
    }

    #[test]
    fn reads_and_writes_utf8_and_tells_the_encoding_from_a_files_start() {
        let utf8 = Encoding::Utf8;
        assert_eq!(
            utf8.encode("naïve ☃ サキュバス"),
            "naïve ☃ サキュバス".as_bytes()
        );
        assert_eq!(utf8.decode("館".as_bytes()).as_deref(), Some("館"));
        assert_eq!(utf8.decode(&[0x83, 0x54]), None);

        let plain = b"\0W\0\0OL\0FC\0";
        assert_eq!(Encoding::marked(plain, plain, 6), Some(Encoding::ShiftJis));
        assert_eq!(
            Encoding::marked(b"\0W\0\0OLUFC\0", plain, 6),
            Some(Encoding::Utf8)
        );
        assert_eq!(Encoding::marked(b"\0W\0\0OLXFC\0", plain, 6), None);
        assert_eq!(Encoding::marked(b"\0P\0\0OLUFC\0", plain, 6), None);
        assert_eq!(Encoding::marked(b"\0W\0\0OLUFD\0", plain, 6), None);
        assert_eq!(Encoding::marked(b"\0W", plain, 6), None);
        assert_eq!(Encoding::Utf8.mark(), b'U');
    }

    #[test]
    fn writes_english_with_plain_marks() {
        assert_eq!(
            plain_english("“Don’t”… café — naïve"),
            "\"Don't\"... cafe - naive"
        );
        assert_eq!(plain_english("ミュスカ♪"), "ミュスカ♪");
    }
}
