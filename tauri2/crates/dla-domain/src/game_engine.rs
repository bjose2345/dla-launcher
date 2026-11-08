use serde::{Deserialize, Serialize};

use crate::installation::RelativePath;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameEngine {
    RpgMakerMz,
    RpgMakerMv,
    RpgMakerVxAce,
    RpgMakerVx,
    RpgMakerXp,
    WolfRpg,
    UnityMono,
    UnityIl2cpp,
    Artemis,
    Kirikiri,
    RenPy,
    TyranoScript,
}

impl GameEngine {
    pub fn label(self) -> &'static str {
        match self {
            Self::RpgMakerMz => "RPG Maker MZ",
            Self::RpgMakerMv => "RPG Maker MV",
            Self::RpgMakerVxAce => "RPG Maker VX Ace",
            Self::RpgMakerVx => "RPG Maker VX",
            Self::RpgMakerXp => "RPG Maker XP",
            Self::WolfRpg => "WOLF RPG Editor",
            Self::UnityMono => "Unity",
            Self::UnityIl2cpp => "Unity (IL2CPP)",
            Self::Artemis => "Artemis",
            Self::Kirikiri => "KiriKiri",
            Self::RenPy => "Ren'Py",
            Self::TyranoScript => "TyranoScript",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameEngineDetection {
    pub engine: GameEngine,
    /// The game's own folder, where the engine keeps its files (for MV, the
    /// `www` folder); `None` when that is the folder that was examined.
    pub root: Option<RelativePath>,
}
