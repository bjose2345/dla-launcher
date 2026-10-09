use serde::{Deserialize, Serialize};

pub mod android_app;
pub mod android_package;
pub mod delivery;
pub mod installation;
pub mod launch;
pub mod library;
pub mod maintenance;
pub mod media;
pub mod package;
pub mod scanner;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogWork {
    pub code: String,
    pub source_code: String,
    pub title: String,
    pub title_english: String,
    pub added_date: String,
    pub release_date: String,
    pub updated_date: String,
    pub age_rating: String,
    pub release_type: String,
    pub main_image_urls: Vec<String>,
    pub thumbnail_urls: Vec<String>,
    pub circles: Vec<NamedReference>,
    pub categories: Vec<Category>,
    pub tags: Vec<NamedReference>,
    #[serde(default)]
    pub synthetic: bool,
    /// The publisher's newest update of the work's archives, when the catalog
    /// records one.
    #[serde(default)]
    pub latest_update: Option<CatalogWorkUpdate>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogWorkUpdate {
    /// YYYY-MM-DD.
    pub date: String,
    /// A version the publisher gave, such as "1.3.1", without a "v" prefix.
    pub version: Option<String>,
}

impl CatalogWorkUpdate {
    pub fn new(date: String, raw_version: Option<String>) -> Self {
        Self {
            date,
            version: raw_version.as_deref().and_then(display_version),
        }
    }
}

/// The version to show for an update, or None when there is none worth
/// showing: it needs a dotted number ("1.02", "v2.35"). Bare integers are
/// leftovers of an old import, not versions the publisher gave. A leading "v"
/// or "Ver." and trailing dots are trimmed. The DLA website uses the same rule.
pub fn display_version(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    let lower = value.to_ascii_lowercase();
    for prefix in ["ver.", "ver", "v.", "v"] {
        if lower.starts_with(prefix) {
            value = value[prefix.len()..].trim_start();
            break;
        }
    }
    let value = value.trim_end_matches('.');
    let dotted = value.as_bytes().windows(3).any(|window| {
        window[0].is_ascii_digit() && window[1] == b'.' && window[2].is_ascii_digit()
    });
    dotted.then(|| value.to_owned())
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogWorkDetail {
    #[serde(flatten)]
    pub work: CatalogWork,
    pub sample_image_urls: Vec<String>,
    pub file_formats: Vec<Category>,
    pub supported_languages: Vec<Category>,
    pub miscellanies: Vec<Category>,
    pub roms: Vec<CatalogRom>,
    #[serde(default)]
    pub related_works: Vec<CatalogRelatedWork>,
    pub rating: Option<CatalogRating>,
    #[serde(default)]
    pub descriptions: CatalogDescriptions,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDescriptions {
    pub included: bool,
    pub versions: Vec<CatalogDescriptionVersion>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDescriptionVersion {
    pub version: u64,
    pub html: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRom {
    pub name: String,
    pub size: String,
    pub crc: String,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
    pub file_count: Option<u64>,
    pub update_date: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRomContents {
    pub status: String,
    pub archive_format: String,
    pub entry_count: Option<u64>,
    pub total_uncompressed_size: Option<String>,
    pub truncated: bool,
    pub entries: Vec<CatalogRomEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRomEntry {
    pub entry_index: u64,
    pub path: String,
    pub extension: String,
    pub is_directory: bool,
    pub size: Option<String>,
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
    pub hash_status: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRelation {
    pub parent_work_code: String,
    pub child_work_code: String,
    pub relation_type_code: String,
    pub relation_type_label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRelatedWork {
    pub code: String,
    pub title: String,
    pub title_english: String,
    pub relation_type_code: String,
    pub relation_type_label: String,
    pub direction: CatalogRelationDirection,
    pub thumbnail_urls: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogRelationDirection {
    Parent,
    Child,
    Sibling,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRating {
    pub score: f64,
    pub rating_count: Option<u64>,
    pub total_sales: Option<u64>,
    pub rankings: Vec<CatalogRanking>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRanking {
    pub range: String,
    pub rank: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedReference {
    pub name: String,
    pub name_english: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub code: String,
    pub name: String,
    pub name_english: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_only_dotted_publisher_versions() {
        assert_eq!(display_version("1.3.1"), Some("1.3.1".to_owned()));
        assert_eq!(display_version(" Ver.1.02. "), Some("1.02".to_owned()));
        assert_eq!(display_version("v2.35"), Some("2.35".to_owned()));
        assert_eq!(display_version("2"), None);
        assert_eq!(display_version("004"), None);
        assert_eq!(display_version(""), None);
    }
}
