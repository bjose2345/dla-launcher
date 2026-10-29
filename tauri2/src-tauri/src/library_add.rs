use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use dla_application::{
    delivery::InstallFolderStore,
    installation::{InstallationLibrary, InstallationStore},
    library_add::{
        ConfirmLibraryAddRequest, LibraryAddBatch, LibraryAddError, LibraryAddFinding,
        LibraryAddItem, LibraryAddJobKind, LibraryAddSourceKind, LibraryAddView,
        PlannedLibraryAddJob, archive_retention, destination_name, findings_for_picked_archives,
        placement,
    },
    scan_findings::{
        AddScanFinding, AddScanFindingsRequest, AddedScanFinding, ROOT_FINDING_KEY, ScanFinding,
        ScanFindingList, ScanFindingRefusal, ScanFindingService, identity_for_add,
    },
    scanner::ScanCancellation,
};
use dla_archive::{FolderTransfer, FolderTransferError, TransferMode};
use dla_domain::{
    installation::{InstallationId, ManualCatalogIdentity},
    scanner::ScanSessionId,
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use crate::{
    scan_installs::{ScanInstall, ScanInstallController, ScanInstallStatus},
    scanner::ScanController,
};

pub const LIBRARY_ADD_CHANGED_EVENT: &str = "library-add-changed";

const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);
const INSTALL_POLL: Duration = Duration::from_millis(300);

/// What the user picked, before anything is scanned.
pub enum LibraryAddPick {
    Folders(Vec<PathBuf>),
    Archives(Vec<PathBuf>),
    Scan(PathBuf),
}

enum SourcePlan {
    Folder(PathBuf),
    Archive { parent: PathBuf, file_name: String },
    Scan(PathBuf),
}

impl SourcePlan {
    fn scanned_folder(&self) -> &Path {
        match self {
            Self::Folder(path) | Self::Scan(path) => path,
            Self::Archive { parent, .. } => parent,
        }
    }
}

struct ScannedSource {
    session_id: ScanSessionId,
    root_path: PathBuf,
}

struct Batch {
    state: LibraryAddBatch,
    cancel: Arc<AtomicBool>,
    plans: Vec<(String, SourcePlan)>,
    scanned: HashMap<String, ScannedSource>,
    install_folder: PathBuf,
}

/// "+ Add" in the Library: scans what the user picked without it showing on
/// the Scanner page, keeps the review list, and brings the chosen works into
/// the install folder one at a time, archives unpacked and folders moved.
pub struct LibraryAddController {
    scanner: Arc<ScanController>,
    findings: Arc<ScanFindingService>,
    installs: Arc<ScanInstallController>,
    library: InstallationLibrary,
    folders: Arc<dyn InstallFolderStore>,
    transfer: FolderTransfer,
    app: AppHandle,
    batch: Mutex<Option<Batch>>,
}

fn platform() -> &'static str {
    std::env::consts::OS
}

impl LibraryAddController {
    pub fn new(
        scanner: Arc<ScanController>,
        findings: Arc<ScanFindingService>,
        installs: Arc<ScanInstallController>,
        installations: Arc<dyn InstallationStore>,
        folders: Arc<dyn InstallFolderStore>,
        app: AppHandle,
    ) -> Self {
        Self {
            scanner,
            findings,
            installs,
            library: InstallationLibrary::new(installations),
            folders,
            transfer: FolderTransfer::new(),
            app,
            batch: Mutex::new(None),
        }
    }

    pub fn view(&self) -> Option<LibraryAddView> {
        self.lock().as_ref().map(|batch| batch.state.view().clone())
    }

    /// Fails before any picker opens when adding cannot start.
    pub fn check_can_start(&self) -> Result<PathBuf, String> {
        if self
            .lock()
            .as_ref()
            .is_some_and(|batch| !batch.state.is_finished())
        {
            return Err(LibraryAddError::AlreadyRunning.to_string());
        }
        self.install_folder()
    }

    pub fn start(self: &Arc<Self>, pick: LibraryAddPick) -> Result<LibraryAddView, String> {
        let install_folder = self.check_can_start()?;
        let plans = match pick {
            LibraryAddPick::Folders(paths) => paths
                .into_iter()
                .map(|path| (display_name(&path), SourcePlan::Folder(path)))
                .map(|(name, plan)| (name, LibraryAddSourceKind::Folder, plan))
                .collect::<Vec<_>>(),
            LibraryAddPick::Archives(paths) => paths
                .into_iter()
                .filter_map(|path| {
                    let parent = path.parent()?.to_path_buf();
                    let file_name = path.file_name()?.to_string_lossy().into_owned();
                    Some((
                        file_name.clone(),
                        LibraryAddSourceKind::Archive,
                        SourcePlan::Archive { parent, file_name },
                    ))
                })
                .collect(),
            LibraryAddPick::Scan(path) => {
                vec![(
                    display_name(&path),
                    LibraryAddSourceKind::Scan,
                    SourcePlan::Scan(path),
                )]
            }
        };
        let state = LibraryAddBatch::new(
            Uuid::new_v4().to_string(),
            Some(install_folder.display().to_string()),
            plans
                .iter()
                .map(|(name, kind, _)| (name.clone(), *kind))
                .collect(),
        );
        let source_ids = state
            .view()
            .sources
            .iter()
            .map(|source| source.source_id.clone())
            .collect::<Vec<_>>();
        let batch = Batch {
            cancel: Arc::new(AtomicBool::new(false)),
            plans: source_ids
                .into_iter()
                .zip(plans)
                .map(|(source_id, (_, _, plan))| (source_id, plan))
                .collect(),
            scanned: HashMap::new(),
            install_folder,
            state,
        };
        let view = batch.state.view().clone();
        *self.lock() = Some(batch);
        self.emit();
        let controller = Arc::clone(self);
        std::thread::spawn(move || controller.scan_sources());
        Ok(view)
    }

    pub fn confirm(
        self: &Arc<Self>,
        request: ConfirmLibraryAddRequest,
    ) -> Result<LibraryAddView, String> {
        let planned = {
            let mut guard = self.lock();
            let batch = guard
                .as_mut()
                .filter(|batch| batch.state.batch_id() == request.batch_id)
                .ok_or_else(|| LibraryAddError::UnknownBatch.to_string())?;
            batch
                .state
                .confirm(request.items, request.delete_archives)
                .map_err(|error| error.to_string())?
        };
        self.emit();
        let controller = Arc::clone(self);
        let delete_archives = request.delete_archives;
        std::thread::spawn(move || controller.run_jobs(planned, delete_archives));
        self.view()
            .ok_or_else(|| LibraryAddError::UnknownBatch.to_string())
    }

    pub fn cancel(&self, batch_id: &str) -> Result<LibraryAddView, String> {
        {
            let mut guard = self.lock();
            let batch = guard
                .as_mut()
                .filter(|batch| batch.state.batch_id() == batch_id)
                .ok_or_else(|| LibraryAddError::UnknownBatch.to_string())?;
            batch.cancel.store(true, Ordering::SeqCst);
            batch.state.cancel();
        }
        self.emit();
        self.view()
            .ok_or_else(|| LibraryAddError::UnknownBatch.to_string())
    }

    pub fn dismiss(&self, batch_id: &str) -> Result<(), String> {
        let mut guard = self.lock();
        match guard.as_ref() {
            Some(batch) if batch.state.batch_id() == batch_id => {
                if !batch.state.is_finished() {
                    return Err(LibraryAddError::StillRunning.to_string());
                }
                *guard = None;
                Ok(())
            }
            _ => Err(LibraryAddError::UnknownBatch.to_string()),
        }
    }

    fn install_folder(&self) -> Result<PathBuf, String> {
        match self.folders.read_install_folder(platform()) {
            Ok(Some(folder)) => Ok(PathBuf::from(folder)),
            Ok(None) => Err(LibraryAddError::NoInstallFolder.to_string()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn scan_sources(&self) {
        let Some((cancel, plans)) = self.with_batch(|batch| {
            (
                Arc::clone(&batch.cancel),
                batch
                    .plans
                    .iter()
                    .map(|(source_id, plan)| (source_id.clone(), clone_plan(plan)))
                    .collect::<Vec<_>>(),
            )
        }) else {
            return;
        };
        let install_folder = self.with_batch(|batch| batch.install_folder.clone());
        let Some(install_folder) = install_folder else {
            return;
        };
        let mut done = vec![false; plans.len()];
        for index in 0..plans.len() {
            if done[index] {
                continue;
            }
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            let folder = plans[index].1.scanned_folder().to_path_buf();
            // Archives picked from one folder share one scan of it.
            let group = (index..plans.len())
                .filter(|other| !done[*other] && plans[*other].1.scanned_folder() == folder)
                .collect::<Vec<_>>();
            self.update(|batch| {
                for member in &group {
                    batch.state.source_scanning(&plans[*member].0);
                }
            });
            let scanned = self
                .scanner
                .scan_folder_now(&folder, &FlagCancellation(&cancel))
                .map_err(|error| error.to_string())
                .and_then(|view| {
                    self.findings
                        .list(&view.session.id)
                        .map_err(|error| error.to_string())
                });
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            let mut claimed = std::collections::BTreeSet::new();
            for member in &group {
                done[*member] = true;
                let (source_id, plan) = &plans[*member];
                match &scanned {
                    Ok(list) => {
                        let chosen = match plan {
                            SourcePlan::Archive { file_name, .. } => findings_for_picked_archives(
                                &list.findings,
                                std::slice::from_ref(file_name),
                            )
                            .into_iter()
                            .filter(|finding| claimed.insert(finding.key.clone()))
                            .cloned()
                            .collect::<Vec<_>>(),
                            SourcePlan::Folder(_) | SourcePlan::Scan(_) => list.findings.clone(),
                        };
                        let root_path = PathBuf::from(&list.root_path);
                        let found = chosen
                            .into_iter()
                            .map(|finding| LibraryAddFinding {
                                source_id: source_id.clone(),
                                placement: self.placement_of(&finding, &root_path, &install_folder),
                                finding,
                            })
                            .collect::<Vec<_>>();
                        let session_id = list.session_id.clone();
                        self.update(|batch| {
                            batch.scanned.insert(
                                source_id.clone(),
                                ScannedSource {
                                    session_id: session_id.clone(),
                                    root_path: root_path.clone(),
                                },
                            );
                            batch.state.source_done(source_id, session_id, found);
                        });
                    }
                    Err(error) => {
                        let error = error.clone();
                        self.update(|batch| batch.state.source_failed(source_id, error));
                    }
                }
            }
        }
    }

    fn placement_of(
        &self,
        finding: &ScanFinding,
        root_path: &Path,
        install_folder: &Path,
    ) -> dla_application::library_add::LibraryAddPlacement {
        let source = finding_path(finding, root_path);
        let inside = match (source.canonicalize(), install_folder.canonicalize()) {
            (Ok(source), Ok(folder)) => source.starts_with(folder),
            _ => false,
        };
        let same_volume = self
            .transfer
            .mode(&source, install_folder)
            .is_ok_and(|mode| mode == TransferMode::Rename);
        placement(finding.kind, inside, same_volume)
    }

    fn run_jobs(&self, planned: Vec<PlannedLibraryAddJob>, delete_archives: bool) {
        for job in planned {
            let Some((cancel, queued, kind)) = self.with_batch(|batch| {
                (
                    Arc::clone(&batch.cancel),
                    batch.state.job_is_queued(job.index),
                    batch.state.view().jobs.get(job.index).map(|job| job.kind),
                )
            }) else {
                return;
            };
            if !queued || cancel.load(Ordering::SeqCst) {
                continue;
            }
            self.update(|batch| batch.state.job_running(job.index));
            let outcome = match kind {
                Some(LibraryAddJobKind::Add) => self.add_in_place(&job),
                Some(LibraryAddJobKind::Unpack) => self.unpack(&job, delete_archives, &cancel),
                Some(LibraryAddJobKind::Move | LibraryAddJobKind::Copy) => {
                    self.move_in(&job, &cancel)
                }
                None => continue,
            };
            self.update(|batch| match outcome {
                JobOutcome::Done {
                    installation_id,
                    warning,
                } => batch.state.job_done(job.index, installation_id, warning),
                JobOutcome::Failed(error) => batch.state.job_failed(job.index, error),
                JobOutcome::Cancelled => batch.state.job_cancelled(job.index),
            });
        }
    }

    fn add_in_place(&self, job: &PlannedLibraryAddJob) -> JobOutcome {
        let Some(scanned) = self.scanned(&job.item.source_id) else {
            return JobOutcome::Failed("the scan of this work is gone".to_owned());
        };
        self.add_from_session(&scanned.session_id, add_finding(&job.item))
    }

    fn unpack(
        &self,
        job: &PlannedLibraryAddJob,
        delete_archives: bool,
        cancel: &AtomicBool,
    ) -> JobOutcome {
        let Some(scanned) = self.scanned(&job.item.source_id) else {
            return JobOutcome::Failed("the scan of this work is gone".to_owned());
        };
        let added = match self.findings.add(
            AddScanFindingsRequest {
                session_id: scanned.session_id,
                findings: vec![add_finding(&job.item)],
            },
            &dla_sqlite::current_timestamp(),
        ) {
            Ok(mut added) if !added.is_empty() => added.remove(0),
            Ok(_) => return JobOutcome::Failed("the scan no longer lists this work".to_owned()),
            Err(error) => return JobOutcome::Failed(error.to_string()),
        };
        let (installation_id, identity, name, work_code) = match added {
            AddedScanFinding::NeedsInstall {
                installation_id,
                identity,
                name,
                work_code,
                ..
            } => (installation_id, identity, name, work_code),
            AddedScanFinding::Added {
                installation_id, ..
            }
            | AddedScanFinding::AlreadyInLibrary {
                installation_id, ..
            } => {
                return JobOutcome::Done {
                    installation_id: Some(installation_id),
                    warning: None,
                };
            }
            AddedScanFinding::Refused { reason, detail, .. } => {
                return JobOutcome::Failed(refusal_text(reason, &detail));
            }
        };
        // The install folder is named from the saved identity, so a work the
        // user picked is unpacked under its own code, not the archive's name.
        if let Some(chosen @ ManualCatalogIdentity::CatalogWork { .. }) = &identity
            && let Err(error) = self.preset_identity(&installation_id, chosen.clone())
        {
            log::warn!(
                target: "dla::library_add",
                "event=identity_preset_failed installation_id={} error={error}",
                installation_id.0
            );
        }
        self.update(|batch| {
            batch
                .state
                .job_installation(job.index, installation_id.clone())
        });
        self.installs.queue(
            ScanInstall {
                installation_id: installation_id.clone(),
                key: job.item.key.clone(),
                name,
                work_code,
                status: ScanInstallStatus::Queued,
                detail: None,
            },
            identity,
            archive_retention(delete_archives),
        );
        let mut cancel_sent = false;
        loop {
            if cancel.load(Ordering::SeqCst) && !cancel_sent {
                cancel_sent = true;
                self.installs.cancel(&installation_id);
            }
            match self.installs.status(&installation_id) {
                Some((ScanInstallStatus::Installed | ScanInstallStatus::NeedsReview, _)) => {
                    return JobOutcome::Done {
                        installation_id: Some(installation_id),
                        warning: None,
                    };
                }
                Some((ScanInstallStatus::Failed, detail)) => {
                    return JobOutcome::Failed(
                        detail.unwrap_or_else(|| "the archive could not be unpacked".to_owned()),
                    );
                }
                Some((ScanInstallStatus::Cancelled, _)) => return JobOutcome::Cancelled,
                Some((ScanInstallStatus::Queued | ScanInstallStatus::Installing, _)) => {}
                None => return JobOutcome::Failed("the archive install was lost".to_owned()),
            }
            std::thread::sleep(INSTALL_POLL);
        }
    }

    fn move_in(&self, job: &PlannedLibraryAddJob, cancel: &AtomicBool) -> JobOutcome {
        let Some((finding, scanned, install_folder)) = self.with_batch(|batch| {
            let finding = batch
                .state
                .view()
                .findings
                .iter()
                .find(|found| {
                    found.source_id == job.item.source_id && found.finding.key == job.item.key
                })
                .map(|found| found.finding.clone());
            let scanned = batch
                .scanned
                .get(&job.item.source_id)
                .map(|scanned| ScannedSource {
                    session_id: scanned.session_id.clone(),
                    root_path: scanned.root_path.clone(),
                });
            (finding, scanned, batch.install_folder.clone())
        }) else {
            return JobOutcome::Failed("the list of works is gone".to_owned());
        };
        let (Some(finding), Some(scanned)) = (finding, scanned) else {
            return JobOutcome::Failed("the scan of this work is gone".to_owned());
        };
        // A choice still to make is asked for before anything moves, so a
        // moved work is never left out of the Library for want of one.
        if let Err(reason) = identity_for_add(&finding, &add_finding(&job.item)) {
            return JobOutcome::Failed(refusal_text(reason, ""));
        }
        let chosen = if job.item.unknown {
            None
        } else {
            job.item
                .work_code
                .clone()
                .filter(|code| !code.trim().is_empty())
                .or_else(|| finding.work_code.clone())
        };
        let source = finding_path(&finding, &scanned.root_path);
        let root_name = display_name(&scanned.root_path);
        let name = destination_name(
            &finding,
            &root_name,
            job.item.work_code.as_deref(),
            job.item.unknown,
        );
        let index = job.index;
        let last = Mutex::new(Instant::now() - PROGRESS_INTERVAL);
        let progress = |done: u64, total: u64| {
            let mut last = last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if done < total && last.elapsed() < PROGRESS_INTERVAL {
                return;
            }
            *last = Instant::now();
            self.update(|batch| batch.state.job_progress(index, done, total));
        };
        let cancelled = || cancel.load(Ordering::SeqCst);
        let outcome =
            match self
                .transfer
                .transfer(&source, &install_folder, &name, &cancelled, &progress)
            {
                Ok(outcome) => outcome,
                Err(FolderTransferError::Cancelled) => return JobOutcome::Cancelled,
                Err(error) => return JobOutcome::Failed(error.to_string()),
            };
        let warning = outcome.source_cleanup_error.map(|error| {
            format!("the copy is in place, but the original could not be removed: {error}")
        });
        let moved_into = |detail: &str| {
            format!("it is now in the install folder as {name}, but could not be added: {detail}")
        };
        let list = match self
            .scanner
            .scan_folder_now(&outcome.destination, &NeverCancelled)
            .map_err(|error| error.to_string())
            .and_then(|view| {
                self.findings
                    .list(&view.session.id)
                    .map_err(|error| error.to_string())
            }) {
            Ok(list) => list,
            Err(error) => return JobOutcome::Failed(moved_into(&error)),
        };
        let Some(key) = arrived_finding(&list) else {
            return JobOutcome::Failed(moved_into(
                "it holds more than one work; add it from the Scanner",
            ));
        };
        let item = AddScanFinding {
            key,
            work_code: chosen,
            unknown: job.item.unknown,
        };
        match self.add_from_session(&list.session_id, item) {
            JobOutcome::Done {
                installation_id, ..
            } => JobOutcome::Done {
                installation_id,
                warning,
            },
            JobOutcome::Failed(error) => JobOutcome::Failed(moved_into(&error)),
            JobOutcome::Cancelled => JobOutcome::Cancelled,
        }
    }

    fn add_from_session(&self, session_id: &ScanSessionId, item: AddScanFinding) -> JobOutcome {
        match self.findings.add(
            AddScanFindingsRequest {
                session_id: session_id.clone(),
                findings: vec![item],
            },
            &dla_sqlite::current_timestamp(),
        ) {
            Ok(mut added) if !added.is_empty() => match added.remove(0) {
                AddedScanFinding::Added {
                    installation_id, ..
                }
                | AddedScanFinding::AlreadyInLibrary {
                    installation_id, ..
                } => JobOutcome::Done {
                    installation_id: Some(installation_id),
                    warning: None,
                },
                AddedScanFinding::NeedsInstall { .. } => {
                    JobOutcome::Failed("this work is an archive to unpack".to_owned())
                }
                AddedScanFinding::Refused { reason, detail, .. } => {
                    JobOutcome::Failed(refusal_text(reason, &detail))
                }
            },
            Ok(_) => JobOutcome::Failed("the scan no longer lists this work".to_owned()),
            Err(error) => JobOutcome::Failed(error.to_string()),
        }
    }

    fn preset_identity(
        &self,
        installation_id: &InstallationId,
        identity: ManualCatalogIdentity,
    ) -> Result<(), String> {
        let installation = self
            .library
            .read(installation_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("{} is not in the Library", installation_id.0))?;
        let mut overrides = installation.overrides;
        overrides.catalog_identity = Some(identity);
        self.library
            .replace_overrides(installation_id, overrides, dla_sqlite::current_timestamp())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn scanned(&self, source_id: &str) -> Option<ScannedSource> {
        self.with_batch(|batch| {
            batch.scanned.get(source_id).map(|scanned| ScannedSource {
                session_id: scanned.session_id.clone(),
                root_path: scanned.root_path.clone(),
            })
        })
        .flatten()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Batch>> {
        self.batch
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn with_batch<T>(&self, read: impl FnOnce(&Batch) -> T) -> Option<T> {
        self.lock().as_ref().map(read)
    }

    fn update(&self, change: impl FnOnce(&mut Batch)) {
        if let Some(batch) = self.lock().as_mut() {
            change(batch);
        }
        self.emit();
    }

    fn emit(&self) {
        let Some(view) = self.view() else {
            return;
        };
        if let Err(error) = self.app.emit(LIBRARY_ADD_CHANGED_EVENT, view) {
            log::warn!("could not emit library add changes: {error}");
        }
    }
}

enum JobOutcome {
    Done {
        installation_id: Option<InstallationId>,
        warning: Option<String>,
    },
    Failed(String),
    Cancelled,
}

struct FlagCancellation<'a>(&'a AtomicBool);

impl ScanCancellation for FlagCancellation<'_> {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

struct NeverCancelled;

impl ScanCancellation for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
}

fn clone_plan(plan: &SourcePlan) -> SourcePlan {
    match plan {
        SourcePlan::Folder(path) => SourcePlan::Folder(path.clone()),
        SourcePlan::Scan(path) => SourcePlan::Scan(path.clone()),
        SourcePlan::Archive { parent, file_name } => SourcePlan::Archive {
            parent: parent.clone(),
            file_name: file_name.clone(),
        },
    }
}

fn add_finding(item: &LibraryAddItem) -> AddScanFinding {
    AddScanFinding {
        key: item.key.clone(),
        work_code: item.work_code.clone(),
        unknown: item.unknown,
    }
}

fn finding_path(finding: &ScanFinding, root_path: &Path) -> PathBuf {
    if finding.key == ROOT_FINDING_KEY || finding.relative_path.is_empty() {
        root_path.to_path_buf()
    } else {
        finding
            .relative_path
            .split('/')
            .fold(root_path.to_path_buf(), |path, part| path.join(part))
    }
}

/// The finding a moved work became once scanned where it now is.
fn arrived_finding(list: &ScanFindingList) -> Option<String> {
    match list.findings.as_slice() {
        [only] => Some(only.key.clone()),
        findings => findings
            .iter()
            .find(|finding| finding.key == ROOT_FINDING_KEY)
            .map(|finding| finding.key.clone()),
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn refusal_text(reason: ScanFindingRefusal, detail: &str) -> String {
    match reason {
        ScanFindingRefusal::NotFound => "the work is no longer where it was found".to_owned(),
        ScanFindingRefusal::ChooseWork => {
            "choose which catalog work this is, or add it as an unknown work".to_owned()
        }
        ScanFindingRefusal::Mixed => "this folder holds files of more than one work".to_owned(),
        ScanFindingRefusal::Incomplete => "a part of this archive set is missing".to_owned(),
        ScanFindingRefusal::Failed if !detail.is_empty() => detail.to_owned(),
        ScanFindingRefusal::Failed => "the work could not be added".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moved_work_with_no_single_finding_is_not_guessed() {
        let list = ScanFindingList {
            session_id: ScanSessionId("session-1".to_owned()),
            root_id: dla_domain::scanner::ScanRootId("root-1".to_owned()),
            root_path: "/library/RJ01000001".to_owned(),
            findings: Vec::new(),
        };
        assert!(arrived_finding(&list).is_none());
    }

    #[test]
    fn refusals_read_as_what_to_do_next() {
        assert_eq!(
            refusal_text(ScanFindingRefusal::Failed, "disk full"),
            "disk full"
        );
        assert_eq!(
            refusal_text(ScanFindingRefusal::ChooseWork, ""),
            "choose which catalog work this is, or add it as an unknown work"
        );
        assert_eq!(display_name(Path::new("/picked/My Folder")), "My Folder");
    }
}
