use std::fmt;

use serde::{Deserialize, Serialize};

use crate::installation::{InstallationId, LaunchActionKind};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct LaunchActivityId(pub String);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchAdapter {
    WindowsNative,
    LinuxNative,
    LinuxWine,
}

/// How Linux shows a game's window through gamescope: full screen with the
/// picture fitted to the monitor, full screen in whole multiples of its
/// pixels, or a window of a chosen size with the picture fitted into it.
/// Without one the window stays as the game made it. Stored and bound as
/// `fit`, `integer` or `<width>x<height>`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(into = "String", try_from = "String")]
pub enum WindowScale {
    Fit,
    Integer,
    Window { width: u32, height: u32 },
}

impl WindowScale {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fit" => Some(Self::Fit),
            "integer" => Some(Self::Integer),
            _ => {
                let (width, height) = value.split_once('x')?;
                let (width, height) = (width.parse().ok()?, height.parse().ok()?);
                ((320..=7680).contains(&width) && (240..=4320).contains(&height))
                    .then_some(Self::Window { width, height })
            }
        }
    }
}

impl fmt::Display for WindowScale {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fit => formatter.write_str("fit"),
            Self::Integer => formatter.write_str("integer"),
            Self::Window { width, height } => write!(formatter, "{width}x{height}"),
        }
    }
}

impl From<WindowScale> for String {
    fn from(scale: WindowScale) -> Self {
        scale.to_string()
    }
}

impl TryFrom<String> for WindowScale {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| format!("unknown window scale {value:?}"))
    }
}

#[cfg(test)]
mod window_scale_tests {
    use super::WindowScale;

    #[test]
    fn reads_and_writes_every_shape() {
        for (text, scale) in [
            ("fit", WindowScale::Fit),
            ("integer", WindowScale::Integer),
            (
                "1920x1440",
                WindowScale::Window {
                    width: 1920,
                    height: 1440,
                },
            ),
        ] {
            assert_eq!(WindowScale::parse(text), Some(scale));
            assert_eq!(scale.to_string(), text);
            assert_eq!(
                serde_json::to_string(&scale).expect("json"),
                format!("\"{text}\"")
            );
        }
        for text in [
            "",
            "huge",
            "1920",
            "1920x",
            "100x100",
            "9000x9000",
            "1920 x 1440",
        ] {
            assert_eq!(WindowScale::parse(text), None, "{text:?}");
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchActivityStatus {
    Starting,
    Running,
    Stopping,
    Exited,
    Failed,
    Stopped,
    Interrupted,
}

impl LaunchActivityStatus {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }

    pub fn is_terminal(self) -> bool {
        !self.is_active()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchActivity {
    pub id: LaunchActivityId,
    pub installation_id: InstallationId,
    pub action: Option<LaunchActionKind>,
    pub target_path: Option<String>,
    pub adapter: Option<LaunchAdapter>,
    pub status: LaunchActivityStatus,
    pub process_id: Option<u32>,
    pub error: Option<String>,
    pub attempted_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub stop_requested_at: Option<String>,
}
