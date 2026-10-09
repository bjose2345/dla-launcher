use std::{path::Path, sync::Arc};

use dla_domain::{
    CatalogRom,
    delivery::{
        DeliveryArchive, DeliveryJob, DeliveryJobId, DeliveryPhase, DeliveryProblem,
        DeliveryProgress, DeliveryStatus,
    },
    installation::InstallationId,
};
use thiserror::Error;

use crate::catalog::CatalogReader;

/// The folder inside the install folder that holds downloads in progress,
/// one subfolder per work. It is on the install folder's volume so space
/// checks cover the archives and their extraction together.
pub const STAGING_FOLDER: &str = ".dla-downloads";

#[derive(Debug, Error)]
pub enum DeliveryError {
    #[error("the download was paused")]
    Paused,
    #[error("the download was cancelled")]
    Cancelled,
    #[error("{detail}")]
    Problem {
        problem: DeliveryProblem,
        detail: String,
    },
    #[error("delivery persistence failed: {0}")]
    Persistence(String),
}

impl DeliveryError {
    pub fn problem(problem: DeliveryProblem, detail: impl Into<String>) -> Self {
        Self::Problem {
            problem,
            detail: detail.into(),
        }
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

/// Lets a running job notice a pause or cancellation between steps.
pub trait DeliveryControl: Send + Sync {
    fn check(&self) -> Result<(), DeliveryError>;
}

pub trait DeliveryProgressSink: Send + Sync {
    fn publish(&self, progress: &DeliveryProgress);
    /// The running job was saved with a new status or a finished archive.
    fn job_saved(&self, job: &DeliveryJob);
}

pub trait DeliveryStore: Send + Sync {
    fn list_delivery_jobs(&self) -> Result<Vec<DeliveryJob>, DeliveryError>;
    fn save_delivery_job(&self, job: &DeliveryJob) -> Result<(), DeliveryError>;
    fn remove_delivery_job(&self, id: &DeliveryJobId) -> Result<(), DeliveryError>;
}

pub struct ReleaseLookup<'a> {
    pub work_code: &'a str,
    /// The work's release date and the release's update date, which suggest
    /// the archive months to search first.
    pub release_date: &'a str,
    pub update_date: &'a str,
    pub archives: &'a [DeliveryArchive],
}

pub trait DeliveryLocator: Send + Sync {
    /// The remote path of each archive, in order, found by its SHA-256.
    fn locate(
        &self,
        lookup: &ReleaseLookup<'_>,
        control: &dyn DeliveryControl,
    ) -> Result<Vec<String>, DeliveryError>;
}

pub trait DeliveryTransfer: Send + Sync {
    /// Downloads one located archive into `folder`, resuming a part saved
    /// there earlier, verifies its size and SHA-256, and renames it to its
    /// file name. Progress reports the bytes of this archive held locally.
    fn fetch(
        &self,
        archive: &DeliveryArchive,
        folder: &str,
        control: &dyn DeliveryControl,
        progress: &mut dyn FnMut(DeliveryPhase, u64),
    ) -> Result<(), DeliveryError>;

    /// Removes a work's staging folder and everything left in it.
    fn discard(&self, folder: &str) -> Result<(), DeliveryError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalInstallOutcome {
    Installed(InstallationId),
    /// In the Library, but preparation stopped for a decision, such as an
    /// occupied destination folder. The reason is shown with Review.
    NeedsReview(InstallationId, String),
}

/// Reports a phase with its processed and total bytes.
pub type InstallProgress<'a> = &'a (dyn Fn(DeliveryPhase, u64, u64) + Sync);

pub trait LocalInstaller: Send + Sync {
    /// Identifies the verified archives, creates the installation, and
    /// prepares it into the install folder, deleting the archives once
    /// committed.
    fn install(
        &self,
        work_code: &str,
        staging_folder: &str,
        install_folder: &str,
        archives: &[DeliveryArchive],
        control: &dyn DeliveryControl,
        progress: InstallProgress<'_>,
    ) -> Result<LocalInstallOutcome, DeliveryError>;
}

/// The install folder chosen once on this computer, per platform.
pub trait InstallFolderStore: Send + Sync {
    fn read_install_folder(&self, platform: &str) -> Result<Option<String>, DeliveryError>;
    fn save_install_folder(
        &self,
        platform: &str,
        folder: &str,
        now: &str,
    ) -> Result<(), DeliveryError>;
}

pub fn staging_folder(install_folder: &str, work_code: &str) -> String {
    Path::new(install_folder)
        .join(STAGING_FOLDER)
        .join(work_code)
        .to_string_lossy()
        .into_owned()
}

/// A work's newest catalog release: its version, update date and archives.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Release {
    pub version: String,
    pub update_date: String,
    pub archives: Vec<DeliveryArchive>,
}

/// The ROMs sharing the latest update date and version form the newest
/// release; a multipart release keeps all its volumes. Among equally dated
/// groups the later one in catalog order wins. Every archive needs a size and
/// a SHA-256, because nothing unverifiable is installed.
pub fn newest_release(roms: &[CatalogRom]) -> Result<Release, DeliveryError> {
    let mut groups: Vec<((&str, &str), Vec<&CatalogRom>)> = Vec::new();
    for rom in roms {
        let key = (rom.update_date.trim(), rom.version.trim());
        match groups.iter_mut().find(|(group, _)| *group == key) {
            Some((_, members)) => members.push(rom),
            None => groups.push((key, vec![rom])),
        }
    }
    let ((update_date, version), members) = groups
        .into_iter()
        .max_by(|(left, _), (right, _)| left.0.cmp(right.0))
        .ok_or_else(|| {
            DeliveryError::problem(
                DeliveryProblem::NotInArchive,
                "the catalog lists no archive for this work",
            )
        })?;
    let archives = members
        .into_iter()
        .map(|rom| {
            let sha256 = rom.sha256.trim().to_ascii_lowercase();
            let size = rom.size.trim().parse::<u64>().unwrap_or(0);
            let valid = !rom.name.trim().is_empty()
                && size > 0
                && sha256.len() == 64
                && sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
            if !valid {
                return Err(DeliveryError::problem(
                    DeliveryProblem::NotInArchive,
                    format!("the catalog has no size or SHA-256 for {}", rom.name),
                ));
            }
            Ok(DeliveryArchive {
                file_name: rom.name.trim().to_owned(),
                size,
                sha256,
                remote_path: None,
                complete: false,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Release {
        version: version.to_owned(),
        update_date: update_date.to_owned(),
        archives,
    })
}

pub struct DeliveryService {
    catalog: Arc<dyn CatalogReader>,
    store: Arc<dyn DeliveryStore>,
    folders: Arc<dyn InstallFolderStore>,
    locator: Arc<dyn DeliveryLocator>,
    transfer: Arc<dyn DeliveryTransfer>,
    installer: Arc<dyn LocalInstaller>,
}

impl DeliveryService {
    pub fn new(
        catalog: Arc<dyn CatalogReader>,
        store: Arc<dyn DeliveryStore>,
        folders: Arc<dyn InstallFolderStore>,
        locator: Arc<dyn DeliveryLocator>,
        transfer: Arc<dyn DeliveryTransfer>,
        installer: Arc<dyn LocalInstaller>,
    ) -> Self {
        Self {
            catalog,
            store,
            folders,
            locator,
            transfer,
            installer,
        }
    }

    pub fn install_folder(&self, platform: &str) -> Result<Option<String>, DeliveryError> {
        self.folders.read_install_folder(platform)
    }

    pub fn set_install_folder(
        &self,
        platform: &str,
        folder: &str,
        now: &str,
    ) -> Result<(), DeliveryError> {
        self.folders.save_install_folder(platform, folder, now)
    }

    /// Jobs a closed app left running are queued again; their downloaded
    /// archives and parts are kept, so they continue where they stopped.
    pub fn reconcile_after_restart(&self, now: &str) -> Result<usize, DeliveryError> {
        let mut count = 0;
        for mut job in self.store.list_delivery_jobs()? {
            if matches!(
                job.status,
                DeliveryStatus::Downloading | DeliveryStatus::Installing
            ) {
                job.status = DeliveryStatus::Queued;
                job.updated_at = now.to_owned();
                self.store.save_delivery_job(&job)?;
                count += 1;
            }
        }
        Ok(count)
    }

    /// Every job, oldest first.
    pub fn jobs(&self) -> Result<Vec<DeliveryJob>, DeliveryError> {
        let mut jobs = self.store.list_delivery_jobs()?;
        jobs.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.id.cmp(&right.id))
        });
        Ok(jobs)
    }

    fn find(&self, id: &DeliveryJobId) -> Result<DeliveryJob, DeliveryError> {
        self.store
            .list_delivery_jobs()?
            .into_iter()
            .find(|job| &job.id == id)
            .ok_or_else(|| DeliveryError::problem(DeliveryProblem::Unavailable, "no such download"))
    }

    /// Queues the newest release of a work. A work that already has an
    /// unfinished job gets that job back instead of a second one.
    pub fn queue(
        &self,
        work_code: &str,
        install_folder: &str,
        id: DeliveryJobId,
        now: &str,
    ) -> Result<DeliveryJob, DeliveryError> {
        let work_code = work_code.trim();
        if work_code.is_empty() || !work_code.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(DeliveryError::problem(
                DeliveryProblem::NotInArchive,
                "not a catalog work code",
            ));
        }
        if install_folder.trim().is_empty() {
            return Err(DeliveryError::problem(
                DeliveryProblem::InstallFolder,
                "choose an install folder first",
            ));
        }
        if let Some(existing) = self
            .jobs()?
            .into_iter()
            .find(|job| job.work_code.eq_ignore_ascii_case(work_code) && !job.status.is_finished())
        {
            return Ok(existing);
        }
        let detail = self
            .catalog
            .read(work_code)
            .map_err(|error| {
                DeliveryError::problem(DeliveryProblem::NotInArchive, error.to_string())
            })?
            .ok_or_else(|| {
                DeliveryError::problem(
                    DeliveryProblem::NotInArchive,
                    "the work is not in the catalog",
                )
            })?;
        let release = newest_release(&detail.roms)?;
        let job = DeliveryJob {
            id,
            work_code: detail.work.code.clone(),
            release_date: detail.work.release_date.clone(),
            version: release.version,
            update_date: release.update_date,
            archives: release.archives,
            install_folder: install_folder.to_owned(),
            status: DeliveryStatus::Queued,
            received_bytes: 0,
            problem: None,
            detail: None,
            installation_id: None,
            created_at: now.to_owned(),
            updated_at: now.to_owned(),
        };
        self.store.save_delivery_job(&job)?;
        Ok(job)
    }

    /// The oldest queued job, if any.
    pub fn next_queued(&self) -> Result<Option<DeliveryJob>, DeliveryError> {
        Ok(self
            .jobs()?
            .into_iter()
            .find(|job| job.status == DeliveryStatus::Queued))
    }

    /// Runs one job until it is installed, needs review, stops on a problem,
    /// or is paused or cancelled through `control`. The returned job is
    /// already saved.
    pub fn run(
        &self,
        id: &DeliveryJobId,
        control: &dyn DeliveryControl,
        sink: &dyn DeliveryProgressSink,
        now: &dyn Fn() -> String,
    ) -> Result<DeliveryJob, DeliveryError> {
        let mut job = self.find(id)?;
        if job.status != DeliveryStatus::Queued {
            return Ok(job);
        }
        job.status = DeliveryStatus::Downloading;
        job.problem = None;
        job.detail = None;
        job.updated_at = now();
        self.store.save_delivery_job(&job)?;
        sink.job_saved(&job);
        let staging = staging_folder(&job.install_folder, &job.work_code);
        match self.deliver(&mut job, &staging, control, sink) {
            Ok(outcome) => {
                let (status, installation) = match outcome {
                    LocalInstallOutcome::Installed(id) => (DeliveryStatus::Installed, id),
                    LocalInstallOutcome::NeedsReview(id, reason) => {
                        job.detail = Some(reason);
                        (DeliveryStatus::NeedsReview, id)
                    }
                };
                job.status = status;
                job.installation_id = Some(installation);
                // The archives are deleted once the installation commits;
                // anything left in staging is no longer needed.
                if status == DeliveryStatus::Installed {
                    self.transfer.discard(&staging).ok();
                }
            }
            Err(DeliveryError::Paused) => job.status = DeliveryStatus::Paused,
            Err(DeliveryError::Cancelled) => {
                job.status = DeliveryStatus::Cancelled;
                self.transfer.discard(&staging).ok();
            }
            Err(DeliveryError::Problem { problem, detail }) => {
                job.status = DeliveryStatus::Failed;
                job.problem = Some(problem);
                job.detail = Some(detail);
            }
            Err(error @ DeliveryError::Persistence(_)) => {
                job.status = DeliveryStatus::Failed;
                job.problem = Some(DeliveryProblem::Unavailable);
                job.detail = Some(error.to_string());
            }
        }
        job.updated_at = now();
        self.store.save_delivery_job(&job)?;
        Ok(job)
    }

    fn deliver(
        &self,
        job: &mut DeliveryJob,
        staging: &str,
        control: &dyn DeliveryControl,
        sink: &dyn DeliveryProgressSink,
    ) -> Result<LocalInstallOutcome, DeliveryError> {
        let total = job.total_bytes();
        let publish = |phase: DeliveryPhase, received: u64| {
            sink.publish(&DeliveryProgress {
                job_id: job.id.clone(),
                work_code: job.work_code.clone(),
                phase,
                received_bytes: received,
                total_bytes: total,
            })
        };
        if job
            .archives
            .iter()
            .any(|archive| archive.remote_path.is_none())
        {
            publish(DeliveryPhase::Locating, job.received_bytes);
            let paths = self.locator.locate(
                &ReleaseLookup {
                    work_code: &job.work_code,
                    release_date: &job.release_date,
                    update_date: &job.update_date,
                    archives: &job.archives,
                },
                control,
            )?;
            if paths.len() != job.archives.len() {
                return Err(DeliveryError::problem(
                    DeliveryProblem::NotInArchive,
                    "the DLA archive does not hold every file of this release",
                ));
            }
            for (archive, path) in job.archives.iter_mut().zip(paths) {
                archive.remote_path = Some(path);
            }
            self.store.save_delivery_job(job)?;
        }
        let mut done: u64 = job
            .archives
            .iter()
            .filter(|archive| archive.complete)
            .map(|archive| archive.size)
            .sum();
        for index in 0..job.archives.len() {
            if job.archives[index].complete {
                continue;
            }
            control.check()?;
            let archive = job.archives[index].clone();
            let progress = DeliveryProgress {
                job_id: job.id.clone(),
                work_code: job.work_code.clone(),
                phase: DeliveryPhase::Checking,
                received_bytes: done,
                total_bytes: total,
            };
            self.transfer
                .fetch(&archive, staging, control, &mut |phase, held| {
                    sink.publish(&DeliveryProgress {
                        phase,
                        received_bytes: done + held,
                        ..progress.clone()
                    })
                })?;
            done += archive.size;
            job.archives[index].complete = true;
            job.received_bytes = done;
            self.store.save_delivery_job(job)?;
            sink.job_saved(job);
        }
        control.check()?;
        job.status = DeliveryStatus::Installing;
        self.store.save_delivery_job(job)?;
        sink.job_saved(job);
        let installing = |phase: DeliveryPhase, processed: u64, of: u64| {
            sink.publish(&DeliveryProgress {
                job_id: job.id.clone(),
                work_code: job.work_code.clone(),
                phase,
                received_bytes: processed,
                total_bytes: of,
            })
        };
        installing(DeliveryPhase::Installing, 0, 0);
        self.installer.install(
            &job.work_code,
            staging,
            &job.install_folder,
            &job.archives,
            control,
            &installing,
        )
    }

    /// Puts a paused or failed job back in the queue.
    pub fn resume(&self, id: &DeliveryJobId, now: &str) -> Result<DeliveryJob, DeliveryError> {
        let mut job = self.find(id)?;
        if matches!(job.status, DeliveryStatus::Paused | DeliveryStatus::Failed) {
            job.status = DeliveryStatus::Queued;
            job.problem = None;
            job.detail = None;
            job.updated_at = now.to_owned();
            self.store.save_delivery_job(&job)?;
        }
        Ok(job)
    }

    /// Pauses a queued job. A running job is paused through its control.
    pub fn pause(&self, id: &DeliveryJobId, now: &str) -> Result<DeliveryJob, DeliveryError> {
        let mut job = self.find(id)?;
        if job.status == DeliveryStatus::Queued {
            job.status = DeliveryStatus::Paused;
            job.updated_at = now.to_owned();
            self.store.save_delivery_job(&job)?;
        }
        Ok(job)
    }

    /// Cancels a job that is not running and deletes what it downloaded. A
    /// running job is cancelled through its control.
    pub fn cancel(&self, id: &DeliveryJobId, now: &str) -> Result<DeliveryJob, DeliveryError> {
        let mut job = self.find(id)?;
        if matches!(
            job.status,
            DeliveryStatus::Queued | DeliveryStatus::Paused | DeliveryStatus::Failed
        ) {
            job.status = DeliveryStatus::Cancelled;
            job.updated_at = now.to_owned();
            self.transfer
                .discard(&staging_folder(&job.install_folder, &job.work_code))?;
            self.store.save_delivery_job(&job)?;
        }
        Ok(job)
    }

    /// Forgets a finished job.
    pub fn remove(&self, id: &DeliveryJobId) -> Result<(), DeliveryError> {
        let job = self.find(id)?;
        if !job.status.is_finished() {
            return Err(DeliveryError::problem(
                DeliveryProblem::Unavailable,
                "cancel the download before removing it",
            ));
        }
        self.store.remove_delivery_job(id)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use dla_domain::{CatalogWork, CatalogWorkDetail};

    use super::*;
    use crate::catalog::{
        CatalogContext, CatalogContextQuery, CatalogError, CatalogPage, CatalogQuery,
    };

    fn rom(name: &str, size: &str, sha: char, update_date: &str, version: &str) -> CatalogRom {
        CatalogRom {
            name: name.to_owned(),
            size: size.to_owned(),
            crc: String::new(),
            md5: String::new(),
            sha1: String::new(),
            sha256: sha.to_string().repeat(64),
            file_count: None,
            update_date: update_date.to_owned(),
            version: version.to_owned(),
        }
    }

    #[test]
    fn the_newest_release_keeps_all_its_volumes() {
        let original = [
            rom("RJ01055431.part1.exe", "100", 'a', "", ""),
            rom("RJ01055431.part2.rar", "50", 'b', "", ""),
        ];
        let release = newest_release(&original).unwrap();
        assert_eq!(release.archives.len(), 2);
        assert_eq!(release.archives[1].file_name, "RJ01055431.part2.rar");

        let updated = [
            original[0].clone(),
            original[1].clone(),
            rom("RJ01055431.zip", "120", 'C', "2026-02-21", "1.1"),
        ];
        let release = newest_release(&updated).unwrap();
        assert_eq!(release.update_date, "2026-02-21");
        assert_eq!(release.version, "1.1");
        assert_eq!(release.archives.len(), 1);
        assert_eq!(release.archives[0].sha256, "c".repeat(64));

        // Equally dated groups: the later one in catalog order.
        let same_day = [
            rom("RJ1.zip", "1", 'a', "2026-01-01", "1.0"),
            rom("RJ1_v2.zip", "1", 'b', "2026-01-01", "2.0"),
        ];
        assert_eq!(newest_release(&same_day).unwrap().version, "2.0");
    }

    #[test]
    fn a_release_without_sizes_or_hashes_is_refused() {
        for roms in [
            vec![],
            vec![rom("RJ1.zip", "", 'a', "", "")],
            vec![
                rom("RJ1.zip", "10", 'a', "", ""),
                CatalogRom {
                    sha256: "short".into(),
                    ..rom("RJ1.part2.rar", "1", 'a', "", "")
                },
            ],
        ] {
            match newest_release(&roms) {
                Err(DeliveryError::Problem {
                    problem: DeliveryProblem::NotInArchive,
                    ..
                }) => {}
                other => panic!("{roms:?}: {other:?}"),
            }
        }
    }

    struct Catalog(Vec<CatalogRom>);

    impl CatalogReader for Catalog {
        fn browse(&self, _: &CatalogQuery) -> Result<CatalogPage, CatalogError> {
            unimplemented!()
        }
        fn context(&self, _: &CatalogContextQuery) -> Result<CatalogContext, CatalogError> {
            unimplemented!()
        }
        fn read(&self, code: &str) -> Result<Option<CatalogWorkDetail>, CatalogError> {
            if code != "RJ01055431" {
                return Ok(None);
            }
            Ok(Some(CatalogWorkDetail {
                work: CatalogWork {
                    code: code.to_owned(),
                    source_code: code.to_owned(),
                    title: String::new(),
                    title_english: String::new(),
                    added_date: String::new(),
                    release_date: "2025-11-07".to_owned(),
                    updated_date: String::new(),
                    age_rating: String::new(),
                    release_type: String::new(),
                    main_image_urls: Vec::new(),
                    thumbnail_urls: Vec::new(),
                    circles: Vec::new(),
                    categories: Vec::new(),
                    tags: Vec::new(),
                    synthetic: false,
                    latest_update: None,
                },
                sample_image_urls: Vec::new(),
                file_formats: Vec::new(),
                supported_languages: Vec::new(),
                miscellanies: Vec::new(),
                roms: self.0.clone(),
                related_works: Vec::new(),
                rating: None,
                descriptions: Default::default(),
            }))
        }
        fn read_rom_contents(
            &self,
            _: &str,
            _: usize,
        ) -> Result<Option<dla_domain::CatalogRomContents>, CatalogError> {
            unimplemented!()
        }
    }

    #[derive(Default)]
    struct Store(Mutex<BTreeMap<DeliveryJobId, DeliveryJob>>);

    impl DeliveryStore for Store {
        fn list_delivery_jobs(&self) -> Result<Vec<DeliveryJob>, DeliveryError> {
            Ok(self.0.lock().unwrap().values().cloned().collect())
        }
        fn save_delivery_job(&self, job: &DeliveryJob) -> Result<(), DeliveryError> {
            self.0.lock().unwrap().insert(job.id.clone(), job.clone());
            Ok(())
        }
        fn remove_delivery_job(&self, id: &DeliveryJobId) -> Result<(), DeliveryError> {
            self.0.lock().unwrap().remove(id);
            Ok(())
        }
    }

    #[derive(Default)]
    struct Folders(Mutex<BTreeMap<String, String>>);

    impl InstallFolderStore for Folders {
        fn read_install_folder(&self, platform: &str) -> Result<Option<String>, DeliveryError> {
            Ok(self.0.lock().unwrap().get(platform).cloned())
        }
        fn save_install_folder(
            &self,
            platform: &str,
            folder: &str,
            _: &str,
        ) -> Result<(), DeliveryError> {
            self.0
                .lock()
                .unwrap()
                .insert(platform.to_owned(), folder.to_owned());
            Ok(())
        }
    }

    #[derive(Default)]
    struct Locator {
        calls: AtomicUsize,
    }

    impl DeliveryLocator for Locator {
        fn locate(
            &self,
            lookup: &ReleaseLookup<'_>,
            _: &dyn DeliveryControl,
        ) -> Result<Vec<String>, DeliveryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(lookup.release_date, "2025-11-07");
            Ok(lookup
                .archives
                .iter()
                .map(|archive| format!("/dlsite/2025-11/{}", archive.file_name))
                .collect())
        }
    }

    #[derive(Default)]
    struct Transfer {
        fetched: Mutex<Vec<String>>,
        discarded: Mutex<Vec<String>>,
        fail_with: Mutex<Option<DeliveryProblem>>,
    }

    impl DeliveryTransfer for Transfer {
        fn fetch(
            &self,
            archive: &DeliveryArchive,
            folder: &str,
            control: &dyn DeliveryControl,
            progress: &mut dyn FnMut(DeliveryPhase, u64),
        ) -> Result<(), DeliveryError> {
            assert!(archive.remote_path.is_some());
            assert!(folder.ends_with(&format!(
                "{STAGING_FOLDER}{}RJ01055431",
                std::path::MAIN_SEPARATOR
            )));
            if let Some(problem) = self.fail_with.lock().unwrap().take() {
                return Err(DeliveryError::problem(problem, "refused"));
            }
            control.check()?;
            progress(DeliveryPhase::Receiving, archive.size / 2);
            progress(DeliveryPhase::Verifying, archive.size);
            self.fetched.lock().unwrap().push(archive.file_name.clone());
            Ok(())
        }
        fn discard(&self, folder: &str) -> Result<(), DeliveryError> {
            self.discarded.lock().unwrap().push(folder.to_owned());
            Ok(())
        }
    }

    struct Installer(LocalInstallOutcome);

    impl LocalInstaller for Installer {
        fn install(
            &self,
            work_code: &str,
            _: &str,
            install_folder: &str,
            archives: &[DeliveryArchive],
            _: &dyn DeliveryControl,
            progress: InstallProgress<'_>,
        ) -> Result<LocalInstallOutcome, DeliveryError> {
            assert_eq!(work_code, "RJ01055431");
            assert_eq!(install_folder, "/games");
            assert!(archives.iter().all(|archive| archive.complete));
            progress(DeliveryPhase::Unpacking, 300, 600);
            Ok(self.0.clone())
        }
    }

    /// Pauses or cancels once `after` checks have passed.
    struct Control {
        checks: AtomicUsize,
        after: usize,
        then: fn() -> DeliveryError,
    }

    impl Control {
        fn never() -> Self {
            Self {
                checks: AtomicUsize::new(0),
                after: usize::MAX,
                then: || DeliveryError::Paused,
            }
        }
    }

    impl DeliveryControl for Control {
        fn check(&self) -> Result<(), DeliveryError> {
            if self.checks.fetch_add(1, Ordering::SeqCst) >= self.after {
                return Err((self.then)());
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct Sink(Mutex<Vec<DeliveryProgress>>, Mutex<Vec<DeliveryStatus>>);

    impl DeliveryProgressSink for Sink {
        fn publish(&self, progress: &DeliveryProgress) {
            self.0.lock().unwrap().push(progress.clone());
        }

        fn job_saved(&self, job: &DeliveryJob) {
            self.1.lock().unwrap().push(job.status);
        }
    }

    struct Fixture {
        service: DeliveryService,
        store: Arc<Store>,
        locator: Arc<Locator>,
        transfer: Arc<Transfer>,
    }

    fn fixture(outcome: LocalInstallOutcome) -> Fixture {
        let store = Arc::new(Store::default());
        let locator = Arc::new(Locator::default());
        let transfer = Arc::new(Transfer::default());
        let roms = vec![
            rom("RJ01055431.part1.exe", "100", 'a', "", ""),
            rom("RJ01055431.part2.rar", "50", 'b', "", ""),
        ];
        let service = DeliveryService::new(
            Arc::new(Catalog(roms)),
            store.clone(),
            Arc::new(Folders::default()),
            locator.clone(),
            transfer.clone(),
            Arc::new(Installer(outcome)),
        );
        Fixture {
            service,
            store,
            locator,
            transfer,
        }
    }

    fn id(value: &str) -> DeliveryJobId {
        DeliveryJobId(value.to_owned())
    }

    fn now() -> String {
        "2026-10-07T21:00:00Z".to_owned()
    }

    #[test]
    fn a_queued_work_is_found_downloaded_and_installed() {
        let f = fixture(LocalInstallOutcome::Installed(InstallationId(
            "installation".into(),
        )));
        let job = f
            .service
            .queue("RJ01055431", "/games", id("one"), "2026-10-07T20:00:00Z")
            .unwrap();
        assert_eq!(
            (job.status, job.total_bytes()),
            (DeliveryStatus::Queued, 150)
        );
        // Asking again returns the same unfinished job.
        assert_eq!(
            f.service
                .queue("rj01055431", "/games", id("two"), "x")
                .unwrap()
                .id,
            id("one")
        );
        assert_eq!(f.service.next_queued().unwrap().unwrap().id, id("one"));

        let sink = Sink::default();
        let job = f
            .service
            .run(&id("one"), &Control::never(), &sink, &now)
            .unwrap();
        assert_eq!(job.status, DeliveryStatus::Installed);
        assert_eq!(
            job.installation_id,
            Some(InstallationId("installation".into()))
        );
        assert_eq!(job.received_bytes, 150);
        assert!(job.archives.iter().all(|archive| archive.complete));
        assert_eq!(
            job.archives[0].remote_path.as_deref(),
            Some("/dlsite/2025-11/RJ01055431.part1.exe")
        );
        assert_eq!(
            *f.transfer.fetched.lock().unwrap(),
            ["RJ01055431.part1.exe", "RJ01055431.part2.rar"]
        );
        assert_eq!(f.transfer.discarded.lock().unwrap().len(), 1);
        let phases: Vec<_> = sink
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|p| (p.phase, p.received_bytes))
            .collect();
        assert_eq!(
            phases,
            [
                (DeliveryPhase::Locating, 0),
                (DeliveryPhase::Receiving, 50),
                (DeliveryPhase::Verifying, 100),
                (DeliveryPhase::Receiving, 125),
                (DeliveryPhase::Verifying, 150),
                (DeliveryPhase::Installing, 0),
                (DeliveryPhase::Unpacking, 300),
            ]
        );
        // Starting, each finished volume and installing are announced, so the
        // interface never shows a running job as waiting.
        assert_eq!(
            *sink.1.lock().unwrap(),
            [
                DeliveryStatus::Downloading,
                DeliveryStatus::Downloading,
                DeliveryStatus::Downloading,
                DeliveryStatus::Installing,
            ]
        );
        assert!(f.service.next_queued().unwrap().is_none());
        f.service.remove(&id("one")).unwrap();
        assert!(f.service.jobs().unwrap().is_empty());
    }

    #[test]
    fn a_paused_job_resumes_where_it_stopped() {
        let f = fixture(LocalInstallOutcome::Installed(InstallationId("i".into())));
        f.service
            .queue("RJ01055431", "/games", id("one"), "t")
            .unwrap();
        // The first archive passes its checks; the second is paused.
        let control = Control {
            checks: AtomicUsize::new(0),
            after: 2,
            then: || DeliveryError::Paused,
        };
        let job = f
            .service
            .run(&id("one"), &control, &Sink::default(), &now)
            .unwrap();
        assert_eq!(job.status, DeliveryStatus::Paused);
        assert_eq!(
            (job.archives[0].complete, job.archives[1].complete),
            (true, false)
        );
        assert_eq!(job.received_bytes, 100);
        assert!(f.service.remove(&id("one")).is_err());

        // Running a paused job does nothing until it is resumed.
        assert_eq!(
            f.service
                .run(&id("one"), &Control::never(), &Sink::default(), &now)
                .unwrap()
                .status,
            DeliveryStatus::Paused
        );
        assert_eq!(
            f.service.resume(&id("one"), "t").unwrap().status,
            DeliveryStatus::Queued
        );
        let job = f
            .service
            .run(&id("one"), &Control::never(), &Sink::default(), &now)
            .unwrap();
        assert_eq!(job.status, DeliveryStatus::Installed);
        assert_eq!(
            *f.transfer.fetched.lock().unwrap(),
            ["RJ01055431.part1.exe", "RJ01055431.part2.rar"]
        );
        assert_eq!(
            f.locator.calls.load(Ordering::SeqCst),
            1,
            "located only once"
        );
    }

    #[test]
    fn problems_are_kept_and_a_retry_continues() {
        let f = fixture(LocalInstallOutcome::NeedsReview(
            InstallationId("i".into()),
            "the destination folder is occupied".into(),
        ));
        f.service
            .queue("RJ01055431", "/games", id("one"), "t")
            .unwrap();
        *f.transfer.fail_with.lock().unwrap() = Some(DeliveryProblem::Allowance);
        let job = f
            .service
            .run(&id("one"), &Control::never(), &Sink::default(), &now)
            .unwrap();
        assert_eq!(
            (job.status, job.problem),
            (DeliveryStatus::Failed, Some(DeliveryProblem::Allowance))
        );
        assert_eq!(f.service.resume(&id("one"), "t").unwrap().problem, None);
        let job = f
            .service
            .run(&id("one"), &Control::never(), &Sink::default(), &now)
            .unwrap();
        assert_eq!(job.status, DeliveryStatus::NeedsReview);
        assert_eq!(
            job.detail.as_deref(),
            Some("the destination folder is occupied")
        );
        // A package that needs review keeps nothing to discard yet.
        assert!(f.transfer.discarded.lock().unwrap().is_empty());
    }

    #[test]
    fn cancelling_deletes_what_was_downloaded() {
        let f = fixture(LocalInstallOutcome::Installed(InstallationId("i".into())));
        f.service
            .queue("RJ01055431", "/games", id("one"), "t")
            .unwrap();
        let control = Control {
            checks: AtomicUsize::new(0),
            after: 1,
            then: || DeliveryError::Cancelled,
        };
        let job = f
            .service
            .run(&id("one"), &control, &Sink::default(), &now)
            .unwrap();
        assert_eq!(job.status, DeliveryStatus::Cancelled);
        assert_eq!(f.transfer.discarded.lock().unwrap().len(), 1);

        f.service
            .queue("RJ01055431", "/games", id("two"), "u")
            .unwrap();
        f.service.pause(&id("two"), "u").unwrap();
        assert_eq!(
            f.service.cancel(&id("two"), "u").unwrap().status,
            DeliveryStatus::Cancelled
        );
        assert_eq!(f.transfer.discarded.lock().unwrap().len(), 2);
        assert_eq!(f.store.0.lock().unwrap().len(), 2);
    }

    #[test]
    fn the_install_folder_is_kept_and_interrupted_jobs_continue() {
        let f = fixture(LocalInstallOutcome::Installed(InstallationId("i".into())));
        assert_eq!(f.service.install_folder("linux").unwrap(), None);
        f.service
            .set_install_folder("linux", "/games", "t")
            .unwrap();
        assert_eq!(
            f.service.install_folder("linux").unwrap().as_deref(),
            Some("/games")
        );

        f.service
            .queue("RJ01055431", "/games", id("one"), "t")
            .unwrap();
        let mut job = f.store.0.lock().unwrap()[&id("one")].clone();
        job.status = DeliveryStatus::Downloading;
        f.store.save_delivery_job(&job).unwrap();
        assert_eq!(f.service.reconcile_after_restart("u").unwrap(), 1);
        assert_eq!(f.service.next_queued().unwrap().unwrap().id, id("one"));
    }

    #[test]
    fn unknown_works_and_missing_folders_are_refused() {
        let f = fixture(LocalInstallOutcome::Installed(InstallationId("i".into())));
        for (code, folder, expected) in [
            ("RJ999", "/games", DeliveryProblem::NotInArchive),
            ("../RJ1", "/games", DeliveryProblem::NotInArchive),
            ("RJ01055431", " ", DeliveryProblem::InstallFolder),
        ] {
            match f.service.queue(code, folder, id("x"), "t") {
                Err(DeliveryError::Problem { problem, .. }) => assert_eq!(problem, expected),
                other => panic!("{code}: {other:?}"),
            }
        }
        assert!(f.service.jobs().unwrap().is_empty());
    }
}
