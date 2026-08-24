use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogArtworkMediaType {
    Avif,
    Gif,
    Jpeg,
    Png,
    Webp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogArtworkRetention {
    #[serde(rename = "days_90", alias = "days90")]
    Days90,
    #[serde(rename = "days_180", alias = "days180")]
    Days180,
    #[serde(rename = "days_360", alias = "days360")]
    Days360,
    Never,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogArtworkCapacity {
    Standard,
    Large,
    VeryLarge,
    Unlimited,
}

impl CatalogArtworkCapacity {
    pub fn limits(self) -> Option<(u64, usize)> {
        match self {
            Self::Standard => Some((512 * 1024 * 1024, 4_000)),
            Self::Large => Some((2 * 1024 * 1024 * 1024, 16_000)),
            Self::VeryLarge => Some((8 * 1024 * 1024 * 1024, 64_000)),
            Self::Unlimited => None,
        }
    }
}

impl CatalogArtworkRetention {
    pub fn days(self) -> Option<u64> {
        match self {
            Self::Days90 => Some(90),
            Self::Days180 => Some(180),
            Self::Days360 => Some(360),
            Self::Never => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogArtworkCacheSummary {
    pub retention: CatalogArtworkRetention,
    pub capacity: CatalogArtworkCapacity,
    pub entry_count: usize,
    pub stored_bytes: u64,
    pub maximum_bytes: Option<u64>,
    pub maximum_entries: Option<usize>,
}

impl CatalogArtworkMediaType {
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Avif => "image/avif",
            Self::Gif => "image/gif",
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogArtworkCacheStatus {
    Hit,
    Miss,
    Revalidated,
    Stale,
}

impl CatalogArtworkCacheStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Revalidated => "revalidated",
            Self::Stale => "stale",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogArtworkAsset {
    pub bytes: Vec<u8>,
    pub media_type: CatalogArtworkMediaType,
    pub cache_status: CatalogArtworkCacheStatus,
}

#[derive(Debug, Error)]
pub enum CatalogArtworkError {
    #[error("the artwork source is invalid")]
    InvalidSource,
    #[error("the artwork source is not allowed")]
    SourceNotAllowed,
    #[error("the artwork was not found")]
    NotFound,
    #[error("the artwork exceeds the cache limit")]
    TooLarge,
    #[error("the response is not a supported image")]
    UnsupportedImage,
    #[error("the artwork source is unavailable: {0}")]
    SourceUnavailable(String),
    #[error("the artwork cache failed: {0}")]
    Storage(String),
}

pub trait CatalogArtworkCache: Send + Sync {
    fn load(&self, source_url: &str) -> Result<CatalogArtworkAsset, CatalogArtworkError>;
    fn summary(&self) -> Result<CatalogArtworkCacheSummary, CatalogArtworkError>;
    fn configure(
        &self,
        retention: CatalogArtworkRetention,
        capacity: CatalogArtworkCapacity,
    ) -> Result<CatalogArtworkCacheSummary, CatalogArtworkError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_wire_values_match_the_frontend_contract() {
        for (retention, wire_value) in [
            (CatalogArtworkRetention::Days90, "days_90"),
            (CatalogArtworkRetention::Days180, "days_180"),
            (CatalogArtworkRetention::Days360, "days_360"),
            (CatalogArtworkRetention::Never, "never"),
        ] {
            assert_eq!(
                serde_json::to_string(&retention).unwrap(),
                format!("\"{wire_value}\"")
            );
            assert_eq!(
                serde_json::from_str::<CatalogArtworkRetention>(&format!("\"{wire_value}\""))
                    .unwrap(),
                retention
            );
        }

        for (legacy_value, retention) in [
            ("days90", CatalogArtworkRetention::Days90),
            ("days180", CatalogArtworkRetention::Days180),
            ("days360", CatalogArtworkRetention::Days360),
        ] {
            assert_eq!(
                serde_json::from_str::<CatalogArtworkRetention>(&format!("\"{legacy_value}\""))
                    .unwrap(),
                retention
            );
        }
    }

    #[test]
    fn capacity_wire_values_match_the_frontend_contract() {
        for (capacity, wire_value) in [
            (CatalogArtworkCapacity::Standard, "standard"),
            (CatalogArtworkCapacity::Large, "large"),
            (CatalogArtworkCapacity::VeryLarge, "very_large"),
            (CatalogArtworkCapacity::Unlimited, "unlimited"),
        ] {
            assert_eq!(
                serde_json::to_string(&capacity).unwrap(),
                format!("\"{wire_value}\"")
            );
            assert_eq!(
                serde_json::from_str::<CatalogArtworkCapacity>(&format!("\"{wire_value}\""))
                    .unwrap(),
                capacity
            );
        }
    }
}
