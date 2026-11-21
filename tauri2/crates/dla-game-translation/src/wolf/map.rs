//! A map file (`MapData/*.mps`): its tiles, and its events with a list of
//! commands for each of their pages. Version 3 packs everything behind the
//! file's first bytes, and from 3.5 on says how many layers of tiles the
//! map has.

use std::io;

use super::{
    commands::{
        Command, Layout, RouteStep, read_commands, read_route_steps, write_commands,
        write_route_steps,
    },
    stream::{Reader, Writer, invalid, pack, unpack, unsupported},
    text::Encoding,
};

const START: [u8; 20] = *b"\0\0\0\0\0\0\0\0\0\0WOLFM\0\0\0\0\0";
const ENCODING_MARK: usize = 16;
/// The start, the format's number and one byte more.
const HEAD: usize = START.len() + 4 + 1;
const FORMAT_2: u32 = 0x64;
const FORMAT_2_HEAD_END: u8 = 0x65;
const FORMAT_PACKED: u32 = 0x65;
const FORMAT_3_5: u32 = 0x67;
const LAYERS_BEFORE_3_5: u32 = 3;
const TILE_BYTES: usize = 4;
const EVENT: u8 = 0x6f;
const EVENTS_END: u8 = 0x66;
const EVENT_START: [u8; 4] = [0x39, 0x30, 0, 0];
const PAGE: u8 = 0x79;
const PAGES_END: u8 = 0x70;
const PAGE_END: u8 = 0x7a;
// Shadow, width and height, and one more byte in newer files.
const PAGE_OPTIONS: std::ops::RangeInclusive<usize> = 3..=4;
// Graphic direction, frame, opacity and render mode; the page's
// conditions; its movement; its flags.
const PAGE_SETTINGS_BYTES: usize = 4 + 37 + 4 + 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Map {
    pub encoding: Encoding,
    format: u32,
    head_end: u8,
    unknown: Vec<u8>,
    tileset: u32,
    width: u32,
    height: u32,
    /// An unknown number and the number of layers, from 3.5 on.
    layers: Option<(u32, u32)>,
    tiles: Vec<u8>,
    pub events: Vec<Event>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    pub id: u32,
    pub name: Vec<u8>,
    x: u32,
    y: u32,
    unknown: u32,
    pub pages: Vec<Page>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Page {
    unknown: u32,
    graphic: Vec<u8>,
    settings: [u8; PAGE_SETTINGS_BYTES],
    route: Vec<RouteStep>,
    pub commands: Vec<Command>,
    options: Vec<u8>,
}

impl Map {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let other =
            || unsupported("the map is of a version the launcher does not read, or protected");
        let (head, rest) = bytes.split_at_checked(HEAD).ok_or_else(other)?;
        let encoding =
            Encoding::marked(&head[..START.len()], &START, ENCODING_MARK).ok_or_else(other)?;
        let format = u32::from_le_bytes([head[20], head[21], head[22], head[23]]);
        let head_end = head[HEAD - 1];
        let known = match format {
            FORMAT_2 => head_end == FORMAT_2_HEAD_END,
            _ => (FORMAT_PACKED..=FORMAT_3_5).contains(&format),
        };
        if !known {
            return Err(other());
        }
        let unpacked;
        let body = if format >= FORMAT_PACKED {
            unpacked = unpack(rest)?;
            &unpacked
        } else {
            rest
        };
        let layout = Self::layout(format);
        let mut reader = Reader::new(body);
        let unknown = reader.string()?;
        let (tileset, width, height) = (reader.u32()?, reader.u32()?, reader.u32()?);
        let event_count = reader.u32()? as usize;
        let layers = if format >= FORMAT_3_5 {
            Some((reader.u32()?, reader.u32()?))
        } else {
            None
        };
        let tiles = (width as usize)
            .checked_mul(height as usize)
            .and_then(|tiles| {
                let layers = layers.map_or(LAYERS_BEFORE_3_5, |(_, layers)| layers);
                tiles.checked_mul(layers as usize * TILE_BYTES)
            })
            .ok_or_else(|| invalid("the map's size is not one a map has"))?;
        let tiles = reader.bytes(tiles)?.to_vec();
        let mut events = Vec::new();
        loop {
            match reader.u8()? {
                EVENT => events.push(Event::read(&mut reader, layout)?),
                EVENTS_END => break,
                other => return Err(invalid(format!("the map's events end with {other}"))),
            }
        }
        if events.len() != event_count || !reader.is_at_end() {
            return Err(invalid("the map does not end where its events do"));
        }
        Ok(Self {
            encoding,
            format,
            head_end,
            unknown,
            tileset,
            width,
            height,
            layers,
            tiles,
            events,
        })
    }

    pub fn write(&self) -> Vec<u8> {
        let layout = Self::layout(self.format);
        let mut writer = Writer::default();
        writer.string(&self.unknown);
        writer.u32(self.tileset);
        writer.u32(self.width);
        writer.u32(self.height);
        writer.count(self.events.len());
        if let Some((unknown, layers)) = self.layers {
            writer.u32(unknown);
            writer.u32(layers);
        }
        writer.bytes(&self.tiles);
        for event in &self.events {
            writer.u8(EVENT);
            event.write(&mut writer, layout);
        }
        writer.u8(EVENTS_END);
        let body = writer.finish();
        let mut bytes = START.to_vec();
        bytes[ENCODING_MARK] = self.encoding.mark();
        bytes.extend_from_slice(&self.format.to_le_bytes());
        bytes.push(self.head_end);
        if self.format >= FORMAT_PACKED {
            bytes.extend_from_slice(&pack(&body));
        } else {
            bytes.extend_from_slice(&body);
        }
        bytes
    }

    fn layout(format: u32) -> Layout {
        if format >= FORMAT_3_5 {
            Layout::Extended
        } else {
            Layout::Plain
        }
    }
}

impl Event {
    fn read(reader: &mut Reader<'_>, layout: Layout) -> io::Result<Self> {
        reader.expect(&EVENT_START, "the start of an event")?;
        let id = reader.u32()?;
        let name = reader.string()?;
        let (x, y) = (reader.u32()?, reader.u32()?);
        let page_count = reader.u32()? as usize;
        let unknown = reader.u32()?;
        let mut pages = Vec::new();
        loop {
            match reader.u8()? {
                PAGE => pages.push(Page::read(reader, layout)?),
                PAGES_END => break,
                other => return Err(invalid(format!("an event's pages end with {other}"))),
            }
        }
        if pages.len() != page_count {
            return Err(invalid("an event has another number of pages than it says"));
        }
        Ok(Self {
            id,
            name,
            x,
            y,
            unknown,
            pages,
        })
    }

    fn write(&self, writer: &mut Writer, layout: Layout) {
        writer.bytes(&EVENT_START);
        writer.u32(self.id);
        writer.string(&self.name);
        writer.u32(self.x);
        writer.u32(self.y);
        writer.count(self.pages.len());
        writer.u32(self.unknown);
        for page in &self.pages {
            writer.u8(PAGE);
            page.write(writer, layout);
        }
        writer.u8(PAGES_END);
    }
}

impl Page {
    fn read(reader: &mut Reader<'_>, layout: Layout) -> io::Result<Self> {
        let unknown = reader.u32()?;
        let graphic = reader.string()?;
        let settings = reader.array()?;
        let route = read_route_steps(reader)?;
        let commands = read_commands(reader, layout)?;
        let at = reader.at();
        let options = reader.u32()? as usize;
        if !PAGE_OPTIONS.contains(&options) {
            return Err(invalid(format!(
                "a page's commands do not end as they should at {at}"
            )));
        }
        let options = reader.bytes(options)?.to_vec();
        reader.expect(&[PAGE_END], "the end of a page")?;
        Ok(Self {
            unknown,
            graphic,
            settings,
            route,
            commands,
            options,
        })
    }

    fn write(&self, writer: &mut Writer, layout: Layout) {
        writer.u32(self.unknown);
        writer.string(&self.graphic);
        writer.bytes(&self.settings);
        write_route_steps(&self.route, writer);
        write_commands(&self.commands, writer, layout);
        writer.count(self.options.len());
        writer.bytes(&self.options);
        writer.u8(PAGE_END);
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::wolf::text::encode;

    /// A 2×1 map whose events have one page each with the given commands.
    pub fn map(events: Vec<Vec<Command>>) -> Map {
        Map {
            encoding: Encoding::ShiftJis,
            format: FORMAT_2,
            head_end: FORMAT_2_HEAD_END,
            unknown: encode("なし"),
            tileset: 3,
            width: 2,
            height: 1,
            layers: None,
            tiles: (0..24).collect(),
            events: events
                .into_iter()
                .enumerate()
                .map(|(id, commands)| Event {
                    id: id as u32,
                    name: encode("イベント"),
                    x: 1,
                    y: 0,
                    unknown: 0,
                    pages: vec![Page {
                        unknown: 0,
                        graphic: encode("CharaChip/人.png"),
                        settings: [7; PAGE_SETTINGS_BYTES],
                        route: Vec::new(),
                        commands,
                        options: vec![0, 1, 1],
                    }],
                })
                .collect(),
        }
    }

    /// The same map as version 3.5 keeps it, with `layers` layers of
    /// tiles; its strings are the caller's to encode.
    pub fn newer(mut map: Map, layers: u32) -> Map {
        map.encoding = Encoding::Utf8;
        map.format = FORMAT_3_5;
        map.head_end = 0x69;
        map.layers = Some((3, layers));
        map.tiles = (0..map.width * map.height * layers * TILE_BYTES as u32)
            .map(|tile| tile as u8)
            .collect();
        map
    }
}

#[cfg(test)]
mod tests {
    use super::{
        testing::{map, newer},
        *,
    };
    use crate::wolf::commands::testing::{message, moving};

    #[test]
    fn writes_a_map_back_byte_for_byte() {
        let mut built = map(vec![vec![message("@1\nこんにちは"), moving()], Vec::new()]);
        let page = built.events[0].pages[0].clone();
        built.events[1].pages.push(page);
        let bytes = built.write();
        assert_eq!(bytes[..25], *b"\0\0\0\0\0\0\0\0\0\0WOLFM\0\0\0\0\0d\0\0\0e");
        let parsed = Map::parse(&bytes).unwrap();
        assert_eq!(parsed, built);
        assert_eq!(parsed.write(), bytes);
    }

    #[test]
    fn reads_and_writes_the_packed_map_of_version_3_5() {
        let events = || vec![vec![message("@1\nこんにちは"), moving()], Vec::new()];
        let mut built = newer(map(events()), 5);
        built.events[1].pages[0].options.push(9);
        let bytes = built.write();
        assert_eq!(bytes[..25], *b"\0\0\0\0\0\0\0\0\0\0WOLFM\0U\0\0\0g\0\0\0i");
        let unpacked = unpack(&bytes[25..]).unwrap();
        // The string, four numbers, then the two only 3.5 has, and five
        // layers of a 2×1 map.
        assert_eq!(unpacked[9 + 16..][..8], [3, 0, 0, 0, 5, 0, 0, 0]);
        assert_eq!(unpacked[9 + 24..][..40], built.tiles[..]);
        let parsed = Map::parse(&bytes).unwrap();
        assert_eq!(parsed, built);
        assert_eq!(parsed.write(), bytes);

        // Between 3.3 and 3.5 a map is packed and otherwise as before.
        let mut between = map(events());
        between.format = FORMAT_PACKED;
        between.head_end = 0x66;
        let bytes = between.write();
        assert_eq!(unpack(&bytes[25..]).unwrap(), map(events()).write()[25..]);
        assert_eq!(Map::parse(&bytes).unwrap(), between);
    }

    #[test]
    fn refuses_other_versions_and_stops_at_damage() {
        let bytes = map(vec![vec![message("あ")]]).write();
        let mut other_head = bytes.clone();
        other_head[24] = 0x66;
        assert_eq!(
            Map::parse(&other_head).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        let mut later = bytes.clone();
        later[20] = 0x68;
        assert_eq!(
            Map::parse(&later).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        let mut options = bytes.clone();
        let at = bytes.len() - 1 - 1 - 1 - 3 - 4;
        assert_eq!(options[at..at + 4], [3, 0, 0, 0]);
        options[at] = 5;
        assert_eq!(
            Map::parse(&options).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            Map::parse(b"short").unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        let mut extra = bytes.clone();
        extra.push(0);
        assert_eq!(
            Map::parse(&extra).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(Map::parse(&bytes[..bytes.len() - 1]).is_err());
    }
}
