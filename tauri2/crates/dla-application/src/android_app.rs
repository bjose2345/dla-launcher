use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use dla_domain::{
    android_app::{
        AndroidAppAssociation, AndroidAppAssociationError, AndroidAppAssociationId,
        AndroidAppRuntimeState, AndroidAppRuntimeStatus, AndroidAppView, valid_package_name,
        valid_sha256,
    },
    android_package::{AndroidPackageInstallState, AndroidPackageState},
};
use serde::Deserialize;
use thiserror::Error;

use crate::identity::{CatalogIdentityError, CatalogIdentityReader};

const ANDROID_APP_OBSERVATION_BATCH_SIZE: usize = 128;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AndroidAppPlatformState {
    Installed,
    Missing,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AndroidAppPlatformObservation {
    pub package_name: String,
    pub state: AndroidAppPlatformState,
    pub application_label: Option<String>,
    pub version_name: Option<String>,
    pub version_code: Option<String>,
    pub signing_certificate_sha256: Vec<String>,
    pub launchable: bool,
    pub technical_detail: Option<String>,
}

#[derive(Debug, Error)]
pub enum AndroidAppError {
    #[error("Android application association is unavailable on this platform")]
    UnsupportedPlatform,
    #[error("the Android package installation has not completed")]
    InstallNotCompleted,
    #[error("the installed Android package no longer matches the reviewed certificate")]
    SignerMismatch,
    #[error("the installed Android package has no launcher activity")]
    NotLaunchable,
    #[error("the installed Android application could not be verified")]
    ObservationUnavailable,
    #[error("Android application association was not found: {0}")]
    NotFound(String),
    #[error("Android package is already associated with another work: {0}")]
    PackageAlreadyAssociated(String),
    #[error("catalog work was not found: {0}")]
    WorkNotFound(String),
    #[error("Android application adapter returned invalid state: {0}")]
    InvalidPlatformState(&'static str),
    #[error("Android application adapter failed: {0}")]
    Adapter(String),
    #[error("Android application persistence failed: {0}")]
    Persistence(String),
    #[error(transparent)]
    InvalidAssociation(#[from] AndroidAppAssociationError),
    #[error(transparent)]
    Catalog(#[from] CatalogIdentityError),
}

impl AndroidAppError {
    pub fn adapter(error: impl std::fmt::Display) -> Self {
        Self::Adapter(error.to_string())
    }

    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait AndroidAppPlatform: Send + Sync {
    fn observe(
        &self,
        package_names: &[String],
    ) -> Result<Vec<AndroidAppPlatformObservation>, AndroidAppError>;
    fn launch(
        &self,
        package_name: &str,
        expected_signing_certificate_sha256: &[String],
    ) -> Result<(), AndroidAppError>;
}

pub trait AndroidAppAssociationStore: Send + Sync {
    fn read(
        &self,
        association_id: &AndroidAppAssociationId,
    ) -> Result<Option<AndroidAppAssociation>, AndroidAppError>;
    fn read_by_work_code(
        &self,
        work_code: &str,
    ) -> Result<Option<AndroidAppAssociation>, AndroidAppError>;
    fn read_by_package_name(
        &self,
        package_name: &str,
    ) -> Result<Option<AndroidAppAssociation>, AndroidAppError>;
    fn list(&self) -> Result<Vec<AndroidAppAssociation>, AndroidAppError>;
    fn save(&self, association: &AndroidAppAssociation) -> Result<(), AndroidAppError>;
    fn remove(&self, association_id: &AndroidAppAssociationId) -> Result<bool, AndroidAppError>;
    fn record_launch(
        &self,
        association_id: &AndroidAppAssociationId,
        launched_at: &str,
    ) -> Result<AndroidAppAssociation, AndroidAppError>;
}

pub struct AndroidAppService {
    store: Arc<dyn AndroidAppAssociationStore>,
    catalog: Arc<dyn CatalogIdentityReader>,
    platform: Arc<dyn AndroidAppPlatform>,
}

impl AndroidAppService {
    pub fn new(
        store: Arc<dyn AndroidAppAssociationStore>,
        catalog: Arc<dyn CatalogIdentityReader>,
        platform: Arc<dyn AndroidAppPlatform>,
    ) -> Self {
        Self {
            store,
            catalog,
            platform,
        }
    }

    pub fn list(&self) -> Result<Vec<AndroidAppView>, AndroidAppError> {
        let associations = self.store.list()?;
        self.views(associations)
    }

    pub fn associate_installed(
        &self,
        work_code: &str,
        candidate_id: AndroidAppAssociationId,
        associated_at: &str,
        package_state: &AndroidPackageState,
    ) -> Result<AndroidAppView, AndroidAppError> {
        let inspection = installed_inspection(package_state)?;
        let work_code = work_code.trim();
        if work_code.is_empty() || work_code.len() > 64 {
            return Err(AndroidAppAssociationError::InvalidWorkCode.into());
        }
        candidate_id.validate()?;
        let works = self.catalog.read_works_by_codes(&[work_code.to_owned()])?;
        if !works
            .iter()
            .any(|work| work.code.eq_ignore_ascii_case(work_code))
        {
            return Err(AndroidAppError::WorkNotFound(work_code.to_owned()));
        }
        if let Some(existing) = self.store.read_by_package_name(&inspection.package_name)?
            && !existing.work_code.eq_ignore_ascii_case(work_code)
        {
            return Err(AndroidAppError::PackageAlreadyAssociated(
                existing.work_code,
            ));
        }

        let runtime = self.observe_one(&inspection.package_name)?;
        require_associable(&runtime, &inspection.signing_certificate_sha256)?;
        let existing = self.store.read_by_work_code(work_code)?;
        let association = AndroidAppAssociation {
            id: existing
                .as_ref()
                .map_or(candidate_id, |association| association.id.clone()),
            work_code: works
                .into_iter()
                .find(|work| work.code.eq_ignore_ascii_case(work_code))
                .expect("catalog work was checked")
                .code,
            package_name: inspection.package_name.clone(),
            application_label: inspection.application_label.clone(),
            expected_signing_certificate_sha256: normalized_fingerprints(
                &inspection.signing_certificate_sha256,
            ),
            associated_version_name: inspection.version_name.clone(),
            associated_version_code: inspection.version_code.clone(),
            associated_at: existing.as_ref().map_or_else(
                || associated_at.to_owned(),
                |association| association.associated_at.clone(),
            ),
            updated_at: associated_at.to_owned(),
            last_launched_at: existing
                .as_ref()
                .and_then(|association| association.last_launched_at.clone()),
            launch_count: existing
                .as_ref()
                .map_or(0, |association| association.launch_count),
        };
        association.validate()?;
        self.store.save(&association)?;
        Ok(AndroidAppView {
            runtime: runtime_status(&association, runtime),
            association,
        })
    }

    pub fn launch(
        &self,
        association_id: &AndroidAppAssociationId,
        launched_at: &str,
    ) -> Result<AndroidAppView, AndroidAppError> {
        association_id.validate()?;
        let association = self
            .store
            .read(association_id)?
            .ok_or_else(|| AndroidAppError::NotFound(association_id.0.clone()))?;
        let observation = self.observe_one(&association.package_name)?;
        require_launchable(&association, &observation)?;
        self.platform.launch(
            &association.package_name,
            &association.expected_signing_certificate_sha256,
        )?;
        let updated = self.store.record_launch(association_id, launched_at)?;
        Ok(AndroidAppView {
            runtime: runtime_status(&updated, observation),
            association: updated,
        })
    }

    pub fn remove(&self, association_id: &AndroidAppAssociationId) -> Result<(), AndroidAppError> {
        association_id.validate()?;
        if self.store.remove(association_id)? {
            Ok(())
        } else {
            Err(AndroidAppError::NotFound(association_id.0.clone()))
        }
    }

    fn views(
        &self,
        associations: Vec<AndroidAppAssociation>,
    ) -> Result<Vec<AndroidAppView>, AndroidAppError> {
        if associations.is_empty() {
            return Ok(Vec::new());
        }
        let package_names = associations
            .iter()
            .map(|association| association.package_name.clone())
            .collect::<Vec<_>>();
        if package_names.iter().collect::<HashSet<_>>().len() != package_names.len() {
            return Err(AndroidAppError::InvalidPlatformState(
                "stored associations contain duplicate package identities",
            ));
        }
        let mut observations = HashMap::with_capacity(package_names.len());
        for batch in package_names.chunks(ANDROID_APP_OBSERVATION_BATCH_SIZE) {
            observations.extend(validate_observations(batch, self.platform.observe(batch)?)?);
        }
        Ok(associations
            .into_iter()
            .map(|association| {
                let observation = observations
                    .get(&association.package_name)
                    .expect("validated observation exists")
                    .clone();
                AndroidAppView {
                    runtime: runtime_status(&association, observation),
                    association,
                }
            })
            .collect())
    }

    fn observe_one(
        &self,
        package_name: &str,
    ) -> Result<AndroidAppPlatformObservation, AndroidAppError> {
        let package_names = vec![package_name.to_owned()];
        let mut observations =
            validate_observations(&package_names, self.platform.observe(&package_names)?)?;
        Ok(observations
            .remove(package_name)
            .expect("validated observation exists"))
    }
}

fn installed_inspection(
    state: &AndroidPackageState,
) -> Result<&dla_domain::android_package::AndroidPackageInspection, AndroidAppError> {
    let status = state
        .install_status
        .as_ref()
        .filter(|status| status.state == AndroidPackageInstallState::Installed)
        .ok_or(AndroidAppError::InstallNotCompleted)?;
    let inspection = state
        .inspection
        .as_ref()
        .filter(|inspection| inspection.selection_id == status.selection_id)
        .ok_or(AndroidAppError::InstallNotCompleted)?;
    if !inspection.installable || inspection.signing_certificate_sha256.is_empty() {
        return Err(AndroidAppError::InstallNotCompleted);
    }
    Ok(inspection)
}

fn require_associable(
    observation: &AndroidAppPlatformObservation,
    expected: &[String],
) -> Result<(), AndroidAppError> {
    match observation.state {
        AndroidAppPlatformState::Installed => {}
        AndroidAppPlatformState::Missing => return Err(AndroidAppError::InstallNotCompleted),
        AndroidAppPlatformState::Unavailable => {
            return Err(AndroidAppError::ObservationUnavailable);
        }
    }
    if !fingerprints_match(expected, &observation.signing_certificate_sha256) {
        return Err(AndroidAppError::SignerMismatch);
    }
    if !observation.launchable {
        return Err(AndroidAppError::NotLaunchable);
    }
    Ok(())
}

fn require_launchable(
    association: &AndroidAppAssociation,
    observation: &AndroidAppPlatformObservation,
) -> Result<(), AndroidAppError> {
    match runtime_state(association, observation) {
        AndroidAppRuntimeState::Ready => Ok(()),
        AndroidAppRuntimeState::NotLaunchable => Err(AndroidAppError::NotLaunchable),
        AndroidAppRuntimeState::Missing => Err(AndroidAppError::NotFound(association.id.0.clone())),
        AndroidAppRuntimeState::SignerMismatch => Err(AndroidAppError::SignerMismatch),
        AndroidAppRuntimeState::Unavailable => Err(AndroidAppError::ObservationUnavailable),
    }
}

fn runtime_status(
    association: &AndroidAppAssociation,
    observation: AndroidAppPlatformObservation,
) -> AndroidAppRuntimeStatus {
    AndroidAppRuntimeStatus {
        state: runtime_state(association, &observation),
        application_label: observation.application_label,
        version_name: observation.version_name,
        version_code: observation.version_code,
        technical_detail: observation.technical_detail,
    }
}

fn runtime_state(
    association: &AndroidAppAssociation,
    observation: &AndroidAppPlatformObservation,
) -> AndroidAppRuntimeState {
    match observation.state {
        AndroidAppPlatformState::Missing => AndroidAppRuntimeState::Missing,
        AndroidAppPlatformState::Unavailable => AndroidAppRuntimeState::Unavailable,
        AndroidAppPlatformState::Installed => {
            if !fingerprints_match(
                &association.expected_signing_certificate_sha256,
                &observation.signing_certificate_sha256,
            ) {
                AndroidAppRuntimeState::SignerMismatch
            } else if observation.launchable {
                AndroidAppRuntimeState::Ready
            } else {
                AndroidAppRuntimeState::NotLaunchable
            }
        }
    }
}

fn validate_observations(
    requested: &[String],
    observations: Vec<AndroidAppPlatformObservation>,
) -> Result<HashMap<String, AndroidAppPlatformObservation>, AndroidAppError> {
    if requested.len() != observations.len() {
        return Err(AndroidAppError::InvalidPlatformState(
            "observation count does not match the request",
        ));
    }
    let mut mapped = HashMap::new();
    for observation in observations {
        validate_observation(&observation)?;
        if !requested.contains(&observation.package_name)
            || mapped
                .insert(observation.package_name.clone(), observation)
                .is_some()
        {
            return Err(AndroidAppError::InvalidPlatformState(
                "observation package identity does not match the request",
            ));
        }
    }
    Ok(mapped)
}

fn validate_observation(
    observation: &AndroidAppPlatformObservation,
) -> Result<(), AndroidAppError> {
    if !valid_package_name(&observation.package_name)
        || observation
            .application_label
            .as_ref()
            .is_some_and(|label| label.trim().is_empty() || label.len() > 512)
        || observation
            .version_name
            .as_ref()
            .is_some_and(|version| version.len() > 256)
        || observation.version_code.as_ref().is_some_and(|version| {
            version.is_empty()
                || version.len() > 32
                || !version.bytes().all(|byte| byte.is_ascii_digit())
        })
        || observation
            .signing_certificate_sha256
            .iter()
            .any(|fingerprint| !valid_sha256(fingerprint))
        || observation.signing_certificate_sha256.len() > 32
        || observation
            .technical_detail
            .as_ref()
            .is_some_and(|detail| detail.len() > 2_048)
    {
        return Err(AndroidAppError::InvalidPlatformState(
            "observation contains invalid package metadata",
        ));
    }
    if observation.state != AndroidAppPlatformState::Installed
        && (observation.application_label.is_some()
            || observation.version_name.is_some()
            || observation.version_code.is_some()
            || !observation.signing_certificate_sha256.is_empty()
            || observation.launchable)
    {
        return Err(AndroidAppError::InvalidPlatformState(
            "an absent package exposes installed metadata",
        ));
    }
    Ok(())
}

fn fingerprints_match(expected: &[String], observed: &[String]) -> bool {
    expected.iter().any(|expected| {
        observed
            .iter()
            .any(|observed| expected.eq_ignore_ascii_case(observed))
    })
}

fn normalized_fingerprints(fingerprints: &[String]) -> Vec<String> {
    let mut normalized = fingerprints
        .iter()
        .map(|fingerprint| fingerprint.to_ascii_lowercase())
        .collect::<Vec<_>>();
    normalized.sort();
    normalized.dedup();
    normalized
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use dla_domain::{CatalogWork, android_package::*};

    use super::*;

    struct MemoryStore(Mutex<Vec<AndroidAppAssociation>>);

    impl AndroidAppAssociationStore for MemoryStore {
        fn read(
            &self,
            id: &AndroidAppAssociationId,
        ) -> Result<Option<AndroidAppAssociation>, AndroidAppError> {
            Ok(self
                .0
                .lock()
                .expect("store")
                .iter()
                .find(|item| item.id == *id)
                .cloned())
        }
        fn read_by_work_code(
            &self,
            code: &str,
        ) -> Result<Option<AndroidAppAssociation>, AndroidAppError> {
            Ok(self
                .0
                .lock()
                .expect("store")
                .iter()
                .find(|item| item.work_code.eq_ignore_ascii_case(code))
                .cloned())
        }
        fn read_by_package_name(
            &self,
            name: &str,
        ) -> Result<Option<AndroidAppAssociation>, AndroidAppError> {
            Ok(self
                .0
                .lock()
                .expect("store")
                .iter()
                .find(|item| item.package_name == name)
                .cloned())
        }
        fn list(&self) -> Result<Vec<AndroidAppAssociation>, AndroidAppError> {
            Ok(self.0.lock().expect("store").clone())
        }
        fn save(&self, association: &AndroidAppAssociation) -> Result<(), AndroidAppError> {
            let mut items = self.0.lock().expect("store");
            items.retain(|item| item.id != association.id);
            items.push(association.clone());
            Ok(())
        }
        fn remove(&self, id: &AndroidAppAssociationId) -> Result<bool, AndroidAppError> {
            let mut items = self.0.lock().expect("store");
            let before = items.len();
            items.retain(|item| item.id != *id);
            Ok(items.len() != before)
        }
        fn record_launch(
            &self,
            id: &AndroidAppAssociationId,
            at: &str,
        ) -> Result<AndroidAppAssociation, AndroidAppError> {
            let mut items = self.0.lock().expect("store");
            let item = items
                .iter_mut()
                .find(|item| item.id == *id)
                .ok_or_else(|| AndroidAppError::NotFound(id.0.clone()))?;
            item.last_launched_at = Some(at.to_owned());
            item.updated_at = at.to_owned();
            item.launch_count += 1;
            Ok(item.clone())
        }
    }

    struct Catalog;
    impl CatalogIdentityReader for Catalog {
        fn read_works_by_codes(
            &self,
            codes: &[String],
        ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
            Ok(codes
                .iter()
                .filter(|code| code.eq_ignore_ascii_case("RJ01326398"))
                .map(|code| work(&code.to_ascii_uppercase()))
                .collect())
        }
        fn resolve_archive_hash(
            &self,
            _hash: &crate::identity::ArchiveHash,
        ) -> Result<Vec<CatalogWork>, CatalogIdentityError> {
            Ok(Vec::new())
        }
        fn find_archive_candidates_by_size(
            &self,
            _size: &str,
            _limit: usize,
        ) -> Result<Vec<crate::identity::CatalogArchiveIdentity>, CatalogIdentityError> {
            Ok(Vec::new())
        }
    }

    struct Platform {
        observation: Mutex<AndroidAppPlatformObservation>,
        launches: Mutex<Vec<String>>,
    }

    struct BatchPlatform(Mutex<Vec<usize>>);
    impl AndroidAppPlatform for BatchPlatform {
        fn observe(
            &self,
            names: &[String],
        ) -> Result<Vec<AndroidAppPlatformObservation>, AndroidAppError> {
            self.0.lock().expect("batches").push(names.len());
            Ok(names
                .iter()
                .map(|name| AndroidAppPlatformObservation {
                    package_name: name.clone(),
                    ..observation()
                })
                .collect())
        }

        fn launch(&self, _name: &str, _expected: &[String]) -> Result<(), AndroidAppError> {
            Ok(())
        }
    }
    impl AndroidAppPlatform for Platform {
        fn observe(
            &self,
            names: &[String],
        ) -> Result<Vec<AndroidAppPlatformObservation>, AndroidAppError> {
            Ok(names
                .iter()
                .map(|name| {
                    let mut item = self.observation.lock().expect("observation").clone();
                    item.package_name = name.clone();
                    item
                })
                .collect())
        }
        fn launch(&self, name: &str, _expected: &[String]) -> Result<(), AndroidAppError> {
            self.launches
                .lock()
                .expect("launches")
                .push(name.to_owned());
            Ok(())
        }
    }

    #[test]
    fn associates_only_a_completed_install_and_records_a_verified_launch() {
        let store = Arc::new(MemoryStore(Mutex::new(Vec::new())));
        let platform = Arc::new(Platform {
            observation: Mutex::new(observation()),
            launches: Mutex::new(Vec::new()),
        });
        let service = AndroidAppService::new(store.clone(), Arc::new(Catalog), platform.clone());
        let associated = service
            .associate_installed(
                "rj01326398",
                AndroidAppAssociationId("android-app-1234567890".to_owned()),
                "2026-08-22T12:00:00Z",
                &installed_state(),
            )
            .expect("associate");
        assert_eq!(associated.association.work_code, "RJ01326398");
        assert_eq!(associated.runtime.state, AndroidAppRuntimeState::Ready);

        let launched = service
            .launch(&associated.association.id, "2026-08-22T12:01:00Z")
            .expect("launch");
        assert_eq!(launched.association.launch_count, 1);
        assert_eq!(
            platform.launches.lock().expect("launches").as_slice(),
            &["org.dlaproject.fixture"]
        );
    }

    #[test]
    fn refuses_a_reinstalled_package_with_another_signer() {
        let store = Arc::new(MemoryStore(Mutex::new(Vec::new())));
        let platform = Arc::new(Platform {
            observation: Mutex::new(observation()),
            launches: Mutex::new(Vec::new()),
        });
        let service = AndroidAppService::new(store, Arc::new(Catalog), platform.clone());
        let associated = service
            .associate_installed(
                "RJ01326398",
                AndroidAppAssociationId("android-app-1234567890".to_owned()),
                "2026-08-22T12:00:00Z",
                &installed_state(),
            )
            .expect("associate");
        platform
            .observation
            .lock()
            .expect("observation")
            .signing_certificate_sha256 = vec!["b".repeat(64)];
        let listed = service.list().expect("list");
        assert_eq!(
            listed[0].runtime.state,
            AndroidAppRuntimeState::SignerMismatch
        );
        assert!(matches!(
            service.launch(&associated.association.id, "2026-08-22T12:01:00Z"),
            Err(AndroidAppError::SignerMismatch)
        ));
        assert!(platform.launches.lock().expect("launches").is_empty());
    }

    #[test]
    fn observes_large_libraries_in_bounded_batches() {
        let associations = (0..513)
            .map(|index| AndroidAppAssociation {
                id: AndroidAppAssociationId(format!("android-app-{index:06}-fixture")),
                work_code: format!("RJ{index:08}"),
                package_name: format!("org.dlaproject.fixture{index}"),
                application_label: format!("Fixture {index}"),
                expected_signing_certificate_sha256: vec!["a".repeat(64)],
                associated_version_name: Some("1.0".to_owned()),
                associated_version_code: "1".to_owned(),
                associated_at: "2026-08-22T12:00:00Z".to_owned(),
                updated_at: "2026-08-22T12:00:00Z".to_owned(),
                last_launched_at: None,
                launch_count: 0,
            })
            .collect();
        let platform = Arc::new(BatchPlatform(Mutex::new(Vec::new())));
        let service = AndroidAppService::new(
            Arc::new(MemoryStore(Mutex::new(associations))),
            Arc::new(Catalog),
            platform.clone(),
        );

        assert_eq!(service.list().expect("list").len(), 513);
        assert_eq!(
            platform.0.lock().expect("batches").as_slice(),
            &[128, 128, 128, 128, 1]
        );
    }

    fn observation() -> AndroidAppPlatformObservation {
        AndroidAppPlatformObservation {
            package_name: "org.dlaproject.fixture".to_owned(),
            state: AndroidAppPlatformState::Installed,
            application_label: Some("Fixture".to_owned()),
            version_name: Some("1.0".to_owned()),
            version_code: Some("1".to_owned()),
            signing_certificate_sha256: vec!["a".repeat(64)],
            launchable: true,
            technical_detail: None,
        }
    }

    fn installed_state() -> AndroidPackageState {
        AndroidPackageState {
            capability: AndroidPackageCapability {
                status: AndroidPackageCapabilityStatus::Ready,
                device_sdk: Some(36),
            },
            inspection: Some(AndroidPackageInspection {
                selection_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
                display_name: "fixture.apk".to_owned(),
                application_label: "Fixture".to_owned(),
                package_name: "org.dlaproject.fixture".to_owned(),
                version_name: Some("1.0".to_owned()),
                version_code: "1".to_owned(),
                size_bytes: 42,
                sha256: "c".repeat(64),
                minimum_sdk: Some(24),
                target_sdk: Some(36),
                signing_certificate_sha256: vec!["a".repeat(64)],
                installable: true,
                block_reason: None,
            }),
            install_status: Some(AndroidPackageInstallStatus {
                operation_id: "87654321-4321-4321-4321-cba987654321".to_owned(),
                selection_id: "12345678-1234-1234-1234-123456789abc".to_owned(),
                state: AndroidPackageInstallState::Installed,
                technical_detail: None,
            }),
        }
    }

    fn work(code: &str) -> CatalogWork {
        CatalogWork {
            code: code.to_owned(),
            source_code: code.to_owned(),
            title: "Fixture work".to_owned(),
            title_english: String::new(),
            added_date: "2026-08-22".to_owned(),
            release_date: "2026-08-22".to_owned(),
            updated_date: "2026-08-22".to_owned(),
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
}
