//! Games packed into one executable with Enigma Virtual Box: the list of
//! files such an executable holds, and taking them out into its folder.
//! The executable reads the files inside it before the ones beside it, so
//! a game is changed only once another program starts it from the folder.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use fs2::available_space;
use sha2::{Digest, Sha256};

const HOLDER: &[u8; 8] = b".enigma1";
const PACKER_SECTIONS: &[u8] = b".enigma";
const SIGNATURE: &[u8] = b"EVB\0";
/// How far into its section the list is looked for.
const SIGNATURE_WINDOW: u64 = 1 << 20;
const LIST_HEADER: u64 = 64;
const LIST_LIMIT: u64 = 64 << 20;
const FILES_LIMIT: usize = 200_000;
const DEPTH_LIMIT: usize = 64;
const SECTIONS_LIMIT: usize = 96;
/// The executable's own folder, as the list names it.
const OWN_FOLDER: &str = "%DEFAULT FOLDER%";
const FOLDER: u8 = 3;
const FILE: u8 = 2;
/// The shortest node: its three numbers, an empty name and its kind.
const SHORTEST_NODE: usize = 15;
const PROGRAM_COMPARED: u64 = 16 << 20;
const BUFFER: usize = 1 << 20;
const SPACE_MARGIN: u64 = 64 << 20;
const PARTIAL: &str = ".dla-unpacking";

/// A file an executable holds, by its path below the executable's folder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedFile {
    pub path: String,
    pub bytes: u64,
    at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Packed {
    pub files: Vec<PackedFile>,
    /// SHA-256 of the list as the executable holds it.
    pub list_sha256: String,
    /// The program the executable was made from, when it holds a copy: it
    /// runs the game from the folder.
    pub program: Option<usize>,
}

/// A file in the folder as the executable holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Written {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

struct Section {
    name: [u8; 8],
    at: u64,
    bytes: u64,
}

/// The files `executable` holds; `None` when it is not such an executable.
/// One the launcher does not read (files stored smaller than they are, or
/// for folders other than its own) is an error.
pub fn read(executable: &Path) -> io::Result<Option<Packed>> {
    let mut file = File::open(executable)?;
    let length = file.metadata()?.len();
    let Some(sections) = sections(&mut file, 0)? else {
        return Ok(None);
    };
    let Some(holder) = sections.iter().find(|section| section.name == *HOLDER) else {
        return Ok(None);
    };
    let end = holder.at.saturating_add(holder.bytes).min(length);
    let window = end.saturating_sub(holder.at).min(SIGNATURE_WINDOW);
    let mut start = vec![0; window as usize];
    if !read_at(&mut file, holder.at, &mut start)? {
        return Err(unreadable("its section is cut short"));
    }
    let found = start
        .windows(SIGNATURE.len())
        .position(|window| window == SIGNATURE)
        .ok_or_else(|| unreadable("it has no list of files"))?;
    let list_at = holder.at + found as u64 + LIST_HEADER;
    let mut size = [0; 4];
    if !read_at(&mut file, list_at, &mut size)? {
        return Err(unreadable("its list of files is cut short"));
    }
    let size = u64::from(u32::from_le_bytes(size));
    let data = list_at + 4 + size;
    if size == 0 || size > LIST_LIMIT || data > end {
        return Err(unreadable("its list of files has an impossible size"));
    }
    let mut list = vec![0; size as usize];
    if !read_at(&mut file, list_at + 4, &mut list)? {
        return Err(unreadable("its list of files is cut short"));
    }
    let mut files = Vec::new();
    let mut cursor = Cursor {
        bytes: &list,
        at: 0,
    };
    node(&mut cursor, &mut Vec::new(), data, end, &mut files)?;
    if cursor.at != list.len() {
        return Err(unreadable("its list of files has more than its nodes"));
    }
    let program = program(&mut file, &sections, &files)?;
    Ok(Some(Packed {
        files,
        list_sha256: hex::encode(Sha256::digest(&list)),
        program,
    }))
}

impl Packed {
    /// Whether the executable holds exactly `content` at `path`.
    pub fn holds(&self, executable: &Path, path: &str, content: &[u8]) -> bool {
        let Some(entry) = self.files.iter().find(|file| file.path == path) else {
            return false;
        };
        if entry.bytes != content.len() as u64 {
            return false;
        }
        let mut compared = 0;
        File::open(executable)
            .and_then(|mut file| {
                stream(&mut file, entry.at, entry.bytes, |chunk| {
                    let same = content[compared..compared + chunk.len()] == *chunk;
                    compared += chunk.len();
                    Ok(same)
                })
            })
            .is_ok_and(|same| same.is_some())
    }
}

/// Writes into the executable's folder every file it holds that is not
/// there, whole or not at all. A file already there with the same bytes
/// counts as written; any other one stays as it is and is left out of the
/// result, except in the program's place, which is an error. A failure
/// takes back what this call wrote.
pub fn unpack(executable: &Path, packed: &Packed) -> io::Result<Vec<Written>> {
    let folder = executable
        .parent()
        .ok_or_else(|| io::Error::other("the executable has no folder"))?;
    let mut file = File::open(executable)?;
    let missing = packed
        .files
        .iter()
        .filter(|entry| fs::symlink_metadata(target(folder, &entry.path)).is_err())
        .map(|entry| entry.bytes)
        .sum::<u64>();
    if available_space(folder).is_ok_and(|free| free < missing.saturating_add(SPACE_MARGIN)) {
        return Err(io::Error::other(format!(
            "unpacking the game needs {} MB of free space",
            missing.saturating_add(SPACE_MARGIN) >> 20
        )));
    }
    let mut made = Made::default();
    let mut order = (0..packed.files.len()).collect::<Vec<_>>();
    if let Some(program) = packed.program {
        order.swap(0, program);
    }
    let mut written = Vec::new();
    for index in order {
        let entry = &packed.files[index];
        match place(&mut file, folder, entry, &mut made) {
            Ok(Some(sha256)) => written.push(Written {
                path: entry.path.clone(),
                bytes: entry.bytes,
                sha256,
            }),
            Ok(None) if packed.program == Some(index) => {
                made.take_back();
                return Err(io::Error::other(format!(
                    "another file is in the place of the game's program ({})",
                    entry.path
                )));
            }
            Ok(None) => {}
            Err(error) => {
                made.take_back();
                return Err(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", entry.path),
                ));
            }
        }
    }
    written.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(written)
}

/// Deletes each file that is still as it was written, then the folders
/// below `root` that leaves empty. A file changed since stays.
pub fn remove(root: &Path, written: &[(PathBuf, u64, &str)]) -> io::Result<()> {
    let mut folders = BTreeSet::new();
    for (path, bytes, sha256) in written {
        let unchanged = fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == *bytes)
            && file_sha256(path)? == *sha256;
        if !unchanged {
            continue;
        }
        fs::remove_file(path)?;
        let mut folder = path.parent();
        while let Some(inside) = folder.filter(|folder| *folder != root && folder.starts_with(root))
        {
            folders.insert(inside.to_path_buf());
            folder = inside.parent();
        }
    }
    // Deepest first; a folder that still has something in it stays.
    let mut folders = folders.into_iter().collect::<Vec<_>>();
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.components().count()));
    for folder in folders {
        let _ = fs::remove_dir(folder);
    }
    Ok(())
}

#[derive(Default)]
struct Made {
    files: Vec<PathBuf>,
    folders: Vec<PathBuf>,
    checked: BTreeSet<PathBuf>,
}

impl Made {
    fn take_back(&self) {
        for file in &self.files {
            let _ = fs::remove_file(file);
        }
        for folder in self.folders.iter().rev() {
            let _ = fs::remove_dir(folder);
        }
    }
}

/// The hash of the file now at the entry's place when it is as the
/// executable holds it, written if nothing was there; `None` when something
/// else is there.
fn place(
    file: &mut File,
    folder: &Path,
    entry: &PackedFile,
    made: &mut Made,
) -> io::Result<Option<String>> {
    let target = target(folder, &entry.path);
    // Nothing is written through a link out of the game's folder.
    let mut parent = folder.to_path_buf();
    let folders = entry.path.matches('/').count();
    for part in entry.path.split('/').take(folders) {
        parent.push(part);
        if made.checked.contains(&parent) {
            continue;
        }
        match fs::symlink_metadata(&parent) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&parent)?;
                made.folders.push(parent.clone());
            }
            Err(error) => return Err(error),
        }
        made.checked.insert(parent.clone());
    }
    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.is_file() && metadata.len() == entry.bytes => {
            let mut there = File::open(&target)?;
            let mut other = vec![0; BUFFER];
            stream(file, entry.at, entry.bytes, |chunk| {
                let other = &mut other[..chunk.len()];
                there.read_exact(other)?;
                Ok(other == chunk)
            })
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut partial = target.clone().into_os_string();
            partial.push(PARTIAL);
            let partial = PathBuf::from(partial);
            made.files.push(partial.clone());
            let mut out = File::create(&partial)?;
            let sha256 = stream(file, entry.at, entry.bytes, |chunk| {
                out.write_all(chunk).map(|()| true)
            })?;
            drop(out);
            fs::rename(&partial, &target)?;
            made.files.pop();
            made.files.push(target);
            Ok(sha256)
        }
        Err(error) => Err(error),
    }
}

fn target(folder: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(folder.to_path_buf(), |target, part| target.join(part))
}

/// Hands `each` the bytes at `at` in pieces, until it says no. The SHA-256
/// of all of them when it never did.
fn stream(
    file: &mut File,
    at: u64,
    bytes: u64,
    mut each: impl FnMut(&[u8]) -> io::Result<bool>,
) -> io::Result<Option<String>> {
    file.seek(SeekFrom::Start(at))?;
    let mut buffer = vec![0; BUFFER.min(bytes.max(1) as usize)];
    let mut digest = Sha256::new();
    let mut left = bytes;
    while left > 0 {
        let piece = &mut buffer[..left.min(BUFFER as u64) as usize];
        file.read_exact(piece)?;
        if !each(piece)? {
            return Ok(None);
        }
        digest.update(&*piece);
        left -= piece.len() as u64;
    }
    Ok(Some(hex::encode(digest.finalize())))
}

fn file_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let bytes = file.metadata()?.len();
    Ok(stream(&mut file, 0, bytes, |_| Ok(true))?.unwrap_or_default())
}

/// The sections of the program that starts at `start` in the file; `None`
/// when no Windows program starts there.
fn sections(file: &mut File, start: u64) -> io::Result<Option<Vec<Section>>> {
    let mut dos = [0; 64];
    if !read_at(file, start, &mut dos)? || &dos[..2] != b"MZ" {
        return Ok(None);
    }
    let header = start + u64::from(u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]));
    let mut coff = [0; 24];
    if !read_at(file, header, &mut coff)? || &coff[..4] != b"PE\0\0" {
        return Ok(None);
    }
    let count = usize::from(u16::from_le_bytes([coff[6], coff[7]]));
    let optional = u64::from(u16::from_le_bytes([coff[20], coff[21]]));
    if count == 0 || count > SECTIONS_LIMIT {
        return Ok(None);
    }
    let mut table = vec![0; count * 40];
    if !read_at(file, header + 24 + optional, &mut table)? {
        return Ok(None);
    }
    Ok(Some(
        table
            .as_chunks::<40>()
            .0
            .iter()
            .map(|entry| {
                let number = |at: usize| {
                    u64::from(u32::from_le_bytes([
                        entry[at],
                        entry[at + 1],
                        entry[at + 2],
                        entry[at + 3],
                    ]))
                };
                let mut name = [0; 8];
                name.copy_from_slice(&entry[..8]);
                Section {
                    name,
                    at: number(20),
                    bytes: number(16),
                }
            })
            .collect(),
    ))
}

/// The file among those in the executable's folder that is the program the
/// executable was made from: the same sections, the packer's left out, and
/// the same first one.
fn program(
    file: &mut File,
    sections_of_packed: &[Section],
    files: &[PackedFile],
) -> io::Result<Option<usize>> {
    let own = sections_of_packed
        .iter()
        .filter(|section| !section.name.starts_with(PACKER_SECTIONS))
        .collect::<Vec<_>>();
    let Some(first) = own.first() else {
        return Ok(None);
    };
    for (index, candidate) in files.iter().enumerate() {
        if candidate.path.contains('/')
            || !candidate.path.to_ascii_lowercase().ends_with(".exe")
            || first.at.saturating_add(first.bytes) > candidate.bytes
        {
            continue;
        }
        let Some(inner) = sections(file, candidate.at)? else {
            continue;
        };
        let same_sections = inner.len() == own.len()
            && inner.iter().zip(&own).all(|(inner, own)| {
                inner.name == own.name && inner.at == own.at && inner.bytes == own.bytes
            });
        if !same_sections {
            continue;
        }
        let compared = first.bytes.min(PROGRAM_COMPARED);
        let mut theirs = vec![0; compared as usize];
        let mut ours = vec![0; compared as usize];
        if read_at(file, candidate.at + first.at, &mut theirs)?
            && read_at(file, first.at, &mut ours)?
            && theirs == ours
        {
            return Ok(Some(index));
        }
    }
    Ok(None)
}

/// Whether `buffer` was filled from `at`; not when the file ends before.
fn read_at(file: &mut File, at: u64, buffer: &mut [u8]) -> io::Result<bool> {
    file.seek(SeekFrom::Start(at))?;
    match file.read_exact(buffer) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(error),
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| unreadable("its list of files ends inside a node"))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn u32(&mut self) -> io::Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> io::Result<u64> {
        let mut number = [0; 8];
        number.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(number))
    }

    /// UTF-16, ended by a zero.
    fn name(&mut self) -> io::Result<String> {
        let mut units = Vec::new();
        loop {
            let unit = self.take(2)?;
            match u16::from_le_bytes([unit[0], unit[1]]) {
                0 => break,
                unit => units.push(unit),
            }
        }
        String::from_utf16(&units).map_err(|_| unreadable("a name in its list is not text"))
    }
}

/// One node of the list and the nodes below it: three numbers (the last is
/// how many nodes follow below), a name, a kind and what the kind has.
fn node(
    cursor: &mut Cursor<'_>,
    parents: &mut Vec<String>,
    data: u64,
    end: u64,
    files: &mut Vec<PackedFile>,
) -> io::Result<()> {
    cursor.take(8)?;
    let children = cursor.u32()? as usize;
    let name = cursor.name()?;
    let kind = cursor.take(1)?[0];
    let first = parents.is_empty() && files.is_empty() && cursor.at == SHORTEST_NODE;
    match kind {
        0 if first && name.is_empty() => {}
        FOLDER => {
            cursor.take(29)?;
        }
        FILE => {
            cursor.take(2)?;
            let bytes = cursor.u64()?;
            cursor.take(31)?;
            let offset = cursor.u64()?;
            let stored = cursor.u64()?;
            if children != 0 {
                return Err(unreadable("a file in its list has files below it"));
            }
            if parents.first().map(String::as_str) != Some(OWN_FOLDER) {
                return Err(unreadable("it holds files for a folder other than its own"));
            }
            if bytes != stored {
                return Err(unreadable("it holds its files packed smaller"));
            }
            let at = data
                .checked_add(offset)
                .filter(|at| at.checked_add(bytes).is_some_and(|last| last <= end))
                .ok_or_else(|| unreadable("a file in its list lies outside it"))?;
            let mut parts = parents[1..].to_vec();
            parts.push(name);
            if !parts.iter().all(|part| is_plain(part)) {
                return Err(unreadable("a name in its list is not a plain file name"));
            }
            if files.len() == FILES_LIMIT {
                return Err(unreadable("it holds too many files"));
            }
            files.push(PackedFile {
                path: parts.join("/"),
                bytes,
                at,
            });
            return Ok(());
        }
        _ => return Err(unreadable("its list has a node of a kind not known")),
    }
    let left = cursor.bytes.len() - cursor.at;
    if children > left / SHORTEST_NODE || parents.len() == DEPTH_LIMIT {
        return Err(unreadable("its list of files has an impossible shape"));
    }
    let named = !name.is_empty();
    if named {
        parents.push(name);
    }
    for _ in 0..children {
        node(cursor, parents, data, end, files)?;
    }
    if named {
        parents.pop();
    }
    Ok(())
}

fn is_plain(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.ends_with(PARTIAL)
        && !part
            .chars()
            .any(|letter| matches!(letter, '/' | '\\' | ':') || letter.is_control())
}

fn unreadable(why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("the game's executable holds its files in a way the launcher does not read: {why}"),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;

    const TEXT: usize = 512;

    /// A Windows program of one section; with `box_bytes`, the packer's
    /// section after it.
    pub(crate) fn program_bytes(text: u8, box_bytes: Option<&[u8]>) -> Vec<u8> {
        let mut bytes = vec![0; TEXT];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
        bytes[64..68].copy_from_slice(b"PE\0\0");
        let count = if box_bytes.is_some() { 2_u16 } else { 1 };
        bytes[70..72].copy_from_slice(&count.to_le_bytes());
        let mut section = |index: usize, name: &[u8], size: usize, at: usize| {
            let entry = &mut bytes[88 + index * 40..128 + index * 40];
            entry[..name.len()].copy_from_slice(name);
            entry[16..20].copy_from_slice(&(size as u32).to_le_bytes());
            entry[20..24].copy_from_slice(&(at as u32).to_le_bytes());
        };
        section(0, b".text", TEXT, TEXT);
        if let Some(box_bytes) = box_bytes {
            section(1, b".enigma1", box_bytes.len(), 2 * TEXT);
        }
        bytes.extend(std::iter::repeat_n(text, TEXT));
        bytes.extend_from_slice(box_bytes.unwrap_or_default());
        bytes
    }

    enum Tree {
        File(Vec<u8>),
        Folder(BTreeMap<String, Tree>),
    }

    fn name(list: &mut Vec<u8>, name: &str) {
        for unit in name.encode_utf16() {
            list.extend_from_slice(&unit.to_le_bytes());
        }
        list.extend_from_slice(&[0, 0]);
    }

    fn write_tree(list: &mut Vec<u8>, data: &mut Vec<u8>, own: &str, tree: &Tree) {
        match tree {
            Tree::File(content) => {
                list.extend_from_slice(&[0; 12]);
                name(list, own);
                list.extend_from_slice(&[FILE, 0, 0]);
                list.extend_from_slice(&(content.len() as u64).to_le_bytes());
                list.extend_from_slice(&[0; 31]);
                list.extend_from_slice(&(data.len() as u64).to_le_bytes());
                list.extend_from_slice(&(content.len() as u64).to_le_bytes());
                data.extend_from_slice(content);
            }
            Tree::Folder(inside) => {
                list.extend_from_slice(&[0; 8]);
                list.extend_from_slice(&(inside.len() as u32).to_le_bytes());
                name(list, own);
                list.push(FOLDER);
                list.extend_from_slice(&[0; 29]);
                for (name, tree) in inside {
                    write_tree(list, data, name, tree);
                }
            }
        }
    }

    /// The packer's section holding `files`, by their paths below `folder`.
    pub(crate) fn box_bytes(folder: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut root = BTreeMap::new();
        for (path, content) in files {
            let mut inside = &mut root;
            let mut parts = path.split('/').peekable();
            while let Some(part) = parts.next() {
                if parts.peek().is_none() {
                    inside.insert(part.to_owned(), Tree::File(content.to_vec()));
                    break;
                }
                let Tree::Folder(next) = inside
                    .entry(part.to_owned())
                    .or_insert_with(|| Tree::Folder(BTreeMap::new()))
                else {
                    panic!("a file and a folder of one name");
                };
                inside = next;
            }
        }
        let mut list = Vec::new();
        list.extend_from_slice(&[0; 8]);
        list.extend_from_slice(&1_u32.to_le_bytes());
        list.extend_from_slice(&[0, 0, 0]);
        let mut data = Vec::new();
        write_tree(&mut list, &mut data, folder, &Tree::Folder(root));
        let mut bytes = vec![0xcc; 192];
        bytes.extend_from_slice(SIGNATURE);
        bytes.extend_from_slice(&[0; 60]);
        bytes.extend_from_slice(&(list.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&list);
        bytes.extend_from_slice(&data);
        bytes
    }

    /// An executable packed from a program whose code is `text`, holding
    /// `files` and a copy of that program as `Game.exe`.
    pub(crate) fn packed_game(text: u8, files: &[(&str, &[u8])]) -> Vec<u8> {
        let program = program_bytes(text, None);
        let mut files = files.to_vec();
        files.push(("Game.exe", &program));
        program_bytes(text, Some(&box_bytes(OWN_FOLDER, &files)))
    }

    /// The same without the copy of the program.
    pub(crate) fn packed_files(text: u8, files: &[(&str, &[u8])]) -> Vec<u8> {
        program_bytes(text, Some(&box_bytes(OWN_FOLDER, files)))
    }

    const FILES: [(&str, &[u8]); 3] = [
        ("data/System.json", b"{\"gameTitle\":\"x\"}"),
        ("js/plugins.js", b"var $plugins =\n[\n];\n"),
        ("js/rmmz_core.js", b"// core"),
    ];

    #[test]
    fn reads_the_files_an_executable_holds_and_finds_its_program() {
        let folder = tempdir().unwrap();
        let executable = folder.path().join("Packed.exe");
        fs::write(&executable, packed_game(7, &FILES)).unwrap();
        let packed = read(&executable).unwrap().unwrap();
        assert_eq!(
            packed
                .files
                .iter()
                .map(|file| (file.path.as_str(), file.bytes))
                .collect::<Vec<_>>(),
            [
                ("Game.exe", 1024),
                ("data/System.json", 17),
                ("js/plugins.js", 20),
                ("js/rmmz_core.js", 7),
            ]
        );
        assert_eq!(packed.program, Some(0));
        assert_eq!(packed.list_sha256.len(), 64);
        assert!(packed.holds(&executable, "js/plugins.js", FILES[1].1));
        assert!(!packed.holds(&executable, "js/plugins.js", b"var $plugins =\n[\n];\r\n"));
        assert!(!packed.holds(&executable, "js/other.js", FILES[1].1));

        fs::write(&executable, program_bytes(7, None)).unwrap();
        assert_eq!(read(&executable).unwrap(), None);
        fs::write(&executable, b"not a program").unwrap();
        assert_eq!(read(&executable).unwrap(), None);
    }

    #[test]
    fn a_program_of_other_code_is_not_the_one_the_executable_was_made_from() {
        let folder = tempdir().unwrap();
        let executable = folder.path().join("Packed.exe");
        let other = program_bytes(9, None);
        fs::write(
            &executable,
            program_bytes(
                7,
                Some(&box_bytes(
                    OWN_FOLDER,
                    &[("Tool.exe", &other), ("a.txt", b"a")],
                )),
            ),
        )
        .unwrap();
        assert_eq!(read(&executable).unwrap().unwrap().program, None);
    }

    #[test]
    fn refuses_a_list_it_does_not_read() {
        let folder = tempdir().unwrap();
        let executable = folder.path().join("Packed.exe");
        let refused = |box_bytes: &[u8]| {
            fs::write(&executable, program_bytes(7, Some(box_bytes))).unwrap();
            read(&executable).unwrap_err().kind() == io::ErrorKind::InvalidData
        };
        assert!(refused(&box_bytes("%SYSTEM FOLDER%", &[("a.dll", b"a")])));
        assert!(refused(&box_bytes(OWN_FOLDER, &[("..", b"a")])));
        assert!(refused(&box_bytes(OWN_FOLDER, &[("a:b", b"a")])));
        assert!(refused(&[0xcc; 300]));
        let whole = box_bytes(OWN_FOLDER, &[("a.txt", b"abcdef")]);
        assert!(refused(&whole[..whole.len() - 3]));
        let mut smaller = whole.clone();
        let stored = smaller.len() - 6 - 8;
        smaller[stored] = 5;
        assert!(refused(&smaller));
        let mut endless = whole;
        endless[192 + 64 + 4 + 8..192 + 64 + 4 + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(refused(&endless));
    }

    #[test]
    fn unpacks_what_is_missing_and_takes_the_same_file_as_written() {
        let folder = tempdir().unwrap();
        let executable = folder.path().join("Packed.exe");
        fs::write(&executable, packed_game(7, &FILES)).unwrap();
        fs::create_dir(folder.path().join("js")).unwrap();
        fs::write(folder.path().join("js/rmmz_core.js"), b"// core").unwrap();
        fs::write(folder.path().join("js/plugins.js"), b"the player's own").unwrap();
        let packed = read(&executable).unwrap().unwrap();

        let written = unpack(&executable, &packed).unwrap();
        assert_eq!(
            written
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["Game.exe", "data/System.json", "js/rmmz_core.js"]
        );
        assert_eq!(
            fs::read(folder.path().join("data/System.json")).unwrap(),
            FILES[0].1
        );
        assert_eq!(
            fs::read(folder.path().join("Game.exe")).unwrap(),
            program_bytes(7, None)
        );
        assert_eq!(
            fs::read(folder.path().join("js/plugins.js")).unwrap(),
            b"the player's own"
        );
        assert_eq!(written[1].sha256, hex::encode(Sha256::digest(FILES[0].1)));
        assert_eq!(unpack(&executable, &packed).unwrap(), written);

        fs::write(
            folder.path().join("data/System.json"),
            b"a save the game made",
        )
        .unwrap();
        let recorded = written
            .iter()
            .map(|file| {
                (
                    target(folder.path(), &file.path),
                    file.bytes,
                    file.sha256.as_str(),
                )
            })
            .collect::<Vec<_>>();
        remove(folder.path(), &recorded).unwrap();
        assert!(!folder.path().join("Game.exe").exists());
        assert!(!folder.path().join("js/rmmz_core.js").exists());
        assert!(folder.path().join("js/plugins.js").exists());
        assert!(folder.path().join("data/System.json").exists());
        assert!(executable.exists());
    }

    #[test]
    fn a_failure_takes_back_what_was_written() {
        let folder = tempdir().unwrap();
        let executable = folder.path().join("Packed.exe");
        fs::write(&executable, packed_game(7, &FILES)).unwrap();
        fs::write(folder.path().join("Game.exe"), b"another program").unwrap();
        let packed = read(&executable).unwrap().unwrap();
        assert!(unpack(&executable, &packed).is_err());
        assert!(!folder.path().join("data").exists());
        assert!(!folder.path().join("js").exists());

        fs::remove_file(folder.path().join("Game.exe")).unwrap();
        fs::write(folder.path().join("js"), b"a file where a folder goes").unwrap();
        let written = unpack(&executable, &packed).unwrap();
        assert_eq!(
            written
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["Game.exe", "data/System.json"]
        );
    }
}
