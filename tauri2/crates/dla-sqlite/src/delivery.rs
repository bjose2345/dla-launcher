use dla_application::delivery::{DeliveryError, DeliveryStore, InstallFolderStore};
use dla_domain::delivery::{DeliveryJob, DeliveryJobId};
use rusqlite::{OptionalExtension, params};

use crate::SqliteLibraryStore;

// A job is stored whole as JSON; the columns beside it serve ordering and
// inspection only.
impl DeliveryStore for SqliteLibraryStore {
    fn list_delivery_jobs(&self) -> Result<Vec<DeliveryJob>, DeliveryError> {
        let rows = self
            .with_connection(|connection| {
                let mut statement = connection.prepare(
                    "SELECT job_json FROM library_delivery_job ORDER BY created_at, job_id",
                )?;
                statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(DeliveryError::persistence)?;
        rows.iter()
            .map(|json| serde_json::from_str(json).map_err(DeliveryError::persistence))
            .collect()
    }

    fn save_delivery_job(&self, job: &DeliveryJob) -> Result<(), DeliveryError> {
        let json = serde_json::to_string(job).map_err(DeliveryError::persistence)?;
        let status = serde_json::to_value(job.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO library_delivery_job
                     (job_id, work_code, status, created_at, updated_at, job_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(job_id) DO UPDATE SET
                     work_code = excluded.work_code,
                     status = excluded.status,
                     updated_at = excluded.updated_at,
                     job_json = excluded.job_json",
                params![
                    job.id.0,
                    job.work_code,
                    status,
                    job.created_at,
                    job.updated_at,
                    json
                ],
            )?;
            Ok(())
        })
        .map_err(DeliveryError::persistence)
    }

    fn remove_delivery_job(&self, id: &DeliveryJobId) -> Result<(), DeliveryError> {
        self.with_connection(|connection| {
            connection.execute(
                "DELETE FROM library_delivery_job WHERE job_id = ?1",
                [&id.0],
            )?;
            Ok(())
        })
        .map_err(DeliveryError::persistence)
    }
}

impl InstallFolderStore for SqliteLibraryStore {
    fn read_install_folder(&self, platform: &str) -> Result<Option<String>, DeliveryError> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT display_path FROM library_install_folder_preference
                     WHERE platform = ?1",
                    [platform],
                    |row| row.get(0),
                )
                .optional()
        })
        .map_err(DeliveryError::persistence)
    }

    fn save_install_folder(
        &self,
        platform: &str,
        folder: &str,
        now: &str,
    ) -> Result<(), DeliveryError> {
        self.with_connection(|connection| {
            connection.execute(
                "INSERT INTO library_install_folder_preference (platform, display_path, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(platform) DO UPDATE SET
                     display_path = excluded.display_path,
                     updated_at = excluded.updated_at",
                params![platform, folder, now],
            )?;
            Ok(())
        })
        .map_err(DeliveryError::persistence)
    }
}

#[cfg(test)]
mod tests {
    use dla_domain::delivery::{DeliveryArchive, DeliveryStatus};

    use super::*;

    fn job(id: &str, created_at: &str) -> DeliveryJob {
        DeliveryJob {
            id: DeliveryJobId(id.to_owned()),
            work_code: "RJ01055431".to_owned(),
            release_date: "2025-11-07".to_owned(),
            version: String::new(),
            update_date: String::new(),
            archives: vec![DeliveryArchive {
                file_name: "RJ01055431.part1.exe".to_owned(),
                size: 100,
                sha256: "a".repeat(64),
                remote_path: None,
                complete: false,
            }],
            install_folder: "/games".to_owned(),
            status: DeliveryStatus::Queued,
            received_bytes: 0,
            problem: None,
            detail: None,
            installation_id: None,
            created_at: created_at.to_owned(),
            updated_at: created_at.to_owned(),
        }
    }

    #[test]
    fn jobs_and_the_install_folder_are_kept() {
        let directory = tempfile::tempdir().unwrap();
        let store = SqliteLibraryStore::open(&directory.path().join("library.sqlite")).unwrap();
        assert!(store.list_delivery_jobs().unwrap().is_empty());
        store
            .save_delivery_job(&job("b", "2026-10-07T20:00:01Z"))
            .unwrap();
        store
            .save_delivery_job(&job("a", "2026-10-07T20:00:00Z"))
            .unwrap();
        let mut updated = job("b", "2026-10-07T20:00:01Z");
        updated.status = DeliveryStatus::Paused;
        updated.archives[0].remote_path = Some("/dlsite/2025-11/RJ01055431.part1.exe".to_owned());
        store.save_delivery_job(&updated).unwrap();

        let reopened = SqliteLibraryStore::open(&directory.path().join("library.sqlite")).unwrap();
        let jobs = reopened.list_delivery_jobs().unwrap();
        assert_eq!(
            jobs.iter().map(|job| job.id.0.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(jobs[1], updated);
        reopened
            .remove_delivery_job(&DeliveryJobId("a".to_owned()))
            .unwrap();
        assert_eq!(reopened.list_delivery_jobs().unwrap().len(), 1);

        assert_eq!(reopened.read_install_folder("linux").unwrap(), None);
        reopened
            .save_install_folder("linux", "/games", "t1")
            .unwrap();
        reopened
            .save_install_folder("linux", "/mnt/games", "t2")
            .unwrap();
        assert_eq!(
            reopened.read_install_folder("linux").unwrap().as_deref(),
            Some("/mnt/games")
        );
        assert_eq!(reopened.read_install_folder("windows").unwrap(), None);
    }
}
