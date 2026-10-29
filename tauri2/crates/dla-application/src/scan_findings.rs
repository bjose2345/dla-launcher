use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use dla_detection::{MultipartVolumeName, discover_package_source_set};
use dla_domain::{
    installation::{Installation, InstallationId, ManualCatalogIdentity},
    scanner::{
        ScanEntry, ScanEntryId, ScanEntryKind, ScanMatchConfidence, ScanMatchOutcome, ScanResult,
        ScanResultId, ScanRootId, ScanSessionId, ScanStatus,
    },
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    installation::{InstallationLibrary, InstallationLibraryError, InstallationStore},
    installation_from_scan::{
        CreateInstallationFromScanRequest, InstallationFromScanService,
        InstallationScanResultScope, InstallationScanSnapshot, InstallationScanSource,
        InstallationScanSourceError, directory_scope_key, entry_scope_key,
        installation_id_for_scan_root, installation_id_for_scan_scope, legacy_installation_matches,
        may_be_package_volume, package_scope_key, session_is_one_work,
    },
    installation_review::{
        InstallationReviewError, InstallationReviewRequest, InstallationReviewService,
    },
};

/// The key of the only finding of a root that is one work.
pub const ROOT_FINDING_KEY: &str = "root";
const CANDIDATE_LIMIT: usize = 6;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanFindingKind {
    Folder,
    Archive,
    File,
}

/// What adding a finding needs from the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanFindingState {
    /// One catalog work matches on strong or exact evidence.
    Ready,
    /// Several catalog works fit, or the only fit is weak.
    Choose,
    /// No catalog work fits.
    Unknown,
    /// Files of more than one catalog work share this folder.
    Mixed,
    /// An archive set with a missing or repeated volume.
    Incomplete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFindingCandidate {
    pub work_code: String,
    pub confidence: ScanMatchConfidence,
}

/// What is wrong with an archive set the scanner could not take as a whole.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncompleteArchiveSet {
    /// Part numbers no file in the folder holds.
    pub missing_parts: Vec<u32>,
    /// Files that hold a part under a name 7-Zip cannot follow.
    pub renames: Vec<VolumeRename>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeRename {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFindingInLibrary {
    pub installation_id: InstallationId,
    /// False when the Library holds another copy of the same work.
    pub same_files: bool,
    /// The catalog work the Library entry is matched to now.
    pub work_code: Option<String>,
}

/// One work a scan found: an archive set, a folder, a file, or the whole
/// root, with what the scanner could tell about its identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFinding {
    pub key: String,
    /// Below the scan root; empty when the whole root is one work.
    pub relative_path: String,
    pub kind: ScanFindingKind,
    pub state: ScanFindingState,
    pub work_code: Option<String>,
    pub candidates: Vec<ScanFindingCandidate>,
    pub file_count: u64,
    pub size_bytes: u64,
    pub volume_count: usize,
    /// Set when the state is incomplete because of the set's volumes.
    pub incomplete_set: Option<IncompleteArchiveSet>,
    pub in_library: Option<ScanFindingInLibrary>,
    pub ignored: bool,
    #[serde(skip)]
    result_id: ScanResultId,
    #[serde(skip)]
    identity_detected: bool,
    #[serde(skip)]
    files: Vec<String>,
}

impl ScanFinding {
    /// The paths below the scan root of the files this finding holds.
    pub fn files(&self) -> &[String] {
        &self.files
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanFindingList {
    pub session_id: ScanSessionId,
    pub root_id: ScanRootId,
    pub root_path: String,
    pub findings: Vec<ScanFinding>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddScanFinding {
    pub key: String,
    /// The catalog work the user picked, which a finding to choose needs.
    #[serde(default)]
    pub work_code: Option<String>,
    /// Adds the finding without a catalog work.
    #[serde(default)]
    pub unknown: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddScanFindingsRequest {
    pub session_id: ScanSessionId,
    pub findings: Vec<AddScanFinding>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanFindingRefusal {
    NotFound,
    ChooseWork,
    Mixed,
    Incomplete,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AddedScanFinding {
    Added {
        key: String,
        installation_id: InstallationId,
    },
    /// An archive joins the Library once it is installed; its identity is
    /// confirmed then.
    NeedsInstall {
        key: String,
        installation_id: InstallationId,
        identity: Option<ManualCatalogIdentity>,
        /// The finding's path below the scan root, or the root's own name.
        name: String,
        work_code: Option<String>,
    },
    AlreadyInLibrary {
        key: String,
        installation_id: InstallationId,
    },
    Refused {
        key: String,
        reason: ScanFindingRefusal,
        detail: String,
    },
}

#[derive(Debug, Error)]
pub enum ScanFindingError {
    #[error("scan session was not found")]
    SessionNotFound,
    #[error("scan finding key is invalid: {0}")]
    InvalidKey(String),
    #[error(transparent)]
    Source(#[from] InstallationScanSourceError),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
}

/// Findings the user chose to leave out of later scans of the same root.
pub trait IgnoredScanFindingStore: Send + Sync {
    fn read_ignored_scan_findings(
        &self,
        root_id: &ScanRootId,
    ) -> Result<BTreeSet<String>, InstallationScanSourceError>;
    fn set_scan_finding_ignored(
        &self,
        root_id: &ScanRootId,
        key: &str,
        ignored: bool,
        now: &str,
    ) -> Result<(), InstallationScanSourceError>;
}

pub struct ScanFindingService {
    source: Arc<dyn InstallationScanSource>,
    ignored: Arc<dyn IgnoredScanFindingStore>,
    library: InstallationLibrary,
    installations: Arc<InstallationFromScanService>,
    review: Arc<InstallationReviewService>,
}

impl ScanFindingService {
    pub fn new(
        source: Arc<dyn InstallationScanSource>,
        ignored: Arc<dyn IgnoredScanFindingStore>,
        store: Arc<dyn InstallationStore>,
        installations: Arc<InstallationFromScanService>,
        review: Arc<InstallationReviewService>,
    ) -> Self {
        Self {
            source,
            ignored,
            library: InstallationLibrary::new(store),
            installations,
            review,
        }
    }

    /// A completed session's findings; a running or stopped session has none.
    pub fn list(&self, session_id: &ScanSessionId) -> Result<ScanFindingList, ScanFindingError> {
        let snapshot = self
            .source
            .load_session(session_id)?
            .ok_or(ScanFindingError::SessionNotFound)?;
        let findings = if snapshot.session.status == ScanStatus::Completed {
            let ignored = self.ignored.read_ignored_scan_findings(&snapshot.root.id)?;
            let installations = self.library.list()?;
            scan_findings(&snapshot, &ignored, &installations)
        } else {
            Vec::new()
        };
        Ok(ScanFindingList {
            session_id: snapshot.session.id,
            root_id: snapshot.root.id,
            root_path: snapshot.root.display_path,
            findings,
        })
    }

    pub fn set_ignored(
        &self,
        root_id: &ScanRootId,
        key: &str,
        ignored: bool,
        now: &str,
    ) -> Result<(), ScanFindingError> {
        let valid = key == ROOT_FINDING_KEY
            || ["package:entry:", "directory:", "entry:"]
                .iter()
                .any(|prefix| key.len() > prefix.len() && key.starts_with(prefix));
        if !valid {
            return Err(ScanFindingError::InvalidKey(key.to_owned()));
        }
        self.ignored
            .set_scan_finding_ignored(root_id, key, ignored, now)?;
        Ok(())
    }

    /// Adds findings in one step: a folder or file joins the Library at once
    /// with its recommended start action; an archive is created and waits to
    /// be installed.
    pub fn add(
        &self,
        request: AddScanFindingsRequest,
        reviewed_at: &str,
    ) -> Result<Vec<AddedScanFinding>, ScanFindingError> {
        let list = self.list(&request.session_id)?;
        let by_key = list
            .findings
            .iter()
            .map(|finding| (finding.key.as_str(), finding))
            .collect::<HashMap<_, _>>();
        Ok(request
            .findings
            .into_iter()
            .map(|item| {
                let finding = by_key.get(item.key.as_str()).copied();
                self.add_one(&request.session_id, finding, item, reviewed_at)
            })
            .collect())
    }

    fn add_one(
        &self,
        session_id: &ScanSessionId,
        finding: Option<&ScanFinding>,
        item: AddScanFinding,
        reviewed_at: &str,
    ) -> AddedScanFinding {
        let refused = |reason, detail: String| AddedScanFinding::Refused {
            key: item.key.clone(),
            reason,
            detail,
        };
        let Some(finding) = finding else {
            return refused(
                ScanFindingRefusal::NotFound,
                "the scan no longer lists this finding".to_owned(),
            );
        };
        if let Some(in_library) = &finding.in_library
            && in_library.same_files
        {
            return AddedScanFinding::AlreadyInLibrary {
                key: item.key,
                installation_id: in_library.installation_id.clone(),
            };
        }
        let identity = match chosen_identity(finding, &item) {
            Ok(identity) => identity,
            Err(reason) => return refused(reason, String::new()),
        };
        let installation =
            match self
                .installations
                .create_or_refresh(CreateInstallationFromScanRequest {
                    session_id: session_id.clone(),
                    selected_result_id: finding.result_id.clone(),
                }) {
                Ok(installation) => installation,
                Err(error) => return refused(ScanFindingRefusal::Failed, error.to_string()),
            };
        if installation.detection.package_inspection.is_some() {
            let work_code = match &identity {
                Some(ManualCatalogIdentity::CatalogWork { work_code }) => Some(work_code.clone()),
                Some(ManualCatalogIdentity::Unidentified) => None,
                None => finding.work_code.clone(),
            };
            let name = if finding.relative_path.is_empty() {
                root_name(&installation.root_path)
            } else {
                finding.relative_path.clone()
            };
            return AddedScanFinding::NeedsInstall {
                key: item.key,
                installation_id: installation.id,
                identity,
                name,
                work_code,
            };
        }
        match self.confirm(&installation, identity, reviewed_at) {
            Ok(saved) => AddedScanFinding::Added {
                key: item.key,
                installation_id: saved.id,
            },
            Err(error) => refused(ScanFindingRefusal::Failed, error.to_string()),
        }
    }

    /// Saves the review a one-step add implies: the chosen identity and, for
    /// files that need no installing, the recommended start action. What the
    /// user already set is kept.
    pub fn confirm(
        &self,
        installation: &Installation,
        identity: Option<ManualCatalogIdentity>,
        reviewed_at: &str,
    ) -> Result<Installation, InstallationReviewError> {
        let overrides = &installation.overrides;
        let preferred_action = overrides.preferred_action.clone().or_else(|| {
            installation
                .detection
                .package_inspection
                .is_none()
                .then(|| installation.detection.recommended_launch())
                .flatten()
        });
        self.review.save(
            InstallationReviewRequest {
                installation_id: installation.id.clone(),
                catalog_identity: identity.or_else(|| overrides.catalog_identity.clone()),
                custom_title: overrides.custom_title.clone(),
                preferred_action,
                content_items: overrides.content_items.clone(),
            },
            reviewed_at.to_owned(),
        )
    }
}

/// The catalog identity adding a finding would save, or why it cannot be
/// added, checked before anything is moved for it.
pub fn identity_for_add(
    finding: &ScanFinding,
    item: &AddScanFinding,
) -> Result<Option<ManualCatalogIdentity>, ScanFindingRefusal> {
    chosen_identity(finding, item)
}

fn root_name(root_path: &str) -> String {
    root_path
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(root_path)
        .to_owned()
}

fn chosen_identity(
    finding: &ScanFinding,
    item: &AddScanFinding,
) -> Result<Option<ManualCatalogIdentity>, ScanFindingRefusal> {
    match finding.state {
        ScanFindingState::Mixed => return Err(ScanFindingRefusal::Mixed),
        ScanFindingState::Incomplete => return Err(ScanFindingRefusal::Incomplete),
        _ => {}
    }
    if item.unknown {
        return Ok(Some(ManualCatalogIdentity::Unidentified));
    }
    if let Some(code) = item.work_code.as_deref().map(str::trim)
        && !code.is_empty()
    {
        let detected = finding.identity_detected
            && finding
                .work_code
                .as_deref()
                .is_some_and(|work_code| work_code.eq_ignore_ascii_case(code));
        return Ok((!detected).then(|| ManualCatalogIdentity::CatalogWork {
            work_code: code.to_ascii_uppercase(),
        }));
    }
    match (finding.state, &finding.work_code) {
        (ScanFindingState::Ready, Some(code)) => {
            Ok(
                (!finding.identity_detected).then(|| ManualCatalogIdentity::CatalogWork {
                    work_code: code.clone(),
                }),
            )
        }
        _ => Err(ScanFindingRefusal::ChooseWork),
    }
}

struct Group<'a> {
    key: String,
    relative_path: String,
    kind: ScanFindingKind,
    incomplete: Option<IncompleteArchiveSet>,
    volume_count: usize,
    primary: Option<ScanEntryId>,
    files: Vec<&'a ScanEntry>,
}

fn scan_findings(
    snapshot: &InstallationScanSnapshot,
    ignored: &BTreeSet<String>,
    installations: &[Installation],
) -> Vec<ScanFinding> {
    let result_by_entry = snapshot
        .results
        .iter()
        .filter_map(|result| {
            result
                .candidate_entry_id
                .as_ref()
                .map(|entry_id| (entry_id, result))
        })
        .collect::<HashMap<_, _>>();
    let result_scopes = snapshot
        .results
        .iter()
        .filter_map(|result| {
            result.candidate_entry_id.clone().map(|candidate_entry_id| {
                InstallationScanResultScope {
                    candidate_entry_id,
                    matched_work_code: (result.outcome == ScanMatchOutcome::Matched)
                        .then(|| result.selected_work_code.clone())
                        .flatten()
                        .map(|code| code.trim().to_ascii_uppercase()),
                }
            })
        })
        .collect::<Vec<_>>();
    let one_work = session_is_one_work(&snapshot.entries, &result_scopes);
    let mut groups = group_files(&snapshot.entries);
    if one_work && !groups.is_empty() {
        let group = if groups.len() == 1 {
            groups.remove(0)
        } else {
            Group {
                key: String::new(),
                relative_path: String::new(),
                kind: ScanFindingKind::Folder,
                incomplete: None,
                volume_count: 0,
                primary: None,
                files: groups.into_iter().flat_map(|group| group.files).collect(),
            }
        };
        groups = vec![Group {
            key: ROOT_FINDING_KEY.to_owned(),
            ..group
        }];
    }

    let by_id = installations
        .iter()
        .map(|installation| (&installation.id, installation))
        .collect::<HashMap<_, _>>();
    let mut reviewed_by_code = HashMap::new();
    for installation in installations {
        if installation.overrides.reviewed_at.is_some()
            && let Some(code) = installation.effective_catalog_work_code()
        {
            reviewed_by_code
                .entry(code.to_ascii_uppercase())
                .or_insert_with(|| installation.id.clone());
        }
    }
    let legacy_id = installation_id_for_scan_root(&snapshot.root);

    let mut findings = groups
        .into_iter()
        .filter(|group| !is_incidental(group))
        .filter_map(|group| {
            let results = group
                .files
                .iter()
                .filter_map(|entry| result_by_entry.get(&entry.id).copied())
                .collect::<Vec<_>>();
            let representative = representative(&group, &results, &result_by_entry)?;
            let mut matched = BTreeMap::<String, ScanMatchConfidence>::new();
            let mut candidates = BTreeMap::<String, ScanMatchConfidence>::new();
            for result in &results {
                if result.outcome == ScanMatchOutcome::Matched
                    && let (Some(code), Some(confidence)) =
                        (&result.selected_work_code, result.confidence)
                {
                    raise(&mut matched, code, confidence);
                }
                for candidate in &result.candidates {
                    raise(&mut candidates, &candidate.work_code, candidate.confidence);
                }
            }
            let work_code = (matched.len() == 1)
                .then(|| matched.keys().next().cloned())
                .flatten();
            let state = if group.incomplete.is_some() {
                ScanFindingState::Incomplete
            } else if matched.len() > 1 {
                ScanFindingState::Mixed
            } else if matched
                .values()
                .next()
                .is_some_and(|confidence| *confidence >= ScanMatchConfidence::Strong)
            {
                ScanFindingState::Ready
            } else if !candidates.is_empty() {
                ScanFindingState::Choose
            } else {
                ScanFindingState::Unknown
            };
            let mut candidates = candidates
                .into_iter()
                .map(|(work_code, confidence)| ScanFindingCandidate {
                    work_code,
                    confidence,
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|left, right| {
                right
                    .confidence
                    .cmp(&left.confidence)
                    .then_with(|| left.work_code.cmp(&right.work_code))
            });
            candidates.truncate(CANDIDATE_LIMIT);
            let identity_detected = representative.outcome == ScanMatchOutcome::Matched
                && representative
                    .selected_work_code
                    .as_deref()
                    .zip(work_code.as_deref())
                    .is_some_and(|(selected, code)| selected.eq_ignore_ascii_case(code));

            let representative_path = representative
                .candidate_entry_id
                .as_ref()
                .and_then(|entry_id| group.files.iter().find(|entry| &entry.id == entry_id))
                .map(|entry| entry.path_key.as_str());
            let reuses_legacy = group.key == ROOT_FINDING_KEY
                || by_id.get(&legacy_id).is_some_and(|existing| {
                    legacy_installation_matches(existing, representative_path, work_code.as_deref())
                });
            let installation_id = if reuses_legacy {
                legacy_id.clone()
            } else {
                installation_id_for_scan_scope(&snapshot.root, &group.key)
            };
            let in_library = by_id
                .get(&installation_id)
                .filter(|installation| installation.overrides.reviewed_at.is_some())
                .map(|installation| ScanFindingInLibrary {
                    installation_id: installation.id.clone(),
                    same_files: true,
                    work_code: installation
                        .effective_catalog_work_code()
                        .map(str::to_owned),
                })
                .or_else(|| {
                    let code = work_code.as_deref()?.to_ascii_uppercase();
                    let installation_id = reviewed_by_code.get(&code)?;
                    Some(ScanFindingInLibrary {
                        installation_id: installation_id.clone(),
                        same_files: false,
                        work_code: Some(code),
                    })
                });

            Some(ScanFinding {
                ignored: ignored.contains(&group.key),
                key: group.key,
                relative_path: group.relative_path,
                kind: group.kind,
                state,
                work_code,
                candidates,
                file_count: group.files.len() as u64,
                size_bytes: group
                    .files
                    .iter()
                    .filter_map(|entry| entry.size.as_deref()?.parse::<u64>().ok())
                    .sum(),
                volume_count: group.volume_count,
                incomplete_set: group.incomplete,
                in_library,
                result_id: representative.id.clone(),
                identity_detected,
                files: group
                    .files
                    .iter()
                    .map(|entry| entry.relative_path.clone())
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    findings.sort_by(|left, right| {
        left.relative_path
            .to_lowercase()
            .cmp(&right.relative_path.to_lowercase())
            .then_with(|| left.key.cmp(&right.key))
    });
    findings
}

/// Groups files the way installation scopes divide a root of several works:
/// archive sets first, then top-level folders, then top-level files.
fn group_files(entries: &[ScanEntry]) -> Vec<Group<'_>> {
    let by_id = entries
        .iter()
        .map(|entry| (&entry.id, entry))
        .collect::<HashMap<_, _>>();
    let files = entries
        .iter()
        .filter(|entry| entry.kind == ScanEntryKind::File)
        .collect::<Vec<_>>();
    let mut groups = Vec::new();
    let mut index_by_key = HashMap::new();
    let mut assigned = BTreeSet::<&ScanEntryId>::new();

    // Named volumes go first, so a set claims a misnamed volume before that
    // file is read as an archive of its own.
    let mut volumes = files
        .iter()
        .copied()
        .filter(|entry| may_be_package_volume(entry))
        .collect::<Vec<_>>();
    volumes.sort_by_key(|entry| MultipartVolumeName::parse(&entry.relative_path).is_none());
    for entry in volumes {
        if assigned.contains(&entry.id) {
            continue;
        }
        let discovered = discover_package_source_set(&entry.id, entries);
        let name = MultipartVolumeName::parse(&entry.relative_path);
        let (set_files, incomplete) = match (discovered, name) {
            (Ok(Some(source_set)), name) => {
                let set_files = source_set
                    .volumes
                    .iter()
                    .filter_map(|volume| by_id.get(&volume.scan_entry_id).copied())
                    .collect::<Vec<_>>();
                match name {
                    Some(name) => {
                        let (misnamed, problem) =
                            volume_problems(&name, &set_files, &files, &assigned);
                        if misnamed.is_empty() && problem.missing_parts.is_empty() {
                            (set_files, None)
                        } else {
                            ([set_files, misnamed].concat(), Some(problem))
                        }
                    }
                    None => (set_files, None),
                }
            }
            (Ok(None), _) => continue,
            (Err(_), Some(name)) => {
                let mut set_files = files
                    .iter()
                    .copied()
                    .filter(|file| !assigned.contains(&file.id))
                    .filter_map(|file| {
                        let part = MultipartVolumeName::parse(&file.relative_path)
                            .filter(|other| other.same_set(&name))?
                            .part;
                        Some((part, file))
                    })
                    .collect::<Vec<_>>();
                set_files.sort_by_key(|(part, _)| *part);
                let set_files = set_files
                    .into_iter()
                    .map(|(_, file)| file)
                    .collect::<Vec<_>>();
                let (misnamed, problem) = volume_problems(&name, &set_files, &files, &assigned);
                ([set_files, misnamed].concat(), Some(problem))
            }
            (Err(_), None) => {
                let index = place(
                    &mut groups,
                    &mut index_by_key,
                    entry_scope_key(&entry.id),
                    |key| Group {
                        key,
                        relative_path: entry.relative_path.clone(),
                        kind: ScanFindingKind::Archive,
                        incomplete: Some(IncompleteArchiveSet::default()),
                        volume_count: 1,
                        primary: Some(entry.id.clone()),
                        files: Vec::new(),
                    },
                );
                assigned.insert(&entry.id);
                groups[index].files.push(entry);
                continue;
            }
        };
        let Some(primary) = set_files.first().copied() else {
            continue;
        };
        let index = place(
            &mut groups,
            &mut index_by_key,
            package_scope_key(&primary.id),
            |key| Group {
                key,
                relative_path: primary.relative_path.clone(),
                kind: ScanFindingKind::Archive,
                incomplete,
                volume_count: set_files.len(),
                primary: Some(primary.id.clone()),
                files: Vec::new(),
            },
        );
        for file in set_files {
            if assigned.insert(&file.id) {
                groups[index].files.push(file);
            }
        }
    }

    for entry in files {
        if assigned.contains(&entry.id) {
            continue;
        }
        let index = match entry.path_key.split_once('/') {
            Some((directory, _)) => place(
                &mut groups,
                &mut index_by_key,
                directory_scope_key(directory),
                |key| Group {
                    key,
                    relative_path: entry
                        .relative_path
                        .split_once('/')
                        .map_or(directory, |(name, _)| name)
                        .to_owned(),
                    kind: ScanFindingKind::Folder,
                    incomplete: None,
                    volume_count: 0,
                    primary: None,
                    files: Vec::new(),
                },
            ),
            None => place(
                &mut groups,
                &mut index_by_key,
                entry_scope_key(&entry.id),
                |key| Group {
                    key,
                    relative_path: entry.relative_path.clone(),
                    kind: ScanFindingKind::File,
                    incomplete: None,
                    volume_count: 0,
                    primary: None,
                    files: Vec::new(),
                },
            ),
        };
        groups[index].files.push(entry);
    }
    groups
}

/// The parts a multipart set lacks, and the files beside it that hold one of
/// those parts under a name 7-Zip cannot follow.
fn volume_problems<'a>(
    name: &MultipartVolumeName,
    set_files: &[&'a ScanEntry],
    files: &[&'a ScanEntry],
    assigned: &BTreeSet<&ScanEntryId>,
) -> (Vec<&'a ScanEntry>, IncompleteArchiveSet) {
    let present = set_files
        .iter()
        .filter_map(|file| MultipartVolumeName::parse(&file.relative_path))
        .map(|volume| volume.part)
        .collect::<BTreeSet<_>>();
    let mut misnamed = Vec::new();
    let mut renamed = BTreeSet::new();
    let mut renames = Vec::new();
    for file in files {
        if assigned.contains(&file.id) {
            continue;
        }
        let Some(part) = name.misnamed_part(&file.relative_path) else {
            continue;
        };
        if present.contains(&part) || !renamed.insert(part) {
            continue;
        }
        let from = file
            .relative_path
            .rsplit('/')
            .next()
            .unwrap_or(&file.relative_path);
        let extension = from
            .rsplit_once('.')
            .map_or("rar".to_owned(), |(_, extension)| {
                extension.to_ascii_lowercase()
            });
        renames.push(VolumeRename {
            from: from.to_owned(),
            to: name.volume_file_name(part, &extension),
        });
        misnamed.push(*file);
    }
    let last = present.iter().chain(&renamed).max().copied().unwrap_or(0);
    let missing_parts = (1..=last)
        .filter(|part| !present.contains(part) && !renamed.contains(part))
        .collect();
    (
        misnamed,
        IncompleteArchiveSet {
            missing_parts,
            renames,
        },
    )
}

fn place<'a>(
    groups: &mut Vec<Group<'a>>,
    index_by_key: &mut HashMap<String, usize>,
    key: String,
    make: impl FnOnce(String) -> Group<'a>,
) -> usize {
    if let Some(index) = index_by_key.get(&key) {
        return *index;
    }
    groups.push(make(key.clone()));
    index_by_key.insert(key, groups.len() - 1);
    groups.len() - 1
}

/// System and hidden files, such as `desktop.ini` or the launcher's own
/// `.dla-*` folders, are never works.
fn is_incidental(group: &Group<'_>) -> bool {
    if group.key != ROOT_FINDING_KEY && group.relative_path.starts_with('.') {
        return true;
    }
    group.files.iter().all(|entry| {
        let name = entry
            .path_key
            .rsplit('/')
            .next()
            .unwrap_or(&entry.path_key)
            .to_ascii_lowercase();
        name.starts_with('.') || matches!(name.as_str(), "desktop.ini" | "thumbs.db")
    })
}

/// The result an installation is created from: an archive set's first
/// volume, otherwise the most confident match, otherwise the first file.
fn representative<'a>(
    group: &Group<'_>,
    results: &[&'a ScanResult],
    result_by_entry: &HashMap<&ScanEntryId, &'a ScanResult>,
) -> Option<&'a ScanResult> {
    if let Some(primary) = &group.primary
        && let Some(result) = result_by_entry.get(primary)
    {
        return Some(result);
    }
    results
        .iter()
        .copied()
        .filter(|result| result.outcome == ScanMatchOutcome::Matched)
        .max_by(|left, right| left.confidence.cmp(&right.confidence))
        .or_else(|| results.first().copied())
}

fn raise(
    best: &mut BTreeMap<String, ScanMatchConfidence>,
    code: &str,
    confidence: ScanMatchConfidence,
) {
    let code = code.trim().to_ascii_uppercase();
    if code.is_empty() {
        return;
    }
    let current = best.entry(code).or_insert(confidence);
    if confidence > *current {
        *current = confidence;
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    pub(crate) fn finding(
        key: &str,
        relative_path: &str,
        kind: ScanFindingKind,
        work_code: Option<&str>,
    ) -> ScanFinding {
        ScanFinding {
            key: key.to_owned(),
            relative_path: relative_path.to_owned(),
            kind,
            state: if work_code.is_some() {
                ScanFindingState::Ready
            } else {
                ScanFindingState::Unknown
            },
            work_code: work_code.map(str::to_owned),
            candidates: Vec::new(),
            file_count: 1,
            size_bytes: 1,
            volume_count: 0,
            incomplete_set: None,
            in_library: None,
            ignored: false,
            result_id: ScanResultId("result-1".to_owned()),
            identity_detected: work_code.is_some(),
            files: vec![relative_path.to_owned()],
        }
    }

    pub(crate) fn set_files(finding: &mut ScanFinding, files: &[&str]) {
        finding.files = files.iter().map(|file| (*file).to_owned()).collect();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_domain::{
        CatalogWork,
        installation::{
            InstallationDetection, InstallationOverrides, InstallationPlatform, InstallationStatus,
            LaunchActionKind, LaunchTarget,
        },
        scanner::{
            ScanCounters, ScanEntryPresence, ScanMatchCandidate, ScanOptions, ScanRoot, ScanSession,
        },
    };

    use crate::{
        identity::{
            ArchiveHash, CatalogArchiveIdentity, CatalogIdentityError, CatalogIdentityReader,
        },
        installation_from_scan::InstallationScanSelection,
    };

    use super::*;

    const SESSION: &str = "session-1";

    struct MemorySource {
        snapshot: InstallationScanSnapshot,
    }

    impl InstallationScanSource for MemorySource {
        fn load(
            &self,
            _session_id: &ScanSessionId,
            selected_result_id: &ScanResultId,
        ) -> Result<Option<InstallationScanSelection>, InstallationScanSourceError> {
            let snapshot = &self.snapshot;
            Ok(snapshot
                .results
                .iter()
                .find(|result| &result.id == selected_result_id)
                .map(|selected| InstallationScanSelection {
                    root: snapshot.root.clone(),
                    session: snapshot.session.clone(),
                    selected_result: selected.clone(),
                    entries: snapshot.entries.clone(),
                    result_scopes: snapshot
                        .results
                        .iter()
                        .map(|result| InstallationScanResultScope {
                            candidate_entry_id: result
                                .candidate_entry_id
                                .clone()
                                .expect("fixture entry"),
                            matched_work_code: result.selected_work_code.clone(),
                        })
                        .collect(),
                }))
        }

        fn load_session(
            &self,
            _session_id: &ScanSessionId,
        ) -> Result<Option<InstallationScanSnapshot>, InstallationScanSourceError> {
            Ok(Some(self.snapshot.clone()))
        }
    }

    #[derive(Default)]
    struct MemoryIgnored {
        keys: Mutex<BTreeSet<String>>,
    }

    impl IgnoredScanFindingStore for MemoryIgnored {
        fn read_ignored_scan_findings(
            &self,
            _root_id: &ScanRootId,
        ) -> Result<BTreeSet<String>, InstallationScanSourceError> {
            Ok(self.keys.lock().expect("keys").clone())
        }

        fn set_scan_finding_ignored(
            &self,
            _root_id: &ScanRootId,
            key: &str,
            ignored: bool,
            _now: &str,
        ) -> Result<(), InstallationScanSourceError> {
            let mut keys = self.keys.lock().expect("keys");
            if ignored {
                keys.insert(key.to_owned());
            } else {
                keys.remove(key);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryStore {
        installations: Mutex<Vec<Installation>>,
    }

    impl InstallationStore for MemoryStore {
        fn create(&self, installation: &Installation) -> Result<(), InstallationLibraryError> {
            self.installations
                .lock()
                .expect("installations")
                .push(installation.clone());
            Ok(())
        }

        fn create_or_refresh(
            &self,
            installation: &Installation,
        ) -> Result<Installation, InstallationLibraryError> {
            let mut installations = self.installations.lock().expect("installations");
            if let Some(existing) = installations
                .iter_mut()
                .find(|existing| existing.id == installation.id)
            {
                existing.replace_detection(
                    installation.detection.clone(),
                    installation.updated_at.clone(),
                )?;
                return Ok(existing.clone());
            }
            installations.push(installation.clone());
            Ok(installation.clone())
        }

        fn read(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<Installation>, InstallationLibraryError> {
            Ok(self
                .installations
                .lock()
                .expect("installations")
                .iter()
                .find(|installation| &installation.id == installation_id)
                .cloned())
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(self.installations.lock().expect("installations").clone())
        }

        fn replace_detection(
            &self,
            _installation_id: &InstallationId,
            _detection: &InstallationDetection,
            _status: InstallationStatus,
            _updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }

        fn replace_overrides(
            &self,
            installation_id: &InstallationId,
            overrides: &InstallationOverrides,
            _status: InstallationStatus,
            updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            let mut installations = self.installations.lock().expect("installations");
            let installation = installations
                .iter_mut()
                .find(|installation| &installation.id == installation_id)
                .ok_or_else(|| InstallationLibraryError::NotFound(installation_id.0.clone()))?;
            installation.replace_overrides(overrides.clone(), updated_at.to_owned())?;
            Ok(())
        }
    }

    struct MemoryCatalog;

    impl CatalogIdentityReader for MemoryCatalog {
        fn read_works_by_codes(
            &self,
            work_codes: &[String],
        ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
            Ok(work_codes.iter().map(|code| work(code)).collect())
        }

        fn resolve_archive_hash(
            &self,
            _hash: &ArchiveHash,
        ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
            Ok(Vec::new())
        }

        fn find_archive_candidates_by_size(
            &self,
            _size: &str,
            _limit: usize,
        ) -> Result<Vec<CatalogArchiveIdentity>, CatalogIdentityError> {
            Ok(Vec::new())
        }
    }

    struct Fixture {
        service: ScanFindingService,
        store: Arc<MemoryStore>,
        ignored: Arc<MemoryIgnored>,
    }

    fn fixture(snapshot: InstallationScanSnapshot) -> Fixture {
        let source: Arc<dyn InstallationScanSource> = Arc::new(MemorySource { snapshot });
        let store = Arc::new(MemoryStore::default());
        let ignored = Arc::new(MemoryIgnored::default());
        let installations = Arc::new(InstallationFromScanService::new(
            Arc::clone(&source),
            store.clone(),
        ));
        let review = Arc::new(InstallationReviewService::new(
            Arc::new(MemoryCatalog),
            store.clone(),
        ));
        Fixture {
            service: ScanFindingService::new(
                source,
                ignored.clone(),
                store.clone(),
                installations,
                review,
            ),
            store,
            ignored,
        }
    }

    struct FileSpec {
        path: &'static str,
        outcome: Option<(&'static str, ScanMatchConfidence)>,
        candidates: &'static [&'static str],
    }

    fn file(path: &'static str) -> FileSpec {
        FileSpec {
            path,
            outcome: None,
            candidates: &[],
        }
    }

    fn matched(
        path: &'static str,
        code: &'static str,
        confidence: ScanMatchConfidence,
    ) -> FileSpec {
        FileSpec {
            path,
            outcome: Some((code, confidence)),
            candidates: &[],
        }
    }

    fn ambiguous(path: &'static str, candidates: &'static [&'static str]) -> FileSpec {
        FileSpec {
            path,
            outcome: None,
            candidates,
        }
    }

    fn snapshot(files: &[FileSpec]) -> InstallationScanSnapshot {
        let session_id = ScanSessionId(SESSION.to_owned());
        let root_id = ScanRootId("root-1".to_owned());
        let now = "2026-10-08T00:00:00Z".to_owned();
        let mut entries = Vec::new();
        let mut results = Vec::new();
        let mut directories = BTreeSet::new();
        for (index, spec) in files.iter().enumerate() {
            let entry_id = ScanEntryId(format!("entry-{index}"));
            if let Some((directory, _)) = spec.path.split_once('/') {
                directories.insert(directory);
            }
            entries.push(ScanEntry {
                id: entry_id.clone(),
                root_id: root_id.clone(),
                relative_path: spec.path.to_owned(),
                path_key: spec.path.to_lowercase(),
                kind: ScanEntryKind::File,
                extension: spec
                    .path
                    .rsplit_once('.')
                    .map_or("", |(_, extension)| extension)
                    .to_lowercase(),
                size: Some("100".to_owned()),
                modified_at: None,
                presence: ScanEntryPresence::Present,
                first_seen_session_id: Some(session_id.clone()),
                last_seen_session_id: Some(session_id.clone()),
                created_at: now.clone(),
                updated_at: now.clone(),
            });
            let mut candidates = spec
                .candidates
                .iter()
                .enumerate()
                .map(|(rank, code)| ScanMatchCandidate {
                    work_code: (*code).to_owned(),
                    confidence: ScanMatchConfidence::Strong,
                    reason_codes: vec!["product_code_directory".to_owned()],
                    rank: rank as u32 + 1,
                })
                .collect::<Vec<_>>();
            if let Some((code, confidence)) = spec.outcome {
                candidates.push(ScanMatchCandidate {
                    work_code: code.to_owned(),
                    confidence,
                    reason_codes: vec!["archive_sha256_match".to_owned()],
                    rank: 1,
                });
            }
            results.push(ScanResult {
                id: ScanResultId(format!("result-{index}")),
                session_id: session_id.clone(),
                candidate_entry_id: Some(entry_id),
                outcome: match (spec.outcome, spec.candidates.len()) {
                    (Some(_), _) => ScanMatchOutcome::Matched,
                    (None, 0) => ScanMatchOutcome::Unmatched,
                    (None, _) => ScanMatchOutcome::Ambiguous,
                },
                selected_work_code: spec.outcome.map(|(code, _)| code.to_owned()),
                confidence: spec.outcome.map(|(_, confidence)| confidence),
                candidates,
                evidence: Vec::new(),
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }
        for (index, directory) in directories.into_iter().enumerate() {
            entries.push(ScanEntry {
                id: ScanEntryId(format!("directory-{index}")),
                root_id: root_id.clone(),
                relative_path: directory.to_owned(),
                path_key: directory.to_lowercase(),
                kind: ScanEntryKind::Directory,
                extension: String::new(),
                size: None,
                modified_at: None,
                presence: ScanEntryPresence::Present,
                first_seen_session_id: Some(session_id.clone()),
                last_seen_session_id: Some(session_id.clone()),
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }
        InstallationScanSnapshot {
            root: ScanRoot {
                id: root_id.clone(),
                platform: "windows".to_owned(),
                path_key: "c:/fixtures/works".to_owned(),
                display_path: "C:\\fixtures\\works".to_owned(),
                created_at: now.clone(),
                updated_at: now.clone(),
            },
            session: ScanSession {
                id: session_id,
                root_id,
                status: ScanStatus::Completed,
                options: ScanOptions::default(),
                counters: ScanCounters::default(),
                started_at: now.clone(),
                finished_at: Some(now),
                fatal_error_code: None,
                fatal_error_message: None,
            },
            entries,
            results,
        }
    }

    fn collection() -> InstallationScanSnapshot {
        snapshot(&[
            matched("RJ01000001.zip", "RJ01000001", ScanMatchConfidence::Exact),
            file("KUNOICHI RED/Game.exe"),
            file("KUNOICHI RED/data/map.dat"),
            matched(
                "RJ01000002 voice/01.mp3",
                "RJ01000002",
                ScanMatchConfidence::Strong,
            ),
            matched(
                "RJ01000002 voice/02.mp3",
                "RJ01000002",
                ScanMatchConfidence::Strong,
            ),
            ambiguous("kanojo/kanojo.exe", &["RJ01000003", "RJ01000004"]),
            matched(
                "bundle/RJ01000005.txt",
                "RJ01000005",
                ScanMatchConfidence::Strong,
            ),
            matched(
                "bundle/RJ01000006.txt",
                "RJ01000006",
                ScanMatchConfidence::Strong,
            ),
            file("RJ01000007.part2.rar"),
            file("desktop.ini"),
            file(".dla-downloads/RJ01000008/RJ01000008.zip"),
        ])
    }

    fn finding<'a>(list: &'a ScanFindingList, path: &str) -> &'a ScanFinding {
        list.findings
            .iter()
            .find(|finding| finding.relative_path == path)
            .unwrap_or_else(|| panic!("no finding at {path}"))
    }

    fn session() -> ScanSessionId {
        ScanSessionId(SESSION.to_owned())
    }

    #[test]
    fn a_self_extracting_set_is_one_work() {
        let list = fixture(snapshot(&[
            matched("RJ01000001.zip", "RJ01000001", ScanMatchConfidence::Exact),
            matched(
                "RJ01000009.part3.rar",
                "RJ01000009",
                ScanMatchConfidence::Exact,
            ),
            matched(
                "RJ01000009.part1.exe",
                "RJ01000009",
                ScanMatchConfidence::Exact,
            ),
            matched(
                "RJ01000009.part2.rar",
                "RJ01000009",
                ScanMatchConfidence::Exact,
            ),
        ]))
        .service
        .list(&session())
        .expect("findings");

        assert_eq!(list.findings.len(), 2);
        let set = finding(&list, "RJ01000009.part1.exe");
        assert_eq!(set.state, ScanFindingState::Ready);
        assert_eq!(set.volume_count, 3);
        assert_eq!(set.incomplete_set, None);
    }

    #[test]
    fn a_misnamed_volume_joins_its_set_with_the_name_it_needs() {
        let list = fixture(snapshot(&[
            matched(
                "RJ01000009part2.rar",
                "RJ01000009",
                ScanMatchConfidence::Exact,
            ),
            matched("RJ01000001.zip", "RJ01000001", ScanMatchConfidence::Exact),
            matched(
                "RJ01000009.part1.exe",
                "RJ01000009",
                ScanMatchConfidence::Exact,
            ),
        ]))
        .service
        .list(&session())
        .expect("findings");

        assert_eq!(list.findings.len(), 2);
        let set = finding(&list, "RJ01000009.part1.exe");
        assert_eq!(set.state, ScanFindingState::Incomplete);
        assert_eq!(set.volume_count, 2);
        assert_eq!(
            set.incomplete_set,
            Some(IncompleteArchiveSet {
                missing_parts: Vec::new(),
                renames: vec![VolumeRename {
                    from: "RJ01000009part2.rar".to_owned(),
                    to: "RJ01000009.part2.rar".to_owned(),
                }],
            })
        );
    }

    #[test]
    fn a_set_with_a_gap_is_one_incomplete_work_naming_the_missing_part() {
        let list = fixture(snapshot(&[
            matched("RJ01000001.zip", "RJ01000001", ScanMatchConfidence::Exact),
            matched(
                "RJ01000009.part1.exe",
                "RJ01000009",
                ScanMatchConfidence::Exact,
            ),
            matched(
                "RJ01000009.part3.rar",
                "RJ01000009",
                ScanMatchConfidence::Exact,
            ),
        ]))
        .service
        .list(&session())
        .expect("findings");

        assert_eq!(list.findings.len(), 2);
        let set = finding(&list, "RJ01000009.part1.exe");
        assert_eq!(set.state, ScanFindingState::Incomplete);
        assert_eq!(set.volume_count, 2);
        assert_eq!(
            set.incomplete_set,
            Some(IncompleteArchiveSet {
                missing_parts: vec![2],
                renames: Vec::new(),
            })
        );
    }

    #[test]
    fn lists_one_finding_per_work_in_a_collection_root() {
        let list = fixture(collection())
            .service
            .list(&session())
            .expect("findings");

        let paths = list
            .findings
            .iter()
            .map(|finding| (finding.relative_path.as_str(), finding.kind, finding.state))
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                ("bundle", ScanFindingKind::Folder, ScanFindingState::Mixed),
                ("kanojo", ScanFindingKind::Folder, ScanFindingState::Choose),
                (
                    "KUNOICHI RED",
                    ScanFindingKind::Folder,
                    ScanFindingState::Unknown
                ),
                (
                    "RJ01000001.zip",
                    ScanFindingKind::Archive,
                    ScanFindingState::Ready
                ),
                (
                    "RJ01000002 voice",
                    ScanFindingKind::Folder,
                    ScanFindingState::Ready
                ),
                (
                    "RJ01000007.part2.rar",
                    ScanFindingKind::Archive,
                    ScanFindingState::Incomplete
                ),
            ]
        );
        let voice = finding(&list, "RJ01000002 voice");
        assert_eq!(voice.work_code.as_deref(), Some("RJ01000002"));
        assert_eq!(voice.file_count, 2);
        assert_eq!(voice.size_bytes, 200);
        let kanojo = finding(&list, "kanojo");
        assert_eq!(
            kanojo
                .candidates
                .iter()
                .map(|candidate| candidate.work_code.as_str())
                .collect::<Vec<_>>(),
            vec!["RJ01000003", "RJ01000004"]
        );
    }

    #[test]
    fn a_root_with_loose_program_files_is_one_work() {
        let list = fixture(snapshot(&[
            file("Game.exe"),
            file("data/map.dat"),
            file("save/slot1.sav"),
        ]))
        .service
        .list(&session())
        .expect("findings");

        assert_eq!(list.findings.len(), 1);
        let root = &list.findings[0];
        assert_eq!(root.key, ROOT_FINDING_KEY);
        assert_eq!(root.relative_path, "");
        assert_eq!(root.state, ScanFindingState::Unknown);
        assert_eq!(root.file_count, 3);
    }

    #[test]
    fn adding_a_folder_reviews_it_with_its_recommended_program() {
        let fixture = fixture(collection());
        let added = fixture
            .service
            .add(
                AddScanFindingsRequest {
                    session_id: session(),
                    findings: vec![AddScanFinding {
                        key: "directory:kunoichi red".to_owned(),
                        work_code: None,
                        unknown: true,
                    }],
                },
                "2026-10-08T01:00:00Z",
            )
            .expect("add");

        let [
            AddedScanFinding::Added {
                installation_id, ..
            },
        ] = added.as_slice()
        else {
            panic!("unexpected outcome: {added:?}");
        };
        let installation = fixture
            .store
            .read(installation_id)
            .expect("read")
            .expect("installation");
        assert_eq!(installation.status, InstallationStatus::Ready);
        assert_eq!(
            installation.overrides.catalog_identity,
            Some(ManualCatalogIdentity::Unidentified)
        );
        let preferred = installation
            .overrides
            .preferred_action
            .expect("recommended program");
        assert_eq!(preferred.action, LaunchActionKind::LaunchExecutable);
        assert!(matches!(
            preferred.target,
            LaunchTarget::RelativePath(path) if path.as_str() == "KUNOICHI RED/Game.exe"
        ));

        let listed = fixture.service.list(&session()).expect("findings");
        assert_eq!(
            finding(&listed, "KUNOICHI RED").in_library,
            Some(ScanFindingInLibrary {
                installation_id: installation.id.clone(),
                same_files: true,
                work_code: None,
            })
        );
    }

    #[test]
    fn a_finding_to_choose_needs_a_work_and_keeps_the_chosen_one() {
        let fixture = fixture(collection());
        let request = |work_code: Option<&str>| AddScanFindingsRequest {
            session_id: session(),
            findings: vec![AddScanFinding {
                key: "directory:kanojo".to_owned(),
                work_code: work_code.map(str::to_owned),
                unknown: false,
            }],
        };

        let refused = fixture
            .service
            .add(request(None), "2026-10-08T01:00:00Z")
            .expect("add");
        assert!(matches!(
            refused.as_slice(),
            [AddedScanFinding::Refused {
                reason: ScanFindingRefusal::ChooseWork,
                ..
            }]
        ));

        let added = fixture
            .service
            .add(request(Some("rj01000004")), "2026-10-08T01:00:00Z")
            .expect("add");
        let [
            AddedScanFinding::Added {
                installation_id, ..
            },
        ] = added.as_slice()
        else {
            panic!("unexpected outcome: {added:?}");
        };
        let installation = fixture
            .store
            .read(installation_id)
            .expect("read")
            .expect("installation");
        assert_eq!(
            installation.effective_catalog_work_code(),
            Some("RJ01000004")
        );
    }

    #[test]
    fn mixed_and_incomplete_findings_are_refused() {
        let fixture = fixture(collection());
        let added = fixture
            .service
            .add(
                AddScanFindingsRequest {
                    session_id: session(),
                    findings: vec![
                        AddScanFinding {
                            key: "directory:bundle".to_owned(),
                            work_code: None,
                            unknown: true,
                        },
                        AddScanFinding {
                            key: "entry:missing".to_owned(),
                            work_code: None,
                            unknown: false,
                        },
                    ],
                },
                "2026-10-08T01:00:00Z",
            )
            .expect("add");

        assert!(matches!(
            added.as_slice(),
            [
                AddedScanFinding::Refused {
                    reason: ScanFindingRefusal::Mixed,
                    ..
                },
                AddedScanFinding::Refused {
                    reason: ScanFindingRefusal::NotFound,
                    ..
                }
            ]
        ));
        assert!(fixture.store.list().expect("list").is_empty());
    }

    #[test]
    fn a_work_already_in_the_library_is_marked_as_another_copy() {
        let fixture = fixture(collection());
        fixture
            .store
            .create(&Installation {
                id: InstallationId("installation-elsewhere".to_owned()),
                scan_root_id: None,
                root_path: "/elsewhere".to_owned(),
                platform: InstallationPlatform::Linux,
                status: InstallationStatus::NeedsReview,
                detection: InstallationDetection {
                    source_scan_session_id: None,
                    catalog_identity: None,
                    suggested_status: InstallationStatus::NeedsReview,
                    content_items: Vec::new(),
                    launch_candidates: Vec::new(),
                    package_inspection: None,
                },
                overrides: InstallationOverrides {
                    catalog_identity: Some(ManualCatalogIdentity::CatalogWork {
                        work_code: "RJ01000002".to_owned(),
                    }),
                    reviewed_at: Some("2026-10-01T00:00:00Z".to_owned()),
                    ..InstallationOverrides::default()
                },
                discovered_at: "2026-10-01T00:00:00Z".to_owned(),
                updated_at: "2026-10-01T00:00:00Z".to_owned(),
            })
            .expect("existing");

        let list = fixture.service.list(&session()).expect("findings");
        assert_eq!(
            finding(&list, "RJ01000002 voice").in_library,
            Some(ScanFindingInLibrary {
                installation_id: InstallationId("installation-elsewhere".to_owned()),
                same_files: false,
                work_code: Some("RJ01000002".to_owned()),
            })
        );
    }

    #[test]
    fn ignoring_a_finding_is_remembered_for_its_root() {
        let fixture = fixture(collection());
        let root_id = ScanRootId("root-1".to_owned());
        fixture
            .service
            .set_ignored(&root_id, "directory:kunoichi red", true, "now")
            .expect("ignore");
        assert!(
            fixture
                .service
                .set_ignored(&root_id, "../etc", true, "now")
                .is_err()
        );

        let list = fixture.service.list(&session()).expect("findings");
        assert!(finding(&list, "KUNOICHI RED").ignored);
        assert!(!finding(&list, "kanojo").ignored);
        assert_eq!(fixture.ignored.keys.lock().expect("keys").len(), 1);
    }

    fn work(code: &str) -> CatalogWork {
        CatalogWork {
            code: code.to_ascii_uppercase(),
            source_code: code.to_ascii_uppercase(),
            title: "Fixture work".to_owned(),
            title_english: String::new(),
            added_date: "2026-08-07".to_owned(),
            release_date: "2026-08-07".to_owned(),
            updated_date: "2026-08-07".to_owned(),
            age_rating: "all_ages".to_owned(),
            release_type: "game".to_owned(),
            main_image_urls: Vec::new(),
            thumbnail_urls: Vec::new(),
            circles: Vec::new(),
            categories: Vec::new(),
            tags: Vec::new(),
            synthetic: true,
            latest_update: None,
        }
    }
}
