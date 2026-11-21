//! The archive WOLF RPG Editor up to 2.24 packs a data folder into
//! (`Data/BasicData.wolf`): DxLib's DXA format, XORed with a 12-byte key.
//! The header's own known values give the key: its magic, version and data
//! start, and what is left of it from the archive's length. The tables
//! (names, files, folders) follow the files' data; packed files use DxLib's
//! LZ.
//!
//! The editor wrote version 6 from 2.20 to 2.24: numbers of eight bytes,
//! the tables XORed from the key's start, and each file's data from the
//! position its unpacked size falls on. Before that it wrote the older
//! layout, seen as version 3 with the key of 2.10: numbers of four bytes,
//! and everything XORed from where it lies in the archive. Versions 4 (a
//! code page in the header) and 5 (keyed as version 6) are read by DxLib's
//! own description of them. Version 8, which the editor writes from 2.25
//! on with its string keys and the protection of version 3 and "Pro", is
//! not read: [`Archive::open`] refuses it.
//!
//! A game may pack its whole `Data` folder into one archive (`Data.wolf`).
//! [`Archive::write_folder`] writes one of that archive's folders as an
//! archive of its own, which is what the engine looks for once the one
//! archive is out of its way.

use std::{
    fs::File,
    io::{self, BufWriter, Read, Seek, SeekFrom, Write},
    path::Path,
};

use super::stream::{invalid, unsupported};

const KEY_BYTES: usize = 12;
const MAGIC: [u8; 2] = *b"DX";
const VERSIONS: [u16; 4] = [6, 3, 4, 5];
const LONGEST_HEADER: usize = 48;
/// A file's three times, between its attributes and its address.
const TIMES_BYTES: usize = 24;
const FOLDER: u64 = 0x10;
// A data folder's tables are a few kilobytes; a damaged size is not read.
const LARGEST_TABLES: u64 = 64 * 1024 * 1024;
// The largest file read into memory; an archive of its own is written
// from files of any size, which are copied and never held.
const LARGEST_FILE: u64 = 1024 * 1024 * 1024;
const DEEPEST_FOLDER: usize = 32;
// DxLib starts each file's data on a multiple of four bytes.
const DATA_ALIGNMENT: u64 = 4;
const COPY_BUFFER: usize = 1024 * 1024;

pub const UNSUPPORTED: &str = "the archive is not one WOLF RPG Editor writes up to 2.24 (DXA version 3 to 6 with a 12-byte key); later versions of the editor pack their data in ways the launcher does not read";

pub type Key = [u8; KEY_BYTES];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The file's path in the archive, with `/` between folders.
    pub name: String,
    offset: u64,
    size: u64,
    packed: Option<u64>,
}

/// How an archive of one version is laid out and keyed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Format {
    version: u16,
}

impl Format {
    /// The bytes of a number in the header and the tables.
    fn width(self) -> usize {
        if self.version >= 6 { 8 } else { 4 }
    }

    fn has_code_page(self) -> bool {
        self.version >= 4
    }

    /// Where the key starts for the tables, which lie at `start`, and for
    /// a file of `size` bytes unpacked whose data lies at `place`.
    fn tables_key(self, start: u64) -> u64 {
        if self.keys_by_place() { start } else { 0 }
    }

    fn file_key(self, place: u64, size: u64) -> u64 {
        if self.keys_by_place() { place } else { size }
    }

    /// The magic, the version and the tables' size, then the numbers.
    fn header_bytes(self) -> usize {
        8 + (4 + usize::from(self.has_code_page())) * self.width()
    }

    /// A file's name, attributes, times, address, size and packed size.
    fn file_entry_bytes(self) -> usize {
        5 * self.width() + TIMES_BYTES
    }

    /// A folder's own entry, its parent, and how many entries it has from
    /// which one on.
    fn folder_entry_bytes(self) -> usize {
        4 * self.width()
    }

    fn attributes(self) -> usize {
        self.width()
    }

    fn address(self) -> usize {
        2 * self.width() + TIMES_BYTES
    }

    fn size(self) -> usize {
        3 * self.width() + TIMES_BYTES
    }

    fn packed(self) -> usize {
        4 * self.width() + TIMES_BYTES
    }

    /// What stands for "none" in a number: a file not packed, a folder
    /// without a parent.
    fn none(self) -> u64 {
        u64::MAX >> (64 - 8 * self.width())
    }

    /// Before version 5 the key falls on every byte by its place in the
    /// archive.
    fn keys_by_place(self) -> bool {
        self.version < 5
    }

    fn number(self, bytes: &[u8], at: usize) -> Option<u64> {
        let bytes = bytes.get(at..at.checked_add(self.width())?)?;
        let mut whole = [0; 8];
        whole[..bytes.len()].copy_from_slice(bytes);
        Some(u64::from_le_bytes(whole))
    }

    fn put(self, bytes: &mut [u8], at: usize, number: u64) {
        bytes[at..at + self.width()].copy_from_slice(&number.to_le_bytes()[..self.width()]);
    }

    fn push(self, bytes: &mut Vec<u8>, number: u64) {
        bytes.extend_from_slice(&number.to_le_bytes()[..self.width()]);
    }
}

#[derive(Clone, Debug)]
pub struct Archive {
    format: Format,
    key: Key,
    data_start: u64,
    data_bytes: u64,
    code_page: u64,
    tables: Vec<u8>,
    files_at: usize,
    folders_at: usize,
    entries: Vec<Entry>,
}

impl Archive {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut start = Vec::with_capacity(LONGEST_HEADER);
        (&mut file)
            .take(LONGEST_HEADER as u64)
            .read_to_end(&mut start)?;
        for version in VERSIONS {
            let format = Format { version };
            match Self::open_as(format, &mut file, length, &start) {
                Ok(archive) => return Ok(archive),
                // A version 6 header has enough known bytes to tell a
                // damaged archive; the older ones are taken only whole.
                Err(error) if format.width() == 8 && error.kind() != io::ErrorKind::Unsupported => {
                    return Err(error);
                }
                Err(_) => {}
            }
        }
        Err(unsupported(UNSUPPORTED))
    }

    fn open_as(format: Format, file: &mut File, length: u64, start: &[u8]) -> io::Result<Self> {
        let other = || unsupported(UNSUPPORTED);
        let mut header = start
            .get(..format.header_bytes())
            .ok_or_else(other)?
            .to_vec();
        let key = derive_key(format, &header, length);
        crypt(&mut header, &key, 0);
        let width = format.width();
        let number = |index: usize| {
            format
                .number(&header, 8 + index * width)
                .unwrap_or(u64::MAX)
        };
        let tables_bytes = u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap()));
        let (data_start, tables_start, files_at, folders_at) =
            (number(0), number(1), number(2), number(3));
        let code_page = if format.has_code_page() { number(4) } else { 0 };
        // The key came from the bytes that are known ahead; the ones it did
        // not come from must fit them.
        let known = header[..2] == MAGIC
            && header[2..4] == format.version.to_le_bytes()
            && data_start == format.header_bytes() as u64
            && [tables_start, files_at, folders_at, code_page]
                .iter()
                .all(|&number| number <= u64::from(u32::MAX))
            && tables_bytes <= LARGEST_TABLES
            && tables_start.checked_add(tables_bytes) == Some(length)
            && tables_start >= data_start
            && files_at <= folders_at
            && folders_at + format.folder_entry_bytes() as u64 <= tables_bytes;
        if !known {
            return Err(other());
        }
        let mut tables = vec![0; tables_bytes as usize];
        file.seek(SeekFrom::Start(tables_start))?;
        file.read_exact(&mut tables)?;
        crypt(&mut tables, &key, format.tables_key(tables_start));
        let mut archive = Self {
            format,
            key,
            data_start,
            data_bytes: tables_start - data_start,
            code_page,
            tables,
            files_at: files_at as usize,
            folders_at: folders_at as usize,
            entries: Vec::new(),
        };
        let mut entries = Vec::new();
        archive.tables().folder(0, "", 0, &mut entries)?;
        archive.entries = entries;
        Ok(archive)
    }

    fn tables(&self) -> Tables<'_> {
        Tables {
            format: self.format,
            bytes: &self.tables,
            files_at: self.files_at,
            folders_at: self.folders_at,
            data_bytes: self.data_bytes,
        }
    }

    #[cfg(test)]
    pub fn key(&self) -> Key {
        self.key
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn read(&self, path: &Path, entry: &Entry) -> io::Result<Vec<u8>> {
        if entry.size > LARGEST_FILE {
            return Err(invalid(format!("{} is too large to read", entry.name)));
        }
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(self.data_start + entry.offset))?;
        let mut stored = vec![0; entry.packed.unwrap_or(entry.size) as usize];
        file.read_exact(&mut stored)?;
        let place = self.data_start + entry.offset;
        crypt(
            &mut stored,
            &self.key,
            self.format.file_key(place, entry.size),
        );
        match entry.packed {
            Some(_) => unpack(&stored, entry.size as usize),
            None => Ok(stored),
        }
    }

    /// Writes the folder `name` of the archive's top level as an archive
    /// of its own at `to`, with the same key. Names, times and every
    /// file's bytes are copied as the archive stores them, so nothing is
    /// unpacked and the new archive is as large as the folder was. From
    /// version 5 on a file is XORed from where its size falls on the key
    /// wherever it lies; before, from where it lies, so there a file is
    /// put where the key falls on it as it did.
    pub fn write_folder(&self, source: &Path, name: &str, to: &Path) -> io::Result<()> {
        let format = self.format;
        let tables = self.tables();
        let mut part = Part::new(format);
        part.folder(&tables, tables.top_folder(name)?, 0, format.none(), 0)?;
        let files_at = part.names.len() as u64;
        let folders_at = files_at + part.files.len() as u64;
        let mut joined = [part.names, part.files, part.folders].concat();
        let tables_start = format.header_bytes() as u64 + part.data_bytes;
        if tables_start + joined.len() as u64 > format.none() {
            return Err(invalid(
                "the folder is too large for an archive of this kind",
            ));
        }
        crypt(&mut joined, &self.key, format.tables_key(tables_start));

        let mut from = File::open(source)?;
        let mut out = BufWriter::with_capacity(COPY_BUFFER, File::create(to)?);
        out.write_all(&header(
            format,
            &self.key,
            joined.len(),
            part.data_bytes,
            files_at,
            folders_at,
            self.code_page,
        ))?;
        let mut written = 0;
        for copy in part.copies {
            io::copy(&mut io::repeat(0).take(copy.to - written), &mut out)?;
            from.seek(SeekFrom::Start(self.data_start + copy.from))?;
            if io::copy(&mut (&mut from).take(copy.bytes), &mut out)? != copy.bytes {
                return Err(invalid("the archive ends inside a file"));
            }
            written = copy.to + copy.bytes;
        }
        io::copy(&mut io::repeat(0).take(part.data_bytes - written), &mut out)?;
        out.write_all(&joined)?;
        // The game's own archive moves aside once this one is in place.
        out.into_inner()
            .map_err(io::IntoInnerError::into_error)?
            .sync_all()
    }
}

/// The header of an archive whose data and tables follow it, XORed.
fn header(
    format: Format,
    key: &Key,
    tables_bytes: usize,
    data_bytes: u64,
    files_at: u64,
    folders_at: u64,
    code_page: u64,
) -> Vec<u8> {
    let start = format.header_bytes() as u64;
    let mut header = Vec::with_capacity(format.header_bytes());
    header.extend_from_slice(&MAGIC);
    header.extend_from_slice(&format.version.to_le_bytes());
    header.extend_from_slice(&(tables_bytes as u32).to_le_bytes());
    for number in [start, start + data_bytes, files_at, folders_at] {
        format.push(&mut header, number);
    }
    if format.has_code_page() {
        format.push(&mut header, code_page);
    }
    crypt(&mut header, key, 0);
    header
}

/// A file's stored bytes in the archive they are copied from, and where
/// they go in the data of the one being written.
struct Copy {
    from: u64,
    bytes: u64,
    to: u64,
}

/// The tables of an archive written from one folder of another.
struct Part {
    format: Format,
    names: Vec<u8>,
    files: Vec<u8>,
    folders: Vec<u8>,
    copies: Vec<Copy>,
    data_bytes: u64,
}

impl Part {
    /// Empty but for the top folder's own entry and its empty name.
    fn new(format: Format) -> Self {
        let mut files = vec![0; format.file_entry_bytes()];
        format.put(&mut files, format.attributes(), FOLDER);
        format.put(&mut files, format.packed(), format.none());
        Self {
            format,
            names: vec![0; 4],
            files,
            folders: Vec::new(),
            copies: Vec::new(),
            data_bytes: 0,
        }
    }

    /// Lays out the folder at `source_folder` of the other archive's
    /// folder table, whose own entry here is at `own`; returns its place
    /// in this folder table.
    fn folder(
        &mut self,
        source: &Tables<'_>,
        source_folder: usize,
        own: u64,
        parent: u64,
        depth: usize,
    ) -> io::Result<u64> {
        let format = self.format;
        let entry_bytes = format.file_entry_bytes();
        let at = self.folders.len() as u64;
        let (count, source_first) = source.folder_entries(source_folder, depth)?;
        let first = self.files.len();
        // Every entry written is one the other archive's tables hold.
        if first + count * entry_bytes > source.bytes.len() {
            return Err(invalid("the archive's folders hold each other"));
        }
        self.files.resize(first + count * entry_bytes, 0);
        for number in [own, parent, count as u64, first as u64] {
            format.push(&mut self.folders, number);
        }
        for index in 0..count {
            let from = source_first + index * entry_bytes;
            let slot = first + index * entry_bytes;
            let mut entry = source
                .bytes
                .get(from..from + entry_bytes)
                .ok_or_else(|| invalid("the archive's tables end inside an entry"))?
                .to_vec();
            let name = self.names.len() as u64;
            self.names
                .extend_from_slice(source.name_record(source.position(0, from)?)?);
            let (address, size, packed) = (
                source.number(from + format.address())?,
                source.number(from + format.size())?,
                source.number(from + format.packed())?,
            );
            let address = if source.number(from + format.attributes())? & FOLDER != 0 {
                let inner = usize::try_from(address)
                    .map_err(|_| invalid("the archive's tables point outside themselves"))?;
                self.folder(source, inner, slot as u64, at, depth + 1)?
            } else {
                let bytes = if packed == format.none() {
                    size
                } else {
                    packed
                };
                if address
                    .checked_add(bytes)
                    .is_none_or(|end| end > source.data_bytes)
                {
                    return Err(invalid("a file lies outside the archive"));
                }
                // Both archives' data starts as far into them, so the key
                // falls on a file as before where its address is as far
                // into the key.
                let to = match format.keys_by_place() {
                    true => {
                        let key = KEY_BYTES as u64;
                        self.data_bytes + (address % key + key - self.data_bytes % key) % key
                    }
                    false => self.data_bytes,
                };
                self.copies.push(Copy {
                    from: address,
                    bytes,
                    to,
                });
                self.data_bytes = (to + bytes).next_multiple_of(DATA_ALIGNMENT);
                to
            };
            format.put(&mut entry, 0, name);
            format.put(&mut entry, format.address(), address);
            self.files[slot..slot + entry_bytes].copy_from_slice(&entry);
        }
        Ok(at)
    }
}

/// The key, from what a header of this version must hold: its magic and
/// version, where the data starts, and then either the empty upper half of
/// a number or the size of the tables, which is what is left of the
/// archive after the place the header gives for them.
fn derive_key(format: Format, header: &[u8], length: u64) -> Key {
    let mut key = [0; KEY_BYTES];
    let known = |key: &mut Key, at: usize, plain: &[u8]| {
        for (at, plain) in (at..).zip(plain) {
            key[at % KEY_BYTES] = header[at] ^ plain;
        }
    };
    known(&mut key, 0, &MAGIC);
    known(&mut key, 2, &format.version.to_le_bytes());
    known(&mut key, 8, &(format.header_bytes() as u32).to_le_bytes());
    if format.width() == 8 {
        // The upper half of the file table's position.
        known(&mut key, 28, &[0; 4]);
    } else {
        let tables_start = (0..4).fold(0, |start, at| {
            start | u64::from(header[12 + at] ^ key[at]) << (8 * at)
        });
        let tables_bytes = length.wrapping_sub(tables_start) as u32;
        known(&mut key, 4, &tables_bytes.to_le_bytes());
    }
    key
}

fn crypt(bytes: &mut [u8], key: &Key, position: u64) {
    let start = (position % KEY_BYTES as u64) as usize;
    for (at, byte) in bytes.iter_mut().enumerate() {
        *byte ^= key[(start + at) % KEY_BYTES];
    }
}

struct Tables<'a> {
    format: Format,
    bytes: &'a [u8],
    files_at: usize,
    folders_at: usize,
    data_bytes: u64,
}

impl Tables<'_> {
    fn number(&self, at: usize) -> io::Result<u64> {
        self.format
            .number(self.bytes, at)
            .ok_or_else(|| invalid("the archive's tables end inside an entry"))
    }

    fn position(&self, base: usize, at: usize) -> io::Result<usize> {
        usize::try_from(self.number(at)?)
            .ok()
            .and_then(|relative| base.checked_add(relative))
            .ok_or_else(|| invalid("the archive's tables point outside themselves"))
    }

    /// A name's whole record: its length in `units` groups of four bytes,
    /// a check sum, an upper-case copy of the name and the name as the
    /// game wrote it.
    fn name_record(&self, at: usize) -> io::Result<&[u8]> {
        let ends_inside = || invalid("the archive's name table ends inside a name");
        let units = self
            .bytes
            .get(at..at.saturating_add(2))
            .map(|bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]])))
            .ok_or_else(ends_inside)?;
        self.bytes
            .get(at..at + 4 + units * 8)
            .ok_or_else(ends_inside)
    }

    fn name(&self, at: usize) -> io::Result<String> {
        let record = self.name_record(at)?;
        let name = &record[4 + (record.len() - 4) / 2..];
        let name = name.split(|&byte| byte == 0).next().unwrap_or_default();
        let (name, _, wrong) = encoding_rs::SHIFT_JIS.decode(name);
        if wrong || name.contains(['/', '\\']) || name == ".." || name == "." {
            return Err(invalid(
                "the archive holds a file name the launcher cannot use",
            ));
        }
        Ok(name.into_owned())
    }

    fn folder(
        &self,
        at: usize,
        prefix: &str,
        depth: usize,
        entries: &mut Vec<Entry>,
    ) -> io::Result<()> {
        let format = self.format;
        let (count, first) = self.folder_entries(at, depth)?;
        // Folders that hold each other would list files without end.
        if entries.len() + count > self.bytes.len() / format.file_entry_bytes() {
            return Err(invalid("the archive lists more files than its tables hold"));
        }
        for index in 0..count {
            let entry = first + index * format.file_entry_bytes();
            let name = self.name(self.position(0, entry)?)?;
            let attributes = self.number(entry + format.attributes())?;
            let (address, size, packed) = (
                self.number(entry + format.address())?,
                self.number(entry + format.size())?,
                self.number(entry + format.packed())?,
            );
            if attributes & FOLDER != 0 {
                let address = usize::try_from(address)
                    .map_err(|_| invalid("the archive's tables point outside themselves"))?;
                self.folder(address, &format!("{prefix}{name}/"), depth + 1, entries)?;
                continue;
            }
            let packed = (packed != format.none()).then_some(packed);
            let stored = packed.unwrap_or(size);
            if address
                .checked_add(stored)
                .is_none_or(|end| end > self.data_bytes)
            {
                return Err(invalid(format!("{prefix}{name} lies outside the archive")));
            }
            entries.push(Entry {
                name: format!("{prefix}{name}"),
                offset: address,
                size,
                packed,
            });
        }
        Ok(())
    }
}

impl Tables<'_> {
    /// How many entries the folder at `at` of the folder table has, and
    /// where in the tables its first one is.
    fn folder_entries(&self, at: usize, depth: usize) -> io::Result<(usize, usize)> {
        if depth > DEEPEST_FOLDER {
            return Err(invalid("the archive's folders are nested without end"));
        }
        let width = self.format.width();
        let folder = self
            .folders_at
            .checked_add(at)
            .ok_or_else(|| invalid("the archive's tables point outside themselves"))?;
        let count = self.number(folder + 2 * width)? as usize;
        let first = self.position(self.files_at, folder + 3 * width)?;
        if count > self.bytes.len() / self.format.file_entry_bytes() {
            return Err(invalid("the archive lists more files than its tables hold"));
        }
        Ok((count, first))
    }

    /// Where in the folder table the folder `name` of the top level is.
    fn top_folder(&self, name: &str) -> io::Result<usize> {
        let format = self.format;
        let (count, first) = self.folder_entries(0, 0)?;
        for index in 0..count {
            let entry = first + index * format.file_entry_bytes();
            if self.number(entry + format.attributes())? & FOLDER != 0
                && self.name(self.position(0, entry)?)? == name
            {
                return usize::try_from(self.number(entry + format.address())?)
                    .map_err(|_| invalid("the archive's tables point outside themselves"));
            }
        }
        Err(invalid(format!("the archive has no folder {name}")))
    }
}

/// DxLib's LZ: the unpacked and packed sizes, the byte that marks a
/// reference, then literal bytes and references (a length and a distance
/// back, in one to three bytes).
fn unpack(packed: &[u8], size: usize) -> io::Result<Vec<u8>> {
    let damaged = || invalid("a packed file in the archive is damaged");
    if packed.len() < 9 {
        return Err(damaged());
    }
    let unpacked = u32::from_le_bytes(packed[..4].try_into().unwrap()) as usize;
    let end = u32::from_le_bytes(packed[4..8].try_into().unwrap()) as usize;
    let mark = packed[8];
    if unpacked != size || end > packed.len() {
        return Err(damaged());
    }
    let mut out = Vec::with_capacity(size);
    let mut at = 9;
    let byte = |at: usize| packed[..end].get(at).copied().ok_or_else(damaged);
    while at < end {
        let value = byte(at)?;
        if value != mark {
            out.push(value);
            at += 1;
            continue;
        }
        let mut code = usize::from(byte(at + 1)?);
        if code == usize::from(mark) {
            out.push(mark);
            at += 2;
            continue;
        }
        if code > usize::from(mark) {
            code -= 1;
        }
        at += 2;
        let mut run = code >> 3;
        if code & 4 != 0 {
            run |= usize::from(byte(at)?) << 5;
            at += 1;
        }
        run += 4;
        let mut distance = 0;
        for shift in 0..(code & 3).min(2) + 1 {
            distance |= usize::from(byte(at)?) << (8 * shift);
            at += 1;
        }
        distance += 1;
        if distance > out.len() || out.len() + run > size {
            return Err(damaged());
        }
        for _ in 0..run {
            out.push(out[out.len() - distance]);
        }
    }
    if out.len() == size {
        Ok(out)
    } else {
        Err(damaged())
    }
}

/// Builds a version 6 archive with the files stored unpacked, in one
/// folder level per `/` of their names, XORed with `key`.
#[cfg(test)]
pub fn build(key: Key, files: &[(&str, &[u8])]) -> Vec<u8> {
    build_as(6, key, files)
}

#[cfg(test)]
pub fn build_as(version: u16, key: Key, files: &[(&str, &[u8])]) -> Vec<u8> {
    let files = files
        .iter()
        .map(|(name, bytes)| (*name, *bytes, None))
        .collect::<Vec<_>>();
    build_stored(version, key, &files)
}

/// Builds an archive from files as they are stored: one given with a size
/// is packed already, and that is its size unpacked.
#[cfg(test)]
fn build_stored(version: u16, key: Key, files: &[(&str, &[u8], Option<usize>)]) -> Vec<u8> {
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Folder<'a> {
        files: Vec<(&'a str, &'a [u8], Option<usize>)>,
        folders: BTreeMap<&'a str, Folder<'a>>,
    }
    let mut root = Folder::default();
    for (name, bytes, unpacked) in files {
        let mut folder = &mut root;
        let mut parts = name.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                folder = folder.folders.entry(part).or_default();
            } else {
                folder.files.push((part, bytes, *unpacked));
            }
        }
    }

    struct Tables {
        format: Format,
        names: Vec<u8>,
        files: Vec<u8>,
        folders: Vec<u8>,
        data: Vec<u8>,
        key: Key,
    }
    impl Tables {
        fn name(&mut self, name: &str) -> u64 {
            let at = self.names.len() as u64;
            let (encoded, _, _) = encoding_rs::SHIFT_JIS.encode(name);
            let units = encoded.len() / 4 + 1;
            let upper = encoded.to_ascii_uppercase();
            let parity = upper.iter().map(|&byte| u32::from(byte)).sum::<u32>() as u16;
            self.names.extend_from_slice(&(units as u16).to_le_bytes());
            self.names.extend_from_slice(&parity.to_le_bytes());
            for copy in [&upper[..], &encoded[..]] {
                self.names.extend_from_slice(copy);
                self.names
                    .resize(self.names.len() + units * 4 - copy.len(), 0);
            }
            at
        }

        fn entry(&mut self, at: usize, name: u64, attributes: u64, fields: [u64; 3]) {
            let format = self.format;
            let mut entry = Vec::with_capacity(format.file_entry_bytes());
            format.push(&mut entry, name);
            format.push(&mut entry, attributes);
            entry.resize(entry.len() + TIMES_BYTES, 0);
            for field in fields {
                format.push(&mut entry, field);
            }
            self.files[at..at + entry.len()].copy_from_slice(&entry);
        }

        /// Lays out a folder whose own entry is at `own`; returns its place
        /// in the folder table.
        fn folder(&mut self, folder: &Folder<'_>, own: u64, parent: u64) -> u64 {
            let format = self.format;
            let at = self.folders.len() as u64;
            let count = folder.files.len() + folder.folders.len();
            let first = self.files.len();
            self.files
                .resize(first + count * format.file_entry_bytes(), 0);
            for number in [own, parent, count as u64, first as u64] {
                format.push(&mut self.folders, number);
            }
            let mut slot = first;
            for (name, bytes, unpacked) in &folder.files {
                let name = self.name(name);
                // As DxLib does, on a multiple of four bytes.
                self.data.resize(self.data.len().next_multiple_of(4), 0);
                let address = self.data.len() as u64;
                let size = unpacked.unwrap_or(bytes.len()) as u64;
                let packed = unpacked.map_or(format.none(), |_| bytes.len() as u64);
                let mut stored = bytes.to_vec();
                let place = format.header_bytes() as u64 + address;
                crypt(&mut stored, &self.key, format.file_key(place, size));
                self.data.extend_from_slice(&stored);
                self.entry(slot, name, 0x20, [address, size, packed]);
                slot += format.file_entry_bytes();
            }
            for (name, inner) in &folder.folders {
                let name = self.name(name);
                let inner = self.folder(inner, slot as u64, at);
                self.entry(slot, name, FOLDER, [inner, 0, format.none()]);
                slot += format.file_entry_bytes();
            }
            at
        }
    }
    let format = Format { version };
    let mut tables = Tables {
        format,
        names: vec![0; 4],
        files: vec![0; format.file_entry_bytes()],
        folders: Vec::new(),
        data: Vec::new(),
        key,
    };
    tables.entry(0, 0, FOLDER, [0, 0, format.none()]);
    tables.folder(&root, 0, format.none());

    let files_at = tables.names.len() as u64;
    let folders_at = files_at + tables.files.len() as u64;
    let mut joined = [tables.names, tables.files, tables.folders].concat();
    let header = header(
        format,
        &key,
        joined.len(),
        tables.data.len() as u64,
        files_at,
        folders_at,
        932,
    );
    let tables_start = (header.len() + tables.data.len()) as u64;
    crypt(&mut joined, &key, format.tables_key(tables_start));
    [&header[..], &tables.data, &joined].concat()
}

/// Packs bytes the way DxLib's LZ reader expects, using references only
/// for runs of one repeated byte.
#[cfg(test)]
pub fn pack(bytes: &[u8]) -> Vec<u8> {
    const MARK: u8 = 0xfe;
    let mut body = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let value = bytes[at];
        let run = bytes[at..]
            .iter()
            .take_while(|&&byte| byte == value)
            .count();
        if at > 0 && bytes[at - 1] == value && run >= 4 {
            let run = run.min(4 + 31);
            // Length in the code's upper five bits, a one-byte distance.
            let code = ((run - 4) << 3) as u8;
            body.push(MARK);
            body.push(if code >= MARK { code + 1 } else { code });
            body.push(0);
            at += run;
            continue;
        }
        body.push(value);
        if value == MARK {
            body.push(MARK);
        }
        at += 1;
    }
    let mut packed = Vec::with_capacity(body.len() + 9);
    packed.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    packed.extend_from_slice(&((body.len() + 9) as u32).to_le_bytes());
    packed.push(MARK);
    packed.extend_from_slice(&body);
    packed
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const KEY: Key = [
        0xc7, 0x05, 0xca, 0x7d, 0x8d, 0xe3, 0xde, 0xf1, 0xd9, 0x0c, 0x85, 0xf4,
    ];

    const HEADER_BYTES: usize = 48;
    /// Where the tables of a version 6 archive with one file of an 8 to
    /// 11 letter name keep its entry: after the root's empty name, the
    /// file's name (two copies of three groups of four bytes) and the
    /// root's entry.
    const FIRST_FILE_ENTRY: usize = 4 + (4 + 2 * 12) + 64;

    fn written(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("BasicData.wolf");
        fs::write(&path, bytes).unwrap();
        (directory, path)
    }

    #[test]
    fn finds_the_key_in_the_header_and_reads_every_file_back() {
        let long = (0..5000).map(|at| (at % 251) as u8).collect::<Vec<_>>();
        let files: [(&str, &[u8]); 4] = [
            ("Game.dat", b"title"),
            ("マップ.mps", &long),
            ("Sub/Inner/deep.txt", b""),
            ("Sub/other.bin", &[0xfe, 0xfe, 0x00]),
        ];
        let (_directory, path) = written(&build(KEY, &files));
        let archive = Archive::open(&path).unwrap();
        assert_eq!(archive.key(), KEY);
        let mut names = archive
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "Game.dat",
                "Sub/Inner/deep.txt",
                "Sub/other.bin",
                "マップ.mps"
            ]
        );
        for (name, bytes) in files {
            let entry = archive
                .entries()
                .iter()
                .find(|entry| entry.name == name)
                .unwrap();
            assert_eq!(archive.read(&path, entry).unwrap(), bytes);
        }
    }

    #[test]
    fn unpacks_files_packed_with_dxlibs_lz() {
        let mut plain = b"abc".to_vec();
        plain.extend_from_slice(&[0x41; 300]);
        plain.extend_from_slice(&[0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0xfe, 0x01]);
        plain.extend_from_slice(&[0; 9]);
        let packed = pack(&plain);
        assert!(packed.len() < plain.len());
        assert_eq!(unpack(&packed, plain.len()).unwrap(), plain);

        // In an archive: the entry's packed size is set and the data is
        // XORed from where the unpacked size falls on the key.
        let archive = build_stored(6, KEY, &[("Packed.dat", &packed, Some(plain.len()))]);
        let (_directory, path) = written(&archive);
        let opened = Archive::open(&path).unwrap();
        assert_eq!(opened.read(&path, &opened.entries()[0]).unwrap(), plain);
    }

    #[test]
    fn writes_a_folder_of_an_archive_as_an_archive_of_its_own() {
        let long = (0..5000).map(|at| (at % 251) as u8).collect::<Vec<_>>();
        let mut plain = b"abc".to_vec();
        plain.extend_from_slice(&[0x41; 300]);
        let packed = pack(&plain);
        let files: [(&str, &[u8], Option<usize>); 7] = [
            ("BasicData/Game.dat", b"title", None),
            ("Picture/title.png", b"a title", None),
            ("Picture/Faces/主人公.png", &long, None),
            ("Picture/Faces/Small/empty.png", b"", None),
            ("Picture/packed.png", &packed, Some(plain.len())),
            ("Sound/town.ogg", b"a tune", None),
            ("key.csv", b"a,b", None),
        ];
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Data.wolf");
        fs::write(&source, build_stored(6, KEY, &files)).unwrap();
        let whole = Archive::open(&source).unwrap();

        let path = directory.path().join("Picture.wolf");
        whole.write_folder(&source, "Picture", &path).unwrap();
        let part = Archive::open(&path).unwrap();
        assert_eq!(part.key(), KEY);
        let read = |archive: &Archive, from: &Path, inside: &str| {
            archive
                .entries()
                .iter()
                .filter_map(|entry| {
                    let name = entry.name.strip_prefix(inside)?.to_owned();
                    Some((name, entry.packed, archive.read(from, entry).unwrap()))
                })
                .collect::<Vec<_>>()
        };
        // The same files in the same order, a packed one still packed.
        let written = read(&part, &path, "");
        assert_eq!(written, read(&whole, &source, "Picture/"));
        assert_eq!(
            written
                .iter()
                .map(|(name, packed, _)| (name.as_str(), packed.is_some()))
                .collect::<Vec<_>>(),
            [
                ("title.png", false),
                ("packed.png", true),
                ("Faces/主人公.png", false),
                ("Faces/Small/empty.png", false),
            ]
        );
        assert_eq!(written[1].2, plain);
        // Each file's data starts on a multiple of four bytes, as DxLib's.
        assert!(part.entries().iter().all(|entry| entry.offset % 4 == 0));

        let error = whole.write_folder(&source, "key.csv", &path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let error = whole.write_folder(&source, "Voice", &path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn reads_the_older_layouts_and_writes_a_folder_of_one_as_its_own_archive() {
        let long = (0..5000).map(|at| (at % 251) as u8).collect::<Vec<_>>();
        let mut plain = b"abc".to_vec();
        plain.extend_from_slice(&[0x41; 300]);
        let packed = pack(&plain);
        let files: [(&str, &[u8], Option<usize>); 7] = [
            ("BasicData/Game.dat", b"title", None),
            ("Picture/title.png", b"a title", None),
            ("Picture/Faces/主人公.png", &long, None),
            ("Picture/Faces/Small/empty.png", b"", None),
            ("Picture/packed.png", &packed, Some(plain.len())),
            ("Sound/town.ogg", b"a tune", None),
            ("key.csv", b"a,b", None),
        ];
        let read = |archive: &Archive, from: &Path, inside: &str| {
            archive
                .entries()
                .iter()
                .filter_map(|entry| {
                    let name = entry.name.strip_prefix(inside)?.to_owned();
                    Some((name, entry.packed, archive.read(from, entry).unwrap()))
                })
                .collect::<Vec<_>>()
        };
        for (version, header_bytes) in [(3, 24), (4, 28), (5, 28)] {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("Data.wolf");
            let bytes = build_stored(version, KEY, &files);
            fs::write(&source, &bytes).unwrap();
            let whole = Archive::open(&source).unwrap();
            assert_eq!(whole.format, Format { version });
            assert_eq!(whole.key(), KEY);
            assert_eq!(whole.data_start, header_bytes);
            let all = read(&whole, &source, "");
            assert_eq!(all.len(), files.len());
            for (name, bytes, unpacked) in files {
                let (_, packed, read) = all.iter().find(|(read, ..)| read == name).unwrap();
                assert_eq!(packed.is_some(), unpacked.is_some(), "{name}");
                assert_eq!(
                    read,
                    if unpacked.is_some() {
                        &plain[..]
                    } else {
                        bytes
                    }
                );
            }

            let path = directory.path().join("Picture.wolf");
            whole.write_folder(&source, "Picture", &path).unwrap();
            let part = Archive::open(&path).unwrap();
            assert_eq!((part.format, part.key()), (whole.format, KEY));
            let written = read(&part, &path, "");
            assert_eq!(written, read(&whole, &source, "Picture/"));
            assert_eq!(written.len(), 4);
            // The bytes are the ones the first archive stores: before
            // version 5 a file lies where the key falls on it as it did.
            let stored = |archive: &Archive, from: &Path, name: &str| {
                let entry = archive
                    .entries()
                    .iter()
                    .find(|entry| entry.name == name)
                    .unwrap();
                let place = (archive.data_start + entry.offset) as usize;
                let length = entry.packed.unwrap_or(entry.size) as usize;
                (
                    place,
                    fs::read(from).unwrap()[place..place + length].to_vec(),
                )
            };
            let (was_at, was) = stored(&whole, &source, "Picture/Faces/主人公.png");
            let (now_at, now) = stored(&part, &path, "Faces/主人公.png");
            assert_eq!(was, now);
            assert_ne!(was_at, now_at);
            assert!(version >= 5 || was_at % 12 == now_at % 12, "{version}");
            assert!(part.entries().iter().all(|entry| entry.offset % 4 == 0));

            // The same bytes are no archive of the next version.
            let mut other = bytes.clone();
            other[2] ^= 0x01;
            fs::write(&source, &other).unwrap();
            assert_eq!(
                Archive::open(&source).unwrap_err().kind(),
                io::ErrorKind::Unsupported
            );
        }
    }

    #[test]
    fn refuses_archives_it_does_not_recognise() {
        let good = build(KEY, &[("Game.dat", b"title")]);
        let refused = |bytes: &[u8]| {
            let (_directory, path) = written(bytes);
            let error = Archive::open(&path).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Unsupported);
            assert_eq!(error.to_string(), UNSUPPORTED);
        };
        // DXA version 8, whose header is not XORed (WOLF RPG Editor 2.25 on).
        let mut newer = vec![0; 64];
        newer[..4].copy_from_slice(b"DX\x08\x00");
        refused(&newer);
        // A key that does not repeat every 12 bytes (a string key).
        let mut other_key = good.clone();
        other_key[44] ^= 0x5a;
        refused(&other_key);
        // A file that is no archive at all, and one cut short.
        refused(b"RGSSAD\0\x01 not a WOLF archive, but long enough for a header");
        refused(&good[..20]);
        let mut cut = good.clone();
        cut.truncate(good.len() - 8);
        refused(&cut);
    }

    #[test]
    fn stops_at_damaged_tables_and_packed_data() {
        let mut archive = build(KEY, &[("Game.dat", b"title")]);
        let tables_start = HEADER_BYTES + 5;
        let size_at = tables_start + FIRST_FILE_ENTRY + 48;
        archive[size_at + 3] ^= 0x7f;
        let (_directory, path) = written(&archive);
        assert_eq!(
            Archive::open(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(unpack(&[4, 0, 0, 0, 12, 0, 0, 0, 0xfe, 0xfe, 0x00, 0x09], 4).is_err());
        assert!(unpack(&[1, 0, 0, 0], 1).is_err());
    }
}
