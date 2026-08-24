use std::{
    collections::HashSet,
    fs::File,
    io::Read,
    path::{Component, Path},
};

use dla_application::catalog_import::{
    CATALOG_PACKAGE_FORMAT, CATALOG_PACKAGE_FORMAT_VERSION, CatalogImportError,
    CatalogPackageManifest, CatalogPackageProfile, CatalogPayloadKind,
};
use fs2::available_space;
use tar::Archive;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use zstd::stream::read::Decoder;

use crate::fields::{CONTENT_FIELDS, ENRICHMENT_FIELDS, omitted_fields, validate_fields};

pub(crate) const MAXIMUM_MANIFEST_BYTES: u64 = 1024 * 1024;
const IMPORT_HEADROOM_BYTES: u64 = 128 * 1024 * 1024;

pub struct InspectedPackage {
    pub manifest: CatalogPackageManifest,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub required_disk_bytes: u64,
    pub available_disk_bytes: u64,
    pub blocking_issues: Vec<String>,
    pub warnings: Vec<String>,
    pub omitted_fields: Vec<String>,
}

pub fn inspect_package(
    package_path: &Path,
    data_directory: &Path,
) -> Result<InspectedPackage, CatalogImportError> {
    let compressed_bytes = package_path
        .metadata()
        .map_err(CatalogImportError::access)?
        .len();
    let manifest = read_manifest(package_path)?;
    let uncompressed_bytes = manifest
        .payloads
        .iter()
        .try_fold(0_u64, |total, payload| {
            total.checked_add(payload.uncompressed_bytes)
        })
        .ok_or_else(|| CatalogImportError::invalid("payload size total overflows u64"))?;
    let required_disk_bytes = uncompressed_bytes
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(IMPORT_HEADROOM_BYTES))
        .ok_or_else(|| CatalogImportError::invalid("required disk estimate overflows u64"))?;
    let available_disk_bytes = available_space(data_directory).unwrap_or(0);
    let mut blocking_issues = validate_manifest(&manifest);
    if available_disk_bytes != 0 && available_disk_bytes < required_disk_bytes {
        blocking_issues.push(format!(
            "import requires approximately {required_disk_bytes} bytes but only {available_disk_bytes} bytes are available"
        ));
    }
    let omitted_fields = omitted_fields(&manifest.fields);
    let warnings = omission_warnings(&manifest, &omitted_fields);
    Ok(InspectedPackage {
        manifest,
        compressed_bytes,
        uncompressed_bytes,
        required_disk_bytes,
        available_disk_bytes,
        blocking_issues,
        warnings,
        omitted_fields,
    })
}

pub fn read_manifest(package_path: &Path) -> Result<CatalogPackageManifest, CatalogImportError> {
    let file = File::open(package_path).map_err(CatalogImportError::access)?;
    let decoder = Decoder::new(file).map_err(CatalogImportError::invalid)?;
    let mut archive = Archive::new(decoder);
    let mut entries = archive.entries().map_err(CatalogImportError::invalid)?;
    let mut entry = entries
        .next()
        .ok_or_else(|| CatalogImportError::invalid("catalog package is empty"))?
        .map_err(CatalogImportError::invalid)?;
    let path = entry.path().map_err(CatalogImportError::invalid)?;
    if path.as_ref() != Path::new("manifest.json") {
        return Err(CatalogImportError::invalid(
            "manifest.json must be the first package entry",
        ));
    }
    if !entry.header().entry_type().is_file() {
        return Err(CatalogImportError::invalid(
            "manifest.json must be a regular file",
        ));
    }
    if entry.size() > MAXIMUM_MANIFEST_BYTES {
        return Err(CatalogImportError::invalid(
            "manifest.json exceeds the 1 MiB safety limit",
        ));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut bytes)
        .map_err(CatalogImportError::invalid)?;
    serde_json::from_slice(&bytes).map_err(CatalogImportError::invalid)
}

pub fn validate_manifest(manifest: &CatalogPackageManifest) -> Vec<String> {
    let mut issues = Vec::new();
    if manifest.format != CATALOG_PACKAGE_FORMAT {
        issues.push(format!(
            "unsupported package format {}; expected {CATALOG_PACKAGE_FORMAT}",
            manifest.format
        ));
    }
    if manifest.format_version != CATALOG_PACKAGE_FORMAT_VERSION {
        issues.push(format!(
            "unsupported package format version {}; this launcher supports {}",
            manifest.format_version, CATALOG_PACKAGE_FORMAT_VERSION
        ));
    }
    if manifest.catalog_schema_version != 1 {
        issues.push(format!(
            "unsupported catalog schema version {}",
            manifest.catalog_schema_version
        ));
    }
    match version_is_supported(
        env!("CARGO_PKG_VERSION"),
        &manifest.minimum_launcher_version,
    ) {
        Ok(true) => {}
        Ok(false) => issues.push(format!(
            "package requires launcher version {} or newer",
            manifest.minimum_launcher_version
        )),
        Err(error) => issues.push(error),
    }
    if manifest.snapshot_id.trim().is_empty() {
        issues.push("snapshotId must not be empty".to_owned());
    } else if manifest.snapshot_id.len() > 128
        || !manifest
            .snapshot_id
            .bytes()
            .enumerate()
            .all(|(index, byte)| {
                byte.is_ascii_alphanumeric()
                    || (index > 0 && matches!(byte, b'.' | b'_' | b':' | b'-'))
            })
    {
        issues.push("snapshotId contains unsupported characters".to_owned());
    }
    if OffsetDateTime::parse(&manifest.created_at, &Rfc3339).is_err() {
        issues.push("createdAt must be an RFC 3339 timestamp".to_owned());
    }
    if manifest.source.id.trim().is_empty() || manifest.source.name.trim().is_empty() {
        issues.push("package source id and name must not be empty".to_owned());
    }
    if let Err(error) = validate_fields(manifest.profile, &manifest.fields) {
        issues.push(error.to_string());
    }
    validate_payloads(manifest, &mut issues);
    issues
}

fn validate_payloads(manifest: &CatalogPackageManifest, issues: &mut Vec<String>) {
    let mut paths = HashSet::new();
    let mut dat_count = 0;
    let mut enrichment_count = 0_u64;
    let mut relation_count = 0_u64;
    let mut previous_order = None;
    for payload in &manifest.payloads {
        if !paths.insert(payload.path.as_str()) {
            issues.push(format!("duplicate payload path {}", payload.path));
        }
        if !safe_relative_path(&payload.path) {
            issues.push(format!("unsafe payload path {}", payload.path));
        }
        if payload.sha256.len() != 64
            || !payload
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            issues.push(format!("payload {} has an invalid SHA-256", payload.path));
        }
        let order = payload_order(payload.kind, &payload.path);
        if previous_order
            .as_ref()
            .is_some_and(|previous| previous >= &order)
        {
            issues.push(
                "payloads must be ordered as catalog.dat, enrichment chunks, then relation chunks"
                    .to_owned(),
            );
        }
        previous_order = Some(order);
        match payload.kind {
            CatalogPayloadKind::Dat => {
                dat_count += 1;
                if payload.path != "catalog.dat" {
                    issues.push("the DAT payload path must be catalog.dat".to_owned());
                }
                if payload.media_type != "application/xml" {
                    issues.push("catalog.dat must use media type application/xml".to_owned());
                }
                if payload.records != manifest.counts.work_entries {
                    issues.push("the DAT record count must equal counts.workEntries".to_owned());
                }
            }
            CatalogPayloadKind::Enrichment => {
                enrichment_count = enrichment_count.saturating_add(payload.records);
                if !chunk_path(&payload.path, "enrichment") {
                    issues.push(format!(
                        "enrichment payload {} must use enrichment/NNNNNN.ndjson",
                        payload.path
                    ));
                }
                if payload.media_type != "application/x-ndjson" {
                    issues.push(format!(
                        "enrichment payload {} must use media type application/x-ndjson",
                        payload.path
                    ));
                }
            }
            CatalogPayloadKind::Relations => {
                relation_count = relation_count.saturating_add(payload.records);
                if !chunk_path(&payload.path, "relations") {
                    issues.push(format!(
                        "relation payload {} must use relations/NNNNNN.ndjson",
                        payload.path
                    ));
                }
                if payload.media_type != "application/x-ndjson" {
                    issues.push(format!(
                        "relation payload {} must use media type application/x-ndjson",
                        payload.path
                    ));
                }
            }
        }
    }
    if dat_count != 1 {
        issues.push("package must contain exactly one DAT payload".to_owned());
    }
    let fields = manifest
        .fields
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let has_content = CONTENT_FIELDS.iter().any(|field| fields.contains(field));
    let has_enrichment = ENRICHMENT_FIELDS
        .iter()
        .filter(|field| **field != "work.relations")
        .any(|field| fields.contains(field));
    let has_relations = fields.contains("work.relations");
    let has_enrichment_payload = manifest
        .payloads
        .iter()
        .any(|payload| payload.kind == CatalogPayloadKind::Enrichment);
    let has_relation_payload = manifest
        .payloads
        .iter()
        .any(|payload| payload.kind == CatalogPayloadKind::Relations);
    if has_content && matches!(manifest.profile, CatalogPackageProfile::Compact) {
        issues.push("Compact packages cannot declare internal-file fields".to_owned());
    }
    if has_enrichment != has_enrichment_payload {
        issues.push(
            "enrichment payload presence must match the selected enrichment fields".to_owned(),
        );
    }
    if has_relations != has_relation_payload {
        issues.push("relation payload presence must match work.relations".to_owned());
    }
    if has_enrichment && enrichment_count != manifest.counts.unique_works {
        issues.push("enrichment record counts must equal counts.uniqueWorks".to_owned());
    }
    if relation_count != manifest.counts.relations {
        issues.push("relation record counts must equal counts.relations".to_owned());
    }
    if manifest.counts.unique_works > manifest.counts.work_entries {
        issues.push("counts.uniqueWorks cannot exceed counts.workEntries".to_owned());
    }
    if !has_content && manifest.counts.files != 0 {
        issues.push("counts.files must be zero when internal-file fields are omitted".to_owned());
    }
}

fn payload_order(kind: CatalogPayloadKind, path: &str) -> (u8, &str) {
    let rank = match kind {
        CatalogPayloadKind::Dat => 0,
        CatalogPayloadKind::Enrichment => 1,
        CatalogPayloadKind::Relations => 2,
    };
    (rank, path)
}

fn chunk_path(path: &str, directory: &str) -> bool {
    let Some(filename) = path.strip_prefix(&format!("{directory}/")) else {
        return false;
    };
    let Some(number) = filename.strip_suffix(".ndjson") else {
        return false;
    };
    number.len() == 6 && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn version_is_supported(current: &str, minimum: &str) -> Result<bool, String> {
    let current = parse_version(current)?;
    let minimum = parse_version(minimum)?;
    Ok(current >= minimum)
}

fn parse_version(value: &str) -> Result<(u64, u64, u64), String> {
    let core = value.split_once('-').map_or(value, |(core, _)| core);
    let components = core
        .split('.')
        .map(|component| component.parse::<u64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("invalid semantic version {value}"))?;
    if components.len() != 3 {
        return Err(format!("invalid semantic version {value}"));
    }
    Ok((components[0], components[1], components[2]))
}

fn omission_warnings(manifest: &CatalogPackageManifest, omitted_fields: &[String]) -> Vec<String> {
    let omitted = omitted_fields
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut warnings = Vec::new();
    if omitted.contains("work.images.main") {
        warnings.push("Cover and gallery images were not included; image surfaces will show an unavailable state.".to_owned());
    }
    if omitted.contains("work.descriptions") {
        warnings.push(
            "Descriptions were not included; the detail page will identify them as not imported."
                .to_owned(),
        );
    }
    if omitted.contains("work.relations") {
        warnings.push(
            "Work relations were not included; related-work sections will be unavailable."
                .to_owned(),
        );
    }
    if omitted.contains("rom.contents.path") {
        warnings.push("Internal archive contents were not included; only package-level hashes can be matched.".to_owned());
    }
    if manifest.counts.unique_works == 0 {
        warnings.push("The manifest declares an empty catalog.".to_owned());
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use dla_application::catalog_import::{
        CatalogPackageCounts, CatalogPackageSource, CatalogPayloadDescriptor,
    };

    #[test]
    fn rejects_incompatible_profile_payload_combinations() {
        let manifest = CatalogPackageManifest {
            format: CATALOG_PACKAGE_FORMAT.to_owned(),
            format_version: 1,
            catalog_schema_version: 1,
            minimum_launcher_version: "0.1.0".to_owned(),
            snapshot_id: "fixture".to_owned(),
            created_at: "2026-08-05T00:00:00Z".to_owned(),
            profile: CatalogPackageProfile::Compact,
            source: CatalogPackageSource {
                id: "DL".to_owned(),
                name: "DLsite".to_owned(),
            },
            fields: crate::fields::COMPACT_FIELDS
                .iter()
                .map(|field| (*field).to_owned())
                .collect(),
            counts: CatalogPackageCounts::default(),
            payloads: vec![CatalogPayloadDescriptor {
                path: "catalog.dat".to_owned(),
                kind: CatalogPayloadKind::Dat,
                media_type: "application/xml".to_owned(),
                records: 0,
                uncompressed_bytes: 0,
                sha256: "a".repeat(64),
            }],
        };
        assert!(validate_manifest(&manifest).is_empty());
    }

    #[test]
    fn manifest_json_requires_exact_v1_properties() {
        let manifest = CatalogPackageManifest {
            format: CATALOG_PACKAGE_FORMAT.to_owned(),
            format_version: 1,
            catalog_schema_version: 1,
            minimum_launcher_version: "0.1.0".to_owned(),
            snapshot_id: "fixture".to_owned(),
            created_at: "2026-08-05T00:00:00Z".to_owned(),
            profile: CatalogPackageProfile::Compact,
            source: CatalogPackageSource {
                id: "DL".to_owned(),
                name: "DLsite".to_owned(),
            },
            fields: crate::fields::COMPACT_FIELDS
                .iter()
                .map(|field| (*field).to_owned())
                .collect(),
            counts: CatalogPackageCounts::default(),
            payloads: vec![CatalogPayloadDescriptor {
                path: "catalog.dat".to_owned(),
                kind: CatalogPayloadKind::Dat,
                media_type: "application/xml".to_owned(),
                records: 0,
                uncompressed_bytes: 0,
                sha256: "a".repeat(64),
            }],
        };
        let mut unknown = serde_json::to_value(&manifest).expect("manifest JSON");
        unknown["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<CatalogPackageManifest>(unknown).is_err());

        let mut missing = serde_json::to_value(&manifest).expect("manifest JSON");
        missing["counts"]
            .as_object_mut()
            .expect("counts object")
            .remove("roms");
        assert!(serde_json::from_value::<CatalogPackageManifest>(missing).is_err());
    }
}
