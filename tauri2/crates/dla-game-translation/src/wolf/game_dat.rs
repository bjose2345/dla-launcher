//! `BasicData/Game.dat`: the game's settings, with its title first among
//! their strings. The file states its own length after them, which is
//! written again for a title of another length.
//!
//! The launcher only looks at how the file starts, to tell a protected
//! game. It never writes it: the engine knows a game's saves by its title
//! and sets the system save aside (`SaveData999.sav.anotherGame`) when the
//! title changes. Reading and writing the file serves the check on a real
//! game, whose window shows the title of the data it loaded.

#[cfg(test)]
use std::io;

#[cfg(test)]
use super::stream::{Reader, Writer, invalid, unsupported};
use super::text::Encoding;

const START: [u8; 10] = *b"\0W\0\0OL\0FM\0";
const ENCODING_MARK: usize = 9;

/// Whether the file starts as an unprotected one does.
pub fn has_known_start(bytes: &[u8]) -> bool {
    encoding(bytes).is_some()
}

fn encoding(bytes: &[u8]) -> Option<Encoding> {
    Encoding::marked(bytes.get(..START.len())?, &START, ENCODING_MARK)
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameDat {
    pub encoding: Encoding,
    settings: Vec<u8>,
    /// The title, then a serial, a key, the fonts and more.
    strings: Vec<Vec<u8>>,
    rest: Vec<u8>,
}

#[cfg(test)]
impl GameDat {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let mut reader = Reader::new(bytes);
        let Some(encoding) = encoding(reader.bytes(START.len()).unwrap_or_default()) else {
            return Err(unsupported(
                "Game.dat is of a version the launcher does not read, or protected",
            ));
        };
        let settings = reader.count(1)?;
        let settings = reader.bytes(settings)?.to_vec();
        let strings = reader.strings()?;
        // One less than the file's length.
        let stated = reader.u32()?;
        if strings.is_empty() || u64::from(stated) + 1 != bytes.len() as u64 {
            return Err(invalid(
                "Game.dat is laid out in a way the launcher does not know",
            ));
        }
        Ok(Self {
            encoding,
            settings,
            strings,
            rest: reader.rest().to_vec(),
        })
    }

    pub fn title(&self) -> &[u8] {
        &self.strings[0]
    }

    pub fn set_title(&mut self, title: Vec<u8>) {
        self.strings[0] = title;
    }

    pub fn write(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        writer.bytes(&START[..ENCODING_MARK]);
        writer.u8(self.encoding.mark());
        writer.count(self.settings.len());
        writer.bytes(&self.settings);
        writer.strings(&self.strings);
        let mut bytes = writer.finish();
        let length = bytes.len() + 4 + self.rest.len();
        bytes.extend_from_slice(&(length as u32 - 1).to_le_bytes());
        bytes.extend_from_slice(&self.rest);
        bytes
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::wolf::text::encode;

    pub fn game_dat(title: &str) -> Vec<u8> {
        GameDat {
            encoding: Encoding::ShiftJis,
            settings: vec![16, 8, 4, 1, 60],
            strings: [
                title,
                "0000-0000",
                "key",
                "ＭＳ ゴシック",
                "",
                "",
                "",
                "",
                "ｖ2.14",
            ]
            .iter()
            .map(|text| encode(text))
            .collect(),
            rest: (0..200).map(|at| (at * 7) as u8).collect(),
        }
        .write()
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::game_dat, *};
    use crate::wolf::text::encode;

    #[test]
    fn writes_the_file_back_and_states_its_length_for_another_title() {
        let bytes = game_dat("サキュバスの館");
        let mut parsed = GameDat::parse(&bytes).unwrap();
        assert_eq!(parsed.title(), encode("サキュバスの館"));
        assert_eq!(parsed.write(), bytes);

        parsed.set_title(encode("The Succubus Mansion"));
        let longer = parsed.write();
        assert_eq!(longer.len(), bytes.len() + 6);
        let again = GameDat::parse(&longer).unwrap();
        assert_eq!(again.title(), b"The Succubus Mansion");
        assert_eq!(again.rest, parsed.rest);
    }

    #[test]
    fn knows_the_start_of_a_version_3_file() {
        let mut bytes = game_dat("題");
        bytes[9] = b'U';
        assert!(has_known_start(&bytes));
        let parsed = GameDat::parse(&bytes).unwrap();
        assert_eq!(parsed.encoding, Encoding::Utf8);
        assert_eq!(parsed.write(), bytes);
        bytes[9] = b'V';
        assert!(!has_known_start(&bytes));
        assert!(!has_known_start(b"\0W\0"));
    }

    #[test]
    fn refuses_protected_files_and_unknown_layouts() {
        let bytes = game_dat("題");
        let mut protected = bytes.clone();
        protected[0] = 0x44;
        assert_eq!(
            GameDat::parse(&protected).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(
            GameDat::parse(&longer).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
