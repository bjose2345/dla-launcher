use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use dla_domain::{
    installation::{InstallationId, MediaType, RelativePath},
    own_work::{
        ImportOwnWorkArtworkRequest, InstallationImage, OwnWorkDetails, SaveOwnWorkDetailsRequest,
    },
};
use thiserror::Error;

use crate::{
    installation::{InstallationLibraryError, InstallationStore},
    media::{MediaError, MediaInventoryReader, direct_inventory, natural_cmp},
    package_preparation::{PackagePreparationError, PackagePreparationStore},
};

pub const MAX_INSTALLATION_IMAGES: usize = 2000;
const MAX_TAGS: usize = 30;
const MAX_TAG_CHARS: usize = 40;
const MAX_DESCRIPTION_CHARS: usize = 4000;
const MAX_LINE_CHARS: usize = 200;
const MAX_SAMPLES: usize = 12;
// A gallery asks for one thumbnail per tile, so the list it checks them
// against is kept for a short while instead of walking the folder each time.
const IMAGE_LIST_LIFETIME: Duration = Duration::from_secs(120);

#[derive(Debug, Error)]
pub enum OwnWorkError {
    #[error("installation was not found: {0}")]
    InstallationNotFound(String),
    #[error("{0}")]
    InvalidRequest(String),
    #[error("this file is not one of the work's images")]
    UnknownImage,
    #[error("artwork was not found for this work: {0}")]
    UnknownArtwork(String),
    #[error("work details persistence failed: {0}")]
    Persistence(String),
    #[error("work artwork failed: {0}")]
    Artwork(String),
    #[error("work images could not be listed: {0}")]
    Inventory(String),
}

impl OwnWorkError {
    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }

    pub fn artwork(error: impl std::fmt::Display) -> Self {
        Self::Artwork(error.to_string())
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidRequest(message.into())
    }
}

impl From<InstallationLibraryError> for OwnWorkError {
    fn from(error: InstallationLibraryError) -> Self {
        match error {
            InstallationLibraryError::NotFound(id) => Self::InstallationNotFound(id),
            other => Self::persistence(other),
        }
    }
}

impl From<PackagePreparationError> for OwnWorkError {
    fn from(error: PackagePreparationError) -> Self {
        Self::persistence(error)
    }
}

impl From<MediaError> for OwnWorkError {
    fn from(error: MediaError) -> Self {
        Self::Inventory(error.to_string())
    }
}

pub trait OwnWorkDetailsStore: Send + Sync {
    fn read_own_work_details(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<OwnWorkDetails>, OwnWorkError>;
    fn list_own_work_details(&self) -> Result<Vec<OwnWorkDetails>, OwnWorkError>;
    fn save_own_work_details(&self, details: &OwnWorkDetails) -> Result<(), OwnWorkError>;
}

/// The pictures a user picked for their own works, kept apart from the
/// works' files so a cover survives the work being moved or rescanned.
pub trait OwnWorkArtworkStore: Send + Sync {
    /// Stores a picture for the work and returns its artwork id.
    fn import(
        &self,
        installation_id: &InstallationId,
        source: &Path,
    ) -> Result<String, OwnWorkError>;
    fn contains(
        &self,
        installation_id: &InstallationId,
        artwork_id: &str,
    ) -> Result<bool, OwnWorkError>;
    fn read(
        &self,
        installation_id: &InstallationId,
        artwork_id: &str,
    ) -> Result<Vec<u8>, OwnWorkError>;
    /// Removes the work's pictures that are not in `keep`.
    fn retain(&self, installation_id: &InstallationId, keep: &[String])
    -> Result<(), OwnWorkError>;
    /// Removes the artwork of works no longer in the Library; returns how many were removed.
    fn remove_all_except(&self, installations: &[InstallationId]) -> Result<usize, OwnWorkError>;
}

struct ImageList {
    root: PathBuf,
    images: Vec<InstallationImage>,
    paths: HashSet<RelativePath>,
    read_at: Instant,
}

pub struct OwnWorkService {
    installations: Arc<dyn InstallationStore>,
    preparations: Arc<dyn PackagePreparationStore>,
    inventory: Arc<dyn MediaInventoryReader>,
    details: Arc<dyn OwnWorkDetailsStore>,
    artwork: Arc<dyn OwnWorkArtworkStore>,
    image_lists: Mutex<HashMap<InstallationId, Arc<ImageList>>>,
}

impl OwnWorkService {
    pub fn new(
        installations: Arc<dyn InstallationStore>,
        preparations: Arc<dyn PackagePreparationStore>,
        inventory: Arc<dyn MediaInventoryReader>,
        details: Arc<dyn OwnWorkDetailsStore>,
        artwork: Arc<dyn OwnWorkArtworkStore>,
    ) -> Self {
        Self {
            installations,
            preparations,
            inventory,
            details,
            artwork,
            image_lists: Mutex::new(HashMap::new()),
        }
    }

    pub fn list(&self) -> Result<Vec<OwnWorkDetails>, OwnWorkError> {
        self.details.list_own_work_details()
    }

    pub fn read(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Option<OwnWorkDetails>, OwnWorkError> {
        self.details.read_own_work_details(installation_id)
    }

    pub fn save(
        &self,
        request: SaveOwnWorkDetailsRequest,
        updated_at: &str,
    ) -> Result<OwnWorkDetails, OwnWorkError> {
        self.require_installation(&request.installation_id)?;
        let details = validated_details(request, updated_at)?;
        for artwork_id in details.cover.iter().chain(details.samples.iter()) {
            if !self
                .artwork
                .contains(&details.installation_id, artwork_id)?
            {
                return Err(OwnWorkError::UnknownArtwork(artwork_id.clone()));
            }
        }
        self.details.save_own_work_details(&details)?;
        let keep = details
            .cover
            .iter()
            .chain(details.samples.iter())
            .cloned()
            .collect::<Vec<_>>();
        self.artwork.retain(&details.installation_id, &keep)?;
        Ok(details)
    }

    pub fn list_images(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Vec<InstallationImage>, OwnWorkError> {
        Ok(self.fresh_image_list(installation_id)?.images.clone())
    }

    /// The file behind one of the work's images, refusing anything else.
    pub fn image_path(
        &self,
        installation_id: &InstallationId,
        relative_path: &RelativePath,
    ) -> Result<PathBuf, OwnWorkError> {
        let root = self.image_root(installation_id, relative_path)?;
        Ok(root.join(relative_path.as_str()))
    }

    /// The folder holding one of the work's images, refusing any other file.
    pub fn image_root(
        &self,
        installation_id: &InstallationId,
        relative_path: &RelativePath,
    ) -> Result<PathBuf, OwnWorkError> {
        let list = self.image_list(installation_id)?;
        if !list.paths.contains(relative_path) {
            return Err(OwnWorkError::UnknownImage);
        }
        Ok(list.root.clone())
    }

    pub fn import_artwork(
        &self,
        request: ImportOwnWorkArtworkRequest,
    ) -> Result<String, OwnWorkError> {
        let source = self.image_path(&request.installation_id, &request.relative_path)?;
        self.artwork.import(&request.installation_id, &source)
    }

    pub fn import_artwork_file(
        &self,
        installation_id: &InstallationId,
        source: &Path,
    ) -> Result<String, OwnWorkError> {
        self.require_installation(installation_id)?;
        self.artwork.import(installation_id, source)
    }

    pub fn read_artwork(
        &self,
        installation_id: &InstallationId,
        artwork_id: &str,
    ) -> Result<Vec<u8>, OwnWorkError> {
        if !is_artwork_id(artwork_id) {
            return Err(OwnWorkError::UnknownArtwork(artwork_id.to_owned()));
        }
        self.artwork.read(installation_id, artwork_id)
    }

    pub fn remove_orphaned_artwork(&self) -> Result<usize, OwnWorkError> {
        let installations = self
            .installations
            .list()?
            .into_iter()
            .map(|installation| installation.id)
            .collect::<Vec<_>>();
        self.artwork.remove_all_except(&installations)
    }

    fn require_installation(&self, installation_id: &InstallationId) -> Result<(), OwnWorkError> {
        self.installations
            .read(installation_id)?
            .map(|_| ())
            .ok_or_else(|| OwnWorkError::InstallationNotFound(installation_id.0.clone()))
    }

    fn image_list(&self, installation_id: &InstallationId) -> Result<Arc<ImageList>, OwnWorkError> {
        let cached = self
            .image_lists
            .lock()
            .map_err(OwnWorkError::persistence)?
            .get(installation_id)
            .filter(|list| list.read_at.elapsed() < IMAGE_LIST_LIFETIME)
            .cloned();
        match cached {
            Some(list) => Ok(list),
            None => self.fresh_image_list(installation_id),
        }
    }

    fn fresh_image_list(
        &self,
        installation_id: &InstallationId,
    ) -> Result<Arc<ImageList>, OwnWorkError> {
        let installation = self
            .installations
            .read(installation_id)?
            .ok_or_else(|| OwnWorkError::InstallationNotFound(installation_id.0.clone()))?;
        let (root, inventory) = match self.preparations.read_prepared_package(installation_id)? {
            Some(prepared) => {
                let inventory = self.inventory.read_inventory(&prepared.destination_root)?;
                (prepared.destination_root, inventory)
            }
            None => {
                let inventory = direct_inventory(&installation);
                (installation.root_path, inventory)
            }
        };
        let mut images = inventory
            .into_iter()
            .filter(|item| item.media_type == MediaType::Image)
            .map(|item| InstallationImage {
                relative_path: item.relative_path,
                size_bytes: item.size_bytes,
            })
            .collect::<Vec<_>>();
        images.sort_by(|left, right| {
            natural_cmp(left.relative_path.as_str(), right.relative_path.as_str())
        });
        images.truncate(MAX_INSTALLATION_IMAGES);
        let list = Arc::new(ImageList {
            root: PathBuf::from(root),
            paths: images
                .iter()
                .map(|image| image.relative_path.clone())
                .collect(),
            images,
            read_at: Instant::now(),
        });
        self.image_lists
            .lock()
            .map_err(OwnWorkError::persistence)?
            .insert(installation_id.clone(), Arc::clone(&list));
        Ok(list)
    }
}

/// An artwork id is the SHA-256 of the stored picture, in lower-case hex.
pub fn is_artwork_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn validated_details(
    request: SaveOwnWorkDetailsRequest,
    updated_at: &str,
) -> Result<OwnWorkDetails, OwnWorkError> {
    let circle = line(request.circle, "circle")?;
    let code = line(request.code, "code")?;
    let release_date = trimmed(request.release_date);
    if let Some(date) = &release_date
        && !is_calendar_date(date)
    {
        return Err(OwnWorkError::invalid(
            "the release date must be a real date as YYYY-MM-DD",
        ));
    }
    let description = trimmed(request.description);
    if description
        .as_ref()
        .is_some_and(|value| value.chars().count() > MAX_DESCRIPTION_CHARS)
    {
        return Err(OwnWorkError::invalid(format!(
            "the description can be at most {MAX_DESCRIPTION_CHARS} characters"
        )));
    }
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for tag in request.tags {
        let tag = tag.trim().to_owned();
        if tag.is_empty() || !seen.insert(tag.to_lowercase()) {
            continue;
        }
        if tag.chars().count() > MAX_TAG_CHARS {
            return Err(OwnWorkError::invalid(format!(
                "a tag can be at most {MAX_TAG_CHARS} characters"
            )));
        }
        tags.push(tag);
    }
    if tags.len() > MAX_TAGS {
        return Err(OwnWorkError::invalid(format!(
            "a work can have at most {MAX_TAGS} tags"
        )));
    }
    let cover = trimmed(request.cover);
    let mut samples = Vec::new();
    for sample in request.samples {
        let sample = sample.trim().to_owned();
        if !sample.is_empty() && !samples.contains(&sample) {
            samples.push(sample);
        }
    }
    if samples.len() > MAX_SAMPLES {
        return Err(OwnWorkError::invalid(format!(
            "a work can have at most {MAX_SAMPLES} sample images"
        )));
    }
    for artwork_id in cover.iter().chain(samples.iter()) {
        if !is_artwork_id(artwork_id) {
            return Err(OwnWorkError::UnknownArtwork(artwork_id.clone()));
        }
    }
    Ok(OwnWorkDetails {
        installation_id: request.installation_id,
        circle,
        code,
        release_date,
        kind: request.kind,
        tags,
        description,
        cover,
        samples,
        updated_at: updated_at.to_owned(),
    })
}

fn trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn line(value: Option<String>, field: &str) -> Result<Option<String>, OwnWorkError> {
    let value = trimmed(value);
    if value
        .as_ref()
        .is_some_and(|value| value.chars().count() > MAX_LINE_CHARS || value.contains(['\n', '\r']))
    {
        return Err(OwnWorkError::invalid(format!(
            "the {field} must be one line of at most {MAX_LINE_CHARS} characters"
        )));
    }
    Ok(value)
}

fn is_calendar_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = &value[range];
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10)) else {
        return false;
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year >= 1 && (1..=days).contains(&day)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use dla_domain::{
        installation::{
            ContentItem, ContentItemOverride, InferenceConfidence, Installation,
            InstallationDetection, InstallationOverrides, InstallationPlatform, InstallationStatus,
        },
        own_work::OwnWorkKind,
        package::{
            ArchiveRetentionPolicy, PackageSourceSet, PackageSourceSetKind,
            PreparedPackageInstallation,
        },
    };

    use super::*;
    use crate::media::MediaInventoryItem;

    const ARTWORK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ARTWORK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct Installations(Vec<Installation>);

    impl InstallationStore for Installations {
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
                .0
                .iter()
                .find(|installation| installation.id == *installation_id)
                .cloned())
        }
        fn list(&self) -> Result<Vec<Installation>, InstallationLibraryError> {
            Ok(self.0.clone())
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

    struct Preparations(Option<PreparedPackageInstallation>);

    impl PackagePreparationStore for Preparations {
        fn read_prepared_package(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<PreparedPackageInstallation>, PackagePreparationError> {
            Ok(self
                .0
                .clone()
                .filter(|prepared| prepared.installation_id == *installation_id))
        }
        fn save_prepared_package(
            &self,
            _prepared: &PreparedPackageInstallation,
        ) -> Result<(), PackagePreparationError> {
            unreachable!()
        }
    }

    struct Inventory {
        items: Vec<MediaInventoryItem>,
        reads: AtomicUsize,
    }

    impl MediaInventoryReader for Inventory {
        fn read_inventory(&self, _root_path: &str) -> Result<Vec<MediaInventoryItem>, MediaError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.items.clone())
        }
    }

    #[derive(Default)]
    struct Details(Mutex<Vec<OwnWorkDetails>>);

    impl OwnWorkDetailsStore for Details {
        fn read_own_work_details(
            &self,
            installation_id: &InstallationId,
        ) -> Result<Option<OwnWorkDetails>, OwnWorkError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|details| details.installation_id == *installation_id)
                .cloned())
        }
        fn list_own_work_details(&self) -> Result<Vec<OwnWorkDetails>, OwnWorkError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn save_own_work_details(&self, details: &OwnWorkDetails) -> Result<(), OwnWorkError> {
            let mut saved = self.0.lock().unwrap();
            saved.retain(|existing| existing.installation_id != details.installation_id);
            saved.push(details.clone());
            Ok(())
        }
    }

    #[derive(Default)]
    struct Artwork {
        stored: Mutex<Vec<String>>,
        imported: Mutex<Vec<PathBuf>>,
    }

    impl OwnWorkArtworkStore for Artwork {
        fn import(
            &self,
            _installation_id: &InstallationId,
            source: &Path,
        ) -> Result<String, OwnWorkError> {
            self.imported.lock().unwrap().push(source.to_path_buf());
            self.stored.lock().unwrap().push(ARTWORK_B.to_owned());
            Ok(ARTWORK_B.to_owned())
        }
        fn contains(
            &self,
            _installation_id: &InstallationId,
            artwork_id: &str,
        ) -> Result<bool, OwnWorkError> {
            Ok(self
                .stored
                .lock()
                .unwrap()
                .iter()
                .any(|stored| stored == artwork_id))
        }
        fn read(
            &self,
            _installation_id: &InstallationId,
            _artwork_id: &str,
        ) -> Result<Vec<u8>, OwnWorkError> {
            Ok(Vec::new())
        }
        fn retain(
            &self,
            _installation_id: &InstallationId,
            keep: &[String],
        ) -> Result<(), OwnWorkError> {
            self.stored
                .lock()
                .unwrap()
                .retain(|stored| keep.contains(stored));
            Ok(())
        }
        fn remove_all_except(
            &self,
            _installations: &[InstallationId],
        ) -> Result<usize, OwnWorkError> {
            Ok(0)
        }
    }

    fn path(value: &str) -> RelativePath {
        RelativePath::parse(value).expect("relative path")
    }

    fn installation(items: &[(&str, MediaType)], ignored: &[&str]) -> Installation {
        Installation {
            id: InstallationId("installation-1".to_owned()),
            scan_root_id: None,
            root_path: "/synthetic/library/own".to_owned(),
            platform: InstallationPlatform::Unknown,
            status: InstallationStatus::Ready,
            detection: InstallationDetection {
                source_scan_session_id: None,
                catalog_identity: None,
                suggested_status: InstallationStatus::Ready,
                content_items: items
                    .iter()
                    .map(|(relative, media_type)| ContentItem {
                        relative_path: path(relative),
                        path_key: relative.to_lowercase(),
                        media_type: *media_type,
                        size_bytes: Some(10),
                        modified_at: None,
                        confidence: InferenceConfidence::High,
                        reason_codes: Vec::new(),
                    })
                    .collect(),
                launch_candidates: Vec::new(),
                package_inspection: None,
            },
            overrides: InstallationOverrides {
                content_items: ignored
                    .iter()
                    .map(|relative| ContentItemOverride {
                        relative_path: path(relative),
                        media_type: None,
                        ignored: true,
                        order: None,
                    })
                    .collect(),
                ..InstallationOverrides::default()
            },
            discovered_at: "2026-10-28T00:00:00Z".to_owned(),
            updated_at: "2026-10-28T00:00:00Z".to_owned(),
        }
    }

    fn service(
        installation: Installation,
        prepared: Option<PreparedPackageInstallation>,
        inventory: Vec<MediaInventoryItem>,
    ) -> (OwnWorkService, Arc<Artwork>, Arc<Inventory>) {
        let artwork = Arc::new(Artwork::default());
        let inventory = Arc::new(Inventory {
            items: inventory,
            reads: AtomicUsize::new(0),
        });
        let service = OwnWorkService::new(
            Arc::new(Installations(vec![installation])),
            Arc::new(Preparations(prepared)),
            inventory.clone(),
            Arc::new(Details::default()),
            artwork.clone(),
        );
        (service, artwork, inventory)
    }

    fn request() -> SaveOwnWorkDetailsRequest {
        SaveOwnWorkDetailsRequest {
            installation_id: InstallationId("installation-1".to_owned()),
            circle: Some("  A circle ".to_owned()),
            code: Some(String::new()),
            release_date: Some("2024-02-29".to_owned()),
            kind: Some(OwnWorkKind::Images),
            tags: vec![
                " CG ".to_owned(),
                "cg".to_owned(),
                String::new(),
                "Bonus".to_owned(),
            ],
            description: Some("   ".to_owned()),
            cover: None,
            samples: Vec::new(),
        }
    }

    #[test]
    fn saves_trimmed_details_and_drops_empty_fields_and_repeated_tags() {
        let (service, ..) = service(installation(&[], &[]), None, Vec::new());

        let saved = service
            .save(request(), "2026-10-28T01:00:00Z")
            .expect("saved");

        assert_eq!(saved.circle.as_deref(), Some("A circle"));
        assert_eq!(saved.code, None);
        assert_eq!(saved.description, None);
        assert_eq!(saved.tags, vec!["CG".to_owned(), "Bonus".to_owned()]);
        assert_eq!(service.list().expect("listed"), vec![saved.clone()]);
        assert_eq!(
            service.read(&saved.installation_id).expect("read"),
            Some(saved)
        );
    }

    #[test]
    fn refuses_details_it_cannot_keep() {
        let (service, ..) = service(installation(&[], &[]), None, Vec::new());
        let rejected = |change: fn(&mut SaveOwnWorkDetailsRequest)| {
            let mut invalid = request();
            change(&mut invalid);
            service
                .save(invalid, "2026-10-28T01:00:00Z")
                .expect_err("refused")
        };

        assert!(matches!(
            rejected(|r| r.release_date = Some("2023-02-29".to_owned())),
            OwnWorkError::InvalidRequest(_)
        ));
        assert!(matches!(
            rejected(|r| r.release_date = Some("28/10/2026".to_owned())),
            OwnWorkError::InvalidRequest(_)
        ));
        assert!(matches!(
            rejected(|r| r.tags = vec!["x".repeat(41)]),
            OwnWorkError::InvalidRequest(_)
        ));
        assert!(matches!(
            rejected(|r| r.tags = (0..31).map(|n| format!("tag {n}")).collect()),
            OwnWorkError::InvalidRequest(_)
        ));
        assert!(matches!(
            rejected(|r| r.description = Some("x".repeat(4001))),
            OwnWorkError::InvalidRequest(_)
        ));
        assert!(matches!(
            rejected(|r| r.samples = (0..13).map(|n| format!("{n:064x}")).collect()),
            OwnWorkError::InvalidRequest(_)
        ));
        assert!(matches!(
            rejected(|r| r.cover = Some("../../etc/passwd".to_owned())),
            OwnWorkError::UnknownArtwork(_)
        ));
        assert!(matches!(
            rejected(|r| r.cover = Some(ARTWORK_A.to_owned())),
            OwnWorkError::UnknownArtwork(_)
        ));
        assert!(matches!(
            rejected(|r| r.installation_id = InstallationId("missing".to_owned())),
            OwnWorkError::InstallationNotFound(_)
        ));
    }

    #[test]
    fn keeps_only_the_artwork_the_saved_details_use() {
        let (service, artwork, _) = service(installation(&[], &[]), None, Vec::new());
        artwork
            .stored
            .lock()
            .unwrap()
            .extend([ARTWORK_A.to_owned(), ARTWORK_B.to_owned()]);

        let mut with_cover = request();
        with_cover.cover = Some(ARTWORK_B.to_owned());
        service
            .save(with_cover, "2026-10-28T01:00:00Z")
            .expect("saved");

        assert_eq!(*artwork.stored.lock().unwrap(), vec![ARTWORK_B.to_owned()]);
    }

    #[test]
    fn lists_a_folder_installs_images_in_natural_order_without_ignored_files() {
        let (service, ..) = service(
            installation(
                &[
                    ("main/10.png", MediaType::Image),
                    ("main/2.png", MediaType::Image),
                    ("main/skipped.png", MediaType::Image),
                    ("manual.pdf", MediaType::Pdf),
                ],
                &["main/skipped.png"],
            ),
            None,
            Vec::new(),
        );

        let images = service
            .list_images(&InstallationId("installation-1".to_owned()))
            .expect("images");

        assert_eq!(
            images
                .iter()
                .map(|image| image.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["main/2.png", "main/10.png"]
        );
    }

    #[test]
    fn reads_an_archive_installs_images_from_its_folder_and_refuses_other_files() {
        let prepared = PreparedPackageInstallation {
            installation_id: InstallationId("installation-1".to_owned()),
            destination_root: "/synthetic/library/archive".to_owned(),
            content_root: None,
            preferred_action: None,
            source_set: PackageSourceSet {
                kind: PackageSourceSetKind::SingleArchive,
                volumes: Vec::new(),
            },
            archive_retention: ArchiveRetentionPolicy::Keep,
            sources_deleted: false,
            source_cleanup_error: None,
            installed_file_count: 2,
            installed_bytes: 10,
            prepared_at: "2026-10-28T00:00:00Z".to_owned(),
        };
        let (service, artwork, inventory) = service(
            installation(&[], &[]),
            Some(prepared),
            vec![
                MediaInventoryItem {
                    relative_path: path("Work/cg_01.png"),
                    media_type: MediaType::Image,
                    size_bytes: Some(5),
                },
                MediaInventoryItem {
                    relative_path: path("Work/manual.pdf"),
                    media_type: MediaType::Pdf,
                    size_bytes: Some(5),
                },
            ],
        );
        let id = InstallationId("installation-1".to_owned());

        assert_eq!(
            service
                .image_path(&id, &path("Work/cg_01.png"))
                .expect("image"),
            PathBuf::from("/synthetic/library/archive/Work/cg_01.png")
        );
        assert!(matches!(
            service.image_path(&id, &path("Work/manual.pdf")),
            Err(OwnWorkError::UnknownImage)
        ));
        assert!(matches!(
            service.image_path(&id, &path("Work/elsewhere.png")),
            Err(OwnWorkError::UnknownImage)
        ));
        assert_eq!(inventory.reads.load(Ordering::SeqCst), 1);

        let imported = service
            .import_artwork(ImportOwnWorkArtworkRequest {
                installation_id: id,
                relative_path: path("Work/cg_01.png"),
            })
            .expect("imported");
        assert_eq!(imported, ARTWORK_B);
        assert_eq!(
            *artwork.imported.lock().unwrap(),
            vec![PathBuf::from("/synthetic/library/archive/Work/cg_01.png")]
        );
    }

    #[test]
    fn artwork_ids_are_plain_hex_digests() {
        assert!(is_artwork_id(ARTWORK_A));
        assert!(!is_artwork_id("../aaaa"));
        assert!(!is_artwork_id(&ARTWORK_A.to_uppercase()));
        assert!(!is_artwork_id(&ARTWORK_A[1..]));
    }
}
