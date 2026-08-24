use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::Path,
    rc::Rc,
};

use dla_application::catalog_import::{
    CatalogImportCancellationToken, CatalogImportCounters, CatalogImportError, CatalogImportStage,
    CatalogPackageManifest, CatalogPayloadKind,
};
use dla_domain::CatalogRelation;
use dla_sqlite::SqliteCatalogImportWriter;
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tar::Archive;
use zstd::stream::read::Decoder;

use crate::{
    dat::import_dat,
    fields::{enrichment_record_fields, validate_enrichment_values},
    package::MAXIMUM_MANIFEST_BYTES,
};

const MAXIMUM_CHECKSUM_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAXIMUM_NDJSON_RECORD_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PayloadImportStats {
    pub counters: CatalogImportCounters,
    pub payload_hashes: HashMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct EnrichmentRecord {
    work_code: String,
    fields: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RelationRecord {
    parent_work_code: String,
    child_work_code: String,
    relation_type_code: String,
    relation_type_label: String,
}

pub fn import_package_payloads(
    package_path: &Path,
    manifest: &CatalogPackageManifest,
    writer: &mut SqliteCatalogImportWriter,
    cancellation: &CatalogImportCancellationToken,
    mut on_progress: impl FnMut(
        CatalogImportStage,
        &str,
        &CatalogImportCounters,
    ) -> Result<(), CatalogImportError>,
) -> Result<PayloadImportStats, CatalogImportError> {
    let file = File::open(package_path).map_err(CatalogImportError::access)?;
    let decoder = Decoder::new(file).map_err(CatalogImportError::invalid)?;
    let mut archive = Archive::new(decoder);
    let mut entries = archive.entries().map_err(CatalogImportError::invalid)?;
    let mut manifest_entry = entries
        .next()
        .ok_or_else(|| CatalogImportError::invalid("catalog package is empty"))?
        .map_err(CatalogImportError::invalid)?;
    if manifest_entry
        .path()
        .map_err(CatalogImportError::invalid)?
        .as_ref()
        != Path::new("manifest.json")
    {
        return Err(CatalogImportError::invalid(
            "manifest.json must be the first package entry",
        ));
    }
    if !manifest_entry.header().entry_type().is_file() {
        return Err(CatalogImportError::invalid(
            "manifest.json must be a regular file",
        ));
    }
    if manifest_entry.size() > MAXIMUM_MANIFEST_BYTES {
        return Err(CatalogImportError::invalid(
            "manifest.json exceeds the 1 MiB safety limit",
        ));
    }
    let mut manifest_bytes = Vec::with_capacity(manifest_entry.size() as usize);
    manifest_entry
        .read_to_end(&mut manifest_bytes)
        .map_err(CatalogImportError::invalid)?;
    let archived_manifest: CatalogPackageManifest =
        serde_json::from_slice(&manifest_bytes).map_err(CatalogImportError::invalid)?;
    if archived_manifest != *manifest {
        return Err(CatalogImportError::invalid(
            "manifest.json changed after package inspection",
        ));
    }
    drop(manifest_entry);

    let payloads = manifest
        .payloads
        .iter()
        .map(|payload| (payload.path.as_str(), payload))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let expected_enrichment_fields = enrichment_record_fields(&manifest.fields)
        .into_iter()
        .collect::<HashSet<_>>();
    let mut enriched_work_codes = HashSet::new();
    let mut checksums = None;
    let mut payload_index = 0_usize;
    let mut stats = PayloadImportStats {
        counters: CatalogImportCounters {
            total_bytes: manifest
                .payloads
                .iter()
                .map(|payload| payload.uncompressed_bytes)
                .sum(),
            ..CatalogImportCounters::default()
        },
        payload_hashes: HashMap::new(),
    };

    for entry in entries {
        if cancellation.is_cancelled() {
            return Err(CatalogImportError::Cancelled);
        }
        let entry = entry.map_err(CatalogImportError::invalid)?;
        if !entry.header().entry_type().is_file() {
            return Err(CatalogImportError::invalid(
                "catalog packages may contain only regular files",
            ));
        }
        let path = entry
            .path()
            .map_err(CatalogImportError::invalid)?
            .into_owned();
        let path_text = path
            .to_str()
            .ok_or_else(|| CatalogImportError::invalid("package path is not UTF-8"))?
            .to_owned();
        if path == Path::new("checksums.sha256") {
            if checksums.is_some() {
                return Err(CatalogImportError::invalid(
                    "catalog package contains duplicate checksums.sha256",
                ));
            }
            if entry.size() > MAXIMUM_CHECKSUM_FILE_BYTES {
                return Err(CatalogImportError::invalid(
                    "checksums.sha256 exceeds the 4 MiB safety limit",
                ));
            }
            checksums = Some(read_checksum_file(entry)?);
            continue;
        }
        if checksums.is_some() {
            return Err(CatalogImportError::invalid(
                "checksums.sha256 must be the final package entry",
            ));
        }
        let descriptor = payloads.get(path_text.as_str()).ok_or_else(|| {
            CatalogImportError::invalid(format!("undeclared package payload {path_text}"))
        })?;
        let expected_path = manifest
            .payloads
            .get(payload_index)
            .map(|payload| payload.path.as_str())
            .unwrap_or("checksums.sha256");
        if path_text != expected_path {
            return Err(CatalogImportError::invalid(format!(
                "package payload {path_text} is out of order; expected {expected_path}"
            )));
        }
        payload_index += 1;
        if !seen.insert(path_text.clone()) {
            return Err(CatalogImportError::invalid(format!(
                "duplicate package payload {path_text}"
            )));
        }
        if entry.size() != descriptor.uncompressed_bytes {
            return Err(CatalogImportError::invalid(format!(
                "payload {path_text} declares {} bytes but tar contains {}",
                descriptor.uncompressed_bytes,
                entry.size()
            )));
        }
        let stage = stage_for(descriptor.kind);
        on_progress(stage, &path_text, &stats.counters)?;
        let processed_before_payload = stats.counters.processed_bytes;
        let mut digest = DigestReader::new(entry);
        let streamed_bytes = digest.byte_counter();
        let records = match descriptor.kind {
            CatalogPayloadKind::Dat => {
                let dat = import_dat(
                    BufReader::new(&mut digest),
                    manifest,
                    writer,
                    cancellation,
                    |dat| {
                        stats.counters.work_entries = dat.work_entries;
                        stats.counters.unique_works = dat.unique_works;
                        stats.counters.roms = dat.roms;
                        stats.counters.files = dat.files;
                        stats.counters.processed_bytes =
                            processed_before_payload.saturating_add(streamed_bytes.get());
                        on_progress(stage, &path_text, &stats.counters)
                    },
                )?;
                stats.counters.work_entries = dat.work_entries;
                stats.counters.unique_works = dat.unique_works;
                stats.counters.roms = dat.roms;
                stats.counters.files = dat.files;
                dat.work_entries
            }
            CatalogPayloadKind::Enrichment => import_enrichment(
                BufReader::new(&mut digest),
                writer,
                cancellation,
                &expected_enrichment_fields,
                &mut enriched_work_codes,
                |_| {
                    stats.counters.processed_bytes =
                        processed_before_payload.saturating_add(streamed_bytes.get());
                    on_progress(stage, &path_text, &stats.counters)
                },
            )?,
            CatalogPayloadKind::Relations => {
                let imported_relations = stats.counters.relations;
                import_relations(
                    BufReader::new(&mut digest),
                    writer,
                    cancellation,
                    |records| {
                        stats.counters.relations = imported_relations + records;
                        stats.counters.processed_bytes =
                            processed_before_payload.saturating_add(streamed_bytes.get());
                        on_progress(stage, &path_text, &stats.counters)
                    },
                )?
            }
        };
        let (digest, bytes) = digest.finish();
        if records != descriptor.records {
            return Err(CatalogImportError::invalid(format!(
                "payload {path_text} declares {} records but contains {records}",
                descriptor.records
            )));
        }
        if bytes != descriptor.uncompressed_bytes {
            return Err(CatalogImportError::invalid(format!(
                "payload {path_text} ended after {bytes} bytes; expected {}",
                descriptor.uncompressed_bytes
            )));
        }
        if !digest.eq_ignore_ascii_case(&descriptor.sha256) {
            return Err(CatalogImportError::invalid(format!(
                "payload {path_text} failed SHA-256 validation"
            )));
        }
        stats.counters.processed_bytes = processed_before_payload.saturating_add(bytes);
        stats.payload_hashes.insert(path_text.clone(), digest);
        on_progress(stage, &path_text, &stats.counters)?;
    }

    if seen.len() != payloads.len() {
        let missing = payloads
            .keys()
            .find(|path| !seen.contains(**path))
            .copied()
            .unwrap_or("unknown");
        return Err(CatalogImportError::invalid(format!(
            "package is missing payload {missing}"
        )));
    }
    if !expected_enrichment_fields.is_empty()
        && enriched_work_codes.len() as u64 != manifest.counts.unique_works
    {
        return Err(CatalogImportError::invalid(format!(
            "enrichment covers {} unique works; expected {}",
            enriched_work_codes.len(),
            manifest.counts.unique_works
        )));
    }
    let checksums = checksums
        .ok_or_else(|| CatalogImportError::invalid("package is missing checksums.sha256"))?;
    validate_checksum_file(&checksums, &stats.payload_hashes)?;
    Ok(stats)
}

fn import_enrichment<R: BufRead>(
    mut input: R,
    writer: &SqliteCatalogImportWriter,
    cancellation: &CatalogImportCancellationToken,
    expected_fields: &HashSet<&str>,
    seen_work_codes: &mut HashSet<String>,
    mut on_progress: impl FnMut(u64) -> Result<(), CatalogImportError>,
) -> Result<u64, CatalogImportError> {
    let mut records = 0_u64;
    let mut line = String::new();
    loop {
        if cancellation.is_cancelled() {
            return Err(CatalogImportError::Cancelled);
        }
        line.clear();
        if read_bounded_line(&mut input, &mut line)? == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let record: EnrichmentRecord =
            serde_json::from_str(&line).map_err(CatalogImportError::invalid)?;
        if record.work_code.trim().is_empty() {
            return Err(CatalogImportError::invalid(
                "enrichment workCode must not be empty",
            ));
        }
        let actual_fields = record
            .fields
            .keys()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        if actual_fields != *expected_fields {
            let missing = expected_fields
                .difference(&actual_fields)
                .copied()
                .collect::<Vec<_>>();
            let unexpected = actual_fields
                .difference(expected_fields)
                .copied()
                .collect::<Vec<_>>();
            return Err(CatalogImportError::invalid(format!(
                "enrichment for {} has a field-set mismatch; missing [{}], unexpected [{}]",
                record.work_code,
                missing.join(", "),
                unexpected.join(", ")
            )));
        }
        validate_enrichment_values(&record.fields)?;
        if !seen_work_codes.insert(record.work_code.to_ascii_lowercase()) {
            return Err(CatalogImportError::invalid(format!(
                "duplicate enrichment record for {}",
                record.work_code
            )));
        }
        writer
            .apply_enrichment(&record.work_code, &record.fields)
            .map_err(CatalogImportError::persistence)?;
        records += 1;
        if records.is_multiple_of(128) {
            on_progress(records)?;
        }
    }
    on_progress(records)?;
    Ok(records)
}

fn import_relations<R: BufRead>(
    mut input: R,
    writer: &SqliteCatalogImportWriter,
    cancellation: &CatalogImportCancellationToken,
    mut on_progress: impl FnMut(u64) -> Result<(), CatalogImportError>,
) -> Result<u64, CatalogImportError> {
    let mut records = 0_u64;
    let mut line = String::new();
    loop {
        if cancellation.is_cancelled() {
            return Err(CatalogImportError::Cancelled);
        }
        line.clear();
        if read_bounded_line(&mut input, &mut line)? == 0 {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let relation: RelationRecord =
            serde_json::from_str(&line).map_err(CatalogImportError::invalid)?;
        if [
            &relation.parent_work_code,
            &relation.child_work_code,
            &relation.relation_type_code,
            &relation.relation_type_label,
        ]
        .into_iter()
        .any(|value| value.trim().is_empty())
        {
            return Err(CatalogImportError::invalid(
                "relation fields must not be empty",
            ));
        }
        if relation
            .parent_work_code
            .eq_ignore_ascii_case(&relation.child_work_code)
        {
            return Err(CatalogImportError::invalid(format!(
                "relation record {} links work {} to itself",
                records + 1,
                relation.parent_work_code
            )));
        }
        let relation = CatalogRelation {
            parent_work_code: relation.parent_work_code,
            child_work_code: relation.child_work_code,
            relation_type_code: relation.relation_type_code,
            relation_type_label: relation.relation_type_label,
        };
        writer
            .insert_relation(&relation)
            .map_err(CatalogImportError::persistence)?;
        records += 1;
        if records.is_multiple_of(128) {
            on_progress(records)?;
        }
    }
    on_progress(records)?;
    Ok(records)
}

fn read_bounded_line<R: BufRead>(
    input: &mut R,
    line: &mut String,
) -> Result<usize, CatalogImportError> {
    let mut bounded = input.take(MAXIMUM_NDJSON_RECORD_BYTES + 1);
    let bytes = bounded
        .read_line(line)
        .map_err(CatalogImportError::invalid)?;
    if bytes as u64 > MAXIMUM_NDJSON_RECORD_BYTES {
        return Err(CatalogImportError::invalid(
            "NDJSON record exceeds the 16 MiB safety limit",
        ));
    }
    Ok(bytes)
}

fn read_checksum_file<R: Read>(
    mut input: R,
) -> Result<HashMap<String, String>, CatalogImportError> {
    let mut value = String::new();
    input
        .read_to_string(&mut value)
        .map_err(CatalogImportError::invalid)?;
    let mut checksums = HashMap::new();
    for line in value.lines().filter(|line| !line.trim().is_empty()) {
        let (digest, path) = line
            .split_once("  ")
            .ok_or_else(|| CatalogImportError::invalid("invalid checksums.sha256 line"))?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(CatalogImportError::invalid(
                "invalid SHA-256 in checksums.sha256",
            ));
        }
        if checksums
            .insert(path.to_owned(), digest.to_owned())
            .is_some()
        {
            return Err(CatalogImportError::invalid(format!(
                "duplicate checksum path {path}"
            )));
        }
    }
    Ok(checksums)
}

fn validate_checksum_file(
    checksums: &HashMap<String, String>,
    actual: &HashMap<String, String>,
) -> Result<(), CatalogImportError> {
    if checksums.len() != actual.len() {
        return Err(CatalogImportError::invalid(
            "checksums.sha256 does not cover exactly the declared payloads",
        ));
    }
    for (path, digest) in actual {
        let expected = checksums.get(path).ok_or_else(|| {
            CatalogImportError::invalid(format!("checksums.sha256 is missing {path}"))
        })?;
        if !expected.eq_ignore_ascii_case(digest) {
            return Err(CatalogImportError::invalid(format!(
                "checksums.sha256 disagrees for {path}"
            )));
        }
    }
    Ok(())
}

fn stage_for(kind: CatalogPayloadKind) -> CatalogImportStage {
    match kind {
        CatalogPayloadKind::Dat => CatalogImportStage::BuildingCatalog,
        CatalogPayloadKind::Enrichment => CatalogImportStage::ApplyingEnrichment,
        CatalogPayloadKind::Relations => CatalogImportStage::ApplyingRelations,
    }
}

struct DigestReader<R> {
    inner: R,
    digest: Sha256,
    bytes: Rc<Cell<u64>>,
}

impl<R> DigestReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            digest: Sha256::new(),
            bytes: Rc::new(Cell::new(0)),
        }
    }

    fn byte_counter(&self) -> Rc<Cell<u64>> {
        Rc::clone(&self.bytes)
    }

    fn finish(self) -> (String, u64) {
        (hex::encode(self.digest.finalize()), self.bytes.get())
    }
}

impl<R: Read> Read for DigestReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.digest.update(&buffer[..read]);
        self.bytes.set(self.bytes.get().saturating_add(read as u64));
        Ok(read)
    }
}
