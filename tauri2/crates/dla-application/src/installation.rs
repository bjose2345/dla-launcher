use std::sync::Arc;

use dla_domain::installation::{
    Installation, InstallationDetection, InstallationError, InstallationId, InstallationOverrides,
    InstallationStatus,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstallationLibraryError {
    #[error("installation was not found: {0}")]
    NotFound(String),
    #[error("invalid installation: {0}")]
    Invalid(#[from] InstallationError),
    #[error("installation persistence failed: {0}")]
    Persistence(String),
}

impl InstallationLibraryError {
    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait InstallationStore: Send + Sync {
    fn create(&self, installation: &Installation) -> Result<(), InstallationLibraryError>;
    fn create_or_refresh(
        &self,
        installation: &Installation,
    ) -> Result<Installation, InstallationLibraryError>;
    fn read(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<Installation>, InstallationLibraryError>;
    fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError>;
    fn find_by_work_code(
        &self,
        work_code: &str,
    ) -> Result<Vec<Installation>, InstallationLibraryError> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|installation| {
                installation
                    .effective_catalog_work_code()
                    .is_some_and(|code| code.eq_ignore_ascii_case(work_code.trim()))
            })
            .collect())
    }
    fn replace_detection(
        &self,
        installation_id: &InstallationId,
        detection: &InstallationDetection,
        status: InstallationStatus,
        updated_at: &str,
    ) -> Result<(), InstallationLibraryError>;
    fn replace_overrides(
        &self,
        installation_id: &InstallationId,
        overrides: &InstallationOverrides,
        status: InstallationStatus,
        updated_at: &str,
    ) -> Result<(), InstallationLibraryError>;
}

pub struct InstallationLibrary {
    store: Arc<dyn InstallationStore>,
}

impl InstallationLibrary {
    pub fn new(store: Arc<dyn InstallationStore>) -> Self {
        Self { store }
    }

    pub fn create(&self, installation: &Installation) -> Result<(), InstallationLibraryError> {
        installation.validate()?;
        self.store.create(installation)
    }

    pub fn create_or_refresh(
        &self,
        installation: &Installation,
    ) -> Result<Installation, InstallationLibraryError> {
        installation.validate()?;
        self.store.create_or_refresh(installation)
    }

    pub fn read(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<Installation>, InstallationLibraryError> {
        self.store.read(installation_id)
    }

    pub fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
        self.store.list()
    }

    pub fn find_by_work_code(
        &self,
        work_code: &str,
    ) -> Result<Vec<Installation>, InstallationLibraryError> {
        if work_code.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.store.find_by_work_code(work_code.trim())
    }

    pub fn replace_detection(
        &self,
        installation_id: &InstallationId,
        detection: InstallationDetection,
        updated_at: String,
    ) -> Result<Installation, InstallationLibraryError> {
        let mut installation = self
            .store
            .read(installation_id)?
            .ok_or_else(|| InstallationLibraryError::NotFound(installation_id.0.clone()))?;
        installation.replace_detection(detection, updated_at)?;
        self.store.replace_detection(
            installation_id,
            &installation.detection,
            installation.status,
            &installation.updated_at,
        )?;
        Ok(installation)
    }

    pub fn replace_overrides(
        &self,
        installation_id: &InstallationId,
        overrides: InstallationOverrides,
        updated_at: String,
    ) -> Result<Installation, InstallationLibraryError> {
        let mut installation = self
            .store
            .read(installation_id)?
            .ok_or_else(|| InstallationLibraryError::NotFound(installation_id.0.clone()))?;
        installation.replace_overrides(overrides, updated_at)?;
        self.store.replace_overrides(
            installation_id,
            &installation.overrides,
            installation.status,
            &installation.updated_at,
        )?;
        Ok(installation)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_domain::installation::{
        InferenceConfidence, InstallationPlatform, InstallationStatus, LaunchActionKind,
        LaunchTarget, ManualCatalogIdentity, ManualLaunchSelection, MediaType, RelativePath,
    };

    use super::*;

    struct MemoryStore {
        installation: Mutex<Option<Installation>>,
    }

    impl InstallationStore for MemoryStore {
        fn create(&self, installation: &Installation) -> Result<(), InstallationLibraryError> {
            *self.installation.lock().expect("memory store") = Some(installation.clone());
            Ok(())
        }

        fn create_or_refresh(
            &self,
            installation: &Installation,
        ) -> Result<Installation, InstallationLibraryError> {
            let mut guard = self.installation.lock().expect("memory store");
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
            Ok(stored)
        }

        fn read(
            &self,
            _installation_id: &InstallationId,
        ) -> Result<Option<Installation>, InstallationLibraryError> {
            Ok(self.installation.lock().expect("memory store").clone())
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(self
                .installation
                .lock()
                .expect("memory store")
                .clone()
                .into_iter()
                .collect())
        }

        fn replace_detection(
            &self,
            _installation_id: &InstallationId,
            detection: &InstallationDetection,
            status: InstallationStatus,
            updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            let mut guard = self.installation.lock().expect("memory store");
            let installation = guard.as_mut().expect("installation");
            installation.detection = detection.clone();
            installation.status = status;
            installation.updated_at = updated_at.to_owned();
            Ok(())
        }

        fn replace_overrides(
            &self,
            _installation_id: &InstallationId,
            overrides: &InstallationOverrides,
            status: InstallationStatus,
            updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            let mut guard = self.installation.lock().expect("memory store");
            let installation = guard.as_mut().expect("installation");
            installation.overrides = overrides.clone();
            installation.status = status;
            installation.updated_at = updated_at.to_owned();
            Ok(())
        }
    }

    #[test]
    fn application_boundary_never_substitutes_a_missing_manual_target() {
        let game = RelativePath::parse("Game.exe").expect("game path");
        let installation = Installation {
            id: InstallationId("installation-1".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/game".to_owned(),
            platform: InstallationPlatform::Windows,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: InstallationStatus::Ready,
                content_items: vec![dla_domain::installation::ContentItem {
                    relative_path: game.clone(),
                    path_key: "game.exe".to_owned(),
                    media_type: MediaType::Executable,
                    size_bytes: Some(2),
                    modified_at: None,
                    confidence: InferenceConfidence::High,
                    reason_codes: vec!["file_extension".to_owned()],
                }],
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                catalog_identity: Some(ManualCatalogIdentity::Unidentified),
                preferred_action: Some(ManualLaunchSelection {
                    action: LaunchActionKind::LaunchExecutable,
                    target: LaunchTarget::RelativePath(game),
                }),
                ..InstallationOverrides::default()
            },
            discovered_at: "2026-08-07T00:00:00Z".to_owned(),
            updated_at: "2026-08-07T00:00:00Z".to_owned(),
        };
        let store = Arc::new(MemoryStore {
            installation: Mutex::new(None),
        });
        let library = InstallationLibrary::new(store);
        library.create(&installation).expect("create installation");

        let refreshed = library
            .replace_detection(
                &installation.id,
                InstallationDetection {
                    source_scan_session_id: None,
                    catalog_identity: None,
                    suggested_status: InstallationStatus::Ready,
                    content_items: Vec::new(),
                    launch_candidates: Vec::new(),
                    package_inspection: None,
                },
                "2026-08-07T01:00:00Z".to_owned(),
            )
            .expect("refresh detection");

        assert_eq!(refreshed.status, InstallationStatus::NeedsReview);
        assert_eq!(refreshed.overrides, installation.overrides);
    }
}
