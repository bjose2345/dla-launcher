use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use dla_application::{
    delivery::{
        DeliveryControl, DeliveryError, DeliveryProgressSink, DeliveryService, InstallProgress,
        LocalInstallOutcome, LocalInstaller,
    },
    identity::{ArchiveHash, ArchiveHashAlgorithm},
    installation_from_scan::{CreateInstallationFromScanRequest, InstallationFromScanService},
    installation_review::{InstallationReviewRequest, InstallationReviewService},
    package_preparation::{PackageDestinationConflictPolicy, PackagePreparationError},
    scanner::{
        ArchiveHashError, ArchiveHashRequest, ArchiveHasher, ScanCancellation, ScanResultRequest,
    },
};
use dla_connector::{Connector, ConnectorConfig, ConnectorState};
use dla_delivery::{DeliveryClient, Dial};
use dla_domain::{
    delivery::{
        DeliveryArchive, DeliveryJob, DeliveryJobId, DeliveryPhase, DeliveryProblem,
        DeliveryProgress,
    },
    installation::InstallationStatus,
    package::{ArchiveRetentionPolicy, PackagePreparationStage},
    scanner::ScanMatchOutcome,
};
use dla_scanner::ScanAccessRegistry;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    delivery_access::EmbeddedAccess, package_preparation::PackagePreparationController,
    scanner::ScanController,
};

pub const DELIVERY_CHANGED_EVENT: &str = "delivery-changed";
pub const DELIVERY_PROGRESS_EVENT: &str = "delivery-progress";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const FORWARD_TIMEOUT: Duration = Duration::from_secs(20);

/// The bundled cloudflared forward to the downloads route, started at the
/// first sign-in and kept for the rest of the session.
pub struct Forward {
    access: EmbeddedAccess,
    program: PathBuf,
    connector: OnceLock<Connector>,
}

impl Forward {
    pub fn new(access: EmbeddedAccess, program: PathBuf) -> Self {
        Self {
            access,
            program,
            connector: OnceLock::new(),
        }
    }

    pub fn dial(self: &Arc<Self>) -> Dial {
        let forward = Arc::clone(self);
        Arc::new(move || forward.connector.get().and_then(Connector::local_address))
    }

    fn ensure(&self) -> Result<(), DeliveryError> {
        let connector = self.connector.get_or_init(|| {
            Connector::start(
                ConnectorConfig {
                    program: self.program.clone(),
                    hostname: self.access.hostname.clone(),
                    token: self.access.token.clone(),
                },
                |state| log::info!(target: "dla::delivery", "event=forward_state state={state:?}"),
            )
        });
        let started = Instant::now();
        while started.elapsed() < FORWARD_TIMEOUT {
            if matches!(connector.state(), ConnectorState::Listening { .. }) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(DeliveryError::problem(
            DeliveryProblem::Unreachable,
            "the connection to DLA could not be opened",
        ))
    }
}

/// The sidecar beside the launcher's own executable.
pub fn cloudflared_sidecar() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_default()
        .join(format!("dla-cloudflared{}", std::env::consts::EXE_SUFFIX))
}

#[derive(Default)]
struct JobControl {
    pause: AtomicBool,
    cancel: AtomicBool,
}

impl DeliveryControl for JobControl {
    fn check(&self) -> Result<(), DeliveryError> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(DeliveryError::Cancelled);
        }
        if self.pause.load(Ordering::Acquire) {
            return Err(DeliveryError::Paused);
        }
        Ok(())
    }
}

#[derive(Default)]
struct Gate {
    phase: Option<DeliveryPhase>,
    emitted_at: Option<Instant>,
}

struct ProgressEvents {
    app: AppHandle,
    latest: Mutex<Option<DeliveryProgress>>,
    gate: Mutex<Gate>,
}

impl ProgressEvents {
    fn publish(&self, progress: &DeliveryProgress) {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(progress.clone());
        }
        let mut gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        let due = gate.phase != Some(progress.phase)
            || gate
                .emitted_at
                .is_none_or(|at| now.saturating_duration_since(at) >= PROGRESS_INTERVAL);
        if due {
            gate.phase = Some(progress.phase);
            gate.emitted_at = Some(now);
            if let Err(error) = self.app.emit(DELIVERY_PROGRESS_EVENT, progress) {
                log::warn!("could not emit delivery progress: {error}");
            }
        }
    }
}

/// What the running job reports: progress as throttled progress events, and
/// each saved status as a full change, so the interface follows the job.
struct JobEvents<'a>(&'a DeliveryController);

impl DeliveryProgressSink for JobEvents<'_> {
    fn publish(&self, progress: &DeliveryProgress) {
        self.0.progress.publish(progress);
    }

    fn job_saved(&self, _job: &DeliveryJob) {
        self.0.changed();
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryView {
    /// Counts changes, so the interface keeps the newest view when a command's
    /// reply and the change events arrive out of order.
    pub revision: u64,
    /// Whether this build can reach DLA at all.
    pub available: bool,
    pub account: Option<String>,
    pub install_folder: Option<String>,
    pub jobs: Vec<DeliveryJob>,
    pub progress: Option<DeliveryProgress>,
}

/// A refused delivery command, as a stable code plus untranslated detail.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryFailure {
    pub problem: DeliveryProblem,
    pub detail: String,
}

impl From<DeliveryError> for DeliveryFailure {
    fn from(error: DeliveryError) -> Self {
        match error {
            DeliveryError::Problem { problem, detail } => Self { problem, detail },
            other => Self {
                problem: DeliveryProblem::Unavailable,
                detail: other.to_string(),
            },
        }
    }
}

pub struct DeliveryController {
    service: Arc<DeliveryService>,
    client: Option<Arc<DeliveryClient>>,
    forward: Option<Arc<Forward>>,
    app: AppHandle,
    progress: Arc<ProgressEvents>,
    running: Mutex<Option<(DeliveryJobId, Arc<JobControl>)>>,
    worker: AtomicBool,
    revision: AtomicU64,
}

fn platform() -> &'static str {
    std::env::consts::OS
}

/// Windows canonical paths carry the `\\?\` prefix, which the rest of the
/// launcher never stores, so a drive path keeps its ordinary form.
fn ordinary_path(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

fn unavailable() -> DeliveryError {
    DeliveryError::problem(
        DeliveryProblem::Unavailable,
        "Install from DLA is not available in this build",
    )
}

// Problems that concern the whole session rather than one work stop the
// queue, so the remaining jobs are not failed one after another.
fn stops_the_queue(problem: Option<DeliveryProblem>) -> bool {
    !matches!(
        problem,
        None | Some(
            DeliveryProblem::NotInArchive | DeliveryProblem::Corrupt | DeliveryProblem::Install
        )
    )
}

impl DeliveryController {
    pub fn new(
        service: Arc<DeliveryService>,
        client: Option<Arc<DeliveryClient>>,
        forward: Option<Arc<Forward>>,
        app: AppHandle,
    ) -> Self {
        Self {
            service,
            client,
            forward,
            progress: Arc::new(ProgressEvents {
                app: app.clone(),
                latest: Mutex::new(None),
                gate: Mutex::new(Gate::default()),
            }),
            app,
            running: Mutex::new(None),
            worker: AtomicBool::new(false),
            revision: AtomicU64::new(0),
        }
    }

    fn signed_in(&self) -> bool {
        self.client
            .as_ref()
            .is_some_and(|client| client.signed_in_as().is_some())
    }

    pub fn view(&self) -> Result<DeliveryView, DeliveryError> {
        // Read before the state, so a view is never older than its revision.
        let revision = self.revision.load(Ordering::SeqCst);
        let running = self
            .running
            .lock()
            .map(|running| running.is_some())
            .unwrap_or(false);
        Ok(DeliveryView {
            revision,
            available: self.client.is_some(),
            account: self
                .client
                .as_ref()
                .and_then(|client| client.signed_in_as()),
            install_folder: self.service.install_folder(platform())?,
            jobs: self.service.jobs()?,
            progress: running
                .then(|| {
                    self.progress
                        .latest
                        .lock()
                        .ok()
                        .and_then(|latest| latest.clone())
                })
                .flatten(),
        })
    }

    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
        match self.view() {
            Ok(view) => {
                if let Err(error) = self.app.emit(DELIVERY_CHANGED_EVENT, view) {
                    log::warn!("could not emit delivery changes: {error}");
                }
            }
            Err(error) => log::warn!("could not read deliveries: {error}"),
        }
    }

    pub fn sign_in(
        self: &Arc<Self>,
        username: &str,
        password: Zeroizing<String>,
    ) -> Result<DeliveryView, DeliveryError> {
        let client = self.client.as_ref().ok_or_else(unavailable)?;
        if let Some(forward) = &self.forward {
            forward.ensure()?;
        }
        client.sign_in(username, password)?;
        log::info!(target: "dla::delivery", "event=signed_in");
        self.kick();
        self.changed();
        self.view()
    }

    pub fn sign_out(&self) -> Result<DeliveryView, DeliveryError> {
        if let Some((_, control)) = self.running.lock().ok().and_then(|running| running.clone()) {
            control.pause.store(true, Ordering::Release);
        }
        if let Some(client) = &self.client {
            client.sign_out();
        }
        self.changed();
        self.view()
    }

    pub fn set_install_folder(&self, folder: &Path) -> Result<DeliveryView, DeliveryError> {
        let metadata = fs::symlink_metadata(folder).map_err(|error| {
            DeliveryError::problem(DeliveryProblem::InstallFolder, error.to_string())
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(DeliveryError::problem(
                DeliveryProblem::InstallFolder,
                "the install folder must be a regular folder",
            ));
        }
        let canonical = ordinary_path(folder.canonicalize().map_err(|error| {
            DeliveryError::problem(DeliveryProblem::InstallFolder, error.to_string())
        })?);
        self.service.set_install_folder(
            platform(),
            &canonical.to_string_lossy(),
            &dla_sqlite::current_timestamp(),
        )?;
        self.changed();
        self.view()
    }

    pub fn install(self: &Arc<Self>, work_code: &str) -> Result<DeliveryView, DeliveryError> {
        if self.client.is_none() {
            return Err(unavailable());
        }
        let folder = self.service.install_folder(platform())?.ok_or_else(|| {
            DeliveryError::problem(
                DeliveryProblem::InstallFolder,
                "choose an install folder first",
            )
        })?;
        let job = self.service.queue(
            work_code,
            &folder,
            DeliveryJobId(Uuid::new_v4().to_string()),
            &dla_sqlite::current_timestamp(),
        )?;
        log::info!(target: "dla::delivery", "event=queued job_id={} work_code={}", job.id.0, job.work_code);
        self.kick();
        self.changed();
        self.view()
    }

    fn running_control(&self, id: &DeliveryJobId) -> Option<Arc<JobControl>> {
        self.running
            .lock()
            .ok()
            .and_then(|running| running.clone())
            .filter(|(running, _)| running == id)
            .map(|(_, control)| control)
    }

    pub fn pause(&self, id: &DeliveryJobId) -> Result<DeliveryView, DeliveryError> {
        match self.running_control(id) {
            Some(control) => control.pause.store(true, Ordering::Release),
            None => {
                self.service.pause(id, &dla_sqlite::current_timestamp())?;
            }
        }
        self.changed();
        self.view()
    }

    pub fn resume(self: &Arc<Self>, id: &DeliveryJobId) -> Result<DeliveryView, DeliveryError> {
        self.service.resume(id, &dla_sqlite::current_timestamp())?;
        self.kick();
        self.changed();
        self.view()
    }

    pub fn cancel(&self, id: &DeliveryJobId) -> Result<DeliveryView, DeliveryError> {
        match self.running_control(id) {
            Some(control) => control.cancel.store(true, Ordering::Release),
            None => {
                self.service.cancel(id, &dla_sqlite::current_timestamp())?;
            }
        }
        self.changed();
        self.view()
    }

    pub fn remove(&self, id: &DeliveryJobId) -> Result<DeliveryView, DeliveryError> {
        self.service.remove(id)?;
        self.changed();
        self.view()
    }

    /// Starts the worker unless it runs already. It works through the queue
    /// one job at a time while the session is signed in.
    fn kick(self: &Arc<Self>) {
        if !self.signed_in() || self.worker.swap(true, Ordering::AcqRel) {
            return;
        }
        let controller = Arc::clone(self);
        thread::spawn(move || {
            controller.work();
            controller.worker.store(false, Ordering::Release);
            // A job queued while the worker was finishing.
            if controller.signed_in() && matches!(controller.service.next_queued(), Ok(Some(_))) {
                controller.kick();
            }
        });
    }

    fn work(&self) {
        while self.signed_in() {
            let job = match self.service.next_queued() {
                Ok(Some(job)) => job,
                Ok(None) => return,
                Err(error) => {
                    log::warn!(target: "dla::delivery", "event=queue_unreadable error={error}");
                    return;
                }
            };
            let control = Arc::new(JobControl::default());
            if let Ok(mut running) = self.running.lock() {
                *running = Some((job.id.clone(), Arc::clone(&control)));
            }
            self.changed();
            let result = self.service.run(
                &job.id,
                control.as_ref(),
                &JobEvents(self),
                &dla_sqlite::current_timestamp,
            );
            if let Ok(mut running) = self.running.lock() {
                *running = None;
            }
            self.changed();
            match result {
                Ok(job) => {
                    log::info!(
                        target: "dla::delivery",
                        "event=job_stopped job_id={} status={:?} problem={:?}",
                        job.id.0,
                        job.status,
                        job.problem
                    );
                    if matches!(
                        job.problem,
                        Some(DeliveryProblem::Credentials | DeliveryProblem::SignInRequired)
                    ) && let Some(client) = &self.client
                    {
                        client.sign_out();
                        self.changed();
                    }
                    if stops_the_queue(job.problem) {
                        return;
                    }
                }
                Err(error) => {
                    log::warn!(target: "dla::delivery", "event=job_unrunnable error={error}");
                    return;
                }
            }
        }
    }
}

/// SHA-256 values Install verified while downloading, so identifying the
/// archives afterwards does not read gigabytes again. An entry counts only
/// while the file keeps the size and modification time it had then.
#[derive(Default)]
pub struct KnownDigests(Mutex<HashMap<PathBuf, KnownDigest>>);

/// Size, modification time and SHA-256 of one verified file.
type KnownDigest = (u64, Option<SystemTime>, String);

impl KnownDigests {
    fn remember(&self, path: &Path, sha256: &str) {
        if let (Ok(metadata), Ok(path), Ok(mut known)) =
            (fs::metadata(path), path.canonicalize(), self.0.lock())
        {
            known.insert(
                path,
                (metadata.len(), metadata.modified().ok(), sha256.to_owned()),
            );
        }
    }

    fn lookup(&self, path: &Path) -> Option<String> {
        let canonical = path.canonicalize().ok()?;
        let metadata = fs::metadata(&canonical).ok()?;
        let known = self.0.lock().ok()?;
        let (size, modified, sha256) = known.get(&canonical)?;
        (*size == metadata.len() && *modified == metadata.modified().ok()).then(|| sha256.clone())
    }

    fn forget(&self, paths: &[PathBuf]) {
        if let Ok(mut known) = self.0.lock() {
            for path in paths {
                if let Ok(path) = path.canonicalize() {
                    known.remove(&path);
                }
            }
        }
    }
}

/// The scanner's archive hasher, answering from `KnownDigests` first.
pub struct KnownDigestHasher {
    inner: Arc<dyn ArchiveHasher>,
    access: Arc<ScanAccessRegistry>,
    known: Arc<KnownDigests>,
}

impl KnownDigestHasher {
    pub fn new(
        inner: Arc<dyn ArchiveHasher>,
        access: Arc<ScanAccessRegistry>,
        known: Arc<KnownDigests>,
    ) -> Self {
        Self {
            inner,
            access,
            known,
        }
    }
}

impl ArchiveHasher for KnownDigestHasher {
    fn hash(
        &self,
        request: &ArchiveHashRequest,
        cancellation: &dyn ScanCancellation,
    ) -> Result<ArchiveHash, ArchiveHashError> {
        if request.algorithm == ArchiveHashAlgorithm::Sha256
            && let Ok(root) = self.access.resolve(&request.access_handle)
            && let Some(digest) = self.known.lookup(&root.join(&request.relative_path))
        {
            return Ok(ArchiveHash {
                algorithm: ArchiveHashAlgorithm::Sha256,
                digest,
            });
        }
        self.inner.hash(request, cancellation)
    }
}

/// Lets a scan notice a pause or cancellation of its job.
struct ControlCancellation<'a>(&'a dyn DeliveryControl);

impl ScanCancellation for ControlCancellation<'_> {
    fn is_cancelled(&self) -> bool {
        self.0.check().is_err()
    }
}

fn install_problem(detail: impl std::fmt::Display) -> DeliveryError {
    DeliveryError::problem(DeliveryProblem::Install, detail.to_string())
}

/// The local half of Install: the same scan, installation and package
/// preparation a subscriber could run by hand, without the choices.
pub struct ShellInstaller {
    scanner: Arc<ScanController>,
    installations: Arc<InstallationFromScanService>,
    preparation: Arc<PackagePreparationController>,
    review: Arc<InstallationReviewService>,
    known: Arc<KnownDigests>,
}

impl ShellInstaller {
    pub fn new(
        scanner: Arc<ScanController>,
        installations: Arc<InstallationFromScanService>,
        preparation: Arc<PackagePreparationController>,
        review: Arc<InstallationReviewService>,
        known: Arc<KnownDigests>,
    ) -> Self {
        Self {
            scanner,
            installations,
            preparation,
            review,
            known,
        }
    }

    fn identify_and_prepare(
        &self,
        work_code: &str,
        staging_folder: &str,
        install_folder: &str,
        control: &dyn DeliveryControl,
        progress: InstallProgress<'_>,
    ) -> Result<LocalInstallOutcome, DeliveryError> {
        let session = match self
            .scanner
            .scan_folder_now(Path::new(staging_folder), &ControlCancellation(control))
        {
            Ok(session) => session,
            Err(error) => {
                control.check()?;
                return Err(install_problem(error));
            }
        };
        let results = self
            .scanner
            .browse_results(ScanResultRequest {
                session_id: session.session.id.0.clone(),
                outcome: Some(ScanMatchOutcome::Matched),
                limit: 200,
                offset: 0,
            })
            .map_err(install_problem)?;
        let selected = results
            .items
            .into_iter()
            .find(|item| {
                item.result
                    .selected_work_code
                    .as_deref()
                    .is_some_and(|code| code.eq_ignore_ascii_case(work_code))
            })
            .ok_or_else(|| {
                install_problem(format!(
                    "the downloaded archives were not recognised as {work_code}"
                ))
            })?;
        let installation = self
            .installations
            .create_or_refresh(CreateInstallationFromScanRequest {
                session_id: session.session.id.clone(),
                selected_result_id: selected.result.id.clone(),
            })
            .map_err(install_problem)?;
        let cancelled = || control.check().is_err();
        let observe = |update: &dla_domain::package::PackagePreparationProgress| {
            let phase = match update.stage {
                PackagePreparationStage::Extracting => DeliveryPhase::Unpacking,
                _ => DeliveryPhase::Installing,
            };
            progress(
                phase,
                update.counters.processed_bytes,
                update.counters.total_bytes,
            );
        };
        let prepared = match self.preparation.execute_now(
            installation.id.clone(),
            install_folder.to_owned(),
            PackageDestinationConflictPolicy::Refuse,
            ArchiveRetentionPolicy::DeleteAfterVerifiedInstall,
            &cancelled,
            &observe,
        ) {
            Ok(prepared) => Some(prepared),
            Err(PackagePreparationError::AlreadyPrepared(_)) => None,
            Err(PackagePreparationError::Cancelled) => {
                control.check()?;
                return Err(DeliveryError::Cancelled);
            }
            Err(error) => {
                return Ok(LocalInstallOutcome::NeedsReview(
                    installation.id,
                    error.to_string(),
                ));
            }
        };
        // The owner chose no Review step for Install: the program the
        // package inspection found is accepted, so the work shows Play.
        // Without a detected program the Library still asks for one.
        if prepared
            .as_ref()
            .is_some_and(|prepared| prepared.preferred_action.is_none())
        {
            return Ok(LocalInstallOutcome::NeedsReview(
                installation.id,
                "the launcher could not tell which program starts this work".to_owned(),
            ));
        }
        let reviewed = self
            .review
            .save(
                InstallationReviewRequest {
                    installation_id: installation.id.clone(),
                    catalog_identity: None,
                    custom_title: None,
                    preferred_action: None,
                    content_items: Vec::new(),
                },
                dla_sqlite::current_timestamp(),
            )
            .map_err(install_problem)?;
        if reviewed.status == InstallationStatus::Ready {
            Ok(LocalInstallOutcome::Installed(installation.id))
        } else {
            Ok(LocalInstallOutcome::NeedsReview(
                installation.id,
                "the installed files need a check before Play".to_owned(),
            ))
        }
    }
}

impl LocalInstaller for ShellInstaller {
    fn install(
        &self,
        work_code: &str,
        staging_folder: &str,
        install_folder: &str,
        archives: &[DeliveryArchive],
        control: &dyn DeliveryControl,
        progress: InstallProgress<'_>,
    ) -> Result<LocalInstallOutcome, DeliveryError> {
        let paths: Vec<PathBuf> = archives
            .iter()
            .map(|archive| Path::new(staging_folder).join(&archive.file_name))
            .collect();
        for (path, archive) in paths.iter().zip(archives) {
            self.known.remember(path, &archive.sha256);
        }
        let outcome =
            self.identify_and_prepare(work_code, staging_folder, install_folder, control, progress);
        self.known.forget(&paths);
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_folders_drop_only_the_windows_drive_prefix() {
        assert_eq!(
            ordinary_path(PathBuf::from(r"\\?\C:\Users\develop\DLA Works")),
            PathBuf::from(r"C:\Users\develop\DLA Works")
        );
        assert_eq!(
            ordinary_path(PathBuf::from(r"\\?\UNC\nas\games")),
            PathBuf::from(r"\\?\UNC\nas\games")
        );
        assert_eq!(
            ordinary_path(PathBuf::from("/home/develop/DLA Works")),
            PathBuf::from("/home/develop/DLA Works")
        );
    }

    #[test]
    fn verified_digests_count_only_for_the_unchanged_file() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("RJ01234567.zip");
        fs::write(&path, b"verified archive").unwrap();
        let known = KnownDigests::default();
        assert_eq!(known.lookup(&path), None);
        known.remember(&path, &"a".repeat(64));
        assert_eq!(
            known.lookup(&path).as_deref(),
            Some("a".repeat(64).as_str())
        );
        // Another spelling of the same file finds it too.
        assert!(
            known
                .lookup(&folder.path().join("./RJ01234567.zip"))
                .is_some()
        );

        fs::write(&path, b"changed after it was verified").unwrap();
        assert_eq!(known.lookup(&path), None, "a changed file is hashed again");
        known.remember(&path, &"b".repeat(64));
        known.forget(std::slice::from_ref(&path));
        assert_eq!(known.lookup(&path), None);
    }
}
