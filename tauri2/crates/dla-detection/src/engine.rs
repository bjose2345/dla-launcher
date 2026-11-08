use dla_domain::{
    game_engine::{GameEngine, GameEngineDetection},
    installation::RelativePath,
};

/// A file that only one engine ships, and the game folder it puts the
/// engine's files in.
type Signature = fn(&str) -> Option<usize>;

// Earlier entries win: an RPG Maker MV/MZ game may also carry files that look
// like another engine's (a bundled `.xp3` tool, a Unity-made launcher).
const SIGNATURES: [(GameEngine, Signature); 11] = [
    (GameEngine::RpgMakerMz, |path| {
        ends_with_file(path, "js/rmmz_core.js")
    }),
    (GameEngine::RpgMakerMv, |path| {
        ends_with_file(path, "js/rpg_core.js")
    }),
    (GameEngine::TyranoScript, |path| {
        before_folder(path, "tyrano/plugins/kag/")
    }),
    (GameEngine::UnityMono, unity),
    (GameEngine::RpgMakerVxAce, |path| {
        rgss(path, ".rgss3a", "rgss3", ".rvdata2")
    }),
    (GameEngine::RpgMakerVx, |path| {
        rgss(path, ".rgss2a", "rgss2", ".rvdata")
    }),
    (GameEngine::RpgMakerXp, |path| {
        rgss(path, ".rgssad", "rgss1", ".rxdata")
    }),
    (GameEngine::WolfRpg, wolf),
    (GameEngine::Artemis, artemis),
    (GameEngine::Kirikiri, |path| {
        has_extension(path, ".xp3").then(|| folder_of(path))
    }),
    (GameEngine::RenPy, renpy),
];

/// Finds the engine a game was made with from its file paths (relative,
/// `/`-separated, any letter case). When several copies are present, the one
/// nearest the top is chosen.
pub fn detect_game_engine<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> Option<GameEngineDetection> {
    let files = paths
        .into_iter()
        .map(|path| (path.to_ascii_lowercase(), path))
        .collect::<Vec<_>>();
    SIGNATURES.iter().find_map(|(engine, signature)| {
        let root = files
            .iter()
            .filter_map(|(lower, original)| signature(lower).map(|end| &original[..end]))
            .min_by_key(|root| (root.matches('/').count(), *root))?;
        let lower_root = root.to_ascii_lowercase();
        let engine = if *engine == GameEngine::UnityMono
            && files.iter().any(|(lower, _)| is_il2cpp(lower, &lower_root))
        {
            GameEngine::UnityIl2cpp
        } else {
            *engine
        };
        Some(GameEngineDetection {
            engine,
            root: RelativePath::parse(root).ok(),
        })
    })
}

fn ends_with_file(path: &str, file: &str) -> Option<usize> {
    if path == file {
        Some(0)
    } else {
        path.strip_suffix(file)?.strip_suffix('/').map(str::len)
    }
}

fn before_folder(path: &str, folder: &str) -> Option<usize> {
    if path.starts_with(folder) {
        return Some(0);
    }
    path.find(&format!("/{folder}"))
}

fn has_extension(path: &str, extension: &str) -> bool {
    let name = file_name(path);
    name.len() > extension.len() && name.ends_with(extension)
}

fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

fn folder_of(path: &str) -> usize {
    path.rfind('/').unwrap_or(0)
}

/// RPG Maker and WOLF keep data in `Data/` and the RGSS library in `System/`,
/// next to `Game.exe`: the game folder is the one above them.
fn game_folder(path: &str) -> usize {
    let folder = &path[..folder_of(path)];
    match folder.rsplit_once('/') {
        Some((parent, "data" | "system")) => parent.len(),
        None if matches!(folder, "data" | "system") => 0,
        _ => folder.len(),
    }
}

fn rgss(path: &str, archive: &str, library: &str, data: &str) -> Option<usize> {
    let name = file_name(path);
    let is_library = name.starts_with(library) && name.ends_with(".dll");
    (has_extension(path, archive) || has_extension(path, data) || is_library)
        .then(|| game_folder(path))
}

fn unity(path: &str) -> Option<usize> {
    if file_name(path) == "unityplayer.dll" {
        return Some(folder_of(path));
    }
    ["globalgamemanagers", "data.unity3d"]
        .iter()
        .find_map(|marker| {
            let data_folder = &path[..path.strip_suffix(marker)?.strip_suffix('/')?.len()];
            data_folder
                .ends_with("_data")
                .then(|| folder_of(data_folder))
        })
}

fn is_il2cpp(path: &str, root: &str) -> bool {
    let inside = if root.is_empty() {
        Some(path)
    } else {
        path.strip_prefix(root)
            .and_then(|rest| rest.strip_prefix('/'))
    };
    inside.is_some_and(|rest| rest == "gameassembly.dll" || rest.contains("_data/il2cpp_data/"))
}

fn wolf(path: &str) -> Option<usize> {
    if has_extension(path, ".wolf") {
        return Some(game_folder(path));
    }
    before_folder(path, "data/basicdata/")
}

fn artemis(path: &str) -> Option<usize> {
    let name = file_name(path);
    let volume = name
        .rsplit_once('.')
        .filter(|(stem, number)| {
            stem.ends_with(".pfs")
                && number.len() == 3
                && number.bytes().all(|b| b.is_ascii_digit())
        })
        .is_some();
    (has_extension(path, ".pfs") || volume).then(|| folder_of(path))
}

fn renpy(path: &str) -> Option<usize> {
    before_folder(path, "renpy/").or_else(|| {
        let folder = &path[..folder_of(path)];
        let game = folder == "game" || folder.ends_with("/game");
        (game && has_extension(path, ".rpa")).then(|| folder_of(folder))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(paths: &[&str]) -> Option<(GameEngine, Option<String>)> {
        detect_game_engine(paths.iter().copied()).map(|detection| {
            (
                detection.engine,
                detection.root.map(|root| root.as_str().to_owned()),
            )
        })
    }

    fn found(engine: GameEngine, root: Option<&str>) -> Option<(GameEngine, Option<String>)> {
        Some((engine, root.map(str::to_owned)))
    }

    #[test]
    fn recognises_rpg_maker_mz_and_mv_and_their_game_folders() {
        assert_eq!(
            detect(&["Game.exe", "js/rmmz_core.js", "data/System.json"]),
            found(GameEngine::RpgMakerMz, None)
        );
        assert_eq!(
            detect(&[
                "Work/Game.exe",
                "Work/www/js/rpg_core.js",
                "Work/www/data/System.json"
            ]),
            found(GameEngine::RpgMakerMv, Some("Work/www"))
        );
    }

    #[test]
    fn ignores_letter_case_but_keeps_the_original_folder_name() {
        assert_eq!(
            detect(&["MyGame Ver2/JS/RMMZ_Core.JS"]),
            found(GameEngine::RpgMakerMz, Some("MyGame Ver2"))
        );
    }

    #[test]
    fn tells_unity_mono_from_il2cpp() {
        let mono = [
            "Game/UnityPlayer.dll",
            "Game/Game_Data/Managed/Assembly-CSharp.dll",
        ];
        assert_eq!(detect(&mono), found(GameEngine::UnityMono, Some("Game")));
        let il2cpp = [
            "Game/UnityPlayer.dll",
            "Game/GameAssembly.dll",
            "Game/Game_Data/il2cpp_data/Metadata/global-metadata.dat",
        ];
        assert_eq!(
            detect(&il2cpp),
            found(GameEngine::UnityIl2cpp, Some("Game"))
        );
        assert_eq!(
            detect(&["Old/Old_Data/globalgamemanagers", "Old/Old.exe"]),
            found(GameEngine::UnityMono, Some("Old"))
        );
    }

    #[test]
    fn recognises_each_rpg_maker_generation_from_archive_library_or_data() {
        assert_eq!(
            detect(&["Game/Game.exe", "Game/Game.rgss3a"]),
            found(GameEngine::RpgMakerVxAce, Some("Game"))
        );
        assert_eq!(
            detect(&["Game/System/RGSS301.dll", "Game/Game.exe"]),
            found(GameEngine::RpgMakerVxAce, Some("Game"))
        );
        assert_eq!(
            detect(&["Data/Map001.rvdata2", "Game.exe"]),
            found(GameEngine::RpgMakerVxAce, None)
        );
        assert_eq!(
            detect(&["Game.rgss2a"]),
            found(GameEngine::RpgMakerVx, None)
        );
        assert_eq!(
            detect(&["RGSS104J.dll", "Data/Scripts.rxdata"]),
            found(GameEngine::RpgMakerXp, None)
        );
    }

    #[test]
    fn recognises_wolf_packed_and_unpacked() {
        assert_eq!(
            detect(&["Game/Game.exe", "Game/Data.wolf"]),
            found(GameEngine::WolfRpg, Some("Game"))
        );
        assert_eq!(
            detect(&["Game/Data/BasicData.wolf", "Game/Data/MapData.wolf"]),
            found(GameEngine::WolfRpg, Some("Game"))
        );
        assert_eq!(
            detect(&["Game/Data/BasicData/Game.dat"]),
            found(GameEngine::WolfRpg, Some("Game"))
        );
    }

    #[test]
    fn recognises_visual_novel_engines() {
        assert_eq!(
            detect(&["Ver2/Story.exe", "Ver2/Story.pfs", "Ver2/Story.pfs.000"]),
            found(GameEngine::Artemis, Some("Ver2"))
        );
        assert_eq!(
            detect(&["data.xp3", "game.exe"]),
            found(GameEngine::Kirikiri, None)
        );
        assert_eq!(
            detect(&["VN/renpy/__init__.py", "VN/game/scripts.rpa"]),
            found(GameEngine::RenPy, Some("VN"))
        );
        assert_eq!(
            detect(&["VN-1.0-pc/game/archive.rpa"]),
            found(GameEngine::RenPy, Some("VN-1.0-pc"))
        );
        assert_eq!(
            detect(&["tyrano/plugins/kag/kag.js", "data/scenario/first.ks"]),
            found(GameEngine::TyranoScript, None)
        );
    }

    #[test]
    fn prefers_rpg_maker_over_files_that_look_like_other_engines() {
        assert_eq!(
            detect(&["tools/patch.xp3", "www/js/rpg_core.js"]),
            found(GameEngine::RpgMakerMv, Some("www"))
        );
    }

    #[test]
    fn picks_the_copy_nearest_the_top() {
        assert_eq!(
            detect(&["old/backup/js/rmmz_core.js", "current/js/rmmz_core.js"]),
            found(GameEngine::RpgMakerMz, Some("current"))
        );
    }

    #[test]
    fn finds_nothing_in_works_that_are_not_games_or_only_look_alike() {
        assert_eq!(detect(&["01 Track.mp3", "cg/001.png", "readme.txt"]), None);
        assert_eq!(detect(&["Tool.exe", "notes/rpg_core.js.txt"]), None);
        assert_eq!(detect(&["image.xp3.png", "plain.wolf.txt", "pfs"]), None);
        assert_eq!(detect(&[]), None);
    }
}
