//! Event commands, as maps and common events both store them: a number,
//! integer arguments, the indent of the branch it is in, string arguments,
//! and for a move command the route it sets. From version 3.5 on a command
//! ends with a counted run of bytes the editor keeps for itself.

use std::io;

use super::stream::{Reader, Writer, invalid};

pub const SHOW_MESSAGE: u32 = 101;
pub const SHOW_CHOICES: u32 = 102;
pub const COMMENT: u32 = 103;
pub const DEBUG_MESSAGE: u32 = 106;
pub const STRING_CONDITION: u32 = 112;
pub const SET_STRING: u32 = 122;
pub const PICTURE: u32 = 150;
pub const CALL_EVENT: u32 = 210;
pub const LABEL: u32 = 212;
pub const JUMP_TO_LABEL: u32 = 213;
pub const DATABASE: u32 = 250;
pub const DATABASE_FILE: u32 = 251;
pub const CALL_EVENT_BY_NAME: u32 = 300;

const ROUTE_STEP_END: [u8; 2] = [1, 0];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Command {
    pub id: u32,
    pub args: Vec<u32>,
    pub indent: u8,
    pub strings: Vec<Vec<u8>>,
    pub route: Option<Route>,
    /// Empty in every command seen so far.
    extension: Vec<u8>,
}

/// How a file's commands are laid out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layout {
    Plain,
    Extended,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Route {
    unknown: [u8; 5],
    flags: u8,
    steps: Vec<RouteStep>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteStep {
    id: u8,
    args: Vec<u32>,
}

impl RouteStep {
    pub fn read(reader: &mut Reader<'_>) -> io::Result<Self> {
        let id = reader.u8()?;
        let args = (0..reader.u8()?)
            .map(|_| reader.u32())
            .collect::<io::Result<_>>()?;
        reader.expect(&ROUTE_STEP_END, "the end of a route step")?;
        Ok(Self { id, args })
    }

    pub fn write(&self, writer: &mut Writer) {
        writer.u8(self.id);
        writer.u8(self.args.len() as u8);
        for arg in &self.args {
            writer.u32(*arg);
        }
        writer.bytes(&ROUTE_STEP_END);
    }
}

pub fn read_route_steps(reader: &mut Reader<'_>) -> io::Result<Vec<RouteStep>> {
    (0..reader.count(4)?)
        .map(|_| RouteStep::read(reader))
        .collect()
}

pub fn write_route_steps(steps: &[RouteStep], writer: &mut Writer) {
    writer.count(steps.len());
    for step in steps {
        step.write(writer);
    }
}

impl Command {
    pub fn read(reader: &mut Reader<'_>, layout: Layout) -> io::Result<Self> {
        let at = reader.at();
        // The count includes the command's number.
        let args = reader
            .u8()?
            .checked_sub(1)
            .ok_or_else(|| invalid(format!("a command at {at} has no number")))?;
        let id = reader.u32()?;
        let args = (0..args).map(|_| reader.u32()).collect::<io::Result<_>>()?;
        let indent = reader.u8()?;
        let strings = (0..reader.u8()?)
            .map(|_| reader.string())
            .collect::<io::Result<_>>()?;
        let route = match reader.u8()? {
            0 => None,
            1 => Some(Route {
                unknown: reader.array()?,
                flags: reader.u8()?,
                steps: read_route_steps(reader)?,
            }),
            other => {
                return Err(invalid(format!(
                    "the command at {at} ends with {other}, which the launcher does not know"
                )));
            }
        };
        let extension = match layout {
            Layout::Plain => Vec::new(),
            Layout::Extended => {
                let length = reader.u8()?;
                reader.bytes(length.into())?.to_vec()
            }
        };
        Ok(Self {
            id,
            args,
            indent,
            strings,
            route,
            extension,
        })
    }

    pub fn write(&self, writer: &mut Writer, layout: Layout) {
        writer.u8(self.args.len() as u8 + 1);
        writer.u32(self.id);
        for arg in &self.args {
            writer.u32(*arg);
        }
        writer.u8(self.indent);
        writer.u8(self.strings.len() as u8);
        for string in &self.strings {
            writer.string(string);
        }
        match &self.route {
            None => writer.u8(0),
            Some(route) => {
                writer.u8(1);
                writer.bytes(&route.unknown);
                writer.u8(route.flags);
                write_route_steps(&route.steps, writer);
            }
        }
        if layout == Layout::Extended {
            writer.u8(self.extension.len() as u8);
            writer.bytes(&self.extension);
        }
    }
}

pub fn read_commands(reader: &mut Reader<'_>, layout: Layout) -> io::Result<Vec<Command>> {
    (0..reader.count(8)?)
        .map(|_| Command::read(reader, layout))
        .collect()
}

pub fn write_commands(commands: &[Command], writer: &mut Writer, layout: Layout) {
    writer.count(commands.len());
    for command in commands {
        command.write(writer, layout);
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::wolf::text::encode;

    pub fn command(id: u32, args: &[u32], strings: &[&str]) -> Command {
        Command {
            id,
            args: args.to_vec(),
            indent: 0,
            strings: strings.iter().map(|text| encode(text)).collect(),
            route: None,
            extension: Vec::new(),
        }
    }

    pub fn message(text: &str) -> Command {
        command(SHOW_MESSAGE, &[], &[text])
    }

    /// A move command with a two-step route.
    pub fn moving() -> Command {
        Command {
            id: 201,
            args: vec![0xffff_fffe],
            indent: 2,
            strings: Vec::new(),
            route: Some(Route {
                unknown: [1, 2, 3, 4, 5],
                flags: 6,
                steps: vec![
                    RouteStep {
                        id: 4,
                        args: Vec::new(),
                    },
                    RouteStep {
                        id: 29,
                        args: vec![7, 8],
                    },
                ],
            }),
            extension: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{testing::*, *};

    #[test]
    fn writes_commands_back_as_they_were_read() {
        let commands = vec![
            message("@1\nこんにちは"),
            command(SHOW_CHOICES, &[2], &["はい", "いいえ"]),
            moving(),
            command(0, &[], &[]),
        ];
        let mut writer = Writer::default();
        write_commands(&commands, &mut writer, Layout::Plain);
        let bytes = writer.finish();
        // Count, then the first command: one argument slot (its number),
        // no indent, one string of 13 bytes and its NUL, no route.
        assert_eq!(bytes[..4], [4, 0, 0, 0]);
        assert_eq!(bytes[4..10], [1, 101, 0, 0, 0, 0]);
        assert_eq!(bytes[10..15], [1, 14, 0, 0, 0]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(read_commands(&mut reader, Layout::Plain).unwrap(), commands);
        assert!(reader.is_at_end());
    }

    #[test]
    fn keeps_what_a_newer_command_ends_with() {
        let mut folded = moving();
        folded.extension = vec![1];
        let commands = vec![message("こんにちは"), folded];
        let mut writer = Writer::default();
        write_commands(&commands, &mut writer, Layout::Extended);
        let bytes = writer.finish();
        // The message: its number, no indent, one string, no route, and
        // nothing kept after it.
        assert_eq!(bytes[4..10], [1, 101, 0, 0, 0, 0]);
        assert_eq!(bytes[10 + 1 + 4 + 16..][..2], [0, 0]);
        assert_eq!(bytes[bytes.len() - 2..], [1, 1]);
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            read_commands(&mut reader, Layout::Extended).unwrap(),
            commands
        );
        assert!(reader.is_at_end());
        assert!(read_commands(&mut Reader::new(&bytes), Layout::Plain).is_err());
        assert!(
            read_commands(
                &mut Reader::new(&bytes[..bytes.len() - 1]),
                Layout::Extended
            )
            .is_err()
        );
    }

    #[test]
    fn stops_at_what_it_does_not_know() {
        let mut writer = Writer::default();
        message("あ").write(&mut writer, Layout::Plain);
        let mut bytes = writer.finish();
        *bytes.last_mut().unwrap() = 2;
        let read = |bytes: &[u8]| Command::read(&mut Reader::new(bytes), Layout::Plain);
        assert!(read(&bytes).is_err());
        assert!(read(&[0, 101, 0, 0, 0]).is_err());
        assert!(read(&bytes[..bytes.len() - 3]).is_err());
    }
}
