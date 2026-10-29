//! Adding works the user picks from the Library: folders and archives are
//! found with the scanner, reviewed as one list, and then brought into the
//! install folder, archives unpacked and folders moved, before they join the
//! Library.

use std::collections::BTreeSet;

use dla_domain::{
    installation::InstallationId, package::ArchiveRetentionPolicy, scanner::ScanSessionId,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    package_preparation::sanitize_directory_name,
    scan_findings::{ROOT_FINDING_KEY, ScanFinding, ScanFindingKind, ScanFindingState},
};

/// What the user asked to pick.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LibraryAddSourceRequest {
    /// Work folders, each one work or a folder of works.
    Folders,
    /// Archive files; each set of volumes is one work.
    Archives,
    /// One folder whose subfolders and archives are all looked at.
    Scan,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StartLibraryAddRequest {
    pub source: LibraryAddSourceRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryAddSourceKind {
    Folder,
    Archive,
    Scan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryAddPhase {
    Scanning,
    Review,
    Adding,
    Done,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryAddSourceStatus {
    Queued,
    Scanning,
    Done,
    Failed,
}

/// How a work arrives in the install folder.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryAddPlacement {
    /// Renamed into the install folder on the same drive.
    Move,
    /// Copied from another drive, checked, then removed from there.
    Copy,
    /// Already inside the install folder; added where it is.
    InPlace,
    /// An archive unpacked into the install folder.
    Unpack,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryAddJobKind {
    Move,
    Copy,
    Unpack,
    Add,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryAddJobState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAddSource {
    pub source_id: String,
    /// The picked folder's or file's own name, never its full path.
    pub display_name: String,
    pub kind: LibraryAddSourceKind,
    pub status: LibraryAddSourceStatus,
    pub error: Option<String>,
    pub session_id: Option<ScanSessionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAddFinding {
    pub source_id: String,
    pub finding: ScanFinding,
    pub placement: LibraryAddPlacement,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAddJob {
    pub source_id: String,
    pub key: String,
    pub kind: LibraryAddJobKind,
    pub state: LibraryAddJobState,
    pub installation_id: Option<InstallationId>,
    pub error: Option<String>,
    /// Set when the work was added but something after it went wrong, such
    /// as removing the original folder after a copy.
    pub warning: Option<String>,
    pub done_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAddView {
    pub batch_id: String,
    /// Counts changes, so the interface keeps the newest view when a reply and
    /// the change events arrive out of order.
    pub revision: u64,
    pub phase: LibraryAddPhase,
    pub install_folder: Option<String>,
    pub delete_archives: bool,
    pub sources: Vec<LibraryAddSource>,
    pub findings: Vec<LibraryAddFinding>,
    pub jobs: Vec<LibraryAddJob>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAddItem {
    pub source_id: String,
    pub key: String,
    #[serde(default)]
    pub work_code: Option<String>,
    #[serde(default)]
    pub unknown: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmLibraryAddRequest {
    pub batch_id: String,
    pub items: Vec<LibraryAddItem>,
    #[serde(default)]
    pub delete_archives: bool,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum LibraryAddError {
    #[error("choose an install folder first")]
    NoInstallFolder,
    #[error("works are already being added; finish or cancel that first")]
    AlreadyRunning,
    #[error("this list of works is no longer open")]
    UnknownBatch,
    #[error("the works can be added once every folder has been looked at")]
    NotReviewing,
    #[error("pick at least one work to add")]
    NothingChosen,
    #[error("adding is still running; cancel it first")]
    StillRunning,
}

/// The unpacked archives are kept unless the user asked to delete them.
pub fn archive_retention(delete_archives: bool) -> ArchiveRetentionPolicy {
    if delete_archives {
        ArchiveRetentionPolicy::DeleteAfterVerifiedInstall
    } else {
        ArchiveRetentionPolicy::Keep
    }
}

/// How a finding will arrive: archives are unpacked; anything else is added
/// where it is inside the install folder, or moved there, or copied when it
/// sits on another drive.
pub fn placement(
    kind: ScanFindingKind,
    inside_install_folder: bool,
    same_volume: bool,
) -> LibraryAddPlacement {
    if kind == ScanFindingKind::Archive {
        LibraryAddPlacement::Unpack
    } else if inside_install_folder {
        LibraryAddPlacement::InPlace
    } else if same_volume {
        LibraryAddPlacement::Move
    } else {
        LibraryAddPlacement::Copy
    }
}

/// The folder a moved or copied work gets in the install folder: the catalog
/// work the user chose or the scanner found, else the work's own folder or
/// file name. A work kept as unknown keeps its own name.
pub fn destination_name(
    finding: &ScanFinding,
    root_name: &str,
    chosen_code: Option<&str>,
    unknown: bool,
) -> String {
    let chosen = chosen_code.map(str::trim).filter(|code| !code.is_empty());
    let code = if unknown {
        None
    } else {
        chosen.or(finding.work_code.as_deref())
    };
    if let Some(code) = code {
        return sanitize_directory_name(&code.to_ascii_uppercase());
    }
    let own = if finding.key == ROOT_FINDING_KEY || finding.relative_path.is_empty() {
        root_name
    } else {
        finding
            .relative_path
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(root_name)
    };
    let own = if finding.kind == ScanFindingKind::File {
        file_stem(own)
    } else {
        own
    };
    sanitize_directory_name(own)
}

/// The archive findings a pick of archive files asked for: those holding a
/// picked file, named by its path below the scanned folder.
pub fn findings_for_picked_archives<'a>(
    findings: &'a [ScanFinding],
    picked: &[String],
) -> Vec<&'a ScanFinding> {
    let picked = picked
        .iter()
        .map(|name| name.to_lowercase())
        .collect::<BTreeSet<_>>();
    findings
        .iter()
        .filter(|finding| finding.kind == ScanFindingKind::Archive)
        .filter(|finding| {
            finding
                .files()
                .iter()
                .any(|file| picked.contains(&file.to_lowercase()))
        })
        .collect()
}

/// Why a finding is never added, in the words its job shows.
pub fn refusal_reason(finding: &ScanFinding) -> Option<&'static str> {
    if finding
        .in_library
        .as_ref()
        .is_some_and(|in_library| in_library.same_files)
    {
        return Some("this work is already in your Library");
    }
    match finding.state {
        ScanFindingState::Mixed => Some("this folder holds files of more than one work"),
        ScanFindingState::Incomplete => Some("a part of this archive set is missing"),
        _ => None,
    }
}

fn file_stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[..dot],
        _ => name,
    }
}

/// A job the batch asks its runner to do, with the user's choice for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedLibraryAddJob {
    pub index: usize,
    pub item: LibraryAddItem,
}

/// One "+ Add": the picked sources, what scanning them found, and the jobs
/// that bring the chosen works in. Each change bumps the revision.
#[derive(Clone, Debug)]
pub struct LibraryAddBatch {
    view: LibraryAddView,
    cancel_requested: bool,
}

impl LibraryAddBatch {
    pub fn new(
        batch_id: String,
        install_folder: Option<String>,
        sources: Vec<(String, LibraryAddSourceKind)>,
    ) -> Self {
        let sources = sources
            .into_iter()
            .enumerate()
            .map(|(index, (display_name, kind))| LibraryAddSource {
                source_id: format!("source-{}", index + 1),
                display_name,
                kind,
                status: LibraryAddSourceStatus::Queued,
                error: None,
                session_id: None,
            })
            .collect();
        Self {
            view: LibraryAddView {
                batch_id,
                revision: 1,
                phase: LibraryAddPhase::Scanning,
                install_folder,
                delete_archives: false,
                sources,
                findings: Vec::new(),
                jobs: Vec::new(),
            },
            cancel_requested: false,
        }
    }

    pub fn view(&self) -> &LibraryAddView {
        &self.view
    }

    pub fn batch_id(&self) -> &str {
        &self.view.batch_id
    }

    pub fn phase(&self) -> LibraryAddPhase {
        self.view.phase
    }

    pub fn cancel_requested(&self) -> bool {
        self.cancel_requested
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.view.phase,
            LibraryAddPhase::Done | LibraryAddPhase::Cancelled
        )
    }

    pub fn source_scanning(&mut self, source_id: &str) {
        if let Some(source) = self.source_mut(source_id) {
            source.status = LibraryAddSourceStatus::Scanning;
        }
        self.touch();
    }

    pub fn source_done(
        &mut self,
        source_id: &str,
        session_id: ScanSessionId,
        findings: Vec<LibraryAddFinding>,
    ) {
        if let Some(source) = self.source_mut(source_id) {
            source.status = LibraryAddSourceStatus::Done;
            source.session_id = Some(session_id);
        }
        self.view.findings.extend(findings);
        self.settle_scanning();
        self.touch();
    }

    pub fn source_failed(&mut self, source_id: &str, error: String) {
        if let Some(source) = self.source_mut(source_id) {
            source.status = LibraryAddSourceStatus::Failed;
            source.error = Some(error);
        }
        self.settle_scanning();
        self.touch();
    }

    /// Turns the user's choices into jobs, in the order of the review list.
    /// A work that can never be added gets a failed job saying why.
    pub fn confirm(
        &mut self,
        items: Vec<LibraryAddItem>,
        delete_archives: bool,
    ) -> Result<Vec<PlannedLibraryAddJob>, LibraryAddError> {
        if self.view.phase != LibraryAddPhase::Review {
            return Err(LibraryAddError::NotReviewing);
        }
        let mut planned = Vec::new();
        let mut jobs = Vec::new();
        let mut seen = BTreeSet::new();
        for item in items {
            if !seen.insert((item.source_id.clone(), item.key.clone())) {
                continue;
            }
            let Some(found) =
                self.view.findings.iter().find(|found| {
                    found.source_id == item.source_id && found.finding.key == item.key
                })
            else {
                continue;
            };
            let kind = match found.placement {
                LibraryAddPlacement::Move => LibraryAddJobKind::Move,
                LibraryAddPlacement::Copy => LibraryAddJobKind::Copy,
                LibraryAddPlacement::Unpack => LibraryAddJobKind::Unpack,
                LibraryAddPlacement::InPlace => LibraryAddJobKind::Add,
            };
            let refusal = refusal_reason(&found.finding);
            jobs.push(LibraryAddJob {
                source_id: item.source_id.clone(),
                key: item.key.clone(),
                kind,
                state: if refusal.is_some() {
                    LibraryAddJobState::Failed
                } else {
                    LibraryAddJobState::Queued
                },
                installation_id: None,
                error: refusal.map(str::to_owned),
                warning: None,
                done_bytes: None,
                total_bytes: None,
            });
            if refusal.is_none() {
                planned.push(PlannedLibraryAddJob {
                    index: jobs.len() - 1,
                    item,
                });
            }
        }
        if jobs.is_empty() {
            return Err(LibraryAddError::NothingChosen);
        }
        self.view.jobs = jobs;
        self.view.delete_archives = delete_archives;
        self.view.phase = LibraryAddPhase::Adding;
        self.settle_adding();
        self.touch();
        Ok(planned)
    }

    pub fn job_running(&mut self, index: usize) {
        if let Some(job) = self.view.jobs.get_mut(index) {
            job.state = LibraryAddJobState::Running;
        }
        self.touch();
    }

    pub fn job_progress(&mut self, index: usize, done_bytes: u64, total_bytes: u64) {
        if let Some(job) = self.view.jobs.get_mut(index) {
            job.done_bytes = Some(done_bytes);
            job.total_bytes = Some(total_bytes);
        }
        self.touch();
    }

    pub fn job_installation(&mut self, index: usize, installation_id: InstallationId) {
        if let Some(job) = self.view.jobs.get_mut(index) {
            job.installation_id = Some(installation_id);
        }
        self.touch();
    }

    pub fn job_done(
        &mut self,
        index: usize,
        installation_id: Option<InstallationId>,
        warning: Option<String>,
    ) {
        if let Some(job) = self.view.jobs.get_mut(index) {
            job.state = LibraryAddJobState::Done;
            if installation_id.is_some() {
                job.installation_id = installation_id;
            }
            job.warning = warning;
        }
        self.settle_adding();
        self.touch();
    }

    pub fn job_failed(&mut self, index: usize, error: String) {
        if let Some(job) = self.view.jobs.get_mut(index) {
            job.state = LibraryAddJobState::Failed;
            job.error = Some(error);
        }
        self.settle_adding();
        self.touch();
    }

    pub fn job_cancelled(&mut self, index: usize) {
        if let Some(job) = self.view.jobs.get_mut(index) {
            job.state = LibraryAddJobState::Cancelled;
        }
        self.settle_adding();
        self.touch();
    }

    /// Stops the batch. Scanning and review end at once; while adding, the
    /// queued jobs are cancelled and the running one is left to its runner,
    /// which reports how it ended.
    pub fn cancel(&mut self) {
        self.cancel_requested = true;
        match self.view.phase {
            LibraryAddPhase::Scanning | LibraryAddPhase::Review => {
                for source in &mut self.view.sources {
                    if matches!(
                        source.status,
                        LibraryAddSourceStatus::Queued | LibraryAddSourceStatus::Scanning
                    ) {
                        source.status = LibraryAddSourceStatus::Failed;
                        source.error = Some("cancelled".to_owned());
                    }
                }
                self.view.phase = LibraryAddPhase::Cancelled;
            }
            LibraryAddPhase::Adding => {
                for job in &mut self.view.jobs {
                    if job.state == LibraryAddJobState::Queued {
                        job.state = LibraryAddJobState::Cancelled;
                    }
                }
                self.settle_adding();
            }
            LibraryAddPhase::Done | LibraryAddPhase::Cancelled => {}
        }
        self.touch();
    }

    pub fn job_is_queued(&self, index: usize) -> bool {
        self.view
            .jobs
            .get(index)
            .is_some_and(|job| job.state == LibraryAddJobState::Queued)
    }

    fn settle_scanning(&mut self) {
        if self.view.phase != LibraryAddPhase::Scanning {
            return;
        }
        let scanned = self.view.sources.iter().all(|source| {
            matches!(
                source.status,
                LibraryAddSourceStatus::Done | LibraryAddSourceStatus::Failed
            )
        });
        if scanned {
            self.view.phase = LibraryAddPhase::Review;
        }
    }

    fn settle_adding(&mut self) {
        if self.view.phase != LibraryAddPhase::Adding {
            return;
        }
        let settled = self.view.jobs.iter().all(|job| {
            !matches!(
                job.state,
                LibraryAddJobState::Queued | LibraryAddJobState::Running
            )
        });
        if settled {
            self.view.phase = if self.cancel_requested {
                LibraryAddPhase::Cancelled
            } else {
                LibraryAddPhase::Done
            };
        }
    }

    fn source_mut(&mut self, source_id: &str) -> Option<&mut LibraryAddSource> {
        self.view
            .sources
            .iter_mut()
            .find(|source| source.source_id == source_id)
    }

    fn touch(&mut self) {
        self.view.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan_findings::tests_support::finding;

    fn batch() -> LibraryAddBatch {
        LibraryAddBatch::new(
            "batch-1".to_owned(),
            Some("/library".to_owned()),
            vec![
                ("Set A".to_owned(), LibraryAddSourceKind::Folder),
                ("Set B".to_owned(), LibraryAddSourceKind::Folder),
            ],
        )
    }

    fn found(source_id: &str, key: &str, placement: LibraryAddPlacement) -> LibraryAddFinding {
        LibraryAddFinding {
            source_id: source_id.to_owned(),
            finding: finding(key, key, ScanFindingKind::Folder, Some("RJ01000001")),
            placement,
        }
    }

    fn item(source_id: &str, key: &str) -> LibraryAddItem {
        LibraryAddItem {
            source_id: source_id.to_owned(),
            key: key.to_owned(),
            work_code: None,
            unknown: false,
        }
    }

    #[test]
    fn a_batch_reviews_once_every_source_is_looked_at_and_finishes_with_its_jobs() {
        let mut batch = batch();
        assert_eq!(batch.phase(), LibraryAddPhase::Scanning);
        batch.source_scanning("source-1");
        batch.source_done(
            "source-1",
            ScanSessionId("session-1".to_owned()),
            vec![found("source-1", "root", LibraryAddPlacement::Move)],
        );
        assert_eq!(batch.phase(), LibraryAddPhase::Scanning);
        batch.source_failed("source-2", "the folder could not be read".to_owned());
        assert_eq!(batch.phase(), LibraryAddPhase::Review);
        let revision = batch.view().revision;

        let planned = batch
            .confirm(vec![item("source-1", "root")], true)
            .expect("confirm");
        assert_eq!(planned.len(), 1);
        assert_eq!(batch.phase(), LibraryAddPhase::Adding);
        assert!(batch.view().delete_archives);
        assert!(batch.view().revision > revision);
        assert_eq!(batch.view().jobs[0].kind, LibraryAddJobKind::Move);

        batch.job_running(0);
        batch.job_progress(0, 5, 10);
        assert_eq!(batch.view().jobs[0].done_bytes, Some(5));
        batch.job_done(0, Some(InstallationId("installation-1".to_owned())), None);
        assert_eq!(batch.phase(), LibraryAddPhase::Done);
        assert!(batch.is_finished());
    }

    #[test]
    fn works_that_can_never_be_added_fail_in_the_job_list_and_are_not_planned() {
        let mut batch = batch();
        let mut duplicate = found("source-1", "directory:a", LibraryAddPlacement::Move);
        duplicate.finding.in_library = Some(crate::scan_findings::ScanFindingInLibrary {
            installation_id: InstallationId("installation-9".to_owned()),
            same_files: true,
            work_code: None,
        });
        let mut mixed = found("source-1", "directory:b", LibraryAddPlacement::Move);
        mixed.finding.state = ScanFindingState::Mixed;
        batch.source_done(
            "source-1",
            ScanSessionId("session-1".to_owned()),
            vec![duplicate, mixed],
        );
        batch.source_done("source-2", ScanSessionId("session-2".to_owned()), vec![]);

        let planned = batch
            .confirm(
                vec![
                    item("source-1", "directory:a"),
                    item("source-1", "directory:b"),
                ],
                false,
            )
            .expect("confirm");
        assert!(planned.is_empty());
        assert_eq!(batch.phase(), LibraryAddPhase::Done);
        assert_eq!(
            batch.view().jobs[0].error.as_deref(),
            Some("this work is already in your Library")
        );
        assert_eq!(batch.view().jobs[1].state, LibraryAddJobState::Failed);
    }

    #[test]
    fn confirming_needs_a_finished_review_and_at_least_one_known_work() {
        let mut batch = batch();
        assert_eq!(
            batch.confirm(vec![item("source-1", "root")], false),
            Err(LibraryAddError::NotReviewing)
        );
        batch.source_done("source-1", ScanSessionId("session-1".to_owned()), vec![]);
        batch.source_done("source-2", ScanSessionId("session-2".to_owned()), vec![]);
        assert_eq!(
            batch.confirm(vec![item("source-1", "missing")], false),
            Err(LibraryAddError::NothingChosen)
        );
    }

    #[test]
    fn cancelling_while_adding_stops_queued_jobs_and_ends_once_the_running_one_stops() {
        let mut batch = batch();
        batch.source_done(
            "source-1",
            ScanSessionId("session-1".to_owned()),
            vec![
                found("source-1", "directory:a", LibraryAddPlacement::Copy),
                found("source-1", "directory:b", LibraryAddPlacement::Unpack),
            ],
        );
        batch.source_done("source-2", ScanSessionId("session-2".to_owned()), vec![]);
        batch
            .confirm(
                vec![
                    item("source-1", "directory:a"),
                    item("source-1", "directory:b"),
                ],
                false,
            )
            .expect("confirm");
        batch.job_running(0);
        batch.cancel();
        assert!(batch.cancel_requested());
        assert_eq!(batch.phase(), LibraryAddPhase::Adding);
        assert!(!batch.job_is_queued(1));
        assert_eq!(batch.view().jobs[1].state, LibraryAddJobState::Cancelled);

        batch.job_cancelled(0);
        assert_eq!(batch.phase(), LibraryAddPhase::Cancelled);
    }

    #[test]
    fn cancelling_a_scan_ends_the_batch_at_once() {
        let mut batch = batch();
        batch.source_scanning("source-1");
        batch.cancel();
        assert_eq!(batch.phase(), LibraryAddPhase::Cancelled);
        assert_eq!(
            batch.view().sources[0].status,
            LibraryAddSourceStatus::Failed
        );
    }

    #[test]
    fn archives_unpack_and_folders_move_copy_or_stay_in_place() {
        assert_eq!(
            placement(ScanFindingKind::Archive, true, true),
            LibraryAddPlacement::Unpack
        );
        assert_eq!(
            placement(ScanFindingKind::Folder, true, false),
            LibraryAddPlacement::InPlace
        );
        assert_eq!(
            placement(ScanFindingKind::Folder, false, true),
            LibraryAddPlacement::Move
        );
        assert_eq!(
            placement(ScanFindingKind::File, false, false),
            LibraryAddPlacement::Copy
        );
    }

    #[test]
    fn moved_works_are_named_by_their_catalog_code_or_their_own_name() {
        let folder = finding(
            "directory:My Set",
            "My Set",
            ScanFindingKind::Folder,
            Some("rj01000001"),
        );
        assert_eq!(
            destination_name(&folder, "Downloads", None, false),
            "RJ01000001"
        );
        assert_eq!(
            destination_name(&folder, "Downloads", Some("RJ01000002"), false),
            "RJ01000002"
        );
        assert_eq!(destination_name(&folder, "Downloads", None, true), "My Set");

        let root = finding("root", "", ScanFindingKind::Folder, None);
        assert_eq!(
            destination_name(&root, "Picked: Work?", None, false),
            "Picked_ Work_"
        );

        let file = finding("entry:1", "extras/movie.mp4", ScanFindingKind::File, None);
        assert_eq!(destination_name(&file, "Downloads", None, false), "movie");
    }

    #[test]
    fn a_pick_of_archives_keeps_only_the_sets_holding_a_picked_file() {
        let mut set = finding(
            "package:entry:1",
            "Work.part1.rar",
            ScanFindingKind::Archive,
            None,
        );
        crate::scan_findings::tests_support::set_files(
            &mut set,
            &["Work.part1.rar", "Work.part2.rar"],
        );
        let mut other = finding(
            "package:entry:3",
            "Other.zip",
            ScanFindingKind::Archive,
            None,
        );
        crate::scan_findings::tests_support::set_files(&mut other, &["Other.zip"]);
        let mut folder = finding(
            "directory:Work.part2.rar",
            "x",
            ScanFindingKind::Folder,
            None,
        );
        crate::scan_findings::tests_support::set_files(&mut folder, &["Work.part2.rar"]);
        let findings = vec![set, other, folder];

        let kept = findings_for_picked_archives(&findings, &["work.PART2.rar".to_owned()]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].key, "package:entry:1");
    }

    #[test]
    fn archives_are_kept_unless_the_user_asks_to_delete_them() {
        assert_eq!(archive_retention(false), ArchiveRetentionPolicy::Keep);
        assert_eq!(
            archive_retention(true),
            ArchiveRetentionPolicy::DeleteAfterVerifiedInstall
        );
    }
}
