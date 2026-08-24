use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use dla_domain::{
    installation::{
        Installation, InstallationId, InstallationPlatform, InstallationStatus, LaunchActionKind,
        LaunchTarget, MediaType, RelativePath,
    },
    launch::{LaunchActivity, LaunchActivityId, LaunchActivityStatus, LaunchAdapter},
};
use thiserror::Error;

use crate::{
    installation::{InstallationLibraryError, InstallationStore},
    package_preparation::{PackagePreparationError, PackagePreparationStore},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchInstallationRequest {
    pub installation_id: InstallationId,
    pub activity_id: LaunchActivityId,
    pub attempted_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchExecutionPlan {
    pub installation_id: InstallationId,
    pub root_path: String,
    pub action: LaunchActionKind,
    pub relative_target: RelativePath,
    pub supported_platforms: Vec<InstallationPlatform>,
    pub expected_sha256: Option<String>,
}

pub struct LaunchExecutionResult {
    pub adapter: LaunchAdapter,
    pub process: Box<dyn ManagedLaunchProcess>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchProcessExit {
    pub exit_code: Option<i32>,
}

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("launch request is missing {0}")]
    InvalidRequest(&'static str),
    #[error("installation was not found: {0}")]
    NotFound(String),
    #[error("installation must be ready before it can be launched")]
    NeedsReview,
    #[error("installation must be explicitly reviewed before it can be launched")]
    NotReviewed,
    #[error("installation has no explicit launch action")]
    MissingAction,
    #[error("this checkpoint supports executable launch actions only")]
    UnsupportedAction,
    #[error("an executable launch action must target a relative file")]
    InvalidTarget,
    #[error("the selected launch target is not detected executable content")]
    MissingContentTarget,
    #[error("the selected launch target is ignored by the installation review")]
    IgnoredTarget,
    #[error("launch adapter failed: {0}")]
    Adapter(String),
    #[error("launch activity persistence failed: {0}")]
    Persistence(String),
    #[error("this installation is already running ({0})")]
    AlreadyRunning(String),
    #[error("launch activity was not found: {0}")]
    ActivityNotFound(String),
    #[error("the selected launch is no longer running")]
    NotRunning,
    #[error("the selected process is not owned by this launcher session")]
    NotOwned,
    #[error("launch supervisor failed: {0}")]
    Supervisor(String),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
    #[error(transparent)]
    Package(#[from] PackagePreparationError),
}

impl LaunchError {
    pub fn adapter(error: impl std::fmt::Display) -> Self {
        Self::Adapter(error.to_string())
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait LaunchExecutor: Send + Sync {
    fn execute(&self, plan: &LaunchExecutionPlan) -> Result<LaunchExecutionResult, LaunchError>;
}

pub trait ManagedLaunchProcess: Send {
    fn process_id(&self) -> u32;
    fn try_wait(&mut self) -> Result<Option<LaunchProcessExit>, LaunchError>;
    fn terminate(&mut self) -> Result<(), LaunchError>;
    fn wait(&mut self) -> Result<LaunchProcessExit, LaunchError>;
}

pub trait LaunchClock: Send + Sync {
    fn now(&self) -> String;
}

pub trait LaunchActivityStore: Send + Sync {
    fn begin_launch_activity(&self, activity: &LaunchActivity) -> Result<(), LaunchError>;
    fn save_launch_activity(&self, activity: &LaunchActivity) -> Result<(), LaunchError>;
    fn read_launch_activity(
        &self,
        activity_id: &LaunchActivityId,
    ) -> Result<Option<LaunchActivity>, LaunchError>;
    fn list_launch_activities(
        &self,
        installation_id: Option<&InstallationId>,
        limit: u32,
    ) -> Result<Vec<LaunchActivity>, LaunchError>;
    fn interrupt_active_launches(
        &self,
        interrupted_at: &str,
        reason: &str,
    ) -> Result<u64, LaunchError>;
}

pub struct LaunchService {
    installations: Arc<dyn InstallationStore>,
    preparations: Arc<dyn PackagePreparationStore>,
    executor: Arc<dyn LaunchExecutor>,
    activities: Arc<dyn LaunchActivityStore>,
    clock: Arc<dyn LaunchClock>,
    active: Arc<Mutex<BTreeMap<LaunchActivityId, ActiveLaunch>>>,
    poll_interval: Duration,
}

#[derive(Clone)]
struct ActiveLaunch {
    process_id: u32,
    commands: Sender<ProcessCommand>,
}

enum ProcessCommand {
    Stop {
        requested_at: String,
        response: Sender<Result<(), LaunchError>>,
    },
}

impl LaunchService {
    pub fn new(
        installations: Arc<dyn InstallationStore>,
        preparations: Arc<dyn PackagePreparationStore>,
        executor: Arc<dyn LaunchExecutor>,
        activities: Arc<dyn LaunchActivityStore>,
        clock: Arc<dyn LaunchClock>,
    ) -> Self {
        Self::with_poll_interval(
            installations,
            preparations,
            executor,
            activities,
            clock,
            Duration::from_millis(200),
        )
    }

    fn with_poll_interval(
        installations: Arc<dyn InstallationStore>,
        preparations: Arc<dyn PackagePreparationStore>,
        executor: Arc<dyn LaunchExecutor>,
        activities: Arc<dyn LaunchActivityStore>,
        clock: Arc<dyn LaunchClock>,
        poll_interval: Duration,
    ) -> Self {
        Self {
            installations,
            preparations,
            executor,
            activities,
            clock,
            active: Arc::new(Mutex::new(BTreeMap::new())),
            poll_interval,
        }
    }

    pub fn reconcile_after_restart(&self) -> Result<u64, LaunchError> {
        self.activities.interrupt_active_launches(
            &self.clock.now(),
            "the launcher restarted before the process exit was observed",
        )
    }

    pub fn launch(
        &self,
        request: LaunchInstallationRequest,
    ) -> Result<LaunchActivity, LaunchError> {
        validate_request(&request)?;
        let installation = self
            .installations
            .read(&request.installation_id)?
            .ok_or_else(|| LaunchError::NotFound(request.installation_id.0.clone()))?;

        let plan = match self.build_plan(&installation) {
            Ok(plan) => plan,
            Err(error) => {
                self.record_preflight_failure(&request, &installation, &error)?;
                return Err(error);
            }
        };

        let mut activity = LaunchActivity {
            id: request.activity_id.clone(),
            installation_id: request.installation_id.clone(),
            action: Some(plan.action),
            target_path: Some(plan.relative_target.to_string()),
            adapter: None,
            status: LaunchActivityStatus::Starting,
            process_id: None,
            error: None,
            attempted_at: request.attempted_at.clone(),
            started_at: None,
            ended_at: None,
            duration_ms: None,
            exit_code: None,
            stop_requested_at: None,
        };
        self.activities.begin_launch_activity(&activity)?;

        let mut execution = match self.executor.execute(&plan) {
            Ok(execution) => execution,
            Err(error) => {
                activity.status = LaunchActivityStatus::Failed;
                activity.error = Some(error.to_string());
                activity.ended_at = Some(self.clock.now());
                self.activities.save_launch_activity(&activity)?;
                return Err(error);
            }
        };

        let process_id = execution.process.process_id();
        let started_at = self.clock.now();
        activity.adapter = Some(execution.adapter);
        activity.status = LaunchActivityStatus::Running;
        activity.process_id = Some(process_id);
        activity.started_at = Some(started_at);
        if let Err(error) = self.activities.save_launch_activity(&activity) {
            let cleanup_error = terminate_and_reap(execution.process.as_mut()).err();
            let failure = match cleanup_error {
                Some(cleanup_error) => {
                    format!("{error}; process cleanup also failed: {cleanup_error}")
                }
                None => error.to_string(),
            };
            activity.status = LaunchActivityStatus::Failed;
            activity.error = Some(failure);
            activity.ended_at = Some(self.clock.now());
            activity.duration_ms = Some(0);
            if let Err(terminal_error) = self.activities.save_launch_activity(&activity) {
                return Err(LaunchError::persistence(format!(
                    "{error}; terminal launch state also failed to persist: {terminal_error}"
                )));
            }
            return Err(error);
        }

        self.supervise(activity.clone(), execution.process)?;
        Ok(activity)
    }

    pub fn stop(&self, activity_id: &LaunchActivityId) -> Result<LaunchActivity, LaunchError> {
        let active = self
            .active
            .lock()
            .map_err(|error| LaunchError::Supervisor(error.to_string()))?
            .get(activity_id)
            .cloned();
        let Some(active) = active else {
            let activity = self
                .activities
                .read_launch_activity(activity_id)?
                .ok_or_else(|| LaunchError::ActivityNotFound(activity_id.0.clone()))?;
            return if activity.status.is_active() {
                Err(LaunchError::NotOwned)
            } else {
                Err(LaunchError::NotRunning)
            };
        };
        let (response_tx, response_rx) = mpsc::channel();
        active
            .commands
            .send(ProcessCommand::Stop {
                requested_at: self.clock.now(),
                response: response_tx,
            })
            .map_err(|_| LaunchError::NotRunning)?;
        match response_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => result?,
            Err(RecvTimeoutError::Timeout) => {
                return Err(LaunchError::Supervisor(
                    "timed out while stopping the process".to_owned(),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => return Err(LaunchError::NotRunning),
        }
        self.activities
            .read_launch_activity(activity_id)?
            .ok_or_else(|| LaunchError::ActivityNotFound(activity_id.0.clone()))
    }

    pub fn recent(&self, limit: u32) -> Result<Vec<LaunchActivity>, LaunchError> {
        self.activities
            .list_launch_activities(None, normalized_history_limit(limit))
    }

    pub fn history(
        &self,
        installation_id: &InstallationId,
        limit: u32,
    ) -> Result<Vec<LaunchActivity>, LaunchError> {
        self.activities
            .list_launch_activities(Some(installation_id), normalized_history_limit(limit))
    }

    fn supervise(
        &self,
        activity: LaunchActivity,
        process: Box<dyn ManagedLaunchProcess>,
    ) -> Result<(), LaunchError> {
        let (commands, receiver) = mpsc::channel();
        let process_id = process.process_id();
        let activity_id = activity.id.clone();
        let (start_sender, start_receiver) = mpsc::sync_channel(0);
        let activities = Arc::clone(&self.activities);
        let clock = Arc::clone(&self.clock);
        let active = Arc::clone(&self.active);
        let poll_interval = self.poll_interval;
        if let Err(error) = thread::Builder::new()
            .name(format!("launch-{}", activity.id.0))
            .spawn(move || {
                if let Ok((activity, process)) = start_receiver.recv() {
                    monitor_process(
                        activity,
                        process,
                        receiver,
                        activities,
                        clock,
                        active,
                        poll_interval,
                    );
                }
            })
        {
            return Err(self.record_supervision_failure(
                activity,
                process,
                format!("could not start process monitor: {error}"),
            ));
        }

        match self.active.lock() {
            Ok(mut active) => {
                active.insert(
                    activity_id.clone(),
                    ActiveLaunch {
                        process_id,
                        commands,
                    },
                );
            }
            Err(error) => {
                drop(start_sender);
                return Err(self.record_supervision_failure(
                    activity,
                    process,
                    format!("could not register process monitor: {error}"),
                ));
            }
        }

        match start_sender.send((activity, process)) {
            Ok(()) => Ok(()),
            Err(error) => {
                remove_active(&self.active, &activity_id, process_id);
                let (activity, process) = error.0;
                Err(self.record_supervision_failure(
                    activity,
                    process,
                    "process monitor stopped before supervision began".to_owned(),
                ))
            }
        }
    }

    fn record_supervision_failure(
        &self,
        mut activity: LaunchActivity,
        mut process: Box<dyn ManagedLaunchProcess>,
        reason: String,
    ) -> LaunchError {
        let cleanup_error = terminate_and_reap(process.as_mut()).err();
        let reason = match cleanup_error {
            Some(error) => format!("{reason}; process cleanup also failed: {error}"),
            None => reason,
        };
        activity.status = LaunchActivityStatus::Failed;
        activity.error = Some(reason.clone());
        activity.ended_at = Some(self.clock.now());
        activity.duration_ms = Some(0);
        match self.activities.save_launch_activity(&activity) {
            Ok(()) => LaunchError::Supervisor(reason),
            Err(error) => LaunchError::Supervisor(format!(
                "{reason}; terminal launch state also failed to persist: {error}"
            )),
        }
    }

    fn build_plan(&self, installation: &Installation) -> Result<LaunchExecutionPlan, LaunchError> {
        if installation.status != InstallationStatus::Ready {
            return Err(LaunchError::NeedsReview);
        }
        if installation.overrides.reviewed_at.is_none() {
            return Err(LaunchError::NotReviewed);
        }

        if let Some(prepared) = self.preparations.read_prepared_package(&installation.id)? {
            let action = prepared
                .preferred_action
                .ok_or(LaunchError::MissingAction)?;
            if action.action != LaunchActionKind::LaunchExecutable {
                return Err(LaunchError::UnsupportedAction);
            }
            return Ok(LaunchExecutionPlan {
                installation_id: installation.id.clone(),
                root_path: prepared.destination_root,
                action: action.action,
                relative_target: action.relative_path,
                supported_platforms: action.supported_platforms,
                expected_sha256: action.expected_sha256,
            });
        }

        let selected = installation
            .overrides
            .preferred_action
            .as_ref()
            .ok_or(LaunchError::MissingAction)?;
        if selected.action != LaunchActionKind::LaunchExecutable {
            return Err(LaunchError::UnsupportedAction);
        }
        let LaunchTarget::RelativePath(target) = &selected.target else {
            return Err(LaunchError::InvalidTarget);
        };
        installation
            .detection
            .content_items
            .iter()
            .find(|item| item.relative_path == *target)
            .filter(|item| item.media_type == MediaType::Executable)
            .ok_or(LaunchError::MissingContentTarget)?;
        if installation
            .overrides
            .content_items
            .iter()
            .any(|item| item.relative_path == *target && item.ignored)
        {
            return Err(LaunchError::IgnoredTarget);
        }
        let supported_platforms = installation
            .detection
            .launch_candidates
            .iter()
            .find(|candidate| {
                candidate.action == selected.action && candidate.target == selected.target
            })
            .map(|candidate| candidate.supported_platforms.clone())
            .unwrap_or_else(|| inferred_platforms(installation, target));

        Ok(LaunchExecutionPlan {
            installation_id: installation.id.clone(),
            root_path: installation.root_path.clone(),
            action: selected.action,
            relative_target: target.clone(),
            supported_platforms,
            expected_sha256: None,
        })
    }

    fn record_preflight_failure(
        &self,
        request: &LaunchInstallationRequest,
        installation: &Installation,
        error: &LaunchError,
    ) -> Result<(), LaunchError> {
        let (action, target_path) = launch_hint(installation);
        self.activities.save_launch_activity(&LaunchActivity {
            id: request.activity_id.clone(),
            installation_id: request.installation_id.clone(),
            action,
            target_path,
            adapter: None,
            status: LaunchActivityStatus::Failed,
            process_id: None,
            error: Some(error.to_string()),
            attempted_at: request.attempted_at.clone(),
            started_at: None,
            ended_at: Some(request.attempted_at.clone()),
            duration_ms: None,
            exit_code: None,
            stop_requested_at: None,
        })
    }
}

fn monitor_process(
    mut activity: LaunchActivity,
    mut process: Box<dyn ManagedLaunchProcess>,
    commands: mpsc::Receiver<ProcessCommand>,
    activities: Arc<dyn LaunchActivityStore>,
    clock: Arc<dyn LaunchClock>,
    active: Arc<Mutex<BTreeMap<LaunchActivityId, ActiveLaunch>>>,
    poll_interval: Duration,
) {
    let started = Instant::now();
    let mut stop_requested = false;
    loop {
        if let Ok(ProcessCommand::Stop {
            requested_at,
            response,
        }) = commands.try_recv()
        {
            let result = process.terminate();
            if result.is_ok() {
                stop_requested = true;
                activity.status = LaunchActivityStatus::Stopping;
                activity.stop_requested_at = Some(requested_at);
                if let Err(error) = activities.save_launch_activity(&activity) {
                    let _ = response.send(Err(error));
                } else {
                    let _ = response.send(Ok(()));
                }
            } else {
                let _ = response.send(result);
            }
        }

        match process.try_wait() {
            Ok(Some(outcome)) => {
                activity.ended_at = Some(clock.now());
                activity.duration_ms = Some(duration_millis(started.elapsed()));
                activity.exit_code = outcome.exit_code;
                if stop_requested {
                    activity.status = LaunchActivityStatus::Stopped;
                    activity.error = None;
                } else if outcome.exit_code == Some(0) {
                    activity.status = LaunchActivityStatus::Exited;
                    activity.error = None;
                } else {
                    activity.status = LaunchActivityStatus::Failed;
                    activity.error = Some(match outcome.exit_code {
                        Some(code) => format!("process exited with code {code}"),
                        None => "process terminated without an exit code".to_owned(),
                    });
                }
                persist_terminal_activity(&activities, &activity, poll_interval);
                remove_active(&active, &activity.id, process.process_id());
                return;
            }
            Ok(None) => thread::sleep(poll_interval),
            Err(error) => {
                let cleanup_error = terminate_and_reap(process.as_mut()).err();
                activity.status = LaunchActivityStatus::Failed;
                activity.error = Some(match cleanup_error {
                    Some(cleanup_error) => {
                        format!("{error}; process cleanup also failed: {cleanup_error}")
                    }
                    None => error.to_string(),
                });
                activity.ended_at = Some(clock.now());
                activity.duration_ms = Some(duration_millis(started.elapsed()));
                persist_terminal_activity(&activities, &activity, poll_interval);
                remove_active(&active, &activity.id, process.process_id());
                return;
            }
        }
    }
}

fn persist_terminal_activity(
    activities: &Arc<dyn LaunchActivityStore>,
    activity: &LaunchActivity,
    retry_interval: Duration,
) {
    for attempt in 1..=3 {
        match activities.save_launch_activity(activity) {
            Ok(()) => return,
            Err(_) if attempt < 3 => thread::sleep(retry_interval),
            Err(error) => eprintln!(
                "could not persist terminal launch activity {} after {attempt} attempts: {error}",
                activity.id.0
            ),
        }
    }
}

fn terminate_and_reap(process: &mut dyn ManagedLaunchProcess) -> Result<(), LaunchError> {
    process.terminate()?;
    process.wait().map(|_| ())
}

fn remove_active(
    active: &Mutex<BTreeMap<LaunchActivityId, ActiveLaunch>>,
    activity_id: &LaunchActivityId,
    process_id: u32,
) {
    if let Ok(mut active) = active.lock()
        && active
            .get(activity_id)
            .is_some_and(|launch| launch.process_id == process_id)
    {
        active.remove(activity_id);
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn normalized_history_limit(limit: u32) -> u32 {
    limit.clamp(1, 100)
}

fn validate_request(request: &LaunchInstallationRequest) -> Result<(), LaunchError> {
    if request.installation_id.0.trim().is_empty() {
        return Err(LaunchError::InvalidRequest("installation ID"));
    }
    if request.activity_id.0.trim().is_empty() {
        return Err(LaunchError::InvalidRequest("activity ID"));
    }
    if request.attempted_at.trim().is_empty() {
        return Err(LaunchError::InvalidRequest("attempt timestamp"));
    }
    Ok(())
}

fn inferred_platforms(
    installation: &Installation,
    target: &RelativePath,
) -> Vec<InstallationPlatform> {
    if target.as_str().to_ascii_lowercase().ends_with(".exe") {
        vec![InstallationPlatform::Windows]
    } else {
        vec![installation.platform]
    }
}

fn launch_hint(installation: &Installation) -> (Option<LaunchActionKind>, Option<String>) {
    installation
        .overrides
        .preferred_action
        .as_ref()
        .map(|selection| {
            let target = match &selection.target {
                LaunchTarget::InstallationRoot => None,
                LaunchTarget::RelativePath(path) => Some(path.to_string()),
            };
            (Some(selection.action), target)
        })
        .unwrap_or((None, None))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use dla_domain::{
        installation::{
            CatalogIdentity, ContentItem, InferenceConfidence, InstallationDetection,
            InstallationOverrides, LaunchCandidate, LaunchCandidateId, ManualLaunchSelection,
        },
        package::{
            ArchiveRetentionPolicy, PackageLaunchCandidate, PackageSourceSet, PackageSourceSetKind,
            PreparedPackageInstallation,
        },
        scanner::ScanMatchConfidence,
    };

    use super::*;

    struct MemoryInstallationStore {
        installation: Installation,
    }

    impl InstallationStore for MemoryInstallationStore {
        fn create(&self, _installation: &Installation) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }

        fn create_or_refresh(
            &self,
            _installation: &Installation,
        ) -> Result<Installation, InstallationLibraryError> {
            unreachable!()
        }

        fn read(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<Installation>, InstallationLibraryError> {
            Ok((self.installation.id == *installation_id).then(|| self.installation.clone()))
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(vec![self.installation.clone()])
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
            _installation_id: &InstallationId,
            _overrides: &InstallationOverrides,
            _status: InstallationStatus,
            _updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            unreachable!()
        }
    }

    struct MemoryPreparationStore {
        prepared: Option<PreparedPackageInstallation>,
    }

    impl PackagePreparationStore for MemoryPreparationStore {
        fn read_prepared_package(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<PreparedPackageInstallation>, PackagePreparationError> {
            Ok(self
                .prepared
                .as_ref()
                .filter(|prepared| prepared.installation_id == *installation_id)
                .cloned())
        }

        fn save_prepared_package(
            &self,
            _prepared: &PreparedPackageInstallation,
        ) -> Result<(), PackagePreparationError> {
            unreachable!()
        }
    }

    struct RecordingExecutor {
        plans: Mutex<Vec<LaunchExecutionPlan>>,
        failure: Option<String>,
        behavior: ProcessBehavior,
    }

    #[derive(Clone, Copy)]
    enum ProcessBehavior {
        ExitAfter { polls: usize, exit_code: i32 },
        LongRunning,
    }

    struct FakeProcess {
        polls_remaining: Option<usize>,
        exit_code: i32,
        terminated: bool,
    }

    impl FakeProcess {
        fn new(behavior: ProcessBehavior) -> Self {
            match behavior {
                ProcessBehavior::ExitAfter { polls, exit_code } => Self {
                    polls_remaining: Some(polls),
                    exit_code,
                    terminated: false,
                },
                ProcessBehavior::LongRunning => Self {
                    polls_remaining: None,
                    exit_code: 0,
                    terminated: false,
                },
            }
        }
    }

    impl ManagedLaunchProcess for FakeProcess {
        fn process_id(&self) -> u32 {
            42
        }

        fn try_wait(&mut self) -> Result<Option<LaunchProcessExit>, LaunchError> {
            if self.terminated {
                return Ok(Some(LaunchProcessExit { exit_code: None }));
            }
            let Some(remaining) = self.polls_remaining.as_mut() else {
                return Ok(None);
            };
            if *remaining == 0 {
                Ok(Some(LaunchProcessExit {
                    exit_code: Some(self.exit_code),
                }))
            } else {
                *remaining -= 1;
                Ok(None)
            }
        }

        fn terminate(&mut self) -> Result<(), LaunchError> {
            self.terminated = true;
            Ok(())
        }

        fn wait(&mut self) -> Result<LaunchProcessExit, LaunchError> {
            if self.terminated {
                Ok(LaunchProcessExit { exit_code: None })
            } else if self.polls_remaining.is_some() {
                Ok(LaunchProcessExit {
                    exit_code: Some(self.exit_code),
                })
            } else {
                Err(LaunchError::Supervisor(
                    "long-running fixture must be terminated before waiting".to_owned(),
                ))
            }
        }
    }

    impl LaunchExecutor for RecordingExecutor {
        fn execute(
            &self,
            plan: &LaunchExecutionPlan,
        ) -> Result<LaunchExecutionResult, LaunchError> {
            self.plans.lock().expect("plans").push(plan.clone());
            if let Some(failure) = &self.failure {
                Err(LaunchError::adapter(failure))
            } else {
                Ok(LaunchExecutionResult {
                    adapter: LaunchAdapter::LinuxWine,
                    process: Box::new(FakeProcess::new(self.behavior)),
                })
            }
        }
    }

    #[derive(Default)]
    struct MemoryActivityStore {
        activities: Mutex<Vec<LaunchActivity>>,
        fail_running_saves: AtomicUsize,
    }

    impl LaunchActivityStore for MemoryActivityStore {
        fn begin_launch_activity(&self, activity: &LaunchActivity) -> Result<(), LaunchError> {
            let activities = self.activities.lock().expect("activities");
            if let Some(existing) = latest_for_installation(&activities, &activity.installation_id)
                && existing.status.is_active()
            {
                return Err(LaunchError::AlreadyRunning(existing.id.0.clone()));
            }
            drop(activities);
            self.save_launch_activity(activity)
        }

        fn save_launch_activity(&self, activity: &LaunchActivity) -> Result<(), LaunchError> {
            if activity.status == LaunchActivityStatus::Running
                && self
                    .fail_running_saves
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                        remaining.checked_sub(1)
                    })
                    .is_ok()
            {
                return Err(LaunchError::persistence(
                    "synthetic running-state write failure",
                ));
            }
            self.activities
                .lock()
                .expect("activities")
                .push(activity.clone());
            Ok(())
        }

        fn read_launch_activity(
            &self,
            activity_id: &LaunchActivityId,
        ) -> Result<Option<LaunchActivity>, LaunchError> {
            Ok(self
                .activities
                .lock()
                .expect("activities")
                .iter()
                .rev()
                .find(|activity| activity.id == *activity_id)
                .cloned())
        }

        fn list_launch_activities(
            &self,
            installation_id: Option<&InstallationId>,
            limit: u32,
        ) -> Result<Vec<LaunchActivity>, LaunchError> {
            let snapshots = self.activities.lock().expect("activities");
            let mut unique = BTreeMap::new();
            for activity in snapshots.iter().rev() {
                if installation_id.is_none_or(|id| activity.installation_id == *id) {
                    unique
                        .entry(activity.id.clone())
                        .or_insert_with(|| activity.clone());
                }
            }
            Ok(unique.into_values().take(limit as usize).collect())
        }

        fn interrupt_active_launches(
            &self,
            interrupted_at: &str,
            reason: &str,
        ) -> Result<u64, LaunchError> {
            let active: Vec<_> = self
                .list_launch_activities(None, 100)?
                .into_iter()
                .filter(|activity| activity.status.is_active())
                .collect();
            for mut activity in active.iter().cloned() {
                activity.status = LaunchActivityStatus::Interrupted;
                activity.error = Some(reason.to_owned());
                activity.ended_at = Some(interrupted_at.to_owned());
                self.save_launch_activity(&activity)?;
            }
            Ok(active.len() as u64)
        }
    }

    fn latest_for_installation<'a>(
        activities: &'a [LaunchActivity],
        installation_id: &InstallationId,
    ) -> Option<&'a LaunchActivity> {
        let latest_id = activities
            .iter()
            .rev()
            .find(|activity| activity.installation_id == *installation_id)?
            .id
            .clone();
        activities
            .iter()
            .rev()
            .find(|activity| activity.id == latest_id)
    }

    #[derive(Default)]
    struct TestClock {
        tick: AtomicUsize,
    }

    impl LaunchClock for TestClock {
        fn now(&self) -> String {
            let tick = self.tick.fetch_add(1, Ordering::Relaxed);
            format!("2026-08-09T01:00:{tick:02}Z")
        }
    }

    fn path(value: &str) -> RelativePath {
        RelativePath::parse(value).expect("fixture path")
    }

    fn installation(reviewed: bool, with_manual_action: bool) -> Installation {
        let executable = path("Game.exe");
        Installation {
            id: InstallationId("installation-1".to_owned()),
            scan_root_id: None,
            root_path: "/library/source".to_owned(),
            platform: InstallationPlatform::Windows,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: Some(CatalogIdentity {
                    work_code: "RJ00000001".to_owned(),
                    confidence: ScanMatchConfidence::Exact,
                    reason_codes: vec!["archive_sha256_match".to_owned()],
                }),
                suggested_status: InstallationStatus::Ready,
                content_items: vec![ContentItem {
                    relative_path: executable.clone(),
                    path_key: "game.exe".to_owned(),
                    media_type: MediaType::Executable,
                    size_bytes: Some(7),
                    modified_at: None,
                    confidence: InferenceConfidence::High,
                    reason_codes: vec!["file_extension".to_owned()],
                }],
                launch_candidates: vec![LaunchCandidate {
                    id: LaunchCandidateId("candidate-1".to_owned()),
                    action: LaunchActionKind::LaunchExecutable,
                    target: LaunchTarget::RelativePath(executable.clone()),
                    supported_platforms: vec![InstallationPlatform::Windows],
                    confidence: InferenceConfidence::High,
                    reason_codes: vec!["preferred_executable_name".to_owned()],
                }],
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                catalog_identity: None,
                custom_title: None,
                preferred_action: with_manual_action.then_some(ManualLaunchSelection {
                    action: LaunchActionKind::LaunchExecutable,
                    target: LaunchTarget::RelativePath(executable),
                }),
                content_items: vec![],
                reviewed_at: reviewed.then(|| "2026-08-09T00:00:00Z".to_owned()),
            },
            discovered_at: "2026-08-09T00:00:00Z".to_owned(),
            updated_at: "2026-08-09T00:00:00Z".to_owned(),
        }
    }

    fn prepared() -> PreparedPackageInstallation {
        PreparedPackageInstallation {
            installation_id: InstallationId("installation-1".to_owned()),
            destination_root: "/library/prepared".to_owned(),
            content_root: None,
            preferred_action: Some(PackageLaunchCandidate {
                action: LaunchActionKind::LaunchExecutable,
                relative_path: path("game/Game.exe"),
                supported_platforms: vec![InstallationPlatform::Windows],
                confidence: InferenceConfidence::High,
                reason_codes: vec!["verified_package_action".to_owned()],
                expected_sha256: Some("abc123".to_owned()),
            }),
            source_set: PackageSourceSet {
                kind: PackageSourceSetKind::SingleArchive,
                volumes: vec![],
            },
            archive_retention: ArchiveRetentionPolicy::Keep,
            sources_deleted: false,
            source_cleanup_error: None,
            installed_file_count: 1,
            installed_bytes: 7,
            prepared_at: "2026-08-09T00:00:00Z".to_owned(),
        }
    }

    fn request() -> LaunchInstallationRequest {
        LaunchInstallationRequest {
            installation_id: InstallationId("installation-1".to_owned()),
            activity_id: LaunchActivityId("launch-1".to_owned()),
            attempted_at: "2026-08-09T01:00:00Z".to_owned(),
        }
    }

    fn lifecycle_service(
        behavior: ProcessBehavior,
        activities: Arc<MemoryActivityStore>,
    ) -> LaunchService {
        LaunchService::with_poll_interval(
            Arc::new(MemoryInstallationStore {
                installation: installation(true, true),
            }),
            Arc::new(MemoryPreparationStore { prepared: None }),
            Arc::new(RecordingExecutor {
                plans: Mutex::new(vec![]),
                failure: None,
                behavior,
            }),
            activities,
            Arc::new(TestClock::default()),
            Duration::from_millis(1),
        )
    }

    fn wait_for_status(
        activities: &MemoryActivityStore,
        activity_id: &LaunchActivityId,
        expected: LaunchActivityStatus,
    ) -> LaunchActivity {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let activity = activities
                .read_launch_activity(activity_id)
                .expect("activity read")
                .expect("activity");
            if activity.status == expected {
                return activity;
            }
            assert!(
                Instant::now() < deadline,
                "timed out at {:?}",
                activity.status
            );
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn prepared_package_action_is_authoritative_and_records_started_activity() {
        let executor = Arc::new(RecordingExecutor {
            plans: Mutex::new(vec![]),
            failure: None,
            behavior: ProcessBehavior::LongRunning,
        });
        let activities = Arc::new(MemoryActivityStore::default());
        let service = LaunchService::new(
            Arc::new(MemoryInstallationStore {
                installation: installation(true, true),
            }),
            Arc::new(MemoryPreparationStore {
                prepared: Some(prepared()),
            }),
            executor.clone(),
            activities.clone(),
            Arc::new(TestClock::default()),
        );

        let running = service.launch(request()).expect("running activity");

        assert_eq!(running.target_path.as_deref(), Some("game/Game.exe"));
        assert_eq!(running.adapter, Some(LaunchAdapter::LinuxWine));
        let plans = executor.plans.lock().expect("plans");
        assert_eq!(plans[0].root_path, "/library/prepared");
        assert_eq!(plans[0].expected_sha256.as_deref(), Some("abc123"));
        let saved = activities.activities.lock().expect("activities");
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].status, LaunchActivityStatus::Starting);
        assert_eq!(saved[1].status, LaunchActivityStatus::Running);
        drop(saved);
        service
            .stop(&running.id)
            .expect("stop long-running process");
    }

    #[test]
    fn launch_requires_an_explicit_review_and_records_the_rejection() {
        let executor = Arc::new(RecordingExecutor {
            plans: Mutex::new(vec![]),
            failure: None,
            behavior: ProcessBehavior::LongRunning,
        });
        let activities = Arc::new(MemoryActivityStore::default());
        let service = LaunchService::new(
            Arc::new(MemoryInstallationStore {
                installation: installation(false, true),
            }),
            Arc::new(MemoryPreparationStore { prepared: None }),
            executor.clone(),
            activities.clone(),
            Arc::new(TestClock::default()),
        );

        assert!(matches!(
            service.launch(request()),
            Err(LaunchError::NotReviewed)
        ));
        assert!(executor.plans.lock().expect("plans").is_empty());
        let saved = activities.activities.lock().expect("activities");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].status, LaunchActivityStatus::Failed);
        assert!(
            saved[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("reviewed"))
        );
    }

    #[test]
    fn direct_installation_never_substitutes_a_generated_candidate_for_manual_consent() {
        let activities = Arc::new(MemoryActivityStore::default());
        let service = LaunchService::new(
            Arc::new(MemoryInstallationStore {
                installation: installation(true, false),
            }),
            Arc::new(MemoryPreparationStore { prepared: None }),
            Arc::new(RecordingExecutor {
                plans: Mutex::new(vec![]),
                failure: None,
                behavior: ProcessBehavior::LongRunning,
            }),
            activities.clone(),
            Arc::new(TestClock::default()),
        );

        assert!(matches!(
            service.launch(request()),
            Err(LaunchError::MissingAction)
        ));
        assert_eq!(activities.activities.lock().expect("activities").len(), 1);
    }

    #[test]
    fn adapter_failure_is_recorded_after_the_starting_attempt() {
        let activities = Arc::new(MemoryActivityStore::default());
        let service = LaunchService::new(
            Arc::new(MemoryInstallationStore {
                installation: installation(true, true),
            }),
            Arc::new(MemoryPreparationStore { prepared: None }),
            Arc::new(RecordingExecutor {
                plans: Mutex::new(vec![]),
                failure: Some("Wine is unavailable".to_owned()),
                behavior: ProcessBehavior::LongRunning,
            }),
            activities.clone(),
            Arc::new(TestClock::default()),
        );

        assert!(matches!(
            service.launch(request()),
            Err(LaunchError::Adapter(_))
        ));
        let saved = activities.activities.lock().expect("activities");
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].status, LaunchActivityStatus::Starting);
        assert_eq!(saved[1].status, LaunchActivityStatus::Failed);
        assert!(
            saved[1]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("Wine"))
        );
    }

    #[test]
    fn running_state_persistence_failure_terminates_and_closes_the_attempt() {
        let activities = Arc::new(MemoryActivityStore {
            activities: Mutex::new(vec![]),
            fail_running_saves: AtomicUsize::new(1),
        });
        let service = lifecycle_service(ProcessBehavior::LongRunning, activities.clone());

        assert!(matches!(
            service.launch(request()),
            Err(LaunchError::Persistence(_))
        ));
        let saved = activities.activities.lock().expect("activities");
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].status, LaunchActivityStatus::Starting);
        assert_eq!(saved[1].status, LaunchActivityStatus::Failed);
        assert_eq!(saved[1].duration_ms, Some(0));
        assert!(
            saved[1]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("running-state write failure"))
        );
    }

    #[test]
    fn normal_exit_is_reaped_and_recorded() {
        let activities = Arc::new(MemoryActivityStore::default());
        let service = lifecycle_service(
            ProcessBehavior::ExitAfter {
                polls: 1,
                exit_code: 0,
            },
            activities.clone(),
        );

        let running = service.launch(request()).expect("launch");
        let exited = wait_for_status(&activities, &running.id, LaunchActivityStatus::Exited);

        assert_eq!(exited.exit_code, Some(0));
        assert!(exited.ended_at.is_some());
        assert!(exited.duration_ms.is_some());
        assert!(matches!(
            service.stop(&running.id),
            Err(LaunchError::NotRunning)
        ));
    }

    #[test]
    fn immediate_nonzero_exit_is_recorded_as_failed() {
        let activities = Arc::new(MemoryActivityStore::default());
        let service = lifecycle_service(
            ProcessBehavior::ExitAfter {
                polls: 0,
                exit_code: 23,
            },
            activities.clone(),
        );

        let running = service.launch(request()).expect("launch");
        let failed = wait_for_status(&activities, &running.id, LaunchActivityStatus::Failed);

        assert_eq!(failed.exit_code, Some(23));
        assert_eq!(failed.error.as_deref(), Some("process exited with code 23"));
    }

    #[test]
    fn duplicate_launch_is_rejected_while_the_first_process_is_owned() {
        let activities = Arc::new(MemoryActivityStore::default());
        let service = lifecycle_service(ProcessBehavior::LongRunning, activities);
        let running = service.launch(request()).expect("first launch");
        let mut duplicate = request();
        duplicate.activity_id = LaunchActivityId("launch-2".to_owned());

        assert!(matches!(
            service.launch(duplicate),
            Err(LaunchError::AlreadyRunning(_))
        ));
        service.stop(&running.id).expect("stop process");
    }

    #[test]
    fn launcher_owned_process_can_be_stopped_and_reaped() {
        let activities = Arc::new(MemoryActivityStore::default());
        let service = lifecycle_service(ProcessBehavior::LongRunning, activities.clone());
        let running = service.launch(request()).expect("launch");

        let stopping = service.stop(&running.id).expect("stop request");
        assert!(matches!(
            stopping.status,
            LaunchActivityStatus::Stopping | LaunchActivityStatus::Stopped
        ));
        let stopped = wait_for_status(&activities, &running.id, LaunchActivityStatus::Stopped);
        assert!(stopped.stop_requested_at.is_some());
        assert!(stopped.ended_at.is_some());
    }

    #[test]
    fn restart_reconciliation_interrupts_unresolved_activity() {
        let activities = Arc::new(MemoryActivityStore::default());
        let unresolved = LaunchActivity {
            id: LaunchActivityId("launch-before-restart".to_owned()),
            installation_id: InstallationId("installation-1".to_owned()),
            action: Some(LaunchActionKind::LaunchExecutable),
            target_path: Some("Game.exe".to_owned()),
            adapter: Some(LaunchAdapter::LinuxWine),
            status: LaunchActivityStatus::Running,
            process_id: Some(77),
            error: None,
            attempted_at: "2026-08-09T00:00:00Z".to_owned(),
            started_at: Some("2026-08-09T00:00:01Z".to_owned()),
            ended_at: None,
            duration_ms: None,
            exit_code: None,
            stop_requested_at: None,
        };
        activities
            .save_launch_activity(&unresolved)
            .expect("unresolved activity");
        let service = lifecycle_service(ProcessBehavior::LongRunning, activities.clone());

        assert_eq!(service.reconcile_after_restart().expect("reconcile"), 1);
        let interrupted = activities
            .read_launch_activity(&unresolved.id)
            .expect("read")
            .expect("activity");
        assert_eq!(interrupted.status, LaunchActivityStatus::Interrupted);
        assert!(
            interrupted
                .error
                .as_deref()
                .is_some_and(|value| value.contains("restarted"))
        );
    }
}
