use std::sync::Arc;

use dla_domain::installation::{
    ContentItemOverride, Installation, InstallationId, InstallationOverrides,
    ManualCatalogIdentity, ManualLaunchSelection,
};
use serde::Deserialize;
use thiserror::Error;

use crate::{
    identity::{CatalogIdentityError, CatalogIdentityReader},
    installation::{InstallationLibrary, InstallationLibraryError, InstallationStore},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstallationReviewRequest {
    pub installation_id: InstallationId,
    pub catalog_identity: Option<ManualCatalogIdentity>,
    pub custom_title: Option<String>,
    pub preferred_action: Option<ManualLaunchSelection>,
    pub content_items: Vec<ContentItemOverride>,
}

#[derive(Debug, Error)]
pub enum InstallationReviewError {
    #[error("catalog work was not found: {0}")]
    CatalogWorkNotFound(String),
    #[error(transparent)]
    Catalog(#[from] CatalogIdentityError),
    #[error(transparent)]
    Library(#[from] InstallationLibraryError),
}

pub struct InstallationReviewService {
    catalog: Arc<dyn CatalogIdentityReader>,
    library: InstallationLibrary,
}

impl InstallationReviewService {
    pub fn new(catalog: Arc<dyn CatalogIdentityReader>, store: Arc<dyn InstallationStore>) -> Self {
        Self {
            catalog,
            library: InstallationLibrary::new(store),
        }
    }

    pub fn list(&self) -> Result<Vec<Installation>, InstallationReviewError> {
        Ok(self.library.list()?)
    }

    pub fn list_for_work(
        &self,
        work_code: &str,
    ) -> Result<Vec<Installation>, InstallationReviewError> {
        Ok(self.library.find_by_work_code(work_code)?)
    }

    pub fn read(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Installation, InstallationReviewError> {
        self.library
            .read(installation_id)?
            .ok_or_else(|| InstallationLibraryError::NotFound(installation_id.0.clone()).into())
    }

    pub fn save(
        &self,
        mut request: InstallationReviewRequest,
        reviewed_at: String,
    ) -> Result<Installation, InstallationReviewError> {
        request.custom_title = request
            .custom_title
            .map(|title| title.trim().to_owned())
            .filter(|title| !title.is_empty());
        request.catalog_identity = self.validate_catalog_identity(request.catalog_identity)?;
        let overrides = InstallationOverrides {
            catalog_identity: request.catalog_identity,
            custom_title: request.custom_title,
            preferred_action: request.preferred_action,
            content_items: request.content_items,
            reviewed_at: Some(reviewed_at.clone()),
        };
        Ok(self
            .library
            .replace_overrides(&request.installation_id, overrides, reviewed_at)?)
    }

    fn validate_catalog_identity(
        &self,
        identity: Option<ManualCatalogIdentity>,
    ) -> Result<Option<ManualCatalogIdentity>, InstallationReviewError> {
        let Some(ManualCatalogIdentity::CatalogWork { work_code }) = identity else {
            return Ok(identity);
        };
        let normalized = work_code.trim().to_ascii_uppercase();
        let works = self
            .catalog
            .read_works_by_codes(std::slice::from_ref(&normalized))?;
        let Some(work) = works
            .into_iter()
            .find(|work| work.code.eq_ignore_ascii_case(&normalized))
        else {
            return Err(InstallationReviewError::CatalogWorkNotFound(normalized));
        };
        Ok(Some(ManualCatalogIdentity::CatalogWork {
            work_code: work.code,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_domain::{
        CatalogWork,
        installation::{
            CatalogIdentity, InstallationDetection, InstallationPlatform, InstallationStatus,
        },
        scanner::ScanMatchConfidence,
    };

    use crate::{
        identity::{ArchiveHash, CatalogArchiveIdentity},
        installation::InstallationLibraryError,
    };

    use super::*;

    struct MemoryCatalog {
        work: CatalogWork,
    }

    impl CatalogIdentityReader for MemoryCatalog {
        fn read_works_by_codes(
            &self,
            work_codes: &[String],
        ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
            Ok(work_codes
                .iter()
                .any(|code| code.eq_ignore_ascii_case(&self.work.code))
                .then(|| self.work.clone())
                .into_iter()
                .collect())
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

    struct MemoryStore {
        installation: Mutex<Installation>,
    }

    impl InstallationStore for MemoryStore {
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
            let installation = self.installation.lock().expect("installation").clone();
            Ok((installation.id == *installation_id).then_some(installation))
        }

        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(vec![
                self.installation.lock().expect("installation").clone(),
            ])
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
            status: InstallationStatus,
            updated_at: &str,
        ) -> Result<(), InstallationLibraryError> {
            let mut installation = self.installation.lock().expect("installation");
            assert_eq!(installation.id, *installation_id);
            installation.overrides = overrides.clone();
            installation.status = status;
            installation.updated_at = updated_at.to_owned();
            Ok(())
        }
    }

    fn work(code: &str) -> CatalogWork {
        CatalogWork {
            code: code.to_owned(),
            source_code: code.to_owned(),
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
        }
    }

    fn installation() -> Installation {
        Installation {
            id: InstallationId("installation-review".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/review".to_owned(),
            platform: InstallationPlatform::Linux,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: Some(CatalogIdentity {
                    work_code: "RJ01326398".to_owned(),
                    confidence: ScanMatchConfidence::Strong,
                    reason_codes: vec!["code_in_directory_name".to_owned()],
                }),
                suggested_status: InstallationStatus::Ready,
                content_items: Vec::new(),
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides::default(),
            discovered_at: "2026-08-07T00:00:00Z".to_owned(),
            updated_at: "2026-08-07T00:00:00Z".to_owned(),
        }
    }

    fn service() -> InstallationReviewService {
        InstallationReviewService::new(
            Arc::new(MemoryCatalog {
                work: work("RJ09999999"),
            }),
            Arc::new(MemoryStore {
                installation: Mutex::new(installation()),
            }),
        )
    }

    #[test]
    fn review_validates_and_canonicalizes_manual_catalog_identity() {
        let service = service();
        let saved = service
            .save(
                InstallationReviewRequest {
                    installation_id: InstallationId("installation-review".to_owned()),
                    catalog_identity: Some(ManualCatalogIdentity::CatalogWork {
                        work_code: "  rj09999999 ".to_owned(),
                    }),
                    custom_title: Some("  My library title  ".to_owned()),
                    preferred_action: None,
                    content_items: Vec::new(),
                },
                "2026-08-07T01:00:00Z".to_owned(),
            )
            .expect("save review");

        assert_eq!(
            saved.overrides.catalog_identity,
            Some(ManualCatalogIdentity::CatalogWork {
                work_code: "RJ09999999".to_owned()
            })
        );
        assert_eq!(
            saved.overrides.custom_title.as_deref(),
            Some("My library title")
        );
        assert_eq!(
            saved.overrides.reviewed_at.as_deref(),
            Some("2026-08-07T01:00:00Z")
        );
        assert_eq!(saved.updated_at, "2026-08-07T01:00:00Z");
    }

    #[test]
    fn review_rejects_a_manual_catalog_identity_that_does_not_exist() {
        let service = service();
        let result = service.save(
            InstallationReviewRequest {
                installation_id: InstallationId("installation-review".to_owned()),
                catalog_identity: Some(ManualCatalogIdentity::CatalogWork {
                    work_code: "RJ00000001".to_owned(),
                }),
                custom_title: None,
                preferred_action: None,
                content_items: Vec::new(),
            },
            "2026-08-07T01:00:00Z".to_owned(),
        );

        assert!(matches!(
            result,
            Err(InstallationReviewError::CatalogWorkNotFound(code)) if code == "RJ00000001"
        ));
    }
}
