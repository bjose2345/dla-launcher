//! RPG Maker VX Ace's archive (`Game.rgss3a`): reading its files, and
//! pointing its scripts entry at a new copy of the scripts appended to its
//! end, so the game runs the launcher's script with its own.
//!
//! The format: `RGSSAD\0\x03`, a key, then one entry per file (offset, size,
//! file key and name length, each XORed with the key, then the name XORed
//! byte by byte), ending with an offset of 0. Each file's bytes are XORed
//! four at a time with its own key, which steps as `key * 7 + 3`.
//!
//! A patch changes only the 12 bytes of the scripts entry's offset, size and
//! key, and ends the file with a trailer that records them and the file's
//! length before, so the original can be put back from the file alone.

use std::{
    fs::{File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::Path,
};

use sha2::{Digest, Sha256};

const HEADER: &[u8; 8] = b"RGSSAD\0\x03";
const TRAILER_MAGIC: &[u8; 16] = b"DLA-RGSS3A-PATCH";
const TRAILER_VERSION: u32 = 1;
// Magic, version, entry position, original entry fields, original length,
// script fingerprint.
const TRAILER_BYTES: u64 = 16 + 4 + 8 + 12 + 8 + 32;
const FIELDS: usize = 12;
const SCRIPTS_KEY: u32 = 0x5EED_1234;
// Entries hold 32-bit offsets.
const LARGEST: u64 = u32::MAX as u64;
const NAME_LIMIT: u32 = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The name as stored, with `\` between folders (`Data\Map001.rvdata2`).
    pub name: String,
    /// Where the entry's offset, size and key are in the archive.
    pub fields_at: u64,
    pub offset: u32,
    pub size: u32,
    pub key: u32,
}

pub struct Archive {
    key: u32,
    entries: Vec<Entry>,
}

impl Archive {
    pub fn open(path: &Path) -> io::Result<Self> {
        let mut file = BufReader::new(File::open(path)?);
        let mut header = [0; 8];
        file.read_exact(&mut header)?;
        if &header != HEADER {
            return Err(invalid("not an RPG Maker VX Ace archive"));
        }
        let key = read_u32(&mut file)?.wrapping_mul(9).wrapping_add(3);
        let length = file.get_ref().metadata()?.len();
        let mut at = 12_u64;
        let mut entries = Vec::new();
        loop {
            let mut fields = [0; 16];
            file.read_exact(&mut fields)?;
            let field = |index: usize| {
                u32::from_le_bytes(fields[index * 4..index * 4 + 4].try_into().unwrap()) ^ key
            };
            let (offset, size, file_key, name_length) = (field(0), field(1), field(2), field(3));
            if offset == 0 {
                break;
            }
            if name_length > NAME_LIMIT || u64::from(offset) + u64::from(size) > length {
                return Err(invalid("the archive's index is damaged"));
            }
            let mut name = vec![0; name_length as usize];
            file.read_exact(&mut name)?;
            for (index, byte) in name.iter_mut().enumerate() {
                *byte ^= key.to_le_bytes()[index % 4];
            }
            entries.push(Entry {
                name: String::from_utf8_lossy(&name).into_owned(),
                fields_at: at,
                offset,
                size,
                key: file_key,
            });
            at += 16 + u64::from(name_length);
        }
        Ok(Self { key, entries })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry for a path written with `/` or `\`, in any case.
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        let wanted = normal(name);
        self.entries
            .iter()
            .find(|entry| normal(&entry.name) == wanted)
    }

    pub fn read(&self, path: &Path, entry: &Entry) -> io::Result<Vec<u8>> {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(u64::from(entry.offset)))?;
        let mut bytes = vec![0; entry.size as usize];
        file.read_exact(&mut bytes)?;
        crypt(&mut bytes, entry.key);
        Ok(bytes)
    }

    fn fields(&self, offset: u32, size: u32, file_key: u32) -> [u8; FIELDS] {
        let mut fields = [0; FIELDS];
        for (index, value) in [offset, size, file_key].into_iter().enumerate() {
            fields[index * 4..index * 4 + 4].copy_from_slice(&(value ^ self.key).to_le_bytes());
        }
        fields
    }
}

/// A patch as the trailer records it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Patch {
    pub fields_at: u64,
    pub original_fields: Vec<u8>,
    pub original_bytes: u64,
    pub fingerprint: [u8; 32],
    /// What the archive is now: its length, and the hash of the entry's
    /// fields as written and everything after `original_bytes`.
    pub written_bytes: u64,
    pub written_sha256: String,
}

/// The launcher's patch in this archive, if any.
pub fn find_patch(path: &Path) -> io::Result<Option<Patch>> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    if length < TRAILER_BYTES {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(length - TRAILER_BYTES))?;
    let mut trailer = [0; TRAILER_BYTES as usize];
    file.read_exact(&mut trailer)?;
    if &trailer[..16] != TRAILER_MAGIC
        || u32::from_le_bytes(trailer[16..20].try_into().unwrap()) != TRAILER_VERSION
    {
        return Ok(None);
    }
    let fields_at = u64::from_le_bytes(trailer[20..28].try_into().unwrap());
    let original_fields = trailer[28..40].to_vec();
    let original_bytes = u64::from_le_bytes(trailer[40..48].try_into().unwrap());
    let fingerprint = trailer[48..80].try_into().unwrap();
    if original_bytes >= length || fields_at + FIELDS as u64 > original_bytes {
        return Ok(None);
    }
    let written_sha256 = written_hash(&mut file, fields_at, original_bytes)?;
    Ok(Some(Patch {
        fields_at,
        original_fields,
        original_bytes,
        fingerprint,
        written_bytes: length,
        written_sha256,
    }))
}

/// Whether the scripts entry points at the launcher's appended copy, which
/// is only the case once a patch is complete.
pub fn patch_in_use(path: &Path, patch: &Patch) -> io::Result<bool> {
    let archive = Archive::open(path)?;
    Ok(archive.entries.iter().any(|entry| {
        entry.fields_at == patch.fields_at && u64::from(entry.offset) == patch.original_bytes
    }))
}

/// Puts the original scripts entry back and cuts the archive to its length
/// before. Leaves an archive without the launcher's trailer alone (a game
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

/// Appends `scripts` and points the entry named `Data\Scripts.rvdata2` at
/// it. The new bytes reach the disk before the 12 bytes of the entry change,
/// so a stop halfway leaves either the original archive with an unused tail
/// (which [`remove_patch`] cuts off) or the finished patch.
pub fn add_patch(path: &Path, scripts: &[u8], fingerprint: [u8; 32]) -> io::Result<Patch> {
    let archive = Archive::open(path)?;
    let entry = archive
        .entry("Data/Scripts.rvdata2")
        .ok_or_else(|| invalid("the archive has no Data\\Scripts.rvdata2"))?
        .clone();
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let original_bytes = file.metadata()?.len();
    if original_bytes + scripts.len() as u64 + TRAILER_BYTES > LARGEST {
        return Err(invalid(
            "the archive is too large to add the translation to",
        ));
    }
    let mut original_fields = vec![0; FIELDS];
    file.seek(SeekFrom::Start(entry.fields_at))?;
    file.read_exact(&mut original_fields)?;

    let mut encrypted = scripts.to_vec();
    crypt(&mut encrypted, SCRIPTS_KEY);
    let mut trailer = Vec::with_capacity(TRAILER_BYTES as usize);
    trailer.extend_from_slice(TRAILER_MAGIC);
    trailer.extend_from_slice(&TRAILER_VERSION.to_le_bytes());
    trailer.extend_from_slice(&entry.fields_at.to_le_bytes());
    trailer.extend_from_slice(&original_fields);
    trailer.extend_from_slice(&original_bytes.to_le_bytes());
    trailer.extend_from_slice(&fingerprint);
    file.seek(SeekFrom::Start(original_bytes))?;
    file.write_all(&encrypted)?;
    file.write_all(&trailer)?;
    file.sync_all()?;

    let fields = archive.fields(original_bytes as u32, scripts.len() as u32, SCRIPTS_KEY);
    file.seek(SeekFrom::Start(entry.fields_at))?;
    file.write_all(&fields)?;
    file.sync_all()?;
    drop(file);
    find_patch(path)?.ok_or_else(|| invalid("the patch could not be read back"))
}

/// XORs four bytes at a time with a key that steps as `key * 7 + 3`; the
/// same call encrypts and decrypts.
pub fn crypt(bytes: &mut [u8], mut key: u32) {
    for chunk in bytes.chunks_mut(4) {
        for (byte, key_byte) in chunk.iter_mut().zip(key.to_le_bytes()) {
            *byte ^= key_byte;
        }
        key = key.wrapping_mul(7).wrapping_add(3);
    }
}

fn written_hash(file: &mut File, fields_at: u64, original_bytes: u64) -> io::Result<String> {
    let mut digest = Sha256::new();
    let mut fields = [0; FIELDS];
    file.seek(SeekFrom::Start(fields_at))?;
    file.read_exact(&mut fields)?;
    digest.update(fields);
    file.seek(SeekFrom::Start(original_bytes))?;
    io::copy(file, &mut DigestWriter(&mut digest))?;
    Ok(hex::encode(digest.finalize()))
}

struct DigestWriter<'a>(&'a mut Sha256);

impl Write for DigestWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_u32(reader: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn normal(name: &str) -> String {
    name.replace('\\', "/").to_lowercase()
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Builds archives for tests, in the format RPG Maker writes.
#[cfg(test)]
pub(crate) fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
    let raw_key = 0x1234_5678_u32;
    let key = raw_key.wrapping_mul(9).wrapping_add(3);
    let index_bytes: usize = files.iter().map(|(name, _)| 16 + name.len()).sum::<usize>() + 16;
    let mut offset = 12 + index_bytes as u32;
    let mut index = Vec::new();
    let mut data = Vec::new();
    for (number, (name, bytes)) in files.iter().enumerate() {
        let file_key = 0x0101_0101_u32.wrapping_mul(number as u32 + 1);
        for value in [offset, bytes.len() as u32, file_key, name.len() as u32] {
            index.extend_from_slice(&(value ^ key).to_le_bytes());
        }
        index.extend(
            name.bytes()
                .enumerate()
                .map(|(at, byte)| byte ^ key.to_le_bytes()[at % 4]),
        );
        let mut encrypted = bytes.to_vec();
        crypt(&mut encrypted, file_key);
        data.extend_from_slice(&encrypted);
        offset += bytes.len() as u32;
    }
    index.extend_from_slice(&key.to_le_bytes());
    index.extend_from_slice(&[0; 12]);
    let mut archive = HEADER.to_vec();
    archive.extend_from_slice(&raw_key.to_le_bytes());
    archive.extend_from_slice(&index);
    archive.extend_from_slice(&data);
    archive
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_with(directory: &Path) -> std::path::PathBuf {
        let path = directory.join("Game.rgss3a");
        std::fs::write(
            &path,
            build(&[
                ("Data\\System.rvdata2", b"system bytes"),
                ("Data\\Scripts.rvdata2", b"the game's scripts"),
                ("Graphics\\Titles1\\Title.png", b"\x89PNG picture"),
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
        assert_eq!(archive.entries().len(), 3);
        let scripts = archive.entry("data/scripts.rvdata2").unwrap();
        assert_eq!(archive.read(&path, scripts).unwrap(), b"the game's scripts");
        let title = archive.entry("Graphics/Titles1/Title.png").unwrap();
        assert_eq!(archive.read(&path, title).unwrap(), b"\x89PNG picture");
    }

    #[test]
    fn a_patch_points_the_scripts_at_the_new_copy_and_comes_off_exactly() {
        let directory = tempfile::tempdir().unwrap();
        let path = archive_with(directory.path());
        let original = std::fs::read(&path).unwrap();
        assert_eq!(find_patch(&path).unwrap(), None);

        let patch = add_patch(&path, b"our scripts first", [7; 32]).unwrap();
        assert_eq!(patch.original_bytes, original.len() as u64);
        assert_eq!(patch.fingerprint, [7; 32]);
        assert!(patch_in_use(&path, &patch).unwrap());
        let archive = Archive::open(&path).unwrap();
        let scripts = archive.entry("Data\\Scripts.rvdata2").unwrap();
        assert_eq!(archive.read(&path, scripts).unwrap(), b"our scripts first");
        // Every other file is where it was.
        let system = archive.entry("Data\\System.rvdata2").unwrap();
        assert_eq!(archive.read(&path, system).unwrap(), b"system bytes");
        let patched = std::fs::read(&path).unwrap();
        let changed = original
            .iter()
            .zip(&patched)
            .filter(|(before, after)| before != after)
            .count();
        assert!(changed <= FIELDS);
        assert_eq!(find_patch(&path).unwrap(), Some(patch));

        assert!(remove_patch(&path).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!remove_patch(&path).unwrap());
    }

    #[test]
    fn a_stop_before_the_entry_changes_leaves_a_tail_that_comes_off() {
        let directory = tempfile::tempdir().unwrap();
        let path = archive_with(directory.path());
        let original = std::fs::read(&path).unwrap();
        let patch = add_patch(&path, b"our scripts first", [7; 32]).unwrap();
        // As if the launcher stopped after the tail but before the entry.
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(patch.fields_at)).unwrap();
        file.write_all(&patch.original_fields).unwrap();
        drop(file);
        let found = find_patch(&path).unwrap().unwrap();
        assert!(!patch_in_use(&path, &found).unwrap());
        assert!(remove_patch(&path).unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}
