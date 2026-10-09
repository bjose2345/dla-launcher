use std::{env, path::Path, process::Command};

/// The official 7-Zip the desktop packages install beside the launcher.
const BUNDLED_SEVEN_ZIP: &str = "dla-7zip";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RarToolKind {
    SevenZip,
    Unrar,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RarTool {
    program: String,
    kind: RarToolKind,
}

impl RarTool {
    fn new(program: impl Into<String>, kind: RarToolKind) -> Self {
        Self {
            program: program.into(),
            kind,
        }
    }

    pub(crate) fn kind(&self) -> RarToolKind {
        self.kind
    }

    pub(crate) fn program(&self) -> &str {
        &self.program
    }

    pub(crate) fn label(&self) -> &'static str {
        match self.kind {
            RarToolKind::SevenZip => "7-Zip",
            RarToolKind::Unrar => "UnRAR",
        }
    }

    pub(crate) fn listing_command(&self, archive: &Path) -> Command {
        let mut command = hidden(&self.program);
        command.env("LC_ALL", "C");
        match self.kind {
            RarToolKind::SevenZip => {
                command.args(["l", "-slt", "-ba"]);
                // A Windows pipe gets the console code page, which turns
                // non-ASCII names into placeholders that never match the files.
                if cfg!(windows) {
                    command.arg("-sccUTF-8");
                }
                command.arg("--").arg(archive);
            }
            RarToolKind::Unrar => {
                command.args(["lt", "-p-", "-idc", "-cfg-"]).arg(archive);
            }
        }
        command
    }

    pub(crate) fn extraction_command(&self, archive: &Path, destination: &Path) -> Command {
        let mut command = hidden(&self.program);
        command.env("LC_ALL", "C");
        match self.kind {
            RarToolKind::SevenZip => {
                command
                    // Progress only, on stdout, for the installer to follow.
                    .args(["x", "-y", "-bb0", "-bso0", "-bsp1"])
                    .arg(format!("-o{}", destination.display()))
                    .arg("--")
                    .arg(archive);
            }
            RarToolKind::Unrar => {
                command
                    .args(["x", "-y", "-o-", "-ol-", "-p-", "-idq", "-cfg-"])
                    .arg(archive)
                    .arg(destination);
            }
        }
        command
    }
}

/// A console tool started by the windowed launcher opens a console window of
/// its own on Windows unless it is told not to.
fn hidden(program: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(windows))]
    Command::new(program)
}

pub(crate) fn rar_tool_candidates() -> Vec<RarTool> {
    let mut candidates = Vec::new();
    if let Some(program) = configured_program("DLA_ARCHIVE_TOOL") {
        let kind = infer_kind(&program);
        push_unique(&mut candidates, RarTool::new(program, kind));
    }
    if let Some(program) = configured_program("DLA_UNRAR_TOOL") {
        push_unique(&mut candidates, RarTool::new(program, RarToolKind::Unrar));
    }
    let beside_launcher = env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    if let Some(program) = beside_launcher.as_deref().and_then(bundled_program) {
        push_unique(
            &mut candidates,
            RarTool::new(program, RarToolKind::SevenZip),
        );
    }
    for (program, kind) in [
        ("7z", RarToolKind::SevenZip),
        ("7zz", RarToolKind::SevenZip),
        ("7z.exe", RarToolKind::SevenZip),
        ("unrar", RarToolKind::Unrar),
        ("unrar.exe", RarToolKind::Unrar),
    ] {
        push_unique(&mut candidates, RarTool::new(program, kind));
    }
    candidates
}

fn bundled_program(folder: &Path) -> Option<String> {
    let program = folder.join(format!("{BUNDLED_SEVEN_ZIP}{}", env::consts::EXE_SUFFIX));
    program
        .is_file()
        .then(|| program.to_string_lossy().into_owned())
}

fn configured_program(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn infer_kind(program: &str) -> RarToolKind {
    let filename = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_ascii_lowercase();
    if filename.starts_with("unrar") {
        RarToolKind::Unrar
    } else {
        RarToolKind::SevenZip
    }
}

fn push_unique(candidates: &mut Vec<RarTool>, candidate: RarTool) {
    if !candidates
        .iter()
        .any(|existing| existing.program == candidate.program)
    {
        candidates.push(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifies_unrar_by_its_executable_name() {
        assert_eq!(infer_kind("/opt/tools/unrar"), RarToolKind::Unrar);
        assert_eq!(infer_kind(r"C:\Tools\UnRAR.exe"), RarToolKind::Unrar);
        assert_eq!(infer_kind("/opt/tools/7zz"), RarToolKind::SevenZip);
    }

    #[test]
    fn finds_the_bundled_seven_zip_beside_the_launcher() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(bundled_program(folder.path()), None);

        let program = folder
            .path()
            .join(format!("{BUNDLED_SEVEN_ZIP}{}", env::consts::EXE_SUFFIX));
        std::fs::write(&program, b"").unwrap();
        assert_eq!(
            bundled_program(folder.path()),
            Some(program.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn seven_zip_lists_names_in_utf8_on_windows() {
        let tool = RarTool::new("7z", RarToolKind::SevenZip);
        let command = tool.listing_command(Path::new("work.part1.exe"));
        let arguments = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            arguments.iter().any(|argument| argument == "-sccUTF-8"),
            cfg!(windows)
        );
        assert_eq!(&arguments[arguments.len() - 2..], ["--", "work.part1.exe"]);
    }

    #[test]
    fn creates_extract_only_unrar_commands() {
        let tool = RarTool::new("unrar", RarToolKind::Unrar);
        let command = tool.extraction_command(Path::new("work.rar"), Path::new("staging"));
        let arguments = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(arguments.first().map(String::as_str), Some("x"));
        assert!(arguments.contains(&"-ol-".to_owned()));
        assert_eq!(arguments.last().map(String::as_str), Some("staging"));
        assert!(!arguments.iter().any(|argument| argument == "a"));
    }
}
