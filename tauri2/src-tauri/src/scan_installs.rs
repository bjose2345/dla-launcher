use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use dla_application::{
    delivery::InstallFolderStore,
    installation_review::InstallationReviewService,
    maintenance::LibraryMaintenanceService,
    package_preparation::{PackageDestinationConflictPolicy, PackagePreparationError},
    scan_findings::{
        AddScanFindingsRequest, AddedScanFinding, ScanFindingError, ScanFindingRefusal,
        ScanFindingService,
    },
};
use dla_domain::{
    installation::{InstallationId, InstallationStatus, ManualCatalogIdentity},
    package::ArchiveRetentionPolicy,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::package_preparation::PackagePreparationController;

pub const SCAN_INSTALLS_CHANGED_EVENT: &str = "scan-installs-changed";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanInstallStatus {
    Queued,
    Installing,
    Installed,
    NeedsReview,
    Failed,
    Cancelled,
}

/// An archive the scanner found, installing so it can join the Library.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanInstall {
    pub installation_id: InstallationId,
    pub key: String,
    pub name: String,
    pub work_code: Option<String>,
    pub status: ScanInstallStatus,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanInstallView {
    /// Counts changes, so the interface keeps the newest view when a reply and
    /// the change events arrive out of order.
    pub revision: u64,
    pub install_folder: Option<String>,
    pub installs: Vec<ScanInstall>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanAddOutcome {
    Added,
    Installing,
    AlreadyInLibrary,
    Refused,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanAddResult {
    pub key: String,
    pub outcome: ScanAddOutcome,
    pub installation_id: Option<InstallationId>,
    pub refusal: Option<ScanFindingRefusal>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanAddResponse {
    pub results: Vec<ScanAddResult>,
    pub installs: ScanInstallView,
}

struct Queued {
    installation_id: InstallationId,
    identity: Option<ManualCatalogIdentity>,
    retention: ArchiveRetentionPolicy,
}

#[derive(Default)]
struct State {
    installs: Vec<ScanInstall>,
    queue: VecDeque<Queued>,
    cancels: HashMap<InstallationId, Arc<AtomicBool>>,
}

/// Installs the archives a scan found, one after another, into the install
/// folder Install from DLA also uses, and confirms each into the Library.
/// The user's archives are kept unless "+ Add" was asked to delete them.
pub struct ScanInstallController {
    findings: Arc<ScanFindingService>,
    preparation: Arc<PackagePreparationController>,
    review: Arc<InstallationReviewService>,
    maintenance: Arc<LibraryMaintenanceService>,
    folders: Arc<dyn InstallFolderStore>,
    app: AppHandle,
    state: Mutex<State>,
    worker: AtomicBool,
    revision: AtomicU64,
}

fn platform() -> &'static str {
    std::env::consts::OS
}

impl ScanInstallController {
    pub fn new(
        findings: Arc<ScanFindingService>,
        preparation: Arc<PackagePreparationController>,
        review: Arc<InstallationReviewService>,
        maintenance: Arc<LibraryMaintenanceService>,
        folders: Arc<dyn InstallFolderStore>,
        app: AppHandle,
    ) -> Self {
        Self {
            findings,
            preparation,
            review,
            maintenance,
            folders,
            app,
            state: Mutex::new(State::default()),
            worker: AtomicBool::new(false),
            revision: AtomicU64::new(0),
        }
    }

    pub fn view(&self) -> ScanInstallView {
        let revision = self.revision.load(Ordering::SeqCst);
        let installs = self
            .state
            .lock()
            .map(|state| state.installs.clone())
            .unwrap_or_default();
        let install_folder = self
            .folders
            .read_install_folder(platform())
            .unwrap_or_else(|error| {
                log::warn!("could not read the install folder: {error}");
                None
            });
        ScanInstallView {
            revision,
            install_folder,
            installs,
        }
    }

    pub fn changed(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
        if let Err(error) = self.app.emit(SCAN_INSTALLS_CHANGED_EVENT, self.view()) {
            log::warn!("could not emit scan install changes: {error}");
        }
    }

    pub fn add(
        self: &Arc<Self>,
        request: AddScanFindingsRequest,
    ) -> Result<ScanAddResponse, ScanFindingError> {
        let added = self
            .findings
            .add(request, &dla_sqlite::current_timestamp())?;
        let mut queued_any = false;
        let results = added
            .into_iter()
            .map(|outcome| match outcome {
                AddedScanFinding::Added {
                    key,
                    installation_id,
                } => ScanAddResult {
                    key,
                    outcome: ScanAddOutcome::Added,
                    installation_id: Some(installation_id),
                    refusal: None,
                    detail: None,
                },
                AddedScanFinding::AlreadyInLibrary {
                    key,
                    installation_id,
                } => ScanAddResult {
                    key,
                    outcome: ScanAddOutcome::AlreadyInLibrary,
                    installation_id: Some(installation_id),
                    refusal: None,
                    detail: None,
                },
                AddedScanFinding::Refused {
                    key,
                    reason,
                    detail,
                } => ScanAddResult {
                    key,
                    outcome: ScanAddOutcome::Refused,
                    installation_id: None,
                    refusal: Some(reason),
                    detail: (!detail.is_empty()).then_some(detail),
                },
                AddedScanFinding::NeedsInstall {
                    key,
                    installation_id,
                    identity,
                    name,
                    work_code,
                } => {
                    queued_any |= self.enqueue(
                        ScanInstall {
                            installation_id: installation_id.clone(),
                            key: key.clone(),
                            name,
                            work_code,
                            status: ScanInstallStatus::Queued,
                            detail: None,
                        },
                        identity,
                        ArchiveRetentionPolicy::Keep,
                    );
                    ScanAddResult {
                        key,
                        outcome: ScanAddOutcome::Installing,
                        installation_id: Some(installation_id),
                        refusal: None,
                        detail: None,
                    }
                }
            })
            .collect();
        if queued_any {
            self.kick();
            self.changed();
        }
        Ok(ScanAddResponse {
            results,
            installs: self.view(),
        })
    }

    /// Queues an archive "+ Add" chose, keeping or deleting its volumes once
    /// it is installed and checked. False when it is already queued.
    pub fn queue(
        self: &Arc<Self>,
        install: ScanInstall,
        identity: Option<ManualCatalogIdentity>,
        retention: ArchiveRetentionPolicy,
    ) -> bool {
        let queued = self.enqueue(install, identity, retention);
        if queued {
            self.kick();
            self.changed();
        }
        queued
    }

    /// Where an install stands, with the reason it failed.
    pub fn status(
        &self,
        installation_id: &InstallationId,
    ) -> Option<(ScanInstallStatus, Option<String>)> {
        let state = self.state.lock().ok()?;
        state
            .installs
            .iter()
            .find(|install| &install.installation_id == installation_id)
            .map(|install| (install.status, install.detail.clone()))
    }

    fn enqueue(
        &self,
        install: ScanInstall,
        identity: Option<ManualCatalogIdentity>,
        retention: ArchiveRetentionPolicy,
    ) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        let active = state.installs.iter().any(|existing| {
            existing.installation_id == install.installation_id
                && matches!(
                    existing.status,
                    ScanInstallStatus::Queued | ScanInstallStatus::Installing
                )
        });
        if active {
            return false;
        }
        state
            .installs
            .retain(|existing| existing.installation_id != install.installation_id);
        state.queue.push_back(Queued {
            installation_id: install.installation_id.clone(),
            identity,
            retention,
        });
        state.installs.push(install);
        true
    }

    /// Installs an archive the Library already records but has not unpacked,
    /// such as one added before adding and installing were one step.
    pub fn install_existing(
        self: &Arc<Self>,
        installation_id: InstallationId,
    ) -> Result<ScanInstallView, String> {
        let installation = self
            .review
            .read(&installation_id)
            .map_err(|error| error.to_string())?;
        let Some(inspection) = &installation.detection.package_inspection else {
            return Err("this work has no archive to install".to_owned());
        };
        let install = ScanInstall {
            installation_id,
            key: String::new(),
            name: inspection.source.relative_path.as_str().to_owned(),
            work_code: installation
                .effective_catalog_work_code()
                .map(str::to_owned),
            status: ScanInstallStatus::Queued,
            detail: None,
        };
        if self.enqueue(install, None, ArchiveRetentionPolicy::Keep) {
            self.kick();
            self.changed();
        }
        Ok(self.view())
    }

    /// Stops a queued or running install and forgets the work, as Undo does;
    /// the scanned archives stay where they are.
    pub fn cancel(&self, installation_id: &InstallationId) -> ScanInstallView {
        let removed_from_queue = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let queued = state
                .queue
                .iter()
                .position(|item| &item.installation_id == installation_id);
            if let Some(position) = queued {
                state.queue.remove(position);
                set_status(
                    &mut state,
                    installation_id,
                    ScanInstallStatus::Cancelled,
                    None,
                );
            } else if let Some(cancel) = state.cancels.get(installation_id) {
                cancel.store(true, Ordering::SeqCst);
            }
            queued.is_some()
        };
        if removed_from_queue {
            self.forget(installation_id);
            self.changed();
        }
        self.view()
    }

    fn forget(&self, installation_id: &InstallationId) {
        if let Err(error) = self.maintenance.remove_from_library(installation_id) {
            log::warn!(
                target: "dla::scanner",
                "event=scan_install_forget_failed installation_id={} error={error}",
                installation_id.0
            );
        }
    }

    fn kick(self: &Arc<Self>) {
        if self.worker.swap(true, Ordering::SeqCst) {
            return;
        }
        let controller = Arc::clone(self);
        std::thread::spawn(move || {
            loop {
                while let Some(next) = controller.next() {
                    controller.install(next);
                }
                controller.worker.store(false, Ordering::SeqCst);
                let more = controller
                    .state
                    .lock()
                    .map(|state| !state.queue.is_empty())
                    .unwrap_or(false);
                if !more || controller.worker.swap(true, Ordering::SeqCst) {
                    break;
                }
            }
        });
    }

    fn next(&self) -> Option<(Queued, Arc<AtomicBool>)> {
        let mut state = self.state.lock().ok()?;
        let next = state.queue.pop_front()?;
        let cancel = Arc::new(AtomicBool::new(false));
        state
            .cancels
            .insert(next.installation_id.clone(), Arc::clone(&cancel));
        set_status(
            &mut state,
            &next.installation_id,
            ScanInstallStatus::Installing,
            None,
        );
        drop(state);
        self.changed();
        Some((next, cancel))
    }

    fn install(&self, (queued, cancel): (Queued, Arc<AtomicBool>)) {
        let id = queued.installation_id.clone();
        let (status, detail) = self.prepare_and_confirm(queued, &cancel);
        if status == ScanInstallStatus::Cancelled {
            self.forget(&id);
        }
        log::info!(
            target: "dla::scanner",
            "event=scan_install_finished installation_id={} status={status:?}",
            id.0
        );
        if let Ok(mut state) = self.state.lock() {
            state.cancels.remove(&id);
            set_status(&mut state, &id, status, detail);
        }
        self.changed();
    }

    fn prepare_and_confirm(
        &self,
        queued: Queued,
        cancel: &AtomicBool,
    ) -> (ScanInstallStatus, Option<String>) {
        let folder = match self.folders.read_install_folder(platform()) {
            Ok(Some(folder)) => folder,
            Ok(None) => {
                return (
                    ScanInstallStatus::Failed,
                    Some("choose an install folder first".to_owned()),
                );
            }
            Err(error) => return (ScanInstallStatus::Failed, Some(error.to_string())),
        };
        let cancelled = || cancel.load(Ordering::SeqCst);
        match self.preparation.execute_now(
            queued.installation_id.clone(),
            folder,
            PackageDestinationConflictPolicy::Refuse,
            queued.retention,
            &cancelled,
            &|_| {},
        ) {
            Ok(_) | Err(PackagePreparationError::AlreadyPrepared(_)) => {}
            Err(PackagePreparationError::Cancelled) => {
                return (ScanInstallStatus::Cancelled, None);
            }
            Err(error) => return (ScanInstallStatus::Failed, Some(error.to_string())),
        }
        if cancelled() {
            return (ScanInstallStatus::Cancelled, None);
        }
        let installation = match self.review.read(&queued.installation_id) {
            Ok(installation) => installation,
            Err(error) => return (ScanInstallStatus::Failed, Some(error.to_string())),
        };
        match self.findings.confirm(
            &installation,
            queued.identity,
            &dla_sqlite::current_timestamp(),
        ) {
            Ok(saved) if saved.status == InstallationStatus::Ready => {
                (ScanInstallStatus::Installed, None)
            }
            Ok(_) => (ScanInstallStatus::NeedsReview, None),
            Err(error) => (ScanInstallStatus::Failed, Some(error.to_string())),
        }
    }
}

fn set_status(
    state: &mut State,
    installation_id: &InstallationId,
    status: ScanInstallStatus,
    detail: Option<String>,
) {
    if let Some(install) = state
        .installs
        .iter_mut()
        .find(|install| &install.installation_id == installation_id)
    {
        install.status = status;
        install.detail = detail;
    }
}
