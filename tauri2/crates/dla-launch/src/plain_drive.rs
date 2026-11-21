//! Starting a Windows game from a drive letter of its own.
//!
//! A program built for one code page asks Windows where it runs in that code
//! page, and Windows answers `?` for every character of the folder that the
//! code page lacks: a folder with a Japanese name on Windows set to English.
//! The game then opens none of its own files (WOLF RPG Editor: "Cannot
//! locate basic info data"), and Locale Remulator does not change those
//! answers. Given a drive letter that stands for the deepest folder with
//! such a name, the game is told a path it can read.
//!
//! The letter is a name in the player's session and nothing on disk. A
//! folder link would do the same, but NSIS follows such links, so the
//! uninstaller's "delete application data" would delete the game through a
//! link left in the launcher's folders.

/// The folder that gets the drive letter and the program's path below it.
#[derive(Debug, PartialEq, Eq)]
struct Split {
    folder: String,
    rest: String,
}

fn split(program: &str, readable: impl Fn(&str) -> bool) -> Option<Split> {
    let (drive, path) = program.split_once(r":\")?;
    if drive.len() != 1 || !drive.chars().all(|letter| letter.is_ascii_alphabetic()) {
        return None;
    }
    let segments: Vec<&str> = path.split('\\').collect();
    let folders = &segments[..segments.len() - 1];
    let deepest = folders.iter().rposition(|folder| !readable(folder))?;
    Some(Split {
        folder: format!(r"{drive}:\{}", folders[..=deepest].join(r"\")),
        rest: segments[deepest + 1..].join(r"\"),
    })
}

fn free_letter(taken: impl Fn(char) -> bool) -> Option<char> {
    ('D'..='Z').rev().find(|&letter| !taken(letter))
}

#[cfg(target_os = "windows")]
pub(crate) use windows::{PlainDrive, on_drive};

#[cfg(target_os = "windows")]
mod windows {
    use std::path::PathBuf;

    use dla_application::launch::{LaunchError, LaunchProcessExit, ManagedLaunchProcess};
    use windows_sys::{
        Win32::{
            Globalization::{CP_ACP, CP_UTF8, GetACP, WC_NO_BEST_FIT_CHARS, WideCharToMultiByte},
            Storage::FileSystem::{
                DDD_EXACT_MATCH_ON_REMOVE, DDD_REMOVE_DEFINITION, DefineDosDeviceW,
                GetLogicalDrives, QueryDosDeviceW,
            },
        },
        core::BOOL,
    };

    use super::{Split, free_letter, split};
    use crate::{
        is_32_bit_windows_program,
        locale::ordinary_path,
        windows_process::{program_of, running_processes},
    };

    pub(crate) struct PlainDrive {
        letter: char,
        folder: String,
        made: bool,
    }

    impl PlainDrive {
        /// The path to start the game by, and the drive it is on when the
        /// game could not read its own. Only 32-bit programs get one: the
        /// engines built for one code page are, and a 64-bit game keeps the
        /// path it has always had.
        pub fn for_game(program: PathBuf) -> (PathBuf, Option<Self>) {
            if !is_32_bit_windows_program(&program) {
                return (program, None);
            }
            let ordinary = ordinary_path(&program);
            let Some(Split { folder, rest }) =
                ordinary.to_str().and_then(|path| split(path, readable))
            else {
                return (program, None);
            };
            match Self::standing_for(folder) {
                Some(drive) => {
                    log::info!(target: "dla::launch", "event=game_on_plain_drive letter={}", drive.letter);
                    let on_drive = PathBuf::from(format!(r"{}:\{rest}", drive.letter));
                    (on_drive, Some(drive))
                }
                None => {
                    log::warn!(target: "dla::launch", "event=game_without_plain_drive");
                    (program, None)
                }
            }
        }

        /// A letter that already stands for the folder is used as it is and
        /// left behind: the launcher did not make it in this run.
        fn standing_for(folder: String) -> Option<Self> {
            let target = format!(r"\??\{folder}");
            if let Some(letter) = ('D'..='Z').find(|&letter| {
                target_of(letter).is_some_and(|found| found.eq_ignore_ascii_case(&target))
            }) {
                return Some(Self {
                    letter,
                    folder,
                    made: false,
                });
            }
            // SAFETY: GetLogicalDrives takes nothing and returns a bit mask.
            let drives = unsafe { GetLogicalDrives() };
            let letter = free_letter(|letter| {
                drives & (1 << (letter as u32 - 'A' as u32)) != 0 || target_of(letter).is_some()
            })?;
            // SAFETY: both strings end in a NUL and live through the call.
            let made = unsafe {
                DefineDosDeviceW(0, wide(&name(letter)).as_ptr(), wide(&folder).as_ptr())
            } != 0;
            made.then_some(Self {
                letter,
                folder,
                made: true,
            })
        }

        /// A game that started another program and ended leaves that
        /// program running from the letter.
        fn runs_a_program(&self) -> bool {
            let on_letter = format!(r"{}:\", self.letter).to_lowercase();
            let in_folder = format!(r"{}\", self.folder).to_lowercase();
            running_processes()
                .into_iter()
                .filter_map(|process| program_of(process.id))
                .map(|program| program.to_lowercase())
                .any(|program| program.starts_with(&on_letter) || program.starts_with(&in_folder))
        }
    }

    impl Drop for PlainDrive {
        fn drop(&mut self) {
            if !self.made || self.runs_a_program() {
                return;
            }
            // SAFETY: both strings end in a NUL and live through the call.
            unsafe {
                DefineDosDeviceW(
                    DDD_REMOVE_DEFINITION | DDD_EXACT_MATCH_ON_REMOVE,
                    wide(&name(self.letter)).as_ptr(),
                    wide(&self.folder).as_ptr(),
                );
            }
        }
    }

    struct OnDrive {
        game: Box<dyn ManagedLaunchProcess>,
        _drive: PlainDrive,
    }

    impl ManagedLaunchProcess for OnDrive {
        fn process_id(&self) -> u32 {
            self.game.process_id()
        }

        fn try_wait(&mut self) -> Result<Option<LaunchProcessExit>, LaunchError> {
            self.game.try_wait()
        }

        fn terminate(&mut self) -> Result<(), LaunchError> {
            self.game.terminate()
        }

        fn wait(&mut self) -> Result<LaunchProcessExit, LaunchError> {
            self.game.wait()
        }
    }

    /// Keeps the letter for as long as the game is followed.
    pub(crate) fn on_drive(
        game: Box<dyn ManagedLaunchProcess>,
        drive: Option<PlainDrive>,
    ) -> Box<dyn ManagedLaunchProcess> {
        match drive {
            Some(drive) => Box::new(OnDrive {
                game,
                _drive: drive,
            }),
            None => game,
        }
    }

    fn name(letter: char) -> String {
        format!("{letter}:")
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    /// What the letter stands for: `\??\C:\folder` for one made as here or
    /// by `subst`, a device for a real drive.
    fn target_of(letter: char) -> Option<String> {
        let mut target = vec![0u16; 32_768];
        // SAFETY: the name ends in a NUL and the buffer is as long as the
        // length passed with it.
        let length = unsafe {
            QueryDosDeviceW(
                wide(&name(letter)).as_ptr(),
                target.as_mut_ptr(),
                target.len() as u32,
            )
        } as usize;
        let first = target[..length].split(|&unit| unit == 0).next()?;
        (!first.is_empty()).then(|| String::from_utf16_lossy(first))
    }

    /// Whether Windows' own code page has every character of the name. A
    /// best-fit stand-in counts as missing: the name it gives is another
    /// folder's.
    fn readable(name: &str) -> bool {
        let name: Vec<u16> = name.encode_utf16().collect();
        if name.is_empty() {
            return true;
        }
        // SAFETY: the name is passed with its length, no output buffer is
        // given, and the flag is written before the call returns.
        unsafe {
            if GetACP() == CP_UTF8 {
                return true;
            }
            let mut replaced: BOOL = 0;
            let length = WideCharToMultiByte(
                CP_ACP,
                WC_NO_BEST_FIT_CHARS,
                name.as_ptr(),
                name.len() as i32,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                &mut replaced,
            );
            length > 0 && replaced == 0
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{fs, path::Path};

        use tempfile::tempdir;

        use super::*;

        #[test]
        fn a_letter_stands_for_the_folder_until_it_is_dropped() {
            let directory = tempdir().expect("temporary directory");
            let folder = directory.path().join("ゲーム");
            fs::create_dir(&folder).expect("game folder");
            fs::write(folder.join("Game.dat"), "data").expect("game file");
            let folder = ordinary_path(&folder.canonicalize().expect("canonical folder"))
                .to_str()
                .expect("folder path")
                .to_owned();

            let drive = PlainDrive::standing_for(folder.clone()).expect("a free letter");
            let on_drive = format!(r"{}:\Game.dat", drive.letter);
            assert!(drive.made);
            assert_eq!(
                fs::read_to_string(&on_drive).expect("file on the drive"),
                "data"
            );

            let again = PlainDrive::standing_for(folder).expect("the same letter");
            assert_eq!(again.letter, drive.letter);
            assert!(!again.made);
            drop(again);
            assert!(Path::new(&on_drive).is_file());

            drop(drive);
            assert!(!Path::new(&on_drive).exists());
        }

        #[test]
        fn plain_names_are_readable_in_every_code_page() {
            assert!(readable("RJ174492 (2.12)"));
            assert!(readable(""));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ascii(name: &str) -> bool {
        name.is_ascii()
    }

    #[test]
    fn the_deepest_unreadable_folder_gets_the_letter() {
        assert_eq!(
            split(
                r"C:\Users\名前\DLA Works\RJ174492 (2.12)\サキュバス\sys\Game.exe",
                ascii
            ),
            Some(Split {
                folder: r"C:\Users\名前\DLA Works\RJ174492 (2.12)\サキュバス".to_owned(),
                rest: r"sys\Game.exe".to_owned(),
            })
        );
    }

    #[test]
    fn a_readable_path_needs_no_letter() {
        assert_eq!(split(r"D:\Games\RJ1\sys\Game.exe", ascii), None);
    }

    #[test]
    fn a_program_with_an_unreadable_name_in_readable_folders_needs_no_letter() {
        assert_eq!(split(r"D:\Games\RJ1\ゲーム.exe", ascii), None);
    }

    #[test]
    fn a_path_without_a_drive_letter_gets_none() {
        assert_eq!(split(r"\\server\share\ゲーム\Game.exe", ascii), None);
    }

    #[test]
    fn letters_are_taken_from_the_end_of_the_alphabet() {
        assert_eq!(free_letter(|letter| letter == 'Z'), Some('Y'));
        assert_eq!(free_letter(|_| true), None);
    }
}
