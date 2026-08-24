use std::{env, path::Path, process::Command};

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
        let mut command = Command::new(&self.program);
        command.env("LC_ALL", "C");
        match self.kind {
            RarToolKind::SevenZip => {
                command.args(["l", "-slt", "-ba", "--"]).arg(archive);
            }
            RarToolKind::Unrar => {
                command.args(["lt", "-p-", "-idc", "-cfg-"]).arg(archive);
            }
        }
        command
    }

    pub(crate) fn extraction_command(&self, archive: &Path, destination: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command.env("LC_ALL", "C");
        match self.kind {
            RarToolKind::SevenZip => {
                command
                    .args(["x", "-y", "-bd", "-bb0"])
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

pub(crate) fn rar_tool_candidates() -> Vec<RarTool> {
    let mut candidates = Vec::new();
    if let Some(program) = configured_program("DLA_ARCHIVE_TOOL") {
        let kind = infer_kind(&program);
        push_unique(&mut candidates, RarTool::new(program, kind));
    }
    if let Some(program) = configured_program("DLA_UNRAR_TOOL") {
        push_unique(&mut candidates, RarTool::new(program, RarToolKind::Unrar));
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
