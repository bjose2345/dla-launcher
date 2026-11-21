//! `BasicData/CommonEvent.dat`: the shared events, each with its commands,
//! its arguments' names and choices, and the names of its own variables.
//! Version 3.5 packs everything behind the file's first bytes and ends it
//! with a byte of its own.

use std::io;

use super::{
    commands::{Command, Layout, read_commands, write_commands},
    stream::{Reader, Writer, invalid, pack, unpack, unsupported},
    text::Encoding,
};

const START: [u8; 10] = *b"\0W\0\0OL\0FC\0";
const ENCODING_MARK: usize = 6;
const VERSION_2: u8 = 0x8f;
const VERSION_3_5: u8 = 0x93;
const EVENT: u8 = 0x8e;
const VARIABLES: usize = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommonEvents {
    pub encoding: Encoding,
    version: u8,
    /// The version in a 2.x file; 3.5 ends with another byte.
    end: u8,
    pub events: Vec<CommonEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommonEvent {
    pub id: u32,
    unknown: u32,
    settings: [u8; 7],
    pub name: Vec<u8>,
    pub commands: Vec<Command>,
    unknown_text: Vec<u8>,
    description: Vec<u8>,
    argument_names: Vec<Vec<u8>>,
    argument_kinds: Vec<u8>,
    argument_choices: Vec<Vec<Vec<u8>>>,
    argument_choice_values: Vec<Vec<u32>>,
    argument_defaults: Vec<u32>,
    colour: u32,
    variable_names: Vec<Vec<u8>>,
    memo: Vec<u8>,
    /// Absent in files of the editor's first 2.x versions.
    returned: Option<(Vec<u8>, u32)>,
}

impl CommonEvents {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let other = || {
            unsupported(
                "the common events are of a version the launcher does not read, or protected",
            )
        };
        let (start, rest) = bytes.split_at_checked(START.len() + 1).ok_or_else(other)?;
        let encoding =
            Encoding::marked(&start[..START.len()], &START, ENCODING_MARK).ok_or_else(other)?;
        let version = start[START.len()];
        let unpacked;
        let body = match version {
            VERSION_2 => rest,
            VERSION_3_5 => {
                unpacked = unpack(rest)?;
                &unpacked
            }
            _ => return Err(other()),
        };
        let layout = Self::layout(version);
        let mut reader = Reader::new(body);
        let events = (0..reader.count(1)?)
            .map(|_| CommonEvent::read(&mut reader, layout))
            .collect::<io::Result<_>>()?;
        let end = reader.u8()?;
        if !reader.is_at_end() || (version == VERSION_2 && end != VERSION_2) {
            return Err(invalid("the common events do not end where they should"));
        }
        Ok(Self {
            encoding,
            version,
            end,
            events,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let layout = Self::layout(self.version);
        let mut body = Writer::default();
        body.count(self.events.len());
        for event in &self.events {
            event.write(&mut body, layout);
        }
        body.u8(self.end);
        let body = body.finish();
        let mut bytes = START.to_vec();
        bytes[ENCODING_MARK] = self.encoding.mark();
        bytes.push(self.version);
        if self.version == VERSION_3_5 {
            bytes.extend_from_slice(&pack(&body));
        } else {
            bytes.extend_from_slice(&body);
        }
        bytes
    }

    fn layout(version: u8) -> Layout {
        if version == VERSION_3_5 {
            Layout::Extended
        } else {
            Layout::Plain
        }
    }
}

impl CommonEvent {
    fn read(reader: &mut Reader<'_>, layout: Layout) -> io::Result<Self> {
        reader.expect(&[EVENT], "the start of a common event")?;
        let id = reader.u32()?;
        let unknown = reader.u32()?;
        let settings = reader.array()?;
        let name = reader.string()?;
        let commands = read_commands(reader, layout)?;
        let unknown_text = reader.string()?;
        let description = reader.string()?;
        reader.expect(&[0x8f], "a common event's arguments")?;
        let argument_names = reader.strings()?;
        let kinds = reader.count(1)?;
        let argument_kinds = reader.bytes(kinds)?.to_vec();
        let argument_choices = (0..reader.count(4)?)
            .map(|_| reader.strings())
            .collect::<io::Result<_>>()?;
        let argument_choice_values = (0..reader.count(4)?)
            .map(|_| reader.u32s())
            .collect::<io::Result<_>>()?;
        let argument_defaults = reader.u32s()?;
        reader.expect(&[0x90], "a common event's colour")?;
        let colour = reader.u32()?;
        let variable_names = (0..VARIABLES)
            .map(|_| reader.string())
            .collect::<io::Result<_>>()?;
        reader.expect(&[0x91], "a common event's memo")?;
        let memo = reader.string()?;
        let at = reader.at();
        let returned = match reader.u8()? {
            0x91 => None,
            0x92 => {
                let returned = (reader.string()?, reader.u32()?);
                reader.expect(&[0x92], "the end of a common event")?;
                Some(returned)
            }
            other => {
                return Err(invalid(format!("a common event ends with {other} at {at}")));
            }
        };
        Ok(Self {
            id,
            unknown,
            settings,
            name,
            commands,
            unknown_text,
            description,
            argument_names,
            argument_kinds,
            argument_choices,
            argument_choice_values,
            argument_defaults,
            colour,
            variable_names,
            memo,
            returned,
        })
    }

    fn write(&self, writer: &mut Writer, layout: Layout) {
        writer.u8(EVENT);
        writer.u32(self.id);
        writer.u32(self.unknown);
        writer.bytes(&self.settings);
        writer.string(&self.name);
        write_commands(&self.commands, writer, layout);
        writer.string(&self.unknown_text);
        writer.string(&self.description);
        writer.u8(0x8f);
        writer.strings(&self.argument_names);
        writer.count(self.argument_kinds.len());
        writer.bytes(&self.argument_kinds);
        writer.count(self.argument_choices.len());
        for choices in &self.argument_choices {
            writer.strings(choices);
        }
        writer.count(self.argument_choice_values.len());
        for values in &self.argument_choice_values {
            writer.u32s(values);
        }
        writer.u32s(&self.argument_defaults);
        writer.u8(0x90);
        writer.u32(self.colour);
        for name in &self.variable_names {
            writer.string(name);
        }
        writer.u8(0x91);
        writer.string(&self.memo);
        match &self.returned {
            None => writer.u8(0x91),
            Some((name, variable)) => {
                writer.u8(0x92);
                writer.string(name);
                writer.u32(*variable);
                writer.u8(0x92);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::wolf::text::encode;

    pub fn common_event(id: u32, name: &str, commands: Vec<Command>) -> CommonEvent {
        CommonEvent {
            id,
            unknown: 0,
            settings: [0, 0, 0, 0, 0, 0, 1],
            name: encode(name),
            commands,
            unknown_text: Vec::new(),
            description: encode("説明"),
            argument_names: (0..10).map(|at| encode(&format!("引数{at}"))).collect(),
            argument_kinds: vec![0; 10],
            argument_choices: vec![vec![encode("はい"), encode("いいえ")], Vec::new()],
            argument_choice_values: vec![vec![1, 0], Vec::new()],
            argument_defaults: vec![0; 5],
            colour: 2,
            variable_names: (0..VARIABLES).map(|_| Vec::new()).collect(),
            memo: encode("メモ"),
            returned: Some((encode("結果"), 3)),
        }
    }

    pub fn common_events(events: Vec<Vec<Command>>) -> CommonEvents {
        CommonEvents {
            encoding: Encoding::ShiftJis,
            version: VERSION_2,
            end: VERSION_2,
            events: events
                .into_iter()
                .enumerate()
                .map(|(id, commands)| common_event(id as u32, &format!("コモン{id}"), commands))
                .collect(),
        }
    }

    /// The same events as version 3.5 keeps them; their strings are the
    /// caller's to encode.
    pub fn newer(mut events: CommonEvents) -> CommonEvents {
        events.encoding = Encoding::Utf8;
        events.version = VERSION_3_5;
        events.end = 0x92;
        events
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::*, *};
    use crate::wolf::commands::testing::{message, moving};

    #[test]
    fn writes_the_common_events_back_byte_for_byte() {
        let mut built = common_events(vec![vec![message("@2\n行くぞ"), moving()], Vec::new()]);
        built.events[1].returned = None;
        let bytes = built.write();
        let parsed = CommonEvents::parse(&bytes).unwrap();
        assert_eq!(parsed, built);
        assert_eq!(parsed.write(), bytes);
    }

    #[test]
    fn reads_and_writes_the_packed_file_of_version_3_5() {
        let built = newer(common_events(vec![
            vec![message("@2\n行くぞ"), moving()],
            Vec::new(),
        ]));
        let bytes = built.write();
        assert_eq!(bytes[..11], *b"\0W\0\0OLUFC\0\x93");
        let unpacked = unpack(&bytes[11..]).unwrap();
        assert_eq!(unpacked[..4], [2, 0, 0, 0]);
        assert_eq!(unpacked.last(), Some(&0x92));
        let parsed = CommonEvents::parse(&bytes).unwrap();
        assert_eq!(parsed, built);
        assert_eq!(parsed.write(), bytes);

        // The older layout has no byte after each command.
        let older = common_events(vec![vec![message("@2\n行くぞ"), moving()], Vec::new()]);
        assert_eq!(unpacked.len(), older.write().len() - 11 + 2);
        let mut cut = bytes.clone();
        cut.truncate(bytes.len() - 1);
        assert_eq!(
            CommonEvents::parse(&cut).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn refuses_other_versions_and_stops_at_damage() {
        let bytes = common_events(vec![vec![message("あ")]]).write();
        let mut newer = bytes.clone();
        newer[10] = 0x90;
        assert_eq!(
            CommonEvents::parse(&newer).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        // Protected files do not start with a zero byte.
        let mut protected = bytes.clone();
        protected[0] = 0x5a;
        assert_eq!(
            CommonEvents::parse(&protected).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        let mut damaged = bytes.clone();
        *damaged.last_mut().unwrap() = 0;
        assert_eq!(
            CommonEvents::parse(&damaged).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(CommonEvents::parse(&bytes[..bytes.len() / 2]).is_err());
    }
}
