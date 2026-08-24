use std::{collections::HashSet, sync::Arc};

use dla_domain::{
    installation::{InstallationStatus, LaunchActionKind},
    library::{LibraryLaunchTotals, LibraryRecentActivity, LibraryShelves},
    media::MediaResume,
};
use thiserror::Error;

use crate::installation::{InstallationLibraryError, InstallationStore};

const RECENT_LIMIT: usize = 12;
const CONTINUE_LIMIT: usize = 8;
const NEVER_LAUNCHED_LIMIT: usize = 12;
const UNFINISHED_LIMIT: usize = 48;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LibraryActivitySnapshot {
    pub recent: Vec<LibraryRecentActivity>,
    pub resumes: Vec<MediaResume>,
    pub launch_totals: Vec<LibraryLaunchTotals>,
}

#[derive(Debug, Error)]
pub enum LibraryShelvesError {
    #[error("library activity persistence failed: {0}")]
    Persistence(String),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
}

impl LibraryShelvesError {
    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait LibraryActivityReader: Send + Sync {
    fn read_library_activity(&self) -> Result<LibraryActivitySnapshot, LibraryShelvesError>;
}

pub struct LibraryShelvesService {
    installations: Arc<dyn InstallationStore>,
    activity: Arc<dyn LibraryActivityReader>,
}

impl LibraryShelvesService {
    pub fn new(
        installations: Arc<dyn InstallationStore>,
        activity: Arc<dyn LibraryActivityReader>,
    ) -> Self {
        Self {
            installations,
            activity,
        }
    }

    pub fn read(&self) -> Result<LibraryShelves, LibraryShelvesError> {
        let mut installations = self.installations.list()?;
        installations.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let known = installations
            .iter()
            .map(|installation| installation.id.clone())
            .collect::<HashSet<_>>();
        let ready = installations
            .iter()
            .filter(|installation| {
                installation.status == InstallationStatus::Ready
                    && installation.overrides.reviewed_at.is_some()
            })
            .map(|installation| installation.id.clone())
            .collect::<HashSet<_>>();
        let mut snapshot = self.activity.read_library_activity()?;
        snapshot
            .recent
            .retain(|activity| known.contains(&activity.installation_id));
        snapshot.recent.sort_by(|left, right| {
            right
                .occurred_at
                .cmp(&left.occurred_at)
                .then_with(|| left.installation_id.cmp(&right.installation_id))
        });
        let engaged = snapshot
            .recent
            .iter()
            .map(|activity| activity.installation_id.clone())
            .collect::<HashSet<_>>();
        snapshot.resumes.retain(|resume| {
            !resume.completed
                && known.contains(&resume.installation_id)
                && ready.contains(&resume.installation_id)
        });
        snapshot.resumes.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.installation_id.cmp(&right.installation_id))
                .then_with(|| action_rank(left.action).cmp(&action_rank(right.action)))
        });
        let never_launched = installations
            .iter()
            .filter(|installation| {
                ready.contains(&installation.id) && !engaged.contains(&installation.id)
            })
            .take(NEVER_LAUNCHED_LIMIT)
            .map(|installation| installation.id.clone())
            .collect();
        snapshot
            .launch_totals
            .retain(|totals| known.contains(&totals.installation_id) && totals.launch_count > 0);
        snapshot
            .launch_totals
            .sort_by(|left, right| left.installation_id.cmp(&right.installation_id));
        Ok(LibraryShelves {
            launch_totals: snapshot.launch_totals,
            installations,
            recent: snapshot.recent.into_iter().take(RECENT_LIMIT).collect(),
            continue_items: snapshot
                .resumes
                .iter()
                .take(CONTINUE_LIMIT)
                .cloned()
                .collect(),
            never_launched,
            unfinished: snapshot
                .resumes
                .into_iter()
                .take(UNFINISHED_LIMIT)
                .collect(),
        })
    }
}

fn action_rank(action: LaunchActionKind) -> u8 {
    match action {
        LaunchActionKind::LaunchExecutable => 0,
        LaunchActionKind::PlayAudio => 1,
        LaunchActionKind::ReadImages => 2,
        LaunchActionKind::OpenDocument => 3,
        LaunchActionKind::PlayVideo => 4,
        LaunchActionKind::OpenArchive => 5,
        LaunchActionKind::OpenAndroidPackage => 6,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_domain::{
        installation::{
            Installation, InstallationDetection, InstallationId, InstallationOverrides,
            InstallationPlatform, LaunchActionKind, ManualCatalogIdentity, RelativePath,
        },
        library::{LibraryActivityKind, LibraryRecentActivity},
        media::MediaResume,
    };

    use super::*;

    struct MemoryInstallationStore {
        installations: Vec<Installation>,
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
            Ok(self
                .installations
                .iter()
                .find(|installation| installation.id == *installation_id)
                .cloned())
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(self.installations.clone())
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

    struct MemoryActivityReader {
        snapshot: Mutex<Option<LibraryActivitySnapshot>>,
    }

    impl LibraryActivityReader for MemoryActivityReader {
        fn read_library_activity(&self) -> Result<LibraryActivitySnapshot, LibraryShelvesError> {
            Ok(self
                .snapshot
                .lock()
                .expect("activity snapshot")
                .take()
                .expect("one read"))
        }
    }

    #[test]
    fn shelves_are_deterministic_views_of_real_activity_and_incomplete_media() {
        let recent = installation(
            "recent",
            InstallationStatus::Ready,
            true,
            "2026-08-01T00:00:00Z",
        );
        let media = installation(
            "media",
            InstallationStatus::Ready,
            true,
            "2026-08-02T00:00:00Z",
        );
        let never = installation(
            "never",
            InstallationStatus::Ready,
            true,
            "2026-08-03T00:00:00Z",
        );
        let unreviewed = installation(
            "unreviewed",
            InstallationStatus::NeedsReview,
            false,
            "2026-08-04T00:00:00Z",
        );
        let service = LibraryShelvesService::new(
            Arc::new(MemoryInstallationStore {
                installations: vec![recent, media, never, unreviewed],
            }),
            Arc::new(MemoryActivityReader {
                snapshot: Mutex::new(Some(LibraryActivitySnapshot {
                    launch_totals: vec![
                        totals("recent", 4, 900_000),
                        totals("never", 0, 0),
                        totals("missing", 7, 120_000),
                    ],
                    recent: vec![
                        activity(
                            "recent",
                            "2026-08-05T10:00:00Z",
                            LibraryActivityKind::ExecutableLaunch,
                        ),
                        activity(
                            "media",
                            "2026-08-06T10:00:00Z",
                            LibraryActivityKind::MediaSession,
                        ),
                        activity(
                            "missing",
                            "2026-08-07T10:00:00Z",
                            LibraryActivityKind::MediaSession,
                        ),
                    ],
                    resumes: vec![
                        resume("media", false, "2026-08-06T10:00:00Z"),
                        resume("recent", true, "2026-08-05T11:00:00Z"),
                        resume("unreviewed", false, "2026-08-07T10:00:00Z"),
                    ],
                })),
            }),
        );

        let shelves = service.read().expect("library shelves");

        assert_eq!(
            shelves
                .installations
                .iter()
                .map(|installation| installation.id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["unreviewed", "never", "media", "recent"]
        );
        assert_eq!(
            shelves
                .recent
                .iter()
                .map(|activity| activity.installation_id.0.as_str())
                .collect::<Vec<_>>(),
            vec!["media", "recent"]
        );
        assert_eq!(shelves.continue_items.len(), 1);
        assert_eq!(shelves.continue_items[0].installation_id.0, "media");
        assert_eq!(shelves.unfinished, shelves.continue_items);
        assert_eq!(
            shelves.never_launched,
            vec![InstallationId("never".to_owned())]
        );
        assert_eq!(shelves.launch_totals, vec![totals("recent", 4, 900_000)]);
    }

    fn totals(
        installation_id: &str,
        launch_count: u32,
        total_duration_ms: u64,
    ) -> LibraryLaunchTotals {
        LibraryLaunchTotals {
            installation_id: InstallationId(installation_id.to_owned()),
            launch_count,
            total_duration_ms,
        }
    }

    fn installation(
        id: &str,
        status: InstallationStatus,
        reviewed: bool,
        updated_at: &str,
    ) -> Installation {
        Installation {
            id: InstallationId(id.to_owned()),
            scan_root_id: None,
            root_path: format!("/library/{id}"),
            platform: InstallationPlatform::Linux,
            status,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: status,
                content_items: Vec::new(),
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                catalog_identity: reviewed.then(|| ManualCatalogIdentity::CatalogWork {
                    work_code: format!("RJ{id}"),
                }),
                custom_title: None,
                preferred_action: None,
                content_items: Vec::new(),
                reviewed_at: reviewed.then(|| updated_at.to_owned()),
            },
            discovered_at: updated_at.to_owned(),
            updated_at: updated_at.to_owned(),
        }
    }

    fn activity(
        installation_id: &str,
        occurred_at: &str,
        kind: LibraryActivityKind,
    ) -> LibraryRecentActivity {
        LibraryRecentActivity {
            installation_id: InstallationId(installation_id.to_owned()),
            action: Some(match kind {
                LibraryActivityKind::ExecutableLaunch => LaunchActionKind::LaunchExecutable,
                LibraryActivityKind::MediaSession => LaunchActionKind::PlayAudio,
            }),
            kind,
            occurred_at: occurred_at.to_owned(),
            active: false,
        }
    }

    fn resume(installation_id: &str, completed: bool, updated_at: &str) -> MediaResume {
        MediaResume {
            installation_id: InstallationId(installation_id.to_owned()),
            action: LaunchActionKind::PlayAudio,
            relative_path: RelativePath::parse("track.flac").expect("resume path"),
            position_ms: 12_000,
            duration_ms: Some(60_000),
            completed,
            updated_at: updated_at.to_owned(),
        }
    }
}
