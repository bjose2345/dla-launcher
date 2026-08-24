use std::{
    collections::HashSet,
    fs::File,
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use dla_application::{
    catalog::{CatalogReader, CatalogSnapshot},
    catalog_import::{
        ActivateCatalogGenerationRequest, CATALOG_PACKAGE_FORMAT, CATALOG_PACKAGE_FORMAT_VERSION,
        CatalogGenerationKind, CatalogGenerationState, CatalogGenerationSummary,
        CatalogImportCancellationToken, CatalogImportError, CatalogImportOperationKind,
        CatalogImportProgress, CatalogImportProgressSink, CatalogImportStage, CatalogImporter,
        CatalogPackageCounts, CatalogPackageManifest, CatalogPackageProfile, CatalogPackageSource,
        CatalogPayloadDescriptor, CatalogPayloadKind, ExecuteCatalogImportRequest,
    },
    search::{
        CatalogIndexSource, CatalogSearchIndex, CatalogSearchService, SearchError, SearchIndexPage,
        SearchIndexState, SearchIndexStatus, SearchQuery,
    },
};
use dla_catalog_import::{
    COMPACT_FIELDS, CONTENT_FIELDS, CatalogImportAdapter, CatalogPackageAccessRegistry,
    ENRICHMENT_FIELDS, all_fields, import_package_payloads, inspect_package,
};
use dla_sqlite::{
    ReloadableCatalogStore, SqliteCatalogImportWriter, SqliteCatalogStore, SqliteLibraryStore,
    StoredCatalogGeneration, database_size,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tar::{Builder, Header};
use tempfile::{TempDir, tempdir};

const COMPACT_REFERENCE: &[u8] =
    include_bytes!("../../../../shared/fixtures/catalog-package/v1/golden/compact/reference.dat");
const COMPACT_PARITY: &[u8] =
    include_bytes!("../../../../shared/fixtures/catalog-package/v1/golden/compact/parity.dat");
const FULL_REFERENCE: &[u8] =
    include_bytes!("../../../../shared/fixtures/catalog-package/v1/golden/full/reference.dat");
const FULL_PARITY: &[u8] =
    include_bytes!("../../../../shared/fixtures/catalog-package/v1/golden/full/parity.dat");

fn cancelled_token() -> CatalogImportCancellationToken {
    let token = CatalogImportCancellationToken::default();
    token.cancel();
    token
}

#[derive(Default)]
struct ProgressRecorder {
    events: Mutex<Vec<CatalogImportProgress>>,
}

impl CatalogImportProgressSink for ProgressRecorder {
    fn publish(&self, progress: &CatalogImportProgress) -> Result<(), CatalogImportError> {
        self.events
            .lock()
            .expect("progress lock")
            .push(progress.clone());
        Ok(())
    }
}

struct CancellingProgress {
    cancellation: CatalogImportCancellationToken,
    stage: CatalogImportStage,
}

impl CatalogImportProgressSink for CancellingProgress {
    fn publish(&self, progress: &CatalogImportProgress) -> Result<(), CatalogImportError> {
        if progress.stage == self.stage {
            self.cancellation.cancel();
        }
        Ok(())
    }
}

struct RecordingIndex {
    status: Mutex<SearchIndexStatus>,
    fail_next: AtomicBool,
}

impl RecordingIndex {
    fn new(path: &Path) -> Self {
        Self {
            status: Mutex::new(SearchIndexStatus::missing(path.display().to_string())),
            fail_next: AtomicBool::new(false),
        }
    }

    fn fail_next_rebuild(&self) {
        self.fail_next.store(true, Ordering::SeqCst);
    }
}

impl CatalogSearchIndex for RecordingIndex {
    fn status(&self) -> SearchIndexStatus {
        self.status.lock().expect("status lock").clone()
    }

    fn rebuild(&self, source: &dyn CatalogIndexSource) -> Result<SearchIndexStatus, SearchError> {
        if self.fail_next.swap(false, Ordering::SeqCst) {
            return Err(SearchError::index("synthetic rebuild failure"));
        }
        let snapshot = source.snapshot()?;
        let mut indexed_documents = 0_usize;
        let mut after = None;
        loop {
            let batch = source.read_search_batch(after.as_deref(), 32)?;
            if batch.is_empty() {
                break;
            }
            indexed_documents += batch.len();
            after = batch.last().map(|work| work.work_code.clone());
        }
        let status = SearchIndexStatus {
            state: SearchIndexState::Ready,
            schema_version: 1,
            catalog_snapshot_id: snapshot.id,
            indexed_documents,
            generation: "test-generation".to_owned(),
            index_path: "memory".to_owned(),
            detail: "ready".to_owned(),
        };
        *self.status.lock().expect("status lock") = status.clone();
        Ok(status)
    }

    fn search(&self, _query: &SearchQuery) -> Result<SearchIndexPage, SearchError> {
        Ok(SearchIndexPage {
            matches: Vec::new(),
            total: 0,
            limit: 0,
            offset: 0,
        })
    }
}

struct ImportHarness {
    _directory: TempDir,
    adapter: CatalogImportAdapter,
    catalog: Arc<ReloadableCatalogStore>,
    library: Arc<SqliteLibraryStore>,
    index: Arc<RecordingIndex>,
    access_handle: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureCatalog {
    fixture_version: u32,
    profiles: Vec<FixtureProfile>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureProfile {
    profile: CatalogPackageProfile,
    snapshot_id: String,
    field_set: String,
    enrichment_fields: Value,
    relations: bool,
    expected: CatalogPackageCounts,
}

#[derive(Clone)]
struct Payload {
    path: String,
    kind: CatalogPayloadKind,
    media_type: String,
    records: u64,
    bytes: Vec<u8>,
}

#[test]
fn golden_dat_producers_remain_byte_equivalent() {
    assert_eq!(COMPACT_REFERENCE, COMPACT_PARITY);
    assert_eq!(FULL_REFERENCE, FULL_PARITY);
}

#[test]
fn legacy_manifest_profiles_are_read_but_only_full_is_emitted() {
    assert_eq!(
        serde_json::from_str::<CatalogPackageProfile>("\"complete\"")
            .expect("legacy complete profile"),
        CatalogPackageProfile::LegacyComplete
    );
    assert_eq!(
        serde_json::from_str::<CatalogPackageProfile>("\"enriched\"")
            .expect("legacy enriched profile"),
        CatalogPackageProfile::LegacyEnriched
    );
    assert_eq!(
        serde_json::to_string(&CatalogPackageProfile::LegacyComplete)
            .expect("serialize legacy complete profile"),
        "\"full\""
    );
    assert_eq!(
        serde_json::to_string(&CatalogPackageProfile::LegacyEnriched)
            .expect("serialize legacy enriched profile"),
        "\"full\""
    );
}

#[test]
fn every_profile_fixture_inspects_and_imports_deterministically() {
    let fixture: FixtureCatalog = serde_json::from_str(include_str!(
        "../../../../shared/fixtures/catalog-package/v1/profiles.json"
    ))
    .expect("fixture catalog");
    assert_eq!(fixture.fixture_version, 1);
    assert_eq!(fixture.profiles.len(), 3);

    for definition in fixture.profiles {
        let directory = tempdir().expect("temporary directory");
        let package_path = directory
            .path()
            .join(format!("{}.dla", definition.snapshot_id));
        let manifest = build_fixture_package(&package_path, &definition);
        let inspected = inspect_package(&package_path, directory.path()).expect("inspect package");
        assert!(inspected.blocking_issues.is_empty());
        assert_eq!(inspected.manifest, manifest);

        let database_path = directory.path().join("catalog.sqlite");
        let mut writer = SqliteCatalogImportWriter::create(&database_path).expect("catalog writer");
        let stats = import_package_payloads(
            &package_path,
            &manifest,
            &mut writer,
            &CatalogImportCancellationToken::default(),
            |_, _, _| Ok(()),
        )
        .expect("import package");
        assert_eq!(
            stats.counters.work_entries,
            definition.expected.work_entries
        );
        assert_eq!(
            stats.counters.unique_works,
            definition.expected.unique_works
        );
        assert_eq!(stats.counters.roms, definition.expected.roms);
        assert_eq!(stats.counters.files, definition.expected.files);
        assert_eq!(stats.counters.relations, definition.expected.relations);
        let counts = writer
            .finish(
                &manifest,
                "2026-08-05T12:00:00Z",
                &CatalogImportCancellationToken::default(),
                |_| Ok(()),
            )
            .expect("finish catalog");
        assert_eq!(counts.unique_works, definition.expected.unique_works);
        assert_eq!(counts.roms, definition.expected.roms);
        assert_eq!(counts.files, definition.expected.files);
        assert_eq!(counts.relations, definition.expected.relations);

        let store = SqliteCatalogStore::open_existing(&database_path).expect("open catalog");
        let first = store.read("RJ000001").expect("read work").expect("work");
        assert_eq!(first.work.title, "標準 & Compact");
        assert_eq!(first.roms.len(), 1);
        if manifest.fields.iter().any(|field| field == "rom.fileCount") {
            assert_eq!(first.roms[0].file_count, Some(1));
        }
        if manifest
            .fields
            .iter()
            .any(|field| field == "rom.updateDate")
        {
            assert_eq!(first.roms[0].update_date, "2026-08-05");
        }
        let descriptions_included = manifest
            .fields
            .iter()
            .any(|field| field == "work.descriptions");
        assert_eq!(first.descriptions.included, descriptions_included);
        if descriptions_included {
            assert_eq!(first.descriptions.versions.len(), 1);
            assert_eq!(first.descriptions.versions[0].version, 1);
            assert_eq!(first.descriptions.versions[0].html, "<p>Fixture</p>");
        } else {
            assert!(first.descriptions.versions.is_empty());
        }
    }
}

#[test]
fn payload_progress_advances_bytes_before_the_payload_finishes() {
    let directory = tempdir().expect("temporary directory");
    let work_count = 512_u64;
    let mut dat = String::from("<?xml version=\"1.0\"?><datafile><header></header>");
    for index in 0..work_count {
        dat.push_str(&format!(
            "<work name=\"RJ{index:06}\"><title>Work {index}</title><rom name=\"RJ{index:06}.zip\" size=\"1\" /></work>"
        ));
    }
    dat.push_str("</datafile>");

    let definition = FixtureProfile {
        profile: CatalogPackageProfile::Compact,
        snapshot_id: "streaming-progress".to_owned(),
        field_set: "compact".to_owned(),
        enrichment_fields: json!([]),
        relations: false,
        expected: CatalogPackageCounts {
            work_entries: work_count,
            unique_works: work_count,
            roms: work_count,
            files: 0,
            relations: 0,
        },
    };
    let payloads = vec![Payload {
        path: "catalog.dat".to_owned(),
        kind: CatalogPayloadKind::Dat,
        media_type: "application/xml".to_owned(),
        records: work_count,
        bytes: dat.into_bytes(),
    }];
    let manifest = manifest_for(&definition, profile_fields(&definition), &payloads);
    let package_path = directory.path().join("progress.dla");
    write_package(&package_path, &manifest, &payloads);
    let database_path = directory.path().join("catalog.sqlite");
    let mut writer = SqliteCatalogImportWriter::create(&database_path).expect("catalog writer");
    let mut samples = Vec::new();

    let stats = import_package_payloads(
        &package_path,
        &manifest,
        &mut writer,
        &CatalogImportCancellationToken::default(),
        |stage, _, counters| {
            if stage == CatalogImportStage::BuildingCatalog {
                samples.push(counters.clone());
            }
            Ok(())
        },
    )
    .expect("import package");

    assert!(samples.windows(2).all(|pair| {
        pair[0].processed_bytes <= pair[1].processed_bytes
            && pair[0].unique_works <= pair[1].unique_works
    }));
    assert!(samples.iter().any(|sample| {
        sample.processed_bytes > 0
            && sample.processed_bytes < sample.total_bytes
            && sample.unique_works > 0
    }));
    assert_eq!(stats.counters.processed_bytes, stats.counters.total_bytes);
    assert_eq!(stats.counters.unique_works, work_count);
}

#[test]
fn headless_import_activates_a_generation_and_can_restore_the_embedded_catalog() {
    let harness = import_harness();
    let progress = ProgressRecorder::default();
    let outcome = harness
        .adapter
        .execute(
            ExecuteCatalogImportRequest {
                operation_id: "import-success".to_owned(),
                access_handle: harness.access_handle.clone(),
            },
            &CatalogImportCancellationToken::default(),
            &progress,
        )
        .expect("execute import");
    assert_eq!(outcome.generation.state, CatalogGenerationState::Active);
    assert_eq!(outcome.generation.package_name, "target.dla");
    assert_eq!(outcome.generation.snapshot_id, "fixture-full-activation");
    assert_eq!(outcome.search_documents, 2);
    assert_eq!(
        catalog_snapshot(&harness.catalog).id,
        "fixture-full-activation"
    );
    assert_eq!(
        harness
            .library
            .read_active_catalog_generation()
            .expect("active generation")
            .summary
            .id,
        outcome.generation.id
    );
    assert_eq!(
        progress
            .events
            .lock()
            .expect("progress lock")
            .last()
            .expect("terminal progress")
            .stage,
        CatalogImportStage::Completed
    );
    let stages = progress
        .events
        .lock()
        .expect("progress lock")
        .iter()
        .map(|event| event.stage)
        .collect::<Vec<_>>();
    assert!(stages.contains(&CatalogImportStage::FinalizingCatalog));
    assert!(stages.contains(&CatalogImportStage::CheckpointingCatalog));
    assert!(stages.contains(&CatalogImportStage::ValidatingCatalog));
    assert!(
        progress
            .events
            .lock()
            .expect("progress lock")
            .iter()
            .all(|event| event.operation_kind == CatalogImportOperationKind::Import)
    );

    let activation_progress = ProgressRecorder::default();
    let restored = harness
        .adapter
        .activate(
            ActivateCatalogGenerationRequest {
                operation_id: "activate-embedded".to_owned(),
                generation_id: "embedded".to_owned(),
            },
            &CatalogImportCancellationToken::default(),
            &activation_progress,
        )
        .expect("activate embedded");
    assert_eq!(restored.generation.state, CatalogGenerationState::Active);
    assert_eq!(catalog_snapshot(&harness.catalog).id, "fixture-embedded");
    assert!(
        activation_progress
            .events
            .lock()
            .expect("activation progress lock")
            .iter()
            .all(|event| event.operation_kind == CatalogImportOperationKind::Activation)
    );
}

#[test]
fn removal_protects_active_and_embedded_catalogs_then_deletes_an_inactive_import() {
    let harness = import_harness();
    assert!(matches!(
        CatalogImporter::remove_generation(&harness.adapter, "embedded"),
        Err(CatalogImportError::CannotRemoveEmbeddedGeneration)
    ));

    let imported = harness
        .adapter
        .execute(
            ExecuteCatalogImportRequest {
                operation_id: "import-for-removal".to_owned(),
                access_handle: harness.access_handle.clone(),
            },
            &CatalogImportCancellationToken::default(),
            &ProgressRecorder::default(),
        )
        .expect("import generation for removal");
    let generation_id = imported.generation.id;
    assert!(matches!(
        CatalogImporter::remove_generation(&harness.adapter, &generation_id),
        Err(CatalogImportError::CannotRemoveActiveGeneration)
    ));

    harness
        .adapter
        .activate(
            ActivateCatalogGenerationRequest {
                operation_id: "activate-embedded-before-removal".to_owned(),
                generation_id: "embedded".to_owned(),
            },
            &CatalogImportCancellationToken::default(),
            &ProgressRecorder::default(),
        )
        .expect("reactivate embedded generation");
    let generation_directory = harness
        ._directory
        .path()
        .join("catalog/generations")
        .join(&generation_id);
    assert!(generation_directory.join("catalog.sqlite").is_file());
    assert!(generation_directory.join("manifest.json").is_file());

    CatalogImporter::remove_generation(&harness.adapter, &generation_id)
        .expect("remove inactive imported generation");

    assert!(!generation_directory.exists());
    assert!(harness._directory.path().join("target.dla").is_file());
    assert!(matches!(
        harness.library.read_catalog_generation(&generation_id),
        Err(CatalogImportError::GenerationNotFound(_))
    ));
    assert!(harness.library.read_catalog_generation("embedded").is_ok());
}

#[test]
fn startup_recovers_or_finishes_interrupted_generation_removal() {
    let harness = import_harness();
    let imported = harness
        .adapter
        .execute(
            ExecuteCatalogImportRequest {
                operation_id: "import-for-removal-recovery".to_owned(),
                access_handle: harness.access_handle.clone(),
            },
            &CatalogImportCancellationToken::default(),
            &ProgressRecorder::default(),
        )
        .expect("import generation for removal recovery");
    let generation_id = imported.generation.id;
    harness
        .adapter
        .activate(
            ActivateCatalogGenerationRequest {
                operation_id: "activate-embedded-before-recovery".to_owned(),
                generation_id: "embedded".to_owned(),
            },
            &CatalogImportCancellationToken::default(),
            &ProgressRecorder::default(),
        )
        .expect("reactivate embedded generation");

    let generations = harness._directory.path().join("catalog/generations");
    let generation_directory = generations.join(&generation_id);
    let deleting_directory = generations.join(format!(".deleting-{generation_id}"));
    std::fs::rename(&generation_directory, &deleting_directory)
        .expect("simulate interruption before history deletion");
    reopen_import_adapter(&harness);
    assert!(generation_directory.is_dir());
    assert!(!deleting_directory.exists());

    std::fs::rename(&generation_directory, &deleting_directory)
        .expect("simulate interruption after history deletion");
    assert!(
        harness
            .library
            .delete_catalog_generation(&generation_id)
            .expect("delete generation history")
    );
    reopen_import_adapter(&harness);
    assert!(!generation_directory.exists());
    assert!(!deleting_directory.exists());
}

#[test]
fn search_failure_restores_the_previous_catalog_and_marks_the_candidate_failed() {
    let harness = import_harness();
    harness.index.fail_next_rebuild();
    let error = harness
        .adapter
        .execute(
            ExecuteCatalogImportRequest {
                operation_id: "import-failure".to_owned(),
                access_handle: harness.access_handle.clone(),
            },
            &CatalogImportCancellationToken::default(),
            &ProgressRecorder::default(),
        )
        .expect_err("search rebuild failure");
    assert!(matches!(error, CatalogImportError::Search(_)));
    assert_eq!(catalog_snapshot(&harness.catalog).id, "fixture-embedded");
    assert_eq!(
        harness
            .library
            .read_active_catalog_generation()
            .expect("active generation")
            .summary
            .id,
        "embedded"
    );
    assert_eq!(
        harness.index.status().catalog_snapshot_id,
        "fixture-embedded"
    );
    let generations = harness.adapter.list_generations().expect("generations");
    assert_eq!(generations.len(), 2);
    assert!(generations.iter().any(|generation| {
        generation.kind == CatalogGenerationKind::Imported
            && generation.state == CatalogGenerationState::Failed
    }));
}

#[test]
fn cancellation_before_building_leaves_no_candidate_generation() {
    let harness = import_harness();
    let error = harness
        .adapter
        .execute(
            ExecuteCatalogImportRequest {
                operation_id: "import-cancelled".to_owned(),
                access_handle: harness.access_handle.clone(),
            },
            &cancelled_token(),
            &ProgressRecorder::default(),
        )
        .expect_err("cancelled import");
    assert!(matches!(error, CatalogImportError::Cancelled));
    assert_eq!(catalog_snapshot(&harness.catalog).id, "fixture-embedded");
    assert_eq!(
        harness
            .adapter
            .list_generations()
            .expect("generations")
            .len(),
        1
    );
}

#[test]
fn cancellation_during_sqlite_validation_leaves_no_candidate_generation() {
    let harness = import_harness();
    let cancellation = CatalogImportCancellationToken::default();
    let progress = CancellingProgress {
        cancellation: cancellation.clone(),
        stage: CatalogImportStage::ValidatingCatalog,
    };
    let error = harness
        .adapter
        .execute(
            ExecuteCatalogImportRequest {
                operation_id: "validation-cancelled".to_owned(),
                access_handle: harness.access_handle.clone(),
            },
            &cancellation,
            &progress,
        )
        .expect_err("cancelled validation");
    assert!(matches!(error, CatalogImportError::Cancelled));
    assert_eq!(catalog_snapshot(&harness.catalog).id, "fixture-embedded");
    assert_eq!(
        harness
            .adapter
            .list_generations()
            .expect("generations")
            .len(),
        1
    );
}

#[test]
fn declared_enrichment_fields_require_explicit_values_or_nulls() {
    let directory = tempdir().expect("temporary directory");
    let definition = FixtureProfile {
        profile: CatalogPackageProfile::Custom,
        snapshot_id: "invalid-missing-field".to_owned(),
        field_set: "compact".to_owned(),
        enrichment_fields: json!(["work.titleEnglish", "work.images.main"]),
        relations: false,
        expected: CatalogPackageCounts {
            work_entries: 2,
            unique_works: 2,
            roms: 2,
            files: 0,
            relations: 0,
        },
    };
    let package_path = directory.path().join("invalid.dla");
    let mut fields = profile_fields(&definition);
    fields.retain(|field| field != "work.images.main");
    let invalid_record = format!(
        "{}\n{}\n",
        json!({"workCode":"RJ000001","fields":{"work.titleEnglish":"English"}}),
        json!({"workCode":"RJ000002","fields":{"work.titleEnglish":"English"}})
    )
    .into_bytes();
    let payloads = vec![
        Payload {
            path: "catalog.dat".to_owned(),
            kind: CatalogPayloadKind::Dat,
            media_type: "application/xml".to_owned(),
            records: 2,
            bytes: COMPACT_PARITY.to_vec(),
        },
        Payload {
            path: "enrichment/000001.ndjson".to_owned(),
            kind: CatalogPayloadKind::Enrichment,
            media_type: "application/x-ndjson".to_owned(),
            records: 2,
            bytes: invalid_record,
        },
    ];
    fields.push("work.images.main".to_owned());
    fields = canonical_fields(fields);
    let manifest = manifest_for(&definition, fields, &payloads);
    write_package(&package_path, &manifest, &payloads);
    let database_path = directory.path().join("catalog.sqlite");
    let mut writer = SqliteCatalogImportWriter::create(&database_path).expect("catalog writer");
    let error = import_package_payloads(
        &package_path,
        &manifest,
        &mut writer,
        &CatalogImportCancellationToken::default(),
        |_, _, _| Ok(()),
    )
    .expect_err("field mismatch");
    assert!(error.to_string().contains("field-set mismatch"));
}

#[test]
fn selected_relations_accept_an_empty_declared_chunk() {
    let directory = tempdir().expect("temporary directory");
    let definition = FixtureProfile {
        profile: CatalogPackageProfile::Custom,
        snapshot_id: "empty-relations-fixture".to_owned(),
        field_set: "compact".to_owned(),
        enrichment_fields: json!([]),
        relations: true,
        expected: CatalogPackageCounts {
            work_entries: 2,
            unique_works: 2,
            roms: 2,
            files: 0,
            relations: 0,
        },
    };
    let package_path = directory.path().join("empty-relations.dla");
    let payloads = vec![
        Payload {
            path: "catalog.dat".to_owned(),
            kind: CatalogPayloadKind::Dat,
            media_type: "application/xml".to_owned(),
            records: 2,
            bytes: COMPACT_PARITY.to_vec(),
        },
        Payload {
            path: "relations/000001.ndjson".to_owned(),
            kind: CatalogPayloadKind::Relations,
            media_type: "application/x-ndjson".to_owned(),
            records: 0,
            bytes: Vec::new(),
        },
    ];
    let manifest = manifest_for(&definition, profile_fields(&definition), &payloads);
    write_package(&package_path, &manifest, &payloads);

    let inspection = inspect_package(&package_path, directory.path()).expect("inspect package");
    assert!(inspection.blocking_issues.is_empty());
    let database_path = directory.path().join("catalog.sqlite");
    let mut writer = SqliteCatalogImportWriter::create(&database_path).expect("catalog writer");
    let stats = import_package_payloads(
        &package_path,
        &manifest,
        &mut writer,
        &CatalogImportCancellationToken::default(),
        |_, _, _| Ok(()),
    )
    .expect("import package");
    assert_eq!(stats.counters.relations, 0);
}

#[test]
fn self_relations_are_rejected_as_invalid_package_data() {
    let directory = tempdir().expect("temporary directory");
    let definition = FixtureProfile {
        profile: CatalogPackageProfile::Custom,
        snapshot_id: "invalid-self-relation".to_owned(),
        field_set: "compact".to_owned(),
        enrichment_fields: json!([]),
        relations: true,
        expected: CatalogPackageCounts {
            work_entries: 2,
            unique_works: 2,
            roms: 2,
            files: 0,
            relations: 1,
        },
    };
    let package_path = directory.path().join("self-relation.dla");
    let relation = format!(
        "{}\n",
        json!({
            "parentWorkCode": "RJ000001",
            "childWorkCode": "rj000001",
            "relationTypeCode": "translation",
            "relationTypeLabel": "Language-only Translation"
        })
    )
    .into_bytes();
    let payloads = vec![
        Payload {
            path: "catalog.dat".to_owned(),
            kind: CatalogPayloadKind::Dat,
            media_type: "application/xml".to_owned(),
            records: 2,
            bytes: COMPACT_PARITY.to_vec(),
        },
        Payload {
            path: "relations/000001.ndjson".to_owned(),
            kind: CatalogPayloadKind::Relations,
            media_type: "application/x-ndjson".to_owned(),
            records: 1,
            bytes: relation,
        },
    ];
    let manifest = manifest_for(&definition, profile_fields(&definition), &payloads);
    write_package(&package_path, &manifest, &payloads);
    let database_path = directory.path().join("catalog.sqlite");
    let mut writer = SqliteCatalogImportWriter::create(&database_path).expect("catalog writer");

    let error = import_package_payloads(
        &package_path,
        &manifest,
        &mut writer,
        &CatalogImportCancellationToken::default(),
        |_, _, _| Ok(()),
    )
    .expect_err("self relation");

    assert!(matches!(error, CatalogImportError::InvalidPackage(_)));
    assert!(error.to_string().contains("RJ000001 to itself"));
}

#[test]
fn import_rejects_a_manifest_that_changed_after_inspection() {
    let directory = tempdir().expect("temporary directory");
    let definition = FixtureProfile {
        profile: CatalogPackageProfile::Compact,
        snapshot_id: "manifest-race-fixture".to_owned(),
        field_set: "compact".to_owned(),
        enrichment_fields: json!([]),
        relations: false,
        expected: CatalogPackageCounts {
            work_entries: 2,
            unique_works: 2,
            roms: 2,
            files: 0,
            relations: 0,
        },
    };
    let package_path = directory.path().join("manifest-race.dla");
    let mut inspected_manifest = build_fixture_package(&package_path, &definition);
    inspected_manifest.snapshot_id = "different-snapshot".to_owned();
    let database_path = directory.path().join("catalog.sqlite");
    let mut writer = SqliteCatalogImportWriter::create(&database_path).expect("catalog writer");
    let error = import_package_payloads(
        &package_path,
        &inspected_manifest,
        &mut writer,
        &CatalogImportCancellationToken::default(),
        |_, _, _| Ok(()),
    )
    .expect_err("changed manifest");
    assert!(
        error
            .to_string()
            .contains("changed after package inspection")
    );
}

fn import_harness() -> ImportHarness {
    let directory = tempdir().expect("temporary directory");
    let embedded_definition = FixtureProfile {
        profile: CatalogPackageProfile::Compact,
        snapshot_id: "fixture-embedded".to_owned(),
        field_set: "compact".to_owned(),
        enrichment_fields: json!([]),
        relations: false,
        expected: CatalogPackageCounts {
            work_entries: 2,
            unique_works: 2,
            roms: 2,
            files: 0,
            relations: 0,
        },
    };
    let embedded_package = directory.path().join("embedded.dla");
    let embedded_manifest = build_fixture_package(&embedded_package, &embedded_definition);
    let embedded_database = directory.path().join("catalog.sqlite");
    import_database(&embedded_package, &embedded_manifest, &embedded_database);
    let embedded_store =
        Arc::new(SqliteCatalogStore::open_existing(&embedded_database).expect("embedded catalog"));
    let catalog = Arc::new(ReloadableCatalogStore::new(embedded_store));
    let library = Arc::new(
        SqliteLibraryStore::open(&directory.path().join("library.sqlite")).expect("library"),
    );
    library
        .initialize_embedded_catalog(&StoredCatalogGeneration {
            summary: CatalogGenerationSummary {
                id: "embedded".to_owned(),
                snapshot_id: embedded_manifest.snapshot_id.clone(),
                kind: CatalogGenerationKind::Embedded,
                state: CatalogGenerationState::Active,
                profile: CatalogPackageProfile::Compact,
                source_name: "Embedded fixture".to_owned(),
                package_name: String::new(),
                imported_at: "2026-08-05T12:00:00Z".to_owned(),
                work_count: embedded_manifest.counts.unique_works,
                rom_count: embedded_manifest.counts.roms,
                database_bytes: database_size(&embedded_database),
                fields: embedded_manifest.fields.clone(),
                failure_detail: String::new(),
            },
            catalog_path: "catalog.sqlite".to_owned(),
        })
        .expect("register embedded generation");

    let index = Arc::new(RecordingIndex::new(&directory.path().join("search")));
    let search = Arc::new(CatalogSearchService::new(
        catalog.clone(),
        catalog.clone(),
        catalog.clone(),
        index.clone(),
    ));
    search.rebuild().expect("initial search index");
    let access = Arc::new(CatalogPackageAccessRegistry::new());
    let target_definition = FixtureProfile {
        profile: CatalogPackageProfile::Full,
        snapshot_id: "fixture-full-activation".to_owned(),
        field_set: "full".to_owned(),
        enrichment_fields: json!("all"),
        relations: true,
        expected: CatalogPackageCounts {
            work_entries: 2,
            unique_works: 2,
            roms: 2,
            files: 2,
            relations: 1,
        },
    };
    let target_package = directory.path().join("target.dla");
    build_fixture_package(&target_package, &target_definition);
    let access_handle = access
        .approve(&target_package)
        .expect("approve target package")
        .access_handle;
    let adapter = CatalogImportAdapter::new(
        directory.path().to_path_buf(),
        access,
        catalog.clone(),
        library.clone(),
        search,
    )
    .expect("import adapter");
    ImportHarness {
        _directory: directory,
        adapter,
        catalog,
        library,
        index,
        access_handle,
    }
}

fn import_database(package: &Path, manifest: &CatalogPackageManifest, database: &Path) {
    let mut writer = SqliteCatalogImportWriter::create(database).expect("catalog writer");
    import_package_payloads(
        package,
        manifest,
        &mut writer,
        &CatalogImportCancellationToken::default(),
        |_, _, _| Ok(()),
    )
    .expect("import database payloads");
    writer
        .finish(
            manifest,
            "2026-08-05T12:00:00Z",
            &CatalogImportCancellationToken::default(),
            |_| Ok(()),
        )
        .expect("finish database");
}

fn reopen_import_adapter(harness: &ImportHarness) -> CatalogImportAdapter {
    let search = Arc::new(CatalogSearchService::new(
        harness.catalog.clone(),
        harness.catalog.clone(),
        harness.catalog.clone(),
        harness.index.clone(),
    ));
    CatalogImportAdapter::new(
        harness._directory.path().to_path_buf(),
        Arc::new(CatalogPackageAccessRegistry::new()),
        harness.catalog.clone(),
        harness.library.clone(),
        search,
    )
    .expect("reopen catalog import adapter")
}

fn catalog_snapshot(catalog: &ReloadableCatalogStore) -> CatalogSnapshot {
    CatalogIndexSource::snapshot(catalog).expect("catalog snapshot")
}

fn build_fixture_package(path: &Path, definition: &FixtureProfile) -> CatalogPackageManifest {
    let fields = profile_fields(definition);
    let dat = if fields
        .iter()
        .any(|field| CONTENT_FIELDS.contains(&field.as_str()))
    {
        FULL_PARITY
    } else {
        COMPACT_PARITY
    };
    let mut payloads = vec![Payload {
        path: "catalog.dat".to_owned(),
        kind: CatalogPayloadKind::Dat,
        media_type: "application/xml".to_owned(),
        records: definition.expected.work_entries,
        bytes: dat.to_vec(),
    }];
    let enrichment_fields = fields
        .iter()
        .filter(|field| {
            ENRICHMENT_FIELDS.contains(&field.as_str()) && field.as_str() != "work.relations"
        })
        .cloned()
        .collect::<Vec<_>>();
    if !enrichment_fields.is_empty() {
        for (index, work_code) in ["RJ000001", "RJ000002"].into_iter().enumerate() {
            let fields = enrichment_fields
                .iter()
                .map(|field| (field.clone(), enrichment_value(field, work_code)))
                .collect::<Map<_, _>>();
            let mut bytes = serde_json::to_vec(&json!({
                "workCode": work_code,
                "fields": fields
            }))
            .expect("enrichment record");
            bytes.push(b'\n');
            payloads.push(Payload {
                path: format!("enrichment/{:06}.ndjson", index + 1),
                kind: CatalogPayloadKind::Enrichment,
                media_type: "application/x-ndjson".to_owned(),
                records: 1,
                bytes,
            });
        }
    }
    if definition.relations {
        let mut bytes = serde_json::to_vec(&json!({
            "parentWorkCode": "RJ000001",
            "childWorkCode": "RJ000002",
            "relationTypeCode": "series",
            "relationTypeLabel": "Series"
        }))
        .expect("relation record");
        bytes.push(b'\n');
        payloads.push(Payload {
            path: "relations/000001.ndjson".to_owned(),
            kind: CatalogPayloadKind::Relations,
            media_type: "application/x-ndjson".to_owned(),
            records: 1,
            bytes,
        });
    }
    let manifest = manifest_for(definition, fields, &payloads);
    write_package(path, &manifest, &payloads);
    manifest
}

fn profile_fields(definition: &FixtureProfile) -> Vec<String> {
    let mut selected = match definition.field_set.as_str() {
        "compact" => COMPACT_FIELDS.iter().copied().collect::<HashSet<_>>(),
        "full" => all_fields().into_iter().collect(),
        value => panic!("unknown field set {value}"),
    };
    match &definition.enrichment_fields {
        Value::String(value) if value == "all" => {
            selected.extend(ENRICHMENT_FIELDS.iter().copied());
        }
        Value::Array(fields) => {
            selected.extend(
                fields
                    .iter()
                    .map(|field| field.as_str().expect("field identifier")),
            );
        }
        _ => panic!("invalid enrichment field fixture"),
    }
    if definition.relations {
        selected.insert("work.relations");
    }
    all_fields()
        .into_iter()
        .filter(|field| selected.contains(field))
        .map(str::to_owned)
        .collect()
}

fn canonical_fields(fields: Vec<String>) -> Vec<String> {
    let selected = fields.iter().map(String::as_str).collect::<HashSet<_>>();
    all_fields()
        .into_iter()
        .filter(|field| selected.contains(field))
        .map(str::to_owned)
        .collect()
}

fn enrichment_value(field: &str, work_code: &str) -> Value {
    match field {
        "work.titleEnglish" => json!(format!("English {work_code}")),
        "work.addedDate" => json!("2026-08-05T00:00:00Z"),
        "work.updatedDate" => json!("2026-08-05T00:00:00Z"),
        "work.images.main" => json!([format!("https://example.invalid/{work_code}_main.webp")]),
        "work.images.thumbnail" => {
            json!([format!("https://example.invalid/{work_code}_sam.webp")])
        }
        "work.images.samples" => {
            json!([format!("https://example.invalid/{work_code}_img_smp1.webp")])
        }
        "work.descriptions" => json!([{"version": 1, "html": "<p>Fixture</p>"}]),
        "work.rating.score" => json!(4.5),
        "work.rating.count" => json!(10),
        "work.rating.totalSales" => json!(100),
        "work.rating.favorites" => json!(12),
        "work.rating.rankings" => json!([{"range": "24h", "rank": 2}]),
        "rom.fileCount" => json!([{"position": 0, "value": 1}]),
        "rom.updateDate" => json!([{"position": 0, "value": "2026-08-05"}]),
        _ => Value::Null,
    }
}

fn manifest_for(
    definition: &FixtureProfile,
    fields: Vec<String>,
    payloads: &[Payload],
) -> CatalogPackageManifest {
    CatalogPackageManifest {
        format: CATALOG_PACKAGE_FORMAT.to_owned(),
        format_version: CATALOG_PACKAGE_FORMAT_VERSION,
        catalog_schema_version: 1,
        minimum_launcher_version: "0.1.0".to_owned(),
        snapshot_id: definition.snapshot_id.clone(),
        created_at: "2026-08-05T12:00:00Z".to_owned(),
        profile: definition.profile,
        source: CatalogPackageSource {
            id: "DL".to_owned(),
            name: "DLsite".to_owned(),
        },
        fields,
        counts: definition.expected.clone(),
        payloads: payloads
            .iter()
            .map(|payload| CatalogPayloadDescriptor {
                path: payload.path.clone(),
                kind: payload.kind,
                media_type: payload.media_type.clone(),
                records: payload.records,
                uncompressed_bytes: payload.bytes.len() as u64,
                sha256: digest(&payload.bytes),
            })
            .collect(),
    }
}

fn write_package(path: &Path, manifest: &CatalogPackageManifest, payloads: &[Payload]) {
    let output = File::create(path).expect("create package");
    let encoder = zstd::stream::write::Encoder::new(output, 3).expect("zstd encoder");
    let mut archive = Builder::new(encoder);
    let manifest_bytes = serde_json::to_vec_pretty(manifest).expect("manifest");
    append_entry(&mut archive, "manifest.json", &manifest_bytes);
    for payload in payloads {
        append_entry(&mut archive, &payload.path, &payload.bytes);
    }
    let checksums = payloads
        .iter()
        .map(|payload| format!("{}  {}\n", digest(&payload.bytes), payload.path))
        .collect::<String>();
    append_entry(&mut archive, "checksums.sha256", checksums.as_bytes());
    let encoder = archive.into_inner().expect("finish tar");
    encoder.finish().expect("finish zstd");
}

fn append_entry(builder: &mut Builder<zstd::Encoder<'static, File>>, path: &str, bytes: &[u8]) {
    let mut header = Header::new_ustar();
    header.set_path(PathBuf::from(path)).expect("tar path");
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append(&header, Cursor::new(bytes))
        .expect("append package entry");
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
