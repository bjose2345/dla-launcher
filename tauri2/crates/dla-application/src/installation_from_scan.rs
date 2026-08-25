use std::{collections::BTreeSet, sync::Arc};

use dla_detection::{
    MediaClassificationError, MediaClassificationRequest, PackageSourceSetError, classify_media,
    classify_package, discover_package_source_set,
};
use dla_domain::{
    installation::{
        CatalogIdentity, Installation, InstallationId, InstallationOverrides, InstallationPlatform,
        InstallationStatus,
    },
    package::{CatalogPackageContext, SourceArtifact},
    scanner::{
        ScanEntry, ScanEntryId, ScanEntryKind, ScanEntryPresence, ScanEvidenceKind,
        ScanMatchOutcome, ScanResult, ScanResultId, ScanRoot, ScanSession, ScanSessionId,
        ScanStatus,
    },
};
use thiserror::Error;

use crate::{
    catalog::{CatalogError, CatalogReader},
    installation::{InstallationLibrary, InstallationLibraryError, InstallationStore},
    package_inspection::{PackageManifestError, PackageManifestReader},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateInstallationFromScanRequest {
    pub session_id: ScanSessionId,
    pub selected_result_id: ScanResultId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationScanSelection {
    pub root: ScanRoot,
    pub session: ScanSession,
    pub selected_result: ScanResult,
    pub entries: Vec<ScanEntry>,
    pub result_scopes: Vec<InstallationScanResultScope>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationScanResultScope {
    pub candidate_entry_id: ScanEntryId,
    pub matched_work_code: Option<String>,
}

#[derive(Debug, Error)]
pub enum InstallationScanSourceError {
    #[error("installation scan source failed: {0}")]
    Persistence(String),
}

impl InstallationScanSourceError {
    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait InstallationScanSource: Send + Sync {
    fn load(
        &self,
        session_id: &ScanSessionId,
        selected_result_id: &ScanResultId,
    ) -> Result<Option<InstallationScanSelection>, InstallationScanSourceError>;
}

#[derive(Debug, Error)]
pub enum InstallationFromScanError {
    #[error("scan selection was not found")]
    SourceNotFound,
    #[error("scan session is not completed: {0}")]
    ScanNotCompleted(String),
    #[error("invalid scan selection: {0}")]
    InvalidSelection(String),
    #[error("selected installation scope contains multiple matched works: {0}")]
    MultipleMatchedWorks(String),
    #[error(transparent)]
    Source(#[from] InstallationScanSourceError),
    #[error(transparent)]
    Classification(#[from] MediaClassificationError),
    #[error(transparent)]
    PackageSourceSet(#[from] PackageSourceSetError),
    #[error(transparent)]
    PackageManifest(#[from] PackageManifestError),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
}

pub struct InstallationFromScanService {
    source: Arc<dyn InstallationScanSource>,
    library: InstallationLibrary,
    package_inspection: Option<PackageInspectionDependencies>,
}

struct PackageInspectionDependencies {
    manifest_reader: Arc<dyn PackageManifestReader>,
    catalog_reader: Arc<dyn CatalogReader>,
}

struct InstallationScope {
    entries: Vec<ScanEntry>,
    key: Option<String>,
}

impl InstallationFromScanService {
    pub fn new(source: Arc<dyn InstallationScanSource>, store: Arc<dyn InstallationStore>) -> Self {
        Self {
            source,
            library: InstallationLibrary::new(store),
            package_inspection: None,
        }
    }

    pub fn with_package_inspection(
        source: Arc<dyn InstallationScanSource>,
        store: Arc<dyn InstallationStore>,
        manifest_reader: Arc<dyn PackageManifestReader>,
        catalog_reader: Arc<dyn CatalogReader>,
    ) -> Self {
        Self {
            source,
            library: InstallationLibrary::new(store),
            package_inspection: Some(PackageInspectionDependencies {
                manifest_reader,
                catalog_reader,
            }),
        }
    }

    pub fn create_or_refresh(
        &self,
        request: CreateInstallationFromScanRequest,
    ) -> Result<Installation, InstallationFromScanError> {
        let selection = self
            .source
            .load(&request.session_id, &request.selected_result_id)?
            .ok_or(InstallationFromScanError::SourceNotFound)?;
        validate_selection(&request, &selection)?;

        let catalog_identity = selected_catalog_identity(&selection.selected_result)?;
        let scope = installation_scope(&selection, catalog_identity.as_ref())?;
        let installation_id =
            self.installation_id(&selection, &scope, catalog_identity.as_ref())?;
        let updated_at = selection
            .session
            .finished_at
            .clone()
            .ok_or_else(|| invalid_selection("completed scan has no finish timestamp"))?;
        let mut detection = classify_media(MediaClassificationRequest {
            source_scan_session_id: Some(selection.session.id.clone()),
            catalog_identity: catalog_identity.clone(),
            entries: &scope.entries,
        })?;
        if let Some(dependencies) = &self.package_inspection {
            detection.package_inspection = inspect_selected_package(
                dependencies,
                &selection.root,
                &selection.selected_result,
                &scope.entries,
                catalog_identity.as_ref(),
                &updated_at,
            )?;
        }
        if detection.catalog_identity.is_none() {
            detection.suggested_status = InstallationStatus::NeedsReview;
        }

        let installation = Installation {
            id: installation_id,
            scan_root_id: Some(selection.root.id.clone()),
            root_path: selection.root.display_path,
            platform: installation_platform(&selection.root.platform),
            status: detection.suggested_status,
            detection,
            overrides: InstallationOverrides::default(),
            discovered_at: selection.session.started_at,
            updated_at,
        };
        Ok(self.library.create_or_refresh(&installation)?)
    }

    fn installation_id(
        &self,
        selection: &InstallationScanSelection,
        scope: &InstallationScope,
        catalog_identity: Option<&CatalogIdentity>,
    ) -> Result<InstallationId, InstallationFromScanError> {
        let legacy_id = installation_id_for_scan_root(&selection.root);
        let Some(scope_key) = scope.key.as_deref() else {
            return Ok(legacy_id);
        };
        if let Some(existing) = self.library.read(&legacy_id)?
            && legacy_installation_matches(&existing, selection, catalog_identity)
        {
            return Ok(legacy_id);
        }
        Ok(installation_id_for_scan_scope(&selection.root, scope_key))
    }
}

fn inspect_selected_package(
    dependencies: &PackageInspectionDependencies,
    root: &ScanRoot,
    selected_result: &ScanResult,
    entries: &[ScanEntry],
    catalog_identity: Option<&CatalogIdentity>,
    inspected_at: &str,
) -> Result<Option<dla_domain::package::PackageInspection>, InstallationFromScanError> {
    let candidate_entry_id = selected_result
        .candidate_entry_id
        .as_ref()
        .ok_or_else(|| invalid_selection("selected result has no candidate entry"))?;
    let selected_entry = entries
        .iter()
        .find(|entry| &entry.id == candidate_entry_id)
        .ok_or_else(|| invalid_selection("selected candidate entry is absent"))?;
    if selected_entry.kind != ScanEntryKind::File {
        return Ok(None);
    }
    let Some(mut source_set) = discover_package_source_set(candidate_entry_id, entries)? else {
        return Ok(None);
    };
    for volume in &mut source_set.volumes {
        volume.sha256 = selected_result
            .evidence
            .iter()
            .find(|evidence| {
                evidence.kind == ScanEvidenceKind::ArchiveSha256
                    && evidence.source_entry_id.as_ref() == Some(&volume.scan_entry_id)
            })
            .map(|evidence| evidence.normalized_value.to_ascii_lowercase());
    }
    let source = source_set
        .volumes
        .first()
        .cloned()
        .ok_or_else(|| invalid_selection("package source set is empty"))?;
    let primary_entry = entries
        .iter()
        .find(|entry| entry.id == source.scan_entry_id)
        .ok_or_else(|| invalid_selection("package primary volume is absent"))?;
    let manifest = dependencies
        .manifest_reader
        .read_manifest(&root.display_path, &source_set)?;
    let catalog = catalog_identity
        .map(|identity| catalog_package_context(dependencies, identity, primary_entry, &source))
        .transpose()?
        .flatten();
    let mut inspection =
        classify_package(source, manifest, catalog.as_ref(), inspected_at.to_owned());
    inspection.source_set = Some(source_set);
    Ok(Some(inspection))
}

fn catalog_package_context(
    dependencies: &PackageInspectionDependencies,
    identity: &CatalogIdentity,
    entry: &ScanEntry,
    source: &SourceArtifact,
) -> Result<Option<CatalogPackageContext>, InstallationFromScanError> {
    let Some(detail) = dependencies.catalog_reader.read(&identity.work_code)? else {
        return Ok(None);
    };
    let source_name = source
        .relative_path
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or(source.relative_path.as_str());
    let source_size = entry.size.as_deref().unwrap_or_default();
    let rom_position = detail
        .roms
        .iter()
        .position(|rom| {
            source.sha256.as_ref().is_some_and(|sha256| {
                !rom.sha256.is_empty() && rom.sha256.eq_ignore_ascii_case(sha256)
            }) || (rom.name.eq_ignore_ascii_case(source_name) && rom.size == source_size)
        })
        .or_else(|| (detail.roms.len() == 1).then_some(0));
    let Some(rom_position) = rom_position else {
        return Ok(None);
    };
    let contents = dependencies
        .catalog_reader
        .read_rom_contents(&identity.work_code, rom_position)?;
    let category_names = detail
        .work
        .categories
        .iter()
        .flat_map(|category| [category.name.clone(), category.name_english.clone()])
        .filter(|name| !name.is_empty())
        .collect();
    let file_format_names = detail
        .file_formats
        .iter()
        .flat_map(|format| [format.name.clone(), format.name_english.clone()])
        .filter(|name| !name.is_empty())
        .collect();
    Ok(Some(CatalogPackageContext {
        work_code: identity.work_code.clone(),
        category_names,
        file_format_names,
        rom_position,
        rom_count: detail.roms.len(),
        rom: detail.roms[rom_position].clone(),
        contents,
    }))
}

pub fn installation_id_for_scan_root(root: &ScanRoot) -> InstallationId {
    InstallationId(format!("installation-{}", root.id.0))
}

fn installation_id_for_scan_scope(root: &ScanRoot, scope_key: &str) -> InstallationId {
    let encoded = scope_key
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    InstallationId(format!("installation-{}-scope-{encoded}", root.id.0))
}

fn installation_scope(
    selection: &InstallationScanSelection,
    catalog_identity: Option<&CatalogIdentity>,
) -> Result<InstallationScope, InstallationFromScanError> {
    let selected_entry_id = selection
        .selected_result
        .candidate_entry_id
        .as_ref()
        .ok_or_else(|| invalid_selection("selected result has no candidate entry"))?;
    let selected_entry = selection
        .entries
        .iter()
        .find(|entry| &entry.id == selected_entry_id)
        .ok_or_else(|| invalid_selection("selected candidate entry is absent"))?;
    let matched_work_codes = result_scope_work_codes(&selection.result_scopes, None);
    let whole_root = catalog_identity.is_some() && matched_work_codes.len() == 1
        || catalog_identity.is_none()
            && matched_work_codes.is_empty()
            && selection.result_scopes.len() == 1;
    if whole_root {
        return Ok(InstallationScope {
            entries: selection.entries.clone(),
            key: None,
        });
    }

    let (entry_ids, scope_key) = if let Some(source_set) =
        discover_package_source_set(selected_entry_id, &selection.entries)?
    {
        let primary = source_set
            .volumes
            .first()
            .ok_or_else(|| invalid_selection("package source set is empty"))?;
        if !selection
            .entries
            .iter()
            .any(|entry| entry.id == primary.scan_entry_id)
        {
            return Err(invalid_selection("package primary volume is absent"));
        }
        let ids = source_set
            .volumes
            .iter()
            .map(|volume| volume.scan_entry_id.clone())
            .collect::<BTreeSet<_>>();
        (ids, format!("package:entry:{}", primary.scan_entry_id.0))
    } else if let Some((directory, _)) = selected_entry.path_key.split_once('/') {
        let prefix = format!("{directory}/");
        let ids = selection
            .entries
            .iter()
            .filter(|entry| entry.path_key == directory || entry.path_key.starts_with(&prefix))
            .map(|entry| entry.id.clone())
            .collect::<BTreeSet<_>>();
        (ids, format!("directory:{directory}"))
    } else {
        (
            BTreeSet::from([selected_entry.id.clone()]),
            format!("entry:{}", selected_entry.id.0),
        )
    };
    let scoped_work_codes = result_scope_work_codes(&selection.result_scopes, Some(&entry_ids));
    if scoped_work_codes.len() > 1 {
        return Err(InstallationFromScanError::MultipleMatchedWorks(
            scoped_work_codes.into_iter().collect::<Vec<_>>().join(", "),
        ));
    }
    if let Some(identity) = catalog_identity
        && !scoped_work_codes.contains(&identity.work_code.trim().to_ascii_uppercase())
    {
        return Err(invalid_selection(
            "selected matched work is absent from its installation scope",
        ));
    }
    let entries = selection
        .entries
        .iter()
        .filter(|entry| entry_ids.contains(&entry.id))
        .cloned()
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Err(invalid_selection("selected installation scope is empty"));
    }
    Ok(InstallationScope {
        entries,
        key: Some(scope_key),
    })
}

fn result_scope_work_codes(
    result_scopes: &[InstallationScanResultScope],
    entry_ids: Option<&BTreeSet<ScanEntryId>>,
) -> BTreeSet<String> {
    result_scopes
        .iter()
        .filter(|scope| entry_ids.is_none_or(|ids| ids.contains(&scope.candidate_entry_id)))
        .filter_map(|scope| scope.matched_work_code.as_deref())
        .map(|code| code.trim().to_ascii_uppercase())
        .filter(|code| !code.is_empty())
        .collect()
}

fn legacy_installation_matches(
    installation: &Installation,
    selection: &InstallationScanSelection,
    catalog_identity: Option<&CatalogIdentity>,
) -> bool {
    let catalog_matches = catalog_identity.is_some_and(|identity| {
        installation
            .detection
            .catalog_identity
            .as_ref()
            .is_some_and(|existing| {
                existing
                    .work_code
                    .eq_ignore_ascii_case(identity.work_code.trim())
            })
    });
    let selected_path_key = selection
        .selected_result
        .candidate_entry_id
        .as_ref()
        .and_then(|entry_id| selection.entries.iter().find(|entry| &entry.id == entry_id))
        .map(|entry| entry.path_key.as_str());
    catalog_matches
        || selected_path_key.is_some_and(|path_key| {
            installation
                .detection
                .content_items
                .iter()
                .any(|item| item.path_key == path_key)
        })
}

fn validate_selection(
    request: &CreateInstallationFromScanRequest,
    selection: &InstallationScanSelection,
) -> Result<(), InstallationFromScanError> {
    if selection.session.id != request.session_id
        || selection.selected_result.id != request.selected_result_id
        || selection.selected_result.session_id != selection.session.id
        || selection.root.id != selection.session.root_id
    {
        return Err(invalid_selection(
            "root, session, result, and request identities do not share one scope",
        ));
    }
    if selection.session.status != ScanStatus::Completed {
        return Err(InstallationFromScanError::ScanNotCompleted(format!(
            "{:?}",
            selection.session.status
        )));
    }
    if selection.root.display_path.trim().is_empty() {
        return Err(invalid_selection("scan root display path is empty"));
    }

    let candidate_entry_id = selection
        .selected_result
        .candidate_entry_id
        .as_ref()
        .ok_or_else(|| invalid_selection("selected result has no candidate entry"))?;
    if !selection.entries.iter().any(|entry| {
        entry.id == *candidate_entry_id
            && entry.root_id == selection.root.id
            && entry.presence == ScanEntryPresence::Present
            && entry.last_seen_session_id.as_ref() == Some(&selection.session.id)
    }) {
        return Err(invalid_selection(
            "selected candidate is not a present entry from the completed session",
        ));
    }
    if selection.entries.iter().any(|entry| {
        entry.root_id != selection.root.id
            || entry.presence != ScanEntryPresence::Present
            || entry.last_seen_session_id.as_ref() != Some(&selection.session.id)
    }) {
        return Err(invalid_selection(
            "classification entries are outside the completed scan snapshot",
        ));
    }

    if selection.result_scopes.iter().any(|scope| {
        !selection
            .entries
            .iter()
            .any(|entry| entry.id == scope.candidate_entry_id)
    }) {
        return Err(invalid_selection(
            "scan result scope references an entry outside the completed snapshot",
        ));
    }
    if selection.selected_result.outcome == ScanMatchOutcome::Matched {
        let selected_work_code = selection
            .selected_result
            .selected_work_code
            .as_deref()
            .ok_or_else(|| invalid_selection("matched result has no selected work code"))?
            .trim()
            .to_ascii_uppercase();
        if !selection.result_scopes.iter().any(|scope| {
            scope.candidate_entry_id == *candidate_entry_id
                && scope
                    .matched_work_code
                    .as_deref()
                    .is_some_and(|code| code.trim().eq_ignore_ascii_case(&selected_work_code))
        }) {
            return Err(invalid_selection(
                "selected matched work is absent from the session identity set",
            ));
        }
    }
    Ok(())
}

fn selected_catalog_identity(
    result: &ScanResult,
) -> Result<Option<CatalogIdentity>, InstallationFromScanError> {
    if result.outcome != ScanMatchOutcome::Matched {
        return Ok(None);
    }
    let work_code = result
        .selected_work_code
        .as_deref()
        .ok_or_else(|| invalid_selection("matched result has no selected work code"))?
        .trim()
        .to_ascii_uppercase();
    let confidence = result
        .confidence
        .ok_or_else(|| invalid_selection("matched result has no identity confidence"))?;
    let candidate = result
        .candidates
        .iter()
        .find(|candidate| candidate.work_code.eq_ignore_ascii_case(&work_code))
        .ok_or_else(|| invalid_selection("matched result has no corresponding candidate"))?;
    if candidate.reason_codes.is_empty() {
        return Err(invalid_selection(
            "selected catalog candidate has no reason codes",
        ));
    }
    Ok(Some(CatalogIdentity {
        work_code,
        confidence,
        reason_codes: candidate.reason_codes.clone(),
    }))
}

fn installation_platform(platform: &str) -> InstallationPlatform {
    match platform.trim().to_ascii_lowercase().as_str() {
        "windows" => InstallationPlatform::Windows,
        "linux" => InstallationPlatform::Linux,
        "macos" => InstallationPlatform::Macos,
        "android" => InstallationPlatform::Android,
        "ios" => InstallationPlatform::Ios,
        _ => InstallationPlatform::Unknown,
    }
}

fn invalid_selection(message: impl Into<String>) -> InstallationFromScanError {
    InstallationFromScanError::InvalidSelection(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_domain::{
        installation::{
            ContentItemOverride, InstallationDetection, LaunchActionKind, LaunchTarget,
            ManualLaunchSelection, MediaType, RelativePath,
        },
        scanner::{
            ScanCounters, ScanEntryId, ScanEntryKind, ScanMatchCandidate, ScanMatchConfidence,
            ScanOptions, ScanRootId,
        },
    };

    use super::*;

    struct MemorySource {
        selection: Option<InstallationScanSelection>,
    }

    impl InstallationScanSource for MemorySource {
        fn load(
            &self,
            _session_id: &ScanSessionId,
            _selected_result_id: &ScanResultId,
        ) -> Result<Option<InstallationScanSelection>, InstallationScanSourceError> {
            Ok(self.selection.clone())
        }
    }

    #[derive(Default)]
    struct MemoryStore {
        installation: Mutex<Option<Installation>>,
        writes: Mutex<usize>,
    }

    impl InstallationStore for MemoryStore {
        fn create(&self, installation: &Installation) -> Result<(), InstallationLibraryError> {
            *self.installation.lock().expect("installation") = Some(installation.clone());
            *self.writes.lock().expect("writes") += 1;
            Ok(())
        }

        fn create_or_refresh(
            &self,
            installation: &Installation,
        ) -> Result<Installation, InstallationLibraryError> {
            let mut guard = self.installation.lock().expect("installation");
            let stored = if let Some(mut existing) = guard.clone() {
                let updated_at = existing
                    .updated_at
                    .clone()
                    .max(installation.updated_at.clone());
                existing.replace_detection(installation.detection.clone(), updated_at)?;
                existing
            } else {
                installation.clone()
            };
            *guard = Some(stored.clone());
            *self.writes.lock().expect("writes") += 1;
            Ok(stored)
        }

        fn read(
            &self,
            _installation_id: &InstallationId,
        ) -> Result<Option<Installation>, InstallationLibraryError> {
            Ok(self.installation.lock().expect("installation").clone())
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(self
                .installation
                .lock()
                .expect("installation")
                .clone()
                .into_iter()
                .collect())
        }

        fn replace_detection(
            &self,
            _installation_id: &InstallationId,
            detection: &InstallationDetection,
            _status: InstallationStatus,
            updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            self.installation
                .lock()
                .expect("installation")
                .as_mut()
                .expect("stored installation")
                .replace_detection(detection.clone(), updated_at.to_owned())?;
            Ok(())
        }

        fn replace_overrides(
            &self,
            _installation_id: &InstallationId,
            overrides: &InstallationOverrides,
            _status: InstallationStatus,
            updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            self.installation
                .lock()
                .expect("installation")
                .as_mut()
                .expect("stored installation")
                .replace_overrides(overrides.clone(), updated_at.to_owned())?;
            Ok(())
        }
    }

    #[test]
    fn creates_a_ready_installation_from_one_completed_matched_scan() {
        let selection = selection(ScanStatus::Completed, ScanMatchOutcome::Matched);
        let store = Arc::new(MemoryStore::default());
        let installation = service(selection, store.clone())
            .create_or_refresh(request())
            .expect("create installation");

        assert_eq!(
            installation.id,
            InstallationId("installation-root-1".to_owned())
        );
        assert_eq!(installation.status, InstallationStatus::Ready);
        assert_eq!(installation.platform, InstallationPlatform::Windows);
        assert_eq!(installation.detection.content_items.len(), 1);
        assert_eq!(installation.detection.launch_candidates.len(), 1);
        assert_eq!(
            installation
                .detection
                .catalog_identity
                .as_ref()
                .map(|identity| identity.work_code.as_str()),
            Some("RJ01326398")
        );
        assert_eq!(*store.writes.lock().expect("writes"), 1);
    }

    #[test]
    fn unmatched_identity_requires_review_even_with_one_launch_candidate() {
        let selection = selection(ScanStatus::Completed, ScanMatchOutcome::Unmatched);
        let store = Arc::new(MemoryStore::default());
        let installation = service(selection, store)
            .create_or_refresh(request())
            .expect("create installation");

        assert_eq!(installation.status, InstallationStatus::NeedsReview);
        assert!(installation.detection.catalog_identity.is_none());
        assert_eq!(installation.detection.launch_candidates.len(), 1);
    }

    #[test]
    fn rejects_incomplete_scans_before_writing() {
        let selection = selection(ScanStatus::Running, ScanMatchOutcome::Matched);
        let store = Arc::new(MemoryStore::default());
        let error = service(selection, store.clone())
            .create_or_refresh(request())
            .expect_err("running scan must fail");

        assert!(matches!(
            error,
            InstallationFromScanError::ScanNotCompleted(_)
        ));
        assert_eq!(*store.writes.lock().expect("writes"), 0);
    }

    #[test]
    fn scopes_a_selected_result_inside_a_multi_work_root() {
        let mut selection = selection(ScanStatus::Completed, ScanMatchOutcome::Matched);
        let mut other_entry = selection.entries[0].clone();
        other_entry.id = ScanEntryId("entry-2".to_owned());
        other_entry.relative_path = "Other.exe".to_owned();
        other_entry.path_key = "other.exe".to_owned();
        selection.entries.push(other_entry);
        selection.result_scopes.push(InstallationScanResultScope {
            candidate_entry_id: ScanEntryId("entry-2".to_owned()),
            matched_work_code: Some("RJ01653537".to_owned()),
        });
        let store = Arc::new(MemoryStore::default());
        let expected_id = installation_id_for_scan_scope(&selection.root, "entry:entry-1");
        let installation = service(selection, store.clone())
            .create_or_refresh(request())
            .expect("selected work installation");

        assert_eq!(installation.id, expected_id);
        assert_eq!(installation.detection.content_items.len(), 1);
        assert_eq!(
            installation.detection.content_items[0]
                .relative_path
                .as_str(),
            "Game.exe"
        );
        assert_eq!(*store.writes.lock().expect("writes"), 1);
    }

    #[test]
    fn rejects_a_selected_directory_that_still_contains_multiple_matched_works() {
        let mut selection = selection(ScanStatus::Completed, ScanMatchOutcome::Matched);
        selection.entries[0].relative_path = "bundle/Game.exe".to_owned();
        selection.entries[0].path_key = "bundle/game.exe".to_owned();
        let mut other_entry = selection.entries[0].clone();
        other_entry.id = ScanEntryId("entry-2".to_owned());
        other_entry.relative_path = "bundle/Other.exe".to_owned();
        other_entry.path_key = "bundle/other.exe".to_owned();
        selection.entries.push(other_entry);
        selection.result_scopes.push(InstallationScanResultScope {
            candidate_entry_id: ScanEntryId("entry-2".to_owned()),
            matched_work_code: Some("RJ01653537".to_owned()),
        });
        let store = Arc::new(MemoryStore::default());
        let error = service(selection, store.clone())
            .create_or_refresh(request())
            .expect_err("mixed selected directory must fail");

        assert!(matches!(
            error,
            InstallationFromScanError::MultipleMatchedWorks(_)
        ));
        assert_eq!(*store.writes.lock().expect("writes"), 0);
    }

    #[test]
    fn rejects_entries_from_another_scan_snapshot_before_writing() {
        let mut selection = selection(ScanStatus::Completed, ScanMatchOutcome::Matched);
        selection.entries[0].last_seen_session_id = Some(ScanSessionId("session-old".to_owned()));
        let store = Arc::new(MemoryStore::default());
        let error = service(selection, store.clone())
            .create_or_refresh(request())
            .expect_err("stale scan entry must fail");

        assert!(matches!(
            error,
            InstallationFromScanError::InvalidSelection(_)
        ));
        assert_eq!(*store.writes.lock().expect("writes"), 0);
    }

    #[test]
    fn refresh_preserves_manual_overrides() {
        let selection = selection(ScanStatus::Completed, ScanMatchOutcome::Matched);
        let store = Arc::new(MemoryStore::default());
        let service = service(selection.clone(), store.clone());
        let mut existing = service
            .create_or_refresh(request())
            .expect("create installation");
        let game = RelativePath::parse("Game.exe").expect("game path");
        existing.overrides = InstallationOverrides {
            custom_title: Some("My game".to_owned()),
            preferred_action: Some(ManualLaunchSelection {
                action: LaunchActionKind::LaunchExecutable,
                target: LaunchTarget::RelativePath(game.clone()),
            }),
            content_items: vec![ContentItemOverride {
                relative_path: game,
                media_type: Some(MediaType::Executable),
                ignored: false,
                order: None,
            }],
            ..InstallationOverrides::default()
        };
        *store.installation.lock().expect("installation") = Some(existing.clone());

        let refreshed = service
            .create_or_refresh(request())
            .expect("refresh installation");

        assert_eq!(refreshed.overrides, existing.overrides);
        assert_eq!(refreshed.discovered_at, existing.discovered_at);
    }

    fn service(
        selection: InstallationScanSelection,
        store: Arc<MemoryStore>,
    ) -> InstallationFromScanService {
        let source: Arc<dyn InstallationScanSource> = Arc::new(MemorySource {
            selection: Some(selection),
        });
        let store: Arc<dyn InstallationStore> = store;
        InstallationFromScanService::new(source, store)
    }

    fn request() -> CreateInstallationFromScanRequest {
        CreateInstallationFromScanRequest {
            session_id: ScanSessionId("session-1".to_owned()),
            selected_result_id: ScanResultId("result-1".to_owned()),
        }
    }

    fn selection(status: ScanStatus, outcome: ScanMatchOutcome) -> InstallationScanSelection {
        let session_id = ScanSessionId("session-1".to_owned());
        let root_id = ScanRootId("root-1".to_owned());
        let entry_id = ScanEntryId("entry-1".to_owned());
        let matched = outcome == ScanMatchOutcome::Matched;
        InstallationScanSelection {
            root: ScanRoot {
                id: root_id.clone(),
                platform: "windows".to_owned(),
                path_key: "c:/fixtures/game".to_owned(),
                display_path: "C:\\fixtures\\game".to_owned(),
                created_at: "2026-08-07T00:00:00Z".to_owned(),
                updated_at: "2026-08-07T00:01:00Z".to_owned(),
            },
            session: ScanSession {
                id: session_id.clone(),
                root_id: root_id.clone(),
                status,
                options: ScanOptions::default(),
                counters: ScanCounters {
                    discovered_files: 1,
                    inspected_files: 1,
                    matched: u64::from(matched),
                    unmatched: u64::from(!matched),
                    ..ScanCounters::default()
                },
                started_at: "2026-08-07T00:00:00Z".to_owned(),
                finished_at: (status == ScanStatus::Completed)
                    .then(|| "2026-08-07T00:01:00Z".to_owned()),
                fatal_error_code: None,
                fatal_error_message: None,
            },
            selected_result: ScanResult {
                id: ScanResultId("result-1".to_owned()),
                session_id: session_id.clone(),
                candidate_entry_id: Some(entry_id.clone()),
                outcome,
                selected_work_code: matched.then(|| "RJ01326398".to_owned()),
                confidence: matched.then_some(ScanMatchConfidence::Exact),
                candidates: if matched {
                    vec![ScanMatchCandidate {
                        work_code: "RJ01326398".to_owned(),
                        confidence: ScanMatchConfidence::Exact,
                        reason_codes: vec!["archive_sha256_match".to_owned()],
                        rank: 1,
                    }]
                } else {
                    Vec::new()
                },
                evidence: Vec::new(),
                created_at: "2026-08-07T00:00:30Z".to_owned(),
                updated_at: "2026-08-07T00:00:30Z".to_owned(),
            },
            entries: vec![ScanEntry {
                id: entry_id,
                root_id,
                relative_path: "Game.exe".to_owned(),
                path_key: "game.exe".to_owned(),
                kind: ScanEntryKind::File,
                extension: "exe".to_owned(),
                size: Some("7".to_owned()),
                modified_at: Some("2026-08-07T00:00:00Z".to_owned()),
                presence: ScanEntryPresence::Present,
                first_seen_session_id: Some(session_id.clone()),
                last_seen_session_id: Some(session_id),
                created_at: "2026-08-07T00:00:00Z".to_owned(),
                updated_at: "2026-08-07T00:00:30Z".to_owned(),
            }],
            result_scopes: vec![InstallationScanResultScope {
                candidate_entry_id: ScanEntryId("entry-1".to_owned()),
                matched_work_code: matched.then(|| "RJ01326398".to_owned()),
            }],
        }
    }
}
