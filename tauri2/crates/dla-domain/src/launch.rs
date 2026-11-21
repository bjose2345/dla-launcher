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
/// picture fitted to the monitor, cropped to cover it or stretched over it,
/// full screen in whole multiples of its pixels, or a window of a chosen size
/// with the picture fitted into it. Without one the window stays as the game
/// made it. Stored and bound as `fit`, `fill`, `stretch`, `integer` or
/// `<width>x<height>`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(into = "String", try_from = "String")]
pub enum WindowScale {
    Fit,
    Fill,
    Stretch,
    Integer,
    Window { width: u32, height: u32 },
}

impl WindowScale {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fit" => Some(Self::Fit),
            "fill" => Some(Self::Fill),
            "stretch" => Some(Self::Stretch),
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
            Self::Fill => formatter.write_str("fill"),
            Self::Stretch => formatter.write_str("stretch"),
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
            ("fill", WindowScale::Fill),
            ("stretch", WindowScale::Stretch),
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

/// How an enlarged game's picture is drawn when the player wants something
/// other than hard pixels: evened out, smoothed, or sharpened by one of
/// gamescope's two upscalers.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(into = "String", try_from = "String")]
pub enum PictureFilter {
    /// Sharp pixels evened out where the enlargement is not a whole number,
    /// which costs a weak graphics card most of its frames at full screen.
    EvenPixels,
    Smooth,
    Fsr(Sharpness),
    Nis(Sharpness),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sharpness {
    Soft,
    Medium,
    Strong,
}

impl Sharpness {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "soft" => Some(Self::Soft),
            "medium" => Some(Self::Medium),
            "strong" => Some(Self::Strong),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Soft => "soft",
            Self::Medium => "medium",
            Self::Strong => "strong",
        }
    }
}

impl PictureFilter {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "even" => return Some(Self::EvenPixels),
            "smooth" => return Some(Self::Smooth),
            _ => {}
        }
        let (upscaler, sharpness) = value.split_once('-')?;
        let sharpness = Sharpness::parse(sharpness)?;
        match upscaler {
            "fsr" => Some(Self::Fsr(sharpness)),
            "nis" => Some(Self::Nis(sharpness)),
            _ => None,
        }
    }
}

impl fmt::Display for PictureFilter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EvenPixels => formatter.write_str("even"),
            Self::Smooth => formatter.write_str("smooth"),
            Self::Fsr(sharpness) => write!(formatter, "fsr-{}", sharpness.as_str()),
            Self::Nis(sharpness) => write!(formatter, "nis-{}", sharpness.as_str()),
        }
    }
}

impl From<PictureFilter> for String {
    fn from(filter: PictureFilter) -> Self {
        filter.to_string()
    }
}

impl TryFrom<String> for PictureFilter {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| format!("unknown picture filter {value:?}"))
    }
}

#[cfg(test)]
mod picture_filter_tests {
    use super::{PictureFilter, Sharpness};

    #[test]
    fn reads_and_writes_every_filter() {
        for (text, filter) in [
            ("even", PictureFilter::EvenPixels),
            ("smooth", PictureFilter::Smooth),
            ("fsr-soft", PictureFilter::Fsr(Sharpness::Soft)),
            ("fsr-medium", PictureFilter::Fsr(Sharpness::Medium)),
            ("nis-strong", PictureFilter::Nis(Sharpness::Strong)),
        ] {
            assert_eq!(PictureFilter::parse(text), Some(filter));
            assert_eq!(filter.to_string(), text);
            assert_eq!(
                serde_json::to_string(&filter).expect("json"),
                format!("\"{text}\"")
            );
        }
        for text in ["", "sharp", "fsr", "fsr-", "fsr-max", "bicubic-soft"] {
            assert_eq!(PictureFilter::parse(text), None, "{text:?}");
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
