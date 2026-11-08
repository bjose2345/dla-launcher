//! File names an older install read wrongly. Japanese ZIPs made on Windows
//! store names in Shift-JIS without the UTF-8 flag; the launcher used to read
//! them as CP437, so `タイトル画面.png` was installed as `â^âCâgâïëµû╩.png`,
//! and a name with ソ or 表 (second byte `\`) was split into a folder.

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
};

use dla_application::maintenance::{InstalledFileNames, LibraryMaintenanceError};
use dla_domain::{installation::RelativePath, maintenance::FileNameFix};
use encoding_rs::SHIFT_JIS;
use walkdir::WalkDir;

use crate::portable_path;

/// CP437's characters for the bytes 0x80 to 0xFF, as the zip crate reads them.
const CP437_HIGH: [char; 128] = [
    '\u{00c7}', '\u{00fc}', '\u{00e9}', '\u{00e2}', '\u{00e4}', '\u{00e0}', '\u{00e5}', '\u{00e7}',
    '\u{00ea}', '\u{00eb}', '\u{00e8}', '\u{00ef}', '\u{00ee}', '\u{00ec}', '\u{00c4}', '\u{00c5}',
    '\u{00c9}', '\u{00e6}', '\u{00c6}', '\u{00f4}', '\u{00f6}', '\u{00f2}', '\u{00fb}', '\u{00f9}',
    '\u{00ff}', '\u{00d6}', '\u{00dc}', '\u{00a2}', '\u{00a3}', '\u{00a5}', '\u{20a7}', '\u{0192}',
    '\u{00e1}', '\u{00ed}', '\u{00f3}', '\u{00fa}', '\u{00f1}', '\u{00d1}', '\u{00aa}', '\u{00ba}',
    '\u{00bf}', '\u{2310}', '\u{00ac}', '\u{00bd}', '\u{00bc}', '\u{00a1}', '\u{00ab}', '\u{00bb}',
    '\u{2591}', '\u{2592}', '\u{2593}', '\u{2502}', '\u{2524}', '\u{2561}', '\u{2562}', '\u{2556}',
    '\u{2555}', '\u{2563}', '\u{2551}', '\u{2557}', '\u{255d}', '\u{255c}', '\u{255b}', '\u{2510}',
    '\u{2514}', '\u{2534}', '\u{252c}', '\u{251c}', '\u{2500}', '\u{253c}', '\u{255e}', '\u{255f}',
    '\u{255a}', '\u{2554}', '\u{2569}', '\u{2566}', '\u{2560}', '\u{2550}', '\u{256c}', '\u{2567}',
    '\u{2568}', '\u{2564}', '\u{2565}', '\u{2559}', '\u{2558}', '\u{2552}', '\u{2553}', '\u{256b}',
    '\u{256a}', '\u{2518}', '\u{250c}', '\u{2588}', '\u{2584}', '\u{258c}', '\u{2590}', '\u{2580}',
    '\u{03b1}', '\u{00df}', '\u{0393}', '\u{03c0}', '\u{03a3}', '\u{03c3}', '\u{00b5}', '\u{03c4}',
    '\u{03a6}', '\u{0398}', '\u{03a9}', '\u{03b4}', '\u{221e}', '\u{03c6}', '\u{03b5}', '\u{2229}',
    '\u{2261}', '\u{00b1}', '\u{2265}', '\u{2264}', '\u{2320}', '\u{2321}', '\u{00f7}', '\u{2248}',
    '\u{00b0}', '\u{2219}', '\u{00b7}', '\u{221a}', '\u{207f}', '\u{00b2}', '\u{25a0}', '\u{00a0}',
];

/// The Japanese path a wrongly read path came from, or `None` when it is not
/// one: it must read back as valid Shift-JIS that has Japanese in it, which a
/// name that was right never does.
pub fn fixed_path(path: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(path.len());
    for character in path.chars() {
        if character.is_ascii() {
            bytes.push(character as u8);
        } else {
            let index = CP437_HIGH.iter().position(|&high| high == character)?;
            bytes.push(0x80 + index as u8);
        }
    }
    if bytes.is_ascii() {
        return None;
    }
    // A lead byte followed by a folder break was split at its second byte
    // `\`. A folder whose name ended in such a character looks the same; the
    // name is joined, the far more common case.
    let mut original = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if is_lead_byte(byte) && at + 1 < bytes.len() {
            let trail = bytes[at + 1];
            original.extend([byte, if trail == b'/' { b'\\' } else { trail }]);
            at += 2;
        } else {
            original.push(byte);
            at += 1;
        }
    }
    let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(&original);
    (!had_errors && decoded.chars().any(is_japanese) && decoded != path)
        .then(|| decoded.into_owned())
}

fn is_lead_byte(byte: u8) -> bool {
    matches!(byte, 0x81..=0x9f | 0xe0..=0xfc)
}

fn is_japanese(character: char) -> bool {
    matches!(
        character,
        '\u{3000}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}'
    )
}

#[derive(Default)]
pub struct DesktopInstalledFileNames;

impl DesktopInstalledFileNames {
    pub fn new() -> Self {
        Self
    }
}

impl InstalledFileNames for DesktopInstalledFileNames {
    fn find(&self, root: &str) -> Result<Vec<FileNameFix>, LibraryMaintenanceError> {
        let root = Path::new(root);
        if !root.is_dir() {
            return Err(LibraryMaintenanceError::adapter(
                "the work's folder is unavailable",
            ));
        }
        let mut fixes = Vec::new();
        for entry in WalkDir::new(root).follow_links(false).min_depth(1) {
            let entry = entry.map_err(LibraryMaintenanceError::adapter)?;
            let kind = entry.file_type();
            // Files, and folders left empty; a folder with files follows them.
            let empty_folder = kind.is_dir()
                && fs::read_dir(entry.path()).is_ok_and(|mut items| items.next().is_none());
            if !kind.is_file() && !empty_folder {
                continue;
            }
            let Some(relative) = relative_text(root, entry.path()) else {
                continue;
            };
            let Some(fixed) = fixed_path(&relative) else {
                continue;
            };
            if let (Ok(from), Ok(to)) = (RelativePath::parse(relative), RelativePath::parse(fixed))
            {
                fixes.push(FileNameFix { from, to });
            }
        }
        Ok(fixes)
    }

    fn fix(&self, root: &str, fixes: &[FileNameFix]) -> Result<(), LibraryMaintenanceError> {
        let root = Path::new(root);
        let mut names = HashSet::new();
        for fix in fixes {
            if !names.insert(fix.to.as_str().to_lowercase())
                || root.join(portable_path(&fix.to)).symlink_metadata().is_ok()
            {
                return Err(LibraryMaintenanceError::adapter(format!(
                    "{} already exists, so no name was changed",
                    fix.to.as_str()
                )));
            }
        }
        let mut done: Vec<(PathBuf, PathBuf)> = Vec::with_capacity(fixes.len());
        for fix in fixes {
            let from = root.join(portable_path(&fix.from));
            let to = root.join(portable_path(&fix.to));
            if let Err(error) = rename(&from, &to) {
                for (from, to) in done.iter().rev() {
                    let _ = rename(to, from);
                }
                remove_empty_folders(root, fixes.iter().map(|fix| &fix.to));
                return Err(LibraryMaintenanceError::adapter(format!(
                    "could not rename {}, so no name was changed: {error}",
                    fix.from.as_str()
                )));
            }
            done.push((from, to));
        }
        remove_empty_folders(root, fixes.iter().map(|fix| &fix.from));
        Ok(())
    }

    fn fixed_path(&self, path: &str) -> Option<String> {
        fixed_path(path)
    }
}

/// Moves a file, or replaces an empty folder, creating the folders on the way.
fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if from.is_dir() {
        fs::create_dir(to)?;
        fs::remove_dir(from)
    } else {
        fs::rename(from, to)
    }
}

/// Removes the folders of `paths` that are now empty, deepest first.
fn remove_empty_folders<'a>(root: &Path, paths: impl Iterator<Item = &'a RelativePath>) {
    let mut folders = BTreeSet::new();
    for path in paths {
        let mut parts = path.as_str().split('/').collect::<Vec<_>>();
        parts.pop();
        while !parts.is_empty() {
            folders.insert(parts.join("/"));
            parts.pop();
        }
    }
    let mut folders = folders.into_iter().collect::<Vec<_>>();
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.matches('/').count()));
    for folder in folders {
        let _ = fs::remove_dir(root.join(folder.split('/').collect::<PathBuf>()));
    }
}

fn relative_text(root: &Path, path: &Path) -> Option<String> {
    let parts = path
        .strip_prefix(root)
        .ok()?
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    /// A Japanese path as the old install wrote it: Shift-JIS read as CP437,
    /// split into folders at every second byte `\`.
    fn garbled(path: &str) -> String {
        SHIFT_JIS
            .encode(path)
            .0
            .iter()
            .map(|&byte| match byte {
                b'\\' => '/',
                0x80..=0xff => CP437_HIGH[usize::from(byte - 0x80)],
                _ => byte as char,
            })
            .collect()
    }

    #[test]
    fn reads_wrongly_read_names_back_as_japanese() {
        assert_eq!(garbled("タイトル画面.png"), "â^âCâgâïëµû╩.png");
        for path in [
            "img/titles1/タイトル画面.png",
            "作品 v2/駅構内.ogg",
            "作品 v2/表示板_フレーム.png",
            "ソフト.txt",
        ] {
            assert_eq!(fixed_path(&garbled(path)).as_deref(), Some(path), "{path}");
        }
        // Right names, and names that never were Japanese, stay.
        assert_eq!(fixed_path("img/titles1/Title.png"), None);
        assert_eq!(fixed_path("img/タイトル画面.png"), None);
        assert_eq!(fixed_path("Résumé.txt"), None);
        assert_eq!(fixed_path("café menu.txt"), None);
    }

    fn write(root: &Path, path: &str, body: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(path, body).expect("file");
    }

    #[test]
    fn gives_files_back_their_names_and_removes_the_folders_left_empty() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        write(root, &garbled("作品 v2/img/タイトル画面.png"), "title");
        write(root, &garbled("作品 v2/表示板_フレーム.png"), "board");
        write(root, "作品 v2/www/save/file1.rmmzsave", "keep");
        write(root, ".dla-installation.json", "{}");
        fs::create_dir_all(root.join(garbled("作品 v2/空"))).expect("empty folder");
        let names = DesktopInstalledFileNames::new();

        let mut fixes = names.find(root.to_str().expect("root")).expect("find");
        fixes.sort_by(|left, right| left.to.as_str().cmp(right.to.as_str()));
        assert_eq!(
            fixes.iter().map(|fix| fix.to.as_str()).collect::<Vec<_>>(),
            [
                "作品 v2/img/タイトル画面.png",
                "作品 v2/空",
                "作品 v2/表示板_フレーム.png"
            ]
        );
        names
            .fix(root.to_str().expect("root"), &fixes)
            .expect("fix");

        assert_eq!(
            fs::read_to_string(root.join("作品 v2/img/タイトル画面.png")).expect("title"),
            "title"
        );
        assert_eq!(
            fs::read_to_string(root.join("作品 v2/表示板_フレーム.png")).expect("board"),
            "board"
        );
        assert!(root.join("作品 v2/空").is_dir());
        assert!(root.join("作品 v2/www/save/file1.rmmzsave").is_file());
        let mut left = fs::read_dir(root)
            .expect("root")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .into_string()
                    .expect("name")
            })
            .collect::<Vec<_>>();
        left.sort();
        assert_eq!(left, [".dla-installation.json", "作品 v2"]);
        assert!(
            names
                .find(root.to_str().expect("root"))
                .expect("again")
                .is_empty()
        );
    }

    #[test]
    fn changes_no_name_when_one_is_already_taken() {
        let directory = tempdir().expect("temporary directory");
        let root = directory.path();
        write(root, &garbled("作品/タイトル画面.png"), "old");
        write(root, &garbled("作品/表紙.png"), "cover");
        write(root, "作品/タイトル画面.png", "taken");
        let names = DesktopInstalledFileNames::new();
        let fixes = names.find(root.to_str().expect("root")).expect("find");

        let error = names
            .fix(root.to_str().expect("root"), &fixes)
            .expect_err("taken");

        assert!(error.to_string().contains("no name was changed"), "{error}");
        assert!(root.join(garbled("作品/表紙.png")).is_file());
        assert!(!root.join("作品/表紙.png").exists());
    }
}
