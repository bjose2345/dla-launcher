use serde::{Deserialize, Serialize};

use crate::installation::{InstallationId, RelativePath};

/// What a work outside the catalog is, chosen by the user.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnWorkKind {
    Images,
    Manga,
    Game,
    Voice,
    Video,
    Document,
}

/// The details a user gives a work the catalog doesn't know, shown where a
/// catalog work shows the catalog's. Artwork is named by its id in the
/// work's own artwork folder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnWorkDetails {
    pub installation_id: InstallationId,
    pub circle: Option<String>,
    pub code: Option<String>,
    pub release_date: Option<String>,
    pub kind: Option<OwnWorkKind>,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub cover: Option<String>,
    pub samples: Vec<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOwnWorkDetailsRequest {
    pub installation_id: InstallationId,
    pub circle: Option<String>,
    pub code: Option<String>,
    pub release_date: Option<String>,
    pub kind: Option<OwnWorkKind>,
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub cover: Option<String>,
    pub samples: Vec<String>,
}

/// An image file of an installed work, which can become its cover or a sample.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationImage {
    pub relative_path: RelativePath,
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOwnWorkArtworkRequest {
    pub installation_id: InstallationId,
    pub relative_path: RelativePath,
}
