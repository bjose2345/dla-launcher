use std::{collections::BTreeMap, path::Path};

use dla_domain::{
    installation::{
        CatalogIdentity, ContentItem, InferenceConfidence, InstallationDetection,
        InstallationError, InstallationStatus, LaunchActionKind, LaunchCandidate,
        LaunchCandidateId, LaunchTarget, MediaType, RelativePath, RelativePathError,
    },
    scanner::{ScanEntry, ScanEntryKind, ScanEntryPresence, ScanSessionId},
};
use thiserror::Error;

const PORTABLE_PLATFORMS: [dla_domain::installation::InstallationPlatform; 5] = [
    dla_domain::installation::InstallationPlatform::Windows,
    dla_domain::installation::InstallationPlatform::Linux,
    dla_domain::installation::InstallationPlatform::Macos,
    dla_domain::installation::InstallationPlatform::Android,
    dla_domain::installation::InstallationPlatform::Ios,
];

#[derive(Clone, Debug)]
pub struct MediaClassificationRequest<'a> {
    pub source_scan_session_id: Option<ScanSessionId>,
    pub catalog_identity: Option<CatalogIdentity>,
    pub entries: &'a [ScanEntry],
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum MediaClassificationError {
    #[error("invalid scanner relative path {path}: {source}")]
    InvalidRelativePath {
        path: String,
        source: RelativePathError,
    },
    #[error("invalid scanner size for {path}: {value}")]
    InvalidSize { path: String, value: String },
    #[error("invalid media classification: {0}")]
    InvalidDetection(#[from] InstallationError),
}

pub fn classify_media(
    request: MediaClassificationRequest<'_>,
) -> Result<InstallationDetection, MediaClassificationError> {
    let mut classified = request
        .entries
        .iter()
        .filter(|entry| entry.presence == ScanEntryPresence::Present)
        .filter(|entry| entry.kind == ScanEntryKind::File)
        .map(classify_entry)
        .collect::<Result<Vec<_>, _>>()?;
    classified.sort_by(|left, right| left.item.relative_path.cmp(&right.item.relative_path));

    let content_items = classified
        .iter()
        .map(|item| item.item.clone())
        .collect::<Vec<_>>();
    let launch_candidates = generate_candidates(&classified);
    let suggested_status = if launch_candidates.len() == 1 {
        InstallationStatus::Ready
    } else {
        InstallationStatus::NeedsReview
    };
    let detection = InstallationDetection {
        source_scan_session_id: request.source_scan_session_id,
        catalog_identity: request.catalog_identity,
        suggested_status,
        content_items,
        launch_candidates,
        package_inspection: None,
    };
    detection.validate()?;
    Ok(detection)
}

#[derive(Clone, Debug)]
struct ClassifiedItem {
    item: ContentItem,
    filename_stem: String,
    numbered_filename: bool,
    preferred_executable: bool,
    installer: bool,
}

fn classify_entry(entry: &ScanEntry) -> Result<ClassifiedItem, MediaClassificationError> {
    let relative_path = RelativePath::parse(entry.relative_path.clone()).map_err(|source| {
        MediaClassificationError::InvalidRelativePath {
            path: entry.relative_path.clone(),
            source,
        }
    })?;
    let size_bytes = entry
        .size
        .as_deref()
        .map(str::parse)
        .transpose()
        .map_err(|_| MediaClassificationError::InvalidSize {
            path: entry.relative_path.clone(),
            value: entry.size.clone().unwrap_or_default(),
        })?;
    let path = Path::new(relative_path.as_str());
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let filename_lower = filename.to_lowercase();
    let filename_stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    let stem_lower = filename_stem.to_lowercase();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let numbered_filename = filename_stem
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_digit);
    let preferred_executable = is_preferred_executable(&stem_lower);
    let installer = is_installer(&stem_lower);
    let (media_type, confidence, reason_codes) = infer_media_type(
        path,
        &filename_lower,
        &stem_lower,
        &extension,
        numbered_filename,
        preferred_executable,
        installer,
    );

    Ok(ClassifiedItem {
        item: ContentItem {
            relative_path,
            path_key: entry.path_key.clone(),
            media_type,
            size_bytes,
            modified_at: entry.modified_at.clone(),
            confidence,
            reason_codes,
        },
        filename_stem,
        numbered_filename,
        preferred_executable,
        installer,
    })
}

pub fn classify_media_type(relative_path: &RelativePath) -> MediaType {
    let path = Path::new(relative_path.as_str());
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let filename_lower = filename.to_lowercase();
    let filename_stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let stem_lower = filename_stem.to_lowercase();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let numbered_filename = filename_stem
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_digit);
    infer_media_type(
        path,
        &filename_lower,
        &stem_lower,
        &extension,
        numbered_filename,
        is_preferred_executable(&stem_lower),
        is_installer(&stem_lower),
    )
    .0
}

fn infer_media_type(
    path: &Path,
    filename_lower: &str,
    stem_lower: &str,
    extension: &str,
    numbered_filename: bool,
    preferred_executable: bool,
    installer: bool,
) -> (MediaType, InferenceConfidence, Vec<String>) {
    if is_known_ignored_filename(filename_lower) {
        (
            MediaType::Unknown,
            InferenceConfidence::High,
            reasons(&["known_ignored_filename"]),
        )
    } else if is_deceptive_executable(path, extension) {
        (
            MediaType::Unknown,
            InferenceConfidence::Low,
            reasons(&["deceptive_double_extension", "signature_required"]),
        )
    } else if extension.is_empty() {
        (
            MediaType::Unknown,
            InferenceConfidence::Low,
            reasons(&["missing_extension", "signature_required"]),
        )
    } else if extension == "exe" {
        let mut reason_codes = reasons(&["executable_extension"]);
        let confidence = if preferred_executable {
            reason_codes.push("preferred_executable_name".to_owned());
            InferenceConfidence::High
        } else if installer {
            reason_codes.push("installer_name".to_owned());
            InferenceConfidence::Medium
        } else {
            InferenceConfidence::Medium
        };
        (MediaType::Executable, confidence, reason_codes)
    } else if is_audio_extension(extension) {
        let mut reason_codes = reasons(&["audio_extension"]);
        if numbered_filename {
            reason_codes.push("numbered_filename".to_owned());
        }
        (MediaType::Audio, InferenceConfidence::High, reason_codes)
    } else if is_image_extension(extension) {
        let mut reason_codes = reasons(&["image_extension"]);
        if numbered_filename {
            reason_codes.push("numbered_filename".to_owned());
        } else if is_cover_filename(stem_lower) {
            reason_codes.push("cover_filename".to_owned());
        }
        (MediaType::Image, InferenceConfidence::High, reason_codes)
    } else if extension == "pdf" {
        (
            MediaType::Pdf,
            InferenceConfidence::High,
            reasons(&["pdf_extension"]),
        )
    } else if is_video_extension(extension) {
        (
            MediaType::Video,
            InferenceConfidence::High,
            reasons(&["video_extension"]),
        )
    } else if extension == "apk" {
        (
            MediaType::AndroidPackage,
            InferenceConfidence::High,
            reasons(&["android_package_extension"]),
        )
    } else if is_archive_extension(extension) {
        (
            MediaType::Archive,
            InferenceConfidence::High,
            reasons(&["archive_extension"]),
        )
    } else if is_unsupported_document_extension(extension) {
        (
            MediaType::Unknown,
            InferenceConfidence::Low,
            reasons(&["unsupported_document_extension"]),
        )
    } else {
        (
            MediaType::Unknown,
            InferenceConfidence::Low,
            reasons(&["unrecognized_extension"]),
        )
    }
}

fn generate_candidates(items: &[ClassifiedItem]) -> Vec<LaunchCandidate> {
    let mut candidates = Vec::new();
    let mut ids = CandidateIds::default();
    add_executable_candidates(items, &mut candidates, &mut ids);
    add_audio_candidates(items, &mut candidates, &mut ids);
    add_pdf_candidates(items, &mut candidates, &mut ids);
    add_image_candidates(items, &mut candidates, &mut ids);
    add_video_candidates(items, &mut candidates, &mut ids);
    add_android_candidates(items, &mut candidates, &mut ids);
    if candidates.is_empty() {
        add_archive_candidate(items, &mut candidates, &mut ids);
    }
    candidates
}

fn add_executable_candidates(
    items: &[ClassifiedItem],
    candidates: &mut Vec<LaunchCandidate>,
    ids: &mut CandidateIds,
) {
    let executables = items
        .iter()
        .filter(|item| item.item.media_type == MediaType::Executable && !item.installer)
        .collect::<Vec<_>>();
    let installer_count = items
        .iter()
        .filter(|item| item.item.media_type == MediaType::Executable && item.installer)
        .count();
    for executable in &executables {
        let (confidence, mut reason_codes) = if executable.preferred_executable {
            (
                InferenceConfidence::High,
                reasons(&["preferred_executable_name"]),
            )
        } else if executables.len() == 1 {
            (InferenceConfidence::Medium, reasons(&["single_executable"]))
        } else {
            (
                InferenceConfidence::Medium,
                reasons(&["executable_candidate"]),
            )
        };
        if executable.preferred_executable && installer_count > 0 {
            reason_codes.push("installer_deprioritized".to_owned());
        }
        candidates.push(candidate(
            ids,
            &format!("launch-{}", slug(&executable.filename_stem)),
            LaunchActionKind::LaunchExecutable,
            LaunchTarget::RelativePath(executable.item.relative_path.clone()),
            vec![dla_domain::installation::InstallationPlatform::Windows],
            confidence,
            reason_codes,
        ));
    }
}

fn add_audio_candidates(
    items: &[ClassifiedItem],
    candidates: &mut Vec<LaunchCandidate>,
    ids: &mut CandidateIds,
) {
    let audio = typed_items(items, MediaType::Audio);
    match audio.as_slice() {
        [] => {}
        [single] => candidates.push(candidate(
            ids,
            &format!("play-{}", slug(&single.filename_stem)),
            LaunchActionKind::PlayAudio,
            LaunchTarget::RelativePath(single.item.relative_path.clone()),
            PORTABLE_PLATFORMS.to_vec(),
            InferenceConfidence::High,
            reasons(&["single_audio"]),
        )),
        many => {
            let mut reason_codes = reasons(&["dominant_audio_set"]);
            if many.iter().all(|item| item.numbered_filename) {
                reason_codes.push("numbered_track_sequence".to_owned());
            }
            candidates.push(candidate(
                ids,
                "play-album",
                LaunchActionKind::PlayAudio,
                LaunchTarget::InstallationRoot,
                PORTABLE_PLATFORMS.to_vec(),
                InferenceConfidence::High,
                reason_codes,
            ));
        }
    }
}

fn add_pdf_candidates(
    items: &[ClassifiedItem],
    candidates: &mut Vec<LaunchCandidate>,
    ids: &mut CandidateIds,
) {
    let pdfs = typed_items(items, MediaType::Pdf);
    for pdf in &pdfs {
        let single = pdfs.len() == 1;
        let base_id = if single {
            "open-pdf".to_owned()
        } else {
            format!("open-{}", slug(&pdf.filename_stem))
        };
        candidates.push(candidate(
            ids,
            &base_id,
            LaunchActionKind::OpenDocument,
            LaunchTarget::RelativePath(pdf.item.relative_path.clone()),
            PORTABLE_PLATFORMS.to_vec(),
            if single {
                InferenceConfidence::High
            } else {
                InferenceConfidence::Medium
            },
            if single {
                reasons(&["single_pdf"])
            } else {
                reasons(&["pdf_document"])
            },
        ));
    }
}

fn add_image_candidates(
    items: &[ClassifiedItem],
    candidates: &mut Vec<LaunchCandidate>,
    ids: &mut CandidateIds,
) {
    let images = typed_items(items, MediaType::Image);
    if images.len() < 2 {
        return;
    }
    let numbered = images.iter().filter(|item| item.numbered_filename).count();
    let (confidence, reason_codes) = if numbered == images.len() {
        (
            InferenceConfidence::High,
            reasons(&["numbered_image_sequence"]),
        )
    } else {
        (
            InferenceConfidence::Medium,
            reasons(&["dominant_image_set"]),
        )
    };
    candidates.push(candidate(
        ids,
        "read-images",
        LaunchActionKind::ReadImages,
        LaunchTarget::InstallationRoot,
        PORTABLE_PLATFORMS.to_vec(),
        confidence,
        reason_codes,
    ));
}

fn add_video_candidates(
    items: &[ClassifiedItem],
    candidates: &mut Vec<LaunchCandidate>,
    ids: &mut CandidateIds,
) {
    let videos = typed_items(items, MediaType::Video);
    for video in &videos {
        let single = videos.len() == 1;
        candidates.push(candidate(
            ids,
            &format!("play-{}", slug(&video.filename_stem)),
            LaunchActionKind::PlayVideo,
            LaunchTarget::RelativePath(video.item.relative_path.clone()),
            PORTABLE_PLATFORMS.to_vec(),
            if single {
                InferenceConfidence::High
            } else {
                InferenceConfidence::Medium
            },
            if single {
                reasons(&["single_video"])
            } else {
                reasons(&["video_file"])
            },
        ));
    }
}

fn add_android_candidates(
    items: &[ClassifiedItem],
    candidates: &mut Vec<LaunchCandidate>,
    ids: &mut CandidateIds,
) {
    let packages = typed_items(items, MediaType::AndroidPackage);
    for package in &packages {
        let single = packages.len() == 1;
        let base_id = if single {
            "open-android-package".to_owned()
        } else {
            format!("open-{}", slug(&package.filename_stem))
        };
        candidates.push(candidate(
            ids,
            &base_id,
            LaunchActionKind::OpenAndroidPackage,
            LaunchTarget::RelativePath(package.item.relative_path.clone()),
            vec![dla_domain::installation::InstallationPlatform::Android],
            if single {
                InferenceConfidence::High
            } else {
                InferenceConfidence::Medium
            },
            if single {
                reasons(&["single_android_package"])
            } else {
                reasons(&["android_package_candidate"])
            },
        ));
    }
}

fn add_archive_candidate(
    items: &[ClassifiedItem],
    candidates: &mut Vec<LaunchCandidate>,
    ids: &mut CandidateIds,
) {
    let archives = typed_items(items, MediaType::Archive);
    if let [archive] = archives.as_slice() {
        candidates.push(candidate(
            ids,
            &format!("open-{}", slug(&archive.filename_stem)),
            LaunchActionKind::OpenArchive,
            LaunchTarget::RelativePath(archive.item.relative_path.clone()),
            PORTABLE_PLATFORMS.to_vec(),
            InferenceConfidence::Medium,
            reasons(&["single_archive"]),
        ));
    }
}

fn typed_items(items: &[ClassifiedItem], media_type: MediaType) -> Vec<&ClassifiedItem> {
    items
        .iter()
        .filter(|item| item.item.media_type == media_type)
        .collect()
}

fn candidate(
    ids: &mut CandidateIds,
    base_id: &str,
    action: LaunchActionKind,
    target: LaunchTarget,
    supported_platforms: Vec<dla_domain::installation::InstallationPlatform>,
    confidence: InferenceConfidence,
    reason_codes: Vec<String>,
) -> LaunchCandidate {
    LaunchCandidate {
        id: LaunchCandidateId(ids.next(base_id)),
        action,
        target,
        supported_platforms,
        confidence,
        reason_codes,
    }
}

#[derive(Default)]
struct CandidateIds {
    counts: BTreeMap<String, usize>,
}

impl CandidateIds {
    fn next(&mut self, base: &str) -> String {
        let count = self.counts.entry(base.to_owned()).or_default();
        *count += 1;
        if *count == 1 {
            base.to_owned()
        } else {
            format!("{base}-{count}")
        }
    }
}

fn reasons(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn slug(value: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            result.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    if result.is_empty() {
        "content".to_owned()
    } else {
        result
    }
}

fn is_known_ignored_filename(filename: &str) -> bool {
    matches!(filename, ".ds_store" | "thumbs.db" | "desktop.ini")
}

fn is_deceptive_executable(path: &Path, extension: &str) -> bool {
    if extension != "exe" {
        return false;
    }
    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    let prior_extension = Path::new(stem)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    is_audio_extension(&prior_extension)
        || is_image_extension(&prior_extension)
        || is_video_extension(&prior_extension)
        || is_archive_extension(&prior_extension)
        || prior_extension == "pdf"
        || prior_extension == "apk"
}

fn is_preferred_executable(stem: &str) -> bool {
    matches!(stem, "game" | "launcher" | "start" | "startgame")
}

fn is_installer(stem: &str) -> bool {
    stem == "setup"
        || stem == "install"
        || stem == "installer"
        || stem == "uninstall"
        || stem == "uninstaller"
        || stem.starts_with("setup_")
        || stem.starts_with("setup-")
}

fn is_cover_filename(stem: &str) -> bool {
    matches!(stem, "cover" | "folder" | "front")
}

fn is_audio_extension(extension: &str) -> bool {
    matches!(
        extension,
        "aac" | "flac" | "m4a" | "mp3" | "ogg" | "opus" | "wav" | "wma"
    )
}

fn is_image_extension(extension: &str) -> bool {
    matches!(
        extension,
        "avif" | "bmp" | "gif" | "jpeg" | "jpg" | "png" | "webp"
    )
}

fn is_video_extension(extension: &str) -> bool {
    matches!(
        extension,
        "avi" | "m4v" | "mkv" | "mov" | "mp4" | "webm" | "wmv"
    )
}

fn is_archive_extension(extension: &str) -> bool {
    matches!(
        extension,
        "7z" | "bz2" | "gz" | "rar" | "tar" | "xz" | "zip" | "zst"
    )
}

fn is_unsupported_document_extension(extension: &str) -> bool {
    matches!(extension, "doc" | "docx" | "epub" | "md" | "rtf" | "txt")
}
