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
