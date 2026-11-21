//! RPG Maker XP's and VX's archive (`Game.rgssad`, `Game.rgss2a`): reading
//! its files, and putting the launcher's copy of the scripts in without
//! moving anything that is already there.
//!
//! The format: `RGSSAD\0\x01`, then one entry after another to the end of
//! the file, with no index: the name's length, the name, the file's size
//! and its bytes. A key starts at `0xDEADCAFE` and steps as `key * 7 + 3`
//! after the length, after each byte of the name and after the size; the
//! length and size are XORed with the key whole, each name byte with its
//! low byte, and the file's bytes four at a time from the key the size left
//! (as the newer archive's files are, `rgss3a::crypt`).
//!
//! Entries cannot be pointed elsewhere, so a patch renames the scripts
//! entry in place (one byte of its name) and appends an entry with the
//! original name holding the new scripts, so the game finds only the copy.
//! The record of the patch is itself an entry, `DLA\patch`, appended before
//! the scripts: the game reads every entry to the end, so nothing may follow
//! the last entry that is not one.

use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::rgss3a::{Patch, crypt, invalid, normal, read_u32, written_hash};

pub const HEADER: &[u8; 8] = b"RGSSAD\0\x01";
const START_KEY: u32 = 0xDEAD_CAFE;
const PATCH_ENTRY: &str = "DLA\\patch";
const TRAILER_MAGIC: &[u8; 16] = b"DLA-RGSSAD-PATCH";
const TRAILER_VERSION: u32 = 1;
// Magic, version, name position, name length, original length, fingerprint;
// the name's bytes follow the length.
const TRAILER_FIXED: usize = 16 + 4 + 8 + 4 + 8 + 32;
const NAME_LIMIT: u32 = 4096;
// The game reads positions as 32-bit numbers.
const LARGEST: u64 = u32::MAX as u64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The name as stored, with `\` between folders (`Data\Map001.rxdata`).
    pub name: String,
    /// Where the name's encrypted bytes are in the archive.
    pub name_at: u64,
    name_key: u32,
    pub offset: u64,
    pub size: u32,
    pub key: u32,
}

pub struct Archive {
    entries: Vec<Entry>,
    /// The key the next entry's length would be XORed with.
    end_key: u32,
    /// Where the last whole entry ends; the file's length unless its tail
    /// is not an entry (a patch stopped halfway).
    end: u64,
    length: u64,
}

impl Archive {
    /// Reads the entries. An archive whose tail is not a whole entry still
    /// opens with the entries before it, so the record of a patch stopped
    /// halfway can be read; [`Archive::is_whole`] tells.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut file = BufReader::new(file);
        let mut header = [0; 8];
        file.read_exact(&mut header)?;
        if &header != HEADER {
            return Err(invalid("not an RPG Maker XP or VX archive"));
        }
        let mut key = START_KEY;
        let mut at = 8_u64;
        let mut entries = Vec::new();
        while at < length {
            let Some(entry) = read_entry(&mut file, at, length, &mut key)? else {
                break;
            };
            at = entry.offset + u64::from(entry.size);
            file.seek_relative(i64::from(entry.size))?;
            entries.push(entry);
        }
        Ok(Self {
            entries,
            end_key: key,
            end: at,
            length,
        })
    }

    pub fn is_whole(&self) -> bool {
        self.end == self.length
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry for a path written with `/` or `\`, in any case; the last
    /// one when the name appears twice.
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        let wanted = normal(name);
        self.entries
            .iter()
            .rev()
            .find(|entry| normal(&entry.name) == wanted)
    }

    pub fn read(&self, path: &Path, entry: &Entry) -> io::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(entry.offset))?;
        let mut bytes = vec![0; entry.size as usize];
        file.read_exact(&mut bytes)?;
        crypt(&mut bytes, entry.key);
        Ok(bytes)
    }
}

/// The entry starting at `at`, or `None` when the bytes there are not a
/// whole entry.
fn read_entry(
    file: &mut BufReader<File>,
    at: u64,
    length: u64,
    key: &mut u32,
) -> io::Result<Option<Entry>> {
    let left = length - at;
    if left < 4 {
        return Ok(None);
    }
    let mut trial = *key;
    let name_length = read_u32(file)? ^ trial;
    trial = step(trial);
    if name_length > NAME_LIMIT || left < 8 + u64::from(name_length) {
        return Ok(None);
    }
    let name_at = at + 4;
    let name_key = trial;
    let mut name = vec![0; name_length as usize];
    file.read_exact(&mut name)?;
    for byte in &mut name {
        *byte ^= trial as u8;
        trial = step(trial);
    }
    let size = read_u32(file)? ^ trial;
    trial = step(trial);
    let offset = name_at + u64::from(name_length) + 4;
    if offset + u64::from(size) > length {
        return Ok(None);
    }
    *key = trial;
    Ok(Some(Entry {
        name: String::from_utf8_lossy(&name).into_owned(),
        name_at,
        name_key,
        offset,
        size,
        key: trial,
    }))
}

fn step(key: u32) -> u32 {
    key.wrapping_mul(7).wrapping_add(3)
}

/// An entry as the archive stores it from `key` on, and the key the entry
/// after it starts with.
fn encode_entry(name: &[u8], data: &[u8], mut key: u32) -> (Vec<u8>, u32) {
    let mut bytes = Vec::with_capacity(8 + name.len() + data.len());
    bytes.extend_from_slice(&(name.len() as u32 ^ key).to_le_bytes());
    key = step(key);
    bytes.extend(encrypt_name(name, &mut key));
    bytes.extend_from_slice(&(data.len() as u32 ^ key).to_le_bytes());
    key = step(key);
    let mut encrypted = data.to_vec();
    crypt(&mut encrypted, key);
    bytes.extend_from_slice(&encrypted);
    (bytes, key)
}

fn encrypt_name(name: &[u8], key: &mut u32) -> Vec<u8> {
    name.iter()
        .map(|byte| {
            let encrypted = byte ^ *key as u8;
            *key = step(*key);
            encrypted
        })
        .collect()
}

/// The launcher's patch in this archive, if any.
pub fn find_patch(path: &Path) -> io::Result<Option<Patch>> {
    let archive = Archive::open(path)?;
    let Some(record) = archive.entry(PATCH_ENTRY) else {
        return Ok(None);
    };
    let trailer = archive.read(path, record)?;
    if trailer.len() < TRAILER_FIXED
        || &trailer[..16] != TRAILER_MAGIC
        || u32::from_le_bytes(trailer[16..20].try_into().unwrap()) != TRAILER_VERSION
    {
        return Ok(None);
    }
    let fields_at = u64::from_le_bytes(trailer[20..28].try_into().unwrap());
    let name_length = u32::from_le_bytes(trailer[28..32].try_into().unwrap()) as usize;
    let original_bytes = u64::from_le_bytes(trailer[32..40].try_into().unwrap());
    let fingerprint = trailer[40..72].try_into().unwrap();
    let Some(original_fields) = trailer.get(72..72 + name_length).map(<[u8]>::to_vec) else {
        return Ok(None);
    };
    if original_bytes >= archive.length
        || original_bytes > record.name_at
        || fields_at + name_length as u64 > original_bytes
    {
        return Ok(None);
    }
    let mut file = File::open(path)?;
    let written_sha256 = written_hash(&mut file, fields_at, name_length, original_bytes)?;
    Ok(Some(Patch {
        fields_at,
        original_fields,
        original_bytes,
        fingerprint,
        written_bytes: archive.length,
        written_sha256,
    }))
}

/// Whether the original scripts entry has been renamed, so the game reads
/// the launcher's copy: only the case once a patch is complete.
pub fn patch_in_use(path: &Path, patch: &Patch) -> io::Result<bool> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(patch.fields_at))?;
    let mut name = vec![0; patch.original_fields.len()];
    file.read_exact(&mut name)?;
    Ok(name != patch.original_fields)
}

/// Gives the scripts entry its name back and cuts the archive to its length
/// before. Leaves an archive without the launcher's record alone (a game
/// update replaced it). Returns whether there was a patch.
pub fn remove_patch(path: &Path) -> io::Result<bool> {
    let Some(patch) = find_patch(path)? else {
        return Ok(false);
    };
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.seek(SeekFrom::Start(patch.fields_at))?;
    file.write_all(&patch.original_fields)?;
    file.sync_all()?;
    file.set_len(patch.original_bytes)?;
    file.sync_all()?;
    Ok(true)
}

/// Appends the record of the patch and then `scripts` under the name of the
/// entry `scripts_name` (`Data/Scripts.rxdata`), and renames that entry so
/// only the copy answers to the name. The record reaches the disk first and
/// the rename last, so a stop halfway leaves either an archive whose record
/// says what to cut off ([`remove_patch`]) or the finished patch.
pub fn add_patch(
    path: &Path,
    scripts_name: &str,
    scripts: &[u8],
    fingerprint: [u8; 32],
) -> io::Result<Patch> {
    let archive = Archive::open(path)?;
    if !archive.is_whole() {
        return Err(invalid("the archive's end is not a whole entry"));
    }
    if archive.entry(PATCH_ENTRY).is_some() {
        return Err(invalid("the archive already holds a patch"));
    }
    let entry = archive.entry(scripts_name).ok_or_else(|| {
        invalid(&format!(
            "the archive has no {}",
            scripts_name.replace('/', "\\")
        ))
    })?;
    let original_bytes = archive.length;
    let name = entry.name.as_bytes();
    if original_bytes
        + 16
        + PATCH_ENTRY.len() as u64
        + TRAILER_FIXED as u64
        + name.len() as u64 * 2
        + scripts.len() as u64
        > LARGEST
    {
        return Err(invalid(
            "the archive is too large to add the translation to",
        ));
    }
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let mut original_fields = vec![0; name.len()];
    file.seek(SeekFrom::Start(entry.name_at))?;
    file.read_exact(&mut original_fields)?;

    let mut trailer = Vec::with_capacity(TRAILER_FIXED + name.len());
    trailer.extend_from_slice(TRAILER_MAGIC);
    trailer.extend_from_slice(&TRAILER_VERSION.to_le_bytes());
    trailer.extend_from_slice(&entry.name_at.to_le_bytes());
    trailer.extend_from_slice(&(name.len() as u32).to_le_bytes());
    trailer.extend_from_slice(&original_bytes.to_le_bytes());
    trailer.extend_from_slice(&fingerprint);
    trailer.extend_from_slice(&original_fields);
    let (record, key) = encode_entry(PATCH_ENTRY.as_bytes(), &trailer, archive.end_key);
    let (copy, _) = encode_entry(name, scripts, key);
    file.seek(SeekFrom::Start(original_bytes))?;
    file.write_all(&record)?;
    file.sync_all()?;
    file.write_all(&copy)?;
    file.sync_all()?;

    let mut renamed = name.to_vec();
    let last = renamed.last_mut().expect("an entry's name is not empty");
    *last = if *last == b'~' { b'_' } else { b'~' };
    let mut name_key = entry.name_key;
    let renamed = encrypt_name(&renamed, &mut name_key);
    file.seek(SeekFrom::Start(entry.name_at))?;
    file.write_all(&renamed)?;
    file.sync_all()?;
    drop(file);
    find_patch(path)?.ok_or_else(|| invalid("the patch could not be read back"))
}

/// Builds archives for tests, in the format RPG Maker writes.
#[cfg(test)]
pub(crate) fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut archive = HEADER.to_vec();
    let mut key = START_KEY;
    for (name, bytes) in files {
        let (entry, next) = encode_entry(name.as_bytes(), bytes, key);
        archive.extend(entry);
        key = next;
    }
    archive
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_with(directory: &Path) -> std::path::PathBuf {
        let path = directory.join("Game.rgssad");
        std::fs::write(
            &path,
            build(&[
                ("Data\\System.rxdata", b"system bytes"),
                ("Data\\Scripts.rxdata", b"the game's scripts"),
                (
                    "Graphics\\Titles\\Title.png",
                    b"\x89PNG picture, longer than a key",
                ),
            ]),
        )
        .unwrap();
        path
    }

    #[test]
    fn reads_every_file_by_either_kind_of_slash() {
        let directory = tempfile::tempdir().unwrap();
        let path = archive_with(directory.path());
        let archive = Archive::open(&path).unwrap();
        assert!(archive.is_whole());
        assert_eq!(archive.entries().len(), 3);
        let scripts = archive.entry("data/scripts.rxdata").unwrap();
        assert_eq!(archive.read(&path, scripts).unwrap(), b"the game's scripts");
        let title = archive.entry("Graphics/Titles/Title.png").unwrap();
        assert_eq!(
            archive.read(&path, title).unwrap(),
            b"\x89PNG picture, longer than a key"
        );
    }

    #[test]
    fn decrypts_as_the_game_does() {
        // The first entry's length, XORed with the start key, then its name
        // byte by byte with the stepping key's low byte.
        let bytes = build(&[("Data\\A", b"x")]);
        let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        assert_eq!(length ^ START_KEY, 6);
        let mut key = step(START_KEY);
        let name = bytes[12..18]
            .iter()
            .map(|byte| {
                let plain = byte ^ key as u8;
                key = step(key);
                plain
            })
            .collect::<Vec<_>>();
        assert_eq!(name, b"Data\\A");
        assert_eq!(
            u32::from_le_bytes(bytes[18..22].try_into().unwrap()) ^ key,
            1
        );
        key = step(key);
        assert_eq!(bytes[22] ^ key as u8, b'x');
    }

    #[test]
    fn a_patch_gives_the_game_the_new_scripts_and_comes_off_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let path = archive_with(directory.path());
        let original = std::fs::read(&path).unwrap();
        assert_eq!(find_patch(&path).unwrap(), None);

        let patch = add_patch(&path, "Data/Scripts.rxdata", b"our scripts first", [7; 32]).unwrap();
        assert_eq!(patch.original_bytes, original.len() as u64);
        assert_eq!(patch.fingerprint, [7; 32]);
        assert!(patch_in_use(&path, &patch).unwrap());
        let archive = Archive::open(&path).unwrap();
        assert!(archive.is_whole());
        let names = archive
            .entries()
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "Data\\System.rxdata",
                "Data\\Scripts.rxdat~",
                "Graphics\\Titles\\Title.png",
                "DLA\\patch",
                "Data\\Scripts.rxdata",
            ]
        );
        let scripts = archive.entry("Data\\Scripts.rxdata").unwrap();
        assert_eq!(archive.read(&path, scripts).unwrap(), b"our scripts first");
        let system = archive.entry("Data\\System.rxdata").unwrap();
        assert_eq!(archive.read(&path, system).unwrap(), b"system bytes");
        let patched = std::fs::read(&path).unwrap();
        let changed = original
            .iter()
            .zip(&patched)
            .filter(|(before, after)| before != after)
            .count();
        assert_eq!(changed, 1);
        assert_eq!(find_patch(&path).unwrap(), Some(patch));

        assert!(remove_patch(&path).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!remove_patch(&path).unwrap());
    }

    #[test]
    fn a_stop_before_the_rename_leaves_a_tail_that_comes_off() {
        let directory = tempfile::tempdir().unwrap();
        let path = archive_with(directory.path());
        let original = std::fs::read(&path).unwrap();
        let patch = add_patch(&path, "Data/Scripts.rxdata", b"our scripts first", [7; 32]).unwrap();
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(patch.fields_at)).unwrap();
        file.write_all(&patch.original_fields).unwrap();
        drop(file);
        let found = find_patch(&path).unwrap().unwrap();
        assert!(!patch_in_use(&path, &found).unwrap());
        assert!(remove_patch(&path).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn a_stop_while_the_scripts_were_written_leaves_a_record_that_cuts_them_off() {
        let directory = tempfile::tempdir().unwrap();
        let path = archive_with(directory.path());
        let original = std::fs::read(&path).unwrap();
        let patch = add_patch(&path, "Data/Scripts.rxdata", b"our scripts first", [7; 32]).unwrap();
        // As if the launcher stopped halfway through the scripts entry,
        // before the rename.
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(patch.fields_at)).unwrap();
        file.write_all(&patch.original_fields).unwrap();
        file.set_len(patch.written_bytes - 6).unwrap();
        drop(file);
        let archive = Archive::open(&path).unwrap();
        assert!(!archive.is_whole());
        assert_eq!(archive.entries().len(), 4);
        assert!(remove_patch(&path).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }

    #[test]
    fn refuses_a_second_patch_and_an_archive_without_scripts() {
        let directory = tempfile::tempdir().unwrap();
        let path = archive_with(directory.path());
        add_patch(&path, "Data/Scripts.rxdata", b"ours", [1; 32]).unwrap();
        assert!(add_patch(&path, "Data/Scripts.rxdata", b"ours", [1; 32]).is_err());
        let other = directory.path().join("Other.rgssad");
        std::fs::write(&other, build(&[("Data\\System.rxdata", b"s")])).unwrap();
        assert!(add_patch(&other, "Data/Scripts.rxdata", b"ours", [1; 32]).is_err());
        assert_eq!(find_patch(&other).unwrap(), None);
    }
}
