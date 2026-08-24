use std::collections::HashSet;

use dla_application::catalog_import::{CatalogImportError, CatalogPackageProfile};
use serde_json::{Map, Value};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub const COMPACT_FIELDS: &[&str] = &[
    "work.code",
    "work.title",
    "work.source",
    "work.site",
    "work.circle.name",
    "work.releaseDate",
    "work.version",
    "work.categories",
    "work.tags",
    "work.fileFormats",
    "work.miscellanies",
    "work.languages",
    "work.drm",
    "rom.name",
    "rom.size",
    "rom.crc",
    "rom.md5",
    "rom.sha1",
    "rom.sha256",
];

pub const CONTENT_FIELDS: &[&str] = &[
    "rom.contents.path",
    "rom.contents.size",
    "rom.contents.crc32",
    "rom.contents.md5",
    "rom.contents.sha1",
    "rom.contents.sha256",
];

pub const ENRICHMENT_FIELDS: &[&str] = &[
    "work.titleEnglish",
    "work.titleKana",
    "work.titleRomaji",
    "work.addedDate",
    "work.updatedDate",
    "work.ageRating",
    "work.releaseType",
    "work.sourceUrl",
    "work.circle.code",
    "work.circle.nameEnglish",
    "work.categoryCodes",
    "work.categoryNamesEnglish",
    "work.tagCodes",
    "work.tagNamesEnglish",
    "work.fileFormatCodes",
    "work.fileFormatNamesEnglish",
    "work.languageCodes",
    "work.languageNamesEnglish",
    "work.miscellanyCodes",
    "work.miscellanyNamesEnglish",
    "work.images.main",
    "work.images.thumbnail",
    "work.images.samples",
    "work.descriptions",
    "work.rating.score",
    "work.rating.count",
    "work.rating.totalSales",
    "work.rating.favorites",
    "work.rating.rankings",
    "work.relations",
    "rom.fileCount",
    "rom.updateDate",
];

pub fn all_fields() -> Vec<&'static str> {
    COMPACT_FIELDS
        .iter()
        .chain(CONTENT_FIELDS)
        .chain(ENRICHMENT_FIELDS)
        .copied()
        .collect()
}

pub fn fields_for_profile(profile: CatalogPackageProfile) -> Vec<&'static str> {
    match profile {
        CatalogPackageProfile::Compact => COMPACT_FIELDS.to_vec(),
        CatalogPackageProfile::LegacyComplete => COMPACT_FIELDS
            .iter()
            .chain(CONTENT_FIELDS)
            .copied()
            .collect(),
        CatalogPackageProfile::Full | CatalogPackageProfile::LegacyEnriched => all_fields(),
        CatalogPackageProfile::Custom => COMPACT_FIELDS.to_vec(),
    }
}

pub fn omitted_fields(fields: &[String]) -> Vec<String> {
    let included = fields.iter().map(String::as_str).collect::<HashSet<_>>();
    all_fields()
        .into_iter()
        .filter(|field| !included.contains(field))
        .map(str::to_owned)
        .collect()
}

pub fn enrichment_record_fields(fields: &[String]) -> Vec<&str> {
    let included = fields.iter().map(String::as_str).collect::<HashSet<_>>();
    ENRICHMENT_FIELDS
        .iter()
        .filter(|field| **field != "work.relations" && included.contains(**field))
        .copied()
        .collect()
}

pub fn validate_fields(
    profile: CatalogPackageProfile,
    fields: &[String],
) -> Result<(), CatalogImportError> {
    let known = all_fields().into_iter().collect::<HashSet<_>>();
    let included = fields.iter().map(String::as_str).collect::<HashSet<_>>();
    if included.len() != fields.len() {
        return Err(CatalogImportError::invalid(
            "manifest field identifiers must be unique",
        ));
    }
    if let Some(unknown) = included.iter().find(|field| !known.contains(**field)) {
        return Err(CatalogImportError::invalid(format!(
            "manifest contains unknown field identifier {unknown}"
        )));
    }
    if let Some(missing) = COMPACT_FIELDS
        .iter()
        .find(|field| !included.contains(**field))
    {
        return Err(CatalogImportError::invalid(format!(
            "manifest is missing required DAT field {missing}"
        )));
    }
    let canonical = all_fields()
        .into_iter()
        .filter(|field| included.contains(field))
        .collect::<Vec<_>>();
    if !fields
        .iter()
        .map(String::as_str)
        .eq(canonical.iter().copied())
    {
        return Err(CatalogImportError::invalid(
            "manifest fields must follow the canonical field-registry order",
        ));
    }

    match profile {
        CatalogPackageProfile::Compact if included != COMPACT_FIELDS.iter().copied().collect() => {
            return Err(CatalogImportError::invalid(
                "Compact profile fields must exactly match the stable Compact registry",
            ));
        }
        CatalogPackageProfile::LegacyComplete
            if included
                != COMPACT_FIELDS
                    .iter()
                    .chain(CONTENT_FIELDS)
                    .copied()
                    .collect() =>
        {
            return Err(CatalogImportError::invalid(
                "legacy Complete profile fields must exactly match Compact plus internal-file fields",
            ));
        }
        CatalogPackageProfile::Full | CatalogPackageProfile::LegacyEnriched
            if included != all_fields().into_iter().collect() =>
        {
            return Err(CatalogImportError::invalid(
                "Full profile fields must exactly match the stable Full registry",
            ));
        }
        _ => {}
    }

    for field in &included {
        for dependency in field_dependencies(field) {
            if !included.contains(dependency) {
                return Err(CatalogImportError::invalid(format!(
                    "field {field} requires {dependency}"
                )));
            }
        }
    }
    Ok(())
}

pub fn field_dependencies(field: &str) -> &'static [&'static str] {
    match field {
        "rom.contents.path" => &["rom.contents.size"],
        "rom.contents.size"
        | "rom.contents.crc32"
        | "rom.contents.md5"
        | "rom.contents.sha1"
        | "rom.contents.sha256" => &["rom.contents.path"],
        "work.circle.code" | "work.circle.nameEnglish" => &["work.circle.name"],
        "work.categoryCodes" | "work.categoryNamesEnglish" => &["work.categories"],
        "work.tagCodes" | "work.tagNamesEnglish" => &["work.tags"],
        "work.fileFormatCodes" | "work.fileFormatNamesEnglish" => &["work.fileFormats"],
        "work.languageCodes" | "work.languageNamesEnglish" => &["work.languages"],
        "work.miscellanyCodes" | "work.miscellanyNamesEnglish" => &["work.miscellanies"],
        "work.rating.count"
        | "work.rating.totalSales"
        | "work.rating.favorites"
        | "work.rating.rankings" => &["work.rating.score"],
        _ => &[],
    }
}

pub fn validate_enrichment_values(fields: &Map<String, Value>) -> Result<(), CatalogImportError> {
    for (field, value) in fields {
        if value.is_null() {
            continue;
        }

        let valid = match field.as_str() {
            "work.titleEnglish"
            | "work.titleKana"
            | "work.titleRomaji"
            | "work.ageRating"
            | "work.releaseType"
            | "work.sourceUrl"
            | "work.circle.code"
            | "work.circle.nameEnglish" => value.is_string(),
            "work.addedDate" | "work.updatedDate" => value
                .as_str()
                .is_some_and(|value| OffsetDateTime::parse(value, &Rfc3339).is_ok()),
            "work.categoryCodes"
            | "work.categoryNamesEnglish"
            | "work.tagCodes"
            | "work.tagNamesEnglish"
            | "work.fileFormatCodes"
            | "work.fileFormatNamesEnglish"
            | "work.languageCodes"
            | "work.languageNamesEnglish"
            | "work.miscellanyCodes"
            | "work.miscellanyNamesEnglish"
            | "work.images.main"
            | "work.images.thumbnail"
            | "work.images.samples" => is_string_array(value),
            "work.descriptions" => is_description_array(value),
            "work.rating.score" => value
                .as_f64()
                .is_some_and(|score| score.is_finite() && (0.0..=5.0).contains(&score)),
            "work.rating.count" | "work.rating.totalSales" | "work.rating.favorites" => {
                value.as_u64().is_some()
            }
            "work.rating.rankings" => is_ranking_array(value),
            "rom.fileCount" => {
                is_rom_value_array(value, |value| value.is_null() || value.as_u64().is_some())
            }
            "rom.updateDate" => is_rom_value_array(value, |value| {
                value.is_null() || value.as_str().is_some_and(is_date)
            }),
            _ => false,
        };

        if !valid {
            return Err(CatalogImportError::invalid(format!(
                "enrichment field {field} has an invalid value"
            )));
        }
    }
    Ok(())
}

fn is_string_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|values| values.iter().all(Value::is_string))
}

fn is_description_array(value: &Value) -> bool {
    value.as_array().is_some_and(|values| {
        values.iter().all(|entry| {
            entry.as_object().is_some_and(|entry| {
                entry.len() == 2
                    && entry
                        .get("version")
                        .and_then(Value::as_u64)
                        .is_some_and(|v| v > 0)
                    && entry.get("html").is_some_and(Value::is_string)
            })
        })
    })
}

fn is_ranking_array(value: &Value) -> bool {
    value.as_array().is_some_and(|values| {
        values.iter().all(|entry| {
            entry.as_object().is_some_and(|entry| {
                entry.len() == 2
                    && entry
                        .get("range")
                        .and_then(Value::as_str)
                        .is_some_and(|range| !range.trim().is_empty())
                    && entry
                        .get("rank")
                        .and_then(Value::as_u64)
                        .is_some_and(|rank| rank > 0)
            })
        })
    })
}

fn is_rom_value_array(value: &Value, value_is_valid: impl Fn(&Value) -> bool) -> bool {
    let Some(entries) = value.as_array() else {
        return false;
    };
    let mut positions = HashSet::new();
    entries.iter().all(|entry| {
        entry.as_object().is_some_and(|entry| {
            entry.len() == 2
                && entry
                    .get("position")
                    .and_then(Value::as_u64)
                    .is_some_and(|position| positions.insert(position))
                && entry.get("value").is_some_and(&value_is_valid)
        })
    })
}

fn is_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let parse =
        |range: std::ops::Range<usize>| value.get(range).and_then(|part| part.parse::<u32>().ok());
    matches!(parse(0..4), Some(year) if year > 0)
        && matches!(parse(5..7), Some(month) if (1..=12).contains(&month))
        && matches!(parse(8..10), Some(day) if (1..=31).contains(&day))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FieldRegistry {
        registry_version: u32,
        fields: Vec<FieldDefinition>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FieldDefinition {
        id: String,
        depends_on: Vec<String>,
    }

    #[test]
    fn custom_fields_enforce_dependencies() {
        let mut fields = COMPACT_FIELDS
            .iter()
            .map(|field| (*field).to_owned())
            .collect::<Vec<_>>();
        fields.push("rom.contents.sha256".to_owned());
        assert!(validate_fields(CatalogPackageProfile::Custom, &fields).is_err());
        fields.push("rom.contents.path".to_owned());
        fields.push("rom.contents.size".to_owned());
        let included = fields.iter().map(String::as_str).collect::<HashSet<_>>();
        fields = all_fields()
            .into_iter()
            .filter(|field| included.contains(field))
            .map(str::to_owned)
            .collect();
        assert!(validate_fields(CatalogPackageProfile::Custom, &fields).is_ok());
    }

    #[test]
    fn full_and_legacy_profiles_keep_their_field_contracts() {
        let compact = COMPACT_FIELDS
            .iter()
            .map(|field| (*field).to_owned())
            .collect::<Vec<_>>();
        let complete = COMPACT_FIELDS
            .iter()
            .chain(CONTENT_FIELDS)
            .map(|field| (*field).to_owned())
            .collect::<Vec<_>>();
        let full = all_fields()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();

        assert!(validate_fields(CatalogPackageProfile::Full, &full).is_ok());
        assert!(validate_fields(CatalogPackageProfile::Full, &complete).is_err());
        assert!(validate_fields(CatalogPackageProfile::LegacyComplete, &complete).is_ok());
        assert!(validate_fields(CatalogPackageProfile::LegacyComplete, &full).is_err());
        assert!(validate_fields(CatalogPackageProfile::LegacyEnriched, &full).is_ok());
        assert!(validate_fields(CatalogPackageProfile::LegacyEnriched, &compact).is_err());
    }

    #[test]
    fn executable_registry_matches_the_shared_contract() {
        let registry: FieldRegistry = serde_json::from_str(include_str!(
            "../../../../shared/contracts/catalog-package/v1/field-registry.json"
        ))
        .expect("field registry");
        assert_eq!(registry.registry_version, 1);
        assert_eq!(
            registry
                .fields
                .iter()
                .map(|field| field.id.as_str())
                .collect::<Vec<_>>(),
            all_fields()
        );
        for field in registry.fields {
            assert_eq!(
                field.depends_on,
                field_dependencies(&field.id)
                    .iter()
                    .map(|dependency| (*dependency).to_owned())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn enrichment_values_are_typed_and_per_rom_positions_are_unique() {
        let valid = serde_json::from_value::<Map<String, Value>>(json!({
            "work.addedDate": "2026-08-05T00:00:00Z",
            "work.images.main": ["https://example.invalid/main.webp"],
            "work.rating.score": 4.5,
            "work.rating.rankings": [{"range": "24h", "rank": 2}],
            "rom.fileCount": [
                {"position": 0, "value": 3},
                {"position": 1, "value": null}
            ],
            "rom.updateDate": [
                {"position": 0, "value": "2026-08-05"},
                {"position": 1, "value": null}
            ]
        }))
        .expect("field map");
        assert!(validate_enrichment_values(&valid).is_ok());

        let duplicate_positions = serde_json::from_value::<Map<String, Value>>(json!({
            "rom.fileCount": [
                {"position": 0, "value": 3},
                {"position": 0, "value": 4}
            ]
        }))
        .expect("field map");
        assert!(validate_enrichment_values(&duplicate_positions).is_err());

        let invalid_rating = serde_json::from_value::<Map<String, Value>>(json!({
            "work.rating.score": 94
        }))
        .expect("field map");
        assert!(validate_enrichment_values(&invalid_rating).is_err());
    }
}
